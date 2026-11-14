//! The application menu's commands on the window's content, as
//! `applicationMenuRuntime.ts` runs them: the menu tells the window what it
//! acts on (`menuContext`), and the window performs the rows. Each row's
//! action is handled here only while production's rule enables the row, so
//! AppKit shows the row enabled exactly then.
use super::Shell;
use crate::app_menu::{self, MenuContext, ScopeKind, Status};
use crate::backend::Screen;
use crate::grid::menu::{self as rows, Action as Row};
use crate::session::Session;
use gpui::*;
use picto_library::query::{ItemScope, RootSummary};
use picto_library::{FolderId, Lifecycle, RootKind};

/// The grid's destination (`gridScopeAtom`), worked out once for the
/// application menu, the item menus, the keys and imports; none while a
/// screen shows instead of the grid.
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

    /// The destination's kind (`gridScope.kind`).
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

    /// The kind as the grid's menus tell destinations apart (`scopeKind`).
    pub fn menu_kind(&self) -> rows::ScopeKind {
        match self.kind() {
            ScopeKind::Folder => rows::ScopeKind::Folder,
            ScopeKind::SmartFolder => rows::ScopeKind::SmartFolder,
            _ => rows::ScopeKind::System,
        }
    }

    /// The folder open, with or without its subfolders.
    pub fn folder(&self) -> Option<FolderId> {
        match &self.0 {
            Some(ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id }) => {
                Some(*folder_id)
            }
            _ => None,
        }
    }

    /// Files, a folder or the clipboard can be imported here from a menu
    /// or a key: not into the Trash, a smart folder, Recently Viewed or
    /// media matches (`canImport`).
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

    /// Files dropped on the grid are imported anywhere but media matches,
    /// as production's drop handler decides.
    pub fn takes_drops(&self) -> bool {
        !matches!(self.0, Some(ItemScope::MediaMatches { .. }))
    }

    /// Where an import from here goes.
    pub fn import_destination(&self) -> crate::imports::plan::Destination {
        self.0.as_ref().map_or_else(
            Default::default,
            crate::imports::plan::Destination::for_scope,
        )
    }

    /// The application menu's `statusFilter` (`menuContext`): the Trash
    /// before the Inbox, else the lifecycle the selection shares.
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

    /// A tile menu's `statusFilter` (`GridScreen`): the Inbox before the
    /// Trash, and the library only in All. Production's two menus order
    /// the two differently; both are kept.
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

