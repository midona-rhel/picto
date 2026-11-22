//! This module holds what a Picto window needs from the system around its
//! content, and the parts of the window's frame that the app draws itself.
//!
//! - On macOS the app draws a rim along the window's edge ([`rim`]).
//! - A window that hides the system's window buttons gets a close button of
//!   its own ([`close_button`]).
//! - On Windows and Linux the windows have no system frame. There the app
//!   draws the minimise, maximise and close buttons ([`caption_buttons`]),
//!   and on Linux also the edges that resize the window ([`resize_edges`]).
//! - The system's blur of what is behind a window is asked for here: for a
//!   pop-up menu ([`prepare_panel_window`]) and for the main window's sidebar
//!   and inspector ([`show_rail_material`]).
//!
//! The functions that call a system directly do so through
//! `crate::platform::macos` and `crate::platform::windows`.
use crate::i18n::t;
#[cfg(target_os = "macos")]
use crate::platform::macos;
use crate::theme;
use gpui::*;

/// Returns the window options that every Picto window is opened with. The
/// window has no system titlebar, because the app draws its own. On macOS
/// the system's window buttons (the traffic lights) stay, and
/// `traffic_lights` gives their position when it is not `None`. The app
/// decides which parts of the window drag it. On Linux the window asks for
/// client-side decorations, so that the window manager draws no frame and
/// no titlebar around it, and the window carries the app's id.
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
        // On Linux a desktop matches a window to the app's launcher entry
        // (`packaging/linux/com.picto.desktop.desktop`) by this id, and
        // groups the app's windows under that entry's icon. macOS and
        // Windows do not read it.
        app_id: Some(crate::desktop::APP_ID.to_owned()),
        // While the environment variable `PICTO_GPUI_BACKGROUND` is set, a
        // window of the app opens without taking keyboard focus.
        focus: !crate::seams::background(),
        // A window that does not have keyboard focus goes on drawing at the
        // display's refresh rate. GPUI's default would limit it to 30 frames
        // a second. This matters for a video in a detail window, and for the
        // main window's thumbnails and progress bars while the Settings
        // window or a detail window has keyboard focus.
        inactive_frame_interval: None,
        ..Default::default()
    }
}

/// Brings `window` to the front and gives it keyboard focus. It does nothing
/// while the environment variable `PICTO_GPUI_BACKGROUND` is set, which asks
/// every window of the app to stay behind the application the user is
/// working in.
pub fn activate(window: &mut Window) {
    if !crate::seams::background() {
        window.activate_window();
    }
}

/// The window decorations that [`frameless`] asks for. On Linux they are
/// client-side decorations, which the app draws. On macOS and Windows the
/// value is `None`, which leaves the system's default.
pub const DECORATIONS: Option<WindowDecorations> =
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        Some(WindowDecorations::Client)
    } else {
        None
    };

/// The left padding of a titlebar's left block, in pixels. On macOS it is
/// 74px, which keeps the block's controls to the right of the traffic
/// lights. On Windows and Linux it is 8px.
pub const TITLEBAR_INSET: f32 = if cfg!(target_os = "macos") { 74. } else { 8. };

/// Returns the window's rim: a rounded outline along the window's edge,
/// drawn over everything else in the window with the glass edge colours
/// (`ui::glass_edges`). It is drawn on macOS only. A window in full screen
/// has no rim, because the rim's rounded corners would be drawn inside the
/// display's own corners.
pub fn rim(window: &Window, cx: &App) -> Option<impl IntoElement + use<>> {
    (cfg!(target_os = "macos") && !window.is_fullscreen()).then(|| {
        deferred(super::glass_edges(theme::WINDOW_RADIUS, cx)).with_priority(super::layer::CHROME)
    })
}

/// Returns whether the app draws the window's minimise, maximise and close
/// buttons. It never draws them on macOS. It always draws them on Windows,
/// where these windows hide the system titlebar. On Linux it draws them
/// while the window has client-side decorations. An X11 desktop without a
/// compositor does not grant those, and the window then keeps the window
/// manager's frame and buttons.
fn draws_caption_buttons(window: &Window) -> bool {
    if cfg!(target_os = "macos") {
        false
    } else if cfg!(target_os = "windows") {
        true
    } else {
        matches!(window.window_decorations(), Decorations::Client { .. })
    }
}

/// Returns the minimise, maximise and close buttons for a titlebar, or
/// `None` when the app does not draw them ([`draws_caption_buttons`]). Each
/// button is 28px wide and there is no gap between them. On Windows each
/// button is declared to the system as a caption button
/// (`WindowControlArea`), so the system carries out the click and shows its
/// snap layouts over the maximise button. On Linux a click asks the window
/// manager to minimise, maximise or close the window.
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

