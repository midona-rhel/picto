//! Image metadata utilities — JPEG quality estimation, subsampling detection,
//! and ICC profile construction.
//!
//! Ported from:
//! - `hydrus/core/files/images/HydrusImageMetadata.py` — JPEG quality estimation & subsampling
//! - `hydrus/core/files/images/HydrusImageICCProfiles.py` — ICC profile construction

use std::path::Path;

use super::FileError;

// ── JPEG Subsampling ────────────────────────────────────────────────

/// JPEG chroma subsampling types.
/// Python: `HydrusImageMetadata.SUBSAMPLING_*`
///
/// The first three values (0, 1, 2) match PIL's `JpegImagePlugin.get_sampling()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum JpegSubsampling {
    /// 4:4:4 — no chroma subsampling
    Subsampling444 = 0,
    /// 4:2:2 — horizontal subsampling
    Subsampling422 = 1,
    /// 4:2:0 — horizontal and vertical subsampling
    Subsampling420 = 2,
    /// Unknown subsampling
    Unknown = 3,
    /// Greyscale — no chroma channels
    Greyscale = 4,
}

impl JpegSubsampling {
    /// Relative quality factor for adjusting quantization scores.
    /// Python: `subsampling_quality_lookup`
    pub fn quality_factor(self) -> f64 {
        match self {
            Self::Subsampling444 => 1.00,
            Self::Subsampling422 => 0.93,
            Self::Subsampling420 => 0.83,
            Self::Unknown => 0.75,
            Self::Greyscale => 0.967,
        }
    }

    /// Human-readable string representation.
    /// Python: `subsampling_str_lookup`
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subsampling444 => "4:4:4",
            Self::Subsampling422 => "4:2:2",
            Self::Subsampling420 => "4:2:0",
            Self::Unknown => "unknown",
            Self::Greyscale => "greyscale (no subsampling)",
        }
    }
}

// ── JPEG marker parsing ─────────────────────────────────────────────

/// Parse JPEG markers to extract quantization tables and component info.
///
/// Returns `(quantization_tables, subsampling)`.
/// Each quantization table is a Vec of 64 coefficient values.
fn parse_jpeg_markers(data: &[u8]) -> Result<(Vec<Vec<u32>>, JpegSubsampling), FileError> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(FileError::DamagedOrUnusualFile(
            "Not a JPEG file".to_string(),
        ));
    }

    let mut tables: Vec<Vec<u32>> = Vec::new();
    let mut subsampling = JpegSubsampling::Unknown;
    let mut pos = 2;

    while pos < data.len() - 1 {
        // Find next marker
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }

        // Skip fill bytes
        while pos < data.len() - 1 && data[pos + 1] == 0xFF {
            pos += 1;
        }

        if pos >= data.len() - 1 {
            break;
        }

        let marker = data[pos + 1];
        pos += 2;

        // Skip markers without length
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            continue;
        }

        // Read marker length
        if pos + 2 > data.len() {
            break;
        }
        let length = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
        if length < 2 || pos + length > data.len() {
            break;
        }

        match marker {
            // DQT — Define Quantization Table
            0xDB => {
                let mut tpos = pos + 2; // skip length field
                let end = pos + length;
                while tpos < end {
                    if tpos >= data.len() {
                        break;
                    }
                    let precision_and_id = data[tpos];
                    let precision = (precision_and_id >> 4) & 0x0F; // 0 = 8-bit, 1 = 16-bit
                    tpos += 1;

                    let mut table = Vec::with_capacity(64);
                    for _ in 0..64 {
                        if precision == 0 {
                            if tpos >= data.len() {
                                break;
                            }
                            table.push(data[tpos] as u32);
                            tpos += 1;
                        } else {
                            if tpos + 1 >= data.len() {
                                break;
                            }
                            table.push(((data[tpos] as u32) << 8) | data[tpos + 1] as u32);
                            tpos += 2;
                        }
                    }
                    if table.len() == 64 {
                        tables.push(table);
                    }
                }
            }
            // SOF0, SOF1, SOF2 — Start of Frame
            0xC0 | 0xC1 | 0xC2 => {
                // Parse component sampling factors
                if pos + 2 + 6 <= data.len() {
                    let num_components = data[pos + 2 + 5] as usize;
                    if num_components == 1 {
                        subsampling = JpegSubsampling::Greyscale;
                    } else if num_components >= 3 && pos + 2 + 6 + num_components * 3 <= data.len()
                    {
                        let comp_offset = pos + 2 + 6;
                        // Component 0 (Y): sampling factors at byte 1
                        let y_sampling = data[comp_offset + 1];
                        let y_h = (y_sampling >> 4) & 0x0F;
                        let y_v = y_sampling & 0x0F;

                        // Match PIL's get_sampling() logic
                        subsampling = match (y_h, y_v) {
                            (1, 1) => JpegSubsampling::Subsampling444,
                            (2, 1) => JpegSubsampling::Subsampling422,
                            (2, 2) => JpegSubsampling::Subsampling420,
                            _ => JpegSubsampling::Unknown,
                        };
                    }
                }
            }
            // SOS — Start of Scan (stop parsing markers, image data follows)
            0xDA => break,
            _ => {}
        }

        pos += length;
    }

    Ok((tables, subsampling))
}

