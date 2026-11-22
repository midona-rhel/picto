//! Manages the pop-up windows that a menu's panels are shown in. This file
//! holds the record of the open menu windows and of the windows that
//! controls inside them opened, the code that opens and closes these
//! windows on each system, and the code that closes the menu when none of
//! its windows has the keyboard or the system closes one of them.
use super::layout::{Rect, Row, place};
use super::{
    ACTIVATION_SETTLE, MARGIN, Menu, Presentation, Role, SHADOW_ROOM, SUBMENU_GAP, Screen,
    VIEWPORT_INSET,
};
use gpui::popup::{PopupAnchor, PopupConstraintAdjustment, PopupGravity, PopupOptions};
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
/// windows that were opened from it. They are closed before the caller
/// closes the menu window itself: a Wayland compositor ends the connection
/// of a client that closes a pop-up before the pop-ups anchored to it.
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

/// Returns whether the compositor places pop-up windows, which is the case
/// on Wayland. A Wayland client cannot give a window a place on the display.
/// It opens a pop-up (`xdg_popup`) at a rectangle of its parent window, and
/// the compositor keeps the pop-up on the display. On macOS, Windows and X11
/// the app gives each pop-up window its rectangle on the display.
pub(crate) fn compositor_places_pop_ups(cx: &App) -> bool {
    cfg!(any(target_os = "linux", target_os = "freebsd")) && cx.compositor_name() == "Wayland"
}

/// Returns the room that the pop-up window of a menu's panel leaves around
/// the panel: `SHADOW_ROOM`, for the panel's shadow. On Windows it is 0
/// where the system blurs behind the menu (`window_chrome::system_blurs`):
/// there the blur is the backdrop of the whole window, so the window has
/// exactly the panel's rectangle, and Windows draws the window's shadow and
/// rounds its corners.
fn shadow_room(window: &Window) -> f32 {
    if cfg!(target_os = "windows") && crate::ui::window_chrome::system_blurs(window) {
        0.
    } else {
        SHADOW_ROOM
    }
}

impl Menu {
    // The methods below open, close and watch the menu's pop-up windows.

    /// Returns whether the root panel's window always has the size of the
    /// panel as it is drawn now. It has on Wayland, where the compositor
    /// keeps the window on the display and so must know the panel's real
    /// size, and on Windows where the window has no room around the panel.
    /// Elsewhere the window is sized once, for the tallest the panel can
    /// become while searching, and the panel moves inside it.
    fn window_follows_panel(&self) -> bool {
        self.anchored || self.room == 0.
    }

    /// Returns the part of the display that the root panel's window covers
    /// when the panel has the rectangle `panel`.
    fn root_window_bounds(&self, panel: Rect, area: Rect) -> Rect {
        if self.anchored {
            // The compositor decides where the window is, so the room is not
            // cut off at the edge of an area.
            panel.outset(self.room)
        } else {
            panel.outset(self.room).intersect(area)
        }
    }

    /// Returns the part of the display that a submenu's window covers when
    /// the submenu's panel has the rectangle `panel` and opened from the
    /// panel `parent`.
    pub(super) fn submenu_window_bounds(&self, panel: Rect, parent: Rect, area: Rect) -> Rect {
        if self.anchored {
            // The compositor may open the submenu on the other side of its
            // parent, so the window keeps its room on both sides. The
            // pointer reaches the parent through that room, because the
            // room is not part of the window's content region.
            panel.outset(self.room)
        } else {
            Rect::submenu_window(panel, parent, self.room, area)
        }
    }

