//! What a drop from a web browser or another application carries besides
//! files on disk, and how production imports it: `preload.cjs`'s `drop`
//! handler reads the drag's in-memory files and its addresses, and
//! `dropImport.mjs` (`picto:drop:materialize`) writes each in-memory file
//! that is an image to a temporary file, which the import deletes once it
//! has it, with the drop's web addresses as its sources.
//!
//! As in production, an address is never fetched: a drop that brings no
//! file is refused ("Links cannot be imported"). Production fetched dropped
//! links until it chose not to ("Reject link-only browser drops").
use gpui::{ExternalDropData, ExternalDropFile};
use std::path::{Path, PathBuf};

// Production's main process writes these in English, without a key.
/// Production's error for a drop that holds no file.
pub const LINKS_ONLY: &str =
    crate::i18n::en("Links cannot be imported. Drop an image file instead.");
/// Production's error when no in-memory file of the drop could be written.
pub const NO_IMAGE: &str =
    crate::i18n::en("The dropped browser item did not contain an importable image.");
const NOT_AN_IMAGE: &str =
    crate::i18n::en("The dropped browser item did not contain valid image data.");
const EMPTY: &str = crate::i18n::en("The dropped browser item did not contain media data.");

/// `readUrls`: the web addresses the drop names, in production's order (a
/// `DownloadURL`'s address, the dragged image's `src`, the address list,
/// then the text), trimmed, each once, `http` and `https` only.
pub fn source_urls(data: &ExternalDropData) -> Vec<String> {
    let mut candidates = Vec::new();
    for download in &data.download_urls {
        candidates.push(download.splitn(3, ':').nth(2).unwrap_or("").to_owned());
    }
    for html in &data.html {
        if let Some(source) = image_source(html) {
            candidates.push(source.replace("&amp;", "&"));
        }
    }
    for value in data.uri_lists.iter().chain(&data.text) {
        candidates.extend(value.lines().map(str::to_owned));
    }
    let mut urls: Vec<String> = Vec::new();
    for candidate in candidates {
        let candidate = candidate.trim();
        let lower = candidate.to_ascii_lowercase();
        // i18n: schemes, not shown
        let web = lower.starts_with("http://") || lower.starts_with("https://");
        if web && !urls.iter().any(|url| url == candidate) {
            urls.push(candidate.to_owned());
        }
    }
    urls
}

/// The `src` of the first `<img>` in `html` that has one
/// (`/<img[^>]+src=["']([^"']+)/i`).
fn image_source(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("<img") {
        let tag_start = from + found + 4;
        let tag_end = lower[tag_start..]
            .find('>')
            .map_or(lower.len(), |end| tag_start + end);
        let tag = &lower[tag_start..tag_end];
        if let Some(at) = tag.find("src=") {
            let value_start = tag_start + at + 4;
            let quote = html[value_start..].chars().next()?;
            if quote == '"' || quote == '\'' {
                let rest = &html[value_start + 1..];
                let end = rest.find(['"', '\'']).unwrap_or(rest.len());
                if end > 0 {
                    return Some(&rest[..end]);
                }
            }
        }
        from = tag_start;
    }
    None
}

/// `detectImageMimeType`: the image type the bytes are, by their start.
pub fn image_type(bytes: &[u8]) -> Option<&'static str> {
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    if starts(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if starts(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return Some("image/gif");
    }
    if starts(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if starts(b"BM") {
        return Some("image/bmp");
    }
    if starts(&[0x49, 0x49, 0x2a, 0x00]) || starts(&[0x4d, 0x4d, 0x00, 0x2a]) {
        return Some("image/tiff");
    }
    if starts(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    if bytes.get(4..8) == Some(b"ftyp") && matches!(bytes.get(8..12), Some(b"avif" | b"avis")) {
        return Some("image/avif");
    }
    // `/^(?:﻿|\s)*(?:<\?xml[^>]*>\s*)?<svg\b/i` over the first 1,024
    // bytes.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    let mut text = head
        .trim_start_matches(|character: char| character == '\u{feff}' || character.is_whitespace());
    if let Some(declaration) = text.strip_prefix("<?xml") {
        text = match declaration.find('>') {
            Some(end) => declaration[end + 1..].trim_start(),
            None => return None,
        };
    }
    let lower = text.get(..4).map(str::to_ascii_lowercase);
    let boundary = text
        .get(4..)
        .and_then(|rest| rest.chars().next())
        .is_none_or(|next| !next.is_alphanumeric() && next != '_');
    (lower.as_deref() == Some("<svg") && boundary).then_some("image/svg+xml")
}

/// `safeExtension`: the name's own extension when it is an image's, else
/// the detected type's.
fn extension(name: Option<&str>, mime: &str) -> &'static str {
    const KNOWN: &[(&str, &str)] = &[
        ("image/avif", ".avif"),
        ("image/bmp", ".bmp"),
        ("image/gif", ".gif"),
        ("image/x-icon", ".ico"),
        ("image/vnd.microsoft.icon", ".ico"),
        ("image/jpeg", ".jpg"),
        ("image/png", ".png"),
        ("image/svg+xml", ".svg"),
        ("image/tiff", ".tiff"),
        ("image/webp", ".webp"),
    ];
    let own = name
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()));
    if let Some(own) = own {
        let images = KNOWN
            .iter()
            .map(|(_, extension)| *extension)
            .chain([".jpeg", ".tif"]);
        if let Some(found) = images.into_iter().find(|known| *known == own) {
            return found;
        }
    }
    KNOWN
        .iter()
        .find(|(known, _)| *known == mime)
        .map_or(".bin", |(_, extension)| extension)
}

