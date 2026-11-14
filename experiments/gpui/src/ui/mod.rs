//! Shared components, one implementation each, as `src/shared/ui` is for the
//! Electron frontend. Views compose these; a control that only one view has
//! in production (its own CSS class) is drawn by that view. Everything that
//! floats over the window takes its drawing order from [`layer`] and, when
//! it opens beside a control, its place and its layer element from
//! [`popover`].
pub mod a11y;
pub mod action_button;
mod action_row;
mod avatar;
pub mod check;
mod chip;
pub mod color_editor;
pub mod color_picker;
pub mod confirm;
pub mod cover_dialog;
pub mod date_picker;
mod empty_state;
pub mod field;
pub mod filter_control;
pub mod focus;
pub mod form;
mod glass;
pub mod icon_button;
pub mod icon_picker;
pub mod layer;
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
pub use glass::{GlassPanel, fill_over_system_blur, fill_without_blur, glass_edges, glass_panel};
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

/// One line of `text` shaped in `font` at `size`, to measure or paint.
pub fn shape_line(
    text: impl Into<SharedString>,
    font: Font,
    size: f32,
    color: impl Into<Hsla>,
    window: &Window,
) -> ShapedLine {
    let text = text.into();
    let run = TextRun {
        len: text.len(),
        font,
        color: color.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text, px(size), &[run], None)
}

/// How wide one line of `text` is in `font` at `size`.
pub fn text_width(text: &str, font: Font, size: f32, window: &Window) -> f32 {
    f32::from(shape_line(text.to_owned(), font, size, black(), window).width)
}

/// A breadcrumb that goes somewhere (the toolbar's path, the viewer's
/// `.viewerBreadcrumb` parent): the secondary text colour, turning to the
/// primary under the pointer, and underlined there when `underline`. It
/// occludes the titlebar's drag region behind it.
pub fn crumb(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    underline: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    div()
        .id(id)
        .occlude()
        .cursor_pointer()
        .text_color(theme.text_secondary())
        .hover(move |style| {
            let style = style.text_color(theme.text());
            if underline { style.underline() } else { style }
        })
        .child(label.into())
}

/// The press-then-drag gesture along a track at `bounds`, set up from a
/// `canvas`'s paint: a left press on the track holds it, reports the point
/// and goes no further; while held, every move anywhere in the window
/// reports its point; the release lets go and, if it was held, calls
/// `released`.
pub fn track_drag(
    bounds: Bounds<Pixels>,
    held: std::rc::Rc<std::cell::Cell<bool>>,
    on_point: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    released: impl Fn(&mut App) + 'static,
    window: &mut Window,
) {
    let on_point = std::rc::Rc::new(on_point);
    let (down, pressed) = (held.clone(), on_point.clone());
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble
            && event.button == MouseButton::Left
            && bounds.contains(&event.position)
        {
            down.set(true);
            pressed(event.position, window, cx);
            cx.stop_propagation();
        }
    });
    let moving = held.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Capture && moving.get() {
            on_point(event.position, window, cx);
        }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Capture && held.replace(false) {
            released(cx);
        }
    });
}

/// A titlebar strip that moves the window, as `data-window-drag-region` does
/// in the Electron shell: a press that then moves drags the window, and a
/// double-click does what the system setting says on macOS and maximises or
/// restores the window on Linux. On Windows the strip is the system's
/// caption (`WindowControlArea::Drag`), which also gives its double-click
/// and snapping. Controls inside the strip opt out by occluding it.
pub fn drag_region(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id = id.into();
    if cfg!(target_os = "windows") {
        return div().id(id).window_control_area(WindowControlArea::Drag);
    }
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
                if cfg!(target_os = "macos") {
                    window.titlebar_double_click();
                } else {
                    window.zoom_window();
                }
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
