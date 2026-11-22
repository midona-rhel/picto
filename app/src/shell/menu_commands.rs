//! Carries out the application menu's commands that act on the window's
//! content. The shell gives the application menu a `MenuContext`, which says
//! what is selected and which destination is shown (`Shell::sync_menu`), and
//! the methods of `MenuContext` say from it which rows apply. Each row's
//! action is handled here only while the row applies, so the menu shows the
//! row as enabled exactly then.
use super::Shell;
use super::shortcuts::Picker;
use crate::app_menu::{self, MenuContext, ScopeKind, Status};
use crate::backend::Screen;
use crate::grid::menu::{self as rows, Action as Row};
use crate::session::Session;
use gpui::*;
use picto_library::query::{ItemScope, RootSummary};
use picto_library::{FolderId, Lifecycle, RootKind};

/// The scope of the destination that the grid shows. The application menu,
/// the item menus, the keyboard shortcuts and the imports all read it from
/// here. It holds `None` while a screen is shown instead of the grid.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct GridScope(Option<ItemScope>);

impl GridScope {
    pub fn of(session: &Session) -> Self {
        Self(
            session
                .nav
                .get(session.displayed)
                .filter(|entry| entry.screen.is_none())
                .map(|entry| entry.scope.clone()),
        )
    }

    /// Returns the destination's kind: All, Inbox, Trash, a folder, a smart
    /// folder, or `Other` for every other destination and for a screen.
    pub fn kind(&self) -> ScopeKind {
        match &self.0 {
            Some(ItemScope::All) => ScopeKind::All,
            Some(ItemScope::Inbox) => ScopeKind::Inbox,
            Some(ItemScope::Trash) => ScopeKind::Trash,
            Some(ItemScope::Folder { .. } | ItemScope::FolderTree { .. }) => ScopeKind::Folder,
            Some(ItemScope::SmartFolder { .. }) => ScopeKind::SmartFolder,
            _ => ScopeKind::Other,
        }
    }

    /// Returns the destination's kind as the grid's item menus distinguish
    /// it: a folder, a smart folder, or `System` for every other destination.
    pub fn menu_kind(&self) -> rows::ScopeKind {
        match self.kind() {
            ScopeKind::Folder => rows::ScopeKind::Folder,
            ScopeKind::SmartFolder => rows::ScopeKind::SmartFolder,
            _ => rows::ScopeKind::System,
        }
    }

    /// Returns the folder that is open, whether the grid shows it with or
    /// without the items of its subfolders. Returns `None` when the
    /// destination is not a folder.
    pub fn folder(&self) -> Option<FolderId> {
        match &self.0 {
            Some(ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id }) => {
                Some(*folder_id)
            }
            _ => None,
        }
    }

    /// Returns whether a menu row or a keyboard shortcut may import files,
    /// a folder or the clipboard into this destination. It may not import
    /// into the Trash, a smart folder, Recently Viewed, or the list of the
    /// items that share a file with one item (`ItemScope::MediaMatches`).
    pub fn imports(&self) -> bool {
        !matches!(
            self.0,
            Some(
                ItemScope::Trash
                    | ItemScope::SmartFolder { .. }
                    | ItemScope::RecentlyViewed
                    | ItemScope::MediaMatches { .. }
            )
        )
    }

    /// Returns whether files dropped on the grid are imported. They are
    /// imported in every destination except `ItemScope::MediaMatches`.
    pub fn takes_drops(&self) -> bool {
        !matches!(self.0, Some(ItemScope::MediaMatches { .. }))
    }

    /// Returns where an import that is started in this destination puts
    /// its items.
    pub fn import_destination(&self) -> crate::imports::plan::Destination {
        self.0.as_ref().map_or_else(
            Default::default,
            crate::imports::plan::Destination::for_scope,
        )
    }

    /// Returns the status that the application menu's rows are chosen for.
    /// `shared` is the lifecycle that all the selected items have, if they
    /// all have the same one. The status is Trash when the items are in
    /// the Trash or the destination is the Trash. Otherwise it is Inbox
    /// when the items are in the Inbox or the destination is the Inbox.
    /// Otherwise it is the shared lifecycle.
    pub fn menu_status(&self, shared: Option<Lifecycle>) -> Option<Status> {
        let kind = self.kind();
        if shared == Some(Lifecycle::Trash) || kind == ScopeKind::Trash {
            Some(Status::Trash)
        } else if shared == Some(Lifecycle::Inbox) || kind == ScopeKind::Inbox {
            Some(Status::Inbox)
        } else {
            shared.map(Status::from)
        }
    }

    /// Returns the status that the rows of a tile's context menu are chosen
    /// for. Unlike `menu_status`, it tests for the Inbox before the Trash,
    /// and outside the Inbox and the Trash it is `Active` in the All
    /// destination and `None` everywhere else. Picto 0.6.19 and earlier
    /// chose the rows of its two menus by these two different rules, and
    /// both rules are kept.
    pub fn tile_status(&self, shared: Option<Lifecycle>) -> Option<Status> {
        let kind = self.kind();
        if shared == Some(Lifecycle::Inbox) || kind == ScopeKind::Inbox {
            Some(Status::Inbox)
        } else if shared == Some(Lifecycle::Trash) || kind == ScopeKind::Trash {
            Some(Status::Trash)
        } else {
            (kind == ScopeKind::All).then_some(Status::Active)
        }
    }
}

