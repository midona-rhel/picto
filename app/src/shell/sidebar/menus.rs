//! This file builds the sidebar's context menus and opens them: the menu of
//! a system row, of a folder, of a smart folder, of several selected rows,
//! of a section header, and of a subfolder tile in the grid.
use super::{
    ALWAYS_SHOWN, Preferences, SYSTEM_ROWS, Section, ShowMenu, Sidebar, Target, expandable, tree,
};
use crate::backend::Navigation;
use crate::grid::menu::CONTENT_SORT;
use crate::i18n::t;
use crate::ui;
use crate::ui::menu::{self, Menu};
use gpui::*;
use picto_library::model::ContentSortField;
use std::collections::HashSet;

// The functions below build the entries of the menus.

/// Builds the menu that shows and hides the system rows and the sections.
/// Each row has a check mark while the thing it names is shown. The menu
/// stays open when a row is chosen.
fn visibility_menu(prefs: &Preferences) -> Vec<menu::Entry> {
    let mut entries = SYSTEM_ROWS
        .iter()
        .map(|&(id, label, icon)| {
            menu::item(id, label)
                .icon(icon)
                .checked(ALWAYS_SHOWN.contains(&id) || prefs.visible_system.contains(id))
                .disabled(ALWAYS_SHOWN.contains(&id))
                .keep_open(true)
                .into()
        })
        .collect::<Vec<menu::Entry>>();
    entries.extend([
        menu::Entry::Separator,
        menu::item("section:quick_access", "Quick Access")
            .icon("menu-star")
            .checked(prefs.show_quick_access)
            .keep_open(true)
            .into(),
        menu::item("section:smart_folders", "Smart Folders")
            .icon("menu-bookmark")
            .checked(prefs.show_smart_folders)
            .keep_open(true)
            .into(),
        menu::item("section:folders", "Folders")
            .icon("menu-folder")
            .checked(prefs.show_folders)
            .keep_open(true)
            .into(),
    ]);
    entries
}

/// The state of a folder or smart folder that decides which rows its
/// context menu shows and how they are drawn.
struct NodeMenu {
    expanded: bool,
    has_children: bool,
    any_expanded: bool,
    any_expandable: bool,
    /// Whether the node is in Quick Access.
    quick: bool,
    /// Whether the folder watches a directory on disk.
    watched: bool,
    /// The node's colour, which the row of colour swatches marks as chosen.
    color: Option<String>,
    /// The icon picker that the "Change Icon" submenu holds.
    icon_picker: AnyView,
    /// The smart folders under which this smart folder can be moved, each
    /// with its label and its node id. The "Move to..." submenu lists them
    /// after "Top Level".
    move_targets: Vec<(String, String)>,
}

fn quick_access_row(quick: bool) -> menu::Entry {
    if quick {
        menu::item("quick-access", "Remove from Quick Access")
            .icon("menu-star-off")
            .into()
    } else {
        menu::item("quick-access", "Add to Quick Access")
            .icon("menu-star")
            .into()
    }
}

/// Builds the "Change Icon" row. Its submenu holds the icon picker and
/// nothing else.
fn change_icon(picker: AnyView) -> menu::Entry {
    menu::submenu(
        t("Change Icon"),
        vec![
            menu::view(
                picker,
                ui::icon_picker::WIDTH + 2. * MENU_PADDING,
                ui::icon_picker::HEIGHT,
            )
            .into(),
        ],
    )
    .icon("menu-change-icon")
    .into()
}

/// Builds the "Sort by" submenu, which lists the fields by which a folder's
/// items can be sorted.
fn content_sort() -> menu::Entry {
    menu::submenu(
        t("Sort by"),
        CONTENT_SORT
            .iter()
            .map(|(_, label, value)| menu::item("sort-contents", *label).value(*value).into())
            .collect(),
    )
    .icon("menu-sort")
    .into()
}

pub(super) fn content_sort_field(value: &str) -> Option<ContentSortField> {
    CONTENT_SORT
        .iter()
        .find(|(.., name)| *name == value)
        .map(|(field, ..)| *field)
}

/// Builds the "Export" submenu of a folder or smart folder.
fn export_submenu() -> menu::Entry {
    menu::submenu(
        t("Export"),
        vec![
            menu::item("export-originals", "Export Originals...").into(),
            menu::item("export-as", "Export As...").into(),
            menu::item("export-pack", "Export as Picto Pack...").into(),
        ],
    )
    .icon("menu-file-export")
    .into()
}

/// The padding in pixels between a menu's panel and its rows on each side.
/// The view that holds the icon picker in a submenu is given the picker's
/// width plus this padding on both sides.
const MENU_PADDING: f32 = 4.;

