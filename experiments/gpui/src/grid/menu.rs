//! The context menus for items, which production builds in
//! `gridContextMenu.tsx`. They open on a right-click on a tile in the grid,
//! on empty space in the grid, in the viewer, and on the preview in the
//! inspector.
//!
//! The functions in this file build a menu's rows from a [`Description`] of
//! what was right-clicked, and they have no other input and no side effect.
//! The description holds the selection, the scope that the grid shows, and
//! which actions the caller can perform (in production these are optional
//! handler functions). When the caller cannot perform a row's action, the
//! row is shown disabled or is left out, whichever production does for that
//! row. The shell turns the rows into a list of [`menu::Entry`] and performs
//! the [`Action`] of the row that the user chooses.
use crate::i18n::format::grouped;
use crate::i18n::t;
pub use crate::search_by_image::{ENGINES, Engine};
use crate::ui::menu;
use picto_library::ContentSortField;

/// Whether the menu was opened in the grid or in the viewer
/// (`GridMenuContext.surface` in production).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Grid,
    Viewer,
}

/// The kind of place whose items the grid shows: one of the system scopes,
/// a folder, or a smart folder (`scopeKind` in production).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    System,
    Folder,
    SmartFolder,
}

/// Whether the items that the menu acts on are in the Inbox, are active, or
/// are in the Trash (`statusFilter` in production). This is the same type
/// that the application menu uses.
pub use crate::app_menu::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Media,
    Collection,
}

/// How the menu offers "Open With Other" (`openWithOptions` and
/// `openWithPending` in production).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum OpenWith {
    #[default]
    None,
    /// The applications that can open the file are still being looked up.
    /// The menu shows a submenu with one disabled row, "Loading
    /// applications...". Production is in this state on macOS until it has
    /// the list of applications.
    Pending,
    /// The applications that can open the file, which macOS provides. The
    /// menu shows a submenu with one row for each application, or no "Open
    /// With Other" row at all when the list is empty.
    Applications(Vec<crate::desktop::Application>),
    /// The application chooser that Windows itself provides. The menu shows
    /// one row, "Open With Other...", which opens the chooser.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Chooser,
}

// Whether the "Open With Other..." row is enabled. The row opens Windows' own
// application chooser for the file (`desktop::open_with_chooser`, which runs
// `rundll32.exe shell32.dll,OpenAs_RunDLL <path>`). Windows is the only
// system that offers "Open With Other..." as a chooser, so the row is
// disabled on every other system.
const OPEN_WITH_CHOOSER_BUILT: bool = cfg!(target_os = "windows");

/// The fields that "Sort by" can sort a folder's contents by
/// (`CONTENT_SORT_OPTIONS` in production). Choosing one rewrites the folder's
/// own order of its items, so that they are sorted by that field. Each entry
/// holds the field, the label of its menu row, and the string that production
/// uses for the field, which a menu row carries as its value.
// i18n: translated where drawn (menu rows)
pub const CONTENT_SORT: [(ContentSortField, &str, &str); 6] = [
    (ContentSortField::Name, "Name", "name"),
    (ContentSortField::ImportedAt, "Import Date", "imported_at"),
    (ContentSortField::CreatedAt, "Date Created", "created_at"),
    (ContentSortField::ModifiedAt, "Date Modified", "modified_at"),
    (ContentSortField::Size, "Size", "size"),
    (ContentSortField::Notes, "Notes", "notes"),
];

/// One of the two panels of view controls that production draws inside the
/// menu as custom entries (`buildContextMenuViewEntries`). This file only
/// marks where a panel goes. The shell supplies the entry that draws it
/// when it calls [`entries`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// Production's `ViewPanel`, which holds the layout and the sort
    /// controls. It is placed directly in the menu.
    Layout,
    /// Production's `DisplayPanel`, which holds the display switches. It is
    /// placed inside the "Display" submenu.
    Display,
}

/// Which actions the caller can perform. Each field stands for one of the
/// optional handler functions that production passes to its menu, and is
/// true when the caller has that handler. When the field for a row is false,
/// the row is disabled or left out, whichever production does for that row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Handlers {
    pub open_default: bool,
    pub open_with_application: bool,
    pub open_with_chooser: bool,
    pub reveal: bool,
    pub open_new_window: bool,
    pub copy_file: bool,
    pub copy_path: bool,
    pub copy_name: bool,
    pub copy_link: bool,
    pub copy_selection: bool,
    pub copy_selection_paths: bool,
    pub copy_selection_names: bool,
    pub copy_selection_links: bool,
    pub move_to_trash: bool,
    pub restore: bool,
    pub permanent_delete: bool,
    pub add_to_folder: bool,
    pub add_to_last_folder: bool,
    pub remove_from_folder: bool,
    pub tag_select: bool,
    pub ai_tagger: bool,
    pub accept: bool,
    pub reject: bool,
    pub rename: bool,
    pub batch_rename: bool,
    pub regenerate: bool,
    pub library_cover: bool,
    pub folder_cover: bool,
    pub copy_tags: bool,
    pub paste_tags: bool,
    pub new_folder_with_selection: bool,
    pub find_media_matches: bool,
    pub search_by_image: bool,
    pub set_rating: bool,
    pub export: bool,
    pub export_originals: bool,
    pub export_pack: bool,
    pub organize_group: bool,
    pub edit_group: bool,
    pub ungroup: bool,
    pub new_folder: bool,
    pub new_smart_folder: bool,
    pub import_files: bool,
    pub import_folder: bool,
    pub paste_import: bool,
    pub sort_contents: bool,
    pub toggle_grayscale: bool,
}

