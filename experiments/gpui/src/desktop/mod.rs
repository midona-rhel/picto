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
//! nothing opens on the screen and the clipboard is left alone. Only files
//! inside the open copy are handed over, whatever path the library stored
//! (the hand-offs, the drag out of the grid and "Copy File Path" refuse any
//! other with a notification).
use crate::application::{self, Event};
use crate::session::{Coalesced, Session};
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
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

/// Notes the session's library as the only one whose files may be handed to
/// the desktop, and starts the badge and the subscription notification for
/// it, if the app enabled them.
pub fn start(session: &Entity<Session>, cx: &mut App) {
    let root = session.read(cx).root().to_path_buf();
    cx.set_global(HandOff(root));
    if !cx.has_global::<Enabled>() {
        return;
    }
    let desktop = cx.new(|cx| Desktop::new(session.clone(), cx));
    cx.set_global(Running { _desktop: desktop });
}

/// The desktop services, kept for the app's life.
struct Running {
    _desktop: Entity<Desktop>,
}

impl Global for Running {}

struct Desktop {
    session: Entity<Session>,
    /// The count the badge shows.
    shown: Option<u64>,
    badge_timer: Option<Task<()>>,
    /// The Inbox count is being read.
    badge_reading: Coalesced,
    /// The runs of the batch being watched, and whether one has been seen
    /// running.
    batch: BTreeSet<i64>,
    batch_active: bool,
    settle_timer: Option<Task<()>>,
    /// The subscriptions and their finished runs are being read.
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
                        if notifications_allowed(cx) {
                            platform::notify("Subscriptions finished", &finished_body(count), cx);
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
                if this.settle_reading.end() {
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

/// The canonical root of the open library copy. Only its own files are
/// handed to the desktop: a stored path that still names the library the
/// copy was made from, or anything else outside the copy, would let another
/// application edit, move or delete a file the user keeps.
struct HandOff(PathBuf);

impl Global for HandOff {}

/// Whether `path` lies below `root`. Both are canonical, so neither holds a
/// link or a `..` step; Windows' verbatim prefix (`\\?\C:\`, `\\?\UNC\`) is
/// read as the plain form it stands for, and Windows compares names without
/// case, as its file systems do. The root itself is not inside.
fn inside(root: &Path, path: &Path) -> bool {
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

/// `paths`, each as the canonical path of a file of the open copy, or why
/// they cannot be handed to the desktop: one of them is missing or lies
/// outside the copy.
fn checked(paths: &[PathBuf], cx: &App) -> Result<Vec<PathBuf>, String> {
    match cx.try_global::<HandOff>() {
        Some(HandOff(root)) => checked_in(root, paths),
        None => Err("No library is open.".into()),
    }
}

/// [`checked`] against the copy at the canonical `root`.
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

/// [`checked`] for one file.
fn checked_one(path: &Path, cx: &App) -> Result<PathBuf, String> {
    checked(&[path.to_path_buf()], cx).map(|mut paths| paths.remove(0))
}

/// The files of the open copy a drag out of the window hands to the system:
/// all of them, or none, with a notification, when one is not the copy's.
pub fn drag_out(paths: &[PathBuf], cx: &mut App) -> Vec<PathBuf> {
    match checked(paths, cx) {
        Ok(paths) => paths,
        Err(error) => {
            crate::ui::notify::error("Could not drag files", error, cx);
            Vec::new()
        }
    }
}

/// What was asked of the desktop in a run that does not reach it.
#[cfg(test)]
static REQUESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether this run reaches the desktop: only the app proper does; the
/// off-screen checks write each request down instead.
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

/// `shell.openPath`: the file in its default application, if it is one of
/// the copy's.
pub fn open_default(path: &Path, cx: &mut App) {
    let path = match checked_one(path, cx) {
        Ok(path) => path,
        Err(error) => return crate::ui::notify::error("Could not open file", error, cx),
    };
    if live(cx) {
        cx.open_with_system(&system_path(&path));
    } else {
        record(format!("open-default {}", path.display()));
    }
}

/// `shell.showItemInFolder`: Finder or Explorer, with the file selected, if
/// it is one of the copy's.
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

/// `picto:shell:openWithApplication`: the file, if it is one of the copy's,
/// in the application at `application` (macOS only, as in production).
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

/// `writeClipboardFilePaths`: the files themselves on the clipboard, each
/// once, for Finder or another application to paste, if all are the copy's.
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

/// "Copy File Path": the paths as text, one a line, if all are the copy's
/// files. The text is the path as the system writes it.
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

/// `picto:shell:openWithChooser`: the system's own application chooser on
/// the file, if it is one of the copy's, which only Windows offers (its
/// "Open with" dialog).
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
#[path = "macos.rs"]
mod platform;

/// Windows: no badge (Electron's `setBadgeCount` does nothing there), the
/// notification as a toast through GPUI, the files on the clipboard as
/// production's native addon puts them (`CF_HDROP`), and Open With Other as
/// the system's own chooser.
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

/// Linux: the Inbox count on the launcher (`app.setBadgeCount`, which
/// Electron sends as the Unity launcher's D-Bus signal), the notification
/// through GPUI (the freedesktop notification service), the files on the
/// clipboard as production writes them (a `text/uri-list` of `file:` URLs),
/// and no Open With Other, as production offers none there.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "linux.rs"]
mod platform;

/// The app's identity to the desktop: Windows files its toasts under it
/// and Linux names their sender with it.
pub const APP_ID: &str = "dev.picto.gpui-experiment";
pub const APP_NAME: &str = "Picto GPUI";

/// The Unity launcher entry the badge updates: the app's desktop file, and
/// the object path libunity sends from.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const LAUNCHER_PATH: &str = "/com/canonical/unity/launcherentry/picto";

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launcher_uri() -> String {
    format!("application://{APP_ID}.desktop")
}

/// `new Notification({ title, body }).show()` off macOS, through GPUI's
/// system notifications under [`APP_ID`].
#[cfg(not(target_os = "macos"))]
fn system_notification(title: &str, body: &str, cx: &App) {
    cx.show_system_notification(gpui::SystemNotification {
        tag: "subscriptions-finished".into(),
        title: title.to_owned().into(),
        body: body.to_owned().into(),
        actions: Vec::new(),
    });
}

/// A `DROPFILES` clipboard payload (`CF_HDROP`): the 20-byte header (the
/// list's offset, an unused point and client flag, and wide characters on),
/// then each path in UTF-16 ending in a NUL, and a NUL ending the list.
#[cfg(any(test, target_os = "windows"))]
fn drop_files(paths: &[PathBuf]) -> Vec<u8> {
    const HEADER: u32 = 20;
    let mut payload = Vec::new();
    payload.extend(HEADER.to_le_bytes());
    payload.extend([0u8; 12]);
    payload.extend(1i32.to_le_bytes());
    for path in paths {
        let path = system_path(path);
        for unit in path.to_string_lossy().encode_utf16().chain([0]) {
            payload.extend(unit.to_le_bytes());
        }
    }
    payload.extend(0u16.to_le_bytes());
    payload
}

/// The `text/uri-list` production writes on Linux: each file's `file:` URL,
/// each line ending in CRLF.
#[cfg(any(not(any(target_os = "macos", target_os = "windows")), all(test, unix)))]
fn uri_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .filter_map(|path| url::Url::from_file_path(path).ok())
        .map(|url| format!("{url}\r\n"))
        .collect()
}

/// A path as the rest of the desktop takes it. The library's root is a
/// canonical path, which on Windows is written with the `\\?\` prefix that
/// Explorer, the clipboard and other applications do not read; it comes off
/// here (`\\?\UNC\server\share` becomes `\\server\share`).
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
    use std::path::{Path, PathBuf};

    /// A drag out of the window hands Explorer the same `CF_HDROP` list as
    /// Copy does.
    #[cfg(windows)]
    #[test]
    fn a_drag_out_on_windows_holds_the_clipboards_file_list() {
        let paths = [PathBuf::from(r"C:\one.jpg"), PathBuf::from(r"D:\two é.png")];
        assert_eq!(
            gpui_windows::drag_drop_files(&paths),
            super::drop_files(&paths)
        );
    }

    /// A drag out of the window on X11 offers the same `text/uri-list` as
    /// Copy does.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_drag_out_on_x11_offers_the_clipboards_address_list() {
        let paths = [
            PathBuf::from("/tmp/one.jpg"),
            PathBuf::from("/tmp/two é#.png"),
        ];
        assert_eq!(
            gpui_linux::xdnd_uri_list(&paths),
            super::uri_list(&paths).into_bytes()
        );
    }

    /// `CF_HDROP`'s list, as production's native addon builds it.
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

    /// `clipboardImport.test.mjs`: "Linux writes the standard URI-list
    /// clipboard format".
    #[cfg(unix)]
    #[test]
    fn files_go_to_the_linux_clipboard_as_a_uri_list() {
        let paths = [PathBuf::from("/one two.jpg"), PathBuf::from("/three.png")];
        assert_eq!(
            super::uri_list(&paths),
            "file:///one%20two.jpg\r\nfile:///three.png\r\n"
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

    /// A canonical path below the copy's root is the copy's; the root
    /// itself, a sibling whose name starts the same, a step back out and
    /// anything elsewhere are not.
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
        assert!(!inside("/Users/someone/Pictures/Main.library/blobs/x.jpg"));
        assert!(!inside("blobs/x.jpg"));
        // Names differ by case on Linux and on a case-sensitive Mac volume;
        // the canonical path has the case the disk has, so no folding.
        assert!(!inside(
            "/private/tmp/picto-gpui-library.a1/main.library/blobs/x.jpg"
        ));
    }

    /// Windows: the canonical root carries the verbatim prefix and the
    /// stored path may not; names compare without case.
    #[cfg(windows)]
    #[test]
    fn only_paths_below_the_copy_are_the_copys() {
        let root =
            Path::new(r"\\?\C:\Users\me\AppData\Local\Temp\picto-gpui-library.a1\Main.library");
        let inside = |path: &str| super::inside(root, Path::new(path));
        assert!(inside(
            r"C:\Users\me\AppData\Local\Temp\picto-gpui-library.a1\Main.library\blobs\f\x.jpg"
        ));
        assert!(inside(
            r"\\?\c:\users\me\appdata\local\temp\picto-gpui-library.a1\main.library\blobs\x.jpg"
        ));
        assert!(!inside(
            r"C:\Users\me\AppData\Local\Temp\picto-gpui-library.a1\Main.library"
        ));
        assert!(!inside(
            r"C:\Users\me\AppData\Local\Temp\picto-gpui-library.a1\Main.library.old\x.jpg"
        ));
        assert!(!inside(
            r"C:\Users\me\AppData\Local\Temp\picto-gpui-library.a1\Main.library\..\x.jpg"
        ));
        assert!(!inside(
            r"D:\Users\me\AppData\Local\Temp\picto-gpui-library.a1\Main.library\x.jpg"
        ));
        assert!(!inside(r"C:\Users\me\Pictures\Main.library\blobs\x.jpg"));
        // A copy on a network share.
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

    /// Real files: one in the copy is handed over by its canonical path; a
    /// missing one, one outside, and (where links can be made) a link in the
    /// copy that leads out are refused, and then nothing is handed over.
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
