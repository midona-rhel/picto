//! This module does for the desktop what the Electron app's main process does
//! outside its window. It sets the app's badge to the Inbox count
//! (`inboxBadgeService.mjs`), and it shows a system notification when a batch
//! of subscription runs has finished (`subscriptionNotificationService.mjs`).
//! Both react to the library's change events. Only the real app starts them;
//! the off-screen checks never do.
//!
//! It also passes files to other parts of the desktop, as the Electron app's
//! main process does (`picto:shell:*`, `picto:clipboard:copyFiles`): it opens
//! a file in its default application or in one the user picks, shows a file in
//! the file manager, and puts files on the clipboard. These requests reach the
//! desktop only in the real app. In the off-screen checks each request is
//! recorded instead, and the checks read the records with [`requests`], so
//! nothing opens on the screen and the clipboard is not changed. Only files
//! inside the open copy are passed on, whatever path the library stored. For
//! any other path, these functions, the drag out of the grid and "Copy File
//! Path" refuse and show a notification.
use crate::application::{self, Event};
use crate::session::{Coalesced, Session};
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;

/// The time the badge waits after a change before it reads the Inbox count,
/// so that several changes close together set the badge once. The Electron
/// app waits the same time (`CHANGE_DEBOUNCE_MS`).
const BADGE_DEBOUNCE: Duration = Duration::from_millis(200);
/// The time to wait after a change to subscriptions or tasks before the
/// subscriptions' runs are read. The Electron app waits the same time
/// (`SETTLE_DEBOUNCE_MS`).
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(250);
/// The largest number the badge shows, as in the Electron app (`MAX_BADGE`).
const MAX_BADGE: u64 = 9_999;

/// A global that `main` sets through [`enable`]. While it is set, [`start`]
/// starts the badge and the subscription notification.
struct Enabled;

impl Global for Enabled {}

/// Allows [`start`] to start the badge and the subscription notification. Only
/// the real app calls this; the off-screen checks do not.
pub fn enable(cx: &mut App) {
    cx.set_global(Enabled);
}

/// Records the root of the session's library as the only folder whose files
/// may be passed to the desktop. If [`enable`] was called, it also starts the
/// badge and the subscription notification for that library.
pub fn start(session: &Entity<Session>, cx: &mut App) {
    let root = session.read(cx).root().to_path_buf();
    cx.set_global(HandOff(root));
    if !cx.has_global::<Enabled>() {
        return;
    }
    let desktop = cx.new(|cx| Desktop::new(session.clone(), cx));
    cx.set_global(Running { _desktop: desktop });
}

/// A global that keeps the [`Desktop`] entity alive for as long as the app
/// runs.
struct Running {
    _desktop: Entity<Desktop>,
}

impl Global for Running {}

