//! JPEG XL decoding shared by ingest metadata, thumbnails, color analysis, and pHash.

use std::fs::File;
use std::path::Path;

use image::DynamicImage;
use jxl_oxide::integration::JxlDecoder;

use super::{FileError, FileResult};

pub fn decode(path: &Path) -> FileResult<DynamicImage> {
    let file = File::open(path).map_err(FileError::Io)?;
    let decoder = JxlDecoder::new(file).map_err(|error| {
        FileError::DamagedOrUnusualFile(format!("Could not decode JPEG XL: {error}"))
    })?;
    DynamicImage::from_decoder(decoder).map_err(FileError::Image)
}

pub fn dimensions(path: &Path) -> FileResult<(u32, u32)> {
    let image = decode(path)?;
    Ok((image.width(), image.height()))
}
