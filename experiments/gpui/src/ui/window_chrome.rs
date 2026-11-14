//! What every Picto window draws around its content: the macOS rim, the
//! close button of a window that hides the system's, and on Windows and
//! Linux, where production's windows are frameless, the minimise, maximise
//! and close buttons and (Linux) the edges that resize the window.
use crate::i18n::t;
#[cfg(target_os = "macos")]
use crate::platform::macos;
use crate::theme;
use gpui::*;

/// The frame every Picto window asks the system for: no system titlebar
/// (the app draws its own; on macOS the traffic lights stay, at
/// `traffic_lights` if given), with the app deciding where the window is
/// dragged from; and on Linux the client's decorations, as production's
/// windows are frameless there (`frame: false`).
pub fn frameless(
    title: impl Into<SharedString>,
    traffic_lights: Option<Point<Pixels>>,
) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: traffic_lights,
        }),
        app_owns_titlebar_drag: true,
        window_decorations: DECORATIONS,
        // `PICTO_GPUI_BACKGROUND`: no window of the app takes the keyboard.
        focus: !crate::seams::background(),
        ..Default::default()
    }
}

/// Brings a window of the app forward with the keyboard, unless
/// `PICTO_GPUI_BACKGROUND` asks every window to stay behind whatever the
/// user is working in.
pub fn activate(window: &mut Window) {
    if !crate::seams::background() {
        window.activate_window();
    }
}

/// The decorations [`frameless`] asks for: the client's on Linux, the
/// system's default elsewhere.
pub const DECORATIONS: Option<WindowDecorations> =
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        Some(WindowDecorations::Client)
    } else {
        None
    };

/// How far a titlebar's left block keeps its controls from the window's
/// left edge: clear of the traffic lights on macOS (`.titlebarLeftMac`),
/// its 8px padding elsewhere (`.titlebarLeft`).
pub const TITLEBAR_INSET: f32 = if cfg!(target_os = "macos") { 74. } else { 8. };

/// `.shellMac::after`: the window's rim, over everything, in the glass
/// edges. Production draws it on macOS only.
pub fn rim(cx: &App) -> Option<impl IntoElement + use<>> {
    cfg!(target_os = "macos").then(|| {
        deferred(super::glass_edges(theme::WINDOW_RADIUS, cx)).with_priority(super::layer::CHROME)
    })
}

/// Whether the app draws the window's minimise, maximise and close buttons
/// (`WindowControls`, shown on Windows and Linux): always on Windows, whose
/// system titlebar these windows hide; on Linux while the window's
/// decorations are the client's (an X11 desktop without a compositor keeps
/// the window manager's frame, buttons and all). Never on macOS.
fn draws_caption_buttons(window: &Window) -> bool {
    if cfg!(target_os = "macos") {
        false
    } else if cfg!(target_os = "windows") {
        true
    } else {
        matches!(window.window_decorations(), Decorations::Client { .. })
    }
}

/// `WindowControls`: minimise, maximise and close, 28px each with no gap,
/// for the main window's titlebar. On Windows they are the system's caption
/// buttons (`WindowControlArea`), so the system performs them, with its
/// snap layouts over maximise; on Linux they ask the window manager.
pub fn caption_buttons(id: &str, window: &Window) -> Option<Div> {
    if !draws_caption_buttons(window) {
        return None;
    }
    let button = |part: &str, glyph: &'static str, glyph_size: f32, area: WindowControlArea| {
        let button = super::icon_button(SharedString::from(format!("{id}-{part}")), glyph)
            .look(if area == WindowControlArea::Close {
                super::icon_button::Look::CaptionClose
            } else {
                super::icon_button::Look::WindowClose
            })
            .glyph(glyph_size)
            .tip(super::tip(match area {
                WindowControlArea::Min => t("Minimize"),
                WindowControlArea::Max => t("Maximize"),
                _ => t("Close"),
            }))
            .window_control(area);
        if cfg!(target_os = "windows") {
            button
        } else {
            button.on_click(move |_, window, _| match area {
                WindowControlArea::Min => window.minimize_window(),
                WindowControlArea::Max => window.zoom_window(),
                _ => window.remove_window(),
            })
        }
    };
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(button(
                "minimize",
                "window-minimize",
                16.,
                WindowControlArea::Min,
            ))
            .child(button(
                "maximize",
                "window-maximize",
                12.,
                WindowControlArea::Max,
            ))
            .child(button(
                "close",
                "window-close",
                16.,
                WindowControlArea::Close,
            )),
    )
}

