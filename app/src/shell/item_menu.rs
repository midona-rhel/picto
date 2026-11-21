//! The item menus that the shell shows: the menu of a grid tile, the menu of
//! empty grid space, the viewer's menu and the menu of the inspector's
//! preview. The rows come from [`crate::grid::menu`]. This module describes
//! what was right-clicked, builds the menu from that description, and
//! performs the chosen row through the session and the library.
use super::menu_commands::shared_lifecycle;
use super::{Shell, view_menu};
use crate::grid::menu::{
    self as rows, Action, Description, Handlers, Kind, OpenWith, Playback, Row, Status, Surface,
};
use crate::i18n::t;
use crate::ui::{self, menu::Menu};
use crate::viewer::{MenuRequested, surface::MediaSurface};
use gpui::*;
use picto_library::query::{ItemScope, RootSummary};
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, Lifecycle, RootId, RootKind};
use std::rc::Rc;

mod actions;
mod organizer;

/// Holds the state of the shell's item menus.
#[derive(Default)]
pub(super) struct ItemMenus {
    /// The items that the open menu acts on, recorded when the menu opened.
    /// The field `actions` below holds the actions of the menu's rows.
    target: Option<Target>,
    actions: Vec<Action>,
    /// The names of the tags that "Copy Tags" copied last. "Paste Tags"
    /// applies these, and does not read the clipboard.
    tag_clipboard: Vec<String>,
    confirm: ui::modal::Host<Confirmation>,
    /// The group organizer dialog, which is open while grouping needs a
    /// decision from the user.
    organizer: ui::modal::Host<Entity<organizer::Organizer>>,
    /// The media surface that the open viewer menu was opened on: the media
    /// view's or Quick Look's. The menu's Flash rows control the Flash movie
    /// of this surface.
    surface: Option<WeakEntity<MediaSurface>>,
    /// The value of the row that was chosen last. For the row of an
    /// application it is the application's path.
    chosen: Option<String>,
    /// The menu that is open now. Rows that are replaced or checked while
    /// the menu stays open are changed through it.
    open: Option<WeakEntity<Menu>>,
    /// The applications that can open each file type, by the type's file
    /// extension. They are looked up once for each extension.
    applications: std::collections::HashMap<String, Vec<crate::desktop::Application>>,
}

/// Holds the items that a menu acts on. It is filled when the menu opens.
#[derive(Clone, Default)]
pub(super) struct Target {
    /// The items, in the order in which the grid shows them. When the menu
    /// acts on the grid's selection, these are only the selected items that
    /// are loaded.
    items: Vec<RootSummary>,
    /// The grid's selection and the number of items in it, when the menu
    /// acts on the selection. An edit addresses this selection, so it also
    /// changes the selected items that are not loaded. When this is `None`,
    /// the menu acts on `items` alone: the viewer's item, the collection of
    /// a member, or the preview's item.
    selection: Option<(SelectionTarget, u64)>,
    /// The one file that the rows for a single item act on: the cover of a
    /// grid tile, the member that the viewer shows, or the file that the
    /// preview shows.
    file: Option<File>,
    /// The folder that the grid shows.
    folder: Option<FolderId>,
    /// True for the menu of a collection's member. Then `file` is the
    /// member and `items` holds its collection.
    member: bool,
    /// True for a menu of a collection's view: the menu of a member, or the
    /// menu of the space between the members. The sort and ungroup rows of
    /// such a menu act on that collection.
    group: bool,
    /// The members that a member's menu acts on, each given by its position
    /// in the collection.
    members: Vec<usize>,
    /// True when the menu was opened in Quick Look. Edits to the collection
    /// are then reported to Quick Look, which closes when the collection no
    /// longer exists.
    quick_look: bool,
    /// The number of collections in the selection, as the library counted
    /// them. It is set for a selection that holds items that are not
    /// loaded.
    collections: Option<u64>,
}

