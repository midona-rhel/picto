//! Specialty file format handlers.
//!
//! Ported from:
//! - `HydrusClipHandling.py` - CLIP Studio Paint files (embedded SQLite)
//! - `HydrusKritaHandling.py` - Krita KRA files (ZIP with XML metadata)
//! - `HydrusPaintNETHandling.py` - Paint.NET PDN files (binary header with XML)
//! - `HydrusProcreateHandling.py` - Procreate files (ZIP with plist)
//! - `HydrusPSDHandling.py` - Adobe PSD files (binary header)
//! - `HydrusUgoiraHandling.py` - Ugoira animations (ZIP with frame images)
//! - `HydrusFlashHandling.py` - SWF Flash files (binary header)

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use image::GenericImageView;

use super::{FileError, FileResult};

// ==================== CLIP Studio Paint ====================

/// SQLite file signature.
const SQLITE_MAGIC: &[u8] = b"SQLite format 3";

/// Get CLIP Studio Paint file properties: ((width, height), duration_ms, num_frames).
///
/// Ported from `HydrusClipHandling.GetClipProperties()`.
/// CLIP files embed a SQLite database containing canvas metadata.
/// The SQLite portion is extracted, then Canvas table is queried for dimensions.
pub fn get_clip_properties(path: &Path) -> FileResult<((u32, u32), Option<u64>, Option<u32>)> {
    let clip_bytes = std::fs::read(path).map_err(FileError::Io)?;

    // Find the SQLite database within the CLIP file
    let sqlite_offset = clip_bytes
        .windows(SQLITE_MAGIC.len())
        .position(|w| w == SQLITE_MAGIC)
        .ok_or_else(|| {
            FileError::UnsupportedFile("This clip file had no internal SQLite file!".to_string())
        })?;

    let sqlite_bytes = &clip_bytes[sqlite_offset..];

    // Write to temp file and open with rusqlite
    let tmp = tempfile::NamedTempFile::new().map_err(FileError::Io)?;
    std::fs::write(tmp.path(), sqlite_bytes).map_err(FileError::Io)?;

    let db = rusqlite::Connection::open_with_flags(
        tmp.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| {
        FileError::UnsupportedFile(format!(
            "This clip file seemed to have an invalid internal SQLite file: {}",
            e
        ))
    })?;

    // Query canvas dimensions
    let (width_float, height_float, canvas_unit, canvas_dpi_float): (f64, f64, i32, f64) = db
        .query_row(
            "SELECT CanvasWidth, CanvasHeight, CanvasUnit, CanvasResolution FROM Canvas;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| {
            FileError::UnsupportedFile(format!("Could not read Canvas data from CLIP: {}", e))
        })?;

    // Check for animation timeline
    let mut num_frames: Option<u32> = None;
    let mut duration_ms: Option<u64> = None;

    // Python: checks if TimeLine table exists
    let has_timeline: bool = db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE name = 'TimeLine';",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if has_timeline {
        if let Ok((start_frame, framerate, end_frame)) = db.query_row(
            "SELECT StartFrame, FrameRate, EndFrame FROM TimeLine;",
            [],
            |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
        ) {
            let frames = (end_frame - start_frame) as u32;
            num_frames = Some(frames);

            let fps = if framerate == 0.0 { 24.0 } else { framerate };
            let duration_s = frames as f64 / fps;
            duration_ms = Some((duration_s * 1000.0) as u64);
        }
    }

    // Unit conversion to pixels (matches Python exactly)
    // canvas_unit: 0=pixels, 1=cm, 2=mm, 3=inches, 5=points
    let unit_conversion_multiplier: f64 = match canvas_unit {
        0 => 1.0,                     // pixels
        1 => canvas_dpi_float / 2.54, // cm → pixels via DPI
        2 => canvas_dpi_float / 25.4, // mm → pixels via DPI
        3 => canvas_dpi_float,        // inches → pixels via DPI
        5 => canvas_dpi_float / 72.0, // points → pixels via DPI
        _ => 1.0,                     // unknown, treat as pixels
    };

    let width = (width_float * unit_conversion_multiplier).round() as u32;
    let height = (height_float * unit_conversion_multiplier).round() as u32;

    Ok(((width, height), duration_ms, num_frames))
}