/// Returns the lifecycle that every one of `items` has. Returns `None` when
/// the items have different lifecycles and when there are no items.
pub(super) fn shared_lifecycle<'a>(
    items: impl IntoIterator<Item = &'a RootSummary>,
) -> Option<Lifecycle> {
    let mut items = items.into_iter();
    let first = items.next()?.lifecycle;
    items.all(|item| item.lifecycle == first).then_some(first)
}

impl Shell {
    pub(super) fn grid_scope(&self, cx: &App) -> GridScope {
        GridScope::of(self.session.read(cx))
    }

    /// Builds the `MenuContext`: the facts that decide which rows of the
    /// application menu apply. The selection counts as empty while a screen
    /// is shown instead of the grid.
    pub(super) fn menu_context(&self, cx: &App) -> MenuContext {
        let session = self.session.read(cx);
        let grid_active = session.screen().is_none();
        let grid = GridScope::of(session);
        let selection = if grid_active {
            session.selection_count()
        } else {
            0
        };
        // `explicit` is true when the user selected the items one by one. A
        // selection made with Select All or as a range is resolved by the
        // library and not from the items the grid has loaded, so for such a
        // selection `selected` is left empty.
        let explicit = !session.selection.query_wide();
        let selected = if explicit {
            session.selected_loaded()
        } else {
            Vec::new()
        };
        let names = grid_active && explicit && selection > 0 && selected.len() as u64 == selection;
        let single = (grid_active && explicit && selection == 1)
            .then(|| selected.first())
            .flatten()
            .map(|item| item.kind == RootKind::Collection);
        let status = grid.menu_status(shared_lifecycle(selected.iter().copied()));
        let thumbnails = names
            && selected
                .iter()
                .all(|item| item.kind == RootKind::Media && !item.content_hash.is_empty());
        MenuContext {
            selection,
            single,
            scope: grid.kind(),
            status,
            names,
            thumbnails,
            import: crate::imports::blocked(cx).is_none(),
            library: session.library_name(),
            recent: Vec::new(),
        }
    }

    /// Gives the application menu the window's current `MenuContext`. When
    /// the context has changed, the shell is drawn again, because
    /// `menu_actions` adds the handlers of the rows that apply each time the
    /// shell is drawn.
    pub(super) fn sync_menu(&mut self, cx: &mut Context<Self>) {
        let context = self.menu_context(cx);
        if app_menu::set_context(context, cx) {
            cx.notify();
        }
    }