/// What was right-clicked and where (`GridMenuContext` in production).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Description {
    pub surface: Surface,
    pub selection_count: usize,
    /// True when the selection is everything that a query or a range
    /// matches, and not a list of individual items.
    pub query_selection: bool,
    /// Whether the "Auto Tag" row is enabled. When this is `None`, the row
    /// is enabled if the caller has the `ai_tagger` handler.
    pub ai_tag_enabled: Option<bool>,
    pub single: bool,
    pub single_hash: Option<String>,
    pub single_item_id: Option<i64>,
    pub single_kind: Option<Kind>,
    pub single_name: Option<String>,
    pub single_mime: Option<String>,
    pub contains_group: bool,
    /// How many collections are in the selection. It is `None` when the
    /// caller does not give a number, and `Some(None)` while the collections
    /// are still being counted.
    pub collection_count: Option<Option<usize>>,
    pub mixed: bool,
    pub scope: Option<ScopeKind>,
    pub status: Option<Status>,
    pub grayscale: bool,
    pub last_folder_name: Option<String>,
    pub clipboard_tags: bool,
    pub open_with: OpenWith,
    /// True while another import is running. The import rows are then
    /// disabled, because a second import cannot start.
    pub import_blocked: bool,
    pub handlers: Handlers,
    /// The fields that the "Sort by" submenu offers (`sortFields` in
    /// production). `None` means every field. A collection's contents can be
    /// sorted only by name or by size.
    pub sort_fields: Option<Vec<ContentSortField>>,
}

/// The action that a row performs when the user chooses it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    OpenDefault,
    OpenWithOther,
    /// Opens the file with one of the applications in the "Open With Other"
    /// submenu. The row's `value` holds the path of the application.
    OpenWithApplication,
    LoadingApplications,
    Reveal,
    OpenInNewWindow,
    CopyFile,
    CopyPath,
    CopyName,
    CopyLink,
    CopySelection,
    CopySelectionPaths,
    CopySelectionNames,
    CopySelectionLinks,
    Accept,
    Reject,
    MoveToTrash,
    Restore,
    DeletePermanently,
    AddToLastFolder,
    AddToFolder,
    NewFolderWithSelection,
    Group,
    EditGroup,
    Ungroup,
    Rename,
    BatchRename,
    AddTags,
    AutoTag,
    CopyTags,
    PasteTags,
    Rate(u8),
    ExportOriginals,
    ExportAs,
    ExportPictoPack,
    SearchByImage(Engine),
    SetLibraryCover,
    FindMediaMatches,
    SetFolderCover,
    RegenerateThumbnails,
    RemoveFromFolder,
    SortContents(ContentSortField),
    Grayscale,
    NewFolder,
    NewSmartFolder,
    ImportFiles,
    ImportFolder,
    PasteImport,
    FlashPlay,
    FlashStop,
    FlashMute,
    SetAsThumbnail,
    /// Removes the selected members from their collection (`detachMembers`
    /// in production).
    RemoveFromGroup,
}

impl Action {
    /// The string by which the shared menu identifies the row of this
    /// action. Every action has a different string, so the shell can find
    /// the action that the user chose, and can check or uncheck one row
    /// while the menu is open.
    pub fn key(self) -> &'static str {
        use Action::*;
        match self {
            OpenDefault => "open-default",
            OpenWithOther => "open-with-other",
            OpenWithApplication => "open-with-application",
            LoadingApplications => "loading-applications",
            Reveal => "reveal",
            OpenInNewWindow => "open-new-window",
            CopyFile => "copy-file",
            CopyPath => "copy-path",
            CopyName => "copy-name",
            CopyLink => "copy-link",
            CopySelection => "copy-selection",
            CopySelectionPaths => "copy-selection-paths",
            CopySelectionNames => "copy-selection-names",
            CopySelectionLinks => "copy-selection-links",
            Accept => "accept",
            Reject => "reject",
            MoveToTrash => "trash",
            Restore => "restore",
            DeletePermanently => "delete-permanently",
            AddToLastFolder => "add-to-last-folder",
            AddToFolder => "add-to-folder",
            NewFolderWithSelection => "new-folder-with-selection",
            Group => "group",
            EditGroup => "edit-group",
            Ungroup => "ungroup",
            Rename => "rename",
            BatchRename => "batch-rename",
            AddTags => "add-tags",
            AutoTag => "auto-tag",
            CopyTags => "copy-tags",
            PasteTags => "paste-tags",
            Rate(0) => "rate-0",
            Rate(1) => "rate-1",
            Rate(2) => "rate-2",
            Rate(3) => "rate-3",
            Rate(4) => "rate-4",
            Rate(_) => "rate-5",
            ExportOriginals => "export-originals",
            ExportAs => "export-as",
            ExportPictoPack => "export-picto-pack",
            SearchByImage(Engine::TinEye) => "search-tineye",
            SearchByImage(Engine::SauceNao) => "search-saucenao",
            SearchByImage(Engine::Yandex) => "search-yandex",
            SearchByImage(Engine::Bing) => "search-bing",
            SetLibraryCover => "set-library-cover",
            FindMediaMatches => "find-media-matches",
            SetFolderCover => "set-folder-cover",
            RegenerateThumbnails => "regenerate-thumbnails",
            RemoveFromFolder => "remove-from-folder",
            SortContents(ContentSortField::Name) => "sort-contents-name",
            SortContents(ContentSortField::ImportedAt) => "sort-contents-imported",
            SortContents(ContentSortField::CreatedAt) => "sort-contents-created",
            SortContents(ContentSortField::ModifiedAt) => "sort-contents-modified",
            SortContents(ContentSortField::Size) => "sort-contents-size",
            SortContents(ContentSortField::Notes) => "sort-contents-notes",
            Grayscale => "grayscale",
            NewFolder => "new-folder",
            NewSmartFolder => "new-smart-folder",
            ImportFiles => "import-files",
            ImportFolder => "import-folder",
            PasteImport => "paste-import",
            FlashPlay => "flash-play",
            FlashStop => "flash-stop",
            FlashMute => "flash-mute",
            SetAsThumbnail => "set-as-thumbnail",
            RemoveFromGroup => "remove-from-group",
        }
    }
}