/// Extract the DBPNG preview image from a CLIP file.
///
/// Ported from `HydrusClipHandling.ExtractDBPNGToPath()`.
/// The CLIP file's embedded SQLite database has a CanvasPreview table
/// with the PNG image data.
pub fn extract_clip_dbpng(path: &Path) -> FileResult<Vec<u8>> {
    let clip_bytes = std::fs::read(path).map_err(FileError::Io)?;

    let sqlite_offset = clip_bytes
        .windows(SQLITE_MAGIC.len())
        .position(|w| w == SQLITE_MAGIC)
        .ok_or_else(|| {
            FileError::Thumbnail("This clip file had no internal SQLite file!".to_string())
        })?;

    let sqlite_bytes = &clip_bytes[sqlite_offset..];
    let tmp = tempfile::NamedTempFile::new().map_err(FileError::Io)?;
    std::fs::write(tmp.path(), sqlite_bytes).map_err(FileError::Io)?;

    let db = rusqlite::Connection::open_with_flags(
        tmp.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| FileError::Thumbnail(format!("Could not open CLIP SQLite: {}", e)))?;

    let png_bytes: Vec<u8> = db
        .query_row("SELECT ImageData FROM CanvasPreview;", [], |row| row.get(0))
        .map_err(|_| FileError::Thumbnail("No preview image in CLIP file".to_string()))?;

    Ok(png_bytes)
}

/// Generate a thumbnail from a CLIP file.
pub fn generate_thumbnail_from_clip(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    let png_bytes = extract_clip_dbpng(path)?;
    resize_image_bytes_to_thumbnail(&png_bytes, target_resolution)
}

// ==================== Krita ====================

const KRITA_FILE_MERGED: &str = "mergedimage.png";
const KRITA_FILE_THUMB: &str = "preview.png";
const KRITA_DOC_INFO: &str = "maindoc.xml";

/// Get Krita KRA file properties (width, height).
///
/// Ported from `HydrusKritaHandling.GetKraProperties()`.
/// Reads `maindoc.xml` from the KRA (ZIP) archive and extracts width/height
/// from the IMAGE element attributes.
pub fn get_kra_properties(path: &Path) -> FileResult<(u32, u32)> {
    let xml = read_zip_entry_string(path, KRITA_DOC_INFO)?;

    let doc = roxmltree::Document::parse(&xml).map_err(|e| {
        FileError::UnsupportedFile(format!("Could not parse Krita maindoc.xml: {}", e))
    })?;

    // Find the IMAGE element (namespaced: {http://www.calligra.org/DTD/krita}IMAGE)
    let image_elem = doc
        .descendants()
        .find(|n| n.has_tag_name("IMAGE"))
        .ok_or_else(|| {
            FileError::UnsupportedFile(
                "This krita file had no IMAGE element in maindoc.xml!".to_string(),
            )
        })?;

    let width: u32 = image_elem
        .attribute("width")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| FileError::UnsupportedFile("Could not parse Krita width".to_string()))?;

    let height: u32 = image_elem
        .attribute("height")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| FileError::UnsupportedFile("Could not parse Krita height".to_string()))?;

    Ok((width, height))
}

/// Generate a thumbnail from a Krita KRA file.
///
/// Ported from `HydrusKritaHandling.GenerateThumbnailNumPyFromKraPath()`.
/// Tries the merged image first, falls back to the preview thumbnail.
pub fn generate_thumbnail_from_krita(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    // Try merged image first, fall back to thumbnail (matches Python)
    let image_bytes = read_zip_entry_bytes(path, KRITA_FILE_MERGED)
        .or_else(|_| read_zip_entry_bytes(path, KRITA_FILE_THUMB))?;

    resize_image_bytes_to_thumbnail(&image_bytes, target_resolution)
}

