//! This module holds the open library and decides what the main window
//! shows. The main window shows either the gate or the shell of the open
//! library. The gate is what the window shows while no library is open and
//! while one is opening. The module opens a library, switches to another
//! library and closes the open one. It asks for the user's consent before
//! it opens a library the user keeps. While another Picto is running and a
//! library the user keeps is open, it shows a notice that blocks the
//! window.
//!
//! A switch does these things in this order. It closes the detail windows,
//! the Settings window and the log window, because their commands would
//! otherwise reach the library that is opened next. It shows the gate's
//! loading state. It closes the library (`close_library`) on a thread of
//! its own. It opens the next library in a new shell.
use crate::paths::Canonical as _;
use super::admission::{self, Decision, Origin, Probes, Refusal};
use super::{Kind, history};
use crate::application::Application;
use crate::i18n::{en, t};
use crate::shell::Shell;
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::*;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

mod disk;
mod view;
pub use disk::{Denied, agree, consent_message, delete, move_library, permit, rename};

/// The library open in the main window.
struct Open {
    application: Arc<Application>,
    /// The instance that this process holds for the library
    /// (`opening::instance`), or `None` when the off-screen tool opened the
    /// library.
    instance: Option<Arc<crate::opening::instance::Instance>>,
}

/// The host's state: the main window, the open library, and what is needed
/// to open another library. The app has one host ([`host`]).
pub struct Host {
    main: Option<WindowHandle<Main>>,
    open: Option<Open>,
    /// Whether a library is being opened or closed now. While it is, a
    /// request to open a library is ignored.
    busy: bool,
    probes: Rc<dyn Probes>,
    /// The channel that receives the files a later launch of the app was
    /// started with (`opening::Incoming`).
    incoming: Option<futures::channel::mpsc::UnboundedSender<crate::opening::Incoming>>,
    working_directory: PathBuf,
    /// The title and the message of the last refusal or failure to open a
    /// library. The Library Manager shows them.
    pub failure: Option<(String, String)>,
    /// The root of a library that this app has just created and is opening.
    /// When it has opened, the user's consent for it is saved without
    /// asking.
    created: Option<PathBuf>,
}

struct HostGlobal(Entity<Host>);

impl Global for HostGlobal {}

/// Returns the host. The first call makes it.
pub fn host(cx: &mut App) -> Entity<Host> {
    if let Some(global) = cx.try_global::<HostGlobal>() {
        return global.0.clone();
    }
    let entity = cx.new(|_| Host {
        main: None,
        open: None,
        busy: false,
        probes: probes(),
        incoming: None,
        working_directory: std::env::current_dir().unwrap_or_default(),
        failure: None,
        created: None,
    });
    cx.set_global(HostGlobal(entity.clone()));
    entity
}

/// Returns the object through which the host asks whether another Picto is
/// running. A debug or release build asks the system. A test build takes the
/// answer from the environment variable `PICTO_GPUI_TEST_PICTO_RUNNING`, so
/// that the off-screen tool never depends on what runs on the machine. When
/// the variable is not set, no other Picto is running. With the value
/// `after-open`, no other Picto is running the first time the host asks,
/// which is when it opens the library, and one is running every time after
/// that, as when a Picto is started while a library is open. With any other
/// value, another Picto is running.
fn probes() -> Rc<dyn Probes> {
    #[cfg(test)]
    {
        Rc::new(TestProbe {
            setting: std::env::var("PICTO_GPUI_TEST_PICTO_RUNNING").ok(),
            looks: std::cell::Cell::new(0),
        })
    }
    #[cfg(not(test))]
    {
        Rc::new(admission::System)
    }
}

#[cfg(test)]
struct TestProbe {
    setting: Option<String>,
    looks: std::cell::Cell<usize>,
}

#[cfg(test)]
impl Probes for TestProbe {
    fn picto_process(&self) -> Result<Option<String>, String> {
        let looks = self.looks.get();
        self.looks.set(looks + 1);
        let running = match self.setting.as_deref() {
            None => false,
            Some("after-open") => looks > 0,
            Some(_) => true,
        };
        Ok(running.then(|| "Picto (PICTO_GPUI_TEST_PICTO_RUNNING)".to_owned()))
    }
}

