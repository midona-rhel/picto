//! Manages the pop-up windows that a menu's panels are shown in. This file
//! holds the record of the open menu windows and of the windows that
//! controls inside them opened, the code that opens and closes these
//! windows, and the code that closes the menu when none of its windows has
//! the keyboard.
use super::layout::{Rect, place};
use super::{ACTIVATION_SETTLE, Menu, Presentation, Role, SHADOW_ROOM, Screen, VIEWPORT_INSET};
use gpui::*;

/// Holds the pop-up windows that menus have open (`menus`), and the pop-up
/// windows that a control inside one of them opened, such as the list of a
/// select (`children`). Each entry of `children` is the menu window that the
/// window was opened from, followed by the window itself. A child window is
/// closed when its menu window closes, and while a child window has the
/// keyboard its menu counts as having the keyboard.
#[derive(Default)]
struct PopUps {
    menus: Vec<AnyWindowHandle>,
    children: Vec<(AnyWindowHandle, AnyWindowHandle)>,
}

impl Global for PopUps {}

/// Returns whether this window is one of a menu's pop-up windows. A control
/// in such a window cannot draw anything that reaches past the window, so it
/// must open a pop-up window of its own and record it with [`adopt`].
pub fn is_pop_up(window: &Window, cx: &App) -> bool {
    let handle = window.window_handle();
    cx.try_global::<PopUps>()
        .is_some_and(|pop_ups| pop_ups.menus.contains(&handle))
}

/// Records that the pop-up window `child` was opened from the menu's pop-up
/// window `owner`, so that `child` is closed when `owner` closes. If `owner`
/// is no longer a menu's window, `child` is closed at once.
pub fn adopt(owner: AnyWindowHandle, child: AnyWindowHandle, cx: &mut App) {
    let pop_ups = cx.default_global::<PopUps>();
    if pop_ups.menus.contains(&owner) {
        pop_ups.children.push((owner, child));
    } else {
        // The menu closed while the child window was opening.
        close_window(child, cx);
    }
}

/// Removes `child` from the record. A control calls this when it has closed
/// its pop-up window itself.
pub fn release(child: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<PopUps>()
        .children
        .retain(|(_, open)| *open != child);
}

fn register(window: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<PopUps>().menus.push(window);
}

/// Removes a menu window that is closing from the record, and closes the
/// windows that were opened from it.
fn forget(window: AnyWindowHandle, cx: &mut App) {
    let pop_ups = cx.default_global::<PopUps>();
    pop_ups.menus.retain(|open| *open != window);
    let mut children = Vec::new();
    pop_ups.children.retain(|(owner, child)| {
        let keep = *owner != window;
        if !keep {
            children.push(*child);
        }
        keep
    });
    for child in children {
        let _ = child.update(cx, |_, window, _| window.remove_window());
    }
}

fn children_of(owners: &[AnyWindowHandle], cx: &App) -> Vec<AnyWindowHandle> {
    cx.try_global::<PopUps>().map_or(Vec::new(), |pop_ups| {
        pop_ups
            .children
            .iter()
            .filter(|(owner, _)| owners.contains(owner))
            .map(|(_, child)| *child)
            .collect()
    })
}

impl Menu {
    // The methods below open, close and watch the menu's pop-up windows.

