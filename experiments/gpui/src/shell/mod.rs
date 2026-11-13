//! The window, arranged as `AppShell.tsx` arranges it: a titlebar row whose
//! left block keeps the sidebar's width, above a body in which the sidebar
//! and inspector are rails that slide away. Each part is its own cached view,
//! so this root only places them.
pub mod sidebar;
pub mod toolbar;

use crate::grid::{self, GridView, nav, thumbnails::Thumbnails};
use crate::inspector::Inspector;
use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::viewer::Viewer;
use crate::{motion, theme, ui};
use gpui::*;
use sidebar::Sidebar;
use std::{path::PathBuf, time::Instant};
use toolbar::Toolbar;

actions!(
    browser,
    [
        Next,
        Previous,
        Down,
        Up,
        First,
        Last,
        PageUp,
        PageDown,
        Preview,
        ClosePreview,
        SelectAll,
        ToggleSidebar,
        ToggleInspector,
        TogglePanels,
        Undo,
        Redo,
        Quit,
        DemoSvg,
        DemoFlash
    ]
);

const MENU_WIDTH: f32 = 270.;
const MENU_ACTIONS: [&str; 3] = ["Quick Look", "Copy Name", "Copy File Path"];

pub struct Shell {
    session: Entity<Session>,
    sidebar: Entity<Sidebar>,
    toolbar: Entity<Toolbar>,
    grid: Entity<GridView>,
    inspector: Entity<Inspector>,
    viewer: Entity<Viewer>,
    focus: FocusHandle,
    sidebar_collapsed: bool,
    inspector_collapsed: bool,
    inspector_pinned: bool,
    /// The inspector rail's width, which its left edge drags.
    inspector_width: f32,
    /// A drag of that edge: where the pointer started, and the width then.
    resizing: Option<(Pixels, f32)>,
    /// How far each rail has slid out of view.
    rails: motion::Fades,
    menu: Option<Point<Pixels>>,
    /// A dismissed menu, still on screen while it fades out.
    menu_closing: Option<(Point<Pixels>, Instant)>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        database: PathBuf,
        blobs: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session = cx.new(|cx| Session::new(database, blobs, cx));
        let thumbnails = cx.new(Thumbnails::new);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let grid =
            cx.new(|cx| GridView::new(session.clone(), thumbnails.clone(), focus.clone(), cx));
        let subscriptions = vec![
            cx.subscribe(&session, |_, _, change: &Change, cx| {
                if matches!(change, Change::Viewer) {
                    cx.notify();
                }
            }),
            cx.subscribe(&grid, |this, _, event: &grid::MenuRequested, cx| {
                this.menu = Some(event.0);
                this.menu_closing = None;
                cx.notify();
            }),
        ];
        Self {
            sidebar: cx.new(|cx| Sidebar::new(session.clone(), thumbnails.clone(), cx)),
            toolbar: cx.new(|cx| Toolbar::new(session.clone(), cx)),
            inspector: cx.new(|cx| Inspector::new(session.clone(), thumbnails, window, cx)),
            viewer: cx.new(|cx| Viewer::new(session.clone(), cx)),
            grid,
            session,
            focus,
            sidebar_collapsed: false,
            inspector_collapsed: false,
            inspector_pinned: false,
            // `PICTO_GPUI_INSPECTOR=272` opens at a stored width, to compare
            // against an Electron window whose inspector was resized.
            inspector_width: std::env::var("PICTO_GPUI_INSPECTOR")
                .ok()
                .and_then(|width| width.parse().ok())
                .unwrap_or(theme::INSPECTOR_WIDTH)
                .clamp(theme::INSPECTOR_MIN, theme::INSPECTOR_MAX),
            resizing: None,
            rails: motion::Fades::default(),
            menu: None,
            menu_closing: None,
            _subscriptions: subscriptions,
        }
    }

    /// Hides or shows the rails. The sidebar slides out over 50 ms and the
    /// inspector over 100 ms; the grid takes the freed width at once.
    fn toggle_rails(&mut self, sidebar: bool, inspector: bool, cx: &mut Context<Self>) {
        if sidebar {
            self.sidebar_collapsed = !self.sidebar_collapsed;
            self.rails.set(
                ("sidebar", 0),
                self.sidebar_collapsed,
                motion::SIDEBAR_SLIDE,
            );
        }
        if inspector {
            self.inspector_collapsed = !self.inspector_collapsed;
            self.rails.set(
                ("inspector", 0),
                self.inspector_collapsed,
                motion::INSPECTOR_SLIDE,
            );
        }
        cx.notify();
    }

    /// Dismisses the context menu, leaving it to fade out. Returns whether one
    /// was open.
    fn close_menu(&mut self) -> bool {
        match self.menu.take() {
            Some(position) => {
                self.menu_closing = Some((position, Instant::now()));
                true
            }
            None => false,
        }
    }

    fn run_menu_action(&mut self, index: usize, cx: &mut Context<Self>) {
        // A menu that is fading out no longer acts.
        if !self.close_menu() {
            return;
        }
        match index {
            0 => self
                .session
                .update(cx, |session, cx| session.open_viewer(cx)),
            1 => {
                if let Some(name) = self
                    .session
                    .read(cx)
                    .selected_item()
                    .map(|item| item.name.clone())
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(name));
                }
            }
            _ => {
                let path = {
                    let session = self.session.read(cx);
                    session
                        .details
                        .as_ref()
                        .and_then(|details| details.media.get(session.member))
                        .map(|media| media.file_path.clone())
                };
                if let Some(path) = path {
                    cx.write_to_clipboard(ClipboardItem::new_string(path));
                }
            }
        }
        cx.notify();
    }

    /// Moves the selection; with shift held, extends it instead.
    fn move_selection(&mut self, action: nav::Move, window: &Window, cx: &mut Context<Self>) {
        let extend = window.modifiers().shift;
        self.grid
            .update(cx, |grid, cx| grid.move_selection(action, extend, cx));
    }

    /// The titlebar's left block. It keeps the sidebar's width when the
    /// sidebar is hidden, so the toolbar never shifts, and holds the shared
    /// window actions clear of the traffic lights.
    fn titlebar_left(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        ui::drag_region("titlebar-left", window, cx)
            .w(px(theme::SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .pl(px(74.))
            .pr(px(8.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .relative()
            // The edge is drawn over the app background, with the rail
            // surface inside it (`background-clip: padding-box`).
            .bg(theme.background())
            .border_r_1()
            .border_color(if self.sidebar_collapsed {
                gpui::transparent_black().into()
            } else {
                theme.border_secondary()
            })
            .children(
                (!self.sidebar_collapsed).then(|| div().absolute().inset_0().bg(theme.rail())),
            )
            // TODO: open the settings window.
            .child(
                ui::icon_button("open-settings", "settings")
                    .tip(ui::tip("Settings").keys(&["⌘", ","])),
            )
            .child(
                ui::icon_button("toggle-panels", "toolbar-panel")
                    .glyph(14.)
                    .tip(ui::tip("Toggle Panels").keys(&["⇥"]))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_rails(true, true, cx))),
            )
    }

    /// The floating glass surface of `surfaces.css`, without its blur.
    fn context_menu(
        &self,
        position: Point<Pixels>,
        opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let size = window.viewport_size();
        let mut menu = div()
            .absolute()
            .left(position.x.min(size.width - px(MENU_WIDTH + 8.)))
            .top(position.y.min(size.height - px(120.)))
            .w(px(MENU_WIDTH))
            .p(px(4.))
            .gap(px(2.))
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.glass_border())
            .bg(theme.glass())
            .shadow(theme::panel_shadow())
            .opacity(opacity)
            .flex()
            .flex_col();
        for (index, label) in MENU_ACTIONS.into_iter().enumerate() {
            menu = menu.child(
                div()
                    .id(("context-action", index))
                    .h(px(theme::CONTROL_SM))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .rounded(px(theme::RADIUS_MD))
                    .hover(move |style| style.bg(theme.hover_overlay()))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.run_menu_action(index, cx))),
            );
        }
        menu
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Shell);
        let theme = theme::theme(cx);
        let viewer_open = self.session.read(cx).viewer;
        // The rails give up their width at once; only their own slide is animated.
        let left = if self.sidebar_collapsed {
            0.
        } else {
            theme::SIDEBAR_WIDTH
        };
        let inspector_width = self.inspector_width;
        let right = if self.inspector_collapsed {
            0.
        } else {
            inspector_width
        };
        let sidebar_hidden = self.rails.value(("sidebar", 0));
        let inspector_hidden = self.rails.value(("inspector", 0));
        let fill = || StyleRefinement::default().size_full();

        let center = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(left))
            .right(px(right))
            .child(if viewer_open {
                self.viewer.clone().cached(fill()).into_any_element()
            } else {
                // The grid keeps its scroll position and thumbnails while the
                // viewer is open; it is only not painted.
                self.grid.clone().cached(fill()).into_any_element()
            });
        let sidebar = (sidebar_hidden < 1.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(-theme::SIDEBAR_WIDTH * sidebar_hidden))
                .w(px(theme::SIDEBAR_WIDTH))
                .child(self.sidebar.clone().cached(fill()))
        });
        // The inspector rail spans the window's full height, titlebar strip
        // included, and slides as one piece.
        let inspector = (inspector_hidden < 1.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(-inspector_width * inspector_hidden))
                .w(px(inspector_width))
                .flex()
                .flex_col()
                .bg(theme.rail())
                // The rail's edge is its resize handle (`.inspectorResizeHandle`):
                // 5px wide over the content, so it takes no width itself.
                .child(
                    div()
                        .id("inspector-resize")
                        .occlude()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(5.))
                        .cursor(CursorStyle::ResizeColumn)
                        .border_l_1()
                        .border_color(theme.border_secondary())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.resizing = Some((event.position.x, this.inspector_width));
                                cx.stop_propagation();
                            }),
                        ),
                )
                .child(
                    ui::drag_region("inspector-titlebar", window, cx)
                        .h(px(theme::TITLEBAR_HEIGHT))
                        .flex_none()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(
                            // TODO: while pinned, the inspector keeps showing
                            // the item it had instead of following the selection.
                            ui::icon_button(
                                "pin-inspector",
                                if self.inspector_pinned {
                                    "pin-filled"
                                } else {
                                    "pin"
                                },
                            )
                            .glyph(14.)
                            .toggled(self.inspector_pinned)
                            .tip(ui::tip(if self.inspector_pinned {
                                "Unpin"
                            } else {
                                "Pin"
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.inspector_pinned = !this.inspector_pinned;
                                cx.notify();
                            })),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.inspector.clone().cached(fill())),
                )
        });
        let titlebar = div()
            .h(px(theme::TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .child(self.titlebar_left(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.toolbar.clone().cached(fill())),
            )
            .child(
                // The inspector's share of the titlebar stays reserved, and
                // draggable, while the inspector is hidden.
                ui::drag_region("titlebar-right", window, cx)
                    .w(px(inspector_width))
                    .flex_none()
                    .h_full(),
            );
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(center)
            .children(sidebar);

        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            // CSS `line-height: normal` for the system font.
            .line_height(px(theme::LINE_HEIGHT))
            .track_focus(&self.focus)
            .key_context("Browser")
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if let Some((from, width)) = this.resizing {
                    let wanted = width + f32::from(from - event.position.x);
                    this.inspector_width = wanted
                        .round()
                        .clamp(theme::INSPECTOR_MIN, theme::INSPECTOR_MAX);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.resizing = None),
            )
            .on_action(cx.listener(|this, _: &Next, window, cx| {
                this.move_selection(nav::Move::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Previous, window, cx| {
                this.move_selection(nav::Move::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Down, window, cx| {
                this.move_selection(nav::Move::Down, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Up, window, cx| {
                this.move_selection(nav::Move::Up, window, cx)
            }))
            .on_action(cx.listener(|this, _: &First, window, cx| {
                this.move_selection(nav::Move::First, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Last, window, cx| {
                this.move_selection(nav::Move::Last, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PageUp, window, cx| {
                this.move_selection(nav::Move::PageUp, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PageDown, window, cx| {
                this.move_selection(nav::Move::PageDown, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Preview, _, cx| {
                this.session
                    .update(cx, |session, cx| session.toggle_viewer(cx));
            }))
            .on_action(cx.listener(|this, _: &ClosePreview, _, cx| {
                // Escape closes the menu, else the viewer, else deselects.
                if !this.close_menu() {
                    this.session.update(cx, |session, cx| {
                        if session.viewer {
                            session.close_viewer(cx)
                        } else {
                            session.clear_selection(cx)
                        }
                    });
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.session
                    .update(cx, |session, cx| session.select_all(cx));
            }))
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_rails(true, false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleInspector, _, cx| this.toggle_rails(false, true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &TogglePanels, _, cx| this.toggle_rails(true, true, cx)),
            )
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.session.read(cx).undo()))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.session.read(cx).redo()))
            .on_action(cx.listener(|this, _: &DemoSvg, _, cx| {
                this.session.update(cx, |session, cx| {
                    session.show_demo("vector.svg", "image/svg+xml", cx)
                });
            }))
            .on_action(cx.listener(|this, _: &DemoFlash, _, cx| {
                this.session.update(cx, |session, cx| {
                    session.show_demo("interactive.swf", "application/x-shockwave-flash", cx)
                });
            }))
            .child(titlebar)
            .child(body)
            .children(inspector);

        // `menuOut`: a dismissed menu fades over 80 ms and takes no more input.
        let closing = self.menu_closing.and_then(|(position, started)| {
            let progress = motion::progress(started.elapsed(), motion::MENU_OUT);
            (progress < 1.).then(|| (position, 1. - motion::EASE_IN.ease(progress)))
        });
        if closing.is_none() {
            self.menu_closing = None;
        }
        if let Some(position) = self.menu {
            root = root
                .child(
                    div()
                        .id("menu-backdrop")
                        .absolute()
                        .inset_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.close_menu();
                                cx.notify();
                            }),
                        ),
                )
                .child(self.context_menu(position, 1., window, cx));
        } else if let Some((position, opacity)) = closing {
            root = root.child(self.context_menu(position, opacity, window, cx));
        }
        // Keep painting while a rail slides or a menu fades.
        if self.rails.animating() || self.menu_closing.is_some() {
            window.request_animation_frame();
        }
        root
    }
}
