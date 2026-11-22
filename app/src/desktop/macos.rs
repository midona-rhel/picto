//! The macOS part of `desktop`. It sets the badge on the Dock icon, shows a
//! notification through the user notification centre, puts files on the
//! pasteboard, and asks Launch Services which applications open a file.
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

/// The Launch Services constant `kLSRolesAll`: an application counts whatever
/// its role for the file's type is (viewer, editor or another).
const ALL_ROLES: u32 = 0xFFFF_FFFF;
/// Picto's bundle identifier. An application with this identifier is left out
/// of the list of applications that open a file.
const PICTO_BUNDLE: &str = "com.picto.desktop";

/// Writes `paths` to `pasteboard` as one property list of the type
/// `NSFilenamesPboardType`. The system also offers such a list to other
/// applications as file URLs. Returns whether the pasteboard accepted the
/// list.
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
    // SAFETY: The messages sent are documented selectors of NSPasteboard, with
    // the argument types they take.
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

/// Puts `paths` on the general pasteboard, which is the system's clipboard,
/// with [`write_paths`].
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let Some(class) = AnyClass::get(c"NSPasteboard") else {
        return Err("Native file copying is unavailable on this platform.".into());
    };
    // SAFETY: `generalPasteboard` takes no argument and returns the shared
    // pasteboard. It is sent on the main thread.
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

/// Opens `file` in the application at `application` and brings that
/// application to the front. Returns an error if one of the two paths does not
/// exist or AppKit's classes are not found.
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
    // SAFETY: The messages sent are documented selectors of NSWorkspace and
    // NSWorkspaceOpenConfiguration, with the argument types they take. The
    // completion handler may be nil.
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

