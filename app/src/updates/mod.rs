//! Software updates.
//!
//! The app looks for a newer release in the list of Picto's releases on
//! GitHub. It does so when the user chooses File › Check for Updates… or
//! Check Now in Settings, 15 seconds after the app starts, and every six
//! hours after that. It shows what it found in a notification and in the
//! update dialog (`dialog.rs`).
//!
//! On Windows and Linux the app downloads the installer that the release's
//! feed names (`feed.rs`), shows the progress of the download, and checks
//! the downloaded file against the SHA-512 and the size that the feed gives.
//! The installer is started when the user chooses Restart and Install, or
//! when the app quits while a downloaded update is waiting. On macOS the app
//! downloads nothing, and the dialog's button opens the release's page in
//! the browser.
//!
//! The first start of a version shows that version's release notes, which
//! are built into the app, until the user closes the dialog.
//!
//! Only a packaged build checks for updates (see [`packaged`]). In any other
//! build the dialog and Settings say that update checks are not available.
//! Three environment variables of `seams.rs` change this, so that the
//! updates can be checked without a release. With `PICTO_GPUI_NET_ORIGIN` a
//! debug build counts as packaged and reads the releases and the feed from
//! that loopback address. With `PICTO_GPUI_HOLD_DOWNLOADS` the installer is
//! not fetched, and the download stays at a given progress or fails. With
//! `PICTO_GPUI_UPDATE_PLATFORM` the app follows the steps of another
//! operating system, and it then never starts the installer.
pub mod dialog;
pub mod feed;

use crate::i18n::{en, t};
use crate::net::{self, Reach};
use feed::{Feed, FeedFile, Release};
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use sha2::Digest as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The app's version, which is the `version` in the repository's
/// `package.json`. The build script sets it.
pub const VERSION: &str = env!("PICTO_VERSION");
/// This version's release notes. The build script copies them from
/// `docs/<version>-release-notes.md`. The text is empty when that file does
/// not exist.
const BUNDLED_NOTES: &str = include_str!(concat!(env!("OUT_DIR"), "/release-notes.md"));
/// The name of the file, in the folder where the app keeps its state, that
/// holds the version whose release notes the user has closed.
const ACKNOWLEDGED: &str = "acknowledged-update-release.json";
/// How long after the app starts the first automatic check runs.
const FIRST_CHECK: Duration = Duration::from_secs(15);
/// How long the app waits between one automatic check and the next.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How long a notification about an update stays on the screen.
const NOTICE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Unavailable,
    Checking,
    Current,
    Available,
    Downloading,
    Downloaded,
    Installed,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub percent: f64,
    pub transferred: u64,
    pub total: u64,
}

/// What the app knows about updates at this moment. The update dialog and
/// Settings show it.
#[derive(Clone, Debug)]
pub struct State {
    pub status: Status,
    /// The operating system whose update steps the app follows: `darwin`,
    /// `win32` or `linux`.
    pub platform: &'static str,
    pub version: Option<String>,
    pub release_name: Option<String>,
    pub release_date: Option<String>,
    pub release_notes: String,
    pub release_url: String,
    pub progress: Option<Progress>,
    pub error: Option<String>,
    /// The path of the downloaded installer, once it has been checked
    /// against the feed.
    pub downloaded: Option<PathBuf>,
}

impl State {
    /// Returns whether a check or a download is running.
    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Checking | Status::Downloading)
    }

    /// Returns whether the app follows the steps of macOS, where it opens
    /// the release's page. On Windows and Linux it downloads the installer
    /// and starts it.
    pub fn mac(&self) -> bool {
        self.platform == "darwin"
    }
}

/// Returns the operating system whose update steps the app follows. It is
/// the one that this process runs on, unless `PICTO_GPUI_UPDATE_PLATFORM`
/// names another.
fn platform() -> &'static str {
    match crate::seams::update_platform().as_deref() {
        Some("darwin") => "darwin",
        Some("win32") => "win32",
        Some("linux") => "linux",
        _ => match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            _ => "linux",
        },
    }
}

