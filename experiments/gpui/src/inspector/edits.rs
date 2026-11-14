//! The inspector's edits to what it describes, one item or every selected
//! item alike: tags and folders from the chips, the pickers and their menus,
//! source addresses, and the confirmation before overwriting what some
//! selected items already have.
use super::membership::{self, Touches};
use super::tag_picker;
use super::{ChipMenu, Confirm, FOLDER_COLOR, Inspector, Mode, Overwrite};
use crate::backend::Navigation;
use crate::session::{Report, Reread};
use crate::tags::record_name as name;
use crate::ui::{
    self,
    menu::{self, Menu},
};
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, Library, MutationReceipt, Rating, RootId, TagId, TagRecord};

/// What the inspector's edits apply to: the items, their tags and their
/// folders (for several items, the ones they all share).
pub(super) struct Target {
    pub ids: Vec<RootId>,
    pub tags: Vec<TagRecord>,
    pub folders: Vec<FolderId>,
}

impl Inspector {
    /// The items the rail describes, if it describes items.
    pub(super) fn target(&self, cx: &App) -> Option<Target> {
        let session = self.session.read(cx);
        let records = |ids: &[TagId]| {
            ids.iter()
                .filter_map(|id| session.tags.get(&id.0).cloned())
                .collect::<Vec<_>>()
        };
        match self.shown {
            Mode::Item => {
                let item = self.item.as_ref()?;
                Some(Target {
                    ids: vec![item.root.root_id],
                    tags: records(&item.tag_ids),
                    folders: item.folder_ids.clone(),
                })
            }
            Mode::Selection => {
                let shown = self.selection.shown.as_ref()?;
                let summary = shown.summary.as_ref()?;
                Some(Target {
                    ids: shown.ids.clone(),
                    tags: records(&summary.shared_tags),
                    folders: summary.shared_folders.clone(),
                })
            }
            _ => None,
        }
    }

    /// The open pickers follow the items' tags and folders.
    pub(super) fn sync_pickers(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target(cx) else {
            return;
        };
        self.tag_picker
            .update(cx, |picker, cx| picker.set_entity(target.tags, cx));
        self.folder_picker
            .update(cx, |picker, cx| picker.set_entity(target.folders, cx));
    }

    /// Applies `work` to the items the rail describes, then reads again what
    /// it changed: the item or the selection's summary, and the grid where
    /// its results may differ. The destinations and their counts follow the
    /// library's change events.
    pub(super) fn edit(
        &mut self,
        touches: Touches,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<MutationReceipt>
        + Send
        + 'static,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.edit_reading(touches, false, work, then, cx);
    }

