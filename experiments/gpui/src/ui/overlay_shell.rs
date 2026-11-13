//! `OverlayShell.tsx`: the glass panel the tag and folder pickers float in,
//! with a 40px header and footer that drag it, over a transparent backdrop
//! whose press closes it; and the pieces its pickers share (`.checkBox`,
//! `.kbdHint`, `.pinBtn`).
use super::tooltip::{self, Tip};
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::rc::Rc;

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
type DragHandler = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App) + 'static>;

/// `MARGIN`: the panel keeps this far from the window's edges.
pub const MARGIN: f32 = 12.;
/// `DRAG_THRESHOLD`: how far a press moves before it drags the panel.
pub const DRAG_THRESHOLD: f32 = 5.;
const BAR: f32 = 40.;

/// The panel's place for an anchor below which it opens
/// (`anchorPlacement: 'below'`): at the anchor, kept inside the window.
pub fn place_below(anchor: Point<f32>, width: f32, height: f32, viewport: Size<f32>) -> Point<f32> {
    let mut y = anchor.y;
    if y + height > viewport.height - MARGIN {
        y = viewport.height - height - MARGIN;
    }
    let y = y.max(MARGIN);
    let x = anchor.x.min(viewport.width - width - MARGIN).max(MARGIN);
    point(x.round(), y.round())
}

/// A dragged panel's place: the start moved by the pointer, kept inside the
/// window, once the pointer has moved past the threshold.
pub fn dragged(
    origin: Point<f32>,
    start: Point<f32>,
    pointer: Point<f32>,
    width: f32,
    height: f32,
    viewport: Size<f32>,
) -> Option<Point<f32>> {
    let (dx, dy) = (pointer.x - start.x, pointer.y - start.y);
    if dx.abs() < DRAG_THRESHOLD && dy.abs() < DRAG_THRESHOLD {
        return None;
    }
    let max_x = viewport.width - width - MARGIN;
    let max_y = viewport.height - height - MARGIN;
    Some(point(
        (origin.x + dx).min(max_x).max(MARGIN),
        (origin.y + dy).min(max_y).max(MARGIN),
    ))
}

#[derive(IntoElement)]
pub struct OverlayShell {
    id: ElementId,
    origin: Point<f32>,
    width: f32,
    height: f32,
    header: Vec<AnyElement>,
    body: Option<AnyElement>,
    footer: Vec<AnyElement>,
    on_close: Option<Handler>,
    on_drag: Option<DragHandler>,
}

pub fn overlay_shell(
    id: impl Into<ElementId>,
    origin: Point<f32>,
    width: f32,
    height: f32,
) -> OverlayShell {
    OverlayShell {
        id: id.into(),
        origin,
        width,
        height,
        header: Vec::new(),
        body: None,
        footer: Vec::new(),
        on_close: None,
        on_drag: None,
    }
}

impl OverlayShell {
    pub fn header(mut self, child: impl IntoElement) -> Self {
        self.header.push(child.into_any_element());
        self
    }
    pub fn body(mut self, child: impl IntoElement) -> Self {
        self.body = Some(child.into_any_element());
        self
    }
    pub fn footer(mut self, child: impl IntoElement) -> Self {
        self.footer.push(child.into_any_element());
        self
    }
    /// A press or right click on the backdrop.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
    /// A press on the header or footer, where a drag may start.
    pub fn on_drag_start(
        mut self,
        handler: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drag = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for OverlayShell {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let drag = |bar: Stateful<Div>, handler: &Option<DragHandler>| match handler.clone() {
            Some(handler) => bar.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                handler(event.position, window, cx)
            }),
            None => bar,
        };
        let header = drag(
            div()
                .id("shell-header")
                .h(px(BAR))
                .flex_none()
                .flex()
                .items_center()
                .px(px(10.))
                .gap(px(4.))
                .cursor(CursorStyle::OpenHand)
                // `box-shadow: 0 -1px 0 var(--color-border-secondary) inset`
                .shadow(vec![
                    BoxShadow::new(px(0.), px(-1.), theme.border_secondary().into()).inset(),
                ])
                .children(self.header),
            &self.on_drag,
        );
        let mut panel = div()
            .id(self.id.clone())
            .occlude()
            .absolute()
            .left(px(self.origin.x))
            .top(px(self.origin.y))
            .w(px(self.width))
            .h(px(self.height))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.glass())
            .border_1()
            .border_color(theme.glass_border())
            .shadow(theme::panel_shadow())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .children(self.body),
            );
        if !self.footer.is_empty() {
            panel = panel.child(drag(
                div()
                    .id("shell-footer")
                    .h(px(BAR))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .bg(theme.footer_surface())
                    .border_t_1()
                    .border_color(theme.border_secondary())
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .cursor(CursorStyle::OpenHand)
                    .children(self.footer),
                &self.on_drag,
            ));
        }
        let (close, right_close) = (self.on_close.clone(), self.on_close);
        div()
            .id(ElementId::NamedChild(
                std::sync::Arc::new(self.id),
                "layer".into(),
            ))
            .absolute()
            .inset_0()
            .child(
                div()
                    .id("shell-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(close) = &close {
                            close(window, cx);
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(close) = &right_close {
                            close(window, cx);
                        }
                    }),
            )
            .child(panel)
    }
}

