//! Integration tests for the CLI-based FFmpeg module.
//!
//! These tests require `vendor/ffmpeg/` binaries to be present.
//! Run `bash scripts/download-ffmpeg.sh` first.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static TEST_VIDEO: OnceLock<PathBuf> = OnceLock::new();
static TEST_AUDIO: OnceLock<PathBuf> = OnceLock::new();

fn find_ffmpeg() -> PathBuf {
    let vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("vendor/ffmpeg/ffmpeg");
    if vendor.exists() {
        return vendor;
    }
    PathBuf::from("ffmpeg")
}

fn test_video() -> &'static Path {
    TEST_VIDEO.get_or_init(|| {
        let path = std::env::temp_dir().join("picto_ffmpeg_test.mp4");
        let ffmpeg = find_ffmpeg();
        let status = std::process::Command::new(&ffmpeg)
            .args([
                "-v",
                "quiet",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=640x480:rate=30",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path)
            .status()
            .expect("failed to run ffmpeg to create test video");
        assert!(status.success(), "ffmpeg failed to create test video");
        path
    })
}

fn test_audio() -> &'static Path {
    TEST_AUDIO.get_or_init(|| {
        let path = std::env::temp_dir().join("picto_ffmpeg_test.mp3");
        let ffmpeg = find_ffmpeg();
        let status = std::process::Command::new(&ffmpeg)
            .args([
                "-v",
                "quiet",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "128k",
            ])
            .arg(&path)
            .status()
            .expect("failed to run ffmpeg to create test audio");
        assert!(status.success(), "ffmpeg failed to create test audio");
        path
    })
}

#[tokio::test]
async fn test_video_properties() {
    let video = test_video();
    let props = picto_core::media_processing::ffmpeg::get_video_properties(video)
        .await
        .expect("get_video_properties failed");

    assert_eq!(props.width, 640);
    assert_eq!(props.height, 480);
    assert!(
        props.duration_ms >= 1900 && props.duration_ms <= 2100,
        "duration_ms = {} (expected ~2000)",
        props.duration_ms
    );
    assert_eq!(props.num_frames, 60);
    assert!(!props.has_audio);
}

#[tokio::test]
async fn test_audio_duration() {
    let audio = test_audio();
    let dur = picto_core::media_processing::ffmpeg::get_audio_duration_ms(audio)
        .await
        .expect("get_audio_duration_ms failed");

    assert!(
        (2900..=3100).contains(&dur),
        "duration_ms = {} (expected ~3000)",
        dur
    );
}

#[tokio::test]
async fn test_render_audio_waveform() {
    let bytes =
        picto_core::media_processing::ffmpeg::render_audio_waveform(test_audio(), (512, 512))
            .await
            .expect("render_audio_waveform failed");

    assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    let image = image::load_from_memory(&bytes).expect("waveform is not a valid image");
    assert_eq!((image.width(), image.height()), (512, 256));
    let pixels = image.to_rgba8();
    assert!(
        pixels.pixels().any(|pixel| pixel[3] == 0),
        "waveform background should be transparent"
    );
    assert!(
        pixels
            .pixels()
            .any(|pixel| pixel[2] > pixel[0] && pixel[2] > pixel[1] && pixel[3] > 0),
        "waveform should contain visible blue samples"
    );
}

#[tokio::test]
async fn test_mime_detection_video() {
    let video = test_video();
    let mime = picto_core::media_processing::ffmpeg::get_mime(video)
        .await
        .expect("get_mime failed");

    assert_eq!(
        mime,
        picto_core::constants::MimeType::VideoMov,
        "expected VideoMov for .mp4 (mov,mp4 format), got {:?}",
        mime
    );
}

#[tokio::test]
async fn test_mime_detection_audio() {
    let audio = test_audio();
    let mime = picto_core::media_processing::ffmpeg::get_mime(audio)
        .await
        .expect("get_mime failed");

    assert_eq!(
        mime,
        picto_core::constants::MimeType::AudioMp3,
        "expected AudioMp3, got {:?}",
        mime
    );
}

#[tokio::test]
async fn test_file_is_animated() {
    let video = test_video();
    assert!(picto_core::media_processing::ffmpeg::file_is_animated(video).await);
}

#[tokio::test]
async fn test_render_video_thumbnail() {
    let video = test_video();

    let props = picto_core::media_processing::ffmpeg::get_video_properties(video)
        .await
        .expect("get_video_properties failed");

    let dur = if props.duration_ms > 0 {
        Some(props.duration_ms)
    } else {
        None
    };
    let bytes = picto_core::media_processing::ffmpeg::render_video_thumbnail(
        video,
        (200, 200),
        50, // 50% into the video
        dur,
    )
    .await
    .expect("render_video_thumbnail failed");

    // Should be a valid JPEG (starts with FF D8)
    assert!(
        bytes.len() > 100,
        "thumbnail too small: {} bytes",
        bytes.len()
    );
    assert_eq!(bytes[0], 0xFF, "not a JPEG: first byte is {:02X}", bytes[0]);
    assert_eq!(
        bytes[1], 0xD8,
        "not a JPEG: second byte is {:02X}",
        bytes[1]
    );
}

#[tokio::test]
async fn test_render_video_thumbnail_at_start() {
    let video = test_video();

    let bytes = picto_core::media_processing::ffmpeg::render_video_thumbnail(
        video,
        (200, 200),
        0,
        Some(2000),
    )
    .await
    .expect("render_video_thumbnail at start failed");

    assert!(bytes.len() > 100);
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1], 0xD8);
}

#[tokio::test]
async fn test_file_info_video() {
    let video = test_video();
    let info = picto_core::media_processing::get_file_info(
        video,
        Some(picto_core::constants::MimeType::VideoMp4),
    )
    .await
    .expect("get_file_info failed");

    assert_eq!(info.width, Some(640));
    assert_eq!(info.height, Some(480));
    assert!(info.duration_ms.is_some());
    assert!(info.num_frames.is_some());
    assert_eq!(info.num_frames.unwrap(), 60);
}

#[tokio::test]
async fn test_file_info_audio() {
    let audio = test_audio();
    let info = picto_core::media_processing::get_file_info(
        audio,
        Some(picto_core::constants::MimeType::AudioMp3),
    )
    .await
    .expect("get_file_info failed");

    assert!(info.duration_ms.is_some());
    let dur = info.duration_ms.unwrap();
    assert!((2900..=3100).contains(&dur), "duration = {}", dur);
}

#[tokio::test]
async fn test_nonexistent_file_errors() {
    let bad_path = Path::new("/tmp/does_not_exist_picto_test.mp4");
    assert!(
        picto_core::media_processing::ffmpeg::get_video_properties(bad_path)
            .await
            .is_err()
    );
    assert!(
        picto_core::media_processing::ffmpeg::get_audio_duration_ms(bad_path)
            .await
            .is_err()
    );
    assert!(picto_core::media_processing::ffmpeg::get_mime(bad_path)
        .await
        .is_err());
}
