//! What the registry's shortcuts do to the window's content, as
//! `GridScreen`'s, `AppShell`'s, `Sidebar`'s and the application menu's
//! handlers do. The keys and the contexts they apply in are in
//! `crate::keymap`; a handler that finds nothing to act on lets the key go
//! on (`cx.propagate()`), as an unhandled key does in production.
use super::Shell;
use crate::keymap::*;
use crate::session::Reread;
use crate::settings;
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, Lifecycle, Rating, RootId, RootKind};

/// The grid's destination, for the shortcuts that act by destination.
pub(super) struct ScopeKind {
    pub folder: Option<FolderId>,
    pub trash: bool,
    pub inbox: bool,
}

impl Shell {
    pub(super) fn scope_kind(&self, cx: &App) -> ScopeKind {
        let session = self.session.read(cx);
        let scope = session
            .nav
            .get(session.displayed)
            .filter(|entry| entry.screen.is_none())
            .map(|entry| &entry.scope);
        ScopeKind {
            folder: match scope {
                Some(ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id }) => {
                    Some(*folder_id)
                }
                _ => None,
            },
            trash: matches!(scope, Some(ItemScope::Trash)),
            inbox: matches!(scope, Some(ItemScope::Inbox)),
        }
    }

    /// The selected items, in grid order.
    fn selected_ids(&self, cx: &App) -> Vec<RootId> {
        self.session.read(cx).selected_ids()
    }

    /// The one selected item, unless it is a collection (`singleFileHash`
    /// with `kind !== 'collection'`).
    fn single_media(&self, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.selection_count() == 1
            && session
                .selected_item()
                .is_some_and(|item| item.kind != RootKind::Collection)
    }

    /// Whether the grid's selection is what the window shows: the grid or
    /// the viewer, not a screen.
    fn selection_shown(&self, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.screen().is_none() || session.viewer
    }

    /// `view.detailView`: opens the viewer on the one selected item.
    pub(super) fn open_selected(&mut self, cx: &mut Context<Self>) {
        if self.session.read(cx).selection_count() == 1 {
            self.session
                .update(cx, |session, cx| session.open_viewer(cx));
        } else {
            cx.propagate();
        }
    }

    /// `view.quicklook`: Space scrolls the grid when the Controls setting
    /// says so; otherwise it opens Quick Look on the one selected item.
    fn quick_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let setting = settings::current(cx).space_key_action;
        let action = crate::viewer::trigger::space(setting, self.trigger_state(cx));
        self.perform(action, window, cx);
    }

    /// Opens a built-in destination (`menu:navigate`).
    fn show_scope(&mut self, wanted: ItemScope, cx: &mut Context<Self>) {
        let index = self
            .session
            .read(cx)
            .nav
            .iter()
            .position(|entry| entry.screen.is_none() && !entry.random && entry.scope == wanted);
        match index {
            Some(index) => self
                .session
                .update(cx, |session, cx| session.navigate(index, cx)),
            None => cx.propagate(),
        }
    }

    /// `nav.search`: the grid's search field, or the Tags screen's.
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (screen, viewer) = {
            let session = self.session.read(cx);
            (session.screen(), session.viewer)
        };
        let field = match screen {
            _ if viewer => None,
            None => Some(self.toolbar.read(cx).search_focus(cx)),
            Some(crate::backend::Screen::Tags) => {
                Some(self.tags.read(cx).toolbar().read(cx).search_focus(cx))
            }
            Some(_) => None,
        };
        match field {
            Some(field) => window.focus(&field, cx),
            None => cx.propagate(),
        }
    }

    /// `rate.0`–`rate.5`: the selection in the grid, the item shown in the
    /// viewer.
    fn rate(&mut self, stars: usize, cx: &mut Context<Self>) {
        let rating = Rating::ALL[stars];
        let session = self.session.read(cx);
        let ids = if session.viewer {
            session
                .selected_item()
                .map(|item| vec![item.root_id])
                .unwrap_or_default()
        } else {
            self.selected_ids(cx)
        };
        if ids.is_empty() {
            if !self.session.read(cx).viewer {
                cx.propagate();
            }
            return;
        }
        self.edit_selection(
            ids,
            move |library, target| library.set_rating(target, rating).map(|_| ()),
            false,
            cx,
        );
    }

    /// Runs an edit of these items on the library's thread, then shows its
    /// result: the details and the selection's summary again, or with
    /// `reload` the destination read again in place, when items may have
    /// left it.
    fn edit_selection(
        &mut self,
        ids: Vec<RootId>,
        work: impl FnOnce(&picto_library::Library, &SelectionTarget) -> picto_library::Result<()>
        + Send
        + 'static,
        reload: bool,
        cx: &mut Context<Self>,
    ) {
        let target = self.session.read(cx).target_for(ids);
        let reread = if reload {
            Reread::RESULTS
        } else {
            Reread::SELECTION
        };
        self.session.update(cx, |session, cx| {
            let _ = session.edit(move |library| work(library, &target), reread, cx);
        });
    }

    /// `file.delete`, as `gridRemovalAction` reads it: in a folder the key
    /// belongs to Remove from Folder, in the Trash it deletes permanently,
    /// elsewhere it moves the selection to the Trash.
    fn delete_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.scope_kind(cx);
        let count = self.session.read(cx).selection_count();
        if (kind.folder.is_some() && !kind.trash) || count == 0 {
            cx.propagate();
        } else if kind.trash {
            // Permanently, after the confirmation the menu row shows.
            self.act_on_selection(crate::grid::menu::Action::DeletePermanently, window, cx);
        } else {
            self.session.update(cx, |session, cx| {
                session.move_selection_to(Lifecycle::Trash, cx)
            });
        }
    }

    /// `file.removeFromFolder`: takes the selection out of the open folder.
    fn remove_from_folder(&mut self, cx: &mut Context<Self>) {
        let kind = self.scope_kind(cx);
        match kind.folder {
            Some(folder) if !kind.trash && !self.session.read(cx).selection.is_empty() => {
                let ids = self.selected_ids(cx);
                self.edit_selection(
                    ids,
                    move |library, target| library.remove_from_folder(target, folder).map(|_| ()),
                    true,
                    cx,
                );
            }
            _ => cx.propagate(),
        }
    }

    /// Moves the selection to a lifecycle when the destination is the one
    /// the shortcut belongs to (`file.restore` in the Trash,
    /// `inbox.accept` in the Inbox).
    fn move_in(&mut self, here: bool, lifecycle: Lifecycle, cx: &mut Context<Self>) {
        if here && !self.session.read(cx).selection.is_empty() {
            self.session
                .update(cx, |session, cx| session.move_selection_to(lifecycle, cx));
        } else {
            cx.propagate();
        }
    }

    /// `inbox.reject` (`rejectTarget`): to the Trash, or deleted, as the
    /// Inbox & Deleting setting says.
    fn reject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.scope_kind(cx).inbox || self.session.read(cx).selection.is_empty() {
            cx.propagate();
        } else if settings::current(cx).reject_action == "trash" {
            self.session.update(cx, |session, cx| {
                session.move_selection_to(Lifecycle::Trash, cx)
            });
        } else {
            // Deleted, asking first only when it cannot be undone, as the
            // menu's Reject row does.
            self.act_on_selection(crate::grid::menu::Action::Reject, window, cx);
        }
    }

    /// `edit.copyFilePath`: the selection's file paths, one per line.
    fn copy_file_paths(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected_ids(cx);
        if ids.is_empty() || !self.selection_shown(cx) {
            cx.propagate();
            return;
        }
        if ids.len() == 1 {
            // The shown member of one item, as the context menu copies it.
            self.act_on_selection(crate::grid::menu::Action::CopyPath, window, cx);
            return;
        }
        let job = self.session.read(cx).run(move |library| {
            let mut paths = Vec::new();
            for id in ids {
                paths.extend(
                    library
                        .details(id)?
                        .media
                        .into_iter()
                        .map(|media| media.file_path),
                );
            }
            Ok::<_, picto_library::LibraryError>(paths)
        });
        cx.spawn(async move |_, cx| match job.await {
            Ok(Ok(paths)) => {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n"))));
            }
            Ok(Err(error)) => eprintln!("picto: {error}"),
            Err(_) => {}
        })
        .detach();
    }

    /// `edit.rename`: the inspector's name field for one item.
    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.session.read(cx).selection_count() {
            1 if self.selection_shown(cx) => {
                if self.inspector_collapsed {
                    self.toggle_rails(false, true, cx);
                }
                self.inspector
                    .update(cx, |inspector, cx| inspector.edit_name(window, cx));
            }
            // TODO: several items open the batch rename dialog, which does
            // not exist yet.
            _ => cx.propagate(),
        }
    }

    /// `organize.addTag`, `organize.addFolder` and `file.addToFolder`, and
    /// the item menu's Add Tags and Add to Folder. Over the grid they open
    /// the Add Tags and Folders dialogs for the selection, as `GridScreen`
    /// does; the viewer's menu opens the inspector's pickers instead
    /// (`useViewerEntityContextMenu`), docked 80px below the inspector's
    /// top, or centred while it is hidden.
    fn pick(&mut self, picker: Picker, window: &mut Window, cx: &mut Context<Self>) {
        let (empty, viewer) = {
            let session = self.session.read(cx);
            (session.selection.is_empty(), session.viewer)
        };
        if empty {
            cx.propagate();
            return;
        }
        let visible = !self.inspector_collapsed;
        self.inspector
            .update(cx, |inspector, cx| match (picker, viewer) {
                (Picker::Tags, false) => inspector.open_tag_dialog(cx),
                (Picker::Folders | Picker::AddToFolder, false) => inspector.open_folder_dialog(cx),
                (Picker::Tags, true) => inspector.add_tags(visible, window, cx),
                (Picker::Folders | Picker::AddToFolder, true) => {
                    inspector.add_to_folder(visible, window, cx)
                }
            });
    }

    /// `view.zoomIn` / `view.zoomOut` in the grid: the tile size one step
    /// of the toolbar's slider buttons.
    fn zoom_grid(&mut self, larger: bool, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let size = crate::ui::zoom::step_tile(session.tile, larger);
            if size != session.tile {
                session.set_tile(size, cx);
            }
        });
    }

    /// Wires the registry's actions on the window's root.
    pub(super) fn shortcut_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &GoBack, _, cx| {
            this.session
                .update(cx, |session, cx| session.step_history(-1, cx));
        }))
        .on_action(cx.listener(|this, _: &GoForward, _, cx| {
            this.session
                .update(cx, |session, cx| session.step_history(1, cx));
        }))
        .on_action(cx.listener(|this, _: &GridZoomIn, _, cx| this.zoom_grid(true, cx)))
        .on_action(cx.listener(|this, _: &GridZoomOut, _, cx| this.zoom_grid(false, cx)))
        .on_action(cx.listener(|this, _: &FocusSearch, window, cx| this.focus_search(window, cx)))
        .on_action(cx.listener(|this, _: &ShowAll, _, cx| this.show_scope(ItemScope::All, cx)))
        .on_action(cx.listener(|this, _: &ShowInbox, _, cx| this.show_scope(ItemScope::Inbox, cx)))
        .on_action(
            cx.listener(|this, _: &ShowUntagged, _, cx| this.show_scope(ItemScope::Untagged, cx)),
        )
        .on_action(cx.listener(|this, _: &ShowTrash, _, cx| this.show_scope(ItemScope::Trash, cx)))
        .on_action(cx.listener(|this, _: &QuickLook, window, cx| this.quick_look(window, cx)))
        .on_action(cx.listener(|this, _: &OpenDefaultApp, window, cx| {
            if this.single_media(cx) {
                this.act_on_selection(crate::grid::menu::Action::OpenDefault, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|this, _: &RevealInFinder, window, cx| {
            if this.single_media(cx) {
                this.act_on_selection(crate::grid::menu::Action::Reveal, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(
            cx.listener(|this, _: &CopyFilePath, window, cx| this.copy_file_paths(window, cx)),
        )
        .on_action(cx.listener(|this, _: &Rename, window, cx| this.rename(window, cx)))
        .on_action(cx.listener(|this, _: &Rate0, _, cx| this.rate(0, cx)))
        .on_action(cx.listener(|this, _: &Rate1, _, cx| this.rate(1, cx)))
        .on_action(cx.listener(|this, _: &Rate2, _, cx| this.rate(2, cx)))
        .on_action(cx.listener(|this, _: &Rate3, _, cx| this.rate(3, cx)))
        .on_action(cx.listener(|this, _: &Rate4, _, cx| this.rate(4, cx)))
        .on_action(cx.listener(|this, _: &Rate5, _, cx| this.rate(5, cx)))
        .on_action(cx.listener(|this, _: &DeleteItems, window, cx| this.delete_items(window, cx)))
        .on_action(cx.listener(|this, _: &RemoveFromFolder, _, cx| this.remove_from_folder(cx)))
        .on_action(cx.listener(|this, _: &RestoreItems, _, cx| {
            let trash = this.scope_kind(cx).trash;
            this.move_in(trash, Lifecycle::Active, cx)
        }))
        // Over the media view or Quick Look, the keys review the item
        // being viewed (`InboxReviewControls`); over the grid, the
        // selection.
        .on_action(cx.listener(|this, _: &AcceptInbox, window, cx| {
            if this.viewing(cx) {
                this.review_key(crate::viewer::review::Decision::Accept, window, cx);
                return;
            }
            let inbox = this.scope_kind(cx).inbox;
            this.move_in(inbox, Lifecycle::Active, cx)
        }))
        .on_action(cx.listener(|this, _: &RejectInbox, window, cx| {
            if this.viewing(cx) {
                this.review_key(crate::viewer::review::Decision::Reject, window, cx);
                return;
            }
            this.reject(window, cx)
        }))
        .on_action(cx.listener(|this, _: &NewFolder, window, cx| {
            this.sidebar
                .update(cx, |sidebar, cx| sidebar.new_folder(window, cx));
        }))
        .on_action(cx.listener(|this, _: &NewSubfolder, window, cx| {
            let made = this
                .sidebar
                .update(cx, |sidebar, cx| sidebar.new_subfolder(window, cx));
            if !made {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|this, _: &NewSmartFolder, window, cx| {
            this.sidebar
                .update(cx, |sidebar, cx| sidebar.new_top_smart_folder(window, cx));
        }))
        .on_action(cx.listener(|this, _: &FolderAutoTags, window, cx| {
            let opened = this
                .sidebar
                .update(cx, |sidebar, cx| sidebar.auto_tags_shortcut(window, cx));
            if !opened {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|this, _: &AddTags, window, cx| this.pick(Picker::Tags, window, cx)))
        .on_action(
            cx.listener(|this, _: &AddToFolders, window, cx| {
                this.pick(Picker::Folders, window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &AddToFolder, window, cx| {
            this.pick(Picker::AddToFolder, window, cx)
        }))
    }
}

/// The pickers the organize shortcuts open.
#[derive(Clone, Copy)]
enum Picker {
    Tags,
    Folders,
    AddToFolder,
}