/// One row of a menu as this file builds it, before [`entries`] turns it
/// into a [`menu::Entry`].
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Item(ItemRow),
    Separator,
    Submenu {
        label: String,
        icon: Option<&'static str>,
        children: Vec<Row>,
    },
    Panel(Panel),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ItemRow {
    pub action: Action,
    pub label: String,
    pub icon: Option<&'static str>,
    pub shortcut: Option<String>,
    pub danger: bool,
    pub disabled: bool,
    pub checked: Option<bool>,
    pub keep_open: bool,
    /// A value that distinguishes rows that have the same action. For a row
    /// in the "Open With Other" submenu, it is the path of the application.
    pub value: Option<String>,
    /// An image that is drawn in place of the row's icon. For a row in the
    /// "Open With Other" submenu, it is the icon of the application.
    pub image: Option<std::sync::Arc<gpui::Image>>,
}

/// Makes a row with the label `label` that performs `action`, as
/// `item(label, { action })` does in production. The row is disabled when
/// `handled` is false, which means that the caller has no handler for it.
fn item(action: Action, label: impl Into<String>, handled: bool) -> ItemRow {
    ItemRow {
        action,
        label: label.into(),
        icon: None,
        shortcut: None,
        danger: false,
        disabled: !handled,
        checked: None,
        keep_open: false,
        value: None,
        image: None,
    }
}

impl ItemRow {
    fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    fn shortcut(mut self, shortcut: Option<String>) -> Self {
        self.shortcut = shortcut;
        self
    }
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
    fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }
}

impl From<ItemRow> for Row {
    fn from(item: ItemRow) -> Self {
        Row::Item(item)
    }
}

fn submenu(label: impl Into<String>, icon: Option<&'static str>, children: Vec<Row>) -> Row {
    Row::Submenu {
        label: label.into(),
        icon,
        children,
    }
}

/// Builds rows. `hint` returns the text of the keyboard shortcut with the
/// given id, or `None` when there is none to show.
struct Builder<'a> {
    hint: &'a dyn Fn(&str) -> Option<String>,
}

impl Builder<'_> {
    fn hint(&self, id: &str) -> Option<String> {
        (self.hint)(id)
    }

    /// Adds the rows that open the file (`buildEntityOpenContextEntries` in
    /// production): "Open with Default App", "Open With Other", the row
    /// that shows the file in the system's file manager, and "Open in New
    /// Window". Each row is added only when the caller has its handler.
    fn open_entries(&self, d: &Description, rows: &mut Vec<Row>) {
        let h = &d.handlers;
        if h.open_default {
            rows.push(
                item(Action::OpenDefault, "Open with Default App", true)
                    .icon("menu-external-link")
                    .shortcut(self.hint("file.openDefaultApp"))
                    .into(),
            );
        }
        match &d.open_with {
            OpenWith::Pending => rows.push(submenu(
                t("Open With Other"),
                Some("menu-apps"),
                vec![
                    item(
                        Action::LoadingApplications,
                        t("Loading applications..."),
                        false,
                    )
                    .into(),
                ],
            )),
            OpenWith::Applications(applications)
                if !applications.is_empty() && h.open_with_application =>
            {
                rows.push(submenu(
                    t("Open With Other"),
                    Some("menu-apps"),
                    applications.iter().map(application_row).collect(),
                ))
            }
            OpenWith::Chooser if h.open_with_chooser => rows.push(
                item(
                    Action::OpenWithOther,
                    t("Open With Other..."),
                    OPEN_WITH_CHOOSER_BUILT,
                )
                .icon("menu-apps")
                .shortcut(self.hint("file.openOtherApp"))
                .into(),
            ),
            _ => {}
        }
        if h.reveal {
            let label = crate::app_menu::Platform::CURRENT.reveal_title();
            let icon = if cfg!(target_os = "macos") {
                "menu-brand-finder"
            } else {
                "menu-folder-search"
            };
            rows.push(
                item(Action::Reveal, label, true)
                    .icon(icon)
                    .shortcut(self.hint("file.revealInFolder"))
                    .into(),
            );
        }
        if h.open_new_window {
            rows.push(self.open_new_window());
        }
    }

    fn open_new_window(&self) -> Row {
        item(Action::OpenInNewWindow, "Open in New Window", true)
            .icon("menu-app-window")
            .shortcut(self.hint("file.openNewWindow"))
            .into()
    }

    /// Adds the rows that copy one item (`buildEntityCopyContextEntries` in
    /// production): "Copy", "Copy File Path", "Copy Name" and "Copy as
    /// Link". Each row is added only when the caller has its handler.
    fn copy_entries(&self, d: &Description, rows: &mut Vec<Row>) {
        let h = &d.handlers;
        if h.copy_file {
            rows.push(
                item(Action::CopyFile, "Copy", true)
                    .icon("menu-copy")
                    .shortcut(self.hint("edit.copy"))
                    .into(),
            );
        }
        if h.copy_path {
            rows.push(
                item(Action::CopyPath, "Copy File Path", true)
                    .icon("menu-clipboard-copy")
                    .shortcut(self.hint("edit.copyFilePath"))
                    .into(),
            );
        }
        if d.single_name.is_some() && h.copy_name {
            rows.push(
                item(Action::CopyName, "Copy Name", true)
                    .icon("menu-clipboard-copy")
                    .into(),
            );
        }
        if d.single_mime.is_some() && h.copy_link {
            rows.push(
                item(Action::CopyLink, "Copy as Link", true)
                    .icon("menu-link")
                    .into(),
            );
        }
    }

    /// Adds the view controls (`buildContextMenuViewEntries` in
    /// production): the panel with the layout and the sort directly in the
    /// menu, then a separator, then the "Display" submenu, which holds the
    /// panel with the display switches.
    fn view_entries(&self, rows: &mut Vec<Row>) {
        rows.push(Row::Panel(Panel::Layout));
        rows.push(Row::Separator);
        rows.push(submenu(
            t("Display"),
            Some("menu-adjustments-rotated"),
            vec![Row::Panel(Panel::Display)],
        ));
    }
}

