//! What the registry's shortcuts do to the window's content, as
//! `GridScreen`'s, `AppShell`'s, `Sidebar`'s and the application menu's
//! handlers do. The keys and the contexts they apply in are in
//! `crate::keymap`; a handler that finds nothing to act on lets the key go
//! on (`cx.propagate()`), as an unhandled key does in production.
use super::Shell;
use crate::keymap::*;
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
        let session = self.session.read(cx);
        session
            .selection
            .iter()
            .filter_map(|index| session.items.get(*index))
            .map(|item| item.root_id)
            .collect()
    }

    /// The one selected item, unless it is a collection (`singleFileHash`
    /// with `kind !== 'collection'`).
    fn single_media(&self, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.selection.len() == 1
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
        if self.session.read(cx).selection.len() == 1 {
            self.session
                .update(cx, |session, cx| session.open_viewer(cx));
        } else {
            cx.propagate();
        }
    }

    /// `view.quicklook`: Space scrolls the grid when the Controls setting
    /// says so; otherwise it opens the one selected item. The viewer stands
    /// in for Quick Look, which this prototype does not have.
    fn quick_look(&mut self, cx: &mut Context<Self>) {
        if settings::current(cx).space_key_action == "scroll" {
            self.grid.update(cx, |grid, cx| grid.scroll_by_page(cx));
        } else if self.session.read(cx).selection.len() == 1 {
            self.session
                .update(cx, |session, cx| session.open_viewer(cx));
        }
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
        if session.viewer {
            if let Some(item) = session.selected_item() {
                session.rate(item.root_id, rating);
            }
            return;
        }
        let ids = self.selected_ids(cx);
        match ids.as_slice() {
            [] => cx.propagate(),
            [one] => self.session.read(cx).rate(*one, rating),
            _ => self.edit_selection(
                move |library, target| library.set_rating(target, rating).map(|_| ()),
                false,
                cx,
            ),
        }
    }

    /// Runs an edit of the selected items on the library's thread, then
    /// shows its result: the details and the selection's summary again, or
    /// the destination read again when items may have left it.
    fn edit_selection(
        &mut self,
        work: impl FnOnce(&picto_library::Library, &SelectionTarget) -> picto_library::Result<()>
        + Send
        + 'static,
        reload: bool,
        cx: &mut Context<Self>,
    ) {
        let target = SelectionTarget::Explicit {
            root_ids: self.selected_ids(cx),
        };
        let job = self
            .session
            .read(cx)
            .run(move |library| work(library, &target));
        cx.spawn(async move |this, cx| {
            let Ok(result) = job.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    eprintln!("picto: {error}");
                }
                if reload {
                    this.session.update(cx, |session, cx| session.reload(cx));
                } else {
                    this.session.read(cx).refresh_details();
                    this.inspector
                        .update(cx, |inspector, cx| inspector.selection_edited(cx));
                }
            });
        })
        .detach();
    }

    /// `file.delete`, as `gridRemovalAction` reads it: in a folder the key
    /// belongs to Remove from Folder, in the Trash it deletes permanently,
    /// elsewhere it moves the selection to the Trash.
    fn delete_items(&mut self, cx: &mut Context<Self>) {
        let kind = self.scope_kind(cx);
        let count = self.session.read(cx).selection.len();
        if (kind.folder.is_some() && !kind.trash) || count == 0 {
            cx.propagate();
        } else if kind.trash {
            // TODO: delete the selection permanently, after the confirmation
            // production shows. This prototype borrows the library's files
            // and must never delete them, so the key does nothing here.
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
                self.edit_selection(
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
    fn reject(&mut self, cx: &mut Context<Self>) {
        if !self.scope_kind(cx).inbox || self.session.read(cx).selection.is_empty() {
            cx.propagate();
        } else if settings::current(cx).reject_action == "trash" {
            self.session.update(cx, |session, cx| {
                session.move_selection_to(Lifecycle::Trash, cx)
            });
        } else {
            // TODO: delete the rejected items, after production's
            // confirmation. This prototype must never delete library files,
            // so with "Delete" chosen the key does nothing.
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
            self.run_menu_action("copy-path", window, cx);
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
        match self.session.read(cx).selection.len() {
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

    /// `organize.addTag`, `organize.addFolder` and `file.addToFolder`: the
    /// pickers for the selection.
    fn pick(&mut self, picker: Picker, cx: &mut Context<Self>) {
        if self.session.read(cx).selection.is_empty() {
            cx.propagate();
            return;
        }
        match picker {
            // TODO: open the tag picker (`TagSelectModal`) for the selection.
            Picker::Tags => {}
            // TODO: open the folder picker (`FolderPickerModal`) for the
            // selection.
            Picker::Folders => {}
            // TODO: open "Add to Folder..." (`addSelectionToFolder`) for the
            // selection.
            Picker::AddToFolder => {}
        }
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
        .on_action(cx.listener(|this, _: &FocusSearch, window, cx| this.focus_search(window, cx)))
        .on_action(cx.listener(|this, _: &ShowAll, _, cx| this.show_scope(ItemScope::All, cx)))
        .on_action(cx.listener(|this, _: &ShowInbox, _, cx| this.show_scope(ItemScope::Inbox, cx)))
        .on_action(
            cx.listener(|this, _: &ShowUntagged, _, cx| this.show_scope(ItemScope::Untagged, cx)),
        )
        .on_action(cx.listener(|this, _: &ShowTrash, _, cx| this.show_scope(ItemScope::Trash, cx)))
        .on_action(cx.listener(|this, _: &QuickLook, _, cx| this.quick_look(cx)))
        .on_action(cx.listener(|this, _: &OpenDefaultApp, window, cx| {
            if this.single_media(cx) {
                this.run_menu_action("open-default", window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|this, _: &RevealInFinder, window, cx| {
            if this.single_media(cx) {
                this.run_menu_action("reveal", window, cx);
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
        .on_action(cx.listener(|this, _: &DeleteItems, _, cx| this.delete_items(cx)))
        .on_action(cx.listener(|this, _: &RemoveFromFolder, _, cx| this.remove_from_folder(cx)))
        .on_action(cx.listener(|this, _: &RestoreItems, _, cx| {
            let trash = this.scope_kind(cx).trash;
            this.move_in(trash, Lifecycle::Active, cx)
        }))
        .on_action(cx.listener(|this, _: &AcceptInbox, _, cx| {
            let inbox = this.scope_kind(cx).inbox;
            this.move_in(inbox, Lifecycle::Active, cx)
        }))
        .on_action(cx.listener(|this, _: &RejectInbox, _, cx| this.reject(cx)))
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
        .on_action(cx.listener(|this, _: &AddTags, _, cx| this.pick(Picker::Tags, cx)))
        .on_action(cx.listener(|this, _: &AddToFolders, _, cx| this.pick(Picker::Folders, cx)))
        .on_action(cx.listener(|this, _: &AddToFolder, _, cx| this.pick(Picker::AddToFolder, cx)))
    }
}

/// The pickers the organize shortcuts open.
#[derive(Clone, Copy)]
enum Picker {
    Tags,
    Folders,
    AddToFolder,
}
