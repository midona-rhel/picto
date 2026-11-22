//! Places and draws floating panels, which are everything that is laid over
//! the window beside the control that opened it: the library switcher, a
//! date field's calendar, a select's list, a tag field's suggestions, the
//! icon and colour popovers of the smart folder dialog, the inspector's list
//! of sources, and the panel of the tag and folder pickers. [`place`] is the
//! one function that places them. [`Floating`] is the one element that lays
//! them over the window on their layer. It closes a panel on a press on its
//! backdrop or outside it. For a panel that takes the keyboard it also
//! closes the panel on Escape and keeps Tab inside the panel. [`Popover`] is
//! the glass panel that the library switcher opens, and it uses both.
use super::text_input;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;
use std::time::Instant;

use super::keys::{self, Dismiss, Key};

pub fn bind(cx: &mut App) {
    keys::bind(cx, "Popover", &[Key::Escape]);
}

/// Says on which side of its anchor a panel opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    /// The panel opens under the anchor, with its left edge at the anchor's
    /// left edge.
    Below,
    /// The panel opens above the anchor, with its left edge at the anchor's
    /// left edge.
    Above,
    /// The panel opens to the left of the anchor, with its top at the
    /// anchor's top.
    Left,
    /// The panel opens over the anchor: with its top-left corner at the
    /// anchor's top-left corner, or, with `centred`, with its centre at the
    /// anchor's centre.
    Over { centred: bool },
}

/// Says when a panel that opens below its anchor is put above it instead,
/// and when one that opens above is put below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flip {
    Never,
    /// The panel goes to the other side when it does not fit between the
    /// anchor's edge and the window's margin on its own side. The gap
    /// between the anchor and the panel is not counted in this test. A panel
    /// that fits is left where it opened and is not moved up or down to keep
    /// the margin.
    WhenNoRoom,
    /// A panel that opens below goes above when `needs` does not fit below
    /// the anchor and there is more room above than below. A panel that
    /// opens above goes below by the same test with the sides exchanged.
    ToRoomierSide {
        needs: f32,
    },
}

/// Says how a panel is placed beside its anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub side: Side,
    /// The space between the anchor and the panel.
    pub gap: f32,
    /// The least space kept between the panel and the window's edges.
    pub margin: f32,
    pub flip: Flip,
    /// Whether the panel is moved inward to keep `margin` from the window's
    /// edges. A panel that is too big for the window keeps the margin at its
    /// left and at its top. A select's list and a tag field's suggestions
    /// are not moved inward (`unclamped`).
    pub clamp: bool,
    /// The height at the top of the window that the panel stays out of, in
    /// addition to the margin. A panel that the user can drag uses this to
    /// stay below the titlebar.
    pub top: f32,
}

impl Placement {
    /// Returns a placement on `side`, `gap` away from the anchor, that keeps
    /// the panel `margin` inside the window and never puts it on the other
    /// side.
    pub const fn new(side: Side, gap: f32, margin: f32) -> Self {
        Self {
            side,
            gap,
            margin,
            flip: Flip::Never,
            clamp: true,
            top: 0.,
        }
    }
    pub const fn flip(mut self, flip: Flip) -> Self {
        self.flip = flip;
        self
    }
    /// Makes the panel stay where it opens, however near the window's edges
    /// that is.
    pub const fn unclamped(mut self) -> Self {
        self.clamp = false;
        self
    }
    /// Makes the panel stay out of the top `top` pixels of the window, in
    /// addition to the margin.
    pub const fn below_top(mut self, top: f32) -> Self {
        self.top = top;
        self
    }
}

