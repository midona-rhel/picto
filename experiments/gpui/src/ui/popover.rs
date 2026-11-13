//! A popover: a glass panel opened under the control that summons it, as
//! `LibrarySwitcherPopover` is. It closes on a press anywhere outside it and
//! its control, and on Escape even while its own text field has the
//! keyboard. Presses outside are not swallowed: they close the popover and
//! still reach what was pressed, as the Electron window listener lets them.
use super::text_input;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;
use std::time::Instant;

actions!(popover, [Dismiss]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some("Popover"))]);
}

/// How a popover is placed below its control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The panel's width when the window has room for it.
    pub width: f32,
    /// The space between the control's bottom edge and the panel.
    pub gap: f32,
    /// The least space kept between the panel and the window's edges.
    pub margin: f32,
    /// How far above the window's bottom the panel's top may come at most,
    /// so that a full panel stays inside the window.
    pub reserve: f32,
}

/// The panel's left, top and width, in window coordinates, for a control
/// whose bottom-left corner is at `anchor`: under the control, aligned to
/// its left edge, moved inward to keep inside the window.
pub fn place(anchor: Point<f32>, viewport: Size<f32>, placement: Placement) -> (f32, f32, f32) {
    let width = placement
        .width
        .min(viewport.width - 2. * placement.margin)
        .max(0.);
    let top = (anchor.y + placement.gap)
        .min(viewport.height - placement.reserve)
        .max(placement.margin);
    let left = anchor
        .x
        .min(viewport.width - width - placement.margin)
        .max(placement.margin);
    (left, top, width)
}

/// The scale a popover is drawn at, `elapsed` after it opened: 0.96 growing
/// to 1 over `--duration-menu-in` on `--ease-scale-in`.
pub fn scale(elapsed: std::time::Duration) -> f32 {
    motion::menu_in(elapsed)
}

/// One showing of a popover: when it opened and where the keyboard was
/// before, to hand it back when the popover closes.
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
    /// the popover has taken it since.
    pub fn finish(self, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
        if let Some(restore) = self.restore
            && (focus.contains_focused(window, cx) || window.focused(cx).is_none())
        {
            window.focus(&restore, cx);
        }
    }
}

type Dismissal = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    focus: FocusHandle,
    since: Instant,
    /// The control the popover belongs to, in window coordinates. A press
    /// on it is left to the control, which toggles the popover itself.
    control: Bounds<Pixels>,
    placement: Placement,
    max_height: f32,
    on_dismiss: Dismissal,
    children: Vec<AnyElement>,
}

/// A popover under `control`. `focus` is the handle the owner gives the
/// keyboard to (directly or through a field inside the popover); Escape
/// within it, or a press outside, calls `on_dismiss`.
pub fn popover(
    id: impl Into<ElementId>,
    opening: &Opening,
    focus: &FocusHandle,
    control: Bounds<Pixels>,
    placement: Placement,
    max_height: f32,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> Popover {
    Popover {
        id: id.into(),
        focus: focus.clone(),
        since: opening.since,
        control,
        placement,
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

/// The panel's width for this window, so the owner can lay out its rows.
pub fn width(placement: Placement, window: &Window) -> f32 {
    let view = window.viewport_size();
    place(
        point(0., 0.),
        size(f32::from(view.width), f32::from(view.height)),
        placement,
    )
    .2
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let view = window.viewport_size();
        let (left, top, width) = place(
            point(
                f32::from(self.control.left()),
                f32::from(self.control.bottom()),
            ),
            size(f32::from(view.width), f32::from(view.height)),
            self.placement,
        );
        let scale = scale(self.since.elapsed());
        if scale < 1. {
            window.request_animation_frame();
        }
        let (escape, cancel, outside) = (
            self.on_dismiss.clone(),
            self.on_dismiss.clone(),
            self.on_dismiss,
        );
        let control = self.control;
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
        let panel = div()
            .id(self.id)
            .occlude()
            .relative()
            .w(px(width))
            .max_h(px(self.max_height))
            .flex()
            .flex_col()
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .track_focus(&self.focus)
            .key_context("Popover")
            .on_action(move |_: &Dismiss, window, cx| escape(window, cx))
            // Escape closes the popover even from its text field, before the
            // field would take it to cancel the edit.
            .capture_action(move |_: &text_input::Cancel, window, cx| {
                cancel(window, cx);
                cx.stop_propagation();
            })
            .on_mouse_down_out(move |event, window, cx| {
                if !control.contains(&event.position) {
                    outside(window, cx);
                }
            })
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
            );
        deferred(anchored().position(point(px(left), px(top))).child(panel)).with_priority(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{Placement, place, scale};
    use gpui::{point, size};
    use std::time::Duration;

    const LIBRARY: Placement = Placement {
        width: 300.,
        gap: 8.,
        margin: 8.,
        reserve: 430.,
    };

    #[test]
    fn a_popover_opens_under_its_control_at_its_left_edge() {
        let (left, top, width) = place(point(16., 100.), size(1200., 800.), LIBRARY);
        assert_eq!((left, top, width), (16., 108., 300.));
    }

    #[test]
    fn a_popover_keeps_inside_a_small_window() {
        // Narrower than the panel: the panel narrows to keep 8px each side.
        let (left, _, width) = place(point(16., 100.), size(250., 800.), LIBRARY);
        assert_eq!((left, width), (8., 234.));
        // Too short for a full panel under the control: it rises, but never
        // above the margin.
        assert_eq!(place(point(16., 100.), size(1200., 500.), LIBRARY).1, 70.);
        assert_eq!(place(point(16., 100.), size(1200., 300.), LIBRARY).1, 8.);
        // A control near the right edge pulls the panel left.
        assert_eq!(
            place(point(1000., 100.), size(1200., 800.), LIBRARY).0,
            892.
        );
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
