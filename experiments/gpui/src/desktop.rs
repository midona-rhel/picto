//! What production's main process does for the desktop around the window:
//! the app's badge is the Inbox count (`inboxBadgeService.mjs`), and a
//! system notification says when a batch of subscription runs has finished
//! (`subscriptionNotificationService.mjs`). Both follow the library's change
//! events. Only the real app starts them, never the off-screen checks.
//!
//! It also hands files to the rest of the desktop as production's main
//! process does (`picto:shell:*`, `picto:clipboard:copyFiles`): opening one
//! in its default application or one the user picks, showing it in the
//! file manager, and putting files on the clipboard. Those reach the desktop
//! only in the real app; anywhere else (the off-screen checks) each is
//! written down instead, for the checks to read with [`requests`], so that
//! nothing opens on the screen and the clipboard is left alone.
use crate::application::{self, Event};
use crate::session::Session;
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `CHANGE_DEBOUNCE_MS`: a burst of changes sets the badge once.
const BADGE_DEBOUNCE: Duration = Duration::from_millis(200);
/// `SETTLE_DEBOUNCE_MS`
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(250);
/// `MAX_BADGE`
const MAX_BADGE: u64 = 9_999;

/// Set by `main` so that the window's shell starts the desktop services.
struct Enabled;

impl Global for Enabled {}

/// Lets [`start`] run: only the app proper calls this.
pub fn enable(cx: &mut App) {
    cx.set_global(Enabled);
}

/// Starts the badge and the subscription notification for the session's
/// library, if the app enabled them.
pub fn start(session: &Entity<Session>, cx: &mut App) {
    if !cx.has_global::<Enabled>() {
        return;
    }
    let desktop = cx.new(|cx| Desktop::new(session.clone(), cx));
    cx.set_global(Running(desktop));
}