/// Returns the string that the bundle's Info.plist holds under `key`, or
/// `None` if the key is missing or its value is not a string.
///
/// # Safety
/// `bundle` is a live `NSBundle`.
unsafe fn info(bundle: &AnyObject, key: &str) -> Option<String> {
    let key = NSString::from_str(key);
    // SAFETY: The caller guarantees that `bundle` is a live `NSBundle`. The
    // value is cast to `NSString` only after `isKindOfClass:` has confirmed
    // that it is one.
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

/// A `CGImageRef`, in a type that can be an argument and a return value of the
/// messages sent to AppKit.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct CgImage(*mut std::ffi::c_void);

// SAFETY: The Objective-C runtime encodes a `CGImageRef` as a pointer to the
// opaque struct `CGImage`, which is the encoding given here.
unsafe impl objc2::Encode for CgImage {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGImage", &[]));
}

/// Returns the icon of the application at `application`, drawn 32 pixels
/// square, as the bytes of a PNG file. Returns `None` if AppKit gives no icon.
///
/// # Safety
/// `workspace` is the shared `NSWorkspace`.
unsafe fn icon(workspace: &AnyObject, application: &Path) -> Option<Vec<u8>> {
    let path = NSString::from_str(&application.to_string_lossy());
    let representation = AnyClass::get(c"NSBitmapImageRep")?;
    let dictionary = AnyClass::get(c"NSDictionary")?;
    let mut rect = CGRect::new(CGPoint::new(0., 0.), CGSize::new(32., 32.));
    // SAFETY: The messages sent are documented selectors of AppKit, with the
    // argument types they take. Every object that a message returns is checked
    // for nil before it is used. The PNG's bytes are copied while `png` is
    // still alive.
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
        // The file type 4 is `NSBitmapImageFileTypePNG`.
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

/// Returns the applications that can open `file`, as `desktop::applications`
/// describes. It runs on a background thread, which has no autorelease pool of
/// its own. So the work is done inside an autorelease pool that is made here,
/// and the images and data that AppKit returns autoreleased are freed when the
/// call ends.
pub fn applications(file: &Path) -> Vec<Application> {
    objc2::rc::autoreleasepool(|_| applications_in_pool(file))
}

fn applications_in_pool(file: &Path) -> Vec<Application> {
    let Some(url) = CFURL::from_path(file, false) else {
        return Vec::new();
    };
    // SAFETY: Both are documented functions of Launch Services. Each returns
    // an object that the caller owns (the create rule), so each is wrapped
    // under the create rule. A null return value means that there is no list,
    // or no default application.
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
    // SAFETY: `sharedWorkspace` is a documented selector of NSWorkspace and
    // takes no argument.
    let workspace: Option<Retained<AnyObject>> = unsafe { msg_send![workspace, sharedWorkspace] };
    let mut found = Vec::new();
    for path in urls {
        let Some(url) = NSURL::from_file_path(&path) else {
            continue;
        };
        // SAFETY: `bundleWithURL:` is a documented selector of NSBundle and
        // takes an `NSURL`. It returns nil when there is no bundle at the URL.
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
            // SAFETY: `workspace` is the shared `NSWorkspace`.
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

/// Compares two application names with NSString's
/// `localizedCaseInsensitiveCompare:`, which orders them as the user's
/// language does and without regard to case.
fn compare_names(left: &str, right: &str) -> std::cmp::Ordering {
    let (left, right) = (NSString::from_str(left), NSString::from_str(right));
    // SAFETY: `localizedCaseInsensitiveCompare:` is a documented selector of
    // NSString. It returns an `NSComparisonResult`, which is an `isize` that
    // is negative, zero or positive.
    let order: isize = unsafe { msg_send![&*left, localizedCaseInsensitiveCompare: &*right] };
    order.cmp(&0)
}

#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

/// Returns the text that the badge on the Dock icon shows for a count: no
/// badge for zero, the number itself up to 99, and "99+" for a larger count.
fn badge_label(count: u64) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".to_owned()),
    }
}

/// Sets the badge on the Dock icon to the text that `badge_label` gives for
/// `count`. For a count of zero it removes the badge.
pub fn set_badge(count: u64) {
    let Some(application) = AnyClass::get(c"NSApplication") else {
        return;
    };
    let label = badge_label(count).map(|label| NSString::from_str(&label));
    // SAFETY: The messages sent are documented selectors of AppKit, with the
    // argument types they take. They are sent on the main thread, which is the
    // thread that GPUI's foreground tasks run on.
    unsafe {
        let shared: Option<Retained<AnyObject>> = msg_send![application, sharedApplication];
        let Some(shared) = shared else { return };
        let tile: Option<Retained<AnyObject>> = msg_send![&*shared, dockTile];
        let Some(tile) = tile else { return };
        let _: () = msg_send![&*tile, setBadgeLabel: label.as_deref()];
    }
}

/// Shows a system notification with `title` and `body` through the user
/// notification centre. The notification centre works only for an app that has
/// a bundle identifier. A program started without a bundle (`cargo run`) has
/// none, and there the notification is written to standard error instead.
pub fn notify(title: &str, body: &str, _cx: &gpui::App) {
    let (Some(bundle), Some(center), Some(content), Some(request)) = (
        AnyClass::get(c"NSBundle"),
        AnyClass::get(c"UNUserNotificationCenter"),
        AnyClass::get(c"UNMutableNotificationContent"),
        AnyClass::get(c"UNNotificationRequest"),
    ) else {
        return;
    };
    // SAFETY: The messages sent are documented selectors of Foundation and
    // UserNotifications, with the argument types they take.
    // `currentNotificationCenter` raises an exception in a process without a
    // bundle identifier, so it is sent only after the identifier was found.
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
        // The options value 6 asks for permission to show alerts and to play
        // sounds. The system asks the user once and remembers the answer.
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

#[cfg(test)]
mod tests {
    use super::badge_label;

    #[test]
    fn the_dock_badge_is_the_count_up_to_ninety_nine() {
        assert_eq!(badge_label(0), None);
        assert_eq!(badge_label(7).as_deref(), Some("7"));
        assert_eq!(badge_label(99).as_deref(), Some("99"));
        assert_eq!(badge_label(100).as_deref(), Some("99+"));
        assert_eq!(badge_label(9_999).as_deref(), Some("99+"));
    }
}

/// Returns an error: on macOS the app installs no update itself. The user gets
/// an update from the release page.
pub fn install_update(
    _installer: &std::path::Path,
    _silent: bool,
    _run_after: bool,
    _admin: bool,
) -> Result<(), String> {
    Err("Updates are installed from the release page on macOS.".to_owned())
}

/// Returns `None`: only the Linux build runs from an AppImage.
pub fn app_image() -> Option<std::path::PathBuf> {
    None
}