/// Detect JPEG chroma subsampling from a file.
/// Python: `HydrusImageMetadata.GetJpegSubsamplingRaw(pil_image)`
pub fn get_jpeg_subsampling(path: &Path) -> Result<JpegSubsampling, FileError> {
    let data = std::fs::read(path)?;
    let (_, subsampling) = parse_jpeg_markers(&data)?;
    Ok(subsampling)
}

/// Estimate JPEG visual quality from quantization tables.
///
/// Python: `HydrusImageMetadata.GetJPEGQuantizationQualityEstimate(pil_image)`
///
/// Returns `(label, quality_score)` where higher score = worse quality.
/// Returns `("unknown", None)` if no quantization tables found.
pub fn get_jpeg_quality_estimate(path: &Path) -> Result<(String, Option<f64>), FileError> {
    let data = std::fs::read(path)?;
    let (tables, subsampling) = parse_jpeg_markers(&data)?;

    if tables.is_empty() {
        return Ok(("unknown".to_string(), None));
    }

    // Sum all coefficients, average across tables
    let total: f64 = tables
        .iter()
        .map(|t| t.iter().map(|&v| v as f64).sum::<f64>())
        .sum();
    let mut quality = total / tables.len() as f64;

    // Adjust for subsampling — matches Python exactly
    // quality = quality ** (1 / subsampling_quality_lookup[subsampling_value])
    let factor = subsampling.quality_factor();
    quality = quality.powf(1.0 / factor);

    // Map to label — matches Python thresholds exactly
    let label = if quality >= 2800.0 {
        "very low"
    } else if quality >= 2000.0 {
        "low"
    } else if quality >= 1400.0 {
        "medium low"
    } else if quality >= 1000.0 {
        "medium"
    } else if quality >= 700.0 {
        "medium high"
    } else if quality >= 480.0 {
        "high"
    } else if quality >= 330.0 {
        "very high"
    } else {
        "extremely high"
    };

    Ok((label.to_string(), Some(quality)))
}

// ── ICC profile construction (via lcms2 crate) ─────────────────────

/// Convert an XYZ chromaticity matrix column to CIE xyY coordinates.
///
/// The matrix is `[X_row, Y_row, Z_row]` with columns for R, G, B primaries.
fn matrix_column_to_ciexyy(matrix: &[[f64; 3]; 3], col: usize) -> lcms2::CIExyY {
    let x = matrix[0][col];
    let y = matrix[1][col];
    let z = matrix[2][col];
    let sum = x + y + z;
    lcms2::CIExyY {
        x: x / sum,
        y: y / sum,
        Y: 1.0,
    }
}

