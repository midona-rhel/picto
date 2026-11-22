//! What a drag from another application offers besides files: image data,
//! links, HTML and text, as a web browser drags them, and files it promises
//! (`NSFilePromiseReceiver`), as Photos, Mail, Safari and Messages drag
//! theirs.
use block2::RcBlock;
use gpui::{ExternalDropData, ExternalDropFile, PromisedFile, PromisedFiles};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSArray, NSData, NSString};
use std::path::PathBuf;

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

/// The pasteboard types of a file promise, old and new
/// (`NSFilePromiseReceiver.readableDraggedTypes`).
pub(crate) fn promise_types() -> Vec<Retained<NSString>> {
    // SAFETY: a class method that answers an array of strings.
    unsafe {
        let types: Option<Retained<NSArray<NSString>>> =
            msg_send![class!(NSFilePromiseReceiver), readableDraggedTypes];
        types.map(|types| types.to_vec()).unwrap_or_default()
    }
}

/// Whether `pasteboard` holds a file promise.
pub(crate) fn offers_promised_files(pasteboard: &AnyObject) -> bool {
    let promised = promise_types();
    // SAFETY: NSPasteboard's `types`, an array of strings or nil.
    unsafe {
        let types: Option<Retained<NSArray<NSString>>> = msg_send![pasteboard, types];
        types
            .iter()
            .flat_map(|types| types.iter())
            .any(|kind| promised.iter().any(|wanted| **wanted == *kind))
    }
}

/// A folder no other drop of this process has used, inside `directory`.
fn drop_folder(directory: &std::path::Path) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let folder = directory.join(format!(
        "promised-{now:x}-{:x}-{:x}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// Asks the promises on the dragging pasteboard to write their files into a
/// folder made for this drop. AppKit allows this only while the drop is
/// being performed; the files are written afterwards, each reported on a
/// queue of its own (never the main thread, where a source in this process
/// would wait on itself). `None` when nothing is promised or the folder
/// cannot be made.
pub(crate) fn receive_promised_files(pasteboard: &AnyObject) -> Option<PromisedFiles> {
    // SAFETY: NSPasteboard's, NSFilePromiseReceiver's, NSURL's,
    // NSOperationQueue's and NSError's documented selectors with the
    // argument types they take; each answer is retained or nil.
    unsafe {
        let classes: Retained<NSArray<AnyObject>> =
            msg_send![class!(NSArray), arrayWithObject: class!(NSFilePromiseReceiver)];
        let receivers: Option<Retained<NSArray<AnyObject>>> = msg_send![
            pasteboard,
            readObjectsForClasses: &*classes,
            options: std::ptr::null::<AnyObject>()
        ];
        let receivers = receivers?.to_vec();
        if receivers.is_empty() {
            return None;
        }
        // One file for each type a receiver promises.
        let count: usize = receivers
            .iter()
            .map(|receiver| {
                let types: Option<Retained<NSArray<NSString>>> = msg_send![&**receiver, fileTypes];
                types.map_or(0, |types| types.len()).max(1)
            })
            .sum();
        let folder = drop_folder(&gpui::promised_files_directory())
            .map_err(|error| log::error!("no folder for promised files: {error}"))
            .ok()?;
        let (promised, sender) = PromisedFiles::new(folder.clone(), count);
        let destination: Retained<AnyObject> = msg_send![
            class!(NSURL),
            fileURLWithPath: &*NSString::from_str(&folder.to_string_lossy()),
            isDirectory: objc2::runtime::Bool::YES
        ];
        let options: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
        let queue: Retained<AnyObject> = msg_send![class!(NSOperationQueue), new];
        let reader = RcBlock::new(move |url: *mut AnyObject, error: *mut AnyObject| {
            let report: PromisedFile = match (url.as_ref(), error.as_ref()) {
                (_, Some(error)) => {
                    let text: Option<Retained<NSString>> = msg_send![error, localizedDescription];
                    Err(text.map_or_else(String::new, |text| text.to_string()))
                }
                (Some(url), None) => {
                    let path: Option<Retained<NSString>> = msg_send![url, path];
                    path.map(|path| PathBuf::from(path.to_string()))
                        .ok_or_else(String::new)
                }
                (None, None) => Err(String::new()),
            };
            let _ = sender.unbounded_send(report);
        });
        for receiver in &receivers {
            let () = msg_send![
                &**receiver,
                receivePromisedFilesAtDestination: &*destination,
                options: &*options,
                operationQueue: &*queue,
                reader: &*reader
            ];
        }
        Some(promised)
    }
}
