//! The item menus the shell shows: a tile's and empty grid space's
//! (`GridScreen.tsx`), the viewer's (`useViewerEntityContextMenu.tsx`) and the
//! inspector preview's. The rows come from [`crate::grid::menu`]; this
//! describes what was right-clicked, as the Electron screens do, and performs
//! the chosen row through the session and the library.
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
    confirm: ui::modal::Host<Confirmation>,
    /// The group organizer, when grouping needs a decision.
    organizer: ui::modal::Host<Entity<organizer::Organizer>>,
    /// The media surface the open viewer menu was opened on (the media
    /// view's or Quick Look's), whose Flash movie its rows control.
    surface: Option<WeakEntity<MediaSurface>>,
    /// The value of the row last chosen (an application's path).
    chosen: Option<String>,
    /// The menu open now, for rows that change it while it stays open.
    open: Option<WeakEntity<Menu>>,
    /// The applications that open each type, looked up once per extension
    /// as production caches them (`openWithOptionsByExtension`).
    applications: std::collections::HashMap<String, Vec<crate::desktop::Application>>,
}

/// The items a menu acts on.
#[derive(Clone, Default)]
pub(super) struct Target {
    /// In the order the grid shows them.
    items: Vec<RootSummary>,
    /// The one file the single-item rows act on: the cover of a grid tile,
    /// the member the viewer shows, or the preview's.
    file: Option<File>,
    /// The folder the grid shows.
    folder: Option<FolderId>,
    /// A collection member's menu: `file` is the member, `items` its
    /// collection.
    member: bool,
    /// A menu of a collection's view (a member's, or the space between
    /// them): its sort and ungroup rows act on that collection.
    group: bool,
    /// The members a member's menu acts on, by their place in the
    /// collection.
    members: Vec<usize>,
    /// Quick Look opened it, which closes when the collection is gone.
    quick_look: bool,
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
    /// What an edit of these items addresses: the destination's query when
    /// they are the whole destination selected, else the items themselves.
    pub(super) fn selection(&self, session: &crate::session::Session) -> SelectionTarget {
        session.target_for(self.ids())
    }
}

/// A question asked before an edit (`ConfirmModal`).
struct Confirmation {
    title: SharedString,
    message: SharedString,
    label: SharedString,
    danger: bool,
    run: Run,
    /// Run when the question is cancelled or closed.
    cancelled: Option<Run>,
}

pub(super) type Run = Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

