//! The inspector's edits. The inspector describes one item or a selection of
//! several items, and an edit applies to whichever of the two it describes.
//! This file holds the edits of tags and folders (made with the chips, the
//! pickers and the chips' menus), the edits of source addresses, and the
//! dialog that asks before an edit replaces notes or addresses that some of
//! the selected items already have.
use super::membership::{self, Touches};
use super::tag_picker;
use super::{ChipMenu, Confirm, FOLDER_COLOR, Inspector, Mode, Overwrite};
use crate::backend::Navigation;
use crate::i18n::t;
use crate::session::{Report, Reread};
use crate::tags::{Common, record_name as name};
use crate::ui::{
    self,
    menu::{self, Menu},
};
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, Library, MutationReceipt, Rating, RootId, TagId, TagRecord};

/// Holds what the inspector's edits apply to: the items, their tags and their
/// folders. When the inspector describes several items, `tags` and `folders`
/// are the ones that every one of the items has.
pub(super) struct Target {
    /// The one item the inspector shows, or the selection its summary
    /// describes.
    pub selection: SelectionTarget,
    pub tags: Vec<TagRecord>,
    pub folders: Vec<FolderId>,
}

impl Inspector {
    /// Returns what an edit applies to now. It returns `None` when the
    /// inspector does not describe items, and while the summary of a selection
    /// is loading.
    pub(super) fn target(&self, cx: &App) -> Option<Target> {
        let session = self.session.read(cx);
        let records = |ids: &[TagId]| {
            let mut records = ids
                .iter()
                .filter_map(|id| session.tags.get(&id.0).cloned())
                .collect::<Vec<_>>();
            crate::tags::sort_as_library(&mut records);
            records
        };
        match self.shown {
            Mode::Item => {
                let item = self.item.as_ref()?;
                Some(Target {
                    selection: SelectionTarget::Explicit {
                        root_ids: vec![item.root.root_id],
                    },
                    tags: records(&item.tag_ids),
                    folders: item.folder_ids.clone(),
                })
            }
            // While the summary of a new selection is loading, the tags and
            // folders that the items share are not known, so nothing can be
            // edited.
            Mode::Selection if self.selection.loading() => None,
            Mode::Selection => {
                let shown = self.selection.shown.as_ref()?;
                let summary = shown.summary.as_ref()?;
                Some(Target {
                    selection: shown.target.clone(),
                    tags: records(&summary.shared_tags),
                    folders: summary.shared_folders.clone(),
                })
            }
            _ => None,
        }
    }

    /// Gives the tag picker and the folder picker the tags and folders the
    /// items have now, so that the ticks in an open picker match the items.
    pub(super) fn sync_pickers(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target(cx) else {
            return;
        };
        self.tag_picker
            .update(cx, |picker, cx| picker.set_entity(target.tags, cx));
        self.folder_picker
            .update(cx, |picker, cx| picker.set_entity(target.folders, cx));
    }

    /// Applies `work` to the items the inspector describes. Afterwards it
    /// reads again what the edit changed: the item's details or the
    /// selection's summary, and the grid's results when the edit may change
    /// which items the grid shows. The sidebar's destinations and counts are
    /// not read here. The session reads them again when the library reports
    /// the change.
    pub(super) fn edit(
        &mut self,
        touches: Touches,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<MutationReceipt>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = self.target(cx) {
            let work = move |library: &Library, target: &SelectionTarget| {
                work(library, target).map(|_| ())
            };
            self.edit_items(target.selection, touches, false, work, |_, _| {}, cx);
        }
    }