/// What a launch opens in the main window.
pub enum Launch {
    /// A disposable copy and the instance that this process holds for it.
    /// The app passes the instance, and the off-screen tool passes `None`.
    Copy(PathBuf, Option<Arc<crate::opening::instance::Instance>>),
    /// A library named on the command line with `--allow-real-libraries`.
    Named(PathBuf),
    /// No library was named. The app opens the library it had open last if
    /// that library may be opened, and otherwise shows the gate.
    Last,
}

/// Starts the host in the main window when the app starts. The app and the
/// off-screen tool both call it. It gives the main window to the application
/// menu and to the host, sets the menus, opens what `launch` names, and
/// records that the launch has asked for its library ([`launched`]).
pub fn boot(
    main: WindowHandle<Main>,
    incoming: futures::channel::mpsc::UnboundedSender<crate::opening::Incoming>,
    launch: Launch,
    cx: &mut App,
) {
    crate::app_menu::set_main_window(main.into(), cx);
    attach(main, incoming, cx);
    crate::set_app_menus(cx);
    match launch {
        Launch::Copy(root, instance) => start(root, Kind::Copy, instance, main, cx),
        Launch::Named(path) => request(&path, Origin::CommandLine { flag: true }, cx),
        // `start_last` reads nothing about the library unless this process
        // may open it.
        Launch::Last => start_last(cx),
    }
    launched(cx);
}

/// Gives the host the main window and the channel that receives the files of
/// later launches.
fn attach(
    main: WindowHandle<Main>,
    incoming: futures::channel::mpsc::UnboundedSender<crate::opening::Incoming>,
    cx: &mut App,
) {
    host(cx).update(cx, |host, _| {
        host.main = Some(main);
        host.incoming = Some(incoming);
    });
    // The first time this app runs, `seed::once` fills this app's list of
    // libraries from the list of Picto 0.6.19 or earlier, if such a list
    // is found. No library is opened.
    super::seed::once(cx);
}

/// Returns the root and the kind of the open library, or `None` when no
/// library is open.
pub fn current(cx: &App) -> Option<(PathBuf, Kind)> {
    let host = cx.try_global::<HostGlobal>()?.0.read(cx);
    let open = host.open.as_ref()?;
    Some((
        open.application.root().to_path_buf(),
        open.application.kind(),
    ))
}

impl Host {
    /// Returns whether a library is being opened or closed now.
    pub fn busy(&self) -> bool {
        self.busy
    }
}

/// Returns the application of the open library, or `None` when no library is
/// open. The Library Manager reads the library's statistics through it, and
/// the off-screen tool's `invoke` sends commands to it.
pub fn application(cx: &App) -> Option<Arc<Application>> {
    let host = cx.try_global::<HostGlobal>()?.0.read(cx);
    host.open.as_ref().map(|open| open.application.clone())
}

/// Returns the shell of the open library, or `None` when no library is open.
pub fn shell(cx: &mut App) -> Option<Entity<Shell>> {
    let main = host(cx).read(cx).main?;
    main.read(cx).ok()?.shell.clone()
}

/// Does Open Library…. The user chooses a folder in the system's dialog. A
/// folder whose name ends in `.library` is opened if the rules of
/// `admission` allow it. For any other folder the "Invalid Library" error is
/// shown.
pub fn open_dialog(cx: &mut App) {
    let chosen = crate::prompt::file_or_folder(en("Select a .library folder"), cx);
    cx.spawn(async move |cx| {
        let Ok(Some(path)) = chosen.await else {
            return;
        };
        cx.update(|cx| {
            let named = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".library"));
            if named {
                request(&path, Origin::OpenDialog, cx);
            } else {
                fail(
                    en("Invalid Library").to_owned(),
                    format!(
                        "{}\n{}",
                        en("The selected folder is not a library."),
                        en("Please select a folder ending in .library")
                    ),
                    cx,
                );
            }
        });
    })
    .detach();
}

/// Creates a library at `path`, which the user chose. This function makes
/// the folder, and the application core creates the library in it when it
/// opens it. The user is not asked for consent, because the user has just
/// made this library in this app.
pub fn create(path: PathBuf, cx: &mut App) {
    let probes = host(cx).read(cx).probes.clone();
    if let Err(refusal) = admission::admit_new(&path, &super::facts(), &*probes) {
        return refuse(&refusal, cx);
    }
    if let Err(error) = std::fs::create_dir_all(path.join("blobs")) {
        return fail(
            en("Could not open Picto Library").to_owned(),
            format!("{}: {error}", path.display()),
            cx,
        );
    }
    let root = path.canonical().unwrap_or(path);
    host(cx).update(cx, |host, _| host.created = Some(root.clone()));
    begin(root, Kind::Real, cx);
}