#[derive(Clone)]
struct File {
    /// The item that the file belongs to. The item's details hold the
    /// file's path.
    root: RootId,
    hash: String,
    mime: String,
    name: String,
}

impl Target {
    fn ids(&self) -> Vec<RootId> {
        self.items.iter().map(|item| item.root_id).collect()
    }
    /// Returns what an edit addresses: the grid's selection, which the
    /// library resolves, or else the items themselves.
    pub(super) fn selection(&self) -> SelectionTarget {
        match &self.selection {
            Some((selection, _)) => selection.clone(),
            None => SelectionTarget::Explicit {
                root_ids: self.ids(),
            },
        }
    }

    /// Returns the number of items that the menu acts on, whether they are
    /// loaded or not.
    pub(super) fn count(&self) -> u64 {
        match &self.selection {
            Some((_, count)) => *count,
            None => self.items.len() as u64,
        }
    }

    /// Returns whether every item that the menu acts on is loaded. The
    /// names and kinds in `items` then cover all of them.
    fn loaded(&self) -> bool {
        self.count() == self.items.len() as u64
    }
}

/// A question that the user answers in a confirmation dialog before an edit
/// is made.
struct Confirmation {
    title: SharedString,
    message: SharedString,
    label: SharedString,
    danger: bool,
    run: Run,
    /// Runs when the user cancels the question or closes the dialog.
    cancelled: Option<Run>,
}

pub(super) type Run = Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