struct Running(#[allow(dead_code)] Entity<Desktop>);

impl Global for Running {}

struct Desktop {
    session: Entity<Session>,
    /// The count the badge shows.
    shown: Option<u64>,
    badge_timer: Option<Task<()>>,
    /// A badge read is under way; `true` inside when another is wanted.
    badge_reading: Option<bool>,
    /// The runs of the batch being watched, and whether one has been seen
    /// running.
    batch: BTreeSet<i64>,
    batch_active: bool,
    settle_timer: Option<Task<()>>,
    settle_reading: Option<bool>,
    _events: Subscription,
}

impl Desktop {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let events = cx.subscribe(&session, |this, _, event: &Event, cx| {
            if let Event::Changed(changed) = event {
                this.badge_changed(cx);
                if application::touches(changed, "subscriptions")
                    || application::touches(changed, "tasks")
                {
                    this.subscriptions_changed(cx);
                }
            }
        });
        let mut desktop = Self {
            session,
            shown: None,
            badge_timer: None,
            badge_reading: None,
            batch: BTreeSet::new(),
            batch_active: false,
            settle_timer: None,
            settle_reading: None,
            _events: events,
        };
        desktop.read_badge(cx);
        desktop.read_runs(cx);
        desktop
    }

    // ── The badge ───────────────────────────────────────────────

    fn badge_changed(&mut self, cx: &mut Context<Self>) {
        self.badge_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BADGE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.badge_timer = None;
                this.read_badge(cx);
            });
        }));
    }

    /// `library.inbox_count`, then the badge.
    fn read_badge(&mut self, cx: &mut Context<Self>) {
        if let Some(again) = &mut self.badge_reading {
            *again = true;
            return;
        }
        self.badge_reading = Some(false);
        let answer = self
            .session
            .read(cx)
            .dispatch("library.inbox_count", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                let again = this.badge_reading.take() == Some(true);
                match result {
                    Ok(Ok(count)) => this.show_badge(badge_count(&count)),
                    Ok(Err(error)) => eprintln!("picto: inbox badge refresh failed: {error}"),
                    Err(_) => {}
                }
                if again {
                    this.read_badge(cx);
                }
            });
        })
        .detach();
    }

    fn show_badge(&mut self, count: u64) {
        if self.shown == Some(count) {
            return;
        }
        self.shown = Some(count);
        platform::set_badge(count);
    }

    // ── Subscriptions finishing ─────────────────────────────────

    fn subscriptions_changed(&mut self, cx: &mut Context<Self>) {
        self.settle_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.settle_timer = None;
                this.read_runs(cx);
            });
        }));
    }

    /// `refreshOnce`: notes the runs going; once none is, reads what the
    /// batch added and says so.
    fn read_runs(&mut self, cx: &mut Context<Self>) {
        if let Some(again) = &mut self.settle_reading {
            *again = true;
            return;
        }
        self.settle_reading = Some(false);
        let session = self.session.clone();
        let list = session
            .read(cx)
            .dispatch("subscriptions.list", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let finished = match list.await {
                Ok(Ok(list)) => this
                    .update(cx, |this, _| this.observe(&list))
                    .ok()
                    .flatten(),
                Ok(Err(error)) => {
                    eprintln!("picto: subscription notification refresh failed: {error}");
                    None
                }
                Err(_) => None,
            };
            if let Some(runs) = finished {
                let mut activities = Vec::new();
                for run in &runs {
                    let answer = cx.update(|cx| {
                        session.read(cx).dispatch(
                            "subscriptions.runs.get",
                            serde_json::json!({ "run_id": run, "source_item_limit": 1 }),
                        )
                    });
                    activities.push(answer.await.ok().and_then(Result::ok));
                }
                let _ = this.update(cx, |this, cx| match new_items(&activities) {
                    Some(count) => {
                        this.batch.clear();
                        this.batch_active = false;
                        if notifications_allowed(cx) {
                            platform::notify("Subscriptions finished", &finished_body(count));
                        }
                    }
                    // An unavailable activity is not evidence that the run
                    // added nothing: the batch waits for a later change.
                    None => eprintln!(
                        "picto: subscription completion details are unavailable; notification is pending"
                    ),
                });
            }
            let _ = this.update(cx, |this, cx| {
                if this.settle_reading.take() == Some(true) {
                    this.read_runs(cx);
                }
            });
        })
        .detach();
    }

    /// Takes in `subscriptions.list`; the batch's runs when it has just
    /// finished.
    fn observe(&mut self, list: &serde_json::Value) -> Option<Vec<i64>> {
        let active = active_runs(list);
        self.batch.extend(active.iter().copied());
        if !active.is_empty() {
            self.batch_active = true;
            return None;
        }
        (self.batch_active && !self.batch.is_empty()).then(|| self.batch.iter().copied().collect())
    }
}

// ── Files handed to the desktop ─────────────────────────────

/// What was asked of the desktop in a run that does not reach it.
static REQUESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether this run reaches the desktop: only the app proper does.
fn live(cx: &App) -> bool {
    cx.has_global::<Enabled>()
}

fn record(request: String) {
    if let Ok(mut requests) = REQUESTS.lock() {
        requests.push(request);
    }
}

/// What was asked of the desktop since the last look, oldest first, in a
/// run that does not reach it.
#[cfg(test)]
pub fn requests() -> Vec<String> {
    REQUESTS
        .lock()
        .map(|mut requests| std::mem::take(&mut *requests))
        .unwrap_or_default()
}

