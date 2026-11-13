//! The pop-up windows a menu's panels are shown in: the registry of open
//! menu windows and the windows their controls opened, opening and closing
//! them, and the watch that closes the menu when none has the keyboard.
use super::layout::{Rect, place};
use super::{ACTIVATION_SETTLE, Menu, Presentation, Role, SHADOW_ROOM, Screen, VIEWPORT_INSET};
use gpui::*;

/// The pop-up windows menus have open, and the pop-up windows a control
/// inside one of them opened (a select's list), each with the menu window it
/// was opened from. A child closes with its menu window, and while a child
/// has the keyboard its menu counts as having it.
#[derive(Default)]
struct PopUps {
    menus: Vec<AnyWindowHandle>,
    children: Vec<(AnyWindowHandle, AnyWindowHandle)>,
}

impl Global for PopUps {}

/// Whether this window is one of a menu's pop-up windows, where a control
/// cannot open anything over the window and must open a pop-up of its own
/// ([`adopt`]).
pub fn is_pop_up(window: &Window, cx: &App) -> bool {
    let handle = window.window_handle();
    cx.try_global::<PopUps>()
        .is_some_and(|pop_ups| pop_ups.menus.contains(&handle))
}

/// Records a pop-up window opened from a menu's pop-up window, so that it
/// closes with it.
pub fn adopt(owner: AnyWindowHandle, child: AnyWindowHandle, cx: &mut App) {
    let pop_ups = cx.default_global::<PopUps>();
    if pop_ups.menus.contains(&owner) {
        pop_ups.children.push((owner, child));
    } else {
        // The menu closed while the child opened.
        close_window(child, cx);
    }
}

/// Forgets a child pop-up its control closed itself.
pub fn release(child: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<PopUps>()
        .children
        .retain(|(_, open)| *open != child);
}

fn register(window: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<PopUps>().menus.push(window);
}

/// Forgets a menu window that is closing, and closes its children.
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
    // ── Pop-up windows ──────────────────────────────────────────────

    /// Finds the display and the owner window's place on it, and opens the
    /// root panel's window. Falls back to the overlay when there is no
    /// display to place it on.
    pub(super) fn open_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(display) = window.display(cx) else {
            self.presentation = Presentation::InWindow;
            return;
        };
        // `Window::bounds` is the frame on its display; the content starts
        // below any system titlebar, at the frame's bottom less its height.
        let frame = window.bounds();
        let content = window.viewport_size();
        let origin = (
            f32::from(frame.origin.x),
            f32::from(frame.origin.y) + f32::from(frame.size.height - content.height),
        );
        let area = Rect::from_bounds(display.visible_bounds());
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

    /// Opens a pop-up window for the root panel or the open submenu, once
    /// this update is over: the window draws the menu as it opens.
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
        // `menuIn`: the root panel scales in from where it was asked for.
        // (Production's `transform-origin` is the panel's corner, which is
        // its bottom edge whenever the panel was moved up to fit; the click
        // itself reads better.) Submenus appear at once.
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
                window_bounds: Some(WindowBounds::Windowed(bounds.to_bounds())),
                display_id: Some(display),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            };
            let view = menu.clone();
            let opened = cx
                .open_window(options, move |window, cx| {
                    without_system_shadow(window);
                    pop_in(window, None);
                    // The scale starts with the first frame the window
                    // shows; begun now, it would be over before then.
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
                    // The platform has no pop-up windows: draw in the window.
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

    /// Closes the pop-up windows. If one of them had taken the keyboard (a
    /// click in a panel makes it the key window), it goes back to the owner.
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

    /// Closes the menu when the keyboard leaves all of its windows: another
    /// application was chosen, or the owner window was. A pop-up stays
    /// above other applications' windows, so it must not outlive that.
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
            // A select's list opened from a panel counts as the menu's.
            windows.extend(children_of(&windows, cx));
            // When a click moves the keyboard from one of the menu's windows
            // to another, the one losing it reports first; wait for the
            // other before deciding.
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

/// Closes a window once the current update is over.
pub(super) fn close_window(window: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        forget(window, cx);
        let _ = window.update(cx, |_, window, _| window.remove_window());
    });
}

/// A transparent window still gets the system's own shadow and edge
/// highlight, traced round whatever it draws, which shows as a second, glassy
/// border round the panel. The panel draws its own shadow, so the system's
/// is turned off. GPUI has no option for this, so it asks AppKit directly.
#[cfg(target_os = "macos")]
pub(crate) fn without_system_shadow(window: &Window) {
    use crate::ui::window_chrome::appkit;
    appkit::with_ns_window(window, |ns_window| {
        // SAFETY: `setHasShadow:` is an NSWindow method taking a BOOL,
        // called on the main thread.
        unsafe {
            let send_bool: extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void, bool) =
                std::mem::transmute(appkit::send());
            send_bool(ns_window, appkit::selector(c"setHasShadow:"), false);
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn without_system_shadow(_: &Window) {}

/// The point a menu asked for at `pointer` scales in about: the pointer,
/// or the nearest point of its `panel` where the panel was moved away from
/// it to fit. Given in the window `bounds` from the window's centre with y
/// upwards, as `appkit::pop_in` takes it.
fn pop_point(pointer: (f32, f32), panel: Rect, bounds: Rect) -> (f32, f32) {
    let x = pointer.0.clamp(panel.x, panel.x + panel.w) - bounds.x;
    let y = pointer.1.clamp(panel.y, panel.y + panel.h) - bounds.y;
    (x - bounds.w / 2., bounds.h / 2. - y)
}

/// Plays `menuIn` on a pop-up window that has a point to scale about, and
/// turns off the system's own animation for every pop-up.
#[cfg(target_os = "macos")]
fn pop_in(window: &Window, about: Option<(f32, f32)>) {
    use crate::ui::window_chrome::appkit;
    appkit::with_ns_window(window, |ns_window| {
        appkit::without_appear_animation(ns_window);
        if let Some(about) = about {
            use crate::motion;
            let curve = motion::EASE_SCALE_IN;
            appkit::pop_in(
                ns_window,
                about,
                motion::MENU_IN_SCALE,
                motion::MENU_IN.as_secs_f64(),
                curve.points(),
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
        // Opened below and right of the pointer: its top-left corner, left
        // of and above the window's centre.
        assert_eq!(pop_point((100., 200.), panel, bounds), (-135., 200.));
        // Moved up to fit: the pointer is part-way down its left edge.
        assert_eq!(pop_point((100., 500.), panel, bounds), (-135., -100.));
        // Moved in from the right and bottom edges past the pointer.
        assert_eq!(pop_point((900., 900.), panel, bounds), (135., -200.));
    }
}
