//! Draws the inspector when several items are selected. It shows a stack of
//! the covers that were selected last, how many items are selected, the
//! notes, sources, folders and tags that all of them share, a rating that
//! applies to all of them, and their media count and size.
use super::stack::{self, Pose, Stack};
use super::turned::{self, Shape, Turned};
use super::{Inspector, Mode, PREVIEW, Rate, fitted};
use crate::i18n::format;
use crate::i18n::t;
use crate::session::{Report, Reread};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_library::selection::{SelectionSummary, SelectionTarget};
use picto_library::{Library, MutationReceipt, Rating, RootId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the rail waits for the summary of a new selection before it
/// shows the loading state. Until then it keeps what it showed.
const LOADING_AFTER: Duration = Duration::from_millis(250);

/// The names under which the thumbnail cache keeps the covers of the stack
/// decoded. There are seven, because one card can be fading out while six
/// others are shown.
const STACK_PINS: [&str; stack::CARDS + 1] = [
    "inspector-stack-0",
    "inspector-stack-1",
    "inspector-stack-2",
    "inspector-stack-3",
    "inspector-stack-4",
    "inspector-stack-5",
    "inspector-stack-6",
];

/// Holds the summary that the rail shows and the selection it describes.
pub(super) struct Shown {
    pub(super) ids: Vec<RootId>,
    /// The items that an edit made in the rail applies to: the whole
    /// selection that is shown, also the items the grid has not loaded.
    pub(super) target: SelectionTarget,
    /// The summary. It is `None` when reading it failed.
    pub(super) summary: Option<SelectionSummary>,
    /// The key the notes field was set with for this selection.
    key: u64,
}

pub(super) struct SelectionState {
    pub(super) notes: Entity<TextInput>,
    /// The selected items in the order in which they were selected. This
    /// order decides which covers the stack shows.
    order: Vec<RootId>,
    pub(super) shown: Option<Shown>,
    /// The selection whose summary has been asked for and has not arrived.
    asked: Option<Vec<RootId>>,
    /// The selection for which the loading state is shown, and the time the
    /// loading state began. It is set when the summary has taken longer than
    /// [`LOADING_AFTER`].
    loading: Option<(Vec<RootId>, Instant)>,
    waiting: Option<Task<()>>,
    stack: Stack,
    /// The width and height of each cover in the stack. They are kept so
    /// that a card can still be drawn while it fades out, after its item has
    /// left the selection.
    sizes: HashMap<String, Option<(u32, u32)>>,
    /// The turned cards that have been drawn, by the cover's hash, each with
    /// the shape and the rail colour it was drawn for.
    turned: HashMap<String, (Shape, [u8; 3], Turned)>,
    /// The hashes of the covers whose turned card is being drawn.
    turning: HashSet<String>,
    keys: u64,
}

/// Returns the selected items in the order in which they were selected. The
/// items of `previous` that are still selected keep their order. The newly
/// selected items follow in the grid's order, except that the item selected
/// last, `focus`, comes at the end.
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
            notes: cx.new(|cx| {
                TextInput::new(Kind::Inspector, cx)
                    .placeholder("Notes")
                    .commits_on_blur(window, cx)
                    .expanding()
            }),
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

    /// Returns true if the summary that is shown is for the current
    /// selection.
    pub(super) fn ready(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.ids == self.order)
    }

    /// Returns true if the loading state is shown for the current selection.
    pub(super) fn loading(&self) -> bool {
        !self.ready()
            && self
                .loading
                .as_ref()
                .is_some_and(|(ids, _)| *ids == self.order)
    }

    /// Returns true if the rail can draw the current selection, either with
    /// its summary or in the loading state.
    pub(super) fn showable(&self) -> bool {
        self.ready() || self.loading()
    }
}

impl Inspector {
    /// Records the grid's new selection, and asks for the summary if several
    /// items are now selected. The summary shown so far stays until the new
    /// one arrives.
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
    /// it, and it is not shown yet and has not been asked for.
    pub(super) fn ensure_summary(&mut self, cx: &mut Context<Self>) {
        if !self.holds()
            && self.wanted(cx) == Mode::Selection
            && !self.selection.ready()
            && self.selection.asked.as_ref() != Some(&self.selection.order)
        {
            self.read_summary(cx);
        }
    }

    /// Reads the summary of the selection again after something outside the
    /// rail edited the selection, for example a rating key.
    pub fn selection_edited(&mut self, cx: &mut Context<Self>) {
        if self.wanted(cx) == Mode::Selection {
            self.read_summary(cx);
        }
    }

    /// Reads the summary of the current selection with the library's
    /// `selection_summary`, and shows it if the selection is still the same
    /// when it arrives.
    fn read_summary(&mut self, cx: &mut Context<Self>) {
        let ids = self.selection.order.clone();
        let target =
            self.session
                .read(cx)
                .selection_target()
                .unwrap_or_else(|| SelectionTarget::Explicit {
                    root_ids: ids.clone(),
                });
        self.read_summary_of(ids, target, cx);
    }

