//! This file carries out the row that the user chose in a sidebar context
//! menu. What a row does depends on what the menu was opened on: a system
//! row, a section header, several selected rows, one folder or smart folder,
//! or a subfolder tile in the grid.
use super::edits::{smart_input, sorted_by_name};
use super::menus::content_sort_field;
use super::{
    EmptyTrash, Section, Sidebar, Target, dialogs, expandable, folder_id, smart_folder_id,
};
use crate::backend::Navigation;
use crate::i18n::t;
use crate::session::{Report, Reread, edit_then_in};
use gpui::*;
use picto_library::query::{ItemScope, RootQuery};

impl Sidebar {
    /// Carries out the menu row `action` on `target`, which is the thing the
    /// menu was opened on. Some rows share one action and differ only in
    /// their value, such as the rows of a sort submenu and the colour
    /// swatches. `value` is the value of the chosen row or swatch.
    pub(super) fn choose(
        &mut self,
        target: &Target,
        action: &'static str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A row of the visibility menu shows or hides a system row or a
        // section. The change is saved in the app settings at once. The
        // menu stays open, and the check mark of the chosen row is updated.
        if crate::settings::current(cx)
            .clone()
            .sidebar_visibility(action)
            .is_some()
        {
            crate::settings::model(cx).update(cx, |model, cx| {
                model.apply_now(
                    |app| {
                        if let Some(on) = app.sidebar_visibility(action) {
                            *on = !*on;
                        }
                    },
                    cx,
                )
            });
            let shown = crate::settings::current(cx)
                .clone()
                .sidebar_visibility(action)
                .map(|on| *on);
            if let (Some(shown), Some(menu)) = (shown, self.menu.as_ref().and_then(|m| m.upgrade()))
            {
                menu.update(cx, |menu, cx| menu.set_checked(action, shown, cx));
            }
            return;
        }
        match target {
            Target::System => match action {
                "clear-recent" => edit_then_in(
                    &self.session,
                    Report::default(),
                    Reread::RESULTS_ONLY,
                    |library| library.clear_recent_views().map(|_| ()),
                    window,
                    cx,
                    |_, _, _, _| {},
                ),
                "restore-all" => edit_then_in(
                    &self.session,
                    Report::default(),
                    Reread::RESULTS_ONLY,
                    |library| {
                        library
                            .set_lifecycle(
                                &picto_library::selection::SelectionTarget::Query {
                                    query: RootQuery {
                                        scope: ItemScope::Trash,
                                        view: Default::default(),
                                    },
                                    excluded_root_ids: Vec::new(),
                                },
                                picto_library::Lifecycle::Active,
                            )
                            .map(|_| ())
                    },
                    window,
                    cx,
                    |_, _, _, _| {},
                ),
                // The shell asks the user to confirm and deletes the items,
                // as it does for "Delete Permanently" in the grid.
                "empty-trash" => cx.emit(EmptyTrash),
                _ => {}
            },
            Target::Section => match action {
                "new-folder" => self.create_folder(None, window, cx),
                "new-group" => self.create_group(None, window, cx),
                "new-smart-folder" => self.new_smart_folder(None, window, cx),
                // "Import Folder..." in the menu of the Folders section
                // imports into the library's top level.
                "import-folder" => import_folder(None, window, cx),
                _ => {}
            },
            Target::Node(node) => self.choose_for_node(node, action, value, window, cx),
            Target::Bulk(nodes) | Target::Tiles(nodes) => {
                self.choose_for_selection(nodes, action, value, window, cx)
            }
            Target::Tile(node) => self.choose_for_tile(node, action, value, window, cx),
        }
    }

