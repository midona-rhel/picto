//! The open library and the main window's content, as production's
//! `libraryHostService.mjs` and `LibraryGate.tsx` share them: the gate
//! (no library, or one opening) or the library's shell; opening, switching
//! (`switchLibrary`) and closing; the user's consent; and the notice that
//! blocks the window while the Electron app runs beside a library the user
//! keeps.
//!
//! A switch closes the detail, Settings and log windows (production leaves
//! them open, where their commands would reach the library opened next),
//! shows the gate's loading state, closes the library (`close_library`, on
//! a thread of its own), and opens the next in a new shell, as production
//! remounts its whole window under the new library's key.
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

/// The library open in the main window.
struct Open {
    application: Arc<Application>,
    /// This app's one instance for the library (`opening::instance`).
    instance: Option<Arc<crate::opening::instance::Instance>>,
}

/// What the host holds for the whole app.
pub struct Host {
    main: Option<WindowHandle<Main>>,
    open: Option<Open>,
    /// A library being opened or closed: nothing else starts meanwhile.
    busy: bool,
    probes: Rc<dyn Probes>,
    /// Where later launches' files go (`opening::Incoming`).
    incoming: Option<futures::channel::mpsc::UnboundedSender<crate::opening::Incoming>>,
    working_directory: PathBuf,
    /// The last refusal or failure, which the Library Manager shows.
    pub failure: Option<(String, String)>,
    /// A library this app just created, whose consent is kept once it
    /// opened.
    created: Option<PathBuf>,
}

struct HostGlobal(Entity<Host>);

impl Global for HostGlobal {}

/// The host, made the first time it is needed.
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

/// The system's probes; in a test build the Electron app is what
/// `PICTO_GPUI_TEST_PICTO_RUNNING` says (unset: not running), so the
/// off-screen tool never depends on what runs on the machine.
fn probes() -> Rc<dyn Probes> {
    #[cfg(test)]
    {
        Rc::new(admission::tests::Fake {
            running: Ok(std::env::var_os("PICTO_GPUI_TEST_PICTO_RUNNING")
                .map(|_| "Picto (PICTO_GPUI_TEST_PICTO_RUNNING)".to_owned())),
        })
    }
    #[cfg(not(test))]
    {
        Rc::new(admission::System)
    }
}

/// Hands the host the main window and where later launches' files go.
pub fn attach(
    main: WindowHandle<Main>,
    incoming: futures::channel::mpsc::UnboundedSender<crate::opening::Incoming>,
    cx: &mut App,
) {
    host(cx).update(cx, |host, _| {
        host.main = Some(main);
        host.incoming = Some(incoming);
    });
    // The first run lists the Electron app's libraries; nothing opens.
    super::seed::once(cx);
}

/// The open library's root and kind.
pub fn current(cx: &App) -> Option<(PathBuf, Kind)> {
    let host = cx.try_global::<HostGlobal>()?.0.read(cx);
    let open = host.open.as_ref()?;
    Some((
        open.application.root().to_path_buf(),
        open.application.kind(),
    ))
}

impl Host {
    /// A library is being opened or closed.
    pub fn busy(&self) -> bool {
        self.busy
    }
}

/// The open library's application: its statistics, and the off-screen
/// tool's `invoke`.
pub fn application(cx: &App) -> Option<Arc<Application>> {
    let host = cx.try_global::<HostGlobal>()?.0.read(cx);
    host.open.as_ref().map(|open| open.application.clone())
}

/// The open library's shell.
pub fn shell(cx: &mut App) -> Option<Entity<Shell>> {
    let main = host(cx).read(cx).main?;
    main.read(cx).ok()?.shell.clone()
}

/// Open Library… (`openLibraryDialog`): a `.library` chosen in the system's
/// dialog, opened if the rules let it.
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

/// A new library (`createLibrary`): its folder made where the user chose,
/// and the core creating the library as it opens it. Made by the user here,
/// it needs no further consent.
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
    let root = path.canonicalize().unwrap_or(path);
    host(cx).update(cx, |host, _| host.created = Some(root.clone()));
    begin(root, Kind::Real, cx);
}

/// Rename (`renameLibrary`): the folder renamed beside itself, keeping its
/// place in the list, its colour and icon and the user's consent; the open
/// library is closed first and opened again after.
pub fn rename(from: PathBuf, to: PathBuf, cx: &mut App) {
    relocate(from, to, Relocation::Rename, cx);
}

