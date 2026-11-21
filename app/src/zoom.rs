//! The zoom of the whole interface. Cmd+= or Cmd++ zooms in, Cmd+− zooms
//! out and Cmd+0 resets the zoom to 100%. On macOS the same keys also work
//! with Control in place of Cmd; on Windows and Linux the modifier is
//! Control. Each step is 5%, the zoom stays between 50% and 200%, and it is
//! rounded to a whole percent. Picto 0.6.19 and earlier used the same keys,
//! step and limits. The zoom is stored per machine, as `zoomFactor` in
//! `state.json`, and applied again at launch. Only Cmd+0 resets it.
//!
//! The keys are bound in the main window's own scope, so they do nothing
//! while a text field, menu, popover or dialog has the keyboard. The
//! viewer's bindings of the same keys take precedence: Cmd+0 is the viewer's
//! Actual Size on an image, a paged document or a font (and Actual Pixels on
//! the Duplicates screen), and Cmd+= and Cmd+− change the size of a font's
//! preview. On any other kind of item the viewer passes the keys on to the
//! bindings here (`viewer::surface::host_actions`). The grid's `+` and `−`
//! keys, without a modifier, change the tile size and are separate from
//! these.
//!
//! GPUI applies the zoom to every window (`App::set_ui_zoom`, a patch in
//! `vendor/`). Each window is laid out in its size divided by the zoom and
//! drawn at the display's scale times the zoom, as a browser's page zoom
//! does. The zoom applies to all windows at once, including the Settings
//! window and the pop-up menus.
//!
//! Zoom Level in Settings is a different value (the library's `zoomFactor`
//! setting). It is saved and never applied.
use crate::keymap::{self, Scope};
use crate::state;
use gpui::*;

/// The amount one zoom step changes the zoom by, and the smallest and
/// largest zoom, as factors (1.0 is 100%).
pub const STEP: f64 = 0.05;
pub const MIN: f64 = 0.5;
pub const MAX: f64 = 2.0;

actions!(zoom, [ZoomIn, ZoomOut, ResetZoom]);

/// Returns the factor rounded to a whole percent and limited to the range
/// from `MIN` to `MAX`. Returns 1.0 (100%) for a value that is not a finite
/// number.
pub fn clamp(factor: f64) -> f64 {
    if !factor.is_finite() {
        return 1.0;
    }
    ((factor * 100.).round() / 100.).clamp(MIN, MAX)
}

/// Returns the zoom one step above `current`.
pub fn zoomed_in(current: f64) -> f64 {
    clamp(current + STEP)
}

/// Returns the zoom one step below `current`.
pub fn zoomed_out(current: f64) -> f64 {
    clamp(current - STEP)
}

/// Binds the zoom keys and applies the stored zoom, so that every window
/// opens at it. Called before the first window opens.
pub fn bind(cx: &mut App) {
    cx.set_ui_zoom(clamp(state::get(cx).zoom_factor) as f32);
    let context = Scope::App.predicate();
    let context = context.as_deref();
    // On macOS the keys work with Cmd and with Control. On Windows and
    // Linux `Mod` is Control, which is the only modifier there.
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

/// Zooms every window to `factor`, rounded and limited as `clamp` does,
/// and stores the zoom in `state.json`.
pub fn set(factor: f64, cx: &mut App) {
    let factor = clamp(factor);
    state::update(cx, |state| state.zoom_factor = factor);
    cx.set_ui_zoom(factor as f32);
}

/// Returns the top-left corner of the window's content on its display, in
/// screen points. The content lies below any titlebar the system draws, so
/// its top is the bottom of the window's frame minus the content's height.
fn content_origin(window: &Window) -> Point<Pixels> {
    let frame = window.bounds();
    let content = window.viewport_size().height * window.ui_zoom();
    point(frame.origin.x, frame.origin.y + frame.size.height - content)
}

/// Converts bounds on the screen (for example a display's area) to the
/// zoomed points that the app's windows are laid out in. Pop-up windows can
/// then be placed with the same coordinates as the elements drawn in a
/// window.
pub fn screen_to_ui(bounds: Bounds<Pixels>, zoom: f32) -> Bounds<Pixels> {
    Bounds::new(
        bounds.origin / zoom,
        bounds.size.map(|length| length / zoom),
    )
}

/// Converts bounds in zoomed points (see [`screen_to_ui`]) back to bounds
/// on the screen. The result is the frame of a pop-up window that shows
/// those bounds.
pub fn ui_to_screen(bounds: Bounds<Pixels>, zoom: f32) -> Bounds<Pixels> {
    Bounds::new(
        bounds.origin * zoom,
        bounds.size.map(|length| length * zoom),
    )
}

/// Returns the top-left corner of the window's content on its display, in
/// zoomed points.
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
        // The zoom does not go above 200%.
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
        // One step up from the smallest zoom is exactly 55%, because each
        // result is rounded to a whole percent.
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

    /// A view that draws a box of 50 points, 100 points from the window's
    /// top-left corner, and counts the mouse presses on the box.
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
        // At 150% the box is drawn from 150 to 225 screen points. A press
        // at 160 is on it. A press at 110, which would be on it at 100%, is
        // not.
        let (zoomed, zoomed_scale, at) = press(&mut cx, 160., 160.);
        assert_eq!(zoomed.width, viewport.width / 1.5);
        assert_eq!(zoomed_scale, scale * 1.5);
        assert!((f32::from(at.x) - 160. / 1.5).abs() < 1e-4);
        press(&mut cx, 110., 110.);
        let count = window.read_with(&cx, |target, _| target.0).unwrap();
        assert_eq!(count, 1);
        // A window opened after the zoom was set is zoomed from the start.
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