/// A name no other staged file of this process has used.
fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!(
        "{now:x}-{:x}-{:x}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    )
}

/// `materializeDroppedMedia`: writes an in-memory file to `directory` as
/// `<id><extension>` when its bytes are an image, and refuses it otherwise.
pub fn materialize(file: &ExternalDropFile, directory: &Path) -> Result<PathBuf, String> {
    if file.bytes.is_empty() {
        return Err(EMPTY.into());
    }
    let mime = image_type(&file.bytes).ok_or(NOT_AN_IMAGE)?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!(
        "{}{}",
        unique_id(),
        extension(file.name.as_deref(), mime)
    ));
    std::fs::write(&path, &file.bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

/// What a drop comes to, before anything is imported.
#[derive(Debug, PartialEq)]
pub enum Staged {
    /// Files on disk only: imported as dropped files are.
    Paths(Vec<PathBuf>),
    /// Production's notification "Could not import dropped media", with
    /// this message.
    Refused(String),
    /// Files on disk, and the in-memory files written to `staged` (deleted
    /// by the import once it has them), which take `sources`.
    Browser {
        paths: Vec<PathBuf>,
        staged: Vec<PathBuf>,
        sources: Vec<String>,
    },
}

/// The drop handler's rules: a drop with no file is refused; each
/// in-memory file is written to `directory` (one that is not an image is
/// skipped, and when none could be written the last error is given).
pub fn stage(paths: Vec<PathBuf>, data: Option<&ExternalDropData>, directory: &Path) -> Staged {
    let files = data.map_or(&[][..], |data| &data.files[..]);
    if paths.is_empty() && files.is_empty() {
        return Staged::Refused(LINKS_ONLY.into());
    }
    if files.is_empty() {
        return Staged::Paths(paths);
    }
    let mut staged = Vec::new();
    let mut last_error = None;
    for file in files {
        match materialize(file, directory) {
            Ok(path) => staged.push(path),
            Err(error) => last_error = Some(error),
        }
    }
    if paths.is_empty() && staged.is_empty() {
        return Staged::Refused(last_error.unwrap_or_else(|| NO_IMAGE.into()));
    }
    Staged::Browser {
        paths,
        staged,
        sources: data.map(source_urls).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];

    fn file(bytes: &[u8], name: Option<&str>) -> ExternalDropFile {
        ExternalDropFile {
            bytes: bytes.to_vec(),
            name: name.map(str::to_owned),
            media_type: None,
        }
    }

    #[test]
    fn image_data_is_known_by_its_start() {
        assert_eq!(image_type(PNG), Some("image/png"));
        assert_eq!(image_type(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(image_type(b"GIF89a...."), Some("image/gif"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_type(b"BM\0\0"), Some("image/bmp"));
        assert_eq!(image_type(&[0x4d, 0x4d, 0x00, 0x2a, 0]), Some("image/tiff"));
        assert_eq!(image_type(&[0, 0, 1, 0, 1]), Some("image/x-icon"));
        assert_eq!(image_type(b"\0\0\0\x1cftypavif"), Some("image/avif"));
        assert_eq!(
            image_type(b"\xef\xbb\xbf <?xml version=\"1.0\"?>\n<svg xmlns=\"\">"),
            Some("image/svg+xml")
        );
        assert_eq!(image_type(b"<SVG>"), Some("image/svg+xml"));
        assert_eq!(image_type(b"<svgfoo>"), None);
        assert_eq!(image_type(b"<html>not an image</html>"), None);
    }

    #[test]
    fn a_file_keeps_an_image_extension_of_its_name_else_takes_its_type() {
        assert_eq!(extension(Some("photo.PNG"), "image/png"), ".png");
        assert_eq!(extension(Some("photo.jpeg"), "image/jpeg"), ".jpeg");
        assert_eq!(extension(Some("page.html"), "image/webp"), ".webp");
        assert_eq!(extension(None, "image/tiff"), ".tiff");
        assert_eq!(extension(None, "application/x-thing"), ".bin");
    }

    #[test]
    fn source_addresses_are_the_web_links_of_the_drop_once_each() {
        let data = ExternalDropData {
            download_urls: vec!["image/png:a.png:https://e.test/dl.png".into()],
            html: vec![
                r#"<a href="x"><IMG alt='' SRC="https://e.test/a.png?x=1&amp;y=2"></a>"#.into(),
            ],
            uri_lists: vec!["https://e.test/page\r\nfile:///tmp/a.png".into()],
            text: vec![" https://e.test/page \nftp://e.test/b".into()],
            ..Default::default()
        };
        assert_eq!(
            source_urls(&data),
            vec![
                "https://e.test/dl.png",
                "https://e.test/a.png?x=1&y=2",
                "https://e.test/page"
            ]
        );
        assert_eq!(image_source("<img alt=x>"), None);
        assert_eq!(image_source("<p><img src='/a.gif'>"), Some("/a.gif"));
    }

    #[test]
    fn a_drop_without_files_is_refused_and_never_fetched() {
        let directory = tempfile::tempdir().expect("folder");
        let links = ExternalDropData {
            uri_lists: vec!["https://e.test/a.png".into()],
            ..Default::default()
        };
        assert_eq!(
            stage(Vec::new(), Some(&links), directory.path()),
            Staged::Refused(LINKS_ONLY.into())
        );
        assert_eq!(
            stage(Vec::new(), None, directory.path()),
            Staged::Refused(LINKS_ONLY.into())
        );
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            0,
            "nothing was written"
        );
    }

    #[test]
    fn image_data_is_staged_with_the_drops_addresses_and_other_data_refused() {
        let directory = tempfile::tempdir().expect("folder");
        let data = ExternalDropData {
            files: vec![
                file(PNG, Some("photo.png")),
                file(b"<html>not an image</html>", Some("page.webp")),
            ],
            uri_lists: vec!["https://e.test/photo.png".into()],
            ..Default::default()
        };
        let Staged::Browser {
            paths,
            staged,
            sources,
        } = stage(Vec::new(), Some(&data), directory.path())
        else {
            panic!("not staged");
        };
        assert!(paths.is_empty());
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].parent(), Some(directory.path()));
        assert_eq!(staged[0].extension().unwrap(), "png");
        assert_eq!(std::fs::read(&staged[0]).unwrap(), PNG);
        assert_eq!(sources, vec!["https://e.test/photo.png"]);

        let refused = ExternalDropData {
            files: vec![file(b"", None), file(b"<html>", None)],
            ..Default::default()
        };
        assert_eq!(
            stage(Vec::new(), Some(&refused), directory.path()),
            Staged::Refused(NOT_AN_IMAGE.into())
        );
    }

    /// The macOS platform's reading of a drag, on a pasteboard of the
    /// test's own (never the system's drag or general pasteboard): a
    /// browser's dragged image gives its data, address and HTML; a dragged
    /// file gives no data, since its path arrives with the drop.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_drag_pasteboard_gives_image_data_and_addresses() {
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject, Bool};
        use objc2_foundation::{NSArray, NSData, NSString};
        let class = AnyClass::get(c"NSPasteboard").expect("AppKit");
        let item_class = AnyClass::get(c"NSPasteboardItem").expect("AppKit");
        // SAFETY: NSPasteboard's and NSPasteboardItem's documented
        // selectors, with the argument types they take.
        unsafe {
            let board: Retained<AnyObject> = msg_send![class, pasteboardWithUniqueName];
            let _: isize = msg_send![&*board, clearContents];
            let image: Retained<AnyObject> = msg_send![item_class, new];
            let set_data = |item: &AnyObject, bytes: &[u8], kind: &str| {
                let data = NSData::with_bytes(bytes);
                let wrote: Bool =
                    msg_send![item, setData: &*data, forType: &*NSString::from_str(kind)];
                assert!(wrote.as_bool());
            };
            let set_text = |item: &AnyObject, text: &str, kind: &str| {
                let wrote: Bool = msg_send![
                    item,
                    setString: &*NSString::from_str(text),
                    forType: &*NSString::from_str(kind)
                ];
                assert!(wrote.as_bool());
            };
            set_data(&image, PNG, "public.png");
            set_text(&image, "https://e.test/a.png", "public.url");
            set_text(&image, "<img src=\"https://e.test/a.png\">", "public.html");
            let file: Retained<AnyObject> = msg_send![item_class, new];
            set_text(&file, "file:///tmp/b.png", "public.file-url");
            set_data(&file, PNG, "public.png");
            let items = NSArray::from_retained_slice(&[image, file]);
            let wrote: Bool = msg_send![&*board, writeObjects: &*items];
            assert!(wrote.as_bool());
            let data = gpui_macos::read_drop_pasteboard(&board);
            let _: () = msg_send![&*board, releaseGlobally];
            assert_eq!(data.files.len(), 1, "the file's own data is not read");
            assert_eq!(data.files[0].bytes, PNG);
            assert_eq!(data.files[0].media_type.as_deref(), Some("image/png"));
            assert_eq!(source_urls(&data), vec!["https://e.test/a.png"]);
            let directory = tempfile::tempdir().expect("folder");
            assert!(matches!(
                stage(Vec::new(), Some(&data), directory.path()),
                Staged::Browser { staged, .. } if staged.len() == 1
            ));
        }
    }

    /// The Windows platform's reading of a browser's virtual files and
    /// `HTML Format`, on data made here.
    #[cfg(windows)]
    #[test]
    fn the_windows_drag_gives_virtual_file_names_and_the_html_fragment() {
        let mut group = 2u32.to_le_bytes().to_vec();
        for (name, attributes) in [("photo.png", 0u32), ("folder", 0x10)] {
            let mut descriptor = vec![0u8; 592];
            descriptor[0..4].copy_from_slice(&0x4u32.to_le_bytes());
            descriptor[36..40].copy_from_slice(&attributes.to_le_bytes());
            for (index, unit) in name.encode_utf16().enumerate() {
                descriptor[72 + index * 2..74 + index * 2].copy_from_slice(&unit.to_le_bytes());
            }
            group.extend(descriptor);
        }
        assert_eq!(
            gpui_windows::file_group_names(&group),
            vec![("photo.png".to_owned(), false), ("folder".to_owned(), true)]
        );
        let fragment = "<img src=\"https://e.test/a.png\">";
        let prefix = "<html><body><!--StartFragment-->";
        let header_length = 105;
        let start = header_length + prefix.len();
        let end = start + fragment.len();
        let header = format!(
            "Version:0.9\r\nStartHTML:{header_length:010}\r\nEndHTML:{:010}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\n",
            end + 32
        );
        assert_eq!(header.len(), header_length);
        let cf_html = format!("{header}{prefix}{fragment}<!--EndFragment--></body></html>");
        assert_eq!(gpui_windows::html_fragment(&cf_html), fragment);
        let data = ExternalDropData {
            html: vec![gpui_windows::html_fragment(&cf_html)],
            ..Default::default()
        };
        assert_eq!(source_urls(&data), vec!["https://e.test/a.png"]);
    }

    /// The X11 and Wayland platforms' choice of what to read from a drag
    /// and what it comes to, on data made here (Firefox's HTML is UTF-16).
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_drag_reads_one_image_the_addresses_html_and_text() {
        let offered: Vec<String> = [
            "text/x-moz-url",
            "text/html",
            "image/jpeg",
            "image/png",
            "text/uri-list",
            "text/plain",
            "UTF8_STRING",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            gpui_linux::wanted_drop_types(&offered),
            vec!["image/png", "text/uri-list", "text/html", "UTF8_STRING"]
        );
        let html: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(
                "<img src='https://e.test/a.png'>"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        let data = gpui_linux::assemble_drop_data(vec![
            ("image/png".into(), PNG.to_vec()),
            ("text/uri-list".into(), b"https://e.test/page\r\n".to_vec()),
            ("text/html".into(), html),
            ("UTF8_STRING".into(), Vec::new()),
        ]);
        assert_eq!(data.files.len(), 1);
        assert_eq!(data.files[0].media_type.as_deref(), Some("image/png"));
        assert!(data.text.is_empty());
        assert_eq!(
            source_urls(&data),
            vec!["https://e.test/a.png", "https://e.test/page"]
        );
    }

    #[test]
    fn files_on_disk_are_dropped_as_before() {
        let directory = tempfile::tempdir().expect("folder");
        let paths = vec![PathBuf::from("/a.png")];
        assert_eq!(
            stage(
                paths.clone(),
                Some(&ExternalDropData::default()),
                directory.path()
            ),
            Staged::Paths(paths.clone())
        );
        assert_eq!(
            stage(paths.clone(), None, directory.path()),
            Staged::Paths(paths)
        );
    }
}
