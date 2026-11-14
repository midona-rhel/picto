//! The interface zoom: production's `zoomController.ts`, which sets CSS
//! `zoom` on the main window's document. Cmd+= or Cmd++ zooms in, Cmd+−
//! zooms out and Cmd+0 resets (Control with them too on macOS, as
//! `AppShell`'s scope reads `metaKey || ctrlKey`; Control elsewhere), by 5%
//! between 50% and 200%, rounded to whole percent. The zoom is kept per
//! machine (`localStorage` `picto-zoom-factor` there, `zoomFactor` in
//! `state.json` here) and restored at launch. Only Cmd+0 resets it.
//!
//! The keys belong to the main window's own scope, so they do nothing while
//! a text field, menu, popover or dialog has the keyboard, and the viewer's
//! keys come first: Cmd+0 is the viewer's Actual Size on an image, a paged
//! document or a font (and the Duplicates screen's Actual Pixels), Cmd+= and
//! Cmd+− size a font's preview; on anything else they pass through to here
//! (`viewer::surface::host_actions`). The grid's own `+` and `−` change the
//! tile size and are not these keys.
//!
//! GPUI draws the zoom for every window (`App::set_ui_zoom`, a patch in
//! `vendor/`): each lays out in its size divided by the zoom and draws at
//! the display's scale times the zoom, as a browser's page zoom does.
//! Production zooms the main window and, when they open, detail windows;
//! here the Settings window and the pop-up menus follow too, at once.
//! Settings' Zoom Level is a different value (the library's `zoomFactor`),
//! saved and never applied, in production as here.
use crate::keymap::{self, Scope};
use crate::state;
use gpui::*;

/// `ZOOM_STEP`, `ZOOM_MIN`, `ZOOM_MAX`.
pub const STEP: f64 = 0.05;
pub const MIN: f64 = 0.5;
pub const MAX: f64 = 2.0;

actions!(zoom, [ZoomIn, ZoomOut, ResetZoom]);

/// `setZoom`'s rounding to whole percent and its limits. A stored value
/// that is not a number is 100%.
pub fn clamp(factor: f64) -> f64 {
    if !factor.is_finite() {
        return 1.0;
    }
    ((factor * 100.).round() / 100.).clamp(MIN, MAX)
}

/// `zoomIn`.
pub fn zoomed_in(current: f64) -> f64 {
    clamp(current + STEP)
}

/// `zoomOut`.
pub fn zoomed_out(current: f64) -> f64 {
    clamp(current - STEP)
}

/// Binds the keys and starts every window at the kept zoom. Called before
/// the first window opens.
pub fn bind(cx: &mut App) {
    cx.set_ui_zoom(clamp(state::get(cx).zoom_factor) as f32);
    let context = Scope::App.predicate();
    let context = context.as_deref();
    // `e.key === '=' || e.key === '+'`, `'-'`, `'0'` with either modifier on
    // the Mac; Control is the only one elsewhere.
    let modifiers: &[&str] = if cfg!(target_os = "macos") {
        &["Mod", "Ctrl"]
    } else {
        &["Mod"]
    };
    for modifier in modifiers {
        keymap::bind_written(&format!("{modifier}+="), Box::new(ZoomIn), context, cx);
        keymap::bind_written(&format!("{modifier}++"), Box::new(ZoomIn), context, cx);
        keymap::bind_written(&format!("{modifier}+-"), Box::new(ZoomOut), context, cx);
        keymap::bind_written(&format!("{modifier}+0"), Box::new(ResetZoom), context, cx);
    }
    cx.on_action(|_: &ZoomIn, cx| set(zoomed_in(current(cx)), cx));
    cx.on_action(|_: &ZoomOut, cx| set(zoomed_out(current(cx)), cx));
    cx.on_action(|_: &ResetZoom, cx| set(1.0, cx));
}

fn current(cx: &App) -> f64 {
    state::get(cx).zoom_factor
}

/// Zooms every window to `factor` (rounded and limited) and keeps it.
pub fn set(factor: f64, cx: &mut App) {
    let factor = clamp(factor);
    state::update(cx, |state| state.zoom_factor = factor);
    cx.set_ui_zoom(factor as f32);
}

/// Where `window`'s content starts on its display, in screen points: below
/// any system titlebar, at the frame's bottom less the content's height.
fn content_origin(window: &Window) -> Point<Pixels> {
    let frame = window.bounds();
    let content = window.viewport_size().height * window.ui_zoom();
    point(frame.origin.x, frame.origin.y + frame.size.height - content)
}

/// Screen bounds (a display's area) in the zoomed points a window of the
/// app is laid out in, so that pop-up windows can be placed with the same
/// numbers as what is drawn in the window.
pub fn screen_to_ui(bounds: Bounds<Pixels>, zoom: f32) -> Bounds<Pixels> {
    Bounds::new(
        bounds.origin / zoom,
        bounds.size.map(|length| length / zoom),
    )
}