fn listed(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Whether a link may be handed to the system: only web addresses, as
/// production's `open_external_url` allows. A source address comes from
/// downloaded metadata or from what was typed, so a `file:` address or one
/// for another application's scheme is never opened.
pub fn is_web_link(link: &str) -> bool {
    let link = link.trim();
    let Some((scheme, rest)) = link.split_once(':') else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && rest.starts_with("//")
        && rest.len() > 2
}

/// `open_external_url`: a web address in the browser. Anything else is
/// refused with production's message.
pub fn open_link(link: &str, cx: &mut App) {
    if !is_web_link(link) {
        crate::ui::notify::error(
            "Could not open link",
            "Only HTTP and HTTPS links can be opened externally",
            cx,
        );
        return;
    }
    if live(cx) {
        cx.open_url(link.trim());
    } else {
        record(format!("open-link {}", link.trim()));
    }
}

/// `shell.openPath`: the file in its default application.
pub fn open_default(path: &Path, cx: &App) {
    if live(cx) {
        cx.open_with_system(path);
    } else {
        record(format!("open-default {}", path.display()));
    }
}

/// `shell.showItemInFolder`: Finder or Explorer, with the file selected.
pub fn reveal(path: &Path, cx: &App) {
    if live(cx) {
        cx.reveal_path(path);
    } else {
        record(format!("reveal {}", path.display()));
    }
}

/// `picto:shell:openWithApplication`: the file in the application at
/// `application` (macOS only, as in production).
pub fn open_with(application: &Path, file: &Path, cx: &App) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Application selection is only available on macOS".into());
    }
    if live(cx) {
        platform::open_with(application, file)
    } else {
        record(format!(
            "open-with {} | {}",
            application.display(),
            file.display()
        ));
        Ok(())
    }
}

/// `writeClipboardFilePaths`: the files themselves on the clipboard, each
/// once, for Finder or another application to paste.
pub fn copy_files(paths: &[PathBuf], cx: &App) -> Result<(), String> {
    let mut unique: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !path.as_os_str().is_empty() && !unique.contains(path) {
            unique.push(path.clone());
        }
    }
    if unique.is_empty() {
        return Err("No files were selected.".into());
    }
    if live(cx) {
        platform::copy_files(&unique)
    } else {
        record(format!("copy-files {}", listed(&unique)));
        Ok(())
    }
}

/// An application that opens a file (`getAssociatedApplications`).
#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub name: String,
    pub path: PathBuf,
    /// Its icon, 32 pixels square, as PNG.
    pub icon: Option<Arc<gpui::Image>>,
    /// The one the system opens the file with.
    pub default: bool,
}

/// The applications that can open `file`, the default first and the rest
/// by name, leaving Picto itself out. Only macOS lists them (production
/// offers Windows' own chooser and nothing on Linux). Asking changes
/// nothing, so it is asked in every run; it takes a while for a new type,
/// so it runs off the thread that draws.
pub fn applications(file: &Path) -> Vec<Application> {
    platform::applications(file)
}

/// The run of each subscription that is pending or running.
fn active_runs(list: &serde_json::Value) -> Vec<i64> {
    list["subscriptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|subscription| {
            matches!(subscription["status"].as_str(), Some("pending" | "running"))
        })
        .filter_map(|subscription| subscription["active_run_id"].as_i64())
        .collect()
}

/// How many library items the batch's runs added, counting each once; none
/// while any run's activity could not be read.
fn new_items(activities: &[Option<serde_json::Value>]) -> Option<usize> {
    let mut keys = BTreeSet::new();
    for activity in activities {
        for key in activity.as_ref()?["new_library_item_keys"].as_array()? {
            keys.insert(key.as_str()?.to_owned());
        }
    }
    Some(keys.len())
}

/// The notification's text.
fn finished_body(count: usize) -> String {
    match count {
        0 => "All subscriptions finished. No new library items were added.".into(),
        1 => "1 new library item is ready to review.".into(),
        count => format!(
            "{} new library items are ready to review.",
            crate::grid::format::grouped(count as i128)
        ),
    }
}

/// The badge's number: the count, at most 9,999.
fn badge_count(count: &serde_json::Value) -> u64 {
    count
        .as_u64()
        .or_else(|| count.as_f64().map(|count| count.max(0.) as u64))
        .unwrap_or(0)
        .min(MAX_BADGE)
}

/// Production shows this notification whatever Settings › Notifications
/// says; here it follows the pop-up switch and the Information tone, as an
/// in-app notification of the same kind would.
fn notifications_allowed(cx: &mut App) -> bool {
    crate::ui::notify::shows(crate::ui::notify::Tone::Info, cx)
}

