//! Reads the environment variables that the checks use to drive and observe
//! the app (README, "Checks"). Each of them is read in debug builds only. A
//! release build answers as though none was set, and contains neither the
//! names of the variables nor the code behind them. So nothing that can set
//! the environment of a shipped app can make it take input from a file, keep
//! painting while covered, hold back its network commands or keep its state
//! in another folder.
//!
//! Every build reads the two variables for debugging, `PICTO_GPUI_FPS`
//! (`meter.rs`) and `PICTO_GPUI_STATS` (`stats.rs`), and the variables that
//! configure the app on any machine (`PICTO_FFMPEG_DIR` and the platform's
//! own). The variables of the off-screen tool (`offscreen.rs`) and of the
//! tests exist in test builds only.
use std::path::PathBuf;

/// Returns the value of the variable in a debug build. In a release build it
/// returns `None`, and the name of the variable is not compiled in.
macro_rules! seam {
    ($name:literal) => {{
        #[cfg(debug_assertions)]
        let value = std::env::var_os($name);
        #[cfg(not(debug_assertions))]
        let value: Option<std::ffi::OsString> = None;
        value
    }};
}

/// Returns the file or FIFO that `PICTO_GPUI_POINTER=<file or FIFO>` names.
/// The app reads input lines from it and plays them into the main window
/// (`pointer.rs`).
#[cfg(debug_assertions)]
pub fn pointer_file() -> Option<PathBuf> {
    seam!("PICTO_GPUI_POINTER").map(PathBuf::from)
}

/// Returns whether `PICTO_GPUI_BACKGROUND` is set. The main window then opens
/// without taking the keyboard, and keeps drawing while it is covered
/// (`covered.rs`).
pub fn background() -> bool {
    seam!("PICTO_GPUI_BACKGROUND").is_some()
}

/// Returns the size that `PICTO_GPUI_WINDOW=<width>x<height>` names. The main
/// window then opens centred at exactly that size, and the size is not saved,
/// so that every picture taken of the window has the same size.
pub fn exact_window() -> Option<(f32, f32)> {
    window_size(&seam!("PICTO_GPUI_WINDOW")?.to_string_lossy())
}