/// Returns the operating system that this process runs on, as `darwin`,
/// `win32` or `linux`.
fn system() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        _ => "linux",
    }
}

/// Returns whether this is a packaged build. Only a packaged build checks
/// for updates. In any other build the status is `Unavailable`, with the
/// error `UNAVAILABLE`.
///
/// A packaged build is a release build whose program is not in Cargo's
/// build folder (`target/<profile>/` or `target/<triple>/<profile>/`). On
/// Linux it must also run from an AppImage, whose path `$APPIMAGE` holds,
/// because the AppImage is the only Linux package of Picto and the only one
/// that an update can replace.
///
/// When `PICTO_GPUI_NET_ORIGIN` is set, a debug build counts as packaged too,
/// and the app reads the releases and the feed from that loopback address.
/// On Linux it must still run from an AppImage, unless
/// `PICTO_GPUI_UPDATE_PLATFORM` makes it follow the steps of another
/// operating system. When any other variable of `seams.rs` is set, and in a
/// test build, no build is packaged.
pub fn packaged() -> bool {
    let appimage = || {
        !(cfg!(target_os = "linux") && platform() == "linux")
            || crate::desktop::app_image().is_some()
    };
    match net::reach() {
        Reach::Nowhere => false,
        Reach::Local(_) => appimage(),
        Reach::Real => {
            !cfg!(debug_assertions)
                && std::env::current_exe().is_ok_and(|exe| !in_build_folder(&exe))
                && appimage()
        }
    }
}

/// Returns whether the program `exe` is in Cargo's build folder:
/// `target/debug/`, `target/release/`, or one of these two under
/// `target/<triple>/`.
fn in_build_folder(exe: &Path) -> bool {
    let names: Vec<String> = exe
        .ancestors()
        .skip(1)
        .take(3)
        .map(|folder| {
            folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    let profile = |name: &String| name == "debug" || name == "release";
    match names.as_slice() {
        [profile_name, target, ..] if profile(profile_name) && target == "target" => true,
        [profile_name, _, target] if profile(profile_name) && target == "target" => true,
        _ => false,
    }
}

/// The error text of the status `Unavailable`. It has no key in the
/// catalogs, so it is English in every language.
const UNAVAILABLE: &str = en("Update checks are available in packaged builds.");

/// The result of a check.
enum Found {
    Current,
    /// A newer release, on macOS. The app shows it and can open its page.
    Page(Release),
    /// A newer release, on Windows or Linux: the release, its feed, the
    /// installer's entry in the feed, and the address that the installer is
    /// downloaded from.
    Installer {
        release: Release,
        feed: Feed,
        file: FeedFile,
        url: String,
    },
}

pub struct Updates {
    state: State,
    /// The functions to call when the check that is running has finished.
    waiting: Vec<Box<dyn FnOnce(&mut App)>>,
    checking: Option<Task<()>>,
    downloading: Option<Task<()>>,
    _schedule: Option<Task<()>>,
    /// The status and version that were announced last, so that the same
    /// status of the same version is announced only once.
    announced: String,
    /// The installer of the last download that finished and matched the
    /// feed, with its entry in the feed.
    ready: Option<(PathBuf, FeedFile)>,
    /// Whether the installer has been started. It is not started a second
    /// time.
    install_called: bool,
    /// Starts the installer of a downloaded update when the app quits.
    _on_quit: Option<Subscription>,
}

struct Shared(Entity<Updates>);

impl Global for Shared {}

/// Returns the app's one `Updates` entity. The first call makes it.
pub fn entity(cx: &mut App) -> Entity<Updates> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let unavailable = !packaged();
    let updates = cx.new(|cx| Updates {
        state: State {
            status: if unavailable {
                Status::Unavailable
            } else {
                Status::Idle
            },
            platform: platform(),
            version: None,
            release_name: None,
            release_date: None,
            release_notes: String::new(),
            release_url: feed::RELEASES_PAGE.to_owned(),
            progress: None,
            error: unavailable.then(|| UNAVAILABLE.to_owned()),
            downloaded: None,
        },
        waiting: Vec::new(),
        checking: None,
        downloading: None,
        _schedule: None,
        announced: String::new(),
        ready: None,
        install_called: false,
        _on_quit: (!unavailable).then(|| {
            cx.on_app_quit(|updates: &mut Updates, _| {
                updates.install_on_quit();
                async {}
            })
        }),
    });
    cx.set_global(Shared(updates.clone()));
    updates
}

/// Returns a copy of the current update state.
pub fn state(cx: &mut App) -> State {
    entity(cx).read(cx).state.clone()
}

/// Shows the release notes when this version starts for the first time,
/// then starts the automatic checks. It does nothing in a build that is not
/// packaged. Only `main.rs` calls it, when the app starts.
pub fn start(cx: &mut App) {
    if !packaged() {
        return;
    }
    let updates = entity(cx);
    updates.update(cx, |updates, cx| {
        updates.restore_installed(cx);
        updates._schedule = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                if this
                    .update(cx, |updates, cx| updates.check(None, cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(EVERY).await;
            }
        }));
    });
}

