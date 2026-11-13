use crate::theme;
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// The gutter every scrolling surface reserves (`::-webkit-scrollbar` width).
pub const GUTTER: f32 = 8.;
/// The thumb is inset by a 2px transparent border on each side.
const INSET: f32 = 2.;
const MINIMUM_THUMB: f32 = 24.;

/// What survives between frames: whether the pointer is over the thumb and,
/// while the thumb is dragged, how far into the thumb it was grabbed.
#[derive(Default)]
struct State {
    hovered: Cell<bool>,
    grab: Cell<Option<Pixels>>,
}

/// The app's one scrollbar, as `globals.css` styles it: an 8px gutter with a
/// rounded thumb inset 2px, at a quarter contrast and at half while the
/// pointer is over the thumb or dragging it. Place it as the last child of the element
/// that wraps the scrolling surface tracked by `scroll`.
pub fn scrollbar(
    id: impl Into<ElementId>,
    scroll: &ScrollHandle,
    window: &mut Window,
    cx: &mut App,
) -> Canvas<()> {
    let theme = theme::theme(cx);
    let state = window.use_keyed_state(id.into(), cx, |_, _| Rc::new(State::default()));
    let state = state.read(cx).clone();
    let scroll = scroll.clone();
    let view = window.current_view();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let viewport = scroll.bounds().size.height;
            let scrollable = scroll.max_offset().y;
            if scrollable <= px(0.) || viewport <= px(0.) {
                return;
            }
            let track = bounds.size.height;
            let thumb = (track * (viewport / (viewport + scrollable))).max(px(MINIMUM_THUMB));
            let travel = track - thumb;
            let top = bounds.top() + travel * (-scroll.offset().y / scrollable);
            let thumb_bounds = Bounds::new(
                point(bounds.left() + px(INSET), top + px(INSET)),
                size(px(GUTTER - 2. * INSET), thumb - px(2. * INSET)),
            );
            let lit = state.hovered.get() || state.grab.get().is_some();
            window.paint_quad(
                fill(thumb_bounds, theme.contrast(if lit { 50. } else { 25. }))
                    .corner_radii(px((GUTTER - 2. * INSET) / 2.)),
            );

            let down = (state.clone(), scroll.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                let (state, scroll) = &down;
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !bounds.contains(&event.position)
                {
                    return;
                }
                // A press beside the thumb brings the thumb's middle to it.
                let grab = if thumb_bounds.contains(&event.position) {
                    event.position.y - top
                } else {
                    thumb / 2.
                };
                state.grab.set(Some(grab));
                drag_to(scroll, bounds, thumb, scrollable, event.position.y - grab);
                cx.stop_propagation();
                window.refresh();
            });
            let moved = (state.clone(), scroll.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                let (state, scroll) = &moved;
                if phase != DispatchPhase::Capture {
                    return;
                }
                if let Some(grab) = state.grab.get() {
                    drag_to(scroll, bounds, thumb, scrollable, event.position.y - grab);
                    cx.notify(view);
                } else {
                    let hovered = thumb_bounds.contains(&event.position);
                    if state.hovered.replace(hovered) != hovered {
                        cx.notify(view);
                    }
                }
            });
            let up = state.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture && up.grab.take().is_some() {
                    cx.notify(view);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .right_0()
    .w(px(GUTTER))
}

/// Scrolls so the thumb's top edge sits at `thumb_top`, within its travel.
fn drag_to(
    scroll: &ScrollHandle,
    track: Bounds<Pixels>,
    thumb: Pixels,
    scrollable: Pixels,
    thumb_top: Pixels,
) {
    let travel = track.size.height - thumb;
    if travel <= px(0.) {
        return;
    }
    let along = ((thumb_top - track.top()) / travel).clamp(0., 1.);
    scroll.set_offset(point(scroll.offset().x, -scrollable * along));
}