    /// Does what [`Inspector::edit`] does for the items of `selection`, and
    /// calls `then` when the edit is done. The Add Tags and Folders dialogs
    /// use it to edit the grid's selection.
    ///
    /// With `navigation` set, the session reads its list of destinations and
    /// its list of tags again together with the edit. An edit that creates a
    /// tag needs this: the new tag has to be in the session's list of tags
    /// before the chips are drawn again.
    pub(super) fn edit_items(
        &mut self,
        selection: SelectionTarget,
        touches: Touches,
        navigation: bool,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<()> + Send + 'static,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
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

    /// Renames one item. The name field calls this when its text is committed.
    pub(super) fn rename_item(&mut self, id: RootId, name: String, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let _ = session.edit(
                move |library| library.rename_root(id, &name, crate::now_ms()),
                Reread::DETAILS,
                cx,
            );
        });
    }

    /// Sets one item's notes. The notes field calls this when its text is
    /// committed.
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

    /// Sets one item's rating. A click on the stars calls this.
    pub(super) fn rate_item(&mut self, id: RootId, rating: Rating, cx: &mut Context<Self>) {
        let target = SelectionTarget::Explicit { root_ids: vec![id] };
        self.session.update(cx, |session, cx| {
            let _ = session.edit_reporting(
                Report::failure(t("Could not change rating")),
                move |library| library.set_rating(&target, rating),
                Reread::DETAILS,
                cx,
            );
        });
    }

    /// Reads again what the inspector shows after an edit, and the grid's
    /// results when the edit may change which items the grid shows.
    pub(super) fn reread(&mut self, touches: Touches, cx: &mut Context<Self>) {
        self.reread_rail(cx);
        let reread = Reread {
            results: self.changes_results(touches, cx),
            ..Reread::NOTHING
        };
        self.session
            .update(cx, |session, cx| session.reread(reread, cx));
    }

    /// Reads again what the inspector shows: the item's details, also when the
    /// item is pinned, or the selection's summary.
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

    /// Returns true if an edit of the kind `touches` may change which items
    /// the grid shows in the open destination with its filters and its search.
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

    /// Reads the details of the item `id` from the library and shows them. A
    /// pinned item is not the selected item, so the details the session holds
    /// for the selection cannot be used for it.
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

    // The functions below edit the tags and the folders of the items.

    /// Puts the tag `tag_name` on the items when `add` is set, or takes it off
    /// them. `created` says that the tag did not exist before this edit.
    pub(super) fn set_tag(
        &mut self,
        tag_name: String,
        add: bool,
        created: bool,
        cx: &mut Context<Self>,
    ) {
        if add {
            // The tag is recorded as used, so that the pickers list it in their
            // Recent view.
            let added = [tag_name.clone()];
            self.session
                .update(cx, |session, _| session.record_tags_use(&added));
        }
        let Some(target) = self.target(cx) else {
            return;
        };
        self.edit_items(
            target.selection,
            Touches::Tags,
            created,
            move |library, target| library.apply_tags(target, &[tag_name], add).map(|_| ()),
            move |this, cx| {
                this.tag_picker
                    .update(cx, |picker, cx| picker.edited(created, cx))
            },
            cx,
        );
    }

    /// Adds the items to `folder` when `add` is set, or takes them out of it.
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
            cx,
        );
    }

    pub(super) fn tag_picker_event(&mut self, event: &tag_picker::Event, cx: &mut Context<Self>) {
        match event {
            tag_picker::Event::Toggle { name, add, created } => {
                self.set_tag(name.clone(), *add, *created, cx)
            }
            tag_picker::Event::Filter(tag) => self.show_tag_items(*tag, cx),
            // A chip takes its tag's name from the list of tags that the
            // session reads together with its destinations. The session reads
            // that list again when the library reports the rename, so only the
            // inspector's content and the grid's results are read again here.
            tag_picker::Event::Renamed => self.reread(Touches::Tags, cx),
            // The picker emits this only when it was opened to choose the tags
            // of a filter. The inspector opens it only to change the items'
            // tags.
            tag_picker::Event::FilterChanged { .. } => {}
        }
    }

    /// Adds the tag to the tags the grid's filter includes and keeps the
    /// filters that are already set, so that the grid shows only the items
    /// that have the tag.
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

    // The functions below open the tag picker and the folder picker.

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

    /// Returns where a picker that was opened with its shortcut is placed.
    /// With `visible` set, which means the inspector is shown, the picker is
    /// placed at the inspector's left edge, 80 px below the inspector's top.
    /// Otherwise this returns `None` and the picker is centred in the window.
    fn keyboard_anchor(&self, visible: bool) -> Option<Point<f32>> {
        let rail = *self.places.borrow().get("rail")?;
        visible.then(|| point(f32::from(rail.left()), f32::from(rail.top()) + 80.))
    }

    /// Opens the tag picker for Add Tags in the viewer's menu. `visible` says
    /// whether the inspector is shown.
    pub fn add_tags(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.keyboard_anchor(visible);
        self.open_tag_picker(anchor, window, cx);
    }

    /// Opens the folder picker for Add to Folder in the viewer's menu.
    /// `visible` says whether the inspector is shown.
    pub fn add_to_folder(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.keyboard_anchor(visible);
        self.open_folder_picker(anchor, window, cx);
    }

    // The functions below open the Add Tags dialog and the Folders dialog and
    // apply what was chosen in them.

    /// Returns the grid's selection as the target of an edit. It also covers
    /// selected items that the grid has not loaded.
    fn grid_selection(&self, cx: &App) -> Option<SelectionTarget> {
        self.session.read(cx).selection_target()
    }

    /// Opens the Add Tags dialog, for the T key and for Add Tags in the grid's
    /// menu. The dialog ticks the tags of the item the inspector shows or last
    /// showed, whatever is selected in the grid. The tags are given as
    /// records, because the dialog's Selected view shows each tag's count.
    pub fn open_tag_dialog(&mut self, cx: &mut Context<Self>) {
        let tags = {
            let session = self.session.read(cx);
            self.item
                .iter()
                .flat_map(|item| &item.tag_ids)
                .filter_map(|id| session.tags.get(&id.0))
                .cloned()
                .collect::<Vec<_>>()
        };
        let selected = self.session.read(cx).selection_count() as usize;
        self.tag_dialog
            .update(cx, |dialog, cx| dialog.open(&tags, selected, cx));
    }

    /// Opens the Folders dialog, for the F key, for Command+Shift+J
    /// (Control+Shift+J on Windows and Linux) and for Add to Folder in the
    /// grid's menu. The dialog ticks the folders of the item the inspector
    /// shows or last showed.
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

    /// Handles Apply in the Add Tags dialog. It puts the tags in `add` on the
    /// selected items, takes the tags in `remove` off them, and records the
    /// tags in `add` as recently used.
    pub(super) fn apply_tag_dialog(
        &mut self,
        add: Vec<String>,
        remove: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.grid_selection(cx) else {
            return;
        };
        if !add.is_empty() {
            self.session
                .update(cx, |session, _| session.record_tags_use(&add));
        }
        self.edit_items(
            selection,
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

    /// Handles Apply in the Folders dialog. It adds the selected items to the
    /// folders in `add`, takes them out of the folders in `remove`, records
    /// the folders in `add` as recently used, and then clears the selection.
    pub(super) fn apply_folder_dialog(
        &mut self,
        add: Vec<FolderId>,
        remove: Vec<FolderId>,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.grid_selection(cx) else {
            return;
        };
        self.session.update(cx, |session, _| {
            for folder in &add {
                session.record_folder_use(*folder);
            }
        });
        self.edit_items(
            selection,
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
        self.ai_tagger
            .update(cx, |tagger, cx| tagger.close(window, cx));
    }

    /// Opens the Auto Tag review, from a menu or from its shortcut. It reviews
    /// `target`, or the grid's selection when `target` is `None`. With
    /// `docked` set, the review is placed where `Inspector::keyboard_anchor`
    /// places a picker that was opened with its shortcut. Otherwise it is
    /// centred in the window.
    pub fn open_auto_tag(
        &mut self,
        target: Option<picto_library::selection::SelectionTarget>,
        docked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.keyboard_anchor(docked);
        self.ai_tagger
            .update(cx, |tagger, cx| tagger.open(anchor, target, window, cx));
    }

    // The functions below build the chips of the Folders section and the Tags
    // section, and the menu of a chip.

    /// Builds the Folders section: one chip for each folder, in the folder's
    /// colour, followed by the "+" button.
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
                || format!("{} {}", t("Folder"), folder.0),
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

    /// Builds the Tags section: one chip for each tag, followed by the "+"
    /// button. With `sort` set, which is used for a single item, the chips are
    /// ordered by tag group and then by name.
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

    /// Opens the menu of a chip at `position`. A tag's menu has the rows that
    /// every tag menu has (`crate::tags::common_menu`) and Remove Tag. A
    /// folder's menu has Open Folder and Remove.
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
            Some(ChipMenu::Tag(tag)) => match crate::tags::run_common(action, None, &tag, cx) {
                Some(Common::Filter) => self.show_tag_items(tag.tag_id, cx),
                None if action == "remove" => self.set_tag(name(&tag), false, false, cx),
                // The remaining actions belong to rows about tag groups, which
                // a chip's menu does not have.
                _ => {}
            },
            Some(ChipMenu::Folder(folder)) => match action {
                // The grid goes to the folder's destination.
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

    // The functions below save source addresses, and ask before notes or
    // addresses of several items are replaced.

    /// Saves `urls` as the source addresses of the items. When several items
    /// are selected and some of them already have addresses, it first asks
    /// whether to replace them.
    pub(super) fn set_sources(&mut self, urls: Vec<String>, cx: &mut Context<Self>) {
        match self.shown {
            Mode::Item => self.edit(
                Touches::Text,
                move |library, target| library.set_source_urls(target, urls, crate::now_ms()),
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

    /// Opens the dialog that asks whether to replace the notes or addresses
    /// that some of the `count` selected items already have.
    pub(super) fn ask(&mut self, overwrite: Overwrite, count: u64, cx: &mut Context<Self>) {
        // With `Return::Stay`, the keyboard focus is not moved back to what had
        // it before the dialog opened when the dialog closes.
        self.confirm
            .open(Confirm { overwrite, count }, menu::Return::Stay, cx);
    }

    /// Writes the notes or addresses to every selected item.
    pub(super) fn overwrite(&mut self, overwrite: Overwrite, cx: &mut Context<Self>) {
        match overwrite {
            Overwrite::Notes(notes) => self.edit(
                Touches::Text,
                move |library, target| library.set_notes(target, Some(notes), crate::now_ms()),
                cx,
            ),
            Overwrite::Sources(urls) => self.edit(
                Touches::Text,
                move |library, target| library.set_source_urls(target, urls, crate::now_ms()),
                cx,
            ),
        }
    }

    /// Builds the dialog that asks before notes or addresses are replaced.
    /// Returns `None` while the dialog is closed.
    pub(super) fn confirm_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let confirm = self.confirm.layer(window, cx)?;
        // The dialog has the keyboard focus while it is open. The focus is
        // taken here, while the frame is being built, so it takes effect only
        // in the next frame. Another frame is requested so that the next frame
        // is drawn.
        if !confirm.focus.contains_focused(window, cx) && !confirm.presence.closing() {
            window.focus(&confirm.focus, cx);
            window.request_animation_frame();
        }
        // The title and the button's label contain the English name of the
        // field in every language, and the message exists in English only.
        // Picto 0.6.19 and earlier showed the same texts.
        let (title, label, what) = match confirm.content.overwrite {
            Overwrite::Notes(_) => (
                crate::tf!("Overwrite {value0}?", "notes"),
                // i18n: the field's name, which is English in every language
                crate::tf!("Overwrite {value0}", "Notes"),
                "notes",
            ),
            Overwrite::Sources(_) => (
                crate::tf!("Overwrite {value0}?", "sources"),
                // i18n: the field's name, which is English in every language
                crate::tf!("Overwrite {value0}", "Sources"),
                "source URLs",
            ),
        };
        let message = format!(
            "This will replace existing {what} on all {} selected items. Are you sure?",
            crate::i18n::format::grouped(confirm.content.count)
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