/// Runs a check, for Check Now in Settings and Check Again in the dialog.
/// The result is in the state.
pub fn check(cx: &mut App) {
    entity(cx).update(cx, |updates, cx| updates.check(None, cx));
}

/// Does File › Check for Updates…: runs a check and then opens the update
/// dialog. The dialog belongs to the main window of an open library. When
/// no library is open, the result is shown in a notification instead.
pub fn check_from_menu(cx: &mut App) {
    entity(cx).update(cx, |updates, cx| {
        updates.check(
            Some(Box::new(|cx: &mut App| {
                if !show_dialog(cx) {
                    tell(cx);
                }
            })),
            cx,
        )
    });
}

/// Opens the update dialog in the main window and brings that window to the
/// front. Returns false, and opens nothing, when no library is open.
pub fn show_dialog(cx: &mut App) -> bool {
    let Some(shell) = crate::libraries::host::shell(cx) else {
        return false;
    };
    shell.update(cx, |shell, cx| shell.open_updates(cx));
    crate::app_menu::bring_forward(cx);
    true
}

/// Shows the result of a check from the menu in a notification. It is used
/// when the dialog cannot be opened.
fn tell(cx: &mut App) {
    let state = state(cx);
    match state.status {
        Status::Current => crate::ui::notify::info(t("Picto is up to date."), "", cx),
        Status::Error | Status::Unavailable => {
            crate::ui::notify::error(t("Software Update"), state.error.unwrap_or_default(), cx)
        }
        // `publish` showed a notification for a newer version when it set
        // the status.
        _ => {}
    }
}

/// Does the main button of the dialog. On macOS it opens the release's page.
/// On Windows and Linux it does Restart and Install: it starts the
/// downloaded installer and then quits the app. If the installer cannot be
/// started, the app keeps running and the dialog shows the error.
pub fn install(cx: &mut App) {
    if state(cx).mac() {
        return open_release(cx);
    }
    entity(cx).update(cx, |updates, cx| updates.install(cx));
}

/// Opens the release's page in the browser, or the page that lists all
/// releases when no release is known.
pub fn open_release(cx: &mut App) {
    let url = state(cx).release_url;
    net::open_in_browser(&url, cx);
}

/// Records that the user closed this version's release notes, by writing
/// the version to the file `ACKNOWLEDGED`, so that the next start does not
/// show them again. The status then becomes `Current`. It does nothing
/// unless the status is `Installed`.
pub fn acknowledge_installed(cx: &mut App) {
    entity(cx).update(cx, |updates, cx| {
        if updates.state.status != Status::Installed {
            return;
        }
        if let Some(folder) = crate::state::location() {
            let path = folder.join(ACKNOWLEDGED);
            let temporary = folder.join(format!("{ACKNOWLEDGED}.tmp"));
            let written = std::fs::create_dir_all(&folder)
                .and_then(|_| {
                    std::fs::write(
                        &temporary,
                        serde_json::json!({ "version": VERSION }).to_string(),
                    )
                })
                .and_then(|_| std::fs::rename(&temporary, &path));
            if let Err(error) = written {
                eprintln!("picto: the release notes could not be marked as read: {error}");
            }
        }
        updates.publish(
            |state| {
                state.status = Status::Current;
                state.version = None;
                state.release_name = None;
                state.release_date = None;
                state.release_notes.clear();
                state.release_url = feed::RELEASES_PAGE.to_owned();
                state.progress = None;
                state.error = None;
            },
            cx,
        );
    });
}

