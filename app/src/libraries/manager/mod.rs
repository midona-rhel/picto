//! The Library Manager. It is a window of its own, 700 by 550 points and
//! not resizable. Its left side lists the libraries on this device. Its
//! right side shows the library chosen in the list: its cover, name and
//! folder, the Open Library button and the actions menu, whether it is
//! open, pinned or missing, its appearance, its statistics and its cloud
//! sync. When the user asks for it, the right side shows the form for a new
//! library or the Open from Cloud… pane instead. The footer holds New
//! Library…, Open Existing… and Open from Cloud….
//!
//! The list is this app's own (`state.json`). Only the libraries that this
//! process may open are listed (`libraries::listed`), so a process that may
//! not open a library the user keeps reads nothing about one. Listing reads
//! the folder and the cover of each listed library and nothing else. The
//! statistics are read for the open library only, through its application.
use super::admission::Origin;
use super::{Kind, history, host};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::menu::{self, Menu};
use crate::ui::text_input::{Kind as InputKind, TextInput};
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::path::{Path, PathBuf};

mod cloud;
mod dialogs;
mod panes;

const WIDTH: f32 = 700.;
const HEIGHT: f32 = 550.;
/// The width of the list of libraries (`SIDEBAR`), the height of one of its
/// rows (`ROW`), the height of the header (`HEADER`) and the height of the
/// footer (`FOOTER`), in points.
const SIDEBAR: f32 = 238.;
const ROW: f32 = 44.;
const HEADER: f32 = 58.;
const FOOTER: f32 = 44.;

/// One library in the Library Manager's list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: String,
    pub name: String,
    pub dir: String,
    pub current: bool,
    pub pinned: bool,
    pub missing: bool,
    pub kind: Kind,
    pub cover: Option<PathBuf>,
    /// The library's colour and icon (`libraryMeta` in the state file).
    pub look: crate::state::LibraryLook,
}

/// Returns the rows of the list: the pinned libraries first, then the
/// libraries of the history that are not pinned. When the open library is
/// not among them, a row for it is put first. A disposable copy gets a row
/// only in this way. A path is read (`exists`) only when `listed` allows
/// it.
pub fn rows(
    history: &[String],
    pinned: &[String],
    current: Option<(&str, Kind)>,
    listed: impl Fn(&str) -> bool,
    exists: impl Fn(&str) -> bool,
) -> Vec<Row> {
    let mut paths = pinned.to_vec();
    paths.extend(
        history
            .iter()
            .filter(|path| !pinned.contains(path))
            .cloned(),
    );
    let mut rows = paths
        .into_iter()
        .filter(|path| listed(path))
        .map(|path| {
            let (name, dir) = crate::shell::library::split(&path);
            Row {
                current: current.is_some_and(|(open, _)| open == path),
                pinned: pinned.contains(&path),
                missing: !exists(&path),
                kind: Kind::Real,
                name,
                dir,
                cover: None,
                look: Default::default(),
                path,
            }
        })
        .collect::<Vec<_>>();
    if let Some((open, kind)) = current
        && !rows.iter().any(|row| row.current)
    {
        let (name, dir) = crate::shell::library::split(open);
        rows.insert(
            0,
            Row {
                path: open.to_owned(),
                name,
                dir,
                current: true,
                pinned: false,
                missing: false,
                kind,
                cover: None,
                look: Default::default(),
            },
        );
    }
    rows
}

/// An action of the actions menu that changes or reveals a library on disk.
/// Each runs only after `host::permit` has allowed it. For a library that
/// the user has not yet agreed to, the user is asked for consent first, as
/// when the library is opened.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Rename,
    Reveal,
    Move,
    Delete,
}

/// What the right side of the window shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pane {
    Library,
    Create,
    /// The Open from Cloud… pane.
    Cloud,
}

struct ManagerGlobal(WindowHandle<Manager>);

impl Global for ManagerGlobal {}

/// Returns the Library Manager's window, or `None` when it is not open.
pub fn window(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<ManagerGlobal>()
        .map(|global| global.0.into())
}

