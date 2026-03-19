//! File processing — MIME detection, file info extraction (size, dimensions,
//! duration, frames, audio, word count), thumbnail generation, and hash computation.

mod adapters;
mod analysis;
mod detection;
mod hashing;
mod thumbnail;

pub mod archive;
pub mod colors;
pub mod ffmpeg;
pub mod ffmpeg_path;
pub mod gallery_dl_path;
pub mod office;
pub mod pdf;
pub mod specialty;
pub mod svg;

use std::path::Path;

pub use analysis::get_file_info;
pub use detection::{get_mime, has_supported_extension, is_allowed_mime, is_image};
pub use hashing::get_hash_from_bytes;
pub use thumbnail::{
    encode_thumbnail, encode_thumbnail_jpeg, generate_thumbnail_bytes, get_thumbnail_resolution,
    ThumbnailScaleType,
};

use crate::constants::MimeType;

#[derive(thiserror::Error, Debug)]
pub enum FileError {
    #[error("Zero-size file: {0}")]
    ZeroSizeFile(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFile(String),
    #[error("Damaged or unusual file: {0}")]
    DamagedOrUnusualFile(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("Hash error: {0}")]
    Hash(String),
    #[error("Thumbnail error: {0}")]
    Thumbnail(String),
}

pub type FileResult<T> = Result<T, FileError>;

pub const DEFAULT_THUMBNAIL_DIMENSIONS: (u32, u32) = (512, 512);

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub mime: MimeType,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub num_frames: Option<u32>,
    pub has_audio: bool,
}

pub fn is_decompression_bomb(path: &Path) -> FileResult<bool> {
    const MAX_IMAGE_PIXELS: u64 = (512 * 1024 * 1024) / 3;

    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(FileError::Io)?;

    match reader.into_dimensions() {
        Ok((w, h)) => Ok(w as u64 * h as u64 > MAX_IMAGE_PIXELS),
        Err(_) => Ok(false),
    }
}