/// Builds the menu for a right-click on a tile or in the viewer
/// (`buildTileContextMenu` in production).
pub fn tile_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let b = Builder { hint };
    let mut rows: Vec<Row> = Vec::new();
    let mut more: Vec<Row> = Vec::new();

    let h = &d.handlers;
    // A mixed selection, which is a selection that has folders in it, gets
    // a menu with only "Move to Trash".
    if d.mixed {
        if h.move_to_trash {
            rows.push(
                item(Action::MoveToTrash, "Move to Trash", true)
                    .icon("menu-trash")
                    .into(),
            );
        }
        return rows;
    }

    inbox_section(d, &b, &mut rows);
    open_section(d, &b, &mut rows, &mut more);
    view_section(d, &b, &mut rows);
    organize_section(d, &b, &mut rows);
    rename_section(d, &b, &mut rows, &mut more);
    copy_section(d, &b, &mut rows);
    tag_section(d, &b, &mut rows);
    share_section(d, &mut rows);
    end_section(d, &b, &mut rows, &mut more);
    rows
}

/// Adds "Accept", "Reject" and a separator as the first rows of the menu
/// when the items are in the Inbox and something is selected.
fn inbox_section(d: &Description, b: &Builder, rows: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    let has_selection = count > 0;
    let several = |one: &str, many: String| several(count, one, many);
    if d.status == Some(Status::Inbox) && has_selection {
        rows.push(
            item(
                Action::Accept,
                several(t("Accept"), format!("Accept {count} Items")),
                h.accept,
            )
            .icon("menu-arrow-back-up")
            .shortcut(b.hint("inbox.accept"))
            .into(),
        );
        rows.push(
            item(
                Action::Reject,
                several(t("Reject"), format!("Reject {count} Items")),
                h.reject,
            )
            .icon("menu-trash")
            .shortcut(b.hint("inbox.reject"))
            .into(),
        );
        rows.push(Row::Separator);
    }
}

/// For a single item, adds the rows that open it, followed by a separator. A
/// collection gets only "Open in New Window". This function also adds "Set
/// as Library Cover" and "Find Items with This Media" to `more`, which holds
/// the rows of the "More" submenu.
fn open_section(d: &Description, b: &Builder, rows: &mut Vec<Row>, more: &mut Vec<Row>) {
    let h = &d.handlers;
    let can_set_library_cover = d.single
        && d.single_hash.is_some()
        && d.single_kind == Some(Kind::Media)
        && h.library_cover;
    if d.single {
        let mut open = Vec::new();
        if d.single_kind == Some(Kind::Collection) {
            if h.open_new_window {
                open.push(b.open_new_window());
            }
        } else if d.single_hash.is_some() {
            b.open_entries(d, &mut open);
        }
        let any = !open.is_empty();
        rows.extend(open);
        if any {
            rows.push(Row::Separator);
        }
    }

    if can_set_library_cover {
        more.push(library_cover_row());
    }
    if d.single && d.single_item_id.is_some() && h.find_media_matches {
        more.push(
            item(Action::FindMediaMatches, "Find Items with This Media", true)
                .icon("menu-search")
                .into(),
        );
    }
}

/// When the menu was not opened in the viewer, adds the "Sort by" submenu,
/// the view panels and the "Grayscale" row.
fn view_section(d: &Description, b: &Builder, rows: &mut Vec<Row>) {
    let h = &d.handlers;
    let viewer = d.surface == Surface::Viewer;
    if !viewer {
        if h.sort_contents {
            rows.push(content_sort(d.sort_fields.as_deref()));
            rows.push(Row::Separator);
        }
        b.view_entries(rows);
        rows.push(
            item(Action::Grayscale, "Grayscale", h.toggle_grayscale)
                .icon("menu-contrast")
                .shortcut(b.hint("view.grayscale"))
                .checked(d.grayscale)
                .keep_open()
                .into(),
        );
        rows.push(Row::Separator);
    }
}