/// Opens the Library Manager's window, or brings it forward when it is
/// already open.
pub fn open(cx: &mut App) {
    let create = false;
    let existing = cx.try_global::<ManagerGlobal>().map(|global| global.0);
    if let Some(existing) = existing
        && existing
            .update(cx, |manager, window, cx| {
                if create {
                    manager.pane = Pane::Create;
                }
                ui::window_chrome::activate(window);
                cx.notify();
            })
            .is_ok()
    {
        return;
    }
    let mut bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    bounds.origin = bounds.origin.map(|at| at.floor());
    let Ok(handle) = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(WIDTH), px(HEIGHT))),
            is_resizable: false,
            is_minimizable: false,
            // i18n: the window's title, English in every language
            ..ui::window_chrome::frameless("Library Manager", None)
        },
        move |window, cx| cx.new(|cx| Manager::new(create, window, cx)),
    ) else {
        return;
    };
    let _ = handle.update(cx, |_, window, _| {
        ui::window_chrome::hide_window_buttons(window);
        ui::window_chrome::activate(window);
    });
    cx.set_global(ManagerGlobal(handle));
}

pub struct Manager {
    rows: Vec<Row>,
    selected: Option<String>,
    pane: Pane,
    name: Entity<TextInput>,
    renaming: Option<Entity<TextInput>>,
    /// The subscription that saves the new name when the user presses Enter
    /// in the rename field.
    renamed: Option<Subscription>,
    /// The open library's statistics (the answer of `library.stats`).
    /// `reading` is whether they are being read now.
    stats: Option<serde_json::Value>,
    reading: bool,
    message: Option<String>,
    error: Option<String>,
    /// The scroll position of the right side. `error_shown` is the error
    /// that the right side was last scrolled to. The error is drawn at the
    /// end of the right side, so the right side is scrolled to its end when
    /// a new error appears.
    detail_scroll: ScrollHandle,
    /// The scroll position of the list of libraries on the left side.
    list_scroll: ScrollHandle,
    error_shown: Option<String>,
    /// The Remove from List question. Its content is the path of the library
    /// that is taken off the list when the user confirms.
    removing: ui::modal::Host<String>,
    /// The Delete… question. Its content is the path of the library that is
    /// deleted when the user confirms.
    deleting: ui::modal::Host<String>,
    /// The consent question that is asked before an `Act`. Its content is
    /// the library's identity, its folder, and the action that runs when
    /// the user agrees.
    asking: ui::modal::Host<(super::Identity, PathBuf, Act)>,
    /// The icon picker. It is `Some` while the user has it open with
    /// Change….
    icons: Option<Entity<ui::icon_picker::IconPicker>>,
    menu: menu::Host,
    busy: bool,
    /// The path of the library that was open when the list was last read.
    shown_current: Option<String>,
    focus: FocusHandle,
    /// The temporary folder that holds the cropped covers of the listed
    /// libraries.
    scratch: Option<tempfile::TempDir>,
    /// The open library's cloud sync state, and the subscription that
    /// redraws this window when the state changes.
    cloud: Option<(Entity<crate::cloud::Cloud>, Subscription)>,
    /// The cloud providers' folders that were detected on this device or
    /// chosen by the user. `choices` holds the Picto libraries found in
    /// those folders.
    roots: Vec<crate::cloud::Root>,
    choices: Vec<crate::cloud::Choice>,
    /// The index in `choices` of the cloud library chosen in the Open from
    /// Cloud… pane. `cloud_name` is the field for the name that the library
    /// gets on this device.
    chosen: Option<usize>,
    cloud_name: Entity<TextInput>,
    /// The name of the cloud operation that is running, or `None` when none
    /// is running.
    working: Option<&'static str>,
    /// The measurements from which the remaining time of the sync is
    /// estimated (`cloud::track`). `work_key` identifies the work that is
    /// measured, and `eta` is the estimate in seconds.
    samples: Vec<crate::cloud::Sample>,
    work_key: Option<String>,
    eta: Option<u64>,
    /// The Stop Syncing question.
    stopping: ui::modal::Host<()>,
    _subscriptions: Vec<Subscription>,
}