/// Move… (`relocateLibrary`): the folder moved into another (`files`), as
/// a rename is.
pub fn move_library(from: PathBuf, to: PathBuf, cx: &mut App) {
    relocate(from, to, Relocation::Move, cx);
}

#[derive(Clone, Copy, PartialEq)]
enum Relocation {
    Rename,
    Move,
}

fn relocate(from: PathBuf, to: PathBuf, how: Relocation, cx: &mut App) {
    if host(cx).read(cx).busy {
        return;
    }
    let probes = host(cx).read(cx).probes.clone();
    // Production renames only what it has closed; this app cannot see
    // what the Electron app has open.
    match probes.picto_process() {
        Ok(None) => {}
        Ok(Some(process)) => return refuse(&Refusal::PictoRunning(process), cx),
        Err(error) => return refuse(&Refusal::ProbeFailed(error), cx),
    }
    if let Err(refusal) = super::facts().permits_library(&from) {
        return refuse(&refusal, cx);
    }
    let is_open = current(cx).is_some_and(|(root, _)| root == from);
    let main = host(cx).read(cx).main;
    let previous = match (is_open, main) {
        (true, Some(main)) => {
            close_views(main, Gate::Loading(super::name(&to.to_string_lossy())), cx)
        }
        _ => None,
    };
    host(cx).update(cx, |host, _| host.busy = true);
    cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            close_application(previous, cx).await;
        }
        let (source, target) = (from.clone(), to.clone());
        let done = cx
            .background_executor()
            .spawn(async move {
                match how {
                    Relocation::Rename => std::fs::rename(&source, &target)
                        .map_err(|error| format!("Failed to rename library: {error}")),
                    Relocation::Move => super::files::move_to(&source, &target),
                }
            })
            .await;
        cx.update(|cx| {
            host(cx).update(cx, |host, cx| {
                host.busy = false;
                cx.notify();
            });
            match done {
                Ok(()) => {
                    let (from_text, to_text) = (
                        from.to_string_lossy().into_owned(),
                        to.to_string_lossy().into_owned(),
                    );
                    crate::state::update(cx, |state| {
                        history::rename(
                            &mut state.library_history,
                            &mut state.pinned_libraries,
                            &mut state.last_library,
                            &from_text,
                            &to_text,
                        );
                        history::rename_look(&mut state.library_meta, &from_text, &to_text);
                        // The consent follows the path; it holds only while
                        // the database file is the same one (a move across
                        // volumes makes a new one, which is asked about).
                        for consent in &mut state.library_consents {
                            if consent.0 == from_text {
                                consent.0 = to_text.clone();
                            }
                        }
                    });
                    crate::set_app_menus(cx);
                    host(cx).update(cx, |_, cx| cx.notify());
                    if is_open {
                        request(&to, Origin::Manager, cx);
                    }
                }
                Err(error) => {
                    fail(en("Could not open Picto Library").to_owned(), error, cx);
                    if is_open {
                        request(&from, Origin::Manager, cx);
                    }
                }
            }
        });
    })
    .detach();
}

/// Delete… (`deleteLibrary`), once the user confirmed: the library's
/// folder removed for good (`files::delete`), never the open one, and
/// taken out of the list. Hands the outcome to `done`.
pub fn delete(
    path: PathBuf,
    cx: &mut App,
    done: impl FnOnce(Result<(), String>, &mut App) + 'static,
) {
    if host(cx).read(cx).busy {
        return;
    }
    let open = current(cx).map(|(root, _)| root);
    host(cx).update(cx, |host, _| host.busy = true);
    let target = path.clone();
    let work = cx
        .background_executor()
        .spawn(async move { super::files::delete(&target, open.as_deref()) });
    cx.spawn(async move |cx| {
        let result = work.await;
        cx.update(|cx| {
            host(cx).update(cx, |host, cx| {
                host.busy = false;
                cx.notify();
            });
            if result.is_ok() {
                let text = path.to_string_lossy().into_owned();
                crate::state::update(cx, |state| {
                    history::remove(
                        &mut state.library_history,
                        &mut state.pinned_libraries,
                        &mut state.last_library,
                        &text,
                    )
                });
                crate::set_app_menus(cx);
            }
            done(result, cx);
        });
    })
    .detach();
}