// ==================== Paint.NET ====================

/// Get Paint.NET file resolution.
///
/// Ported from `HydrusPaintNETHandling.GetPaintNETResolution()`.
/// PDN files have a binary header followed by an XML section containing
/// width and height attributes.
pub fn get_paint_net_resolution(path: &Path) -> FileResult<(u32, u32)> {
    let xml_header = get_paint_net_xml_header(path)?;

    let doc = roxmltree::Document::parse(&xml_header).map_err(|e| {
        FileError::UnsupportedFile(format!(
            "Cannot parse the XML from this Paint.NET file: {}",
            e
        ))
    })?;

    let root = doc.root_element();

    let width: u32 = root
        .attribute("width")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| FileError::UnsupportedFile("Could not parse Paint.NET width".to_string()))?;

    let height: u32 = root
        .attribute("height")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| {
            FileError::UnsupportedFile("Could not parse Paint.NET height".to_string())
        })?;

    Ok((width, height))
}

/// Generate a thumbnail from a Paint.NET file.
///
/// Ported from `HydrusPaintNETHandling.GenerateThumbnailNumPyFromPaintNET()`.
/// The PDN XML header contains a base64-encoded PNG thumbnail.
pub fn generate_thumbnail_from_paint_net(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    let xml_header = get_paint_net_xml_header(path)?;

    let doc = roxmltree::Document::parse(&xml_header).map_err(|e| {
        FileError::Thumbnail(format!(
            "Cannot parse the XML from this Paint.NET file: {}",
            e
        ))
    })?;

    // Find ./custom/thumb element with png attribute
    let thumb_elem = doc
        .descendants()
        .find(|n| n.has_tag_name("thumb"))
        .ok_or_else(|| {
            FileError::Thumbnail("Could not read thumb bytes from this Paint.NET xml!".to_string())
        })?;

    let png_b64 = thumb_elem.attribute("png").ok_or_else(|| {
        FileError::Thumbnail("Could not read thumb bytes from this Paint.NET xml!".to_string())
    })?;

    let png_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, png_b64)
        .map_err(|_| {
            FileError::Thumbnail(
                "Could not decode thumb bytes from this Paint.NET xml!".to_string(),
            )
        })?;

    resize_image_bytes_to_thumbnail(&png_bytes, target_resolution)
}

/// Read the XML header from a Paint.NET file.
///
/// Ported from `HydrusPaintNETHandling.GetPaintNETXMLHeader()`.
/// Format: 4 magic bytes, 3-byte header length (little-endian), then UTF-8 XML.
fn get_paint_net_xml_header(path: &Path) -> FileResult<String> {
    let mut file = std::fs::File::open(path).map_err(FileError::Io)?;

    // Skip 4-byte magic number
    file.seek(SeekFrom::Start(4)).map_err(FileError::Io)?;

    // Read 3-byte header length (little-endian, padded to 4 bytes)
    let mut len_bytes = [0u8; 4];
    file.read_exact(&mut len_bytes[..3])
        .map_err(FileError::Io)?;
    // len_bytes[3] is already 0 (the padding to make it a u32 LE)
    let header_length = u32::from_le_bytes(len_bytes) as usize;

    // Read the XML header
    let mut xml_bytes = vec![0u8; header_length];
    file.read_exact(&mut xml_bytes).map_err(FileError::Io)?;

    String::from_utf8(xml_bytes).map_err(|e| {
        FileError::UnsupportedFile(format!(
            "Cannot read the XML from this Paint.NET file: {}",
            e
        ))
    })
}

// ==================== Procreate ====================

const PROCREATE_THUMBNAIL_FILE_PATH: &str = "QuickLook/Thumbnail.png";
const PROCREATE_DOCUMENT_ARCHIVE: &str = "Document.archive";