impl Manager {
    fn new(create: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(InputKind::Form, cx)
                .placeholder("My Inspirations")
                .submits()
        });
        let host = host::host(cx);
        let cloud_name = cx.new(|cx| TextInput::new(InputKind::Form, cx).submits());
        let subscriptions = vec![
            cx.observe(&name, |_, _, cx| cx.notify()),
            cx.observe(&cloud_name, |_, _, cx| cx.notify()),
            cx.subscribe(
                &cloud_name,
                |this, _, _: &crate::ui::text_input::Submitted, cx| this.join(cx),
            ),
            // Enter in the name field does what the Choose Location… button
            // does.
            cx.subscribe(
                &name,
                |this, _, _: &crate::ui::text_input::Submitted, cx| this.create(cx),
            ),
            // The list is read again whenever the host changes: when a
            // library has opened, when the app has switched to another
            // library, and when a library has failed to open.
            cx.observe(&host, |this, host, cx| {
                if let Some((_, message)) = host.read(cx).failure.clone() {
                    this.error = Some(message);
                }
                this.read(cx);
            }),
            cx.on_release(|_, cx| {
                cx.remove_global::<ManagerGlobal>();
            }),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            rows: Vec::new(),
            selected: None,
            pane: if create { Pane::Create } else { Pane::Library },
            name,
            renaming: None,
            renamed: None,
            stats: None,
            reading: false,
            message: None,
            error: host.read(cx).failure.clone().map(|(_, message)| message),
            detail_scroll: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            error_shown: None,
            removing: Default::default(),
            deleting: Default::default(),
            asking: Default::default(),
            icons: None,
            menu: menu::Host::deferred(ui::layer::MENU, menu::Return::Opener),
            busy: false,
            shown_current: None,
            focus,
            scratch: tempfile::Builder::new()
                .prefix("picto-gpui-covers.")
                .tempdir()
                .ok(),
            cloud: None,
            roots: Vec::new(),
            choices: Vec::new(),
            chosen: None,
            cloud_name,
            working: None,
            samples: Vec::new(),
            work_key: None,
            eta: None,
            stopping: Default::default(),
            _subscriptions: subscriptions,
        };
        this.read(cx);
        this.refresh_cloud(cx);
        this
    }

    /// Reads the list of libraries again, and the open library's
    /// statistics.
    fn read(&mut self, cx: &mut Context<Self>) {
        let facts = super::facts();
        let current = host::current(cx);
        let current_text = current
            .as_ref()
            .map(|(root, kind)| (root.to_string_lossy().into_owned(), *kind));
        // When another library has been opened, that library is chosen in
        // the list and its pane is shown.
        let opened = current_text.as_ref().map(|(path, _)| path.clone());
        if opened != self.shown_current {
            self.shown_current = opened.clone();
            if opened.is_some() {
                self.selected = opened;
                self.pane = Pane::Library;
            }
        }
        let state = crate::state::get(cx);
        let mut rows = rows(
            &state.library_history,
            &state.pinned_libraries,
            current_text
                .as_ref()
                .map(|(path, kind)| (path.as_str(), *kind)),
            |path| super::listed(path, &facts),
            |path| Path::new(path).join("library.sqlite").is_file(),
        );
        let scratch = self
            .scratch
            .as_ref()
            .map(|scratch| scratch.path().to_path_buf());
        for row in &mut rows {
            row.look = state
                .library_meta
                .get(&row.path)
                .cloned()
                .unwrap_or_default();
            if row.missing {
                continue;
            }
            row.cover = scratch.as_ref().and_then(|scratch| {
                let mut hasher = std::hash::DefaultHasher::new();
                std::hash::Hash::hash(&row.path, &mut hasher);
                let target = scratch.join(format!(
                    "library-cover-{:016x}.png",
                    std::hash::Hasher::finish(&hasher)
                ));
                crate::cover::prepare_into(Path::new(&row.path), &target)
            });
        }
        // When the chosen library is no longer in the list, the open library
        // is chosen, or the first row when no library is open.
        let keep = self
            .selected
            .as_ref()
            .is_some_and(|selected| rows.iter().any(|row| row.path == *selected));
        if !keep {
            self.selected = rows
                .iter()
                .find(|row| row.current)
                .or(rows.first())
                .map(|row| row.path.clone());
        }
        self.rows = rows;
        self.read_stats(cx);
        self.follow_cloud(cx);
        cx.notify();
    }

    fn read_stats(&mut self, cx: &mut Context<Self>) {
        let Some(application) = host::application(cx) else {
            self.stats = None;
            return;
        };
        self.reading = true;
        let answer = application.dispatch("library.stats", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let stats = answer.await.ok().and_then(Result::ok);
            let _ = this.update(cx, |this, cx| {
                this.stats = stats;
                this.reading = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn selected_row(&self) -> Option<&Row> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().find(|row| row.path == *selected)
    }

    fn choose(&mut self, path: String, cx: &mut Context<Self>) {
        self.selected = Some(path);
        self.pane = Pane::Library;
        self.chosen = None;
        self.renaming = None;
        self.icons = None;
        cx.notify();
    }

    /// Runs `act` on the library at `path` if `host::permit` allows it now.
    /// When the user has not yet agreed to the library, the consent question
    /// is shown first.
    fn guarded(&mut self, path: String, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.message = None;
        match host::permit(Path::new(&path), cx) {
            Ok(()) => self.perform(path, act, window, cx),
            Err(denied) => self.denied(denied, Some(act), cx),
        }
    }

    /// Handles a refusal of the host. For `Denied::Ask` it shows the consent
    /// question, and `act` runs when the user agrees. For `Denied::Error` it
    /// shows the message. For `Denied::Copy` it does nothing, because a copy
    /// that is not open is never listed.
    fn denied(&mut self, denied: host::Denied, act: Option<Act>, cx: &mut Context<Self>) {
        match denied {
            host::Denied::Ask { root, identity } => {
                if let Some(act) = act {
                    self.asking
                        .open((identity, root, act), ui::menu::Return::Opener, cx);
                }
            }
            host::Denied::Error(error) => self.error = Some(error),
            host::Denied::Copy => {}
        }
        cx.notify();
    }

    fn perform(&mut self, path: String, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            Act::Rename => self.start_rename(window, cx),
            Act::Reveal => crate::desktop::reveal_library(Path::new(&path), cx),
            Act::Move => self.move_library(path, cx),
            Act::Delete => {
                self.deleting.open(path, ui::menu::Return::Opener, cx);
            }
        }
    }

    /// Does Move…: the user chooses a folder in the system's dialog, and the
    /// library is moved into that folder.
    fn move_library(&mut self, path: String, cx: &mut Context<Self>) {
        let from = PathBuf::from(&path);
        let basename = from
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        // The system's folder dialog shows this text as the prompt of its
        // button.
        let chosen = crate::prompt::folder(
            &format!("{basename} will be placed in the selected folder"),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Some(folder)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match super::files::destination(&from, &folder) {
                    Ok(super::files::Moved::To(target)) => {
                        this.selected = Some(target.to_string_lossy().into_owned());
                        let manager = cx.entity().downgrade();
                        cx.defer(move |cx| {
                            host::move_library(from.clone(), target, cx, move |result, cx| {
                                let _ = manager.update(cx, |this, cx| this.relocated(from, result, cx));
                            })
                        });
                    }
                    Ok(super::files::Moved::Unchanged) => {}
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Called when the host has finished a rename or a move. After a failure
    /// the library's row stays chosen under its old path, and the reason is
    /// shown.
    fn relocated(&mut self, from: PathBuf, result: Result<(), host::Denied>, cx: &mut Context<Self>) {
        if let Err(denied) = result {
            self.selected = Some(from.to_string_lossy().into_owned());
            self.denied(denied, None, cx);
        }
        self.read(cx);
    }

    /// Called when the user has confirmed Delete…. The host checks the
    /// library again and then removes it (`host::delete`). If the host
    /// refuses, for example because the consent was withdrawn in the
    /// meantime, the library is left as it is.
    fn delete(&mut self, path: String, cx: &mut Context<Self>) {
        self.deleting.dismiss(cx);
        let manager = cx.entity().downgrade();
        host::delete(PathBuf::from(&path), cx, move |result, cx| {
            let _ = manager.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.selected = None;
                        this.message = Some(t("Library deleted.").to_owned());
                    }
                    Err(denied) => this.denied(denied, None, cx),
                }
                this.read(cx);
            });
        });
    }

    /// Changes the colour or the icon of the library at `path` with `change`
    /// and stores the result in this app's state. An entry that holds only
    /// default values is removed.
    fn set_look(
        &mut self,
        path: String,
        change: impl FnOnce(&mut crate::state::LibraryLook),
        cx: &mut Context<Self>,
    ) {
        crate::state::update(cx, |state| {
            let mut look = state.library_meta.get(&path).cloned().unwrap_or_default();
            change(&mut look);
            if look.is_default() {
                state.library_meta.remove(&path);
            } else {
                state.library_meta.insert(path, look);
            }
        });
        self.read(cx);
    }

    /// Does Change… and Done: opens the icon picker in the Appearance card,
    /// or closes it when it is open.
    fn toggle_icons(&mut self, path: String, current: Option<String>, cx: &mut Context<Self>) {
        if self.icons.take().is_some() {
            cx.notify();
            return;
        }
        let manager = cx.entity().downgrade();
        self.icons = Some(cx.new(|_| {
            ui::icon_picker::IconPicker::new(current, move |icon, _, cx| {
                let path = path.clone();
                let _ = manager.update(cx, |this, cx| {
                    this.set_look(path, |look| look.icon = icon, cx)
                });
            })
            .default_label(en("Use default library icon"))
        }));
        cx.notify();
    }

    /// Opens the library at `path`. The Open Library button and a double
    /// click on a row call this.
    fn open_row(&mut self, path: String, cx: &mut Context<Self>) {
        self.error = None;
        self.message = None;
        cx.defer(move |cx| host::request(Path::new(&path), Origin::Manager, cx));
    }

    /// Does Choose Location… of the New Library form: the user chooses the
    /// folder that the new library goes into, and the library is then
    /// created and opened (`host::create`).
    fn create(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value();
        if name.trim().is_empty() {
            self.error = Some(en("Library name cannot be empty").to_owned());
            cx.notify();
            return;
        }
        self.error = None;
        let chosen = crate::prompt::folder(t("Choose where to store the library"), cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(parent)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match super::new_library_path(&name, &parent) {
                    Ok(path) => {
                        this.message =
                            Some(crate::tf!("Created \"{value0}\".", name.trim()).to_owned());
                        this.name.update(cx, |field, cx| field.clear(cx));
                        this.pane = Pane::Library;
                        this.selected = Some(path.to_string_lossy().into_owned());
                        cx.notify();
                        cx.defer(move |cx| host::create(path, cx));
                    }
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let name = row.name.clone();
        let field = cx.new(|cx| {
            let mut field = TextInput::new(InputKind::Form, cx).submits();
            field.set_text(name, cx);
            field
        });
        // Enter in the rename field saves the new name.
        self.renamed = Some(cx.subscribe(
            &field,
            |this, _, _: &crate::ui::text_input::Submitted, cx| this.save_rename(cx),
        ));
        window.focus(&field.focus_handle(cx), cx);
        self.renaming = Some(field);
        cx.notify();
    }

    fn save_rename(&mut self, cx: &mut Context<Self>) {
        let (Some(field), Some(row)) = (self.renaming.take(), self.selected_row().cloned()) else {
            return;
        };
        let name = field.read(cx).value();
        cx.notify();
        if name.trim() == row.name {
            return;
        }
        match super::renamed_path(Path::new(&row.path), &name) {
            Ok(renamed) => {
                self.selected = Some(renamed.to_string_lossy().into_owned());
                let from = PathBuf::from(&row.path);
                let manager = cx.entity().downgrade();
                cx.defer(move |cx| {
                    host::rename(from.clone(), renamed, cx, move |result, cx| {
                        let _ = manager.update(cx, |this, cx| this.relocated(from, result, cx));
                    })
                });
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn open_actions(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        let real = row.kind == Kind::Real;
        let entries: Vec<menu::Entry> = vec![
            menu::item(
                "pin",
                if row.pinned {
                    t("Remove from Quick Access")
                } else {
                    t("Add to Quick Access")
                },
            )
            .icon("pin")
            .disabled(!real)
            .into(),
            menu::item("rename", t("Rename"))
                .icon("menu-pencil")
                .disabled(row.missing || !real)
                .into(),
            menu::item("reveal", t("Show in File Manager"))
                .icon("menu-folder-open")
                .disabled(row.missing)
                .into(),
            // For a missing library this row is Locate… and not Move….
            menu::item(
                "move",
                if row.missing {
                    t("Locate…")
                } else {
                    t("Move…")
                },
            )
            .icon("menu-folder-symlink")
            .disabled(self.busy || !real)
            .into(),
        ];
        let mut entries = entries;
        if row.current {
            entries.push(menu::Entry::Separator);
            entries.extend(self.cloud_rows(cx));
        }
        entries.extend([
            menu::Entry::Separator,
            menu::item("remove", t("Remove from List"))
                .icon("menu-x")
                .danger(true)
                .disabled(row.current || !real)
                .into(),
            // Delete… is disabled for the open library. For any other
            // library the user is asked to confirm it first.
            menu::item("delete", t("Delete…"))
                .icon("menu-trash")
                .danger(true)
                .disabled(row.current || row.missing || self.busy || !real)
                .into(),
        ]);
        let menu = cx.new(|cx| {
            Menu::new(entries, position, cx)
                .width(236.)
                .search(false)
                .in_window()
        });
        let path = row.path.clone();
        let missing = row.missing;
        self.menu.open(
            menu,
            window,
            cx,
            move |this, _, action, window, cx| match action {
                "pin" => {
                    let path = path.clone();
                    crate::state::update(cx, |state| {
                        history::toggle_pinned(&mut state.pinned_libraries, &path)
                    });
                    crate::set_app_menus(cx);
                    this.read(cx);
                }
                "rename" => this.guarded(path.clone(), Act::Rename, window, cx),
                "reveal" => this.guarded(path.clone(), Act::Reveal, window, cx),
                "move" if missing => {
                    let path = PathBuf::from(&path);
                    cx.defer(move |cx| host::locate(path, false, cx));
                }
                "move" => this.guarded(path.clone(), Act::Move, window, cx),
                "delete" => this.guarded(path.clone(), Act::Delete, window, cx),
                "remove" => {
                    this.removing
                        .open(path.clone(), ui::menu::Return::Opener, cx);
                }
                action => this.cloud_action(action, cx),
            },
        );
    }

    fn remove(&mut self, path: String, cx: &mut Context<Self>) {
        self.removing.dismiss(cx);
        crate::state::update(cx, |state| {
            history::remove(
                &mut state.library_history,
                &mut state.pinned_libraries,
                &mut state.last_library,
                &path,
            )
        });
        crate::set_app_menus(cx);
        if self.selected.as_deref() == Some(path.as_str()) {
            self.selected = None;
        }
        self.read(cx);
    }
}

/// Returns the library's colour as a colour to draw with, or `None` when the
/// library has none. The colour is stored as text of the form `#RRGGBB`.
fn look_color(look: &crate::state::LibraryLook) -> Option<Rgba> {
    look.color.as_deref().and_then(theme::parse_hex).map(rgb)
}

/// Does Choose… in the Cover row for the open library: brings the main
/// window forward and opens its cover dialog.
fn choose_cover(cx: &mut App) {
    if let Some(shell) = host::shell(cx) {
        crate::app_menu::bring_forward(cx);
        shell.update(cx, |shell, cx| shell.open_library_cover(None, cx));
    }
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        self.busy = host::host(cx).read(cx).busy();
        let detail = match (self.pane, self.selected_row().cloned()) {
            (Pane::Create, _) => self.create_pane(cx),
            (Pane::Cloud, _) => self.cloud_pane(cx),
            (Pane::Library, Some(row)) => self.library_pane(&row, window, cx),
            (Pane::Library, None) => div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text(theme::TEXT_XL)
                        .font_weight(FontWeight::BOLD)
                        .child(t("Get started")),
                )
                .child(
                    div()
                        .text(theme::TEXT_MD)
                        .text_color(theme.text_secondary())
                        .child(t(
                            "Create a library on this device or connect an existing cloud folder.",
                        )),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .w_full()
                        .child(self.connect_buttons("library-start-cloud", cx)),
                )
                .into_any_element(),
        };
        // A new error is drawn at the end of the right side, which is
        // usually below the visible part. So the right side is scrolled to
        // its end on the next frame, when the error has been laid out.
        if self.error != self.error_shown {
            self.error_shown = self.error.clone();
            if self.error.is_some() {
                let scroll = self.detail_scroll.clone();
                window.on_next_frame(move |window, _| {
                    let end = scroll.max_offset().y;
                    scroll.set_offset(point(scroll.offset().x, -end));
                    window.refresh();
                });
            }
        }
        // The right side scrolls when the library's cards are higher than
        // the window. Its scrollbar is drawn in the 22px of padding on the
        // right, so it covers none of the content.
        let detail = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .id("library-detail")
                    .size_full()
                    .p(px(22.))
                    .overflow_y_scroll()
                    .track_scroll(&self.detail_scroll)
                    .child(detail)
                    .children(self.feedback(cx)),
            )
            .child(ui::scrollbar(
                "library-detail-scrollbar",
                &self.detail_scroll,
                window,
                cx,
            ));
        div()
            .id("library-manager")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            // The window's background is the rail colour: the contrast
            // colour mixed 3% into the background colour.
            .bg(theme.rail())
            .text_color(theme.text())
            .text(theme::TEXT_MD)
            .font_family(theme::FONT_UI)
            .child(self.header(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.sidebar(window, cx))
                    .child(detail),
            )
            .child(self.footer(cx))
            .children(self.menu.layer(window, cx))
            .children(self.removal(window, cx))
            .children(self.deletion(window, cx))
            .children(self.consent(window, cx))
            .children(self.stop_question(window, cx))
            .children(ui::window_chrome::rim(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, Path, rows};

    #[test]
    fn rows_list_pinned_first_the_open_one_and_only_what_may_be_listed() {
        let history = vec![
            "/a/One.library".to_owned(),
            "/a/Two.library".to_owned(),
            "/k/Kept.library".to_owned(),
        ];
        let pinned = vec!["/a/Two.library".to_owned()];
        let rows = rows(
            &history,
            &pinned,
            Some(("/tmp/picto-gpui-x/Copy.library", Kind::Copy)),
            |path| path.starts_with("/a/"),
            |path| path != "/a/One.library",
        );
        let shown = rows
            .iter()
            .map(|row| {
                (
                    row.name.as_str(),
                    row.current,
                    row.pinned,
                    row.missing,
                    row.kind,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            shown,
            [
                ("Copy", true, false, false, Kind::Copy),
                ("Two", false, true, false, Kind::Real),
                ("One", false, false, true, Kind::Real),
            ]
        );
    }

    /// Listing libraries in the Library Manager reads their folders and
    /// covers and writes nothing to them.
    #[test]
    fn listing_a_kept_library_writes_nothing_to_it() {
        let folder = tempfile::tempdir().unwrap();
        let root = crate::libraries::admission::tests::real_library(folder.path(), "Kept");
        std::fs::write(root.join(".picto-library-cover.png"), b"not a picture").unwrap();
        let before = crate::libraries::admission::tests::snapshot(&root);
        let path = root.to_string_lossy().into_owned();
        let listed = rows(
            &[path.clone()],
            &[],
            None,
            |_| true,
            |path| Path::new(path).join("library.sqlite").is_file(),
        );
        assert_eq!(listed.len(), 1);
        let scratch = tempfile::tempdir().unwrap();
        let _ = crate::cover::prepare_into(&root, &scratch.path().join("cover.png"));
        assert_eq!(crate::libraries::admission::tests::snapshot(&root), before);
    }
}