/// Whether the library at `path` may be changed on disk or revealed from
/// the Library Manager right now: what `admission::admit` decides for an
/// open from the manager. A library the user has not agreed to yet comes
/// back as `Ask`, so the manager asks first, as opening does.
pub fn check(path: &Path, cx: &mut App) -> Decision {
    let probes = host(cx).read(cx).probes.clone();
    admission::admit(
        path,
        Origin::Manager,
        &super::facts(),
        &super::consents(cx),
        &*probes,
    )
}

/// The consent question's message for the library at `root`.
pub fn consent_message(root: &Path) -> String {
    format!(
        "{}\n\n{}",
        root.display(),
        en(
            "This app will read and write this library directly: what you change here changes the library itself, with no copy in between. Picto must not have it open at the same time. You are asked once for each library."
        )
    )
}

/// Keeps the user's consent for `identity`.
pub fn agree(identity: &super::Identity, cx: &mut App) {
    crate::state::update(cx, |state| {
        let pair = (identity.path.clone(), identity.database.clone());
        if !state.library_consents.contains(&pair) {
            state.library_consents.push(pair);
        }
    });
}

/// Notes this instance as the one most recently active, and looks again
/// for the Electron app while a library the user keeps is open.
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

/// Shows the blocking notice while the Electron app runs beside a library
/// the user keeps, and takes it away once it has quit.
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
            // Not knowing is treated as running.
            Err(error) => Some(error),
        }
    } else {
        None
    };
    let _ = main.update(cx, |main, _, cx| {
        if main.blocked != blocked {
            main.blocked = blocked;
            cx.notify();
        }
    });
}

/// Opens `path` if the rules let it (`admission`), asking the user's
/// consent first where it is needed.
pub fn request(path: &Path, origin: Origin, cx: &mut App) {
    let host = host(cx);
    if host.read(cx).busy {
        return;
    }
    // The open library asked for again only comes forward.
    if let Ok(canonical) = path.canonicalize()
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
                    main.asking = Some(Asking {
                        root,
                        identity,
                        origin,
                        presence: ui::modal::Presence::new(),
                    });
                    crate::ui::window_chrome::activate(window);
                    cx.notify();
                });
            }
        }
        Decision::Refused(refusal) => refuse(&refusal, cx),
    }
}

/// At start, with no library named: the library this app had open last,
/// if this process may open it at all (`listed`); when it is no longer
/// there, production's "Library Not Found" question (`handleMissingLibrary`)
/// with Locate…, Remove from List and Cancel. Production first falls back
/// to the first library of its history that is still there; this app opens
/// at start only the library it had open last, and otherwise asks.
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
            main.missing = Some((path, ui::modal::Presence::new()));
            cx.notify();
        });
    }
}

/// Locate… for a library that is missing (`handleMissingLibrary`,
/// `relocateLibrary`): a folder chosen in the system's dialog, the library
/// found there or inside it under the same name, the list made to name it
/// there, and (`open`) the library opened through the same rules as any
/// other the user chose. The user's consent does not follow it: a library
/// found at a new place is asked about again.
pub fn locate(missing: PathBuf, open: bool, cx: &mut App) {
    let basename = missing.file_name().unwrap_or_default().to_owned();
    // Production's dialog says this as its message; the system's dialog
    // here shows it as its button's prompt.
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
                    // Production's main process writes it in English,
                    // without a key.
                    format!(
                        "\"{}\" was not found in the selected folder.",
                        basename.to_string_lossy()
                    ),
                    cx,
                );
            };
            let found = found.canonicalize().unwrap_or(found);
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

/// Tells the user why a library was not opened: a notification in the main
/// window, and the Library Manager's error box.
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

/// The user agreed: kept, and the library opened.
fn consented(asking: Asking, cx: &mut App) {
    let Asking {
        root,
        identity,
        origin,
        ..
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
    // One instance of this app per library: another that has it open
    // comes forward, and this one opens nothing.
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

/// Opens `root` in the main window, closing what was open first. Used by
/// [`begin`], and by `main` for the copy on the command line, whose
/// instance it already holds.
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
            Err(_) => Err("The application has stopped".to_owned()),
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
                    host.update(cx, |_, cx| cx.notify());
                    crate::set_app_menus(cx);
                    check_picto(cx);
                }
                Err(error) => {
                    // `library-open-failed`: back to the gate, the Library
                    // Manager showing why.
                    close(cx);
                    fail(en("Could not open Picto Library").to_owned(), error, cx);
                    super::manager::open(cx);
                }
            }
        });
    })
    .detach();
}

