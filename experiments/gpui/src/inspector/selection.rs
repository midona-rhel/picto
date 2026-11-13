//! The inspector with several items selected (the multi-select state of
//! `Inspector.tsx`): a stack of the last covers chosen, how many are
//! selected, the notes, sources, folders and tags they all share, a rating
//! that applies to all of them, and their media count and size.
use super::stack::{self, Stack};
use super::{Inspector, Mode, PREVIEW, Rate, fitted};
use crate::grid::format;
use crate::session::Reread;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, field::Look, text_input::TextInput};
use gpui::*;
use picto_library::selection::{SelectionSummary, SelectionTarget};
use picto_library::{Library, MutationReceipt, Rating, RootId};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// A stack card may be leaving while six others show.
const STACK_PINS: [&str; stack::CARDS + 1] = [
    "inspector-stack-0",
    "inspector-stack-1",
    "inspector-stack-2",
    "inspector-stack-3",
    "inspector-stack-4",
    "inspector-stack-5",
    "inspector-stack-6",
];

/// The summary on display and the selection it describes.
pub(super) struct Shown {
    pub(super) ids: Vec<RootId>,
    /// `None` when reading the summary failed.
    pub(super) summary: Option<SelectionSummary>,
    /// The key the notes field was set with for this selection.
    key: u64,
}

pub(super) struct SelectionState {
    pub(super) notes: Entity<TextInput>,
    /// The selected items in the order they were chosen, which decides the
    /// covers the stack shows.
    order: Vec<RootId>,
    pub(super) shown: Option<Shown>,
    /// The selection whose summary is on its way.
    asked: Option<Vec<RootId>>,
    stack: Stack,
    /// Each stacked cover's size, kept while it leaves the selection.
    sizes: HashMap<String, Option<(u32, u32)>>,
    keys: u64,
}

/// The selection in the order it was chosen: what was selected before keeps
/// its place, and what was added follows in grid order with the item last
/// chosen at the end.
fn chosen_order(previous: &[RootId], selected: &[RootId], focus: Option<RootId>) -> Vec<RootId> {
    let selected_set = selected.iter().copied().collect::<HashSet<_>>();
    let mut order = previous
        .iter()
        .copied()
        .filter(|id| selected_set.contains(id))
        .collect::<Vec<_>>();
    let kept = order.iter().copied().collect::<HashSet<_>>();
    let mut added = selected
        .iter()
        .copied()
        .filter(|id| !kept.contains(id))
        .collect::<Vec<_>>();
    if let Some(focus) = focus
        && let Some(position) = added.iter().position(|id| *id == focus)
    {
        let focus = added.remove(position);
        added.push(focus);
    }
    order.extend(added);
    order
}

impl SelectionState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Inspector>) -> Self {
        Self {
            notes: cx.new(|cx| TextInput::expanding("Notes", window, cx)),
            order: Vec::new(),
            shown: None,
            asked: None,
            stack: Stack::default(),
            sizes: HashMap::new(),
            keys: 0,
        }
    }

    /// Whether the summary on display is the current selection's.
    pub(super) fn ready(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.ids == self.order)
    }
}

impl Inspector {
    /// Follows the grid's selection, and asks for the summary of a new
    /// multiple selection. The summary shown so far stays until it arrives.
    pub(super) fn selection_changed(&mut self, cx: &mut Context<Self>) {
        let (selected, focus) = {
            let session = self.session.read(cx);
            (
                session
                    .selection
                    .iter()
                    .filter_map(|index| session.items.get(*index))
                    .map(|item| item.root_id)
                    .collect::<Vec<_>>(),
                session.selected_item().map(|item| item.root_id),
            )
        };
        self.selection.order = chosen_order(&self.selection.order, &selected, focus);
        self.ensure_summary(cx);
    }

    /// Asks for the summary of the current selection if the rail is to show
    /// it and it is neither shown nor on its way.
    pub(super) fn ensure_summary(&mut self, cx: &mut Context<Self>) {
        if !self.holds()
            && self.wanted(cx) == Mode::Selection
            && !self.selection.ready()
            && self.selection.asked.as_ref() != Some(&self.selection.order)
        {
            self.read_summary(cx);
        }
    }