/// `mediaLinkFor`: the link the Electron app serves a file at.
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
    // ── Opening ─────────────────────────────────────────────────

    /// The grid's selection, in grid order, with the one item's cover file.
    fn selection_target(&self, cx: &App) -> Option<Target> {
        let session = self.session.read(cx);
        let items: Vec<RootSummary> = session.selected_loaded().into_iter().cloned().collect();
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
            folder: self.grid_scope(cx).folder(),
            ..Default::default()
        })
    }

    /// Performs a row's action without its menu, for a shortcut or an
    /// application menu row: on the item the viewer shows while it is open
    /// (in the folder it was opened from), else on the selection.
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

    /// A tile was right-clicked: the selection's menu. The grid has already
    /// made the tile the selection when it was outside it.
    pub(super) fn open_tile_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.items.surface = None;
        let grid = self.grid_scope(cx);
        let folder = grid.folder();
        let Some(Target { items, file, .. }) = self.selection_target(cx) else {
            return;
        };
        let count = items.len();
        let collections = items
            .iter()
            .filter(|item| item.kind == RootKind::Collection)
            .count();
        let single = (count == 1).then(|| &items[0]);
        let status = grid.tile_status(shared_lifecycle(&items));
        let media = single.is_some_and(|item| item.kind == RootKind::Media);
        let last_folder_name = self.last_folder_name(cx);
        let whole = self.session.read(cx).selects_whole();
        let description = Description {
            surface: Surface::Grid,
            // With the whole destination selected, all of it.
            selection_count: self.session.read(cx).selection_count() as usize,
            query_selection: whole,
            ai_tag_enabled: Some(count > 0),
            single: single.is_some(),
            single_hash: single.map(|item| item.content_hash.clone()),
            single_item_id: single.map(|item| item.root_id.0 as i64),
            single_kind: single.map(|item| kind(item.kind)),
            single_name: single.map(|item| item.name.clone()),
            single_mime: single.map(|item| item.mime.clone()),
            contains_group: collections > 0,
            // The loaded items say how many collections a whole destination
            // holds only when it is all loaded; otherwise the count is not
            // known here.
            // TODO: count the collections the destination's query holds, as
            // production looks them up, and name the number.
            collection_count: Some(
                (!whole || self.session.read(cx).selection_count() as usize == count)
                    .then_some(collections),
            ),
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
                ungroup: collections > 0,
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
        let target = Target {
            items,
            file: file.clone(),
            folder,
            ..Default::default()
        };
        let menu = self.show_item_menu(built, target, true, position, window, cx);
        if pending && let Some(file) = file {
            self.look_up_applications(file, menu.downgrade(), cx);
        }
    }

    /// How "Open With Other" is offered for a media item: on macOS its
    /// applications, once looked up for its type, and pending until then;
    /// on Windows the system's chooser; nothing elsewhere.
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

    /// Looks up the applications that open `file`'s type off the thread
    /// that draws, keeps them for the type, and puts them in place of the
    /// pending row if the menu is still open (no row when there are none,
    /// or when the lookup failed).
    fn look_up_applications(&mut self, file: File, menu: WeakEntity<Menu>, cx: &mut Context<Self>) {
        let key = type_key(&file.mime);
        let File { root, hash, .. } = file;
        crate::session::edit_then(
            &self.session,
            crate::session::Report::default(),
            crate::session::Reread::NOTHING,
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

    /// Empty grid space was right-clicked (`openEmptyGridContextMenu`).
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
        // `hasClipboardImport`: files or an image on the clipboard.
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
            // A member is named by its own media (`memberToGridItem`).
            let session = self.session.read(cx);
            if let Some(media) = session
                .selected_details()
                .and_then(|details| details.media.get(session.member))
            {
                file.name = media.media_name.clone();
            }
        }
        // A member's menu (`openMemberMenu`) describes the member, a media
        // item, with no lifecycle of its own; its edits go to the
        // collection. A member open by itself has `MediaView`'s menu for a
        // file with no item to edit.
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
                // Production's viewer menu leaves Export Originals... with
                // nothing to do; here it exports the shown item's files.
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
            // No item to edit: the file's own rows only.
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
            // `openMemberMenu`: the editor's selection, or the one member.
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
        // The current frame of a video or Flash movie can become the
        // thumbnail.
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
        // A collection member's menu has the search field, as the grid's
        // menus do (`contextMenu.openAt`).
        self.show_item_menu(
            built,
            target,
            member && !request.alone,
            position,
            window,
            cx,
        );
    }

    /// `openEmptyMenu`: the space between a collection's members, or
    /// around its tiles in the editor.
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

    /// The inspector's preview was right-clicked.
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
        // The grid's menus hold the view panels. In pop-up windows their
        // selects open lists of their own (`ui::select`), so these menus
        // reach past the window's edge as the others do.
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
        // The viewer's menus give the keyboard back to the viewer or Quick
        // Look that opened them; the others to the shell.
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

    // ── Confirmation ────────────────────────────────────────────

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

    /// The same, running `cancelled` when the question is turned down.
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
        // Over a viewer the keyboard goes back to it, so its keys go on
        // working; over the grid, to the grid.
        let back = if self.viewing(cx) {
            ui::menu::Return::Opener
        } else {
            ui::menu::Return::To(self.focus.clone())
        };
        self.items.confirm.open(confirmation, back, cx);
    }

    /// Whether a question is waiting for an answer (`confirmModalAtom`).
    pub(super) fn asking(&mut self) -> bool {
        self.items
            .confirm
            .get_mut()
            .is_some_and(|dialog| !dialog.presence.closing())
    }

    /// Opens the group organizer on `request` (`groupOrganizerModalAtom`).
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

    /// The open group organizer: the decision in its body, Cancel and the
    /// grouping button in its footer.
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

    /// The open confirmation, drawn over everything. The keyboard goes back
    /// to the grid as it leaves.
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
            // Cancel takes the dialog away at once.
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

/// What production caches applications by: the file's extension.
fn type_key(mime: &str) -> String {
    picto_core::blob_store::mime_to_extension(mime).to_owned()
}

/// `resolveFilePath`: where the library keeps the item's file with this
/// content.
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
