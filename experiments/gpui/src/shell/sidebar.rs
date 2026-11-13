//! The sidebar (`Sidebar.tsx`): the library, the system destinations in the
//! Electron order, then Folders and Smart Folders, over a tree filter. Rows
//! take Cmd and Shift selection, open their context menus, and folders and
//! smart folders are created, renamed in place and deleted here.
use crate::backend::{self, Navigation};
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::ui::menu::{self, Menu};
use crate::ui::sidebar_row::DropPosition;
use crate::ui::text_input::{Renamed, TextInput};
use crate::ui::{self, field::Look, sidebar_row::Branch};
use crate::{motion, theme};
use gpui::*;
use picto_library::query::{ItemScope, RootQuery};
use picto_library::{FolderId, Library, LibraryError, SmartFolderId, SmartFolderInput};
use std::collections::HashSet;

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

/// Asks the shell to show a context menu. The sidebar acts on what is chosen.
pub struct ShowMenu(pub Entity<Menu>);

impl EventEmitter<ShowMenu> for Sidebar {}

/// What a double-click on a folder does (`sidebarDoubleClickAction`).
#[derive(Clone, Copy, PartialEq)]
enum DoubleClick {
    // Chosen in Settings, which this prototype does not read yet.
    #[allow(dead_code)]
    Rename,
    Collapse,
}

/// `SidebarPreferences` in `state/navigation.ts`, with its defaults.
// TODO: read and write these through the app settings (`showSidebarInbox`
// and the rest, `sidebarDoubleClickAction`), as `settingsController` does.
struct Preferences {
    visible_system: HashSet<&'static str>,
    show_quick_access: bool,
    show_folders: bool,
    show_smart_folders: bool,
    double_click: DoubleClick,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            visible_system: SYSTEM_ROWS.iter().map(|row| row.0).collect(),
            show_quick_access: true,
            show_folders: true,
            show_smart_folders: true,
            double_click: DoubleClick::Collapse,
        }
    }
}

impl Preferences {
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

/// What a context menu was opened on.
#[derive(Clone)]
enum Target {
    System,
    Node(String),
    /// Several selected folders or smart folders.
    Bulk(Vec<String>),
    Section,
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
    /// sidebar's `picto-sidebar-collapsed` set.
    // TODO: persist across launches, as `usePersistedSet` does.
    collapsed: HashSet<String>,
    prefs: Preferences,
    /// Rows chosen with Cmd or Shift, by id.
    selection: HashSet<String>,
    /// Where a Shift-click range starts.
    last_clicked: Option<String>,
    /// The ids each section showed when last drawn, for Shift ranges.
    rendered: [Vec<String>; 2],
    /// The row whose context menu is open.
    context: Option<String>,
    renaming: Option<Renaming>,
    // TODO: move keyboard focus through the rows, as the Electron rows'
    // `tabIndex` does; the row is drawn focused while this names it.
    keyboard_row: Option<String>,
    // TODO: set while a folder is dragged over the sidebar (drag reorder
    // and drops from the grid).
    drop: Option<(String, DropPosition)>,
    drop_target: Option<String>,
    _menu: Vec<Subscription>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
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
        let filter = cx.new(|cx| TextInput::sidebar("Filter", cx));
        let subscriptions = vec![
            cx.observe(&filter, |_, _, cx| cx.notify()),
            cx.subscribe(&session, |_, _, change: &Change, cx| {
                if matches!(change, Change::Navigation) {
                    cx.notify();
                }
            }),
        ];
        Self {
            session,
            thumbnails,
            filter,
            home,
            collapsed: HashSet::new(),
            prefs: Preferences::default(),
            selection: HashSet::new(),
            last_clicked: None,
            rendered: Default::default(),
            context: None,
            renaming: None,
            keyboard_row: None,
            drop: None,
            drop_target: None,
            _menu: Vec::new(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The library switcher (`LibrarySwitcherButton`): the cover, the
    /// library's name and a selector glyph, over a highlight shaped as the
    /// cover's circle joined to a bar.
    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (cover, name) = {
            let session = self.session.read(cx);
            let name = session
                .library_name()
                .unwrap_or_else(|| "No library".into());
            (session.library_cover.clone(), name)
        };
        let fade = window.use_keyed_state("library-switcher", cx, |_, _| motion::Fade::default());
        let hint = window.use_keyed_state("library-switcher-tip", cx, |_, _| {
            ui::tooltip::Hover::default()
        });
        let hover = fade.read(cx).value();
        if fade.read(cx).animating() || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        // The cover stays decoded however far the grid scrolls.
        let pinned = cover.clone().map(|path| Resource::Path(path.into()));
        self.thumbnails
            .update(cx, |thumbnails, _| thumbnails.pin("library-cover", pinned));
        let avatar = match cover {
            Some(path) => img(path)
                .image_cache(&self.thumbnails)
                .size_full()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => ui::icon("books", 28., theme.text_secondary()).into_any_element(),
        };
        // A canvas takes no opacity of its own, so the fade is in the colour.
        let fill = theme::blend(
            gpui::transparent_black().into(),
            theme.surface_hover(),
            hover,
        );
        let highlight = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                // A 24px-radius circle about the cover, and a 30px bar from
                // the circle's centre to the right edge, drawn as one outline
                // so the overlap is not painted twice.
                let (left, top, right) = (bounds.left(), bounds.top(), bounds.right());
                let meet = px(24. + (24f32 * 24. - 15. * 15.).sqrt());
                let corner = point(px(6.), px(6.));
                let mut path = PathBuilder::fill();
                path.move_to(point(left + meet, top + px(9.)));
                path.line_to(point(right - px(6.), top + px(9.)));
                path.arc_to(corner, px(0.), false, true, point(right, top + px(15.)));
                path.line_to(point(right, top + px(33.)));
                path.arc_to(
                    corner,
                    px(0.),
                    false,
                    true,
                    point(right - px(6.), top + px(39.)),
                );
                path.line_to(point(left + meet, top + px(39.)));
                path.arc_to(
                    point(px(24.), px(24.)),
                    px(0.),
                    true,
                    true,
                    point(left + meet, top + px(9.)),
                );
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, fill);
                }
            },
        )
        .absolute()
        .inset_0();
        let entered = (fade, hint.clone());
        div()
            .id("library-switcher")
            .relative()
            .h(px(48.))
            .flex_none()
            .mt(px(4.))
            .mb(px(8.))
            .ml(px(theme::SIDEBAR_INSET))
            .mr(px(theme::SIDEBAR_INSET + 2.))
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                entered.0.update(cx, |fade, cx| {
                    fade.set(*hovered, motion::SIDEBAR_HOVER);
                    cx.notify();
                });
                entered.1.update(cx, |hint, cx| hint.set(*hovered, cx));
            })
            .child(highlight)
            .child(
                // `LibraryAvatar`: the cover in a 39px disc with a soft edge.
                div()
                    .relative()
                    .size(px(39.))
                    .ml(px(4.5))
                    .flex_none()
                    .rounded_full()
                    .overflow_hidden()
                    .bg(theme.avatar_surface())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(avatar)
                    // `.avatar::after`: the bevel that sets the cover into
                    // the rail, darker at the top and lit along the bottom.
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .shadow(theme::avatar_bevel()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .line_height(px(24.))
                    .child(name),
            )
            .child(
                ui::icon(
                    "selector",
                    14.,
                    theme::blend(theme.text_tertiary(), theme.text(), hover),
                )
                .relative()
                .flex_none()
                .opacity(0.7 + 0.3 * hover),
            )
            .children(hint.read(cx).layer(&ui::tip("Switch Library"), 48., cx))
    }
}