    /// Reads the summary that is shown again after an edit. This also works
    /// when the rail is pinned to it and the grid has another selection.
    pub(super) fn reread_summary(&mut self, cx: &mut Context<Self>) {
        if let Some(shown) = &self.selection.shown {
            let (ids, target) = (shown.ids.clone(), shown.target.clone());
            self.read_summary_of(ids, target, cx);
        }
    }

    /// Returns the source addresses that every selected item has, which is
    /// an empty list when the items share none. Returns `None` if no summary
    /// is shown.
    pub(super) fn selection_sources(&self) -> Option<Vec<String>> {
        let summary = self.selection.shown.as_ref()?.summary.as_ref()?;
        Some(summary.shared_source_urls.clone().unwrap_or_default())
    }

    fn read_summary_of(
        &mut self,
        ids: Vec<RootId>,
        target: SelectionTarget,
        cx: &mut Context<Self>,
    ) {
        self.selection.asked = Some(ids.clone());
        let refreshing = |shown: &Option<Shown>, ids: &[RootId]| {
            shown.as_ref().is_some_and(|shown| shown.ids == ids)
        };
        // When the summary that is shown is read again, it stays on screen
        // until the new one arrives. For a new selection the loading state
        // is shown if the summary takes longer than `LOADING_AFTER`.
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
        let job = {
            let target = target.clone();
            self.session
                .read(cx)
                .run(move |library| library.selection_summary(&target))
        };
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
                this.selection.shown = Some(Shown {
                    ids,
                    target,
                    summary,
                    key,
                });
                this.sync_pickers(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Applies an edit to every item of the selection that is shown, and
    /// then reads its summary again.
    fn edit_selection_as(
        &mut self,
        report: Report,
        work: impl FnOnce(&Library, &SelectionTarget) -> picto_library::Result<MutationReceipt>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .selection
            .shown
            .as_ref()
            .map(|shown| shown.target.clone())
        else {
            return;
        };
        // The details of the focused item are read again, because it was
        // edited together with the others.
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
            Report::failure(t("Could not change rating")),
            move |library, target| library.set_rating(target, rating),
            cx,
        );
    }

    /// Saves notes that were typed for the whole selection. If some of the
    /// items already have notes, the new notes would replace them, so the
    /// user is asked to confirm first.
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
            // The field shows the notes that the items share again until the
            // user confirms the change.
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
        // The stack is emptied, so that it starts again without cards the
        // next time several items are selected.
        self.selection.stack = Stack::default();
        self.selection.sizes.clear();
        self.selection.turning.clear();
        for (_, (_, _, turned)) in self.selection.turned.drain() {
            crate::atlas::release(turned.card, cx);
            crate::atlas::release(turned.mask, cx);
        }
        self.thumbnails.update(cx, |thumbnails, _| {
            for holder in STACK_PINS {
                thumbnails.pin(holder, None);
            }
        });
    }