/// When something is selected, adds the rows that put the selection into a
/// folder ("Add to Folder" and the rows next to it), and the rows that group
/// the selection into a collection, edit a collection, or ungroup
/// collections ("Group...", "Edit Group" and "Ungroup...").
fn organize_section(d: &Description, b: &Builder, rows: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    let has_selection = count > 0;
    if has_selection {
        if h.add_to_last_folder {
            let label = match &d.last_folder_name {
                Some(name) => crate::tf!("Add to “{value0}”", name),
                None => t("Add to Last Used Folder").to_owned(),
            };
            rows.push(
                item(Action::AddToLastFolder, label, true)
                    .icon("menu-folder-plus")
                    .shortcut(b.hint("file.addToLastFolder"))
                    .into(),
            );
        }
        rows.push(
            item(Action::AddToFolder, "Add to Folder", h.add_to_folder)
                .icon("menu-folder-plus")
                .shortcut(b.hint("file.addToFolder"))
                .into(),
        );
        if h.new_folder_with_selection {
            rows.push(
                item(
                    Action::NewFolderWithSelection,
                    t("New Folder with Selection"),
                    true,
                )
                .icon("menu-folder-new-selection")
                .into(),
            );
        }
        if count > 1 && h.organize_group {
            // Grouping a selection that contains collections merges them
            // into one collection, and the row then says "Merge into One
            // Group...". While the collections in the selection are still
            // being counted, the selection may contain some, so the row
            // says the same.
            let may_merge = d.contains_group
                || matches!(d.collection_count, Some(None))
                || matches!(d.collection_count, Some(Some(n)) if n > 0);
            rows.push(group_row(may_merge));
        }
        if d.single && d.single_kind == Some(Kind::Collection) {
            rows.push(
                item(Action::EditGroup, "Edit Group", h.edit_group)
                    .icon("menu-group-edit")
                    .into(),
            );
        }
        let collections = match d.collection_count {
            None => Some(usize::from(
                d.single && d.single_kind == Some(Kind::Collection),
            )),
            Some(counted) => counted,
        };
        if collections.is_none_or(|n| n > 0) && h.ungroup {
            rows.push(ungroup_row(collections));
        }
        rows.push(Row::Separator);
    }
}

/// Adds "Rename" for a single item, and "Batch Rename N Items..." for a
/// selection of more than one item. For a single item in a folder, it also
/// adds "Set as Folder Cover" to `more`, which holds the rows of the "More"
/// submenu.
fn rename_section(d: &Description, b: &Builder, rows: &mut Vec<Row>, more: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    if d.single && h.rename {
        rows.push(
            item(Action::Rename, "Rename", true)
                .icon("menu-rename")
                .shortcut(b.hint("edit.rename"))
                .into(),
        );
        rows.push(Row::Separator);
    }
    if d.single && d.scope == Some(ScopeKind::Folder) && h.folder_cover {
        more.push(
            item(Action::SetFolderCover, "Set as Folder Cover", true)
                .icon("menu-photo")
                .into(),
        );
    }
    if count > 1 && !d.query_selection && h.batch_rename {
        rows.push(
            item(
                Action::BatchRename,
                crate::tf!("Batch Rename {value0} Items...", count),
                true,
            )
            .icon("menu-rename")
            .shortcut(b.hint("edit.rename"))
            .into(),
        );
        rows.push(Row::Separator);
    }
}

/// Adds the rows that copy to the clipboard. A single item that is not a
/// collection gets the rows for one item. Any other selection of listed
/// items gets the rows that copy the whole selection.
fn copy_section(d: &Description, b: &Builder, rows: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    let has_selection = count > 0;
    if d.single && d.single_hash.is_some() && d.single_kind != Some(Kind::Collection) {
        b.copy_entries(d, rows);
        rows.push(Row::Separator);
    } else if has_selection && !d.query_selection && h.copy_selection {
        rows.push(
            item(Action::CopySelection, "Copy", true)
                .icon("menu-copy")
                .shortcut(b.hint("edit.copy"))
                .into(),
        );
        if h.copy_selection_paths {
            rows.push(
                item(Action::CopySelectionPaths, "Copy File Paths", true)
                    .icon("menu-clipboard-copy")
                    .shortcut(b.hint("edit.copyFilePath"))
                    .into(),
            );
        }
        if h.copy_selection_names {
            rows.push(
                item(
                    Action::CopySelectionNames,
                    if count == 1 {
                        t("Copy Name")
                    } else {
                        t("Copy Names")
                    },
                    true,
                )
                .icon("menu-clipboard-copy")
                .into(),
            );
        }
        if h.copy_selection_links {
            rows.push(
                item(Action::CopySelectionLinks, "Copy as Links", true)
                    .icon("menu-link")
                    .into(),
            );
        }
        rows.push(Row::Separator);
    }
}

/// When something is selected, adds the tag rows: "Add Tags", "Auto Tag",
/// "Copy Tags" and "Paste Tags".
fn tag_section(d: &Description, b: &Builder, rows: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    let has_selection = count > 0;
    let several = |one: &str, many: String| several(count, one, many);
    let ai_tag_enabled = d.ai_tag_enabled.unwrap_or(h.ai_tagger);
    if has_selection {
        rows.push(
            item(Action::AddTags, "Add Tags", h.tag_select)
                .icon("menu-bookmark")
                .shortcut(b.hint("organize.addTag"))
                .into(),
        );
        rows.push(
            item(
                Action::AutoTag,
                several(t("Auto Tag"), crate::tf!("Auto Tag {value0} Images", count)),
                h.ai_tagger,
            )
            .icon("menu-auto-tag")
            .shortcut(b.hint("organize.autoTag"))
            .disabled(!ai_tag_enabled)
            .into(),
        );
        rows.push(
            item(
                Action::CopyTags,
                if d.single {
                    t("Copy Tags")
                } else {
                    t("Copy Shared Tags")
                },
                h.copy_tags,
            )
            .icon("menu-bookmarks")
            .shortcut(b.hint("edit.copyTags"))
            .into(),
        );
        rows.push(
            item(Action::PasteTags, "Paste Tags", h.paste_tags)
                .icon("menu-paste-tags")
                .shortcut(b.hint("edit.pasteTags"))
                .disabled(!d.clipboard_tags)
                .into(),
        );
        rows.push(Row::Separator);
    }
}

