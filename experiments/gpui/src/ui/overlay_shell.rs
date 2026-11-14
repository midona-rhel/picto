//! The floating selection panel (`OverlayShell.tsx`) the tag and folder
//! pickers open in: a glass panel beside the inspector, or centred, over a
//! clear backdrop that closes it on a click or right-click. Its 40px header
//! and footer drag it (after 5px), Escape closes it, and while pinned the
//! backdrop is gone so the rest of the window stays usable.
//!
//! Also the panel's own small controls (`OverlayShell.module.css`): the
//! header buttons, the compact view tabs and the key hints. Its check box is
//! [`super::check::check_box`]'s compact size.
use super::text_input;
use super::tooltip::Tip;
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

actions!(
    overlay_shell,
    [Dismiss, Up, Down, Left, Right, Confirm, NextView]
);

pub fn bind(cx: &mut App) {
    let context = Some("OverlayShell");
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("tab", NextView, context),
        // Production reads only the key, so Shift-Tab turns the same way.
        KeyBinding::new("shift-tab", NextView, context),
    ]);
}

/// A key the panel's owner can act on, from the search field or the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    /// Tab: the next view of the panel's rail.
    Next,
}

/// `MARGIN`: the panel keeps this far inside the window.
pub const MARGIN: f32 = 12.;
/// `DRAG_THRESHOLD`
const DRAG_THRESHOLD: f32 = 5.;
const HEADER: f32 = 40.;
const FOOTER: f32 = 40.;

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
type MoveHandler = Rc<dyn Fn(Geometry, &mut Window, &mut App) + 'static>;
/// Answers whether the owner used the key; one it did not use goes on to
/// the search field (the side arrows move its caret).
type NavHandler = Rc<dyn Fn(Nav, &mut Window, &mut App) -> bool + 'static>;

/// Where the panel is. One opened from the inspector keeps its right edge
/// where it was placed, so a change of width grows it to the left, as the
/// CSS `right` offset does; a centred one keeps its left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub right: Option<f32>,
    pub left: f32,
    pub top: f32,
}

impl Geometry {
    /// The panel's left edge at `width`.
    pub fn left(&self, width: f32) -> f32 {
        self.right.map_or(self.left, |right| right - width)
    }
}

/// Where a panel of `width` × `height` opens (`OverlayShell`'s placement):
/// with an anchor, its right edge 12px left of the anchor's x (the
/// inspector's left edge) and its top at the anchor's y; without one,
/// centred. Either way it is kept 12px inside the window.
pub fn place(anchor: Option<Point<f32>>, viewport: Size<f32>, width: f32, height: f32) -> Geometry {
    match anchor {
        Some(anchor) => {
            let mut top = anchor.y;
            if top + height > viewport.height - MARGIN {
                top = viewport.height - height - MARGIN;
            }
            if top < MARGIN {
                top = MARGIN;
            }
            // `right` as the CSS has it: the distance from the window's
            // right edge.
            let mut right = viewport.width - anchor.x + MARGIN;
            if right < MARGIN {
                right = MARGIN;
            }
            if viewport.width - right - width < MARGIN {
                right = viewport.width - width - MARGIN;
            }
            let edge = (viewport.width - right).round();
            Geometry {
                right: Some(edge),
                left: edge - width,
                top: top.round(),
            }
        }
        None => Geometry {
            right: None,
            left: ((viewport.width - width) / 2.).round(),
            top: ((viewport.height - height) / 2.).round(),
        },
    }
}

/// Where a panel opens below a control (`anchorPlacement: 'below'`): its
/// left edge and top at the anchor, kept 12px inside the window.
pub fn place_below(anchor: Point<f32>, viewport: Size<f32>, width: f32, height: f32) -> Geometry {
    let mut top = anchor.y;
    if top + height > viewport.height - MARGIN {
        top = viewport.height - height - MARGIN;
    }
    let top = top.max(MARGIN);
    let left = anchor.x.min(viewport.width - width - MARGIN).max(MARGIN);
    Geometry {
        right: None,
        left: left.round(),
        top: top.round(),
    }
}