    /// Reads the selection's summary again after it was edited from outside
    /// the rail, such as by a rating key.
    pub fn selection_edited(&mut self, cx: &mut Context<Self>) {
        if self.wanted(cx) == Mode::Selection {
            self.read_summary(cx);
        }
    }

    /// Reads the summary of the current selection (`selection_summary`, as
    /// `getTargetSelectionSummary` does), and shows it if the selection is
    /// still the same when it arrives.
    fn read_summary(&mut self, cx: &mut Context<Self>) {
        let ids = self.selection.order.clone();
        self.read_summary_of(ids, cx);
    }

    /// Reads the summary shown again after an edit, even when the rail is
    /// pinned to it and the grid has another selection.
    pub(super) fn reread_summary(&mut self, cx: &mut Context<Self>) {
        if let Some(ids) = self.selection.shown.as_ref().map(|shown| shown.ids.clone()) {
            self.read_summary_of(ids, cx);
        }
    }

    /// The source addresses every selected item shares, or none.
    pub(super) fn selection_sources(&self) -> Option<Vec<String>> {
        let summary = self.selection.shown.as_ref()?.summary.as_ref()?;
        Some(summary.shared_source_urls.clone().unwrap_or_default())
    }

    fn read_summary_of(&mut self, ids: Vec<RootId>, cx: &mut Context<Self>) {
        self.selection.asked = Some(ids.clone());
        let target = SelectionTarget::Explicit {
            root_ids: ids.clone(),
        };
        let job = self
            .session
            .read(cx)
            .run(move |library| library.selection_summary(&target));
        cx.spawn(async move |this, cx| {
            let Ok(result) = job.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.selection.asked.as_ref() == Some(&ids) {
                    this.selection.asked = None;
                }
                let refreshing = this
                    .selection
                    .shown
                    .as_ref()
                    .is_some_and(|shown| shown.ids == ids);
                if this.selection.order != ids && !refreshing {
                    return;
                }
                if let Err(error) = &result {
                    eprintln!("inspector: reading the selection failed: {error}");
                }
                let summary = result.ok();
                let key = match &this.selection.shown {
                    Some(shown) if shown.ids == ids => shown.key,
                    _ => {
                        this.selection.keys += 1;
                        this.selection.keys
                    }
                };
                let notes = summary
                    .as_ref()
                    .and_then(|summary| summary.shared_notes.clone())
                    .unwrap_or_default();
                this.selection
                    .notes
                    .update(cx, |field, cx| field.set_field(key, notes, cx));
                this.selection.shown = Some(Shown { ids, summary, key });
                this.sync_pickers(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Applies an edit to every item in the selection shown, then reads its
    /// summary again.
    fn edit_selection(
        &mut self,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<MutationReceipt>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(ids) = self.selection.shown.as_ref().map(|shown| shown.ids.clone()) else {
            return;
        };
        let target = SelectionTarget::Explicit { root_ids: ids };
        // The focused item was edited with the rest.
        let job = self.session.update(cx, |session, cx| {
            session.edit(move |library| work(library, &target), Reread::DETAILS, cx)
        });
        cx.spawn(async move |this, cx| {
            if job.await.is_ok() {
                let _ = this.update(cx, |this, cx| this.reread_summary(cx));
            }
        })
        .detach();
    }

    pub(super) fn rate_selection(&mut self, rating: Rating, cx: &mut Context<Self>) {
        self.edit_selection(
            move |library, target| library.set_rating(target, rating),
            cx,
        );
    }

    /// Notes typed for the whole selection. Items that already have notes
    /// would lose them, which the Electron app confirms first.
    pub(super) fn commit_selection_notes(
        &mut self,
        key: u64,
        notes: String,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = self
            .selection
            .shown
            .as_ref()
            .filter(|shown| shown.key == key)
        else {
            return;
        };
        let (has_notes, shared) =
            shown
                .summary
                .as_ref()
                .map_or((false, String::new()), |summary| {
                    (
                        summary.has_notes,
                        summary.shared_notes.clone().unwrap_or_default(),
                    )
                });
        if has_notes && notes != shared {
            // The field shows the notes the items share until the change is
            // confirmed (`confirmSelectionOverwrite`).
            let count = shown
                .summary
                .as_ref()
                .map_or(shown.ids.len() as u64, |summary| summary.selected_count);
            self.selection
                .notes
                .update(cx, |field, cx| field.set_field(key, shared, cx));
            self.ask(super::Overwrite::Notes(notes), count, cx);
            return;
        }
        self.overwrite(super::Overwrite::Notes(notes), cx);
    }

    pub(super) fn release_stack(&mut self, cx: &mut Context<Self>) {
        if self.selection.stack.is_empty() {
            return;
        }
        // The stack starts over the next time several items are selected.
        self.selection.stack = Stack::default();
        self.selection.sizes.clear();
        self.thumbnails.update(cx, |thumbnails, _| {
            for holder in STACK_PINS {
                thumbnails.pin(holder, None);
            }
        });
    }

    /// `StackedPreview`: up to six covers, the newest on top and the older
    /// ones under a rail-coloured shade. GPUI cannot rotate an image, so the
    /// turned cards keep their size and motion but lie straight.
    fn stack(
        &mut self,
        sample: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let now = Instant::now();
        self.selection.stack.show(sample, now);
        let cards = self.selection.stack.placed(now);
        if self.selection.stack.animating(now) {
            window.request_animation_frame();
        }
        if cards.is_empty() {
            return None;
        }
        let theme = theme::theme(cx);
        let (thumbnails, sizes) = {
            let session = self.session.read(cx);
            let sizes = &mut self.selection.sizes;
            for card in &cards {
                if let Some(item) = session
                    .selection
                    .iter()
                    .filter_map(|index| session.items.get(*index))
                    .find(|item| item.content_hash == card.hash)
                {
                    sizes.insert(card.hash.clone(), item.width.zip(item.height));
                }
            }
            (
                cards
                    .iter()
                    .map(|card| session.thumbnail(&card.hash))
                    .collect::<Vec<_>>(),
                cards
                    .iter()
                    .map(|card| sizes.get(&card.hash).copied().flatten())
                    .collect::<Vec<_>>(),
            )
        };
        self.thumbnails.update(cx, |cache, _| {
            for (index, holder) in STACK_PINS.into_iter().enumerate() {
                let thumbnail = thumbnails
                    .get(index)
                    .map(|path| Resource::Path(path.clone().into()));
                cache.pin(holder, thumbnail);
            }
        });
        // `.stackContainer`: 80% of the rail, 200px tall.
        let mut container = div().relative().w(relative(0.8)).h(px(PREVIEW));
        for ((card, thumbnail), size) in cards.into_iter().zip(thumbnails).zip(sizes) {
            let (width, height) = fitted(size);
            let (width, height) = (width * card.scale, height * card.scale);
            let mut shade = theme.rail();
            shade.a = card.shade;
            container = container.child(
                // `.stackItem`
                div()
                    .absolute()
                    .top(px(card.offset.1))
                    .left(px(card.offset.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(card.opacity)
                    .child(
                        // `.previewFrame` with `.previewGlass`'s shadow.
                        div()
                            .relative()
                            .w(px(width.round()))
                            .h(px(height.round()))
                            .rounded(px(theme::RADIUS_MD))
                            .overflow_hidden()
                            .shadow(vec![
                                BoxShadow::new(px(0.), px(0.), rgba(0x0000001a).into())
                                    .blur_radius(px(5.)),
                            ])
                            .child(
                                img(thumbnail)
                                    .image_cache(&self.thumbnails)
                                    .size_full()
                                    .rounded(px(theme::RADIUS_MD))
                                    .object_fit(ObjectFit::Cover),
                            )
                            // `.stackShade`
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(theme::RADIUS_MD))
                                    .bg(shade),
                            )
                            // The glass edge: a 1px ring over the image.
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(theme::RADIUS_MD))
                                    .border_1()
                                    .border_color(rgba(0x0000001a)),
                            ),
                    ),
            );
        }
        Some(
            div()
                .w_full()
                .min_h(px(PREVIEW))
                .mb(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .child(container),
        )
    }

    /// The multi-select state's column.
    pub(super) fn selection_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some((summary, selected)) = self
            .selection
            .shown
            .as_ref()
            .map(|shown| (shown.summary.clone(), shown.ids.len() as u64))
        else {
            return self.scroller();
        };
        let theme = theme::theme(cx);
        let sample = summary
            .as_ref()
            .map_or_else(Vec::new, |summary| summary.sample_hashes.clone());
        let mut panel = Self::panel();
        if let Some(stack) = self.stack(&sample, window, cx) {
            panel = panel.child(stack);
        }
        let count = summary
            .as_ref()
            .map_or(selected, |summary| summary.selected_count);
        let sources = summary
            .as_ref()
            .and_then(|summary| summary.shared_source_urls.clone())
            .unwrap_or_default();
        panel = panel
            .child(self.palette(&[], cx))
            // `.selectionNameField`
            .child(
                div()
                    .w_full()
                    .h(px(40.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .whitespace_nowrap()
                    .text(theme::TEXT_MD)
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(
                        div()
                            .text_color(theme.primary())
                            .child(format::grouped(count)),
                    )
                    .child("items selected"),
            )
            .child(div().mb(px(8.)).child(ui::field(
                "selection-notes",
                &self.selection.notes,
                Look::Inspector,
            )))
            .child(self.source(Some(&sources), window, cx));

        let (entries, tags) = {
            let session = self.session.read(cx);
            let tags = summary.as_ref().map_or_else(Vec::new, |summary| {
                summary
                    .shared_tags
                    .iter()
                    .filter_map(|id| session.tags.get(&id.0).cloned())
                    .collect::<Vec<_>>()
            });
            (session.nav.clone(), tags)
        };
        let shared_folders = summary
            .as_ref()
            .map_or_else(Vec::new, |summary| summary.shared_folders.clone());
        let folders = self.folder_chips(&entries, &shared_folders, window, cx);
        panel = panel.child(self.picker_section("Folders", folders, cx));
        let tags = self.tag_chips(tags, false, window, cx);
        panel = panel.child(self.picker_section("Tags", tags, cx));

        let rating = summary
            .as_ref()
            .and_then(|summary| summary.shared_rating)
            .map_or(0, |rating| rating.bitmap_key() as usize);
        let mut properties = div().child(self.rating(Rate::Selection, rating, cx));
        if let Some(summary) = &summary {
            properties = properties
                .child(Self::property(
                    "Media",
                    format::grouped(summary.media_count as u64),
                    cx,
                ))
                .child(Self::property(
                    "Size",
                    crate::text::size_to_gigabytes(summary.total_size_bytes as u64),
                    cx,
                ));
        }
        if count > 0 {
            // TODO: Export opens the export dialog for the selected items.
            properties = properties.child(
                div().mt(px(12.)).child(
                    ui::action_row("selection-export", "inspector-export", "Export")
                        .glyph(16.)
                        .height(theme::CONTROL_MD),
                ),
            );
        }
        panel = panel.child(self.section("Properties", Some(properties.into_any_element()), cx));
        if summary.is_none() {
            panel = panel.child(Self::status("Could not load selection details.", true, cx));
        }
        self.scroller().child(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::chosen_order;
    use picto_library::RootId;

    fn ids(values: &[u32]) -> Vec<RootId> {
        values.iter().copied().map(RootId).collect()
    }

    #[test]
    fn items_added_later_come_after_those_chosen_before() {
        let order = chosen_order(&ids(&[5]), &ids(&[1, 5, 9]), Some(RootId(1)));
        assert_eq!(order, ids(&[5, 9, 1]));
    }

    #[test]
    fn items_taken_out_of_the_selection_leave_the_order() {
        let order = chosen_order(&ids(&[5, 9, 1]), &ids(&[1, 5]), Some(RootId(5)));
        assert_eq!(order, ids(&[5, 1]));
    }

    #[test]
    fn a_new_selection_starts_from_grid_order() {
        let order = chosen_order(&ids(&[7]), &ids(&[2, 3, 4]), Some(RootId(3)));
        assert_eq!(order, ids(&[2, 4, 3]));
    }
}
