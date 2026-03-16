use std::io::BufReader;
use std::path::Path;

use image::GenericImageView;

use crate::constants::MimeType;

use super::adapters::populate_file_info;
use super::detection::{get_mime, is_allowed_mime};
use super::{FileError, FileInfo, FileResult};

fn definitely_has_audio(mime: MimeType) -> bool {
    mime.is_audio() || mime == MimeType::ApplicationFlash
}

pub fn get_file_info(path: &Path, mime: Option<MimeType>) -> FileResult<FileInfo> {
    let file_size = std::fs::metadata(path)
        .map_err(|e| FileError::NotFound(format!("{}: {}", path.display(), e)))?
        .len();

    if file_size == 0 {
        return Err(FileError::ZeroSizeFile(path.display().to_string()));
    }

    let mime = match mime {
        Some(m) => m,
        None => get_mime(path)?,
    };

    if !is_allowed_mime(mime) {
        return Err(match mime {
            MimeType::TextHtml => FileError::UnsupportedFile("Looks like HTML".to_string()),
            MimeType::ApplicationJson => FileError::UnsupportedFile("Looks like JSON".to_string()),
            MimeType::ApplicationUnknown => FileError::UnsupportedFile("Unknown filetype!".to_string()),
            _ => FileError::UnsupportedFile("Filetype is not permitted!".to_string()),
        });
    }

    let mut info = FileInfo {
        mime,
        width: None,
        height: None,
        duration_ms: None,
        num_frames: None,
        has_audio: definitely_has_audio(mime),
    };

    populate_file_info(path, &mut info);
    Ok(info)
}

pub(crate) fn get_image_dimensions(path: &Path) -> FileResult<(u32, u32)> {
    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let (w, h) = reader.into_dimensions()?;
    Ok((w, h))
}

pub(crate) fn get_animation_properties(
    path: &Path,
    mime: MimeType,
) -> FileResult<(u32, u32, u64, u32)> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    use image::AnimationDecoder;

    match mime {
        MimeType::AnimationGif => {
            let decoder = image::codecs::gif::GifDecoder::new(reader).map_err(FileError::Image)?;
            let (w, h) = image::ImageDecoder::dimensions(&decoder);
            let mut frame_count = 0u32;
            let mut total_delay_ms = 0u64;
            for f in decoder.into_frames().flatten() {
                frame_count += 1;
                let (num, den) = f.delay().numer_denom_ms();
                total_delay_ms += num as u64 / den.max(1) as u64;
            }
            Ok((w, h, total_delay_ms, frame_count))
        }
        MimeType::AnimationWebp => {
            let decoder = image::codecs::webp::WebPDecoder::new(reader).map_err(FileError::Image)?;
            let (w, h) = image::ImageDecoder::dimensions(&decoder);
            let mut frame_count = 0u32;
            let mut total_delay_ms = 0u64;
            for f in decoder.into_frames().flatten() {
                frame_count += 1;
                let (num, den) = f.delay().numer_denom_ms();
                total_delay_ms += num as u64 / den.max(1) as u64;
            }
            Ok((w, h, total_delay_ms, frame_count))
        }
        MimeType::AnimationApng => {
            let decoder = image::codecs::png::PngDecoder::new(reader).map_err(FileError::Image)?;
            if decoder.is_apng().unwrap_or(false) {
                let (w, h) = image::ImageDecoder::dimensions(&decoder);
                let apng = decoder.apng().map_err(FileError::Image)?;
                let mut frame_count = 0u32;
                let mut total_delay_ms = 0u64;
                for f in apng.into_frames().flatten() {
                    frame_count += 1;
                    let (num, den) = f.delay().numer_denom_ms();
                    total_delay_ms += num as u64 / den.max(1) as u64;
                }
                Ok((w, h, total_delay_ms, frame_count))
            } else {
                let (w, h) = image::ImageDecoder::dimensions(&decoder);
                Ok((w, h, 0, 1))
            }
        }
        _ => {
            let img = image::open(path)?;
            let (w, h) = img.dimensions();
            Ok((w, h, 0, 1))
        }
    }
}
