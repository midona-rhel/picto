//! The controls that accept or reject the Inbox item being viewed. They are
//! a rounded bar with an Accept button and a Reject button, which this code
//! calls the pill, drawn over the media view and Quick Look. [`Reviewer`]
//! keeps track of the reviews.
//!
//! The view moves on to the next item at once, and the change is saved in
//! the background, because waiting for the save would make every review take
//! most of a second. The next item is found by its id. If the save fails or
//! is cancelled, the view goes back to the item, but only while the view
//! still shows what this review moved it to.
use super::review_art::{self, Art, GLOW_INSET, GLYPH, HALF_WIDTH, HEIGHT, Half, SHADOW_MARGIN};
use crate::i18n::t;
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

/// The view that a review is made in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    QuickLook,
    Detail,
}

/// Where a review moved the view to. `ticket` identifies the review. `moved`
/// is false until the review has moved the view. After that, `item` is the
/// item the view moved to, or `None` when the review closed the view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Destination {
    ticket: u64,
    item: Option<RootId>,
    moved: bool,
}

/// Keeps track of the reviews that are in progress.
#[derive(Default)]
pub struct Reviewer {
    /// The items whose review is still being saved.
    in_flight: HashSet<RootId>,
    /// The items whose review was saved and which the list of results has
    /// not dropped yet.
    leaving: HashSet<RootId>,
    /// Where the latest review moved the view to.
    latest: Option<Destination>,
    tickets: u64,
}

impl Reviewer {
    /// Returns whether `id` is already reviewed: its review is being saved,
    /// or it was saved and the item is about to leave the list.
    pub fn reviewed(&self, id: RootId) -> bool {
        self.in_flight.contains(&id) || self.leaving.contains(&id)
    }

    /// Returns whether the review of `id` may move the view to `item`:
    /// `item` is another item and is not reviewed itself.
    fn open(&self, id: RootId, item: RootId) -> bool {
        item != id && !self.reviewed(item)
    }

    /// Returns the first item after `id` in `items` that the view may move
    /// to, or `Some(None)` when there is none. Returns `None` when `id` is
    /// not in `items`.
    pub fn following(&self, items: &[RootId], id: RootId) -> Option<Option<RootId>> {
        let index = items.iter().position(|item| *item == id)?;
        Some(
            items[index + 1..]
                .iter()
                .copied()
                .find(|item| self.open(id, *item)),
        )
    }

    /// Returns the nearest item before `id` in `items` that the view may
    /// move to. Returns `None` when there is none or `id` is not in `items`.
    pub fn preceding(&self, items: &[RootId], id: RootId) -> Option<RootId> {
        let index = items.iter().position(|item| *item == id)?;
        items[..index]
            .iter()
            .rev()
            .copied()
            .find(|item| self.open(id, *item))
    }

    /// Starts the review of `id` and returns its ticket, the number that
    /// identifies this review in the later calls.
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

    /// Returns whether the review `ticket` may move the view on to `next`,
    /// and records `next` when it may. Only the latest review moves the
    /// view, and only once.
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

    /// Records that the save of the review `ticket` of `id` has ended.
    /// `committed` says whether the change was saved, `listed` whether the
    /// item is still in the list, and `shown` what the view shows now.
    /// Returns the item to show again when the change was not saved and the
    /// view still shows what the review moved it to.
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
        // The review of the last item closes the view, and `destination.item`
        // is then `None`. In that case the view is still where the review left
        // it while it shows no item.
        (destination.moved && shown == destination.item && listed).then_some(id)
    }

    /// Drops the review `ticket` of `id`, which saved nothing because the
    /// next items could not be loaded. The item stays unreviewed.
    pub fn abandon(&mut self, ticket: u64, id: RootId) {
        self.in_flight.remove(&id);
        if self.latest.is_some_and(|latest| latest.ticket == ticket) {
            self.latest = None;
        }
    }

    /// Takes the list's new items. Saved items that are no longer in the
    /// list are forgotten, so they are no longer skipped.
    pub fn items_changed(&mut self, items: &[RootId]) {
        let listed = items.iter().copied().collect::<HashSet<_>>();
        self.leaving.retain(|id| listed.contains(id));
    }
}

/// How long a button flashes after it was pressed.
pub const FLASH: Duration = Duration::from_millis(360);

/// A press of a button that was accepted: which button flashes, and since
/// when.
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

/// The width of the pill: two halves of 64px each and a 1px divider between
/// them. `PILL_BOTTOM` is the distance from the pill to the bottom edge of
/// the window. The pill is centred horizontally in the window.
const PILL_WIDTH: f32 = 2. * HALF_WIDTH + 1.;
const PILL_BOTTOM: f32 = 96.;
/// The space above and below the divider inside the pill.
const DIVIDER_MARGIN: f32 = 9.;
/// When the pill appears, it moves up by this many points into its place,
/// and its background grows from `PILL_SCALE` (96%) to its full size.
const PILL_RISE: f32 = 7.;
const PILL_SCALE: f32 = 0.96;
/// The priority that the pill is drawn at, which is just above that of the
/// dialogs.
const LAYER: usize = crate::ui::layer::REVIEW;
/// How far inside a half's edge the focus ring is drawn.
const RING_INSET: f32 = 3.;
/// The opacity of a half's glow while the pointer is over the half.
const GLOW_HOVERED: f32 = 0.72;

