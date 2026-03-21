//! PDF file handling — parses PDF structure for metadata (page count, dimensions).
//! Resolution is calculated from page point size at 300 DPI.

use std::path::Path;

use super::{FileError, FileResult};

/// Assumed DPI for calculating PDF page resolution.
const PDF_ASSUMED_DPI: f64 = 300.0;

/// Get PDF info: word count (always None — needs PDF library) and page dimensions.
pub fn get_pdf_info(path: &Path) -> FileResult<(Option<u32>, (Option<u32>, Option<u32>))> {
    let data = std::fs::read(path).map_err(FileError::Io)?;
    let resolution = get_pdf_resolution_from_bytes(&data);
    let num_words = None;

    match resolution {
        Ok((w, h)) => Ok((num_words, (Some(w), Some(h)))),
        Err(_) => Ok((num_words, (None, None))),
    }
}

/// Parse PDF MediaBox from raw bytes to get page dimensions.
/// Searches for `/MediaBox [x0 y0 x1 y1]` and converts points to pixels at 300 DPI.
fn get_pdf_resolution_from_bytes(data: &[u8]) -> FileResult<(u32, u32)> {
    // Search for /MediaBox in the PDF content
    // MediaBox format: /MediaBox [x0 y0 x1 y1] where dimensions are in points (1/72 inch)
    let data_str = String::from_utf8_lossy(data);

    // Try to find /MediaBox entries - the first one for a page object
    // is typically the page dimensions
    if let Some(pos) = data_str.find("/MediaBox") {
        let after = &data_str[pos..];

        // Find the bracket-delimited array
        if let Some(bracket_start) = after.find('[') {
            if let Some(bracket_end) = after[bracket_start..].find(']') {
                let array_str = &after[bracket_start + 1..bracket_start + bracket_end];
                let parts: Vec<&str> = array_str.split_whitespace().collect();

                if parts.len() >= 4 {
                    // MediaBox [x0 y0 x1 y1]
                    let x0: f64 = parts[0].parse().unwrap_or(0.0);
                    let y0: f64 = parts[1].parse().unwrap_or(0.0);
                    let x1: f64 = parts[2].parse().unwrap_or(0.0);
                    let y1: f64 = parts[3].parse().unwrap_or(0.0);

                    let width_pts = (x1 - x0).abs();
                    let height_pts = (y1 - y0).abs();

                    let width = (width_pts * (PDF_ASSUMED_DPI / 72.0)).round() as u32;
                    let height = (height_pts * (PDF_ASSUMED_DPI / 72.0)).round() as u32;

                    if width > 0 && height > 0 {
                        return Ok((width, height));
                    }
                }
            }
        }
    }

    Err(FileError::UnsupportedFile(
        "Could not parse PDF MediaBox for resolution".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mediabox() {
        // Minimal PDF with a MediaBox
        let pdf_content = b"%PDF-1.4\n1 0 obj\n<< /Type /Page /MediaBox [0 0 612 792] >>\nendobj\n";
        let result = get_pdf_resolution_from_bytes(pdf_content);
        assert!(result.is_ok());
        let (w, h) = result.unwrap();
        // 612 pts * (300/72) = 2550, 792 pts * (300/72) = 3300
        assert_eq!(w, 2550);
        assert_eq!(h, 3300);
    }

    #[test]
    fn test_parse_mediabox_letter() {
        // US Letter size: 8.5 x 11 inches = 612 x 792 points
        let pdf_content = b"%PDF-1.7\n/MediaBox [0 0 612 792]\n";
        let result = get_pdf_resolution_from_bytes(pdf_content);
        assert!(result.is_ok());
        let (w, h) = result.unwrap();
        assert_eq!(w, 2550); // 612 * 300/72
        assert_eq!(h, 3300); // 792 * 300/72
    }
}
