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
use crate::ui::menu::{self, Menu};
use crate::viewer::Viewer;
use crate::{motion, theme, ui};
use gpui::*;
use sidebar::Sidebar;
use std::path::PathBuf;
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
    /// The open context menu. It stays here while it fades out.
    menu: Option<Entity<Menu>>,
    _menu: Option<Subscription>,
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
            cx.subscribe_in(
                &grid,
                window,
                |this, _, event: &grid::MenuRequested, window, cx| {
                    this.open_menu(event.0, window, cx)
                },
            ),
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
            _menu: None,
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

    /// Opens the grid's context menu for the selection, at the pointer. The
    /// rows follow `gridContextMenu.tsx`, for the actions this prototype has.
    fn open_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let (count, trash) = {
            let session = self.session.read(cx);
            let trash = session
                .nav
                .get(session.displayed)
                .is_some_and(|entry| entry.label == "Trash");
            (session.selection.len(), trash)
        };
        if count == 0 {
            return;
        }
        let mut entries: Vec<menu::Entry> = Vec::new();
        if count == 1 {
            entries.extend([
                menu::item("open-default", "Open with Default App")
                    .icon("menu-external-link")
                    .shortcut("⇧↩")
                    .into(),
                menu::item("reveal", "Reveal in Finder")
                    .icon("menu-brand-finder")
                    .shortcut("⌘↩")
                    .into(),
                menu::Entry::Separator,
                menu::item("copy-path", "Copy File Path")
                    .icon("menu-clipboard-copy")
                    .shortcut("⌘⌥C")
                    .into(),
                menu::item("copy-name", "Copy Name")
                    .icon("menu-clipboard-copy")
                    .into(),
                menu::Entry::Separator,
                menu::item("rename", "Rename")
                    .icon("menu-rename")
                    .shortcut("⌃R")
                    .into(),
                menu::Entry::Separator,
            ]);
        }
        let several = |one: &str, many: String| if count > 1 { many } else { one.to_owned() };
        if trash {
            entries.push(
                menu::item(
                    "restore",
                    several("Restore", format!("Restore {count} Items")),
                )
                .icon("menu-arrow-back-up")
                .into(),
            );
            // The one destructive row. This prototype borrows the library's
            // files, so it never deletes them: the row is shown, not offered.
            entries.push(
                menu::item(
                    "delete",
                    several("Delete Permanently", format!("Delete {count} Permanently")),
                )
                .icon("menu-trash")
                .shortcut("⌘⌫")
                .danger(true)
                .disabled(true)
                .into(),
            );
        } else {
            entries.push(
                menu::item(
                    "trash",
                    several("Move to Trash", format!("Move {count} to Trash")),
                )
                .icon("menu-trash")
                .shortcut("⌘⌫")
                .into(),
            );
        }
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self._menu =
            Some(
                cx.subscribe_in(&menu, window, |this, _, event: &menu::Event, window, cx| {
                    let menu::Event::Chosen(action) = event;
                    this.run_menu_action(action, window, cx);
                }),
            );
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(menu);
        cx.notify();
    }

    /// Dismisses the context menu, leaving it to fade out. Returns whether
    /// one was open.
    fn close_menu(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.menu {
            Some(menu) if !menu.read(cx).finished() => {
                menu.update(cx, |menu, cx| menu.dismiss(cx));
                true
            }
            _ => false,
        }
    }

    fn run_menu_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        // The file the menu acts on: the shown member of the focused item.
        let (name, path) = {
            let session = self.session.read(cx);
            (
                session.selected_item().map(|item| item.name.clone()),
                session
                    .details
                    .as_ref()
                    .and_then(|details| details.media.get(session.member))
                    .map(|media| media.file_path.clone()),
            )
        };
        match action {
            "open-default" => {
                if let Some(path) = path {
                    cx.open_with_system(std::path::Path::new(&path));
                }
            }
            "reveal" => {
                if let Some(path) = path {
                    cx.reveal_path(std::path::Path::new(&path));
                }
            }
            "copy-path" => {
                if let Some(path) = path {
                    cx.write_to_clipboard(ClipboardItem::new_string(path));
                }
            }
            "copy-name" => {
                if let Some(name) = name {
                    cx.write_to_clipboard(ClipboardItem::new_string(name));
                }
            }
            "rename" => {
                self.inspector
                    .update(cx, |inspector, cx| inspector.edit_name(window, cx));
                return;
            }
            "trash" => self.session.update(cx, |session, cx| {
                session.move_selection_to(picto_library::Lifecycle::Trash, cx)
            }),
            "restore" => self.session.update(cx, |session, cx| {
                session.move_selection_to(picto_library::Lifecycle::Active, cx)
            }),
            _ => {}
        }
        window.focus(&self.focus, cx);
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
                if !this.close_menu(cx) {
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

        // A dismissed menu stays until its 80 ms fade is over; the keyboard
        // then goes back to the grid.
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.read(cx).finished())
        {
            self.menu = None;
            self._menu = None;
            // Unless its action moved the keyboard somewhere, such as into
            // the name field.
            if window.focused(cx).is_none() {
                window.focus(&self.focus, cx);
            }
        }
        root = root.children(self.menu.clone());
        // Keep painting while a rail slides.
        if self.rails.animating() {
            window.request_animation_frame();
        }
        root
    }
}