/// Sets the state that a check would have reached. It is for the line
/// `update <status> [darwin|win32|linux]` of a script that `offscreen.rs`
/// plays to save pictures of the app. A test build runs no checks, so the
/// state is set directly. The notifications of the state are shown as they
/// are after a real check.
#[cfg(test)]
pub fn show_for_pictures(status: &str, flow: &str, cx: &mut App) {
    let status = match status {
        "idle" => Status::Idle,
        "unavailable" => Status::Unavailable,
        "checking" => Status::Checking,
        "current" => Status::Current,
        "available" => Status::Available,
        "downloading" => Status::Downloading,
        "downloaded" => Status::Downloaded,
        "installed" => Status::Installed,
        "error" => Status::Error,
        other => panic!("update {other}: not a status"),
    };
    let platform = match flow {
        "darwin" => "darwin",
        "win32" => "win32",
        "linux" => "linux",
        _ => platform(),
    };
    let found = !matches!(
        status,
        Status::Idle | Status::Unavailable | Status::Checking | Status::Current | Status::Error
    );
    let version = if status == Status::Installed {
        VERSION.to_owned()
    } else {
        "0.7.0-alpha".to_owned()
    };
    entity(cx).update(cx, |updates, cx| {
        updates.publish(
            |state| {
                state.status = status;
                state.platform = platform;
                state.version = found.then(|| version.clone());
                state.release_name = found.then(|| format!("Picto {version}"));
                state.release_date = (found && status != Status::Installed)
                    .then(|| "2026-11-01T10:00:00Z".to_owned());
                state.release_notes = if found {
                    "## Highlights\n\n- Faster **imports**\n- Smoother grid scrolling\n\nThanks for testing."
                        .to_owned()
                } else {
                    String::new()
                };
                state.progress = match status {
                    Status::Downloading => Some(Progress {
                        percent: 42.,
                        transferred: 42,
                        total: 100,
                    }),
                    Status::Downloaded => Some(Progress {
                        percent: 100.,
                        transferred: 1,
                        total: 1,
                    }),
                    _ => None,
                };
                state.error = match status {
                    Status::Unavailable => Some(UNAVAILABLE.to_owned()),
                    Status::Error => Some("Release check failed (503)".to_owned()),
                    _ => None,
                };
            },
            cx,
        )
    });
}

impl Updates {
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Changes the state and redraws the views that show it. When the state
    /// has a version, and this status of this version was not announced
    /// yet, it also announces it: `Installed` opens the dialog, and
    /// `Available` and `Downloaded` show a notification with a View button
    /// that opens the dialog.
    fn publish(&mut self, change: impl FnOnce(&mut State), cx: &mut Context<Self>) {
        change(&mut self.state);
        cx.notify();
        let Some(version) = self.state.version.clone() else {
            return;
        };
        let key = format!("{:?}:{version}", self.state.status);
        if self.announced == key {
            return;
        }
        let view = |window: &mut gpui::Window, cx: &mut App| {
            let _ = window;
            show_dialog(cx);
        };
        match self.state.status {
            Status::Installed => {
                self.announced = key;
                cx.defer(|cx| {
                    show_dialog(cx);
                });
            }
            Status::Available => {
                self.announced = key;
                // The message for macOS has no key in the catalogs, so it is
                // English in every language.
                let message = if self.state.mac() {
                    en("Open the release notes to download it.")
                } else {
                    t("Updates download in the background and install after Picto closes.")
                };
                let options = crate::ui::notify::Options::new(
                    crate::tf!("Picto {value0} is available", version),
                    message,
                )
                .duration(NOTICE)
                .action(t("View"), view);
                cx.defer(move |cx| {
                    crate::ui::notify::show(crate::ui::notify::Tone::Info, options, cx)
                });
            }
            Status::Downloaded => {
                self.announced = key;
                let options = crate::ui::notify::Options::new(
                    crate::tf!("Picto {value0} is ready", version),
                    // This message has no key in the catalogs, so it is
                    // English in every language.
                    en("Restart to finish updating."),
                )
                .duration(NOTICE)
                .action(t("View"), view);
                cx.defer(move |cx| {
                    crate::ui::notify::show(crate::ui::notify::Tone::Success, options, cx)
                });
            }
            _ => {}
        }
    }

