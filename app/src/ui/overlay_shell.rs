//! Draws the floating selection panel that the tag picker and the folder
//! picker open in. It is a glass panel beside the inspector, or in the
//! centre of the window, over a transparent backdrop that closes it on a
//! click or a right click. The user drags the panel by its 40px header or
//! footer, once the pointer has moved 5px. Escape closes the panel. While
//! the panel is pinned it has no backdrop, so the rest of the window can be
//! used.
//!
//! This file also holds the panel's small controls: the header buttons, the
//! compact view tabs and the key hints. The panel's check box is the compact
//! size of [`super::check::check_box`].
use super::popover::{self, Dismissal, Placement, Side, floating};
use super::text_input;
use super::tooltip::Tip;
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

use super::keys::{self, Confirm, Dismiss, Down, Key, Left, NextView, Right, Up};

pub fn bind(cx: &mut App) {
    keys::bind(
        cx,
        "OverlayShell",
        &[
            Key::Escape,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Enter,
            Key::Tab,
        ],
    );
}

/// A key that the panel passes to its owner. The key can be pressed while
/// the keyboard focus is in the search field or anywhere else in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    /// The Tab key, which shows the next view of the panel's rail.
    Next,
}

/// The panel stays at least this far inside the window.
const MARGIN: f32 = 12.;
/// How far the pointer must move after a press on the header or footer
/// before the panel is dragged.
const DRAG_THRESHOLD: f32 = 5.;
const HEADER: f32 = 40.;
const FOOTER: f32 = 40.;

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
type MoveHandler = Rc<dyn Fn(Geometry, &mut Window, &mut App) + 'static>;
/// Returns whether the owner used the key. A key that the owner did not use
/// goes on to the search field, where the left and right arrows move the
/// caret.
type NavHandler = Rc<dyn Fn(Nav, &mut Window, &mut App) -> bool + 'static>;

/// Says where the panel is. A panel that was opened beside the inspector has
/// `right` set and keeps its right edge there, so that a change of its width
/// moves its left edge. Every other panel has no `right` and keeps its left
/// edge at `left`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub right: Option<f32>,
    pub left: f32,
    pub top: f32,
}

impl Geometry {
    /// Returns the panel's left edge when the panel is `width` wide.
    pub fn left(&self, width: f32) -> f32 {
        self.right.map_or(self.left, |right| right - width)
    }
}

/// Says where the panel opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Opens {
    /// The panel's right edge is 12px to the left of this point's x, which
    /// is the inspector's left edge, and its top is at the point's y.
    Beside(Point<f32>),
    /// The panel's top-left corner is at this point.
    Below(Point<f32>),
    /// The panel's bottom is 4px above this point, and its left edge is at
    /// the point's x.
    Above(Point<f32>),
    /// The panel is in the centre of the window.
    Centred,
}

impl Opens {
    /// Returns `Beside(anchor)`, or `Centred` when there is no anchor.
    pub fn beside(anchor: Option<Point<f32>>) -> Self {
        anchor.map_or(Opens::Centred, Opens::Beside)
    }
}

/// Returns where a panel of `width` × `height` opens. The panel is kept 12px
/// inside the window, except a centred panel, which is not moved.
pub fn place(opens: Opens, viewport: Size<Pixels>, width: f32, height: f32) -> Geometry {
    let spot = |at: Point<f32>| Bounds::new(point(px(at.x), px(at.y)), Size::default());
    let (anchor, placement) = match opens {
        Opens::Beside(at) => (spot(at), Placement::new(Side::Left, MARGIN, MARGIN)),
        Opens::Below(at) => (spot(at), Placement::new(Side::Below, 0., MARGIN)),
        Opens::Above(at) => (spot(at), Placement::new(Side::Above, 4., MARGIN)),
        Opens::Centred => (
            Bounds::new(point(px(0.), px(0.)), viewport),
            Placement::new(Side::Over { centred: true }, 0., MARGIN).unclamped(),
        ),
    };
    let corner = popover::place(anchor, size(width, height), viewport, placement);
    let (left, top) = (corner.x.round(), corner.y.round());
    match opens {
        // A panel beside the inspector is given its right edge, so that a
        // change of its width moves its left edge.
        Opens::Beside(_) => {
            let edge = (corner.x + width).round();
            Geometry {
                right: Some(edge),
                left: edge - width,
                top,
            }
        }
        _ => Geometry {
            right: None,
            left,
            top,
        },
    }
}

