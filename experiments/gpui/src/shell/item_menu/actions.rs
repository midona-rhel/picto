//! What a chosen item-menu row does: the dispatch of each action through
//! the session and the library, and the edits behind them.
use super::{File, Target, media_link};
use crate::grid::format::grouped;
use crate::grid::menu::Action;
use crate::session::{Report, Reread};
use crate::shell::Shell;
use gpui::*;
use picto_library::{ContentSortField, FolderId, Library, Lifecycle, RootKind};
use std::path::PathBuf;
use std::rc::Rc;

type Work<T> = Box<dyn FnOnce(&Library) -> picto_library::Result<T> + Send>;

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
                    self.with_path(file, |path, cx| cx.open_with_system(&path), window, cx);
                }
            }
            Action::Reveal => {
                if let Some(file) = file {
                    self.with_path(file, |path, cx| cx.reveal_path(&path), window, cx);
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
            Action::OpenWithOther => {
                // TODO: list the applications that open this type (macOS
                // shows them as a submenu, Windows opens its chooser) and
                // open the file with the one chosen.
            }
            Action::OpenFolder | Action::LoadingApplications => {
                // TODO: open the folder tile's folder once the grid shows
                // subfolder tiles.
            }
            // ── Copying ──
            Action::CopyPath => {
                if let Some(file) = file {
                    self.with_path(
                        file,
                        |path, cx| write_text(path.to_string_lossy().into_owned(), cx),
                        window,
                        cx,
                    );
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
                            Action::CopySelectionNames if media.media_name.is_empty() => {
                                "Untitled".to_owned()
                            }
                            Action::CopySelectionNames => media.media_name.clone(),
                            Action::CopySelectionPaths => media.file_path.clone(),
                            _ => media_link(&media.facts.content_hash, &media.facts.mime),
                        })
                        .collect()
                };
                if !lines.is_empty() {
                    write_text(lines.join("\n"), cx);
                }
            }
            Action::CopySelectionNames => {
                let names = target
                    .items
                    .iter()
                    .map(|item| {
                        if item.name.is_empty() {
                            "Untitled".to_owned()
                        } else {
                            item.name.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                write_text(names.join("\n"), cx);
            }
            Action::CopySelectionPaths | Action::CopySelectionLinks => {
                // `resolveTargetFilePaths`: every file of the selection,
                // collection members included, in order.
                let links = action == Action::CopySelectionLinks;
                self.library(
                    Box::new(move |library| {
                        let mut lines = Vec::new();
                        for id in ids {
                            for media in library.details(id)?.media {
                                lines.push(if links {
                                    let extension = std::path::Path::new(&media.file_path)
                                        .extension()
                                        .map_or("bin".into(), |extension| {
                                            extension.to_string_lossy().into_owned()
                                        });
                                    format!(
                                        "media://localhost/file/{}.{extension}",
                                        media.facts.content_hash
                                    )
                                } else {
                                    media.file_path
                                });
                            }
                        }
                        Ok(lines)
                    }),
                    |_, lines, _, cx| {
                        if !lines.is_empty() {
                            write_text(lines.join("\n"), cx);
                        }
                    },
                    window,
                    cx,
                );
            }
            Action::CopyFile | Action::CopySelection => {
                // TODO: put the files themselves on the clipboard. GPUI's
                // macOS pasteboard writes text and images only; a file list
                // (`ClipboardEntry::ExternalPaths`) is dropped.
            }
            // ── Lifecycle ──
            Action::Accept | Action::Restore => self.set_lifecycle(Lifecycle::Active, cx),
            // A member leaves its collection into the Trash
            // (`detachMembers(selected, 'trash')`).
            Action::MoveToTrash if target.member => self.detach_members(target, true, window, cx),
            Action::RemoveFromGroup => self.detach_members(target, false, window, cx),
            Action::MoveToTrash => self.set_lifecycle(Lifecycle::Trash, cx),
            Action::Reject => {
                if crate::settings::current(cx).reject_action == "delete" {
                    self.request_permanent_delete(target, true, window, cx);
                } else {
                    self.set_lifecycle(Lifecycle::Trash, cx);
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
                    self.edit_as(
                        Report::failure(format!("Could not add to {name}")),
                        Box::new(move |library| library.add_to_folder(&selection, folder)),
                        Reread::SELECTION,
                        cx,
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
                    self.edit(
                        Box::new(move |library| library.remove_from_folder(&selection, folder)),
                        Reread::RESULTS,
                        cx,
                    );
                }
            }
            Action::SetFolderCover => {
                if let (Some(folder), Some(id)) = (target.folder, ids.first().copied()) {
                    self.edit_as(
                        Report::failure("Could not set folder cover"),
                        Box::new(move |library| library.set_folder_cover(folder, id)),
                        Reread::SELECTION,
                        cx,
                    );
                }
            }
            Action::SortContents(field) if target.group => {
                if target.quick_look {
                    let details = self.session.read(cx).selected_details().cloned();
                    if let Some(details) = details {
                        drop(crate::viewer::group::sort(
                            &self.session,
                            &details,
                            field,
                            cx,
                        ));
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
            Action::Group => self.group(target, window, cx),
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
            Action::BatchRename => {
                // TODO: open the batch rename dialog for these items.
            }
            Action::AddTags => {
                self.dispatch(Box::new(crate::keymap::AddTags), window, cx);
                return;
            }
            Action::AutoTag => {
                // TODO: open the AI tagger on the selection, beside the
                // inspector.
            }
            Action::CopyTags => self.copy_tags(target, window, cx),
            Action::PasteTags => {
                let tags = self.items.tag_clipboard.clone();
                if !tags.is_empty() && !ids.is_empty() {
                    let selection = target.selection(self.session.read(cx));
                    self.edit(
                        Box::new(move |library| library.apply_tags(&selection, &tags, true)),
                        Reread::SELECTION,
                        cx,
                    );
                }
            }
            Action::Rate(stars) => {
                let rating = picto_library::Rating::ALL[usize::from(stars).min(5)];
                let selection = target.selection(self.session.read(cx));
                self.edit(
                    Box::new(move |library| library.set_rating(&selection, rating)),
                    Reread::SELECTION,
                    cx,
                );
            }
            // ── Waiting on other parts of the app ──
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
            Action::SearchByImage(_) => {
                // TODO: production uploads the original to the engine from a
                // hidden browser window (`runReverseImageSearch`) and shows
                // the results page; this needs that upload.
            }
            Action::SetLibraryCover => {
                // TODO: open the library cover dialog (`MediaCoverDialog`)
                // with this item chosen, where the square is framed before
                // the cover is written beside the library's database.
            }
            Action::FindMediaMatches => {
                // TODO: open the media-matches destination
                // (`ItemScope::MediaMatches`) for this item.
            }
            Action::RegenerateThumbnails => {
                // TODO: regenerate the thumbnails of these files.
            }
            Action::Grayscale => {
                // TODO: draw the grid's thumbnails in grayscale and check
                // the row while it is on.
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
                // TODO: capture the shown frame and make it the thumbnail.
            }
        }
        if self.items.surface.is_none() {
            window.focus(&self.focus, cx);
        }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let File { root, hash, .. } = file;
        self.library(
            Box::new(move |library| {
                Ok(library
                    .details(root)?
                    .media
                    .into_iter()
                    .find(|media| media.facts.content_hash == hash)
                    .map(|media| PathBuf::from(media.file_path)))
            }),
            |_, path, _, cx| {
                if let Some(path) = path {
                    then(path, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Runs `work` on the library's thread and hands its answer to `then`.
    /// A failure is reported as the session's edits report one.
    pub(in crate::shell) fn library<T: Send + 'static>(
        &mut self,
        work: Work<T>,
        then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library_as(Report::default(), work, then, window, cx);
    }

    /// [`Shell::library`], telling the user what `report` says.
    pub(in crate::shell) fn library_as<T: Send + 'static>(
        &mut self,
        report: Report,
        work: Work<T>,
        then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = self.session.update(cx, |session, cx| {
            session.edit_reporting(report, work, Reread::NOTHING, cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(value)) = answer.await {
                let _ = this.update_in(cx, move |this, window, cx| then(this, value, window, cx));
            }
        })
        .detach();
    }

    /// An edit of the selected items, and what to read again afterwards.
    fn edit<T: Send + 'static>(&mut self, work: Work<T>, reread: Reread, cx: &mut Context<Self>) {
        self.edit_as(Report::default(), work, reread, cx);
    }

    /// [`Shell::edit`], telling the user what `report` says, as the
    /// production menu row does.
    fn edit_as<T: Send + 'static>(
        &mut self,
        report: Report,
        work: Work<T>,
        reread: Reread,
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |session, cx| {
            let _ = session.edit_reporting(report, work, reread, cx);
        });
    }

    /// Moves the selected items into or out of the Trash or the Inbox
    /// through the session, which reads the grid again in place.
    fn set_lifecycle(&mut self, lifecycle: Lifecycle, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.move_selection_to(lifecycle, cx));
    }

    fn sort_folder(&mut self, folder: FolderId, field: ContentSortField, cx: &mut Context<Self>) {
        self.edit(
            Box::new(move |library| library.sort_folder_items(folder, field)),
            Reread::RESULTS,
            cx,
        );
    }

    /// `onCopyTags`: one item's tags, or the tags every selected item
    /// shares, as JSON on the clipboard and kept for "Paste Tags".
    fn copy_tags(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let single = target.items.len() == 1;
        let ids = target.ids();
        let selection = target.selection(self.session.read(cx));
        self.library(
            Box::new(move |library| {
                let tag_ids = if single {
                    library.details(ids[0])?.tag_ids
                } else {
                    library.selection_summary(&selection)?.shared_tags
                };
                Ok(library
                    .tags_by_id(&tag_ids)?
                    .iter()
                    .map(crate::tags::record_name)
                    .collect::<Vec<_>>())
            }),
            |this, tags, _, cx| {
                write_text(serde_json::to_string(&tags).unwrap_or_default(), cx);
                this.items.tag_clipboard = tags;
            },
            window,
            cx,
        );
    }

    /// `organizeSelection`: one selected group and no notes to merge groups
    /// at once; anything else needs the group organizer.
    fn group(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cover) = target.items.first().map(|item| item.root_id) else {
            return;
        };
        let selection = target.selection(self.session.read(cx));
        self.library_as(
            Report::failure("Could not create group").and_announce("collections.organize"),
            Box::new(move |library| {
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
            }),
            |this, decide, _, cx| match decide {
                None => {
                    // `clearSelection`, then the merged group's results.
                    // TODO: select the merged group once the results hold
                    // it, as production does.
                    this.session.update(cx, |session, cx| {
                        session.clear_selection(cx);
                        session.reread(Reread::RESULTS, cx)
                    });
                }
                Some(request) => this.open_organizer(request, cx),
            },
            window,
            cx,
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
        let answer =
            crate::viewer::group::detach(&self.session, details.root.root_id, ids, trash, cx);
        let quick_look = self.quick_look.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(())) = answer.await
                && closes
            {
                let _ =
                    quick_look.update_in(cx, |quick_look, window, cx| quick_look.close(window, cx));
            }
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
                let answer = this.session.update(cx, |session, cx| {
                    session.edit_reporting(
                        Report::announce("collections.ungroup"),
                        move |library| library.ungroup_collection(root, crate::now_ms()),
                        Reread::RESULTS,
                        cx,
                    )
                });
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(_)) = answer.await {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if quick_look {
                                this.quick_look
                                    .update(cx, |quick_look, cx| quick_look.close(window, cx));
                            } else {
                                this.session
                                    .update(cx, |session, cx| session.close_viewer(cx));
                            }
                        });
                    }
                })
                .detach();
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
                this.edit_as(
                    Report::failure("Could not ungroup items").and_announce("collections.ungroup"),
                    Box::new(move |library| library.ungroup_selection(&selection, crate::now_ms())),
                    Reread::RESULTS,
                    cx,
                );
            }),
            cx,
        );
    }
}

fn write_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}
