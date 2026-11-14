//! The folder picker (`FolderPickerPanel.tsx` in its `OverlayShell`, with
//! `FolderTree`): the folder tree with a search, the recent and selected
//! views, and New Subfolder and New Sibling Folder from a row's menu. The
//! inspector opens it with the items' folders checked, a click adding the
//! items to a folder or taking them out at once; the sidebar opens it in
//! its move mode (`onApplyFolderParent`), choosing one new parent, Library
//! for the top level, among the folders the moving ones can go under.
use super::membership;
use crate::backend::Navigation;
use crate::session::{Report, Reread, Session};
use crate::ui::{
    self,
    menu::{self, Menu},
    modal::{self},
    overlay_shell::{self, Geometry, Nav},
    text_input::{Submitted, TextInput},
};
use gpui::*;
use picto_library::FolderId;
use picto_library::predicate::SetMatchMode;
use picto_library::query::ItemScope;
use std::collections::{BTreeSet, HashSet};

mod view;

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
    /// Move mode's Move: the chosen parent, `None` for Library.
    MoveTo(Option<FolderId>),
    /// In the filter mode, the folders included and excluded and the match
    /// rule (`onApplyFolderFilter`).
    FilterChanged {
        include: Vec<FolderId>,
        exclude: Vec<FolderId>,
        mode: SetMatchMode,
    },
}

/// The move mode: the folders that can be chosen, and whether Library (the
/// top level) is.
struct ParentMode {
    available: HashSet<FolderId>,
    root: bool,
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

/// A folder as the sidebar lists it (`SidebarNodeDto`), for the folder
/// trees of this picker and of the Folders dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    pub parent: Option<FolderId>,
    pub color: Option<u32>,
    pub count: Option<u64>,
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
                count: entry.count,
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

/// `matchesSearch`: the folder's name or a descendant's holds the search.
fn matches(folders: &[Folder], folder: &Folder, query: &str) -> bool {
    folder.name.to_lowercase().contains(query)
        || folders
            .iter()
            .filter(|child| child.parent == Some(folder.id))
            .any(|child| matches(folders, child, query))
}

/// The visible rows of a folder tree (`FolderTree`, and `renderTreeNode`
/// over `buildTree` in the Folders dialog): each folder under its parent,
/// folded branches left out, and while searching only the folders whose
/// name holds the text and their ancestors. The picker opens every branch
/// while it searches by passing no folds.
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
            let expanded = !folded.contains(&folder.id);
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
    /// A row's menu and the dialog it opens; the keyboard goes back to the
    /// search once each has gone.
    menu: menu::Host,
    menu_folder: Option<Folder>,
    create: modal::Host<Create>,
    filter: Option<FilterMode>,
    parent_mode: Option<ParentMode>,
    _subscriptions: Vec<Subscription>,
}

impl FolderPicker {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(TextInput::menu_search);
        let menu = menu::Host::deferred(30, menu::Return::To(search.read(cx).focus_handle(cx)));
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
            menu,
            menu_folder: None,
            create: modal::Host::default(),
            filter: None,
            parent_mode: None,
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

    /// Opens with its bottom just above a button at `anchor`
    /// (`anchorPlacement: 'above'`), on the folders `folders` names.
    pub fn open_above(
        &mut self,
        anchor: Point<f32>,
        folders: Vec<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open(None, folders, window, cx);
        let view = window.viewport_size();
        self.geometry = overlay_shell::place_above(
            anchor,
            size(f32::from(view.width), f32::from(view.height)),
            WIDTH,
            HEIGHT,
        );
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
        self.parent_mode = None;
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

    /// The move mode: `available` are the folders the moving ones can go
    /// under, `current` their shared parent (Library when `None`).
    pub fn open_for_parent(
        &mut self,
        anchor: Option<Point<f32>>,
        available: Vec<FolderId>,
        current: Option<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open(anchor, current.into_iter().collect(), window, cx);
        self.parent_mode = Some(ParentMode {
            available: available.into_iter().collect(),
            root: current.is_none(),
        });
    }

    /// Library's row: in move mode, unless a search leaves it out.
    fn shows_root(&self) -> bool {
        self.parent_mode.is_some()
            && (self.query.is_empty() || "library".contains(&self.query.to_lowercase()))
    }

    /// The rows the keyboard reaches before the tree's.
    fn key_offset(&self) -> usize {
        usize::from(self.shows_root())
    }

    /// Move: the chosen parent goes to the owner, and the picker closes.
    fn apply_parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = &self.parent_mode else {
            return;
        };
        let parent = if mode.root {
            None
        } else {
            match self.selected.iter().next() {
                Some(folder) => Some(*folder),
                None => return,
            }
        };
        cx.emit(Event::MoveTo(parent));
        self.close(window, cx);
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
        self.menu.close(cx);
        self.create.close(cx);
        opening.finish(&self.focus, window, cx);
        cx.notify();
    }

