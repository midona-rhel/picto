//! What a chosen sidebar menu row does, for a system row, a section,
//! a selection or one node.
use super::edits::{smart_input, sorted_by_name};
use super::menus::content_sort_field;
use super::{
    EmptyTrash, Section, Sidebar, Target, dialogs, expandable, folder_id, smart_folder_id,
};
use crate::backend::Navigation;
use crate::session::Reread;
use gpui::*;
use picto_library::query::{ItemScope, RootQuery};

impl Sidebar {
    /// Does what a menu row says, for what the menu was opened on. `value`
    /// is the chosen row's or swatch's value, for rows that share an action.
    pub(super) fn choose(
        &mut self,
        target: &Target,
        action: &'static str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The visibility rows and sections, saved as the Settings window
        // saves them (`persistSystemVisibility`, `persistSectionVisibility`).
        // The menu stays open with the row's check following.
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
                "clear-recent" => self.edit_reading(
                    |library| library.clear_recent_views().map(|_| ()),
                    Reread::RESULTS_ONLY,
                    |_, (), _, _| {},
                    window,
                    cx,
                ),
                "restore-all" => self.edit_reading(
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
                    Reread::RESULTS_ONLY,
                    |_, (), _, _| {},
                    window,
                    cx,
                ),
                // The shell asks and deletes, as it does for the grid's
                // Delete Permanently.
                "empty-trash" => cx.emit(EmptyTrash),
                _ => {}
            },
            Target::Section => match action {
                "new-folder" => self.create_folder(None, window, cx),
                "new-group" => self.create_group(None, window, cx),
                "new-smart-folder" => self.new_smart_folder(None, window, cx),
                // TODO: "Import Folder..." chooses a folder and imports it
                // into the library; import is not built yet.
                _ => {}
            },
            Target::Node(node) => self.choose_for_node(node, action, value, window, cx),
            Target::Bulk(nodes) => self.choose_for_selection(nodes, action, value, window, cx),
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
                self.edit(
                    move |library| {
                        for id in ids {
                            library.update_smart_folder(
                                id,
                                sorted_by_name(smart_input(library, id)?),
                            )?;
                        }
                        Ok(())
                    },
                    |this, (), _, cx| this.reload_if_showing(None, cx),
                    window,
                    cx,
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
                self.edit(
                    |_| Ok(()),
                    move |this, (), _, cx| this.reload_if_showing(Some(&node), cx),
                    window,
                    cx,
                );
            }
            "sort-by-name" => {
                let (Some(id), node) = (smart_folder_id(node), node.to_owned()) else {
                    return;
                };
                self.edit(
                    move |library| {
                        library
                            .update_smart_folder(id, sorted_by_name(smart_input(library, id)?))
                            .map(|_| ())
                    },
                    move |this, (), _, cx| this.reload_if_showing(Some(&node), cx),
                    window,
                    cx,
                );
            }
            "sort-tree" => {
                let (Some(id), Some(order)) = (folder_id(node), value) else {
                    return;
                };
                let descending = order.starts_with("za");
                let recursive = order.ends_with("-all");
                self.edit(
                    move |library| {
                        library
                            .sort_folder_tree(id, descending, recursive)
                            .map(|_| ())
                    },
                    |_, (), _, _| {},
                    window,
                    cx,
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
                            title: "Remove Watch",
                            message: format!("Stop watching the folder for \"{name}\"?"),
                            label: "Remove",
                            then: dialogs::Confirmed::RemoveWatch(id),
                        },
                        cx,
                    );
                }
            }
            // TODO: "Import Folder Here..." chooses a folder on disk and
            // imports it into this folder; import is not built yet. "Export"
            // waits for export.
            _ => {}
        }
    }
}