/// Bounds in zoomed points ([`screen_to_ui`]) back on the screen: the frame
/// of a pop-up window that shows them.
pub fn ui_to_screen(bounds: Bounds<Pixels>, zoom: f32) -> Bounds<Pixels> {
    Bounds::new(
        bounds.origin * zoom,
        bounds.size.map(|length| length * zoom),
    )
}

/// Where `window`'s content starts on its display, in zoomed points.
pub fn content_origin_in_ui(window: &Window) -> (f32, f32) {
    let origin = content_origin(window) / window.ui_zoom();
    (f32::from(origin.x), f32::from(origin.y))
}

#[cfg(test)]
mod tests {
    use super::{clamp, screen_to_ui, ui_to_screen, zoomed_in, zoomed_out};
    use gpui::{Bounds, point, px, size};

    #[test]
    fn steps_by_five_percent_between_half_and_double() {
        let mut zoom = 1.0;
        let mut seen = Vec::new();
        for _ in 0..25 {
            zoom = zoomed_in(zoom);
            seen.push(zoom);
        }
        assert_eq!(seen[0], 1.05);
        assert_eq!(seen[9], 1.5);
        // Stops at 200%.
        assert_eq!(seen[19], 2.0);
        assert_eq!(seen[24], 2.0);
        let mut zoom = 1.0;
        for _ in 0..4 {
            zoom = zoomed_out(zoom);
        }
        assert_eq!(zoom, 0.8);
        for _ in 0..20 {
            zoom = zoomed_out(zoom);
        }
        assert_eq!(zoom, 0.5);
        // Back from the floor in exact steps, as production's rounding does.
        assert_eq!(zoomed_in(0.5), 0.55);
    }

    #[test]
    fn rounds_to_whole_percent_and_limits() {
        assert_eq!(clamp(1.234), 1.23);
        assert_eq!(clamp(0.1), 0.5);
        assert_eq!(clamp(7.), 2.0);
        assert_eq!(clamp(f64::NAN), 1.0);
        assert_eq!(clamp(f64::INFINITY), 1.0);
    }

    /// A box 100 points from the window's corner, which counts the presses
    /// that land on it.
    struct Target(u32);

    impl gpui::Render for Target {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{InteractiveElement as _, ParentElement as _, Styled as _};
            gpui::div().size_full().child(
                gpui::div()
                    .id("target")
                    .absolute()
                    .left(px(100.))
                    .top(px(100.))
                    .size(px(50.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, _| this.0 += 1),
                    ),
            )
        }
    }

    #[test]
    fn a_zoomed_window_lays_out_and_hit_tests_in_zoomed_points() {
        use gpui::{
            AnyWindowHandle, AppContext as _, Modifiers, MouseButton, MouseDownEvent, PlatformInput,
        };
        let mut cx = gpui::TestAppContext::single();
        let window = cx.add_window(|_, _| Target(0));
        let handle: AnyWindowHandle = window.into();
        let (viewport, scale) = cx
            .update_window(handle, |_, window, _| {
                (window.viewport_size(), window.scale_factor())
            })
            .unwrap();
        cx.update(|cx| cx.set_ui_zoom(1.5));
        let press = |cx: &mut gpui::TestAppContext, x: f32, y: f32| {
            cx.update_window(handle, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                window.dispatch_platform_input(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: point(px(x), px(y)),
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                (
                    window.viewport_size(),
                    window.scale_factor(),
                    window.mouse_position(),
                )
            })
            .unwrap()
        };
        // The box is drawn at 150..225 screen points: a press at 160 lands
        // on it, one at 110 (on it at 100%) does not.
        let (zoomed, zoomed_scale, at) = press(&mut cx, 160., 160.);
        assert_eq!(zoomed.width, viewport.width / 1.5);
        assert_eq!(zoomed_scale, scale * 1.5);
        assert!((f32::from(at.x) - 160. / 1.5).abs() < 1e-4);
        press(&mut cx, 110., 110.);
        let count = window.read_with(&cx, |target, _| target.0).unwrap();
        assert_eq!(count, 1);
        // A window opened afterwards starts at the zoom.
        let later: AnyWindowHandle = cx.add_window(|_, _| Target(0)).into();
        let later_scale = cx
            .update_window(later, |_, window, _| window.scale_factor())
            .unwrap();
        assert_eq!(later_scale, scale * 1.5);
    }

    #[test]
    fn screen_and_ui_points_convert_both_ways() {
        let screen = Bounds::new(point(px(150.), px(300.)), size(px(1500.), px(900.)));
        let ui = screen_to_ui(screen, 1.5);
        assert_eq!(
            ui,
            Bounds::new(point(px(100.), px(200.)), size(px(1000.), px(600.)))
        );
        assert_eq!(ui_to_screen(ui, 1.5), screen);
        assert_eq!(screen_to_ui(screen, 1.0), screen);
    }
}
