//! The dock tile's badge, the user notification centre, the pasteboard
//! and Launch Services.
use super::Application;
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::TCFType;
use core_foundation::url::{CFURL, CFURLRef};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSString, NSURL};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSCopyApplicationURLsForURL(url: CFURLRef, roles: u32) -> CFArrayRef;
    fn LSCopyDefaultApplicationURLForURL(
        url: CFURLRef,
        roles: u32,
        error: *mut *const std::ffi::c_void,
    ) -> CFURLRef;
}

/// `kLSRolesAll`.
const ALL_ROLES: u32 = 0xFFFF_FFFF;
/// Production's own bundle, which never offers itself.
const PICTO_BUNDLE: &str = "com.picto.desktop";

/// Writes `paths` to `pasteboard` as production does: one
/// `NSFilenamesPboardType` property list of the paths, which the system
/// also offers as file URLs.
///
/// # Safety
/// `pasteboard` is a live `NSPasteboard`.
pub(super) unsafe fn write_paths(pasteboard: &AnyObject, paths: &[PathBuf]) -> bool {
    let names: Vec<Retained<NSString>> = paths
        .iter()
        .map(|path| NSString::from_str(&path.to_string_lossy()))
        .collect();
    let list = NSArray::from_retained_slice(&names);
    let kind = NSString::from_str("NSFilenamesPboardType");
    let kinds = NSArray::from_retained_slice(&[kind.clone()]);
    // SAFETY: NSPasteboard's documented selectors with the argument
    // types they take.
    unsafe {
        let _: isize = msg_send![
            pasteboard,
            declareTypes: &*kinds,
            owner: Option::<&AnyObject>::None
        ];
        let written: Bool = msg_send![pasteboard, setPropertyList: &*list, forType: &*kind];
        written.as_bool()
    }
}

/// The general pasteboard, then [`write_paths`].
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let Some(class) = AnyClass::get(c"NSPasteboard") else {
        return Err("Native file copying is unavailable on this platform.".into());
    };
    // SAFETY: `generalPasteboard` takes nothing and returns the shared
    // pasteboard, on the main thread.
    let written = unsafe {
        let general: Option<Retained<AnyObject>> = msg_send![class, generalPasteboard];
        general.is_some_and(|general| write_paths(&general, paths))
    };
    if written {
        Ok(())
    } else {
        Err("Could not put the files on the clipboard.".into())
    }
}

/// `picto_open_with_application`: the file, in that application, which
/// comes forward.
pub fn open_with(application: &Path, file: &Path) -> Result<(), String> {
    let failed = || "Could not open file with the selected application".to_owned();
    if !application.exists() || !file.exists() {
        return Err(failed());
    }
    let (Some(application), Some(file), Some(workspace), Some(configuration)) = (
        NSURL::from_file_path(application),
        NSURL::from_file_path(file),
        AnyClass::get(c"NSWorkspace"),
        AnyClass::get(c"NSWorkspaceOpenConfiguration"),
    ) else {
        return Err(failed());
    };
    let files = NSArray::from_retained_slice(&[file]);
    // SAFETY: NSWorkspace's documented selectors with the argument types
    // they take; the completion handler may be nil.
    unsafe {
        let workspace: Option<Retained<AnyObject>> = msg_send![workspace, sharedWorkspace];
        let configuration: Option<Retained<AnyObject>> = msg_send![configuration, configuration];
        let (Some(workspace), Some(configuration)) = (workspace, configuration) else {
            return Err(failed());
        };
        let _: () = msg_send![&*configuration, setActivates: Bool::YES];
        let done: Option<&block2::Block<dyn Fn(*mut AnyObject, *mut AnyObject)>> = None;
        let _: () = msg_send![
            &*workspace,
            openURLs: &*files,
            withApplicationAtURL: &*application,
            configuration: &*configuration,
            completionHandler: done
        ];
    }
    Ok(())
}

/// The string an Info.plist key holds, if any.
///
/// # Safety
/// `bundle` is a live `NSBundle`.
unsafe fn info(bundle: &AnyObject, key: &str) -> Option<String> {
    let key = NSString::from_str(key);
    // SAFETY: as the caller promises; the value is checked to be a
    // string before it is read as one.
    unsafe {
        let value: Option<Retained<AnyObject>> =
            msg_send![bundle, objectForInfoDictionaryKey: &*key];
        let value = value?;
        let string = AnyClass::get(c"NSString")?;
        let is_string: Bool = msg_send![&*value, isKindOfClass: string];
        if !is_string.as_bool() {
            return None;
        }
        let value: Retained<NSString> = Retained::cast_unchecked(value);
        Some(value.to_string())
    }
}

