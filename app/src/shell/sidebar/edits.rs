//! This file holds the changes that the sidebar makes to the library. They
//! create, duplicate, delete, move and rename folders and smart folders, set
//! their icon and colour, sort a folder's items, and save the Quick Access
//! list. The inspector also calls `rename_node` and `set_node_notes` here to
//! save the name and the notes of a folder or smart folder.
use super::{Renaming, Sidebar, folder_id, smart_folder, smart_folder_id, tree};
use crate::i18n::t;
use crate::session::{Change, Report, Reread, Session, edit_then, edit_then_in};
use crate::ui::text_input::{Kind, Renamed, TextInput};
use gpui::*;
use picto_library::model::ContentSortField;
use picto_library::{FolderId, Library, LibraryError, SmartFolderId, SmartFolderInput};

/// Returns the definition of the smart folder `id` as the library holds it.
/// A caller changes one field of it and writes the whole definition back.
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

/// Shows `name` for the node in the list of destinations at once, before
/// the library has saved the name. Returns the name the list showed before,
/// or `None` if the node is not in the list.
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

/// Renames a folder or a smart folder. The sidebar and the title show the
/// new name at once, and then the name is written to the library. If the
/// library refuses the name, the list shows the old name again, and no
/// notification is shown. `then` is called on the view with whether the
/// name was saved.
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

/// Writes the notes of a folder or a smart folder, and keeps the rest of
/// the definition as the library has it now. Empty notes are saved as no
/// notes. `then` is called on the view with whether the notes were saved.
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

