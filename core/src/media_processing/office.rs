//! Office document file handling (OOXML and OLE formats).
//!
//! Ported from:
//! - `hydrus/core/files/HydrusOfficeOpenXMLHandling.py` (DOCX, XLSX, PPTX)
//! - `hydrus/core/files/HydrusOLEHandling.py` (DOC, XLS, PPT)
//!
//! OOXML files are ZIP archives containing XML metadata.
//! OLE files use the legacy Compound Binary File format.

use std::io::{Cursor, Read};
use std::path::Path;

use image::GenericImageView;

use super::{FileError, FileResult};

// --- OOXML (Office Open XML) Handling ---

/// Assumed DPI for PPTX resolution calculation (matches Python's PPTX_ASSUMED_DPI = 300).
const PPTX_ASSUMED_DPI: f64 = 300.0;

/// EMU (English Metric Units) per inch. PowerPoint uses EMU for coordinates.
/// 1 inch = 914400 EMU.
const EMU_PER_INCH: f64 = 914400.0;

/// Pixels per EMU at assumed DPI.
const PPTX_PIXEL_PER_EMU: f64 = PPTX_ASSUMED_DPI / EMU_PER_INCH;

/// Get PowerPoint presentation slide dimensions.
///
/// Ported from `HydrusOfficeOpenXMLHandling.PowerPointResolution()`.
/// Reads `ppt/presentation.xml` from the PPTX (ZIP) archive and extracts
/// the `sldSz` element's `cx` and `cy` attributes (in EMU).
pub fn powerpoint_resolution(path: &Path) -> FileResult<(u32, u32)> {
    let xml = read_ooxml_entry(path, "ppt/presentation.xml")?;

    let doc = roxmltree::Document::parse(&xml).map_err(|e| {
        FileError::UnsupportedFile(format!("Could not parse presentation.xml: {}", e))
    })?;

    // Find sldSz element (may be namespaced)
    let sld_sz = doc
        .descendants()
        .find(|n| n.has_tag_name("sldSz"))
        .ok_or_else(|| {
            FileError::UnsupportedFile(
                "Could not find sldSz element in presentation.xml".to_string(),
            )
        })?;

    let cx: f64 = sld_sz
        .attribute("cx")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| {
            FileError::UnsupportedFile("Could not parse sldSz cx attribute".to_string())
        })?;

    let cy: f64 = sld_sz
        .attribute("cy")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| {
            FileError::UnsupportedFile("Could not parse sldSz cy attribute".to_string())
        })?;

    let width = (cx * PPTX_PIXEL_PER_EMU).round() as u32;
    let height = (cy * PPTX_PIXEL_PER_EMU).round() as u32;

    Ok((width, height))
}

/// Get word count from an OOXML document's extended properties.
///
/// Ported from `HydrusOfficeOpenXMLHandling.OfficeDocumentWordCount()`.
/// Reads `docProps/app.xml` from the OOXML archive and extracts the `Words` element.
pub fn office_document_word_count(path: &Path) -> FileResult<u32> {
    let xml = read_ooxml_entry(path, "docProps/app.xml")?;

    let doc = roxmltree::Document::parse(&xml)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not parse app.xml: {}", e)))?;

    // Find the Words element (may be namespaced with ep: prefix)
    let words_text = doc
        .descendants()
        .find(|n| n.has_tag_name("Words"))
        .and_then(|n| n.text())
        .ok_or_else(|| {
            FileError::UnsupportedFile("Could not find Words element in app.xml".to_string())
        })?;

    let num_words: u32 = words_text.parse().map_err(|_| {
        FileError::UnsupportedFile(format!(
            "Could not parse word count '{}' as integer",
            words_text
        ))
    })?;

    Ok(num_words)
}

/// Get PPTX info: word count and slide dimensions.
///
/// Ported from `HydrusOfficeOpenXMLHandling.GetPPTXInfo()`.
/// Returns `(num_words, (width, height))`.
pub fn get_pptx_info(path: &Path) -> (Option<u32>, (Option<u32>, Option<u32>)) {
    let resolution = powerpoint_resolution(path).ok();
    let num_words = office_document_word_count(path).ok();

    let (width, height) = match resolution {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    };

    (num_words, (width, height))
}

/// Get DOCX info: word count.
///
/// Ported from `HydrusOfficeOpenXMLHandling.GetDOCXInfo()`.
/// Returns word count or None.
pub fn get_docx_info(path: &Path) -> Option<u32> {
    office_document_word_count(path).ok()
}

