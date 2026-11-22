//! The sidebar. From top to bottom it shows the library switcher, the system
//! destinations, the Quick Access section, the Folders section and the Smart
//! Folders section. Below them is a field that filters the folders and
//! smart folders by name. Rows are selected with a click, a Cmd-click or a
//! Shift-click, and a right-click on a row opens its context menu. Folders
//! and smart folders are created, renamed in place and deleted here.
use super::library::{LibraryPopover, Toggled};
use crate::backend::Navigation;
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::t;
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
            Section::Folders => t("Folders"),
            Section::SmartFolders => t("Smart Folders"),
        }
    }
    /// Returns the id that stands for this section in the set of folded
    /// sections and nodes (`Sidebar::collapsed`).
    fn id(self) -> &'static str {
        match self {
            Section::Folders => "folders",
            Section::SmartFolders => "smart_folders",
        }
    }
    /// Returns the text that the id of every node in this section starts
    /// with.
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

/// Registers the sidebar's key bindings.
pub fn bind(cx: &mut App) {
    keyboard::bind(cx);
}

/// The event with which the sidebar asks the shell to show a context menu.
/// The sidebar itself carries out the row that the user chooses in the menu.
pub struct ShowMenu(pub Entity<Menu>);

impl EventEmitter<ShowMenu> for Sidebar {}

/// The event that the sidebar emits when the user chooses "Empty Trash". The
/// shell asks the user to confirm and then deletes the items.
pub struct EmptyTrash;

impl EventEmitter<EmptyTrash> for Sidebar {}

/// What a double-click on the row of a folder or smart folder does. The
/// setting `sidebar_double_click_action` chooses it.
#[derive(Clone, Copy, PartialEq)]
enum DoubleClick {
    Rename,
    Collapse,
}

/// The app settings that the sidebar draws by: which system rows and
/// sections are shown, whether counts and tree guides are shown, and what a
/// double-click does. The settings model owns these values. The sidebar
/// reads them each time it draws and changes them through the model.
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

/// The system rows that the visibility menu lists. Each entry holds the
/// row's id, its label and the name of its glyph. The rows All and Trash are
/// always shown (`ALWAYS_SHOWN`).
// i18n: translated where drawn (menu rows)
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
/// The id that stands for the Quick Access section in the set of folded
/// sections and nodes.
const QUICK_ACCESS: &str = "quick_access";

/// The thing on which a context menu was opened.
#[derive(Clone)]
enum Target {
    System,
    Node(String),
    /// The menu was opened on several selected folders or smart folders.
    Bulk(Vec<String>),
    Section,
    /// The menu was opened on a subfolder tile in the grid.
    Tile(String),
    /// The menu was opened on several subfolder tiles that are selected in
    /// the grid.
    Tiles(Vec<String>),
}

/// Holds the row that is being renamed in place: the node's id, the name it
/// had when the rename started, and the text field.
struct Renaming {
    node: String,
    original: String,
    input: Entity<TextInput>,
    _subscription: Subscription,
}

/// How a click changes the selection. A plain click replaces the selection
/// with the clicked row, a Cmd-click adds the clicked row or removes it, and
/// a Shift-click adds the given rows.
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

/// Returns the rows from `from` to `to`, both included, out of `rows`, which
/// are the rows a section shows. The two may be given in either order.
/// Returns `None` when one of them is not in `rows`.
fn range_between<'a>(rows: &'a [String], from: &str, to: &str) -> Option<&'a [String]> {
    let from = rows.iter().position(|row| row == from)?;
    let to = rows.iter().position(|row| row == to)?;
    Some(&rows[from.min(to)..=from.max(to)])
}