    fn all_folders(&self, cx: &App) -> Vec<Folder> {
        let all = folders(&self.session.read(cx).nav);
        match &self.parent_mode {
            Some(mode) => all
                .into_iter()
                .filter(|folder| mode.available.contains(&folder.id))
                .collect(),
            None => all,
        }
    }

    fn visible(&self, cx: &App) -> Vec<Row> {
        let all = self.all_folders(cx);
        let open = HashSet::new();
        let folded = if self.query.trim().is_empty() {
            &self.folded
        } else {
            &open
        };
        match self.view {
            View::All => rows(&all, folded, &self.query),
            View::Recent => {
                let shown = self
                    .session
                    .read(cx)
                    .recent_folders
                    .iter()
                    .copied()
                    .take(RECENT_SHOWN)
                    .collect::<Vec<_>>();
                rows(&listed(&all, &shown), folded, &self.query)
            }
            View::Selected => {
                let ids = self.selected.iter().copied().collect::<Vec<_>>();
                rows(&listed(&all, &ids), folded, &self.query)
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
        // Move mode chooses one parent and moves nothing yet.
        if let Some(mode) = &mut self.parent_mode {
            mode.root = false;
            self.selected = [folder].into();
            cx.notify();
            return;
        }
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
        if self.create.is_open() || self.menu.is_open() {
            return false;
        }
        let rows = self.visible(cx);
        let offset = self.key_offset();
        let targets = rows.len() + offset;
        match key {
            Nav::Next => {
                self.view = self.view.next();
                self.focus_index = None;
            }
            Nav::Down => {
                if targets > 0 {
                    self.focus_index =
                        Some(self.focus_index.map_or(0, |at| (at + 1).min(targets - 1)));
                }
            }
            Nav::Up => {
                self.focus_index = Some(self.focus_index.map_or(0, |at| at.saturating_sub(1)));
            }
            Nav::Confirm => {
                let at = self.focus_index.unwrap_or(0);
                if offset == 1 && at == 0 {
                    // Library: the top level.
                    if let Some(mode) = &mut self.parent_mode {
                        mode.root = true;
                    }
                    self.selected.clear();
                } else if let Some(row) = rows.get(at - offset) {
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
        // Library's row sits above the scrolling tree.
        let Some(at) = at.checked_sub(self.key_offset()) else {
            return;
        };
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
        self.menu.open(menu, window, cx, |this, _, action, _, cx| {
            let Some(folder) = this.menu_folder.clone() else {
                return;
            };
            match action {
                "new-subfolder" => this.start_create("New Subfolder", Some(folder.id), cx),
                "new-sibling" => this.start_create("New Sibling Folder", folder.parent, cx),
                _ => {}
            }
        });
        self.menu_folder = Some(folder);
    }

    fn start_create(
        &mut self,
        title: &'static str,
        parent: Option<FolderId>,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| TextInput::dialog("Folder name", cx));
        let subscriptions = vec![
            cx.subscribe(&name, |this, _, _: &Submitted, cx| this.submit_create(cx)),
            // The Create button follows what is typed.
            cx.observe(&name, |_, _, cx| cx.notify()),
        ];
        let handle = name.read(cx).focus_handle(cx);
        let back = menu::Return::To(self.search.read(cx).focus_handle(cx));
        let create = Create {
            title,
            parent,
            name,
            busy: false,
            _subscriptions: subscriptions,
        };
        self.create.open(create, back, cx).focus_first(handle);
    }

    /// `createNamedFolder`: creates it, then adds the items to it.
    fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(create) = self
            .create
            .get_mut()
            .map(|dialog| &mut dialog.content)
            .filter(|create| !create.busy)
        else {
            return;
        };
        let name = create.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        create.busy = true;
        let parent = create.parent;
        // `foldersController.create` tells of a folder nested too deep;
        // anything else stays on the console.
        crate::session::edit_then(
            &self.session,
            Report::folder_depth(),
            Reread::NAVIGATION,
            move |library| library.create_folder(&name, parent),
            cx,
            move |this, result, cx| match result {
                Ok((folder, _)) => {
                    this.create.close(cx);
                    if let Some(parent) = parent {
                        this.session
                            .update(cx, |session, _| session.record_folder_use(parent));
                    }
                    this.toggle(folder, cx);
                }
                Err(_) => {
                    if let Some(dialog) = this.create.get_mut() {
                        dialog.content.busy = false;
                    }
                    cx.notify();
                }
            },
        );
    }

    fn dismiss_create(&mut self, cx: &mut Context<Self>) {
        self.create.dismiss(cx);
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
            count: None,
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
    fn a_search_keeps_matches_with_their_ancestors() {
        let rows = rows(&tree(), &HashSet::new(), " OIL");
        assert_eq!(names(&rows), ["Art", "Paintings", "Oils"]);
        assert!(rows.iter().all(|row| row.expanded));
        // A folded branch stays folded (the Folders dialog keeps its folds).
        let folded = HashSet::from([FolderId(3)]);
        assert_eq!(
            names(&super::rows(&tree(), &folded, "oil")),
            ["Art", "Paintings"]
        );
        assert!(super::rows(&tree(), &HashSet::new(), "zzz").is_empty());
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
