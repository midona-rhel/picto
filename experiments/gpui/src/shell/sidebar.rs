//! The sidebar (`Sidebar.tsx`): the library, the system destinations in the
//! Electron order, then Folders and Smart Folders, over a tree filter. Rows
//! take Cmd and Shift selection, open their context menus, and folders and
//! smart folders are created, renamed in place and deleted here.
use super::library::{LibraryPopover, Toggled};
use crate::backend::Navigation;
use crate::grid::thumbnails::Thumbnails;
use crate::motion;
use crate::session::{Change, Session};
use crate::ui;
use crate::ui::menu::Menu;
use crate::ui::sidebar_row::DropPosition;
use crate::ui::text_input::{Kind, TextInput};
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::{FolderId, SmartFolderId};
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

mod actions;
mod dialogs;
mod drag;
mod edits;
mod keyboard;
mod menus;
mod rules;
mod smart_folder;
mod tree;
mod view;

pub use drag::drop_into_folder;
use edits::smart_input;
pub(crate) use edits::{rename_node, set_node_notes};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Section {
    Folders,
    SmartFolders,
}

const SECTIONS: [Section; 2] = [Section::Folders, Section::SmartFolders];

impl Section {
    fn label(self) -> &'static str {
        match self {
            Section::Folders => "Folders",
            Section::SmartFolders => "Smart Folders",
        }
    }
    /// The id the section is folded under, as `picto-sidebar-collapsed`
    /// stores it.
    fn id(self) -> &'static str {
        match self {
            Section::Folders => "folders",
            Section::SmartFolders => "smart_folders",
        }
    }
    /// The start of its nodes' ids.
    fn prefix(self) -> &'static str {
        match self {
            Section::Folders => "folder:",
            Section::SmartFolders => "smart:",
        }
    }
    fn of_node(id: &str) -> Option<Self> {
        SECTIONS
            .into_iter()
            .find(|section| id.starts_with(section.prefix()))
    }
}

fn section_of(scope: &ItemScope, screen: bool) -> Option<Section> {
    match scope {
        _ if screen => None,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. } => Some(Section::Folders),
        ItemScope::SmartFolder { .. } => Some(Section::SmartFolders),
        _ => None,
    }
}

fn folder_id(node: &str) -> Option<FolderId> {
    node.strip_prefix("folder:")?.parse().ok().map(FolderId)
}

fn smart_folder_id(node: &str) -> Option<SmartFolderId> {
    node.strip_prefix("smart:")?.parse().ok().map(SmartFolderId)
}

/// The sidebar's key bindings.
pub fn bind(cx: &mut App) {
    keyboard::bind(cx);
}

/// Asks the shell to show a context menu. The sidebar acts on what is chosen.
pub struct ShowMenu(pub Entity<Menu>);

impl EventEmitter<ShowMenu> for Sidebar {}

/// Empty Trash was chosen; the shell confirms and deletes.
pub struct EmptyTrash;

impl EventEmitter<EmptyTrash> for Sidebar {}

/// What a double-click on a folder does (`sidebarDoubleClickAction`).
#[derive(Clone, Copy, PartialEq)]
enum DoubleClick {
    Rename,
    Collapse,
}

/// `SidebarPreferences` in `state/navigation.ts`, as `AppShell.tsx` derives
/// them from the app settings. The settings model is their one owner; the
/// sidebar reads them each time it draws and writes through the model.
struct Preferences {
    visible_system: HashSet<&'static str>,
    show_quick_access: bool,
    show_folders: bool,
    show_smart_folders: bool,
    show_counts: bool,
    show_guides: bool,
    double_click: DoubleClick,
}

impl Preferences {
    fn current(cx: &App) -> Self {
        let settings = crate::settings::current(cx);
        Self {
            visible_system: SYSTEM_ROWS
                .iter()
                .map(|row| row.0)
                .filter(|id| settings.sidebar_shows(id))
                .collect(),
            show_quick_access: settings.show_sidebar_quick_access,
            show_folders: settings.show_sidebar_folders,
            show_smart_folders: settings.show_sidebar_smart_folders,
            show_counts: settings.show_sidebar_counts,
            show_guides: settings.show_tree_guides,
            double_click: if settings.sidebar_double_click_action == "rename" {
                DoubleClick::Rename
            } else {
                DoubleClick::Collapse
            },
        }
    }

    fn shows(&self, section: Section) -> bool {
        match section {
            Section::Folders => self.show_folders,
            Section::SmartFolders => self.show_smart_folders,
        }
    }
}

