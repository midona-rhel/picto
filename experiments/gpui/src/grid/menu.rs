//! The context menus of items (`gridContextMenu.tsx`): a right-click on a
//! tile, on empty grid space, in the viewer and on the inspector's preview.
//!
//! The rows are built by pure functions from a [`Description`] of what was
//! right-clicked: the selection, the scope, and which actions the caller can
//! perform (production's optional handlers). A row whose handler is missing
//! is shown disabled, or left out, exactly where production does either. The
//! shell turns the rows into a [`menu::Entry`] list and performs the chosen
//! [`Action`].
use crate::grid::format::grouped;
use crate::i18n::t;
use crate::ui::menu;
use picto_library::ContentSortField;

/// Where the menu was opened (`GridMenuContext.surface`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Grid,
    Viewer,
}

/// `scopeKind`: what kind of destination the grid shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    System,
    Folder,
    SmartFolder,
}

/// `statusFilter`: the lifecycle the menu acts within, as the application
/// menu has it.
pub use crate::app_menu::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Media,
    Collection,
}

/// How "Open With Other" can be offered (`openWithOptions`/`openWithPending`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum OpenWith {
    #[default]
    None,
    /// The applications are being looked up: a submenu with one disabled row.
    /// Production's state on macOS until it has listed them.
    Pending,
    /// The applications that open the file (macOS): a submenu of them, or
    /// nothing when there are none.
    Applications(Vec<crate::desktop::Application>),
    /// The system's own chooser (Windows): one row.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Chooser,
}

// Rows production offers and runs whose feature this app does not have
// yet: they are drawn where production draws them, disabled, as the
// application menu's rows for the same features are (`.todo()`).

// TODO: open the AI tagger on the items, beside the inspector
// (`aiTaggerPortalAtom`); then enable Auto Tag here and in the inspector.
pub const AUTO_TAG_BUILT: bool = false;
// TODO: production uploads the original to the engine from a hidden browser
// window and shows the results page (`runReverseImageSearch`); search by
// image needs that upload.
const SEARCH_BY_IMAGE_BUILT: bool = false;
// Windows' own application chooser on the file (`desktop::open_with_chooser`,
// `rundll32.exe shell32.dll,OpenAs_RunDLL <path>`); Windows is the only
// system that offers "Open With Other..." as a chooser.
const OPEN_WITH_CHOOSER_BUILT: bool = cfg!(target_os = "windows");

/// The reverse image search providers (`REVERSE_IMAGE_SEARCH_ENGINES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    TinEye,
    SauceNao,
    Yandex,
    Sogou,
    Bing,
}

// i18n: translated where drawn (menu rows)
pub const ENGINES: [(Engine, &str); 5] = [
    (Engine::TinEye, "TinEye"),
    (Engine::SauceNao, "SauceNAO"),
    (Engine::Yandex, "Yandex Images"),
    (Engine::Sogou, "Sogou"),
    (Engine::Bing, "Bing Visual Search"),
];

/// `CONTENT_SORT_OPTIONS`: a folder's own order, rewritten by one field.
// i18n: translated where drawn (menu rows)
pub const CONTENT_SORT: [(ContentSortField, &str); 6] = [
    (ContentSortField::Name, "Name"),
    (ContentSortField::ImportedAt, "Import Date"),
    (ContentSortField::CreatedAt, "Date Created"),
    (ContentSortField::ModifiedAt, "Date Modified"),
    (ContentSortField::Size, "Size"),
    (ContentSortField::Notes, "Notes"),
];

/// One of the view panels production draws in the menu as custom
/// entries (`buildContextMenuViewEntries`); the shell supplies them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// `ViewPanel`: the layout and the sort, inline.
    Layout,
    /// `DisplayPanel`: the display switches, inside "Display".
    Display,
}

/// Which of production's optional handlers the caller has: a row whose
/// handler is missing is disabled or left out, as production decides.
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

