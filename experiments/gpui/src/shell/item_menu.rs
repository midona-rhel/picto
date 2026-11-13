//! The item menus the shell shows: a tile's and empty grid space's
//! (`GridScreen.tsx`), the viewer's (`useViewerEntityContextMenu.tsx`) and the
//! inspector preview's. The rows come from [`crate::grid::menu`]; this
//! describes what was right-clicked, as the Electron screens do, and performs
//! the chosen row through the session and the library.
use super::Shell;
use crate::backend;
use crate::grid::layout::ViewMode;
use crate::grid::menu::{
    self as rows, Action, Description, Handlers, Kind, OpenWith, Playback, Row, ScopeKind, Status,
    Surface, Toggle, ViewState,
};
use crate::session::Change;
use crate::ui::{self, menu::Menu, modal::Presence};
use gpui::*;
use picto_library::predicate::{SortDirection, SortField};
use picto_library::query::{ItemScope, RootSummary};
use picto_library::selection::SelectionTarget;
use picto_library::{ContentSortField, FolderId, Library, Lifecycle, RootId, RootKind};
use std::path::PathBuf;
use std::rc::Rc;

/// The item menus' state in the shell.
#[derive(Default)]
pub(super) struct ItemMenus {
    /// What the open menu acts on, captured when it opened, and its rows'
    /// actions.
    target: Option<Target>,
    actions: Vec<Action>,
    /// The tags "Copy Tags" took, for "Paste Tags" (production keeps them on
    /// the window, beside the JSON it writes to the clipboard).
    tag_clipboard: Vec<String>,
    confirm: Option<Confirmation>,
}

/// The items a menu acts on.
#[derive(Clone, Default)]
struct Target {
    /// In the order the grid shows them.
    items: Vec<RootSummary>,
    /// The one file the single-item rows act on: the cover of a grid tile,
    /// the member the viewer shows, or the preview's.
    file: Option<File>,
    /// The folder the grid shows.
    folder: Option<FolderId>,
}

#[derive(Clone)]
struct File {
    /// The item it belongs to, whose details say where it is.
    root: RootId,
    hash: String,
    mime: String,
    name: String,
}

impl Target {
    fn ids(&self) -> Vec<RootId> {
        self.items.iter().map(|item| item.root_id).collect()
    }
    fn selection(&self) -> SelectionTarget {
        SelectionTarget::Explicit {
            root_ids: self.ids(),
        }
    }
}

/// A question asked before an edit (`ConfirmModal`).
struct Confirmation {
    title: SharedString,
    message: SharedString,
    label: SharedString,
    danger: bool,
    presence: Presence,
    focus: FocusHandle,
    run: Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>,
}

/// What an edit changed, and so what is read again after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reread {
    /// The item shown in the inspector.
    Item,
    /// That, and the sidebar's destinations with their counts.
    Destinations,
    /// That, and the grid's results, which clears the selection.
    Results,
}

type Work<T> = Box<dyn FnOnce(&Library) -> picto_library::Result<T> + Send>;

/// `mediaLinkFor`: the link the Electron app serves a file at.
fn media_link(hash: &str, mime: &str) -> String {
    let extension = match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ => "bin",
    };
    format!("media://localhost/file/{hash}.{extension}")
}

/// `tagName`: general tags go without their namespace.
fn tag_name(tag: &picto_library::TagRecord) -> String {
    let namespace = tag.namespace.trim().to_lowercase();
    if namespace.is_empty() || namespace == "general" {
        tag.subname.clone()
    } else {
        format!("{namespace}:{}", tag.subname)
    }
}

/// `permanentDeleteMessage`.
fn permanent_delete_message(media: Option<u128>, undo_limit: i64) -> String {
    let limit = grouped(undo_limit.max(0) as u64);
    match media {
        _ if undo_limit == 0 => "Delete undo is disabled. This cannot be undone.".to_owned(),
        Some(count) if count <= undo_limit as u128 => {
            "You can undo this until Picto closes. After that the files are removed.".to_owned()
        }
        None => format!(
            "Deleting up to {limit} media items, including collection members, can be undone until Picto closes. Larger deletes cannot be undone."
        ),
        Some(_) => format!(
            "This exceeds the undo limit of {limit} media items, including collection members, so it cannot be undone."
        ),
    }
}