/// Adds the "Set Rating" submenu, the "Export" submenu and, for a single
/// image, the "Search by Image" submenu, which has one row for each search
/// engine.
fn share_section(d: &Description, rows: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    let has_selection = count > 0;
    if has_selection && h.set_rating {
        rows.push(submenu(
            t("Set Rating"),
            Some("menu-star"),
            (0..=5u8)
                .map(|stars| {
                    let label = match stars {
                        0 => t("No Rating").to_owned(),
                        _ => "\u{2605}".repeat(stars as usize),
                    };
                    item(Action::Rate(stars), label, true).into()
                })
                .collect(),
        ));
    }

    if has_selection && h.export {
        rows.push(export_row(h));
        rows.push(Row::Separator);
    }

    if d.single
        && d.single_hash.is_some()
        && d.single_mime
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        && h.search_by_image
    {
        rows.push(submenu(
            t("Search by Image"),
            Some("menu-search"),
            ENGINES
                .iter()
                .map(|(engine, label)| item(Action::SearchByImage(*engine), *label, true).into())
                .collect(),
        ));
        rows.push(Row::Separator);
    }
}

/// Adds the last rows of the menu: the "More" submenu, then "Remove from
/// Folder" when the grid shows a folder, and then "Move to Trash". When the
/// items are in the Trash, "Restore" and "Delete Permanently" take the place
/// of "Move to Trash". Only "Delete Permanently" is marked as dangerous.
fn end_section(d: &Description, b: &Builder, rows: &mut Vec<Row>, more: &mut Vec<Row>) {
    let h = &d.handlers;
    let count = d.selection_count;
    let has_selection = count > 0;
    let several = |one: &str, many: String| several(count, one, many);
    if has_selection && !d.contains_group {
        more.push(
            item(
                Action::RegenerateThumbnails,
                several(
                    t("Regenerate Thumbnail"),
                    format!("Regenerate {count} Thumbnails"),
                ),
                h.regenerate,
            )
            .icon("menu-refresh")
            .shortcut(b.hint("file.regenerateThumbnail"))
            .into(),
        );
    }
    if !more.is_empty() {
        rows.push(submenu("More", Some("menu-dots"), std::mem::take(more)));
        rows.push(Row::Separator);
    }

    if d.scope == Some(ScopeKind::Folder) && has_selection {
        rows.push(
            item(
                Action::RemoveFromFolder,
                several(
                    t("Remove from Folder"),
                    format!("Remove {count} from Folder"),
                ),
                h.remove_from_folder,
            )
            .icon("menu-folder-minus")
            .shortcut(b.hint("file.removeFromFolder"))
            .into(),
        );
        rows.push(Row::Separator);
    }

    if has_selection {
        if d.status == Some(Status::Trash) {
            rows.push(
                item(
                    Action::Restore,
                    several(t("Restore"), format!("Restore {count} Items")),
                    h.restore,
                )
                .icon("menu-arrow-back-up")
                .shortcut(b.hint("file.restore"))
                .into(),
            );
            rows.push(
                item(
                    Action::DeletePermanently,
                    several(
                        t("Delete Permanently"),
                        format!("Delete {count} Permanently"),
                    ),
                    h.permanent_delete,
                )
                .icon("menu-trash")
                .shortcut(b.hint("file.delete"))
                .danger()
                .into(),
            );
        } else {
            rows.push(
                item(
                    Action::MoveToTrash,
                    several(t("Move to Trash"), format!("Move {count} to Trash")),
                    h.move_to_trash,
                )
                .icon("menu-trash")
                .shortcut(if d.scope == Some(ScopeKind::Folder) {
                    None
                } else {
                    b.hint("file.delete")
                })
                .into(),
            );
        }
    }
}

/// Returns `many` when `count` is more than 1, and `one` otherwise.
fn several(count: usize, one: &str, many: String) -> String {
    if count > 1 { many } else { one.to_owned() }
}

/// Builds the menu for a right-click on empty space in the grid
/// (`buildEmptyContextMenu` in production).
pub fn empty_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let b = Builder { hint };
    let h = &d.handlers;
    let mut rows = Vec::new();
    let new_folder = h.new_folder && d.scope != Some(ScopeKind::SmartFolder);
    let new_smart_folder = h.new_smart_folder
        && d.scope != Some(ScopeKind::Folder)
        && d.scope != Some(ScopeKind::SmartFolder);
    if new_folder {
        rows.push(
            item(Action::NewFolder, "New Folder", true)
                .icon("menu-folder-plus")
                .shortcut(b.hint("file.newFolder"))
                .into(),
        );
    }
    if new_smart_folder {
        rows.push(
            item(Action::NewSmartFolder, "New Smart Folder", true)
                .icon("menu-filter-plus")
                .shortcut(b.hint("file.newSmartFolder"))
                .into(),
        );
    }
    if new_folder || new_smart_folder {
        rows.push(Row::Separator);
    }
    if h.import_files {
        rows.push(
            item(Action::ImportFiles, "Import Files...", true)
                .icon("menu-file-import")
                .shortcut(b.hint("file.import"))
                .disabled(d.import_blocked)
                .into(),
        );
    }
    if h.import_folder {
        rows.push(
            item(Action::ImportFolder, "Import Folder...", true)
                .icon("menu-folder-plus")
                .disabled(d.import_blocked)
                .into(),
        );
    }
    if h.paste_import {
        rows.push(
            item(Action::PasteImport, "Paste Import", true)
                .icon("menu-clipboard")
                .shortcut(b.hint("edit.paste"))
                .disabled(d.import_blocked)
                .into(),
        );
    }
    if h.import_files || h.import_folder || h.paste_import {
        rows.push(Row::Separator);
    }
    if h.sort_contents {
        rows.push(content_sort(None));
        rows.push(Row::Separator);
    }
    b.view_entries(&mut rows);
    rows.push(Row::Separator);
    rows
}

