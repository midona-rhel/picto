//! The folder picker the inspector opens (`FolderPickerPanel.tsx` in its
//! `OverlayShell`, with `FolderTree`): the folder tree with a search, the
//! items' folders checked, a click that adds the items to a folder or takes
//! them out at once, the recent and selected views, and New Subfolder and
//! New Sibling Folder from a row's menu.
use super::membership;
use crate::backend::Navigation;
use crate::session::Session;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look as ButtonLook,
    field::Look,
    menu::{self, Menu},
    modal::{Presence, Size},
    overlay_shell::{self, Geometry, Mark, Nav},
    text_input::{Submitted, TextInput},
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::FolderId;
use picto_library::predicate::SetMatchMode;
use picto_library::query::ItemScope;
use std::collections::{BTreeSet, HashSet};

const WIDTH: f32 = 360.;
const HEIGHT: f32 = 480.;
const ROW: f32 = 26.;
/// `useRecentFolders(20)`, of the 30 remembered.
const RECENT_SHOWN: usize = 20;

/// `FolderView`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    All,
    Recent,
    Selected,
}

impl View {
    /// Tab: All, Recent, Selected, then All again.
    pub fn next(self) -> Self {
        match self {
            View::All => View::Recent,
            View::Recent => View::Selected,
            View::Selected => View::All,
        }
    }
}

pub enum Event {
    /// Add the items to the folder (`add`) or take them out.
    Toggle { folder: FolderId, add: bool },
    /// In the filter mode, the folders included and excluded and the match
    /// rule (`onApplyFolderFilter`).
    FilterChanged {
        include: Vec<FolderId>,
        exclude: Vec<FolderId>,
        mode: SetMatchMode,
    },
}

/// The picker as the filter bar opens it (`onApplyFolderFilter`): a click
/// includes a folder (Shift takes the range from the last one clicked), a
/// right click excludes it, under a match rule.
struct FilterMode {
    excluded: BTreeSet<FolderId>,
    mode: SetMatchMode,
    last_clicked: Option<FolderId>,
}

impl EventEmitter<Event> for FolderPicker {}

/// A folder as the tree shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    pub parent: Option<FolderId>,
    pub color: Option<u32>,
}

/// The library's folders from the sidebar's destinations, in their order.
pub fn folders(nav: &[Navigation]) -> Vec<Folder> {
    let id = |entry: &Navigation| match entry.scope {
        ItemScope::FolderTree { folder_id } if entry.screen.is_none() => Some(folder_id),
        _ => None,
    };
    nav.iter()
        .filter_map(|entry| {
            Some(Folder {
                id: id(entry)?,
                name: entry.label.clone(),
                parent: entry.parent.and_then(|parent| nav.get(parent)).and_then(id),
                color: entry.tint,
            })
        })
        .collect()
}

/// One line of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub folder: Folder,
    pub depth: usize,
    /// For each ancestor below the top level, whether its line continues
    /// past this row.
    pub continues: Vec<bool>,
    pub last: bool,
    pub children: bool,
    pub expanded: bool,
    pub parent_name: Option<String>,
}

fn matches(folders: &[Folder], folder: &Folder, query: &str) -> bool {
    folder.name.to_lowercase().contains(query)
        || folders
            .iter()
            .filter(|child| child.parent == Some(folder.id))
            .any(|child| matches(folders, child, query))
}