    /// Does Restart and Install. It starts the installer, not silently and
    /// with the instruction to start the app again when it has finished,
    /// and then quits the app.
    fn install(&mut self, cx: &mut Context<Self>) {
        if self.state.status != Status::Downloaded {
            // The dialog offers Restart and Install only when the update is
            // downloaded, so the user does not get here. The message is
            // written to standard error only.
            eprintln!("picto: The update has not finished downloading.");
            return;
        }
        if self.install_called {
            eprintln!("picto: install call ignored: quitAndInstallCalled is set to true");
            return;
        }
        match self.start_installer(false, true) {
            Ok(true) => {
                self.install_called = true;
                cx.defer(|cx| cx.quit());
            }
            Ok(false) => {}
            Err(error) => {
                eprintln!("picto: the update's installer was not started: {error}");
                self.publish(
                    |state| {
                        state.status = Status::Error;
                        state.error = Some(error);
                    },
                    cx,
                );
            }
        }
    }

    /// Runs when the app quits. If an update is downloaded and its
    /// installer was not started by Restart and Install, it starts the
    /// installer silently and without the instruction to start the app
    /// again.
    fn install_on_quit(&mut self) {
        if self.install_called || self.ready.is_none() {
            return;
        }
        eprintln!("picto: Auto install update on quit");
        self.install_called = true;
        if let Err(error) = self.start_installer(true, false) {
            eprintln!("picto: the update's installer was not started: {error}");
        }
    }

    /// Starts the installer of the last finished download. It does so only
    /// in a packaged build, only when the file is in this app's `updates/`
    /// folder, and only when the file still has the SHA-512 and the size
    /// that the feed gives ([`ready_installer`]). A file that fails this
    /// check is deleted. `silent` asks the installer to show no window, and
    /// `run_after` asks it to start the app again when it has finished.
    /// Returns `Ok(false)`, and starts nothing, when
    /// `PICTO_GPUI_UPDATE_PLATFORM` makes the app follow the steps of
    /// another operating system than the one it runs on.
    fn start_installer(&mut self, silent: bool, run_after: bool) -> Result<bool, String> {
        let Some((path, file)) = self.ready.clone() else {
            return Err("No update filepath provided, can't quit and install".to_owned());
        };
        if !packaged() {
            return Err(UNAVAILABLE.to_owned());
        }
        let folder = download_folder()?;
        let installer = match ready_installer(&path, &folder, &file) {
            Ok(installer) => installer,
            Err(error) => {
                // The file is not the downloaded update. It is deleted, and
                // the state no longer offers it for installing.
                let _ = std::fs::remove_file(&path);
                self.ready = None;
                self.state.downloaded = None;
                return Err(error);
            }
        };
        if self.state.platform != system() {
            eprintln!(
                "picto: would start the installer {} ({} flow on {})",
                installer.display(),
                self.state.platform,
                system()
            );
            return Ok(false);
        }
        eprintln!(
            "picto: Install: isSilent: {silent}, isForceRunAfter: {run_after}: {}",
            installer.display()
        );
        crate::desktop::install_update(&installer, silent, run_after, file.admin)?;
        Ok(true)
    }

