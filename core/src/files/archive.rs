//! Archive file handling (CBZ, EPUB, ZIP).
//!
//! Ported from `hydrus/core/files/HydrusArchiveHandling.py`.
//! Handles extraction of cover pages from archive files for resolution detection
//! and thumbnail generation.

use std::io::{Cursor, Read};
use std::path::Path;

use image::GenericImageView;

use super::{FileError, FileResult};

/// Image file extensions recognized in archives (matches Python's HC.IMAGE_FILE_EXTS).
const IMAGE_FILE_EXTS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp", ".tiff", ".tif", ".ico",
];

/// Get the path to the cover page (first image) inside a ZIP archive.
///
/// Ported from `HydrusArchiveHandling.GetCoverPagePath()`.
/// Finds the first image file in the archive, sorted by filename.
pub fn get_cover_page_path(archive_path: &Path) -> FileResult<String> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open zip archive: {}", e)))?;

    let mut image_paths: Vec<String> = Vec::new();

    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| FileError::UnsupportedFile(format!("Could not read zip entry: {}", e)))?;

        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();

        // Skip macOS resource fork files (matches Python)
        if name.starts_with("__MACOSX/") {
            continue;
        }

        if filename_has_image_ext(&name) {
            image_paths.push(name);
        }
    }

    image_paths.sort();

    image_paths
        .into_iter()
        .next()
        .ok_or_else(|| FileError::Thumbnail("No image files found in archive".to_string()))
}

/// Get cover page path from an EPUB file.
///
/// Ported from `HydrusArchiveHandling.GetCoverPagePathFromEpub()`.
/// EPUBs specify cover images in their OPF metadata. Supports EPUB 2 and EPUB 3 standards,
/// plus Apple iBooks format.
pub fn get_cover_page_path_from_epub(archive_path: &Path) -> FileResult<String> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open EPUB: {}", e)))?;

    // Step 1: Read META-INF/container.xml to find the rootfile (content.opf path)
    let container_xml = read_zip_entry_string(&mut zip, "META-INF/container.xml")?;
    let container_doc = roxmltree::Document::parse(&container_xml)
        .map_err(|e| FileError::Thumbnail(format!("Could not parse EPUB container.xml: {}", e)))?;

    // Find rootfile element
    let content_opf_path = container_doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .ok_or_else(|| FileError::Thumbnail("EPUB does not declare a rootfile".to_string()))?
        .to_string();

    // Step 2: Read the OPF file and find cover image
    let opf_xml = read_zip_entry_string(&mut zip, &content_opf_path)?;
    let opf_doc = roxmltree::Document::parse(&opf_xml)
        .map_err(|e| FileError::Thumbnail(format!("Could not parse EPUB OPF: {}", e)))?;

    // EPUB 3: look for item with properties="cover-image"
    let mut cover_href: Option<String> = opf_doc
        .descendants()
        .find(|n| n.has_tag_name("item") && n.attribute("properties") == Some("cover-image"))
        .and_then(|n| n.attribute("href").map(String::from));

    // EPUB 2: look for meta name="cover" -> content -> item id
    if cover_href.is_none() {
        if let Some(meta_content) = opf_doc
            .descendants()
            .find(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover"))
            .and_then(|n| n.attribute("content"))
        {
            cover_href = opf_doc
                .descendants()
                .find(|n| n.has_tag_name("item") && n.attribute("id") == Some(meta_content))
                .and_then(|n| n.attribute("href").map(String::from));
        }
    }

    // Fallback: look for item with id="cover"
    if cover_href.is_none() {
        cover_href = opf_doc
            .descendants()
            .find(|n| n.has_tag_name("item") && n.attribute("id") == Some("cover"))
            .and_then(|n| n.attribute("href").map(String::from));
    }

    // Apple iBooks: look for reference type="cover"
    if cover_href.is_none() {
        cover_href = opf_doc
            .descendants()
            .find(|n| n.has_tag_name("reference") && n.attribute("type") == Some("cover"))
            .and_then(|n| n.attribute("href").map(String::from));
    }

    let cover_href = cover_href.ok_or_else(|| {
        FileError::Thumbnail("Sorry, could not find a cover image in the EPUB xml!".to_string())
    })?;

    // Resolve relative path from content.opf directory
    let content_dir = Path::new(&content_opf_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let cover_image_path = if content_dir.is_empty() {
        cover_href
    } else {
        format!("{}/{}", content_dir, cover_href)
    };

    // Verify the cover image exists in the archive
    zip.by_name(&cover_image_path).map_err(|_| {
        FileError::Thumbnail(format!(
            "EPUB declares {}, but this does not exist",
            cover_image_path
        ))
    })?;

    Ok(cover_image_path)
}

/// Extract a single file from a ZIP archive as bytes.
///
/// Ported from `HydrusArchiveHandling.GetSingleFileFromZipBytes()`.
pub fn get_single_file_from_zip_bytes(
    archive_path: &Path,
    path_in_zip: &str,
) -> FileResult<Vec<u8>> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open zip archive: {}", e)))?;

    let mut entry = zip.by_name(path_in_zip).map_err(|e| {
        FileError::UnsupportedFile(format!("Could not find '{}' in zip: {}", path_in_zip, e))
    })?;

    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf).map_err(FileError::Io)?;

    Ok(buf)
}