/// Builds the context menu of a folder's row in the sidebar.
fn folder_menu(node: NodeMenu, cx: &App) -> Vec<menu::Entry> {
    let mut entries = vec![
        menu::item("new-folder", "New Folder")
            .icon("menu-folder-plus")
            .shortcut_for("file.newFolder", cx)
            .into(),
        menu::item("new-subfolder", "New Subfolder")
            .icon("menu-new-subfolder")
            .shortcut_for("file.newSubfolder", cx)
            .into(),
        menu::Entry::Separator,
        quick_access_row(node.quick),
        menu::item("rename", "Rename")
            .icon("menu-rename")
            .shortcut_for("edit.rename", cx)
            .into(),
        menu::item("move-to", "Move to...")
            .icon("menu-folder-open")
            .into(),
        menu::item("duplicate", "Duplicate")
            .icon("menu-copy")
            .into(),
        menu::item("auto-tags", "Set Auto Tags...")
            .icon("menu-auto-tags")
            .shortcut_for("folder.autoTags", cx)
            .into(),
        menu::Entry::Separator,
        menu::item("import-here", "Import Folder Here...")
            .icon("menu-folder-plus")
            .disabled(crate::imports::blocked(cx).is_some())
            .into(),
        menu::item("attach-watch", "Attach Watched Folder...")
            .icon("menu-watch-folder")
            .into(),
    ];
    if node.watched {
        entries.push(
            menu::item("remove-watch", "Remove Watched Folder")
                .icon("menu-watch-folder")
                .into(),
        );
    }
    entries.extend([
        menu::Entry::Separator,
        sort_folders(),
        content_sort(),
        expand_row(false, node.expanded, node.has_children),
        expand_all_row(false, node.any_expanded, node.any_expandable),
        menu::Entry::Separator,
        change_icon(node.icon_picker),
        ui::color_picker::menu_row("color", node.color.as_deref()).into(),
        menu::Entry::Separator,
        export_submenu(),
        menu::Entry::Separator,
        menu::item("delete", "Delete")
            .icon("menu-trash")
            .danger(true)
            .into(),
    ]);
    entries
}

/// Builds the "Sort Folders" submenu. Its rows sort a folder's children by
/// name, or its children and all their descendants, from A to Z or from Z
/// to A.
fn sort_folders() -> menu::Entry {
    menu::submenu(
        t("Sort Folders"),
        vec![
            menu::item("sort-tree", "This Level A–Z").value("az").into(),
            menu::item("sort-tree", "This Level Z–A").value("za").into(),
            menu::item("sort-tree", "This Level and Descendants A–Z")
                .value("az-all")
                .into(),
            menu::item("sort-tree", "This Level and Descendants Z–A")
                .value("za-all")
                .into(),
        ],
    )
    .icon("menu-sort")
    .into()
}

/// Builds the context menu of a subfolder tile in the grid.
fn tile_menu(
    quick: bool,
    watched: bool,
    color: Option<&str>,
    picker: AnyView,
    cx: &App,
) -> Vec<menu::Entry> {
    let mut entries = vec![
        menu::item("open", "Open Folder")
            .icon("menu-folder-open")
            .into(),
        menu::item("new-subfolder", "New Subfolder")
            .icon("menu-new-subfolder")
            .into(),
        menu::Entry::Separator,
        quick_access_row(quick),
        menu::item("rename", "Rename").icon("menu-rename").into(),
        menu::item("move-to", "Move to...")
            .icon("menu-folder-open")
            .into(),
        menu::item("duplicate", "Duplicate")
            .icon("menu-copy")
            .into(),
        menu::item("auto-tags", "Set Auto Tags...")
            .icon("menu-auto-tags")
            .into(),
        menu::Entry::Separator,
        menu::item("import-here", "Import Folder Here...")
            .icon("menu-folder-plus")
            .disabled(crate::imports::blocked(cx).is_some())
            .into(),
        menu::item("attach-watch", "Attach Watched Folder...")
            .icon("menu-watch-folder")
            .into(),
    ];
    if watched {
        entries.push(
            menu::item("remove-watch", "Remove Watched Folder")
                .icon("menu-watch-folder")
                .into(),
        );
    }
    entries.extend([
        menu::Entry::Separator,
        sort_folders(),
        content_sort(),
        menu::Entry::Separator,
        change_icon(picker),
        ui::color_picker::menu_row("color", color).into(),
        menu::Entry::Separator,
        menu::item("export-as", "Export to Computer...")
            .icon("menu-upload")
            .into(),
        menu::item("export-pack", "Export as Picto Pack...")
            .icon("menu-upload")
            .into(),
        menu::Entry::Separator,
        menu::item("delete", "Delete")
            .icon("menu-trash")
            .danger(true)
            .into(),
    ]);
    entries
}

