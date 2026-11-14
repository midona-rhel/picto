//! What a drag from another application offers besides files (`CF_HDROP`):
//! a browser's dragged image as a virtual file (`FileGroupDescriptorW` with
//! `FileContents`) or as image data (`PNG`, `CF_DIBV5`, `CF_DIB`), its
//! address (`UniformResourceLocatorW`), the page's HTML for it
//! (`HTML Format`) and text.
use std::sync::LazyLock;

use gpui::{ExternalDropData, ExternalDropFile};
use windows::Win32::Foundation::S_OK;
use windows::Win32::System::{
    Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL, TYMED_ISTREAM},
    DataExchange::RegisterClipboardFormatW,
    Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, ReleaseStgMedium},
};
use windows::core::{PCWSTR, w};

/// The most one in-memory file may hold; a larger one is left out.
const FILE_LIMIT: usize = 1 << 30;
/// `FILE_ATTRIBUTE_DIRECTORY`, and `FD_ATTRIBUTES` (the attributes field
/// is valid).
const DIRECTORY: u32 = 0x10;
const FD_ATTRIBUTES: u32 = 0x4;
/// The size of one `FILEDESCRIPTORW`.
const DESCRIPTOR: usize = 592;

fn format(name: PCWSTR) -> u16 {
    // SAFETY: a nul-terminated wide string; 0 (failure) matches nothing.
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

static PNG: LazyLock<u16> = LazyLock::new(|| format(w!("PNG")));
static URL_WIDE: LazyLock<u16> = LazyLock::new(|| format(w!("UniformResourceLocatorW")));
static URL: LazyLock<u16> = LazyLock::new(|| format(w!("UniformResourceLocator")));
static HTML: LazyLock<u16> = LazyLock::new(|| format(w!("HTML Format")));
static FILE_GROUP: LazyLock<u16> = LazyLock::new(|| format(w!("FileGroupDescriptorW")));
static FILE_CONTENTS: LazyLock<u16> = LazyLock::new(|| format(w!("FileContents")));

fn etc(format: u16, lindex: i32, tymed: u32) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex,
        tymed,
    }
}

fn offers(data: &IDataObject, format: u16) -> bool {
    format != 0 && unsafe { data.QueryGetData(&etc(format, -1, TYMED_HGLOBAL.0 as u32)) } == S_OK
}

/// Whether a drag with no files offers anything [`read`] reads.
pub(crate) fn offers_drop_data(data: &IDataObject) -> bool {
    [
        *FILE_GROUP,
        *PNG,
        CF_DIBV5.0,
        CF_DIB.0,
        *URL_WIDE,
        *URL,
        *HTML,
        CF_UNICODETEXT.0,
    ]
    .into_iter()
    .any(|format| offers(data, format))
}

/// The bytes of `format` (item `lindex`), from global memory or a stream.
fn bytes(data: &IDataObject, format: u16, lindex: i32) -> Option<Vec<u8>> {
    if format == 0 {
        return None;
    }
    let wanted = etc(format, lindex, (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32);
    // SAFETY: `GetData` hands over a medium the caller releases with
    // `ReleaseStgMedium`, which every path below does once; the union
    // field read is the one `tymed` names.
    unsafe {
        let mut medium = data.GetData(&wanted).ok()?;
        let read = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            crate::clipboard::LockedGlobal::lock(medium.u.hGlobal)
                .map(|locked| locked.as_bytes().to_vec())
                .filter(|bytes| bytes.len() <= FILE_LIMIT)
        } else if medium.tymed == TYMED_ISTREAM.0 as u32 {
            (*medium.u.pstm).as_ref().and_then(|stream| {
                let mut all = Vec::new();
                let mut chunk = vec![0u8; 1 << 16];
                loop {
                    let mut got = 0u32;
                    let result = stream.Read(
                        chunk.as_mut_ptr().cast(),
                        chunk.len() as u32,
                        Some(&mut got as *mut u32),
                    );
                    if result.is_err() {
                        return None;
                    }
                    if got == 0 {
                        return Some(all);
                    }
                    all.extend_from_slice(&chunk[..got as usize]);
                    if all.len() > FILE_LIMIT {
                        return None;
                    }
                }
            })
        } else {
            None
        };
        ReleaseStgMedium(&mut medium);
        read
    }
}