/// Returns the definition of a new smart folder group named "Untitled"
/// under `parent`. A smart folder that has no rules is a group.
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
    // The functions below change folders and smart folders in the library.

    /// Creates a top-level folder and starts renaming it in place. The
    /// shortcut `file.newFolder` runs this.
    pub fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_folder(None, window, cx);
    }

    /// Creates a subfolder of the folder that is selected in the sidebar, or
    /// of the open folder when none is selected, and starts renaming it in
    /// place. The shortcut `file.newSubfolder` runs this. Returns false and
    /// creates nothing unless that is exactly one folder.
    pub fn new_subfolder(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.chosen_or_open_folders(cx).as_slice() {
            [folder] => {
                self.create_folder(Some(*folder), window, cx);
                true
            }
            _ => false,
        }
    }

    /// Opens the smart-folder editor for a new top-level smart folder. The
    /// shortcut `file.newSmartFolder` runs this.
    pub fn new_top_smart_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_smart_folder(None, window, cx);
    }

    /// Opens the auto-tags dialog for the folders that are selected in the
    /// sidebar, or for the open folder when none is selected. The shortcut
    /// `folder.autoTags` runs this. Returns false when there is no such
    /// folder.
    pub fn auto_tags_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let targets = self.chosen_or_open_folders(cx);
        if targets.is_empty() {
            return false;
        }
        self.edit_auto_tags(targets, window, cx);
        true
    }

    /// Returns the folders that are selected in the sidebar. When none is
    /// selected it returns the open folder, or nothing when the open
    /// destination is not a folder.
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

    /// Creates a folder named "New Folder" under `parent`, expands the
    /// branch it is in and starts renaming it in place.
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
            // i18n: a new folder's name, English in every language
            move |library| library.create_folder("New Folder", parent),
            window,
            cx,
            |this, result, window, cx| {
                if let Ok((id, _)) = result {
                    // i18n: a new folder's name, English in every language
                    this.rename_new(format!("folder:{}", id.0), "New Folder".into(), window, cx)
                }
            },
        );
    }

    /// Creates a folder named "New Folder" under `parent`, expands the
    /// branch it is in and starts renaming it in place. When `members` is
    /// given, those items are added to the folder. The grid's "New Folder"
    /// and "New Folder with Selection" commands call this.
    pub fn new_folder_in(
        &mut self,
        parent: Option<FolderId>,
        members: Option<picto_library::selection::SelectionTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // "New Folder" shows a notification for any failure. "New Folder
        // with Selection" shows one only for a folder nested too deep.
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
                // i18n: a new folder's name, English in every language
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
                    // After the selected items were added to the new folder,
                    // the folder is recorded as the last folder used and the
                    // grid's selection is cleared.
                    if adds {
                        this.session.update(cx, |session, cx| {
                            session.record_folder_use(id);
                            session.clear_selection(cx);
                        });
                    }
                    // i18n: a new folder's name, English in every language
                    this.rename_new(format!("folder:{}", id.0), "New Folder".into(), window, cx)
                }
            },
        );
    }

    /// Creates a smart folder group named "Untitled" under `parent`, expands
    /// the branch it is in and starts renaming it in place.
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

    /// Duplicates folders and smart folders. When exactly one node is
    /// duplicated, the sidebar starts renaming the duplicate in place, and
    /// the rename field holds "<name> copy".
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

    /// Deletes folders and smart folders. The library removes their
    /// definitions and the record of which items were in them. It deletes
    /// no media file.
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
        // The notification with the Undo button names one history command.
        // Smart folders are deleted after folders, so the command is
        // `smart_folders.delete` when a smart folder is among the nodes.
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

    /// Saves the name typed in the rename field with `rename_node`. Nothing
    /// is saved when the name is empty or is the name the node already had.
    fn apply_rename(&mut self, node: &str, original: &str, name: String, cx: &mut Context<Self>) {
        if !name.is_empty() && name != original {
            rename_node(&self.session, node.to_owned(), name, cx, |_, _, _| {});
        }
    }

    /// Returns the ids of the nodes in Quick Access, in the order in which
    /// the `sidebar_quick_access` setting stores them.
    pub(super) fn quick_access(&self, cx: &App) -> Vec<String> {
        crate::settings::current(cx).sidebar_quick_access.clone()
    }

    /// Saves `ids` as the Quick Access list. An id that occurs more than
    /// once is kept only at its first place. A failure to save is reported
    /// with the title "Unable to update Quick Access".
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

    /// Sets the icon of folders and smart folders. `None` sets the default
    /// icon.
    pub(super) fn apply_icon(
        &mut self,
        nodes: Vec<String>,
        icon: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.edit_appearance(nodes, Some(icon), None, cx);
    }

    /// Sets the colour of folders and smart folders. `None` sets the default
    /// colour.
    pub(super) fn apply_color(
        &mut self,
        nodes: Vec<String>,
        color: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.edit_appearance(nodes, None, Some(color), cx);
    }

    /// Writes an icon, a colour or both to folders and smart folders, and
    /// keeps the rest of each definition. The icon picker can be in a menu's
    /// pop-up window, so this function waits for the library without using a
    /// window.
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

    /// Orders the items of each of `folders` by `field` in the library, then
    /// reads the grid again.
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

    /// Moves smart folders under the smart folder `under`. `None` and
    /// `Some("")` move them to the top level. A node in `nodes` that is
    /// inside another node in `nodes` is not moved by itself, because it
    /// moves with that node.
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

    /// Opens the smart-folder editor with all of its parts (`Part::All`) for
    /// a new smart folder under `parent`. The menu rows "New Smart Folder"
    /// and "New Child Smart Folder" call this.
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

    /// Opens the smart-folder editor for an existing smart folder. The
    /// editor shows the rules when `rules` is true (the menu row "Edit
    /// Rules...") and the details otherwise ("Edit Smart Folder...").
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
        // The editor repaints itself while it fades out. The sidebar is
        // repainted with it, because the sidebar's `render` is where the
        // editor is dropped once it has finished.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        self.smart_editor = Some((editor, subscription));
        cx.notify();
    }

    // The functions below rename a row in place.

    /// Expands the section and the branches that hold a new or duplicated
    /// node, then starts renaming the node in place.
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

    /// Starts renaming a row in place. The row shows a text field that has
    /// keyboard focus and holds `name`.
    pub(super) fn start_rename(
        &mut self,
        node: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // If another row is still being renamed, its name is saved first.
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
                // An event from the field of an earlier rename is ignored.
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
                // After Enter or Escape nothing has keyboard focus, so the
                // focus is given to `home`.
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