/// `RESIZE_EDGE` is the width, in screen pixels, of the strip along each
/// edge of a window that resizes the window. `RESIZE_CORNER` is the side, in
/// screen pixels, of the square in each corner that resizes the window in
/// two directions.
const RESIZE_EDGE: f32 = 5.;
const RESIZE_CORNER: f32 = 10.;

/// Returns the strips of a window of `size` that resize it, each with the
/// edge or corner that it moves. A side that is tiled against the edge of
/// the screen has no strip, so a maximised window has none at all. The
/// strips are divided by the interface `zoom`, so that they keep the same
/// width on the screen at every zoom, as a system's resize border does.
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

/// Returns the cursor that is shown while the pointer is over the resize
/// strip of `edge`.
fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// Returns the resize strips of a window that has client-side decorations,
/// which is a frameless window on Linux. Pressing the left button on a strip
/// asks the window manager to start resizing the window from that edge or
/// corner. Each window's root view adds this element last, so that it is
/// drawn over the window's content. The function returns `None` while the
/// system draws the window's frame, and for a window that cannot be resized.
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
/// GPUI has no option for this, so the function asks AppKit directly. It
/// does nothing on Windows and Linux.
pub fn hide_window_buttons(window: &Window) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, macos::hide_window_buttons);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Makes the system keep the window's content at the aspect ratio `ratio`
/// (width divided by height) while the user resizes the window. It works on
/// macOS only and does nothing on Windows and Linux.
pub fn lock_aspect_ratio(window: &Window, ratio: f64) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, |ns_window| {
        macos::lock_aspect_ratio(ns_window, ratio)
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (window, ratio);
}

/// Returns whether [`set_always_on_top`] works on this system. It works on
/// macOS and Windows. On Linux it does not: GPUI gives the app no handle to
/// the X11 connection, which is needed to set `_NET_WM_STATE_ABOVE`, and
/// Wayland has no request for it. The control that would call it is
/// therefore not shown on Linux.
pub const fn can_stay_on_top() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Keeps the window above other windows when `on_top` is true, and puts it
/// back among them when `on_top` is false. On macOS it gives the window the
/// level `NSFloatingWindowLevel`, and on Windows it makes the window
/// topmost. It does nothing on Linux ([`can_stay_on_top`]).
pub fn set_always_on_top(window: &Window, on_top: bool) {
    #[cfg(target_os = "windows")]
    crate::platform::windows::set_topmost(window, on_top);
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, |ns_window| macos::set_floating(ns_window, on_top));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, on_top);
}

/// Removes the window from the screen without closing it (AppKit's
/// `orderOut:`). A Mac app does this to its main window when the user
/// closes the window and the app keeps running. It does nothing on Windows
/// and Linux.
pub fn order_out(window: &Window) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, macos::order_out);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Returns whether the system blurs what is behind the panel of a menu's
/// pop-up window ([`prepare_panel_window`]). `window` is any window of the
/// app; the answer is the same for all of them.
///
/// - macOS: always. The blur is an `NSVisualEffectView` under the panel.
/// - Windows: on Windows 11 version 22H2 and later, where the blur is the
///   system's Acrylic backdrop of the whole window. Earlier versions can
///   blur only behind a whole window whose corners are not rounded.
/// - Wayland: when the compositor has the ext-background-effect protocol
///   and has announced that it blurs, or has the kde-blur protocol.
/// - X11: when a compositing manager is running that blurs behind the
///   region a window names in `_KDE_NET_WM_BLUR_BEHIND_REGION`, which KWin
///   does.
///
/// Where the answer is `false`, the panel has a more opaque fill instead
/// (`ui::fill_without_blur`).
pub fn system_blurs(window: &Window) -> bool {
    if cfg!(target_os = "macos") {
        return true;
    }
    #[cfg(target_os = "windows")]
    if !crate::platform::windows::has_system_backdrop() {
        return false;
    }
    window.is_background_blur_supported()
}