/// Records that the launch has asked for its library, or has found that
/// there is none to ask for. From then on, a main window that shows the gate
/// with no library open may open the Library Manager by itself
/// (`Main::launched`).
pub fn launched(cx: &mut App) {
    let main = host(cx).read(cx).main;
    if let Some(main) = main {
        let _ = main.update(cx, |main, _, cx| {
            main.launched = true;
            cx.notify();
        });
    }
}

/// Called when the main window becomes active. Notes this instance as the
/// one most recently active, and looks again for another running Picto while
/// a library the user keeps is open.
pub fn activated(cx: &mut App) {
    let host = host(cx);
    if let Some(instance) = host
        .read(cx)
        .open
        .as_ref()
        .and_then(|open| open.instance.clone())
    {
        instance.mark_active();
    }
    check_picto(cx);
}

/// Shows the notice that blocks the main window while another Picto is
/// running and a library the user keeps is open. Removes the notice when no
/// other Picto is running any more, and when the open library is not one the
/// user keeps.
pub fn check_picto(cx: &mut App) {
    let host = host(cx);
    let (main, real, probes) = {
        let host = host.read(cx);
        let real = host
            .open
            .as_ref()
            .is_some_and(|open| open.application.kind() == Kind::Real);
        (host.main, real, host.probes.clone())
    };
    let Some(main) = main else {
        return;
    };
    let blocked = if real {
        match probes.picto_process() {
            Ok(None) => None,
            Ok(Some(process)) => Some(process),
            // When the list of processes cannot be read, the notice is shown
            // with the error, as if another Picto were running.
            Err(error) => Some(error),
        }
    } else {
        None
    };
    let _ = main.update(cx, |main, _, cx| {
        let leaving = main
            .blocked
            .get_mut()
            .is_some_and(|notice| notice.presence.closing());
        let shown = main.blocked.content().filter(|_| !leaving);
        if shown != blocked.as_ref() {
            // The notice is opened only when it is not shown yet or names
            // another process, so that its opening animation does not start
            // again at every check. It is removed with the closing animation
            // of a dialog.
            match blocked {
                Some(process) => {
                    main.blocked.open(process, ui::menu::Return::Opener, cx);
                }
                None => main.blocked.dismiss(cx),
            }
        }
    });
}

/// Opens the library at `path` if the rules of `admission` allow it. Where
/// the rules need the user's consent, it shows the consent question first.
/// It does nothing while another library is being opened or closed.
pub fn request(path: &Path, origin: Origin, cx: &mut App) {
    let host = host(cx);
    if host.read(cx).busy {
        return;
    }
    // When `path` is the library that is already open, the main window is
    // brought forward and nothing is opened.
    if let Ok(canonical) = path.canonical()
        && current(cx).is_some_and(|(root, _)| root == canonical)
    {
        crate::app_menu::bring_forward(cx);
        return;
    }
    let probes = host.read(cx).probes.clone();
    let decision = admission::admit(
        path,
        origin,
        &super::facts(),
        &super::consents(cx),
        &*probes,
    );
    match decision {
        Decision::Copy(root) => begin(root, Kind::Copy, cx),
        Decision::Real { root, .. } => begin(root, Kind::Real, cx),
        Decision::Ask { root, identity } => {
            let main = host.read(cx).main;
            if let Some(main) = main {
                let _ = main.update(cx, |main, window, cx| {
                    main.asking.open(
                        Asking {
                            root,
                            identity,
                            origin,
                        },
                        ui::menu::Return::Opener,
                        cx,
                    );
                    crate::ui::window_chrome::activate(window);
                });
            }
        }
        Decision::Refused(refusal) => refuse(&refusal, cx),
    }
}

