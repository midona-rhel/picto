//! Tests of what the core does with the `ffmpeg` and `ffprobe` programs:
//! reading the facts of a video or audio file, and making a video's
//! thumbnail and the picture of its first frame.
//!
//! The tests run the two programs. They are taken from `vendor/ffmpeg/`,
//! where `bash scripts/download-ffmpeg.sh` puts them, or else from the
//! folders in `PATH`.

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
        // The frame is taken from 50% of the way into the video.
        50,
        dur,
    )
    .await
    .expect("render_video_thumbnail failed");

    // The result is a JPEG, which starts with the bytes FF D8.
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

/// Makes a video of 320 × 240 pixels at 30 frames a second that is red for
/// its first second and blue for the three seconds after it, and returns its
/// path. The first frame is therefore red, and the frame at 35% of the
/// video's length, where the thumbnail is taken, is blue.
fn red_then_blue_video(folder: &Path) -> PathBuf {
    let path = folder.join("red-then-blue.mp4");
    let status = std::process::Command::new(find_ffmpeg())
        .args([
            "-v",
            "quiet",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=320x240:r=30:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=320x240:r=30:d=3",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1[v]",
            "-map",
            "[v]",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .status()
        .expect("failed to run ffmpeg to create the red and blue video");
    assert!(status.success(), "ffmpeg failed to create the video");
    path
}

/// Imports the video at `path` into the library as one active item and
/// returns its content hash and its root.
fn import_video(
    application: &picto_core::library_application::LibraryApplication,
    path: &Path,
) -> (String, picto_library::RootId) {
    use sha2::Digest as _;
    let bytes = std::fs::read(path).unwrap();
    let content_hash = hex::encode(sha2::Sha256::digest(&bytes));
    application
        .library()
        .enqueue_ingest_job(
            &picto_library::PreparedIngestJob {
                job_key: "manual:red-then-blue".into(),
                source_kind: "manual".into(),
                source_path: path.to_string_lossy().into_owned(),
                source_item_id: None,
                delete_after_ingest: false,
                payload: picto_library::PreparedIngestPayload::Item(
                    picto_library::PreparedImport {
                        stable_key: "red-then-blue".into(),
                        media_name: "red-then-blue.mp4".into(),
                        file_path: path.to_string_lossy().into_owned(),
                        facts: picto_library::ImmutableMediaFacts {
                            mime: "video/mp4".into(),
                            size_bytes: bytes.len() as u64,
                            width: Some(320),
                            height: Some(240),
                            duration_ms: Some(4_000),
                            frame_count: Some(120),
                            content_hash: content_hash.clone(),
                            perceptual_hash: None,
                            palette: Vec::new(),
                        },
                        lifecycle: picto_library::Lifecycle::Active,
                        rating: picto_library::Rating::Unrated,
                        notes: None,
                        tags: Vec::new(),
                        folders: Vec::new(),
                        source_urls: Vec::new(),
                        source_identity: None,
                        imported_at_ms: 1_700_000_000_000,
                        captured_at_ms: None,
                    },
                ),
            },
            "2026-11-22T12:00:00Z",
        )
        .unwrap();
    let ingested = picto_core::library_ingest_runtime::run_batch(application, 64).unwrap();
    (content_hash, ingested.root_ids[0])
}

/// Returns the average red, green and blue of the picture in the file
/// `path`, each from 0 to 255.
fn average_colour(path: &Path) -> (u64, u64, u64) {
    let picture = image::open(path)
        .expect("the picture is not a valid image")
        .to_rgb8();
    let pixels = u64::from(picture.width()) * u64::from(picture.height());
    let sum = picture.pixels().fold((0, 0, 0), |sum, pixel| {
        (
            sum.0 + u64::from(pixel[0]),
            sum.1 + u64::from(pixel[1]),
            sum.2 + u64::from(pixel[2]),
        )
    });
    (sum.0 / pixels, sum.1 / pixels, sum.2 / pixels)
}

fn is_red((red, _, blue): (u64, u64, u64)) -> bool {
    red > 200 && blue < 60
}

fn is_blue((red, _, blue): (u64, u64, u64)) -> bool {
    blue > 200 && red < 60
}

/// The background work that makes an imported video's thumbnail also makes
/// the picture of its first frame, and stores it beside the thumbnail. The
/// first-frame picture shows the video's first frame, which is red. The
/// thumbnail shows the frame at 35% of the video's length, which is blue.
#[tokio::test]
async fn a_videos_first_frame_picture_is_its_first_frame_and_its_thumbnail_is_not() {
    let folder = tempfile::tempdir().unwrap();
    let application =
        picto_core::library_application::LibraryApplication::create(folder.path().join("library"))
            .unwrap();
    let (hash, _) = import_video(&application, &red_then_blue_video(folder.path()));
    let blobs = application.blobs();
    assert!(blobs.find_thumbnail_path(&hash).unwrap().is_none());
    assert!(blobs.find_first_frame_path(&hash).unwrap().is_none());

    let report = picto_core::library_media_runtime::drain_batch(&application, 8)
        .await
        .unwrap();
    assert_eq!(report.failed + report.retried, 0);

    let thumbnail = blobs.find_thumbnail_path(&hash).unwrap().unwrap();
    let first_frame = blobs.find_first_frame_path(&hash).unwrap().unwrap();
    assert_eq!(first_frame.parent(), thumbnail.parent());
    assert!(
        is_red(average_colour(&first_frame)),
        "the first-frame picture is {:?}",
        average_colour(&first_frame)
    );
    assert!(
        is_blue(average_colour(&thumbnail)),
        "the thumbnail is {:?}",
        average_colour(&thumbnail)
    );
}

/// A video that has a thumbnail and no first-frame picture, as a video has
/// that was imported before first-frame pictures were kept, gets the picture
/// when it is asked for with `render_thumbnail_now`. Its thumbnail is left
/// as it is. When the thumbnail is missing, both pictures are made again,
/// and the first-frame picture that was there is replaced.
#[tokio::test]
async fn a_video_without_a_first_frame_picture_gets_one_when_it_is_asked_for() {
    let folder = tempfile::tempdir().unwrap();
    let application =
        picto_core::library_application::LibraryApplication::create(folder.path().join("library"))
            .unwrap();
    let (hash, _) = import_video(&application, &red_then_blue_video(folder.path()));
    let blobs = application.blobs();
    // The video gets a thumbnail that this test writes, and no first-frame
    // picture.
    blobs
        .write_thumbnail(&hash, b"the thumbnail", "jpg")
        .unwrap();

    picto_core::library_media_runtime::render_thumbnail_now(&application, &hash)
        .await
        .unwrap();

    let thumbnail = blobs.find_thumbnail_path(&hash).unwrap().unwrap();
    let first_frame = blobs.find_first_frame_path(&hash).unwrap().unwrap();
    assert_eq!(std::fs::read(&thumbnail).unwrap(), b"the thumbnail");
    assert!(is_red(average_colour(&first_frame)));

    // Regenerate Thumbnails moves the thumbnail away and asks for the
    // pictures again.
    std::fs::remove_file(&thumbnail).unwrap();
    std::fs::write(&first_frame, b"an earlier first-frame picture").unwrap();

    picto_core::library_media_runtime::render_thumbnail_now(&application, &hash)
        .await
        .unwrap();

    assert!(is_blue(average_colour(&thumbnail)));
    assert!(is_red(average_colour(&first_frame)));
}

/// Deleting a video permanently removes its original, its thumbnail and its
/// first-frame picture from the library's folder.
#[tokio::test]
async fn deleting_a_video_permanently_removes_its_thumbnail_and_its_first_frame_picture() {
    let folder = tempfile::tempdir().unwrap();
    let application =
        picto_core::library_application::LibraryApplication::create(folder.path().join("library"))
            .unwrap();
    let (hash, root) = import_video(&application, &red_then_blue_video(folder.path()));
    picto_core::library_media_runtime::render_thumbnail_now(&application, &hash)
        .await
        .unwrap();
    let blobs = application.blobs();
    let original = blobs.find_original(&hash, Some("mp4")).unwrap().unwrap().0;
    let thumbnail = blobs.find_thumbnail_path(&hash).unwrap().unwrap();
    let first_frame = blobs.find_first_frame_path(&hash).unwrap().unwrap();

    let target = picto_library::selection::SelectionTarget::Explicit {
        root_ids: vec![root],
    };
    application
        .set_lifecycle(&target, picto_library::Lifecycle::Trash)
        .unwrap();
    // An undo limit of zero makes this a delete that cannot be undone, so
    // the files are removed by the next cleanup and are not kept for an
    // undo.
    application
        .library()
        .permanently_delete(&target, 1_700_000_100_000, 0, false)
        .unwrap();
    let removed = picto_core::library_media_runtime::drain_blob_cleanup(&application, 8).unwrap();

    assert_eq!(removed, 1);
    assert!(!original.exists());
    assert!(!thumbnail.exists());
    assert!(!first_frame.exists());
}
