//! Floating panels: everything laid over the window beside the control
//! that opened it (the library switcher, a day picker's calendar, a
//! select's list, a tag field's suggestions, the smart-folder dialog's icon
//! and colour popovers, the inspector's list of sources, the tag and folder
//! pickers' panel). One rule places them ([`place`]) and one element
//! ([`Floating`]) lays them over the window on their layer, with the
//! backdrop or outside press that closes them and, when they take the
//! keyboard, Escape and a Tab that stays inside. [`Popover`] is the glass
//! panel the library switcher opens, built on them.
use super::text_input;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;
use std::time::Instant;

actions!(popover, [Close]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("Popover"))]);
}

/// Which side of its anchor a panel opens on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    /// Under the anchor, their left edges lined up.
    Below,
    /// Above the anchor, their left edges lined up.
    Above,
    /// Left of the anchor, their tops lined up.
    Left,
    /// On the anchor: their top-left corners together, or centred on it.
    Over { centred: bool },
}

/// When a panel below (or above) its anchor goes to the other side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flip {
    Never,
    /// When the panel does not fit between the anchor's edge and the
    /// window's margin on its own side, as `DatePickerButton` measures it
    /// (from the anchor's edge, leaving out the gap). A panel that fits is
    /// left where it is.
    WhenNoRoom,
    /// When `needs` does not fit below and there is more room above than
    /// below (`CmSelect`'s `flipUp`), for a panel that prefers below.
    ToRoomierSide {
        needs: f32,
    },
}

/// How a panel is placed by its anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub side: Side,
    /// The space between the anchor and the panel.
    pub gap: f32,
    /// The least space kept between the panel and the window's edges.
    pub margin: f32,
    pub flip: Flip,
    /// Whether the panel is moved inward to keep `margin` from the window's
    /// edges. A panel too big for the window keeps the margin at its left
    /// and top. Production leaves a select's list and a tag field's
    /// suggestions where they fall.
    pub clamp: bool,
    /// How much of the window's top the panel keeps clear of besides the
    /// margin (the titlebar, for a panel dragged about).
    pub top: f32,
}

impl Placement {
    /// On `side`, `gap` from the anchor and kept `margin` inside the
    /// window, never flipped.
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
    /// Left where it falls, however near the window's edges.
    pub const fn unclamped(mut self) -> Self {
        self.clamp = false;
        self
    }
    /// Kept clear of the window's top `top` deep, besides the margin.
    pub const fn below_top(mut self, top: f32) -> Self {
        self.top = top;
        self
    }
}

/// The top-left corner, in window coordinates, of a panel of `panel`'s
/// size placed by `anchor` (a control's bounds, or a point as bounds of no
/// size) in a window of `window`'s size.
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
    // Whether the panel fits where it opened and stays put.
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

/// What closes a floating panel from outside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dismissal {
    /// Nothing outside it.
    Never,
    /// A press outside the panel, which still reaches what was pressed, as
    /// a window listener lets it. A press on `keep` (the control that
    /// opened the panel, which toggles it itself) is left alone.
    Outside(Bounds<Pixels>),
    /// A clear backdrop over the whole window (for a panel placed
    /// [`Floating::at`] a corner): a press on it, and with `right` a right
    /// press, closes the panel and reaches nothing else, and nothing under
    /// it scrolls.
    Backdrop { right: bool },
}

/// Where a floating panel is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Position {
    /// Where the element is laid out, moved back inside the window by GPUI
    /// should it leave it.
    InPlace,
    /// At this corner, moved back inside the window by GPUI should it
    /// leave it.
    Anchored(Point<f32>),
    /// At exactly this corner, in a layer over the whole window.
    At(Point<f32>),
}

/// A panel laid over the window on `layer` (`ui::layer`), drawn where it
/// is laid out until it is given a corner.
#[derive(IntoElement)]
pub struct Floating {
    id: ElementId,
    layer: usize,
    position: Position,
    dismiss: Dismissal,
    on_dismiss: Option<Handler>,
    keyboard: Option<FocusHandle>,
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
        children: Vec::new(),
    }
}

impl Floating {
    /// At `corner`, which [`place`] has kept inside the window.
    pub fn at(mut self, corner: Point<f32>) -> Self {
        self.position = Position::At(corner);
        self
    }
    /// At `corner`, for a panel whose height is known only once it lays
    /// out: GPUI moves it back inside the window should it leave it.
    pub fn anchored(mut self, corner: Point<f32>) -> Self {
        self.position = Position::Anchored(corner);
        self
    }
    /// What closes the panel from outside it, and what closing does.
    pub fn dismiss(
        mut self,
        dismiss: Dismissal,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.dismiss = dismiss;
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
    /// The panel holds `focus`, which its owner gives the keyboard to
    /// (directly or through a field inside): Escape closes it, even from
    /// its own text field, and Tab and Shift-Tab stay among its controls.
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
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
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
                    .on_action(move |_: &Close, window, cx| handler(window, cx))
                    // Escape closes the panel even from its text field,
                    // before the field would take it to cancel the edit.
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
                let viewport = window.viewport_size();
                let mut layer = div()
                    .id(named("backdrop"))
                    .w(viewport.width)
                    .h(viewport.height);
                if let (Dismissal::Backdrop { right }, Some(handler)) =
                    (self.dismiss, self.on_dismiss)
                {
                    let click = handler.clone();
                    layer = layer
                        .occlude()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            click(window, cx);
                        })
                        .on_scroll(|_, _, cx| cx.stop_propagation());
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

/// The scale a popover is drawn at, `elapsed` after it opened: 0.96 growing
/// to 1 over `--duration-menu-in` on `--ease-scale-in`.
fn scale(elapsed: std::time::Duration) -> f32 {
    motion::menu_in(elapsed)
}

/// One showing of a floating panel that takes the keyboard: when it
/// opened, and where the keyboard was before, to hand it back when the
/// panel closes.
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