/// Returns the link that "Copy Link" puts on the clipboard for a file: the
/// file's content hash, followed by the extension of its type, or by `bin`
/// for a type that is not listed here. Picto 0.6.19 and earlier copied links
/// of the same form.
pub(crate) fn media_link(hash: &str, mime: &str) -> String {
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

impl Shell {
    // The functions from here to `last_folder_name` open the menus.

    /// Returns the grid's selection as the target of a menu, whether its
    /// items are loaded or not. When exactly one item is selected and it is
    /// loaded, the target also holds that item's cover file.
    fn selection_target(&self, cx: &App) -> Option<Target> {
        let session = self.session.read(cx);
        let selection = session.selection_target()?;
        let count = session.selection_count();
        let items: Vec<RootSummary> = session.selected_loaded().into_iter().cloned().collect();
        let file = match items.as_slice() {
            [item] if count == 1 => Some(File {
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
            selection: Some((selection, count)),
            folder: self.grid_scope(cx).folder(),
            ..Default::default()
        })
    }

    /// Performs a row's action without opening a menu. A shortcut or a row
    /// of the application menu calls this. While the viewer is open, the
    /// action applies to the item that the viewer shows, in the folder that
    /// the grid shows. Otherwise it applies to the selection.
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
                folder: self.grid_scope(cx).folder(),
                ..Default::default()
            })
        } else {
            self.selection_target(cx)
        };
        if let Some(target) = target {
            self.items.target = Some(target);
            self.items.surface = None;
            self.run_item_action(action, window, cx);
        }
    }

    /// Opens the menu of the selection after a tile was right-clicked. If
    /// the tile was not selected, the grid has already made it the
    /// selection.
    pub(super) fn open_tile_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.items.surface = None;
        let grid = self.grid_scope(cx);
        let folder = grid.folder();
        let Some(target) = self.selection_target(cx) else {
            return;
        };
        // This is the number of selected items, whether they are loaded or
        // not. The rows for a single item are offered only when it is 1, so
        // they are not offered when a second selected item is not loaded.
        let count = target.count() as usize;
        let items = &target.items;
        let collections = items
            .iter()
            .filter(|item| item.kind == RootKind::Collection)
            .count();
        let single = items.first().filter(|_| count == 1);
        let status = grid.tile_status(shared_lifecycle(items));
        let media = single.is_some_and(|item| item.kind == RootKind::Media);
        let last_folder_name = self.last_folder_name(cx);
        let whole = self.session.read(cx).selects_whole();
        // When the loaded items are the whole selection, they say how many
        // collections it holds. Otherwise only the library knows, and the
        // menu asks the library once it is open (`count_collections`).
        let loaded = !whole && target.loaded();
        let description = Description {
            surface: Surface::Grid,
            // When the whole destination is selected, this is the number of
            // all its items.
            selection_count: count,
            query_selection: whole,
            ai_tag_enabled: Some(count > 0),
            single: single.is_some(),
            single_hash: single.map(|item| item.content_hash.clone()),
            single_item_id: single.map(|item| item.root_id.0 as i64),
            single_kind: single.map(|item| kind(item.kind)),
            single_name: single.map(|item| item.name.clone()),
            single_mime: single.map(|item| item.mime.clone()),
            contains_group: collections > 0,
            collection_count: Some(loaded.then_some(collections)),
            scope: Some(grid.menu_kind()),
            status,
            grayscale: self.session.read(cx).grayscale,
            clipboard_tags: !self.items.tag_clipboard.is_empty(),
            open_with: self.open_with_for(media.then_some(single).flatten()),
            handlers: Handlers {
                open_default: true,
                open_with_application: true,
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
                ungroup: collections > 0 || !loaded,
                sort_contents: folder.is_some(),
                toggle_grayscale: true,
                ..Default::default()
            },
            last_folder_name,
            ..Default::default()
        };
        let pending = description.open_with == OpenWith::Pending;
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::tile_menu(&description, &hints);
        let file = target.file.clone();
        let selection = target.selection();
        let menu = self.show_item_menu(built, target, true, position, window, cx);
        if pending && let Some(file) = file {
            self.look_up_applications(file, menu.downgrade(), cx);
        }
        if !loaded {
            let organize = count > 1 && collections == 0;
            self.count_collections(selection, organize, menu.downgrade(), cx);
        }
    }

    /// Asks the library how many collections the selection holds. This is
    /// done for a selection that holds items that are not loaded. If the
    /// menu is still open when the answer arrives, the Ungroup row without
    /// a count is replaced by one with the count, or removed when the count
    /// is 0. `organize` says that the menu offers the group row and that no
    /// loaded item is a collection. When it is set and the count is 0, the
    /// row "Merge into One Group..." is replaced by the row "Group...".
    fn count_collections(
        &mut self,
        selection: SelectionTarget,
        organize: bool,
        menu: WeakEntity<Menu>,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.read(cx);
        let job = session.run(move |library| library.selection_collection_count(&selection));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(count)) = job.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.items.open.as_ref() != Some(&menu) {
                    return;
                }
                if let Some(target) = &mut this.items.target {
                    target.collections = Some(count);
                }
                let mut replace = |label: String, rows: Vec<Row>, this: &mut Self| {
                    this.items.actions.extend(rows::actions(&rows));
                    let entries = rows::entries(rows, &mut |_| ui::menu::Entry::Separator);
                    let _ = menu.update(cx, |menu, cx| menu.replace_entry(&label, entries, cx));
                };
                let ungroup = (count > 0)
                    .then(|| rows::ungroup_row(Some(count as usize)))
                    .into_iter()
                    .collect();
                replace(rows::ungroup_row_label(None), ungroup, this);
                if count == 0 && organize {
                    replace(
                        rows::group_row_label(true),
                        vec![rows::group_row(false)],
                        this,
                    );
                }
            });
        })
        .detach();
    }

    /// Returns how "Open With Other" is offered for a media item. On macOS
    /// it lists the applications that open the item's type once they have
    /// been looked up, and is pending until then. On Windows it opens the
    /// system's application chooser. It is not offered on Linux, nor for
    /// `None` or an item without a content hash.
    fn open_with_for(&self, media: Option<&RootSummary>) -> OpenWith {
        let Some(item) = media.filter(|item| !item.content_hash.is_empty()) else {
            return OpenWith::None;
        };
        if cfg!(target_os = "macos") {
            match self.items.applications.get(&type_key(&item.mime)) {
                Some(applications) => OpenWith::Applications(applications.clone()),
                None => OpenWith::Pending,
            }
        } else if cfg!(target_os = "windows") {
            OpenWith::Chooser
        } else {
            OpenWith::None
        }
    }

    /// Looks up the applications that open the type of `file` on a
    /// background thread and keeps them for that type. If the menu is still
    /// open, the pending row is replaced by the rows of the applications.
    /// The pending row is removed when there are no applications or when
    /// the file's path could not be read.
    fn look_up_applications(&mut self, file: File, menu: WeakEntity<Menu>, cx: &mut Context<Self>) {
        let key = type_key(&file.mime);
        let File { root, hash, .. } = file;
        crate::session::read_then(
            &self.session,
            move |library| file_path(library, root, &hash),
            cx,
            move |_, path, cx| {
                let Ok(Some(path)) = path else {
                    let _ = menu.update(cx, |menu, cx| {
                        menu.replace_entry(t("Open With Other"), Vec::new(), cx)
                    });
                    return;
                };
                let found = cx
                    .background_executor()
                    .spawn(async move { crate::desktop::applications(&path) });
                cx.spawn(async move |this, cx| {
                    let applications = found.await;
                    let _ = this.update(cx, |this, cx| {
                        this.items.applications.insert(key, applications.clone());
                        let rows = rows::open_with_rows(applications);
                        this.items.actions.extend(rows::actions(&rows));
                        let _ = menu.update(cx, |menu, cx| {
                            let entries = rows::entries(rows, &mut |_| ui::menu::Entry::Separator);
                            menu.replace_entry(t("Open With Other"), entries, cx);
                        });
                    });
                })
                .detach();
            },
        );
    }

    /// Opens the menu of empty grid space after it was right-clicked.
    pub(super) fn open_empty_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.items.surface = None;
        let grid = self.grid_scope(cx);
        let folder = grid.folder();
        let can_import = grid.imports();
        // The paste row is offered when this destination takes imports and
        // the clipboard holds files or an image.
        let clipboard_import = can_import && super::import::can_paste(cx);
        let description = Description {
            scope: Some(grid.menu_kind()),
            handlers: Handlers {
                new_folder: true,
                new_smart_folder: true,
                import_files: can_import,
                import_folder: can_import,
                paste_import: clipboard_import,
                sort_contents: folder.is_some(),
                ..Default::default()
            },
            import_blocked: crate::imports::blocked(cx).is_some(),
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

    /// Returns the item that the viewer and the preview act on, which is
    /// the selected item, together with one of its files. With `member`
    /// set, the file is the member that the viewer shows. Otherwise it is
    /// the item's cover, which the preview shows.
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

    /// Opens the viewer's menu for the item that the viewer shows, after the
    /// viewer was right-clicked.
    pub(super) fn open_viewer_menu(
        &mut self,
        request: &MenuRequested,
        surface: Entity<MediaSurface>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = request.position;
        let quick_look = self.quick_look.read(cx).surface().entity_id() == surface.entity_id();
        if request.collection {
            self.open_collection_menu(position, quick_look, window, cx);
            return;
        }
        let Some((item, mut file)) = self.shown_item(true, cx) else {
            return;
        };
        if request.member {
            // The menu of a member uses the name of the member's own media,
            // and not the collection's name.
            let session = self.session.read(cx);
            if let Some(media) = session
                .selected_details()
                .and_then(|details| details.media.get(session.member))
            {
                file.name = media.media_name.clone();
            }
        }
        // The menu of a collection's member describes the member as a media
        // item that has no lifecycle of its own, and its edits go to the
        // collection. A member that is open by itself (`request.alone`) gets
        // the menu of a file that has no item to edit.
        let member = request.member;
        let media = member || item.kind == RootKind::Media;
        let trash = !member && item.lifecycle == Lifecycle::Trash;
        let count = request.members.len().max(1);
        let image = file.mime.starts_with("image/");
        let mut description = Description {
            surface: Surface::Viewer,
            selection_count: 1,
            ai_tag_enabled: Some(media && image),
            single: true,
            single_hash: Some(file.hash.clone()),
            single_kind: Some(if member { Kind::Media } else { kind(item.kind) }),
            single_name: Some(if member {
                file.name.clone()
            } else {
                item.name.clone()
            }),
            single_mime: Some(file.mime.clone()),
            contains_group: !media,
            status: (!member).then_some(Status::from(item.lifecycle)),
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
                ai_tagger: media && image,
                copy_tags: true,
                paste_tags: true,
                set_rating: true,
                export: true,
                // In the viewer's menu, Export Originals... exports the files
                // of the item that the viewer shows.
                export_originals: true,
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
        if request.alone {
            // There is no item to edit, so the menu has only the rows that
            // act on the file.
            description.selection_count = 0;
            description.ai_tag_enabled = Some(image);
            description.status = None;
            description.handlers = Handlers {
                open_default: true,
                reveal: true,
                open_new_window: true,
                copy_file: true,
                copy_path: true,
                copy_name: true,
                copy_link: true,
                search_by_image: true,
                regenerate: true,
                library_cover: true,
                ..Default::default()
            };
        } else if member {
            // The menu acts on the members in `request.members`: the members
            // selected in the collection's editor, or the one member that
            // was right-clicked.
            let mimes: Vec<String> = {
                let session = self.session.read(cx);
                let details = session.selected_details();
                request
                    .members
                    .iter()
                    .filter_map(|index| details.and_then(|details| details.media.get(*index)))
                    .map(|media| media.facts.mime.clone())
                    .collect()
            };
            let single = count == 1;
            description.surface = if request.editor {
                Surface::Grid
            } else {
                Surface::Viewer
            };
            description.selection_count = count;
            description.ai_tag_enabled = Some(mimes.iter().all(|mime| mime.starts_with("image/")));
            description.single = single;
            if !single {
                description.single_hash = None;
                description.single_kind = None;
                description.single_name = None;
                description.single_mime = None;
            } else {
                description.single_item_id = self
                    .session
                    .read(cx)
                    .selected_details()
                    .and_then(|details| details.media.get(request.members[0]))
                    .map(|media| i64::from(media.media_id.0));
            }
            description.contains_group = false;
            description.handlers = rows::member_handlers(single, request.editor);
            description.sort_fields = Some(rows::COLLECTION_SORT.to_vec());
        }
        // `capture` says whether the surface can capture the current frame
        // of a video or Flash movie, which the menu then offers to set as
        // the thumbnail.
        let playback = Playback {
            flash: surface.read(cx).flash_playback(),
            capture: surface.read(cx).can_capture(cx),
        };
        self.items.surface = Some(surface.downgrade());
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let mut built = rows::viewer_menu(&description, playback, &hints);
        if member && !request.alone {
            built = rows::with_remove_from_group(built, count);
        }
        let target = Target {
            items: vec![item],
            file: Some(file),
            member,
            group: member && !request.alone,
            members: request.members.clone(),
            quick_look,
            ..Default::default()
        };
        // The menu of a collection's member has the search field, as the
        // grid's menus have. The viewer's menu of any other item does not.
        self.show_item_menu(
            built,
            target,
            member && !request.alone,
            position,
            window,
            cx,
        );
    }

    /// Opens the menu of the space between a collection's members, or of
    /// the space around the collection's tiles in its editor.
    fn open_collection_menu(
        &mut self,
        position: Point<Pixels>,
        quick_look: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.session.read(cx).selected_item().cloned() else {
            return;
        };
        let target = Target {
            items: vec![item],
            group: true,
            quick_look,
            ..Default::default()
        };
        self.show_item_menu(rows::collection_menu(), target, true, position, window, cx);
    }

    /// Opens the menu of the inspector's preview after it was
    /// right-clicked.
    pub(super) fn open_preview_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.items.surface = None;
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
            ..Default::default()
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
    ) -> Entity<Menu> {
        self.items.actions = rows::actions(&built);
        self.items.target = Some(target);
        // The grid's menus hold the view panels, which are made the first
        // time a row asks for one. A select of a panel that is in a pop-up
        // window opens its list in a pop-up window of its own
        // (`ui::select`), so these menus can extend past the window's edge
        // as the other menus do.
        let mut panels = None;
        let entries = rows::entries(built, &mut |panel| {
            panels
                .get_or_insert_with(|| {
                    view_menu::Panels::new(
                        &self.session,
                        !self.sidebar_collapsed,
                        !self.inspector_collapsed,
                        window,
                        cx,
                    )
                })
                .entry(panel)
        });
        // When a viewer's menu closes, the keyboard focus goes back to the
        // viewer or Quick Look that opened it. When another menu closes,
        // the focus goes to the shell.
        let viewer_menu = self.items.surface.is_some();
        self.menu.returning(if viewer_menu {
            ui::menu::Return::Opener
        } else {
            ui::menu::Return::To(self.focus.clone())
        });
        let menu = cx.new(|cx| Menu::new(entries, position, cx).search(search));
        self.items.open = Some(menu.downgrade());
        if let Some(panels) = panels {
            self.watch_view_panels(&menu, panels, cx);
        }
        self.menu
            .open(menu.clone(), window, cx, |this, menu, key, window, cx| {
                let action = this
                    .items
                    .actions
                    .iter()
                    .copied()
                    .find(|action| action.key() == key);
                if let Some(action) = action {
                    this.items.chosen = menu.read(cx).chosen_value().map(String::from);
                    this.run_item_action(action, window, cx);
                }
            });
        menu
    }

    fn last_folder_name(&self, cx: &App) -> Option<String> {
        // The row "Add to “…”" names the folder that was used last, which
        // is the first of the session's recent folders.
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

    // The functions from here on open and draw the confirmation dialog and
    // the group organizer.

    #[allow(clippy::too_many_arguments)]
    pub(super) fn ask(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        label: impl Into<SharedString>,
        danger: bool,
        run: Run,
        cx: &mut Context<Self>,
    ) {
        self.ask_with(title, message, label, danger, run, None, cx);
    }

    /// Opens a confirmation dialog, as `ask` does. `run` runs when the user
    /// confirms, and `cancelled` runs when the user cancels the question or
    /// closes the dialog.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ask_with(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        label: impl Into<SharedString>,
        danger: bool,
        run: Run,
        cancelled: Option<Run>,
        cx: &mut Context<Self>,
    ) {
        let confirmation = Confirmation {
            title: title.into(),
            message: message.into(),
            label: label.into(),
            danger,
            run,
            cancelled,
        };
        // When the dialog closes over a viewer, the keyboard focus goes back
        // to the viewer, so that the viewer's keys keep working. Over the
        // grid, the focus goes back to the shell.
        let back = if self.viewing(cx) {
            ui::menu::Return::Opener
        } else {
            ui::menu::Return::To(self.focus.clone())
        };
        self.items.confirm.open(confirmation, back, cx);
    }

    /// Returns whether a confirmation dialog is open and has not started to
    /// close.
    pub(super) fn asking(&mut self) -> bool {
        self.items
            .confirm
            .get_mut()
            .is_some_and(|dialog| !dialog.presence.closing())
    }

    /// Opens the group organizer dialog on `request`. The dialog closes
    /// when the organizer reports that the group has been made.
    pub(super) fn open_organizer(&mut self, request: organizer::Request, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let content = cx.new(|cx| organizer::Organizer::new(session, request, cx));
        let first = content.read(cx).first_focus(cx);
        cx.subscribe(&content, |this, _, _: &organizer::Organized, cx| {
            this.items.organizer.dismiss(cx);
        })
        .detach();
        cx.observe(&content, |_, _, cx| cx.notify()).detach();
        let back = ui::menu::Return::To(self.focus.clone());
        let dialog = self.items.organizer.open(content, back, cx);
        if let Some(first) = first {
            dialog.focus_first(first);
        }
    }

    /// Returns the open group organizer dialog as an element, or `None`
    /// when it is not open. The organizer is the dialog's body, and the
    /// Cancel button and the button that makes the group are in its footer.
    pub(super) fn group_organizer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.items.organizer.layer(window, cx)?;
        let organizer = dialog.content.clone();
        let (title, label, saving, ready) = {
            let organizer = organizer.read(cx);
            (
                organizer.title(),
                organizer.submit_label(),
                organizer.saving(),
                organizer.can_submit(cx),
            )
        };
        let close = cx.listener(|this, _: &(), _, cx| this.items.organizer.dismiss(cx));
        let cancel = cx.listener(|this, _: &(), _, cx| {
            this.items.organizer.close(cx);
        });
        let submit = organizer.downgrade();
        Some(
            ui::modal::modal("group-organizer", title, dialog.presence)
                .size(ui::modal::Size::Medium)
                .focus(&dialog.focus)
                .on_close(move |window, cx| close(&(), window, cx))
                .child(organizer)
                .footer(
                    ui::action_button::action_button(
                        "group-organizer-cancel",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Cancel"))
                    .disabled(saving)
                    .on_click(move |_, window, cx| cancel(&(), window, cx)),
                )
                .footer(
                    ui::action_button::action_button(
                        "group-organizer-submit",
                        ui::action_button::Look::Primary,
                    )
                    .label(label)
                    .disabled(!ready)
                    .on_click(move |_, _, cx| {
                        let _ = submit.update(cx, |organizer, cx| organizer.submit(cx));
                    }),
                )
                .into_any_element(),
        )
    }

    /// Returns the open confirmation dialog as an element, or `None` when
    /// none is open. When the user confirms and no viewer is open, the
    /// keyboard focus goes to the shell before the confirmed edit runs.
    pub(super) fn confirmation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.items.confirm.layer(window, cx)?;
        let confirm = &dialog.content;
        let run = confirm.run.clone();
        let refused = confirm.cancelled.clone();
        let close = cx.listener({
            let refused = refused.clone();
            move |this, _: &(), window, cx| {
                this.items.confirm.dismiss(cx);
                if let Some(refused) = &refused {
                    refused(this, window, cx);
                }
            }
        });
        let cancel = cx.listener(move |this, _: &(), window, cx| {
            // Cancel removes the dialog at once, without a fade-out.
            this.items.confirm.close(cx);
            if let Some(refused) = &refused {
                refused(this, window, cx);
            }
        });
        let accept = cx.listener(move |this, _: &(), window, cx| {
            this.items.confirm.dismiss(cx);
            if !this.viewing(cx) {
                window.focus(&this.focus, cx);
            }
            run(this, window, cx);
        });
        Some(
            ui::confirm::confirm(
                "item-confirm",
                confirm.title.clone(),
                confirm.message.clone(),
                confirm.label.clone(),
                dialog.presence,
            )
            .danger(confirm.danger)
            .focus(&dialog.focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }
}

/// Returns the key under which the applications that open a type are kept:
/// the file extension of the type.
fn type_key(mime: &str) -> String {
    picto_core::blob_store::mime_to_extension(mime).to_owned()
}

/// Returns the path at which the library keeps the file of the item `root`
/// whose content hash is `hash`, or `None` when the item has no such file.
fn file_path(
    library: &picto_library::Library,
    root: RootId,
    hash: &str,
) -> picto_library::Result<Option<std::path::PathBuf>> {
    Ok(library
        .details(root)?
        .media
        .into_iter()
        .find(|media| media.facts.content_hash == hash)
        .map(|media| std::path::PathBuf::from(media.file_path)))
}

fn kind(kind: RootKind) -> Kind {
    match kind {
        RootKind::Media => Kind::Media,
        RootKind::Collection => Kind::Collection,
    }
}

#[cfg(test)]
mod tests {
    use super::media_link;

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