/// Called at start when no library was named. Opens the library this app had
/// open last, if this process may open it at all (`listed`). When that
/// library is no longer at its path, it shows the "Library Not Found"
/// question, with the buttons Locate…, Remove from List and Cancel. It does
/// not open another library of the history in its place.
pub fn start_last(cx: &mut App) {
    let last = crate::state::get(cx).last_library.clone();
    let Some(last) = last.filter(|last| super::listed(last, &super::facts())) else {
        return;
    };
    let path = PathBuf::from(&last);
    if path.join("library.sqlite").is_file() {
        request(&path, Origin::LastOpened, cx);
        return;
    }
    let main = host(cx).read(cx).main;
    if let Some(main) = main {
        let _ = main.update(cx, |main, _, cx| {
            main.missing.open(path, ui::menu::Return::Opener, cx);
        });
    }
}

/// Does Locate… for a library that is missing. The user chooses a folder in
/// the system's dialog. The library is looked for in that folder itself and
/// inside it under the missing library's name (`located`). When it is found,
/// the list of libraries is changed to name the new path. With `open` set,
/// the library is then opened under the same rules as any other library the
/// user chose. The consent that the user gave for the old path does not
/// apply to the new path, so the user is asked again.
pub fn locate(missing: PathBuf, open: bool, cx: &mut App) {
    let basename = missing.file_name().unwrap_or_default().to_owned();
    // The system's folder dialog shows this text as the prompt of its
    // button.
    let chosen = crate::prompt::folder(
        &format!(
            "Select the folder containing {}",
            basename.to_string_lossy()
        ),
        cx,
    );
    cx.spawn(async move |cx| {
        let Ok(Some(folder)) = chosen.await else {
            return;
        };
        cx.update(|cx| {
            let Some(found) = super::located(&folder, &basename) else {
                return fail(
                    en("Library Not Found").to_owned(),
                    // The locale files have no key for this message, so it
                    // is shown in English in every language.
                    format!(
                        "\"{}\" was not found in the selected folder.",
                        basename.to_string_lossy()
                    ),
                    cx,
                );
            };
            let found = found.canonical().unwrap_or(found);
            if found == missing {
                return;
            }
            let (from, to) = (
                missing.to_string_lossy().into_owned(),
                found.to_string_lossy().into_owned(),
            );
            crate::state::update(cx, |state| {
                history::rename(
                    &mut state.library_history,
                    &mut state.pinned_libraries,
                    &mut state.last_library,
                    &from,
                    &to,
                );
                history::rename_look(&mut state.library_meta, &from, &to);
            });
            crate::set_app_menus(cx);
            host(cx).update(cx, |_, cx| cx.notify());
            if open {
                request(&found, Origin::OpenDialog, cx);
            }
        });
    })
    .detach();
}

/// Tells the user why a library was not opened. It shows an error
/// notification in the main window, and it keeps the title and the message
/// for the Library Manager's error box.
fn refuse(refusal: &Refusal, cx: &mut App) {
    let (title, message) = refusal.words();
    fail(title, message, cx);
}

fn fail(title: String, message: String, cx: &mut App) {
    eprintln!("picto: {title}: {message}");
    host(cx).update(cx, |host, cx| {
        host.failure = Some((title.clone(), message.clone()));
        cx.notify();
    });
    ui::notify::error(title, message, cx);
}

/// Called when the user has agreed to the consent question. Saves the
/// consent and opens the library.
fn consented(asking: Asking, cx: &mut App) {
    let Asking {
        root,
        identity,
        origin,
    } = asking;
    agree(&identity, cx);
    request(&root, origin, cx);
}

/// Closes the open library and opens `root` in its place.
fn begin(root: PathBuf, kind: Kind, cx: &mut App) {
    let host = host(cx);
    let Some(main) = host.read(cx).main else {
        return;
    };
    // Only one instance of this app has a library open at a time. When
    // another instance already has `root` open, that instance's window is
    // brought forward and this one opens nothing.
    let request = crate::opening::instance::Request {
        cwd: host.read(cx).working_directory.clone(),
        args: Vec::new(),
    };
    let incoming = host.read(cx).incoming.clone();
    let instance = match crate::opening::instance::start(&root, &request, move |request| {
        if let Some(incoming) = &incoming {
            let _ = incoming.unbounded_send(crate::opening::Incoming {
                entries: crate::opening::from_arguments(&request.args, &request.cwd),
                forward: true,
            });
        }
    }) {
        Ok(crate::opening::instance::Started::Primary(instance)) => Arc::new(instance),
        Ok(crate::opening::instance::Started::HandedOver) => {
            fail(
                en("Could not open Picto Library").to_owned(),
                en("This library is open in another window of this app, which was brought forward.")
                    .to_owned(),
                cx,
            );
            return;
        }
        Err(error) => {
            fail(en("Could not open Picto Library").to_owned(), error, cx);
            return;
        }
    };
    start(root, kind, Some(instance), main, cx);
}