/// Build a complete ICC profile from gamma, white point, and chromaticity matrix.
///
/// Uses the lcms2 crate instead of hand-written binary ICC construction.
/// Produces a standards-compliant ICC v4 profile with proper chromatic adaptation.
///
/// Python equivalent: `HydrusImageICCProfiles.make_gamma_and_chromaticity_icc_profile(gamma, white_xy, chromaticity_xyz_matrix)`
pub fn make_gamma_and_chromaticity_icc_profile(
    gamma: f64,
    white_xy: (f64, f64),
    chromaticity_xyz_matrix: &[[f64; 3]; 3],
) -> Result<Vec<u8>, FileError> {
    let white_point = lcms2::CIExyY {
        x: white_xy.0,
        y: white_xy.1,
        Y: 1.0,
    };

    let primaries = lcms2::CIExyYTRIPLE {
        Red: matrix_column_to_ciexyy(chromaticity_xyz_matrix, 0),
        Green: matrix_column_to_ciexyy(chromaticity_xyz_matrix, 1),
        Blue: matrix_column_to_ciexyy(chromaticity_xyz_matrix, 2),
    };

    let tone_curve = lcms2::ToneCurve::new(gamma);

    let profile = lcms2::Profile::new_rgb(
        &white_point,
        &primaries,
        &[&tone_curve, &tone_curve, &tone_curve],
    )
    .map_err(|e| FileError::Hash(format!("Failed to create ICC profile: {}", e)))?;

    profile
        .icc()
        .map_err(|e| FileError::Hash(format!("Failed to serialize ICC profile: {}", e)))
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsampling_quality_factors() {
        assert_eq!(JpegSubsampling::Subsampling444.quality_factor(), 1.0);
        assert_eq!(JpegSubsampling::Subsampling422.quality_factor(), 0.93);
        assert_eq!(JpegSubsampling::Subsampling420.quality_factor(), 0.83);
        assert_eq!(JpegSubsampling::Unknown.quality_factor(), 0.75);
        assert_eq!(JpegSubsampling::Greyscale.quality_factor(), 0.967);
    }

    #[test]
    fn test_subsampling_str() {
        assert_eq!(JpegSubsampling::Subsampling444.as_str(), "4:4:4");
        assert_eq!(JpegSubsampling::Subsampling420.as_str(), "4:2:0");
        assert_eq!(
            JpegSubsampling::Greyscale.as_str(),
            "greyscale (no subsampling)"
        );
    }

    #[test]
    fn test_full_icc_profile_lcms2() {
        // sRGB chromaticity matrix
        let srgb_matrix: [[f64; 3]; 3] = [
            [0.4124564, 0.3575761, 0.1804375],
            [0.2126729, 0.7151522, 0.0721750],
            [0.0193339, 0.1191920, 0.9503041],
        ];
        let profile = make_gamma_and_chromaticity_icc_profile(
            2.2,
            (0.3127, 0.3290), // D65 white point
            &srgb_matrix,
        )
        .unwrap();

        // Verify basic structure
        assert!(profile.len() > 128, "Profile too short");

        // Check ICC signature at offset 36
        assert_eq!(&profile[36..40], b"acsp");
    }

    #[test]
    fn test_jpeg_marker_parsing_minimal() {
        // Minimal JPEG with SOI marker only
        let data = vec![0xFF, 0xD8, 0xFF, 0xD9]; // SOI + EOI
        let (tables, sub) = parse_jpeg_markers(&data).unwrap();
        assert!(tables.is_empty());
        assert_eq!(sub, JpegSubsampling::Unknown);
    }

    #[test]
    fn test_jpeg_not_jpeg() {
        let data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header
        assert!(parse_jpeg_markers(&data).is_err());
    }
}