    /// [`Inspector::edit`], reading the destination list and the tags with
    /// the edit when `navigation`: a tag it creates must be among the
    /// session's tags by the time the item's chips are drawn again.
    fn edit_reading(
        &mut self,
        touches: Touches,
        navigation: bool,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<MutationReceipt>
        + Send
        + 'static,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.target(cx) else {
            return;
        };
        self.edit_items(
            target.ids,
            touches,
            navigation,
            move |library, target| work(library, target).map(|_| ()),
            then,
            cx,
        );
    }

    /// [`Inspector::edit_reading`] for the items given rather than those the
    /// rail describes: the Add Tags and Folders dialogs edit the grid's
    /// selection.
    pub(super) fn edit_items(
        &mut self,
        ids: Vec<RootId>,
        touches: Touches,
        navigation: bool,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<()> + Send + 'static,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let selection = self.session.read(cx).target_for(ids);
        let reread = Reread {
            results: self.changes_results(touches, cx),
            navigation,
            ..Reread::NOTHING
        };
        crate::session::edit_then(
            &self.session,
            crate::session::Report::default(),
            reread,
            move |library| work(library, &selection),
            cx,
            |this, _, cx| {
                this.reread_rail(cx);
                then(this, cx);
            },
        );
    }

    /// One item's name, as its field commits it.
    pub(super) fn rename_item(&mut self, id: RootId, name: String, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let _ = session.edit(
                move |library| library.rename_root(id, &name, crate::now_ms()),
                Reread::DETAILS,
                cx,
            );
        });
    }

    /// One item's notes, as their field commits them.
    pub(super) fn set_item_notes(&mut self, id: RootId, notes: String, cx: &mut Context<Self>) {
        let target = SelectionTarget::Explicit { root_ids: vec![id] };
        self.session.update(cx, |session, cx| {
            let _ = session.edit(
                move |library| library.set_notes(&target, Some(notes), crate::now_ms()),
                Reread::DETAILS,
                cx,
            );
        });
    }

    /// One item's rating, from its stars.
    pub(super) fn rate_item(&mut self, id: RootId, rating: Rating, cx: &mut Context<Self>) {
        let target = SelectionTarget::Explicit { root_ids: vec![id] };
        self.session.update(cx, |session, cx| {
            let _ = session.edit_reporting(
                Report::failure("Could not change rating"),
                move |library| library.set_rating(&target, rating),
                Reread::DETAILS,
                cx,
            );
        });
    }

    /// Reads again what the rail shows after an edit, and the grid when the
    /// edit may change which items its destination holds.
    pub(super) fn reread(&mut self, touches: Touches, cx: &mut Context<Self>) {
        self.reread_rail(cx);
        let reread = Reread {
            results: self.changes_results(touches, cx),
            ..Reread::NOTHING
        };
        self.session
            .update(cx, |session, cx| session.reread(reread, cx));
    }

    /// Reads again what the rail shows: the item, even a pinned one, or the
    /// selection's summary.
    fn reread_rail(&mut self, cx: &mut Context<Self>) {
        match self.shown {
            Mode::Item => {
                if let Some(id) = self.item.as_ref().map(|item| item.root.root_id) {
                    self.reread_item(id, cx);
                }
                if !self.pinned {
                    self.session
                        .update(cx, |session, cx| session.refresh_details(cx));
                }
            }
            Mode::Selection => self.reread_summary(cx),
            _ => {}
        }
    }

    /// Whether an edit that `touches` this may change which items the
    /// grid's destination holds.
    fn changes_results(&self, touches: Touches, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.nav.get(session.scope).is_some_and(|entry| {
            membership::changes_results(
                &entry.scope,
                session.filter.is_some(),
                session.searching(),
                touches,
            )
        })
    }

    /// The details of the item shown, read directly, as a pinned item's
    /// are not the selection's.
    fn reread_item(&mut self, id: RootId, cx: &mut Context<Self>) {
        let answer = self
            .session
            .read(cx)
            .run(move |library| library.details(id));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(details)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.item.as_ref().map(|item| item.root.root_id) != Some(id) {
                    return;
                }
                let key = id.0 as u64;
                this.name.update(cx, |field, cx| {
                    field.set_field(key, details.root.name.clone(), cx)
                });
                this.notes.update(cx, |field, cx| {
                    field.set_field(key, details.root.notes.clone().unwrap_or_default(), cx)
                });
                this.item = Some(details);
                this.sync_pickers(cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ── Tags and folders ────────────────────────────────────────

    /// Puts the tag on the items or takes it off.
    pub(super) fn set_tag(
        &mut self,
        tag_name: String,
        add: bool,
        created: bool,
        cx: &mut Context<Self>,
    ) {
        if add {
            // `addTargetTags` remembers the tags for the Recent view.
            let added = [tag_name.clone()];
            self.session
                .update(cx, |session, _| session.record_tags_use(&added));
        }
        self.edit_reading(
            Touches::Tags,
            created,
            move |library, target| library.apply_tags(target, &[tag_name], add),
            move |this, cx| {
                this.tag_picker
                    .update(cx, |picker, cx| picker.edited(created, cx))
            },
            cx,
        );
    }

    /// Adds the items to the folder or takes them out.
    pub(super) fn set_membership(&mut self, folder: FolderId, add: bool, cx: &mut Context<Self>) {
        self.edit(
            Touches::Folders,
            move |library, target| {
                if add {
                    library.add_to_folder(target, folder)
                } else {
                    library.remove_from_folder(target, folder)
                }
            },
            |_, _| {},
            cx,
        );
    }

    pub(super) fn tag_picker_event(&mut self, event: &tag_picker::Event, cx: &mut Context<Self>) {
        match event {
            tag_picker::Event::Toggle { name, add, created } => {
                self.set_tag(name.clone(), *add, *created, cx)
            }
            tag_picker::Event::Filter(tag) => self.show_tag_items(*tag, cx),
            // The chips' labels follow the destination list, which the
            // library's change events read again.
            tag_picker::Event::Renamed => self.reread(Touches::Tags, cx),
            // The inspector opens the picker only to change the items' tags.
            tag_picker::Event::FilterChanged { .. } => {}
        }
    }

    /// `showTagItems`: the grid's destination, narrowed to items with the
    /// tag as well as by the filters it has.
    fn show_tag_items(&mut self, tag: TagId, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let mut filters = session.filters.clone();
            if !filters
                .include_tags
                .iter()
                .any(|choice| choice.tag_id == tag.0)
            {
                let name = session.tags.get(&tag.0).map(name).unwrap_or_default();
                filters.include_tags.push(crate::grid::filters::TagChoice {
                    tag_id: tag.0,
                    name,
                });
            }
            session.set_filters(filters, cx);
        });
    }

    // ── The pickers ─────────────────────────────────────────────

    pub(super) fn open_tag_picker_at(
        &mut self,
        control: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.anchor(control);
        self.open_tag_picker(anchor, window, cx);
    }

    pub(super) fn open_folder_picker_at(
        &mut self,
        control: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.anchor(control);
        self.open_folder_picker(anchor, window, cx);
    }

    fn open_tag_picker(
        &mut self,
        anchor: Option<Point<f32>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.target(cx) else {
            return;
        };
        self.picker_destination = super::displayed_id(self.session.read(cx));
        self.folder_picker
            .update(cx, |picker, cx| picker.close(window, cx));
        self.tag_picker.update(cx, |picker, cx| {
            picker.open(anchor, target.tags, window, cx)
        });
    }

    fn open_folder_picker(
        &mut self,
        anchor: Option<Point<f32>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.target(cx) else {
            return;
        };
        self.picker_destination = super::displayed_id(self.session.read(cx));
        self.tag_picker
            .update(cx, |picker, cx| picker.close(window, cx));
        self.folder_picker.update(cx, |picker, cx| {
            picker.open(anchor, target.folders, window, cx)
        });
    }

    /// `inspectorAnchor`: a picker opened by its shortcut docks 80px below
    /// the rail's top, or is centred when the rail is hidden.
    fn keyboard_anchor(&self, visible: bool) -> Option<Point<f32>> {
        let rail = *self.places.borrow().get("rail")?;
        visible.then(|| point(f32::from(rail.left()), f32::from(rail.top()) + 80.))
    }

    /// The viewer menu's Add Tags.
    pub fn add_tags(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.keyboard_anchor(visible);
        self.open_tag_picker(anchor, window, cx);
    }

    /// The viewer menu's Add to Folder.
    pub fn add_to_folder(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.keyboard_anchor(visible);
        self.open_folder_picker(anchor, window, cx);
    }

    // ── The dialogs ─────────────────────────────────────────────

    /// The grid's selected items, in grid order.
    fn selected_ids(&self, cx: &App) -> Vec<RootId> {
        self.session.read(cx).selected_ids()
    }

    /// T and the grid menu's Add Tags (`TagSelectModal`): the dialog checks
    /// the tags of the item the inspector shows, or last showed
    /// (`displayedInspectorItemDetailsAtom`), whatever is selected.
    pub fn open_tag_dialog(&mut self, cx: &mut Context<Self>) {
        let names = {
            let session = self.session.read(cx);
            self.item
                .iter()
                .flat_map(|item| &item.tag_ids)
                .filter_map(|id| session.tags.get(&id.0))
                .map(name)
                .collect()
        };
        let selected = self.session.read(cx).selection_count() as usize;
        self.tag_dialog
            .update(cx, |dialog, cx| dialog.open(names, selected, cx));
    }

    /// F, ⌘⇧J and the grid menu's Add to Folder (`FolderPickerModal`), on
    /// the folders of the item the inspector shows, or last showed.
    pub fn open_folder_dialog(&mut self, cx: &mut Context<Self>) {
        let members = self
            .item
            .iter()
            .flat_map(|item| item.folder_ids.iter().copied())
            .collect();
        let selected = self.session.read(cx).selection_count() as usize;
        self.folder_dialog
            .update(cx, |dialog, cx| dialog.open(members, selected, cx));
    }

    /// The Add Tags dialog's Apply: the tags go on the selected items and
    /// come off, and the ones added become recent.
    pub(super) fn apply_tag_dialog(
        &mut self,
        add: Vec<String>,
        remove: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let ids = self.selected_ids(cx);
        if ids.is_empty() {
            return;
        }
        if !add.is_empty() {
            self.session
                .update(cx, |session, _| session.record_tags_use(&add));
        }
        self.edit_items(
            ids,
            Touches::Tags,
            false,
            move |library, target| {
                if !add.is_empty() {
                    library.apply_tags(target, &add, true)?;
                }
                if !remove.is_empty() {
                    library.apply_tags(target, &remove, false)?;
                }
                Ok(())
            },
            |_, _| {},
            cx,
        );
    }

    /// The Folders dialog's Apply: the selected items join and leave the
    /// folders, the ones joined become recent, and the selection is
    /// cleared (`settleSelectionAfterMutation`).
    pub(super) fn apply_folder_dialog(
        &mut self,
        add: Vec<FolderId>,
        remove: Vec<FolderId>,
        cx: &mut Context<Self>,
    ) {
        let ids = self.selected_ids(cx);
        if ids.is_empty() {
            return;
        }
        self.session.update(cx, |session, _| {
            for folder in &add {
                session.record_folder_use(*folder);
            }
        });
        self.edit_items(
            ids,
            Touches::Folders,
            false,
            move |library, target| {
                for folder in add {
                    library.add_to_folder(target, folder)?;
                }
                for folder in remove {
                    library.remove_from_folder(target, folder)?;
                }
                Ok(())
            },
            |this, cx| {
                this.session
                    .update(cx, |session, cx| session.clear_selection(cx))
            },
            cx,
        );
    }

    pub(super) fn close_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tag_picker
            .update(cx, |picker, cx| picker.close(window, cx));
        self.folder_picker
            .update(cx, |picker, cx| picker.close(window, cx));
    }

    // ── Chips ───────────────────────────────────────────────────

    /// The Folders section: a chip each in its folder's colour, then "+".
    pub(super) fn folder_chips(
        &self,
        entries: &[Navigation],
        folders: &[FolderId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut chips = div().flex().flex_wrap().gap(px(4.));
        for folder in folders {
            let folder = *folder;
            let entry = entries.iter().find(|entry| {
                matches!(entry.scope, ItemScope::FolderTree { folder_id } if folder_id == folder)
            });
            let label = entry.map_or_else(
                || format!("Folder {}", folder.0),
                |entry| entry.label.clone(),
            );
            let color = entry.and_then(|entry| entry.tint).unwrap_or(FOLDER_COLOR);
            chips = chips.child(
                ui::chip(("inspector-folder", folder.0 as usize), label, color)
                    .on_remove(
                        cx.listener(move |this, _, _, cx| this.set_membership(folder, false, cx)),
                    )
                    .on_menu(
                        cx.listener(move |this, position: &Point<Pixels>, window, cx| {
                            this.open_chip_menu(ChipMenu::Folder(folder), *position, window, cx)
                        }),
                    ),
            );
        }
        self.folders_row(chips, folders.is_empty(), window, cx)
    }

    /// The Tags section: a chip each, by group then name for one item
    /// (`tagKey`), then "+".
    pub(super) fn tag_chips(
        &self,
        mut tags: Vec<TagRecord>,
        sort: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if sort {
            tags.sort_by_key(|tag| {
                (
                    crate::tags::inspector_order(&tag.namespace),
                    tag.subname.to_lowercase(),
                )
            });
        }
        let mut chips = div().flex().flex_wrap().gap(px(4.));
        let empty = tags.is_empty();
        for tag in tags {
            let raw = name(&tag);
            let menu_tag = tag.clone();
            chips = chips.child(
                super::tag_chip(&tag, cx)
                    .on_remove(cx.listener(move |this, _, _, cx| {
                        this.set_tag(raw.clone(), false, false, cx)
                    }))
                    .on_menu(
                        cx.listener(move |this, position: &Point<Pixels>, window, cx| {
                            this.open_chip_menu(
                                ChipMenu::Tag(menu_tag.clone()),
                                *position,
                                window,
                                cx,
                            )
                        }),
                    ),
            );
        }
        self.tags_row(chips, empty, window, cx)
    }

    /// The chips' menus: a tag's common rows with Remove Tag
    /// (`buildCommonTagContextEntries`), a folder's Open Folder and Remove.
    fn open_chip_menu(
        &mut self,
        target: ChipMenu,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = match &target {
            ChipMenu::Tag(tag) => {
                let mut entries = crate::tags::common_menu(tag, &[], false, cx);
                entries.push(menu::item("remove", "Remove Tag").icon("menu-x").into());
                entries
            }
            ChipMenu::Folder(_) => vec![
                menu::item("open-folder", "Open Folder").into(),
                menu::item("remove", "Remove").into(),
            ],
            ChipMenu::Swatch => return,
        };
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self.menu
            .open(menu, window, cx, |this, _, action, window, cx| {
                this.run_chip_menu(action, window, cx)
            });
        self.menu_target = Some(target);
    }

    fn run_chip_menu(&mut self, action: &str, _: &mut Window, cx: &mut Context<Self>) {
        match self.menu_target.clone() {
            Some(ChipMenu::Tag(tag)) => match action {
                "filter" => self.show_tag_items(tag.tag_id, cx),
                "star" => crate::tags::set_starred(&name(&tag), true, cx),
                "unstar" => crate::tags::set_starred(&name(&tag), false, cx),
                "copy" => cx.write_to_clipboard(ClipboardItem::new_string(name(&tag))),
                "remove" => self.set_tag(name(&tag), false, false, cx),
                _ => {}
            },
            Some(ChipMenu::Folder(folder)) => match action {
                // `navigateToFolder`
                "open-folder" => self.session.update(cx, |session, cx| {
                    if let Some(index) = session.nav.iter().position(|entry| {
                        matches!(entry.scope, ItemScope::FolderTree { folder_id } if folder_id == folder)
                    }) {
                        session.navigate(index, cx);
                    }
                }),
                "remove" => self.set_membership(folder, false, cx),
                _ => {}
            },
            Some(ChipMenu::Swatch) | None => {}
        }
    }

    // ── Sources and notes of several items ──────────────────────

    /// Saves the items' source addresses. For several items that already
    /// have some, it asks first (`commitSources`).
    pub(super) fn set_sources(&mut self, urls: Vec<String>, cx: &mut Context<Self>) {
        match self.shown {
            Mode::Item => self.edit(
                Touches::Text,
                move |library, target| library.set_source_urls(target, urls, crate::now_ms()),
                |_, _| {},
                cx,
            ),
            Mode::Selection => {
                let Some(summary) = self
                    .selection
                    .shown
                    .as_ref()
                    .and_then(|shown| shown.summary.as_ref())
                else {
                    return;
                };
                if summary.shared_source_urls.as_ref() == Some(&urls) {
                    return;
                }
                if summary.has_source_urls {
                    let count = summary.selected_count;
                    self.ask(Overwrite::Sources(urls), count, cx);
                } else {
                    self.overwrite(Overwrite::Sources(urls), cx);
                }
            }
            _ => {}
        }
    }

    /// Asks before replacing what some of the selected items have.
    pub(super) fn ask(&mut self, overwrite: Overwrite, count: u64, cx: &mut Context<Self>) {
        // The keyboard is left where the dialog leaves it.
        self.confirm
            .open(Confirm { overwrite, count }, menu::Return::Stay, cx);
    }

    /// Writes the notes or addresses to every selected item.
    pub(super) fn overwrite(&mut self, overwrite: Overwrite, cx: &mut Context<Self>) {
        match overwrite {
            Overwrite::Notes(notes) => self.edit(
                Touches::Text,
                move |library, target| library.set_notes(target, Some(notes), crate::now_ms()),
                |_, _| {},
                cx,
            ),
            Overwrite::Sources(urls) => self.edit(
                Touches::Text,
                move |library, target| library.set_source_urls(target, urls, crate::now_ms()),
                |_, _| {},
                cx,
            ),
        }
    }

    /// `ConfirmModal` for `confirmSelectionOverwrite`.
    pub(super) fn confirm_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let confirm = self.confirm.layer(window, cx)?;
        // It keeps the keyboard while it is open.
        if !confirm.focus.contains_focused(window, cx) && !confirm.presence.closing() {
            window.focus(&confirm.focus, cx);
        }
        let (title, label, what) = match confirm.content.overwrite {
            Overwrite::Notes(_) => ("Overwrite notes?", "Overwrite Notes", "notes"),
            Overwrite::Sources(_) => ("Overwrite sources?", "Overwrite Sources", "source URLs"),
        };
        let message = format!(
            "This will replace existing {what} on all {} selected items. Are you sure?",
            crate::grid::format::grouped(confirm.content.count)
        );
        let this = cx.entity().downgrade();
        let (dismiss, accept) = (this.clone(), this);
        Some(
            ui::confirm::confirm("inspector-confirm", title, message, label, confirm.presence)
                .danger(true)
                .focus(&confirm.focus)
                .on_close(move |_, cx| {
                    let _ = dismiss.update(cx, |this, cx| this.confirm.dismiss(cx));
                })
                .on_confirm(move |_, cx| {
                    let _ = accept.update(cx, |this, cx| {
                        this.confirm.dismiss(cx);
                        if let Some(confirm) = this.confirm.get_mut() {
                            let overwrite = std::mem::replace(
                                &mut confirm.content.overwrite,
                                Overwrite::Notes(String::new()),
                            );
                            this.overwrite(overwrite, cx);
                        }
                    });
                })
                .into_any_element(),
        )
    }
}
