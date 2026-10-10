//! Archive-backed media handling (CBZ and EPUB) — cover page extraction for
//! resolution detection and thumbnail generation.

use std::fs::OpenOptions;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use image::GenericImageView;

use super::{FileError, FileResult};

/// Image file extensions recognized in archives.
const IMAGE_FILE_EXTS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp", ".tiff", ".tif", ".ico",
];

const MAX_ZIP_ENTRIES: usize = 4_096;
const MAX_ZIP_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ZIP_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ZIP_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ZIP_COMPRESSION_RATIO: u64 = 200;
/// An image or other preview read out of an archive (a cover, a frame).
pub(crate) const MAX_PREVIEW_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// A metadata document read out of an archive (container XML, OPF, JSON).
pub(crate) const MAX_METADATA_ENTRY_BYTES: u64 = 4 * 1024 * 1024;
/// Cover discovery looks at no more than this many central-directory entries.
const MAX_PREVIEW_DISCOVERY_ENTRIES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedArchiveEntry {
    pub path: PathBuf,
    pub archive_name: String,
}

/// Extract accepted members from a ZIP into Picto-owned staging. Nested archives
/// are ignored so every expansion remains bounded by this container's limits.
pub fn extract_library_files(
    archive_path: &Path,
    staging_root: &Path,
) -> FileResult<Vec<ExtractedArchiveEntry>> {
    let archive_bytes = std::fs::metadata(archive_path)
        .map_err(FileError::Io)?
        .len();
    if archive_bytes > MAX_ZIP_ARCHIVE_BYTES {
        return Err(unsafe_zip(format!(
            "file exceeds the {MAX_ZIP_ARCHIVE_BYTES}-byte limit"
        )));
    }
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| FileError::UnsupportedFile(format!("Invalid ZIP archive: {error}")))?;
    if zip.len() > MAX_ZIP_ENTRIES {
        return Err(unsafe_zip(format!(
            "contains {} entries; limit is {MAX_ZIP_ENTRIES}",
            zip.len()
        )));
    }

    let output_dir = staging_root.join(format!("{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&output_dir).map_err(FileError::Io)?;
    match extract_zip_members(&mut zip, &output_dir) {
        Ok(entries) if entries.is_empty() => {
            let _ = std::fs::remove_dir(&output_dir);
            Ok(entries)
        }
        Ok(entries) => Ok(entries),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&output_dir);
            Err(error)
        }
    }
}

fn extract_zip_members(
    zip: &mut zip::ZipArchive<std::fs::File>,
    output_dir: &Path,
) -> FileResult<Vec<ExtractedArchiveEntry>> {
    let mut total_bytes = 0_u64;
    let mut extracted = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| {
            FileError::UnsupportedFile(format!("Could not read ZIP entry: {error}"))
        })?;
        if entry.is_dir() {
            continue;
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| unsafe_zip(format!("entry {:?} has an unsafe path", entry.name())))?;
        let extension = enclosed
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        let accepted = extension
            .as_deref()
            .and_then(super::formats::format_for_extension);
        if !accepted.is_some_and(|format| {
            !matches!(
                format.mime_type,
                "application/zip" | "application/vnd.rar" | "application/x-7z-compressed"
            )
        }) {
            continue;
        }

        let declared_size = entry.size();
        let compressed_size = entry.compressed_size();
        if declared_size > MAX_ZIP_ENTRY_BYTES {
            return Err(unsafe_zip(format!(
                "entry {:?} exceeds the {MAX_ZIP_ENTRY_BYTES}-byte limit",
                entry.name()
            )));
        }
        if compressed_size > 0 && declared_size / compressed_size > MAX_ZIP_COMPRESSION_RATIO {
            return Err(unsafe_zip(format!(
                "entry {:?} exceeds the {MAX_ZIP_COMPRESSION_RATIO}:1 compression-ratio limit",
                entry.name()
            )));
        }
        total_bytes = total_bytes
            .checked_add(declared_size)
            .ok_or_else(|| unsafe_zip("declared size overflowed".into()))?;
        if total_bytes > MAX_ZIP_TOTAL_BYTES {
            return Err(unsafe_zip(format!(
                "accepted entries exceed the {MAX_ZIP_TOTAL_BYTES}-byte total limit"
            )));
        }

        let extension = extension.expect("accepted member has an extension");
        let output_path = output_dir.join(format!("{index:04}.{extension}"));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(FileError::Io)?;
        let copied = std::io::copy(
            &mut entry.by_ref().take(MAX_ZIP_ENTRY_BYTES + 1),
            &mut output,
        )
        .map_err(FileError::Io)?;
        output.flush().map_err(FileError::Io)?;
        if copied > MAX_ZIP_ENTRY_BYTES {
            return Err(unsafe_zip(format!(
                "entry {:?} exceeded its extraction limit",
                entry.name()
            )));
        }
        extracted.push(ExtractedArchiveEntry {
            path: output_path,
            archive_name: enclosed.to_string_lossy().into_owned(),
        });
    }
    Ok(extracted)
}

fn unsafe_zip(reason: String) -> FileError {
    FileError::UnsupportedFile(format!("Unsafe ZIP archive: {reason}"))
}