/// Builds the "Sort by" submenu (`contentSortSubmenu` in production). It has
/// one row for each field in [`CONTENT_SORT`], or only for the fields in
/// `fields` when that is given. The rows are in the same order as in
/// production.
fn content_sort(fields: Option<&[ContentSortField]>) -> Row {
    submenu(
        t("Sort by"),
        Some("menu-sort"),
        CONTENT_SORT
            .iter()
            .filter(|(field, ..)| fields.is_none_or(|fields| fields.contains(field)))
            .map(|(field, label, _)| item(Action::SortContents(*field), *label, true).into())
            .collect(),
    )
}

/// The two fields that a collection's contents can be sorted by, which are
/// name and size (`sortCollectionContents` in production).
pub const COLLECTION_SORT: [ContentSortField; 2] = [ContentSortField::Name, ContentSortField::Size];

/// Turns the tile menu in `rows` into the menu of a collection's member
/// (`openMemberMenu` in production). It adds a separator and then the row
/// "Remove from Group", or "Remove N from Group" when `count` is more than
/// 1. Both are put directly above the "Move to Trash" row, or at the end of
/// the menu when there is no "Move to Trash" row.
pub fn with_remove_from_group(mut rows: Vec<Row>, count: usize) -> Vec<Row> {
    let label = if count > 1 {
        crate::tf!("Remove {value0} from Group", count)
    } else {
        t("Remove from Group").to_owned()
    };
    let remove: Row = item(Action::RemoveFromGroup, label, true)
        .icon("menu-group-remove")
        .into();
    let trash = rows
        .iter()
        .position(|row| matches!(row, Row::Item(item) if item.action == Action::MoveToTrash));
    match trash {
        Some(at) => {
            rows.splice(at..at, [Row::Separator, remove]);
        }
        None => rows.extend([Row::Separator, remove]),
    }
    rows
}

/// Returns the handlers that the menu of a collection's members has
/// (`openMemberMenu` in production). `single` is true when the menu is for
/// one member, and false when it is for several members that are selected in
/// the collection's editor. `editor` is true when the menu was opened in the
/// editor (`selectionEnabled` in production), and only then does the menu
/// offer "Sort by". The rows that change items change the whole collection,
/// and not only the members that the menu was opened on.
pub fn member_handlers(single: bool, editor: bool) -> Handlers {
    Handlers {
        open_default: true,
        reveal: true,
        open_new_window: single,
        copy_file: true,
        copy_selection: true,
        copy_selection_paths: true,
        copy_selection_names: true,
        copy_selection_links: true,
        copy_path: true,
        copy_name: true,
        copy_link: true,
        add_to_folder: true,
        tag_select: true,
        ai_tagger: true,
        copy_tags: true,
        paste_tags: true,
        set_rating: true,
        export: true,
        export_originals: true,
        search_by_image: true,
        find_media_matches: true,
        regenerate: true,
        library_cover: single,
        move_to_trash: true,
        sort_contents: editor,
        ..Default::default()
    }
}

/// Builds the menu for a right-click on the empty space between a
/// collection's members, or around its tiles in the collection's editor
/// (`openEmptyMenu` in production). It has the "Sort by" submenu for the
/// collection's contents, a separator, and "Ungroup...".
pub fn collection_menu() -> Vec<Row> {
    vec![
        content_sort(Some(&COLLECTION_SORT)),
        Row::Separator,
        item(Action::Ungroup, "Ungroup...", true)
            .icon("menu-group-remove")
            .into(),
    ]
}

/// What the viewer is showing, for the rows that only the viewer's menu has:
/// the play, stop and mute rows of a Flash movie, and "Set as Thumbnail" for
/// a frame that can be captured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Playback {
    /// `Some((playing, muted))` when the viewer shows a Flash movie, and
    /// `None` otherwise.
    pub flash: Option<(bool, bool)>,
    /// True when the current frame can be captured, so that it can be set
    /// as the thumbnail.
    pub capture: bool,
}

/// Builds the viewer's menu (`useViewerEntityContextMenu` in production).
/// The rows that only the viewer has come first, then a separator, and then
/// the rows of the tile menu for the item.
pub fn viewer_menu(
    d: &Description,
    playback: Playback,
    hint: &dyn Fn(&str) -> Option<String>,
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    if let Some((playing, muted)) = playback.flash {
        rows.push(
            item(
                Action::FlashPlay,
                if playing { t("Pause") } else { t("Play") },
                true,
            )
            .icon(if playing {
                "menu-player-pause"
            } else {
                "menu-player-play"
            })
            .into(),
        );
        rows.push(
            item(Action::FlashStop, "Stop", true)
                .icon("menu-player-stop")
                .into(),
        );
        rows.push(
            item(
                Action::FlashMute,
                if muted { t("Unmute") } else { t("Mute") },
                true,
            )
            .icon(if muted {
                "menu-volume"
            } else {
                "menu-volume-off"
            })
            .into(),
        );
    }
    if playback.capture {
        rows.push(
            item(Action::SetAsThumbnail, "Set as Thumbnail", true)
                .icon("menu-photo")
                .into(),
        );
    }
    if !rows.is_empty() {
        rows.push(Row::Separator);
    }
    rows.extend(tile_menu(d, hint));
    rows
}

/// Builds a menu that has only the rows that open the file
/// (`buildEntityOpenContextEntries` in production). The cards on the
/// Duplicates screen show this menu.
pub fn open_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    Builder { hint }.open_entries(d, &mut rows);
    rows
}

/// Builds the menu of the preview in the inspector (`InspectorPreview` in
/// production). It has the rows that open the file, and then "Set as Library
/// Cover".
pub fn preview_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let b = Builder { hint };
    let mut rows = Vec::new();
    b.open_entries(d, &mut rows);
    rows.push(library_cover_row());
    rows
}

