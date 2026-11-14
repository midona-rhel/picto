//! Accepting or rejecting the Inbox item being viewed
//! (`InboxReviewControls.tsx`, `inboxReview.ts`): the Accept and Reject pill
//! over the media view and Quick Look, and the reviewer behind it.
//!
//! The view moves on to the next item at once, chosen by id, and the change
//! is saved in the background: waiting for the save would make every review
//! take most of a second. If the save fails or is cancelled, the view goes
//! back to the item, but only while it is still where this review left it.
use super::review_art::{self, Art, GLOW_INSET, GLYPH, HALF_WIDTH, HEIGHT, Half, SHADOW_MARGIN};
use crate::ui::tooltip::{self, Tip};
use crate::{motion, theme, ui};
use gpui::*;
use picto_library::RootId;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject,
}

/// The view a review happens in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    QuickLook,
    Detail,
}

/// Where a review left the view: nothing until it has moved on, then the
/// item it moved to (none when it closed the view).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Destination {
    ticket: u64,
    item: Option<RootId>,
    moved: bool,
}

/// The reviews in progress (`createInboxReviewer`).
#[derive(Default)]
pub struct Reviewer {
    /// Items whose review is still being saved.
    in_flight: HashSet<RootId>,
    /// Saved items the list has not dropped yet.
    leaving: HashSet<RootId>,
    /// The latest review's destination.
    latest: Option<Destination>,
    tickets: u64,
}

impl Reviewer {
    /// Whether `id` is already reviewed: being saved, or saved and about to
    /// leave the list.
    pub fn reviewed(&self, id: RootId) -> bool {
        self.in_flight.contains(&id) || self.leaving.contains(&id)
    }

    /// An item the review of `id` may land on.
    fn open(&self, id: RootId, item: RootId) -> bool {
        item != id && !self.reviewed(item)
    }

    /// The first open item after `id` in `items`; `None` when `id` is not
    /// listed.
    pub fn following(&self, items: &[RootId], id: RootId) -> Option<Option<RootId>> {
        let index = items.iter().position(|item| *item == id)?;
        Some(
            items[index + 1..]
                .iter()
                .copied()
                .find(|item| self.open(id, *item)),
        )
    }

    /// The nearest open item before `id` in `items`.
    pub fn preceding(&self, items: &[RootId], id: RootId) -> Option<RootId> {
        let index = items.iter().position(|item| *item == id)?;
        items[..index]
            .iter()
            .rev()
            .copied()
            .find(|item| self.open(id, *item))
    }

    /// Starts reviewing `id`; the ticket names this review from now on.
    pub fn begin(&mut self, id: RootId) -> u64 {
        self.in_flight.insert(id);
        self.tickets += 1;
        self.latest = Some(Destination {
            ticket: self.tickets,
            item: None,
            moved: false,
        });
        self.tickets
    }

    /// Whether the review `ticket` moves the view on to `next`: only the
    /// latest review does, and only once.
    pub fn move_on(&mut self, ticket: u64, next: Option<RootId>) -> bool {
        match &mut self.latest {
            Some(destination) if destination.ticket == ticket && !destination.moved => {
                destination.item = next;
                destination.moved = true;
                true
            }
            _ => false,
        }
    }

    /// The save of the review `ticket` of `id` is over. `listed` says
    /// whether the item is still in the list, `shown` what the view shows
    /// now. Hands back the item to show again when the save did not happen
    /// and the view is still where the review left it.
    pub fn saved(
        &mut self,
        ticket: u64,
        id: RootId,
        committed: bool,
        listed: bool,
        shown: Option<RootId>,
    ) -> Option<RootId> {
        self.in_flight.remove(&id);
        if committed {
            if listed {
                self.leaving.insert(id);
            }
            return None;
        }
        let destination = self.latest.filter(|latest| latest.ticket == ticket)?;
        self.latest = None;
        // The last item closes the view, so "still there" is "still closed".
        (destination.moved && shown == destination.item && listed).then_some(id)
    }

    /// The review `ticket` of `id` was dropped before it saved anything
    /// (the next items could not be loaded): the item stays, unreviewed.
    pub fn abandon(&mut self, ticket: u64, id: RootId) {
        self.in_flight.remove(&id);
        if self.latest.is_some_and(|latest| latest.ticket == ticket) {
            self.latest = None;
        }
    }