/// Returns the top-left corner, in window coordinates, of a panel of the
/// size `panel` that is placed beside `anchor` in a window of the size
/// `window`. `anchor` is the bounds of a control, or a point given as bounds
/// of no size.
pub fn place(
    anchor: Bounds<Pixels>,
    panel: Size<f32>,
    window: Size<Pixels>,
    placement: Placement,
) -> Point<f32> {
    let Placement {
        side,
        gap,
        margin,
        flip,
        clamp,
        top,
    } = placement;
    let (left, right) = (f32::from(anchor.left()), f32::from(anchor.right()));
    let (upper, lower) = (f32::from(anchor.top()), f32::from(anchor.bottom()));
    let (width, height) = (f32::from(window.width), f32::from(window.height));
    let mut x = match side {
        Side::Below | Side::Above | Side::Over { centred: false } => left,
        Side::Left => left - gap - panel.width,
        Side::Over { centred: true } => left + (right - left - panel.width) / 2.,
    };
    // Whether the panel fits where it opened. Such a panel is not moved up or
    // down afterwards.
    let mut settled = false;
    let mut y = match side {
        Side::Below | Side::Above => {
            let prefers_below = side == Side::Below;
            let room_below = height - lower - margin;
            let flipped = match flip {
                Flip::Never => false,
                Flip::WhenNoRoom if prefers_below => lower + panel.height > height - margin,
                Flip::WhenNoRoom => upper - panel.height < margin,
                Flip::ToRoomierSide { needs } if prefers_below => {
                    needs > room_below && upper > room_below
                }
                Flip::ToRoomierSide { needs } => needs > upper - margin && room_below > upper,
            };
            settled = flip == Flip::WhenNoRoom && !flipped;
            if prefers_below != flipped {
                lower + gap
            } else {
                upper - gap - panel.height
            }
        }
        Side::Left | Side::Over { centred: false } => upper,
        Side::Over { centred: true } => upper + (lower - upper - panel.height) / 2.,
    };
    if clamp {
        x = x.min(width - panel.width - margin).max(margin);
        if !settled {
            y = y.min(height - panel.height - margin).max(top + margin);
        }
    }
    point(x, y)
}

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Says what outside a floating panel closes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dismissal {
    /// Nothing outside the panel closes it.
    Never,
    /// A press outside the panel closes it, and still reaches what was
    /// pressed. A press inside the bounds given here does not close the
    /// panel. They are the bounds of the control that opened the panel,
    /// which opens and closes it itself.
    Outside(Bounds<Pixels>),
    /// A transparent backdrop covers the whole window behind the panel. Only
    /// a panel placed with [`Floating::at`] has one. A press on the backdrop
    /// with the left button, and with `right` also a press with the right
    /// button, closes the panel. The press reaches nothing else, and nothing
    /// under the backdrop scrolls.
    Backdrop { right: bool },
}

/// Says where a floating panel is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Position {
    /// The panel is drawn where the element is laid out. GPUI moves it back
    /// inside the window if it would reach outside.
    InPlace,
    /// The panel is drawn with its top-left corner at this point. GPUI moves
    /// it back inside the window if it would reach outside.
    Anchored(Point<f32>),
    /// The panel is drawn with its top-left corner at exactly this point, in
    /// a layer that covers the whole window.
    At(Point<f32>),
}

/// A panel that is laid over the window on the layer `layer` (see
/// `ui::layer`). It is drawn where it is laid out, unless `at` or `anchored`
/// gives it a corner.
#[derive(IntoElement)]
pub struct Floating {
    id: ElementId,
    layer: usize,
    position: Position,
    dismiss: Dismissal,
    on_dismiss: Option<Handler>,
    keyboard: Option<FocusHandle>,
    dimmed: bool,
    children: Vec<AnyElement>,
}

pub fn floating(id: impl Into<ElementId>, layer: usize) -> Floating {
    Floating {
        id: id.into(),
        layer,
        position: Position::InPlace,
        dismiss: Dismissal::Never,
        on_dismiss: None,
        keyboard: None,
        dimmed: false,
        children: Vec::new(),
    }
}