/// `anchorPlacement: 'above'`: the panel's bottom 4px above the anchor,
/// from its left edge, kept 12px inside the window.
pub fn place_above(anchor: Point<f32>, viewport: Size<f32>, width: f32, height: f32) -> Geometry {
    let left = anchor.x.min(viewport.width - width - MARGIN).max(MARGIN);
    let top = (anchor.y - height - 4.).max(MARGIN);
    Geometry {
        right: None,
        left: left.round(),
        top: top.round(),
    }
}

/// The panel dragged by `delta` from `start`, kept 12px inside the window
/// and below the titlebar, whose buttons it must never cover.
pub fn dragged(
    start: Geometry,
    delta: Point<f32>,
    viewport: Size<f32>,
    width: f32,
    height: f32,
) -> Geometry {
    let max_left = (viewport.width - width - MARGIN).max(MARGIN);
    let max_top = (viewport.height - height - MARGIN).max(MARGIN);
    let left = (start.left(width) + delta.x).clamp(MARGIN, max_left);
    let min_top = theme::TITLEBAR_HEIGHT + MARGIN;
    let top = (start.top + delta.y).clamp(min_top, max_top.max(min_top));
    Geometry {
        right: start.right.map(|_| left + width),
        left,
        top,
    }
}

/// A drag of the header or footer: where the pointer went down, where the
/// panel was, and whether it has passed the threshold.
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
    focus: FocusHandle,
    header: Vec<AnyElement>,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    on_close: Handler,
    on_move: Option<MoveHandler>,
    on_nav: Option<NavHandler>,
    layer: usize,
}

/// A panel at `geometry`, `width` × `height`. `focus` takes the keyboard
/// while nothing inside it does, so Escape reaches the panel.
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
        focus: focus.clone(),
        header: Vec::new(),
        body: Vec::new(),
        footer: Vec::new(),
        on_close: Rc::new(on_close),
        on_move: None,
        on_nav: None,
        layer: 5,
    }
}

impl OverlayShell {
    /// The layer the panel is drawn on: above a dialog (`ui::modal`, 10)
    /// when a dialog opened it, as the portal that follows the dialog's is.
    pub fn layer(mut self, layer: usize) -> Self {
        self.layer = layer;
        self
    }
    /// Pinned, the backdrop goes and only Escape closes the panel.
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
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
    /// The arrow keys, Enter and Tab, from anywhere in the panel.
    pub fn on_nav(
        mut self,
        handler: impl Fn(Nav, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_nav = Some(Rc::new(handler));
        self
    }
    /// What a drag of the header or footer does with the panel's new place.
    pub fn on_move(mut self, handler: impl Fn(Geometry, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Rc::new(handler));
        self
    }
}

/// Starts a drag of the panel from a press on the header or footer.
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
        let viewport = window.viewport_size();
        let view = size(f32::from(viewport.width), f32::from(viewport.height));
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
                // `box-shadow: 0 -1px 0 var(--color-border-secondary) inset`
                .child(separator(false))
                .children(self.header),
            drag.clone(),
            self.geometry,
        );
        // `.panel`: the glass, with its edges.
        let mut panel = super::glass_panel(self.id.clone(), theme::RADIUS_LG, cx)
            .occlude()
            .relative()
            .w(px(width))
            .h(px(height))
            // The glass edges are drawn over the panel; the 1px each takes
            // in production's border box is kept clear here.
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
            // Escape closes the panel even from its search field.
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

        // The drag follows the pointer anywhere in the window.
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

        let mut layer = div()
            .id(ElementId::NamedChild(
                std::sync::Arc::new(self.id),
                "backdrop".into(),
            ))
            .w(viewport.width)
            .h(viewport.height)
            .child(tracker);
        if !self.pinned {
            let (click, right) = (self.on_close.clone(), self.on_close);
            layer = layer
                .occlude()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| click(window, cx))
                .on_mouse_down(MouseButton::Right, move |_, window, cx| right(window, cx))
                .on_scroll(|_, _, cx| cx.stop_propagation());
        }
        let panel = div()
            .absolute()
            .left(px(left))
            .top(px(self.geometry.top))
            .child(panel);
        deferred(
            anchored()
                .position(point(px(0.), px(0.)))
                .child(layer.child(panel)),
        )
        .with_priority(self.layer)
    }
}

