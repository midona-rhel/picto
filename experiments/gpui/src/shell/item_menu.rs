//! The item menus the shell shows: a tile's and empty grid space's
//! (`GridScreen.tsx`), the viewer's (`useViewerEntityContextMenu.tsx`) and the
//! inspector preview's. The rows come from [`crate::grid::menu`]; this
//! describes what was right-clicked, as the Electron screens do, and performs
//! the chosen row through the session and the library.
use super::{Shell, view_menu};
use crate::grid::menu::{
    self as rows, Action, Description, Handlers, Kind, OpenWith, Playback, Row, ScopeKind, Status,
    Surface,
};
use crate::ui::{self, menu::Menu};
use crate::viewer::{MenuRequested, surface::MediaSurface};
use gpui::*;
use picto_library::query::{ItemScope, RootSummary};
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, Lifecycle, RootId, RootKind};
use std::rc::Rc;

mod actions;

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
    /// The media surface the open viewer menu was opened on (the media
    /// view's or Quick Look's), whose Flash movie its rows control.
    surface: Option<WeakEntity<MediaSurface>>,
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
            folder: self.grid_scope(cx).2,
            member: false,
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
                member: false,
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
            // With the whole destination selected, all of it.
            selection_count: self.session.read(cx).selection_count() as usize,
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
            // TODO: on macOS, look up the applications that open this type
            // (`OpenWith::Pending` while that runs) and list them, as
            // production does; until then this offers the system chooser's
            // one row.
            open_with: if media {
                OpenWith::Chooser
            } else {
                OpenWith::None
            },
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
            member: false,
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
        self.items.surface = None;
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
        let clipboard_import = can_import && super::import::can_paste(cx);
        let description = Description {
            scope: Some(scope_kind),
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
        let (position, flash) = (request.position, request.flash);
        let Some((item, file)) = self.shown_item(true, cx) else {
            return;
        };
        // A member's menu (`openMemberMenu`) describes the member, a media
        // item, with no lifecycle of its own; its edits go to the
        // collection.
        let member = request.member;
        let media = member || item.kind == RootKind::Media;
        let trash = !member && item.lifecycle == Lifecycle::Trash;
        let description = Description {
            surface: Surface::Viewer,
            selection_count: 1,
            ai_tag_enabled: Some(media && file.mime.starts_with("image/")),
            single: true,
            single_hash: Some(file.hash.clone()),
            single_kind: Some(if member { Kind::Media } else { kind(item.kind) }),
            single_name: Some(item.name.clone()),
            single_mime: Some(file.mime.clone()),
            contains_group: !media,
            status: (!member).then_some(match item.lifecycle {
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
            flash: surface.read(cx).flash_playback(),
            capture: flash,
        };
        self.items.surface = Some(surface.downgrade());
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::viewer_menu(&description, playback, &hints);
        let target = Target {
            items: vec![item],
            file: Some(file),
            folder: None,
            member,
        };
        // A collection member's menu has the search field, as the grid's
        // menus do (`contextMenu.openAt`).
        // TODO: it also has "Remove from Group" above "Move to Trash", and
        // its Trash row moves the member out of the collection into the
        // Trash (`detachMembers`).
        self.show_item_menu(built, target, request.member, position, window, cx);
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
            folder: None,
            member: false,
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
        if let Some(panels) = panels {
            self.watch_view_panels(&menu, panels, cx);
        }
        self.menu
            .open(menu, window, cx, |this, _, key, window, cx| {
                let action = this
                    .items
                    .actions
                    .iter()
                    .copied()
                    .find(|action| action.key() == key);
                if let Some(action) = action {
                    this.run_item_action(action, window, cx);
                }
            });
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