#[cfg(target_os = "macos")]
mod platform {
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
            let configuration: Option<Retained<AnyObject>> =
                msg_send![configuration, configuration];
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

    /// `picto_get_associated_applications`.
    pub fn applications(file: &Path) -> Vec<Application> {
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
        let workspace: Option<Retained<AnyObject>> =
            unsafe { msg_send![workspace, sharedWorkspace] };
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
                        info(bundle, "CFBundleDisplayName")
                            .or_else(|| info(bundle, "CFBundleName")),
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
    pub fn notify(title: &str, body: &str) {
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
}

/// Windows would set the taskbar button's overlay icon
/// (`ITaskbarList3::SetOverlayIcon`) and show a toast through
/// `ToastNotificationManager` under the app's AppUserModelID; Linux would
/// send the Unity launcher's `com.canonical.Unity.LauncherEntry` `Update`
/// signal with the count and `org.freedesktop.Notifications.Notify` over
/// D-Bus. Neither is built: on those systems both do nothing.
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Application;
    use std::path::{Path, PathBuf};

    pub fn set_badge(_count: u64) {}

    pub fn notify(_title: &str, _body: &str) {}

    /// TODO: put the files on the clipboard as production's native addon
    /// does: on Windows a `CF_HDROP` `DROPFILES` list (`copy_files` in
    /// `native/picto-node`), on Linux a `text/uri-list` of `file://` URLs.
    pub fn copy_files(_paths: &[PathBuf]) -> Result<(), String> {
        Err("Native file copying is unavailable on this platform.".into())
    }

    /// Production picks an application on macOS only.
    pub fn open_with(_application: &Path, _file: &Path) -> Result<(), String> {
        Err("Application selection is only available on macOS".into())
    }

    /// TODO: on Windows, Open With Other opens the system's own chooser
    /// (`rundll32.exe shell32.dll,OpenAs_RunDLL <path>`), as production
    /// does; Linux offers nothing.
    pub fn applications(_file: &Path) -> Vec<Application> {
        Vec::new()
    }
}

/// Production's order: the default application first, then by name.
fn sort_applications(
    applications: &mut [Application],
    compare: impl Fn(&str, &str) -> std::cmp::Ordering,
) {
    applications.sort_by(|left, right| {
        right
            .default
            .cmp(&left.default)
            .then_with(|| compare(&left.name, &right.name))
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_web_links_are_opened() {
        for link in [
            "https://example.com/a?b=c",
            "http://example.com",
            " HTTPS://Example.com ",
        ] {
            assert!(super::is_web_link(link), "{link}");
        }
        for link in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "picto://open",
            "smb://server/share",
            "https:",
            "https://",
            "example.com",
            "",
        ] {
            assert!(!super::is_web_link(link), "{link}");
        }
    }

    use super::{
        Application, active_runs, badge_count, finished_body, new_items, sort_applications,
    };
    use serde_json::json;

    fn application(name: &str, default: bool) -> Application {
        Application {
            name: name.into(),
            path: format!("/Applications/{name}.app").into(),
            icon: None,
            default,
        }
    }

    #[test]
    fn the_default_application_comes_first_then_the_rest_by_name() {
        let mut found = vec![
            application("Safari", false),
            application("affinity", false),
            application("Preview", true),
        ];
        sort_applications(&mut found, |a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        let names: Vec<&str> = found.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["Preview", "affinity", "Safari"]);
    }

    /// Writes to a pasteboard of its own, never the user's clipboard, and
    /// reads back what Finder and GPUI read.
    #[cfg(target_os = "macos")]
    #[test]
    fn files_go_on_the_pasteboard_as_a_list_of_paths() {
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2_foundation::{NSArray, NSString};
        let class = AnyClass::get(c"NSPasteboard").unwrap();
        let paths = [
            std::path::PathBuf::from("/tmp/a b.png"),
            std::path::PathBuf::from("/tmp/c.jpg"),
        ];
        // SAFETY: NSPasteboard's documented selectors; the pasteboard is
        // one made for this test and released at the end.
        unsafe {
            let pasteboard: Retained<AnyObject> = msg_send![class, pasteboardWithUniqueName];
            assert!(super::platform::write_paths(&pasteboard, &paths));
            let kind = NSString::from_str("NSFilenamesPboardType");
            let list: Option<Retained<NSArray<NSString>>> =
                msg_send![&*pasteboard, propertyListForType: &*kind];
            let read: Vec<String> = list.unwrap().iter().map(|path| path.to_string()).collect();
            assert_eq!(read, ["/tmp/a b.png", "/tmp/c.jpg"]);
            let _: () = msg_send![&*pasteboard, releaseGlobally];
        }
    }

    /// Opening a file in an application cannot be tried here without
    /// opening it, so its message is checked against AppKit's own types.
    #[cfg(target_os = "macos")]
    #[test]
    fn opening_with_an_application_sends_what_appkit_takes() {
        use objc2::runtime::{AnyClass, AnyObject, Bool};
        use objc2::sel;
        let workspace = AnyClass::get(c"NSWorkspace").unwrap();
        workspace
            .verify_sel::<(
                &AnyObject,
                &AnyObject,
                &AnyObject,
                Option<&block2::Block<dyn Fn(*mut AnyObject, *mut AnyObject)>>,
            ), ()>(sel!(openURLs:withApplicationAtURL:configuration:completionHandler:))
            .unwrap();
        AnyClass::get(c"NSWorkspaceOpenConfiguration")
            .unwrap()
            .verify_sel::<(Bool,), ()>(sel!(setActivates:))
            .unwrap();
    }

    /// Asking which applications open a type changes nothing; every Mac
    /// opens a PNG with something, and has a default for it.
    #[cfg(target_os = "macos")]
    #[test]
    fn applications_that_open_a_picture_are_listed_with_icons() {
        let folder = std::env::temp_dir().join(format!("picto-open-with-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("probe.png");
        std::fs::write(&file, b"").unwrap();
        let found = super::applications(&file);
        std::fs::remove_dir_all(&folder).unwrap();
        assert!(!found.is_empty());
        assert!(found[0].default);
        assert!(found.iter().skip(1).all(|app| !app.default));
        assert!(found.iter().any(|app| app.icon.is_some()));
    }

    #[test]
    fn running_and_pending_runs_make_the_batch() {
        let list = json!({ "subscriptions": [
            { "active_run_id": 4, "status": "running" },
            { "active_run_id": 5, "status": "pending" },
            { "active_run_id": 6, "status": "paused" },
            { "active_run_id": null, "status": "running" },
        ]});
        assert_eq!(active_runs(&list), vec![4, 5]);
    }

    #[test]
    fn new_items_are_counted_once_across_runs() {
        let runs = [
            Some(json!({ "new_library_item_keys": ["a", "b"] })),
            Some(json!({ "new_library_item_keys": ["b", "c"] })),
        ];
        assert_eq!(new_items(&runs), Some(3));
        // An unreadable run leaves the count unknown.
        assert_eq!(new_items(&[runs[0].clone(), None]), None);
        assert_eq!(
            new_items(&[Some(json!({ "new_library_item_keys": [1] }))]),
            None
        );
    }

    #[test]
    fn the_notification_reads_as_production_writes_it() {
        assert_eq!(
            finished_body(0),
            "All subscriptions finished. No new library items were added."
        );
        assert_eq!(finished_body(1), "1 new library item is ready to review.");
        assert_eq!(
            finished_body(1234),
            "1,234 new library items are ready to review."
        );
    }

    #[test]
    fn the_badge_is_capped() {
        assert_eq!(badge_count(&json!(12)), 12);
        assert_eq!(badge_count(&json!(120000)), 9_999);
        assert_eq!(badge_count(&json!(null)), 0);
    }
}