/// How far into the window its edges resize it, and how far along an edge
/// a corner reaches.
const RESIZE_EDGE: f32 = 5.;
const RESIZE_CORNER: f32 = 10.;

/// The strips along a window of `size` that resize it, and which edge or
/// corner each moves. A side tiled against the screen's edge (or every
/// side of a maximised window) has none. The strips keep their width on the screen whatever the interface `zoom`,
/// as a system's resize border does.
fn resize_zones(
    size: Size<Pixels>,
    tiling: Tiling,
    zoom: f32,
) -> Vec<(ResizeEdge, Bounds<Pixels>)> {
    let (width, height) = (f32::from(size.width), f32::from(size.height));
    let (edge, corner) = (RESIZE_EDGE / zoom, RESIZE_CORNER / zoom);
    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size_of(w, h));
    fn size_of(w: f32, h: f32) -> Size<Pixels> {
        gpui::size(px(w.max(0.)), px(h.max(0.)))
    }
    let mut zones = Vec::new();
    if !tiling.top {
        zones.push((ResizeEdge::Top, rect(corner, 0., width - 2. * corner, edge)));
    }
    if !tiling.bottom {
        zones.push((
            ResizeEdge::Bottom,
            rect(corner, height - edge, width - 2. * corner, edge),
        ));
    }
    if !tiling.left {
        zones.push((
            ResizeEdge::Left,
            rect(0., corner, edge, height - 2. * corner),
        ));
    }
    if !tiling.right {
        zones.push((
            ResizeEdge::Right,
            rect(width - edge, corner, edge, height - 2. * corner),
        ));
    }
    let corners = [
        (ResizeEdge::TopLeft, tiling.top || tiling.left, 0., 0.),
        (
            ResizeEdge::TopRight,
            tiling.top || tiling.right,
            width - corner,
            0.,
        ),
        (
            ResizeEdge::BottomLeft,
            tiling.bottom || tiling.left,
            0.,
            height - corner,
        ),
        (
            ResizeEdge::BottomRight,
            tiling.bottom || tiling.right,
            width - corner,
            height - corner,
        ),
    ];
    for (which, tiled, x, y) in corners {
        if !tiled {
            zones.push((which, rect(x, y, corner, corner)));
        }
    }
    zones
}

/// The pointer over a resizing edge or corner.
fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// The edges that resize a window whose decorations are the client's (a
/// frameless window on Linux): a press on one hands the resize to the
/// window manager. Drawn last, over the window's content, by each window's
/// root; nothing while the system draws the frame.
pub fn resize_edges(window: &Window) -> Option<impl IntoElement + use<>> {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return None;
    };
    if !window.is_resizable() {
        return None;
    }
    let zones = resize_zones(window.viewport_size(), tiling, window.ui_zoom());
    Some(
        div()
            .absolute()
            .inset_0()
            .children(zones.into_iter().enumerate().map(|(index, (edge, at))| {
                div()
                    .id(("resize-edge", index))
                    .absolute()
                    .left(at.origin.x)
                    .top(at.origin.y)
                    .w(at.size.width)
                    .h(at.size.height)
                    .occlude()
                    .cursor(resize_cursor(edge))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        window.start_window_resize(edge);
                    })
            })),
    )
}

/// Hides the traffic lights of a window that draws its own close button.
/// GPUI has no option for this, so it asks AppKit directly.
pub fn hide_window_buttons(window: &Window) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, macos::hide_window_buttons);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Keeps the window's content at `width / height` while it is resized, as
/// Electron's `setAspectRatio` does.
pub fn lock_aspect_ratio(window: &Window, ratio: f64) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, |ns_window| {
        macos::lock_aspect_ratio(ns_window, ratio)
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (window, ratio);
}

/// Whether [`set_always_on_top`] can float a window here: on macOS and
/// Windows. GPUI gives no handle to the X11 connection to set
/// `_NET_WM_STATE_ABOVE` with, and Wayland has no such request, so on Linux
/// the control that asks for it is not shown.
pub const fn can_stay_on_top() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Floats the window above others, or puts it back among them, as
/// Electron's `setAlwaysOnTop` does with its default `floating` level.
pub fn set_always_on_top(window: &Window, on_top: bool) {
    #[cfg(target_os = "windows")]
    crate::platform::windows::set_topmost(window, on_top);
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, |ns_window| macos::set_floating(ns_window, on_top));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, on_top);
}