    /// Finds the display that the owner's window is on and where the
    /// window's content starts on that display, and opens the pop-up window
    /// of the root panel. When the window has no display, the menu is drawn
    /// inside the owner's window instead.
    pub(super) fn open_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(display) = window.display(cx) else {
            self.presentation = Presentation::InWindow;
            return;
        };
        self.anchored = compositor_places_pop_ups(cx);
        self.room = shadow_room(window);
        // The display's area and the origin of the owner's content are
        // converted to the points of the interface zoom, because the menu
        // is laid out in those points. `open_pop_up` multiplies by the zoom
        // again for the frames of the windows.
        let display_area = Rect::from_bounds(crate::zoom::screen_to_ui(
            display.visible_bounds(),
            window.ui_zoom(),
        ));
        let (area, origin) = if self.anchored {
            // On Wayland the app does not know where its window is on the
            // display. The menu is laid out in the coordinates of the
            // owner's window, in an area that has the display's size and
            // starts `MARGIN` up and left of the point the menu opens at.
            // `place` therefore puts the panel's corner at that point and
            // limits the panel's height to what the display can show. The
            // compositor then moves the pop-up as far as it needs to keep
            // it on the display.
            let pointer = (f32::from(self.position.x), f32::from(self.position.y));
            (
                Rect {
                    x: pointer.0 - MARGIN,
                    y: pointer.1 - MARGIN,
                    w: display_area.w,
                    h: display_area.h,
                },
                (0., 0.),
            )
        } else {
            (display_area, crate::zoom::content_origin_in_ui(window))
        };
        self.screen = Some(Screen {
            area,
            origin,
            display: display.id(),
            bounds: Rect::default(),
            window: None,
        });
        let (area, pointer) = self.area();
        let bounds = if self.window_follows_panel() {
            self.root_window_bounds(self.root_rect(&self.rows()), area)
        } else {
            let tallest = self.tallest();
            let (x, y) = place(pointer, self.width, tallest, area);
            let rect = Rect {
                x,
                y,
                w: self.width,
                h: tallest.min(area.h - VIEWPORT_INSET),
            };
            self.root_window_bounds(rect, area)
        };
        if let Some(screen) = &mut self.screen {
            screen.bounds = bounds;
        }
        self.open_pop_up(Role::Root, cx);
    }

    /// Gives the root panel's window the size of the panel as it is drawn
    /// with `rows`, where the window follows the panel
    /// ([`Menu::window_follows_panel`]). The panel's height changes while
    /// the user types in the search row. On Wayland the window is resized,
    /// and the compositor places it again. On Windows the window is moved
    /// and resized to the panel's rectangle.
    pub(super) fn fit_root_window(&mut self, rows: &[Row], window: &mut Window, cx: &App) {
        if !self.window_follows_panel() {
            return;
        }
        let (area, _) = self.area();
        let wanted = self.root_window_bounds(self.root_rect(rows), area);
        let Some(screen) = &mut self.screen else {
            return;
        };
        if screen.bounds == wanted {
            return;
        }
        screen.bounds = wanted;
        let frame = crate::zoom::ui_to_screen(wanted.to_bounds(), window.ui_zoom());
        if self.anchored {
            window.resize(frame.size);
        } else {
            crate::ui::window_chrome::set_panel_window_frame(window, wanted.to_bounds(), cx);
        }
    }

    /// Returns how the pop-up window of `role` is opened on Wayland: as a
    /// pop-up (`xdg_popup`) that the compositor places beside a rectangle
    /// of its parent window. Returns `None` when the parent window is not
    /// open.
    ///
    /// The compositor places the window's geometry, which is the panel with
    /// `MARGIN` around it, so the panel keeps `MARGIN` from the edges of
    /// the display as it does on the other systems. The rest of the room
    /// around the panel holds only the panel's shadow and may leave the
    /// display.
    ///
    /// The root panel's corner is put at the point the menu was opened at,
    /// and the compositor slides the pop-up along both axes to keep it on
    /// the display. The root pop-up takes the grab: it then has the
    /// keyboard, and the compositor closes it when the user clicks on
    /// another application or on the desktop.
    ///
    /// A submenu's parent is the root pop-up. It opens `SUBMENU_GAP` to the
    /// right of the root panel. Where it does not fit there, the compositor
    /// puts it at the same distance on the left, and it slides the submenu
    /// up or down. A submenu takes no grab, so the keyboard stays in the
    /// root pop-up.
    fn anchored_kind(&self, role: Role, zoom: f32) -> Option<WindowKind> {
        let screen = self.screen.as_ref()?;
        let on_screen = |length: f32| px(length * zoom);
        let geometry_inset = on_screen(self.room - MARGIN);
        match role {
            Role::Root => Some(WindowKind::AnchoredPopup(PopupOptions {
                parent: self.main?,
                anchor_rect: Bounds::new(
                    point(
                        on_screen(f32::from(self.position.x)),
                        on_screen(f32::from(self.position.y)),
                    ),
                    size(px(1.), px(1.)),
                ),
                anchor: PopupAnchor::TopLeft,
                gravity: PopupGravity::BottomRight,
                constraint_adjustment: PopupConstraintAdjustment::SLIDE_X
                    | PopupConstraintAdjustment::SLIDE_Y,
                // The geometry starts `MARGIN` up and left of the panel.
                offset: point(on_screen(-MARGIN), on_screen(-MARGIN)),
                geometry_inset,
                grab: true,
            })),
            Role::Submenu(_) => {
                let open = self.submenu.as_ref()?;
                let parent = self.root_rect(&self.rows());
                // The anchor rectangle is in the coordinates of the root
                // pop-up's window. It is the root panel made narrower by
                // `MARGIN - SUBMENU_GAP` on each side, at the height where
                // the submenu's geometry starts. The submenu's geometry
                // touches this rectangle's right or left edge, which puts
                // the submenu's panel `SUBMENU_GAP` from the root panel on
                // either side.
                let narrower = MARGIN - SUBMENU_GAP;
                Some(WindowKind::AnchoredPopup(PopupOptions {
                    parent: screen.window?,
                    anchor_rect: Bounds::new(
                        point(
                            on_screen(parent.x + narrower - screen.bounds.x),
                            on_screen(open.rect.y - MARGIN - screen.bounds.y),
                        ),
                        size(on_screen(parent.w - 2. * narrower), px(1.)),
                    ),
                    anchor: PopupAnchor::TopRight,
                    gravity: PopupGravity::BottomRight,
                    constraint_adjustment: PopupConstraintAdjustment::FLIP_X
                        | PopupConstraintAdjustment::SLIDE_Y,
                    offset: Point::default(),
                    geometry_inset,
                    grab: false,
                }))
            }
        }
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
        // On macOS the window of the root panel is scaled in about the point
        // the menu was opened at, or about the nearest point of the panel
        // when the panel was moved away from that point to fit
        // (`pop_point`). The window of a submenu is shown without an
        // animation.
        let pop = match role {
            Role::Root => {
                let (_, pointer) = self.area();
                let panel = self.root_rect(&self.rows());
                Some(pop_point(pointer, panel, bounds))
            }
            Role::Submenu(_) => None,
        };
        let display = screen.display;
        let anchored = self.anchored;
        let focus = match role {
            Role::Root => self.focus_handle(cx),
            Role::Submenu(_) => self.focus.clone(),
        };
        let menu = cx.entity();
        cx.defer(move |cx| {
            let kind = menu.update(cx, |this, cx| {
                let wanted = this.closing.is_none()
                    && match role {
                        Role::Root => true,
                        Role::Submenu(serial) => this
                            .submenu
                            .as_ref()
                            .is_some_and(|open| open.serial == serial),
                    };
                if !wanted {
                    return None;
                }
                let kind = if anchored {
                    this.anchored_kind(role, cx.ui_zoom())
                } else {
                    Some(WindowKind::PopUp)
                };
                if kind.is_some() {
                    this.opening = Some(role);
                }
                // `None` here means that the pop-up's parent window is not
                // open. A root panel is then drawn inside the owner's
                // window, and a submenu is closed.
                match (&kind, role) {
                    (Some(_), _) => {}
                    (None, Role::Root) => {
                        this.presentation = Presentation::InWindow;
                        this.screen = None;
                        cx.notify();
                    }
                    (None, Role::Submenu(_)) => this.close_submenu(cx),
                }
                kind
            });
            let Some(kind) = kind else {
                return;
            };
            let options = WindowOptions {
                // For a pop-up that the compositor places, only the size of
                // these bounds is used.
                window_bounds: Some(WindowBounds::Windowed(crate::zoom::ui_to_screen(
                    bounds.to_bounds(),
                    cx.ui_zoom(),
                ))),
                display_id: Some(display),
                titlebar: None,
                focus: false,
                show: true,
                kind,
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
            let asked_to_close = menu.downgrade();
            let opened = cx
                .open_window(options, move |window, cx| {
                    crate::ui::window_chrome::prepare_panel_window(window);
                    pop_in(window, None);
                    // The first call only turns off the system's own
                    // animation. The scale animation is started on the
                    // first frame the window shows, because one started
                    // now would have ended before the window is visible.
                    window.on_next_frame(move |window, _| pop_in(window, pop));
                    // The system asks a pop-up window to close when the user
                    // presses a mouse button outside the app's windows (X11).
                    // The menu is dismissed, and it closes its windows itself
                    // when it has faded out.
                    window.on_window_should_close(cx, move |_, cx| {
                        let _ = asked_to_close.update(cx, |this, cx| this.dismiss(cx));
                        false
                    });
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

    /// Closes the menu's pop-up windows: first the submenu's, then the root
    /// panel's, because a Wayland compositor requires that a pop-up is
    /// closed before the pop-up it is anchored to. If one of them had the
    /// keyboard (on macOS a click in a panel makes its window the key
    /// window), the owner's window is activated, so that it has the
    /// keyboard again.
    pub(super) fn close_pop_ups(&mut self, cx: &mut Context<Self>) {
        let mut windows = Vec::new();
        if let Some(open) = &mut self.submenu {
            windows.extend(open.window.take());
        }
        if let Some(screen) = &mut self.screen {
            windows.extend(screen.window.take());
        }
        let main = self.main;
        let anchored = self.anchored;
        cx.defer(move |cx| {
            let mut watched = windows.clone();
            watched.extend(children_of(&windows, cx));
            // On Wayland the keyboard goes back to the owner's window by
            // itself when the pop-up that grabbed it closes.
            let had_keyboard = !anchored
                && watched.iter().any(|window| {
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

    /// Called when any window of the app has closed. When it was one of the
    /// menu's pop-up windows and the menu did not close it itself, the
    /// system closed it, and the menu is dismissed. A Wayland compositor
    /// closes the root pop-up when the user clicks on another application
    /// or on the desktop.
    pub(super) fn window_closed(&mut self, closed: WindowId, cx: &mut Context<Self>) {
        let is = |window: Option<AnyWindowHandle>| {
            window.is_some_and(|window| window.window_id() == closed)
        };
        let root = is(self.screen.as_ref().and_then(|screen| screen.window));
        let submenu = is(self.submenu.as_ref().and_then(|open| open.window));
        if root || submenu {
            self.dismiss(cx);
        }
    }

    /// Starts watching the activation of `window`, once for each window.
    /// When a watched window is activated or deactivated and none of the
    /// menu's windows has the keyboard afterwards, the menu closes. This
    /// happens when the user switches to another application, or clicks on
    /// one. A pop-up window stays above the windows of other applications,
    /// so it must not stay open then.
    ///
    /// On Windows and X11 a pop-up window never has the keyboard. There the
    /// menu closes at once when the owner's window is no longer the active
    /// one. On macOS and Wayland the keyboard can be in a pop-up window, so
    /// the menu waits `ACTIVATION_SETTLE` and then looks at all its windows.
    pub(super) fn watch_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = window.window_handle();
        if self.presentation != Presentation::PopUpWindows || self.watched.contains(&handle) {
            return;
        }
        self.watched.push(handle);
        let subscription = cx.observe_window_activation(window, |this, window, cx| {
            if !cfg!(target_os = "macos") && !this.anchored {
                if this.main == Some(window.window_handle()) && !window.is_window_active() {
                    this.dismiss(cx);
                }
                return;
            }
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

/// Returns whether the system scales the root panel's window in when it
/// opens ([`pop_in`]). Only macOS does. On Windows and Linux a program
/// cannot scale what one of its windows shows, and GPUI cannot scale an
/// element with its children, so the root panel fades in there over
/// `motion::MENU_IN`, as a menu drawn inside the owner's window does.
pub(super) const fn window_scales_in() -> bool {
    cfg!(target_os = "macos")
}

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

/// Does nothing on Windows and Linux, where the window is not scaled in
/// ([`window_scales_in`]).
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