/// Builds the context menu of several selected subfolder tiles in the grid.
/// `count` is the number of tiles, and `all_quick` says whether all of them
/// are in Quick Access.
fn tile_bulk_menu(count: usize, all_quick: bool) -> Vec<menu::Entry> {
    vec![
        if all_quick {
            menu::item("quick-access-remove", "Remove from Quick Access")
                .icon("menu-star-off")
                .into()
        } else {
            menu::item("quick-access-add", "Add to Quick Access")
                .icon("menu-star")
                .into()
        },
        menu::item("duplicate", crate::tf!("Duplicate {value0} Folders", count))
            .icon("menu-copy")
            .into(),
        menu::item("move-to", "Move to...")
            .icon("menu-folder-open")
            .into(),
        menu::item("auto-tags", "Set Auto Tags...")
            .icon("menu-auto-tags")
            .into(),
        content_sort(),
        menu::Entry::Separator,
        menu::item("delete", crate::tf!("Delete {value0} Folders", count))
            .icon("menu-trash")
            .danger(true)
            .into(),
    ]
}

/// Builds the "Move to..." submenu of a smart folder. It lists "Top Level"
/// and then every smart folder in `targets`.
fn smart_move_submenu(targets: &[(String, String)]) -> menu::Entry {
    let mut rows: Vec<menu::Entry> = vec![menu::item("move-under", "Top Level").value("").into()];
    rows.extend(targets.iter().map(|(label, node)| {
        menu::item("move-under", label.clone())
            .value(node.clone())
            .into()
    }));
    menu::submenu("Move to...", rows)
        .icon("menu-folder-open")
        .into()
}

