//! FFmpeg integration via CLI subprocess calls.
//!
//! Calls bundled `ffmpeg` and `ffprobe` binaries instead of linking to libav* directly.
//! Binary paths are resolved by `ffmpeg_path.rs`.

use std::path::Path;
use std::process::Command;

use crate::constants::MimeType;

// ===========================================================================
// Error type
// ===========================================================================

#[derive(thiserror::Error, Debug)]
pub enum FfmpegError {
    #[error("ffmpeg process error: {0}")]
    Process(String),
    #[error("no video stream found")]
    NoVideoStream,
    #[error("no audio stream found")]
    NoAudioStream,
    #[error("damaged or unusual file: {0}")]
    DamagedOrUnusualFile(String),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type FfmpegResult<T> = Result<T, FfmpegError>;

// ===========================================================================
// Internal helpers — run ffprobe/ffmpeg
// ===========================================================================

/// Run ffprobe and return parsed JSON output.
fn run_ffprobe(path: &Path) -> FfmpegResult<serde_json::Value> {
    let bin = super::ffmpeg_path::ffprobe_path().map_err(FfmpegError::Process)?;

    let output = Command::new(bin.as_path())
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| FfmpegError::Process(format!("failed to run ffprobe: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FfmpegError::Process(format!(
            "ffprobe exited with {}: {stderr}",
            output.status
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| FfmpegError::Process(format!("failed to parse ffprobe JSON: {e}")))
}

/// Run ffmpeg to extract a single frame as JPEG bytes.
fn run_ffmpeg_frame(
    path: &Path,
    seek_secs: f64,
    target_w: u32,
    target_h: u32,
) -> FfmpegResult<Vec<u8>> {
    let bin = super::ffmpeg_path::ffmpeg_path().map_err(FfmpegError::Process)?;

    // Build the scale filter: scale down to fit within target_w x target_h, maintaining
    // aspect ratio. Only scale down, never up (min with input dimensions).
    let vf = format!(
        "scale='min({target_w},iw)':'min({target_h},ih)':force_original_aspect_ratio=decrease"
    );

    let mut cmd = Command::new(bin.as_path());
    cmd.args(["-v", "quiet"]);

    // Pre-seek (before -i) for fast seeking to nearest keyframe
    if seek_secs > 0.01 {
        cmd.args(["-ss", &format!("{seek_secs:.3}")]);
    }

    cmd.arg("-i").arg(path);
    cmd.args([
        "-frames:v",
        "1",
        "-vf",
        &vf,
        "-f",
        "image2pipe",
        "-c:v",
        "mjpeg",
        "-q:v",
        "4", // JPEG quality (2=best, 31=worst; 4 ≈ 82% quality)
        "-",
    ]);

    let output = cmd
        .output()
        .map_err(|e| FfmpegError::Process(format!("failed to run ffmpeg: {e}")))?;

    if !output.status.success() || output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FfmpegError::DamagedOrUnusualFile(format!(
            "ffmpeg frame extraction failed (status {}): {stderr}",
            output.status
        )));
    }

    Ok(output.stdout)
}

// ===========================================================================
// ffprobe JSON helpers
// ===========================================================================

/// Find the first stream of a given codec_type in ffprobe JSON.
fn find_stream<'a>(
    probe: &'a serde_json::Value,
    codec_type: &str,
) -> Option<&'a serde_json::Value> {
    probe["streams"]
        .as_array()?
        .iter()
        .find(|s| s["codec_type"].as_str() == Some(codec_type))
}

/// Parse a duration string (seconds as decimal) to milliseconds.
fn parse_duration_ms(val: &serde_json::Value) -> Option<u64> {
    val.as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as u64)
        .filter(|&ms| ms > 0)
}

// ===========================================================================
// Video properties
// ===========================================================================

#[derive(Debug, Clone)]
pub struct VideoProperties {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub num_frames: u64,
    pub has_audio: bool,
}