/// Closes the open library, leaving the gate in the main window: Close
/// Library on the blocking notice, a failed open, and quitting.
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

/// Stops the open library's workers and checkpoints it before the process
/// ends, as the Electron app closes its library before quitting.
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

/// The open shell's session.
fn shell_session(cx: &mut App) -> Option<Entity<crate::session::Session>> {
    let main = host(cx).read(cx).main?;
    main.read(cx)
        .ok()?
        .shell
        .as_ref()
        .map(|shell| shell.read(cx).session().clone())
}

/// Takes the open library's views away: the windows that show its items
/// (detail windows, Settings, the log window, menus) close, its session
/// stops reading, the main window shows `gate`. Hands back what is to be
/// closed.
fn close_views(main: WindowHandle<Main>, gate: Gate, cx: &mut App) -> Option<Open> {
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
        main.blocked = None;
        window.set_window_title(en("Picto GPUI"));
        // The keyboard goes back to the window itself.
        window.focus(&main.focus, cx);
        cx.notify();
    });
    let open = host(cx).update(cx, |host, _| host.open.take());
    crate::set_app_menus(cx);
    open
}

/// `close_library` off the thread that draws, and the application dropped
/// there too (its runtime cannot be dropped inside an asynchronous task).
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

/// The main window's title: what the window menu and the system's window
/// lists show. A copy says so.
pub fn title(root: &Path, kind: Kind) -> String {
    match kind {
        // i18n: the window's title, as before for a copy
        Kind::Copy => "Picto GPUI · Library copy".to_owned(),
        Kind::Real => format!(
            "{} · {}",
            en("Picto GPUI"),
            super::name(&root.to_string_lossy())
        ),
    }
}

/// What the gate shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    /// No library is open (`closed`).
    Closed,
    /// One is opening (`loading`), named.
    Loading(String),
}

/// A library the user is asked about.
struct Asking {
    root: PathBuf,
    identity: super::Identity,
    origin: Origin,
    presence: ui::modal::Presence,
}

/// The main window's root: the gate, or the open library's shell.
pub struct Main {
    shell: Option<Entity<Shell>>,
    gate: Gate,
    asking: Option<Asking>,
    /// The library this app had open last, missing at start: the "Library
    /// Not Found" question.
    missing: Option<(PathBuf, ui::modal::Presence)>,
    /// The Electron app's process while it runs beside a library the user
    /// keeps.
    blocked: Option<String>,
    focus: FocusHandle,
    notifications: Entity<ui::notify::Host>,
    /// Whether the gate has opened the Library Manager by itself yet.
    offered: bool,
}

