//! The video backend for macOS, which plays a file with AVFoundation. An
//! `AVPlayer` decodes the file (in hardware for H.264 and HEVC), plays its
//! sound, keeps the picture and the sound in step, seeks, and changes the
//! playback rate without changing the pitch. An `AVPlayerItemVideoOutput`
//! delivers each frame as a CoreVideo pixel buffer, which GPUI's `surface`
//! element draws without copying it.
//!
//! GPUI's shader for a surface accepts only buffers in the bi-planar
//! full-range 4:2:0 format, and it converts them to RGB with the BT.601
//! matrix. Most video is encoded with the BT.709 matrix. So the player item
//! gets a video composition that makes AVFoundation write its frames with
//! the BT.601 matrix. The composition also applies the rotation of a video
//! track, which the video output does not do without one.
use super::backend::{Backend, Frame, Status};
use super::model::snap_fps;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_av_foundation::{
    AVMutableVideoComposition, AVPlayer as NativePlayer, AVPlayerActionAtItemEnd, AVPlayerItem,
    AVPlayerItemOutput, AVPlayerItemStatus, AVPlayerItemVideoOutput, AVURLAsset,
    AVVideoColorPrimaries_ITU_R_709_2, AVVideoTransferFunction_ITU_R_709_2,
    AVVideoYCbCrMatrix_ITU_R_601_4, NSValueAVFoundationExtensions,
};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::{CMTime, CMTimeFlags, kCMTimeZero};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetPixelFormatType, kCVPixelBufferIOSurfacePropertiesKey,
    kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL, NSValue};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

/// A video composition that is passed from the queue on which AVFoundation
/// calls the completion handler to the main thread.
struct Composition(Retained<AVMutableVideoComposition>);
// SAFETY: the composition is configured before it is passed on and is only
// read afterwards, and AVFoundation's video compositions may be passed
// between threads.
unsafe impl Send for Composition {}

/// The backend that plays a video file with AVFoundation.
pub struct AvPlayer {
    /// The AVFoundation player.
    player: Retained<NativePlayer>,
    /// The player item, which stands for the file in the player.
    item: Retained<AVPlayerItem>,
    /// The video output, from which the frames are taken.
    output: Retained<AVPlayerItemVideoOutput>,
    /// The video composition, from the moment AVFoundation has made it
    /// until `compose` sets it on the item.
    composition: Arc<Mutex<Option<Composition>>>,
    /// True once the video composition has been set on the item.
    composed: bool,
    /// True once the item has become ready to play. At that moment, if
    /// playback was paused, the player sought to the start so that
    /// AVFoundation delivers the first frame.
    primed: bool,
    /// True after `play`, until `pause` or the end of the media.
    playing: bool,
    /// The playback rate that was set last. `play` starts playback at this
    /// rate.
    rate: f32,
    /// The display times, in seconds, of the frames that were taken last,
    /// at most 11 of them. The frame rate is measured from them.
    frame_times: Vec<f64>,
    /// The measured frame rate, or `None` until it has been measured.
    fps: Option<f64>,
}

/// Returns `time` in seconds. Returns `None` when the time is not valid,
/// is indefinite or infinite, or does not give a finite number.
fn seconds(time: CMTime) -> Option<f64> {
    if !time.flags.contains(CMTimeFlags::Valid)
        || time.flags.intersects(CMTimeFlags::ImpliedValueFlagsMask)
    {
        return None;
    }
    let seconds = unsafe { time.seconds() };
    seconds.is_finite().then_some(seconds)
}

/// Returns a CoreFoundation string constant as an `NSString`.
fn ns(string: &CFString) -> &NSString {
    // SAFETY: `CFString` and `NSString` are toll-free bridged, so a pointer
    // to one is a valid pointer to the other.
    unsafe { &*(string as *const CFString as *const NSString) }
}