/// `.checkBox` and its states.
#[derive(Clone, Copy, PartialEq)]
pub enum CheckState {
    Empty,
    /// `.checkBoxFilterChecked`: included by a filter.
    Included,
    /// `.checkBoxExcluded`: left out by a filter.
    Excluded,
}

/// A 14px check box in the pickers' rows.
pub fn check_box(state: CheckState, cx: &App) -> Div {
    let theme = theme::theme(cx);
    let base = div()
        .size(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM));
    match state {
        CheckState::Empty => base
            .bg(theme.control_surface())
            .border_1()
            .border_color(theme.pressed_overlay()),
        CheckState::Included => {
            base.bg(theme.primary())
                .child(super::icon("check", 10., theme.on_primary()))
        }
        CheckState::Excluded => {
            base.bg(theme.negative())
                .child(super::icon("x", 10., theme.on_primary()))
        }
    }
}

/// `.kbd`: a key in the footer's hints.
pub fn kbd(key: impl Into<SharedString>, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .h(px(17.))
        .min_w(px(18.))
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

/// `.kbdHint`: what a key does, then the key.
pub fn kbd_hint(label: &'static str, keys: &[&'static str], cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .text(theme::TEXT_CAPTION)
        .text_color(theme.text_tertiary())
        .child(label)
        .children(keys.iter().map(|key| kbd(*key, cx)))
}

/// `.pinBtn`: the header's 24px glyph buttons.
pub fn shell_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    tip: Tip,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let id = id.into();
    let button = div()
        .id(id.clone())
        .size(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_CONTROL))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.subtle_overlay()))
        .child(super::icon(icon, 14., theme.text()));
    tooltip::attach(
        button,
        ElementId::NamedChild(std::sync::Arc::new(id), "tip".into()),
        tip,
        24.,
        window,
        cx,
    )
}

#[cfg(test)]
mod tests {
    use super::{dragged, place_below};
    use gpui::{point, size};

    #[test]
    fn the_panel_opens_below_its_anchor_inside_the_window() {
        let viewport = size(1000., 700.);
        assert_eq!(
            place_below(point(100., 80.), 360., 480., viewport),
            point(100., 80.)
        );
        // Pushed in from the right edge and up from the bottom.
        assert_eq!(
            place_below(point(900., 400.), 360., 480., viewport),
            point(1000. - 360. - 12., 700. - 480. - 12.)
        );
    }

    #[test]
    fn a_drag_starts_past_five_pixels_and_stays_inside() {
        let viewport = size(1000., 700.);
        let origin = point(100., 100.);
        let start = point(200., 120.);
        assert_eq!(
            dragged(origin, start, point(203., 124.), 360., 480., viewport),
            None
        );
        assert_eq!(
            dragged(origin, start, point(260., 100.), 360., 480., viewport),
            Some(point(160., 80.))
        );
        assert_eq!(
            dragged(origin, start, point(0., 0.), 360., 480., viewport),
            Some(point(12., 12.))
        );
    }
}
