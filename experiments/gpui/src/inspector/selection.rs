//! The inspector with several items selected (the multi-select state of
//! `Inspector.tsx`): a stack of the last covers chosen, how many are
//! selected, the notes, sources, folders and tags they all share, a rating
//! that applies to all of them, and their media count and size.
use super::stack::{self, Pose, Stack};
use super::turned::{self, Shape, Turned};
use super::{Inspector, Mode, PREVIEW, Rate, fitted};
use crate::grid::format;
use crate::session::{Report, Reread};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, field::Look, text_input::TextInput};
use gpui::*;
use picto_library::selection::{SelectionSummary, SelectionTarget};
use picto_library::{Library, MutationReceipt, Rating, RootId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A new selection's summary that takes longer than this gives way to the
/// loading state (`useSelectionSummary`'s 250 ms timer); until then the rail
/// keeps what it showed.
const LOADING_AFTER: Duration = Duration::from_millis(250);
/// `.summarySpinner`'s turn.
const SPIN: Duration = Duration::from_millis(650);

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
    /// The selection shown loading, since when, once its summary has taken
    /// longer than [`LOADING_AFTER`].
    loading: Option<(Vec<RootId>, Instant)>,
    waiting: Option<Task<()>>,
    stack: Stack,
    /// Each stacked cover's size, kept while it leaves the selection.
    sizes: HashMap<String, Option<(u32, u32)>>,
    /// The turned cards drawn so far, by cover, with the shape and rail
    /// colour they were drawn for.
    turned: HashMap<String, (Shape, [u8; 3], Turned)>,
    /// Covers whose turned card is being drawn.
    turning: HashSet<String>,
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
            loading: None,
            waiting: None,
            stack: Stack::default(),
            sizes: HashMap::new(),
            turned: HashMap::new(),
            turning: HashSet::new(),
            keys: 0,
        }
    }

    /// Whether the summary on display is the current selection's.
    pub(super) fn ready(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.ids == self.order)
    }

    /// Whether the current selection is shown loading.
    pub(super) fn loading(&self) -> bool {
        !self.ready()
            && self
                .loading
                .as_ref()
                .is_some_and(|(ids, _)| *ids == self.order)
    }

    /// Whether the rail can show the current selection: its summary, or
    /// the loading state.
    pub(super) fn showable(&self) -> bool {
        self.ready() || self.loading()
    }
}

impl Inspector {
    /// Follows the grid's selection, and asks for the summary of a new
    /// multiple selection. The summary shown so far stays until it arrives.
    pub(super) fn selection_changed(&mut self, cx: &mut Context<Self>) {
        let (selected, focus) = {
            let session = self.session.read(cx);
            (
                session.selected_ids(),
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
        let refreshing = |shown: &Option<Shown>, ids: &[RootId]| {
            shown.as_ref().is_some_and(|shown| shown.ids == ids)
        };
        // A summary read again stays up meanwhile; a new selection's shows
        // the loading state if it is slow.
        if !refreshing(&self.selection.shown, &ids) {
            let waited = ids.clone();
            self.selection.waiting = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(LOADING_AFTER).await;
                let _ = this.update(cx, |this, cx| {
                    if this.selection.asked.as_ref() == Some(&waited) {
                        this.selection.loading = Some((waited, Instant::now()));
                        cx.notify();
                    }
                });
            }));
        }
        let target = self.session.read(cx).target_for(ids.clone());
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
                let refreshing = refreshing(&this.selection.shown, &ids);
                if this.selection.order != ids && !refreshing {
                    return;
                }
                this.selection.waiting = None;
                this.selection.loading = None;
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
    fn edit_selection_as(
        &mut self,
        report: Report,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<MutationReceipt>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(ids) = self.selection.shown.as_ref().map(|shown| shown.ids.clone()) else {
            return;
        };
        let target = self.session.read(cx).target_for(ids);
        // The focused item was edited with the rest.
        crate::session::edit_then(
            &self.session,
            report,
            Reread::DETAILS,
            move |library| work(library, &target),
            cx,
            |this, _, cx| this.reread_summary(cx),
        );
    }

