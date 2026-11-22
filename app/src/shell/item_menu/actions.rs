//! Performs the row that the user chose in an item menu. `run_item_action`
//! carries out each action through the session and the library, and the
//! other functions here make the edits that the actions need.
use super::{File, Target, media_link};
use crate::grid::menu::Action;
use crate::i18n::format::grouped;
use crate::i18n::{en, t};
use crate::session::{Report, Reread, edit_then, edit_then_in, read_then};
use crate::shell::Shell;
use gpui::*;
use picto_library::selection::SelectionTarget;
use picto_library::{ContentSortField, FolderId, Library, Lifecycle, RootKind};
use std::path::PathBuf;
use std::rc::Rc;

impl Shell {
    pub(super) fn run_item_action(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.items.target.clone() else {
            return;
        };
        let file = target.file.clone();
        let ids = target.ids();
        match action {
            // The actions from here to `LoadingApplications` open the file.
            Action::OpenDefault => {
                if let Some(file) = file {
                    self.with_path(file, |path, cx| crate::desktop::open_default(&path, cx), cx);
                }
            }
            Action::Reveal => {
                if let Some(file) = file {
                    self.with_path(file, |path, cx| crate::desktop::reveal(&path, cx), cx);
                }
            }
            Action::OpenInNewWindow if target.member => {
                // In a member's menu the row opens the member's file by
                // itself in a detail window, without its collection.
                if let (Some(file), Some(item)) = (file, target.items.first()) {
                    let size = self
                        .session
                        .read(cx)
                        .selected_details()
                        .and_then(|details| {
                            details
                                .media
                                .iter()
                                .find(|media| media.facts.content_hash == file.hash)
                                .map(|media| (media.facts.width, media.facts.height))
                        });
                    let entry = crate::viewer::detail::Entry {
                        root: item.root_id,
                        hash: file.hash,
                        mime: file.mime,
                        name: file.name,
                        width: size.and_then(|size| size.0),
                        height: size.and_then(|size| size.1),
                        collection: false,
                    };
                    crate::viewer::detail::open(self.session.clone(), vec![entry], 0, cx);
                }
            }
            Action::OpenInNewWindow => {
                // The row opens the item alone in a detail window. This is
                // the same for the menu of a single selected item, the
                // viewer's menu and the preview's menu.
                let ordinal = target
                    .items
                    .first()
                    .and_then(|item| self.session.read(cx).position_of(item.root_id));
                if let Some(ordinal) = ordinal {
                    self.open_in_window(vec![ordinal], ordinal, cx);
                }
            }
            Action::OpenWithApplication => {
                let application = self.items.chosen.take().map(PathBuf::from);
                if let (Some(file), Some(application)) = (file, application) {
                    self.with_path(
                        file,
                        move |path, cx| {
                            if let Err(error) = crate::desktop::open_with(&application, &path, cx) {
                                crate::ui::notify::error(t("Could not open file"), error, cx);
                            }
                        },
                        cx,
                    );
                }
            }
            // This row opens Windows' own application chooser for the file.
            // The row is offered only on Windows.
            Action::OpenWithOther => {
                if let Some(file) = file {
                    self.with_path(
                        file,
                        |path, cx| {
                            if let Err(error) = crate::desktop::open_with_chooser(&path, cx) {
                                crate::ui::notify::error(t("Could not open file"), error, cx);
                            }
                        },
                        cx,
                    );
                }
            }
            Action::LoadingApplications => {}
            // The actions from here to `CopySelection` copy to the clipboard.
            Action::CopyPath => {
                if let Some(file) = file {
                    self.with_path(file, |path, cx| copy_paths(&[path], cx), cx);
                }
            }
            Action::CopyName => {
                if let Some(file) = file {
                    write_text(file.name, cx);
                }
            }
            Action::CopyLink => {
                if let Some(file) = file {
                    write_text(media_link(&file.hash, &file.mime), cx);
                }
            }
            // A member's menu copies the names, the paths or the links of
            // the members that it acts on, and not those of the collection.
            Action::CopySelectionNames
            | Action::CopySelectionPaths
            | Action::CopySelectionLinks
                if target.member =>
            {
                let lines: Vec<String> = {
                    let session = self.session.read(cx);
                    let details = session.selected_details();
                    target
                        .members
                        .iter()
                        .filter_map(|index| details.and_then(|details| details.media.get(*index)))
                        .map(|media| match action {
                            Action::CopySelectionNames => display_name(&media.media_name),
                            Action::CopySelectionPaths => media.file_path.clone(),
                            _ => media_link(&media.facts.content_hash, &media.facts.mime),
                        })
                        .collect()
                };
                if action == Action::CopySelectionPaths {
                    let paths: Vec<PathBuf> = lines.into_iter().map(PathBuf::from).collect();
                    if !paths.is_empty() {
                        copy_paths(&paths, cx);
                    }
                } else if !lines.is_empty() {
                    write_text(lines.join("\n"), cx);
                }
            }
            // The names come from the loaded items. When the selection holds
            // items that are not loaded, nothing is copied.
            Action::CopySelectionNames if !target.loaded() => {}
            Action::CopySelectionNames => {
                let names = target
                    .items
                    .iter()
                    .map(|item| display_name(&item.name))
                    .collect::<Vec<_>>();
                write_text(names.join("\n"), cx);
            }
            Action::CopySelectionPaths | Action::CopySelectionLinks => {
                // The library returns every file of the selection in the
                // selection's order, whether its item is loaded or not. A
                // collection gives each of its members.
                let links = action == Action::CopySelectionLinks;
                self.with_files(target.selection(), cx, move |files, cx| {
                    if links {
                        let lines = files
                            .iter()
                            .map(|(hash, path)| link(hash, path))
                            .collect::<Vec<_>>();
                        if !lines.is_empty() {
                            write_text(lines.join("\n"), cx);
                        }
                    } else {
                        let paths = files.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
                        if !paths.is_empty() {
                            copy_paths(&paths, cx);
                        }
                    }
                });
            }
            Action::CopyFile => {
                // The row copies the one file that the menu was opened on.
                if let Some(file) = file {
                    self.with_path(
                        file,
                        |path, cx| {
                            if let Err(error) = crate::desktop::copy_files(&[path], cx) {
                                crate::ui::notify::error(en("Could not copy file"), error, cx);
                            }
                        },
                        cx,
                    );
                }
            }
            // A member's menu copies the files of the members that it acts
            // on.
            Action::CopySelection if target.member => {
                let paths: Vec<PathBuf> =
                    self.member_media(&target, cx, |media| PathBuf::from(&media.file_path));
                if let Err(error) = crate::desktop::copy_files(&paths, cx) {
                    crate::ui::notify::error(t("Could not copy selection"), error, cx);
                }
            }
            Action::CopySelection => self.copy_files(target.selection(), cx),
            // The actions from here to `DeletePermanently` accept, restore,
            // trash and delete items, and take members out of a collection.
            Action::Accept | Action::Restore => {
                self.set_lifecycle(&target, Lifecycle::Active, window, cx)
            }
            // In a member's menu, "Move to Trash" takes the members out of
            // their collection and moves them to the Trash.
            Action::MoveToTrash if target.member => self.detach_members(target, true, window, cx),
            Action::RemoveFromGroup => self.detach_members(target, false, window, cx),
            Action::MoveToTrash => self.set_lifecycle(&target, Lifecycle::Trash, window, cx),
            Action::Reject => {
                if crate::settings::current(cx).reject_action == "delete" {
                    self.request_permanent_delete(target, true, window, cx);
                } else {
                    self.set_lifecycle(&target, Lifecycle::Trash, window, cx);
                }
            }
            Action::DeletePermanently => {
                self.request_permanent_delete(target, false, window, cx);
            }
            // The actions from here to `SortContents` change folders.
            Action::AddToFolder => {
                self.dispatch(Box::new(crate::keymap::AddToFolder), window, cx);
                return;
            }
            Action::AddToLastFolder => {
                let last = self.session.read(cx).recent_folders.first().copied();
                if let Some(folder) = last {
                    self.session
                        .update(cx, |session, _| session.record_folder_use(folder));
                    let selection = target.selection();
                    let node = format!("folder:{}", folder.0);
                    let name = self
                        .session
                        .read(cx)
                        .nav
                        .iter()
                        .find(|entry| entry.id() == node)
                        .map(|entry| entry.label.clone())
                        .unwrap_or_default();
                    edit_then(
                        &self.session,
                        Report::failure(crate::tf!("Could not add to {value0}", name)),
                        Reread::SELECTION,
                        move |library| library.add_to_folder(&selection, folder),
                        cx,
                        // The selection is cleared when the items have been
                        // added.
                        |this, added, cx| {
                            if added.is_ok() {
                                this.session
                                    .update(cx, |session, cx| session.clear_selection(cx));
                            }
                        },
                    );
                }
            }
            Action::NewFolderWithSelection => {
                let selection = target.selection();
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.new_folder_in(None, Some(selection), window, cx)
                });
                return;
            }
            Action::NewFolder => {
                let parent = target.folder;
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.new_folder_in(parent, None, window, cx)
                });
                return;
            }
            Action::RemoveFromFolder => {
                if let Some(folder) = target.folder {
                    let selection = target.selection();
                    edit_then(
                        &self.session,
                        Report::default(),
                        Reread::RESULTS,
                        move |library| library.remove_from_folder(&selection, folder),
                        cx,
                        |_, _, _| {},
                    );
                }
            }
            Action::SetFolderCover => {
                if let (Some(folder), Some(id)) = (target.folder, ids.first().copied()) {
                    edit_then(
                        &self.session,
                        Report::failure(t("Could not set folder cover")),
                        Reread::SELECTION,
                        move |library| library.set_folder_cover(folder, id),
                        cx,
                        |_, _, _| {},
                    );
                }
            }
            Action::SortContents(field) if target.group => {
                if target.quick_look {
                    let details = self.session.read(cx).selected_details().cloned();
                    if let Some(details) = details {
                        let root = details.root.root_id;
                        let answer = crate::viewer::group::sort(&self.session, &details, field, cx);
                        let quick_look = self.quick_look.downgrade();
                        cx.spawn(async move |_, cx| {
                            if let Ok(result) = answer.await {
                                let _ = quick_look.update(cx, |quick_look, cx| {
                                    let failure =
                                        result.err().map(|error| error.to_string().into());
                                    quick_look.collection_edited(root, failure, cx);
                                });
                            }
                        })
                        .detach();
                    }
                } else {
                    self.viewer
                        .update(cx, |viewer, cx| viewer.sort_collection(field, cx));
                }
            }
            Action::SortContents(field) => {
                if let Some(folder) = target.folder {
                    self.sort_folder(folder, field, cx);
                }
            }
            // The actions from here to `EditGroup` make, remove and edit
            // collections.
            Action::Group => self.group(target, cx),
            Action::Ungroup if target.group => self.confirm_ungroup_shown(target, cx),
            Action::Ungroup => self.confirm_ungroup(target, cx),
            Action::EditGroup => {
                // The collection opens in the media view, in its editor.
                self.viewer
                    .update(cx, |viewer, cx| viewer.edit_collection(window, cx));
                self.session
                    .update(cx, |session, cx| session.open_viewer(cx));
                return;
            }
            // The actions from here to `Rate` change names, tags and
            // ratings.
            Action::Rename => {
                if self.inspector_collapsed {
                    self.toggle_rails(false, true, cx);
                }
                self.inspector
                    .update(cx, |inspector, cx| inspector.edit_name(window, cx));
                return;
            }
            // The dialog lists the name of each item, so it opens only when
            // every selected item is loaded.
            Action::BatchRename if target.loaded() => self.batch_rename(target.items, cx),
            Action::BatchRename => {}
            Action::AddTags => {
                self.dispatch(Box::new(crate::keymap::AddTags), window, cx);
                return;
            }
            Action::AutoTag => {
                // When the menu was opened in the viewer or in a collection's
                // view, the Auto Tag review is given the items that the menu
                // acts on. When it was opened on the grid, the review is
                // given no items. It then reviews the grid's selection, also
                // after the selection changes.
                let session = self.session.read(cx);
                let named = target.member || target.group || session.viewer;
                let explicit = named.then(|| target.selection());
                // From a menu of the grid, the review opens in the centre of
                // the window, where Add Tags and Add to Folder open their
                // dialogs. While the viewer is open and the inspector is
                // shown, the review is docked where the viewer's pickers are
                // docked.
                let docked = session.viewer && !self.inspector_collapsed;
                self.inspector.update(cx, |inspector, cx| {
                    inspector.open_auto_tag(explicit, docked, window, cx)
                });
                return;
            }
            Action::CopyTags => self.copy_tags(target, cx),
            Action::PasteTags => {
                let tags = self.items.tag_clipboard.clone();
                if !tags.is_empty() && target.count() > 0 {
                    let selection = target.selection();
                    edit_then(
                        &self.session,
                        Report::default(),
                        Reread::SELECTION,
                        move |library| library.apply_tags(&selection, &tags, true),
                        cx,
                        |_, _, _| {},
                    );
                }
            }
            Action::Rate(stars) => {
                let rating = picto_library::Rating::ALL[usize::from(stars).min(5)];
                let selection = target.selection();
                edit_then(
                    &self.session,
                    Report::default(),
                    Reread::SELECTION,
                    move |library| library.set_rating(&selection, rating),
                    cx,
                    |_, _, _| {},
                );
            }
            // The export actions come next, and after them the actions that
            // belong to none of the groups above.
            Action::ExportOriginals | Action::ExportAs | Action::ExportPictoPack => {
                // The export takes everything that the menu acts on. For the
                // grid's selection that includes the selected items that are
                // not loaded.
                let selection = target.selection();
                let count = target.count();
                if let Some(exports) = crate::export::get(cx) {
                    exports.update(cx, |exports, cx| match action {
                        Action::ExportOriginals => exports.export_originals(selection, cx),
                        Action::ExportAs => exports.export_as(selection, count, cx),
                        _ => exports.export_pack(
                            crate::export::PackSource::Items(selection),
                            count,
                            None,
                            cx,
                        ),
                    });
                }
            }
            // The user chose a search engine in the submenu. The original
            // file is given to the search with that engine.
            Action::SearchByImage(engine) => {
                if let Some(file) = file {
                    let mime = file.mime.clone();
                    self.with_path(
                        file,
                        move |path, cx| {
                            crate::search_by_image::search(
                                engine,
                                crate::search_by_image::Upload { path, mime },
                                cx,
                            )
                        },
                        cx,
                    );
                }
            }
            Action::SetLibraryCover => {
                // The library cover dialog opens with the file that the menu
                // was opened on as its candidate.
                if let (Some(item), Some(file)) = (target.items.first(), file) {
                    let candidate = crate::ui::cover_dialog::Candidate {
                        media_item: i64::from(item.root_id.0),
                        size: (item.width.unwrap_or(1), item.height.unwrap_or(1)),
                        thumbnail: crate::grid::thumbnails::present(
                            &self.session.read(cx).thumbnail(&file.hash),
                        ),
                        hash: file.hash,
                    };
                    self.open_library_cover(Some(candidate), cx);
                }
            }
            Action::FindMediaMatches => {
                if let Some(item) = ids.first().copied() {
                    self.session
                        .update(cx, |session, cx| session.open_media_matches(item, cx));
                }
            }
            Action::RegenerateThumbnails => {
                // A member's menu that acts on several members regenerates
                // the thumbnail of each of them. A menu for one member or
                // one item regenerates the thumbnail of its one file, which
                // in the viewer is the file shown. A menu for several items
                // regenerates the thumbnail of each loaded item that has a
                // content hash.
                let files = match &file {
                    _ if target.member && target.members.len() > 1 => {
                        self.member_media(&target, cx, |media| {
                            (media.facts.content_hash.clone(), media.facts.mime.clone())
                        })
                    }
                    Some(file) if target.member || target.items.len() == 1 => {
                        vec![(file.hash.clone(), file.mime.clone())]
                    }
                    _ => target
                        .items
                        .iter()
                        .filter(|item| !item.content_hash.is_empty())
                        .map(|item| (item.content_hash.clone(), item.mime.clone()))
                        .collect(),
                };
                self.regenerate_thumbnails(files, cx);
            }
            Action::Grayscale => {
                // The menu stays open, and the row's check mark is set to
                // the new state.
                self.session
                    .update(cx, |session, cx| session.toggle_grayscale(cx));
                let on = self.session.read(cx).grayscale;
                if let Some(menu) = &self.items.open {
                    let _ = menu.update(cx, |menu, cx| {
                        menu.set_checked(Action::Grayscale.key(), on, cx)
                    });
                }
                return;
            }
            Action::NewSmartFolder => {
                self.dispatch(Box::new(crate::keymap::NewSmartFolder), window, cx);
                return;
            }
            Action::ImportFiles => self.import(crate::shell::import::Import::Files, window, cx),
            Action::ImportFolder => self.import(crate::shell::import::Import::Folder, window, cx),
            Action::PasteImport => self.import(crate::shell::import::Import::Paste, window, cx),
            Action::FlashPlay | Action::FlashStop | Action::FlashMute => {
                use crate::viewer::flash::Flash;
                let control: Box<dyn Fn(&mut Flash)> = match action {
                    Action::FlashPlay => Box::new(Flash::toggle_play),
                    Action::FlashStop => Box::new(Flash::stop),
                    _ => Box::new(Flash::toggle_mute),
                };
                if let Some(surface) = self.items.surface.as_ref().and_then(|s| s.upgrade()) {
                    surface.update(cx, |surface, cx| surface.control_flash(control, cx));
                }
            }
            Action::SetAsThumbnail => {
                let surface = self.items.surface.as_ref().and_then(|s| s.upgrade());
                if let (Some(surface), Some(file)) = (surface, file) {
                    crate::viewer::capture::set_as_thumbnail(
                        &surface,
                        &self.session,
                        file.hash,
                        cx,
                    );
                }
            }
        }
        if self.items.surface.is_none() {
            window.focus(&self.focus, cx);
        }
    }

    /// Returns what `read` returns for the media record of each member that
    /// a member's menu acts on.
    fn member_media<T>(
        &self,
        target: &Target,
        cx: &App,
        read: impl Fn(&picto_library::MediaRecord) -> T,
    ) -> Vec<T> {
        let details = self.session.read(cx).selected_details();
        target
            .members
            .iter()
            .filter_map(|index| details.and_then(|details| details.media.get(*index)))
            .map(read)
            .collect()
    }

    /// Puts the files of `selection` on the clipboard. A collection gives
    /// each of its members. When the files cannot be copied, a notification
    /// says why.
    pub(in crate::shell) fn copy_files(
        &mut self,
        selection: SelectionTarget,
        cx: &mut Context<Self>,
    ) {
        self.with_files(selection, cx, |files, cx| {
            let paths: Vec<PathBuf> = files.into_iter().map(|(_, path)| path).collect();
            let copied = if paths.is_empty() {
                Err("The selection has no physical files to copy.".to_owned())
            } else {
                crate::desktop::copy_files(&paths, cx)
            };
            if let Err(error) = copied {
                crate::ui::notify::error(t("Could not copy selection"), error, cx);
            }
        });
    }

    /// Asks the session for the content hash and the path of every file of
    /// `selection` ([`crate::session::Session::resolve_files`]) and calls
    /// `then` with them. A failure is printed to the console.
    fn with_files(
        &mut self,
        selection: SelectionTarget,
        cx: &mut Context<Self>,
        then: impl FnOnce(Vec<(String, PathBuf)>, &mut App) + 'static,
    ) {
        let files = self.session.read(cx).resolve_files(&selection);
        cx.spawn(async move |_, cx| match files.await {
            Ok(files) => {
                let _ = cx.update(|cx| then(files, cx));
            }
            Err(error) => eprintln!("picto: resolving the selection's files failed: {error}"),
        })
        .detach();
    }

    /// Makes the thumbnails of these files again (see
    /// [`crate::grid::thumbnails::regenerate`]). Each file is given by its
    /// content hash and its type.
    pub(in crate::shell) fn regenerate_thumbnails(
        &mut self,
        files: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        crate::grid::thumbnails::regenerate(&self.session, files, cx);
    }

    /// Opens the batch rename dialog on these items, with the names that
    /// the grid shows. An item without a name is listed as "Untitled".
    pub(in crate::shell) fn batch_rename(
        &mut self,
        items: Vec<picto_library::query::RootSummary>,
        cx: &mut Context<Self>,
    ) {
        let items = items
            .into_iter()
            .map(|item| crate::shell::batch_rename::Item {
                id: item.root_id,
                name: display_name(&item.name),
            })
            .collect();
        self.batch_rename
            .update(cx, |dialog, cx| dialog.open(items, cx));
    }

    /// Opens the dialog that sets the cover of the open library. When
    /// `candidate` is given, the dialog opens with that picture as its
    /// candidate.
    pub fn open_library_cover(
        &mut self,
        candidate: Option<crate::ui::cover_dialog::Candidate>,
        cx: &mut Context<Self>,
    ) {
        self.library_cover
            .update(cx, |dialog, cx| dialog.open(candidate, cx));
    }

    /// Opens the update dialog. `updates` calls this when the user asks to
    /// see the update, from the application menu or from the View button of
    /// a notification.
    pub fn open_updates(&mut self, cx: &mut Context<Self>) {
        self.updates.update(cx, |dialog, cx| dialog.open(cx));
    }

    /// Sends an action that another part of the app handles. The keyboard
    /// focus is given to the shell first, because the menu that had it has
    /// closed, and the action is dispatched from there.
    fn dispatch(&mut self, action: Box<dyn gpui::Action>, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
        window.dispatch_action(action, cx);
    }

    /// Reads the path at which the library keeps `file` from the details of
    /// the file's item, and calls `then` with the path. `then` is not called
    /// when the path cannot be read.
    fn with_path(
        &mut self,
        file: File,
        then: impl FnOnce(PathBuf, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let File { root, hash, .. } = file;
        let work = move |library: &Library| super::file_path(library, root, &hash);
        read_then(
            &self.session,
            work,
            cx,
            |_, path, cx| {
                if let Ok(Some(path)) = path {
                    then(path, cx);
                }
            },
        );
    }

    /// Moves the items that the menu was opened for to the Trash, back out
    /// of the Trash, or out of the Inbox. The viewer's rows move the item
    /// that the viewer shows, and not the grid's selection.
    ///
    /// When the media view or Quick Look shows the one item that is moved,
    /// the view stays open and shows the item that takes its place: the
    /// next item, or the one before it when the moved item was the last.
    /// When there is no other item, the view closes. If the edit fails, the
    /// view goes back to the moved item.
    ///
    /// In every other case the media view is closed before the items are
    /// moved.
    fn set_lifecycle(
        &mut self,
        target: &Target,
        lifecycle: Lifecycle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target.count() == 0 {
            return;
        }
        let quick_look = self.quick_look.read(cx).is_open();
        let (selection, shown) = {
            let session = self.session.read(cx);
            let viewing = session.viewer || quick_look;
            let shown = session
                .focus_ordinal()
                .zip(session.selected_item().map(|item| item.root_id))
                .filter(|(_, id)| viewing && target.ids() == [*id]);
            (target.selection(), shown)
        };
        let Some((ordinal, id)) = shown else {
            self.session.update(cx, |session, cx| {
                session.close_viewer(cx);
                let _ = session.set_lifecycle(selection, lifecycle, cx);
            });
            return;
        };
        let next = {
            let session = self.session.read(cx);
            [ordinal + 1, ordinal.wrapping_sub(1)]
                .into_iter()
                .find(|neighbour| session.item_at(*neighbour).is_some())
        };
        let answer = self.session.update(cx, |session, cx| {
            match next {
                Some(next) => session.select(next, cx),
                None => session.close_viewer(cx),
            }
            session.set_lifecycle(selection, lifecycle, cx)
        });
        if next.is_none() && quick_look {
            self.quick_look
                .update(cx, |quick_look, cx| quick_look.close(window, cx));
        }
        let session = self.session.downgrade();
        cx.spawn(async move |_, cx| {
            if let Ok(Err(_)) = answer.await {
                let _ = session.update(cx, |session, cx| {
                    if let Some(back) = session.position_of(id) {
                        session.select(back, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn sort_folder(&mut self, folder: FolderId, field: ContentSortField, cx: &mut Context<Self>) {
        edit_then(
            &self.session,
            Report::default(),
            Reread::RESULTS,
            move |library| library.sort_folder_items(folder, field),
            cx,
            |_, _, _| {},
        );
    }

    /// Does "Copy Tags". It reads the tags of one item, or the tags that
    /// every selected item has, writes their names to the clipboard as a
    /// JSON list, and keeps them for "Paste Tags".
    fn copy_tags(&mut self, target: Target, cx: &mut Context<Self>) {
        let single = target.count() == 1 && target.items.len() == 1;
        let ids = target.ids();
        let selection = target.selection();
        let work = move |library: &Library| {
            let tag_ids = if single {
                library.details(ids[0])?.tag_ids
            } else {
                library.selection_summary(&selection)?.shared_tags
            };
            Ok::<_, picto_library::LibraryError>(
                library
                    .tags_by_id(&tag_ids)?
                    .iter()
                    .map(crate::tags::record_name)
                    .collect::<Vec<_>>(),
            )
        };
        read_then(
            &self.session,
            work,
            cx,
            |this, tags: picto_library::Result<Vec<String>>, cx| {
                if let Ok(tags) = tags {
                    write_text(serde_json::to_string(&tags).unwrap_or_default(), cx);
                    this.items.tag_clipboard = tags;
                }
            },
        );
    }

    /// Does the group row. When the selection holds exactly one collection
    /// and the library's draft of the collection's notes has no source
    /// (`source_count` is 0), the selection is merged into that collection
    /// at once. In every other case the group organizer opens, and the user
    /// decides there.
    fn group(&mut self, target: Target, cx: &mut Context<Self>) {
        let Some(cover) = target.items.first().map(|item| item.root_id) else {
            return;
        };
        let selection = target.selection();
        edit_then(
            &self.session,
            Report::failure(t("Could not create group")).and_announce("collections.organize"),
            Reread::NOTHING,
            // The work returns the merged group, or the request that the
            // group organizer is opened with.
            move |library: &Library| -> picto_library::Result<
                Result<picto_library::RootId, super::organizer::Request>,
            > {
                let summary = library.selection_summary(&selection)?;
                let notes = library.collection_note_draft(&selection)?;
                if let ([group], 0) = (summary.collection_candidates.as_slice(), notes.source_count)
                {
                    let (merged, _) =
                        library.organize_into_collection(&picto_library::GroupRequest {
                            target: selection,
                            cover_root_id: cover,
                            winning_collection_id: Some(group.collection_id),
                            name: None,
                            notes: None,
                            modified_at_ms: crate::now_ms(),
                        })?;
                    Ok(Ok(merged))
                } else {
                    // In every other case the user decides in the group
                    // organizer.
                    Ok(Err(super::organizer::Request {
                        target: selection,
                        cover,
                        groups: summary.collection_candidates,
                        notes: notes.notes,
                        maximum_bytes: notes.maximum_bytes,
                    }))
                }
            },
            cx,
            |this, decide, cx| match decide {
                Ok(Ok(merged)) => {
                    // The selection is cleared, and the merged group is
                    // selected alone once the results list it.
                    this.session.update(cx, |session, cx| {
                        session.clear_selection(cx);
                        session.select_when_listed(merged, cx)
                    });
                }
                Ok(Err(request)) => this.open_organizer(request, cx),
                Err(_) => {}
            },
        );
    }

    /// Does "Remove from Group" and "Move to Trash" of a member's menu. The
    /// members that the menu acts on are taken out of the collection that
    /// the media view or Quick Look shows. With `trash` set, they are also
    /// moved to the Trash. Quick Look closes when at most one member is
    /// left, because the collection then no longer exists.
    fn detach_members(
        &mut self,
        target: Target,
        trash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !target.quick_look {
            self.viewer.update(cx, |viewer, cx| {
                viewer.detach_members(target.members, trash, cx)
            });
            return;
        }
        let Some(details) = self.session.read(cx).selected_details().cloned() else {
            return;
        };
        let ids: Vec<_> = target
            .members
            .iter()
            .filter_map(|index| details.media.get(*index))
            .map(|media| media.media_id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let closes = details.media.len() - ids.len() <= 1;
        let root = details.root.root_id;
        let answer = crate::viewer::group::detach(&self.session, root, ids, trash, cx);
        let quick_look = self.quick_look.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = quick_look.update_in(cx, |quick_look, window, cx| match result {
                Ok(()) if closes => quick_look.close(window, cx),
                result => {
                    let failure = result.err().map(|error| error.to_string().into());
                    quick_look.collection_edited(root, failure, cx);
                }
            });
        })
        .detach();
    }

    /// Asks the user to confirm Ungroup for the collection that the media
    /// view or Quick Look shows. When the user confirms, the members return
    /// to the library as separate items and the view closes.
    fn confirm_ungroup_shown(&mut self, target: Target, cx: &mut Context<Self>) {
        let Some(root) = target.items.first().map(|item| item.root_id) else {
            return;
        };
        let quick_look = target.quick_look;
        self.ask(
            t("Ungroup?"),
            en("The media will return to the library as separate items. Files and metadata will not be deleted."),
            t("Ungroup"),
            false,
            Rc::new(move |this, window, cx| {
                edit_then_in(
                    &this.session,
                    Report::announce("collections.ungroup"),
                    Reread::RESULTS,
                    move |library| library.ungroup_collection(root, crate::now_ms()),
                    window,
                    cx,
                    move |this, ungrouped, window, cx| match ungrouped {
                        Ok(_) if quick_look => this
                            .quick_look
                            .update(cx, |quick_look, cx| quick_look.close(window, cx)),
                        Ok(_) => this
                            .session
                            .update(cx, |session, cx| session.close_viewer(cx)),
                        Err(_) => {}
                    },
                );
            }),
            cx,
        );
    }

    fn confirm_ungroup(&mut self, target: Target, cx: &mut Context<Self>) {
        let selection = target.selection();
        // The number of collections is the library's count when the library
        // has given one. Otherwise it is counted from the loaded items,
        // which is possible only when they are the whole selection. In the
        // remaining case the number is not known.
        let loaded = target.loaded();
        let collections = target.collections.or_else(|| {
            loaded.then(|| {
                target
                    .items
                    .iter()
                    .filter(|item| item.kind == RootKind::Collection)
                    .count() as u64
            })
        });
        let title = match collections {
            Some(1) => t("Ungroup?").to_owned(),
            Some(n) => crate::tf!("Ungroup {value0} collections?", grouped(n)),
            None => t("Ungroup the selected collections?").to_owned(),
        };
        let message = if collections == Some(1) && loaded && target.items.len() == 1 {
            t(
                "Every member will return to the library as a separate item. Files and metadata will not be deleted.",
            )
        } else {
            t(
                "Every member of these collections will return to the library as a separate item. Other selected items stay as they are. Files and metadata will not be deleted.",
            )
        };
        self.ask(
            title,
            message,
            t("Ungroup"),
            false,
            Rc::new(move |this, _, cx| {
                let selection = selection.clone();
                this.session
                    .update(cx, |session, cx| session.clear_selection(cx));
                edit_then(
                    &this.session,
                    Report::failure(t("Could not ungroup items"))
                        .and_announce("collections.ungroup"),
                    Reread::RESULTS,
                    move |library| library.ungroup_selection(&selection, crate::now_ms()),
                    cx,
                    |_, _, _| {},
                );
            }),
            cx,
        );
    }
}

/// Returns an item's name as the grid shows it, which is "Untitled" for an
/// item without a name.
fn display_name(name: &str) -> String {
    if name.is_empty() {
        "Untitled".to_owned()
    } else {
        name.to_owned()
    }
}

fn write_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// Does "Copy File Path": puts the paths of the files on the clipboard as
/// text. `desktop::copy_paths` does this only if every path is a file
/// inside the open library copy. Otherwise a notification shows its error.
fn copy_paths(paths: &[PathBuf], cx: &mut App) {
    if let Err(error) = crate::desktop::copy_paths(paths, cx) {
        crate::ui::notify::error(en("Could not copy file path"), error, cx);
    }
}

/// Returns the `media://` link of the file whose content hash is `hash`.
/// The link has the extension of the file's path, or `bin` when the path
/// has none.
fn link(hash: &str, path: &std::path::Path) -> String {
    let extension = path.extension().map_or("bin".into(), |extension| {
        extension.to_string_lossy().into_owned()
    });
    format!("media://localhost/file/{hash}.{extension}")
}