    /// Finds the display that the owner's window is on and where the
    /// window's content starts on that display, and opens the pop-up window
    /// of the root panel. When the window has no display, the menu is drawn
    /// inside the owner's window instead.
    pub(super) fn open_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(display) = window.display(cx) else {
            self.presentation = Presentation::InWindow;
            return;
        };
        // The display's area and the origin of the owner's content are
        // converted to the points of the interface zoom, because the menu
        // is laid out in those points. `open_pop_up` multiplies by the zoom
        // again for the frames of the windows.
        let origin = crate::zoom::content_origin_in_ui(window);
        let area = Rect::from_bounds(crate::zoom::screen_to_ui(
            display.visible_bounds(),
            window.ui_zoom(),
        ));
        self.screen = Some(Screen {
            area,
            origin,
            display: display.id(),
            bounds: Rect::default(),
            window: None,
        });
        let (area, pointer) = self.area();
        let tallest = self.tallest();
        let (x, y) = place(pointer, self.width, tallest, area);
        let rect = Rect {
            x,
            y,
            w: self.width,
            h: tallest.min(area.h - VIEWPORT_INSET),
        };
        if let Some(screen) = &mut self.screen {
            screen.bounds = rect.outset(SHADOW_ROOM).intersect(area);
        }
        self.open_pop_up(Role::Root, cx);
    }

    /// Opens a pop-up window for the root panel or for the open submenu.
    /// The window is opened after the current update has ended, because a
    /// window draws its view while it opens, and the menu cannot be drawn
    /// while it is being updated.
    pub(super) fn open_pop_up(&mut self, role: Role, cx: &mut Context<Self>) {
        let Some(screen) = &self.screen else {
            return;
        };
        let bounds = match role {
            Role::Root => screen.bounds,
            Role::Submenu(_) => match &self.submenu {
                Some(open) => open.bounds,
                None => return,
            },
        };
        // The window of the root panel is scaled in about the point the menu
        // was opened at, or about the nearest point of the panel when the
        // panel was moved away from that point to fit (`pop_point`). The
        // window of a submenu is shown without an animation.
        let pop = match role {
            Role::Root => {
                let (_, pointer) = self.area();
                let panel = self.root_rect(&self.rows());
                Some(pop_point(pointer, panel, bounds))
            }
            Role::Submenu(_) => None,
        };
        let display = screen.display;
        let focus = match role {
            Role::Root => self.focus_handle(cx),
            Role::Submenu(_) => self.focus.clone(),
        };
        let menu = cx.entity();
        cx.defer(move |cx| {
            let still_wanted = menu.update(cx, |this, _| {
                let wanted = this.closing.is_none()
                    && match role {
                        Role::Root => true,
                        Role::Submenu(serial) => this
                            .submenu
                            .as_ref()
                            .is_some_and(|open| open.serial == serial),
                    };
                if wanted {
                    this.opening = Some(role);
                }
                wanted
            });
            if !still_wanted {
                return;
            }
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(crate::zoom::ui_to_screen(
                    bounds.to_bounds(),
                    cx.ui_zoom(),
                ))),
                display_id: Some(display),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                // A pop-up window is never the key window. With GPUI's
                // default interval for inactive windows, its hover
                // highlights and fades would be drawn at 30 frames a second.
                inactive_frame_interval: None,
                ..Default::default()
            };
            let view = menu.clone();
            let opened = cx
                .open_window(options, move |window, cx| {
                    without_system_shadow(window);
                    crate::ui::window_chrome::system_blur_behind(window);
                    pop_in(window, None);
                    // The first call only turns off the system's own
                    // animation. The scale animation is started on the
                    // first frame the window shows, because one started
                    // now would have ended before the window is visible.
                    window.on_next_frame(move |window, _| pop_in(window, pop));
                    window.focus(&focus, cx);
                    view
                })
                .ok()
                .map(AnyWindowHandle::from);
            menu.update(cx, |this, cx| {
                this.opening = None;
                match (role, opened) {
                    (_, Some(handle)) if this.closing.is_some() => close_window(handle, cx),
                    (Role::Root, Some(handle)) => {
                        register(handle, cx);
                        if let Some(screen) = &mut this.screen {
                            screen.window = Some(handle);
                        }
                    }
                    (Role::Submenu(serial), Some(handle)) => match &mut this.submenu {
                        Some(open) if open.serial == serial => {
                            register(handle, cx);
                            open.window = Some(handle);
                        }
                        _ => close_window(handle, cx),
                    },
                    // The pop-up window could not be opened, so the menu is
                    // drawn inside the owner's window.
                    (Role::Root, None) => {
                        this.presentation = Presentation::InWindow;
                        this.screen = None;
                    }
                    (Role::Submenu(_), None) => this.close_submenu(cx),
                }
                cx.notify();
            });
        });
    }

    /// Closes the menu's pop-up windows. If one of them had the keyboard (a
    /// click in a panel makes its window the key window), the owner's window
    /// is activated, so that it has the keyboard again.
    pub(super) fn close_pop_ups(&mut self, cx: &mut Context<Self>) {
        let mut windows = Vec::new();
        if let Some(screen) = &mut self.screen {
            windows.extend(screen.window.take());
        }
        if let Some(open) = &mut self.submenu {
            windows.extend(open.window.take());
        }
        let main = self.main;
        cx.defer(move |cx| {
            let mut watched = windows.clone();
            watched.extend(children_of(&windows, cx));
            let had_keyboard = watched.iter().any(|window| {
                window
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(false)
            });
            for window in windows {
                forget(window, cx);
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            if had_keyboard && let Some(main) = main {
                let _ = main.update(cx, |_, window, _| window.activate_window());
            }
        });
    }

    /// Starts watching the activation of `window`, once for each window.
    /// When a watched window is activated or deactivated and none of the
    /// menu's windows has the keyboard afterwards, the menu closes. This
    /// happens when the user switches to another application. A pop-up
    /// window stays above the windows of other applications, so it must not
    /// stay open then.
    pub(super) fn watch_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = window.window_handle();
        if self.presentation != Presentation::PopUpWindows || self.watched.contains(&handle) {
            return;
        }
        self.watched.push(handle);
        let subscription = cx.observe_window_activation(window, |this, _, cx| {
            let mut windows = this.main.into_iter().collect::<Vec<_>>();
            windows.extend(this.screen.as_ref().and_then(|screen| screen.window));
            windows.extend(this.submenu.as_ref().and_then(|open| open.window));
            // A window that a control in a panel opened, such as a select's
            // list, counts as one of the menu's windows.
            windows.extend(children_of(&windows, cx));
            // When a click moves the keyboard from one of the menu's windows
            // to another, the window that loses it reports first. The check
            // waits `ACTIVATION_SETTLE`, so that the other window has
            // reported by then.
            this.activation = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ACTIVATION_SETTLE).await;
                let active = windows.iter().any(|window| {
                    window
                        .update(cx, |_, window, _| window.is_window_active())
                        .unwrap_or(false)
                });
                if !active {
                    let _ = this.update(cx, |this, cx| this.dismiss(cx));
                }
            }));
        });
        self._subscriptions.push(subscription);
    }
}