struct Desktop {
    session: Entity<Session>,
    /// The count that the badge shows now, or `None` before the first count
    /// was read.
    shown: Option<u64>,
    badge_timer: Option<Task<()>>,
    /// Whether the Inbox count is being read now, and whether to read it again
    /// afterwards.
    badge_reading: Coalesced,
    /// The ids of the runs in the batch that is being watched. `batch_active`
    /// says whether any of them has been seen pending or running.
    batch: BTreeSet<i64>,
    batch_active: bool,
    settle_timer: Option<Task<()>>,
    /// Whether the subscriptions and their finished runs are being read now,
    /// and whether to read them again afterwards.
    settle_reading: Coalesced,
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
            badge_reading: Coalesced::default(),
            batch: BTreeSet::new(),
            batch_active: false,
            settle_timer: None,
            settle_reading: Coalesced::default(),
            _events: events,
        };
        desktop.read_badge(cx);
        desktop.read_runs(cx);
        desktop
    }

    // The next three functions keep the badge at the Inbox count.

    fn badge_changed(&mut self, cx: &mut Context<Self>) {
        self.badge_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BADGE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.badge_timer = None;
                this.read_badge(cx);
            });
        }));
    }

    /// Reads the Inbox count with `library.inbox_count` and shows it on the
    /// badge. If a read is already running, it reads once more when that one
    /// ends.
    fn read_badge(&mut self, cx: &mut Context<Self>) {
        if !self.badge_reading.begin() {
            return;
        }
        let answer = self
            .session
            .read(cx)
            .dispatch("library.inbox_count", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                let again = this.badge_reading.end();
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

    // The functions that follow watch the subscriptions' runs and show the
    // system notification when a batch of runs has finished.

    fn subscriptions_changed(&mut self, cx: &mut Context<Self>) {
        self.settle_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.settle_timer = None;
                this.read_runs(cx);
            });
        }));
    }

    /// Reads `subscriptions.list` and adds the pending and running runs to
    /// the batch. When no run is pending or running any more, it reads each
    /// run of the batch with `subscriptions.runs.get`, counts the library
    /// items the batch added, and shows the system notification with that
    /// count. The Electron app does this in `refreshOnce`.
    fn read_runs(&mut self, cx: &mut Context<Self>) {
        if !self.settle_reading.begin() {
            return;
        }
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
                        // The system notification is shown whatever Settings ›
                        // Notifications says, as in the Electron app. Those
                        // settings apply only to the notifications shown
                        // inside the window.
                        platform::notify("Subscriptions finished", &finished_body(count), cx);
                    }
                    // If a run's activity could not be read, that does not
                    // show that the run added nothing. The batch is kept, and
                    // a later change event reads it again.
                    None => eprintln!(
                        "picto: subscription completion details are unavailable; notification is pending"
                    ),
                });
            }
            let _ = this.update(cx, |this, cx| {
                if this.settle_reading.end() {
                    this.read_runs(cx);
                }
            });
        })
        .detach();
    }

    /// Updates the batch from the answer of `subscriptions.list`. Returns the
    /// ids of the batch's runs when no run is pending or running now and at
    /// least one was seen pending or running earlier. Otherwise it returns
    /// `None`.
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

// The functions that pass files and links to other parts of the desktop
// start here.

/// A global that holds the canonical root of the open library copy. Only files
/// inside it are passed to the desktop. A stored path that still names the
/// library the copy was made from, or any other path outside the copy, would
/// let another application edit, move or delete a file the user keeps.
struct HandOff(PathBuf);

impl Global for HandOff {}