/// Extract video properties from a file.
pub fn get_video_properties(path: &Path) -> FfmpegResult<VideoProperties> {
    let probe = run_ffprobe(path)?;

    let video = find_stream(&probe, "video").ok_or(FfmpegError::NoVideoStream)?;

    let width = video["width"].as_u64().unwrap_or(0) as u32;
    let height = video["height"].as_u64().unwrap_or(0) as u32;

    // Duration: try stream, then format
    let duration_ms = parse_duration_ms(&video["duration"])
        .or_else(|| parse_duration_ms(&probe["format"]["duration"]))
        .unwrap_or(0);

    // Frame count: try nb_frames, then estimate from duration * fps
    let num_frames = video["nb_frames"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| estimate_frame_count(video, duration_ms));

    let has_audio = find_stream(&probe, "audio").is_some();

    // Handle SAR (Sample Aspect Ratio)
    let (adj_w, adj_h) = apply_sar(video, width, height);

    // Handle rotation
    let (final_w, final_h) = apply_rotation(video, adj_w, adj_h);

    Ok(VideoProperties {
        width: final_w.max(1),
        height: final_h.max(1),
        duration_ms,
        num_frames: num_frames.max(1),
        has_audio,
    })
}

/// Estimate frame count from duration and frame rate.
fn estimate_frame_count(video: &serde_json::Value, duration_ms: u64) -> u64 {
    if duration_ms == 0 {
        return 1;
    }
    let fps = parse_fps(&video["avg_frame_rate"])
        .or_else(|| parse_fps(&video["r_frame_rate"]))
        .unwrap_or(24.0);
    let secs = duration_ms as f64 / 1000.0;
    (secs * fps).round().max(1.0) as u64
}

/// Parse a fractional frame rate string like "30000/1001" or "24/1".
fn parse_fps(val: &serde_json::Value) -> Option<f64> {
    let s = val.as_str()?;
    if let Some((num_s, den_s)) = s.split_once('/') {
        let num: f64 = num_s.parse().ok()?;
        let den: f64 = den_s.parse().ok()?;
        if den > 0.0 {
            return Some(num / den);
        }
    }
    s.parse::<f64>().ok().filter(|&v| v > 0.0)
}

/// Adjust width by Sample Aspect Ratio if present.
fn apply_sar(video: &serde_json::Value, width: u32, height: u32) -> (u32, u32) {
    if let Some(sar_str) = video["sample_aspect_ratio"].as_str() {
        if let Some((num_s, den_s)) = sar_str.split_once(':') {
            if let (Ok(num), Ok(den)) = (num_s.parse::<f64>(), den_s.parse::<f64>()) {
                if den > 0.0 && num > 0.0 && (num - den).abs() > 0.01 {
                    let adj = (width as f64 * num / den).round() as u32;
                    return (adj, height);
                }
            }
        }
    }
    (width, height)
}

/// Swap width/height if rotation metadata indicates ±90°.
fn apply_rotation(video: &serde_json::Value, width: u32, height: u32) -> (u32, u32) {
    // Check side_data_list for rotation, or tags.rotate
    let rotation = video["tags"]["rotate"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            video["side_data_list"].as_array().and_then(|arr| {
                arr.iter().find_map(|sd| {
                    if sd["side_data_type"].as_str() == Some("Display Matrix") {
                        sd["rotation"].as_f64()
                    } else {
                        None
                    }
                })
            })
        });

    if let Some(rot) = rotation {
        let abs_rot = rot.abs() % 360.0;
        if (abs_rot - 90.0).abs() < 1.0 || (abs_rot - 270.0).abs() < 1.0 {
            return (height, width);
        }
    }

    (width, height)
}

// ===========================================================================
// Audio properties
// ===========================================================================

/// Get audio duration in milliseconds.
pub fn get_audio_duration_ms(path: &Path) -> FfmpegResult<u64> {
    let probe = run_ffprobe(path)?;

    let audio = find_stream(&probe, "audio").ok_or(FfmpegError::NoAudioStream)?;

    let duration_ms = parse_duration_ms(&audio["duration"])
        .or_else(|| parse_duration_ms(&probe["format"]["duration"]))
        .ok_or_else(|| {
            FfmpegError::DamagedOrUnusualFile("Could not determine audio duration".to_string())
        })?;

    Ok(duration_ms)
}