/// Extract the cover page image bytes from an archive.
///
/// Ported from the cover page extraction logic in `HydrusFileHandling.py`.
/// For EPUBs, uses the OPF metadata to find the cover. For CBZs, uses
/// the first image file sorted alphabetically.
pub fn extract_cover_page(archive_path: &Path, is_epub: bool) -> FileResult<Vec<u8>> {
    let cover_path = if is_epub {
        // Try EPUB-specific cover detection first, fall back to generic
        get_cover_page_path_from_epub(archive_path)
            .or_else(|_| get_cover_page_path(archive_path))?
    } else {
        get_cover_page_path(archive_path)?
    };

    get_single_file_from_zip_bytes(archive_path, &cover_path)
}

/// Get the resolution of an archive file by extracting and measuring its cover page.
///
/// This is the main entry point for getting archive dimensions, used by `files.rs`.
pub fn get_archive_resolution(archive_path: &Path, is_epub: bool) -> FileResult<(u32, u32)> {
    let cover_bytes = extract_cover_page(archive_path, is_epub)?;

    // Use the image crate to get the cover image dimensions
    let reader = image::ImageReader::new(Cursor::new(&cover_bytes))
        .with_guessed_format()
        .map_err(FileError::Io)?;

    let dims = reader.into_dimensions().map_err(|e| {
        FileError::UnsupportedFile(format!("Could not read cover image dimensions: {}", e))
    })?;

    Ok(dims)
}

/// Generate a thumbnail from an archive file's cover page.
///
/// Extracts the cover page and resizes it to the target resolution.
pub fn generate_thumbnail_from_archive(
    archive_path: &Path,
    target_resolution: (u32, u32),
    is_epub: bool,
) -> FileResult<Vec<u8>> {
    let cover_bytes = extract_cover_page(archive_path, is_epub)?;

    // Load cover image
    let reader = image::ImageReader::new(Cursor::new(&cover_bytes))
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let img = reader
        .decode()
        .map_err(|e| FileError::Thumbnail(format!("Could not decode cover image: {}", e)))?;

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

/// Public wrapper for `filename_has_image_ext` for use by other modules.
pub fn filename_has_image_ext_pub(filename: &str) -> bool {
    filename_has_image_ext(filename)
}

/// Check if a filename has an image extension.
///
/// Ported from `HydrusArchiveHandling.filename_has_image_ext()`.
fn filename_has_image_ext(filename: &str) -> bool {
    if let Some(dot_pos) = filename.rfind('.') {
        let ext = &filename[dot_pos..];
        IMAGE_FILE_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
    } else {
        false
    }
}

/// Read a zip entry as a UTF-8 string.
fn read_zip_entry_string(
    zip: &mut zip::ZipArchive<std::fs::File>,
    path: &str,
) -> FileResult<String> {
    let mut entry = zip.by_name(path).map_err(|e| {
        FileError::Thumbnail(format!("Could not find '{}' in archive: {}", path, e))
    })?;

    let mut buf = String::new();
    entry.read_to_string(&mut buf).map_err(FileError::Io)?;

    Ok(buf)
}

// Note: ZipLooksLikeCBZ and ZipLooksLikeUgoira are MIME detection functions
// and belong in the file identification pipeline (files.rs), not here.
// They are omitted because they are called during MIME detection which is
// already ported in files.rs.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filename_has_image_ext() {
        assert!(filename_has_image_ext("page001.jpg"));
        assert!(filename_has_image_ext("cover.PNG"));
        assert!(filename_has_image_ext("art.gif"));
        assert!(!filename_has_image_ext("readme.txt"));
        assert!(!filename_has_image_ext("noext"));
        assert!(!filename_has_image_ext("archive.zip"));
    }
}
