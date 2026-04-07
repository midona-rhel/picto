use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::constants::MimeType;
use crate::media_capabilities::{capabilities_for_detected_mime, capabilities_for_stored_media, MediaCapabilities};

use super::analysis::get_file_info;
use super::detection::get_mime;
use super::phash::compute_phash_base64_from_image;
use super::thumbnail::{generate_thumbnail_bytes, generate_thumbnail_from_decoded_image};
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
    pub file_bytes: Option<Vec<u8>>,
    detected_mime: Option<MimeType>,
    decoded_raster: Option<DynamicImage>,
}

impl PreparedMediaSource {
    pub async fn prepare_ingest(path: &Path) -> FileResult<Self> {
        let file_bytes = tokio::fs::read(path).await?;
        if file_bytes.is_empty() {
            return Err(FileError::ZeroSizeFile(path.display().to_string()));
        }

        let detected_mime = get_mime(path).await?;
        let info = get_file_info(path, Some(detected_mime)).await?;
        if info.mime.is_image() && matches!(super::is_decompression_bomb(path), Ok(true)) {
            return Err(FileError::UnsupportedFile("Decompression bomb".to_string()));
        }

        Ok(Self {
            path: path.to_path_buf(),
            mime_type: info.mime.mime_string().to_string(),
            caps: capabilities_for_detected_mime(info.mime),
            size_bytes: Some(file_bytes.len() as u64),
            pixel_width: info.width,
            pixel_height: info.height,
            duration_ms: info.duration_ms,
            num_frames: info.num_frames,
            has_audio: info.has_audio,
            file_bytes: Some(file_bytes),
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
            file_bytes: None,
            detected_mime: None,
            decoded_raster: None,
        }
    }

    pub fn require_file_bytes(&mut self) -> FileResult<&[u8]> {
        if self.file_bytes.is_none() {
            self.file_bytes = Some(std::fs::read(&self.path)?);
        }
        Ok(self.file_bytes.as_deref().expect("file bytes loaded"))
    }

    pub fn require_decoded_raster(&mut self) -> FileResult<&DynamicImage> {
        if self.decoded_raster.is_none() {
            let bytes = self.require_file_bytes()?;
            let decoded = image::load_from_memory(bytes)?;
            self.decoded_raster = Some(decoded);
        }
        Ok(self.decoded_raster.as_ref().expect("decoded raster loaded"))
    }

    pub fn compute_phash_base64(&mut self) -> FileResult<Option<String>> {
        if !self.caps.can_perceptual_hash {
            return Ok(None);
        }
        let decoded = self.require_decoded_raster()?;
        compute_phash_base64_from_image(decoded)
            .map(Some)
            .map_err(FileError::Image)
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

        if self.caps.can_dominant_colors || self.caps.can_perceptual_hash {
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