/// Prepares the pop-up window of a menu's panel, when the window opens.
/// `dark` is whether the app's theme is a dark theme.
///
/// Where the system blurs ([`system_blurs`]), the window gets the background
/// appearance `Blurred`. On macOS, Wayland and X11 the blur is shown only
/// under the panel that [`place_panel`] names, and nothing is blurred until
/// a panel has been placed. On Windows the blur is the backdrop of the whole
/// window (`platform::windows::show_backdrop`), and it is the dark or the
/// light backdrop according to `dark`.
///
/// Where the system does not blur, the window stays transparent, and what
/// the system draws around a window is taken away
/// ([`without_system_frame`]).
pub fn prepare_panel_window(window: &Window, dark: bool) {
    #[cfg(not(target_os = "windows"))]
    let _ = dark;
    if !system_blurs(window) {
        without_system_frame(window);
        return;
    }
    window.set_background_appearance(WindowBackgroundAppearance::Blurred);
    #[cfg(target_os = "macos")]
    {
        without_system_frame(window);
        macos::with_ns_window(window, macos::backdrop::hide);
    }
    #[cfg(target_os = "windows")]
    crate::platform::windows::show_backdrop(window, dark);
}

/// Takes away what the system draws around a transparent pop-up window. The
/// panel in such a window draws its own shadow and edge.
///
/// - macOS draws a shadow and an edge highlight around whatever a
///   transparent window draws, which around a panel look like a second
///   border. AppKit turns them off (`setHasShadow:`).
/// - Windows draws a shadow, a border and rounded corners around the
///   window's rectangle (`platform::windows::without_frame`).
/// - On Linux a pop-up window gets nothing from the system, so there is
///   nothing to take away.
pub fn without_system_frame(window: &Window) {
    #[cfg(target_os = "macos")]
    macos::with_ns_window(window, macos::without_shadow);
    #[cfg(target_os = "windows")]
    crate::platform::windows::without_frame(window);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = window;
}

/// Tells the system which rectangle of a pop-up window its panel covers:
/// `panel`, with rounded corners of `radius` pixels. The panel is drawn at
/// the opacity `alpha`.
///
/// - macOS: the blur of [`prepare_panel_window`] is limited to this
///   rectangle and gets the opacity `alpha`.
/// - Wayland and X11: the rectangle becomes the window's content region
///   (`Window::set_content_region`). Where the system blurs, it blurs only
///   behind this rectangle. The pointer reaches what is behind the rest of
///   the window. On an X11 screen without a compositing manager, where a
///   window cannot be transparent, the window is cut to this rectangle.
/// - Windows: nothing. A window that shows the backdrop has exactly the
///   panel's rectangle ([`set_panel_window_frame`]).
pub fn place_panel(window: &Window, panel: Bounds<Pixels>, radius: f32, alpha: f32) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let _ = alpha;
        window.set_content_region(Some((panel, px(radius))));
    }
    #[cfg(target_os = "macos")]
    {
        // AppKit measures in the window's screen points. `panel` is in the
        // points the window draws in, so every length is multiplied by the
        // interface zoom.
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
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
    let _ = (window, panel, radius, alpha);
}

/// Moves and resizes the pop-up window of a menu's panel to `frame`, which
/// is given on the display in the points the app's windows are laid out in.
/// Only Windows needs this, for a window that shows the system's backdrop
/// and so must keep exactly its panel's rectangle while the panel changes
/// its height. The window is moved by a task, after the current update of
/// the app has ended (`platform::windows::set_frame`). It does nothing on
/// macOS and Linux.
pub fn set_panel_window_frame(window: &Window, frame: Bounds<Pixels>, cx: &App) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = crate::platform::windows::hwnd(window) else {
            return;
        };
        // Windows measures in device pixels. `frame` is in the points the
        // window is laid out in, and the window's scale factor is the
        // number of device pixels in one such point.
        let scale = window.scale_factor();
        let device = |length: Pixels| (f32::from(length) * scale).round() as i32;
        let frame = (
            device(frame.origin.x),
            device(frame.origin.y),
            device(frame.size.width),
            device(frame.size.height),
        );
        cx.spawn(async move |_| crate::platform::windows::set_frame(hwnd, frame))
            .detach();
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (window, frame, cx);
}

/// Holds whether the main window shows the system's frosted material behind
/// its sidebar and its inspector. The user turns the material on in
/// Settings > General > Appearance, with the switch "Frosted glass sidebar
/// and inspector" on macOS and "Acrylic sidebar and inspector" on Windows.
/// It is off until the user turns it on.
///
/// `wanted` is what the user chose. `supported` is whether the system can
/// blur what is behind the main window, as the window last reported it.
/// `shown` is whether the main window is showing the material behind its
/// sidebar and inspector now, which is the case when both of the others
/// are true.
#[derive(Clone, Copy, Default)]
pub struct RailMaterial {
    wanted: bool,
    supported: bool,
    shown: bool,
}

impl Global for RailMaterial {}