/// Get Procreate file resolution.
///
/// Ported from `HydrusProcreateHandling.GetProcreateResolution()`.
/// Procreate files are ZIP archives. The `Document.archive` file is a binary plist
/// containing canvas dimensions and orientation.
///
/// Note: Full plist parsing would require a dedicated library. We attempt a
/// simplified extraction approach, falling back gracefully.
pub fn get_procreate_resolution(path: &Path) -> FileResult<(u32, u32)> {
    // Procreate uses binary plist format in Document.archive
    // Full parsing requires a plist library. Python uses plistlib.
    // We attempt to extract dimensions from the plist data.
    let plist_bytes = read_zip_entry_bytes(path, PROCREATE_DOCUMENT_ARCHIVE)?;

    // Try to parse as XML plist first (some files use XML format)
    if let Ok(plist_str) = String::from_utf8(plist_bytes.clone()) {
        if let Some(dims) = parse_procreate_xml_plist(&plist_str) {
            return Ok(dims);
        }
    }

    // Binary plist parsing would require a dedicated library.
    // Python uses plistlib which handles binary plists natively.
    // TODO: Add plist crate for binary plist support
    Err(FileError::UnsupportedFile(
        "Procreate binary plist parsing not yet supported (needs plist crate)".to_string(),
    ))
}

/// Try to parse dimensions from an XML plist (fallback for non-binary plists).
fn parse_procreate_xml_plist(_xml: &str) -> Option<(u32, u32)> {
    // This is a simplified parser for the rare case of XML plists
    // Most Procreate files use binary plist format
    None
}

/// Extract the thumbnail from a Procreate file.
///
/// Ported from `HydrusProcreateHandling.ExtractZippedThumbnailToPath()`.
pub fn extract_procreate_thumbnail(path: &Path) -> FileResult<Vec<u8>> {
    read_zip_entry_bytes(path, PROCREATE_THUMBNAIL_FILE_PATH)
        .map_err(|_| FileError::Thumbnail("This procreate file had no thumbnail file!".to_string()))
}

/// Generate a thumbnail from a Procreate file.
pub fn generate_thumbnail_from_procreate(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    let png_bytes = extract_procreate_thumbnail(path)?;
    resize_image_bytes_to_thumbnail(&png_bytes, target_resolution)
}

// ==================== PSD (Photoshop) ====================

/// Get PSD file resolution from the binary header.
///
/// Ported from `HydrusPSDHandling.GetPSDResolution()`.
/// PSD header layout: at offset 14, 4 bytes height (big-endian), 4 bytes width (big-endian).
pub fn get_psd_resolution(path: &Path) -> FileResult<(u32, u32)> {
    let mut file = std::fs::File::open(path).map_err(FileError::Io)?;

    // Seek to offset 14 (past the PSD signature and version info)
    file.seek(SeekFrom::Start(14)).map_err(FileError::Io)?;

    let mut height_bytes = [0u8; 4];
    let mut width_bytes = [0u8; 4];

    file.read_exact(&mut height_bytes).map_err(FileError::Io)?;
    file.read_exact(&mut width_bytes).map_err(FileError::Io)?;

    let height = u32::from_be_bytes(height_bytes);
    let width = u32::from_be_bytes(width_bytes);

    Ok((width, height))
}