impl Main {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            shell: None,
            gate: Gate::Closed,
            asking: None,
            missing: None,
            blocked: None,
            focus: cx.focus_handle(),
            notifications: cx.new(ui::notify::Host::new),
            offered: false,
        }
    }

    fn gate(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let titlebar = ui::drag_region("library-gate-titlebar", window, cx)
            .h(px(theme::TITLEBAR_HEIGHT))
            .flex_none()
            .pl(px(74.))
            .pr(px(8.))
            .flex()
            .items_center()
            .justify_end()
            .border_b_1()
            .border_color(theme.border_secondary())
            .children(ui::window_chrome::caption_buttons("library-gate", window));
        let content = match &self.gate {
            Gate::Closed => {
                // `LibraryGate` opens the Library Manager once by itself,
                // after the question about a missing library (production
                // asks it before the window opens).
                if !self.offered && self.missing.is_none() {
                    self.offered = true;
                    cx.defer(|cx| super::manager::open(cx));
                }
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .text_color(theme.text_tertiary())
                    .child(ui::icon("books", 30., theme.text_tertiary()))
                    .child(
                        div()
                            .mt(px(4.))
                            .text(theme::TEXT_XL)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text())
                            .child(t("Open a library to start")),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_MD)
                            .text_color(theme.text_secondary())
                            .child(t(
                                "Create a new Picto library or open one already on this device.",
                            )),
                    )
                    .child(
                        div().mt(px(8.)).child(
                            ui::action_button::action_button(
                                "library-gate-choose",
                                ui::action_button::Look::Primary,
                            )
                            .label(t("Choose Library…"))
                            .on_click(|_, _, cx| super::manager::open(cx)),
                        ),
                    )
                    .into_any_element()
            }
            Gate::Loading(_) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .mt(px(4.))
                        .text(theme::TEXT_XL)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text())
                        .child(t("Opening your library")),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .w(px(320.))
                        .child(ui::progress_bar::progress_bar().height(3.)),
                )
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .child(titlebar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(content),
            )
            .into_any_element()
    }

    fn consent(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asking = self.asking.as_ref()?;
        let message = consent_message(&asking.root);
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking = None;
            cx.notify();
        });
        let accept = cx.listener(|this: &mut Self, _: &(), _, cx| {
            if let Some(asking) = this.asking.take() {
                cx.notify();
                cx.defer(move |cx| consented(asking, cx));
            }
        });
        Some(
            ui::confirm::confirm(
                "library-consent",
                en("Open this library directly?"),
                message,
                t("Open Library"),
                asking.presence,
            )
            .layer(ui::layer::OVER_DIALOG)
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    /// `handleMissingLibrary`'s message box: Locate…, Remove from List,
    /// Cancel.
    fn missing(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, presence) = self.missing.as_ref()?;
        let theme = theme::theme(cx);
        let name = super::name(&path.to_string_lossy());
        let dismiss = |this: &mut Self, cx: &mut Context<Self>| {
            this.missing = None;
            cx.notify();
        };
        let close = cx.listener(move |this: &mut Self, _: &(), _, cx| dismiss(this, cx));
        let cancel = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| dismiss(this, cx));
        let located = path.clone();
        let locate = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
            dismiss(this, cx);
            let located = located.clone();
            cx.defer(move |cx| locate(located, true, cx));
        });
        let removed = path.to_string_lossy().into_owned();
        let remove = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
            dismiss(this, cx);
            let removed = removed.clone();
            crate::state::update(cx, |state| {
                history::remove(
                    &mut state.library_history,
                    &mut state.pinned_libraries,
                    &mut state.last_library,
                    &removed,
                )
            });
            cx.defer(crate::set_app_menus);
        });
        // Production's main process writes these in English, without keys.
        let message = format!(
            "Library \"{name}\" could not be found.\n\nPath: {}",
            path.display()
        );
        Some(
            ui::modal::modal("library-missing", en("Library Not Found"), *presence)
                .size(ui::modal::Size::Small)
                .layer(ui::layer::OVER_DIALOG)
                .on_close(move |window, cx| close(&(), window, cx))
                .child(
                    div()
                        .text(theme::TEXT_MD)
                        .line_height(px(19.5))
                        .text_color(theme.text_secondary())
                        .child(message),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-missing-cancel",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Cancel"))
                    .on_click(cancel),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-missing-remove",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Remove from List"))
                    .on_click(remove),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-missing-locate",
                        ui::action_button::Look::Primary,
                    )
                    .label(t("Locate…"))
                    .on_click(locate),
                )
                .into_any_element(),
        )
    }

    fn notice(&self) -> Option<AnyElement> {
        self.blocked.as_ref()?;
        Some(
            ui::confirm::confirm(
                "library-picto-running",
                en("Quit Picto first"),
                en("Picto is running while this app has a library you keep open. Both must never have a library open at the same time: quit Picto, then choose Check Again, or close the library here."),
                t("Check Again"),
                ui::modal::Presence::new(),
            )
            .cancel_label(en("Close Library"))
            .layer(ui::layer::OVER_DIALOG)
            // It stays until Picto has quit or the library is closed.
            .on_close(|_, _| {})
            .on_cancel(|_, cx| cx.defer(close))
            .on_confirm(|_, cx| cx.defer(check_picto))
            .into_any_element(),
        )
    }
}

impl Render for Main {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.shell {
            Some(shell) => shell.clone().into_any_element(),
            None => self.gate(window, cx),
        };
        let mut root = div()
            .id("main")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .child(content)
            .children(self.missing(cx))
            .children(self.consent(cx))
            .children(self.notice());
        // The shell draws its own notifications; the gate needs these.
        if self.shell.is_none() {
            root = root
                .child(self.notifications.clone())
                .children(ui::window_chrome::rim(cx));
        }
        root
    }
}
