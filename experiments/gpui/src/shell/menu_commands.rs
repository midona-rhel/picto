//! The application menu's commands on the window's content, as
//! `applicationMenuRuntime.ts` runs them: the menu tells the window what it
//! acts on (`menuContext`), and the window performs the rows. Each row's
//! action is handled here only while production's rule enables the row, so
//! AppKit shows the row enabled exactly then.
use super::Shell;
use crate::app_menu::{self, MenuContext, ScopeKind, Status};
use crate::backend::Screen;
use crate::grid::menu::Action as Row;
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::{Lifecycle, RootKind};

impl Shell {
    /// What the menu's rows depend on (`menuContext`). The selection counts
    /// only while the grid is the destination (`gridActive`).
    pub(super) fn menu_context(&self, cx: &App) -> MenuContext {
        let session = self.session.read(cx);
        let grid_active = session.screen().is_none();
        let scope = match session
            .nav
            .get(session.displayed)
            .filter(|entry| entry.screen.is_none())
            .map(|entry| &entry.scope)
        {
            Some(ItemScope::All) => ScopeKind::All,
            Some(ItemScope::Inbox) => ScopeKind::Inbox,
            Some(ItemScope::Trash) => ScopeKind::Trash,
            Some(ItemScope::Folder { .. } | ItemScope::FolderTree { .. }) => ScopeKind::Folder,
            Some(ItemScope::SmartFolder { .. }) => ScopeKind::SmartFolder,
            _ => ScopeKind::Other,
        };
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
        let common = selected
            .first()
            .map(|first| first.lifecycle)
            .filter(|lifecycle| selected.iter().all(|item| item.lifecycle == *lifecycle));
        let status = if common == Some(Lifecycle::Trash) || scope == ScopeKind::Trash {
            Some(Status::Trash)
        } else if common == Some(Lifecycle::Inbox) || scope == ScopeKind::Inbox {
            Some(Status::Inbox)
        } else {
            common.map(|_| Status::Active)
        };
        let thumbnails = names
            && selected
                .iter()
                .all(|item| item.kind == RootKind::Media && !item.content_hash.is_empty());
        MenuContext {
            selection,
            single,
            scope,
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
            match self.scope_kind(cx).folder {
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
        root = when(
            root,
            has,
            cx,
            |this, _: &app_menu::CopyPaths, window, cx| {
                this.act_on_selection(Row::CopySelectionPaths, window, cx)
            },
        );
        root = when(
            root,
            menu.names,
            cx,
            |this, _: &app_menu::CopyNames, window, cx| {
                this.act_on_selection(Row::CopySelectionNames, window, cx)
            },
        );
        root = when(
            root,
            has,
            cx,
            |this, _: &app_menu::CopyLinks, window, cx| {
                this.act_on_selection(Row::CopySelectionLinks, window, cx)
            },
        );
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
        root = when(
            root,
            menu.in_trash() && has,
            cx,
            |this, _: &app_menu::Restore, window, cx| {
                if !app_menu::typed_in_text_field(window) {
                    this.act_on_selection(Row::Restore, window, cx)
                }
            },
        );
        root = when(
            root,
            menu.in_trash() && has,
            cx,
            |this, _: &app_menu::DeletePermanently, window, cx| {
                if !app_menu::typed_in_text_field(window) {
                    this.act_on_selection(Row::DeletePermanently, window, cx)
                }
            },
        );
        root = when(
            root,
            !menu.in_trash() && has,
            cx,
            |this, _: &app_menu::MoveToTrash, window, cx| {
                if !app_menu::typed_in_text_field(window) {
                    this.act_on_selection(Row::MoveToTrash, window, cx)
                }
            },
        );
        root = when(
            root,
            menu.in_inbox() && has,
            cx,
            |this, _: &app_menu::Accept, window, cx| this.act_on_selection(Row::Accept, window, cx),
        );
        root = when(
            root,
            menu.in_inbox() && has,
            cx,
            |this, _: &app_menu::Reject, window, cx| this.act_on_selection(Row::Reject, window, cx),
        );
        root = when(root, has, cx, |_, _: &app_menu::AddTags, window, cx| {
            window.dispatch_action(Box::new(crate::keymap::AddTags), cx)
        });
        root = when(root, has, cx, |_, _: &app_menu::AddToFolder, window, cx| {
            window.dispatch_action(Box::new(crate::keymap::AddToFolder), cx)
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
            menu.single_media(),
            cx,
            |this, _: &app_menu::OpenDefault, window, cx| {
                this.act_on_selection(Row::OpenDefault, window, cx)
            },
        );
        root = when(
            root,
            menu.single_media(),
            cx,
            |this, _: &app_menu::Reveal, window, cx| this.act_on_selection(Row::Reveal, window, cx),
        );
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