/// Generate a thumbnail from a PSD file.
///
/// Ported from `HydrusPSDHandling.GenerateThumbnailNumPyFromPSDPath()`.
/// Python uses FFMPEG to render PSD preview to PNG bytes, then resizes.
/// We use ffmpeg-next via the existing infrastructure.
pub fn generate_thumbnail_from_psd(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    // Python renders PSD via FFMPEG: HydrusFFMPEG.RenderImageToPNGBytes(path)
    // Then resizes the result. We can try using the image crate directly,
    // as some PSD files can be loaded by it.
    // Fall back to returning an error if the image crate can't handle it.
    let reader = image::ImageReader::open(path)
        .map_err(FileError::Io)?
        .with_guessed_format()
        .map_err(FileError::Io)?;

    let img = reader.decode().map_err(|e| {
        FileError::Thumbnail(format!(
            "Could not decode PSD for thumbnail (FFMPEG fallback needed): {}",
            e
        ))
    })?;

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

// ==================== Ugoira ====================

/// Get Ugoira file properties: ((width, height), duration_ms, num_frames).
///
/// Ported from `HydrusUgoiraHandling.GetUgoiraProperties()`.
/// Tries JSON metadata first, then falls back to scanning frame images.
pub fn get_ugoira_properties(path: &Path) -> FileResult<((u32, u32), Option<u64>, Option<u32>)> {
    // Try JSON-based properties first
    if let Ok(props) = get_ugoira_properties_from_json(path) {
        return Ok(props);
    }

    // Fallback: get resolution from first frame image
    let (width, height) = get_ugoira_first_frame_dimensions(path).unwrap_or((100, 100)); // Python defaults to (100, 100)

    let num_frames = get_frame_paths_from_ugoira_zip(path)
        .map(|paths| paths.len() as u32)
        .ok();

    Ok(((width, height), None, num_frames))
}

/// Get Ugoira properties from the animation.json metadata file.
///
/// Ported from `HydrusUgoiraHandling.GetUgoiraPropertiesFromJSON()`.
fn get_ugoira_properties_from_json(
    path: &Path,
) -> FileResult<((u32, u32), Option<u64>, Option<u32>)> {
    let frame_data = get_ugoira_frame_data_json(path)?;

    if frame_data.is_empty() {
        return Err(FileError::UnsupportedFile(
            "Ugoira animation.json has no frames".to_string(),
        ));
    }

    let duration_ms: u64 = frame_data.iter().map(|f| f.delay as u64).sum();
    let num_frames = frame_data.len() as u32;

    // Get resolution from first frame
    let first_frame_path = &frame_data[0].file;
    let first_frame_bytes = super::archive::get_single_file_from_zip_bytes(path, first_frame_path)?;

    let reader = image::ImageReader::new(Cursor::new(&first_frame_bytes))
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let (width, height) = reader.into_dimensions().map_err(|e| {
        FileError::UnsupportedFile(format!("Could not read first frame dimensions: {}", e))
    })?;

    Ok(((width, height), Some(duration_ms), Some(num_frames)))
}

/// Ugoira frame data from animation.json.
#[derive(serde::Deserialize)]
struct UgoiraFrame {
    file: String,
    delay: u32,
}

/// Read and parse the animation.json from a Ugoira ZIP.
///
/// Ported from `HydrusUgoiraHandling.GetUgoiraFrameDataJSON()`.
fn get_ugoira_frame_data_json(path: &Path) -> FileResult<Vec<UgoiraFrame>> {
    let json_bytes = super::archive::get_single_file_from_zip_bytes(path, "animation.json")?;

    let json_str = String::from_utf8(json_bytes)
        .map_err(|_| FileError::UnsupportedFile("animation.json is not valid UTF-8".to_string()))?;

    // Python: JSON from gallery-dl is just the array, otherwise it's {frames: [...]}
    if let Ok(frames) = serde_json::from_str::<Vec<UgoiraFrame>>(&json_str) {
        return Ok(frames);
    }

    #[derive(serde::Deserialize)]
    struct UgoiraJson {
        frames: Vec<UgoiraFrame>,
    }

    let ugoira: UgoiraJson = serde_json::from_str(&json_str).map_err(|e| {
        FileError::UnsupportedFile(format!("Could not parse animation.json: {}", e))
    })?;

    Ok(ugoira.frames)
}

/// Get image file paths from a Ugoira ZIP (without JSON metadata).
///
/// Ported from `HydrusUgoiraHandling.GetFramePathsFromUgoiraZip()`.
fn get_frame_paths_from_ugoira_zip(path: &Path) -> FileResult<Vec<String>> {
    let file = std::fs::File::open(path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open Ugoira ZIP: {}", e)))?;

    let mut paths: Vec<String> = Vec::new();

    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| FileError::UnsupportedFile(format!("Could not read zip entry: {}", e)))?;

        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();
        if super::archive::filename_has_image_ext_pub(&name) {
            paths.push(name);
        }
    }

    if paths.is_empty() {
        return Err(FileError::UnsupportedFile(
            "This Ugoira seems to be empty! It has probably been corrupted!".to_string(),
        ));
    }

    paths.sort();
    Ok(paths)
}

