//! What the registry's shortcuts do to the window's content, as
//! `GridScreen`'s, `AppShell`'s, `Sidebar`'s and the application menu's
//! handlers do. The keys and the contexts they apply in are in
//! `crate::keymap`; a handler that finds nothing to act on lets the key go
//! on (`cx.propagate()`), as an unhandled key does in production.
use super::Shell;
use crate::app_menu::ScopeKind;
use crate::grid::menu::Action as Row;
use crate::keymap::*;
use crate::settings;
use gpui::*;
use picto_library::RootKind;
use picto_library::query::ItemScope;

impl Shell {
    /// Whether the grid shows this kind of destination.
    fn in_scope(&self, kind: ScopeKind, cx: &App) -> bool {
        self.grid_scope(cx).kind() == kind
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
    /// viewer, as Set Rating's rows rate them.
    fn rate(&mut self, stars: u8, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        if !session.viewer && session.selection.is_empty() {
            cx.propagate();
            return;
        }
        self.act_on_selection(Row::Rate(stars), window, cx);
    }

    /// Runs a menu row on the selection when there is one, else lets the
    /// key go on.
    fn on_selection(
        &mut self,
        applies: bool,
        row: Row,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if applies && !self.session.read(cx).selection.is_empty() {
            self.act_on_selection(row, window, cx);
        } else {
            cx.propagate();
        }
    }

    /// `file.delete`, as `gridRemovalAction` reads it: in a folder the key
    /// belongs to Remove from Folder, in the Trash it deletes permanently
    /// (after the confirmation the menu row shows), elsewhere it moves the
    /// selection to the Trash.
    fn delete_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let grid = self.grid_scope(cx);
        let row = if grid.kind() == ScopeKind::Trash {
            Row::DeletePermanently
        } else {
            Row::MoveToTrash
        };
        self.on_selection(grid.folder().is_none(), row, window, cx);
    }

    /// `edit.copyFilePath`: the file shown of one item, as the context menu
    /// copies it, or every file of several, one per line.
    fn copy_file_paths(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = if self.session.read(cx).selection_count() == 1 {
            Row::CopyPath
        } else {
            Row::CopySelectionPaths
        };
        let shown = self.selection_shown(cx);
        self.on_selection(shown, row, window, cx);
    }

    /// `edit.copy`: the files themselves on the clipboard. Over the grid,
    /// the items chosen one by one (`copyTarget`; a Select All or a range
    /// is not copied, as in production); in the viewer and Quick Look, the
    /// file shown.
    fn copy_files_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewing(cx) {
            self.act_on_selection(Row::CopyFile, window, cx);
            return;
        }
        let session = self.session.read(cx);
        if session.selection.is_empty() || session.selection.query_wide() {
            cx.propagate();
            return;
        }
        let ids = session.selected_ids();
        self.copy_files(ids, cx);
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
            // Several items chosen one by one, all loaded: the batch rename
            // dialog (the Edit menu's Batch Rename… row, whose keys these
            // are).
            count if count > 1 && self.menu_context(cx).names => self.rename_selected(cx),
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
                this.act_on_selection(Row::OpenDefault, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|this, _: &RevealInFinder, window, cx| {
            if this.single_media(cx) {
                this.act_on_selection(Row::Reveal, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(
            cx.listener(|this, _: &CopyFilePath, window, cx| this.copy_file_paths(window, cx)),
        )
        .on_action(cx.listener(|this, _: &CopyFiles, window, cx| this.copy_files_key(window, cx)))
        .on_action(
            cx.listener(|this, _: &RegenerateThumbnails, _, cx| this.regenerate_selected(cx)),
        )
        .on_action(cx.listener(|this, _: &ToggleGrayscale, _, cx| {
            this.session
                .update(cx, |session, cx| session.toggle_grayscale(cx))
        }))
        .on_action(cx.listener(|this, _: &Rename, window, cx| this.rename(window, cx)))
        .on_action(cx.listener(|this, _: &Rate0, window, cx| this.rate(0, window, cx)))
        .on_action(cx.listener(|this, _: &Rate1, window, cx| this.rate(1, window, cx)))
        .on_action(cx.listener(|this, _: &Rate2, window, cx| this.rate(2, window, cx)))
        .on_action(cx.listener(|this, _: &Rate3, window, cx| this.rate(3, window, cx)))
        .on_action(cx.listener(|this, _: &Rate4, window, cx| this.rate(4, window, cx)))
        .on_action(cx.listener(|this, _: &Rate5, window, cx| this.rate(5, window, cx)))
        .on_action(cx.listener(|this, _: &DeleteItems, window, cx| this.delete_items(window, cx)))
        .on_action(cx.listener(|this, _: &RemoveFromFolder, window, cx| {
            let folder = this.grid_scope(cx).folder().is_some();
            this.on_selection(folder, Row::RemoveFromFolder, window, cx)
        }))
        .on_action(cx.listener(|this, _: &RestoreItems, window, cx| {
            let trash = this.in_scope(ScopeKind::Trash, cx);
            this.on_selection(trash, Row::Restore, window, cx)
        }))
        // Over the media view or Quick Look, the keys review the item
        // being viewed (`InboxReviewControls`); over the grid, the
        // selection.
        .on_action(cx.listener(|this, _: &AcceptInbox, window, cx| {
            if this.viewing(cx) {
                this.review_key(crate::viewer::review::Decision::Accept, window, cx);
                return;
            }
            let inbox = this.in_scope(ScopeKind::Inbox, cx);
            this.on_selection(inbox, Row::Accept, window, cx)
        }))
        // Reject sends the selection to the Trash or deletes it, as the
        // Inbox & Deleting setting says (`rejectTarget`).
        .on_action(cx.listener(|this, _: &RejectInbox, window, cx| {
            if this.viewing(cx) {
                this.review_key(crate::viewer::review::Decision::Reject, window, cx);
                return;
            }
            let inbox = this.in_scope(ScopeKind::Inbox, cx);
            this.on_selection(inbox, Row::Reject, window, cx)
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
