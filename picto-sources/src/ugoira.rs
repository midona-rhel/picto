use std::fmt::Write as _;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::{DownloadedMedia, MediaPostprocess, SourceError, SourceErrorKind, UgoiraFrame};

const MAX_FRAMES: usize = 100_000;
const MAX_FRAME_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub(crate) async fn postprocess(
    downloaded: DownloadedMedia,
    cancel: &CancellationToken,
) -> Result<DownloadedMedia, SourceError> {
    let Some(MediaPostprocess::UgoiraToWebm { frames }) =
        downloaded.descriptor.postprocess.as_ref()
    else {
        return Ok(downloaded);
    };
    let input = downloaded.path.clone();
    let frames = frames.clone();
    let prepared = tokio::task::spawn_blocking(move || prepare(&input, &frames))
        .await
        .map_err(|error| conversion_error(format!("Ugoira preparation failed: {error}"), true))??;

    let ffmpeg = discover_ffmpeg();
    let mut command = Command::new(ffmpeg);
    command
        .kill_on_drop(true)
        .current_dir(prepared.work.path())
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
        ]);
    if let Some(rate) = prepared.input_rate.as_deref() {
        command.args(["-r", rate]);
    }
    command.args(["-safe", "1", "-i", "ffconcat.txt"]);
    if let Some(rate) = prepared.output_rate.as_deref() {
        command.args(["-r", rate]);
    }
    command.args([
        "-c:v",
        "libvpx-vp9",
        "-crf",
        "12",
        "-b:v",
        "0",
        // JPEG frames carry full-range pixels. Convert, rather than merely
        // relabel, so VP9 remains playable by Windows hardware decoders.
        "-vf",
        "scale=in_range=auto:out_range=tv",
        "-color_range",
        "tv",
        "-pix_fmt",
        "yuv420p",
        "-an",
        "-y",
    ]);
    command.arg(&prepared.output);
    let output = tokio::select! {
        _ = cancel.cancelled() => return Err(SourceError::new(
            SourceErrorKind::Cancelled,
            "Ugoira conversion cancelled",
            true,
        )),
        output = command.output() => output.map_err(|error| {
            conversion_error(format!("failed to run FFmpeg for Ugoira: {error}"), true)
        })?,
    };
    if !output.status.success() {
        return Err(conversion_error(
            format!(
                "FFmpeg Ugoira conversion failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            false,
        ));
    }
    let size_bytes = tokio::fs::metadata(&prepared.output)
        .await
        .map_err(|error| conversion_error(error.to_string(), true))?
        .len();
    if size_bytes == 0 {
        return Err(conversion_error(
            "FFmpeg produced an empty Ugoira video",
            false,
        ));
    }

    replace_source(&downloaded.path, &prepared.output).await?;
    Ok(DownloadedMedia {
        descriptor: downloaded.descriptor,
        path: downloaded.path,
        size_bytes,
    })
}

struct PreparedConversion {
    work: tempfile::TempDir,
    output: PathBuf,
    input_rate: Option<String>,
    output_rate: Option<String>,
}

fn prepare(input: &Path, frames: &[UgoiraFrame]) -> Result<PreparedConversion, SourceError> {
    validate_frames(frames)?;
    let work = tempfile::tempdir().map_err(|error| conversion_error(error.to_string(), true))?;
    let archive = File::open(input).map_err(|error| conversion_error(error.to_string(), true))?;
    let mut archive = zip::ZipArchive::new(archive)
        .map_err(|error| conversion_error(format!("invalid Ugoira ZIP: {error}"), false))?;
    let mut concat = String::from("ffconcat version 1.0\n");
    let mut extracted_bytes = 0_u64;

    for (index, frame) in frames.iter().enumerate() {
        let extension = safe_frame_extension(&frame.file)?;
        let local_name = format!("frame-{index:06}.{extension}");
        let path = work.path().join(&local_name);
        let mut source = archive.by_name(&frame.file).map_err(|error| {
            conversion_error(
                format!("Ugoira frame {} is missing: {error}", frame.file),
                false,
            )
        })?;
        if source.size() > MAX_FRAME_BYTES {
            return Err(conversion_error("Ugoira frame exceeds 256 MiB", false));
        }
        extracted_bytes = extracted_bytes.saturating_add(source.size());
        if extracted_bytes > MAX_EXTRACTED_BYTES {
            return Err(conversion_error(
                "Ugoira expands beyond the 8 GiB safety limit",
                false,
            ));
        }
        let mut destination =
            File::create(path).map_err(|error| conversion_error(error.to_string(), true))?;
        io::copy(&mut source, &mut destination)
            .map_err(|error| conversion_error(error.to_string(), true))?;
        writeln!(concat, "file '{local_name}'")
            .and_then(|_| {
                writeln!(
                    concat,
                    "duration {}.{:03}",
                    frame.delay / 1000,
                    frame.delay % 1000
                )
            })
            .map_err(|error| conversion_error(error.to_string(), true))?;
    }
    let last_extension = safe_frame_extension(&frames.last().expect("validated nonempty").file)?;
    writeln!(
        concat,
        "file 'frame-{:06}.{last_extension}'",
        frames.len() - 1
    )
    .map_err(|error| conversion_error(error.to_string(), true))?;
    std::fs::write(work.path().join("ffconcat.txt"), concat)
        .map_err(|error| conversion_error(error.to_string(), true))?;

    let (input_rate, output_rate) = frame_rates(frames);
    let output = work.path().join("converted.webm");
    Ok(PreparedConversion {
        work,
        output,
        input_rate,
        output_rate,
    })
}

