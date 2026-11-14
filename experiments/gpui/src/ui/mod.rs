//! Shared components, one implementation each, as `src/shared/ui` is for the
//! Electron frontend. Views compose these and style nothing themselves.
pub mod action_button;
mod action_row;
mod avatar;
pub mod check;
mod chip;
pub mod color_editor;
pub mod color_picker;
pub mod confirm;
pub mod date_picker;
mod empty_state;
pub mod field;
pub mod filter_control;
pub mod focus;
mod glass;
pub mod icon_button;
pub mod icon_picker;
pub mod menu;
pub mod modal;
pub mod notify;
pub mod number_input;
pub mod overlay_shell;
mod panel_section;
pub mod popover;
pub mod progress_bar;
pub mod property_row;
mod scroll_to_top;
pub mod scrollbar;
mod section_header;
pub mod segmented;
pub mod select;
pub mod selection_modal;
pub mod sidebar_row;
pub mod slider;
mod small_button;
pub mod status_badge;
mod tabs;
pub mod tag_token_input;
pub mod text_input;
mod toggle;
pub mod tooltip;
mod tracked;
pub mod window_chrome;
pub mod zoom;

pub use action_button::action_button;
pub use action_row::action_row;
pub use avatar::library_avatar;
pub use check::check;
pub use chip::{Chip, chip};
pub use empty_state::{empty_state, empty_state_action};
pub use field::field;
pub use glass::{GlassPanel, glass_edges, glass_panel};
pub use icon_button::icon_button;
pub use modal::modal;
pub use number_input::number_input;
pub use panel_section::{PanelSection, panel_section};
pub use progress_bar::progress_bar;
pub use property_row::property_row;
pub use scroll_to_top::scroll_to_top;
pub use scrollbar::scrollbar;
pub use section_header::section_header;
pub use select::{choice, select};
pub use sidebar_row::sidebar_row;
pub use slider::slider;
pub use small_button::small_button;
pub use status_badge::status_badge;
pub use tabs::tabs;
pub use toggle::toggle;
pub use tooltip::tip;
pub use tracked::tracked;

use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// A monochrome glyph from the embedded icon set.
pub fn icon(name: impl Into<SharedString>, size: f32, color: impl Into<Hsla>) -> Svg {
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

/// A badge's text, set 1px higher than GPUI centres its line, where the
/// canvas's `textBaseline = 'middle'` puts it.
pub fn badge_text(text: impl Into<SharedString>) -> Div {
    div().relative().top(px(-1.)).child(text.into())
}

/// The grid's translucent label chip (`drawBadge` in `primitives.ts`): the
/// box is text + 2 × padding, filled, then stroked 1px along its edge, so the
/// stroke lies half over the fill and half outside it, as a canvas stroke
/// does: a 0.5px border over the fill, and a 0.5px border on a ring just
/// outside (a shadow would also darken the translucent fill, which GPUI
/// draws over it).
pub fn badge(padding: f32) -> Div {
    let stroke = rgba(theme::BADGE_BORDER);
    div()
        .absolute()
        .h(px(18.))
        .px(px(padding - 0.5).max(px(0.)))
        .flex()
        .items_center()
        .rounded(px(theme::TILE_RADIUS))
        .bg(rgba(theme::BADGE_BACKGROUND))
        .border(px(0.5))
        .border_color(stroke)
        .child(
            div()
                .absolute()
                .top(px(-1.))
                .left(px(-1.))
                .right(px(-1.))
                .bottom(px(-1.))
                .rounded(px(theme::TILE_RADIUS + 0.5))
                .border(px(0.5))
                .border_color(stroke),
        )
        .text(theme::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(theme::BADGE_TEXT))
}
