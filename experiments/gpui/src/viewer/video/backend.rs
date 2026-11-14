//! The one seam between the player and whatever decodes the media. The
//! player's model and controls know only this trait; each platform's decoder
//! implements it.
//!
//! - macOS video (QuickTime, MPEG-4): AVFoundation (`avf.rs`), whose frames
//!   GPUI presents as CoreVideo pixel buffers without copying.
//! - Other video (WebM, Matroska, and anything AVFoundation refuses): the
//!   `ffmpeg` binary the project already bundles for thumbnails
//!   (`ffmpeg.rs`), frames uploaded as images, sound through `audio.rs`.
//! - Audio: decoded in-process (`audio.rs`), played through CPAL, the output
//!   the Flash player uses, which also hands the visualizer its samples.
use super::audio::Analysis;
use gpui::RenderImage;
use std::path::Path;
use std::sync::Arc;

/// A decoded picture ready to draw.
#[derive(Clone)]
pub enum Frame {
    /// A CoreVideo buffer in the bi-planar full-range 4:2:0 format, the only
    /// one GPUI's `surface` element accepts.
    #[cfg(target_os = "macos")]
    Surface(objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>),
    /// Pixels uploaded as an image (BGRA, as `RenderImage` holds them).
    Image(Arc<RenderImage>),
}

/// What the media is doing, polled once per drawn frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// Where playback is, in seconds.
    pub position: f64,
    /// The length, once known; 0 until then.
    pub duration: f64,
    /// The end of the last buffered range, in seconds.
    pub buffered: f64,
    /// Playing (not paused, not stopped at the end).
    pub playing: bool,
    /// Stopped at the end.
    pub ended: bool,
    /// The media cannot be played; the reason.
    pub error: Option<String>,
    /// The picture's size in pixels, once known.
    pub size: Option<(u32, u32)>,
    /// Frames per second, when the decoder knows it.
    pub fps: Option<f64>,
}

/// A media decoder and its output. Every call returns at once: decoding and
/// file reading happen on the backend's own threads, never on the thread
/// that draws.
pub trait Backend {
    fn play(&mut self);
    fn pause(&mut self);
    /// Moves to `seconds`, keeping whether it plays.
    fn seek(&mut self, seconds: f64);
    fn set_rate(&mut self, rate: f32);
    fn set_volume(&mut self, volume: f32, muted: bool);
    fn status(&mut self) -> Status;
    /// The picture to show now, if it changed since the last call.
    fn take_frame(&mut self) -> Option<Frame>;
    /// The samples the sound card is playing, for the audio visualizer.
    fn analysis(&self) -> Option<Analysis> {
        None
    }
    /// Whether this backend is AVFoundation, which the player falls back from
    /// to `ffmpeg` when it refuses a file.
    fn is_native(&self) -> bool {
        false
    }
}

/// Container types AVFoundation plays on macOS.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn native_video(mime: &str) -> bool {
    matches!(
        mime,
        "video/quicktime" | "video/mp4" | "video/x-m4v" | "video/3gpp" | "video/3gpp2"
    )
}

/// Opens a video with the platform's player when it takes the container,
/// else through `ffmpeg`.
pub fn open_video(path: &Path, mime: &str) -> Result<Box<dyn Backend>, String> {
    // AVFoundation can refuse at once (off the main thread, as in the
    // off-screen checks); `ffmpeg` then plays the file, as it does when
    // AVFoundation turns the file down later.
    #[cfg(target_os = "macos")]
    if native_video(mime)
        && let Ok(player) = super::avf::AvPlayer::open(path)
    {
        return Ok(Box::new(player));
    }
    let _ = mime;
    open_video_fallback(path)
}

/// Opens a video through the bundled `ffmpeg`.
pub fn open_video_fallback(path: &Path) -> Result<Box<dyn Backend>, String> {
    Ok(Box::new(super::ffmpeg::FfmpegPlayer::open(path)?))
}

/// Opens an audio file.
pub fn open_audio(path: &Path) -> Result<Box<dyn Backend>, String> {
    Ok(Box::new(super::audio::AudioPlayer::open(path)?))
}