// ── Menus ───────────────────────────────────────────────────────────

/// `sidebarVisibilityEntries`: which system rows and sections show.
// TODO: a check mark on each row that is shown, once the menu draws them,
// and keep the menu open when one is toggled (`keepOpen`).
fn visibility_menu() -> Vec<menu::Entry> {
    let mut entries = SYSTEM_ROWS
        .iter()
        .map(|&(id, label, icon)| {
            menu::item(id, label)
                .icon(icon)
                .disabled(ALWAYS_SHOWN.contains(&id))
                .into()
        })
        .collect::<Vec<menu::Entry>>();
    entries.extend([
        menu::Entry::Separator,
        menu::item("section:quick_access", "Quick Access")
            .icon("menu-star")
            .into(),
        menu::item("section:smart_folders", "Smart Folders")
            .icon("menu-bookmark")
            .into(),
        menu::item("section:folders", "Folders")
            .icon("menu-folder")
            .into(),
    ]);
    entries
}

/// `openFolderMenu`.
fn folder_menu(
    expanded: bool,
    has_children: bool,
    any_expanded: bool,
    any_expandable: bool,
) -> Vec<menu::Entry> {
    vec![
        menu::item("new-folder", "New Folder")
            .icon("menu-folder-plus")
            .shortcut("⌘⇧N")
            .into(),
        menu::item("new-subfolder", "New Subfolder")
            .icon("menu-new-subfolder")
            .shortcut("⌥N")
            .into(),
        menu::Entry::Separator,
        // Nothing is in Quick Access here, so the row always offers to add.
        menu::item("quick-access", "Add to Quick Access")
            .icon("menu-star")
            .into(),
        menu::item("rename", "Rename")
            .icon("menu-rename")
            .shortcut("⌃R")
            .into(),
        menu::item("move-to", "Move to...")
            .icon("menu-folder-open")
            .into(),
        menu::item("duplicate", "Duplicate")
            .icon("menu-copy")
            .into(),
        menu::item("auto-tags", "Set Auto Tags...")
            .icon("menu-auto-tags")
            .shortcut("⌘⇧R")
            .into(),
        menu::Entry::Separator,
        menu::item("import-here", "Import Folder Here...")
            .icon("menu-folder-plus")
            .into(),
        menu::item("attach-watch", "Attach Watched Folder...")
            .icon("menu-watch-folder")
            .into(),
        // TODO: "Remove Watched Folder" for a watched folder, once the
        // navigation carries `watch_enabled`.
        menu::Entry::Separator,
        // TODO: "Sort Folders" and "Sort by" are submenus (this level or
        // with descendants, A-Z or Z-A; name, import date, dates, size,
        // notes); plain rows until the menu has submenus.
        menu::item("sort-folders", "Sort Folders")
            .icon("menu-sort")
            .into(),
        menu::item("sort-contents", "Sort by")
            .icon("menu-sort")
            .into(),
        expand_row("Folder", expanded, has_children),
        expand_all_row("Folders", any_expanded, any_expandable),
        menu::Entry::Separator,
        // TODO: "Change Icon" is a submenu holding the icon picker.
        menu::item("change-icon", "Change Icon")
            .icon("menu-change-icon")
            .into(),
        // TODO: the colour picker row (`ColorPicker` in the menu).
        menu::Entry::Separator,
        // TODO: "Export" is a submenu: Export Originals..., Export As...,
        // Export as Picto Pack....
        menu::item("export", "Export")
            .icon("menu-file-export")
            .into(),
        menu::Entry::Separator,
        menu::item("delete", "Delete")
            .icon("menu-trash")
            .danger(true)
            .into(),
    ]
}

