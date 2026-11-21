//! Software updates (`updateService.mjs`, `updateRuntime.ts`,
//! `UpdateModal.tsx`): production's release feed checked by hand (File ›
//! Check for Updates…, Settings' Check Now) and 15 s after start and every
//! six hours; what it found, told in a notification and the update dialog;
//! on Windows and Linux the installer named by the feed downloaded with its
//! progress and verified against the feed's SHA-512 and size, then
//! installed as electron-updater installs it (Restart and Install, or when
//! the app quits with the update waiting); on macOS the release page, as
//! production opens it. The first start of a version shows that version's
//! bundled release notes until the dialog is closed, as production does.
//!
//! Checks run only where production's would (`app.isPackaged`), see
//! [`packaged`]; elsewhere the dialog and Settings show production's
//! "unavailable" state. A debug build is packaged only under the checks'
//! seam `PICTO_GPUI_NET_ORIGIN`, against its local feed.
//! `PICTO_GPUI_HOLD_DOWNLOADS` holds or fails the download instead of
//! fetching the installer, and `PICTO_GPUI_UPDATE_PLATFORM` follows another
//! system's flow, whose installer is then never started.
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

/// The version this app reports: production's `package.json` version.
pub const VERSION: &str = env!("PICTO_VERSION");
/// `docs/<version>-release-notes.md`, bundled with the build.
const BUNDLED_NOTES: &str = include_str!(concat!(env!("OUT_DIR"), "/release-notes.md"));
/// `ACKNOWLEDGED_RELEASE_FILE`
const ACKNOWLEDGED: &str = "acknowledged-update-release.json";
/// The first automatic check, after start.
const FIRST_CHECK: Duration = Duration::from_secs(15);
/// Automatic checks after that.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The notifications' `duration: 10_000`.
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

/// `UpdateState`
#[derive(Clone, Debug)]
pub struct State {
    pub status: Status,
    /// `darwin`, `win32` or `linux`.
    pub platform: &'static str,
    pub version: Option<String>,
    pub release_name: Option<String>,
    pub release_date: Option<String>,
    pub release_notes: String,
    pub release_url: String,
    pub progress: Option<Progress>,
    pub error: Option<String>,
    /// The verified installer.
    pub downloaded: Option<PathBuf>,
}

impl State {
    /// A check or download is under way (`busy`).
    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Checking | Status::Downloading)
    }

    /// macOS opens the release page; the others download the installer and
    /// install it (production's `automaticInstall` is `!mac()`).
    pub fn mac(&self) -> bool {
        self.platform == "darwin"
    }
}

/// The system whose update flow this run follows.
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

/// The system this process runs on, in production's words.
fn system() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        _ => "linux",
    }
}

/// Production's `app.isPackaged`, which decides whether there are update
/// checks at all (else the state is `unavailable`, with its message), for
/// this app: a release build that is not run from Cargo's build folder
/// (`target/<profile>/`, `target/<triple>/<profile>/`), and on Linux one
/// run from an AppImage (`$APPIMAGE`, an absolute path), the only Linux
/// package production makes and the only one electron-updater installs
/// over. A debug build never is, except under the checks' seam
/// `PICTO_GPUI_NET_ORIGIN`, whose loopback feed it then follows as a
/// packaged build follows the real one; under any other seam, or in a test
/// build, it is not.
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

/// Whether `exe` is where Cargo builds it.
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

/// The unavailable state's error, which production writes in English.
const UNAVAILABLE: &str = en("Update checks are available in packaged builds.");

/// What a check found.
enum Found {
    Current,
    /// macOS: the release, for its page.
    Page(Release),
    /// Windows and Linux: the release, its feed and the installer.
    Installer {
        release: Release,
        feed: Feed,
        file: FeedFile,
        url: String,
    },
}

pub struct Updates {
    state: State,
    /// Run once the check under way has finished.
    waiting: Vec<Box<dyn FnOnce(&mut App)>>,
    checking: Option<Task<()>>,
    downloading: Option<Task<()>>,
    _schedule: Option<Task<()>>,
    /// The last status and version announced (`announced`).
    announced: String,
    /// The verified installer and what the feed says of it, from the last
    /// download that finished (`downloadedUpdateHelper`).
    ready: Option<(PathBuf, FeedFile)>,
    /// Restart and Install has started the installer
    /// (`quitAndInstallCalled`).
    install_called: bool,
    /// `autoInstallOnAppQuit`: the waiting update installed as the app
    /// quits.
    _on_quit: Option<Subscription>,
}

struct Shared(Entity<Updates>);

impl Global for Shared {}

/// The app's updates, made on first use.
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

/// The update state now.
pub fn state(cx: &mut App) -> State {
    entity(cx).read(cx).state.clone()
}

/// `start`: the release notes of a version started for the first time,
/// then the automatic checks. Only the app proper calls this.
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