/// `GridMenuContext`: what was right-clicked, where.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Description {
    pub surface: Surface,
    pub selection_count: usize,
    /// The selection is a whole query or range rather than listed items.
    pub query_selection: bool,
    /// Whether Auto Tag can run; without it, whether there is a tagger.
    pub ai_tag_enabled: Option<bool>,
    pub single: bool,
    pub single_hash: Option<String>,
    pub single_item_id: Option<i64>,
    pub single_kind: Option<Kind>,
    pub single_name: Option<String>,
    pub single_mime: Option<String>,
    pub contains_group: bool,
    /// The selected collections: `None` when the caller does not say,
    /// `Some(None)` while they are being counted.
    pub collection_count: Option<Option<usize>>,
    pub mixed: bool,
    pub scope: Option<ScopeKind>,
    pub status: Option<Status>,
    pub grayscale: bool,
    pub last_folder_name: Option<String>,
    pub clipboard_tags: bool,
    pub open_with: OpenWith,
    /// Another import is running, so none can start.
    pub import_blocked: bool,
    pub handlers: Handlers,
    /// `sortFields`: the fields "Sort by" offers, when not all of them (a
    /// collection's contents sort by name or size only).
    pub sort_fields: Option<Vec<ContentSortField>>,
}

/// What a row does when chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    OpenDefault,
    OpenWithOther,
    /// One of "Open With Other"'s applications, named by its row's value.
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
    /// A collection's members leave it (`detachMembers`).
    RemoveFromGroup,
}

impl Action {
    /// The menu's name for the row; each action has its own, so a row can
    /// be checked or unchecked while the menu is open.
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
            SearchByImage(Engine::Sogou) => "search-sogou",
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

/// A row as built, before it becomes a [`menu::Entry`].
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
    /// Tells rows that share an action apart (an application's path).
    pub value: Option<String>,
    /// A picture in place of the glyph (an application's icon).
    pub image: Option<std::sync::Arc<gpui::Image>>,
}

/// `item(label, { action })`: without its handler a row is disabled.
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

/// Builds the rows; `hint` gives each shortcut's text.
struct Builder<'a> {
    hint: &'a dyn Fn(&str) -> Option<String>,
}

impl Builder<'_> {
    fn hint(&self, id: &str) -> Option<String> {
        (self.hint)(id)
    }

    /// `buildEntityOpenContextEntries`.
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

    /// `buildEntityCopyContextEntries`.
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

    /// `buildContextMenuViewEntries`: the layout and sort panel inline,
    /// then the display panel inside "Display".
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

