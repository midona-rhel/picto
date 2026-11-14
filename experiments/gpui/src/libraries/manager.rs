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
            },
        );
    }
    rows
}

/// What the right side shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pane {
    Library,
    Create,
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
    /// The open library's statistics (`library.stats`), and whether they
    /// are being read.
    stats: Option<serde_json::Value>,
    reading: bool,
    message: Option<String>,
    error: Option<String>,
    removing: Option<(String, ui::modal::Presence)>,
    menu: menu::Host,
    busy: bool,
    /// The open library when the list was last read.
    shown_current: Option<String>,
    focus: FocusHandle,
    /// Where the other libraries' covers are cropped to.
    scratch: Option<tempfile::TempDir>,
    _subscriptions: Vec<Subscription>,
}

impl Manager {
    fn new(create: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| TextInput::new(InputKind::Form, cx).placeholder("My Inspirations"));
        let host = host::host(cx);
        let subscriptions = vec![
            cx.observe(&name, |_, _, cx| cx.notify()),
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
            stats: None,
            reading: false,
            message: None,
            error: host.read(cx).failure.clone().map(|(_, message)| message),
            removing: None,
            menu: menu::Host::deferred(ui::layer::MENU, menu::Return::Opener),
            busy: false,
            shown_current: None,
            focus,
            scratch: tempfile::Builder::new()
                .prefix("picto-gpui-covers.")
                .tempdir()
                .ok(),
            _subscriptions: subscriptions,
        };
        this.read(cx);
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
        self.renaming = None;
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
            let mut field = TextInput::new(InputKind::Form, cx);
            field.set_text(name, cx);
            field
        });
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
                cx.defer(move |cx| host::rename(from, renamed, cx));
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
            .disabled(!real)
            .into(),
            menu::item("rename", t("Rename"))
                .disabled(row.missing || !real)
                .into(),
            menu::item("reveal", t("Show in File Manager"))
                .disabled(row.missing)
                .into(),
            // TODO: move a library to another folder, and locate a missing
            // one (`relocateLibrary`).
            menu::item(
                "move",
                if row.missing {
                    t("Locate…")
                } else {
                    t("Move…")
                },
            )
            .disabled(true)
            .into(),
            menu::Entry::Separator,
            menu::item("remove", t("Remove from List"))
                .danger(true)
                .disabled(row.current || !real)
                .into(),
            // TODO: delete a library and its files (`deleteLibrary`); not
            // built, as it removes a library the user keeps for good.
            menu::item("delete", t("Delete…"))
                .danger(true)
                .disabled(true)
                .into(),
        ];
        let menu = cx.new(|cx| Menu::new(entries, position, cx).width(236.).in_window());
        let path = row.path.clone();
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
                "rename" => this.start_rename(window, cx),
                "reveal" => crate::desktop::reveal_library(Path::new(&path), cx),
                "remove" => {
                    this.removing = Some((path.clone(), ui::modal::Presence::new()));
                    cx.notify();
                }
                _ => {}
            },
        );
    }

    fn remove(&mut self, path: String, cx: &mut Context<Self>) {
        self.removing = None;
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

    fn header(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        ui::drag_region("library-manager-header", window, cx)
            .h(px(HEADER))
            .flex_none()
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .flex_1()
                    .text(theme::TEXT_LG)
                    .font_weight(FontWeight::BOLD)
                    .child(t("Library Manager")),
            )
            .child(ui::window_chrome::close_button(
                "library-manager-close",
                |_, window, _| window.remove_window(),
            ))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let mut list = div().flex().flex_col().gap(px(2.));
        if self.rows.is_empty() {
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(12.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(t("No libraries yet.")),
            );
        }
        for (index, row) in self.rows.iter().enumerate() {
            let selected = self.selected.as_deref() == Some(row.path.as_str());
            let status = if row.current {
                Some(ui::icon("check", 13., theme.text()))
            } else if row.pinned {
                Some(ui::icon("pin", 12., theme.text_tertiary()))
            } else if row.missing {
                Some(ui::icon("alert-circle", 12., theme.negative()))
            } else {
                None
            };
            let path = row.path.clone();
            let open = row.path.clone();
            let can_open = !row.current && !row.missing;
            list = list.child(
                div()
                    .id(("library-row", index))
                    .role(Role::Button)
                    .aria_label(row.name.clone())
                    .aria_selected(selected)
                    .h(px(ROW))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(theme::RADIUS_MD))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.surface_active()))
                    .when(!selected, |row| {
                        row.hover(move |style| style.bg(theme.surface_hover()))
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() >= 2 && can_open {
                            this.open_row(open.clone(), cx);
                        } else {
                            this.choose(path.clone(), cx);
                        }
                    }))
                    .child(
                        ui::library_avatar(("library-row-avatar", index), row.cover.clone(), 42.)
                            .highlighted(selected),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text(theme::TEXT_MD)
                                    .truncate()
                                    .child(row.name.clone()),
                            )
                            .child(
                                div()
                                    .text(theme::TEXT_CAPTION)
                                    .text_color(theme.text_tertiary())
                                    .truncate()
                                    .child(row.dir.clone()),
                            ),
                    )
                    .children(status),
            );
        }
        div()
            .w(px(SIDEBAR))
            .flex_none()
            .h_full()
            .px(px(8.))
            .py(px(14.))
            .border_r_1()
            .border_color(theme.border_secondary())
            // A further 3% of the contrast colour over the window's.
            .bg(theme.rail_raised())
            .flex()
            .flex_col()
            .child(
                div()
                    .mx(px(8.))
                    .mb(px(7.))
                    .text(theme::TEXT_CAPTION)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary())
                    .child(t("On this device")),
            )
            .child(
                div()
                    .id("library-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
    }

    fn card(title: &'static str, cx: &App) -> Div {
        let theme = theme::theme(cx);
        div()
            .p(px(14.))
            .border_1()
            .border_color(theme.border_secondary())
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.control_surface())
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .font_weight(FontWeight::MEDIUM)
                    .child(t(title)),
            )
    }

    fn library_pane(&self, row: &Row, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let can_open = !row.current && !row.missing;
        let open_path = row.path.clone();
        let mut actions = div().flex().items_center().gap(px(8.));
        if can_open {
            actions = actions.child(
                ui::action_button::action_button("library-open", ui::action_button::Look::Primary)
                    .label(t("Open Library"))
                    .disabled(self.busy)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.open_row(open_path.clone(), cx)),
                    ),
            );
        }
        actions = actions.child(
            ui::icon_button("library-actions", "dots-vertical")
                .name(t("Library actions"))
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    this.open_actions(event.position(), window, cx)
                })),
        );
        let title = match &self.renaming {
            Some(field) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(240.))
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(theme::RADIUS_MD))
                        .border_1()
                        .border_color(theme.primary_at(60.))
                        .flex()
                        .items_center()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            match event.keystroke.key.as_str() {
                                "enter" => this.save_rename(cx),
                                "escape" => {
                                    this.renaming = None;
                                    cx.notify();
                                }
                                _ => {}
                            }
                        }))
                        .child(field.clone()),
                )
                .child(
                    ui::action_button::action_button(
                        "library-rename-save",
                        ui::action_button::Look::Primary,
                    )
                    .label(t("Save"))
                    .on_click(cx.listener(|this, _, _, cx| this.save_rename(cx))),
                )
                .child(
                    ui::action_button::action_button(
                        "library-rename-cancel",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.renaming = None;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            None => div()
                .text(theme::TEXT_XL)
                .font_weight(FontWeight::BOLD)
                .truncate()
                .child(row.name.clone())
                .into_any_element(),
        };
        let hero = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::library_avatar(
                "library-hero-avatar",
                row.cover.clone(),
                56.,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(title)
                    .child(
                        div()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .truncate()
                            .child(row.path.clone()),
                    ),
            )
            .child(actions);
        let mut status = div()
            .min_h(px(32.))
            .flex()
            .items_center()
            .gap(px(14.))
            .text(theme::TEXT_CAPTION)
            .text_color(theme.text_secondary());
        if row.current {
            status = status.child(t("Current library"));
        }
        if row.kind == Kind::Copy {
            status = status.child(en("Library copy"));
        }
        if row.pinned {
            status = status.child(t("Pinned"));
        }
        if row.missing {
            status = status.child(
                div()
                    .text_color(theme.negative())
                    .child(t("Missing on disk")),
            );
        }
        // `Appearance`: the cover, chosen in the main window's cover dialog
        // for the open library.
        // TODO: a library's colour and icon (`libraryMeta.color`, `.icon`).
        let current = row.current;
        // i18n: translated where drawn
        let appearance = Self::card("Appearance", cx).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .w(px(64.))
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_secondary())
                        .child(t("Cover")),
                )
                .child(
                    ui::action_button::action_button(
                        "library-cover",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Choose…"))
                    .disabled(!current)
                    .tip(ui::tip(if current {
                        t("Choose a media item and crop the library cover")
                    } else {
                        t("Open this library before choosing its media")
                    }))
                    .on_click(|_, _, cx| cx.defer(choose_cover)),
                ),
        );
        // i18n: translated where drawn
        let statistics = Self::card("Library", cx)
            .mt(px(12.))
            .child(self.statistics(row, cx));
        let _ = window;
        div()
            .flex()
            .flex_col()
            .child(hero)
            .child(status)
            .child(appearance)
            .child(statistics)
            .into_any_element()
    }

    fn statistics(&self, row: &Row, cx: &App) -> AnyElement {
        let theme = theme::theme(cx);
        let line = |text: &str| {
            div()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child(text.to_owned())
                .into_any_element()
        };
        if !row.current {
            return line(t("Open this library to view its statistics."));
        }
        let Some(stats) = &self.stats else {
            return line(t("Loading library statistics…"));
        };
        let count =
            |key: &str| crate::i18n::format::grouped(i128::from(stats[key].as_i64().unwrap_or(0)));
        // i18n: translated where drawn
        let fields = [
            ("All", count("active_items")),
            ("Inbox", count("inbox_items")),
            ("Trash", count("trash_items")),
            ("Media", count("media_assets")),
            ("Images", count("image_assets")),
            ("Tags", count("tags")),
            ("Subscriptions", count("subscriptions")),
            (
                "Size",
                crate::text::size_to_terabytes(stats["original_bytes"].as_u64().unwrap_or(0)),
            ),
        ];

        let mut grid = div().flex().flex_wrap().gap_y(px(12.));
        for (label, value) in fields {
            grid = grid.child(
                div()
                    .w(relative(0.25))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text(theme::TEXT_LG)
                            .font_weight(FontWeight::MEDIUM)
                            .child(value),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(t(label)),
                    ),
            );
        }
        grid.into_any_element()
    }

    fn create_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let field = self.name.focus_handle(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon("library-plus", 18., theme.text()))
                    .child(
                        div()
                            .text(theme::TEXT_XL)
                            .font_weight(FontWeight::BOLD)
                            .child(t("Create a new library")),
                    ),
            )
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text_secondary())
                    .child(t(
                        "Choose a name now; Picto will ask where the library should be stored.",
                    )),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .child(t("Library name")),
            )
            .child(
                div()
                    .w(px(360.))
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border_secondary())
                    .bg(theme.control_surface())
                    .flex()
                    .items_center()
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&field, cx)
                    })
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" {
                            this.create(cx);
                        }
                    }))
                    .child(self.name.clone()),
            )
            .child(
                div().mt(px(4.)).child(
                    ui::action_button::action_button(
                        "library-create",
                        ui::action_button::Look::Primary,
                    )
                    .label(if self.busy {
                        t("Creating…")
                    } else {
                        t("Choose Location…")
                    })
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                ),
            )
            .into_any_element()
    }

    fn feedback(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = theme::theme(cx);
        if let Some(error) = &self.error {
            return Some(
                div()
                    .mt(px(14.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(theme::RADIUS_LG))
                    .border_1()
                    .border_color(theme.negative_at(45.))
                    .bg(theme.control_surface())
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text())
                            .child(error.clone()),
                    )
                    .child(
                        // i18n: an icon's name
                        ui::icon_button("library-error-dismiss", "x")
                            .name(t("Dismiss notification"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        self.message.as_ref().map(|message| {
            div()
                .mt(px(14.))
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(message.clone())
                .into_any_element()
        })
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        div()
            .h(px(FOOTER))
            .flex_none()
            .px(px(14.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(
                ui::action_button::action_button("library-new", ui::action_button::Look::Standard)
                    .icon("plus", 14.)
                    .label(t("New Library…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.pane = Pane::Create;
                        this.error = None;
                        window.focus(&this.name.focus_handle(cx), cx);
                        cx.notify();
                    })),
            )
            .child(
                ui::action_button::action_button(
                    "library-open-existing",
                    ui::action_button::Look::Standard,
                )
                .icon("library-folder-open", 14.)
                .label(t("Open Existing…"))
                .on_click(|_, _, cx| cx.defer(host::open_dialog)),
            )
        // TODO: Open from Cloud… (`joinCloudLibrary`); cloud sync is not
        // built.
    }

    fn removal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, presence) = self.removing.as_ref()?;
        let name = super::name(path);
        let path = path.clone();
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.removing = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.removing = None;
            cx.notify();
        });
        let accept =
            cx.listener(move |this: &mut Self, _: &(), _, cx| this.remove(path.clone(), cx));
        Some(
            ui::confirm::confirm(
                "library-remove",
                t("Remove from List"),
                crate::tf!(
                    "Remove \"{value0}\" from this list?\n\nThe library and its files remain on disk.",
                    name
                ),
                t("Remove"),
                *presence,
            )
            .danger(true)
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }
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
                .into_any_element(),
        };
        let detail = div()
            .id("library-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(22.))
            .overflow_y_scroll()
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
            .children(self.removal(cx))
            .children(ui::window_chrome::rim(cx))
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