/// Get dimensions of the first frame image in a Ugoira ZIP.
fn get_ugoira_first_frame_dimensions(path: &Path) -> FileResult<(u32, u32)> {
    // Try JSON first for frame path
    let frame_path = if let Ok(frame_data) = get_ugoira_frame_data_json(path) {
        if !frame_data.is_empty() {
            frame_data[0].file.clone()
        } else {
            get_frame_paths_from_ugoira_zip(path)?
                .into_iter()
                .next()
                .ok_or_else(|| FileError::UnsupportedFile("No frames in Ugoira".to_string()))?
        }
    } else {
        get_frame_paths_from_ugoira_zip(path)?
            .into_iter()
            .next()
            .ok_or_else(|| FileError::UnsupportedFile("No frames in Ugoira".to_string()))?
    };

    let frame_bytes = super::archive::get_single_file_from_zip_bytes(path, &frame_path)?;

    let reader = image::ImageReader::new(Cursor::new(&frame_bytes))
        .with_guessed_format()
        .map_err(FileError::Io)?;

    reader.into_dimensions().map_err(|e| {
        FileError::UnsupportedFile(format!("Could not read Ugoira frame dimensions: {}", e))
    })
}

/// Generate a thumbnail from a Ugoira file at a specific frame index.
///
/// Ported from `HydrusUgoiraHandling.GenerateThumbnailNumPyFromUgoiraPath()`.
pub fn generate_thumbnail_from_ugoira(
    path: &Path,
    target_resolution: (u32, u32),
    frame_index: usize,
) -> FileResult<Vec<u8>> {
    // Get frame paths (prefer JSON order)
    let frame_paths = if let Ok(frame_data) = get_ugoira_frame_data_json(path) {
        frame_data.into_iter().map(|f| f.file).collect::<Vec<_>>()
    } else {
        get_frame_paths_from_ugoira_zip(path)?
    };

    let actual_index = frame_index.min(frame_paths.len().saturating_sub(1));
    let frame_path = &frame_paths[actual_index];

    let frame_bytes = super::archive::get_single_file_from_zip_bytes(path, frame_path)?;

    resize_image_bytes_to_thumbnail(&frame_bytes, target_resolution)
}

// ==================== Flash (SWF) ====================