/// A `CGImageRef`, which AppKit's selectors take and answer.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct CgImage(*mut std::ffi::c_void);

// SAFETY: a pointer to the opaque `CGImage` struct, as the runtime
// encodes it.
unsafe impl objc2::Encode for CgImage {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGImage", &[]));
}

/// The application's icon drawn 32 pixels square, as PNG
/// (`picto_png_data_url`).
///
/// # Safety
/// `workspace` is the shared `NSWorkspace`.
unsafe fn icon(workspace: &AnyObject, application: &Path) -> Option<Vec<u8>> {
    let path = NSString::from_str(&application.to_string_lossy());
    let representation = AnyClass::get(c"NSBitmapImageRep")?;
    let dictionary = AnyClass::get(c"NSDictionary")?;
    let mut rect = CGRect::new(CGPoint::new(0., 0.), CGSize::new(32., 32.));
    // SAFETY: AppKit's documented selectors with the argument types
    // they take; every object is checked before it is used.
    unsafe {
        let image: Option<Retained<AnyObject>> = msg_send![workspace, iconForFile: &*path];
        let image = image?;
        let null: *mut AnyObject = std::ptr::null_mut();
        let cg_image: CgImage = msg_send![
            &*image,
            CGImageForProposedRect: &mut rect,
            context: null,
            hints: null
        ];
        if cg_image.0.is_null() {
            return None;
        }
        let bitmap: Allocated<AnyObject> = msg_send![representation, alloc];
        let bitmap: Option<Retained<AnyObject>> = msg_send![bitmap, initWithCGImage: cg_image];
        let bitmap = bitmap?;
        let properties: Option<Retained<AnyObject>> = msg_send![dictionary, dictionary];
        // `NSBitmapImageFileTypePNG`.
        let png: Option<Retained<AnyObject>> = msg_send![
            &*bitmap,
            representationUsingType: 4usize,
            properties: &*properties?
        ];
        let png = png?;
        let length: usize = msg_send![&*png, length];
        let bytes: *const std::ffi::c_void = msg_send![&*png, bytes];
        if bytes.is_null() || length == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(bytes.cast::<u8>(), length).to_vec())
    }
}

/// `picto_get_associated_applications`. It runs on a background thread,
/// which has no autorelease pool of its own, so the images and data
/// AppKit hands back autoreleased are let go in one made for the call.
pub fn applications(file: &Path) -> Vec<Application> {
    objc2::rc::autoreleasepool(|_| applications_in_pool(file))
}

fn applications_in_pool(file: &Path) -> Vec<Application> {
    let Some(url) = CFURL::from_path(file, false) else {
        return Vec::new();
    };
    // SAFETY: Launch Services' documented functions; both answers follow
    // the create rule, and a null answer is no list or no default.
    let (urls, default) = unsafe {
        let list = LSCopyApplicationURLsForURL(url.as_concrete_TypeRef(), ALL_ROLES);
        let urls: Vec<PathBuf> = if list.is_null() {
            Vec::new()
        } else {
            CFArray::<CFURL>::wrap_under_create_rule(list)
                .iter()
                .filter_map(|url| url.to_path())
                .collect()
        };
        let default = LSCopyDefaultApplicationURLForURL(
            url.as_concrete_TypeRef(),
            ALL_ROLES,
            std::ptr::null_mut(),
        );
        let default = (!default.is_null())
            .then(|| CFURL::wrap_under_create_rule(default).to_path())
            .flatten();
        (urls, default)
    };
    let (Some(bundle_class), Some(workspace)) =
        (AnyClass::get(c"NSBundle"), AnyClass::get(c"NSWorkspace"))
    else {
        return Vec::new();
    };
    // SAFETY: Foundation's and AppKit's documented selectors with the
    // argument types they take.
    let workspace: Option<Retained<AnyObject>> = unsafe { msg_send![workspace, sharedWorkspace] };
    let mut found = Vec::new();
    for path in urls {
        let Some(url) = NSURL::from_file_path(&path) else {
            continue;
        };
        // SAFETY: as above; a missing bundle has no Info.plist keys.
        let bundle: Option<Retained<AnyObject>> =
            unsafe { msg_send![bundle_class, bundleWithURL: &*url] };
        let (name, identifier) = match &bundle {
            // SAFETY: `bundle` is a live NSBundle.
            Some(bundle) => unsafe {
                (
                    info(bundle, "CFBundleDisplayName").or_else(|| info(bundle, "CFBundleName")),
                    info(bundle, "CFBundleIdentifier"),
                )
            },
            None => (None, None),
        };
        if identifier.as_deref() == Some(PICTO_BUNDLE) {
            continue;
        }
        let name = name.unwrap_or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let icon = workspace
            .as_ref()
            // SAFETY: the shared workspace.
            .and_then(|workspace| unsafe { icon(workspace, &path) })
            .map(|png| Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)));
        found.push(Application {
            default: default.as_ref() == Some(&path),
            name,
            path,
            icon,
        });
    }
    super::sort_applications(&mut found, compare_names);
    found
}