impl Floating {
    /// Draws the panel with its top-left corner at exactly `corner`. The
    /// caller gets `corner` from [`place`], which keeps the panel inside the
    /// window.
    pub fn at(mut self, corner: Point<f32>) -> Self {
        self.position = Position::At(corner);
        self
    }
    /// Draws the panel with its top-left corner at `corner`. This is for a
    /// panel whose height is known only after it is laid out: GPUI moves the
    /// panel back inside the window if it would reach outside.
    pub fn anchored(mut self, corner: Point<f32>) -> Self {
        self.position = Position::Anchored(corner);
        self
    }
    /// Makes the backdrop of [`Dismissal::Backdrop`] dark, like the backdrop
    /// of a dialog, instead of transparent. It fades in as a dialog's
    /// backdrop does. This is for a panel that opens in the centre of the
    /// window, as a dialog does.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }
    /// Sets what outside the panel closes it, and the function that is
    /// called to close it.
    pub fn dismiss(
        mut self,
        dismiss: Dismissal,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.dismiss = dismiss;
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
    /// Makes the panel one that takes the keyboard. `focus` is the handle
    /// that the owner gives the keyboard focus to, directly or through a
    /// field inside the panel. Escape then closes the panel, also while a
    /// text field inside it has the focus, and Tab and Shift-Tab move only
    /// among the panel's controls.
    pub fn keyboard(mut self, focus: &FocusHandle) -> Self {
        self.keyboard = Some(focus.clone());
        self
    }
}

impl ParentElement for Floating {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Floating {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let named = |name: &'static str| {
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), name.into())
        };
        let mut panel = div().id(named("panel")).occlude().children(self.children);
        if let Some(focus) = &self.keyboard {
            panel = super::focus::scope(panel, focus)
                .track_focus(focus)
                .key_context("Popover");
            if let Some(handler) = self.on_dismiss.clone() {
                let cancel = handler.clone();
                panel = panel
                    .on_action(move |_: &Dismiss, window, cx| handler(window, cx))
                    // Escape closes the panel also while a text field inside
                    // it has the focus. The panel handles the key in the
                    // capture phase, before the field would use it to cancel
                    // the edit.
                    .capture_action(move |_: &text_input::Cancel, window, cx| {
                        cancel(window, cx);
                        cx.stop_propagation();
                    });
            }
        }
        match (self.dismiss, self.on_dismiss.clone()) {
            (Dismissal::Outside(keep), Some(handler)) => {
                panel = panel.on_mouse_down_out(move |event, window, cx| {
                    if !keep.contains(&event.position) {
                        handler(window, cx);
                    }
                });
            }
            (Dismissal::Backdrop { right }, _) => {
                // A press on the panel is not a press on the backdrop.
                panel = panel.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
                if right {
                    panel =
                        panel.on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());
                }
            }
            _ => {}
        }
        let element = match self.position {
            Position::InPlace => deferred(anchored().child(panel)),
            Position::Anchored(corner) => deferred(
                anchored()
                    .position(point(px(corner.x), px(corner.y)))
                    .child(panel),
            ),
            Position::At(corner) => {
                let mut layer = match (self.dismiss, self.on_dismiss.is_some()) {
                    // The layer is the backdrop that dialogs use, transparent
                    // unless `dimmed` is set. It takes every press and every
                    // scroll, so that nothing under it receives them.
                    (Dismissal::Backdrop { .. }, true) => {
                        let strength = if self.dimmed {
                            let opened = *window
                                .use_keyed_state(named("dimmed"), cx, |_, _| Instant::now())
                                .read(cx);
                            let presence = super::modal::Presence::since(opened);
                            if presence.animating() {
                                window.request_animation_frame();
                            }
                            presence.backdrop()
                        } else {
                            0.
                        };
                        super::modal::backdrop(named("backdrop"), strength, None, window)
                    }
                    _ => {
                        let viewport = window.viewport_size();
                        div()
                            .id(named("backdrop"))
                            .w(viewport.width)
                            .h(viewport.height)
                    }
                };
                if let (Dismissal::Backdrop { right }, Some(handler)) =
                    (self.dismiss, self.on_dismiss)
                {
                    let click = handler.clone();
                    layer = layer.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        click(window, cx);
                    });
                    if right {
                        layer = layer.on_mouse_down(MouseButton::Right, move |_, window, cx| {
                            handler(window, cx)
                        });
                    }
                }
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        layer.child(
                            div()
                                .absolute()
                                .left(px(corner.x))
                                .top(px(corner.y))
                                .child(panel),
                        ),
                    ),
                )
            }
        };
        element.with_priority(self.layer)
    }
}

