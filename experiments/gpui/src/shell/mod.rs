//! The window, arranged as `AppShell.tsx` arranges it: a titlebar row whose
//! left block keeps the sidebar's width, above a body in which the sidebar
//! and inspector are rails that slide away. Each part is its own cached view,
//! so this root only places them.
pub mod sidebar;
pub mod toolbar;

use crate::grid::{self, GridView, nav, thumbnails::Thumbnails};
use crate::inspector::Inspector;
use crate::session::{Change, Session};
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

    fn move_selection(&mut self, action: nav::Move, cx: &mut Context<Self>) {
        self.grid
            .update(cx, |grid, cx| grid.move_selection(action, cx));
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
            .bg(if self.sidebar_collapsed {
                theme.background()
            } else {
                theme.rail()
            })
            .border_r_1()
            .border_color(if self.sidebar_collapsed {
                rgba(0x00000000)
            } else {
                theme.border_secondary()
            })
            .child(ui::icon_button("open-settings", "settings"))
            .child(
                ui::icon_button("toggle-panels", "toolbar-panel")
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
        let theme = theme::theme(cx);
        let viewer_open = self.session.read(cx).viewer;
        // The rails give up their width at once; only their own slide is animated.
        let left = if self.sidebar_collapsed {
            0.
        } else {
            theme::SIDEBAR_WIDTH
        };
        let right = if self.inspector_collapsed {
            0.
        } else {
            theme::INSPECTOR_WIDTH
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
                .right(px(-theme::INSPECTOR_WIDTH * inspector_hidden))
                .w(px(theme::INSPECTOR_WIDTH))
                .flex()
                .flex_col()
                .bg(theme.rail())
                .border_l_1()
                .border_color(theme.border_secondary())
                .child(
                    ui::drag_region("inspector-titlebar", window, cx)
                        .h(px(theme::TITLEBAR_HEIGHT))
                        .flex_none(),
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
                    .w(px(theme::INSPECTOR_WIDTH))
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
            .text_size(px(theme::TEXT_MD))
            .track_focus(&self.focus)
            .key_context("Browser")
            .on_action(
                cx.listener(|this, _: &Next, _, cx| this.move_selection(nav::Move::Right, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Previous, _, cx| this.move_selection(nav::Move::Left, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Down, _, cx| this.move_selection(nav::Move::Down, cx)),
            )
            .on_action(cx.listener(|this, _: &Up, _, cx| this.move_selection(nav::Move::Up, cx)))
            .on_action(
                cx.listener(|this, _: &First, _, cx| this.move_selection(nav::Move::First, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Last, _, cx| this.move_selection(nav::Move::Last, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PageUp, _, cx| this.move_selection(nav::Move::PageUp, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PageDown, _, cx| {
                    this.move_selection(nav::Move::PageDown, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Preview, _, cx| {
                this.session
                    .update(cx, |session, cx| session.toggle_viewer(cx));
            }))
            .on_action(cx.listener(|this, _: &ClosePreview, _, cx| {
                if !this.close_menu() {
                    this.session
                        .update(cx, |session, cx| session.close_viewer(cx));
                }
                cx.notify();
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
