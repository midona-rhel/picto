//! The interface between the media player and the code that decodes the
//! media. The player calls only the `Backend` trait, which has three
//! implementations:
//!
//! - On macOS, AVFoundation (`avf.rs`) plays QuickTime and MPEG-4 video.
//!   Its frames are CoreVideo pixel buffers, which GPUI draws without
//!   copying them.
//! - The `ffmpeg` program that Picto ships with for making thumbnails
//!   (`ffmpeg.rs`) plays every other video, such as WebM and Matroska, and
//!   a video that AVFoundation cannot play. Its frames are drawn as images,
//!   and `audio.rs` plays its sound.
//! - `audio.rs` decodes an audio file inside the app's process and plays it
//!   through CPAL, which the Flash player also uses for its sound. It also
//!   gives the audio visualizer the samples that are playing.
use super::audio::Analysis;
use gpui::RenderImage;
use std::path::Path;
use std::sync::Arc;

/// A decoded video frame that is ready to be drawn.
#[derive(Clone)]
pub enum Frame {
    /// A CoreVideo pixel buffer in the bi-planar full-range 4:2:0 format,
    /// which is the only format that GPUI's `surface` element draws.
    #[cfg(target_os = "macos")]
    Surface(objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>),
    /// An image whose pixels are in the order blue, green, red, alpha,
    /// which is the order `RenderImage` holds them in.
    Image(Arc<RenderImage>),
}

/// The state of the media, as a backend reports it. The player asks for it
/// once each time the player is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// The playback position, in seconds.
    pub position: f64,
    /// The length of the media in seconds. It is 0 until the length is
    /// known.
    pub duration: f64,
    /// The end of the last buffered range, in seconds.
    pub buffered: f64,
    /// True while the media plays. It is false while playback is paused and
    /// after the media has stopped at its end.
    pub playing: bool,
    /// True when the media has stopped at its end.
    pub ended: bool,
    /// The reason the media cannot be played, or `None` when it can.
    pub error: Option<String>,
    /// The width and the height of the video's picture in pixels, or `None`
    /// while they are not known.
    pub size: Option<(u32, u32)>,
    /// The video's frame rate in frames a second, or `None` while the
    /// backend does not know it.
    pub fps: Option<f64>,
}

/// What plays one media file: it decodes the file and produces its frames
/// and its sound. Every function returns at once. A backend reads and
/// decodes the file on its own threads, never on the thread that draws.
pub trait Backend {
    /// Starts playback, or continues it after a pause.
    fn play(&mut self);
    /// Pauses playback.
    fn pause(&mut self);
    /// Moves the position to `seconds`. Media that plays keeps playing, and
    /// media that is paused stays paused.
    fn seek(&mut self, seconds: f64);
    /// Sets the playback rate, where 1 is the normal speed.
    fn set_rate(&mut self, rate: f32);
    /// Sets the volume, from 0 to 1, and whether the sound is muted.
    fn set_volume(&mut self, volume: f32, muted: bool);
    /// Returns the state of the media now.
    fn status(&mut self) -> Status;
    /// Returns the video frame to draw now. Returns `None` when that is
    /// still the frame the last call returned, or when there is no frame.
    fn take_frame(&mut self) -> Option<Frame>;
    /// Returns access to the samples that the output device played last,
    /// which the audio visualizer draws. The default returns `None`, for a
    /// backend that does not keep these samples.
    fn analysis(&self) -> Option<Analysis> {
        None
    }
    /// Returns true when the backend is AVFoundation. When AVFoundation
    /// reports an error for a file before it has delivered a frame, the
    /// player opens the file with `ffmpeg` in its place.
    fn is_native(&self) -> bool {
        false
    }
}

/// Returns true when `mime` is one of the media types that AVFoundation
/// plays on macOS: QuickTime, MPEG-4 and 3GPP video.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn native_video(mime: &str) -> bool {
    matches!(
        mime,
        "video/quicktime" | "video/mp4" | "video/x-m4v" | "video/3gpp" | "video/3gpp2"
    )
}

/// Opens the video file `path`. On macOS a file whose media type
/// AVFoundation plays (`native_video`) is opened with AVFoundation. Every
/// other file, and on Windows and Linux every file, is opened with
/// `ffmpeg`.
pub fn open_video(path: &Path, mime: &str) -> Result<Box<dyn Backend>, String> {
    // AVFoundation can fail to open the file at once. It does so when this
    // function is not called on the main thread, which is the case in the
    // checks of `offscreen.rs`. The file is then opened with `ffmpeg`, as
    // it is when AVFoundation reports an error for the file later.
    #[cfg(target_os = "macos")]
    if native_video(mime)
        && let Ok(player) = super::avf::AvPlayer::open(path)
    {
        return Ok(Box::new(player));
    }
    let _ = mime;
    open_video_fallback(path)
}

/// Opens the video file `path` with the `ffmpeg` program that Picto ships
/// with.
pub fn open_video_fallback(path: &Path) -> Result<Box<dyn Backend>, String> {
    Ok(Box::new(super::ffmpeg::FfmpegPlayer::open(path)?))
}

/// Opens the audio file `path` with the decoder in `audio.rs`.
pub fn open_audio(path: &Path) -> Result<Box<dyn Backend>, String> {
    Ok(Box::new(super::audio::AudioPlayer::open(path)?))
}