/// Returns where the panel is after a drag by `delta` from `start`. The
/// panel is kept 12px inside the window and below the titlebar, so that it
/// never covers the titlebar's buttons.
fn dragged(
    start: Geometry,
    delta: Point<f32>,
    viewport: Size<Pixels>,
    width: f32,
    height: f32,
) -> Geometry {
    let moved = point(px(start.left(width) + delta.x), px(start.top + delta.y));
    let corner = popover::place(
        Bounds::new(moved, Size::default()),
        size(width, height),
        viewport,
        Placement::new(Side::Over { centred: false }, 0., MARGIN).below_top(theme::TITLEBAR_HEIGHT),
    );
    Geometry {
        right: start.right.map(|_| corner.x + width),
        left: corner.x,
        top: corner.y,
    }
}

/// Holds a drag of the header or footer: where the pointer was pressed,
/// where the panel was then, and whether the pointer has moved past the
/// threshold.
#[derive(Clone, Copy)]
struct Drag {
    from: Point<Pixels>,
    start: Geometry,
    active: bool,
}

#[derive(IntoElement)]
pub struct OverlayShell {
    id: ElementId,
    geometry: Geometry,
    width: f32,
    height: f32,
    pinned: bool,
    dimmed: bool,
    focus: FocusHandle,
    header: Vec<AnyElement>,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    on_close: Handler,
    on_move: Option<MoveHandler>,
    on_nav: Option<NavHandler>,
    layer: usize,
}

/// Returns a panel at `geometry` that is `width` × `height`. `focus` is the
/// handle that has the keyboard focus while nothing inside the panel has it,
/// so that Escape reaches the panel.
pub fn overlay_shell(
    id: impl Into<ElementId>,
    geometry: Geometry,
    width: f32,
    height: f32,
    focus: &FocusHandle,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
) -> OverlayShell {
    OverlayShell {
        id: id.into(),
        geometry,
        width,
        height,
        pinned: false,
        dimmed: false,
        focus: focus.clone(),
        header: Vec::new(),
        body: Vec::new(),
        footer: Vec::new(),
        on_close: Rc::new(on_close),
        on_move: None,
        on_nav: None,
        layer: super::layer::PANEL,
    }
}

impl OverlayShell {
    /// Sets the layer that the panel is drawn on. It is `ui::layer::PANEL`
    /// unless this is called. A panel that a dialog opened is drawn on
    /// `ui::layer::OVER_DIALOG`, above the dialogs.
    pub fn layer(mut self, layer: usize) -> Self {
        self.layer = layer;
        self
    }
    /// Sets whether the panel is pinned. A pinned panel has no backdrop, so
    /// a click outside it does not close it.
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }
    /// Sets whether the panel's backdrop darkens the window, as the backdrop
    /// of a dialog does. A pinned panel has no backdrop.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    pub fn header(mut self, child: impl IntoElement) -> Self {
        self.header.push(child.into_any_element());
        self
    }
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.body.push(child.into_any_element());
        self
    }
    pub fn footer(mut self, child: impl IntoElement) -> Self {
        self.footer.push(child.into_any_element());
        self
    }
    /// Sets the handler of the arrow keys, Enter and Tab, pressed anywhere
    /// in the panel. The handler returns whether it used the key.
    pub fn on_nav(
        mut self,
        handler: impl Fn(Nav, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_nav = Some(Rc::new(handler));
        self
    }
    /// Sets the handler that is called with the panel's new place while the
    /// user drags the header or footer.
    pub fn on_move(mut self, handler: impl Fn(Geometry, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Rc::new(handler));
        self
    }
}

