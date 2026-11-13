//! What every Picto window draws around its content: the macOS rim, and the
//! close button of a window that hides the system's.
use crate::{motion, theme};
use gpui::*;

/// `.shellMac::after`: the window's rim, over everything. Top 25% white,
/// sides 15%, bottom 10%; one border has one colour, so the bottom's goes
/// all the way round and the others are laid over it.
pub fn rim() -> impl IntoElement {
    let over = |alpha: f32| Rgba {
        r: 1.,
        g: 1.,
        b: 1.,
        a: alpha,
    };
    let edge = || div().absolute().inset_0().rounded(px(theme::WINDOW_RADIUS));
    deferred(
        div()
            .absolute()
            .inset_0()
            .child(edge().border_1().border_color(over(0.10)))
            // 1 − 0.9 × (1 − a) gives 0.15 at the sides, 0.25 on top.
            .child(edge().border_l_1().border_r_1().border_color(over(0.0556)))
            .child(edge().border_t_1().border_color(over(0.1667))),
    )
    .with_priority(100)
}

struct Press {
    hover: motion::Fade,
    press: motion::Fade,
}

/// `WindowCloseButton`: a 28px square with a thin 16px cross, over the
/// subtle overlay under the pointer and the pressed overlay while held,
/// changing over 150 ms.
pub fn close_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let id = id.into();
    let state = window.use_keyed_state(id.clone(), cx, |_, _| Press {
        hover: motion::Fade::default(),
        press: motion::Fade::default(),
    });
    let (hover, press) = {
        let state = state.read(cx);
        if state.hover.animating() || state.press.animating() {
            window.request_animation_frame();
        }
        (state.hover.value(), state.press.value())
    };
    // `--color-subtle-overlay` under the pointer, `--color-pressed-overlay`
    // while held.
    let hovered = 10. * hover;
    let overlay = hovered + (20. - hovered) * press;
    let (entered, down, up, out) = (state.clone(), state.clone(), state.clone(), state);
    let release = |state: &Entity<Press>, cx: &mut App| {
        state.update(cx, |state, cx| {
            state.press.set(false, motion::BUTTON_HOVER);
            cx.notify();
        })
    };
    div()
        .id(id)
        .occlude()
        .size(px(theme::CONTROL_MD))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_CONTROL))
        .bg(theme.contrast(overlay))
        .cursor_pointer()
        .child(super::icon("window-close", 16., theme.text()))
        .on_hover(move |hovered, _, cx| {
            entered.update(cx, |state, cx| {
                state.hover.set(*hovered, motion::BUTTON_HOVER);
                cx.notify();
            })
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            down.update(cx, |state, cx| {
                state.press.set(true, motion::BUTTON_HOVER);
                cx.notify();
            })
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| release(&up, cx))
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| release(&out, cx))
        .on_click(on_click)
}