/// Returns the ids of the nodes of `section` that have children.
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
    /// The focus handle that receives keyboard focus when a rename ends.
    home: FocusHandle,
    /// The ids of the folded nodes and sections. The set is kept across
    /// launches in the app's state (`state.folded`).
    collapsed: HashSet<String>,
    /// The ids of the selected rows of folders and smart folders.
    selection: HashSet<String>,
    /// The row that was clicked last. A Shift-click selects the rows from
    /// this row to the clicked row.
    last_clicked: Option<String>,
    /// The ids of the rows that each section showed when it was last drawn.
    /// A Shift-click takes its range of rows from them.
    rendered: [Vec<String>; 2],
    /// The row whose context menu is open.
    context: Option<String>,
    renaming: Option<Renaming>,
    /// The grid that holds the subfolder tile whose context menu is open.
    /// The tile is renamed in place in that grid.
    tile_grid: Option<WeakEntity<crate::grid::GridView>>,
    /// The row that is drawn as focused while the rows have keyboard focus.
    keyboard_row: Option<String>,
    /// The focus handle of the list of rows.
    rows_focus: FocusHandle,
    /// The ids of the rows as they were last drawn, from top to bottom. The
    /// keyboard moves through the rows in this order.
    shown_rows: Vec<String>,
    _rows_focus: Option<Subscription>,
    /// The row that a dragged folder or smart folder is over, and whether
    /// the dragged node would be dropped before, inside or after that row.
    drop: Option<(String, DropPosition)>,
    /// The row that the items dragged from the grid are over.
    grid_drop: Option<String>,
    /// Whether items are being dragged from the grid.
    grid_dragging: bool,
    /// The bounds at which the rows that accept drops were last drawn.
    bounds: drag::RowBounds,
    /// The row of a folder or smart folder on which the left mouse button
    /// was pressed, and the position of the press.
    press: Option<(String, Point<Pixels>)>,
    /// The row that is being dragged.
    node_drag: Option<drag::NodeDrag>,
    /// Whether the next click is the release of the mouse button that ended
    /// a drag. That click opens nothing.
    swallow_click: bool,
    _drags: Option<Subscription>,
    /// The open dialog.
    dialog: ui::modal::Host<dialogs::Dialog>,
    /// The folder picker that "Move to..." opens. `moving_folders` holds the
    /// folders it moves. `menu_position` is where the last context menu
    /// opened, and the picker opens at that position.
    folder_picker: Entity<crate::inspector::FolderPicker>,
    moving_folders: Vec<FolderId>,
    menu_position: Point<Pixels>,
    /// The tag picker that the add button of the auto-tags dialog opens.
    auto_tag_picker: Entity<crate::inspector::TagPicker>,
    /// The smart-folder editor and the subscription to its events, while the
    /// editor is open.
    smart_editor: Option<(Entity<smart_folder::SmartFolderEditor>, Subscription)>,
    /// The open context menu. When a row of the visibility menu is chosen,
    /// the sidebar updates that row's check mark in this menu.
    menu: Option<WeakEntity<Menu>>,
    _menu: Vec<Subscription>,
    scroll: ScrollHandle,
    /// The library switcher's popover. `switcher` holds the bounds of the
    /// switcher, and the popover opens under them.
    library_popover: Entity<LibraryPopover>,
    switcher: Rc<Cell<Bounds<Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

/// Holds the state of the library switcher's highlight.
#[derive(Default)]
struct Switcher {
    hovered: bool,
    /// Fades the highlight in while the pointer is over the switcher or the
    /// library popover is open.
    lit: motion::Fade,
    /// Changes the highlight's colour from `surface_hover` to
    /// `surface_active` while the library popover is open.
    open: motion::Fade,
}

/// The distance in pixels that the list has to be scrolled before the
/// "Return to Top" button appears.
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
            // The switcher is drawn differently while its popover is open,
            // so the sidebar is redrawn when the popover opens or closes.
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
    /// Expands the section that holds `node` and every branch above the
    /// node, so that the node's row is shown.
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

    /// Saves the set of folded sections and nodes in the app's state.
    fn keep_folds(&self, cx: &mut App) {
        let mut folded = self.collapsed.iter().cloned().collect::<Vec<_>>();
        folded.sort();
        crate::state::update(cx, |state| state.folded = folded);
    }

    // The functions below act on the rows of folders and smart folders.

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

    /// Expands the nodes `ids` when `expanded` is true, and folds them
    /// otherwise.
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

    /// Handles a click on the row of a folder or smart folder. A Cmd-click
    /// or Ctrl-click adds the row to the selection or removes it. A
    /// Shift-click adds the rows from the row clicked last to this row, when
    /// both are in the same section. A plain click selects this row alone
    /// and opens it.
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

    /// Reads the grid's items again when the grid shows `node`. When `node`
    /// is `None`, it reads them again whatever the grid shows.
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
    fn selection_follows_the_earlier_versions_sidebar() {
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
        // A range can be given from the lower row to the upper row, and the
        // rows that were selected before stay selected.
        let range = range_between(&rows, "folder:6", "folder:4").unwrap();
        assert_eq!(range, &rows[1..4]);
        assert_eq!(
            next_selection(&set(&["folder:9"]), "folder:4", Pick::Range(range)),
            set(&["folder:9", "folder:4", "folder:5", "folder:6"])
        );
        // There is no range when one of the two rows is not among the rows
        // that the section shows.
        assert!(range_between(&rows, "folder:7", "folder:4").is_none());
    }

    #[test]
    fn node_ids_parse_as_the_earlier_version_wrote_them() {
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
