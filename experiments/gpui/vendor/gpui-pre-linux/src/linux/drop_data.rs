//! What a drag from another application offers besides files, read from an
//! XDND selection or a Wayland data offer by media type: an image's data,
//! its addresses, the page's HTML for it and text, as a web browser drags
//! them.
use gpui::{ExternalDropData, ExternalDropFile};

/// Image types read, in the order preferred.
const IMAGE_TYPES: [&str; 6] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/tiff",
];
/// Addresses.
pub(crate) const URI_LIST: &str = "text/uri-list";
const HTML: &str = "text/html";
/// Plain text, in the order preferred.
const TEXT_TYPES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

/// Whether a drag offering `offered` carries anything [`wanted`] reads.
pub(crate) fn takes(offered: &[String]) -> bool {
    offered.iter().any(|kind| {
        kind == URI_LIST
            || kind == HTML
            || IMAGE_TYPES.contains(&kind.as_str())
            || TEXT_TYPES.contains(&kind.as_str())
    })
}

/// The types read from a drag that brought no files: the first image type
/// offered, the address list, HTML and the first text type offered.
pub fn wanted(offered: &[String]) -> Vec<String> {
    let has = |kind: &str| offered.iter().any(|offer| offer == kind);
    let mut wanted = Vec::new();
    wanted.extend(IMAGE_TYPES.into_iter().find(|kind| has(kind)));
    wanted.extend([URI_LIST, HTML].into_iter().filter(|kind| has(kind)));
    wanted.extend(TEXT_TYPES.into_iter().find(|kind| has(kind)));
    wanted.into_iter().map(str::to_owned).collect()
}

/// Text in UTF-8, or UTF-16 when it starts with a byte order mark (Firefox
/// sends HTML so), up to a terminating nul.
fn text(bytes: &[u8]) -> String {
    let text = match bytes {
        [0xff, 0xfe, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        [0xfe, 0xff, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    match text.find('\0') {
        Some(end) => text[..end].to_owned(),
        None => text,
    }
}

/// What was read, by type, as the application takes it.
pub fn assemble(read: Vec<(String, Vec<u8>)>) -> ExternalDropData {
    let mut data = ExternalDropData::default();
    for (kind, bytes) in read {
        if bytes.is_empty() {
            continue;
        }
        if IMAGE_TYPES.contains(&kind.as_str()) {
            data.files.push(ExternalDropFile {
                bytes,
                name: None,
                media_type: Some(kind),
            });
        } else if kind == URI_LIST {
            data.uri_lists.push(text(&bytes));
        } else if kind == HTML {
            data.html.push(text(&bytes));
        } else if TEXT_TYPES.contains(&kind.as_str()) {
            data.text.push(text(&bytes));
        }
    }
    data
}