type ReviewHandler = Rc<dyn Fn(Decision, &mut Window, &mut App)>;
type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The pill. It is drawn over everything else while the item being viewed is
/// in the Inbox. The pill positions itself in the window, so the shell does
/// not place it.
#[derive(IntoElement)]
pub struct Controls {
    /// True while a question to the user is open. The controls then take no
    /// input.
    paused: bool,
    flash: Option<Flash>,
    /// The button that shows the keyboard focus ring. A button that was
    /// clicked keeps keyboard focus, and shows the ring once a key is
    /// pressed.
    focus_ring: Option<Decision>,
    on_review: ReviewHandler,
    on_blur: Option<Handler>,
}

/// Builds the pill. `on_review` is called when either button is clicked.
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

    /// Sets the handler of a press outside the pill. The shell uses it to
    /// take keyboard focus away from the pill's buttons.
    pub fn on_blur(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_blur = Some(Rc::new(handler));
        self
    }
}

/// The animated state of one half: whether the pointer is over it, the
/// opacity of its glow, which is the coloured light behind the button
/// (160 ms), the tint of its glyph (140 ms) and the glyph's shadow (160 ms).
/// `flashing` records whether the half was flashing in the last frame, so
/// that when the flash ends the glow goes straight to its resting opacity
/// without a fade.
#[derive(Default)]
struct HalfState {
    hovered: bool,
    glow: motion::Fade,
    tint: motion::Fade,
    shadow: motion::Fade,
    flashing: bool,
}

/// The time at which the pill appeared. Its entry animation runs from then.
struct Entered(Instant);

/// The bitmaps of the pill, one set for each display scale, kept for as long
/// as the app runs. The pill appears and disappears with each Inbox item.
/// Drawing the bitmaps again each time would cost a blur on the drawing
/// thread and would leave the old set in the sprite atlas.
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
        // GPUI cannot scale an element together with its children. Only the
        // glass panel is therefore drawn at the entry scale, about the pill's
        // centre, and the glyphs keep their size.
        let scale = PILL_SCALE + (1. - PILL_SCALE) * appear;
        let glass = ui::glass_panel("inbox-review-glass", HEIGHT / 2. * scale, cx)
            .absolute()
            .left(px(PILL_WIDTH * (1. - scale) / 2.))
            .top(px(HEIGHT * (1. - scale) / 2.))
            .w(px(PILL_WIDTH * scale))
            .h(px(HEIGHT * scale));
        // The divider is a 1px line that fades in over its top fifth and out
        // over its bottom fifth, with a dark line of the same height to its
        // right.
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
        // The pill is drawn above the dialogs (layer 10 of `ui::modal`), so
        // that the backdrop of the question that a reject asks does not cover
        // it.
        deferred(pill).with_priority(LAYER)
    }
}

/// Returns the bitmaps for the window's display scale, and draws them on
/// first use.
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

/// Builds one half of the pill: its glow, the shadow of its glyph, the glyph
/// and its tooltip.
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
            t("Accept"),
            "inbox.accept",
            rgba(theme::REVIEW_ACCEPT_TINT),
        ),
        Half::Reject => (
            "inbox-review-reject",
            1,
            Decision::Reject,
            // i18n: an icon's name
            "x",
            t("Reject"),
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
    // The half is lit while the pointer is over it and the controls are not
    // paused, or while it has the focus ring. The glyph's tint and shadow
    // are also shown while the press flashes.
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
            // When a flash ends, the glow goes straight to its resting opacity
            // instead of fading from the opacity of the flash.
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
    // During the flash the glow is at full opacity, and its brightness goes
    // from twice the normal brightness back to normal on an ease-out curve.
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
    // The focus ring is a 1px line, 3px inside the half's edge, that follows
    // the rounded outer side of the half.
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
    let tip = Tip::new(label).shortcut(shortcut, cx).above();
    let layer = hint.read(cx).layer(&tip, HEIGHT, cx);
    let on_review = controls.on_review.clone();
    let mut element = div()
        .id(id)
        .role(Role::Button)
        .aria_label(match side {
            Half::Accept => t("Accept item"),
            Half::Reject => t("Reject item"),
        })
        .aria_disabled(paused)
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
        // The hover state and the tooltip are kept when a key is pressed
        // while the pointer rests on the half.
        .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
        .on_hover(move |hovered, _, cx| {
            state.update(cx, |state, cx| {
                state.hovered = *hovered;
                cx.notify();
            });
            // The tooltip is shown on a disabled button too.
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
        // The next review skips the item whose review is still being saved.
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
        // The view has been moved on to another item since, so it stays
        // there.
        assert_eq!(
            reviewer.saved(ticket, RootId(1), false, true, Some(RootId(3))),
            None
        );

        // A later review has moved the view, so the failed save of the earlier
        // review does not move it back.
        let early = reviewer.begin(RootId(1));
        reviewer.move_on(early, Some(RootId(2)));
        let late = reviewer.begin(RootId(2));
        reviewer.move_on(late, Some(RootId(3)));
        assert_eq!(
            reviewer.saved(early, RootId(1), false, true, Some(RootId(2))),
            None
        );

        // The review of the last item closed the view. The view is still
        // closed, so it reopens on the item.
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
