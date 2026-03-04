//! Blurhash encoding and color sorting utilities.
//!
//! Core blurhash encode/decode uses the `blurhash` crate.
//! Color sorting functions (HSL, CIELAB) are kept for Hydrus-compatible sort keys.
//!
//! References:
//! - `hydrus/core/files/images/HydrusBlurhash.py` — Hydrus blurhash helpers

use std::path::Path;

use super::FileError;

// ── Blurhash encode/decode (via crate) ──────────────────────────────

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

/// Generate a blurhash string from an image file path.
pub fn get_blurhash_from_path(path: &Path) -> Result<String, FileError> {
    let data = std::fs::read(path)?;
    get_blurhash_from_thumbnail_bytes(&data)
}

/// Decode a blurhash to an `image::RgbaImage`.
///
/// For performance, decodes at 32x32 max and scales up (matching Python behavior).
pub fn get_image_from_blurhash(
    hash: &str,
    width: u32,
    height: u32,
) -> Result<image::RgbaImage, FileError> {
    let decode_w = width.min(32);
    let decode_h = height.min(32);

    let pixels = blurhash::decode(hash, decode_w, decode_h, 1.0)
        .map_err(|e| FileError::Hash(format!("Blurhash decode failed: {:?}", e)))?;

    let small = image::RgbaImage::from_raw(decode_w, decode_h, pixels)
        .ok_or_else(|| FileError::Hash("Failed to create image from blurhash decode".into()))?;

    if decode_w < width || decode_h < height {
        Ok(image::imageops::resize(
            &small,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ))
    } else {
        Ok(small)
    }
}

// ── Average color extraction ────────────────────────────────────────

/// Extract the average RGB color from a blurhash without full decoding.
///
/// The DC component (chars 2-6) encodes the average sRGB color.
pub fn get_average_colour_from_blurhash(blurhash: &str) -> Result<(u8, u8, u8), FileError> {
    if blurhash.len() < 6 {
        return Err(FileError::Hash(
            "Blurhash must be at least 6 characters long.".into(),
        ));
    }
    let dc = base83_decode(&blurhash[2..6])?;
    let r = ((dc >> 16) & 0xFF) as u8;
    let g = ((dc >> 8) & 0xFF) as u8;
    let b = (dc & 0xFF) as u8;
    Ok((r, g, b))
}

// Minimal base83 decode needed for average colour extraction
const ALPHABET: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%*+,-.:;=?@[]^_{|}~";

fn base83_decode(s: &str) -> Result<u64, FileError> {
    let mut value: u64 = 0;
    for ch in s.bytes() {
        let idx = ALPHABET
            .iter()
            .position(|&c| c == ch)
            .ok_or_else(|| FileError::Hash(format!("Invalid base83 character: {}", ch as char)))?;
        value = value * 83 + idx as u64;
    }
    Ok(value)
}

// ── Color space conversions (for sorting) ───────────────────────────

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;

    let max_c = rf.max(gf).max(bf);
    let min_c = rf.min(gf).min(bf);
    let delta = max_c - min_c;

    let l = (max_c + min_c) / 2.0;

    let s = if delta == 0.0 {
        0.0
    } else if l > 0.5 {
        delta / (2.0 - max_c - min_c)
    } else {
        delta / (max_c + min_c)
    };

    let h = if delta == 0.0 {
        0.0
    } else if max_c == rf {
        ((gf - bf) / delta) % 6.0
    } else if max_c == gf {
        (bf - rf) / delta + 2.0
    } else {
        (rf - gf) / delta + 4.0
    };

    (h * 60.0, s, l)
}

fn rgb_to_xyz(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let gamma = |v: f64| -> f64 {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let rf = gamma(r as f64 / 255.0);
    let gf = gamma(g as f64 / 255.0);
    let bf = gamma(b as f64 / 255.0);

    (
        rf * 0.4124564 + gf * 0.3575761 + bf * 0.1804375,
        rf * 0.2126729 + gf * 0.7151522 + bf * 0.0721750,
        rf * 0.0193339 + gf * 0.1191920 + bf * 0.9503041,
    )
}

fn xyz_to_lab(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let f = |t: f64| -> f64 {
        if t > 0.008856 {
            t.powf(1.0 / 3.0)
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let fx = f(x / 0.95047);
    let fy = f(y / 1.00000);
    let fz = f(z / 1.08883);
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

// ── Sorting functions ───────────────────────────────────────────────

pub fn blurhash_to_sortable_chromatic_magnitude(
    blurhash: &str,
    _reverse: bool,
) -> Result<(f64, f64), FileError> {
    let (r, g, b) = get_average_colour_from_blurhash(blurhash)?;
    let (l, a, bv) = xyz_to_lab(
        rgb_to_xyz(r, g, b).0,
        rgb_to_xyz(r, g, b).1,
        rgb_to_xyz(r, g, b).2,
    );
    Ok((a * a + bv * bv, l))
}

pub fn blurhash_to_sortable_blue_yellow(
    blurhash: &str,
    _reverse: bool,
) -> Result<(f64, f64), FileError> {
    let (r, g, b) = get_average_colour_from_blurhash(blurhash)?;
    let (x, y, z) = rgb_to_xyz(r, g, b);
    let (l, _a, bv) = xyz_to_lab(x, y, z);
    Ok((bv, -l))
}

pub fn blurhash_to_sortable_green_red(
    blurhash: &str,
    _reverse: bool,
) -> Result<(f64, f64), FileError> {
    let (r, g, b) = get_average_colour_from_blurhash(blurhash)?;
    let (x, y, z) = rgb_to_xyz(r, g, b);
    let (l, a, _bv) = xyz_to_lab(x, y, z);
    Ok((a, -l))
}

pub fn blurhash_to_sortable_hue(
    blurhash: &str,
    reverse: bool,
) -> Result<(i32, f64, f64), FileError> {
    let (r, g, b) = get_average_colour_from_blurhash(blurhash)?;
    let (h, s, _l) = rgb_to_hsl(r, g, b);
    let initial = if s < 0.03 {
        if reverse {
            -1
        } else {
            1
        }
    } else {
        0
    };
    Ok((initial, h, -s))
}

pub fn blurhash_to_sortable_lightness(
    blurhash: &str,
    _reverse: bool,
) -> Result<(f64, f64), FileError> {
    let (r, g, b) = get_average_colour_from_blurhash(blurhash)?;
    let (x, y, z) = rgb_to_xyz(r, g, b);
    let (l, a, bv) = xyz_to_lab(x, y, z);
    Ok((l, a * a + bv * bv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_average_colour_known_blurhash() {
        let hash = "LEHV6nWB2yk8pyo0adR*.7kCMdnj";
        let (_r, _g, _b) = get_average_colour_from_blurhash(hash).unwrap();
        // Values are u8, so always <= 255. Just verify decoding succeeded.
    }

    #[test]
    fn test_sorting_functions() {
        let hash = "LEHV6nWB2yk8pyo0adR*.7kCMdnj";
        let (cm, l) = blurhash_to_sortable_chromatic_magnitude(hash, false).unwrap();
        assert!(cm >= 0.0);
        assert!(l >= -16.0 && l <= 100.0);

        let (initial, h, ns) = blurhash_to_sortable_hue(hash, false).unwrap();
        assert!(initial == 0 || initial == 1);
        assert!(h.is_finite());
        assert!(ns <= 0.0);
    }
}
