//! The window, arranged as `AppShell.tsx` arranges it: a titlebar row whose
//! left block keeps the sidebar's width, above a body in which the sidebar
//! and inspector are rails that slide away. Each part is its own cached view,
//! so this root only places them.
mod batch_rename;
mod delete;
mod export;
mod import;
mod item_menu;
mod rails;
pub(crate) use item_menu::media_link;
pub mod library;
mod library_cover;
mod menu_commands;
mod review;
mod shortcuts;
pub mod sidebar;
pub mod toolbar;
pub mod view_menu;

use crate::backend::Screen;
use crate::grid::{
    self, GridView,
    filter_bar::{BarEvent, FilterBar},
    nav,
    thumbnails::Thumbnails,
};
use crate::inspector::Inspector;
use crate::session::{Change, Session, Transition};
use crate::theme::Text as _;
use crate::viewer::{
    Viewer, detail,
    quick_look::{self, QuickLook},
    trigger,
};
use crate::{motion, screens, theme, ui};
use gpui::prelude::FluentBuilder as _;
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
        OpenInNewWindow,
        SelectAll,
        ToggleSidebar,
        ToggleInspector,
        TogglePanels,
        Undo,
        Redo,
        Quit
    ]
);

pub struct Shell {
    session: Entity<Session>,
    sidebar: Entity<Sidebar>,
    toolbar: Entity<Toolbar>,
    grid: Entity<GridView>,
    inspector: Entity<Inspector>,
    viewer: Entity<Viewer>,
    /// Quick Look, drawn over the whole window while it is open.
    quick_look: Entity<QuickLook>,
    tags: Entity<screens::tags::TagsScreen>,
    subscriptions: Entity<screens::subscriptions::SubscriptionsScreen>,
    duplicates: Entity<screens::duplicates::DuplicatesScreen>,
    focus: FocusHandle,
    sidebar_collapsed: bool,
    inspector_collapsed: bool,
    inspector_pinned: bool,
    /// The rails' widths, which their inner edges drag.
    sidebar_width: f32,
    inspector_width: f32,
    /// A drag of one of those edges.
    resizing: Option<rails::Resize>,
    /// How far each rail has slid out of view.
    rails: motion::Fades,
    /// Whether the inspector belonged to what was shown last, and its fade
    /// when that changes: it fades in and out over 170 ms when a screen
    /// takes or gives back the window (`data-motion='fade'`), rather than
    /// sliding as a toggle does.
    inspector_available: Option<bool>,
    inspector_presence: motion::Fade,
    /// Set when the viewer closed by going to another destination: the
    /// viewer stays in the pane and fades out with it, as `MediaView` does
    /// inside the fading workspace surface, until the destination replaces it.
    viewer_exit: bool,
    /// The open context menu, the grid's, the sidebar's or the filter
    /// bar's, above everything. The keyboard goes back to the grid after.
    menu: ui::menu::Host,
    /// The filter bar under the titlebar, over the grid.
    filter_bar: Entity<FilterBar>,
    /// The view menu's panels while it is open, which show the rails.
    view_panels: Vec<Entity<view_menu::ViewOptions>>,
    _view_panels: Vec<Subscription>,
    /// The item menus' state: what the open one acts on, copied tags, and
    /// the confirmation it may ask.
    items: item_menu::ItemMenus,
    /// Imports, their dialogs and the task dialog and bar.
    imports: Entity<crate::imports::Imports>,
    /// Exports and Picto Packs, and their dialogs.
    exports: Entity<crate::export::Exports>,
    /// The batch rename dialog.
    batch_rename: Entity<batch_rename::BatchRename>,
    /// The library cover dialog.
    library_cover: Entity<library_cover::LibraryCover>,
    /// Files dragged from the system over the window, until they are
    /// dropped or leave it.
    file_drag: Option<Vec<PathBuf>>,
    /// The Inbox reviews in progress, the press flashing on the review
    /// pill, a review waiting for the next page, and whether a review key
    /// is held down.
    reviewer: crate::viewer::review::Reviewer,
    review_flash: Option<crate::viewer::review::Flash>,
    pending_review: Option<review::Pending>,
    review_keys: review::ReviewKeys,
    /// The review button a click focused, and whether a key since has
    /// made its focus ring show.
    review_focus: Option<(crate::viewer::review::Decision, bool)>,
    /// The notifications, drawn above everything.
    notifications: Entity<ui::notify::Host>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        root: PathBuf,
        application: std::sync::Arc<crate::application::Application>,
        events: std::sync::mpsc::Receiver<crate::application::Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session = cx.new(|cx| Session::new(root, application, events, cx));
        crate::settings::init(session.clone(), cx);
        crate::desktop::start(&session, cx);
        // The frame meter says "idle" once nothing has drawn for a second.
        if crate::meter::enabled() {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    if crate::meter::idle() {
                        crate::meter::idle_look();
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                }
            })
            .detach();
        }
        let thumbnails = cx.new(Thumbnails::new);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let grid =
            cx.new(|cx| GridView::new(session.clone(), thumbnails.clone(), focus.clone(), cx));
        let sidebar =
            cx.new(|cx| Sidebar::new(session.clone(), thumbnails.clone(), focus.clone(), cx));
        let viewer = cx.new(|cx| Viewer::new(session.clone(), window, cx));
        let inspector =
            cx.new(|cx| Inspector::new(session.clone(), thumbnails.clone(), window, cx));
        let quick_look = cx.new(|cx| QuickLook::new(session.clone(), window, cx));
        let subscriptions = vec![
            // The "Auto" theme follows the system's appearance as it changes.
            cx.observe_window_appearance(window, |_, _, cx| {
                let scheme = crate::settings::current(cx).color_scheme.clone();
                crate::theme::show(&scheme, cx);
            }),
            cx.subscribe(&quick_look, |_, _, _: &quick_look::Toggled, cx| cx.notify()),
            cx.subscribe_in(
                &grid,
                window,
                |this, _, event: &grid::Activated, window, cx| {
                    this.activate(event.ordinal, event.middle, window, cx)
                },
            ),
            // The shell chooses what fills the centre: the grid, the viewer
            // or a screen.
            cx.subscribe(&session, |this, session, change: &Change, cx| {
                if matches!(change, Change::Viewer) {
                    let session = session.read(cx);
                    this.viewer_exit =
                        !session.viewer && matches!(session.transition, Transition::FadingOut(_));
                }
                if matches!(change, Change::Viewer | Change::Navigation) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &grid,
                window,
                |this, _, event: &grid::MenuRequested, window, cx| {
                    this.open_tile_menu(event.0, window, cx)
                },
            ),
            // A subfolder tile's menu is the folder's: the sidebar builds it
            // and acts on it.
            cx.subscribe_in(
                &grid,
                window,
                |this, grid, event: &grid::subfolders::FolderMenuRequested, window, cx| {
                    let (node, selected, position) =
                        (event.node.clone(), event.selected.clone(), event.position);
                    let grid = grid.downgrade();
                    this.sidebar.update(cx, |sidebar, cx| {
                        sidebar.tile_menu(node, selected, position, grid, window, cx)
                    });
                },
            ),
            cx.subscribe_in(
                &grid,
                window,
                |this, _, event: &grid::EmptyMenuRequested, window, cx| {
                    this.open_empty_menu(event.0, window, cx)
                },
            ),
            cx.subscribe_in(
                &viewer,
                window,
                |this, _, event: &crate::viewer::MenuRequested, window, cx| {
                    let surface = this.viewer.read(cx).surface();
                    this.open_viewer_menu(event, surface, window, cx)
                },
            ),
            // The collection editor's Space: Quick Look on the member.
            cx.subscribe_in(
                &viewer,
                window,
                |this, _, event: &crate::viewer::MemberQuickLook, window, cx| {
                    this.quick_look.update(cx, |quick_look, cx| {
                        quick_look.open_member(event.0, window, cx)
                    });
                },
            ),
            cx.subscribe_in(
                &quick_look,
                window,
                |this, _, event: &crate::viewer::MenuRequested, window, cx| {
                    let surface = this.quick_look.read(cx).surface();
                    this.open_viewer_menu(event, surface, window, cx)
                },
            ),
            cx.subscribe_in(
                &inspector,
                window,
                |this, _, event: &crate::inspector::PreviewMenuRequested, window, cx| {
                    this.open_preview_menu(event.0, window, cx)
                },
            ),
            // The sidebar builds its own menus and acts on them; the shell
            // shows them above everything, as it does the grid's.
            cx.subscribe_in(
                &sidebar,
                window,
                |this, _, event: &sidebar::ShowMenu, window, cx| {
                    this.menu.show(event.0.clone(), window, cx)
                },
            ),
            cx.subscribe_in(
                &sidebar,
                window,
                |this, _, _: &sidebar::EmptyTrash, window, cx| this.empty_trash(window, cx),
            ),
            cx.observe_keystrokes(|this, event, _, cx| this.review_keystroke(event, cx)),
            {
                let shell = cx.entity().downgrade();
                cx.intercept_keystrokes(move |event, _, cx| {
                    let _ = shell.update(cx, |this, _| this.review_keystroke_coming(event));
                })
            },
            // The Inbox review pill follows the item being viewed.
            cx.subscribe_in(&session, window, |this, _, change: &Change, window, cx| {
                if matches!(change, Change::Results) {
                    this.review_items_changed(window, cx);
                }
                if matches!(change, Change::Selection | Change::Results) && this.viewing(cx) {
                    cx.notify();
                }
            }),
        ];
        let duplicates = cx.new(|cx| {
            screens::duplicates::DuplicatesScreen::new(
                session.clone(),
                thumbnails.clone(),
                window,
                cx,
            )
        });
        let tags = cx.new(|cx| {
            screens::tags::TagsScreen::new(session.clone(), thumbnails.clone(), window, cx)
        });
        let toolbar = cx.new(|cx| Toolbar::new(session.clone(), &viewer, cx));
        let filter_bar = cx.new(|cx| FilterBar::new(session.clone(), cx));
        let imports = cx.new(|cx| crate::imports::Imports::new(session.clone(), focus.clone(), cx));
        let exports = cx.new(|cx| crate::export::Exports::new(session.clone(), focus.clone(), cx));
        let batch_rename =
            cx.new(|cx| batch_rename::BatchRename::new(session.clone(), focus.clone(), cx));
        let library_cover =
            cx.new(|cx| library_cover::LibraryCover::new(session.clone(), focus.clone(), cx));
        let mut subscriptions = subscriptions;
        // The rails as the user left them.
        let kept = crate::state::get(cx).clone();
        let mut rails = motion::Fades::default();
        rails.set(("sidebar", 0), kept.sidebar_collapsed, Default::default());
        rails.set(
            ("inspector", 0),
            kept.inspector_collapsed,
            Default::default(),
        );
        subscriptions.extend([
            // The bar's height belongs to the layout here.
            cx.observe(&filter_bar, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &filter_bar,
                window,
                |this, _, event: &BarEvent, window, cx| match event {
                    BarEvent::Menu(menu) => this.menu.show(menu.clone(), window, cx),
                },
            ),
            cx.subscribe_in(
                &toolbar,
                window,
                |this, _, event: &toolbar::ViewMenuRequested, window, cx| {
                    this.open_view_menu(event.0, window, cx)
                },
            ),
            // The application menu follows the selection, the destination
            // and whether imports may start.
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if matches!(
                    change,
                    Change::Selection | Change::Results | Change::Navigation | Change::Viewer
                ) {
                    this.sync_menu(cx);
                }
            }),
            cx.observe(&imports, |this, _, cx| this.sync_menu(cx)),
        ]);
        crate::app_menu::set_main_window(window.window_handle(), cx);
        // A screen's titlebar controls take the grid's place in the toolbar.
        let controls = duplicates.read(cx).toolbar();
        let tag_controls = tags.read(cx).toolbar();
        toolbar.update(cx, |toolbar, _| {
            toolbar.set_screen_controls(Screen::Duplicates, controls.into());
            toolbar.set_screen_controls(Screen::Tags, tag_controls.into());
        });
        Self {
            sidebar,
            toolbar,
            inspector,
            viewer,
            quick_look,
            tags,
            subscriptions: cx.new(|cx| {
                screens::subscriptions::SubscriptionsScreen::new(
                    session.clone(),
                    thumbnails.clone(),
                    window,
                    cx,
                )
            }),
            duplicates,
            grid,
            session,
            focus: focus.clone(),
            sidebar_collapsed: kept.sidebar_collapsed,
            inspector_collapsed: kept.inspector_collapsed,
            inspector_pinned: false,
            sidebar_width: kept.sidebar_width,
            // `PICTO_GPUI_INSPECTOR=272` opens at that width whatever was
            // kept, to compare against an Electron window whose inspector
            // was resized.
            inspector_width: crate::seams::inspector_width()
                .map_or(kept.inspector_width, crate::state::inspector_width),
            resizing: None,
            rails,
            inspector_available: None,
            inspector_presence: motion::Fade::at(true),
            viewer_exit: false,
            menu: ui::menu::Host::deferred(ui::layer::MENU, ui::menu::Return::To(focus.clone())),
            filter_bar,
            view_panels: Vec::new(),
            _view_panels: Vec::new(),
            items: Default::default(),
            imports,
            exports,
            batch_rename,
            library_cover,
            file_drag: None,
            reviewer: Default::default(),
            review_flash: None,
            pending_review: None,
            review_keys: Default::default(),
            review_focus: None,
            notifications: cx.new(ui::notify::Host::new),
            _subscriptions: subscriptions,
        }
    }

    /// Opens the view menu (`GridViewMenu`) at `position`, and keeps the
    /// button lit while it is open.
    fn open_view_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (menu, panels) = view_menu::build(
            &self.session,
            position,
            !self.sidebar_collapsed,
            !self.inspector_collapsed,
            window,
            cx,
        );
        self.watch_view_panels(&menu, panels, cx);
        self.toolbar
            .update(cx, |toolbar, cx| toolbar.watch_view_menu(menu.clone(), cx));
        self.menu.show(menu, window, cx);
    }

    /// Keeps a menu's view panels while it is open: their rail switches
    /// toggle the rails, and the rails' state reaches them.
    fn watch_view_panels(
        &mut self,
        menu: &Entity<ui::menu::Menu>,
        panels: view_menu::Panels,
        cx: &mut Context<Self>,
    ) {
        let panels = panels.both();
        let mut subscriptions = panels
            .iter()
            .map(|panel| {
                cx.subscribe(
                    panel,
                    |this, _, event: &view_menu::RailToggled, cx| match event {
                        view_menu::RailToggled::Sidebar => this.toggle_rails(true, false, cx),
                        view_menu::RailToggled::Inspector => this.toggle_rails(false, true, cx),
                    },
                )
            })
            .collect::<Vec<_>>();
        subscriptions.push(cx.observe_release(menu, |this, _, _| this.view_panels.clear()));
        self.view_panels = panels.to_vec();
        self._view_panels = subscriptions;
    }

    /// Hides or shows the rails. The sidebar slides out over 50 ms and the
    /// inspector over 100 ms; the grid takes the freed width at once.
    fn toggle_rails(&mut self, sidebar: bool, inspector: bool, cx: &mut Context<Self>) {
        let shown = (
            !(self.sidebar_collapsed ^ sidebar),
            !(self.inspector_collapsed ^ inspector),
        );
        for panel in &self.view_panels {
            panel.update(cx, |panel, cx| {
                (panel.sidebar_shown, panel.inspector_shown) = shown;
                cx.notify();
            });
        }
        if sidebar {
            self.sidebar_collapsed = !self.sidebar_collapsed;
            self.rails
                .set(("sidebar", 0), self.sidebar_collapsed, motion::FAST);
        }
        if inspector {
            self.inspector_collapsed = !self.inspector_collapsed;
            self.rails.set(
                ("inspector", 0),
                self.inspector_collapsed,
                motion::INSPECTOR_SLIDE,
            );
        }
        let (sidebar, inspector) = (self.sidebar_collapsed, self.inspector_collapsed);
        crate::state::update(cx, |state| {
            state.sidebar_collapsed = sidebar;
            state.inspector_collapsed = inspector;
        });
        cx.notify();
    }

    /// Starts dragging a rail's edge.
    fn start_resize(&mut self, rail: rails::Rail, x: Pixels) {
        let width = match rail {
            rails::Rail::Sidebar => self.sidebar_width,
            rails::Rail::Inspector => self.inspector_width,
        };
        self.resizing = Some(rails::Resize {
            rail,
            from: f32::from(x),
            width,
        });
    }

    /// Ends a drag of a rail's edge and keeps the width it left.
    fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.resizing.take().is_none() {
            return;
        }
        let (sidebar, inspector) = (self.sidebar_width, self.inspector_width);
        crate::state::update(cx, |state| {
            state.sidebar_width = sidebar;
            state.inspector_width = inspector;
        });
        cx.notify();
    }

    /// What the grid has open, for deciding what a key or click does.
    fn trigger_state(&self, cx: &App) -> trigger::State {
        let session = self.session.read(cx);
        trigger::State {
            media_view: session.viewer,
            quick_look: self.quick_look.read(cx).is_open(),
            single: session.selection_count() == 1 && session.selected_item().is_some(),
        }
    }

    /// A double- or middle-click on the tile at `ordinal`, under the
    /// Controls settings (`onTileDoubleClick`, `onTileMiddleClick`).
    fn activate(
        &mut self,
        ordinal: usize,
        middle: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collection = self
            .session
            .read(cx)
            .item_at(ordinal)
            .is_some_and(|item| item.kind == picto_library::RootKind::Collection);
        let settings = crate::settings::current(cx);
        let action = if middle {
            trigger::middle_click(settings.grid_middle_click_action)
        } else {
            trigger::double_click(settings.grid_double_click_action, collection)
        };
        match action {
            // A middle-click opens the clicked item alone.
            trigger::Action::OpenWindow => self.open_in_window(vec![ordinal], ordinal, cx),
            trigger::Action::OpenDefault => self.open_default(ordinal, cx),
            action => self.perform(action, window, cx),
        }
    }

    /// Carries out what a key asked for.
    fn perform(&mut self, action: trigger::Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            trigger::Action::OpenMediaView => self
                .session
                .update(cx, |session, cx| session.open_viewer(cx)),
            trigger::Action::CloseMediaView => self
                .session
                .update(cx, |session, cx| session.close_viewer(cx)),
            trigger::Action::OpenQuickLook => self
                .quick_look
                .update(cx, |quick_look, cx| quick_look.open(window, cx)),
            trigger::Action::CloseQuickLook => self
                .quick_look
                .update(cx, |quick_look, cx| quick_look.close(window, cx)),
            trigger::Action::ScrollPage => self.grid.update(cx, |grid, cx| grid.scroll_by_page(cx)),
            trigger::Action::OpenWindow
            | trigger::Action::OpenDefault
            | trigger::Action::Nothing => {}
        }
    }

    /// `openGridItemInWindow`: opens the item at `start` in a detail window
    /// that steps through the items at `ordinals`, those the grid's window
    /// holds.
    fn open_in_window(&mut self, ordinals: Vec<usize>, start: usize, cx: &mut Context<Self>) {
        let (entries, at) = {
            let session = self.session.read(cx);
            let held = ordinals
                .into_iter()
                .filter(|ordinal| session.item_at(*ordinal).is_some())
                .collect::<Vec<_>>();
            let entries = held
                .iter()
                .filter_map(|ordinal| session.item_at(*ordinal))
                .map(detail::Entry::from)
                .collect::<Vec<_>>();
            let at = held.iter().position(|ordinal| *ordinal == start);
            (entries, at)
        };
        if let Some(at) = at {
            detail::open(self.session.clone(), entries, at, cx);
        }
    }

    /// `openDefaultAppForHash`: the item's file in the system's application.
    fn open_default(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(root) = self
            .session
            .read(cx)
            .item_at(ordinal)
            .map(|item| item.root_id)
        else {
            return;
        };
        let read = self.session.read(cx).run(move |library| {
            library
                .details(root)
                .ok()
                .and_then(|details| details.media.first().map(|media| media.file_path.clone()))
        });
        cx.spawn(async move |_, cx| {
            if let Ok(Some(path)) = read.await {
                cx.update(|cx| crate::desktop::open_default(std::path::Path::new(&path), cx));
            }
        })
        .detach();
    }

    /// Moves the selection; with shift held, extends it instead.
    fn move_selection(&mut self, action: nav::Move, window: &Window, cx: &mut Context<Self>) {
        // Quick Look's steps never extend the selection it follows.
        let extend = window.modifiers().shift && !self.quick_look.read(cx).is_open();
        self.grid
            .update(cx, |grid, cx| grid.move_selection(action, extend, cx));
    }

    /// The titlebar's left block. It keeps the sidebar's width when the
    /// sidebar is hidden, so the toolbar never shifts, and holds the shared
    /// window actions clear of the traffic lights.
    fn titlebar_left(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        ui::drag_region("titlebar-left", window, cx)
            .w(px(self.sidebar_width))
            .flex_none()
            .h_full()
            .pl(px(ui::window_chrome::TITLEBAR_INSET))
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
            // Off the Mac the application menu is a button at the left
            // (`ApplicationMenuButton`), the actions kept at the right.
            .children(crate::app_menu::in_window().then(|| {
                ui::icon_button("application-menu", "app-menu")
                    .size(theme::CONTROL_MD)
                    .glyph_element(
                        svg()
                            .path("app-menu")
                            .w(px(16.))
                            .h(px(15.))
                            .text_color(theme.text_secondary()),
                    )
                    .tip(ui::tip("Application menu"))
                    .on_click(cx.listener(|this, _, window, cx| this.open_app_menu(window, cx)))
                    .into_any_element()
            }))
            .children(crate::app_menu::in_window().then(|| div().flex_1()))
            .child(
                ui::icon_button("open-settings", "settings")
                    .tip(ui::tip("Settings").shortcut("file.settings", cx))
                    .on_click(|_, _, cx| crate::settings::open(cx)),
            )
            .child(
                ui::icon_button("toggle-panels", "toolbar-panel")
                    .glyph(14.)
                    .tip(ui::tip("Toggle Panels").shortcut("view.toggleBothPanels", cx))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_rails(true, true, cx))),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::meter::frame_started();
        crate::stats::rendered(crate::stats::View::Shell);
        let theme = theme::theme(cx);
        // While a screen shows, or a closed viewer fades out in the grid's
        // place, there is no grid to move the scope change along, so the
        // shell does.
        let exiting = self.viewer_exit;
        let quick_look_open = self.quick_look.read(cx).is_open();
        let (viewer_open, screen, fade) = self.session.update(cx, |session, cx| {
            if session.screen().is_some() || exiting {
                session.advance_transition(cx);
            }
            (
                session.viewer,
                session.screen(),
                session.transition.opacity(),
            )
        });
        let transition = self.session.read(cx).transition;
        if (screen.is_some() || self.viewer_exit) && transition.animating() {
            window.request_animation_frame();
        }
        // The viewer that closed by going elsewhere stays until the surface
        // has faded out.
        if !matches!(transition, Transition::FadingOut(_)) {
            self.viewer_exit = false;
        }
        // The rails give up their width at once; only their own slide is animated.
        let sidebar_width = self.sidebar_width;
        let left = if self.sidebar_collapsed {
            0.
        } else {
            sidebar_width
        };
        // The inspector belongs to the grid and the viewer. A screen has the
        // window's full width, titlebar included (`inspectorAvailable`).
        let inspector_available = screen.is_none() || viewer_open;
        match self.inspector_available {
            Some(was) if was != inspector_available => self
                .inspector_presence
                .set(inspector_available, motion::INSPECTOR_FADE),
            Some(_) => {}
            None => self.inspector_presence = motion::Fade::at(inspector_available),
        }
        self.inspector_available = Some(inspector_available);
        let inspector_shown = self.inspector_presence.value();
        if self.inspector_presence.animating() {
            window.request_animation_frame();
        }
        let inspector_width = if inspector_available {
            self.inspector_width
        } else {
            0.
        };
        let right = if self.inspector_collapsed {
            0.
        } else {
            inspector_width
        };
        let filter_bar = !viewer_open && screen.is_none() && self.session.read(cx).filter_bar_open;
        let sidebar_hidden = self.rails.value(("sidebar", 0));
        let inspector_hidden = self.rails.value(("inspector", 0));
        let fill = || StyleRefinement::default().size_full();
        self.place_task_bar(left, if inspector_available { right } else { 0. }, cx);
        let drop_overlay = self.drop_overlay(cx);

        let center = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(left))
            .right(px(right))
            .child(match screen {
                _ if viewer_open => self.viewer.clone().cached(fill()).into_any_element(),
                // The viewer left for another destination fades out as the
                // grid would.
                _ if self.viewer_exit => div()
                    .size_full()
                    .opacity(fade)
                    .child(self.viewer.clone().cached(fill()))
                    .into_any_element(),
                // A screen fades as the grid does when the destination changes.
                Some(screen) => div()
                    .size_full()
                    .opacity(fade)
                    .child(match screen {
                        Screen::Tags => self.tags.clone().cached(fill()).into_any_element(),
                        Screen::Subscriptions => {
                            self.subscriptions.clone().cached(fill()).into_any_element()
                        }
                        Screen::Duplicates => {
                            self.duplicates.clone().cached(fill()).into_any_element()
                        }
                    })
                    .into_any_element(),
                // The grid keeps its scroll position and thumbnails while the
                // viewer is open; it is only not painted. The filter bar sits
                // above it, aligned with the titlebar's panel edges even while
                // a rail is hidden.
                None if filter_bar => div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .ml(px(sidebar_width - left))
                            .mr(px(inspector_width - right))
                            .child(self.filter_bar.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(self.grid.clone().cached(fill())),
                    )
                    .into_any_element(),
                None => self.grid.clone().cached(fill()).into_any_element(),
            })
            .children(drop_overlay);
        let sidebar = (sidebar_hidden < 1.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(-sidebar_width * sidebar_hidden))
                .w(px(sidebar_width))
                .child(self.sidebar.clone().cached(fill()))
        });
        // The inspector rail spans the window's full height, titlebar strip
        // included, and slides as one piece.
        // While it fades out it keeps its width over the content that has
        // already taken the window's.
        let dragging = |rail| self.resizing.is_some_and(|resize| resize.rail == rail);
        let edge_listeners = |edge: Stateful<Div>, rail, cx: &mut Context<Self>| {
            edge.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.start_resize(rail, event.position.x);
                    cx.stop_propagation();
                }),
            )
            // The edge lies over the window's own release listener while
            // the pointer is on it.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_resize(cx)),
            )
        };
        // The sidebar's edge spans the window's height, titlebar included,
        // its right side on the sidebar's divider.
        let sidebar_edge = (!self.sidebar_collapsed).then(|| {
            let edge = rails::edge(
                rails::Rail::Sidebar,
                dragging(rails::Rail::Sidebar),
                window,
                cx,
            )
            .left(px(sidebar_width - 5.));
            edge_listeners(edge, rails::Rail::Sidebar, cx)
        });
        let inspector_edge = edge_listeners(
            rails::edge(
                rails::Rail::Inspector,
                dragging(rails::Rail::Inspector),
                window,
                cx,
            )
            .left_0(),
            rails::Rail::Inspector,
            cx,
        );
        let inspector = (inspector_shown > 0. && inspector_hidden < 1.).then(|| {
            let inspector_width = self.inspector_width;
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(-inspector_width * inspector_hidden))
                .w(px(inspector_width))
                .opacity(inspector_shown)
                .flex()
                .flex_col()
                .bg(theme.rail())
                .child(
                    ui::drag_region("inspector-titlebar", window, cx)
                        .h(px(theme::TITLEBAR_HEIGHT))
                        .flex_none()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(
                            // While pinned, the inspector keeps showing what
                            // it had instead of following the selection.
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
                                let pinned = this.inspector_pinned;
                                this.inspector
                                    .update(cx, |inspector, cx| inspector.set_pinned(pinned, cx));
                                cx.notify();
                            })),
                        )
                        .children(ui::window_chrome::caption_buttons(
                            "inspector-caption",
                            window,
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.inspector.clone().cached(fill())),
                )
                // The rail's edge is its resize handle: 5px wide over the
                // content, so it takes no width itself.
                .child(inspector_edge)
        });
        // The window's own buttons (Windows, Linux) go in the inspector's
        // titlebar while the inspector is drawn, else in its reserved strip.
        let caption = inspector
            .is_none()
            .then(|| ui::window_chrome::caption_buttons("titlebar-caption", window))
            .flatten();
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
                // draggable, while the inspector is hidden; it holds the
                // window's buttons then (`.titlebarInspectorHidden`), and
                // while a screen has the inspector's width, at the end of
                // the toolbar.
                ui::drag_region("titlebar-right", window, cx)
                    .when(inspector_width > 0., |region| region.w(px(inspector_width)))
                    .flex_none()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .when(caption.is_some(), |region| region.pr(px(8.)))
                    .children(caption),
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
            // `Grid` while the grid itself shows: its shortcuts need it.
            // Quick Look over the grid has the keyboard instead.
            .key_context(if screen.is_none() && !viewer_open && !quick_look_open {
                "Browser Grid"
            } else {
                "Browser"
            })
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if let Some(resize) = this.resizing {
                    // A release the window did not see ends the drag.
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.end_resize(cx);
                        return;
                    }
                    let width = resize.width_at(f32::from(event.position.x));
                    match resize.rail {
                        rails::Rail::Sidebar => this.sidebar_width = width,
                        rails::Rail::Inspector => this.inspector_width = width,
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_resize(cx)),
            )
            // A review key reviews once per press, not per repeat.
            .on_key_up(cx.listener(|this, _: &KeyUpEvent, _, _| this.review_keys.released()))
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
            .on_action(cx.listener(|this, _: &OpenInNewWindow, _, cx| {
                // `file.openNewWindow`: the shown item, with the rest of the
                // selection to step through.
                let (ordinals, start) = {
                    let session = this.session.read(cx);
                    let ordinals = session.selected_ordinals();
                    let start = session.focus_ordinal().or(ordinals.first().copied());
                    (ordinals, start)
                };
                if let Some(start) = start {
                    this.open_in_window(ordinals, start, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Preview, _, cx| this.open_selected(cx)))
            .on_action(cx.listener(|this, _: &ClosePreview, _, cx| {
                // Escape closes the menu, else the viewer, else deselects.
                if !this.menu.close(cx) {
                    let handled = this.session.update(cx, |session, cx| {
                        if session.viewer {
                            session.close_viewer(cx);
                            true
                        } else if session.screen().is_none() && !session.selection.is_empty() {
                            session.clear_selection(cx);
                            true
                        } else {
                            false
                        }
                    });
                    if !handled {
                        cx.propagate();
                    }
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
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                this.session.update(cx, |session, cx| session.undo(cx))
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                this.session.update(cx, |session, cx| session.redo(cx))
            }))
            .child(titlebar)
            .child(body)
            .children(sidebar_edge)
            .children(inspector);
        // Quick Look covers the whole window, titlebar and rails included,
        // below the context menus (`--layer-overlay`).
        if quick_look_open {
            root = root.child(self.quick_look.clone());
        }
        // Imports in progress, below the menus and dialogs.
        root = root.child(self.imports.clone());
        root = root.child(self.exports.clone());
        root = root.child(self.batch_rename.clone());
        root = root.child(self.library_cover.clone());
        // The registry's shortcuts that act on the window's content.
        root = self.shortcut_actions(root, cx);
        root = self.import_actions(root, cx);
        // The application menu's rows that act on the window's content.
        root = self.menu_actions(root, cx);
        root = self.export_actions(root, cx);
        root = self.file_drop(root, cx);

        root = root.children(self.menu.layer(window, cx));
        root = root.children(self.group_organizer(window, cx));
        root = root.children(self.confirmation(window, cx));
        // The Inbox review pill, over the media view and Quick Look, and
        // over the question a reject may ask, which it waits on
        // (`--layer-modal`, after the dialog's).
        root = root.children(self.review_controls(cx));
        // The inspector's tag and folder pickers float over everything.
        let (tag_picker, folder_picker) = self.inspector.read(cx).pickers();
        root = root.child(tag_picker).child(folder_picker);
        let (tag_dialog, folder_dialog) = self.inspector.read(cx).dialogs();
        root = root.child(tag_dialog).child(folder_dialog);
        // So are the filter bar's, in their filter mode.
        let (tag_picker, folder_picker) = self.filter_bar.read(cx).pickers();
        root = root.child(tag_picker).child(folder_picker);
        // The notifications, over every layer (`--layer-notification`).
        root = root.child(self.notifications.clone());
        // A video player that has the window to itself, over everything.
        root = root.children(crate::viewer::video::fullscreen_layer(window, cx));
        // A frameless window's resizing edges (Linux), over the content.
        root = root.children(ui::window_chrome::resize_edges(window));
        // `.shellMac::after`: the window's rim, over everything.
        root = root.children(ui::window_chrome::rim(cx));
        // The frame meter, when debugging: last, so that it ends the frame.
        root = root.children(crate::meter::overlay(cx));
        // Keep painting while a rail slides.
        if self.rails.animating() {
            window.request_animation_frame();
        }
        root
    }
}