/// Makes `strip`, which is the header or the footer, start a drag of the
/// panel when it is pressed.
fn grab(strip: Div, drag: Rc<Cell<Option<Drag>>>, geometry: Geometry) -> Div {
    strip
        .cursor(CursorStyle::OpenHand)
        .on_mouse_down(MouseButton::Left, move |event, _, _| {
            drag.set(Some(Drag {
                from: event.position,
                start: geometry,
                active: false,
            }));
        })
}

impl RenderOnce for OverlayShell {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let view = window.viewport_size();
        let drag = window
            .use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "drag".into()),
                cx,
                |_, _| Rc::new(Cell::new(None::<Drag>)),
            )
            .read(cx)
            .clone();
        let left = self.geometry.left(self.width);
        let (width, height) = (self.width, self.height);

        let separator = |top: bool| {
            let line = div()
                .absolute()
                .left_0()
                .right_0()
                .h(px(1.))
                .bg(theme.border_secondary());
            if top { line.top_0() } else { line.bottom_0() }
        };
        let header = grab(
            div()
                .relative()
                .h(px(HEADER))
                .flex_none()
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_color(theme.text())
                .text(theme::TEXT_MD)
                // A 1px line in the secondary border colour along the bottom
                // of the header.
                .child(separator(false))
                .children(self.header),
            drag.clone(),
            self.geometry,
        );
        // The panel's surface is glass.
        let mut panel = super::glass_panel(self.id.clone(), theme::RADIUS_LG, cx)
            .occlude()
            .relative()
            .w(px(width))
            .h(px(height))
            // The edges of the glass are drawn over the outermost 1px of the
            // panel, so the content is kept 1px inside.
            .p(px(1.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .track_focus(&self.focus)
            .key_context("OverlayShell")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .children(self.body),
            );
        if !self.footer.is_empty() {
            panel = panel.child(grab(
                div()
                    .relative()
                    .h(px(FOOTER))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(theme.footer_surface())
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .child(separator(true))
                    .children(self.footer),
                drag.clone(),
                self.geometry,
            ));
        }
        let (close, cancel) = (self.on_close.clone(), self.on_close.clone());
        panel = panel
            .on_action(move |_: &Dismiss, window, cx| close(window, cx))
            // Escape closes the panel also while its search field has the
            // keyboard focus.
            .capture_action(move |_: &text_input::Cancel, window, cx| {
                cancel(window, cx);
                cx.stop_propagation();
            });
        if let Some(nav) = self.on_nav.clone() {
            let send = move |key: Nav| {
                let nav = nav.clone();
                move |window: &mut Window, cx: &mut App| nav(key, window, cx)
            };
            let (up, down, left, right, confirm, next) = (
                send(Nav::Up),
                send(Nav::Down),
                send(Nav::Left),
                send(Nav::Right),
                send(Nav::Confirm),
                send(Nav::Next),
            );
            let (field_left, field_right, field_confirm) =
                (left.clone(), right.clone(), confirm.clone());
            panel = panel
                .on_action(move |_: &Up, window, cx| {
                    up(window, cx);
                })
                .on_action(move |_: &Down, window, cx| {
                    down(window, cx);
                })
                .on_action(move |_: &Left, window, cx| {
                    left(window, cx);
                })
                .on_action(move |_: &Right, window, cx| {
                    right(window, cx);
                })
                .on_action(move |_: &Confirm, window, cx| {
                    confirm(window, cx);
                })
                .on_action(move |_: &NextView, window, cx| {
                    next(window, cx);
                })
                .capture_action(move |_: &text_input::Left, window, cx| {
                    if field_left(window, cx) {
                        cx.stop_propagation();
                    }
                })
                .capture_action(move |_: &text_input::Right, window, cx| {
                    if field_right(window, cx) {
                        cx.stop_propagation();
                    }
                })
                .capture_action(move |_: &text_input::Commit, window, cx| {
                    field_confirm(window, cx);
                    cx.stop_propagation();
                });
        }

        // The drag follows the pointer anywhere in the window, so the handlers
        // are on the window.
        let on_move = self.on_move.clone();
        let tracker = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moved = (drag.clone(), on_move.clone());
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    let (drag, on_move) = &moved;
                    let Some(mut state) = drag.get() else {
                        return;
                    };
                    if phase != DispatchPhase::Capture || !event.dragging() {
                        return;
                    }
                    let delta = event.position - state.from;
                    let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
                    if !state.active && dx.abs() < DRAG_THRESHOLD && dy.abs() < DRAG_THRESHOLD {
                        return;
                    }
                    state.active = true;
                    drag.set(Some(state));
                    if let Some(on_move) = on_move {
                        let next = dragged(state.start, point(dx, dy), view, width, height);
                        on_move(next, window, cx);
                    }
                });
                let up = drag.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                    if phase == DispatchPhase::Capture {
                        up.set(None);
                    }
                });
            },
        )
        .absolute()
        .size_0();

        let mut layer = floating(
            ElementId::NamedChild(std::sync::Arc::new(self.id), "floating".into()),
            self.layer,
        )
        .at(point(left, self.geometry.top));
        // A pinned panel has no backdrop, so a click outside it does not close
        // it.
        if !self.pinned {
            let close = self.on_close;
            layer = layer
                .dimmed(self.dimmed)
                .dismiss(Dismissal::Backdrop { right: true }, move |window, cx| {
                    close(window, cx)
                });
        }
        layer.child(panel.child(tracker))
    }
}