/// `localizedCaseInsensitiveCompare:`.
fn compare_names(left: &str, right: &str) -> std::cmp::Ordering {
    let (left, right) = (NSString::from_str(left), NSString::from_str(right));
    // SAFETY: NSString's documented selector; it answers an
    // `NSComparisonResult`.
    let order: isize = unsafe { msg_send![&*left, localizedCaseInsensitiveCompare: &*right] };
    order.cmp(&0)
}

#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

/// `app.setBadgeCount`: the number on the dock icon, none for zero.
pub fn set_badge(count: u64) {
    let Some(application) = AnyClass::get(c"NSApplication") else {
        return;
    };
    let label = (count > 0).then(|| NSString::from_str(&count.to_string()));
    // SAFETY: AppKit's documented selectors, sent on the main thread
    // (GPUI's foreground), with the argument types they take.
    unsafe {
        let shared: Option<Retained<AnyObject>> = msg_send![application, sharedApplication];
        let Some(shared) = shared else { return };
        let tile: Option<Retained<AnyObject>> = msg_send![&*shared, dockTile];
        let Some(tile) = tile else { return };
        let _: () = msg_send![&*tile, setBadgeLabel: label.as_deref()];
    }
}

/// `new Notification({ title, body }).show()`. The notification centre
/// serves only an app with a bundle identifier; a bare binary (`cargo
/// run`) has none, and the notification is written to standard error
/// instead.
pub fn notify(title: &str, body: &str, _cx: &gpui::App) {
    let (Some(bundle), Some(center), Some(content), Some(request)) = (
        AnyClass::get(c"NSBundle"),
        AnyClass::get(c"UNUserNotificationCenter"),
        AnyClass::get(c"UNMutableNotificationContent"),
        AnyClass::get(c"UNNotificationRequest"),
    ) else {
        return;
    };
    // SAFETY: Foundation's and UserNotifications' documented selectors
    // with the argument types they take; the centre is asked for only
    // when the bundle has an identifier, without which it raises.
    unsafe {
        let main: Option<Retained<AnyObject>> = msg_send![bundle, mainBundle];
        let identifier: Option<Retained<NSString>> = match &main {
            Some(main) => msg_send![&**main, bundleIdentifier],
            None => None,
        };
        if identifier.is_none() {
            eprintln!("picto: {title}: {body} (no bundle, so no system notification)");
            return;
        }
        let center: Option<Retained<AnyObject>> = msg_send![center, currentNotificationCenter];
        let Some(center) = center else { return };
        // Alert and sound; asked once, then remembered by the system.
        let asked = block2::RcBlock::new(|_granted: Bool, _error: *mut AnyObject| {});
        let _: () = msg_send![
            &*center,
            requestAuthorizationWithOptions: 6usize,
            completionHandler: &*asked
        ];
        let content: Option<Retained<AnyObject>> = msg_send![content, new];
        let Some(content) = content else { return };
        let _: () = msg_send![&*content, setTitle: &*NSString::from_str(title)];
        let _: () = msg_send![&*content, setBody: &*NSString::from_str(body)];
        let identifier = NSString::from_str(&format!(
            "picto-subscriptions-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis())
        ));
        let request: Option<Retained<AnyObject>> = msg_send![
            request,
            requestWithIdentifier: &*identifier,
            content: &*content,
            trigger: Option::<&AnyObject>::None
        ];
        let Some(request) = request else { return };
        let done: Option<&block2::Block<dyn Fn(*mut AnyObject)>> = None;
        let _: () = msg_send![
            &*center,
            addNotificationRequest: &*request,
            withCompletionHandler: done
        ];
    }
}
