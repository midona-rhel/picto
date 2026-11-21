//! The handlers for the keyboard shortcuts that act on the main window's
//! content: the grid, the viewer, the sidebar and the toolbar. They do what
//! the handlers in production's `GridScreen`, `AppShell`, `Sidebar` and
//! application menu do.
//!
//! The shortcut registry names each shortcut with an id such as
//! `view.quicklook` or `file.delete`. `crate::keymap` binds the keys of each
//! id to an action, in the key context where the shortcut applies.
//! `Shell::shortcut_actions`, at the end of this file, connects most of
//! those actions to the handlers here. The rest are connected in
//! `shell/menu_commands.rs` and `shell/mod.rs`.
//!
//! A handler that has nothing to act on calls `cx.propagate()`. GPUI then
//! offers the key press to the next handler, which is what happens to a key
//! that no handler takes in production.
use super::Shell;
use crate::app_menu::ScopeKind;
use crate::grid::menu::Action as Row;
use crate::keymap::*;
use crate::settings;
use gpui::*;
use picto_library::query::ItemScope;

impl Shell {
    /// Returns true when the window shows the grid or the viewer, and false
    /// when it shows a screen (Tags, Subscriptions or Duplicates) with the
    /// viewer closed. The selected item is visible only in the first case,
    /// so the Rename shortcut renames one selected item only then.
    fn selection_shown(&self, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.screen().is_none() || session.viewer
    }

    /// Handles the `view.detailView` shortcut in the grid. When exactly one
    /// item is selected, it opens that item in the viewer. With no item or
    /// several items selected it passes the key on.
    pub(super) fn open_selected(&mut self, cx: &mut Context<Self>) {
        if self.session.read(cx).selection_count() == 1 {
            self.session
                .update(cx, |session, cx| session.open_viewer(cx));
        } else {
            cx.propagate();
        }
    }