/// `FolderTree`'s visible rows: each folder under its parent, folded
/// branches left out. Searching shows the folders whose name holds the text
/// with their ancestors, every branch open.
pub fn rows(folders: &[Folder], folded: &HashSet<FolderId>, query: &str) -> Vec<Row> {
    let query = query.trim().to_lowercase();
    let known = folders
        .iter()
        .map(|folder| folder.id)
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    fn walk(
        folders: &[Folder],
        parent: Option<FolderId>,
        depth: usize,
        continues: &[bool],
        folded: &HashSet<FolderId>,
        query: &str,
        known: &HashSet<FolderId>,
        out: &mut Vec<Row>,
    ) {
        let children = folders
            .iter()
            .filter(|folder| folder.parent.filter(|parent| known.contains(parent)) == parent)
            .collect::<Vec<_>>();
        let count = children.len();
        for (index, folder) in children.into_iter().enumerate() {
            if !query.is_empty() && !matches(folders, folder, query) {
                continue;
            }
            let last = index + 1 == count;
            let has_children = folders.iter().any(|child| child.parent == Some(folder.id));
            let expanded = if query.is_empty() {
                !folded.contains(&folder.id)
            } else {
                true
            };
            let parent_name = folder
                .parent
                .and_then(|parent| folders.iter().find(|other| other.id == parent))
                .map(|parent| parent.name.clone());
            out.push(Row {
                folder: folder.clone(),
                depth,
                continues: continues.to_vec(),
                last,
                children: has_children,
                expanded,
                parent_name,
            });
            if has_children && expanded {
                let mut below = continues.to_vec();
                if depth > 0 {
                    below.push(!last);
                }
                walk(
                    folders,
                    Some(folder.id),
                    depth + 1,
                    &below,
                    folded,
                    query,
                    known,
                    out,
                );
            }
        }
    }
    walk(folders, None, 0, &[], folded, &query, &known, &mut out);
    out
}

/// The Recent and Selected views: those folders alone, in that order, as a
/// flat list without their parents.
pub fn listed(all: &[Folder], ids: &[FolderId]) -> Vec<Folder> {
    ids.iter()
        .filter_map(|id| all.iter().find(|folder| folder.id == *id))
        .map(|folder| Folder {
            parent: None,
            ..folder.clone()
        })
        .collect()
}

/// The New Subfolder and New Sibling Folder dialog.
struct Create {
    title: &'static str,
    parent: Option<FolderId>,
    name: Entity<TextInput>,
    presence: Presence,
    focus: FocusHandle,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

pub struct FolderPicker {
    session: Entity<Session>,
    search: Entity<TextInput>,
    focus: FocusHandle,
    opening: Option<ui::popover::Opening>,
    geometry: Geometry,
    query: String,
    view: View,
    /// The folders the items are in, changed by each click.
    selected: BTreeSet<FolderId>,
    dirty: bool,
    folded: HashSet<FolderId>,
    focus_index: Option<usize>,
    scroll: ScrollHandle,
    menu: Option<Entity<Menu>>,
    menu_folder: Option<Folder>,
    _menu: Option<Subscription>,
    create: Option<Create>,
    /// The keyboard goes back to the search at the next frame.
    refocus: bool,
    filter: Option<FilterMode>,
    _subscriptions: Vec<Subscription>,
}

impl FolderPicker {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(TextInput::menu_search);
        let subscriptions = vec![cx.observe(&search, |this, search, cx| {
            let query = search.read(cx).value();
            if query != this.query {
                this.query = query;
                this.focus_index = None;
                cx.notify();
            }
        })];
        Self {
            session,
            search,
            focus: cx.focus_handle(),
            opening: None,
            geometry: Geometry {
                right: None,
                left: 0.,
                top: 0.,
            },
            query: String::new(),
            view: View::All,
            selected: BTreeSet::new(),
            dirty: false,
            folded: HashSet::new(),
            focus_index: None,
            scroll: ScrollHandle::new(),
            menu: None,
            menu_folder: None,
            _menu: None,
            create: None,
            refocus: false,
            filter: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.opening.is_some()
    }

    pub fn open(
        &mut self,
        anchor: Option<Point<f32>>,
        folders: Vec<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = None;
        self.show(anchor, folders, window, cx);
    }

    /// Opens in the filter mode below a control of the filter bar, on the
    /// folders the filter includes and excludes.
    pub fn open_filter(
        &mut self,
        below: Point<f32>,
        include: Vec<FolderId>,
        exclude: Vec<FolderId>,
        mode: SetMatchMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = Some(FilterMode {
            excluded: exclude.into_iter().collect(),
            mode,
            last_clicked: None,
        });
        self.show(None, include, window, cx);
        let view = window.viewport_size();
        self.geometry = overlay_shell::place_below(
            below,
            size(f32::from(view.width), f32::from(view.height)),
            WIDTH,
            HEIGHT,
        );
    }