/// Records whether the user has turned the frosted material on. The main
/// window's shell observes [`RailMaterial`] and calls
/// [`show_rail_material`], so the window changes in its next frame.
pub fn want_rail_material(wanted: bool, cx: &mut App) {
    let material = cx.try_global::<RailMaterial>().copied();
    if material.map(|material| material.wanted) != Some(wanted) {
        cx.set_global(RailMaterial {
            wanted,
            ..material.unwrap_or_default()
        });
    }
}

/// Returns whether the system can blur what is behind the main window, as
/// [`show_rail_material`] last found it. It is `false` until a library has
/// been opened in the main window, and always `false` on Linux. The
/// Settings window shows the switch for the material only while this is
/// `true`.
pub fn rail_material_supported(cx: &App) -> bool {
    cx.try_global::<RailMaterial>()
        .is_some_and(|material| material.supported)
}

/// Returns whether the main window is showing the system's frosted material
/// behind its sidebar and inspector. While it is, those two are filled with
/// [`rail_fill`], and the rest of the window is drawn opaque.
pub fn rails_translucent(cx: &App) -> bool {
    cx.try_global::<RailMaterial>()
        .is_some_and(|material| material.shown)
}

/// How opaque the rail colour is over the frosted material: 80% in the dark
/// themes and 85% in the light themes. The rest shows the blurred picture
/// of what is behind the window. The hardest case for reading is a white
/// window behind a dark theme, or a black one behind a light theme. In that
/// case, over all six themes, a rail's primary text keeps a contrast of at
/// least 5.6 to 1 and its secondary text at least 3.7 to 1. With the solid
/// rail colour the lowest values are 10.1 to 1 and 4.5 to 1.
const RAIL_OPACITY_DARK: f32 = 0.8;
const RAIL_OPACITY_LIGHT: f32 = 0.85;

/// Returns the colour that fills the sidebar and the inspector. It is the
/// theme's rail colour. While the main window shows the frosted material
/// ([`rails_translucent`]), the colour is partly transparent
/// ([`RAIL_OPACITY_DARK`], [`RAIL_OPACITY_LIGHT`]), so that the blurred
/// picture of what is behind the window shows through it.
pub fn rail_fill(cx: &App) -> Rgba {
    let theme = theme::theme(cx);
    if !rails_translucent(cx) {
        return theme.rail();
    }
    Rgba {
        a: if theme.is_light() {
            RAIL_OPACITY_LIGHT
        } else {
            RAIL_OPACITY_DARK
        },
        ..theme.rail()
    }
}

/// Makes `window`, the main window, show or stop showing the system's
/// frosted material, and draws the window again when that changed. The
/// material is shown when the user wants it ([`want_rail_material`]) and
/// the system can blur what is behind the window
/// (`Window::is_background_blur_supported`).
///
/// The material is the window's whole background: an `NSVisualEffectView`
/// behind the content on macOS and Acrylic on Windows. The shell draws an
/// opaque background everywhere except behind the sidebar and the
/// inspector, so the material is seen only through those two. Where the
/// system cannot blur, which includes every Linux desktop, the window stays
/// opaque and the sidebar and inspector keep their solid colour.
pub fn show_rail_material(window: &mut Window, cx: &mut App) {
    let material = cx.try_global::<RailMaterial>().copied().unwrap_or_default();
    // The material is offered on macOS and Windows only.
    let supported = cfg!(any(target_os = "macos", target_os = "windows"))
        && window.is_background_blur_supported();
    let shown = material.wanted && supported;
    if shown == material.shown && supported == material.supported {
        return;
    }
    if shown != material.shown {
        window.set_background_appearance(if shown {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        });
        // Cached views are drawn again only when the window refreshes.
        window.refresh();
    }
    cx.set_global(RailMaterial {
        supported,
        shown,
        ..material
    });
}

/// Carries out the Window menu's "Bring All to Front" row. On macOS it
/// calls AppKit's `arrangeInFront:`. On Windows and Linux it brings each of
/// the app's windows to the front in turn.
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

/// Returns the close button of a window that draws its own. It is the
/// shared icon button with the look `Look::WindowClose`: a 28px square with
/// a thin 16px cross.
// a11y: the window's buttons are the shared icon buttons. Each is named by
// its tooltip, and this close button is named "Close".
pub fn close_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> super::icon_button::IconButton {
    // The button's accessible name is "Close". A caller can give it another
    // name.
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
        // A window 1200 screen points wide is laid out 800 points wide at
        // 150% zoom. Its right resize strip is still 5 screen points wide,
        // which is 5 / 1.5 of the points the window is laid out in.
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