/// Returns a 24px button for the panel's header, with its tooltip. Its
/// background appears over 120 ms when the pointer moves onto it
/// (`icon_button::Look::Pin`).
pub fn header_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    tip: Tip,
) -> super::icon_button::IconButton {
    super::icon_button(id, icon)
        .look(super::icon_button::Look::Pin)
        .tip(tip)
}

/// Returns the panel's view tabs: the shared segmented control in its look
/// `segmented::Look::View`, with one tab for each icon and label in `tabs`.
pub fn view_tabs(
    id: impl Into<ElementId>,
    tabs: &[(&'static str, &'static str)],
    selected: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> super::segmented::Segmented {
    let segments = tabs
        .iter()
        .map(|(icon, label)| super::segmented::segment(*icon, *label))
        .collect();
    super::segmented::segmented(id, super::segmented::Look::View, segments, Some(selected))
        .on_select(on_select)
}

/// Returns a row's label with the first place where it matches the search
/// text drawn bold, in the primary text colour and underlined.
pub fn search_match(label: impl Into<SharedString>, query: &str, cx: &App) -> StyledText {
    let theme = theme::theme(cx);
    let label = label.into();
    let range = crate::text::match_range(&label, query);
    StyledText::new(label).with_highlights(range.map(|range| {
        (
            range,
            HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                color: Some(theme.text().into()),
                underline: Some(UnderlineStyle {
                    color: Some(theme.contrast(20.).into()),
                    thickness: px(1.),
                    wavy: false,
                }),
                ..Default::default()
            },
        )
    }))
}