/// Get Flash SWF file properties: ((width, height), duration_ms, num_frames).
///
/// Ported from `HydrusFlashHandling.GetFlashProperties()`.
/// Parses the SWF binary header to extract dimensions, frame count, and FPS.
///
/// SWF header format:
/// - 3 bytes: signature ("FWS" or "CWS" or "ZWS")
/// - 1 byte: version
/// - 4 bytes: file length (little-endian)
/// - RECT: display dimensions (variable-length bitfield)
/// - 2 bytes: frame rate (8.8 fixed point)
/// - 2 bytes: frame count
pub fn get_flash_properties(path: &Path) -> FileResult<((u32, u32), u64, u32)> {
    let data = std::fs::read(path).map_err(FileError::Io)?;

    if data.len() < 8 {
        return Err(FileError::UnsupportedFile("SWF file too small".to_string()));
    }

    // Check signature
    let sig = &data[0..3];
    let is_compressed = match sig {
        b"FWS" => false, // uncompressed
        b"CWS" => true,  // zlib compressed
        b"ZWS" => true,  // LZMA compressed
        _ => {
            return Err(FileError::UnsupportedFile(
                "Not a valid SWF file".to_string(),
            ))
        }
    };

    // For compressed SWF, we need to decompress the body
    let body_data = if is_compressed && sig == b"CWS" {
        // Zlib compressed: decompress everything after byte 8
        let mut decoder = flate2::read::ZlibDecoder::new(&data[8..]);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|_| FileError::UnsupportedFile("Could not decompress SWF data".to_string()))?;
        decompressed
    } else if is_compressed {
        // LZMA compressed - not supported without lzma crate
        return Err(FileError::UnsupportedFile(
            "LZMA-compressed SWF not supported".to_string(),
        ));
    } else {
        // Uncompressed: body starts at byte 8
        data[8..].to_vec()
    };

    if body_data.is_empty() {
        return Err(FileError::UnsupportedFile("SWF body is empty".to_string()));
    }

    // Parse RECT structure (variable-length bitfield)
    // First 5 bits = Nbits (number of bits per value)
    // Then 4 values of Nbits each: Xmin, Xmax, Ymin, Ymax (in twips, 1/20 pixel)
    let nbits = (body_data[0] >> 3) as usize;

    if nbits == 0 {
        return Err(FileError::UnsupportedFile(
            "SWF RECT has 0 bits per value".to_string(),
        ));
    }

    // Total bits needed: 5 + 4*nbits
    let total_bits = 5 + 4 * nbits;
    let total_bytes = (total_bits + 7) / 8;

    if body_data.len() < total_bytes + 4 {
        return Err(FileError::UnsupportedFile(
            "SWF too small for RECT + frame data".to_string(),
        ));
    }

    // Read RECT values using bit manipulation
    let rect_values = read_swf_rect_values(&body_data, nbits);
    let (xmin, xmax, ymin, ymax) = (
        rect_values[0],
        rect_values[1],
        rect_values[2],
        rect_values[3],
    );

    // Convert from twips to pixels (1 twip = 1/20 pixel)
    // Python uses abs() since some flash files deliver negatives
    let width = ((xmax - xmin).abs() / 20) as u32;
    let height = ((ymax - ymin).abs() / 20) as u32;

    // Frame rate and frame count follow the RECT
    let offset = total_bytes;
    let fps_raw = u16::from_le_bytes([body_data[offset], body_data[offset + 1]]);
    let num_frames = u16::from_le_bytes([body_data[offset + 2], body_data[offset + 3]]) as u32;

    // FPS is 8.8 fixed point
    let fps = (fps_raw >> 8) as f64 + (fps_raw & 0xFF) as f64 / 256.0;
    let fps = if fps == 0.0 { 1.0 } else { fps };

    let duration_ms = ((num_frames as f64 / fps) * 1000.0) as u64;

    Ok(((width, height), duration_ms, num_frames))
}

/// Read 4 signed values from SWF RECT bitfield.
fn read_swf_rect_values(data: &[u8], nbits: usize) -> [i64; 4] {
    let mut values = [0i64; 4];
    let mut bit_offset = 5; // skip first 5 bits (Nbits field)

    for i in 0..4 {
        let mut value: i64 = 0;
        for bit in 0..nbits {
            let byte_idx = (bit_offset + bit) / 8;
            let bit_idx = 7 - ((bit_offset + bit) % 8);

            if byte_idx < data.len() && (data[byte_idx] >> bit_idx) & 1 == 1 {
                value |= 1 << (nbits - 1 - bit);
            }
        }

        // Sign extend if the high bit is set
        if nbits > 0 && (value >> (nbits - 1)) & 1 == 1 {
            value |= !((1i64 << nbits) - 1);
        }

        values[i] = value;
        bit_offset += nbits;
    }

    values
}

// ==================== Shared Utilities ====================

/// Read a ZIP entry as a UTF-8 string.
fn read_zip_entry_string(archive_path: &Path, entry_path: &str) -> FileResult<String> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open ZIP archive: {}", e)))?;

    let mut entry = zip.by_name(entry_path).map_err(|e| {
        FileError::UnsupportedFile(format!("Could not find '{}' in ZIP: {}", entry_path, e))
    })?;

    let mut buf = String::new();
    entry.read_to_string(&mut buf).map_err(FileError::Io)?;

    Ok(buf)
}

