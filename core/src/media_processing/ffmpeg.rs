//! Reads facts about video and audio files, and makes pictures of them, by
//! running the `ffmpeg` and `ffprobe` programs as separate processes. The
//! FFmpeg libraries are not linked into the app. `ffmpeg_path.rs` finds the
//! two programs.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

use crate::constants::MimeType;

const FFPROBE_TIMEOUT: Duration = Duration::from_secs(30);
const FFMPEG_FRAME_TIMEOUT: Duration = Duration::from_secs(60);

// --- Error type ---

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
    #[error("ffmpeg timed out after {0:?}")]
    Timeout(Duration),
}

pub type FfmpegResult<T> = Result<T, FfmpegError>;

// --- Internal helpers — run ffprobe/ffmpeg ---

/// Runs ffprobe on `path` and returns what it prints, parsed as JSON. The
/// process is killed if the caller stops waiting for it
/// (`kill_on_drop(true)`).
async fn run_ffprobe(path: &Path) -> FfmpegResult<serde_json::Value> {
    let bin = super::ffmpeg_path::ffprobe_path().map_err(FfmpegError::Process)?;

    let mut cmd = Command::new(bin.as_path());
    cmd.args([
        "-v",
        "quiet",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
    ])
    .arg(path)
    .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = tokio::time::timeout(FFPROBE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| FfmpegError::Timeout(FFPROBE_TIMEOUT))?
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

/// Runs ffmpeg to read one frame of `path`, `seek_secs` seconds in, and
/// returns it as a JPEG no larger than `target_w` × `target_h`. The process is
/// killed if the caller stops waiting for it (`kill_on_drop(true)`). ffmpeg
/// prints only its error messages (`-v error`), and a failure's message
/// includes them.
async fn run_ffmpeg_frame(
    path: &Path,
    seek_secs: f64,
    target_w: u32,
    target_h: u32,
) -> FfmpegResult<Vec<u8>> {
    let bin = super::ffmpeg_path::ffmpeg_path().map_err(FfmpegError::Process)?;

    let vf = format!(
        "scale='min({target_w},iw)':'min({target_h},ih)':force_original_aspect_ratio=decrease"
    );

    let mut cmd = Command::new(bin.as_path());
    cmd.kill_on_drop(true);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.args(["-v", "error"]);
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
        "4",
        "-",
    ]);

    let output = tokio::time::timeout(FFMPEG_FRAME_TIMEOUT, cmd.output())
        .await
        .map_err(|_| FfmpegError::Timeout(FFMPEG_FRAME_TIMEOUT))?
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

/// Draws the waveform of the audio in `path` as a PNG with a transparent
/// background. All channels are mixed into one, and the waveform is scaled so
/// that its highest peak reaches the full height.
pub async fn render_audio_waveform(
    path: &Path,
    target_resolution: (u32, u32),
) -> FfmpegResult<Vec<u8>> {
    let bin = super::ffmpeg_path::ffmpeg_path().map_err(FfmpegError::Process)?;
    let width = target_resolution.0.max(2);
    let height = (target_resolution.1 / 2).max(1);
    let filter = format!(
        "aformat=channel_layouts=mono,showwavespic=s={width}x{height}:colors=0x0072EFFF:scale=lin:draw=full:filter=peak,format=rgba"
    );

    let mut cmd = Command::new(bin.as_path());
    cmd.kill_on_drop(true);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd.args(["-v", "quiet", "-i"]).arg(path).args([
        "-filter_complex",
        &filter,
        "-frames:v",
        "1",
        "-f",
        "image2pipe",
        "-c:v",
        "png",
        "-",
    ]);

    let output = tokio::time::timeout(FFMPEG_FRAME_TIMEOUT, cmd.output())
        .await
        .map_err(|_| FfmpegError::Timeout(FFMPEG_FRAME_TIMEOUT))?
        .map_err(|error| FfmpegError::Process(format!("failed to run ffmpeg: {error}")))?;
    if !output.status.success() || output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FfmpegError::DamagedOrUnusualFile(format!(
            "ffmpeg waveform generation failed (status {}): {stderr}",
            output.status
        )));
    }
    Ok(output.stdout)
}

// --- ffprobe JSON helpers ---

/// The first stream with the given `codec_type` in ffprobe's JSON.
fn find_stream<'a>(
    probe: &'a serde_json::Value,
    codec_type: &str,
) -> Option<&'a serde_json::Value> {
    probe["streams"]
        .as_array()?
        .iter()
        .find(|s| s["codec_type"].as_str() == Some(codec_type))
}

/// Converts a duration that ffprobe gives as decimal seconds in a string to
/// milliseconds.
fn parse_duration_ms(val: &serde_json::Value) -> Option<u64> {
    val.as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as u64)
        .filter(|&ms| ms > 0)
}