fn validate_frames(frames: &[UgoiraFrame]) -> Result<(), SourceError> {
    if frames.is_empty() || frames.len() > MAX_FRAMES {
        return Err(conversion_error(
            "Ugoira metadata has an invalid frame count",
            false,
        ));
    }
    if frames.iter().any(|frame| frame.delay == 0) {
        return Err(conversion_error(
            "Ugoira metadata contains a zero frame delay",
            false,
        ));
    }
    Ok(())
}

fn safe_frame_extension(file: &str) -> Result<String, SourceError> {
    let extension = Path::new(file)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "jpg" | "jpeg" | "png" | "webp"))
        .ok_or_else(|| conversion_error("Ugoira frame has an unsupported image type", false))?;
    Ok(extension)
}

fn frame_rates(frames: &[UgoiraFrame]) -> (Option<String>, Option<String>) {
    if frames.iter().all(|frame| frame.delay == frames[0].delay) {
        return (Some(format!("1000/{}", frames[0].delay)), None);
    }
    let gcd = frames.iter().skip(1).fold(frames[0].delay, |value, frame| {
        greatest_common_divisor(value, frame.delay)
    });
    (None, (gcd >= 10).then(|| format!("1000/{gcd}")))
}

fn greatest_common_divisor(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn discover_ffmpeg() -> PathBuf {
    let binary = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Ok(directory) = std::env::var("PICTO_FFMPEG_DIR") {
        let candidate = PathBuf::from(directory).join(binary);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(mut directory) = std::env::current_dir() {
        loop {
            let candidate = directory.join("vendor").join("ffmpeg").join(binary);
            if candidate.is_file() {
                return candidate;
            }
            if !directory.pop() {
                break;
            }
        }
    }
    PathBuf::from(binary)
}

async fn replace_source(source: &Path, converted: &Path) -> Result<(), SourceError> {
    let backup = source.with_extension("picto-ugoira-source");
    let _ = tokio::fs::remove_file(&backup).await;
    tokio::fs::rename(source, &backup)
        .await
        .map_err(|error| conversion_error(error.to_string(), true))?;
    if let Err(error) = tokio::fs::rename(converted, source).await {
        let _ = tokio::fs::rename(&backup, source).await;
        return Err(conversion_error(error.to_string(), true));
    }
    let _ = tokio::fs::remove_file(backup).await;
    Ok(())
}

fn conversion_error(message: impl Into<String>, retryable: bool) -> SourceError {
    SourceError::new(SourceErrorKind::Download, message, retryable)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn frame(file: &str, delay: u32) -> UgoiraFrame {
        UgoiraFrame {
            file: file.into(),
            delay,
        }
    }

    #[test]
    fn uniform_delays_use_an_exact_input_rate() {
        assert_eq!(
            frame_rates(&[frame("0.jpg", 60), frame("1.jpg", 60)]),
            (Some("1000/60".into()), None)
        );
    }

    #[test]
    fn variable_delays_use_their_common_output_rate() {
        assert_eq!(
            frame_rates(&[frame("0.jpg", 60), frame("1.jpg", 100)]),
            (None, Some("1000/20".into()))
        );
    }

    #[test]
    fn very_irregular_delays_preserve_concat_timing_without_quantizing() {
        assert_eq!(
            frame_rates(&[frame("0.jpg", 17), frame("1.jpg", 19)]),
            (None, None)
        );
    }

    #[tokio::test]
    async fn converts_a_timed_pixiv_archive_to_vp9_webm() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("animation.webm");
        let file = File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (index, color) in [[64, 64, 64], [192, 192, 192]].into_iter().enumerate() {
            let image = image::RgbImage::from_pixel(600, 600, image::Rgb(color));
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image)
                .write_to(&mut bytes, image::ImageFormat::Jpeg)
                .unwrap();
            archive
                .start_file(format!("{index:06}.jpg"), options)
                .unwrap();
            io::copy(&mut Cursor::new(bytes.into_inner()), &mut archive).unwrap();
        }
        archive.finish().unwrap();
        let frames = vec![frame("000000.jpg", 60), frame("000001.jpg", 70)];
        let mut descriptor = crate::MediaDescriptorBuilder::new(
            "pixiv:1:ugoira",
            0,
            "https://i.pximg.net/animation.zip",
        )
        .file_name("1.webm")
        .build();
        descriptor.mime_hint = Some("video/webm".into());
        descriptor.postprocess = Some(MediaPostprocess::UgoiraToWebm { frames });
        let input_size = std::fs::metadata(&path).unwrap().len();

        let converted = postprocess(
            DownloadedMedia {
                descriptor,
                path: path.clone(),
                size_bytes: input_size,
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], &[0x1a, 0x45, 0xdf, 0xa3]);
        assert_eq!(converted.size_bytes, bytes.len() as u64);
        assert_eq!(
            converted.descriptor.mime_hint.as_deref(),
            Some("video/webm")
        );
        let ffmpeg = discover_ffmpeg();
        let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        let probe = Command::new(ffprobe)
            .args(["-v", "error", "-show_streams", "-of", "json"])
            .arg(&path)
            .output()
            .await
            .unwrap();
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let video = &metadata["streams"][0];
        assert_eq!(video["codec_name"], "vp9");
        assert_eq!(video["pix_fmt"], "yuv420p");
        assert_eq!(video["color_range"], "tv");
        assert_eq!(video["width"], 600);
        let decoded = Command::new(ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&path)
            .args([
                "-frames:v",
                "1",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert_eq!(decoded.stdout.len(), 600 * 600 * 3);
        // A metadata-only range change would shift this mid-gray's brightness.
        assert!(decoded.stdout.iter().all(|value| (61..=67).contains(value)));
    }
}
