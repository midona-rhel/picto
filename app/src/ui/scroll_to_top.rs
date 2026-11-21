use crate::{motion, theme};
use gpui::*;

const SIZE: f32 = 25.;

/// Returns the "Return to Top" button: a small glass button at the bottom
/// right of a scrolling surface. It moves up into view once the surface has
/// scrolled past `threshold`, and a click on it scrolls the surface back to
/// the top. Returns `None` while the button is hidden. Place it after the
/// surface, inside the element that wraps the surface.
pub fn scroll_to_top(
    id: impl Into<ElementId>,
    scroll: &ScrollHandle,
    threshold: f32,
    bottom: f32,
    window: &mut Window,
    cx: &mut App,
) -> Option<super::GlassPanel> {
    let theme = theme::theme(cx);
    let id = id.into();
    let visible = -scroll.offset().y >= px(threshold);
    let fade = window.use_keyed_state(id.clone(), cx, |_, _| motion::Fade::at(visible));
    if fade.read(cx).on() != visible {
        // The button moves and fades over `motion::RETURN_TO_TOP` along
        // `motion::EASE_IN_OUT`, after a delay of
        // `motion::RETURN_TO_TOP_DELAY`.
        fade.update(cx, |fade, _| {
            fade.set_after(visible, motion::RETURN_TO_TOP, motion::RETURN_TO_TOP_DELAY)
        });
    }
    let shown = fade.read(cx).value_with(motion::EASE_IN_OUT);
    if fade.read(cx).animating() {
        window.request_animation_frame();
    }
    if shown <= 0. {
        return None;
    }
    let scroll = scroll.clone();
    let view = window.current_view();
    Some(
        super::GlassPanel::new(
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(crate::i18n::t("Return to Top")),
            theme::RADIUS_MD,
        )
        .backdrop_blur(theme.glass_blur())
        .drop_shadows(theme.panel_shadow_small())
        .group("return-to-top")
        .occlude()
        .absolute()
        .right(px(10.))
        // While hidden, the button is one and a half times its own height
        // below its place.
        .bottom(px(bottom - SIZE * 1.5 * (1. - shown)))
        .size(px(SIZE))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(theme.glass_border())
        .bg(theme.glass())
        .opacity(0.9 * shown)
        .hover(|style| style.opacity(1.))
        .child(
            div()
                .absolute()
                .top(px(-1.))
                .left(px(5.))
                .right(px(5.))
                .h(px(1.))
                .bg(theme.glass_border_top()),
        )
        .child(
            svg()
                .path("arrow-up")
                .size(px(15.))
                .text_color(theme.text())
                .opacity(0.5)
                .group_hover("return-to-top", |style| style.opacity(1.)),
        )
        .on_click(move |_, _, cx| {
            scroll.set_offset(point(scroll.offset().x, px(0.)));
            cx.notify(view);
        }),
    )
}