/// `buildTileContextMenu`: a right-click on a tile, or in the viewer.
pub fn tile_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let b = Builder { hint };
    let h = &d.handlers;
    let count = d.selection_count;
    let viewer = d.surface == Surface::Viewer;
    let has_selection = count > 0;
    let ai_tag_enabled = d.ai_tag_enabled.unwrap_or(h.ai_tagger);
    let can_set_library_cover = d.single
        && d.single_hash.is_some()
        && d.single_kind == Some(Kind::Media)
        && h.library_cover;
    let several = |one: &str, many: String| if count > 1 { many } else { one.to_owned() };
    let mut rows: Vec<Row> = Vec::new();
    let mut more: Vec<Row> = Vec::new();

    // A selection with folders in it has a short menu of its own.
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

    // Inbox review comes first.
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

    if !viewer {
        if h.sort_contents {
            rows.push(content_sort(d.sort_fields.as_deref()));
            rows.push(Row::Separator);
        }
        b.view_entries(&mut rows);
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

    if has_selection {
        if h.add_to_last_folder {
            let label = match &d.last_folder_name {
                Some(name) => format!("Add to \u{201c}{name}\u{201d}"),
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
            // Grouping merges any selected collections into one; a selection
            // whose collections are still being counted may hold some.
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

    if d.single && d.single_hash.is_some() && d.single_kind != Some(Kind::Collection) {
        b.copy_entries(d, &mut rows);
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
            .disabled(!ai_tag_enabled || !AUTO_TAG_BUILT)
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
                .map(|(engine, label)| {
                    item(
                        Action::SearchByImage(*engine),
                        *label,
                        SEARCH_BY_IMAGE_BUILT,
                    )
                    .into()
                })
                .collect(),
        ));
        rows.push(Row::Separator);
    }

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
        rows.push(submenu("More", Some("menu-dots"), more));
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

    // Only "Delete Permanently" is drawn as dangerous.
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
    rows
}

/// `buildEmptyContextMenu`: a right-click on empty grid space.
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

/// `contentSortSubmenu`: "Sort by" with each field, or only `fields`, in
/// production's order.
fn content_sort(fields: Option<&[ContentSortField]>) -> Row {
    submenu(
        t("Sort by"),
        Some("menu-sort"),
        CONTENT_SORT
            .iter()
            .filter(|(field, _)| fields.is_none_or(|fields| fields.contains(field)))
            .map(|(field, label)| item(Action::SortContents(*field), *label, true).into())
            .collect(),
    )
}

/// The fields a collection's contents sort by (`sortCollectionContents`).
pub const COLLECTION_SORT: [ContentSortField; 2] = [ContentSortField::Name, ContentSortField::Size];

/// `openMemberMenu`: a collection member's menu is the tile menu with
/// "Remove from Group" (or "Remove N from Group") after a separator, just
/// above the Trash row, or at the end when there is none.
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

/// The handlers `openMemberMenu` gives a collection's members: one member
/// or the editor's selection of them. Sorting the contents is the editor's
/// (`selectionEnabled`); the edits that act on items act on the collection.
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

/// `openEmptyMenu`: the space between a collection's members, or around its
/// tiles in the editor: sort its contents, then ungroup it.
pub fn collection_menu() -> Vec<Row> {
    vec![
        content_sort(Some(&COLLECTION_SORT)),
        Row::Separator,
        item(Action::Ungroup, "Ungroup...", true)
            .icon("menu-group-remove")
            .into(),
    ]
}

/// What the viewer shows that has its own commands: the Flash movie's
/// playback, and a frame that can become the thumbnail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Playback {
    /// `Some((playing, muted))` for a Flash movie.
    pub flash: Option<(bool, bool)>,
    /// The current frame can be captured.
    pub capture: bool,
}

/// `useViewerEntityContextMenu`: the viewer's own rows, then the item's.
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

/// `buildEntityOpenContextEntries` alone: the rows that open the file, as
/// the Duplicates screen's cards offer them.
pub fn open_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    Builder { hint }.open_entries(d, &mut rows);
    rows
}

/// The inspector preview's menu (`InspectorPreview`): opening the file, then
/// "Set as Library Cover".
pub fn preview_menu(d: &Description, hint: &dyn Fn(&str) -> Option<String>) -> Vec<Row> {
    let b = Builder { hint };
    let mut rows = Vec::new();
    b.open_entries(d, &mut rows);
    rows.push(library_cover_row());
    rows
}

/// One application of "Open With Other": its icon, or the apps glyph
/// without one, and "(Default)" after the default's name.
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

/// The "Open With Other" row as the applications looked up make it, to
/// replace the pending one in the open menu: none when nothing opens the
/// file.
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

/// `buildLibraryCoverContextEntry`.
fn library_cover_row() -> Row {
    item(Action::SetLibraryCover, "Set as Library Cover", true)
        .icon("menu-photo")
        .into()
}

/// `groupMenuEntry`.
fn group_row(may_merge: bool) -> Row {
    item(
        Action::Group,
        if may_merge {
            t("Merge into One Group...")
        } else {
            t("Group...")
        },
        true,
    )
    .icon("menu-group-create")
    .into()
}

/// `ungroupMenuEntry`: `None` while the collections are being counted.
fn ungroup_row(count: Option<usize>) -> Row {
    let label = match count {
        None => t("Ungroup Collections...").to_owned(),
        Some(n) if n > 1 => crate::tf!("Ungroup {value0} Collections...", grouped(n as u64)),
        Some(_) => t("Ungroup...").to_owned(),
    };
    item(Action::Ungroup, label, true)
        .icon("menu-group-remove")
        .into()
}

/// `buildExportContextEntry`.
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

/// Every action the rows offer, to find the one chosen by its key.
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

/// The rows as the shared menu draws them, with `panel` drawing the view
/// panels.
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