/// Builds the context menu of a smart folder's row. A group has no rules,
/// so its menu has no rows that edit, refresh, sort or export, and no Quick
/// Access row.
fn smart_folder_menu(group: bool, node: NodeMenu) -> Vec<menu::Entry> {
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
        entries.push(quick_access_row(node.quick));
    }
    entries.extend([
        menu::item("rename", "Rename").icon("menu-rename").into(),
        smart_move_submenu(&node.move_targets),
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
        expand_row(true, node.expanded, node.has_children),
        expand_all_row(true, node.any_expanded, node.any_expandable),
        menu::Entry::Separator,
        change_icon(node.icon_picker),
        ui::color_picker::menu_row("color", node.color.as_deref()).into(),
        menu::Entry::Separator,
    ]);
    if !group {
        entries.push(export_submenu());
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

/// Builds the row that collapses or expands a folder's branch, or a smart
/// folder's branch when `smart` is true. The row is disabled when the node
/// has no children.
fn expand_row(smart: bool, expanded: bool, has_children: bool) -> menu::Entry {
    let (label, icon) = match (smart, expanded) {
        (false, true) => (t("Collapse Folder"), "menu-collapse"),
        (false, false) => (t("Expand Folder"), "menu-expand"),
        (true, true) => (t("Collapse Smart Folder"), "menu-collapse"),
        (true, false) => (t("Expand Smart Folder"), "menu-expand"),
    };
    menu::item("toggle-expand", label)
        .icon(icon)
        .disabled(!has_children)
        .into()
}

/// Builds the row that collapses or expands every branch of the Folders
/// section, or of the Smart Folders section when `smart` is true. The row
/// is disabled when no node of the section has children.
fn expand_all_row(smart: bool, any_expanded: bool, any_expandable: bool) -> menu::Entry {
    let (label, icon) = match (smart, any_expanded) {
        (false, true) => (t("Collapse All Folders"), "menu-collapse-all"),
        (false, false) => (t("Expand All Folders"), "menu-expand-all"),
        (true, true) => (t("Collapse All Smart Folders"), "menu-collapse-all"),
        (true, false) => (t("Expand All Smart Folders"), "menu-expand-all"),
    };
    menu::item("toggle-expand-all", label)
        .icon(icon)
        .disabled(!any_expandable)
        .into()
}

/// The state of several selected rows that decides which rows their context
/// menu shows.
struct BulkMenu {
    has_collapsed: bool,
    has_expanded: bool,
    /// Whether a selected row is in Quick Access. `any_not_quick` says
    /// whether a selected row is not in Quick Access.
    any_quick: bool,
    any_not_quick: bool,
    /// The smart folders under which the selected smart folders can be
    /// moved, each with its label and its node id.
    move_targets: Vec<(String, String)>,
}

/// Builds the context menu of several selected rows, which can be folders,
/// smart folders, or both.
fn bulk_menu(nodes: &[String], bulk: BulkMenu, cx: &App) -> Vec<menu::Entry> {
    let all_folders = nodes.iter().all(|id| id.starts_with("folder:"));
    let all_smart = nodes.iter().all(|id| id.starts_with("smart:"));
    let count = nodes.len();
    let mut entries: Vec<menu::Entry> = Vec::new();
    if bulk.any_not_quick {
        entries.push(
            menu::item("quick-access-add", "Add to Quick Access")
                .icon("menu-star")
                .into(),
        );
    }
    if bulk.any_quick {
        entries.push(
            menu::item("quick-access-remove", "Remove from Quick Access")
                .icon("menu-star-off")
                .into(),
        );
    }
    entries.push(
        menu::item(
            "duplicate",
            crate::tf!(
                "Duplicate {value0} item{value1}",
                count,
                crate::i18n::plural_s(count as i128)
            ),
        )
        .icon("menu-copy")
        .into(),
    );
    if all_folders {
        entries.push(
            menu::item("auto-tags", "Set Auto Tags...")
                .icon("menu-auto-tags")
                .shortcut_for("folder.autoTags", cx)
                .into(),
        );
    }
    entries.push(menu::Entry::Separator);
    if all_folders {
        entries.extend([
            menu::item("move-to", "Move to...")
                .icon("menu-folder-open")
                .into(),
            content_sort(),
        ]);
    } else if all_smart {
        entries.extend([
            smart_move_submenu(&bulk.move_targets),
            menu::item("sort-by-name", "Sort Results by Name")
                .icon("menu-sort")
                .into(),
        ]);
    }
    if all_folders || all_smart {
        entries.extend([
            menu::item("expand-selected", "Expand Selected")
                .icon("menu-expand")
                .disabled(!bulk.has_collapsed)
                .into(),
            menu::item("collapse-selected", "Collapse Selected")
                .icon("menu-collapse")
                .disabled(!bulk.has_expanded)
                .into(),
            menu::Entry::Separator,
            ui::color_picker::menu_row("color", None).into(),
            menu::Entry::Separator,
        ]);
    }
    entries.push(
        menu::item("delete", crate::tf!("Delete {value0} items", count))
            .icon("menu-trash")
            .danger(true)
            .into(),
    );
    entries
}

impl Sidebar {
    fn open_menu(
        &mut self,
        target: Target,
        entries: Vec<menu::Entry>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu_position = position;
        // The menu of a system row has no search field.
        let searchable = !matches!(target, Target::System);
        let menu = cx.new(|cx| Menu::new(entries, position, cx).search(searchable));
        let chosen = target.clone();
        // This assignment drops the subscriptions of the menu that this menu
        // replaces. Otherwise the release of that menu would clear
        // `context`, and the row this menu was opened on would lose its
        // highlight.
        self._menu = vec![
            cx.subscribe_in(
                &menu,
                window,
                move |this, menu, event: &menu::Event, window, cx| {
                    let menu::Event::Chosen(action) = event;
                    let value = menu.read(cx).chosen_value().map(|value| value.to_string());
                    this.choose(&chosen, action, value, window, cx);
                },
            ),
            cx.observe_release(&menu, |this, _, cx| {
                this.context = None;
                cx.notify();
            }),
        ];
        self.menu = Some(menu.downgrade());
        // A row is highlighted while its menu is open only when the menu
        // belongs to a single folder or smart folder.
        self.context = match target {
            Target::Node(node) => Some(node),
            _ => None,
        };
        cx.emit(ShowMenu(menu));
        cx.notify();
    }

    /// Opens the context menu of a system row. Recently Viewed and Trash
    /// have menus of their own. Every other system row opens the menu that
    /// shows and hides the system rows and the sections.
    pub(super) fn system_menu(
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
                // "Empty Trash" permanently deletes everything in the Trash
                // after the user confirms (`shell::delete`).
                menu::item("empty-trash", "Empty Trash")
                    .icon("menu-trash")
                    .danger(true)
                    .disabled(count == 0)
                    .into(),
                menu::item("restore-all", "Restore All")
                    .icon("menu-download")
                    .disabled(count == 0)
                    .into(),
            ],
            _ => visibility_menu(&Preferences::current(cx)),
        };
        self.open_menu(Target::System, entries, position, window, cx);
    }

    /// Creates the icon picker that a menu's "Change Icon" submenu holds.
    /// The picker marks `current` as chosen, and choosing an icon sets it on
    /// `nodes`.
    fn icon_picker(
        &self,
        nodes: Vec<String>,
        current: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyView {
        let sidebar = cx.entity().downgrade();
        cx.new(move |_| {
            ui::icon_picker::IconPicker::new(current, move |icon, _, cx| {
                let nodes = nodes.clone();
                let _ = sidebar.update(cx, |this, cx| this.apply_icon(nodes, icon, cx));
            })
        })
        .into()
    }

    /// Returns the label and the node id of every smart folder under which
    /// the smart folders `moving` can be moved.
    fn smart_move_targets(&self, moving: &[String], cx: &App) -> Vec<(String, String)> {
        let entries = &self.session.read(cx).nav;
        let nodes = tree::nodes(entries, "smart:");
        let moving = tree::top_level(&nodes, moving);
        tree::move_targets(&nodes, &moving)
            .into_iter()
            .filter_map(|id| {
                let label = entries.iter().find(|entry| entry.id() == id)?.label.clone();
                Some((label, id))
            })
            .collect()
    }

    /// Opens the context menu of the row of a folder or smart folder. When
    /// the row is one of several selected rows, the menu acts on all of
    /// them. Otherwise the selection is cleared and the menu acts on this
    /// row.
    pub(super) fn node_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.session.read(cx).nav.clone();
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        let node = ids[index].clone();
        let quick = self.quick_access(cx);
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
            let bulk = BulkMenu {
                has_collapsed: folding.clone().any(|id| self.collapsed.contains(id)),
                has_expanded: folding.clone().any(|id| !self.collapsed.contains(id)),
                any_quick: nodes.iter().any(|id| quick.contains(id)),
                any_not_quick: nodes.iter().any(|id| !quick.contains(id)),
                move_targets: self.smart_move_targets(&nodes, cx),
            };
            let menu = bulk_menu(&nodes, bulk, cx);
            self.open_menu(Target::Bulk(nodes), menu, position, window, cx);
            return;
        }
        self.selection.clear();
        let Some(section) = Section::of_node(&node) else {
            return;
        };
        let all = expandable(&entries, &ids, section);
        let entry = &entries[index];
        let details = NodeMenu {
            expanded: !self.collapsed.contains(&node),
            has_children: all.contains(&node),
            any_expanded: all.iter().any(|id| !self.collapsed.contains(id)),
            any_expandable: !all.is_empty(),
            quick: quick.contains(&node),
            watched: entry.watched,
            color: entry.tint.map(|tint| format!("#{tint:06X}")),
            icon_picker: self.icon_picker(vec![node.clone()], entry.icon.clone(), cx),
            move_targets: match section {
                Section::SmartFolders => self.smart_move_targets(std::slice::from_ref(&node), cx),
                Section::Folders => Vec::new(),
            },
        };
        let menu = match section {
            Section::Folders => folder_menu(details, cx),
            Section::SmartFolders => smart_folder_menu(entry.group, details),
        };
        self.open_menu(Target::Node(node), menu, position, window, cx);
    }

    /// Opens the context menu of a subfolder tile in the grid. When the tile
    /// is one of several selected tiles, the menu acts on all of them.
    /// "Rename" renames the tile in place in `grid`.
    pub fn tile_menu(
        &mut self,
        node: String,
        selected: Vec<String>,
        position: Point<Pixels>,
        grid: WeakEntity<crate::grid::GridView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tile_grid = Some(grid);
        let quick = self.quick_access(cx);
        let folders = if selected.contains(&node) {
            selected
        } else {
            vec![node.clone()]
        };
        if folders.len() > 1 {
            let all_quick = folders.iter().all(|id| quick.contains(id));
            let entries = tile_bulk_menu(folders.len(), all_quick);
            self.open_menu(Target::Tiles(folders), entries, position, window, cx);
            return;
        }
        let Some(entry) = self
            .session
            .read(cx)
            .nav
            .iter()
            .find(|entry| entry.id() == node)
            .cloned()
        else {
            return;
        };
        let picker = self.icon_picker(vec![node.clone()], entry.icon.clone(), cx);
        let color = entry.tint.map(|tint| format!("#{tint:06X}"));
        let entries = tile_menu(
            quick.contains(&node),
            entry.watched,
            color.as_deref(),
            picker,
            cx,
        );
        self.open_menu(Target::Tile(node), entries, position, window, cx);
    }

    /// Opens the menu of a section header. The add button of the Smart
    /// Folders section opens the same menu.
    pub(super) fn section_menu(
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
                    .shortcut_for("file.newFolder", cx)
                    .into(),
                menu::item("import-folder", "Import Folder...")
                    .icon("menu-folder-plus")
                    .disabled(crate::imports::blocked(cx).is_some())
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
}