/// Parses a text such as `1213x865` into a width and a height.
fn window_size(value: &str) -> Option<(f32, f32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

/// Returns whether `PICTO_GPUI_MENU=window` is set. Pop-up menus are then
/// drawn inside the window on macOS too (`ui::menu::presentation`).
pub fn menus_in_window() -> bool {
    seam!("PICTO_GPUI_MENU").is_some_and(|value| value == "window")
}

/// Returns whether `PICTO_GPUI_APP_MENU=window` is set. The window then has
/// the application menu button on macOS too (`app_menu::in_window`).
pub fn app_menu_in_window() -> bool {
    seam!("PICTO_GPUI_APP_MENU").is_some_and(|value| value == "window")
}

/// Returns the folder that `PICTO_GPUI_STATE=<folder>` names. The app then
/// keeps its state in that folder instead of in its own folder (`state.rs`).
pub fn state_folder() -> Option<PathBuf> {
    seam!("PICTO_GPUI_STATE")
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// Returns the width that `PICTO_GPUI_INSPECTOR=<width>` names. The inspector
/// then opens at that width, whatever width was saved.
pub fn inspector_width() -> Option<f32> {
    seam!("PICTO_GPUI_INSPECTOR")?.to_str()?.trim().parse().ok()
}

/// Returns whether `PICTO_GPUI_HOLD_RUNS` is set. The Subscriptions screen
/// then records each of its commands. A command that would start network work
/// is not sent; the screen answers it itself.
pub fn hold_runs() -> bool {
    seam!("PICTO_GPUI_HOLD_RUNS").is_some()
}

/// Returns the origin that `PICTO_GPUI_LOGIN_ORIGIN=http://127.0.0.1:<port>`
/// names. The addresses of every site login are then moved onto that origin,
/// with their path and query kept, and the sign-in window loads nothing
/// outside it. A check can then run a whole sign-in against local pages
/// without reaching a real site (`logins::sites`). Only a loopback `http:`
/// origin is accepted.
pub fn login_origin() -> Option<String> {
    loopback_origin(seam!("PICTO_GPUI_LOGIN_ORIGIN")?.to_str()?)
}

/// Returns the origin that `PICTO_GPUI_NET_ORIGIN=http://127.0.0.1:<port>`
/// names. The update feed and its downloads, the pages of the search engines
/// and the site icons are then requested from that origin. The host of each
/// address becomes the first part of its path: `https://tineye.com/` becomes
/// `<origin>/tineye.com/`. The checks answer every request from a local
/// server in this way (`net`). Only a loopback `http:` origin is accepted.
pub fn net_origin() -> Option<String> {
    loopback_origin(seam!("PICTO_GPUI_NET_ORIGIN")?.to_str()?)
}

/// Returns the folder that `PICTO_GPUI_NET_CHECK=<folder>` names. Together
/// with `PICTO_GPUI_NET_ORIGIN`, the app then serves the update feed, the
/// pages of the search engines and the site icons itself at that origin. It
/// runs the update check and the download, a search on every engine and the
/// requests for the icons against them, writes what happened to the folder
/// and quits (`net_check`).
#[cfg(debug_assertions)]
pub fn net_check() -> Option<PathBuf> {
    seam!("PICTO_GPUI_NET_CHECK")
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// Returns the system that `PICTO_GPUI_UPDATE_PLATFORM=darwin|win32|linux`
/// names. The update check then follows the steps of that system on any
/// machine, so that one machine can show the steps of every system
/// (`updates`).
pub fn update_platform() -> Option<String> {
    seam!("PICTO_GPUI_UPDATE_PLATFORM")
        .and_then(|value| value.into_string().ok())
        .filter(|value| matches!(value.as_str(), "darwin" | "win32" | "linux"))
}

/// Returns the origin of `value` when `value` is a loopback `http:` address.
fn loopback_origin(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    let loopback = match url.host()? {
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
        url::Host::Domain(name) => name == "localhost",
    };
    (url.scheme() == "http" && loopback).then(|| url.origin().ascii_serialization())
}

/// Returns the folder that `PICTO_GPUI_LOGIN_CHECK=<folder>` names. Together
/// with `PICTO_GPUI_LOGIN_ORIGIN`, the app then signs in to a login that it
/// serves itself at that origin, in the system's sign-in window. It writes
/// what happened and pictures of the window to the folder, and quits
/// (`logins::check`).
#[cfg(debug_assertions)]
pub fn login_check() -> Option<PathBuf> {
    seam!("PICTO_GPUI_LOGIN_CHECK")
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// Returns whether `PICTO_GPUI_SILENT` is set. Every video player and every
/// Flash movie then starts muted.
pub fn silent() -> bool {
    seam!("PICTO_GPUI_SILENT").is_some()
}

/// Returns the folder that `PICTO_GPUI_APP_DATA=<folder>` names. The
/// application data that the core keeps outside a library (its AI models, in
/// `models/`) is then in that folder instead of in its usual place
/// (`application::application_data`).
pub fn app_data() -> Option<PathBuf> {
    seam!("PICTO_GPUI_APP_DATA")
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// Lists every variable above. Each of them drives, observes or redirects the
/// app.
#[cfg(debug_assertions)]
const ALL: [&str; 16] = [
    "PICTO_GPUI_POINTER",
    "PICTO_GPUI_BACKGROUND",
    "PICTO_GPUI_WINDOW",
    "PICTO_GPUI_MENU",
    "PICTO_GPUI_APP_MENU",
    "PICTO_GPUI_STATE",
    "PICTO_GPUI_INSPECTOR",
    "PICTO_GPUI_HOLD_RUNS",
    "PICTO_GPUI_SILENT",
    "PICTO_GPUI_APP_DATA",
    "PICTO_GPUI_HOLD_DOWNLOADS",
    "PICTO_GPUI_LOGIN_ORIGIN",
    "PICTO_GPUI_LOGIN_CHECK",
    "PICTO_GPUI_NET_ORIGIN",
    "PICTO_GPUI_UPDATE_PLATFORM",
    "PICTO_GPUI_NET_CHECK",
];

/// Returns the automation seam that is active in this process, if there is
/// one: the name of the first variable above that is set, or "a test build"
/// in a test build, which holds the off-screen tool, the queue of dialog
/// answers and the record of desktop requests. A process with an automation
/// seam never opens a library the user keeps (`crate::libraries`). A release
/// build has no automation seam.
pub fn automation() -> Option<&'static str> {
    if cfg!(test) {
        return Some("a test build");
    }
    #[cfg(debug_assertions)]
    {
        ALL.into_iter()
            .find(|name| std::env::var_os(name).is_some())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// Says how a held download of an AI model answers
/// (`PICTO_GPUI_HOLD_DOWNLOADS`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeldDownload {
    /// The download never finishes. The model reports this share (0 to 1) as
    /// downloaded until Cancel ends the download.
    Progress(f64),
    /// The download fails at once.
    Fail,
}

/// Returns what `PICTO_GPUI_HOLD_DOWNLOADS[=<percent>|fail]` names. Downloads
/// of AI models are then recorded and answered by the app. They are not sent
/// to the core, which would fetch the model from the network
/// (`ai_tagger::commands`).
pub fn hold_downloads() -> Option<HeldDownload> {
    held_download(&seam!("PICTO_GPUI_HOLD_DOWNLOADS")?.to_string_lossy())
}

fn held_download(value: &str) -> Option<HeldDownload> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("fail") {
        return Some(HeldDownload::Fail);
    }
    let percent = value.parse::<f64>().unwrap_or(0.).clamp(0., 100.);
    Some(HeldDownload::Progress(percent / 100.))
}