/// `openSmartFolderMenu`. A group has no rules, so it has nothing to edit,
/// refresh, sort or export.
fn smart_folder_menu(
    group: bool,
    expanded: bool,
    has_children: bool,
    any_expanded: bool,
    any_expandable: bool,
) -> Vec<menu::Entry> {
    let mut entries: Vec<menu::Entry> = Vec::new();
    if !group {
        entries.extend([
            menu::item("edit-smart-folder", "Edit Smart Folder...")
                .icon("menu-folder-open")
                .into(),
            menu::item("edit-rules", "Edit Rules...")
                .icon("menu-adjustments")
                .into(),
        ]);
    }
    entries.extend([
        menu::item("new-child-smart-folder", "New Child Smart Folder")
            .icon("menu-folder-plus")
            .into(),
        menu::item("new-child-group", "New Child Smart Folder Group")
            .icon("menu-layout-grid")
            .into(),
        menu::Entry::Separator,
    ]);
    if !group {
        entries.push(
            menu::item("quick-access", "Add to Quick Access")
                .icon("menu-star")
                .into(),
        );
    }
    entries.extend([
        menu::item("rename", "Rename").icon("menu-rename").into(),
        // TODO: "Move to..." is a submenu of Top Level and every smart
        // folder it can move under.
        menu::item("move-to", "Move to...")
            .icon("menu-folder-open")
            .into(),
        menu::item("duplicate", "Duplicate")
            .icon("menu-copy")
            .into(),
        menu::Entry::Separator,
    ]);
    if !group {
        entries.extend([
            menu::item("refresh", "Refresh Results")
                .icon("menu-refresh")
                .into(),
            menu::item("sort-by-name", "Sort Results by Name")
                .icon("menu-sort")
                .into(),
        ]);
    }
    entries.extend([
        expand_row("Smart Folder", expanded, has_children),
        expand_all_row("Smart Folders", any_expanded, any_expandable),
        menu::Entry::Separator,
        // TODO: "Change Icon" is a submenu holding the icon picker.
        menu::item("change-icon", "Change Icon")
            .icon("menu-change-icon")
            .into(),
        // TODO: the colour picker row (`ColorPicker` in the menu).
        menu::Entry::Separator,
    ]);
    if !group {
        // TODO: "Export" is a submenu: Export Originals..., Export As...,
        // Export as Picto Pack....
        entries.push(
            menu::item("export", "Export")
                .icon("menu-file-export")
                .into(),
        );
    }
    entries.extend([
        menu::Entry::Separator,
        menu::item("delete", "Delete")
            .icon("menu-trash")
            .danger(true)
            .into(),
    ]);
    entries
}

fn expand_row(noun: &str, expanded: bool, has_children: bool) -> menu::Entry {
    let (label, icon) = if expanded {
        (format!("Collapse {noun}"), "menu-collapse")
    } else {
        (format!("Expand {noun}"), "menu-expand")
    };
    menu::item("toggle-expand", label)
        .icon(icon)
        .disabled(!has_children)
        .into()
}

fn expand_all_row(noun: &str, any_expanded: bool, any_expandable: bool) -> menu::Entry {
    let (label, icon) = if any_expanded {
        (format!("Collapse All {noun}"), "menu-collapse-all")
    } else {
        (format!("Expand All {noun}"), "menu-expand-all")
    };
    menu::item("toggle-expand-all", label)
        .icon(icon)
        .disabled(!any_expandable)
        .into()
}

/// `openBulkMenu`: several folders, several smart folders, or a mix.
fn bulk_menu(nodes: &[String], has_collapsed: bool, has_expanded: bool) -> Vec<menu::Entry> {
    let all_folders = nodes.iter().all(|id| id.starts_with("folder:"));
    let all_smart = nodes.iter().all(|id| id.starts_with("smart:"));
    let count = nodes.len();
    let mut entries: Vec<menu::Entry> = vec![
        menu::item("quick-access", "Add to Quick Access")
            .icon("menu-star")
            .into(),
        menu::item(
            "duplicate",
            format!(
                "Duplicate {count} item{}",
                if count == 1 { "" } else { "s" }
            ),
        )
        .icon("menu-copy")
        .into(),
    ];
    if all_folders {
        entries.push(
            menu::item("auto-tags", "Set Auto Tags...")
                .icon("menu-auto-tags")
                .shortcut("⌘⇧R")
                .into(),
        );
    }
    entries.push(menu::Entry::Separator);
    if all_folders {
        entries.extend([
            menu::item("move-to", "Move to...")
                .icon("menu-folder-open")
                .into(),
            // TODO: "Sort by" is a submenu of content sort fields.
            menu::item("sort-contents", "Sort by")
                .icon("menu-sort")
                .into(),
        ]);
    } else if all_smart {
        entries.extend([
            // TODO: "Move to..." is a submenu of Top Level and the smart
            // folders the selection can move under.
            menu::item("move-to", "Move to...")
                .icon("menu-folder-open")
                .into(),
            menu::item("sort-by-name", "Sort Results by Name")
                .icon("menu-sort")
                .into(),
        ]);
    }
    if all_folders || all_smart {
        entries.extend([
            menu::item("expand-selected", "Expand Selected")
                .icon("menu-expand")
                .disabled(!has_collapsed)
                .into(),
            menu::item("collapse-selected", "Collapse Selected")
                .icon("menu-collapse")
                .disabled(!has_expanded)
                .into(),
            menu::Entry::Separator,
            // TODO: the colour picker row (`ColorPicker` in the menu).
            menu::Entry::Separator,
        ]);
    }
    entries.push(
        menu::item("delete", format!("Delete {count} items"))
            .icon("menu-trash")
            .danger(true)
            .into(),
    );
    entries
}