/// Opens `root` in the main window, after it has closed the library that was
/// open. [`begin`] calls it. [`boot`] also calls it for the copy named on
/// the command line, for which the process already holds the instance.
pub fn start(
    root: PathBuf,
    kind: Kind,
    instance: Option<Arc<crate::opening::instance::Instance>>,
    main: WindowHandle<Main>,
    cx: &mut App,
) {
    let host = host(cx);
    host.update(cx, |host, _| {
        host.busy = true;
        host.failure = None;
    });
    let previous = close_views(
        main,
        Gate::Loading(super::name(&root.to_string_lossy())),
        cx,
    );
    cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            close_application(previous, cx).await;
        }
        let (events, delivered) = std::sync::mpsc::channel();
        let application = Application::new(root.clone(), kind, events);
        let mut opened = application.opened();
        let shown = cx.update(|cx| {
            let made = main.update(cx, |main, window, cx| {
                let shell = cx
                    .new(|cx| Shell::new(root.clone(), application.clone(), delivered, window, cx));
                main.shell = Some(shell);
                window.set_window_title(&title(&root, kind));
                cx.notify();
            });
            host.update(cx, |host, _| {
                host.open = Some(Open {
                    application: application.clone(),
                    instance,
                });
            });
            made.is_ok()
        });
        if !shown {
            return;
        }
        let result = match opened.wait_for(Option::is_some).await {
            Ok(result) => result.clone().unwrap_or(Ok(())),
            Err(_) => Err(crate::application::STOPPED.to_owned()),
        };
        cx.update(|cx| {
            host.update(cx, |host, _| host.busy = false);
            match result {
                Ok(()) => {
                    if kind == Kind::Real {
                        let path = root.to_string_lossy().into_owned();
                        let created = host.update(cx, |host, _| {
                            host.created.take().is_some_and(|created| created == root)
                        });
                        let database = created
                            .then(|| {
                                super::platform::file_identity(&root.join("library.sqlite")).ok()
                            })
                            .flatten();
                        crate::state::update(cx, |state| {
                            history::add(
                                &mut state.library_history,
                                &mut state.last_library,
                                &path,
                            );
                            if let Some(database) = database {
                                state.library_consents.push((path.clone(), database));
                            }
                        });
                    }
                    // A release build gives the folder of a library the user
                    // keeps its icon in the file manager, which only Windows
                    // needs. It runs off the main thread, because it starts
                    // the system's `attrib` program three times.
                    if kind == Kind::Real && super::facts().release {
                        let folder = root.clone();
                        cx.background_spawn(async move {
                            if let Err(error) = super::platform::mark_library_folder(&folder) {
                                log::warn!(
                                    "the icon of the library's folder {} was not set: {error}",
                                    folder.display()
                                );
                            }
                        })
                        .detach();
                    }
                    host.update(cx, |_, cx| cx.notify());
                    crate::set_app_menus(cx);
                    check_picto(cx);
                }
                Err(error) => {
                    // The library did not open. The main window shows the
                    // gate again, and the Library Manager opens and shows
                    // the reason.
                    close(cx);
                    fail(en("Could not open Picto Library").to_owned(), error, cx);
                    super::manager::open(cx);
                }
            }
        });
    })
    .detach();
}

/// Closes the open library and shows the gate in the main window. It is
/// called for Close Library on the blocking notice and after a library has
/// failed to open.
pub fn close(cx: &mut App) {
    let host = host(cx);
    let Some(main) = host.read(cx).main else {
        return;
    };
    if let Some(previous) = close_views(main, Gate::Closed, cx) {
        host.update(cx, |host, _| host.busy = true);
        cx.spawn(async move |cx| {
            close_application(previous, cx).await;
            cx.update(|cx| {
                host.update(cx, |host, _| host.busy = false);
                crate::set_app_menus(cx);
            });
        })
        .detach();
    }
}