impl AvPlayer {
    /// Opens the file `path`: creates the player, its item and its video
    /// output, and asks AVFoundation for the video composition. Returns an
    /// error when it is not called on the main thread, or when the path is
    /// not valid UTF-8.
    pub fn open(path: &Path) -> Result<Self, String> {
        // An `AVPlayer` must be created and used on the main thread.
        let mtm = MainThreadMarker::new().ok_or("AVFoundation playback needs the main thread")?;
        let path = path.to_str().ok_or("The file's path is not valid UTF-8")?;
        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            let item = AVPlayerItem::playerItemWithAsset(&asset, mtm);
            let format = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange);
            let metal = NSNumber::new_bool(true);
            let surface = NSDictionary::<NSString, AnyObject>::new();
            let keys: [&NSString; 3] = [
                ns(kCVPixelBufferPixelFormatTypeKey),
                ns(kCVPixelBufferMetalCompatibilityKey),
                ns(kCVPixelBufferIOSurfacePropertiesKey),
            ];
            let values: [&AnyObject; 3] = [format.as_ref(), metal.as_ref(), surface.as_ref()];
            let attributes = NSDictionary::from_slices(&keys, &values);
            let output = AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&attributes),
            );
            item.addOutput(&output);
            let player = NativePlayer::playerWithPlayerItem(Some(&item), mtm);
            player.setActionAtItemEnd(AVPlayerActionAtItemEnd::Pause);
            // AVFoundation makes the video composition from the asset's
            // tracks, their sizes and their rotation, and calls the handler
            // with it once it has read the tracks. The handler sets the
            // composition to write its frames with the BT.601 matrix and
            // stores it. `compose` sets it on the item the next time the
            // player asks for the status.
            let composition = Arc::new(Mutex::new(None));
            let slot = composition.clone();
            let handler = RcBlock::new(
                move |composition: *mut AVMutableVideoComposition,
                      _: *mut objc2_foundation::NSError| {
                    let Some(composition) = Retained::retain(composition) else {
                        return;
                    };
                    composition.setColorPrimaries(AVVideoColorPrimaries_ITU_R_709_2);
                    composition.setColorTransferFunction(AVVideoTransferFunction_ITU_R_709_2);
                    composition.setColorYCbCrMatrix(AVVideoYCbCrMatrix_ITU_R_601_4);
                    *slot.lock().unwrap() = Some(Composition(composition));
                },
            );
            AVMutableVideoComposition::videoCompositionWithPropertiesOfAsset_completionHandler(
                &asset, &handler,
            );
            Ok(Self {
                player,
                item,
                output,
                composition,
                composed: false,
                primed: false,
                playing: false,
                rate: 1.,
                frame_times: Vec::new(),
                fps: None,
            })
        }
    }

    /// Sets the video composition on the item once AVFoundation has made
    /// it. It does nothing before that, and nothing after the composition
    /// has been set.
    fn compose(&mut self) {
        if self.composed {
            return;
        }
        if let Some(Composition(composition)) = self.composition.lock().unwrap().take() {
            unsafe { self.item.setVideoComposition(Some(&composition)) };
            self.composed = true;
        }
    }

    /// Records `time`, the display time in seconds of a frame that was
    /// taken, and measures the frame rate once the times of 11 consecutive
    /// frames are recorded. The frame rate is the median of the rates
    /// between each two consecutive times, changed to a common frame rate
    /// by `snap_fps`. A time that is not later than the one before, as
    /// after a seek back, discards the times recorded so far.
    fn measure(&mut self, time: f64) {
        if let Some(last) = self.frame_times.last()
            && time <= *last
        {
            self.frame_times.clear();
        }
        self.frame_times.push(time);
        if self.frame_times.len() > 11 {
            self.frame_times.remove(0);
        }
        if self.frame_times.len() == 11 {
            let mut rates: Vec<f64> = self
                .frame_times
                .windows(2)
                .map(|pair| 1. / (pair[1] - pair[0]))
                .filter(|rate| rate.is_finite())
                .collect();
            rates.sort_by(f64::total_cmp);
            if let Some(median) = rates.get(rates.len() / 2) {
                self.fps = Some(snap_fps(*median));
            }
        }
    }
}

impl Backend for AvPlayer {
    fn play(&mut self) {
        self.playing = true;
        unsafe { self.player.setRate(self.rate) };
    }