/// A smart folder's definition, to write back with one thing changed.
fn smart_input(library: &Library, id: SmartFolderId) -> picto_library::Result<SmartFolderInput> {
    let record = library
        .smart_folders()?
        .into_iter()
        .find(|record| record.smart_folder_id == id)
        .ok_or_else(|| LibraryError::NotFound(format!("smart folder {id}")))?;
    Ok(SmartFolderInput {
        name: record.name,
        parent_id: record.parent_id,
        icon: record.icon,
        color: record.color,
        notes: record.notes,
        view: record.view,
    })
}

/// `emptySmartFolderPayload`: a smart folder with no rules is a group.
fn new_group(parent_id: Option<SmartFolderId>) -> SmartFolderInput {
    SmartFolderInput {
        name: "Untitled".into(),
        parent_id,
        icon: None,
        color: None,
        notes: None,
        view: Default::default(),
    }
}

fn sorted_by_name(mut input: SmartFolderInput) -> SmartFolderInput {
    input.view.sort = picto_library::predicate::ItemSort {
        field: picto_library::predicate::SortField::Name,
        direction: picto_library::predicate::SortDirection::Ascending,
        random_seed: None,
    };
    input
}

impl Sidebar {
    // ── Edits ───────────────────────────────────────────────────────

    /// Runs an edit on the library's thread, then reads the destinations
    /// again, since names, counts and the trees may have changed, and hands
    /// the edit's answer to `then`.
    fn edit<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Library) -> picto_library::Result<T> + Send + 'static,
        then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = self
            .session
            .read(cx)
            .run(move |library| (work(library), backend::navigation(library)));
        cx.spawn_in(window, async move |this, cx| {
            let Ok((result, navigation)) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, move |this, window, cx| {
                if let Ok((nav, tags)) = navigation {
                    this.session
                        .update(cx, |session, cx| session.replace_navigation(nav, tags, cx));
                }
                match result {
                    Ok(value) => then(this, value, window, cx),
                    // TODO: show this as a notification, as the Electron
                    // sidebar's `showErrorNotification` does.
                    Err(error) => eprintln!("sidebar: {error}"),
                }
            });
        })
        .detach();
    }

    /// `createFolderAndRename`: a folder named "New Folder", revealed and
    /// renamed in place.
    fn create_folder(
        &mut self,
        parent: Option<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| library.create_folder("New Folder", parent),
            |this, (id, _), window, cx| {
                this.rename_new(format!("folder:{}", id.0), "New Folder".into(), window, cx)
            },
            window,
            cx,
        );
    }

    /// `createSmartFolderGroupAndRename`.
    fn create_group(
        &mut self,
        parent: Option<SmartFolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| library.create_smart_folder(new_group(parent)),
            |this, (id, _), window, cx| {
                this.rename_new(format!("smart:{}", id.0), "Untitled".into(), window, cx)
            },
            window,
            cx,
        );
    }

    /// Duplicates folders and smart folders. One duplicate is renamed in
    /// place under the name the Electron sidebar offers, "<name> copy".
    fn duplicate(&mut self, nodes: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let names = nodes
            .iter()
            .map(|node| self.label_of(node, cx).unwrap_or_default())
            .collect::<Vec<_>>();
        let single = nodes.len() == 1;
        self.edit(
            move |library| {
                let mut made = Vec::new();
                for (node, name) in nodes.iter().zip(&names) {
                    if let Some(id) = folder_id(node) {
                        let (copy, _) = library.duplicate_folder(id)?;
                        made.push((format!("folder:{}", copy.0), format!("{name} copy")));
                    } else if let Some(id) = smart_folder_id(node) {
                        let mut input = smart_input(library, id)?;
                        input.name = format!("{name} copy");
                        let (copy, _) = library.create_smart_folder(input)?;
                        made.push((format!("smart:{}", copy.0), format!("{name} copy")));
                    }
                }
                Ok(made)
            },
            move |this, made, window, cx| {
                if let (true, Some((node, name))) = (single, made.into_iter().next()) {
                    this.rename_new(node, name, window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Deletes folder and smart-folder definitions. The library removes the
    /// definitions and their memberships; no media file is touched.
    fn delete(&mut self, nodes: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let folders = nodes
            .iter()
            .filter_map(|node| folder_id(node))
            .collect::<Vec<_>>();
        let smart = nodes
            .iter()
            .filter_map(|node| smart_folder_id(node))
            .collect::<Vec<_>>();
        let bulk = nodes.len() > 1;
        self.edit(
            move |library| {
                if !folders.is_empty() {
                    library.delete_folders(&folders)?;
                }
                for id in smart {
                    library.delete_smart_folder(id)?;
                }
                Ok(())
            },
            move |this, (), _, cx| {
                if bulk {
                    this.selection.clear();
                }
                cx.notify();
            },
            window,
            cx,
        );
    }

    /// Writes a renamed folder or smart folder. The row shows the new name
    /// at once, as the Electron sidebar patches its node before the write.
    fn apply_rename(
        &mut self,
        node: &str,
        original: &str,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if name.is_empty() || name == original {
            return;
        }
        self.session.update(cx, |session, cx| {
            if let Some(entry) = session.nav.iter_mut().find(|entry| entry.id() == node) {
                entry.label = name.clone();
                cx.emit(Change::Navigation);
            }
        });
        if let Some(id) = folder_id(node) {
            self.edit(
                move |library| library.rename_folder(id, &name).map(|_| ()),
                |_, (), _, _| {},
                window,
                cx,
            );
        } else if let Some(id) = smart_folder_id(node) {
            self.edit(
                move |library| {
                    let mut input = smart_input(library, id)?;
                    input.name = name;
                    library.update_smart_folder(id, input).map(|_| ())
                },
                |_, (), _, _| {},
                window,
                cx,
            );
        }
    }

    // ── Rename in place ─────────────────────────────────────────────

    /// A new or duplicated node: open its section and branch, then rename it.
    fn rename_new(
        &mut self,
        node: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = Section::of_node(&node) {
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
        self.start_rename(node, name, window, cx);
    }

    /// `useInlineRename.startRename`: the name selected in a focused field.
    fn start_rename(
        &mut self,
        node: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A rename already open is committed first.
        if let Some(open) = self.renaming.take() {
            let value = open.input.read(cx).value().trim().to_owned();
            self.apply_rename(&open.node, &open.original, value, window, cx);
        }
        let input = cx.new(|cx| TextInput::rename(&name, window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &Renamed, window, cx| {
                // Only the field of the rename still open counts.
                if this
                    .renaming
                    .as_ref()
                    .is_none_or(|open| open.input.entity_id() != input.entity_id())
                {
                    return;
                }
                let Some(open) = this.renaming.take() else {
                    return;
                };
                if let Renamed::Commit(value) = event {
                    this.apply_rename(&open.node, &open.original, value.clone(), window, cx);
                }
                // Enter and Escape leave nothing focused; the keyboard goes back
                // to the window.
                if window.focused(cx).is_none() {
                    window.focus(&this.home, cx);
                }
                cx.notify();
            },
        );
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            node,
            original: name,
            input,
            _subscription: subscription,
        });
        cx.notify();
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

    fn open_menu(
        &mut self,
        target: Target,
        entries: Vec<menu::Entry>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        let chosen = target.clone();
        // The subscriptions of a menu replaced by this one go first, so its
        // release does not clear this menu's highlight.
        self._menu = vec![
            cx.subscribe_in(
                &menu,
                window,
                move |this, _, event: &menu::Event, window, cx| {
                    let menu::Event::Chosen(action) = event;
                    this.choose(&chosen, action, window, cx);
                },
            ),
            cx.observe_release(&menu, |this, _, cx| {
                this.context = None;
                cx.notify();
            }),
        ];
        // Only a single folder's or smart folder's menu marks its row.
        self.context = match target {
            Target::Node(node) => Some(node),
            _ => None,
        };
        cx.emit(ShowMenu(menu));
        cx.notify();
    }

    /// `openSystemMenu`: Recently Viewed and Trash have their own menus;
    /// every other system row shows which rows and sections are visible.
    fn system_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, count) = {
            let entry = &self.session.read(cx).nav[index];
            (entry.id(), entry.count.unwrap_or(0))
        };
        let entries = match id.as_str() {
            "system:recent_viewed" => vec![
                menu::item("clear-recent", "Clear Recently Viewed")
                    .icon("menu-trash")
                    .disabled(count == 0)
                    .into(),
            ],
            "system:trash" => vec![
                // This prototype borrows the library's files and never
                // deletes them, so the row is shown but not offered.
                menu::item("empty-trash", "Empty Trash")
                    .icon("menu-trash")
                    .danger(true)
                    .disabled(true)
                    .into(),
                menu::item("restore-all", "Restore All")
                    .icon("menu-download")
                    .disabled(count == 0)
                    .into(),
            ],
            _ => visibility_menu(),
        };
        self.open_menu(Target::System, entries, position, window, cx);
    }

    /// `handleFolderContextMenu` and `handleSmartFolderContextMenu`: the
    /// selection's menu when the row is part of a larger selection,
    /// otherwise the row's own, which clears the selection.
    fn node_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.session.read(cx).nav.clone();
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        let node = ids[index].clone();
        if self.selection.len() > 1 && self.selection.contains(&node) {
            let nodes = ids
                .iter()
                .filter(|id| self.selection.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            let parents = ids
                .iter()
                .enumerate()
                .filter(|(at, _)| entries.iter().any(|entry| entry.parent == Some(*at)))
                .map(|(_, id)| id)
                .collect::<HashSet<_>>();
            let folding = nodes.iter().filter(|id| parents.contains(id));
            let has_collapsed = folding.clone().any(|id| self.collapsed.contains(id));
            let has_expanded = folding.clone().any(|id| !self.collapsed.contains(id));
            let menu = bulk_menu(&nodes, has_collapsed, has_expanded);
            self.open_menu(Target::Bulk(nodes), menu, position, window, cx);
            return;
        }
        self.selection.clear();
        let Some(section) = Section::of_node(&node) else {
            return;
        };
        let all = expandable(&entries, &ids, section);
        let any_expanded = all.iter().any(|id| !self.collapsed.contains(id));
        let has_children = all.contains(&node);
        let expanded = !self.collapsed.contains(&node);
        let menu = match section {
            Section::Folders => folder_menu(expanded, has_children, any_expanded, !all.is_empty()),
            Section::SmartFolders => smart_folder_menu(
                entries[index].group,
                expanded,
                has_children,
                any_expanded,
                !all.is_empty(),
            ),
        };
        self.open_menu(Target::Node(node), menu, position, window, cx);
    }

    /// The section header's menu, and the Smart Folders add button's.
    fn section_menu(
        &mut self,
        section: Section,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = match section {
            Section::Folders => vec![
                menu::item("new-folder", "New Folder")
                    .icon("menu-folder-plus")
                    .shortcut("⌘⇧N")
                    .into(),
                menu::item("import-folder", "Import Folder...")
                    .icon("menu-folder-plus")
                    .into(),
            ],
            Section::SmartFolders => vec![
                menu::item("new-smart-folder", "New Smart Folder")
                    .icon("menu-filter-plus")
                    .into(),
                menu::item("new-group", "New Smart Folder Group")
                    .icon("menu-layout-grid")
                    .into(),
            ],
        };
        self.open_menu(Target::Section, entries, position, window, cx);
    }

    /// Does what a menu row says, for what the menu was opened on.
    fn choose(
        &mut self,
        target: &Target,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The visibility rows.
        if let Some((id, ..)) = SYSTEM_ROWS.iter().find(|row| row.0 == action) {
            if !self.prefs.visible_system.remove(id) {
                self.prefs.visible_system.insert(id);
            }
            cx.notify();
            return;
        }
        let flag = match action {
            "section:quick_access" => Some(&mut self.prefs.show_quick_access),
            "section:folders" => Some(&mut self.prefs.show_folders),
            "section:smart_folders" => Some(&mut self.prefs.show_smart_folders),
            _ => None,
        };
        if let Some(flag) = flag {
            *flag = !*flag;
            cx.notify();
            return;
        }
        match target {
            Target::System => match action {
                "clear-recent" => self.edit(
                    |library| library.clear_recent_views().map(|_| ()),
                    |this, (), _, cx| this.session.update(cx, |session, cx| session.reload(cx)),
                    window,
                    cx,
                ),
                "restore-all" => self.edit(
                    |library| {
                        library
                            .set_lifecycle(
                                &picto_library::selection::SelectionTarget::Query {
                                    query: RootQuery {
                                        scope: ItemScope::Trash,
                                        view: Default::default(),
                                    },
                                    excluded_root_ids: Vec::new(),
                                },
                                picto_library::Lifecycle::Active,
                            )
                            .map(|_| ())
                    },
                    |this, (), _, cx| this.session.update(cx, |session, cx| session.reload(cx)),
                    window,
                    cx,
                ),
                _ => {}
            },
            Target::Section => match action {
                "new-folder" => self.create_folder(None, window, cx),
                "new-group" => self.create_group(None, window, cx),
                // TODO: "Import Folder..." chooses a folder and imports it,
                // and "New Smart Folder" opens the smart-folder editor.
                _ => {}
            },
            Target::Node(node) => self.choose_for_node(node, action, window, cx),
            Target::Bulk(nodes) => match action {
                "duplicate" => self.duplicate(nodes.clone(), window, cx),
                "delete" => self.delete(nodes.clone(), window, cx),
                "expand-selected" | "collapse-selected" => {
                    let parents = {
                        let nav = &self.session.read(cx).nav;
                        nodes
                            .iter()
                            .filter(|node| {
                                nav.iter()
                                    .position(|entry| entry.id() == **node)
                                    .is_some_and(|index| {
                                        nav.iter().any(|entry| entry.parent == Some(index))
                                    })
                            })
                            .cloned()
                            .collect::<Vec<_>>()
                    };
                    self.set_expanded(&parents, action == "expand-selected", cx);
                }
                "sort-by-name" => {
                    let ids = nodes
                        .iter()
                        .filter_map(|node| smart_folder_id(node))
                        .collect::<Vec<_>>();
                    self.edit(
                        move |library| {
                            for id in ids {
                                library.update_smart_folder(
                                    id,
                                    sorted_by_name(smart_input(library, id)?),
                                )?;
                            }
                            Ok(())
                        },
                        |this, (), _, cx| this.reload_if_showing(None, cx),
                        window,
                        cx,
                    );
                }
                // TODO: "Add to Quick Access", "Set Auto Tags...", "Move
                // to..." and "Sort by" need Quick Access, the auto-tags
                // editor, the folder picker and submenus.
                _ => {}
            },
        }
    }

    fn choose_for_node(
        &mut self,
        node: &str,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.session.read(cx).nav.clone();
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        let Some(index) = ids.iter().position(|id| id == node) else {
            return;
        };
        let Some(section) = Section::of_node(node) else {
            return;
        };
        match action {
            "new-folder" => self.create_folder(None, window, cx),
            "new-subfolder" => self.create_folder(folder_id(node), window, cx),
            "new-child-group" => self.create_group(smart_folder_id(node), window, cx),
            "rename" => {
                self.start_rename(node.to_owned(), entries[index].label.clone(), window, cx)
            }
            "duplicate" => self.duplicate(vec![node.to_owned()], window, cx),
            "delete" => {
                // TODO: ask first, as the Electron sidebar's confirmation
                // does ("Media inside these folders will remain untouched").
                self.delete(vec![node.to_owned()], window, cx)
            }
            "toggle-expand" => {
                if entries.iter().any(|entry| entry.parent == Some(index)) {
                    self.toggle_collapsed(node, cx);
                }
            }
            "toggle-expand-all" => {
                let all = expandable(&entries, &ids, section);
                let any_expanded = all.iter().any(|id| !self.collapsed.contains(id));
                self.set_expanded(&all, !any_expanded, cx);
            }
            "refresh" => {
                let node = node.to_owned();
                self.edit(
                    |_| Ok(()),
                    move |this, (), _, cx| this.reload_if_showing(Some(&node), cx),
                    window,
                    cx,
                );
            }
            "sort-by-name" => {
                let (Some(id), node) = (smart_folder_id(node), node.to_owned()) else {
                    return;
                };
                self.edit(
                    move |library| {
                        library
                            .update_smart_folder(id, sorted_by_name(smart_input(library, id)?))
                            .map(|_| ())
                    },
                    move |this, (), _, cx| this.reload_if_showing(Some(&node), cx),
                    window,
                    cx,
                );
            }
            // TODO: these need parts the prototype does not have yet: Quick
            // Access ("quick-access"), the folder picker ("move-to"), the
            // auto-tags editor ("auto-tags"), import ("import-here"), watched
            // folders ("attach-watch"), submenus ("sort-folders",
            // "sort-contents", "change-icon", "export") and the smart-folder
            // editor ("edit-smart-folder", "edit-rules",
            // "new-child-smart-folder").
            _ => {}
        }
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

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Sidebar);
        let theme = theme::theme(cx);
        let filter = self.filter.read(cx).value().trim().to_lowercase();
        let filtering = !filter.is_empty();
        let (entries, active) = {
            let session = self.session.read(cx);
            (session.nav.clone(), session.scope)
        };
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        let section =
            |index: usize| section_of(&entries[index].scope, entries[index].screen.is_some());
        let count_of = |wanted| {
            (0..entries.len())
                .filter(|index| section(*index) == Some(wanted))
                .count()
        };
        // The tree filter keeps each matching folder and its ancestors, and
        // opens every branch (`treeFilter.ts`). System rows are not filtered.
        let mut kept = vec![!filtering; entries.len()];
        if filtering {
            for index in 0..entries.len() {
                if section(index).is_none() {
                    kept[index] = true;
                } else if entries[index].label.to_lowercase().contains(&filter) {
                    let mut at = Some(index);
                    while let Some(index) = at {
                        kept[index] = true;
                        at = entries[index].parent;
                    }
                }
            }
        }
        let shown_at = |index: usize| {
            if !kept[index] {
                return false;
            }
            if filtering {
                return true;
            }
            let mut parent = entries[index].parent;
            while let Some(ancestor) = parent {
                if self.collapsed.contains(&ids[ancestor]) {
                    return false;
                }
                parent = entries[ancestor].parent;
            }
            true
        };
        // Whether a row has a sibling below it among the rows that show.
        let has_next_sibling = |index: usize| {
            (index + 1..entries.len()).any(|other| {
                entries[other].parent == entries[index].parent
                    && section(other) == section(index)
                    && kept[other]
            })
        };

        let mut list = div()
            .id("navigation")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(ui::scrollbar::GUTTER + theme::SIDEBAR_LIST_PADDING))
            .pb(px(16.));

        // System rows, those the visibility menu leaves on.
        for (index, entry) in entries.iter().enumerate() {
            if section(index).is_some() || !self.prefs.visible_system.contains(ids[index].as_str())
            {
                continue;
            }
            list = list.child(
                ui::sidebar_row(
                    ElementId::Name(ids[index].clone().into()),
                    entry.icon.clone().unwrap_or_default(),
                    entry.label.clone(),
                )
                .count(entry.count)
                .active(active == index)
                .focused(self.keyboard_row.as_ref() == Some(&ids[index]))
                .drop_target(self.drop_target.as_ref() == Some(&ids[index]))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selection.clear();
                    this.session
                        .update(cx, |session, cx| session.navigate(index, cx));
                    cx.notify();
                }))
                .on_secondary(cx.listener(
                    move |this, event: &MouseDownEvent, window, cx| {
                        this.system_menu(index, event.position, window, cx)
                    },
                )),
            );
        }

        let mut matches = 0;
        for which in SECTIONS {
            if !self.prefs.shows(which) {
                self.rendered[which as usize].clear();
                continue;
            }
            let expanded = filtering || !self.collapsed.contains(which.id());
            let (tip, element) = match which {
                Section::Folders => (ui::tip("New Folder").keys(&["⌘", "⇧", "N"]), "folders"),
                Section::SmartFolders => (ui::tip("New Smart Folder or Group"), "smart-folders"),
            };
            list = list.child(
                ui::section_header(element, which.label())
                    .count(count_of(which))
                    .expanded(expanded)
                    .on_toggle(cx.listener(move |this, _, _, cx| {
                        let filtering = !this.filter.read(cx).value().trim().is_empty();
                        if !filtering {
                            this.toggle_collapsed(which.id(), cx);
                        }
                    }))
                    .on_add(
                        tip,
                        cx.listener(
                            move |this, bounds: &Bounds<Pixels>, window, cx| match which {
                                Section::Folders => this.create_folder(None, window, cx),
                                // A menu below the button, at its left edge.
                                Section::SmartFolders => this.section_menu(
                                    which,
                                    point(bounds.left(), bounds.bottom() + px(4.)),
                                    window,
                                    cx,
                                ),
                            },
                        ),
                    )
                    .on_secondary(
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.section_menu(which, event.position, window, cx)
                        }),
                    ),
            );
            let mut rendered = Vec::new();
            if expanded {
                for (index, entry) in entries.iter().enumerate() {
                    if section(index) != Some(which) || !shown_at(index) {
                        continue;
                    }
                    // A section's first row sits 2px below its header.
                    if rendered.is_empty() {
                        list = list.child(div().h(px(2.)));
                    }
                    rendered.push(ids[index].clone());
                    list = list.child(
                        self.node_row(
                            index,
                            entry,
                            &ids[index],
                            active == index,
                            (0..entries.len())
                                .any(|other| entries[other].parent == Some(index) && kept[other]),
                            filtering,
                            {
                                // Each ancestor's vertical guide continues through
                                // this row while that ancestor has siblings still
                                // to come.
                                let mut continues = vec![false; entry.depth];
                                let mut ancestor = entry.parent;
                                while let Some(at) = ancestor {
                                    let depth = entries[at].depth;
                                    if depth < continues.len() {
                                        continues[depth] = has_next_sibling(at);
                                    }
                                    ancestor = entries[at].parent;
                                }
                                Branch {
                                    continues,
                                    last_child: !has_next_sibling(index),
                                }
                            },
                            cx,
                        ),
                    );
                }
            }
            matches += rendered.len();
            self.rendered[which as usize] = rendered;
        }
        if filtering && matches == 0 {
            list = list.child(
                div()
                    .mt(px(6.))
                    .ml(px(theme::SIDEBAR_INSET))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child("No matching folders"),
            );
        }
        let tree_filter = self.prefs.show_folders || self.prefs.show_smart_folders;
        // The rail's edge is drawn over the app background, as the shell's
        // border is in the Electron layout, with the rail surface inside it.
        div()
            .id("sidebar")
            .size_full()
            .bg(theme.background())
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .size_full()
                    .bg(theme.rail())
                    .font_family(theme::FONT_INSPECTOR)
                    .flex()
                    .flex_col()
                    .child(self.library(window, cx))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(list)
                            .child(ui::scrollbar("sidebar-scrollbar", &self.scroll, window, cx))
                            .children(ui::scroll_to_top(
                                "sidebar-return-to-top",
                                &self.scroll,
                                RETURN_TO_TOP_AFTER,
                                // 48px above the sidebar's foot, 38 of which
                                // is the filter below this list.
                                if tree_filter { 10. } else { 48. },
                                window,
                                cx,
                            )),
                    )
                    .children(tree_filter.then(|| {
                        div()
                            .relative()
                            .h(px(38.))
                            .px(px(10.))
                            .pb(px(10.))
                            .flex_none()
                            // The footer covers the last 10px of the list.
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-10.))
                                    .left_0()
                                    .right_0()
                                    .h(px(10.))
                                    .bg(theme.rail()),
                            )
                            .child(
                                ui::field("sidebar-filter", &self.filter, Look::SidebarFilter)
                                    .icon("toolbar-filter", 16., 8., theme.text())
                                    .clearable(),
                            )
                    })),
            )
    }
}