/// Stops the open library's workers and checkpoints the library before the
/// process ends (`Application::close`). The shell's session is disconnected
/// from the library first.
pub fn close_for_quit(cx: &mut App) {
    let host = host(cx);
    let open = host.update(cx, |host, _| host.open.take());
    if let Some(open) = open {
        if let Some(session) = shell_session(cx) {
            session.update(cx, |session, _| session.shut());
        }
        open.application.close();
    }
}

/// Returns the session of the open library's shell, or `None` when no
/// library is open.
fn shell_session(cx: &mut App) -> Option<Entity<crate::session::Session>> {
    let main = host(cx).read(cx).main?;
    main.read(cx)
        .ok()?
        .shell
        .as_ref()
        .map(|shell| shell.read(cx).session().clone())
}

/// Removes everything that shows the open library. It closes every window
/// except the main window and the Library Manager (the detail windows,
/// Settings, the log window and the menus), stops the session's reads, and
/// shows `gate` in the main window. Returns the open library, which the
/// caller then closes, or `None` when no library was open.
fn close_views(main: WindowHandle<Main>, gate: Gate, cx: &mut App) -> Option<Open> {
    // A login that is in progress belongs to this library, so it is ended
    // here, whether it runs in the sign-in window or in the browser.
    crate::logins::Logins::library_closing(cx);
    let manager = super::manager::window(cx);
    let main_any: AnyWindowHandle = main.into();
    for window in cx.windows() {
        if window == main_any || Some(window) == manager {
            continue;
        }
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
    if let Some(session) = shell_session(cx) {
        session.update(cx, |session, _| session.shut());
    }
    let _ = main.update(cx, |main, window, cx| {
        main.shell = None;
        main.gate = gate;
        main.blocked.close(cx);
        window.set_window_title(crate::desktop::APP_NAME);
        // The shell that had the keyboard focus is gone, so the focus is
        // given to the main window's own focus handle.
        window.focus(&main.focus, cx);
        cx.notify();
    });
    let open = host(cx).update(cx, |host, _| host.open.take());
    crate::set_app_menus(cx);
    open
}

/// Closes the library (`close_library`) on a thread of its own, so that the
/// thread that draws does not wait for it. The application is dropped on
/// that thread too, because its runtime cannot be dropped inside an
/// asynchronous task.
async fn close_application(open: Open, cx: &mut AsyncApp) {
    let (done, closed) = futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("picto-close".into())
        .spawn(move || {
            open.application.close();
            drop(open);
            let _ = done.send(());
        })
        .ok();
    let _ = closed.await;
    let _ = cx.update(|_| ());
}

/// Returns the main window's title, which the Window menu and the system's
/// lists of windows show. The title of a library the user keeps holds the
/// library's name. The title of a copy holds the words "Library copy".
pub fn title(root: &Path, kind: Kind) -> String {
    let app = crate::desktop::APP_NAME;
    match kind {
        // i18n: the window's title for a copy, which is not translated
        Kind::Copy => format!("{app} · Library copy"),
        Kind::Real => format!("{app} · {}", super::name(&root.to_string_lossy())),
    }
}

/// What the gate shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    /// No library is open.
    Closed,
    /// A library is opening. The text is the library's name.
    Loading(String),
}

/// A library that the consent question asks the user about.
#[derive(Clone)]
struct Asking {
    root: PathBuf,
    identity: super::Identity,
    origin: Origin,
}

/// The root view of the main window. It shows the gate or the shell of the
/// open library.
pub struct Main {
    shell: Option<Entity<Shell>>,
    gate: Gate,
    asking: ui::modal::Host<Asking>,
    /// The "Library Not Found" question. Its content is the path of the
    /// library this app had open last, when that library is missing at
    /// start.
    missing: ui::modal::Host<PathBuf>,
    /// The notice that blocks the window. Its content names the process of
    /// the other Picto that is running while a library the user keeps is
    /// open.
    blocked: ui::modal::Host<String>,
    focus: FocusHandle,
    notifications: Entity<ui::notify::Host>,
    /// Whether the gate has opened the Library Manager by itself yet.
    offered: bool,
    /// Whether the launch has asked for its library yet. The window draws
    /// once before that with no library open, and the gate must not open the
    /// Library Manager for that frame.
    launched: bool,
}
