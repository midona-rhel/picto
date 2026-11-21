//! The sidebar's edits through the library: creating, duplicating,
//! deleting, moving and renaming folders and smart folders, their
//! appearance, sorting and Quick Access. A folder's or smart folder's name
//! and notes are written here for the inspector too.
use super::{Renaming, Sidebar, folder_id, smart_folder, smart_folder_id, tree};
use crate::i18n::t;
use crate::session::{Change, Report, Reread, Session, edit_then, edit_then_in};
use crate::ui::text_input::{Kind, Renamed, TextInput};
use gpui::*;
use picto_library::model::ContentSortField;
use picto_library::{FolderId, Library, LibraryError, SmartFolderId, SmartFolderInput};

/// A smart folder's definition, to write back with one thing changed.
pub(super) fn smart_input(
    library: &Library,
    id: SmartFolderId,
) -> picto_library::Result<SmartFolderInput> {
    let record = library
        .smart_folders()?
        .into_iter()
        .find(|record| record.smart_folder_id == id)
        .ok_or_else(|| LibraryError::NotFound(format!("smart folder {id}")))?;
    Ok(SmartFolderInput {
        name: record.name,
        parent_id: record.parent_id,
        icon: record.icon,
        color: record.color,
        notes: record.notes,
        view: record.view,
    })
}

/// The destination list shows `name` for the node at once; the name it
/// showed before, if the node is listed.
fn relabel(session: &Entity<Session>, node: &str, name: &str, cx: &mut App) -> Option<String> {
    session.update(cx, |session, cx| {
        let entry = session.nav.iter_mut().find(|entry| entry.id() == node)?;
        let previous = std::mem::replace(&mut entry.label, name.to_owned());
        if previous != name {
            cx.emit(Change::Navigation);
        }
        Some(previous)
    })
}

/// `foldersController.rename`, and `smartFoldersController.update` with a
/// new name: the sidebar and the title show the name at once, the
/// definition is written, and a name the library refuses puts the old one
/// back. Production leaves the failure to the console. `then` is told on
/// the view whether it was saved.
pub(crate) fn rename_node<V: 'static>(
    session: &Entity<Session>,
    node: String,
    name: String,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, bool, &mut Context<V>) + 'static,
) {
    let previous = relabel(session, &node, &name, cx);
    let (folder, smart, written) = (folder_id(&node), smart_folder_id(&node), name.clone());
    let restore = session.clone();
    edit_then(
        session,
        Report::default(),
        Reread::NOTHING,
        move |library| -> picto_library::Result<()> {
            if let Some(id) = folder {
                library.rename_folder(id, &written)?;
            } else if let Some(id) = smart {
                let mut input = smart_input(library, id)?;
                input.name = written;
                library.update_smart_folder(id, input)?;
            }
            Ok(())
        },
        cx,
        move |view, result, cx| {
            let shown = || {
                restore
                    .read(cx)
                    .nav
                    .iter()
                    .any(|entry| entry.id() == node && entry.label == name)
            };
            if result.is_err()
                && let Some(previous) = previous.filter(|_| shown())
            {
                relabel(&restore, &node, &previous, cx);
            }
            then(view, result.is_ok(), cx)
        },
    );
}

/// `foldersController.applyNotes`, and `smartFoldersController.update` with
/// new notes, keeping the rest of the definition as the library has it now.
/// `then` is told on the view whether they were saved.
pub(crate) fn set_node_notes<V: 'static>(
    session: &Entity<Session>,
    node: &str,
    notes: String,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, bool, &mut Context<V>) + 'static,
) {
    let (folder, smart) = (folder_id(node), smart_folder_id(node));
    let notes = (!notes.is_empty()).then_some(notes);
    edit_then(
        session,
        Report::default(),
        Reread::NOTHING,
        move |library| -> picto_library::Result<()> {
            if let Some(id) = folder {
                let record = library
                    .folders()?
                    .into_iter()
                    .find(|record| record.folder_id == id)
                    .ok_or_else(|| LibraryError::NotFound(format!("folder {id}")))?;
                library.set_folder_metadata(
                    id,
                    record.icon.as_deref(),
                    record.color.as_deref(),
                    notes.as_deref(),
                )?;
            } else if let Some(id) = smart {
                let mut input = smart_input(library, id)?;
                input.notes = notes;
                library.update_smart_folder(id, input)?;
            }
            Ok(())
        },
        cx,
        move |view, result, cx| then(view, result.is_ok(), cx),
    );
}

/// `emptySmartFolderPayload`: a smart folder with no rules is a group.
fn new_group(parent_id: Option<SmartFolderId>) -> SmartFolderInput {
    SmartFolderInput {
        name: "Untitled".into(),
        parent_id,
        icon: None,
        color: None,
        notes: None,
        view: Default::default(),
    }
}