/// Takes the window off the screen without closing it (`orderOut:`), as a
/// Mac app's main window goes when it is closed while the app runs on.
pub fn order_out(window: &Window) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, macos::order_out);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Whether [`system_blur_behind`] blurs what is behind a pop-up window:
/// on macOS, the one system whose menus are pop-up windows
/// (`ui::menu::presentation`), through an `NSVisualEffectView` beneath the
/// panel. Windows and Linux show menus inside the window, as production's
/// renderer does, where the window's own backdrop blur (or, on Windows,
/// the more opaque glass) applies.
pub const fn system_blurs() -> bool {
    cfg!(target_os = "macos")
}

/// Gives a transparent pop-up window the system's blur of what is behind
/// it, shown only beneath the panel [`place_system_blur`] names. Called as
/// the window opens; until a panel is placed nothing is blurred.
pub fn system_blur_behind(window: &Window) {
    if !system_blurs() {
        return;
    }
    window.set_background_appearance(WindowBackgroundAppearance::Blurred);
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, macos::backdrop::hide);
}

/// Keeps the blur of [`system_blur_behind`] to the panel at `panel` in the
/// window, with corners of `radius`, at `alpha`.
pub fn place_system_blur(window: &Window, panel: Bounds<Pixels>, radius: f32, alpha: f32) {
    #[cfg(target_os = "macos")]
    {
        // AppKit's points are the window's screen points: the drawn
        // panel's times the interface zoom.
        let zoom = window.ui_zoom();
        let height = f32::from(window.viewport_size().height) * zoom;
        let rect = (
            f32::from(panel.origin.x) * zoom,
            f32::from(panel.origin.y) * zoom,
            f32::from(panel.size.width) * zoom,
            f32::from(panel.size.height) * zoom,
        );
        let radius = radius * zoom;
        macos::with_ns_window(window, |ns_window| {
            macos::backdrop::place(ns_window, rect, height, radius, alpha)
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, panel, radius, alpha);
}

/// The Window menu's Bring All to Front (`arrangeInFront:`).
pub fn bring_all_to_front(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        let _ = cx;
        macos::arrange_in_front();
    }
    #[cfg(not(target_os = "macos"))]
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
    }
}

/// `WindowCloseButton`: a 28px square with a thin 16px cross, as the shared
/// icon button's window-close look.
// a11y: the window's buttons are the shared icon buttons, named by their
// tooltips or as `WindowCloseButton` names itself.
pub fn close_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> super::icon_button::IconButton {
    // `WindowCloseButton`'s `ariaLabel`, "Close" unless a caller names it.
    super::icon_button(id, "window-close")
        .look(super::icon_button::Look::WindowClose)
        .name(crate::i18n::t("Close"))
        .on_click(on_click)
}

#[cfg(test)]
mod tests {
    use super::{RESIZE_CORNER, RESIZE_EDGE, resize_zones};
    use gpui::{Bounds, ResizeEdge, Tiling, point, px, size};

    #[test]
    fn a_floating_window_resizes_from_every_edge_and_corner() {
        let zones = resize_zones(size(px(800.), px(600.)), Tiling::default(), 1.);
        assert_eq!(zones.len(), 8);
        let at = |edge| zones.iter().find(|(which, _)| *which == edge).unwrap().1;
        assert_eq!(
            at(ResizeEdge::Right),
            Bounds::new(
                point(px(800. - RESIZE_EDGE), px(RESIZE_CORNER)),
                size(px(RESIZE_EDGE), px(600. - 2. * RESIZE_CORNER))
            )
        );
        assert_eq!(
            at(ResizeEdge::BottomLeft).origin,
            point(px(0.), px(600. - RESIZE_CORNER))
        );
    }

    #[test]
    fn tiled_sides_and_their_corners_do_not_resize() {
        let left_half = Tiling {
            top: true,
            bottom: true,
            left: true,
            right: false,
        };
        let edges: Vec<ResizeEdge> = resize_zones(size(px(800.), px(600.)), left_half, 1.)
            .into_iter()
            .map(|(edge, _)| edge)
            .collect();
        assert_eq!(edges, [ResizeEdge::Right]);
        assert!(resize_zones(size(px(800.), px(600.)), Tiling::tiled(), 1.).is_empty());
    }

    #[test]
    fn the_edges_keep_their_width_on_the_screen_under_zoom() {
        // A 1200-point window at 150% is laid out 800 points wide; its right
        // edge is still 5 screen points, 5 / 1.5 of the window's own.
        let zones = resize_zones(size(px(800.), px(600.)), Tiling::default(), 1.5);
        let right = zones
            .iter()
            .find(|(which, _)| *which == ResizeEdge::Right)
            .unwrap()
            .1;
        assert_eq!(right.size.width * 1.5, px(RESIZE_EDGE));
        assert_eq!(right.origin.x + right.size.width, px(800.));
    }
}