/// Read one named entry of `zip` without ever holding more than `limit`
/// bytes. The declared size and compression ratio are checked before
/// allocating, and the bytes actually inflated are checked again, so a forged
/// size in the archive's metadata cannot get past the budget.
pub(crate) fn read_entry_bounded<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> FileResult<Vec<u8>> {
    let mut entry = zip.by_name(name).map_err(|error| {
        FileError::UnsupportedFile(format!("Could not find '{name}' in archive: {error}"))
    })?;
    let declared = entry.size();
    let compressed = entry.compressed_size();
    if declared > limit {
        return Err(unsafe_zip(format!(
            "entry {name:?} declares {declared} bytes; the limit is {limit}"
        )));
    }
    let too_compressed =
        |inflated: u64| inflated > 0 && (compressed == 0 || inflated / compressed > MAX_ZIP_COMPRESSION_RATIO);
    if too_compressed(declared) {
        return Err(unsafe_zip(format!(
            "entry {name:?} exceeds the {MAX_ZIP_COMPRESSION_RATIO}:1 compression-ratio limit"
        )));
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    entry
        .by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(FileError::Io)?;
    let inflated = bytes.len() as u64;
    if inflated > limit || inflated > declared || too_compressed(inflated) {
        return Err(unsafe_zip(format!("entry {name:?} inflated past its declared size")));
    }
    Ok(bytes)
}

/// [`read_entry_bounded`] for a UTF-8 metadata document.
pub(crate) fn read_entry_text_bounded<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> FileResult<String> {
    String::from_utf8(read_entry_bounded(zip, name, MAX_METADATA_ENTRY_BYTES)?)
        .map_err(|_| FileError::UnsupportedFile(format!("Archive entry {name:?} is not UTF-8")))
}

/// Get the path to the cover page (first image) inside a ZIP archive.
///
/// Finds the first image file in the archive, sorted by filename.
pub fn get_cover_page_path(archive_path: &Path) -> FileResult<String> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open zip archive: {}", e)))?;

    if zip.len() > MAX_PREVIEW_DISCOVERY_ENTRIES {
        return Err(unsafe_zip(format!(
            "contains {} entries; cover discovery reads at most {MAX_PREVIEW_DISCOVERY_ENTRIES}",
            zip.len()
        )));
    }
    let mut image_paths: Vec<String> = Vec::new();

    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| FileError::UnsupportedFile(format!("Could not read zip entry: {}", e)))?;

        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();

        // Skip macOS resource fork files
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
pub fn get_single_file_from_zip_bytes(
    archive_path: &Path,
    path_in_zip: &str,
) -> FileResult<Vec<u8>> {
    let file = std::fs::File::open(archive_path).map_err(FileError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not open zip archive: {}", e)))?;

    read_entry_bounded(&mut zip, path_in_zip, MAX_PREVIEW_ENTRY_BYTES)
}

/// Extract the cover page image bytes from an archive.
///
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
    read_entry_text_bounded(zip, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_extraction_rejects_zip_larger_than_one_gib_before_opening_it() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("oversized.zip");
        let file = std::fs::File::create(&archive).unwrap();
        file.set_len(MAX_ZIP_ARCHIVE_BYTES + 1).unwrap();

        let error = extract_library_files(&archive, &directory.path().join("staging")).unwrap_err();

        assert!(error.to_string().contains("1073741824-byte limit"));
        assert!(!directory.path().join("staging").exists());
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 40]))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// Incompressible filler, so only the size limit can reject it.
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x2545_f491_u32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    #[test]
    fn ordinary_cbz_and_epub_covers_still_preview() {
        let directory = tempfile::tempdir().unwrap();
        let cbz = directory.path().join("book.cbz");
        write_zip(&cbz, &[("pages/002.png", &png(3, 3)), ("pages/001.png", &png(4, 2))]);
        assert_eq!(get_archive_resolution(&cbz, false).unwrap(), (4, 2));

        let epub = directory.path().join("book.epub");
        let container = br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;
        let opf = br#"<?xml version="1.0"?><package><manifest><item id="c" href="cover.png" properties="cover-image"/></manifest></package>"#;
        write_zip(
            &epub,
            &[
                ("META-INF/container.xml", container),
                ("OEBPS/content.opf", opf),
                ("OEBPS/cover.png", &png(5, 7)),
            ],
        );
        assert_eq!(get_archive_resolution(&epub, true).unwrap(), (5, 7));
    }

    #[test]
    fn previews_reject_high_ratio_and_oversized_entries_before_reading_them() {
        let directory = tempfile::tempdir().unwrap();
        // 8 MiB of zeros deflates to a few KiB: far past the ratio limit.
        let bomb = directory.path().join("bomb.cbz");
        write_zip(&bomb, &[("001.png", &vec![0u8; 8 * 1024 * 1024])]);
        let error = extract_cover_page(&bomb, false).unwrap_err().to_string();
        assert!(error.contains("compression-ratio"), "{error}");

        // An incompressible metadata document over its own, smaller budget.
        let epub = directory.path().join("huge.epub");
        let oversized = noise(MAX_METADATA_ENTRY_BYTES as usize + 1);
        write_zip(&epub, &[("META-INF/container.xml", &oversized), ("cover.png", &png(2, 2))]);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&epub).unwrap()).unwrap();
        let error = read_entry_text_bounded(&mut zip, "META-INF/container.xml")
            .unwrap_err()
            .to_string();
        assert!(error.contains("limit"), "{error}");
        // The EPUB still previews through the generic first-image fallback.
        assert_eq!(get_archive_resolution(&epub, true).unwrap(), (2, 2));
    }
}

// Note: ZipLooksLikeCBZ and ZipLooksLikeUgoira are MIME detection functions
// and belong in the file identification pipeline (files.rs), not here.
// They are omitted because they are called during MIME detection which is
// already ported in files.rs.