/// Returns the scale a popover is drawn at when `elapsed` has passed since
/// it opened. The scale grows from 0.96 to 1 over `motion::MENU_IN` on the
/// curve `motion::EASE_SCALE_IN`.
fn scale(elapsed: std::time::Duration) -> f32 {
    motion::menu_in(elapsed)
}

/// Holds what is remembered while a floating panel that takes the keyboard
/// is open: the time it opened, and what had the keyboard focus before, so
/// that the focus can go back there when the panel closes.
pub struct Opening {
    since: Instant,
    restore: Option<FocusHandle>,
}

impl Opening {
    pub fn new(window: &Window, cx: &App) -> Self {
        Self {
            since: Instant::now(),
            restore: window.focused(cx),
        }
    }

    /// Gives the keyboard focus back to what had it before the panel opened,
    /// unless something outside the panel has taken the focus since.
    pub fn finish(self, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
        let still_there = focus.contains_focused(window, cx);
        super::menu::host::give_back(self.restore, still_there, window, cx);
    }
}

/// Returns the width a panel has in this window: `width`, or less when the
/// window is too narrow for `width` with `margin` on each side. The owner
/// needs the width to lay out the panel's rows.
pub fn width(width: f32, margin: f32, window: &Window) -> f32 {
    width
        .min(f32::from(window.viewport_size().width) - 2. * margin)
        .max(0.)
}

/// A glass popover beside the control that opens it. The library switcher is
/// one. It closes on a press anywhere outside itself and its control, and
/// the press still reaches what was pressed. It also closes on Escape, also
/// while a text field inside it has the keyboard focus.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    focus: FocusHandle,
    since: Instant,
    /// The bounds of the control that the popover belongs to, in window
    /// coordinates.
    control: Bounds<Pixels>,
    placement: Placement,
    /// The size that the popover is placed with: its width, and the height
    /// that `place` uses to keep it inside the window.
    size: Size<f32>,
    max_height: f32,
    on_dismiss: Handler,
    children: Vec<AnyElement>,
}