    /// The list changed: saved items that have left it stop being skipped.
    pub fn items_changed(&mut self, items: &[RootId]) {
        let listed = items.iter().copied().collect::<HashSet<_>>();
        self.leaving.retain(|id| listed.contains(id));
    }
}

/// `FLASH_DURATION_MS`, `reviewFlash`.
pub const FLASH: Duration = Duration::from_millis(360);

/// A press that was taken: which button flashes, and since when.
#[derive(Clone, Copy)]
pub struct Flash {
    pub decision: Decision,
    pub since: Instant,
}

impl Flash {
    pub fn over(&self) -> bool {
        self.since.elapsed() >= FLASH
    }
}

/// `.pill`: 129 wide (two 64px halves and a 1px divider), 96px above the
/// window's bottom edge, centred on the window.
const PILL_WIDTH: f32 = 2. * HALF_WIDTH + 1.;
const PILL_BOTTOM: f32 = 96.;
/// `.divider`'s `margin-block: 9px`.
const DIVIDER_MARGIN: f32 = 9.;
/// `reviewPillIn`: it rises the last 7px into place from 96%.
const PILL_RISE: f32 = 7.;
const PILL_SCALE: f32 = 0.96;
/// The pill's drawing layer: just above the dialogs'.
const LAYER: usize = 11;
/// `outline-offset: -3px`.
const RING_INSET: f32 = 3.;
/// `.glow`'s hovered opacity.
const GLOW_HOVERED: f32 = 0.72;

type ReviewHandler = Rc<dyn Fn(Decision, &mut Window, &mut App)>;
type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The pill, drawn over everything while the item being viewed is in the
/// Inbox. The shell positions nothing: the pill places itself in the window.
#[derive(IntoElement)]
pub struct Controls {
    /// A question is open: the controls take no input.
    paused: bool,
    flash: Option<Flash>,
    /// The button that has the keyboard's focus ring (`:focus-visible`): a
    /// clicked button keeps the focus, and shows it once a key is pressed.
    focus_ring: Option<Decision>,
    on_review: ReviewHandler,
    on_blur: Option<Handler>,
}

/// The pill. `on_review` hears a click on either button.
pub fn controls(
    paused: bool,
    flash: Option<Flash>,
    on_review: impl Fn(Decision, &mut Window, &mut App) + 'static,
) -> Controls {
    Controls {
        paused,
        flash,
        focus_ring: None,
        on_review: Rc::new(on_review),
        on_blur: None,
    }
}

impl Controls {
    pub fn focus_ring(mut self, focus_ring: Option<Decision>) -> Self {
        self.focus_ring = focus_ring;
        self
    }

    /// A press outside the pill, which takes the focus from its buttons.
    pub fn on_blur(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_blur = Some(Rc::new(handler));
        self
    }
}

/// One half's moving state: its hover, the light (`.glow` opacity, 160 ms),
/// the glyph's tint (140 ms) and shadow (160 ms), and the flash it showed
/// last, so the light snaps back when the flash ends, as production's
/// remounted `.glow` does.
#[derive(Default)]
struct HalfState {
    hovered: bool,
    glow: motion::Fade,
    tint: motion::Fade,
    shadow: motion::Fade,
    flashing: bool,
}

/// When the pill appeared, for `reviewPillIn`.
struct Entered(Instant);

/// The drawn bitmaps, one set per display scale, kept for the app's life:
/// the pill comes and goes with each Inbox item, and drawing them again
/// each time would cost a blur on the drawing thread and leave the old set
/// in the sprite atlas.
#[derive(Default)]
struct ArtCache(Vec<Rc<Art>>);

impl Global for ArtCache {}