pub(super) fn sorted_by_name(mut input: SmartFolderInput) -> SmartFolderInput {
    input.view.sort = picto_library::predicate::ItemSort {
        field: picto_library::predicate::SortField::Name,
        direction: picto_library::predicate::SortDirection::Ascending,
        random_seed: None,
    };
    input
}

impl Sidebar {
    // ── Edits ───────────────────────────────────────────────────────

    /// `file.newFolder`: a top-level folder, renamed in place.
    pub fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_folder(None, window, cx);
    }

    /// `file.newSubfolder`: a subfolder of the folder chosen in the sidebar,
    /// or of the open folder when none is chosen. Returns false, creating
    /// nothing, unless that is exactly one folder.
    pub fn new_subfolder(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.chosen_or_open_folders(cx).as_slice() {
            [folder] => {
                self.create_folder(Some(*folder), window, cx);
                true
            }
            _ => false,
        }
    }

    /// `file.newSmartFolder`: the smart-folder editor for a new top-level
    /// smart folder.
    pub fn new_top_smart_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_smart_folder(None, window, cx);
    }

    /// `folder.autoTags`: the auto-tags editor for the folders chosen in the
    /// sidebar, or the open folder. Returns false when there is none.
    pub fn auto_tags_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let targets = self.chosen_or_open_folders(cx);
        if targets.is_empty() {
            return false;
        }
        self.edit_auto_tags(targets, window, cx);
        true
    }

    /// The folders chosen in the sidebar, or else the open folder.
    fn chosen_or_open_folders(&self, cx: &App) -> Vec<FolderId> {
        let chosen = self.selection.iter().filter_map(|node| folder_id(node));
        let mut folders = chosen.collect::<Vec<_>>();
        if folders.is_empty() {
            let session = self.session.read(cx);
            let open = session.nav.get(session.scope);
            folders.extend(open.and_then(|entry| folder_id(&entry.id())));
        }
        folders
    }

    /// `createFolderAndRename`: a folder named "New Folder", revealed and
    /// renamed in place.
    pub(super) fn create_folder(
        &mut self,
        parent: Option<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        edit_then_in(
            &self.session,
            Report::folder_depth(),
            Reread::NAVIGATION,
            // i18n: production names a new folder in English
            move |library| library.create_folder("New Folder", parent),
            window,
            cx,
            |this, result, window, cx| {
                if let Ok((id, _)) = result {
                    // i18n: production names a new folder in English
                    this.rename_new(format!("folder:{}", id.0), "New Folder".into(), window, cx)
                }
            },
        );
    }

    /// A folder named "New Folder" under `parent`, revealed and renamed in
    /// place, holding `members` when given (the grid's "New Folder" and "New
    /// Folder with Selection").
    pub fn new_folder_in(
        &mut self,
        parent: Option<FolderId>,
        members: Option<picto_library::selection::SelectionTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The grid's "New Folder" reports any failure; "New Folder with
        // Selection" only a folder nested too deep.
        let adds = members.is_some();
        let report = if adds {
            Report::folder_depth()
        } else {
            Report::failure(t("Could not create folder"))
        };
        edit_then_in(
            &self.session,
            report,
            Reread::NAVIGATION,
            move |library| -> picto_library::Result<_> {
                // i18n: production names a new folder in English
                let (id, _) = library.create_folder("New Folder", parent)?;
                if let Some(members) = &members {
                    library.add_to_folder(members, id)?;
                }
                Ok(id)
            },
            window,
            cx,
            move |this, result, window, cx| {
                if let Ok(id) = result {
                    // Adding the selection to the new folder makes it the
                    // last folder used and lets the selection go
                    // (`updateTargetFolderMembership`,
                    // `settleSelectionAfterMutation`).
                    if adds {
                        this.session.update(cx, |session, cx| {
                            session.record_folder_use(id);
                            session.clear_selection(cx);
                        });
                    }
                    // i18n: production names a new folder in English
                    this.rename_new(format!("folder:{}", id.0), "New Folder".into(), window, cx)
                }
            },
        );
    }

    /// `createSmartFolderGroupAndRename`.
    pub(super) fn create_group(
        &mut self,
        parent: Option<SmartFolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        edit_then_in(
            &self.session,
            Report::default(),
            Reread::NAVIGATION,
            move |library| library.create_smart_folder(new_group(parent)),
            window,
            cx,
            |this, result, window, cx| {
                if let Ok((id, _)) = result {
                    this.rename_new(format!("smart:{}", id.0), "Untitled".into(), window, cx)
                }
            },
        );
    }

    /// Duplicates folders and smart folders. One duplicate is renamed in
    /// place under the name the Electron sidebar offers, "<name> copy".
    pub(super) fn duplicate(
        &mut self,
        nodes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names = nodes
            .iter()
            .map(|node| self.label_of(node, cx).unwrap_or_default())
            .collect::<Vec<_>>();
        let single = nodes.len() == 1;
        edit_then_in(
            &self.session,
            Report::folder_depth(),
            Reread::NAVIGATION,
            move |library| -> picto_library::Result<_> {
                let mut made = Vec::new();
                for (node, name) in nodes.iter().zip(&names) {
                    if let Some(id) = folder_id(node) {
                        let (copy, _) = library.duplicate_folder(id)?;
                        made.push((format!("folder:{}", copy.0), format!("{name} copy")));
                    } else if let Some(id) = smart_folder_id(node) {
                        let mut input = smart_input(library, id)?;
                        input.name = format!("{name} copy");
                        let (copy, _) = library.create_smart_folder(input)?;
                        made.push((format!("smart:{}", copy.0), format!("{name} copy")));
                    }
                }
                Ok(made)
            },
            window,
            cx,
            move |this, result, window, cx| {
                if let Ok(made) = result {
                    if let (true, Some((node, name))) = (single, made.into_iter().next()) {
                        this.rename_new(node, name, window, cx);
                    }
                }
            },
        );
    }

    /// Deletes folder and smart-folder definitions. The library removes the
    /// definitions and their memberships; no media file is touched.
    pub(super) fn delete(
        &mut self,
        nodes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders = nodes
            .iter()
            .filter_map(|node| folder_id(node))
            .collect::<Vec<_>>();
        let smart = nodes
            .iter()
            .filter_map(|node| smart_folder_id(node))
            .collect::<Vec<_>>();
        let bulk = nodes.len() > 1;
        // `deleteMany` and `smartFoldersController.delete` each announce
        // what they deleted; the smart folders are deleted last.
        let announce = if smart.is_empty() {
            "folders.delete"
        } else {
            "smart_folders.delete"
        };
        edit_then_in(
            &self.session,
            Report::announce(announce),
            Reread::NOTHING,
            move |library| -> picto_library::Result<_> {
                if !folders.is_empty() {
                    library.delete_folders(&folders)?;
                }
                for id in smart {
                    library.delete_smart_folder(id)?;
                }
                Ok(())
            },
            window,
            cx,
            move |this, result, _, cx| {
                if result.is_ok() {
                    if bulk {
                        this.selection.clear();
                    }
                    cx.notify();
                }
            },
        );
    }

    /// Writes a renamed folder or smart folder (`rename_node`).
    fn apply_rename(&mut self, node: &str, original: &str, name: String, cx: &mut Context<Self>) {
        if !name.is_empty() && name != original {
            rename_node(&self.session, node.to_owned(), name, cx, |_, _, _| {});
        }
    }

    /// The Quick Access node ids, in their order (`sidebarQuickAccess`).
    pub(super) fn quick_access(&self, cx: &App) -> Vec<String> {
        crate::settings::current(cx).sidebar_quick_access.clone()
    }

    /// Saves Quick Access as `reorderQuickAccess` does: each id once.
    pub(super) fn set_quick_access(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let mut unique = Vec::new();
        for id in ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        crate::settings::model(cx).update(cx, |model, cx| {
            model.apply_now_reporting(
                |app| app.sidebar_quick_access = unique.clone(),
                Some(t("Unable to update Quick Access")),
                cx,
            )
        });
        cx.notify();
    }

    /// Gives folders and smart folders an icon (`None` for the default).
    pub(super) fn apply_icon(
        &mut self,
        nodes: Vec<String>,
        icon: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.edit_appearance(nodes, Some(icon), None, cx);
    }

    /// Gives folders and smart folders a colour (`None` for the default).
    pub(super) fn apply_color(
        &mut self,
        nodes: Vec<String>,
        color: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.edit_appearance(nodes, None, Some(color), cx);
    }

    /// Writes an icon, a colour or both, keeping what else the definition
    /// holds. The icon picker may be in a menu's pop-up window, so this
    /// waits for the library without holding any window.
    fn edit_appearance(
        &mut self,
        nodes: Vec<String>,
        icon: Option<Option<String>>,
        color: Option<Option<String>>,
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |session, cx| {
            let _ = session.edit(
                move |library| -> picto_library::Result<()> {
                    let folders = library.folders()?;
                    for node in &nodes {
                        if let Some(id) = folder_id(node) {
                            let Some(folder) = folders.iter().find(|folder| folder.folder_id == id)
                            else {
                                continue;
                            };
                            let icon = icon.clone().unwrap_or(folder.icon.clone());
                            let color = color.clone().unwrap_or(folder.color.clone());
                            library.set_folder_metadata(
                                id,
                                icon.as_deref(),
                                color.as_deref(),
                                folder.notes.as_deref(),
                            )?;
                        } else if let Some(id) = smart_folder_id(node) {
                            let mut input = smart_input(library, id)?;
                            if let Some(icon) = &icon {
                                input.icon = icon.clone();
                            }
                            if let Some(color) = &color {
                                input.color = color.clone();
                            }
                            library.update_smart_folder(id, input)?;
                        }
                    }
                    Ok(())
                },
                Reread::NOTHING,
                cx,
            );
        });
    }

    /// `foldersController.sortContents`: orders each folder's items by a
    /// field.
    pub(super) fn sort_contents(
        &mut self,
        folders: Vec<FolderId>,
        field: ContentSortField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        edit_then_in(
            &self.session,
            Report::default(),
            Reread::NOTHING,
            move |library| -> picto_library::Result<_> {
                for id in folders {
                    library.sort_folder_items(id, field)?;
                }
                Ok(())
            },
            window,
            cx,
            |this, result, _, cx| {
                if result.is_ok() {
                    this.reload_if_showing(None, cx)
                }
            },
        );
    }

    /// `smartFoldersController.moveMany`: moves smart folders under another
    /// (`Some("")` or `None` for the top level), the topmost of a
    /// selection only.
    pub(super) fn move_smart(
        &mut self,
        nodes: Vec<String>,
        under: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moving = {
            let tree = tree::nodes(&self.session.read(cx).nav, "smart:");
            tree::top_level(&tree, &nodes)
                .iter()
                .filter_map(|node| smart_folder_id(node))
                .collect::<Vec<_>>()
        };
        let parent = under.as_deref().and_then(smart_folder_id);
        edit_then_in(
            &self.session,
            Report::default(),
            Reread::NOTHING,
            move |library| -> picto_library::Result<_> {
                for id in moving {
                    let mut input = smart_input(library, id)?;
                    input.parent_id = parent;
                    library.update_smart_folder(id, input)?;
                }
                Ok(())
            },
            window,
            cx,
            |_, _, _, _| {},
        );
    }

    /// "New Smart Folder" and "New Child Smart Folder": the editor with
    /// every part, for a folder under `parent`.
    pub(super) fn new_smart_folder(
        &mut self,
        parent: Option<SmartFolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_smart_editor(
            smart_folder::Target::Create { parent },
            smart_folder::Part::All,
            window,
            cx,
        );
    }

    /// "Edit Smart Folder..." (its details) and "Edit Rules...".
    pub(super) fn edit_smart_folder(
        &mut self,
        node: &str,
        rules: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = smart_folder_id(node) else {
            return;
        };
        let answer = self
            .session
            .read(cx)
            .run(move |library| smart_input(library, id));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(original)) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_smart_editor(
                    smart_folder::Target::Edit { id, original },
                    if rules {
                        smart_folder::Part::Rules
                    } else {
                        smart_folder::Part::Details
                    },
                    window,
                    cx,
                )
            });
        })
        .detach();
    }

    fn open_smart_editor(
        &mut self,
        target: smart_folder::Target,
        part: smart_folder::Part,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.clone();
        let editor =
            cx.new(|cx| smart_folder::SmartFolderEditor::new(session, target, part, window, cx));
        let subscription = cx.subscribe(&editor, |this, _, event: &smart_folder::Event, cx| {
            let smart_folder::Event::Created(id) = event;
            this.reveal(&format!("smart:{}", id.0), cx);
            cx.notify();
        });
        // The editor repaints itself while it fades out; the sidebar has to
        // repaint too, to drop it when it is done.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        self.smart_editor = Some((editor, subscription));
        cx.notify();
    }

    // ── Rename in place ─────────────────────────────────────────────

    /// A new or duplicated node: open its section and branch, then rename it.
    fn rename_new(
        &mut self,
        node: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reveal(&node, cx);
        self.start_rename(node, name, window, cx);
    }

    /// `useInlineRename.startRename`: the name selected in a focused field.
    pub(super) fn start_rename(
        &mut self,
        node: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A rename already open is committed first.
        if let Some(open) = self.renaming.take() {
            let value = open.input.read(cx).value().trim().to_owned();
            self.apply_rename(&open.node, &open.original, value, cx);
        }
        let input =
            cx.new(|cx| TextInput::new(Kind::SidebarRename, cx).renaming(&name, window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &Renamed, window, cx| {
                // Only the field of the rename still open counts.
                if this
                    .renaming
                    .as_ref()
                    .is_none_or(|open| open.input.entity_id() != input.entity_id())
                {
                    return;
                }
                let Some(open) = this.renaming.take() else {
                    return;
                };
                if let Renamed::Commit(value) = event {
                    this.apply_rename(&open.node, &open.original, value.clone(), cx);
                }
                // Enter and Escape leave nothing focused; the keyboard goes back
                // to the window.
                if window.focused(cx).is_none() {
                    window.focus(&this.home, cx);
                }
                cx.notify();
            },
        );
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            node,
            original: name,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }
}