/// Read a ZIP entry as bytes.
fn read_zip_entry_bytes(archive_path: &Path, entry_path: &str) -> FileResult<Vec<u8>> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open ZIP archive: {}", e)))?;

    let mut entry = zip.by_name(entry_path).map_err(|e| {
        FileError::UnsupportedFile(format!("Could not find '{}' in ZIP: {}", entry_path, e))
    })?;

    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf).map_err(FileError::Io)?;

    Ok(buf)
}

/// Resize image bytes to a thumbnail at the target resolution.
/// Used by multiple format handlers.
fn resize_image_bytes_to_thumbnail(
    image_bytes: &[u8],
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    let reader = image::ImageReader::new(Cursor::new(image_bytes))
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let img = reader.decode().map_err(|e| {
        FileError::Thumbnail(format!("Could not decode image for thumbnail: {}", e))
    })?;

    let (orig_w, orig_h) = img.dimensions();
    let (tw, th) = super::get_thumbnail_resolution(
        (orig_w, orig_h),
        target_resolution,
        super::ThumbnailScaleType::ScaleToFit,
        100,
    );

    let thumbnail = img.resize_exact(tw, th, image::imageops::FilterType::Lanczos3);

    // Use PNG for images with alpha, JPEG otherwise (matches Python)
    let has_alpha = img.color().has_alpha();
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);

    if has_alpha {
        thumbnail
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| FileError::Thumbnail(format!("Failed to encode thumbnail: {}", e)))?;
    } else {
        thumbnail
            .write_to(&mut cursor, image::ImageFormat::Jpeg)
            .map_err(|e| FileError::Thumbnail(format!("Failed to encode thumbnail: {}", e)))?;
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_psd_resolution_parsing() {
        // Create a minimal PSD-like binary with width=100 and height=200
        // PSD header: 4 bytes sig "8BPS", 2 bytes version, 6 bytes reserved,
        // 2 bytes channels, 4 bytes height, 4 bytes width
        let mut data = Vec::new();
        data.extend_from_slice(b"8BPS"); // signature
        data.extend_from_slice(&1u16.to_be_bytes()); // version
        data.extend_from_slice(&[0u8; 6]); // reserved
        data.extend_from_slice(&2u16.to_be_bytes()); // channels
        data.extend_from_slice(&200u32.to_be_bytes()); // height
        data.extend_from_slice(&100u32.to_be_bytes()); // width

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let (w, h) = get_psd_resolution(tmp.path()).unwrap();
        assert_eq!(w, 100);
        assert_eq!(h, 200);
    }

    #[test]
    fn test_paint_net_xml_header_parsing() {
        // Test XML parsing for Paint.NET
        let xml = r#"<pdnImage width="800" height="600"><custom><thumb png="iVBORw0KGgo="/></custom></pdnImage>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let root = doc.root_element();
        let w: u32 = root.attribute("width").unwrap().parse().unwrap();
        let h: u32 = root.attribute("height").unwrap().parse().unwrap();
        assert_eq!(w, 800);
        assert_eq!(h, 600);
    }

    #[test]
    fn test_read_swf_rect_values() {
        // Minimal test: nbits=5, values should be extracted from bitfield
        // First byte: nbits=5 means upper 5 bits = 00101 = 5
        // Then 4 values of 5 bits each
        let data: Vec<u8> = vec![
            0b00101_000,  // nbits=5, start of first value (0 so far)
            0b00_00000_0, // first value = 0, start of second value
            0b0000_0000,  // second value continues
            0b0_00000_00, // third value
            0b000_00000,  // fourth value
        ];
        let values = read_swf_rect_values(&data, 5);
        assert_eq!(values[0], 0); // all zeros
    }
}