    fn choose_for_selection(
        &mut self,
        nodes: &[String],
        action: &str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders = nodes
            .iter()
            .filter_map(|node| folder_id(node))
            .collect::<Vec<_>>();
        match action {
            "duplicate" => self.duplicate(nodes.to_vec(), window, cx),
            "delete" => self.confirm_delete(nodes.to_vec(), cx),
            "quick-access-add" => {
                let mut next = self.quick_access(cx);
                next.extend(
                    nodes
                        .iter()
                        .filter(|id| !next.contains(id))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                self.set_quick_access(next, cx);
            }
            "quick-access-remove" => {
                let next = self
                    .quick_access(cx)
                    .into_iter()
                    .filter(|id| !nodes.contains(id))
                    .collect();
                self.set_quick_access(next, cx);
            }
            "expand-selected" | "collapse-selected" => {
                let parents = {
                    let nav = &self.session.read(cx).nav;
                    nodes
                        .iter()
                        .filter(|node| {
                            nav.iter()
                                .position(|entry| entry.id() == **node)
                                .is_some_and(|index| {
                                    nav.iter().any(|entry| entry.parent == Some(index))
                                })
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                };
                self.set_expanded(&parents, action == "expand-selected", cx);
            }
            "sort-by-name" => {
                let ids = nodes
                    .iter()
                    .filter_map(|node| smart_folder_id(node))
                    .collect::<Vec<_>>();
                edit_then_in(
                    &self.session,
                    Report::default(),
                    Reread::NOTHING,
                    move |library| -> picto_library::Result<_> {
                        for id in ids {
                            library.update_smart_folder(
                                id,
                                sorted_by_name(smart_input(library, id)?),
                            )?;
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
            "sort-contents" => {
                if let Some(field) = value.as_deref().and_then(content_sort_field) {
                    self.sort_contents(folders, field, window, cx);
                }
            }
            "color" => self.apply_color(nodes.to_vec(), value.filter(|hex| !hex.is_empty()), cx),
            "move-under" => self.move_smart(nodes.to_vec(), value, window, cx),
            "move-to" => self.pick_folder_parent(nodes.to_vec(), window, cx),
            "auto-tags" => self.edit_auto_tags(folders, window, cx),
            _ => {}
        }
    }

    /// Carries out a row of the menu of a subfolder tile. "Open Folder"
    /// opens the folder, and "Rename" renames the tile in place in the grid.
    /// "New Subfolder" and "Duplicate" create a folder, and the grid renames
    /// its tile in place once the tile is shown. Every other row does what
    /// it does in the menu of the folder's row in the sidebar.
    fn choose_for_tile(
        &mut self,
        node: &str,
        action: &str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let grid = self.tile_grid.clone();
        let rename_later = move |made: String, cx: &mut App| {
            if let Some(grid) = grid.as_ref().and_then(WeakEntity::upgrade) {
                grid.update(cx, |grid, cx| grid.rename_subfolder_when_shown(made, cx));
            }
        };
        match action {
            "open" => {
                let index = {
                    let nav = &self.session.read(cx).nav;
                    nav.iter().position(|entry| entry.id() == node)
                };
                if let Some(index) = index {
                    self.session
                        .update(cx, |session, cx| session.navigate(index, cx));
                }
            }
            "rename" => {
                let label = self.label_of(node, cx).unwrap_or_default();
                if let Some(grid) = self.tile_grid.as_ref().and_then(WeakEntity::upgrade) {
                    let node = node.to_owned();
                    grid.update(cx, |grid, cx| {
                        grid.rename_subfolder(node, label, window, cx)
                    });
                }
            }
            // Creates a folder named "New Folder" inside this folder. The
            // grid renames its tile in place once the tile is shown.
            "new-subfolder" => {
                let parent = folder_id(node);
                edit_then_in(
                    &self.session,
                    Report::folder_depth(),
                    Reread::NAVIGATION,
                    // i18n: a new folder's name, English in every language
                    move |library| library.create_folder("New Folder", parent),
                    window,
                    cx,
                    move |_, result, _, cx| {
                        if let Ok((id, _)) = result {
                            rename_later(format!("folder:{}", id.0), cx)
                        }
                    },
                );
            }
            "duplicate" => {
                let Some(id) = folder_id(node) else {
                    return;
                };
                edit_then_in(
                    &self.session,
                    Report::folder_depth(),
                    Reread::NAVIGATION,
                    move |library| library.duplicate_folder(id),
                    window,
                    cx,
                    move |_, result, _, cx| {
                        if let Ok((copy, _)) = result {
                            rename_later(format!("folder:{}", copy.0), cx)
                        }
                    },
                );
            }
            _ => self.choose_for_node(node, action, value, window, cx),
        }
    }

    fn choose_for_node(
        &mut self,
        node: &str,
        action: &str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.session.read(cx).nav.clone();
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        let Some(index) = ids.iter().position(|id| id == node) else {
            return;
        };
        let Some(section) = Section::of_node(node) else {
            return;
        };
        match action {
            "new-folder" => self.create_folder(None, window, cx),
            "new-subfolder" => self.create_folder(folder_id(node), window, cx),
            "new-child-group" => self.create_group(smart_folder_id(node), window, cx),
            "new-child-smart-folder" => self.new_smart_folder(smart_folder_id(node), window, cx),
            "edit-smart-folder" => self.edit_smart_folder(node, false, window, cx),
            "edit-rules" => self.edit_smart_folder(node, true, window, cx),
            "rename" => {
                self.start_rename(node.to_owned(), entries[index].label.clone(), window, cx)
            }
            "duplicate" => self.duplicate(vec![node.to_owned()], window, cx),
            "delete" => self.confirm_delete(vec![node.to_owned()], cx),
            "quick-access" => {
                let mut next = self.quick_access(cx);
                if let Some(at) = next.iter().position(|id| id == node) {
                    next.remove(at);
                } else {
                    next.push(node.to_owned());
                }
                self.set_quick_access(next, cx);
            }
            "toggle-expand" => {
                if entries.iter().any(|entry| entry.parent == Some(index)) {
                    self.toggle_collapsed(node, cx);
                }
            }
            "toggle-expand-all" => {
                let all = expandable(&entries, &ids, section);
                let any_expanded = all.iter().any(|id| !self.collapsed.contains(id));
                self.set_expanded(&all, !any_expanded, cx);
            }
            "refresh" => {
                let node = node.to_owned();
                edit_then_in(
                    &self.session,
                    Report::failure(t("Could not refresh smart folder")),
                    Reread::NOTHING,
                    |_| Ok::<_, picto_library::LibraryError>(()),
                    window,
                    cx,
                    move |this, result, _, cx| {
                        if result.is_ok() {
                            this.reload_if_showing(Some(&node), cx)
                        }
                    },
                );
            }
            "sort-by-name" => {
                let (Some(id), node) = (smart_folder_id(node), node.to_owned()) else {
                    return;
                };
                edit_then_in(
                    &self.session,
                    Report::default(),
                    Reread::NOTHING,
                    move |library| {
                        library
                            .update_smart_folder(id, sorted_by_name(smart_input(library, id)?))
                            .map(|_| ())
                    },
                    window,
                    cx,
                    move |this, result, _, cx| {
                        if result.is_ok() {
                            this.reload_if_showing(Some(&node), cx)
                        }
                    },
                );
            }
            "sort-tree" => {
                let (Some(id), Some(order)) = (folder_id(node), value) else {
                    return;
                };
                let descending = order.starts_with("za");
                let recursive = order.ends_with("-all");
                edit_then_in(
                    &self.session,
                    Report::default(),
                    Reread::NOTHING,
                    move |library| {
                        library
                            .sort_folder_tree(id, descending, recursive)
                            .map(|_| ())
                    },
                    window,
                    cx,
                    |_, _, _, _| {},
                );
            }
            "sort-contents" => {
                if let (Some(id), Some(field)) = (
                    folder_id(node),
                    value.as_deref().and_then(content_sort_field),
                ) {
                    self.sort_contents(vec![id], field, window, cx);
                }
            }
            "color" => self.apply_color(
                vec![node.to_owned()],
                value.filter(|hex| !hex.is_empty()),
                cx,
            ),
            "move-under" => self.move_smart(vec![node.to_owned()], value, window, cx),
            "move-to" => self.pick_folder_parent(vec![node.to_owned()], window, cx),
            "auto-tags" => {
                if let Some(id) = folder_id(node) {
                    self.edit_auto_tags(vec![id], window, cx);
                }
            }
            "attach-watch" => {
                if let Some(id) = folder_id(node) {
                    self.edit_watch(id, window, cx);
                }
            }
            "remove-watch" => {
                if let Some(id) = folder_id(node) {
                    let name = entries[index].label.clone();
                    self.open_dialog(
                        dialogs::Dialog::Confirm {
                            title: t("Remove Watch"),
                            message: format!("Stop watching the folder for \"{name}\"?"),
                            label: t("Remove"),
                            then: dialogs::Confirmed::RemoveWatch(id),
                        },
                        cx,
                    );
                }
            }
            "import-here" => {
                if let Some(id) = folder_id(node) {
                    import_folder(Some(id), window, cx);
                }
            }
            "export-originals" | "export-as" | "export-pack" => {
                // The export takes every item that the folder or smart
                // folder holds, in the default order of its query.
                let source = match (folder_id(node), smart_folder_id(node)) {
                    (Some(id), _) => crate::export::PackSource::Folder(id),
                    (_, Some(id)) => crate::export::PackSource::SmartFolder(id),
                    _ => return,
                };
                let scope = match source {
                    crate::export::PackSource::Folder(folder_id) => ItemScope::Folder { folder_id },
                    crate::export::PackSource::SmartFolder(smart_folder_id) => {
                        ItemScope::SmartFolder { smart_folder_id }
                    }
                    crate::export::PackSource::Items(_) => return,
                };
                let target = picto_library::selection::SelectionTarget::Query {
                    query: RootQuery {
                        scope,
                        view: Default::default(),
                    },
                    excluded_root_ids: Vec::new(),
                };
                let (count, name) = (
                    entries[index].count.unwrap_or(0),
                    entries[index].label.clone(),
                );
                if let Some(exports) = crate::export::get(cx) {
                    exports.update(cx, |exports, cx| match action {
                        "export-originals" => exports.export_originals(target, cx),
                        "export-as" => exports.export_as(target, count, cx),
                        _ => exports.export_pack(source, count, Some(name), cx),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Starts an import of a folder from disk into `folder`, or into the
/// library's top level when `folder` is `None`. The user first chooses the
/// folder on disk (`Imports::choose_folder`). The menu rows "Import
/// Folder..." and "Import Folder Here..." call this.
fn import_folder(folder: Option<picto_library::FolderId>, window: &mut Window, cx: &mut App) {
    if let Some(imports) = crate::imports::get(cx) {
        imports.update(cx, |imports, cx| {
            imports.choose_folder(
                crate::imports::plan::Destination::folder(folder),
                window,
                cx,
            )
        });
    }
}
