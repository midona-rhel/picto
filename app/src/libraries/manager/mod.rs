//! The Library Manager (`LibraryManager.tsx`, `library-manager.html`): a
//! window of its own, 700 by 550 points and not resizable, with the
//! libraries on this device at the left and the chosen one at the right
//! (its cover, name and folder, Open Library and its actions, whether it is
//! open, pinned or missing, its appearance and its statistics), or the form
//! for a new library; New Library… and Open Existing… at the foot.
//!
//! The list is this app's own (`state.json`), and only the libraries this
//! process may open are listed (`libraries::listed`): a process that cannot
//! open a library the user keeps reads nothing of one. Listing reads each
//! listed library's folder and cover and nothing else; statistics are read
//! for the open library only, through its application, as production reads
//! them.
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
/// `.sidebar`'s width, `.libraryRow`'s height, the header's and the
/// footer's.
const SIDEBAR: f32 = 238.;
const ROW: f32 = 44.;
const HEADER: f32 = 58.;
const FOOTER: f32 = 44.;

/// A library in the list.
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
    /// Its colour and icon (`libraryMeta`).
    pub look: crate::state::LibraryLook,
}

/// The rows in production's order (`orderedLibraryPaths`): pinned first,
/// then the history without them; the open library first when the list
/// does not hold it (a disposable copy is never listed otherwise). Only
/// paths `listed` allows are read at all.
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

/// What the actions menu does to a library that is not open, each only
/// once `host::permit` lets it (asking the user's consent first, as opening
/// does, for a library not yet agreed to): rename and Show in File Manager
/// act on a library the user has not opened here, so they are asked about
/// as opening is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Rename,
    Reveal,
    Move,
    Delete,
}

/// What the right side shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pane {
    Library,
    Create,
    /// Open from Cloud… (`showCloudOpen`).
    Cloud,
}

struct ManagerGlobal(WindowHandle<Manager>);

impl Global for ManagerGlobal {}

/// The open Library Manager's window, if any.
pub fn window(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<ManagerGlobal>()
        .map(|global| global.0.into())
}

/// `open_library_manager`: opens the window, or brings it forward.
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
            // i18n: the window's title, English as library-manager.html's <title>
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
    /// Hears Enter in the rename field.
    renamed: Option<Subscription>,
    /// The open library's statistics (`library.stats`), and whether they
    /// are being read.
    stats: Option<serde_json::Value>,
    reading: bool,
    message: Option<String>,
    error: Option<String>,
    /// The detail pane's scrolling, and the error it has yet to bring into
    /// view (the error is drawn at the pane's end, as production draws it).
    detail_scroll: ScrollHandle,
    error_shown: Option<String>,
    /// A library to take off the list, once the user confirms.
    removing: ui::modal::Host<String>,
    /// A library about to be deleted, once the user confirms (`deleteLibrary`).
    deleting: ui::modal::Host<String>,
    /// A library the user is asked about before `Act` (its consent).
    asking: ui::modal::Host<(super::Identity, PathBuf, Act)>,
    /// The icon picker, while Change… has it open.
    icons: Option<Entity<ui::icon_picker::IconPicker>>,
    menu: menu::Host,
    busy: bool,
    /// The open library when the list was last read.
    shown_current: Option<String>,
    focus: FocusHandle,
    /// Where the other libraries' covers are cropped to.
    scratch: Option<tempfile::TempDir>,
    /// The open library's cloud sync state, followed while it is open.
    cloud: Option<(Entity<crate::cloud::Cloud>, Subscription)>,
    /// The provider folders on this device and those the user chose, and
    /// the Picto libraries found in them (`refreshCloud`).
    roots: Vec<crate::cloud::Root>,
    choices: Vec<crate::cloud::Choice>,
    /// The cloud library chosen in Open from Cloud…, and its local name.
    chosen: Option<usize>,
    cloud_name: Entity<TextInput>,
    /// A cloud operation under way (production's `busy` key).
    working: Option<&'static str>,
    /// How fast the sync's remaining work falls, and the estimate.
    samples: Vec<crate::cloud::Sample>,
    work_key: Option<String>,
    eta: Option<u64>,
    /// Stop Syncing's question.
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
            // Enter in the name chooses the location, as the button does.
            cx.subscribe(
                &name,
                |this, _, _: &crate::ui::text_input::Submitted, cx| this.create(cx),
            ),
            // An open, a switch, a failure: read the list again.
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

    /// Reads the list again, and the open library's statistics.
    fn read(&mut self, cx: &mut Context<Self>) {
        let facts = super::facts();
        let current = host::current(cx);
        let current_text = current
            .as_ref()
            .map(|(root, kind)| (root.to_string_lossy().into_owned(), *kind));
        // A library opened or switched to is chosen, as production's
        // manager follows `library-switched`.
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
        // The open library is chosen when nothing else is.
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

    /// Runs `act` on the library at `path` if `host::permit` lets it now,
    /// asking the user's consent first where opening would.
    fn guarded(&mut self, path: String, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.message = None;
        match host::permit(Path::new(&path), cx) {
            Ok(()) => self.perform(path, act, window, cx),
            Err(denied) => self.denied(denied, Some(act), cx),
        }
    }

    /// What the host refused: the consent asked for (then `act` run), or
    /// its words shown. A copy that is not open is never listed.
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

    /// Move… (`relocateLibrary` for a library on disk): the folder it goes
    /// into, chosen in the system's dialog, then moved there.
    fn move_library(&mut self, path: String, cx: &mut Context<Self>) {
        let from = PathBuf::from(&path);
        let basename = from
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        // Production's dialog says this as its message.
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

    /// A rename or move the host finished: on failure the library keeps
    /// its row selected under its old path, and the reason is shown.
    fn relocated(&mut self, from: PathBuf, result: Result<(), host::Denied>, cx: &mut Context<Self>) {
        if let Err(denied) = result {
            self.selected = Some(from.to_string_lossy().into_owned());
            self.denied(denied, None, cx);
        }
        self.read(cx);
    }

    /// The user confirmed Delete…: the host checks it again, as the moment
    /// may have changed, then removes it. A consent withdrawn meanwhile
    /// leaves it as it is.
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

    /// The library's colour or icon (`setLibraryMeta`), kept in this app's
    /// state.
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

    /// Change… / Done: the icon picker under the appearance card.
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

    /// Open Library, or a double click on a row.
    fn open_row(&mut self, path: String, cx: &mut Context<Self>) {
        self.error = None;
        self.message = None;
        cx.defer(move |cx| host::request(Path::new(&path), Origin::Manager, cx));
    }

    /// Choose Location…: where the new library goes, then it is made and
    /// opened (`createLibrary`).
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
        // Enter saves (`onKeyDown` Enter).
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
            // A missing library is located instead of moved.
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
            // Production asks for confirmation and refuses the open library.
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

/// A library's colour (`#RRGGBB`), to draw.
fn look_color(look: &crate::state::LibraryLook) -> Option<Rgba> {
    look.color.as_deref().and_then(theme::parse_hex).map(rgb)
}

/// Cover › Choose… for the open library: the main window's cover dialog.
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
        // A new error is scrolled to once it has been laid out: at the
        // window's usual size it lies below what the pane shows.
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
        let detail = div()
            .id("library-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(22.))
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .child(detail)
            .children(self.feedback(cx));
        div()
            .id("library-manager")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            // `--manager-shell-bg`: 3% of the contrast colour.
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
                    .child(self.sidebar(cx))
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