    pub(super) fn rate_selection(&mut self, rating: Rating, cx: &mut Context<Self>) {
        self.edit_selection_as(
            Report::failure("Could not change rating"),
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
        self.selection.turning.clear();
        for (_, (_, _, turned)) in self.selection.turned.drain() {
            cx.drop_image(turned.card, None);
            cx.drop_image(turned.mask, None);
        }
        self.thumbnails.update(cx, |thumbnails, _| {
            for holder in STACK_PINS {
                thumbnails.pin(holder, None);
            }
        });
    }

    /// `StackedPreview`: up to six covers, the newest on top and the older
    /// ones under a rail-coloured shade, the turned ones drawn turned
    /// (`turned.rs`).
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
                    .selected_loaded()
                    .into_iter()
                    .find(|item| item.content_hash == card.hash)
                {
                    sizes.insert(card.hash.clone(), item.width.zip(item.height));
                }
            }
            (
                cards
                    .iter()
                    .map(|card| crate::grid::thumbnails::existing(session.thumbnail(&card.hash)))
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
        let device = window.scale_factor();
        let rail = {
            let rail = theme.rail();
            let byte = |value: f32| (value * 255.).round() as u8;
            [byte(rail.b), byte(rail.g), byte(rail.r)]
        };
        // Turned cards no longer in the stack give back their bitmaps.
        let present = cards
            .iter()
            .map(|card| card.hash.clone())
            .collect::<HashSet<_>>();
        let gone = self
            .selection
            .turned
            .keys()
            .filter(|hash| !present.contains(*hash))
            .cloned()
            .collect::<Vec<_>>();
        for hash in gone {
            if let Some((_, _, turned)) = self.selection.turned.remove(&hash) {
                cx.drop_image(turned.card, Some(window));
                cx.drop_image(turned.mask, Some(window));
            }
        }
        for ((card, thumbnail), size) in cards.into_iter().zip(thumbnails).zip(sizes) {
            let resource = Resource::Path(thumbnail.clone().into());
            // `.previewImage`: the thumbnail at its own size, at most 200px.
            let natural = self
                .thumbnails
                .read(cx)
                .size(&resource)
                .map(|(width, height)| (width.round() as u32, height.round() as u32));
            let (width, height) = fitted(natural.or(size));
            let mut shade = theme.rail();
            shade.a = card.shade;
            let turned = if card.pose == Pose::Base {
                None
            } else {
                let shape = Shape {
                    width: width.round(),
                    height: height.round(),
                    scale: card.pose.scale(),
                    degrees: card.pose.degrees(),
                    radius: theme::RADIUS_MD,
                    device,
                };
                self.turned_card(&card.hash, &resource, shape, rail, cx)
            };
            // `.stackItem`
            let item = div()
                .absolute()
                .top(px(card.offset.1))
                .left(px(card.offset.0))
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .opacity(card.opacity);
            let item = match turned {
                // The turned card, scaled from its resting size while it
                // enters.
                Some((image, mask, (card_width, card_height))) => {
                    let grow = card.scale / card.pose.scale();
                    item.child(
                        div()
                            .relative()
                            .flex_none()
                            .w(px(card_width * grow))
                            .h(px(card_height * grow))
                            .child(img(image).size_full())
                            .child(
                                img(mask)
                                    .absolute()
                                    .inset_0()
                                    .size_full()
                                    .opacity(card.shade),
                            ),
                    )
                }
                // A straight card, or a turned one while its bitmap is being
                // drawn.
                None => {
                    let (width, height) = (width * card.scale, height * card.scale);
                    item.child(
                        // `.previewFrame` with `.previewGlass`'s shadow.
                        div()
                            .relative()
                            .w(px(width.round()))
                            .h(px(height.round()))
                            .rounded(px(theme::RADIUS_MD))
                            .overflow_hidden()
                            .shadow(vec![
                                BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_10).into())
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
                                    .border_color(rgba(theme::BLACK_10)),
                            ),
                    )
                }
            };
            container = container.child(item);
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

    /// The bitmaps of a turned card, once drawn for this shape. The first
    /// time a shape is asked for, and the cover is decoded, it is drawn off
    /// the drawing thread; until then the card lies straight. A card drawn
    /// for another shape is shown until the new one is ready.
    fn turned_card(
        &mut self,
        hash: &str,
        resource: &Resource,
        shape: Shape,
        rail: [u8; 3],
        cx: &mut Context<Self>,
    ) -> Option<(Arc<RenderImage>, Arc<RenderImage>, (f32, f32))> {
        let current = self.selection.turned.get(hash);
        let fresh = current.is_some_and(|(drawn, colour, _)| *drawn == shape && *colour == rail);
        if !fresh && !self.selection.turning.contains(hash) {
            // The cover is still loading: the view repaints when it is ready.
            if let Some(cover) = self.thumbnails.read(cx).image(resource) {
                self.selection.turning.insert(hash.to_owned());
                let hash = hash.to_owned();
                let job = cx.background_spawn(async move { turned::turn(&cover, shape, rail) });
                cx.spawn(async move |this, cx| {
                    let drawn = job.await;
                    let _ = this.update(cx, |this, cx| {
                        // The stack was let go while this was drawn.
                        if !this.selection.turning.remove(&hash) {
                            if let Some(drawn) = drawn {
                                cx.drop_image(drawn.card, None);
                                cx.drop_image(drawn.mask, None);
                            }
                            return;
                        }
                        if let Some(drawn) = drawn
                            && let Some((_, _, old)) =
                                this.selection.turned.insert(hash, (shape, rail, drawn))
                        {
                            cx.drop_image(old.card, None);
                            cx.drop_image(old.mask, None);
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        self.selection
            .turned
            .get(hash)
            .map(|(_, _, turned)| (turned.card.clone(), turned.mask.clone(), turned.size))
    }

    /// `.summarySpinner`: a 10px ring open on its right, turning once every
    /// 650 ms while the selection loads.
    fn spinner(&self, window: &mut Window, cx: &App) -> Svg {
        window.request_animation_frame();
        let since = self
            .selection
            .loading
            .as_ref()
            .map_or_else(Instant::now, |(_, since)| *since);
        let turn = since.elapsed().as_secs_f32() / SPIN.as_secs_f32();
        let mut color = theme::theme(cx).text_secondary();
        color.a *= 0.65;
        ui::icon("inspector-spinner", 10., color).with_transformation(Transformation::rotate(
            radians(turn.fract() * std::f32::consts::TAU),
        ))
    }

    /// `.pendingSection` and `.pendingRating`: a 20px line with the spinner
    /// at its end, in place of what the selection shares.
    fn pending(&self, window: &mut Window, cx: &App) -> Div {
        div()
            .min_h(px(20.))
            .flex()
            .items_center()
            .justify_end()
            .child(self.spinner(window, cx))
    }

    /// The multi-select state's column: the summary on display, or while a
    /// new selection's loads, its count and spinners where the shared values
    /// go (`summaryPending`).
    pub(super) fn selection_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let loading = self.selection.loading();
        let Some((summary, selected)) = (if loading {
            Some((None, self.session.read(cx).selection_count()))
        } else {
            self.selection
                .shown
                .as_ref()
                .map(|shown| (shown.summary.clone(), shown.ids.len() as u64))
        }) else {
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
            .child(self.palette(&[], window, cx))
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
            .child(div().mb(px(8.)).child(if loading {
                ui::field::read_only("Notes", cx).into_any_element()
            } else {
                ui::field("selection-notes", &self.selection.notes, Look::Inspector)
                    .into_any_element()
            }))
            .child(self.source((!loading).then_some(&sources[..]), cx));

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
        if loading {
            for title in ["Folders", "Tags"] {
                let pending = self.pending(window, cx).into_any_element();
                panel = panel.child(self.section(title, Some(pending), cx));
            }
        } else {
            let folders = self.folder_chips(&entries, &shared_folders, window, cx);
            panel = panel.child(self.picker_section("Folders", folders, cx));
            let tags = self.tag_chips(tags, false, window, cx);
            panel = panel.child(self.picker_section("Tags", tags, cx));
        }

        let rating = summary
            .as_ref()
            .and_then(|summary| summary.shared_rating)
            .map_or(0, |rating| rating.bitmap_key() as usize);
        let mut properties = if loading {
            let mut properties = div().child(self.pending(window, cx));
            for label in ["Media", "Size"] {
                // `PropertyRow` with `loading`: the spinner as its value.
                properties = properties.child(
                    div()
                        .relative()
                        .child(Self::property(label, String::new(), cx))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .child(self.spinner(window, cx)),
                        ),
                );
            }
            properties
        } else {
            div().child(self.rating(Rate::Selection, rating, cx))
        };
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
        // The whole selection, loaded or not.
        let target = self.session.read(cx).selection_target();
        if let Some(target) = target.filter(|_| count > 0) {
            properties = properties.child(div().mt(px(12.)).child(Self::export_row(
                "selection-export",
                target,
                count,
                cx,
            )));
        }
        panel = panel.child(self.section("Properties", Some(properties.into_any_element()), cx));
        let taggable = summary
            .as_ref()
            .map_or(0, |summary| summary.taggable_root_count);
        let auto_tag = self.auto_tag(
            "selection-auto-tag",
            if summary.is_some() { taggable } else { count },
            taggable > 0,
            window,
            cx,
        );
        panel = panel.child(auto_tag);
        if summary.is_none() && !loading {
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