/// The system rows the visibility menu lists: id, label and glyph. All and
/// Trash are always shown.
const SYSTEM_ROWS: [(&str, &str, &str); 10] = [
    ("system:active", "All", "menu-photo"),
    ("system:inbox", "Inbox", "menu-inbox"),
    ("system:recent_viewed", "Recently Viewed", "menu-clock"),
    (
        "system:uncategorized",
        "Uncategorized",
        "menu-folder-question",
    ),
    ("system:untagged", "Untagged", "menu-bookmark-question"),
    ("system:tag_manager", "Tag Manager", "menu-bookmark"),
    ("system:random", "Random", "menu-arrows-shuffle"),
    ("system:subscriptions", "Subscriptions", "menu-download"),
    ("system:duplicates", "Duplicates", "menu-copy"),
    ("system:trash", "Trash", "menu-trash"),
];
const ALWAYS_SHOWN: [&str; 2] = ["system:active", "system:trash"];
/// The id Quick Access is folded under.
const QUICK_ACCESS: &str = "quick_access";

/// What a context menu was opened on.
#[derive(Clone)]
enum Target {
    System,
    Node(String),
    /// Several selected folders or smart folders.
    Bulk(Vec<String>),
    Section,
    /// A subfolder tile in the grid.
    Tile(String),
    /// Several subfolder tiles selected in the grid.
    Tiles(Vec<String>),
}

/// A row being renamed in place.
struct Renaming {
    node: String,
    original: String,
    input: Entity<TextInput>,
    _subscription: Subscription,
}

/// `nextSidebarSelection`: a click replaces the selection, Cmd-click toggles
/// one row, and Shift-click adds the rows between.
#[derive(Clone, Copy)]
enum Pick<'a> {
    Replace,
    Toggle,
    Range(&'a [String]),
}

fn next_selection(current: &HashSet<String>, clicked: &str, pick: Pick) -> HashSet<String> {
    match pick {
        Pick::Replace => [clicked.to_owned()].into(),
        Pick::Toggle => {
            let mut next = current.clone();
            if !next.remove(clicked) {
                next.insert(clicked.to_owned());
            }
            next
        }
        Pick::Range(rows) => current.iter().chain(rows).cloned().collect(),
    }
}

/// The rows from `from` to `to` inclusive among the rows a section shows,
/// when both are there.
fn range_between<'a>(rows: &'a [String], from: &str, to: &str) -> Option<&'a [String]> {
    let from = rows.iter().position(|row| row == from)?;
    let to = rows.iter().position(|row| row == to)?;
    Some(&rows[from.min(to)..=from.max(to)])
}

/// The ids of a section's nodes that have children (`expandableNodeIds`).
fn expandable(entries: &[Navigation], ids: &[String], section: Section) -> Vec<String> {
    let mut parents = vec![false; entries.len()];
    for entry in entries {
        if let Some(parent) = entry.parent {
            parents[parent] = true;
        }
    }
    (0..entries.len())
        .filter(|index| parents[*index] && ids[*index].starts_with(section.prefix()))
        .map(|index| ids[index].clone())
        .collect()
}