    /// Opens the log window if it is not open.
    pub(crate) fn show_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diagnostics.is_none() {
            self.toggle_diagnostics(window, cx);
        }
    }

    /// Opens the log window, or closes it if it is open. The View menu's
    /// Log Window row runs this.
    pub(crate) fn toggle_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diagnostics.is_some() {
            self.close_diagnostics(window, cx);
            return;
        }
        let session = self.session.clone();
        let panel = cx.new(|cx| crate::diagnostics::panel::Panel::new(session, cx));
        let closed = cx.subscribe_in(
            &panel,
            window,
            |this, _, _: &crate::diagnostics::panel::Closed, window, cx| {
                this.close_diagnostics(window, cx)
            },
        );
        // Escape closes the log window wherever the keyboard focus is,
        // also in a text field: the key press is taken here before any key
        // binding sees it. While a context menu is open, Escape closes that
        // menu and the log window stays open.
        let shell = cx.entity().downgrade();
        let this_window = window.window_handle();
        let escape = cx.intercept_keystrokes(move |event, window, cx| {
            let keystroke = &event.keystroke;
            if window.window_handle() != this_window
                || keystroke.key != "escape"
                || keystroke.modifiers.number_of_modifiers() != 0
            {
                return;
            }
            let _ = shell.update(cx, |this, cx| {
                if !this.menu.close(cx) {
                    this.close_diagnostics(window, cx);
                    cx.stop_propagation();
                }
            });
        });
        self.diagnostics = Some((panel, [closed, escape]));
        cx.notify();
    }

    /// Closes the log window. If the keyboard focus was inside the log
    /// window, it goes to the shell's `focus` handle.
    pub(crate) fn close_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((panel, _)) = self.diagnostics.take() {
            if panel.focus_handle(cx).contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
            cx.notify();
        }
    }

    pub(crate) fn show_screen(&mut self, screen: Screen, cx: &mut Context<Self>) {
        let index = self
            .session
            .read(cx)
            .nav
            .iter()
            .position(|entry| entry.screen == Some(screen));
        if let Some(index) = index {
            self.session
                .update(cx, |session, cx| session.navigate(index, cx));
        }
    }

    /// Carries out the New Folder and New Subfolder rows. When `inside` is
    /// false, it creates a top-level folder. When `inside` is true, it
    /// creates a folder inside the folder that is open, and does nothing if
    /// no folder is open.
    fn menu_new_folder(&mut self, inside: bool, window: &mut Window, cx: &mut Context<Self>) {
        let parent = if inside {
            match self.grid_scope(cx).folder() {
                Some(folder) => Some(folder),
                None => return,
            }
        } else {
            None
        };
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.new_folder_in(parent, None, window, cx)
        });
    }

    /// Opens the batch rename dialog on the selected items. The Rename
    /// action is handled only while every selected item is loaded and was
    /// selected individually (`MenuContext::names`), so `selected_loaded`
    /// returns all of them.
    pub(super) fn rename_selected(&mut self, cx: &mut Context<Self>) {
        let items = self
            .session
            .read(cx)
            .selected_loaded()
            .into_iter()
            .cloned()
            .collect();
        self.batch_rename(items, cx);
    }

    /// Regenerates the thumbnails of the selected items' files, for the
    /// Regenerate Thumbnail(s) row. The row applies only while every
    /// selected item is a media item that is loaded and was selected
    /// individually (`MenuContext::thumbnails`).
    fn regenerate_selected(&mut self, cx: &mut Context<Self>) {
        let files = self
            .session
            .read(cx)
            .selected_loaded()
            .into_iter()
            .map(|item| (item.content_hash.clone(), item.mime.clone()))
            .collect();
        self.regenerate_thumbnails(files, cx);
    }

    /// Adds to `root`, the window's root element, the handlers of the
    /// commands that the application menu and the keyboard shortcuts share,
    /// and returns `root`.
    ///
    /// A menu row that has a shortcut uses the action that `crate::keymap`
    /// binds to the shortcut's keys, and the menu shows those keys on the
    /// row. A row without a shortcut has an action of its own in
    /// `app_menu`. Each action is handled only while its row applies (the
    /// methods of `MenuContext`). The menu shows the row as enabled, and
    /// the shortcut's key press reaches the handler, exactly then. While
    /// the media view or Quick Look is open, Accept, Reject, Auto Tag and
    /// the rating rows also apply to the item that is shown.
    pub(super) fn menu_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        use crate::keymap;
        let menu = app_menu::context(cx);
        let has = menu.has_selection();
        let viewing = self.viewing(cx);
        let mut root = root;
        // The handlers of the Library and File menus' rows follow.
        root = when(
            root,
            menu.can_create_folder(),
            cx,
            |this, _: &app_menu::NewFolder, window, cx| this.menu_new_folder(false, window, cx),
        );
        root = when(
            root,
            menu.can_create_folder(),
            cx,
            |this, _: &app_menu::NewSubfolder, window, cx| this.menu_new_folder(true, window, cx),
        );
        root = when(
            root,
            menu.can_create_smart_folder(),
            cx,
            |this, _: &app_menu::NewSmartFolder, window, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.new_top_smart_folder(window, cx))
            },
        );
        root = when(
            root,
            menu.import,
            cx,
            |this, _: &keymap::ImportFiles, window, cx| {
                this.import(super::import::Import::Files, window, cx)
            },
        );
        root = when(
            root,
            menu.import,
            cx,
            |this, _: &keymap::ImportFolder, window, cx| {
                this.import(super::import::Import::Folder, window, cx)
            },
        );
        // The handlers of the Edit menu's rows follow. `rename` renames one
        // selected item and opens the batch rename dialog for several.
        root = when(
            root,
            menu.names,
            cx,
            |this, _: &keymap::Rename, window, cx| this.rename(window, cx),
        );
        root = when(
            root,
            has,
            cx,
            |this, _: &keymap::CopyFilePath, window, cx| {
                // While the media view or Quick Look is open, the path of
                // the file that is shown is copied. Otherwise the paths of
                // the selected items' files are copied.
                let row = if this.viewing(cx) {
                    Row::CopyPath
                } else {
                    Row::CopySelectionPaths
                };
                this.act_on_selection(row, window, cx)
            },
        );
        root = row::<app_menu::CopyNames>(root, menu.names, Row::CopySelectionNames, false, cx);
        root = row::<app_menu::CopyLinks>(root, has, Row::CopySelectionLinks, false, cx);
        root = when(
            root,
            menu.thumbnails,
            cx,
            |this, _: &keymap::RegenerateThumbnails, _, cx| this.regenerate_selected(cx),
        );
        root = when(root, has, cx, |this, _: &app_menu::DeselectAll, _, cx| {
            this.session
                .update(cx, |session, cx| session.clear_selection(cx))
        });
        // The shortcuts of the next rows use the Delete key. Their handlers
        // do nothing while the keyboard focus is in a text field, so that
        // ⌘⌫ deletes text in the field.
        let remove = menu.in_folder() && has;
        root = row::<app_menu::RemoveFromFolder>(root, remove, Row::RemoveFromFolder, true, cx);
        root = row::<keymap::RemoveFromFolder>(root, remove, Row::RemoveFromFolder, true, cx);
        let trash = menu.in_trash() && has;
        root = row::<keymap::RestoreItems>(root, trash, Row::Restore, true, cx);
        root = row::<app_menu::DeletePermanently>(root, trash, Row::DeletePermanently, true, cx);
        root =
            row::<app_menu::MoveToTrash>(root, !menu.in_trash() && has, Row::MoveToTrash, true, cx);
        // The handlers of the Organize menu's rows follow. While the media
        // view or Quick Look is open, Accept and Reject review the item that
        // is shown. Over the grid they review the selected items.
        let inbox = menu.in_inbox() && has;
        root = when(
            root,
            inbox || viewing,
            cx,
            |this, _: &app_menu::Accept, window, cx| this.accept(window, cx),
        );
        root = when(
            root,
            inbox || viewing,
            cx,
            |this, _: &keymap::AcceptInbox, window, cx| this.accept(window, cx),
        );
        root = when(
            root,
            inbox || viewing,
            cx,
            |this, _: &app_menu::Reject, window, cx| this.reject(window, cx),
        );
        root = when(
            root,
            inbox || viewing,
            cx,
            |this, _: &keymap::RejectInbox, window, cx| this.reject(window, cx),
        );
        root = when(root, has, cx, |this, _: &app_menu::AddTags, window, cx| {
            this.pick(Picker::Tags, window, cx)
        });
        root = when(root, has, cx, |this, _: &keymap::AddTags, window, cx| {
            this.pick(Picker::Tags, window, cx)
        });
        root = when(
            root,
            has,
            cx,
            |this, _: &keymap::AddToFolder, window, cx| this.pick(Picker::AddToFolder, window, cx),
        );
        root = when(
            root,
            has,
            cx,
            |this, _: &keymap::AddToFolders, window, cx| this.pick(Picker::Folders, window, cx),
        );
        root = row::<keymap::AutoTag>(root, has || viewing, Row::AutoTag, false, cx);
        root = when(
            root,
            has || viewing,
            cx,
            |this, rating: &app_menu::SetRating, window, cx| {
                this.act_on_selection(Row::Rate(rating.stars), window, cx)
            },
        );
        root = when(
            root,
            has || viewing,
            cx,
            |this, rating: &keymap::Rate, window, cx| {
                this.act_on_selection(Row::Rate(rating.stars), window, cx)
            },
        );
        root = when(root, true, cx, |this, _: &app_menu::ShowTags, _, cx| {
            this.show_screen(Screen::Tags, cx)
        });
        // The handlers of the View menu's rows follow.
        root = when(
            root,
            true,
            cx,
            |this, _: &app_menu::ShowDuplicates, _, cx| this.show_screen(Screen::Duplicates, cx),
        );
        root = when(
            root,
            true,
            cx,
            |this, _: &app_menu::LogWindow, window, cx| this.toggle_diagnostics(window, cx),
        );
        root =
            row::<keymap::OpenDefaultApp>(root, menu.single_media(), Row::OpenDefault, false, cx);
        root = row::<keymap::RevealInFinder>(root, menu.single_media(), Row::Reveal, false, cx);
        when(
            root,
            menu.single(),
            cx,
            |_, _: &app_menu::OpenInNewWindow, window, cx| {
                window.dispatch_action(Box::new(super::OpenInNewWindow), cx)
            },
        )
    }
}