    /// Sets the status to `Installed`, with this version's release notes,
    /// unless the file `ACKNOWLEDGED` says that the user already closed
    /// them. When this version has no release notes, the status becomes
    /// `Error`.
    fn restore_installed(&mut self, cx: &mut Context<Self>) {
        let acknowledged = crate::state::location()
            .and_then(|folder| std::fs::read_to_string(folder.join(ACKNOWLEDGED)).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("version")?.as_str().map(str::to_owned));
        if acknowledged.as_deref() == Some(VERSION) {
            return;
        }
        if BUNDLED_NOTES.trim().is_empty() {
            eprintln!("picto: no release notes are bundled for {VERSION}");
            return self.publish(
                |state| {
                    state.status = Status::Error;
                    state.error = Some(format!("Bundled release notes are empty for {VERSION}"));
                },
                cx,
            );
        }
        self.publish(
            |state| {
                state.status = Status::Installed;
                state.version = Some(VERSION.to_owned());
                state.release_name = Some(crate::tf!("Picto {value0}", VERSION));
                state.release_date = None;
                state.release_notes = BUNDLED_NOTES.to_owned();
                state.release_url = format!("{}/tag/v{VERSION}", feed::RELEASES_PAGE);
                state.progress = None;
                state.error = None;
            },
            cx,
        );
    }

    /// Starts a check, unless one is already running. No check starts in a
    /// build that is not packaged, while this version's release notes are
    /// shown, or while the installer downloads. `then` is called when the
    /// check has finished, or at once when no check starts.
    fn check(&mut self, then: Option<Box<dyn FnOnce(&mut App)>>, cx: &mut Context<Self>) {
        if !packaged() {
            if let Some(then) = then {
                cx.defer(then);
            }
            return;
        }
        let state = &self.state;
        if matches!(
            state.status,
            Status::Unavailable | Status::Installed | Status::Downloading
        ) {
            if let Some(then) = then {
                cx.defer(then);
            }
            return;
        }
        self.waiting.extend(then);
        if self.checking.is_some() {
            return;
        }
        self.publish(
            |state| {
                state.status = Status::Checking;
                state.error = None;
            },
            cx,
        );
        let platform = self.state.platform;
        let work = cx.background_spawn(async move { net::block_on(find(platform)) });
        self.checking = Some(cx.spawn(async move |this, cx| {
            let found = work.await.and_then(|found| found);
            let _ = this.update(cx, |updates, cx| {
                updates.checking = None;
                updates.checked(found, cx);
                for then in std::mem::take(&mut updates.waiting) {
                    cx.defer(then);
                }
            });
        }));
    }

    fn checked(&mut self, found: Result<Found, String>, cx: &mut Context<Self>) {
        match found {
            Err(error) => self.publish(
                |state| {
                    state.status = Status::Error;
                    state.error = Some(error);
                },
                cx,
            ),
            Ok(Found::Current) => self.publish(
                |state| {
                    state.status = Status::Current;
                    state.error = None;
                },
                cx,
            ),
            Ok(Found::Page(release)) => self.publish(
                |state| {
                    state.status = Status::Available;
                    state.version = Some(release.tag.trim_start_matches('v').to_owned());
                    state.release_name = release.name;
                    state.release_date = release.published_at;
                    state.release_notes = release.body;
                    state.release_url = release
                        .html_url
                        .unwrap_or_else(|| feed::RELEASES_PAGE.to_owned());
                    state.error = None;
                },
                cx,
            ),
            Ok(Found::Installer {
                release,
                feed,
                file,
                url,
            }) => {
                // The name, the date and the notes are the feed's. Where the
                // feed has none, they are those of the release on GitHub.
                self.publish(
                    |state| {
                        state.status = Status::Available;
                        state.version = Some(feed.version.clone());
                        state.release_name = feed.release_name.clone().or(release.name);
                        state.release_date = feed.release_date.clone().or(release.published_at);
                        state.release_notes = feed
                            .release_notes
                            .clone()
                            .filter(|notes| !notes.is_empty())
                            .unwrap_or(release.body);
                        state.progress = None;
                        state.error = None;
                        state.downloaded = None;
                    },
                    cx,
                );
                self.download(file, url, cx);
            }
        }
    }