impl RenderOnce for Controls {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let art = art(window, cx);
        let entered =
            window.use_keyed_state("inbox-review-entered", cx, |_, _| Entered(Instant::now()));
        let appear = motion::EASE_DEFAULT.ease(motion::progress(
            entered.read(cx).0.elapsed(),
            motion::REVIEW_PILL_IN,
        ));
        if appear < 1. {
            window.request_animation_frame();
        }
        let viewport = window.viewport_size();
        let left = (f32::from(viewport.width) - PILL_WIDTH) / 2.;
        let top = f32::from(viewport.height) - PILL_BOTTOM - HEIGHT + PILL_RISE * (1. - appear);
        // GPUI scales no boxes, so only the glass is drawn at the entry
        // scale, about the pill's centre; the glyphs keep their size.
        let scale = PILL_SCALE + (1. - PILL_SCALE) * appear;
        let glass = ui::glass_panel("inbox-review-glass", HEIGHT / 2. * scale, cx)
            .absolute()
            .left(px(PILL_WIDTH * (1. - scale) / 2.))
            .top(px(HEIGHT * (1. - scale) / 2.))
            .w(px(PILL_WIDTH * scale))
            .h(px(HEIGHT * scale));
        // `.divider`: a 1px line fading in over its top fifth and out over
        // its bottom fifth, with a dark line beside it (`box-shadow: 1px 0`).
        let line = HEIGHT - 2. * DIVIDER_MARGIN;
        let divider = |from: u32, to: u32, share: f32| {
            div().w_full().h(px(line * share)).bg(linear_gradient(
                180.,
                linear_color_stop(rgba(from), 0.),
                linear_color_stop(rgba(to), 1.),
            ))
        };
        let divider_line = div()
            .absolute()
            .left(px(HALF_WIDTH))
            .top(px(DIVIDER_MARGIN))
            .w(px(1.))
            .h(px(line))
            .flex()
            .flex_col()
            .child(divider(
                theme::REVIEW_DIVIDER_EDGE,
                theme::REVIEW_DIVIDER_TOP,
                0.2,
            ))
            .child(divider(
                theme::REVIEW_DIVIDER_TOP,
                theme::REVIEW_DIVIDER_BOTTOM,
                0.6,
            ))
            .child(divider(
                theme::REVIEW_DIVIDER_BOTTOM,
                theme::REVIEW_DIVIDER_EDGE,
                0.2,
            ));
        let divider_shade = div()
            .absolute()
            .left(px(HALF_WIDTH + 1.))
            .top(px(DIVIDER_MARGIN))
            .w(px(1.))
            .h(px(HEIGHT - 2. * DIVIDER_MARGIN))
            .bg(rgba(theme::REVIEW_DIVIDER_SHADE));
        let on_blur = self.on_blur.clone();
        let pill = div()
            .id("inbox-review-controls")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(PILL_WIDTH))
            .h(px(HEIGHT))
            .opacity(appear)
            .on_mouse_down_out(move |_, window, cx| {
                if let Some(on_blur) = &on_blur {
                    on_blur(window, cx);
                }
            })
            .child(glass)
            .child(half(Half::Accept, &self, &art, window, cx))
            .child(divider_line)
            .child(divider_shade)
            .child(half(Half::Reject, &self, &art, window, cx));
        // Over the dialogs (`ui::modal`'s layer 10), so it stays clear of
        // the backdrop of the question a reject asks.
        deferred(pill).with_priority(LAYER)
    }
}

/// The bitmaps for the window's display scale, drawn on first use.
fn art(window: &mut Window, cx: &mut App) -> Rc<Art> {
    let scale = window.scale_factor();
    let cache = cx.default_global::<ArtCache>();
    if let Some(art) = cache.0.iter().find(|art| art.scale == scale) {
        return art.clone();
    }
    let color = |token: u32| {
        let color = rgb(token);
        [color.r, color.g, color.b]
    };
    let art = Rc::new(Art::draw(
        scale,
        [
            color(theme::REVIEW_ACCEPT_GLOW),
            color(theme::REVIEW_REJECT_GLOW),
        ],
    ));
    cache.0.push(art.clone());
    art
}