    fn pause(&mut self) {
        self.playing = false;
        unsafe { self.player.pause() };
    }

    fn seek(&mut self, seconds: f64) {
        self.frame_times.clear();
        unsafe {
            let time = CMTime::with_seconds(seconds.max(0.), 600);
            self.player
                .seekToTime_toleranceBefore_toleranceAfter(time, kCMTimeZero, kCMTimeZero);
        }
    }

    fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
        if self.playing {
            unsafe { self.player.setRate(rate) };
        }
    }

    fn set_volume(&mut self, volume: f32, muted: bool) {
        unsafe {
            self.player.setVolume(volume.clamp(0., 1.));
            self.player.setMuted(muted);
        }
    }

    fn status(&mut self) -> Status {
        self.compose();
        unsafe {
            // AVFoundation delivers no frame for a paused item until the
            // position changes. So when the item has become ready to play
            // and playback is paused, the player seeks to the start. The
            // first frame then arrives and is drawn.
            if !self.primed && self.item.status() == AVPlayerItemStatus::ReadyToPlay {
                self.primed = true;
                if !self.playing {
                    self.player.seekToTime_toleranceBefore_toleranceAfter(
                        kCMTimeZero,
                        kCMTimeZero,
                        kCMTimeZero,
                    );
                }
            }
            let error = (self.item.status() == AVPlayerItemStatus::Failed).then(|| {
                self.item
                    .error()
                    .map(|error| error.localizedDescription().to_string())
                    .unwrap_or_else(|| "The video cannot be played".into())
            });
            let duration = seconds(self.item.duration()).unwrap_or(0.);
            let position = seconds(self.player.currentTime()).unwrap_or(0.);
            let buffered = self
                .item
                .loadedTimeRanges()
                .iter()
                .filter_map(|range: Retained<NSValue>| {
                    let range = range.CMTimeRangeValue();
                    Some(seconds(range.start)? + seconds(range.duration)?)
                })
                .fold(0f64, f64::max);
            // The player was set to pause at the end of the item
            // (`AVPlayerActionAtItemEnd::Pause`). The media has ended when
            // the player's rate is 0 and the position is within 0.05 s of
            // the length.
            let ended = duration > 0. && self.player.rate() == 0. && position >= duration - 0.05;
            if ended {
                self.playing = false;
            }
            let size = self.item.presentationSize();
            Status {
                position,
                duration,
                buffered,
                playing: self.playing,
                ended,
                error,
                size: (size.width > 0. && size.height > 0.)
                    .then(|| (size.width.round() as u32, size.height.round() as u32)),
                fps: self.fps,
            }
        }
    }

    fn take_frame(&mut self) -> Option<Frame> {
        unsafe {
            let now = objc2_quartz_core::CACurrentMediaTime();
            let time = self.output.itemTimeForHostTime(now);
            if !self.output.hasNewPixelBufferForItemTime(time) {
                return None;
            }
            let mut shown = kCMTimeZero;
            let buffer = self
                .output
                .copyPixelBufferForItemTime_itemTimeForDisplay(time, &mut shown)?;
            if let Some(shown) = seconds(shown) {
                self.measure(shown);
            }
            // GPUI fails to draw the whole frame of the window when a
            // surface has any other pixel format. So a buffer that the
            // output delivered in another format is not returned.
            if CVPixelBufferGetPixelFormatType(&buffer)
                != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            {
                return None;
            }
            let raw = NonNull::new(Retained::into_raw(buffer))?;
            // SAFETY: `copyPixelBufferForItemTime` returned one owned
            // reference to the buffer, and `CFRetained` takes it over.
            let buffer: CFRetained<CVPixelBuffer> = CFRetained::from_raw(raw);
            Some(Frame::Surface(buffer))
        }
    }

    fn is_native(&self) -> bool {
        true
    }
}

impl Drop for AvPlayer {
    fn drop(&mut self) {
        unsafe {
            self.player.pause();
            let output: &AVPlayerItemOutput = &self.output;
            self.item.removeOutput(output);
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
    }
}