    /// Downloads the installer in the background and publishes the progress
    /// of the download. When the download has finished and matches the
    /// feed, the status becomes `Downloaded`. When it fails or does not
    /// match, the status becomes `Error`.
    fn download(&mut self, file: FeedFile, url: String, cx: &mut Context<Self>) {
        let total = file.size.unwrap_or(0);
        if let Some(held) = crate::seams::hold_downloads() {
            eprintln!("picto: update download held: {url}");
            return match held {
                crate::seams::HeldDownload::Progress(share) => self.publish(
                    |state| {
                        state.status = Status::Downloading;
                        state.progress = Some(Progress {
                            percent: share * 100.,
                            transferred: (share * total as f64) as u64,
                            total,
                        });
                    },
                    cx,
                ),
                crate::seams::HeldDownload::Fail => self.publish(
                    |state| {
                        state.status = Status::Error;
                        state.error = Some(en("The update could not be downloaded.").to_owned());
                    },
                    cx,
                ),
            };
        }
        let Some(address) = net::address(&url) else {
            return;
        };
        let folder = match download_folder() {
            Ok(folder) => folder,
            Err(error) => {
                return self.publish(
                    |state| {
                        state.status = Status::Error;
                        state.error = Some(error);
                    },
                    cx,
                );
            }
        };
        self.ready = None;
        self.publish(
            |state| {
                state.status = Status::Downloading;
                state.progress = Some(Progress {
                    percent: 0.,
                    transferred: 0,
                    total,
                });
            },
            cx,
        );
        let (sender, mut received) = futures::channel::mpsc::unbounded();
        let described = file.clone();
        let work = cx.background_spawn(async move {
            net::block_on(fetch_installer(&address, &folder, &described, sender))
                .and_then(|done| done)
        });
        self.downloading = Some(cx.spawn(async move |this, cx| {
            use futures::StreamExt as _;
            while let Some(progress) = received.next().await {
                let _ = this.update(cx, |updates, cx| {
                    updates.publish(|state| state.progress = Some(progress), cx)
                });
            }
            let done = work.await;
            let _ = this.update(cx, |updates, cx| {
                updates.downloading = None;
                match done {
                    Ok(path) => {
                        updates.ready = Some((path.clone(), file));
                        updates.publish(
                            |state| {
                                state.status = Status::Downloaded;
                                state.progress = Some(Progress {
                                    percent: 100.,
                                    transferred: total,
                                    total,
                                });
                                state.downloaded = Some(path);
                                state.error = None;
                            },
                            cx,
                        )
                    }
                    Err(error) => updates.publish(
                        |state| {
                            state.status = Status::Error;
                            state.progress = None;
                            state.error = Some(error);
                        },
                        cx,
                    ),
                }
            });
        }));
    }
}

/// Returns the folder where downloaded installers are kept: `updates/` in
/// the folder where the app keeps its state. An installer is started only
/// from this folder.
fn download_folder() -> Result<PathBuf, String> {
    crate::state::location()
        .map(|folder| folder.join("updates"))
        .ok_or_else(|| "This app has no folder to keep the update in.".to_owned())
}

/// Returns the path of the installer to start. `path` must be a file
/// directly in `folder` with the file name that the feed gives, and the file
/// must have the SHA-512 and the size that the feed gives. The file is read
/// again to check this. Returns an error in every other case.
fn ready_installer(path: &Path, folder: &Path, file: &FeedFile) -> Result<PathBuf, String> {
    let name = file
        .name()
        .ok_or_else(|| "The update feed names no installer file.".to_owned())?;
    let canonical = |path: &Path| std::fs::canonicalize(path).ok();
    let expected = folder.join(&name);
    let in_folder = path.file_name() == Some(std::ffi::OsStr::new(&name))
        && canonical(path.parent().unwrap_or(path)).is_some()
        && canonical(path.parent().unwrap_or(path)) == canonical(folder);
    // The file must be a regular file. A symbolic link to a file in another
    // folder is refused.
    let plain = std::fs::symlink_metadata(&expected).is_ok_and(|meta| meta.file_type().is_file());
    if !in_folder || !plain {
        return Err(format!(
            "The installer {} is not this app's downloaded update.",
            path.display()
        ));
    }
    let mut reader = std::fs::File::open(&expected).map_err(|error| error.to_string())?;
    let mut hasher = feed::hasher();
    let length = std::io::copy(&mut reader, &mut hasher).map_err(|error| error.to_string())?;
    feed::verify(&hasher.finalize(), length, file)?;
    Ok(expected)
}

