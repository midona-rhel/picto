//! Software updates (`updateService.mjs`, `updateRuntime.ts`,
//! `UpdateModal.tsx`): production's release feed checked by hand (File ›
//! Check for Updates…, Settings' Check Now) and, where checks are live, 15 s
//! after start and every six hours; what it found, told in a notification
//! and the update dialog; on Windows and Linux the installer named by the
//! feed downloaded with its progress and verified against the feed's
//! SHA-512 and size; on macOS the release page, as production opens it.
//!
//! This app is not packaged, so it cannot install what it downloads: where
//! production restarts into the installer, the verified installer is shown
//! in the file manager (production Picto's installer; running it is the
//! user's choice). The first start of a version shows that version's
//! bundled release notes until the dialog is closed, as production does.
//!
//! Checks are live in a release build. A debug build checks only against
//! the local feed of `PICTO_GPUI_NET_ORIGIN`, and under any other seam (or
//! in a test build) nothing is fetched: the dialog says checks are not
//! available. `PICTO_GPUI_HOLD_DOWNLOADS` holds or fails the download
//! instead of fetching the installer, and `PICTO_GPUI_UPDATE_PLATFORM`
//! follows another system's flow.
pub mod dialog;
pub mod feed;

use crate::i18n::{en, t};
use crate::net::{self, Reach};
use feed::{Feed, FeedFile, Release};
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
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

    /// macOS opens the release page; the others download the installer
    /// (production's `automaticInstall`, which this app cannot honour).
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

/// Whether checks reach a feed in this run, and whether they run by
/// themselves (production's `app.isPackaged`).
fn reach() -> Reach {
    net::reach()
}

fn automatic() -> bool {
    match reach() {
        Reach::Real => !cfg!(debug_assertions),
        Reach::Local(_) => true,
        Reach::Nowhere => false,
    }
}

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
}

struct Shared(Entity<Updates>);

impl Global for Shared {}

/// The app's updates, made on first use.
pub fn entity(cx: &mut App) -> Entity<Updates> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let unavailable = reach() == Reach::Nowhere;
    let updates = cx.new(|_| Updates {
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
            error: unavailable.then(|| {
                en("Update checks are not available while the app runs under a check.").to_owned()
            }),
            downloaded: None,
        },
        waiting: Vec::new(),
        checking: None,
        downloading: None,
        _schedule: None,
        announced: String::new(),
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
    if !automatic() {
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

/// `install`: on macOS the release page; elsewhere the verified installer
/// in the file manager, since this app cannot run it in its own place.
pub fn install(cx: &mut App) {
    let state = state(cx);
    if state.mac() {
        return open_release(cx);
    }
    if let (Status::Downloaded, Some(path)) = (state.status, state.downloaded) {
        crate::desktop::reveal_download(&path, cx);
    }
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
                let message = if self.state.mac() {
                    en("Open the release notes to download it.")
                } else {
                    en("It downloads in the background.")
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
                    en("Its installer has been downloaded and verified."),
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
        let folder = download_folder();
        let work = cx.background_spawn(async move {
            net::block_on(fetch_installer(&address, &folder, &file, sender)).and_then(|done| done)
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
                    Ok(path) => updates.publish(
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
                    ),
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

/// Where downloaded installers are kept: `updates/` in this app's folder,
/// else the system's temporary folder.
fn download_folder() -> PathBuf {
    crate::state::location()
        .map(|folder| folder.join("updates"))
        .unwrap_or_else(|| std::env::temp_dir().join("picto-gpui-updates"))
}

/// `latestAppRelease`, then on Windows and Linux the release's feed.
async fn find(platform: &'static str) -> Result<Found, String> {
    let client = net::client()?;
    let Some(list) = net::address(feed::RELEASES_API) else {
        return Err(
            en("Update checks are not available while the app runs under a check.").to_owned(),
        );
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
        .url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .map(str::to_owned)
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']) && name != "..")
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