    /// Draws the stack of up to six covers. The cover selected last is on
    /// top. The covers under it have a shade in the rail colour over them.
    /// A card that is turned is drawn from the bitmaps that `turned.rs`
    /// makes. Returns `None` if there is no card to draw.
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
        // The stack's area is 80% of the rail's width and 200px high.
        let mut container = div().relative().w(relative(0.8)).h(px(PREVIEW));
        let device = window.scale_factor();
        let rail = {
            let rail = theme.rail();
            let byte = |value: f32| (value * 255.).round() as u8;
            [byte(rail.b), byte(rail.g), byte(rail.r)]
        };
        // The bitmaps of turned cards that are no longer in the stack are
        // released.
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
                crate::atlas::release(turned.card, cx);
                crate::atlas::release(turned.mask, cx);
            }
        }
        for ((card, thumbnail), size) in cards.into_iter().zip(thumbnails).zip(sizes) {
            let resource = Resource::Path(thumbnail.clone().into());
            // The thumbnail is shown at its own size, at most 200px each way.
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
            // Each card is centred in an area of the stack's size, moved by the
            // card's offset.
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
                // The card is drawn from its turned bitmap. The bitmap has the
                // size of the card when it is not moving, so it is scaled
                // while the card enters.
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
                // A card that is not turned is drawn here, and so is a turned
                // card whose bitmap is not drawn yet.
                None => {
                    let (width, height) = (width * card.scale, height * card.scale);
                    item.child(
                        // The frame has rounded corners and a shadow around it.
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
                            // The shade is a layer in the rail colour over the cover.
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(theme::RADIUS_MD))
                                    .bg(shade),
                            )
                            // A 1px ring, black at 10%, is drawn over the edge of
                            // the image.
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

    /// Returns the bitmaps of a turned card, if they have been drawn. When
    /// none has been drawn for this shape and rail colour, and the cover is
    /// decoded, the card is drawn off the drawing thread. Until it is done,
    /// this returns the bitmaps that were drawn for an earlier shape, or
    /// `None`, in which case the caller draws the card without the turn.
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
            // If the cover is not decoded yet, nothing is started here. The
            // view is drawn again when the cover is decoded.
            if let Some(cover) = self.thumbnails.read(cx).image(resource) {
                self.selection.turning.insert(hash.to_owned());
                let hash = hash.to_owned();
                let job = cx.background_spawn(async move { turned::turn(&cover, shape, rail) });
                cx.spawn(async move |this, cx| {
                    let drawn = job.await;
                    let _ = this.update(cx, |this, cx| {
                        // The stack was emptied while the card was being drawn, so
                        // the bitmaps are released and not kept.
                        if !this.selection.turning.remove(&hash) {
                            if let Some(drawn) = drawn {
                                crate::atlas::release(drawn.card, cx);
                                crate::atlas::release(drawn.mask, cx);
                            }
                            return;
                        }
                        if let Some(drawn) = drawn
                            && let Some((_, _, old)) =
                                this.selection.turned.insert(hash, (shape, rail, drawn))
                        {
                            crate::atlas::release(old.card, cx);
                            crate::atlas::release(old.mask, cx);
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

    /// Returns the spinner of a property row. Its rotation is counted from
    /// the time the selection started loading.
    fn spinner(&self, window: &mut Window, cx: &mut App) -> Svg {
        ui::property_row::spinner(self.loading_since(), window, cx)
    }

    /// Returns the time at which the loading state of the selection began,
    /// or the current time if no loading state is shown.
    fn loading_since(&self) -> Instant {
        self.selection
            .loading
            .as_ref()
            .map_or_else(Instant::now, |(_, since)| *since)
    }

    /// Returns a line 20px high with the spinner at its right end. It is
    /// drawn in place of a value that the selection shares while the summary
    /// is loading.
    fn pending(&self, window: &mut Window, cx: &mut App) -> Div {
        div()
            .min_h(px(20.))
            .flex()
            .items_center()
            .justify_end()
            .child(self.spinner(window, cx))
    }

    /// Draws the column for several selected items. It shows the summary, or,
    /// while the summary of a new selection is loading, the number of
    /// selected items and spinners in the places of the shared values.
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
            // The line that says how many items are selected takes the place
            // of the name field.
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
                    .child(t("items selected")),
            )
            .child(div().mb(px(8.)).child(if loading {
                ui::field::read_only(t("Notes"), cx).into_any_element()
            } else {
                ui::field("selection-notes", &self.selection.notes, Look::Inspector)
                    .into_any_element()
            }))
            .child(self.source((!loading).then_some(&sources[..]), cx));

        let (entries, tags) = {
            let session = self.session.read(cx);
            let tags = summary.as_ref().map_or_else(Vec::new, |summary| {
                let mut tags = summary
                    .shared_tags
                    .iter()
                    .filter_map(|id| session.tags.get(&id.0).cloned())
                    .collect::<Vec<_>>();
                // The tags are listed in the order in which the library
                // returns tags that are read by id: by namespace, then by
                // name, then by id.
                crate::tags::sort_as_library(&mut tags);
                tags
            });
            (session.nav.clone(), tags)
        };
        let shared_folders = summary
            .as_ref()
            .map_or_else(Vec::new, |summary| summary.shared_folders.clone());
        if loading {
            for title in [t("Folders"), t("Tags")] {
                let pending = self.pending(window, cx).into_any_element();
                panel = panel.child(self.section(title, Some(pending), cx));
            }
        } else {
            let folders = self.folder_chips(&entries, &shared_folders, window, cx);
            // i18n: the section's English title, which picker_section translates
            panel = panel.child(self.picker_section("Folders", folders, cx));
            let tags = self.tag_chips(tags, false, window, cx);
            // i18n: the section's English title, which picker_section translates
            panel = panel.child(self.picker_section("Tags", tags, cx));
        }

        let rating = summary
            .as_ref()
            .and_then(|summary| summary.shared_rating)
            .map_or(0, |rating| rating.bitmap_key() as usize);
        let mut properties = if loading {
            let mut properties = div().child(self.pending(window, cx));
            for label in [t("Media"), t("Size")] {
                properties = properties
                    .child(Self::property(label, String::new(), cx).loading(self.loading_since()));
            }
            properties
        } else {
            div().child(self.rating(Rate::Selection, rating, cx))
        };
        if let Some(summary) = &summary {
            properties = properties
                .child(Self::property(
                    t("Media"),
                    format::grouped(summary.media_count as u64),
                    cx,
                ))
                .child(Self::property(
                    t("Size"),
                    crate::text::size_to_gigabytes(summary.total_size_bytes as u64),
                    cx,
                ));
        }
        // The export takes the whole selection, also the items the grid has
        // not loaded.
        let target = self.session.read(cx).selection_target();
        if let Some(target) = target.filter(|_| count > 0) {
            properties = properties.child(div().mt(px(12.)).child(Self::export_row(
                "selection-export",
                target,
                count,
                cx,
            )));
        }
        panel = panel.child(self.section(t("Properties"), Some(properties.into_any_element()), cx));
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
            panel = panel.child(Self::status(
                t("Could not load selection details."),
                true,
                cx,
            ));
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