    fn show(
        &mut self,
        anchor: Option<Point<f32>>,
        folders: Vec<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.opening.is_none() {
            self.opening = Some(ui::popover::Opening::new(window, cx));
        }
        let view = window.viewport_size();
        self.geometry = overlay_shell::place(
            anchor,
            size(f32::from(view.width), f32::from(view.height)),
            WIDTH,
            HEIGHT,
        );
        self.query.clear();
        self.search.update(cx, |search, cx| search.clear(cx));
        self.view = View::All;
        self.selected = folders.into_iter().collect();
        self.dirty = false;
        // `FolderTree` opens with every branch open.
        self.folded.clear();
        self.focus_index = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        let handle = self.search.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// The items' folders changed (or arrived).
    pub fn set_entity(&mut self, folders: Vec<FolderId>, cx: &mut Context<Self>) {
        if self.is_open() {
            self.selected = folders.into_iter().collect();
            cx.notify();
        }
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opening) = self.opening.take() else {
            return;
        };
        if let Some(menu) = &self.menu {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
        self.create = None;
        opening.finish(&self.focus, window, cx);
        cx.notify();
    }

    fn all_folders(&self, cx: &App) -> Vec<Folder> {
        folders(&self.session.read(cx).nav)
    }

    fn visible(&self, cx: &App) -> Vec<Row> {
        let all = self.all_folders(cx);
        match self.view {
            View::All => rows(&all, &self.folded, &self.query),
            View::Recent => {
                let shown = self
                    .session
                    .read(cx)
                    .recent_folders
                    .iter()
                    .copied()
                    .take(RECENT_SHOWN)
                    .collect::<Vec<_>>();
                rows(&listed(&all, &shown), &self.folded, &self.query)
            }
            View::Selected => {
                let ids = self.selected.iter().copied().collect::<Vec<_>>();
                rows(&listed(&all, &ids), &self.folded, &self.query)
            }
        }
    }

    /// A click on a folder. In the filter mode, Shift takes the range from
    /// the last one clicked (keeping the rest with Cmd) and a plain click
    /// toggles; either way the folder is no longer excluded.
    fn click(&mut self, folder: FolderId, modifiers: Modifiers, cx: &mut Context<Self>) {
        if self.filter.is_none() {
            self.toggle(folder, cx);
            return;
        }
        let order = self
            .visible(cx)
            .iter()
            .map(|row| row.folder.id)
            .collect::<Vec<_>>();
        let Some(filter) = &mut self.filter else {
            return;
        };
        filter.excluded.remove(&folder);
        let range = filter
            .last_clicked
            .filter(|_| modifiers.shift)
            .and_then(|last| {
                Some((
                    order.iter().position(|id| *id == last)?,
                    order.iter().position(|id| *id == folder)?,
                ))
            });
        match range {
            Some((from, to)) => {
                if !(modifiers.platform || modifiers.control) {
                    self.selected.clear();
                }
                self.selected
                    .extend(order[from.min(to)..=from.max(to)].iter().copied());
            }
            None => {
                membership::toggle(&mut self.selected, folder);
            }
        }
        filter.last_clicked = Some(folder);
        self.dirty = true;
        self.session
            .update(cx, |session, _| session.record_folder_use(folder));
        self.emit_filter(cx);
        cx.notify();
    }

    /// `handleExclude`: in the filter mode, a right click excludes the
    /// folder, or no longer.
    fn exclude(&mut self, folder: FolderId, cx: &mut Context<Self>) {
        let Some(filter) = &mut self.filter else {
            return;
        };
        self.selected.remove(&folder);
        membership::toggle(&mut filter.excluded, folder);
        self.dirty = true;
        self.session
            .update(cx, |session, _| session.record_folder_use(folder));
        self.emit_filter(cx);
        cx.notify();
    }

    fn set_mode(&mut self, mode: SetMatchMode, cx: &mut Context<Self>) {
        if let Some(filter) = &mut self.filter {
            filter.mode = mode;
            self.emit_filter(cx);
            cx.notify();
        }
    }

    fn emit_filter(&mut self, cx: &mut Context<Self>) {
        if let Some(filter) = &self.filter {
            cx.emit(Event::FilterChanged {
                include: self.selected.iter().copied().collect(),
                exclude: filter.excluded.iter().copied().collect(),
                mode: filter.mode,
            });
        }
    }

    /// `handleToggle` for the inspector's items: added or taken out at once.
    fn toggle(&mut self, folder: FolderId, cx: &mut Context<Self>) {
        self.dirty = true;
        let add = membership::toggle(&mut self.selected, folder);
        if add {
            self.session
                .update(cx, |session, _| session.record_folder_use(folder));
        }
        cx.emit(Event::Toggle { folder, add });
        cx.notify();
    }

    fn navigate(&mut self, key: Nav, cx: &mut Context<Self>) -> bool {
        if self.create.is_some() || self.menu.is_some() {
            return false;
        }
        let rows = self.visible(cx);
        match key {
            Nav::Next => {
                self.view = self.view.next();
                self.focus_index = None;
            }
            Nav::Down => {
                if !rows.is_empty() {
                    self.focus_index = Some(
                        self.focus_index
                            .map_or(0, |at| (at + 1).min(rows.len() - 1)),
                    );
                }
            }
            Nav::Up => {
                self.focus_index = Some(self.focus_index.map_or(0, |at| at.saturating_sub(1)));
            }
            Nav::Confirm => {
                if let Some(row) = rows.get(self.focus_index.unwrap_or(0)) {
                    self.click(row.folder.id, Modifiers::default(), cx);
                }
            }
            Nav::Left | Nav::Right => return false,
        }
        self.reveal_focus();
        cx.notify();
        true
    }

    fn reveal_focus(&mut self) {
        let Some(at) = self.focus_index else { return };
        let top = 4. + at as f32 * ROW;
        let viewport = f32::from(self.scroll.bounds().size.height);
        let scrolled = -f32::from(self.scroll.offset().y);
        let next = if top < scrolled {
            top
        } else if top + ROW > scrolled + viewport {
            top + ROW - viewport
        } else {
            return;
        };
        self.scroll.set_offset(point(px(0.), px(-next.max(0.))));
    }

    // ── New folders ─────────────────────────────────────────────

    fn open_menu(
        &mut self,
        folder: Folder,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = vec![
            menu::item("new-subfolder", "New Subfolder")
                .icon("menu-folder-plus")
                .into(),
            menu::item("new-sibling", "New Sibling Folder")
                .icon("menu-folder-plus")
                .into(),
        ];
        let menu = cx.new(|cx| Menu::new(entries, position, cx).search(false));
        self._menu =
            Some(
                cx.subscribe_in(&menu, window, |this, _, event: &menu::Event, window, cx| {
                    let menu::Event::Chosen(action) = event;
                    let Some(folder) = this.menu_folder.clone() else {
                        return;
                    };
                    match *action {
                        "new-subfolder" => {
                            this.start_create("New Subfolder", Some(folder.id), window, cx)
                        }
                        "new-sibling" => {
                            this.start_create("New Sibling Folder", folder.parent, window, cx)
                        }
                        _ => {}
                    }
                }),
            );
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(menu);
        self.menu_folder = Some(folder);
        cx.notify();
    }

    fn start_create(
        &mut self,
        title: &'static str,
        parent: Option<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| TextInput::dialog("Folder name", cx));
        let subscriptions = vec![
            cx.subscribe(&name, |this, _, _: &Submitted, cx| this.submit_create(cx)),
            // The Create button follows what is typed.
            cx.observe(&name, |_, _, cx| cx.notify()),
        ];
        let handle = name.read(cx).focus_handle(cx);
        self.create = Some(Create {
            title,
            parent,
            name,
            presence: Presence::new(),
            focus: cx.focus_handle(),
            busy: false,
            _subscriptions: subscriptions,
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    /// `createNamedFolder`: creates it, then adds the items to it.
    fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(create) = self.create.as_mut().filter(|create| !create.busy) else {
            return;
        };
        let name = create.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        create.busy = true;
        let parent = create.parent;
        let answer = self
            .session
            .read(cx)
            .run(move |library| library.create_folder(&name, parent));
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else { return };
            let _ = this.update(cx, |this, cx| match result {
                Ok((folder, _)) => {
                    this.create = None;
                    if let Some(parent) = parent {
                        this.session
                            .update(cx, |session, _| session.record_folder_use(parent));
                    }
                    this.toggle(folder, cx);
                    this.refocus = true;
                }
                Err(error) => {
                    // TODO: show this as the notification production shows.
                    eprintln!("folder picker: {error}");
                    if let Some(create) = &mut this.create {
                        create.busy = false;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn dismiss_create(&mut self, cx: &mut Context<Self>) {
        if let Some(create) = &mut self.create {
            create.presence.close();
            cx.notify();
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────

/// `FolderTree`'s guide lines: a line down through the rows of each
/// ancestor that continues, and this row's branch, which ends in a curve on
/// the last child.
fn guides(depth: usize, continues: Vec<bool>, last: bool, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let unit = bounds.size.height / 26.;
            let top = bounds.top();
            let line = |window: &mut Window, x: Pixels, from: Pixels, to: Pixels| {
                window.paint_quad(fill(
                    Bounds::from_corners(point(x, from), point(x + px(1.), to)),
                    color,
                ));
            };
            if depth > 1 {
                for (index, continues) in continues.iter().enumerate() {
                    if *continues {
                        let x = bounds.left() + px(14. + index as f32 * 24.);
                        line(window, x, top, bounds.bottom());
                    }
                }
            }
            if depth == 0 {
                return;
            }
            let x = bounds.left() + px(14. + (depth as f32 - 1.) * 24.);
            let arm_y = (top + unit * 13.).floor();
            if last {
                // `M0 0 V9.5 A3.5 3.5 0 0 0 3.5 13 H13`
                let mut path = PathBuilder::stroke(px(1.));
                path.move_to(point(x + px(0.5), top));
                path.line_to(point(x + px(0.5), top + unit * 9.5));
                path.arc_to(
                    point(px(3.5), unit * 3.5),
                    px(0.),
                    false,
                    false,
                    point(x + px(4.), top + unit * 13.),
                );
                path.line_to(point(x + px(13.5), top + unit * 13.));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            } else {
                // `M0 0 V26 M0 13 H13`
                line(window, x, top, bounds.bottom());
                window.paint_quad(fill(
                    Bounds::from_corners(point(x, arm_y), point(x + px(13.), arm_y + px(1.))),
                    color,
                ));
            }
        },
    )
    .absolute()
    .inset_0()
}

/// The first case-insensitive match of `query` in `text`.
fn highlight(text: &str, query: &str) -> Option<std::ops::Range<usize>> {
    let query = query.trim();
    let lower = text.to_lowercase();
    if query.is_empty() || lower.len() != text.len() {
        return None;
    }
    let start = lower.find(&query.to_lowercase())?;
    Some(start..start + query.len())
}

impl FolderPicker {
    fn row(&self, index: usize, row: &Row, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let folder = row.folder.clone();
        let selected = self.selected.contains(&folder.id);
        let excluded = self
            .filter
            .as_ref()
            .is_some_and(|filter| filter.excluded.contains(&folder.id));
        let focused = self.focus_index == Some(index);
        let indent = 4. + row.depth as f32 * 24.;
        let group = SharedString::from(format!("folder-row-{index}"));
        // `.row::before`: the highlight, kept clear of the tree's indent.
        let (fill, outline) = if selected {
            (theme.surface_active(), Some(theme.contrast(25.)))
        } else if focused {
            (theme.surface_hover(), Some(theme.border_focus()))
        } else {
            (gpui::transparent_black().into(), None)
        };
        let mut highlight_box = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .left(px(indent))
            .rounded(px(theme::RADIUS_MD))
            .bg(fill);
        if let Some(outline) = outline {
            highlight_box = highlight_box.border_1().border_color(outline);
        } else {
            highlight_box = highlight_box
                .group_hover(group.clone(), move |style| style.bg(theme.surface_hover()));
        }
        let name = StyledText::new(folder.name.clone()).with_highlights(
            highlight(&folder.name, &self.query).map(|range| {
                (
                    range,
                    HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        color: Some(theme.text().into()),
                        underline: Some(UnderlineStyle {
                            color: Some(theme.contrast(20.).into()),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        ..Default::default()
                    },
                )
            }),
        );
        let id = folder.id;
        let mut right = div()
            .flex_1()
            .min_w(px(26.))
            .max_w(relative(0.4))
            .ml_auto()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.));
        if let Some(parent) = &row.parent_name {
            right = right.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .group_hover(group.clone(), move |style| style.text_color(theme.text()))
                    .child(parent.clone()),
            );
        }
        right = if row.children {
            let expanded = row.expanded;
            right.child(
                div()
                    .id(("folder-expand", index))
                    .group(SharedString::from(format!("folder-expand-{index}")))
                    .size(px(26.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_active()))
                    .child(
                        svg()
                            .path("picker-chevron")
                            .size(px(12.))
                            .text_color(theme.text_secondary())
                            .group_hover(format!("folder-expand-{index}"), move |style| {
                                style.text_color(theme.text())
                            })
                            .with_transformation(Transformation::rotate(radians(if expanded {
                                std::f32::consts::FRAC_PI_2
                            } else {
                                0.
                            }))),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.folded.remove(&id) {
                            this.folded.insert(id);
                        }
                        cx.notify();
                    })),
            )
        } else {
            right.child(div().size(px(26.)).flex_none())
        };
        let menu_folder = folder.clone();
        div()
            .id(("folder-row", index))
            .group(group)
            .relative()
            .h(px(ROW))
            .flex_none()
            .pl(px(indent))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(ROW))
            .text_color(theme.text())
            .whitespace_nowrap()
            .child(guides(
                row.depth,
                row.continues.clone(),
                row.last,
                theme.tree_guide(),
            ))
            .child(highlight_box)
            .child(
                div()
                    .relative()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(overlay_shell::check_box(
                        match (excluded, selected, self.filter.is_some()) {
                            (true, _, _) => Mark::Excluded,
                            (false, true, true) => Mark::Included,
                            (false, true, false) => Mark::Selected,
                            (false, false, _) => Mark::Empty,
                        },
                        cx,
                    )),
            )
            .child(
                ui::icon("picker-folder", 20., folder.color.map_or(theme.text(), rgb))
                    .relative()
                    .flex_none(),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    // `.rowExcluded .name`
                    .when(excluded, |name| name.opacity(0.7))
                    .child(name),
            )
            .child(right.relative())
            // `.rowExcluded`
            .when(excluded, |row| row.opacity(0.58))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.focus_index = Some(index);
                this.click(id, event.modifiers(), cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if this.filter.is_some() {
                        this.exclude(menu_folder.id, cx);
                    } else {
                        this.open_menu(menu_folder.clone(), event.position, window, cx);
                    }
                }),
            )
    }

    fn create_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let create = self.create.as_ref()?;
        let typed = !create.name.read(cx).value().trim().is_empty();
        let busy = create.busy;
        Some(
            ui::modal("folder-picker-create", create.title, create.presence)
                .size(Size::Small)
                .focus(&create.focus)
                .layer(11)
                .on_close({
                    let this = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| this.dismiss_create(cx));
                    }
                })
                .child(ui::field("folder-picker-name", &create.name, Look::Modal))
                .footer(
                    ui::action_button("folder-picker-cancel", ButtonLook::Standard)
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.create = None;
                            this.refocus = true;
                            cx.notify();
                        })),
                )
                .footer(
                    ui::action_button("folder-picker-submit", ButtonLook::Primary)
                        .label("Create")
                        .disabled(!typed || busy)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_create(cx))),
                )
                .into_any_element(),
        )
    }
}