/// One half of the pill: its light, its glyph's shadow and glyph, and its
/// tooltip.
fn half(
    side: Half,
    controls: &Controls,
    art: &Art,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let (id, index, decision, icon, label, shortcut, tint) = match side {
        Half::Accept => (
            "inbox-review-accept",
            0,
            Decision::Accept,
            "check",
            "Accept",
            "inbox.accept",
            rgba(theme::REVIEW_ACCEPT_TINT),
        ),
        Half::Reject => (
            "inbox-review-reject",
            1,
            Decision::Reject,
            "x",
            "Reject",
            "inbox.reject",
            rgba(theme::REVIEW_REJECT_TINT),
        ),
    };
    let state = window.use_keyed_state(id, cx, |_, _| HalfState::default());
    let hint = window.use_keyed_state(
        ElementId::NamedChild(std::sync::Arc::new(id.into()), "tip".into()),
        cx,
        |_, _| tooltip::Hover::default(),
    );
    let flash = controls
        .flash
        .filter(|flash| flash.decision == decision && !flash.over());
    let paused = controls.paused;
    let ring = controls.focus_ring == Some(decision);
    // `:hover:not(:disabled)` and `:focus-visible`, and `.chosen` while the
    // press flashes.
    let (glow, lit, shade, flashing) = state.update(cx, |state, _| {
        let hovered = state.hovered && !paused || ring;
        state.glow.set(hovered, motion::REVIEW_GLOW);
        state
            .tint
            .set(hovered || flash.is_some(), motion::REVIEW_TINT);
        state
            .shadow
            .set(hovered || flash.is_some(), motion::REVIEW_GLOW);
        if state.flashing && flash.is_none() {
            // The flash's light is a fresh element: the one after it starts
            // at rest rather than fading.
            state.glow = motion::Fade::at(hovered);
        }
        state.flashing = flash.is_some();
        (
            state.glow.value_with(motion::EASE),
            state.tint.value_with(motion::EASE),
            state.shadow.value_with(motion::EASE),
            state.flashing,
        )
    });
    let animating = {
        let state = state.read(cx);
        state.glow.animating() || state.tint.animating() || state.shadow.animating()
    };
    if animating || flashing || hint.read(cx).animating() {
        window.request_animation_frame();
    }
    // The flash shows the light at full strength, from `brightness(2)` back
    // to none on `ease-out`.
    let (opacity, step) = match flash {
        Some(flash) => {
            let progress = motion::progress(flash.since.elapsed(), FLASH);
            let brightness = 2. - motion::EASE_OUT.ease(progress);
            (1., review_art::flash_step(brightness))
        }
        None => (GLOW_HOVERED * glow, 0),
    };
    let size = HALF_WIDTH - 2. * GLOW_INSET;
    let light = (opacity > 0.).then(|| {
        div()
            .absolute()
            .left(px(GLOW_INSET))
            .top(px(GLOW_INSET))
            .w(px(size))
            .h(px(HEIGHT - 2. * GLOW_INSET))
            .opacity(opacity)
            .child(img(art.glow[index][step].clone()).size_full())
    });
    let glyph_left = (HALF_WIDTH - GLYPH) / 2.;
    let glyph_top = (HEIGHT - GLYPH) / 2.;
    let shadow = div()
        .absolute()
        .left(px(glyph_left - SHADOW_MARGIN))
        .top(px(glyph_top - SHADOW_MARGIN))
        .size(px(GLYPH + 2. * SHADOW_MARGIN))
        .child(img(art.shadow[index][review_art::shadow_step(shade)].clone()).size_full());
    let glyph = div()
        .absolute()
        .left(px(glyph_left))
        .top(px(glyph_top))
        .child(ui::icon(icon, GLYPH, theme::blend(theme.text(), tint, lit)));
    // `outline: 1px solid rgba(255, 255, 255, 0.75); outline-offset: -3px`,
    // following the half's rounded outer side.
    let ring = ring.then(|| {
        let round = HEIGHT / 2. - RING_INSET;
        let outline = div()
            .absolute()
            .left(px(RING_INSET))
            .top(px(RING_INSET))
            .w(px(HALF_WIDTH - 2. * RING_INSET))
            .h(px(HEIGHT - 2. * RING_INSET))
            .border_1()
            .border_color(rgba(theme::REVIEW_FOCUS_RING));
        match side {
            Half::Accept => outline.rounded_l(px(round)),
            Half::Reject => outline.rounded_r(px(round)),
        }
    });
    let tip = Tip::new(label).shortcut(shortcut, cx).above().over(LAYER);
    let layer = hint.read(cx).layer(&tip, HEIGHT, cx);
    let on_review = controls.on_review.clone();
    let mut element = div()
        .id(id)
        .absolute()
        .left(px(match side {
            Half::Accept => 0.,
            Half::Reject => HALF_WIDTH + 1.,
        }))
        .top_0()
        .w(px(HALF_WIDTH))
        .h(px(HEIGHT))
        .occlude()
        .children(light)
        .child(shadow)
        .child(glyph)
        .children(ring)
        // CSS `:hover` and the tooltip stay through a key press under a
        // still pointer.
        .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
        .on_hover(move |hovered, _, cx| {
            state.update(cx, |state, cx| {
                state.hovered = *hovered;
                cx.notify();
            });
            // The tooltip shows on a disabled button too.
            hint.update(cx, |hint, cx| hint.set(*hovered, cx));
        })
        .children(layer);
    if !paused {
        element = element
            .cursor_pointer()
            .on_click(move |_, window, cx| on_review(decision, window, cx));
    }
    element
}