pub struct Sidebar {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    filter: Entity<TextInput>,
    /// Where the keyboard goes back to when a rename ends.
    home: FocusHandle,
    /// Folded nodes and sections, by id, in the shape of the Electron
    /// sidebar's `picto-sidebar-collapsed` set, kept across launches in the
    /// app's state as `usePersistedSet` keeps it.
    collapsed: HashSet<String>,
    /// Rows chosen with Cmd or Shift, by id.
    selection: HashSet<String>,
    /// Where a Shift-click range starts.
    last_clicked: Option<String>,
    /// The ids each section showed when last drawn, for Shift ranges.
    rendered: [Vec<String>; 2],
    /// The row whose context menu is open.
    context: Option<String>,
    renaming: Option<Renaming>,
    /// The grid whose subfolder tile's menu is open: it renames the tile in
    /// place.
    tile_grid: Option<WeakEntity<crate::grid::GridView>>,
    /// The row the keyboard is on while the rows have it.
    keyboard_row: Option<String>,
    /// The rows' keyboard focus.
    rows_focus: FocusHandle,
    /// The rows as last drawn, in order, for the keyboard.
    shown_rows: Vec<String>,
    _rows_focus: Option<Subscription>,
    /// Where a dragged folder or smart folder would land.
    drop: Option<(String, DropPosition)>,
    /// The row items dragged from the grid are over.
    grid_drop: Option<String>,
    /// Items are being dragged from the grid.
    grid_dragging: bool,
    /// Where the rows that take drops were last drawn.
    bounds: drag::RowBounds,
    /// A left press on a folder or smart-folder row, and where.
    press: Option<(String, Point<Pixels>)>,
    /// The row being dragged.
    node_drag: Option<drag::NodeDrag>,
    /// The click that ends a drag opens nothing.
    swallow_click: bool,
    _drags: Option<Subscription>,
    /// The open dialog.
    dialog: ui::modal::Host<dialogs::Dialog>,
    /// The folder picker "Move to..." opens, the folders it moves, and
    /// where the last context menu opened, which it opens at.
    folder_picker: Entity<crate::inspector::FolderPicker>,
    moving_folders: Vec<FolderId>,
    menu_position: Point<Pixels>,
    /// The tag picker the auto-tags dialog's add button opens.
    auto_tag_picker: Entity<crate::inspector::TagPicker>,
    /// The smart-folder editor, while open.
    smart_editor: Option<(Entity<smart_folder::SmartFolderEditor>, Subscription)>,
    /// The open context menu, whose checks follow what its rows toggle.
    menu: Option<WeakEntity<Menu>>,
    _menu: Vec<Subscription>,
    scroll: ScrollHandle,
    /// The library switcher's popover, and the switcher's bounds it opens
    /// under.
    library_popover: Entity<LibraryPopover>,
    switcher: Rc<Cell<Bounds<Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

/// The library switcher's highlight and the pointer over it.
#[derive(Default)]
struct Switcher {
    hovered: bool,
    /// The highlight shows while hovered or open.
    lit: motion::Fade,
    /// Open: the highlight takes the active surface.
    open: motion::Fade,
}

/// The list scrolls this far before "Return to Top" appears.
const RETURN_TO_TOP_AFTER: f32 = 360.;

impl Sidebar {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        home: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| TextInput::new(Kind::Sidebar, cx).placeholder("Filter"));
        let switcher = Rc::new(Cell::new(Bounds::default()));
        let library_popover = cx.new(|cx| {
            LibraryPopover::new(session.clone(), thumbnails.clone(), switcher.clone(), cx)
        });
        let folder_picker = cx.new(|cx| crate::inspector::FolderPicker::new(session.clone(), cx));
        let auto_tag_picker = cx.new(|cx| crate::inspector::TagPicker::new(session.clone(), cx));
        let subscriptions = vec![
            cx.subscribe(&auto_tag_picker, |this, _, event, cx| {
                this.auto_tag_picked(event, cx)
            }),
            cx.observe(&filter, |_, _, cx| cx.notify()),
            // The switcher shows whether its popover is open.
            cx.subscribe(&library_popover, |_, _, _: &Toggled, cx| cx.notify()),
            cx.subscribe(&session, |_, _, change: &Change, cx| {
                if matches!(change, Change::Navigation | Change::Activity) {
                    cx.notify();
                }
            }),
            cx.subscribe(
                &crate::settings::model(cx),
                |_, _, _: &crate::settings::Applied, cx| cx.notify(),
            ),
        ];
        Self {
            session,
            thumbnails,
            filter,
            home,
            collapsed: crate::state::get(cx).folded.iter().cloned().collect(),
            selection: HashSet::new(),
            last_clicked: None,
            rendered: Default::default(),
            context: None,
            renaming: None,
            tile_grid: None,
            keyboard_row: None,
            rows_focus: cx.focus_handle(),
            shown_rows: Vec::new(),
            _rows_focus: None,
            drop: None,
            grid_drop: None,
            grid_dragging: false,
            bounds: drag::RowBounds::default(),
            press: None,
            node_drag: None,
            swallow_click: false,
            _drags: None,
            dialog: ui::modal::Host::default(),
            smart_editor: None,
            folder_picker,
            moving_folders: Vec::new(),
            menu_position: Point::default(),
            auto_tag_picker,
            menu: None,
            _menu: Vec::new(),
            scroll: ScrollHandle::new(),
            library_popover,
            switcher,
            _subscriptions: subscriptions,
        }
    }
}

impl Sidebar {
    /// Opens a node's section and the branches above it
    /// (`pendingSidebarRevealNodeId`).
    fn reveal(&mut self, node: &str, cx: &mut Context<Self>) {
        if let Some(section) = Section::of_node(node) {
            self.collapsed.remove(section.id());
        }
        let ancestors = {
            let nav = &self.session.read(cx).nav;
            let mut ancestors = Vec::new();
            let mut at = nav
                .iter()
                .position(|entry| entry.id() == node)
                .and_then(|index| nav[index].parent);
            while let Some(index) = at {
                ancestors.push(nav[index].id());
                at = nav[index].parent;
            }
            ancestors
        };
        for ancestor in ancestors {
            self.collapsed.remove(&ancestor);
        }
        self.keep_folds(cx);
    }

    /// Keeps the folded sections and nodes in the app's state.
    fn keep_folds(&self, cx: &mut App) {
        let mut folded = self.collapsed.iter().cloned().collect::<Vec<_>>();
        folded.sort();
        crate::state::update(cx, |state| state.folded = folded);
    }

    // ── Rows ────────────────────────────────────────────────────────