/// Adds to `root` a handler of the action `A` while `enabled` is true. The
/// handler runs the item menu's row `row` on the selected items. When
/// `delete_key` is true, the handler does nothing while the keyboard focus is
/// in a text field, so that ⌘⌫ deletes text in the field.
fn row<A: Action>(
    root: Div,
    enabled: bool,
    row: Row,
    delete_key: bool,
    cx: &mut Context<Shell>,
) -> Div {
    when(root, enabled, cx, move |this, _: &A, window, cx| {
        if !(delete_key && app_menu::typed_in_text_field(window)) {
            this.act_on_selection(row, window, cx)
        }
    })
}

/// Adds `handler` to `root` as the handler of the action `A` when `enabled`
/// is true, and returns `root`.
fn when<A: Action>(
    root: Div,
    enabled: bool,
    cx: &mut Context<Shell>,
    handler: impl Fn(&mut Shell, &A, &mut Window, &mut Context<Shell>) + 'static,
) -> Div {
    if enabled {
        root.on_action(cx.listener(handler))
    } else {
        root
    }
}

#[cfg(test)]
mod tests {
    use super::{GridScope, ScopeKind, Status, rows, shared_lifecycle};
    use picto_library::query::{ItemScope, RootSummary};
    use picto_library::{FolderId, Lifecycle, MediaId, Rating, RootId, RootKind};

