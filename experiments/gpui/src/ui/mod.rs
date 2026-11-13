//! Shared components, one implementation each, as `src/shared/ui` is for the
//! Electron frontend. Views compose these and style nothing themselves.
mod icon_button;
pub mod scrollbar;
mod sidebar_row;
pub mod text_input;
pub mod zoom;

pub use icon_button::icon_button;
pub use scrollbar::scrollbar;
pub use sidebar_row::sidebar_row;

use crate::theme;
use gpui::*;

/// A monochrome glyph from the embedded icon set.
pub fn icon(name: &'static str, size: f32, color: impl Into<Hsla>) -> Svg {
    svg().path(name).size(px(size)).text_color(color.into())
}

/// A titlebar strip that moves the window, as `data-window-drag-region` does
/// in the Electron shell: a press that then moves drags the window, and a
/// double-click does what the system setting says. Controls inside the strip
/// opt out by occluding it.
pub fn drag_region(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id = id.into();
    let pressed = window.use_keyed_state(id.clone(), cx, |_, _| false);
    let (down, up, out, moved) = (pressed.clone(), pressed.clone(), pressed.clone(), pressed);
    div()
        .id(id)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            down.update(cx, |pressed, _| *pressed = true)
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
            up.update(cx, |pressed, _| *pressed = false)
        })
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
            out.update(cx, |pressed, _| *pressed = false)
        })
        .on_mouse_move(move |_, window, cx| {
            if moved.update(cx, |pressed, _| std::mem::take(pressed)) {
                window.start_window_move();
            }
        })
        .on_click(|event, window, _| {
            if event.click_count() == 2 {
                window.titlebar_double_click();
            }
        })
}

/// A plain text push button. It occludes what is behind it, so it works
/// inside a titlebar strip.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let theme = theme::theme(cx);
    div()
        .id(id.into())
        .occlude()
        .h(px(theme::CONTROL_SM))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(theme::RADIUS_MD))
        .bg(theme.contrast(8.))
        .hover(move |style| style.bg(theme.contrast(14.)))
        .cursor_pointer()
        .child(label.into())
}

/// The grid's translucent label chip (`drawBadge` in `primitives.ts`).
pub fn badge(padding: f32) -> Div {
    div()
        .absolute()
        .h(px(18.))
        .px(px(padding))
        .flex()
        .items_center()
        .rounded(px(theme::TILE_RADIUS))
        .bg(rgba(theme::BADGE_BACKGROUND))
        .border_1()
        .border_color(rgba(theme::BADGE_BORDER))
        .text_size(px(theme::TEXT_XS))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(theme::BADGE_TEXT))
}

/// A sidebar section title, with its item count when it has any.
pub fn section_header(label: &'static str, count: usize, cx: &App) -> Div {
    let mut header = div()
        .pl(px(theme::SIDEBAR_INSET + theme::SIDEBAR_SECTION_INSET))
        .mt(px(8.))
        .h(px(24.))
        .flex()
        .items_center()
        .text_size(px(theme::TEXT_SM))
        .text_color(theme::theme(cx).text_tertiary())
        .child(label);
    if count > 0 {
        header = header.child(format!("({count})"));
    }
    header
}