/// Returns the box that shows one key in a key hint: the key's name in the
/// monospace font at caption size, on a background of the contrast colour at
/// 10% opacity.
pub fn kbd(key: impl Into<SharedString>, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .min_w(px(18.))
        .h(px(17.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .bg(theme.subtle_overlay())
        .font_family(theme::FONT_MONO)
        .text(theme::TEXT_CAPTION)
        .text_color(theme.text())
        .child(key.into())
}

/// Returns the box that shows the Tab key in a key hint, as a glyph of an
/// arrow that points at a bar.
pub fn tab_key(cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .w(px(20.))
        .h(px(17.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .bg(theme.subtle_overlay())
        .child(super::icon("picker-tab", 13., theme.text()))
}

/// Returns the row of a key hint. Its children are words and keys side by
/// side, at caption size and in the tertiary text colour.
pub fn hint(cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .text(theme::TEXT_CAPTION)
        .text_color(theme.text_tertiary())
}

#[cfg(test)]
mod tests {
    use super::{Geometry, Opens, dragged, place};
    use gpui::{point, px, size};

    #[test]
    fn a_panel_opens_above_its_button_within_the_window() {
        let view = size(px(1200.), px(800.));
        let geometry = place(Opens::Above(point(420., 600.)), view, 340., 480.);
        assert_eq!((geometry.left(340.), geometry.top), (420., 116.));
        // A panel that would reach past the top or the right edge is kept
        // 12px inside the window.
        let geometry = place(Opens::Above(point(1000., 300.)), view, 340., 480.);
        assert_eq!((geometry.left(340.), geometry.top), (848., 12.));
    }

    #[test]
    fn a_panel_opens_left_of_the_inspector_at_its_button() {
        // The inspector's edge is at x = 900 and the button's top is at
        // y = 300, in a window of 1200 × 800.
        let view = size(px(1200.), px(800.));
        let geometry = place(Opens::Beside(point(900., 300.)), view, 540., 480.);
        assert_eq!(geometry.right, Some(888.));
        assert_eq!(geometry.left(540.), 348.);
        assert_eq!(geometry.top, 300.);
        // When the panel is made narrower, it keeps its right edge.
        assert_eq!(geometry.left(340.), 548.);
    }

    #[test]
    fn a_panel_is_kept_inside_the_window() {
        let view = size(px(1200.), px(800.));
        // A panel that would reach below the window is moved up to 12px
        // above the bottom.
        let geometry = place(Opens::Beside(point(900., 700.)), view, 340., 480.);
        assert_eq!(geometry.top, 308.);
        // The inspector's edge is so far left that the panel would reach
        // past the window's left edge. The panel starts 12px inside it.
        let geometry = place(Opens::Beside(point(200., 100.)), view, 540., 480.);
        assert_eq!(geometry.left(540.), 12.);
        // Without an anchor the panel is centred.
        let geometry = place(Opens::beside(None), view, 360., 480.);
        assert_eq!(
            (geometry.right, geometry.left, geometry.top),
            (None, 420., 160.)
        );
    }

    #[test]
    fn a_panel_opens_below_its_control() {
        let view = size(px(1000.), px(700.));
        let below = place(Opens::Below(point(100., 80.)), view, 360., 480.);
        assert_eq!((below.right, below.left, below.top), (None, 100., 80.));
        // The panel is moved in from the right edge and up from the bottom.
        let pushed = place(Opens::Below(point(900., 400.)), view, 360., 480.);
        assert_eq!((pushed.left, pushed.top), (628., 208.));
    }

    #[test]
    fn a_drag_moves_the_panel_but_not_out_of_the_window() {
        let start = Geometry {
            right: Some(888.),
            left: 348.,
            top: 300.,
        };
        let view = size(px(1200.), px(800.));
        let moved = dragged(start, point(-20., 10.), view, 540., 480.);
        assert_eq!(moved.left(540.), 328.);
        assert_eq!(moved.right, Some(868.));
        assert_eq!(moved.top, 308.);
        let far = dragged(start, point(-1000., 1000.), view, 540., 480.);
        assert_eq!((far.left(540.), far.top), (12., 308.));
        // The panel is never dragged up over the titlebar and its window
        // buttons.
        let up = dragged(start, point(0., -1000.), view, 540., 480.);
        assert_eq!(up.top, crate::theme::TITLEBAR_HEIGHT + 12.);
    }
}