    fn item(lifecycle: Lifecycle) -> RootSummary {
        RootSummary {
            root_id: RootId(1),
            kind: RootKind::Media,
            lifecycle,
            name: String::new(),
            cover_media_id: MediaId(1),
            content_hash: String::new(),
            mime: String::new(),
            width: None,
            height: None,
            duration_ms: None,
            frame_count: None,
            palette: Vec::new(),
            imported_at_ms: 0,
            captured_at_ms: None,
            modified_at_ms: 0,
            media_count: 1,
            total_size_bytes: 0,
            rating: Rating::ALL[0],
        }
    }

    fn at(scope: ItemScope) -> GridScope {
        GridScope(Some(scope))
    }

    #[test]
    fn each_destination_is_one_kind_for_every_menu() {
        let folder = FolderId(4);
        let cases = [
            (
                at(ItemScope::All),
                ScopeKind::All,
                rows::ScopeKind::System,
                None,
            ),
            (
                at(ItemScope::Trash),
                ScopeKind::Trash,
                rows::ScopeKind::System,
                None,
            ),
            (
                at(ItemScope::Folder { folder_id: folder }),
                ScopeKind::Folder,
                rows::ScopeKind::Folder,
                Some(folder),
            ),
            (
                at(ItemScope::FolderTree { folder_id: folder }),
                ScopeKind::Folder,
                rows::ScopeKind::Folder,
                Some(folder),
            ),
            (
                at(ItemScope::RecentlyViewed),
                ScopeKind::Other,
                rows::ScopeKind::System,
                None,
            ),
            // A screen is shown instead of the grid.
            (
                GridScope::default(),
                ScopeKind::Other,
                rows::ScopeKind::System,
                None,
            ),
        ];
        for (grid, kind, menu_kind, open) in cases {
            assert_eq!(grid.kind(), kind, "{grid:?}");
            assert_eq!(grid.menu_kind(), menu_kind, "{grid:?}");
            assert_eq!(grid.folder(), open, "{grid:?}");
        }
    }