/// Returns a popover beside `control` that is `size.width` wide (less in a
/// narrow window) and at most `max_height` tall. `focus` is the handle that
/// the owner gives the keyboard focus to. Escape while the focus is inside
/// the popover, or a press outside it, calls `on_dismiss`.
#[allow(clippy::too_many_arguments)]
pub fn popover(
    id: impl Into<ElementId>,
    opening: &Opening,
    focus: &FocusHandle,
    control: Bounds<Pixels>,
    placement: Placement,
    size: Size<f32>,
    max_height: f32,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> Popover {
    Popover {
        id: id.into(),
        focus: focus.clone(),
        since: opening.since,
        control,
        placement,
        size,
        max_height,
        on_dismiss: Rc::new(on_dismiss),
        children: Vec::new(),
    }
}

impl ParentElement for Popover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let width = width(self.size.width, self.placement.margin, window);
        let corner = place(
            self.control,
            size(width, self.size.height),
            window.viewport_size(),
            self.placement,
        );
        let scale = scale(self.since.elapsed());
        if scale < 1. {
            window.request_animation_frame();
        }
        // The glass surface is drawn behind the rows. GPUI cannot scale the
        // panel's contents, so only this surface is scaled while the popover
        // opens. It grows from the top-left corner.
        let surface = super::glass_panel(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "surface".into()),
            theme::RADIUS_LG,
            cx,
        )
        .absolute()
        .top_0()
        .left_0()
        .w(relative(scale))
        .h(relative(scale));
        let on_dismiss = self.on_dismiss;
        floating(self.id, super::layer::MENU)
            .anchored(corner)
            .keyboard(&self.focus)
            .dismiss(Dismissal::Outside(self.control), move |window, cx| {
                on_dismiss(window, cx)
            })
            .child(
                div()
                    .relative()
                    .w(px(width))
                    .max_h(px(self.max_height))
                    .flex()
                    .flex_col()
                    .font_family(theme::FONT_UI)
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .child(surface)
                    .child(
                        // The rows are clipped to the panel's rounded corners.
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .max_h(px(self.max_height))
                            .min_h_0()
                            .rounded(px(theme::RADIUS_LG))
                            .overflow_hidden()
                            .children(self.children),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Flip, Placement, Side, place, scale};
    use gpui::{Bounds, Pixels, Point, Size, point, px, size};
    use std::time::Duration;

    fn at(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }
    fn spot(x: f32, y: f32) -> Bounds<Pixels> {
        at(x, y, 0., 0.)
    }
    fn window(w: f32, h: f32) -> Size<Pixels> {
        size(px(w), px(h))
    }
    fn corner(point: Point<f32>) -> (f32, f32) {
        (point.x, point.y)
    }

    /// The placement of the library switcher: 8px under its control and kept
    /// 8px inside the window. Its panel is 300 × 422, so its top is at least
    /// 430px above the window's bottom.
    const LIBRARY: Placement = Placement::new(Side::Below, 8., 8.);
    const LIBRARY_PANEL: (f32, f32) = (300., 422.);

    #[test]
    fn a_panel_opens_under_its_control_at_its_left_edge() {
        let (w, h) = LIBRARY_PANEL;
        let control = at(16., 64., 200., 36.);
        let placed = place(control, size(w, h), window(1200., 800.), LIBRARY);
        assert_eq!(corner(placed), (16., 108.));
    }

    #[test]
    fn a_panel_moves_inward_to_keep_its_margin() {
        let (w, h) = LIBRARY_PANEL;
        let control = at(16., 64., 200., 36.);
        // The window is too short for the whole panel under the control. The
        // panel is moved up, but not above the top margin.
        let short = place(control, size(w, h), window(1200., 500.), LIBRARY);
        assert_eq!(short.y, 70.);
        let shorter = place(control, size(w, h), window(1200., 300.), LIBRARY);
        assert_eq!(shorter.y, 8.);
        // For a control near the right edge, the panel is moved left.
        let right = at(1000., 64., 100., 36.);
        assert_eq!(
            place(right, size(w, h), window(1200., 800.), LIBRARY).x,
            892.
        );
        // A panel that is wider than the window keeps the left margin.
        assert_eq!(
            place(control, size(w, h), window(250., 800.), LIBRARY).x,
            8.
        );
    }

    /// The placement of a date field's calendar: 5px under its button, and
    /// above the button when there is no room below.
    const CALENDAR: Placement = Placement::new(Side::Below, 5., 8.).flip(Flip::WhenNoRoom);

    #[test]
    fn a_calendar_flips_above_only_when_it_does_not_fit_below() {
        let calendar = size(278., 340.);
        let viewport = window(1000., 800.);
        // The calendar is kept 8px inside the right edge, and is 5px below
        // the button.
        let button = at(900., 100., 140., 26.);
        assert_eq!(
            corner(place(button, calendar, viewport, CALENDAR)),
            (1000. - 278. - 8., 131.)
        );
        // A button low in the window gets its calendar above it.
        let low = at(20., 700., 140., 26.);
        assert_eq!(
            corner(place(low, calendar, viewport, CALENDAR)),
            (20., 700. - 340. - 5.)
        );
        // The calendar fits when its height, without the gap, fits under the
        // button. It then stays where it opened.
        let snug = at(20., 800. - 8. - 340. - 26., 140., 26.);
        assert_eq!(
            place(snug, calendar, viewport, CALENDAR).y,
            800. - 8. - 340. + 5.
        );
        // A smaller popover of 230 × 286 fits below a button at a height
        // where the calendar goes above.
        let middle = at(20., 480., 140., 26.);
        assert_eq!(
            corner(place(middle, size(230., 286.), viewport, CALENDAR)),
            (20., 511.)
        );
        assert_eq!(place(middle, calendar, viewport, CALENDAR).y, 480. - 345.);
        // With no room below and none above, the calendar starts at the top
        // margin.
        let tall = at(20., 200., 140., 26.);
        assert_eq!(place(tall, calendar, window(1000., 400.), CALENDAR).y, 8.);
    }

    #[test]
    fn a_select_list_opens_upward_only_when_there_is_more_room_above() {
        let select = |needs: f32| {
            Placement::new(Side::Below, 4., 8.)
                .flip(Flip::ToRoomierSide { needs })
                .unclamped()
        };
        let list = size(130., 92.);
        // Three choices need 86px, and there is 400px below.
        let high = at(500., 100., 130., 26.);
        assert_eq!(
            corner(place(high, list, window(1000., 534.), select(86.))),
            (500., 130.)
        );
        // 50px below is too little and there is more room above, so the list
        // opens above, with its bottom 4px over the button.
        let low = at(500., 500., 130., 26.);
        assert_eq!(
            place(low, list, window(1000., 584.), select(86.)).y,
            500. - 4. - 92.
        );
        // With too little room on both sides, the list opens below unless
        // there is more room above.
        let short = at(500., 20., 130., 26.);
        assert_eq!(place(short, list, window(1000., 120.), select(164.)).y, 50.);
        // The list is never moved sideways, also when it reaches past the
        // window's edge.
        let edge = at(950., 100., 130., 26.);
        assert_eq!(place(edge, list, window(1000., 534.), select(86.)).x, 950.);
    }

    #[test]
    fn a_panel_opens_left_of_a_point_or_centred() {
        let beside = Placement::new(Side::Left, 12., 12.);
        let view = window(1200., 800.);
        // The anchor is at the inspector's edge, x = 900, and at the top of
        // a button, y = 300.
        let placed = place(spot(900., 300.), size(540., 480.), view, beside);
        assert_eq!(corner(placed), (348., 300.));
        // A panel that would reach below the window is moved up to 12px
        // above the bottom. A panel that would reach past the left edge
        // starts 12px inside it.
        assert_eq!(
            place(spot(900., 700.), size(340., 480.), view, beside).y,
            308.
        );
        assert_eq!(
            place(spot(200., 100.), size(540., 480.), view, beside).x,
            12.
        );
        let centred = Placement::new(Side::Over { centred: true }, 0., 12.).unclamped();
        let whole = at(0., 0., 1200., 800.);
        assert_eq!(
            corner(place(whole, size(360., 480.), view, centred)),
            (420., 160.)
        );
    }

    #[test]
    fn a_panel_opens_above_its_control_within_the_window() {
        let above = Placement::new(Side::Above, 4., 12.);
        let view = window(1200., 800.);
        let placed = place(spot(420., 600.), size(340., 480.), view, above);
        assert_eq!(corner(placed), (420., 116.));
        // A panel that would reach past the top or the right edge is kept
        // 12px inside the window.
        let placed = place(spot(1000., 300.), size(340., 480.), view, above);
        assert_eq!(corner(placed), (848., 12.));
    }

    #[test]
    fn a_panel_keeps_clear_of_the_titlebar_when_asked() {
        let dragged = Placement::new(Side::Over { centred: false }, 0., 12.).below_top(52.);
        let view = window(1200., 800.);
        let up = place(spot(300., -500.), size(540., 480.), view, dragged);
        assert_eq!(corner(up), (300., 64.));
        let away = place(spot(-50., 900.), size(540., 480.), view, dragged);
        assert_eq!(corner(away), (12., 308.));
        // In a window that is too short for the panel, the panel still
        // starts below the titlebar.
        let short = place(
            spot(300., 300.),
            size(540., 480.),
            window(1200., 400.),
            dragged,
        );
        assert_eq!(short.y, 64.);
    }

    #[test]
    fn a_popover_grows_from_ninety_six_percent() {
        assert_eq!(scale(Duration::ZERO), 0.96);
        assert_eq!(scale(Duration::from_millis(70)), 1.);
        let early = scale(Duration::from_millis(10));
        // `motion::EASE_SCALE_IN` covers most of the change at the start.
        assert!(early > 0.98 && early < 1., "{early}");
    }
}