/// Builds the row for one application in the "Open With Other" submenu. The
/// row shows the application's icon, or the `menu-apps` icon when the
/// application has none. Its label is the application's name, followed by
/// "(Default)" when it is the default application.
fn application_row(application: &crate::desktop::Application) -> Row {
    let label = if application.default {
        crate::tf!("{value0} (Default)", application.name)
    } else {
        application.name.clone()
    };
    let mut row = item(Action::OpenWithApplication, label, true);
    row.value = Some(application.path.to_string_lossy().into_owned());
    match &application.icon {
        Some(icon) => row.image = Some(icon.clone()),
        None => row.icon = Some("menu-apps"),
    }
    row.into()
}

/// Builds the "Open With Other" row from `applications`, once they have been
/// looked up. The shell puts the result in place of the row that says
/// "Loading applications..." in the menu that is already open. The result
/// is empty when no application can open the file.
pub fn open_with_rows(applications: Vec<crate::desktop::Application>) -> Vec<Row> {
    let d = Description {
        open_with: OpenWith::Applications(applications),
        handlers: Handlers {
            open_with_application: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut rows = Vec::new();
    Builder { hint: &|_| None }.open_entries(&d, &mut rows);
    rows
}

/// Builds the "Set as Library Cover" row (`buildLibraryCoverContextEntry` in
/// production).
fn library_cover_row() -> Row {
    item(Action::SetLibraryCover, "Set as Library Cover", true)
        .icon("menu-photo")
        .into()
}

/// Builds the row that groups the selection into a collection
/// (`groupMenuEntry` in production). `may_merge` is true when the selection
/// may contain collections, which grouping merges into one.
pub fn group_row(may_merge: bool) -> Row {
    item(Action::Group, group_row_label(may_merge), true)
        .icon("menu-group-create")
        .into()
}

/// Returns the label of [`group_row`]: "Merge into One Group..." when
/// `may_merge` is true, and "Group..." otherwise. The shell also uses the
/// label to find the row in a menu that is already open, so that it can
/// replace the row.
pub fn group_row_label(may_merge: bool) -> String {
    if may_merge {
        t("Merge into One Group...").to_owned()
    } else {
        t("Group...").to_owned()
    }
}

/// Builds the row that ungroups the selected collections (`ungroupMenuEntry`
/// in production). `count` is the number of selected collections, or `None`
/// while they are still being counted.
pub fn ungroup_row(count: Option<usize>) -> Row {
    item(Action::Ungroup, ungroup_row_label(count), true)
        .icon("menu-group-remove")
        .into()
}

/// Returns the label of [`ungroup_row`]. The shell also uses the label to
/// find the row in a menu that is already open, so that it can replace the
/// row.
pub fn ungroup_row_label(count: Option<usize>) -> String {
    match count {
        None => t("Ungroup Collections...").to_owned(),
        Some(n) if n > 1 => crate::tf!("Ungroup {value0} Collections...", grouped(n as u64)),
        Some(_) => t("Ungroup...").to_owned(),
    }
}

/// Builds the "Export" submenu (`buildExportContextEntry` in production). It
/// has "Export Originals...", "Export As..." and, when the caller has the
/// `export_pack` handler, "Export as Picto Pack...".
fn export_row(h: &Handlers) -> Row {
    let mut children: Vec<Row> = vec![
        item(
            Action::ExportOriginals,
            t("Export Originals..."),
            h.export_originals,
        )
        .into(),
        item(Action::ExportAs, "Export As...", h.export).into(),
    ];
    if h.export_pack {
        children.push(item(Action::ExportPictoPack, "Export as Picto Pack...", true).into());
    }
    submenu("Export", Some("menu-file-export"), children)
}

/// Returns the action of every row in `rows`, including the rows inside
/// submenus. The shell searches this list for the action whose
/// [`Action::key`] the menu reported as chosen.
pub fn actions(rows: &[Row]) -> Vec<Action> {
    let mut found = Vec::new();
    for row in rows {
        match row {
            Row::Item(item) => found.push(item.action),
            Row::Submenu { children, .. } => found.extend(actions(children)),
            Row::Separator | Row::Panel(_) => {}
        }
    }
    found
}

/// Turns `rows` into the entries that the shared menu (`ui::menu`) draws.
/// `panel` is called for each view panel and returns the entry that draws
/// that panel.
pub fn entries(rows: Vec<Row>, panel: &mut dyn FnMut(Panel) -> menu::Entry) -> Vec<menu::Entry> {
    rows.into_iter()
        .map(|row| match row {
            Row::Item(row) => item_entry(row).into(),
            Row::Separator => menu::Entry::Separator,
            Row::Panel(which) => panel(which),
            Row::Submenu {
                label,
                icon,
                children,
            } => {
                let mut submenu = menu::submenu(label, entries(children, panel));
                if let Some(icon) = icon {
                    submenu = submenu.icon(icon);
                }
                submenu.into()
            }
        })
        .collect()
}

fn item_entry(row: ItemRow) -> menu::Item {
    let mut entry = menu::item(row.action.key(), row.label)
        .danger(row.danger)
        .disabled(row.disabled)
        .keep_open(row.keep_open);
    if let Some(icon) = row.icon {
        entry = entry.icon(icon);
    }
    if let Some(shortcut) = row.shortcut {
        entry = entry.shortcut(shortcut);
    }
    if let Some(checked) = row.checked {
        entry = entry.checked(checked);
    }
    if let Some(value) = row.value {
        entry = entry.value(value);
    }
    if let Some(image) = row.image {
        entry = entry.image(image);
    }
    entry
}

#[cfg(test)]
mod tests;