    /// Gives the keyboard back to where it was, unless something outside
    /// the panel has taken it since.
    pub fn finish(self, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
        if let Some(restore) = self.restore
            && (focus.contains_focused(window, cx) || window.focused(cx).is_none())
        {
            window.focus(&restore, cx);
        }
    }
}

/// A panel's width in this window: `width`, or less to keep `margin` each
/// side, so the owner can lay out its rows.
pub fn width(width: f32, margin: f32, window: &Window) -> f32 {
    width
        .min(f32::from(window.viewport_size().width) - 2. * margin)
        .max(0.)
}

/// A glass popover by the control that summons it, as
/// `LibrarySwitcherPopover` is. It closes on a press anywhere outside it and
/// its control, which still reaches what was pressed, and on Escape even
/// while its own text field has the keyboard.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    focus: FocusHandle,
    since: Instant,
    /// The control the popover belongs to, in window coordinates.
    control: Bounds<Pixels>,
    placement: Placement,
    /// The size the popover is placed by: its width, and the height it is
    /// kept inside the window by.
    size: Size<f32>,
    max_height: f32,
    on_dismiss: Handler,
    children: Vec<AnyElement>,
}

/// A popover by `control`, `size.width` wide (less in a narrow window) and
/// up to `max_height` tall. `focus` is the handle the owner gives the
/// keyboard to; Escape within it, or a press outside, calls `on_dismiss`.
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
        // `.floatingGlassSurface::after`: the glass with its edges, behind
        // the rows. GPUI cannot scale the panel's contents, so the opening
        // scale is carried by this surface alone, grown from the top-left
        // corner as `transform-origin` puts it.
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
                        // `.content`: the rows, clipped to the panel's corners.
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

    /// The library switcher: 8px under its control, kept 8px inside the
    /// window, its top no lower than 430px above the window's bottom.
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
        // Too short a window for the whole panel under the control: it
        // rises, but never above the margin.
        let short = place(control, size(w, h), window(1200., 500.), LIBRARY);
        assert_eq!(short.y, 70.);
        let shorter = place(control, size(w, h), window(1200., 300.), LIBRARY);
        assert_eq!(shorter.y, 8.);
        // A control near the right edge pulls the panel left.
        let right = at(1000., 64., 100., 36.);
        assert_eq!(
            place(right, size(w, h), window(1200., 800.), LIBRARY).x,
            892.
        );
        // Wider than the window: the left margin is kept.
        assert_eq!(
            place(control, size(w, h), window(250., 800.), LIBRARY).x,
            8.
        );
    }

    /// `DatePickerButton`'s calendar: 5px under its button, above it
    /// without room below.
    const CALENDAR: Placement = Placement::new(Side::Below, 5., 8.).flip(Flip::WhenNoRoom);

    #[test]
    fn a_calendar_flips_above_only_when_it_does_not_fit_below() {
        let calendar = size(278., 340.);
        let viewport = window(1000., 800.);
        // Kept 8px inside the right edge, 5px below the button.
        let button = at(900., 100., 140., 26.);
        assert_eq!(
            corner(place(button, calendar, viewport, CALENDAR)),
            (1000. - 278. - 8., 131.)
        );
        // Low in the window: above the button.
        let low = at(20., 700., 140., 26.);
        assert_eq!(
            corner(place(low, calendar, viewport, CALENDAR)),
            (20., 700. - 340. - 5.)
        );
        // It fits when its height fits under the button, the gap left out,
        // and then stays where it opened.
        let snug = at(20., 800. - 8. - 340. - 26., 140., 26.);
        assert_eq!(
            place(snug, calendar, viewport, CALENDAR).y,
            800. - 8. - 340. + 5.
        );
        // A smaller popover (the rules' colour, 230×286) fits below there.
        let middle = at(20., 480., 140., 26.);
        assert_eq!(
            corner(place(middle, size(230., 286.), viewport, CALENDAR)),
            (20., 511.)
        );
        assert_eq!(place(middle, calendar, viewport, CALENDAR).y, 480. - 345.);
        // No room either way: the top margin.
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
        // Three choices need 86px; 400px below is plenty.
        let high = at(500., 100., 130., 26.);
        assert_eq!(
            corner(place(high, list, window(1000., 534.), select(86.))),
            (500., 130.)
        );
        // 50px below is too little, and more is above: the list's bottom
        // 4px over the button.
        let low = at(500., 500., 130., 26.);
        assert_eq!(
            place(low, list, window(1000., 584.), select(86.)).y,
            500. - 4. - 92.
        );
        // Too little either way: below wins unless above is larger.
        let short = at(500., 20., 130., 26.);
        assert_eq!(place(short, list, window(1000., 120.), select(164.)).y, 50.);
        // Never moved sideways, even past the window's edge.
        let edge = at(950., 100., 130., 26.);
        assert_eq!(place(edge, list, window(1000., 534.), select(86.)).x, 950.);
    }

    #[test]
    fn a_panel_opens_left_of_a_point_or_centred() {
        let beside = Placement::new(Side::Left, 12., 12.);
        let view = window(1200., 800.);
        // The inspector's edge at x = 900, a button's top at y = 300.
        let placed = place(spot(900., 300.), size(540., 480.), view, beside);
        assert_eq!(corner(placed), (348., 300.));
        // Too low: it rises to 12px above the bottom; too far left: 12px in.
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
        // Too near the top or the right edge, it stays 12px inside.
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
        // A window too short for the panel: the titlebar still wins.
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
        // `--ease-scale-in` covers most of the way at once.
        assert!(early > 0.98 && early < 1., "{early}");
    }
}