/// UTF-16 text up to its terminating nul.
fn wide_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Narrow text up to its terminating nul.
fn narrow_text(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The names and attributes of a `FILEGROUPDESCRIPTORW`'s files, in order.
pub fn file_group_names(group: &[u8]) -> Vec<(String, bool)> {
    let Some(count) = group
        .get(..4)
        .map(|count| u32::from_le_bytes(count.try_into().unwrap()))
    else {
        return Vec::new();
    };
    (0..count as usize)
        .map_while(|index| {
            let descriptor = group.get(4 + index * DESCRIPTOR..4 + (index + 1) * DESCRIPTOR)?;
            let flags = u32::from_le_bytes(descriptor[0..4].try_into().unwrap());
            let attributes = u32::from_le_bytes(descriptor[36..40].try_into().unwrap());
            let directory = flags & FD_ATTRIBUTES != 0 && attributes & DIRECTORY != 0;
            Some((wide_text(&descriptor[72..592]), directory))
        })
        .collect()
}

/// The fragment `CF_HTML` marks (`StartFragment`/`EndFragment` byte
/// offsets), else its HTML from `StartHTML`, else all of it.
pub fn html_fragment(cf_html: &str) -> String {
    let offset = |name: &str| -> Option<usize> {
        let at = cf_html.find(name)? + name.len();
        let digits: String = cf_html[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    let bytes = cf_html.as_bytes();
    let slice = |start: Option<usize>, end: Option<usize>| -> Option<String> {
        let (start, end) = (start?, end.unwrap_or(bytes.len()));
        let text = bytes.get(start..end.min(bytes.len()))?;
        Some(String::from_utf8_lossy(text).into_owned())
    };
    slice(offset("StartFragment:"), offset("EndFragment:"))
        .or_else(|| slice(offset("StartHTML:"), offset("EndHTML:")))
        .unwrap_or_else(|| cf_html.to_owned())
}

/// What `data` offers besides files: its virtual files (else one image's
/// data), its address, HTML and text. Nothing of a drag of files.
pub(crate) fn read(data: &IDataObject) -> ExternalDropData {
    let mut dropped = ExternalDropData::default();
    if offers(data, CF_HDROP.0) {
        return dropped;
    }
    if let Some(group) = bytes(data, *FILE_GROUP, -1) {
        for (index, (name, directory)) in file_group_names(&group).into_iter().enumerate() {
            if directory {
                continue;
            }
            if let Some(contents) = bytes(data, *FILE_CONTENTS, index as i32) {
                dropped.files.push(ExternalDropFile {
                    bytes: contents,
                    name: Some(name),
                    media_type: None,
                });
            }
        }
    }
    if dropped.files.is_empty() {
        let image = bytes(data, *PNG, -1)
            .map(|png| (png, "image/png"))
            .or_else(|| {
                [CF_DIBV5.0, CF_DIB.0].into_iter().find_map(|format| {
                    let dib = bytes(data, format, -1)?;
                    Some((crate::clipboard::convert_dib_to_bmp(&dib)?, "image/bmp"))
                })
            });
        if let Some((bytes, media_type)) = image {
            dropped.files.push(ExternalDropFile {
                bytes,
                name: None,
                media_type: Some(media_type.to_owned()),
            });
        }
    }
    let address = bytes(data, *URL_WIDE, -1)
        .map(|bytes| wide_text(&bytes))
        .or_else(|| bytes(data, *URL, -1).map(|bytes| narrow_text(&bytes)));
    dropped
        .uri_lists
        .extend(address.filter(|address| !address.is_empty()));
    if let Some(html) = bytes(data, *HTML, -1) {
        dropped.html.push(html_fragment(&narrow_text(&html)));
    }
    if let Some(text) = bytes(data, CF_UNICODETEXT.0, -1) {
        dropped.text.push(wide_text(&text));
    }
    dropped
}