/// Asks GitHub for the list of releases and takes the newest release of the
/// app that is newer than this version. On Windows and Linux it then reads
/// that release's feed and returns the installer that the feed names.
async fn find(platform: &'static str) -> Result<Found, String> {
    let client = net::client()?;
    let Some(list) = net::address(feed::RELEASES_API) else {
        return Err(UNAVAILABLE.to_owned());
    };
    let response = client
        .get(&list)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header(reqwest::header::USER_AGENT, "Picto-Updater")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Release check failed ({})",
            response.status().as_u16()
        ));
    }
    let releases: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
    let Some(release) = feed::latest_app_release(&releases, VERSION) else {
        return Ok(Found::Current);
    };
    let Some(file_name) = feed::feed_file(platform) else {
        return Ok(Found::Page(release));
    };
    let feed_url = feed::asset_url(&release.tag, file_name);
    let address = net::address(&feed_url).unwrap_or_default();
    let response = client
        .get(&address)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Cannot find {file_name} in the latest release artifacts ({feed_url}): HttpError: {}",
            response.status().as_u16()
        ));
    }
    let text = response.text().await.map_err(|error| error.to_string())?;
    let parsed = feed::parse_feed(&text)?;
    if !feed::is_newer(&parsed.version, VERSION) {
        return Ok(Found::Current);
    }
    let Some(file) = feed::installer(&parsed, platform).cloned() else {
        return Err(format!(
            "The update feed {file_name} names no installer for this system."
        ));
    };
    let url = feed::asset_url(&release.tag, &file.url);
    Ok(Found::Installer {
        release,
        feed: parsed,
        file,
        url,
    })
}

/// Downloads the installer at `address` into `folder` under the file name
/// that the feed gives, and sends the progress of the download to
/// `progress`. The file is kept only when it has the SHA-512 and the size
/// that the feed gives. An incomplete download and a file that does not
/// match are deleted. When a file with that name is already in the folder
/// and matches the feed, nothing is downloaded.
async fn fetch_installer(
    address: &str,
    folder: &Path,
    file: &FeedFile,
    progress: futures::channel::mpsc::UnboundedSender<Progress>,
) -> Result<PathBuf, String> {
    let name = file
        .name()
        .ok_or_else(|| "The update feed names no installer file.".to_owned())?;
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let destination = folder.join(&name);
    if let Ok(bytes) = std::fs::read(&destination) {
        let digest = sha2::Sha512::digest(&bytes);
        if feed::verify(&digest, bytes.len() as u64, file).is_ok() {
            return Ok(destination);
        }
        let _ = std::fs::remove_file(&destination);
    }
    let partial = folder.join(format!("{name}.part"));
    let fetched = async {
        let client = net::client()?;
        let mut response = client
            .get(address)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "The update could not be downloaded ({}).",
                response.status().as_u16()
            ));
        }
        let total = response.content_length().or(file.size).unwrap_or(0);
        let mut out = std::fs::File::create(&partial).map_err(|error| error.to_string())?;
        let mut hasher = feed::hasher();
        let mut transferred = 0_u64;
        while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
            out.write_all(&chunk).map_err(|error| error.to_string())?;
            hasher.update(&chunk);
            transferred += chunk.len() as u64;
            let percent = if total > 0 {
                transferred as f64 / total as f64 * 100.
            } else {
                0.
            };
            let _ = progress.unbounded_send(Progress {
                percent,
                transferred,
                total,
            });
        }
        out.sync_all().map_err(|error| error.to_string())?;
        drop(out);
        feed::verify(&hasher.finalize(), transferred, file)?;
        std::fs::rename(&partial, &destination).map_err(|error| error.to_string())?;
        Ok(destination)
    }
    .await;
    if fetched.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    fetched
}

#[cfg(test)]
mod tests;
