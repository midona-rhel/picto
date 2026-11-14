//! What a chosen item-menu row does: the dispatch of each action through
//! the session and the library, and the edits behind them.
use super::{File, Target, media_link};
use crate::grid::format::grouped;
use crate::grid::menu::Action;
use crate::session::{Report, Reread, edit_then, edit_then_in};
use crate::shell::Shell;
use gpui::*;
use picto_library::{ContentSortField, FolderId, Library, Lifecycle, RootId, RootKind};
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
            // ── Opening ──
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
                // The member's file by itself (`openDetailWindow({ hash })`).
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
                        name: file.name,
                        width: size.and_then(|size| size.0),
                        height: size.and_then(|size| size.1),
                        collection: false,
                    };
                    crate::viewer::detail::open(self.session.clone(), vec![entry], 0, cx);
                }
            }
            Action::OpenInNewWindow => {
                // The item alone, as production's detail window opens it
                // from a single selection, the viewer and the preview.
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
                                crate::ui::notify::error("Could not open file", error, cx);
                            }
                        },
                        cx,
                    );
                }
            }
            // Windows' own application chooser on the file, the only
            // system that offers this row.
            Action::OpenWithOther => {
                if let Some(file) = file {
                    self.with_path(
                        file,
                        |path, cx| {
                            if let Err(error) = crate::desktop::open_with_chooser(&path, cx) {
                                crate::ui::notify::error("Could not open file", error, cx);
                            }
                        },
                        cx,
                    );
                }
            }
            Action::LoadingApplications => {}
            // ── Copying ──
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
            // The members a member's menu acts on: their own names and
            // files (`onCopySelection*` over `selected`).
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
            Action::CopySelectionNames => {
                let names = target
                    .items
                    .iter()
                    .map(|item| display_name(&item.name))
                    .collect::<Vec<_>>();
                write_text(names.join("\n"), cx);
            }
            Action::CopySelectionPaths | Action::CopySelectionLinks => {
                // `resolveTargetFilePaths`: every file of the selection,
                // collection members included, in order.
                let links = action == Action::CopySelectionLinks;
                let report = Report::default();
                edit_then(
                    &self.session,
                    report,
                    Reread::NOTHING,
                    files_of(ids),
                    cx,
                    move |_, files, cx| {
                        let files = files.into_iter().flatten();
                        if links {
                            let lines = files
                                .map(|(hash, path)| link(&hash, &path))
                                .collect::<Vec<_>>();
                            if !lines.is_empty() {
                                write_text(lines.join("\n"), cx);
                            }
                        } else {
                            let paths = files
                                .map(|(_, path)| PathBuf::from(path))
                                .collect::<Vec<_>>();
                            if !paths.is_empty() {
                                copy_paths(&paths, cx);
                            }
                        }
                    },
                );
            }
            Action::CopyFile => {
                // `copyFileForHash`: the one file the row is about.
                if let Some(file) = file {
                    self.with_path(
                        file,
                        |path, cx| {
                            if let Err(error) = crate::desktop::copy_files(&[path], cx) {
                                crate::ui::notify::error("Could not copy file", error, cx);
                            }
                        },
                        cx,
                    );
                }
            }
            // A member's menu copies the members it acts on
            // (`copyHashes(selected)`).
            Action::CopySelection if target.member => {
                let paths: Vec<PathBuf> =
                    self.member_media(&target, cx, |media| PathBuf::from(&media.file_path));
                if let Err(error) = crate::desktop::copy_files(&paths, cx) {
                    crate::ui::notify::error("Could not copy selection", error, cx);
                }
            }
            Action::CopySelection => self.copy_files(ids, cx),
            // ── Lifecycle ──
            Action::Accept | Action::Restore => self.set_lifecycle(&target, Lifecycle::Active, cx),
            // A member leaves its collection into the Trash
            // (`detachMembers(selected, 'trash')`).
            Action::MoveToTrash if target.member => self.detach_members(target, true, window, cx),
            Action::RemoveFromGroup => self.detach_members(target, false, window, cx),
            Action::MoveToTrash => self.set_lifecycle(&target, Lifecycle::Trash, cx),
            Action::Reject => {
                if crate::settings::current(cx).reject_action == "delete" {
                    self.request_permanent_delete(target, true, window, cx);
                } else {
                    self.set_lifecycle(&target, Lifecycle::Trash, cx);
                }
            }
            Action::DeletePermanently => {
                self.request_permanent_delete(target, false, window, cx);
            }
            // ── Folders ──
            Action::AddToFolder => {
                self.dispatch(Box::new(crate::keymap::AddToFolder), window, cx);
                return;
            }
            Action::AddToLastFolder => {
                let last = self.session.read(cx).recent_folders.first().copied();
                if let Some(folder) = last {
                    self.session
                        .update(cx, |session, _| session.record_folder_use(folder));
                    let selection = target.selection(self.session.read(cx));
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
                        Report::failure(format!("Could not add to {name}")),
                        Reread::SELECTION,
                        move |library| library.add_to_folder(&selection, folder),
                        cx,
                        |_, _, _| {},
                    );
                }
            }
            Action::NewFolderWithSelection => {
                let selection = target.selection(self.session.read(cx));
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
                    let selection = target.selection(self.session.read(cx));
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
                        Report::failure("Could not set folder cover"),
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
            // ── Groups ──
            Action::Group => self.group(target, cx),
            Action::Ungroup if target.group => self.confirm_ungroup_shown(target, cx),
            Action::Ungroup => self.confirm_ungroup(target, cx),
            Action::EditGroup => {
                // `openGridItem(item, items, 'editor')`: the collection opens
                // in the media view, in its editor.
                self.viewer
                    .update(cx, |viewer, cx| viewer.edit_collection(window, cx));
                self.session
                    .update(cx, |session, cx| session.open_viewer(cx));
                return;
            }
            // ── Names and tags ──
            Action::Rename => {
                if self.inspector_collapsed {
                    self.toggle_rails(false, true, cx);
                }
                self.inspector
                    .update(cx, |inspector, cx| inspector.edit_name(window, cx));
                return;
            }
            Action::BatchRename => self.batch_rename(target.items, cx),
            Action::AddTags => {
                self.dispatch(Box::new(crate::keymap::AddTags), window, cx);
                return;
            }
            // TODO: open the AI tagger on the selection, beside the inspector
            // (the row stays disabled until then).
            Action::AutoTag => {}
            Action::CopyTags => self.copy_tags(target, cx),
            Action::PasteTags => {
                let tags = self.items.tag_clipboard.clone();
                if !tags.is_empty() && !ids.is_empty() {
                    let selection = target.selection(self.session.read(cx));
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
                let selection = target.selection(self.session.read(cx));
                edit_then(
                    &self.session,
                    Report::default(),
                    Reread::SELECTION,
                    move |library| library.set_rating(&selection, rating),
                    cx,
                    |_, _, _| {},
                );
            }
            // ── Exporting ──
            Action::ExportOriginals | Action::ExportAs | Action::ExportPictoPack => {
                // The whole selection, loaded or not, when these are it.
                let session = self.session.read(cx);
                let selection = target.selection(session);
                let count = match &selection {
                    picto_library::selection::SelectionTarget::Explicit { root_ids } => {
                        root_ids.len() as u64
                    }
                    _ => session.selection_count(),
                };
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
            // TODO: reverse image search (the rows stay disabled until then).
            Action::SearchByImage(_) => {}
            Action::SetLibraryCover => {
                // The shown file, framed at once (`initialCandidate`).
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
                // The selection's own files, a collection's never (the row
                // is not offered for one); the viewer's, the file shown.
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
                // The row stays open and follows the change.
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

    /// What a member's menu reads of each member it acts on.
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

    /// `copyTarget`: the files of these items, collection members
    /// included, on the clipboard, or a notification saying why not.
    pub(in crate::shell) fn copy_files(&mut self, ids: Vec<RootId>, cx: &mut Context<Self>) {
        let report = Report::default();
        edit_then(
            &self.session,
            report,
            Reread::NOTHING,
            files_of(ids),
            cx,
            |_, files, cx| {
                let Ok(files) = files else {
                    return;
                };
                let paths: Vec<PathBuf> = files.into_iter().map(|(_, path)| path.into()).collect();
                let copied = if paths.is_empty() {
                    Err("The selection has no physical files to copy.".to_owned())
                } else {
                    crate::desktop::copy_files(&paths, cx)
                };
                if let Err(error) = copied {
                    crate::ui::notify::error("Could not copy selection", error, cx);
                }
            },
        );
    }

    /// `media.regenerate_thumbnails` for these files (see
    /// [`crate::grid::thumbnails::regenerate`]).
    pub(in crate::shell) fn regenerate_thumbnails(
        &mut self,
        files: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        crate::grid::thumbnails::regenerate(&self.session, files, cx);
    }

    /// `batchRenameModalAtom`: the dialog on these items, as the grid shows
    /// them (an unnamed one as "Untitled").
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

    /// `openCurrentLibraryCoverPicker`, framing `candidate` at once when
    /// it is given.
    pub(crate) fn open_library_cover(
        &mut self,
        candidate: Option<crate::ui::cover_dialog::Candidate>,
        cx: &mut Context<Self>,
    ) {
        self.library_cover
            .update(cx, |dialog, cx| dialog.open(candidate, cx));
    }

    /// Sends an action for another part of the app from the shell, which
    /// has the keyboard once the menu has closed.
    fn dispatch(&mut self, action: Box<dyn gpui::Action>, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
        window.dispatch_action(action, cx);
    }

    /// `resolveFilePath`: where the library keeps the file, from the item's
    /// details, then `then` with it.
    fn with_path(
        &mut self,
        file: File,
        then: impl FnOnce(PathBuf, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let File { root, hash, .. } = file;
        let work = move |library: &Library| super::file_path(library, root, &hash);
        edit_then(
            &self.session,
            Report::default(),
            Reread::NOTHING,
            work,
            cx,
            |_, path, cx| {
                if let Ok(Some(path)) = path {
                    then(path, cx);
                }
            },
        );
    }

    /// Moves the items the menu was opened for into or out of the Trash or
    /// the Inbox (production's `setTargetLifecycle(target, …)`: the viewer's
    /// rows move the item it shows, not the grid's selection), closing the
    /// media view, and reads the grid again in place.
    fn set_lifecycle(&mut self, target: &Target, lifecycle: Lifecycle, cx: &mut Context<Self>) {
        if target.items.is_empty() {
            return;
        }
        self.session.update(cx, |session, cx| {
            let selection = target.selection(session);
            session.close_viewer(cx);
            let _ = session.set_lifecycle(selection, lifecycle, cx);
        });
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

    /// `onCopyTags`: one item's tags, or the tags every selected item
    /// shares, as JSON on the clipboard and kept for "Paste Tags".
    fn copy_tags(&mut self, target: Target, cx: &mut Context<Self>) {
        let single = target.items.len() == 1;
        let ids = target.ids();
        let selection = target.selection(self.session.read(cx));
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
        edit_then(
            &self.session,
            Report::default(),
            Reread::NOTHING,
            work,
            cx,
            |this, tags, cx| {
                if let Ok(tags) = tags {
                    write_text(serde_json::to_string(&tags).unwrap_or_default(), cx);
                    this.items.tag_clipboard = tags;
                }
            },
        );
    }

    /// `organizeSelection`: one selected group and no notes to merge groups
    /// at once; anything else needs the group organizer.
    fn group(&mut self, target: Target, cx: &mut Context<Self>) {
        let Some(cover) = target.items.first().map(|item| item.root_id) else {
            return;
        };
        let selection = target.selection(self.session.read(cx));
        edit_then(
            &self.session,
            Report::failure("Could not create group").and_announce("collections.organize"),
            Reread::NOTHING,
            move |library: &Library| -> picto_library::Result<Option<super::organizer::Request>> {
                let summary = library.selection_summary(&selection)?;
                let notes = library.collection_note_draft(&selection)?;
                if let ([group], 0) = (summary.collection_candidates.as_slice(), notes.source_count)
                {
                    library.organize_into_collection(&picto_library::GroupRequest {
                        target: selection,
                        cover_root_id: cover,
                        winning_collection_id: Some(group.collection_id),
                        name: None,
                        notes: None,
                        modified_at_ms: crate::now_ms(),
                    })?;
                    Ok(None)
                } else {
                    // Anything else is the organizer's to decide.
                    Ok(Some(super::organizer::Request {
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
                Ok(None) => {
                    // `clearSelection`, then the merged group's results.
                    // TODO: select the merged group once the results hold
                    // it, as production does.
                    this.session.update(cx, |session, cx| {
                        session.clear_selection(cx);
                        session.reread(Reread::RESULTS, cx)
                    });
                }
                Ok(Some(request)) => this.open_organizer(request, cx),
                Err(_) => {}
            },
        );
    }

    /// A member's "Remove from Group" and "Move to Trash": the members the
    /// menu acts on leave the collection the media view or Quick Look
    /// shows, which closes once the collection is gone.
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

    /// `confirmUngroup` in the collection's view: on Ungroup the members
    /// return to the library and the view closes.
    fn confirm_ungroup_shown(&mut self, target: Target, cx: &mut Context<Self>) {
        let Some(root) = target.items.first().map(|item| item.root_id) else {
            return;
        };
        let quick_look = target.quick_look;
        self.ask(
            "Ungroup?",
            "The media will return to the library as separate items. Files and metadata will not be deleted.",
            "Ungroup",
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
        let collections = target
            .items
            .iter()
            .filter(|item| item.kind == RootKind::Collection)
            .count();
        let title = if collections == 1 {
            "Ungroup?".to_owned()
        } else {
            format!("Ungroup {} collections?", grouped(collections as u64))
        };
        let message = if collections == 1 && target.items.len() == 1 {
            "Every member will return to the library as a separate item. Files and metadata will not be deleted."
        } else {
            "Every member of these collections will return to the library as a separate item. Other selected items stay as they are. Files and metadata will not be deleted."
        };
        let selection = target.selection(self.session.read(cx));
        self.ask(
            title,
            message,
            "Ungroup",
            false,
            Rc::new(move |this, _, cx| {
                let selection = selection.clone();
                this.session
                    .update(cx, |session, cx| session.clear_selection(cx));
                edit_then(
                    &this.session,
                    Report::failure("Could not ungroup items").and_announce("collections.ungroup"),
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

/// An item's name as the grid shows it: "Untitled" for none.
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

/// "Copy File Path": the files' paths as text, only if each is a file of
/// the library copy.
fn copy_paths(paths: &[PathBuf], cx: &mut App) {
    if let Err(error) = crate::desktop::copy_paths(paths, cx) {
        crate::ui::notify::error("Could not copy file path", error, cx);
    }
}

/// `resolveTargetFilePaths`: the content hash and path of every file of
/// these items, collection members included, in order.
fn files_of(
    ids: Vec<RootId>,
) -> impl FnOnce(&Library) -> picto_library::Result<Vec<(String, String)>> + Send + 'static {
    move |library| {
        let mut files = Vec::new();
        for id in ids {
            for media in library.details(id)?.media {
                files.push((media.facts.content_hash, media.file_path));
            }
        }
        Ok(files)
    }
}

/// `copyTargetLinks`: a file's link, with the extension its path has.
fn link(hash: &str, path: &str) -> String {
    let extension = std::path::Path::new(path)
        .extension()
        .map_or("bin".into(), |extension| {
            extension.to_string_lossy().into_owned()
        });
    format!("media://localhost/file/{hash}.{extension}")
}