/// Returns whether `path` is inside the folder `root`. Both must be canonical
/// paths, so neither contains a symbolic link or a `..` step. A path with
/// Windows' verbatim prefix (`\\?\C:\`, `\\?\UNC\`) is compared as the same
/// path without the prefix. On Windows, names are compared without regard to
/// case, as Windows' file systems compare them. The root itself does not count
/// as inside.
pub(crate) fn inside(root: &Path, path: &Path) -> bool {
    let (root, path) = (system_path(root), system_path(path));
    let same = |left: std::path::Component, right: std::path::Component| {
        if cfg!(windows) {
            left.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
        } else {
            left == right
        }
    };
    let mut parts = path.components();
    for part in root.components() {
        match parts.next() {
            Some(next) if same(part, next) => {}
            _ => return false,
        }
    }
    let rest: Vec<_> = parts.collect();
    !rest.is_empty()
        && rest
            .iter()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

/// Returns the canonical path of each of `paths` when all of them are files
/// inside the open copy. Returns an error message when no library is open, or
/// when one of the paths does not exist or is outside the copy.
fn checked(paths: &[PathBuf], cx: &App) -> Result<Vec<PathBuf>, String> {
    match cx.try_global::<HandOff>() {
        Some(HandOff(root)) => checked_in(root, paths),
        None => Err("No library is open.".into()),
    }
}

/// Does the work of [`checked`] for the copy whose canonical root is `root`.
fn checked_in(root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    paths
        .iter()
        .map(|path| match path.canonicalize() {
            Ok(canonical) if inside(root, &canonical) => Ok(canonical),
            Ok(_) => Err(format!(
                "{} is not a file of this library copy, so it was not handed to the desktop.",
                path.display()
            )),
            Err(error) => Err(format!("{}: {error}", path.display())),
        })
        .collect()
}

/// Does what [`checked`] does, for one file.
fn checked_one(path: &Path, cx: &App) -> Result<PathBuf, String> {
    checked(&[path.to_path_buf()], cx).map(|mut paths| paths.remove(0))
}

/// Returns the paths that a drag out of the window gives to the system. Each
/// path is in the form that [`copy_paths`] writes, without the `\\?\` prefix
/// on Windows. If any of `paths` is not a file of the open copy, it returns no
/// paths and shows an error notification.
pub fn drag_out(paths: &[PathBuf], cx: &mut App) -> Vec<PathBuf> {
    match checked(paths, cx) {
        Ok(paths) => paths.iter().map(|path| system_path(path)).collect(),
        Err(error) => {
            crate::ui::notify::error("Could not drag files", error, cx);
            Vec::new()
        }
    }
}

/// The requests that were recorded and not sent to the desktop, in a test run
/// that did not call [`enable`].
#[cfg(test)]
static REQUESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Returns whether requests in this run are sent to the desktop. In a test
/// build they are sent only after [`enable`] was called, which only the real
/// app does. Otherwise each request is recorded for the off-screen checks.
#[cfg(test)]
fn live(cx: &App) -> bool {
    cx.has_global::<Enabled>()
}

#[cfg(not(test))]
fn live(_: &App) -> bool {
    true
}

#[cfg(test)]
fn record(request: String) {
    if let Ok(mut requests) = REQUESTS.lock() {
        requests.push(request);
    }
}

#[cfg(not(test))]
fn record(_: String) {}

/// Returns the requests recorded since the last call, oldest first, and
/// empties the record. Requests are recorded only in a run that does not send
/// them to the desktop.
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

/// Returns whether a link may be opened by the system. Only `http` and `https`
/// addresses may, as in the Electron app's `open_external_url`. A source
/// address comes from downloaded metadata or from what the user typed, so a
/// `file:` address, or an address with another application's scheme, is never
/// opened.
pub fn is_web_link(link: &str) -> bool {
    let link = link.trim();
    let Some((scheme, rest)) = link.split_once(':') else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && rest.starts_with("//")
        && rest.len() > 2
}

/// Opens a web address in the browser, as the Electron app's
/// `open_external_url` does. For any other link it shows the Electron app's
/// error message and opens nothing.
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

/// Opens the file in its default application, if the file is inside the open
/// copy. Otherwise it shows an error notification. The Electron app opens the
/// file with `shell.openPath`.
pub fn open_default(path: &Path, cx: &mut App) {
    let path = match checked_one(path, cx) {
        Ok(path) => path,
        Err(error) => {
            return crate::ui::notify::error(crate::i18n::t("Could not open file"), error, cx);
        }
    };
    if live(cx) {
        cx.open_with_system(&system_path(&path));
    } else {
        record(format!("open-default {}", path.display()));
    }
}

/// Shows the file, selected, in the system's file manager (Finder or
/// Explorer), if the file is inside the open copy. Otherwise it shows an
/// error notification. The Electron app shows the file with
/// `shell.showItemInFolder`.
pub fn reveal(path: &Path, cx: &mut App) {
    let path = match checked_one(path, cx) {
        Ok(path) => path,
        Err(error) => return crate::ui::notify::error("Could not show file", error, cx),
    };
    if live(cx) {
        cx.reveal_path(&system_path(&path));
    } else {
        record(format!("reveal {}", path.display()));
    }
}

/// Starts an update's verified installer in the way that electron-updater's
/// updater for this operating system starts it (`doInstall`). The details are
/// in each system's file (`macos.rs`, `windows.rs`, `linux.rs`). A test build
/// records the request and starts nothing.
pub fn install_update(
    installer: &Path,
    silent: bool,
    run_after: bool,
    admin: bool,
) -> Result<(), String> {
    if cfg!(test) {
        record(format!(
            "install-update {} silent={silent} run-after={run_after} admin={admin}",
            installer.display()
        ));
        return Ok(());
    }
    platform::install_update(installer, silent, run_after, admin)
}

/// Returns the path of the AppImage that this process runs from, which is
/// `$APPIMAGE` on Linux. On macOS and Windows it returns `None`.
pub fn app_image() -> Option<PathBuf> {
    platform::app_image()
}

/// Does Show in File Manager for a library in the Library Manager: shows the
/// library's folder in the file manager. It does so only when the library is
/// the open one, or is in the library history, or is a pinned library.
/// Otherwise it shows an error notification.
pub fn reveal_library(path: &Path, cx: &mut App) {
    let text = path.to_string_lossy();
    let open = crate::libraries::host::current(cx).is_some_and(|(root, _)| root == path);
    let state = crate::state::get(cx);
    let listed = state.library_history.iter().any(|known| *known == text)
        || state.pinned_libraries.iter().any(|known| *known == text);
    if !open && !listed {
        return crate::ui::notify::error(
            "Could not show file",
            format!("{} is not a library in the list", path.display()),
            cx,
        );
    }
    if live(cx) {
        cx.reveal_path(&system_path(path));
    } else {
        record(format!("reveal {}", path.display()));
    }
}

/// Does Show Cloud Folder: shows `path` in the file manager, only when `path`
/// is the cloud folder that the open library syncs to and that folder exists.
/// Otherwise it shows an error notification.
pub fn reveal_cloud_folder(path: &Path, cx: &mut App) {
    let configured = crate::libraries::host::shell(cx)
        .and_then(|shell| {
            let cloud = shell.read(cx).cloud().read(cx);
            cloud.configuration.as_ref()?.root_path.clone()
        })
        .is_some_and(|root| Path::new(&root) == path);
    if !configured || !path.is_dir() {
        return crate::ui::notify::error(
            "Could not show file",
            format!("{} is not the open library's cloud folder", path.display()),
            cx,
        );
    }
    if live(cx) {
        cx.reveal_path(&system_path(path));
    } else {
        record(format!("reveal {}", path.display()));
    }
}

/// Opens the file in the application at `application`, if the file is inside
/// the open copy. This exists on macOS only, as in the Electron app
/// (`picto:shell:openWithApplication`).
#[cfg(target_os = "macos")]
pub fn open_with(application: &Path, file: &Path, cx: &App) -> Result<(), String> {
    let file = checked_one(file, cx)?;
    if live(cx) {
        platform::open_with(application, &file)
    } else {
        record(format!(
            "open-with {} | {}",
            application.display(),
            file.display()
        ));
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open_with(_application: &Path, _file: &Path, _cx: &App) -> Result<(), String> {
    Err("Application selection is only available on macOS".into())
}

/// Puts the files themselves on the clipboard, each one once, so that Finder
/// or another application can paste them. It does so only if all of them are
/// inside the open copy. The Electron app does this in
/// `writeClipboardFilePaths`.
pub fn copy_files(paths: &[PathBuf], cx: &App) -> Result<(), String> {
    let mut unique: Vec<PathBuf> = Vec::new();
    for path in checked(paths, cx)? {
        if !unique.contains(&path) {
            unique.push(path);
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

/// Does "Copy File Path": puts the paths on the clipboard as text, one path
/// on each line, if all of them are files inside the open copy. Each path is
/// written as [`system_path`] gives it, without Windows' `\\?\` prefix.
pub fn copy_paths(paths: &[PathBuf], cx: &mut App) -> Result<(), String> {
    let paths = checked(paths, cx)?;
    if paths.is_empty() {
        return Err("No files were selected.".into());
    }
    let text = paths
        .iter()
        .map(|path| system_path(path).display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    Ok(())
}

/// Opens the system's own dialog for choosing an application for the file, if
/// the file is inside the open copy. Only Windows has such a dialog (its
/// "Open with" dialog). The Electron app opens it for
/// `picto:shell:openWithChooser`.
#[cfg(target_os = "windows")]
pub fn open_with_chooser(file: &Path, cx: &App) -> Result<(), String> {
    let file = checked_one(file, cx)?;
    if live(cx) {
        platform::open_with_chooser(&system_path(&file))
    } else {
        record(format!("open-with-chooser {}", file.display()));
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn open_with_chooser(_file: &Path, _cx: &App) -> Result<(), String> {
    Err("The system application chooser is unavailable".into())
}

/// One application that can open a file, as `getAssociatedApplications` lists
/// it.
#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub name: String,
    pub path: PathBuf,
    /// Its icon, 32 pixels square, as PNG.
    pub icon: Option<Arc<gpui::Image>>,
    /// Whether this is the application that the system opens the file with by
    /// default.
    pub default: bool,
}

/// Returns the applications that can open `file`: the default one first, then
/// the rest by name, without Picto itself. Only macOS lists them; the Electron
/// app offers Windows' own chooser on Windows and nothing on Linux. Asking the
/// system for the list changes nothing on the desktop, so the off-screen
/// checks ask too. The answer can take a long time for a file type that has
/// not been asked about before, so this is called on a background thread, not
/// on the thread that draws.
pub fn applications(file: &Path) -> Vec<Application> {
    platform::applications(file)
}

/// Returns the `active_run_id` of each subscription in the answer of
/// `subscriptions.list` whose status is pending or running.
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

/// Returns how many library items the batch's runs added, counting an item
/// once even if several runs list it. Returns `None` if the activity of any
/// run could not be read.
fn new_items(activities: &[Option<serde_json::Value>]) -> Option<usize> {
    let mut keys = BTreeSet::new();
    for activity in activities {
        for key in activity.as_ref()?["new_library_item_keys"].as_array()? {
            keys.insert(key.as_str()?.to_owned());
        }
    }
    Some(keys.len())
}

/// Returns the body text of the "Subscriptions finished" notification for
/// `count` new library items.
fn finished_body(count: usize) -> String {
    match count {
        0 => "All subscriptions finished. No new library items were added.".into(),
        1 => "1 new library item is ready to review.".into(),
        count => format!(
            "{} new library items are ready to review.",
            crate::i18n::format::grouped(count as i128)
        ),
    }
}

/// Returns the number the badge shows: the Inbox count, or 9,999 if the count
/// is larger.
fn badge_count(count: &serde_json::Value) -> u64 {
    count
        .as_u64()
        .or_else(|| count.as_f64().map(|count| count.max(0.) as u64))
        .unwrap_or(0)
        .min(MAX_BADGE)
}

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

/// On Windows there is no badge, because Electron's `setBadgeCount` does
/// nothing there. The notification is shown as a toast through GPUI. Files are
/// put on the clipboard in the `CF_HDROP` format, as the Electron app's native
/// addon does. Open With Other opens the system's own chooser.
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

/// On Linux the Inbox count is shown on the launcher, as `app.setBadgeCount`
/// does; Electron sends it as the Unity launcher's D-Bus signal. The
/// notification is shown through GPUI, which uses the freedesktop notification
/// service. Files are put on the clipboard as a `text/uri-list` of `file:`
/// URLs, as the Electron app does. There is no Open With Other, because the
/// Electron app has none on Linux.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "linux.rs"]
mod platform;

/// The id by which the desktop knows the app. Windows groups the app's toasts
/// under this id, Linux names the app's windows and its desktop file after
/// it, and the packaged macOS app has it as its bundle identifier
/// (`packaging/macos/Info.plist`).
///
/// A release build is Picto itself: it has the id and the name that the
/// Electron app has, because it is installed over the Electron app as its
/// next version. A debug build keeps an id and a name of its own, so that a
/// developer's window is never taken for the installed app.
pub const APP_ID: &str = if cfg!(debug_assertions) {
    "dev.picto.gpui-experiment"
} else {
    "com.picto.desktop"
};
/// The app's name, as the system and the window titles show it.
pub const APP_NAME: &str = if cfg!(debug_assertions) {
    crate::i18n::en("Picto GPUI")
} else {
    // i18n: the app's name, which is the same in every language
    "Picto"
};

/// The D-Bus object path from which the badge's signal to the Unity launcher
/// is sent; libunity sends from the same kind of path. The signal also carries
/// [`launcher_uri`], which names the app's desktop file and so tells the
/// launcher which entry to update.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const LAUNCHER_PATH: &str = "/com/canonical/unity/launcherentry/picto";

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launcher_uri() -> String {
    format!("application://{APP_ID}.desktop")
}

/// Shows a notification on Windows and Linux through GPUI's system
/// notifications, under [`APP_ID`]. The Electron app shows it with
/// `new Notification({ title, body }).show()`.
#[cfg(not(target_os = "macos"))]
fn system_notification(title: &str, body: &str, cx: &App) {
    cx.show_system_notification(gpui::SystemNotification {
        tag: "subscriptions-finished".into(),
        title: title.to_owned().into(),
        body: body.to_owned().into(),
        actions: Vec::new(),
    });
}

/// Returns the `DROPFILES` list (`CF_HDROP`) of `paths`, with each path in the
/// form [`system_path`] gives. The vendored GPUI's drag source builds it, so
/// that Copy and a drag out of the window give Explorer the same list.
#[cfg(target_os = "windows")]
fn drop_files(paths: &[PathBuf]) -> Vec<u8> {
    let paths: Vec<PathBuf> = paths.iter().map(|path| system_path(path)).collect();
    gpui_windows::drag_drop_files(&paths)
}

/// Returns the `text/uri-list` that the Electron app writes on Linux. The
/// vendored GPUI's X11 drag source builds it, so that Copy and a drag out of
/// the window offer the same list.
#[cfg(all(unix, not(target_os = "macos")))]
fn uri_list(paths: &[PathBuf]) -> Vec<u8> {
    gpui_linux::xdnd_uri_list(paths)
}

/// Returns `path` in the form that other applications accept. The library's
/// root is a canonical path, which on Windows starts with the `\\?\` prefix.
/// Explorer, the clipboard and other applications do not accept that prefix,
/// so this function removes it (`\\?\UNC\server\share` becomes
/// `\\server\share`).
pub fn system_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{share}"))
    } else if let Some(local) = text.strip_prefix(r"\\?\") {
        PathBuf::from(local)
    } else {
        path.to_path_buf()
    }
}

/// Sorts the applications in the Electron app's order: the default application
/// first, then the rest by name.
#[cfg(any(test, target_os = "macos"))]
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
    use std::path::{Path, PathBuf};

    /// The `CF_HDROP` list has the layout that the Electron app's native addon
    /// builds: a 20-byte `DROPFILES` header, then the paths as UTF-16 text,
    /// each ended by a zero, and one more zero at the end.
    #[cfg(windows)]
    #[test]
    fn files_go_to_the_windows_clipboard_as_a_wide_dropfiles_list() {
        let paths = [PathBuf::from(r"\\?\C:\one.jpg"), PathBuf::from("é")];
        let payload = super::drop_files(&paths);
        assert_eq!(&payload[..4], &20u32.to_le_bytes());
        assert_eq!(&payload[4..16], &[0; 12]);
        assert_eq!(&payload[16..20], &1i32.to_le_bytes());
        let units: Vec<u16> = payload[20..]
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let expected: Vec<u16> = r"C:\one.jpg"
            .encode_utf16()
            .chain([0])
            .chain("é".encode_utf16())
            .chain([0, 0])
            .collect();
        assert_eq!(units, expected);
    }

    /// Checks what the Electron app's test "Linux writes the standard
    /// URI-list clipboard format" in `clipboardImport.test.mjs` checks.
    #[cfg(target_os = "linux")]
    #[test]
    fn files_go_to_the_linux_clipboard_as_a_uri_list() {
        let paths = [PathBuf::from("/one two.jpg"), PathBuf::from("/three.png")];
        assert_eq!(
            super::uri_list(&paths),
            b"file:///one%20two.jpg\r\nfile:///three.png\r\n"
        );
    }

    #[test]
    fn windows_verbatim_paths_lose_their_prefix_for_the_desktop() {
        assert_eq!(
            super::system_path(Path::new(r"\\?\C:\Temp\a.jpg")),
            PathBuf::from(r"C:\Temp\a.jpg")
        );
        assert_eq!(
            super::system_path(Path::new(r"\\?\UNC\server\share\a.jpg")),
            PathBuf::from(r"\\server\share\a.jpg")
        );
        assert_eq!(
            super::system_path(Path::new("/tmp/a.jpg")),
            PathBuf::from("/tmp/a.jpg")
        );
    }

    /// A canonical path inside the copy's root counts as the copy's. The root
    /// itself, a sibling folder whose name starts with the root's name, a path
    /// that leaves the root with `..`, and a path anywhere else do not.
    #[cfg(unix)]
    #[test]
    fn only_paths_below_the_copy_are_the_copys() {
        let root = Path::new("/private/tmp/picto-gpui-library.a1/Main.library");
        let inside = |path: &str| super::inside(root, Path::new(path));
        assert!(inside(
            "/private/tmp/picto-gpui-library.a1/Main.library/blobs/f/ab/cd/x.jpg"
        ));
        assert!(!inside("/private/tmp/picto-gpui-library.a1/Main.library"));
        assert!(!inside(
            "/private/tmp/picto-gpui-library.a1/Main.library2/x.jpg"
        ));
        assert!(!inside(
            "/private/tmp/picto-gpui-library.a1/Main.library/../Main.library2/x.jpg"
        ));
        assert!(!inside("/home/someone/Pictures/Main.library/blobs/x.jpg"));
        assert!(!inside("blobs/x.jpg"));
        // On Linux and on a case-sensitive Mac volume, two names that differ
        // only in case are different files. The canonical path has the case
        // that the disk has, so names are compared exactly.
        assert!(!inside(
            "/private/tmp/picto-gpui-library.a1/main.library/blobs/x.jpg"
        ));
    }

    /// On Windows the canonical root has the verbatim prefix and the stored
    /// path may not have it. Names are compared without regard to case.
    #[cfg(windows)]
    #[test]
    fn only_paths_below_the_copy_are_the_copys() {
        let root =
            Path::new(r"\\?\C:\Work\Temp\picto-gpui-library.a1\Main.library");
        let inside = |path: &str| super::inside(root, Path::new(path));
        assert!(inside(
            r"C:\Work\Temp\picto-gpui-library.a1\Main.library\blobs\f\x.jpg"
        ));
        assert!(inside(
            r"\\?\c:\work\temp\picto-gpui-library.a1\main.library\blobs\x.jpg"
        ));
        assert!(!inside(
            r"C:\Work\Temp\picto-gpui-library.a1\Main.library"
        ));
        assert!(!inside(
            r"C:\Work\Temp\picto-gpui-library.a1\Main.library.old\x.jpg"
        ));
        assert!(!inside(
            r"C:\Work\Temp\picto-gpui-library.a1\Main.library\..\x.jpg"
        ));
        assert!(!inside(
            r"D:\Work\Temp\picto-gpui-library.a1\Main.library\x.jpg"
        ));
        assert!(!inside(r"C:\Work\Pictures\Main.library\blobs\x.jpg"));
        // The same rules hold for a copy on a network share.
        let share = Path::new(r"\\?\UNC\server\share\picto-gpui-library.a1\Main.library");
        assert!(super::inside(
            share,
            Path::new(r"\\server\share\picto-gpui-library.a1\Main.library\blobs\x.jpg")
        ));
        assert!(!super::inside(
            share,
            Path::new(r"\\other\share\picto-gpui-library.a1\Main.library\blobs\x.jpg")
        ));
    }

    /// This test uses real files. A file in the copy is returned by its
    /// canonical path. A missing file, a file outside the copy and, where
    /// links can be made, a link in the copy that points outside it are
    /// refused. When one path of a request is refused, no path of that request
    /// is returned.
    #[test]
    fn only_the_copys_own_files_are_handed_to_the_desktop() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("Main.library");
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        let file = root.join("blobs").join("a.jpg");
        std::fs::write(&file, b"a").unwrap();
        let outside = folder.path().join("kept.jpg");
        std::fs::write(&outside, b"b").unwrap();
        let root = root.canonicalize().unwrap();
        let handed = super::checked_in(&root, &[file.clone()]).unwrap();
        assert_eq!(handed, [file.canonicalize().unwrap()]);
        assert!(super::checked_in(&root, &[outside.clone()]).is_err());
        assert!(super::checked_in(&root, &[file.clone(), outside]).is_err());
        assert!(super::checked_in(&root, &[root.join("blobs").join("gone.jpg")]).is_err());
        assert!(
            super::checked_in(
                &root,
                &[root.join("blobs").join("..").join("..").join("kept.jpg")]
            )
            .is_err()
        );
        #[cfg(unix)]
        {
            let link = root.join("blobs").join("link.jpg");
            std::os::unix::fs::symlink(folder.path().join("kept.jpg"), &link).unwrap();
            assert!(super::checked_in(&root, &[link]).is_err());
        }
    }

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
        Application, active_runs, finished_body, new_items, sort_applications,
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

    /// This test writes to a pasteboard made for the test, never to the user's
    /// clipboard, and reads back the `NSFilenamesPboardType` list that Finder
    /// and GPUI read.
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
        // SAFETY: The messages sent are documented selectors of
        // NSPasteboard, with the argument types they take. The pasteboard
        // is made for this test and is released at the end.
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

    /// A test cannot open a file in an application without that application
    /// appearing on the screen. So this test only checks that the selectors
    /// `open_with` sends exist in AppKit with the argument types it uses.
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

    /// Asking which applications open a file type changes nothing on the
    /// desktop. Every Mac has at least one application that opens a PNG, and a
    /// default application for it, so the test expects both.
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
        // If one run's activity could not be read, the count is `None`.
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
}