// ===========================================================================
// MIME detection
// ===========================================================================

/// Determine MIME type using ffprobe's format probing.
pub fn get_mime(path: &Path) -> FfmpegResult<MimeType> {
    let probe = run_ffprobe(path)?;

    let format_name = probe["format"]["format_name"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let has_video = find_stream(&probe, "video").is_some();
    let has_audio = find_stream(&probe, "audio").is_some();

    Ok(map_ffmpeg_format_to_mime(
        &format_name,
        has_video,
        has_audio,
    ))
}

/// Map ffmpeg format name to MimeType.
fn map_ffmpeg_format_to_mime(format_name: &str, has_video: bool, has_audio: bool) -> MimeType {
    let formats: Vec<&str> = format_name.split(',').collect();
    let primary = formats[0].trim();

    match primary {
        "matroska" | "webm" => {
            if has_video {
                if formats.iter().any(|f| f.trim() == "webm") {
                    MimeType::VideoWebm
                } else {
                    MimeType::VideoMkv
                }
            } else if has_audio {
                MimeType::AudioMkv
            } else {
                MimeType::VideoMkv
            }
        }
        "mov" => {
            if has_video {
                MimeType::VideoMov
            } else if has_audio {
                MimeType::AudioM4a
            } else {
                MimeType::VideoMov
            }
        }
        "mp4" | "m4a" => {
            if has_video {
                MimeType::VideoMp4
            } else if has_audio {
                MimeType::AudioM4a
            } else {
                MimeType::VideoMp4
            }
        }
        "avi" => MimeType::VideoAvi,
        "flv" => MimeType::VideoFlv,
        "mpeg" | "mpegts" | "mpegvideo" => MimeType::VideoMpeg,
        "asf" | "wmv" => {
            if has_video {
                MimeType::VideoWmv
            } else {
                MimeType::AudioWma
            }
        }
        "rm" | "realmedia" => {
            if has_video {
                MimeType::VideoRealmedia
            } else {
                MimeType::AudioRealmedia
            }
        }
        "ogg" => {
            if has_video {
                MimeType::VideoOgv
            } else {
                MimeType::AudioOgg
            }
        }
        "mp3" => MimeType::AudioMp3,
        "flac" => MimeType::AudioFlac,
        "wav" => MimeType::AudioWave,
        "wma" => MimeType::AudioWma,
        "tta" => MimeType::AudioTrueaudio,
        "wv" | "wavpack" => MimeType::AudioWavpack,
        _ => {
            if has_video {
                MimeType::GeneralVideo
            } else if has_audio {
                MimeType::GeneralAudio
            } else {
                MimeType::ApplicationUnknown
            }
        }
    }
}

// ===========================================================================
// Legacy wrapper
// ===========================================================================

/// Extract video properties matching the tuple return signature.
pub fn get_ffmpeg_video_properties(
    path: &Path,
    _force_count_frames_manually: bool,
) -> FfmpegResult<((u32, u32), u64, u64, bool)> {
    let props = get_video_properties(path)?;
    Ok((
        (props.width, props.height),
        props.duration_ms,
        props.num_frames,
        props.has_audio,
    ))
}

// ===========================================================================
// Animation detection
// ===========================================================================

/// Check if a file is animated (has more than 1 frame).
pub fn file_is_animated(path: &Path) -> bool {
    match get_video_properties(path) {
        Ok(props) => props.num_frames > 1,
        Err(_) => false,
    }
}

// ===========================================================================
// Video thumbnail generation
// ===========================================================================

/// Render a video frame as JPEG bytes for thumbnail generation.
///
/// Seeks to `percentage_in`% of the video and extracts a single frame,
/// scaled down to fit within `target_resolution`.
pub fn render_video_frame_to_png(
    path: &Path,
    target_resolution: (u32, u32),
    percentage_in: u32,
    _num_frames: u64,
    duration_ms: u64,
) -> FfmpegResult<Vec<u8>> {
    render_video_thumbnail(
        path,
        target_resolution,
        percentage_in,
        if duration_ms > 0 {
            Some(duration_ms)
        } else {
            None
        },
    )
}

/// Render a specific frame from a video file as JPEG bytes.
///
/// `percentage_in` is 0-100, indicating where in the video to grab the frame.
/// `target_resolution` is the bounding box to resize into.
pub fn render_video_thumbnail(
    path: &Path,
    target_resolution: (u32, u32),
    percentage_in: u32,
    duration_ms: Option<u64>,
) -> FfmpegResult<Vec<u8>> {
    let seek_secs = if let Some(dur) = duration_ms {
        let fraction = (percentage_in as f64 / 100.0).clamp(0.0, 1.0);
        (dur as f64 / 1000.0) * fraction
    } else {
        0.0
    };

    let (tw, th) = target_resolution;
    run_ffmpeg_frame(path, seek_secs, tw, th)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_ffmpeg_format_mp4_video() {
        let mime = map_ffmpeg_format_to_mime("mov,mp4,m4a,3gp,3g2,mj2", true, true);
        assert_eq!(mime, MimeType::VideoMov);
    }

    #[test]
    fn test_map_ffmpeg_format_mp4_audio_only() {
        let mime = map_ffmpeg_format_to_mime("mov,mp4,m4a,3gp,3g2,mj2", false, true);
        assert_eq!(mime, MimeType::AudioM4a);
    }

    #[test]
    fn test_map_ffmpeg_format_mkv() {
        let mime = map_ffmpeg_format_to_mime("matroska,webm", true, true);
        assert_eq!(mime, MimeType::VideoWebm);
    }

    #[test]
    fn test_map_ffmpeg_format_mp3() {
        let mime = map_ffmpeg_format_to_mime("mp3", false, true);
        assert_eq!(mime, MimeType::AudioMp3);
    }

    #[test]
    fn test_map_ffmpeg_format_flac() {
        let mime = map_ffmpeg_format_to_mime("flac", false, true);
        assert_eq!(mime, MimeType::AudioFlac);
    }

    #[test]
    fn test_map_ffmpeg_format_wmv_video() {
        let mime = map_ffmpeg_format_to_mime("asf", true, true);
        assert_eq!(mime, MimeType::VideoWmv);
    }

    #[test]
    fn test_map_ffmpeg_format_wma_audio() {
        let mime = map_ffmpeg_format_to_mime("asf", false, true);
        assert_eq!(mime, MimeType::AudioWma);
    }

    #[test]
    fn test_map_ffmpeg_format_unknown() {
        let mime = map_ffmpeg_format_to_mime("some_unknown_format", false, false);
        assert_eq!(mime, MimeType::ApplicationUnknown);
    }

    #[test]
    fn test_map_ffmpeg_format_ogg_video() {
        let mime = map_ffmpeg_format_to_mime("ogg", true, true);
        assert_eq!(mime, MimeType::VideoOgv);
    }

    #[test]
    fn test_map_ffmpeg_format_ogg_audio() {
        let mime = map_ffmpeg_format_to_mime("ogg", false, true);
        assert_eq!(mime, MimeType::AudioOgg);
    }

    #[test]
    fn test_parse_fps_fractional() {
        let val = serde_json::json!("30000/1001");
        let fps = parse_fps(&val).unwrap();
        assert!((fps - 29.97).abs() < 0.01);
    }

    #[test]
    fn test_parse_fps_integer() {
        let val = serde_json::json!("24/1");
        let fps = parse_fps(&val).unwrap();
        assert!((fps - 24.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_fps_zero_den() {
        let val = serde_json::json!("0/0");
        assert!(parse_fps(&val).is_none());
    }
}