    #[test]
    fn menus_and_keys_import_where_the_earlier_version_did_and_drops_go_further() {
        let matches = at(ItemScope::MediaMatches { item_id: 3 });
        for refused in [
            at(ItemScope::Trash),
            at(ItemScope::RecentlyViewed),
            matches.clone(),
        ] {
            assert!(!refused.imports(), "{refused:?}");
        }
        assert!(at(ItemScope::Inbox).imports());
        assert!(GridScope::default().imports());
        // A drop is refused only over `ItemScope::MediaMatches`, so the
        // Trash takes one.
        assert!(at(ItemScope::Trash).takes_drops());
        assert!(!matches.takes_drops());
        assert!(at(ItemScope::Inbox).import_destination().inbox);
        assert_eq!(
            at(ItemScope::Folder {
                folder_id: FolderId(9)
            })
            .import_destination()
            .folder,
            Some(FolderId(9))
        );
    }

    #[test]
    fn the_two_menus_read_the_lifecycle_as_the_earlier_version_did() {
        let all = at(ItemScope::All);
        let folder = at(ItemScope::Folder {
            folder_id: FolderId(2),
        });
        // In the Trash and in the Inbox, both menus give that destination's
        // status.
        for grid in [at(ItemScope::Trash), at(ItemScope::Inbox)] {
            let status = grid.menu_status(None);
            assert!(status.is_some());
            assert_eq!(grid.tile_status(None), status);
        }
        // For active items, the application menu gives `Active` in every
        // destination, and a tile's menu gives it only in All.
        assert_eq!(
            all.menu_status(Some(Lifecycle::Active)),
            Some(Status::Active)
        );
        assert_eq!(
            all.tile_status(Some(Lifecycle::Active)),
            Some(Status::Active)
        );
        assert_eq!(
            folder.menu_status(Some(Lifecycle::Active)),
            Some(Status::Active)
        );
        assert_eq!(folder.tile_status(Some(Lifecycle::Active)), None);
        // The selected items have different lifecycles, and the destination
        // is neither the Trash nor the Inbox.
        assert_eq!(all.menu_status(None), None);
        assert_eq!(all.tile_status(None), Some(Status::Active));
        // Items that are in the Trash are selected in another destination,
        // here Recently Viewed.
        let recent = at(ItemScope::RecentlyViewed);
        assert_eq!(
            recent.menu_status(Some(Lifecycle::Trash)),
            Some(Status::Trash)
        );
        assert_eq!(
            recent.tile_status(Some(Lifecycle::Trash)),
            Some(Status::Trash)
        );
    }