impl Render for FolderPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.read(cx).finished())
        {
            self.menu = None;
            self._menu = None;
            if self.create.is_none() {
                self.refocus = true;
            }
        }
        if self
            .create
            .as_ref()
            .is_some_and(|create| create.presence.finished())
        {
            self.create = None;
            self.refocus = true;
        }
        if !self.is_open() {
            return div().into_any_element();
        }
        // The keyboard goes back to the search once a menu or dialog is gone.
        if std::mem::take(&mut self.refocus) {
            let handle = self.search.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        let theme = theme::theme(cx);
        let entity = cx.entity().downgrade();
        let (moved, nav, close, chosen) = (
            entity.clone(),
            entity.clone(),
            entity.clone(),
            entity.clone(),
        );
        let search_handle = self.search.read(cx).focus_handle(cx);
        let search = div()
            .flex_1()
            .min_w_0()
            .h(px(36.))
            .pl(px(3.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.focus(&search_handle, cx);
            })
            .child(ui::icon("picker-search", 14., theme.text_secondary()).flex_none())
            .child(div().flex_1().min_w_0().h_full().child(self.search.clone()));
        let tabs = overlay_shell::view_tabs(
            "folder-picker-view",
            &[
                ("picker-folders", "All folders"),
                ("picker-history", "Recent folders"),
                ("picker-checks", "Selected folders"),
            ],
            match self.view {
                View::All => 0,
                View::Recent => 1,
                View::Selected => 2,
            },
            move |index, _, cx| {
                let _ = chosen.update(cx, |this, cx| {
                    this.view = [View::All, View::Recent, View::Selected][index];
                    this.focus_index = None;
                    cx.notify();
                });
            },
            window,
            cx,
        );

        let rows = self.visible(cx);
        let body = if rows.is_empty() {
            div()
                .h(px(60.))
                .flex()
                .items_center()
                .justify_center()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child("No folders")
                .into_any_element()
        } else {
            let mut tree = div().w_full().p(px(4.)).flex().flex_col();
            for (index, row) in rows.iter().enumerate() {
                tree = tree.child(self.row(index, row, cx));
            }
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .id("folder-picker-tree")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .pr(px(ui::scrollbar::GUTTER))
                        .child(tree),
                )
                .child(ui::scrollbar(
                    "folder-picker-scrollbar",
                    &self.scroll,
                    window,
                    cx,
                ))
                .into_any_element()
        };

        let hints = div().flex().items_center().gap(px(12.)).min_w_0();
        let hints = if self.filter.is_some() {
            hints
                .child(
                    overlay_shell::hint(cx)
                        .child("Select")
                        .child(overlay_shell::kbd("L-Click", cx)),
                )
                .child(
                    overlay_shell::hint(cx)
                        .child("Exclude")
                        .child(overlay_shell::kbd("R-Click", cx)),
                )
        } else {
            hints
                .child(
                    overlay_shell::hint(cx)
                        .child("Switch ")
                        .child(overlay_shell::tab_key(cx)),
                )
                .child(
                    overlay_shell::hint(cx).child("Move ").child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(overlay_shell::kbd("↑", cx))
                            .child(overlay_shell::kbd("↓", cx)),
                    ),
                )
                .child(
                    overlay_shell::hint(cx)
                        .child("Select ")
                        .child(overlay_shell::kbd("↵", cx)),
                )
        };
        // The filter mode's match rule follows the view tabs.
        let logic = self.filter.as_ref().map(|filter| {
            let modes = [SetMatchMode::Any, SetMatchMode::All, SetMatchMode::Exact];
            let logic = cx.entity().downgrade();
            ui::segmented::segmented(
                "folder-picker-logic",
                ui::segmented::Look::Logic,
                vec![
                    ui::segmented::segment("logic-any", "Match any"),
                    ui::segmented::segment("logic-all", "Match all"),
                    ui::segmented::segment("logic-exact", "Match exactly"),
                ],
                modes.iter().position(|mode| *mode == filter.mode),
            )
            .on_select(move |index, _, cx| {
                let _ = logic.update(cx, |this, cx| this.set_mode(modes[index], cx));
            })
        });
        let mut shell = overlay_shell::overlay_shell(
            "folder-picker",
            self.geometry,
            WIDTH,
            HEIGHT,
            &self.focus,
            move |window, cx| {
                let _ = close.update(cx, |this, cx| this.close(window, cx));
            },
        )
        .on_move(move |geometry, _, cx| {
            let _ = moved.update(cx, |this, cx| {
                this.geometry = geometry;
                cx.notify();
            });
        })
        .on_nav(move |key, _, cx| {
            nav.update(cx, |this, cx| this.navigate(key, cx))
                .unwrap_or(false)
        })
        .header(search)
        .header(tabs);
        if let Some(logic) = logic {
            shell = shell.header(logic);
        }
        let shell = shell.child(body).footer(hints).footer(
            div().ml_auto().flex_none().child(
                overlay_shell::hint(cx)
                    .child("Close ")
                    .child(overlay_shell::kbd("Esc", cx)),
            ),
        );
        let mut root = div().child(shell).children(self.create_dialog(cx));
        if let Some(menu) = self.menu.clone() {
            let view = window.viewport_size();
            root = root.child(
                deferred(
                    anchored()
                        .position(point(px(0.), px(0.)))
                        .child(div().w(view.width).h(view.height).child(menu)),
                )
                .with_priority(30),
            );
        }
        if self
            .create
            .as_ref()
            .is_some_and(|create| create.presence.animating())
        {
            window.request_animation_frame();
        }
        root.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Folder, View, listed, rows};
    use picto_library::FolderId;
    use std::collections::HashSet;

    fn folder(id: u32, name: &str, parent: Option<u32>) -> Folder {
        Folder {
            id: FolderId(id),
            name: name.into(),
            parent: parent.map(FolderId),
            color: None,
        }
    }

    fn tree() -> Vec<Folder> {
        vec![
            folder(1, "Art", None),
            folder(2, "Sketches", Some(1)),
            folder(3, "Paintings", Some(1)),
            folder(4, "Oils", Some(3)),
            folder(5, "Photos", None),
        ]
    }

    fn names(rows: &[super::Row]) -> Vec<&str> {
        rows.iter().map(|row| row.folder.name.as_str()).collect()
    }

    #[test]
    fn the_tree_lists_children_under_their_parents_with_their_guides() {
        let rows = rows(&tree(), &HashSet::new(), "");
        assert_eq!(
            names(&rows),
            ["Art", "Sketches", "Paintings", "Oils", "Photos"]
        );
        assert_eq!(rows[1].depth, 1);
        assert!(!rows[1].last && rows[2].last);
        // Oils sits under Paintings, the last child: no line continues.
        assert_eq!(rows[3].depth, 2);
        assert_eq!(rows[3].continues, [false]);
        assert_eq!(rows[1].parent_name.as_deref(), Some("Art"));
        assert!(rows[0].children && rows[0].expanded);
    }

    #[test]
    fn a_folded_branch_hides_its_folders() {
        let folded = HashSet::from([FolderId(1)]);
        let rows = rows(&tree(), &folded, "");
        assert_eq!(names(&rows), ["Art", "Photos"]);
        assert!(!rows[0].expanded);
    }

    #[test]
    fn a_search_keeps_matches_with_their_ancestors_and_opens_them() {
        let folded = HashSet::from([FolderId(1), FolderId(3)]);
        let rows = rows(&tree(), &folded, " OIL");
        assert_eq!(names(&rows), ["Art", "Paintings", "Oils"]);
        assert!(rows.iter().all(|row| row.expanded));
    }

    #[test]
    fn recent_and_selected_views_are_flat_lists_in_their_order() {
        let shown = listed(&tree(), &[FolderId(4), FolderId(9), FolderId(1)]);
        let rows = rows(&shown, &HashSet::new(), "");
        assert_eq!(names(&rows), ["Oils", "Art"]);
        assert!(
            rows.iter()
                .all(|row| row.depth == 0 && row.parent_name.is_none())
        );
    }

    #[test]
    fn tab_cycles_the_views() {
        assert_eq!(View::All.next(), View::Recent);
        assert_eq!(View::Selected.next(), View::All);
    }
}
