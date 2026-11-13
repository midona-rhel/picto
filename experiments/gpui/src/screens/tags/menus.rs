//! The Tags screen's context menus: a tag's and a group's, and what their
//! rows do.
use super::dialogs::Dialog;
use super::{Browse, MenuTarget, TagsScreen, dialogs};
use crate::ui::{
    self,
    menu::{self, Menu},
};
use gpui::*;
use picto_library::{TagNamespaceRecord, TagRecord};

impl TagsScreen {
    fn open_menu(
        &mut self,
        entries: Vec<menu::Entry>,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self.menu
            .open(menu, window, cx, |this, menu, action, window, cx| {
                let value = menu.read(cx).chosen_value();
                this.run_menu_action(action, value, window, cx);
            });
        self.menu_target = Some(target);
    }

    /// `openTagContextMenu`, with `buildCommonTagContextEntries`.
    pub(super) fn open_tag_menu(
        &mut self,
        tag: TagRecord,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut entries = crate::tags::common_menu(&tag, &self.namespaces, true, cx);
        entries.extend([
            menu::Entry::Separator,
            menu::item("edit", "Edit Tag").icon("menu-edit").into(),
            menu::item("merge", "Merge Into…")
                .icon("menu-git-merge")
                .into(),
            menu::Entry::Separator,
            menu::item("delete", "Delete Tag")
                .icon("menu-trash")
                .danger(true)
                .into(),
        ]);
        self.open_menu(entries, MenuTarget::Tag(tag), position, window, cx);
    }

    /// `openGroupContextMenu`
    pub(super) fn open_group_menu(
        &mut self,
        group: TagNamespaceRecord,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = self.preferences.colors.get(&group.name).cloned();
        let entries = vec![
            menu::item("show-group", "Show Tags in Group")
                .icon("menu-bookmark")
                .into(),
            menu::Entry::Separator,
            menu::item("rename-group", "Rename Group…")
                .icon("menu-edit")
                .into(),
            ui::color_picker::menu_row("group-color", chosen.as_deref()).into(),
            menu::item("delete-group", "Delete Group")
                .icon("menu-trash")
                .into(),
        ];
        self.open_menu(entries, MenuTarget::Group(group), position, window, cx);
    }

    fn run_menu_action(
        &mut self,
        action: &str,
        value: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.menu_target.as_ref() {
            Some(MenuTarget::Tag(tag)) => {
                let tag = tag.clone();
                let name = crate::tags::record_name(&tag);
                match action {
                    "filter" => self.show_items(tag.tag_id, cx),
                    "star" => self.set_starred(name, true, cx),
                    "unstar" => self.set_starred(name, false, cx),
                    "move-to-group" => {
                        if let Some(group) = value {
                            let next = crate::tags::name_in_group(&tag.subname, Some(&group));
                            self.rename_tag(&tag, next, false, cx);
                        }
                    }
                    "remove-from-group" => {
                        let next = crate::tags::name_in_group(&tag.subname, None);
                        self.rename_tag(&tag, next, false, cx);
                    }
                    "copy" => cx.write_to_clipboard(ClipboardItem::new_string(name)),
                    "edit" => self.open_editor(tag, None, window, cx),
                    "merge" => {
                        let merge = dialogs::merge(&tag, cx);
                        self.open_editor(tag, Some(merge), window, cx);
                    }
                    "delete" => {
                        let delete = Dialog::DeleteTag { tag: tag.clone() };
                        self.open_editor(tag, Some(delete), window, cx);
                    }
                    _ => {}
                }
            }
            Some(MenuTarget::Group(group)) => {
                let group = group.clone();
                match action {
                    "show-group" => self.show(Browse::Group(group.name), cx),
                    "rename-group" => {
                        let dialog = dialogs::rename_group(group, cx);
                        self.push(dialog, cx);
                    }
                    "group-color" => {
                        let color = value.filter(|value| !value.is_empty());
                        self.set_group_color(&group.name, color.as_deref(), cx);
                    }
                    "delete-group" => self.push(Dialog::DeleteGroup { group }, cx),
                    _ => {}
                }
            }
            None => {}
        }
        cx.notify();
    }
}