fn grouped(count: u64) -> String {
    crate::grid::format::grouped(count)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

impl Shell {
    // ── Opening ─────────────────────────────────────────────────

    /// The view panel's choices: the grid's saved view and the rails.
    fn view_state(&self, cx: &App) -> ViewState {
        let prefs = &self.session.read(cx).prefs;
        ViewState {
            mode: prefs.mode,
            sort: prefs.sort.field,
            direction: prefs.sort.direction,
            show_name: prefs.show_name,
            show_resolution: prefs.show_resolution,
            show_extension: prefs.show_extension,
            show_file_type: prefs.show_extension_label,
            show_item_count: prefs.show_item_count,
            compact: prefs.gap < 16.,
            fit_thumbnails: prefs.fit_cover,
            show_subfolders: prefs.show_subfolders,
            sidebar: !self.sidebar_collapsed,
            inspector: !self.inspector_collapsed,
        }
    }

    /// The destination the grid shows, as the menu tells destinations apart.
    fn grid_scope(&self, cx: &App) -> (Option<ItemScope>, ScopeKind, Option<FolderId>) {
        let session = self.session.read(cx);
        let scope = session
            .nav
            .get(session.displayed)
            .map(|entry| entry.scope.clone());
        match scope {
            Some(ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id }) => {
                (scope, ScopeKind::Folder, Some(folder_id))
            }
            Some(ItemScope::SmartFolder { .. }) => (scope, ScopeKind::SmartFolder, None),
            _ => (scope, ScopeKind::System, None),
        }
    }

    /// The grid's selection, in grid order, with the one item's cover file.
    fn selection_target(&self, cx: &App) -> Option<Target> {
        let session = self.session.read(cx);
        let items: Vec<RootSummary> = session
            .selection
            .iter()
            .filter_map(|index| session.items.get(*index).cloned())
            .collect();
        let file = match items.as_slice() {
            [] => return None,
            [item] => Some(File {
                root: item.root_id,
                hash: item.content_hash.clone(),
                mime: item.mime.clone(),
                name: item.name.clone(),
            }),
            _ => None,
        };
        Some(Target {
            items,
            file,
            folder: self.grid_scope(cx).2,
        })
    }

    /// Performs a row's action without its menu, for a shortcut: on the
    /// item the viewer shows while it is open, else on the selection.
    pub(super) fn act_on_selection(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = if self.session.read(cx).viewer {
            self.shown_item(true, cx).map(|(item, file)| Target {
                items: vec![item],
                file: Some(file),
                folder: None,
            })
        } else {
            self.selection_target(cx)
        };
        if let Some(target) = target {
            self.items.target = Some(target);
            self.run_item_action(action, None, window, cx);
        }
    }

    /// A tile was right-clicked: the selection's menu. The grid has already
    /// made the tile the selection when it was outside it.
    pub(super) fn open_tile_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (scope, scope_kind, folder) = self.grid_scope(cx);
        let Some(Target { items, file, .. }) = self.selection_target(cx) else {
            return;
        };
        let count = items.len();
        let collections = items
            .iter()
            .filter(|item| item.kind == RootKind::Collection)
            .count();
        let single = (count == 1).then(|| &items[0]);
        // `selectionLifecycle`: shared by every selected item.
        let lifecycle = items
            .iter()
            .all(|item| item.lifecycle == items[0].lifecycle)
            .then_some(items[0].lifecycle);
        let status = match (lifecycle, &scope) {
            (Some(Lifecycle::Inbox), _) | (_, Some(ItemScope::Inbox)) => Some(Status::Inbox),
            (Some(Lifecycle::Trash), _) | (_, Some(ItemScope::Trash)) => Some(Status::Trash),
            (_, Some(ItemScope::All)) => Some(Status::Active),
            _ => None,
        };
        let media = single.is_some_and(|item| item.kind == RootKind::Media);
        let last_folder_name = self.last_folder_name(cx);
        let description = Description {
            surface: Surface::Grid,
            selection_count: count,
            ai_tag_enabled: Some(count > 0),
            single: single.is_some(),
            single_hash: single.map(|item| item.content_hash.clone()),
            single_item_id: single.map(|item| item.root_id.0 as i64),
            single_kind: single.map(|item| kind(item.kind)),
            single_name: single.map(|item| item.name.clone()),
            single_mime: single.map(|item| item.mime.clone()),
            contains_group: collections > 0,
            collection_count: Some(Some(collections)),
            scope: Some(scope_kind),
            status,
            grayscale: false,
            clipboard_tags: !self.items.tag_clipboard.is_empty(),
            // Production lists the associated applications on macOS; this
            // offers the system chooser's one row until they can be listed.
            open_with: if media {
                OpenWith::Chooser
            } else {
                OpenWith::None
            },
            fixed_order: fixed_order(&scope),
            view: self.view_state(cx),
            handlers: Handlers {
                open_default: true,
                open_with_chooser: true,
                reveal: true,
                open_new_window: single.is_some(),
                copy_file: true,
                copy_path: true,
                copy_name: true,
                copy_link: true,
                copy_selection: true,
                copy_selection_paths: true,
                copy_selection_names: true,
                copy_selection_links: true,
                move_to_trash: true,
                restore: true,
                permanent_delete: true,
                add_to_folder: true,
                add_to_last_folder: last_folder_name.is_some(),
                remove_from_folder: true,
                tag_select: true,
                ai_tagger: count > 0,
                accept: true,
                reject: true,
                rename: single.is_some(),
                batch_rename: count > 1,
                regenerate: true,
                library_cover: true,
                folder_cover: folder.is_some() && single.is_some(),
                copy_tags: true,
                paste_tags: true,
                new_folder_with_selection: true,
                find_media_matches: true,
                search_by_image: true,
                set_rating: true,
                export: true,
                export_originals: true,
                export_pack: true,
                organize_group: count > 1,
                edit_group: single.is_some_and(|item| item.kind == RootKind::Collection),
                ungroup: collections > 0,
                sort_contents: folder.is_some(),
                toggle_grayscale: true,
                ..Default::default()
            },
            last_folder_name,
            ..Default::default()
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::tile_menu(&description, &hints);
        let target = Target {
            items,
            file,
            folder,
        };
        self.show_item_menu(built, target, true, position, window, cx);
    }

    /// Empty grid space was right-clicked (`openEmptyGridContextMenu`).
    pub(super) fn open_empty_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (scope, scope_kind, folder) = self.grid_scope(cx);
        let can_import = !matches!(
            scope,
            Some(
                ItemScope::Trash
                    | ItemScope::SmartFolder { .. }
                    | ItemScope::RecentlyViewed
                    | ItemScope::MediaMatches { .. }
            )
        );
        // `hasClipboardImport`: files or an image on the clipboard.
        let clipboard_import = can_import
            && cx.read_from_clipboard().is_some_and(|item| {
                item.entries().iter().any(|entry| {
                    matches!(
                        entry,
                        ClipboardEntry::ExternalPaths(_) | ClipboardEntry::Image(_)
                    )
                })
            });
        let description = Description {
            scope: Some(scope_kind),
            fixed_order: fixed_order(&scope),
            view: self.view_state(cx),
            handlers: Handlers {
                new_folder: true,
                new_smart_folder: true,
                import_files: can_import,
                import_folder: can_import,
                paste_import: clipboard_import,
                sort_contents: folder.is_some(),
                ..Default::default()
            },
            ..Default::default()
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::empty_menu(&description, &hints);
        let target = Target {
            folder,
            ..Default::default()
        };
        self.show_item_menu(built, target, true, position, window, cx);
    }

    /// The item the viewer and the preview act on: the selected item, with
    /// the member the viewer shows or the cover the preview shows.
    fn shown_item(&self, member: bool, cx: &App) -> Option<(RootSummary, File)> {
        let session = self.session.read(cx);
        let item = session.selected_item()?.clone();
        let details = session.selected_details();
        let media = details.and_then(|details| {
            if member {
                details.media.get(session.member)
            } else {
                details
                    .media
                    .iter()
                    .find(|media| media.media_id == details.root.cover_media_id)
                    .or(details.media.first())
            }
        });
        let (hash, mime) = match media {
            Some(media) => (media.facts.content_hash.clone(), media.facts.mime.clone()),
            None => (item.content_hash.clone(), item.mime.clone()),
        };
        let file = File {
            root: item.root_id,
            name: item.name.clone(),
            hash,
            mime,
        };
        Some((item, file))
    }

    /// The viewer was right-clicked: the viewer's rows for the item it shows.
    pub(super) fn open_viewer_menu(
        &mut self,
        position: Point<Pixels>,
        flash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((item, file)) = self.shown_item(true, cx) else {
            return;
        };
        let media = item.kind == RootKind::Media;
        let trash = item.lifecycle == Lifecycle::Trash;
        let description = Description {
            surface: Surface::Viewer,
            selection_count: 1,
            ai_tag_enabled: Some(media && file.mime.starts_with("image/")),
            single: true,
            single_hash: Some(file.hash.clone()),
            single_kind: Some(kind(item.kind)),
            single_name: Some(item.name.clone()),
            single_mime: Some(file.mime.clone()),
            contains_group: !media,
            status: Some(match item.lifecycle {
                Lifecycle::Inbox => Status::Inbox,
                Lifecycle::Active => Status::Active,
                Lifecycle::Trash => Status::Trash,
            }),
            clipboard_tags: !self.items.tag_clipboard.is_empty(),
            handlers: Handlers {
                open_default: true,
                reveal: true,
                open_new_window: true,
                copy_file: true,
                copy_path: true,
                copy_name: true,
                copy_link: true,
                add_to_folder: true,
                tag_select: true,
                ai_tagger: media && file.mime.starts_with("image/"),
                copy_tags: true,
                paste_tags: true,
                set_rating: true,
                export: true,
                search_by_image: media,
                regenerate: media,
                library_cover: media,
                move_to_trash: !trash,
                restore: trash,
                permanent_delete: trash,
                ..Default::default()
            },
            ..Default::default()
        };
        // Production captures the current frame of a video or Flash movie;
        // this viewer plays Flash only.
        let playback = Playback {
            flash: flash.then_some((true, false)),
            capture: flash,
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::viewer_menu(&description, playback, &hints);
        let target = Target {
            items: vec![item],
            file: Some(file),
            folder: None,
        };
        self.show_item_menu(built, target, false, position, window, cx);
    }

    /// The inspector's preview was right-clicked.
    pub(super) fn open_preview_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((item, file)) = self.shown_item(false, cx) else {
            return;
        };
        let description = Description {
            single: true,
            single_hash: Some(file.hash.clone()),
            handlers: Handlers {
                open_default: true,
                reveal: true,
                open_new_window: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::preview_menu(&description, &hints);
        let target = Target {
            items: vec![item],
            file: Some(file),
            folder: None,
        };
        self.show_item_menu(built, target, true, position, window, cx);
    }

    fn show_item_menu(
        &mut self,
        built: Vec<Row>,
        target: Target,
        search: bool,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.items.actions = rows::actions(&built);
        self.items.target = Some(target);
        let entries = rows::entries(built);
        let menu = cx.new(|cx| Menu::new(entries, position, cx).search(search));
        self._menu = Some(cx.subscribe_in(
            &menu,
            window,
            |this, menu, event: &ui::menu::Event, window, cx| {
                let ui::menu::Event::Chosen(key) = event;
                let action = this
                    .items
                    .actions
                    .iter()
                    .copied()
                    .find(|action| action.key() == *key);
                if let Some(action) = action {
                    this.run_item_action(action, Some(menu), window, cx);
                }
            },
        ));
        self.show_menu(menu, window, cx);
    }

    fn last_folder_name(&self, cx: &App) -> Option<String> {
        // "Add to “…”" is the folder used last (`readRecentFolderIds`).
        let folder = *self.session.read(cx).recent_folders.first()?;
        self.session
            .read(cx)
            .nav
            .iter()
            .find(|entry| {
                matches!(entry.scope, ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id } if folder_id == folder)
            })
            .map(|entry| entry.label.clone())
    }

    // ── Performing ──────────────────────────────────────────────

    fn run_item_action(
        &mut self,
        action: Action,
        menu: Option<&Entity<Menu>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.items.target.clone() else {
            return;
        };
        let file = target.file.clone();
        let ids = target.ids();
        match action {
            // ── Opening ──
            Action::OpenDefault => {
                if let Some(file) = file {
                    self.with_path(file, |path, cx| cx.open_with_system(&path), window, cx);
                }
            }
            Action::Reveal => {
                if let Some(file) = file {
                    self.with_path(file, |path, cx| cx.reveal_path(&path), window, cx);
                }
            }
            Action::OpenInNewWindow => {
                // The item alone, as production's detail window opens it
                // from a single selection, the viewer and the preview.
                let index = target.items.first().and_then(|item| {
                    self.session
                        .read(cx)
                        .items
                        .iter()
                        .position(|each| each.root_id == item.root_id)
                });
                if let Some(index) = index {
                    self.open_in_window(vec![index], index, cx);
                }
            }
            Action::OpenWithOther => {
                // TODO: list the applications that open this type (macOS
                // shows them as a submenu, Windows opens its chooser) and
                // open the file with the one chosen.
            }
            Action::OpenFolder | Action::LoadingApplications => {
                // TODO: open the folder tile's folder once the grid shows
                // subfolder tiles.
            }
            // ── Copying ──
            Action::CopyPath => {
                if let Some(file) = file {
                    self.with_path(
                        file,
                        |path, cx| write_text(path.to_string_lossy().into_owned(), cx),
                        window,
                        cx,
                    );
                }
            }
            Action::CopyName => {
                if let Some(file) = file {
                    write_text(file.name, cx);
                }
            }
            Action::CopyLink => {
                if let Some(file) = file {
                    write_text(media_link(&file.hash, &file.mime), cx);
                }
            }
            Action::CopySelectionNames => {
                let names = target
                    .items
                    .iter()
                    .map(|item| {
                        if item.name.is_empty() {
                            "Untitled".to_owned()
                        } else {
                            item.name.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                write_text(names.join("\n"), cx);
            }
            Action::CopySelectionPaths | Action::CopySelectionLinks => {
                // `resolveTargetFilePaths`: every file of the selection,
                // collection members included, in order.
                let links = action == Action::CopySelectionLinks;
                self.library(
                    Box::new(move |library| {
                        let mut lines = Vec::new();
                        for id in ids {
                            for media in library.details(id)?.media {
                                lines.push(if links {
                                    let extension = std::path::Path::new(&media.file_path)
                                        .extension()
                                        .map_or("bin".into(), |extension| {
                                            extension.to_string_lossy().into_owned()
                                        });
                                    format!(
                                        "media://localhost/file/{}.{extension}",
                                        media.facts.content_hash
                                    )
                                } else {
                                    media.file_path
                                });
                            }
                        }
                        Ok(lines)
                    }),
                    |_, lines, _, cx| {
                        if !lines.is_empty() {
                            write_text(lines.join("\n"), cx);
                        }
                    },
                    window,
                    cx,
                );
            }
            Action::CopyFile | Action::CopySelection => {
                // TODO: put the files themselves on the clipboard. GPUI's
                // macOS pasteboard writes text and images only; a file list
                // (`ClipboardEntry::ExternalPaths`) is dropped.
            }
            // ── Lifecycle ──
            Action::Accept | Action::Restore => self.set_lifecycle(Lifecycle::Active, window, cx),
            Action::MoveToTrash => self.set_lifecycle(Lifecycle::Trash, window, cx),
            Action::Reject => {
                if crate::settings::current(cx).reject_action == "delete" {
                    self.request_permanent_delete(target, true, window, cx);
                } else {
                    self.set_lifecycle(Lifecycle::Trash, window, cx);
                }
            }
            Action::DeletePermanently => {
                self.request_permanent_delete(target, false, window, cx);
            }
            // ── Folders ──
            Action::AddToFolder => {
                self.dispatch(Box::new(crate::keymap::AddToFolder), window, cx);
                return;
            }
            Action::AddToLastFolder => {
                let last = self.session.read(cx).recent_folders.first().copied();
                if let Some(folder) = last {
                    self.session
                        .update(cx, |session, _| session.record_folder_use(folder));
                    let selection = target.selection();
                    self.edit(
                        Box::new(move |library| library.add_to_folder(&selection, folder)),
                        Reread::Destinations,
                        window,
                        cx,
                    );
                }
            }
            Action::NewFolderWithSelection => {
                let selection = target.selection();
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.new_folder_in(None, Some(selection), window, cx)
                });
                return;
            }
            Action::NewFolder => {
                let parent = target.folder;
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.new_folder_in(parent, None, window, cx)
                });
                return;
            }
            Action::RemoveFromFolder => {
                if let Some(folder) = target.folder {
                    let selection = target.selection();
                    self.edit(
                        Box::new(move |library| library.remove_from_folder(&selection, folder)),
                        Reread::Results,
                        window,
                        cx,
                    );
                }
            }
            Action::SetFolderCover => {
                if let (Some(folder), Some(id)) = (target.folder, ids.first().copied()) {
                    self.edit(
                        Box::new(move |library| library.set_folder_cover(folder, id)),
                        Reread::Destinations,
                        window,
                        cx,
                    );
                }
            }
            Action::SortContents(field) => {
                if let Some(folder) = target.folder {
                    self.sort_folder(folder, field, window, cx);
                }
            }
            // ── Groups ──
            Action::Group => self.group(target, window, cx),
            Action::Ungroup => self.confirm_ungroup(target, window, cx),
            Action::EditGroup => {
                // TODO: open the collection editor on this group.
            }
            // ── Names and tags ──
            Action::Rename => {
                if self.inspector_collapsed {
                    self.toggle_rails(false, true, cx);
                }
                self.inspector
                    .update(cx, |inspector, cx| inspector.edit_name(window, cx));
                return;
            }
            Action::BatchRename => {
                // TODO: open the batch rename dialog for these items.
            }
            Action::AddTags => {
                self.dispatch(Box::new(crate::keymap::AddTags), window, cx);
                return;
            }
            Action::AutoTag => {
                // TODO: open the AI tagger on the selection, beside the
                // inspector.
            }
            Action::CopyTags => self.copy_tags(target, window, cx),
            Action::PasteTags => {
                let tags = self.items.tag_clipboard.clone();
                if !tags.is_empty() && !ids.is_empty() {
                    let selection = target.selection();
                    self.edit(
                        Box::new(move |library| library.apply_tags(&selection, &tags, true)),
                        Reread::Destinations,
                        window,
                        cx,
                    );
                }
            }
            Action::Rate(stars) => {
                let rating = picto_library::Rating::ALL[usize::from(stars).min(5)];
                let selection = target.selection();
                self.edit(
                    Box::new(move |library| library.set_rating(&selection, rating)),
                    Reread::Item,
                    window,
                    cx,
                );
            }
            // ── Waiting on other parts of the app ──
            Action::ExportOriginals | Action::ExportAs | Action::ExportPictoPack => {
                // TODO: export through the application backend (originals to
                // a chosen folder, the export dialog, a Picto Pack).
            }
            Action::SearchByImage(_) => {
                // TODO: production uploads the original to the engine from a
                // hidden browser window (`runReverseImageSearch`) and shows
                // the results page; this needs that upload.
            }
            Action::SetLibraryCover => {
                // TODO: open the library cover dialog (`MediaCoverDialog`)
                // with this item chosen, where the square is framed before
                // the cover is written beside the library's database.
            }
            Action::FindMediaMatches => {
                // TODO: open the media-matches destination
                // (`ItemScope::MediaMatches`) for this item.
            }
            Action::RegenerateThumbnails => {
                // TODO: regenerate the thumbnails of these files.
            }
            Action::Grayscale => {
                // TODO: draw the grid's thumbnails in grayscale and check
                // the row while it is on.
            }
            Action::NewSmartFolder => {
                // TODO: open the smart folder editor for a new one.
            }
            Action::ImportFiles | Action::ImportFolder | Action::PasteImport => {
                // TODO: choose files or a folder, or take the clipboard's,
                // and import them into this destination.
            }
            Action::FlashPlay | Action::FlashStop | Action::FlashMute => {
                // TODO: pause, stop and mute the Flash player.
            }
            Action::SetAsThumbnail => {
                // TODO: capture the shown frame and make it the thumbnail.
            }
            // ── View ──
            Action::Layout(mode) => {
                for (each, _, _) in rows::LAYOUTS {
                    self.check(menu, Action::Layout(each), each == mode, cx);
                }
                self.change_view(
                    |prefs| prefs.mode = mode,
                    serde_json::json!({ "view_mode": mode_value(mode) }),
                    false,
                    cx,
                );
                return;
            }
            Action::Sort(field) => {
                for (each, _) in rows::SORTS {
                    self.check(menu, Action::Sort(each), each == field, cx);
                }
                let direction = self.session.read(cx).prefs.sort.direction;
                self.change_view(
                    |prefs| prefs.sort.field = field,
                    serde_json::json!({
                        "sort_field": sort_value(field),
                        "sort_order": direction_value(direction),
                    }),
                    true,
                    cx,
                );
                return;
            }
            Action::Direction(direction) => {
                for each in [SortDirection::Ascending, SortDirection::Descending] {
                    self.check(menu, Action::Direction(each), each == direction, cx);
                }
                let field = self.session.read(cx).prefs.sort.field;
                self.change_view(
                    |prefs| prefs.sort.direction = direction,
                    serde_json::json!({
                        "sort_field": sort_value(field),
                        "sort_order": direction_value(direction),
                    }),
                    true,
                    cx,
                );
                return;
            }
            Action::Display(toggle) => {
                let on = !self.view_state(cx).toggled(toggle);
                self.check(menu, action, on, cx);
                self.toggle_display(toggle, on, cx);
                return;
            }
        }
        window.focus(&self.focus, cx);
    }

    /// Sends an action for another part of the app from the shell, which
    /// has the keyboard once the menu has closed.
    fn dispatch(&mut self, action: Box<dyn gpui::Action>, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
        window.dispatch_action(action, cx);
    }

    /// `resolveFilePath`: where the library keeps the file, from the item's
    /// details, then `then` with it.
    fn with_path(
        &mut self,
        file: File,
        then: impl FnOnce(PathBuf, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let File { root, hash, .. } = file;
        self.library(
            Box::new(move |library| {
                Ok(library
                    .details(root)?
                    .media
                    .into_iter()
                    .find(|media| media.facts.content_hash == hash)
                    .map(|media| PathBuf::from(media.file_path)))
            }),
            |_, path, _, cx| {
                if let Some(path) = path {
                    then(path, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Checks or unchecks a row of the open menu.
    fn check(&self, menu: Option<&Entity<Menu>>, action: Action, checked: bool, cx: &mut App) {
        if let Some(menu) = menu {
            menu.update(cx, |menu, cx| menu.set_checked(action.key(), checked, cx));
        }
    }

    /// Runs `work` on the library's thread and hands its answer to `then`.
    /// A failure is reported on the console, as the shell's other edits do.
    fn library<T: Send + 'static>(
        &mut self,
        work: Work<T>,
        then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = self.session.read(cx).run(work);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, move |this, window, cx| match result {
                Ok(value) => then(this, value, window, cx),
                // TODO: show this as a notification, as production's
                // `showErrorNotification` does.
                Err(error) => eprintln!("picto: {error}"),
            });
        })
        .detach();
    }

    /// An edit of the selected items, and what to read again afterwards.
    fn edit<T: Send + 'static>(
        &mut self,
        work: Work<T>,
        reread: Reread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library(
            work,
            move |this, _, window, cx| this.refresh(reread, window, cx),
            window,
            cx,
        );
    }

    /// Reads the item shown again, and the destinations (names, counts) and
    /// the grid's results as far as `reread` says.
    fn refresh(&mut self, reread: Reread, window: &mut Window, cx: &mut Context<Self>) {
        self.session.read(cx).refresh_details();
        self.inspector
            .update(cx, |inspector, cx| inspector.selection_edited(cx));
        if reread == Reread::Item {
            return;
        }
        self.library(
            Box::new(backend::navigation),
            move |this, (nav, tags), _, cx| {
                this.session.update(cx, |session, cx| {
                    session.replace_navigation(nav, tags, cx);
                    if reread == Reread::Results {
                        session.reload(cx);
                    }
                });
            },
            window,
            cx,
        );
    }

    /// Moves the selected items into or out of the Trash or the Inbox
    /// through the session, which reloads the grid, then reads the
    /// destinations' counts again.
    fn set_lifecycle(&mut self, lifecycle: Lifecycle, window: &mut Window, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.move_selection_to(lifecycle, cx));
        self.library(
            Box::new(backend::navigation),
            |this, (nav, tags), _, cx| {
                this.session
                    .update(cx, |session, cx| session.replace_navigation(nav, tags, cx));
            },
            window,
            cx,
        );
    }

    fn sort_folder(
        &mut self,
        folder: FolderId,
        field: ContentSortField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            Box::new(move |library| library.sort_folder_items(folder, field)),
            Reread::Results,
            window,
            cx,
        );
    }

    /// `onCopyTags`: one item's tags, or the tags every selected item
    /// shares, as JSON on the clipboard and kept for "Paste Tags".
    fn copy_tags(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let single = target.items.len() == 1;
        let ids = target.ids();
        let selection = target.selection();
        self.library(
            Box::new(move |library| {
                let tag_ids = if single {
                    library.details(ids[0])?.tag_ids
                } else {
                    library.selection_summary(&selection)?.shared_tags
                };
                Ok(library
                    .tags_by_id(&tag_ids)?
                    .iter()
                    .map(tag_name)
                    .collect::<Vec<_>>())
            }),
            |this, tags, _, cx| {
                write_text(serde_json::to_string(&tags).unwrap_or_default(), cx);
                this.items.tag_clipboard = tags;
            },
            window,
            cx,
        );
    }

    /// `organizeSelection`: one selected group and no notes to merge groups
    /// at once; anything else needs the group organizer.
    fn group(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cover) = target.items.first().map(|item| item.root_id) else {
            return;
        };
        let selection = target.selection();
        self.library(
            Box::new(move |library| {
                let summary = library.selection_summary(&selection)?;
                let notes = library.collection_note_draft(&selection)?;
                if let ([group], 0) = (summary.collection_candidates.as_slice(), notes.source_count)
                {
                    library.organize_into_collection(&picto_library::GroupRequest {
                        target: selection,
                        cover_root_id: cover,
                        winning_collection_id: Some(group.collection_id),
                        name: None,
                        notes: None,
                        modified_at_ms: now_ms(),
                    })?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }),
            |this, grouped, window, cx| {
                if grouped {
                    this.refresh(Reread::Results, window, cx);
                }
                // TODO: otherwise open the group organizer
                // (`GroupOrganizerModal`) with the selection's groups and
                // notes.
            },
            window,
            cx,
        );
    }

    fn confirm_ungroup(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let collections = target
            .items
            .iter()
            .filter(|item| item.kind == RootKind::Collection)
            .count();
        let title = if collections == 1 {
            "Ungroup?".to_owned()
        } else {
            format!("Ungroup {} collections?", grouped(collections as u64))
        };
        let message = if collections == 1 && target.items.len() == 1 {
            "Every member will return to the library as a separate item. Files and metadata will not be deleted."
        } else {
            "Every member of these collections will return to the library as a separate item. Other selected items stay as they are. Files and metadata will not be deleted."
        };
        let selection = target.selection();
        self.ask(
            title,
            message,
            "Ungroup",
            false,
            Rc::new(move |this, window, cx| {
                let selection = selection.clone();
                this.session
                    .update(cx, |session, cx| session.clear_selection(cx));
                this.edit(
                    Box::new(move |library| library.ungroup_selection(&selection, now_ms())),
                    Reread::Results,
                    window,
                    cx,
                );
            }),
            window,
            cx,
        );
    }

    /// `requestPermanentDelete`: asks first, saying whether the delete can
    /// be undone, and deletes through the library. A rejection (the
    /// setting that skips the Trash) asks only when it cannot be undone.
    ///
    /// Only a marked disposable copy of a library is changed this way. The
    /// library keeps the files of an undoable delete until it leaves the
    /// history, and hands back the files a larger delete frees.
    fn request_permanent_delete(
        &mut self,
        target: Target,
        reject: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = target.selection();
        let undo_limit = crate::settings::current(cx).undo_delete_limit;
        self.library(
            Box::new(move |library| library.selection_summary(&selection)),
            move |this, summary, window, cx| {
                let count = summary.selected_count;
                let undoable = undo_limit > 0 && summary.media_count <= undo_limit as u128;
                let title = if count == 1 {
                    "Delete permanently?".to_owned()
                } else {
                    format!("Delete {} items permanently?", grouped(count))
                };
                if reject && undoable {
                    this.delete_permanently(target, reject, undoable, title, window, cx);
                    return;
                }
                let message = permanent_delete_message(Some(summary.media_count), undo_limit);
                let ask_title = title.clone();
                this.ask(
                    ask_title,
                    message,
                    "Delete Permanently",
                    true,
                    Rc::new(move |this, window, cx| {
                        this.delete_permanently(
                            target.clone(),
                            reject,
                            undoable,
                            title.clone(),
                            window,
                            cx,
                        )
                    }),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    fn delete_permanently(
        &mut self,
        target: Target,
        reject: bool,
        require_undoable: bool,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.session.read(cx).disposable() {
            eprintln!("picto: refusing to delete from a library that is not a disposable copy");
            return;
        }
        let undo_limit = crate::settings::current(cx).undo_delete_limit.max(0) as usize;
        let selection = target.selection();
        self.session
            .update(cx, |session, cx| session.close_viewer(cx));
        let answer = self.session.read(cx).run(move |library| {
            let deleted = if reject {
                library.delete_rejected(&selection, now_ms(), undo_limit, require_undoable)
            } else {
                library.permanently_delete(&selection, now_ms(), undo_limit, require_undoable)
            };
            // The freed files are left where they are.
            // TODO: remove the files a delete that cannot be undone frees
            // (`clean_pending_blobs`), once the app owns its blob store.
            deleted.map(|_| ())
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, move |this, window, cx| match result {
                Ok(()) => this.refresh(Reread::Results, window, cx),
                // The library resolved a different selection or limit: never
                // drop the promise of undo silently; ask again.
                Err(picto_library::LibraryError::UndoLimitExceeded) if require_undoable => {
                    this.ask(
                        title.clone(),
                        "This delete can no longer be undone. Delete permanently anyway?",
                        "Delete Permanently",
                        true,
                        Rc::new(move |this, window, cx| {
                            this.delete_permanently(
                                target.clone(),
                                reject,
                                false,
                                title.clone(),
                                window,
                                cx,
                            )
                        }),
                        window,
                        cx,
                    );
                }
                Err(error) => eprintln!("picto: {error}"),
            });
        })
        .detach();
    }

    // ── View ────────────────────────────────────────────────────

    /// Changes the grid's view at once and saves it as the destination's
    /// own (`saveViewPref`). A new sort fetches the results again.
    fn change_view(
        &mut self,
        change: impl FnOnce(&mut crate::grid::prefs::ViewPrefs),
        patch: serde_json::Value,
        requery: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(scope) = self.grid_scope(cx).0 else {
            return;
        };
        let key = crate::grid::prefs::scope_key(&scope);
        let saved = self.session.read(cx).run(move |library| {
            library.update_auxiliary_json(
                "settings.view.patch",
                "Change view preferences",
                "view_pref",
                &key,
                false,
                move |before| {
                    let mut value = before
                        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                        .filter(serde_json::Value::is_object)
                        .unwrap_or_else(|| serde_json::json!({}));
                    if let (Some(value), Some(patch)) = (value.as_object_mut(), patch.as_object()) {
                        for (field, changed) in patch {
                            value.insert(field.clone(), changed.clone());
                        }
                    }
                    Ok(Some(value.to_string()))
                },
            )
        });
        self.session.update(cx, |session, cx| {
            change(&mut session.prefs);
            cx.emit(Change::Results);
            // The query reads the saved view, so it goes after the write on
            // the library's thread.
            if requery {
                session.reload(cx);
            }
        });
        cx.spawn(async move |_, _| {
            if let Ok(Err(error)) = saved.await {
                eprintln!("picto: saving the view failed: {error}");
            }
        })
        .detach();
    }

    fn toggle_display(&mut self, toggle: Toggle, on: bool, cx: &mut Context<Self>) {
        let (field, value) = match toggle {
            Toggle::Sidebar => {
                if on == self.sidebar_collapsed {
                    self.toggle_rails(true, false, cx);
                }
                return;
            }
            Toggle::Inspector => {
                if on == self.inspector_collapsed {
                    self.toggle_rails(false, true, cx);
                }
                return;
            }
            Toggle::Name => ("show_name", serde_json::json!(on)),
            Toggle::Resolution => ("show_resolution", serde_json::json!(on)),
            Toggle::Extension => ("show_extension", serde_json::json!(on)),
            Toggle::FileType => ("show_label", serde_json::json!(on)),
            Toggle::ItemCount => ("show_item_count", serde_json::json!(on)),
            Toggle::Compact => (
                "spacing",
                serde_json::json!(if on { "tight" } else { "wide" }),
            ),
            Toggle::FitThumbnails => (
                "thumbnail_fit",
                serde_json::json!(if on { "cover" } else { "contain" }),
            ),
            Toggle::Subfolders => ("show_subfolders", serde_json::json!(on)),
        };
        self.change_view(
            move |prefs| match toggle {
                Toggle::Name => prefs.show_name = on,
                Toggle::Resolution => prefs.show_resolution = on,
                // TODO: the grid's names do not show the extension yet.
                Toggle::Extension => prefs.show_extension = on,
                Toggle::FileType => prefs.show_extension_label = on,
                Toggle::ItemCount => prefs.show_item_count = on,
                Toggle::Compact => prefs.gap = if on { 8. } else { 16. },
                Toggle::FitThumbnails => prefs.fit_cover = on,
                // TODO: a folder always shows its subfolders' items here
                // (`FolderTree`); follow the switch.
                Toggle::Subfolders => prefs.show_subfolders = on,
                Toggle::Sidebar | Toggle::Inspector => {}
            },
            serde_json::json!({ field: value }),
            false,
            cx,
        );
    }

    // ── Confirmation ────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn ask(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        label: impl Into<SharedString>,
        danger: bool,
        run: Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.items.confirm = Some(Confirmation {
            title: title.into(),
            message: message.into(),
            label: label.into(),
            danger,
            presence: Presence::new(),
            focus,
            run,
        });
        cx.notify();
    }

    /// The open confirmation, drawn over everything. Once it has faded out
    /// the keyboard goes back to the grid.
    pub(super) fn confirmation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self
            .items
            .confirm
            .as_ref()
            .is_some_and(|confirm| confirm.presence.finished())
        {
            self.items.confirm = None;
            window.focus(&self.focus, cx);
        }
        let confirm = self.items.confirm.as_ref()?;
        let run = confirm.run.clone();
        let close = cx.listener(|this, _: &(), _, cx| this.close_confirmation(cx));
        let cancel = cx.listener(|this, _: &(), window, cx| {
            // Cancel takes the dialog away at once.
            this.items.confirm = None;
            window.focus(&this.focus, cx);
            cx.notify();
        });
        let accept = cx.listener(move |this, _: &(), window, cx| {
            this.close_confirmation(cx);
            window.focus(&this.focus, cx);
            run(this, window, cx);
        });
        Some(
            ui::confirm::confirm(
                "item-confirm",
                confirm.title.clone(),
                confirm.message.clone(),
                confirm.label.clone(),
                confirm.presence,
            )
            .danger(confirm.danger)
            .focus(&confirm.focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    fn close_confirmation(&mut self, cx: &mut Context<Self>) {
        if let Some(confirm) = &mut self.items.confirm {
            confirm.presence.close();
            cx.notify();
        }
    }
}

fn write_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

fn kind(kind: RootKind) -> Kind {
    match kind {
        RootKind::Media => Kind::Media,
        RootKind::Collection => Kind::Collection,
    }
}

/// `hasFixedOrder`: a folder or Recently Viewed keeps its own order.
fn fixed_order(scope: &Option<ItemScope>) -> bool {
    matches!(
        scope,
        Some(ItemScope::Folder { .. } | ItemScope::FolderTree { .. } | ItemScope::RecentlyViewed)
    )
}

fn mode_value(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Waterfall => "waterfall",
        ViewMode::Grid => "grid",
        ViewMode::Justified => "justified",
    }
}

fn sort_value(field: SortField) -> &'static str {
    match field {
        SortField::ImportedAt => "imported_at",
        SortField::CapturedAt => "captured_at",
        SortField::Name => "name",
        SortField::TotalSize => "total_size",
        SortField::Rating => "rating",
        SortField::Random => "random",
        SortField::FolderOrder => "folder_order",
    }
}

fn direction_value(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "ascending",
        SortDirection::Descending => "descending",
    }
}

#[cfg(test)]
mod tests {
    use super::{media_link, permanent_delete_message};

    #[test]
    fn delete_messages_say_whether_undo_is_possible() {
        assert_eq!(
            permanent_delete_message(Some(3), 0),
            "Delete undo is disabled. This cannot be undone."
        );
        assert_eq!(
            permanent_delete_message(Some(500), 500),
            "You can undo this until Picto closes. After that the files are removed."
        );
        assert_eq!(
            permanent_delete_message(Some(1500), 1000),
            "This exceeds the undo limit of 1,000 media items, including collection members, so it cannot be undone."
        );
        assert!(permanent_delete_message(None, 500).starts_with("Deleting up to 500 media"));
    }

    #[test]
    fn links_name_the_known_types_and_bin_otherwise() {
        assert_eq!(
            media_link("ab12", "image/jpeg"),
            "media://localhost/file/ab12.jpg"
        );
        assert_eq!(
            media_link("ab12", "application/pdf"),
            "media://localhost/file/ab12.bin"
        );
    }
}