impl Sidebar {
    /// One folder or smart-folder row.
    #[allow(clippy::too_many_arguments)]
    fn node_row(
        &self,
        index: usize,
        entry: &Navigation,
        id: &str,
        active: bool,
        has_children: bool,
        filtering: bool,
        branch: Branch,
        cx: &mut Context<Self>,
    ) -> ui::sidebar_row::SidebarRow {
        let expanded = filtering || !self.collapsed.contains(id);
        let group = entry.group;
        let in_section = section_of(&entry.scope, false).is_some();
        // `NodeIcon`: the glyph the user chose, else the group's grid, else
        // a folder that opens with its branch. A name the icon picker no
        // longer has falls back to the folder.
        let icon = match &entry.icon {
            Some(name) if !in_section || crate::assets::has_icon(name) => name.as_str(),
            Some(_) => "folder",
            None if group => "layout-grid",
            None if has_children && expanded => "folder-open",
            None => "folder",
        }
        .to_owned();
        let node = id.to_owned();
        let double_click = self.prefs.double_click;
        // Keyed by id, so a row keeps its state when the list is rebuilt.
        let mut row = ui::sidebar_row(
            ElementId::Name(id.to_owned().into()),
            icon,
            entry.label.clone(),
        )
        .depth(entry.depth)
        .tint(entry.tint.map(rgb))
        .count(entry.count)
        .active(active)
        .selected(!active && self.selection.contains(id))
        .context_highlight(!active && self.context.as_deref() == Some(id))
        .focused(self.keyboard_row.as_deref() == Some(id))
        .drop_target(self.drop_target.as_deref() == Some(id))
        .drop(
            self.drop
                .as_ref()
                .filter(|(target, _)| target == id)
                .map(|(_, position)| *position),
        )
        .rename(
            self.renaming
                .as_ref()
                .filter(|open| open.node == id)
                .map(|open| open.input.clone()),
        )
        .disclosure(has_children.then_some(expanded))
        .branch(branch)
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            // A group is not a destination: a click folds or opens it.
            if group {
                if has_children && !filtering {
                    this.toggle_collapsed(&node, cx);
                }
            } else {
                this.click_node(index, node.clone(), event.modifiers(), cx);
            }
            // The second press of a double-click also does what the
            // setting says: rename, or fold the branch.
            if event.click_count() == 2 {
                match double_click {
                    DoubleClick::Rename => {
                        let name = this.label_of(&node, cx).unwrap_or_default();
                        this.start_rename(node.clone(), name, window, cx);
                    }
                    DoubleClick::Collapse if has_children && !filtering => {
                        this.toggle_collapsed(&node, cx)
                    }
                    DoubleClick::Collapse => {}
                }
            }
        }))
        .on_secondary(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.node_menu(index, event.position, window, cx)
            }),
        );
        // The filter holds every branch open.
        if !filtering {
            let node = id.to_owned();
            row =
                row.on_toggle(cx.listener(move |this, _, _, cx| this.toggle_collapsed(&node, cx)));
        }
        row
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
