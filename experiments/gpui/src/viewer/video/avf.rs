//! macOS video through AVFoundation: an `AVPlayer` decodes (in hardware for
//! H.264 and HEVC), plays the sound, keeps picture and sound in step, seeks
//! and changes speed with the pitch kept; an `AVPlayerItemVideoOutput`
//! vends each picture as a CoreVideo pixel buffer, which GPUI's `surface`
//! element draws without copying.
//!
//! GPUI's surface shader takes only bi-planar full-range 4:2:0 buffers and
//! converts them with the BT.601 matrix. Most video is BT.709, so the item
//! gets a video composition whose compositor writes BT.601; the composition
//! also applies a track's rotation, which the output alone would not.
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

/// A composition handed over from the queue AVFoundation completes on.
struct Composition(Retained<AVMutableVideoComposition>);
// SAFETY: the composition is configured before it is handed over and only
// read afterwards; AVFoundation's video compositions are safe to pass
// between threads.
unsafe impl Send for Composition {}

pub struct AvPlayer {
    player: Retained<NativePlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    composition: Arc<Mutex<Option<Composition>>>,
    composed: bool,
    playing: bool,
    rate: f32,
    /// Display times of recent frames, to measure the frame rate as
    /// `useFrameTime` does.
    frame_times: Vec<f64>,
    fps: Option<f64>,
}

fn seconds(time: CMTime) -> Option<f64> {
    if !time.flags.contains(CMTimeFlags::Valid)
        || time.flags.intersects(CMTimeFlags::ImpliedValueFlagsMask)
    {
        return None;
    }
    let seconds = unsafe { time.seconds() };
    seconds.is_finite().then_some(seconds)
}

/// A CoreFoundation string constant as the `NSString` it is bridged to.
fn ns(string: &CFString) -> &NSString {
    // SAFETY: CFString and NSString are toll-free bridged.
    unsafe { &*(string as *const CFString as *const NSString) }
}

impl AvPlayer {
    pub fn open(path: &Path) -> Result<Self, String> {
        // The player lives on the main thread, as AVPlayer expects.
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
            // The asset's own composition (its tracks, sizes and rotation),
            // told to write BT.601, arrives once AVFoundation has read the
            // tracks; it is set on the item at the next frame.
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
                playing: false,
                rate: 1.,
                frame_times: Vec::new(),
                fps: None,
            })
        }
    }

    /// Sets the composition once it has arrived.
    fn compose(&mut self) {
        if self.composed {
            return;
        }
        if let Some(Composition(composition)) = self.composition.lock().unwrap().take() {
            unsafe { self.item.setVideoComposition(Some(&composition)) };
            self.composed = true;
        }
    }

    /// Measures the frame rate from the display times of consecutive
    /// frames, snapped to a common rate.
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
            // The item pauses itself at its end.
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
            // GPUI fails the whole frame on any other format, so a buffer the
            // output did not convert is never handed over.
            if CVPixelBufferGetPixelFormatType(&buffer)
                != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            {
                return None;
            }
            let raw = NonNull::new(Retained::into_raw(buffer))?;
            // SAFETY: the copy gave us one reference, which moves over.
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
