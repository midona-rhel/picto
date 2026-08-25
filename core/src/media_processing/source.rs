use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::constants::MimeType;
use crate::media_capabilities::{
    capabilities_for_detected_mime, capabilities_for_stored_media, MediaCapabilities,
    ThumbnailBackend,
};

use super::analysis::get_file_info;
use super::detection::{get_mime, is_allowed_mime};
use super::thumbnail::{
    generate_jpeg_thumbnail, generate_thumbnail_bytes, generate_thumbnail_from_decoded_image,
};
use super::{FileError, FileResult};

#[derive(Debug)]
pub struct PreparedMediaSource {
    pub path: PathBuf,
    pub mime_type: String,
    pub caps: MediaCapabilities,
    pub size_bytes: Option<u64>,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub num_frames: Option<u32>,
    pub has_audio: bool,
    detected_mime: Option<MimeType>,
    decoded_raster: Option<DynamicImage>,
}

impl PreparedMediaSource {
    fn accepted_without_probe(
        path: &Path,
        mime_type: &str,
        size_bytes: u64,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            mime_type: mime_type.to_string(),
            caps: capabilities_for_stored_media(mime_type, None),
            size_bytes: Some(size_bytes),
            pixel_width: None,
            pixel_height: None,
            duration_ms: None,
            num_frames: None,
            has_audio: mime_type.starts_with("audio/"),
            detected_mime: None,
            decoded_raster: None,
        }
    }

    pub async fn prepare_ingest(path: &Path) -> FileResult<Self> {
        let size_bytes = tokio::fs::metadata(path).await?.len();
        if size_bytes == 0 {
            return Err(FileError::ZeroSizeFile(path.display().to_string()));
        }

        let accepted_format = super::formats::format_for_path(path);
        let media_extension = accepted_format.is_some_and(|format| {
            format.mime_type.starts_with("audio/") || format.mime_type.starts_with("video/")
        });
        let (detected_mime, info) = if media_extension {
            match get_file_info(path, None).await {
                Ok(info) => (info.mime, info),
                Err(_) => {
                    let format = accepted_format.expect("media extension came from accepted format");
                    return Ok(Self::accepted_without_probe(
                        path,
                        format.mime_type,
                        size_bytes,
                    ));
                }
            }
        } else {
            let detected_mime = get_mime(path).await?;
            let info = if is_allowed_mime(detected_mime) {
                Some(get_file_info(path, Some(detected_mime)).await?)
            } else {
                None
            };
            if let Some(info) = info {
                (detected_mime, info)
            } else {
                let format = accepted_format.ok_or_else(|| {
                    FileError::UnsupportedFile(format!(
                        "The .{} file format is not accepted",
                        path.extension()
                            .and_then(|extension| extension.to_str())
                            .unwrap_or_default()
                    ))
                })?;
                return Ok(Self::accepted_without_probe(path, format.mime_type, size_bytes));
            }
        };
        if !is_allowed_mime(detected_mime) {
            return Err(FileError::UnsupportedFile(format!(
                "Unsupported media: {}",
                path.display()
            )));
        }
        if info.mime.is_image() && matches!(super::is_decompression_bomb(path), Ok(true)) {
            return Err(FileError::UnsupportedFile("Decompression bomb".to_string()));
        }

        Ok(Self {
            path: path.to_path_buf(),
            mime_type: info.mime.mime_string().to_string(),
            caps: capabilities_for_detected_mime(info.mime),
            size_bytes: Some(size_bytes),
            pixel_width: info.width,
            pixel_height: info.height,
            duration_ms: info.duration_ms,
            num_frames: info.num_frames,
            has_audio: info.has_audio,
            detected_mime: Some(info.mime),
            decoded_raster: None,
        })
    }

    pub fn from_stored_metadata(
        path: PathBuf,
        mime_type: &str,
        duration_ms: Option<i64>,
        num_frames: Option<i64>,
    ) -> Self {
        Self {
            path,
            mime_type: mime_type.to_string(),
            caps: capabilities_for_stored_media(mime_type, num_frames),
            size_bytes: None,
            pixel_width: None,
            pixel_height: None,
            duration_ms: duration_ms.map(|value| value as u64),
            num_frames: num_frames.map(|value| value as u32),
            has_audio: mime_type.starts_with("audio/"),
            detected_mime: None,
            decoded_raster: None,
        }
    }

    pub fn require_decoded_raster(&mut self) -> FileResult<&DynamicImage> {
        if self.decoded_raster.is_none() {
            let decoded = if self.mime_type == "image/jxl" {
                super::jxl::decode(&self.path)?
            } else {
                image::ImageReader::open(&self.path)?
                    .with_guessed_format()
                    .map_err(FileError::Io)?
                    .decode()?
            };
            self.decoded_raster = Some(decoded);
        }
        Ok(self.decoded_raster.as_ref().expect("decoded raster loaded"))
    }

    /// Render formats handled by the in-process raster decoder without
    /// entering an async adapter. Ingestion uses this as its minimum display
    /// readiness gate before publishing a new visible item.
    pub fn render_inline_thumbnail_bytes(
        &mut self,
        target_resolution: (u32, u32),
    ) -> FileResult<(Vec<u8>, String)> {
        if self.caps.thumbnail_backend != Some(ThumbnailBackend::Inline) {
            return Err(FileError::Thumbnail(format!(
                "No inline thumbnail backend for {}",
                self.mime_type
            )));
        }
        if self.mime_type == "image/jpeg" {
            return generate_jpeg_thumbnail(&self.path, target_resolution)
                .map(|(bytes, extension)| (bytes, extension.to_string()));
        }
        let decoded = self.require_decoded_raster()?;
        generate_thumbnail_from_decoded_image(decoded, target_resolution)
            .map(|(bytes, extension)| (bytes, extension.to_string()))
    }

    async fn require_detected_mime(&mut self) -> FileResult<MimeType> {
        if let Some(mime) = self.detected_mime {
            return Ok(mime);
        }
        let mime = get_mime(&self.path).await?;
        self.detected_mime = Some(mime);
        Ok(mime)
    }

    pub async fn render_thumbnail_bytes(
        &mut self,
        target_resolution: (u32, u32),
        percentage_in: u32,
    ) -> FileResult<(Vec<u8>, String)> {
        if !self.caps.can_thumbnail() {
            return Err(FileError::Thumbnail(format!(
                "No thumbnail backend for {}",
                self.mime_type
            )));
        }

        if self.caps.thumbnail_backend == Some(ThumbnailBackend::Inline) {
            if self.mime_type == "image/jpeg" {
                return generate_jpeg_thumbnail(&self.path, target_resolution)
                    .map(|(bytes, extension)| (bytes, extension.to_string()));
            }
            let decoded = self.require_decoded_raster()?;
            return generate_thumbnail_from_decoded_image(decoded, target_resolution)
                .map(|(bytes, ext)| (bytes, ext.to_string()));
        }

        let mime = self.require_detected_mime().await?;
        generate_thumbnail_bytes(
            &self.path,
            target_resolution,
            mime,
            self.duration_ms,
            self.num_frames,
            percentage_in,
        )
        .await
    }
}