    fn label_of(&self, node: &str, cx: &App) -> Option<String> {
        self.session
            .read(cx)
            .nav
            .iter()
            .find(|entry| entry.id() == node)
            .map(|entry| entry.label.clone())
    }

    fn toggle_collapsed(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        self.keep_folds(cx);
        cx.notify();
    }

    /// `setNodesExpanded`.
    fn set_expanded(&mut self, ids: &[String], expanded: bool, cx: &mut Context<Self>) {
        for id in ids {
            if expanded {
                self.collapsed.remove(id);
            } else {
                self.collapsed.insert(id.clone());
            }
        }
        self.keep_folds(cx);
        cx.notify();
    }

    /// `handleRowClick` for folders and smart folders: Cmd toggles the row,
    /// Shift adds the rows from the last one clicked in the same section,
    /// and a plain click selects the row alone and opens it.
    fn click_node(
        &mut self,
        index: usize,
        node: String,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if modifiers.platform || modifiers.control {
            self.selection = next_selection(&self.selection, &node, Pick::Toggle);
            self.last_clicked = Some(node);
            cx.notify();
            return;
        }
        if modifiers.shift
            && let Some(last) = &self.last_clicked
            && let Some(section) = Section::of_node(&node)
            && Section::of_node(last) == Some(section)
            && let Some(rows) = range_between(&self.rendered[section as usize], last, &node)
        {
            self.selection = next_selection(&self.selection, &node, Pick::Range(rows));
            cx.notify();
            return;
        }
        self.selection = next_selection(&self.selection, &node, Pick::Replace);
        self.last_clicked = Some(node);
        self.session
            .update(cx, |session, cx| session.navigate(index, cx));
        cx.notify();
    }

    /// Fetches the grid again when it shows `node`, or any destination
    /// when `node` is `None`.
    fn reload_if_showing(&mut self, node: Option<&str>, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let showing = session.nav.get(session.scope).map(Navigation::id);
            if node.is_none() || showing.as_deref() == node {
                session.reload(cx);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Pick, Section, expandable, folder_id, next_selection, range_between, smart_folder_id,
    };
    use crate::backend::Navigation;
    use picto_library::query::ItemScope;
    use picto_library::{FolderId, SmartFolderId};
    use std::collections::HashSet;

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn selection_follows_the_electron_sidebar() {
        let current = set(&["folder:1", "folder:2"]);
        assert_eq!(
            next_selection(&current, "folder:3", Pick::Replace),
            set(&["folder:3"])
        );
        assert_eq!(
            next_selection(&current, "folder:2", Pick::Toggle),
            set(&["folder:1"])
        );
        assert_eq!(
            next_selection(&current, "folder:3", Pick::Toggle),
            set(&["folder:1", "folder:2", "folder:3"])
        );
        let rows = ["folder:1", "folder:4", "folder:5", "folder:6"].map(String::from);
        // A range runs either way and keeps what was selected before.
        let range = range_between(&rows, "folder:6", "folder:4").unwrap();
        assert_eq!(range, &rows[1..4]);
        assert_eq!(
            next_selection(&set(&["folder:9"]), "folder:4", Pick::Range(range)),
            set(&["folder:9", "folder:4", "folder:5", "folder:6"])
        );
        // A row the section no longer shows has no range.
        assert!(range_between(&rows, "folder:7", "folder:4").is_none());
    }

    #[test]
    fn node_ids_parse_as_production_writes_them() {
        assert_eq!(folder_id("folder:12"), Some(FolderId(12)));
        assert_eq!(folder_id("smart:12"), None);
        assert_eq!(smart_folder_id("smart:7"), Some(SmartFolderId(7)));
        assert_eq!(Section::of_node("smart:7"), Some(Section::SmartFolders));
        assert_eq!(Section::of_node("system:inbox"), None);
    }

    #[test]
    fn only_parents_in_the_section_expand() {
        let node = |scope, parent| Navigation {
            label: String::new(),
            count: None,
            scope,
            screen: None,
            random: false,
            icon: None,
            tint: None,
            group: false,
            watched: false,
            depth: 0,
            parent,
        };
        let folder = |id| ItemScope::FolderTree {
            folder_id: FolderId(id),
        };
        let smart = |id| ItemScope::SmartFolder {
            smart_folder_id: SmartFolderId(id),
        };
        let entries = vec![
            node(folder(1), None),
            node(folder(2), Some(0)),
            node(folder(3), None),
            node(smart(4), None),
            node(smart(5), Some(3)),
        ];
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        assert_eq!(ids[4], "smart:5");
        assert_eq!(expandable(&entries, &ids, Section::Folders), ["folder:1"]);
        assert_eq!(
            expandable(&entries, &ids, Section::SmartFolders),
            ["smart:4"]
        );
    }
}
