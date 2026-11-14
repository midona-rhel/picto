//! The sidebar's context menus: what each row, selection and section
//! header offers, and the opening of those menus.
use super::{
    ALWAYS_SHOWN, Preferences, SYSTEM_ROWS, Section, ShowMenu, Sidebar, Target, expandable, tree,
};
use crate::backend::Navigation;
use crate::i18n::t;
use crate::ui;
use crate::ui::menu::{self, Menu};
use gpui::*;
use picto_library::model::ContentSortField;
use std::collections::HashSet;

// ── Menus ───────────────────────────────────────────────────────────

/// `sidebarVisibilityEntries`: which system rows and sections show, each
/// with a check while it is shown. Toggling a row keeps the menu open.
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

/// What a folder's or smart folder's menu shows besides its fixed rows.
struct NodeMenu {
    expanded: bool,
    has_children: bool,
    any_expanded: bool,
    any_expandable: bool,
    /// The node is in Quick Access.
    quick: bool,
    /// The folder watches a directory.
    watched: bool,
    /// The colour the swatch row shows chosen.
    color: Option<String>,
    /// The icon picker the "Change Icon" submenu holds.
    icon_picker: AnyView,
    /// Where a smart folder can move: its label and node id, after Top Level.
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

/// "Change Icon": the compact icon picker in a submenu of its own.
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

/// `contentSortSubmenu`: the field a folder's items are sorted by.
fn content_sort() -> menu::Entry {
    menu::submenu(
        t("Sort by"),
        CONTENT_SORT
            .iter()
            .map(|(value, label, _)| menu::item("sort-contents", *label).value(*value).into())
            .collect(),
    )
    .icon("menu-sort")
    .into()
}

/// The content sort fields, with the value a menu row carries.
// i18n: translated where drawn (menu rows)
const CONTENT_SORT: [(&str, &str, ContentSortField); 6] = [
    ("name", "Name", ContentSortField::Name),
    ("imported_at", "Import Date", ContentSortField::ImportedAt),
    ("created_at", "Date Created", ContentSortField::CreatedAt),
    ("modified_at", "Date Modified", ContentSortField::ModifiedAt),
    ("size", "Size", ContentSortField::Size),
    ("notes", "Notes", ContentSortField::Notes),
];

pub(super) fn content_sort_field(value: &str) -> Option<ContentSortField> {
    CONTENT_SORT
        .iter()
        .find(|(name, ..)| *name == value)
        .map(|(.., field)| *field)
}

/// `buildExportContextEntry`: the export submenu of a folder or smart
/// folder.
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

/// A menu row's padding inside its panel, which a view in a submenu fills.
const MENU_PADDING: f32 = 4.;

/// `openFolderMenu`.
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

/// "Sort Folders": a folder's children, or its whole tree, by name.
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

/// `buildFolderContextMenu`: a subfolder tile's menu in the grid, the folder
/// operations every surface but the sidebar shows.
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

/// `buildBulkFolderContextMenu`: several subfolder tiles selected.
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

/// A smart folder's "Move to..." submenu: Top Level, then every smart folder
/// it can move under.
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

/// `openSmartFolderMenu`. A group has no rules, so it has nothing to edit,
/// refresh, sort or export.
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

/// Collapse or Expand a folder (`smart`: a smart folder).
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

/// Collapse or Expand every folder of a section (`smart`: Smart Folders).
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

/// What the selection's menu shows besides its fixed rows.
struct BulkMenu {
    has_collapsed: bool,
    has_expanded: bool,
    /// Selected rows in and out of Quick Access.
    any_quick: bool,
    any_not_quick: bool,
    /// Where selected smart folders can move.
    move_targets: Vec<(String, String)>,
}

/// `openBulkMenu`: several folders, several smart folders, or a mix.
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
        // The system rows' menus never offer a search (`showSearch: false`).
        let searchable = !matches!(target, Target::System);
        let menu = cx.new(|cx| Menu::new(entries, position, cx).search(searchable));
        let chosen = target.clone();
        // The subscriptions of a menu replaced by this one go first, so its
        // release does not clear this menu's highlight.
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
                // Deletes everything in the Trash for good, after a
                // confirmation. The files are the disposable copy's.
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

    /// The icon picker a menu's "Change Icon" holds, for these nodes.
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

    /// The labels and ids of the nodes a smart-folder selection can move under.
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

    /// `handleFolderContextMenu` and `handleSmartFolderContextMenu`: the
    /// selection's menu when the row is part of a larger selection,
    /// otherwise the row's own, which clears the selection.
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

    /// A subfolder tile's menu (`onFolderContextMenu`), or the menu of the
    /// tiles selected with it. The grid that asked does its renames.
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

    /// The section header's menu, and the Smart Folders add button's.
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