    #[test]
    fn a_lifecycle_is_shared_only_by_all_of_the_items() {
        let (inbox, trash) = (item(Lifecycle::Inbox), item(Lifecycle::Trash));
        assert_eq!(shared_lifecycle([&inbox, &inbox]), Some(Lifecycle::Inbox));
        assert_eq!(shared_lifecycle([&inbox, &trash]), None);
        assert_eq!(shared_lifecycle(std::iter::empty()), None);
    }
}

impl Shell {
    /// Opens the application menu inside the window, for the application
    /// menu button in the titlebar (Windows and Linux). The menu's left
    /// edge is at the button's left edge and its top is 4 px below the
    /// button. It is 220 px wide (`app_menu::IN_WINDOW_WIDTH`) and has no
    /// search row. Before the action of the chosen row is dispatched, the
    /// keyboard focus goes back to the element that had it when the menu
    /// opened, so the action reaches the handlers that a row of the
    /// system's menu bar would reach.
    pub(super) fn open_app_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = app_menu::in_window_entries(window, cx);
        let button_bottom = (crate::theme::TITLEBAR_HEIGHT + crate::theme::CONTROL_MD) / 2.;
        let position = point(
            px(crate::ui::window_chrome::TITLEBAR_INSET),
            px(button_bottom + 4.),
        );
        let opener = window.focused(cx);
        let menu = cx.new(|cx| {
            crate::ui::menu::Menu::new(entries, position, cx)
                .search(false)
                .width(app_menu::IN_WINDOW_WIDTH)
        });
        self.menu
            .open(menu, window, cx, move |_, menu, _, window, cx| {
                let Some(path) = menu.read(cx).chosen_value() else {
                    return;
                };
                let Some(action) = app_menu::in_window_action(&path, cx) else {
                    return;
                };
                if let Some(opener) = &opener {
                    window.focus(opener, cx);
                }
                window.dispatch_action(action, cx);
            });
    }

    /// Opens the context menu of the Toggle Panels button at `position`.
    /// Its rows toggle both panels, the sidebar or the inspector, and each
    /// row shows its shortcut. The menu stays open after a row is chosen,
    /// so the user can toggle several panels.
    pub(super) fn open_panels_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::ui::menu;
        let row = |action: &'static str, label: &'static str, shortcut: &str| {
            menu::item(action, label)
                .shortcut_for(shortcut, cx)
                .keep_open(true)
                .into()
        };
        // i18n: translated where drawn
        let entries = vec![
            row("all", "Toggle All Panels", "view.toggleBothPanels"),
            row("sidebar", "Toggle Sidebar", "view.toggleSidebar"),
            row("inspector", "Toggle Inspector", "view.toggleInspector"),
        ];
        let menu = cx.new(|cx| menu::Menu::new(entries, position, cx).search(false));
        self.menu
            .open(menu, window, cx, |this, _, action, _, cx| match action {
                "all" => this.toggle_all_rails(cx),
                "sidebar" => this.toggle_rails(true, false, cx),
                "inspector" => this.toggle_rails(false, true, cx),
                _ => {}
            });
    }
}