/// Generate a thumbnail from an OOXML document.
///
/// Ported from `HydrusOfficeOpenXMLHandling.GenerateThumbnailNumPyFromOfficePath()`.
/// Extracts `docProps/thumbnail.jpeg` from the archive and resizes it.
pub fn generate_thumbnail_from_office(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    let thumb_bytes = read_ooxml_entry_bytes(path, "docProps/thumbnail.jpeg")?;

    // Load thumbnail image
    let reader = image::ImageReader::new(Cursor::new(&thumb_bytes))
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let img = reader
        .decode()
        .map_err(|e| FileError::Thumbnail(format!("Could not decode Office thumbnail: {}", e)))?;

    let (orig_w, orig_h) = img.dimensions();
    let (tw, th) = super::get_thumbnail_resolution(
        (orig_w, orig_h),
        target_resolution,
        super::ThumbnailScaleType::ScaleToFit,
        100,
    );

    let thumbnail = img.resize_exact(tw, th, image::imageops::FilterType::Lanczos3);

    super::encode_thumbnail_jpeg(&thumbnail)
}

/// Read a file entry from an OOXML (ZIP) archive as a UTF-8 string.
fn read_ooxml_entry(path: &Path, entry_path: &str) -> FileResult<String> {
    let file = std::fs::File::open(path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open OOXML archive: {}", e)))?;

    let mut entry = zip.by_name(entry_path).map_err(|e| {
        FileError::UnsupportedFile(format!(
            "Could not find '{}' in OOXML archive: {}",
            entry_path, e
        ))
    })?;

    let mut buf = String::new();
    entry.read_to_string(&mut buf).map_err(FileError::Io)?;

    Ok(buf)
}

/// Read a file entry from an OOXML (ZIP) archive as bytes.
fn read_ooxml_entry_bytes(path: &Path, entry_path: &str) -> FileResult<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open OOXML archive: {}", e)))?;

    let mut entry = zip.by_name(entry_path).map_err(|e| {
        FileError::Thumbnail(format!(
            "Could not find '{}' in OOXML archive: {}",
            entry_path, e
        ))
    })?;

    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf).map_err(FileError::Io)?;

    Ok(buf)
}

// --- OLE (Legacy Office) Handling ---

/// OLE Compound Binary File header signature.
const OLE_HEADER: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Check if a file is an OLE Compound Binary File.
///
/// Ported from `HydrusOLEHandling.isOleFile()`.
pub fn is_ole_file(path: &Path) -> bool {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut header = [0u8; 8];
    match file.read_exact(&mut header) {
        Ok(()) => header == OLE_HEADER,
        Err(_) => false,
    }
}

/// Get word count from a legacy OLE Office document.
///
/// Ported from `HydrusOLEHandling.OfficeOLEDocumentWordCount()`.
///
/// Note: Full OLE parsing requires a dedicated library (like `olefile` in Python).
/// Without one, we cannot reliably extract metadata from OLE files.
/// This returns None as we don't have an OLE parsing library.
pub fn ole_document_word_count(path: &Path) -> FileResult<Option<u32>> {
    if !is_ole_file(path) {
        return Err(FileError::UnsupportedFile("File is not OLE!".to_string()));
    }

    // TODO: Port full OLE metadata parsing when an OLE library is available.
    // Python uses `olefile.OleFileIO(path).get_metadata().num_words`.
    // Without an OLE parsing library, we cannot extract word count.
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pptx_pixel_per_emu() {
        // Standard slide size: 12192000 EMU x 6858000 EMU (10" x 7.5")
        let width = (12192000.0 * PPTX_PIXEL_PER_EMU).round() as u32;
        let height = (6858000.0 * PPTX_PIXEL_PER_EMU).round() as u32;
        // At 300 DPI: 10 inches * 300 = 3000, 7.5 inches * 300 = 2250
        assert_eq!(width, 4000); // 12192000 * 300 / 914400 = 4000
        assert_eq!(height, 2250); // 6858000 * 300 / 914400 = 2250
    }

    #[test]
    fn test_is_ole_file_with_non_ole() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not an ole file").unwrap();
        assert!(!is_ole_file(tmp.path()));
    }

    #[test]
    fn test_is_ole_file_with_ole_header() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut data = OLE_HEADER.to_vec();
        data.extend_from_slice(&[0u8; 100]); // pad
        std::fs::write(tmp.path(), &data).unwrap();
        assert!(is_ole_file(tmp.path()));
    }
}
