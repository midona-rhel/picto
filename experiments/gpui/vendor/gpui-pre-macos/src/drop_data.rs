//! What a drag from another application offers besides files: image data,
//! links, HTML and text, as a web browser drags them.
use gpui::{ExternalDropData, ExternalDropFile};
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSArray, NSData, NSString};

/// Image types read from a dragged item, in the order preferred, with the
/// media type each is.
const IMAGE_TYPES: &[(&str, &str)] = &[
    ("public.png", "image/png"),
    ("public.jpeg", "image/jpeg"),
    ("com.compuserve.gif", "image/gif"),
    ("org.webmproject.webp", "image/webp"),
    ("public.tiff", "image/tiff"),
];
const URL: &str = "public.url";
const HTML: &str = "public.html";
const TEXT: &str = "public.utf8-plain-text";
const FILE_URL: &str = "public.file-url";

/// The types a window takes dragged over it besides files.
pub(crate) fn dragged_types() -> Vec<Retained<NSString>> {
    IMAGE_TYPES
        .iter()
        .map(|(kind, _)| *kind)
        .chain([URL, HTML, TEXT])
        .map(NSString::from_str)
        .collect()
}

/// Whether `pasteboard` holds anything [`read_drop_pasteboard`] reads.
pub(crate) fn offers_drop_data(pasteboard: &AnyObject) -> bool {
    // SAFETY: NSPasteboard's `types`, an array of strings or nil.
    let types: Vec<String> = unsafe {
        let types: Option<Retained<NSArray<NSString>>> = msg_send![pasteboard, types];
        types
            .iter()
            .flat_map(|types| types.iter())
            .map(|kind| kind.to_string())
            .collect()
    };
    dragged_types()
        .iter()
        .any(|wanted| types.iter().any(|kind| *kind == wanted.to_string()))
}

/// What `pasteboard` offers item by item: an item's image data (not for an
/// item that is a file, whose path the drop already has), its link, HTML
/// and text.
pub fn read_drop_pasteboard(pasteboard: &AnyObject) -> ExternalDropData {
    let mut data = ExternalDropData::default();
    // SAFETY: NSPasteboard's and NSPasteboardItem's documented selectors,
    // with the argument types they take; each answer is retained or nil.
    unsafe {
        let items: Option<Retained<NSArray<AnyObject>>> = msg_send![pasteboard, pasteboardItems];
        for item in items.iter().flat_map(|items| items.iter()) {
            let types: Option<Retained<NSArray<NSString>>> = msg_send![&*item, types];
            let types: Vec<String> = types
                .iter()
                .flat_map(|types| types.iter())
                .map(|kind| kind.to_string())
                .collect();
            let has = |kind: &str| types.iter().any(|offered| offered == kind);
            if !has(FILE_URL)
                && let Some((kind, media_type)) = IMAGE_TYPES.iter().find(|(kind, _)| has(kind))
            {
                let bytes: Option<Retained<NSData>> =
                    msg_send![&*item, dataForType: &*NSString::from_str(kind)];
                if let Some(bytes) = bytes {
                    data.files.push(ExternalDropFile {
                        bytes: bytes.to_vec(),
                        name: None,
                        media_type: Some((*media_type).to_owned()),
                    });
                }
            }
            let text = |kind: &str| -> Option<String> {
                if !has(kind) {
                    return None;
                }
                let value: Option<Retained<NSString>> =
                    msg_send![&*item, stringForType: &*NSString::from_str(kind)];
                value.map(|value| value.to_string())
            };
            if !has(FILE_URL) {
                data.uri_lists.extend(text(URL));
            }
            data.html.extend(text(HTML));
            data.text.extend(text(TEXT));
        }
    }
    data
}