/// Check Now: a check, its outcome in the state.
pub fn check(cx: &mut App) {
    entity(cx).update(cx, |updates, cx| updates.check(None, cx));
}

/// File › Check for Updates… (`checkForUpdates`): a check, then the
/// dialog (`menu:show-updates`). Without a library open there is no dialog
/// to show; the outcome is told in a notification instead.
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

/// The dialog, in the main window, if a library is open there.
pub fn show_dialog(cx: &mut App) -> bool {
    let Some(shell) = crate::libraries::host::shell(cx) else {
        return false;
    };
    shell.update(cx, |shell, cx| shell.open_updates(cx));
    crate::app_menu::bring_forward(cx);
    true
}

/// The outcome of a check from the menu without the dialog.
fn tell(cx: &mut App) {
    let state = state(cx);
    match state.status {
        Status::Current => crate::ui::notify::info(t("Picto is up to date."), "", cx),
        Status::Error | Status::Unavailable => {
            crate::ui::notify::error(t("Software Update"), state.error.unwrap_or_default(), cx)
        }
        // Announced as they arrive.
        _ => {}
    }
}

/// `install`: on macOS the release page; elsewhere Restart and Install
/// (`quitAndInstall(false, true)`): the downloaded installer started as
/// electron-updater starts it, then the app quits. If it cannot be started,
/// the app stays and the error is shown.
pub fn install(cx: &mut App) {
    if state(cx).mac() {
        return open_release(cx);
    }
    entity(cx).update(cx, |updates, cx| updates.install(cx));
}

/// `openRelease`: the release's page (or the releases page) in the
/// browser.
pub fn open_release(cx: &mut App) {
    let url = state(cx).release_url;
    net::open_in_browser(&url, cx);
}

/// `acknowledgeInstalled`: the bundled notes have been read; they are not
/// shown again for this version.
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

/// The off-screen tool's `update <status> [darwin|win32|linux]`: the state
/// a check would have reached, for pictures (a test build has no checks).
/// Its notifications show as they would.
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

    /// Changes the state, tells whoever shows it, and announces what is
    /// new (`publish` and `announce`).
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
                // Production writes the macOS message in English.
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
                    // Production writes it in English.
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

    /// `quitAndInstall(false, true)`: not silent, so the installer runs
    /// the app again afterwards (`autoRunAppAfterInstall`); then the app
    /// quits (`app.quit()`).
    fn install(&mut self, cx: &mut Context<Self>) {
        if self.state.status != Status::Downloaded {
            // Production's `install` throws this to a caller that drops it.
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

    /// `autoInstallOnAppQuit`: the app quits with an update downloaded and
    /// Restart and Install not chosen: the installer runs silently
    /// (`install(true, false)`), without starting the app again.
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

    /// `BaseUpdater.install`: the installer of the last finished download,
    /// only in a packaged build, only from this app's `updates/` folder and
    /// only if it is still the file the feed describes, started by the
    /// system's `doInstall`. `Ok(false)`: another system's flow is being
    /// followed (`PICTO_GPUI_UPDATE_PLATFORM`), so nothing was started.
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
                // Never offered again.
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

    /// `restoreInstalledRelease`: this version's notes, unless they were
    /// read before.
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

    /// `check`: one at a time; none while the notes of a new version are
    /// shown or the installer downloads. `then` runs once it has finished.
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
                // `normalizeInfo`: the feed's words first, the release's next.
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

    /// `autoDownload`: the installer, with its progress, verified.
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

/// Where downloaded installers are kept, and the only place one is ever
/// started from: `updates/` in this app's folder.
fn download_folder() -> Result<PathBuf, String> {
    crate::state::location()
        .map(|folder| folder.join("updates"))
        .ok_or_else(|| "This app has no folder to keep the update in.".to_owned())
}

/// The installer to start: the file at `path`, directly in `folder` under
/// the name the feed gives it, and still the file the feed describes (its
/// SHA-512 and size, read again now). Anything else is refused.
fn ready_installer(path: &Path, folder: &Path, file: &FeedFile) -> Result<PathBuf, String> {
    let name = file
        .name()
        .ok_or_else(|| "The update feed names no installer file.".to_owned())?;
    let canonical = |path: &Path| std::fs::canonicalize(path).ok();
    let expected = folder.join(&name);
    let in_folder = path.file_name() == Some(std::ffi::OsStr::new(&name))
        && canonical(path.parent().unwrap_or(path)).is_some()
        && canonical(path.parent().unwrap_or(path)) == canonical(folder);
    // A plain file there, not a link to one elsewhere.
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

/// `latestAppRelease`, then on Windows and Linux the release's feed.
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

/// Downloads the installer at `address` into `folder` under the name the
/// feed gives it, reporting progress, and keeps it only if it is the file
/// the feed describes; a partial or mismatching download is removed. An
/// installer already there and verified is not downloaded again.
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