    /// Handles the `view.quicklook` shortcut (Space) in the grid.
    /// `viewer::trigger::space` decides what the key does:
    ///
    /// - If the viewer is open, the key closes it.
    /// - Otherwise, if "Space key" in Settings › Controls is set to
    ///   "Scroll page", the key scrolls the grid down by nine tenths of its
    ///   visible height.
    /// - Otherwise the key closes Quick Look if it is open, and opens Quick
    ///   Look if it is closed and exactly one item is selected.
    fn quick_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let setting = settings::current(cx).space_key_action;
        let action = crate::viewer::trigger::space(setting, self.trigger_state(cx));
        self.perform(action, window, cx);
    }

    /// Handles the shortcuts that open a built-in destination: All, Inbox,
    /// Untagged or Trash, as production's `menu:navigate` message does. It
    /// opens the sidebar destination whose scope is `wanted`. A destination
    /// that is a screen, or that shows the All scope in a shuffled order,
    /// is not a match. If no destination matches, it passes the key on.
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

    /// Handles the `nav.search` shortcut by putting the keyboard focus in a
    /// search field. Over the grid that is the toolbar's search field, and
    /// on the Tags screen it is that screen's own search field. While the
    /// viewer is open, or on a screen with no search field, it passes the
    /// key on.
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

    /// Runs the action of the item menu's row `row` on the selected items.
    /// It does so only when `applies` is true and at least one item is
    /// selected. Otherwise it passes the key on.
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

    /// Handles the `file.delete` shortcut in the grid. With no item selected
    /// it passes the key on. Otherwise what the key does depends on the
    /// destination shown, as production's `gridRemovalAction` decides it:
    ///
    /// - In a folder, this handler passes the key on, because there the
    ///   key belongs to the `file.removeFromFolder` shortcut.
    /// - In the Trash, it deletes the selected items permanently. It asks
    ///   for confirmation first, as the menu's Delete Permanently row does.
    /// - Anywhere else, it moves the selected items to the Trash.
    fn delete_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let grid = self.grid_scope(cx);
        let row = if grid.kind() == ScopeKind::Trash {
            Row::DeletePermanently
        } else {
            Row::MoveToTrash
        };
        self.on_selection(grid.folder().is_none(), row, window, cx);
    }

    /// Handles the `edit.copy` shortcut. It puts the files themselves on the
    /// clipboard, not their paths.
    ///
    /// While the viewer or Quick Look is open, it copies the file that is
    /// shown. Over the grid it copies the files of the selected items
    /// (production's `copyTarget`), but only when each item was selected
    /// individually. A selection made with Select All or as a range is not
    /// copied and the key is passed on, as in production.
    fn copy_files_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewing(cx) {
            self.act_on_selection(Row::CopyFile, window, cx);
            return;
        }
        let session = self.session.read(cx);
        let selection = session
            .selection_target()
            .filter(|_| !session.selection.query_wide());
        match selection {
            Some(selection) => self.copy_files(selection, cx),
            None => cx.propagate(),
        }
    }

    /// Handles the `edit.rename` shortcut, which is the key of the Edit
    /// menu's Rename and Batch Rename… rows.
    ///
    /// With one item selected, and the grid or the viewer shown, it puts
    /// the keyboard focus in the inspector's name field. It shows the
    /// inspector first if the inspector is hidden. With several items
    /// selected it opens the batch rename dialog. In every other case it
    /// passes the key on.
    pub(super) fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.session.read(cx).selection_count() {
            1 if self.selection_shown(cx) => {
                if self.inspector_collapsed {
                    self.toggle_rails(false, true, cx);
                }
                self.inspector
                    .update(cx, |inspector, cx| inspector.edit_name(window, cx));
            }
            count if count > 1 => self.rename_selected(cx),
            _ => cx.propagate(),
        }
    }

    /// Handles the `organize.addTag`, `organize.addFolder` and
    /// `file.addToFolder` shortcuts, and the item menu's Add Tags and Add
    /// to Folder rows. With no item selected it passes the key on.
    ///
    /// Over the grid it opens the Add Tags dialog or the Folders dialog for
    /// the selected items, as production's `GridScreen` does. While the
    /// viewer is open it opens the inspector's tag picker or folder picker
    /// instead, as production's `useViewerEntityContextMenu` does. That
    /// picker is placed beside the inspector, 80 px below the inspector's
    /// top, or in the centre of the window when the inspector is hidden.
    pub(super) fn pick(&mut self, picker: Picker, window: &mut Window, cx: &mut Context<Self>) {
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

    /// Handles Accept, from its shortcut or the application menu. While the
    /// viewer or Quick Look is open, it accepts the item being viewed if
    /// that item is in the Inbox, the same as a click on the Accept button
    /// of the Inbox review controls. Otherwise it accepts the selected
    /// items.
    pub(super) fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewing(cx) {
            self.review_key(crate::viewer::review::Decision::Accept, window, cx);
        } else {
            self.act_on_selection(Row::Accept, window, cx);
        }
    }

    /// Handles Reject, from its shortcut or the application menu. While the
    /// viewer or Quick Look is open, it rejects the item being viewed if
    /// that item is in the Inbox, the same as a click on the Reject button
    /// of the Inbox review controls. Otherwise it rejects the selected
    /// items (production's `rejectTarget`). A rejected item is moved to the
    /// Trash or deleted permanently, whichever "Rejected items" in
    /// Settings › Inbox & Deleting is set to.
    pub(super) fn reject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewing(cx) {
            self.review_key(crate::viewer::review::Decision::Reject, window, cx);
        } else {
            self.act_on_selection(Row::Reject, window, cx);
        }
    }

    /// Handles the `view.zoomIn` and `view.zoomOut` keys in the grid. Each
    /// key press makes one zoom step, the same step as the toolbar's + and −
    /// buttons (`Session::zoom_step`).
    fn zoom_grid(&mut self, larger: bool, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.zoom_step(larger, cx));
    }

    /// Adds a listener to `root`, the window's root element, for each
    /// shortcut action that `crate::keymap` binds and this file handles,
    /// and returns `root`.
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
        .on_action(cx.listener(|this, _: &CopyFiles, window, cx| this.copy_files_key(window, cx)))
        .on_action(cx.listener(|this, _: &ToggleGrayscale, _, cx| {
            this.session
                .update(cx, |session, cx| session.toggle_grayscale(cx))
        }))
        .on_action(cx.listener(|this, _: &DeleteItems, window, cx| this.delete_items(window, cx)))
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
    }
}

/// Which picker `Shell::pick` opens: `Tags` for `organize.addTag`,
/// `Folders` for `organize.addFolder` and `AddToFolder` for
/// `file.addToFolder`. `Folders` and `AddToFolder` open the same picker.
#[derive(Clone, Copy)]
pub(super) enum Picker {
    Tags,
    Folders,
    AddToFolder,
}