/// `.pinBtn`: a 24px header button whose overlay shows under the pointer
/// over 120 ms, with its tooltip.
pub fn header_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    tip: Tip,
) -> super::icon_button::IconButton {
    super::icon_button(id, icon)
        .look(super::icon_button::Look::Pin)
        .tip(tip)
}

/// `.viewTabs`: the shared segmented control's view-tab look, with a
/// tooltip on each tab.
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

/// `highlightMatch`: a row's label with the search's first match in bold,
/// in the text colour and underlined.
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

/// `.kbd`: one key, in the monospace caption on the subtle overlay.
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

/// `TabKeyHint`: the Tab key as its arrow-to-bar glyph.
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

/// `.kbdHint`: words and keys side by side in the tertiary caption.
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
    use super::{Geometry, dragged, place, place_above};
    use gpui::{point, size};

    #[test]
    fn a_panel_opens_above_its_button_within_the_window() {
        let geometry = place_above(point(420., 600.), size(1200., 800.), 340., 480.);
        assert_eq!((geometry.left(340.), geometry.top), (420., 116.));
        // Too near the top or the right edge, it stays 12px inside.
        let geometry = place_above(point(1000., 300.), size(1200., 800.), 340., 480.);
        assert_eq!((geometry.left(340.), geometry.top), (848., 12.));
    }

    #[test]
    fn a_panel_opens_left_of_the_inspector_at_its_button() {
        // Inspector edge at x = 900, button top at y = 300, 1200 × 800 window.
        let geometry = place(Some(point(900., 300.)), size(1200., 800.), 540., 480.);
        assert_eq!(geometry.right, Some(888.));
        assert_eq!(geometry.left(540.), 348.);
        assert_eq!(geometry.top, 300.);
        // Narrowed, it keeps its right edge.
        assert_eq!(geometry.left(340.), 548.);
    }

    #[test]
    fn a_panel_is_kept_inside_the_window() {
        // Too low: it rises to 12px above the bottom.
        let geometry = place(Some(point(900., 700.)), size(1200., 800.), 340., 480.);
        assert_eq!(geometry.top, 308.);
        // An inspector edge so far left the panel would leave the window.
        let geometry = place(Some(point(200., 100.)), size(1200., 800.), 540., 480.);
        assert_eq!(geometry.left(540.), 12.);
        // No anchor: centred.
        let geometry = place(None, size(1200., 800.), 360., 480.);
        assert_eq!(
            (geometry.right, geometry.left, geometry.top),
            (None, 420., 160.)
        );
    }

    #[test]
    fn a_panel_opens_below_its_control() {
        let viewport = size(1000., 700.);
        let below = super::place_below(point(100., 80.), viewport, 360., 480.);
        assert_eq!((below.right, below.left, below.top), (None, 100., 80.));
        // Pushed in from the right edge and up from the bottom.
        let pushed = super::place_below(point(900., 400.), viewport, 360., 480.);
        assert_eq!((pushed.left, pushed.top), (628., 208.));
    }

    #[test]
    fn a_drag_moves_the_panel_but_not_out_of_the_window() {
        let start = Geometry {
            right: Some(888.),
            left: 348.,
            top: 300.,
        };
        let moved = dragged(start, point(-20., 10.), size(1200., 800.), 540., 480.);
        assert_eq!(moved.left(540.), 328.);
        assert_eq!(moved.right, Some(868.));
        assert_eq!(moved.top, 308.);
        let far = dragged(start, point(-1000., 1000.), size(1200., 800.), 540., 480.);
        assert_eq!((far.left(540.), far.top), (12., 308.));
        // Never up under the titlebar and its window buttons.
        let up = dragged(start, point(0., -1000.), size(1200., 800.), 540., 480.);
        assert_eq!(up.top, crate::theme::TITLEBAR_HEIGHT + 12.);
    }
}
