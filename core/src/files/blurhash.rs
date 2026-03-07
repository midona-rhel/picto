//! Blurhash encoding utilities.
//!
//! Core blurhash encode uses the `blurhash` crate.

use super::FileError;

// ── Blurhash encode (via crate) ──────────────────────────────────

/// Generate a blurhash string from thumbnail bytes.
///
/// Decodes the image, downscales to 100x100 max, then encodes via the blurhash crate.
pub fn get_blurhash_from_thumbnail_bytes(thumbnail_bytes: &[u8]) -> Result<String, FileError> {
    let img = image::load_from_memory(thumbnail_bytes)
        .map_err(|e| FileError::Hash(format!("Failed to decode thumbnail for blurhash: {}", e)))?;

    let (width, height) = (img.width(), img.height());
    if width == 0 || height == 0 {
        return Ok(String::new());
    }

    // Choose component counts based on aspect ratio — matches Python exactly
    let ratio = width as f64 / height as f64;
    let (components_x, components_y) = if ratio > 4.0 / 3.0 {
        (5, 3)
    } else if ratio < 3.0 / 4.0 {
        (3, 5)
    } else {
        (4, 4)
    };

    // Downscale to 100x100 max — matches Python's CUTOFF_DIMENSION = 100
    let cutoff = 100;
    let rgba = if width > cutoff || height > cutoff {
        let resized = image::imageops::resize(
            &img.to_rgba8(),
            cutoff,
            cutoff,
            image::imageops::FilterType::Triangle,
        );
        resized
    } else {
        img.to_rgba8()
    };

    blurhash::encode(
        components_x,
        components_y,
        rgba.width(),
        rgba.height(),
        rgba.as_raw(),
    )
    .map_err(|e| FileError::Hash(format!("Blurhash encode failed: {:?}", e)))
}