/// The lifecycle every one of `items` shares (`selectionLifecycle`,
/// `commonLifecycle`); none for no items.
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

    /// What the menu's rows depend on (`menuContext`). The selection counts
    /// only while the grid is the destination (`gridActive`).
    pub(super) fn menu_context(&self, cx: &App) -> MenuContext {
        let session = self.session.read(cx);
        let grid_active = session.screen().is_none();
        let grid = GridScope::of(session);
        let selection = if grid_active {
            session.selection_count()
        } else {
            0
        };
        // Items chosen one by one; a Select All or a shift-click span is
        // resolved by the library, never from what the grid holds.
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
        }
    }

    /// Gives the menu the window's context; when the rows change, the root
    /// is drawn again so that its handlers follow.
    pub(super) fn sync_menu(&mut self, cx: &mut Context<Self>) {
        let context = self.menu_context(cx);
        if app_menu::set_context(context, cx) {
            cx.notify();
        }
    }

    /// Opens a screen destination (`menu:navigate`).
    /// View › Log Window: opens the log window, or closes it.
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
        self.diagnostics = Some((panel, closed));
        cx.notify();
    }

    /// Closes the log window; the keyboard goes back to the window's
    /// content if it was in the panel.
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

    /// The new folder rows: a top-level folder, or, in a folder, one inside
    /// it (`foldersController.create('New Folder', parentId)`).
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

    /// Batch Rename… and its keys: the dialog on the selected items, every
    /// one loaded and chosen one by one.
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

    /// Regenerate Thumbnail(s) and its keys: the selected media's files,
    /// while the menu's rule allows it (every selected item a loaded media
    /// item, chosen one by one).
    pub(super) fn regenerate_selected(&mut self, cx: &mut Context<Self>) {
        if !self.menu_context(cx).thumbnails {
            cx.propagate();
            return;
        }
        let files = self
            .session
            .read(cx)
            .selected_loaded()
            .into_iter()
            .map(|item| (item.content_hash.clone(), item.mime.clone()))
            .collect();
        self.regenerate_thumbnails(files, cx);
    }

    /// Wires the menu's rows on the window's root, each while its rule holds.
    pub(super) fn menu_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        let menu = app_menu::context(cx);
        let has = menu.has_selection();
        let mut root = root;
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
            |_, _: &app_menu::NewSmartFolder, window, cx| {
                window.dispatch_action(Box::new(crate::keymap::NewSmartFolder), cx)
            },
        );
        root = when(
            root,
            menu.import,
            cx,
            |_, _: &app_menu::ImportFiles, window, cx| {
                window.dispatch_action(Box::new(crate::keymap::ImportFiles), cx)
            },
        );
        root = when(
            root,
            menu.import,
            cx,
            |_, _: &app_menu::ImportFolder, window, cx| {
                window.dispatch_action(Box::new(crate::keymap::ImportFolder), cx)
            },
        );
        // The export rows are handled by `export_actions`.
        root = when(
            root,
            menu.can_rename_one(),
            cx,
            |_, _: &app_menu::Rename, window, cx| {
                window.dispatch_action(Box::new(crate::keymap::Rename), cx)
            },
        );
        root = when(
            root,
            menu.names && menu.selection > 1,
            cx,
            |this, _: &app_menu::BatchRename, _, cx| this.rename_selected(cx),
        );
        root = row::<app_menu::CopyPaths>(root, has, Row::CopySelectionPaths, false, cx);
        root = row::<app_menu::CopyNames>(root, menu.names, Row::CopySelectionNames, false, cx);
        root = row::<app_menu::CopyLinks>(root, has, Row::CopySelectionLinks, false, cx);
        root = when(
            root,
            menu.thumbnails,
            cx,
            |this, _: &app_menu::RegenerateThumbnails, _, cx| this.regenerate_selected(cx),
        );
        root = when(root, has, cx, |this, _: &app_menu::DeselectAll, _, cx| {
            this.session
                .update(cx, |session, cx| session.clear_selection(cx))
        });
        // The rows on the Delete keys leave a text field's ⌘⌫ to the field.
        root = when(
            root,
            menu.in_folder() && has,
            cx,
            |_, _: &app_menu::RemoveFromFolder, window, cx| {
                if !app_menu::typed_in_text_field(window) {
                    window.dispatch_action(Box::new(crate::keymap::RemoveFromFolder), cx)
                }
            },
        );
        root = row::<app_menu::Restore>(root, menu.in_trash() && has, Row::Restore, true, cx);
        root = row::<app_menu::DeletePermanently>(
            root,
            menu.in_trash() && has,
            Row::DeletePermanently,
            true,
            cx,
        );
        root =
            row::<app_menu::MoveToTrash>(root, !menu.in_trash() && has, Row::MoveToTrash, true, cx);
        root = row::<app_menu::Accept>(root, menu.in_inbox() && has, Row::Accept, false, cx);
        root = row::<app_menu::Reject>(root, menu.in_inbox() && has, Row::Reject, false, cx);
        root = when(root, has, cx, |_, _: &app_menu::AddTags, window, cx| {
            window.dispatch_action(Box::new(crate::keymap::AddTags), cx)
        });
        root = when(root, has, cx, |_, _: &app_menu::AddToFolder, window, cx| {
            window.dispatch_action(Box::new(crate::keymap::AddToFolder), cx)
        });
        root = when(root, has, cx, |this, _: &app_menu::AutoTag, window, cx| {
            this.act_on_selection(Row::AutoTag, window, cx)
        });
        root = when(
            root,
            has,
            cx,
            |this, rating: &app_menu::SetRating, window, cx| {
                this.act_on_selection(Row::Rate(rating.stars), window, cx)
            },
        );
        root = when(root, true, cx, |this, _: &app_menu::ShowTags, _, cx| {
            this.show_screen(Screen::Tags, cx)
        });
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
        root = row::<app_menu::OpenDefault>(root, menu.single_media(), Row::OpenDefault, false, cx);
        root = row::<app_menu::Reveal>(root, menu.single_media(), Row::Reveal, false, cx);
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

/// Runs the item menu's `row` for `A` while `enabled`. A row on the Delete
/// keys (`delete_key`) leaves a text field's ⌘⌫ to the field.
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

/// Handles `A` on `root` while `enabled`.
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
            // A screen instead of the grid.
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
    fn menus_and_keys_import_where_production_does_and_drops_go_further() {
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
        // A drop is refused only over media matches (`GridScreen`'s drop
        // handler), so the Trash takes one.
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
    fn the_two_menus_read_the_lifecycle_as_production_does() {
        let all = at(ItemScope::All);
        let folder = at(ItemScope::Folder {
            folder_id: FolderId(2),
        });
        // In the Trash and the Inbox both menus act within them.
        for grid in [at(ItemScope::Trash), at(ItemScope::Inbox)] {
            let status = grid.menu_status(None);
            assert!(status.is_some());
            assert_eq!(grid.tile_status(None), status);
        }
        // Items the library holds: the application menu says so wherever
        // they are; a tile's menu only in All.
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
        // A mixed selection outside the Trash and the Inbox.
        assert_eq!(all.menu_status(None), None);
        assert_eq!(all.tile_status(None), Some(Status::Active));
        // Trashed items seen elsewhere (Recently Viewed).
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
    /// `ApplicationMenuButton`: the application menu in the window, under
    /// the button (its left edge, 4px below it), 220px wide and without a
    /// search row. A row chosen goes where the keyboard was when the menu
    /// opened, as the menu bar's rows do.
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
}