#[cfg(test)]
mod tests {
    use super::Reviewer;
    use picto_library::RootId;

    fn ids(values: &[u32]) -> Vec<RootId> {
        values.iter().map(|value| RootId(*value)).collect()
    }

    #[test]
    fn moves_on_past_items_already_reviewed() {
        let items = ids(&[1, 2, 3, 4]);
        let mut reviewer = Reviewer::default();
        let first = reviewer.begin(RootId(2));
        assert_eq!(reviewer.following(&items, RootId(2)), Some(Some(RootId(3))));
        assert!(reviewer.move_on(first, Some(RootId(3))));
        // The same review moves the view only once.
        assert!(!reviewer.move_on(first, Some(RootId(4))));
        // The next review skips the item still saving.
        reviewer.begin(RootId(3));
        assert_eq!(reviewer.following(&items, RootId(3)), Some(Some(RootId(4))));
        assert_eq!(reviewer.preceding(&items, RootId(3)), Some(RootId(1)));
        assert_eq!(reviewer.following(&items, RootId(9)), None);
    }

    #[test]
    fn at_the_last_item_the_review_falls_back_then_closes() {
        let mut reviewer = Reviewer::default();
        let items = ids(&[1, 2]);
        reviewer.begin(RootId(2));
        assert_eq!(reviewer.following(&items, RootId(2)), Some(None));
        assert_eq!(reviewer.preceding(&items, RootId(2)), Some(RootId(1)));
        reviewer.begin(RootId(1));
        assert_eq!(reviewer.following(&items, RootId(1)), Some(None));
        assert_eq!(reviewer.preceding(&items, RootId(1)), None);
    }

    #[test]
    fn a_failed_save_goes_back_only_while_the_view_is_where_it_was_left() {
        let mut reviewer = Reviewer::default();
        let ticket = reviewer.begin(RootId(1));
        reviewer.move_on(ticket, Some(RootId(2)));
        assert_eq!(
            reviewer.saved(ticket, RootId(1), false, true, Some(RootId(2))),
            Some(RootId(1))
        );
        assert!(!reviewer.reviewed(RootId(1)));

        let ticket = reviewer.begin(RootId(1));
        reviewer.move_on(ticket, Some(RootId(2)));
        // The view was moved on since: it stays.
        assert_eq!(
            reviewer.saved(ticket, RootId(1), false, true, Some(RootId(3))),
            None
        );

        // A later review owns the view.
        let early = reviewer.begin(RootId(1));
        reviewer.move_on(early, Some(RootId(2)));
        let late = reviewer.begin(RootId(2));
        reviewer.move_on(late, Some(RootId(3)));
        assert_eq!(
            reviewer.saved(early, RootId(1), false, true, Some(RootId(2))),
            None
        );

        // The last item closed the view; still closed, it reopens.
        let mut reviewer = Reviewer::default();
        let ticket = reviewer.begin(RootId(1));
        reviewer.move_on(ticket, None);
        assert_eq!(
            reviewer.saved(ticket, RootId(1), false, true, None),
            Some(RootId(1))
        );
    }

    #[test]
    fn saved_items_are_skipped_until_the_list_drops_them() {
        let mut reviewer = Reviewer::default();
        let ticket = reviewer.begin(RootId(1));
        reviewer.move_on(ticket, Some(RootId(2)));
        assert_eq!(
            reviewer.saved(ticket, RootId(1), true, true, Some(RootId(2))),
            None
        );
        assert!(reviewer.reviewed(RootId(1)));
        reviewer.items_changed(&ids(&[1, 2]));
        assert!(reviewer.reviewed(RootId(1)));
        reviewer.items_changed(&ids(&[2]));
        assert!(!reviewer.reviewed(RootId(1)));
    }

    #[test]
    fn an_abandoned_review_leaves_the_item_unreviewed() {
        let mut reviewer = Reviewer::default();
        let ticket = reviewer.begin(RootId(5));
        reviewer.abandon(ticket, RootId(5));
        assert!(!reviewer.reviewed(RootId(5)));
        assert!(!reviewer.move_on(ticket, None));
    }
}
