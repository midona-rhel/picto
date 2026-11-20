//! The components that more than one view uses. Each component has one
//! implementation, in this module, in the same way that `src/shared/ui`
//! holds the shared components of the Electron frontend. Views build their
//! screens from these components. A control that only one view has in
//! production (a control with its own CSS class) is drawn by that view and
//! is not in this module. Everything that is drawn over the rest of the
//! window, such as a menu, a tooltip or a dialog, takes its drawing order
//! from [`layer`]. Something that opens beside a control also takes its
//! position from [`popover`] (`popover::place`) and is drawn by the element
//! that [`popover`] provides (`popover::Floating`).
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
pub mod keys;
pub mod layer;
pub mod menu;
pub mod modal;
pub mod momentum;
pub mod notify;
pub mod number_input;
pub mod overlay_shell;
mod panel_section;
pub mod popover;
pub mod progress_bar;
pub mod progress_dialog;
pub mod property_row;
pub mod rail_item;
mod scroll_to_top;
pub mod scrollbar;
mod section_header;
pub mod segmented;
pub mod select;
pub mod selection_modal;
mod sized;
pub mod sidebar_row;
pub mod slider;
mod small_button;
pub mod status_badge;
pub mod tag_token_input;
pub mod text_input;
pub mod titlebar_button;
mod toggle;
pub mod tooltip;
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
pub use sized::sized;
pub use slider::slider;
pub use small_button::small_button;
pub use status_badge::status_badge;
pub use titlebar_button::titlebar_button;
pub use toggle::toggle;
pub use tooltip::tip;

use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// Returns the icon at the path `name` in the embedded icon set. The icon
/// is `size` pixels wide and high and is drawn in the single colour `color`.
pub fn icon(name: impl Into<SharedString>, size: f32, color: impl Into<Hsla>) -> Svg {
    svg().path(name).size(px(size)).text_color(color.into())
}

/// Shapes one line of `text` in `font`, at `size` pixels and in `color`. The
/// caller can measure the result or paint it.
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

/// Returns the width in pixels of one line of `text` in `font` at `size`
/// pixels.
pub fn text_width(text: &str, font: Font, size: f32, window: &Window) -> f32 {
    f32::from(shape_line(text.to_owned(), font, size, black(), window).width)
}

/// A breadcrumb that the user can click to go somewhere else: a part of the
/// path in the toolbar, or the parent in the viewer's breadcrumb
/// (`.viewerBreadcrumb` in production). Its text has the secondary text
/// colour. While the pointer is over it, the text has the primary text
/// colour and, when `underline` is true, is underlined. It calls `occlude`,
/// so a press on it does not reach the titlebar's drag region behind it.
pub fn crumb(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    underline: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let label = label.into();
    // Production draws this as a `<button>`. Assistive technology is given
    // the role of a button, and the label as the button's name.
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .occlude()
        .cursor_pointer()
        .text_color(theme.text_secondary())
        .hover(move |style| {
            let style = style.text_color(theme.text());
            if underline { style.underline() } else { style }
        })
        .child(label)
}

/// Registers the mouse handlers for pressing a track and then dragging along
/// it. `bounds` is where the track is in the window. Call this from the
/// paint function of a `canvas`. A press of the left button inside `bounds`
/// sets `held` to true, passes the pointer's position to `on_point`, and
/// stops the press from reaching any other element. While `held` is true,
/// every move of the pointer anywhere in the window passes its position to
/// `on_point`. When a mouse button is released, `held` is set to false and,
/// if it was true, `released` is called.
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

/// A strip of the titlebar that the user drags to move the window, as an
/// element with `data-window-drag-region` does in the Electron shell. On
/// macOS and Linux, pressing the left button on the strip and then moving
/// the pointer starts moving the window. A double-click does what the system
/// setting for a double-click on a titlebar says on macOS, and maximises or
/// restores the window on Linux. On Windows the strip is registered as the
/// system's caption area (`WindowControlArea::Drag`), and Windows itself
/// then handles the drag, the double-click and window snapping. A control
/// inside the strip calls `occlude`, so that a press on the control does not
/// move the window.
pub fn drag_region(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id = id.into();
    // The strip has the role `TitleBar`, so assistive technology reports it
    // as the window's title bar.
    if cfg!(target_os = "windows") {
        return div()
            .id(id)
            .role(Role::TitleBar)
            .window_control_area(WindowControlArea::Drag);
    }
    let pressed = window.use_keyed_state(id.clone(), cx, |_, _| false);
    let (down, up, out, moved) = (pressed.clone(), pressed.clone(), pressed.clone(), pressed);
    div()
        .id(id)
        .role(Role::TitleBar)
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

/// Wraps a badge's text in an element that is moved up by 1px. GPUI centres
/// a line of text 1px lower than production's canvas draws it with
/// `textBaseline = 'middle'`, and the 1px shift makes the two match.
pub fn badge_text(text: impl Into<SharedString>) -> Div {
    div().relative().top(px(-1.)).child(text.into())
}

/// A translucent badge that looks like the label badges that the grid draws
/// on its tiles (`drawBadge` in `primitives.ts`). It is 18px high and as
/// wide as its text plus `padding` on each side. Production fills the box
/// and then strokes a 1px line along its edge. A canvas stroke is centred on
/// the edge, so half of the line is over the fill and half is outside it.
/// Here the line is drawn as two borders of 0.5px: one on the box itself,
/// over the fill, and one on a child element whose border lies just outside
/// the box. A box shadow is not used for the outer half, because GPUI draws
/// a shadow under the whole box, and it would show through the translucent
/// fill and darken it.
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