// --- Video properties ---

#[derive(Debug, Clone)]
pub struct VideoProperties {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub num_frames: u64,
    pub has_audio: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ProbedMediaInfo {
    pub mime: MimeType,
    pub video: Option<VideoProperties>,
    pub audio_duration_ms: Option<u64>,
    pub has_audio: bool,
}

pub(crate) async fn probe_media_info(path: &Path) -> FfmpegResult<ProbedMediaInfo> {
    let probe = run_ffprobe(path).await?;
    let has_video = find_stream(&probe, "video").is_some();
    let has_audio = find_stream(&probe, "audio").is_some();
    let format_name = probe["format"]["format_name"].as_str().unwrap_or("");
    let mime = map_ffmpeg_format_to_mime(format_name, has_video, has_audio);
    let video = find_stream(&probe, "video").map(|video| video_properties(&probe, video));
    let audio_duration_ms = find_stream(&probe, "audio").and_then(|audio| {
        parse_duration_ms(&audio["duration"])
            .or_else(|| parse_duration_ms(&probe["format"]["duration"]))
    });
    Ok(ProbedMediaInfo {
        mime,
        video,
        audio_duration_ms,
        has_audio,
    })
}

/// The size, duration, frame rate and frame count of the video in a file.
pub async fn get_video_properties(path: &Path) -> FfmpegResult<VideoProperties> {
    probe_media_info(path)
        .await?
        .video
        .ok_or(FfmpegError::NoVideoStream)
}

fn video_properties(probe: &serde_json::Value, video: &serde_json::Value) -> VideoProperties {
    let width = video["width"].as_u64().unwrap_or(0) as u32;
    let height = video["height"].as_u64().unwrap_or(0) as u32;

    // The duration is taken from the video stream, or else from the file as
    // a whole.
    let duration_ms = parse_duration_ms(&video["duration"])
        .or_else(|| parse_duration_ms(&probe["format"]["duration"]))
        .unwrap_or(0);

    // The frame count is ffprobe's `nb_frames`, or else an estimate: the
    // duration multiplied by the frame rate.
    let num_frames = video["nb_frames"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| estimate_frame_count(video, duration_ms));

    let has_audio = find_stream(probe, "audio").is_some();

    // The width is corrected for pixels that are not square (the sample
    // aspect ratio).
    let (adj_w, adj_h) = apply_sar(video, width, height);

    // Width and height are swapped for a video that is stored rotated by a
    // quarter turn.
    let (final_w, final_h) = apply_rotation(video, adj_w, adj_h);

    VideoProperties {
        width: final_w.max(1),
        height: final_h.max(1),
        duration_ms,
        num_frames: num_frames.max(1),
        has_audio,
    }
}

/// An estimate of the frame count: the duration multiplied by the frame
/// rate.
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

/// Reads a frame rate that ffprobe gives as a fraction, such as
/// "30000/1001" or "24/1".
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

/// Corrects the width for pixels that are not square, when the stream gives
/// a sample aspect ratio.
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

/// Swaps width and height when the stream says the video is rotated by a
/// quarter turn in either direction.
fn apply_rotation(video: &serde_json::Value, width: u32, height: u32) -> (u32, u32) {
    // The rotation is in `side_data_list`, or else in `tags.rotate`.
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

// --- Audio properties ---

/// The duration of the audio in a file, in milliseconds.
pub async fn get_audio_duration_ms(path: &Path) -> FfmpegResult<u64> {
    probe_media_info(path)
        .await?
        .audio_duration_ms
        .ok_or_else(|| {
            FfmpegError::DamagedOrUnusualFile("Could not determine audio duration".to_string())
        })
}

// --- MIME detection ---

/// The type of a file, from the container format that ffprobe detects.
pub async fn get_mime(path: &Path) -> FfmpegResult<MimeType> {
    probe_media_info(path).await.map(|info| info.mime)
}

/// The `MimeType` for one of ffmpeg's format names.
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

// --- Animation detection ---

/// Whether a file is animated, which means it has more than one frame.
pub async fn file_is_animated(path: &Path) -> bool {
    match get_video_properties(path).await {
        Ok(props) => props.num_frames > 1,
        Err(_) => false,
    }
}

// --- Video thumbnail generation ---

/// Returns one frame of a video file as a JPEG.
///
/// `percentage_in` (0 to 100) says how far into the video the frame is
/// taken. Without a known duration the first frame is taken. The picture is
/// made to fit inside `target_resolution` (width, height).
pub async fn render_video_thumbnail(
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
    run_ffmpeg_frame(path, seek_secs, tw, th).await
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

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