/// Closes a window after the current update has ended, and removes it from
/// the record of pop-up windows.
pub(super) fn close_window(window: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        forget(window, cx);
        let _ = window.update(cx, |_, window, _| window.remove_window());
    });
}

/// Turns off the shadow that macOS gives the window. The system draws its
/// shadow and an edge highlight around whatever a transparent window draws,
/// and around a panel these look like a second border. The panel draws its
/// own shadow. GPUI has no option that turns the system's shadow off, so
/// this function calls AppKit directly.
#[cfg(target_os = "macos")]
pub(crate) fn without_system_shadow(window: &Window) {
    use crate::platform::macos;
    macos::with_ns_window(window, macos::without_shadow);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn without_system_shadow(_: &Window) {}

/// Returns the point that the window of a menu opened at `pointer` is scaled
/// in about. The point is `pointer`, or the point of `panel` nearest to it
/// when the panel was moved away from the pointer to fit. It is measured
/// from the centre of the window `bounds` with y upwards, which is what
/// `platform::macos::pop_in` takes.
fn pop_point(pointer: (f32, f32), panel: Rect, bounds: Rect) -> (f32, f32) {
    let x = pointer.0.clamp(panel.x, panel.x + panel.w) - bounds.x;
    let y = pointer.1.clamp(panel.y, panel.y + panel.h) - bounds.y;
    (x - bounds.w / 2., bounds.h / 2. - y)
}

/// Turns off the animation that macOS plays when a window appears. With a
/// point in `about`, it also scales the window in about that point, from
/// `motion::MENU_IN_SCALE` to full size over `motion::MENU_IN`.
#[cfg(target_os = "macos")]
fn pop_in(window: &Window, about: Option<(f32, f32)>) {
    use crate::{motion, platform::macos};
    macos::with_ns_window(window, |ns_window| {
        macos::without_appear_animation(ns_window);
        if let Some(about) = about {
            macos::pop_in(
                ns_window,
                about,
                motion::MENU_IN_SCALE,
                motion::MENU_IN.as_secs_f64(),
                motion::EASE_SCALE_IN.points(),
            );
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn pop_in(_: &Window, _: Option<(f32, f32)>) {}

#[cfg(test)]
mod tests {
    use super::{Rect, pop_point};

    #[test]
    fn a_menu_pops_in_from_where_it_was_asked_for() {
        let bounds = Rect {
            x: 60.,
            y: 160.,
            w: 350.,
            h: 480.,
        };
        let panel = Rect {
            x: 100.,
            y: 200.,
            w: 270.,
            h: 400.,
        };
        // The panel opened below and to the right of the pointer. The point
        // is the panel's top-left corner, which is left of and above the
        // window's centre.
        assert_eq!(pop_point((100., 200.), panel, bounds), (-135., 200.));
        // The panel was moved up to fit. The point is the pointer, which is
        // part of the way down the panel's left edge.
        assert_eq!(pop_point((100., 500.), panel, bounds), (-135., -100.));
        // The panel was moved left and up so far that the pointer is outside
        // it. The point is the panel's bottom-right corner.
        assert_eq!(pop_point((900., 900.), panel, bounds), (135., -200.));
    }
}
