//! The root view of the main window. It lays the window out the way
//! `AppShell.tsx` does in production. At the top is the titlebar row; the left
//! block of that row is always as wide as the sidebar. Below the titlebar is
//! the body, which holds the sidebar on the left and, in the centre, the grid,
//! the viewer or a screen. The inspector is on the right and spans the
//! window's full height. The sidebar and the inspector are called rails in
//! this code. The user can hide either one, and it then slides out of the
//! window. Each part is a separate cached view, so this root view only
//! positions them.
//!
//! The user can turn on the system's frosted material for the two rails
//! (`ui::window_chrome::show_rail_material`). The rails are then filled with
//! a partly transparent colour, through which the blurred picture of what is
//! behind the window is seen, and this view draws the app background only in
//! the rest of the window.
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
use crate::i18n::t;
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

/// A rail that belongs to a screen. The shell draws `view` where the inspector
/// normally is, and gives it the inspector's width, resize edge, show/hide
/// toggle and titlebar strip. The shell draws it only while `shows` returns
/// true; the screen uses `shows` to say that it is showing the page the rail
/// belongs to.
pub struct ScreenRail {
    pub view: AnyView,
    pub shows: Box<dyn Fn(&App) -> bool>,
}

pub struct Shell {
    session: Entity<Session>,
    sidebar: Entity<Sidebar>,
    toolbar: Entity<Toolbar>,
    grid: Entity<GridView>,
    inspector: Entity<Inspector>,
    viewer: Entity<Viewer>,
    /// Quick Look. While it is open it is drawn over the whole window.
    quick_look: Entity<QuickLook>,
    tags: Entity<screens::tags::TagsScreen>,
    subscriptions: Entity<screens::subscriptions::SubscriptionsScreen>,
    /// The rails that screens have registered. Each one is drawn where the
    /// inspector normally is while its `shows` returns true. The only one so
    /// far is the rail of a subscription's page.
    screen_rails: Vec<(Screen, ScreenRail)>,
    duplicates: Entity<screens::duplicates::DuplicatesScreen>,
    focus: FocusHandle,
    sidebar_collapsed: bool,
    inspector_collapsed: bool,
    inspector_pinned: bool,
    /// The widths of the sidebar and the inspector, in pixels. The user
    /// changes a width by dragging the rail's inner edge.
    sidebar_width: f32,
    inspector_width: f32,
    /// The drag of one of those edges that is in progress, if there is one.
    resizing: Option<rails::Resize>,
    /// How far each rail has slid out of the window: 0 when the rail is fully
    /// shown and 1 when it is fully hidden.
    rails: motion::Fades,
    /// `inspector_available` is whether the last frame had room for the
    /// inspector: the grid, the viewer and a screen with a rail of its own
    /// have it, and any other screen uses that width itself.
    /// `inspector_presence` is the inspector's opacity. When
    /// `inspector_available` changes because the user opened or left a screen,
    /// the inspector fades in or out over 170 ms (`data-motion='fade'` in
    /// production). It does not slide then; it slides only when the user hides
    /// or shows it.
    inspector_available: Option<bool>,
    inspector_presence: motion::Fade,
    /// True from the moment the viewer is closed by navigating to another
    /// destination until the centre of the window has finished fading out.
    /// During that time the viewer is still drawn in the centre and fades out
    /// with it, and after that the destination replaces it. Production does
    /// the same: `MediaView` stays inside the workspace surface while that
    /// surface fades out.
    viewer_exit: bool,
    /// The context menu that is open, if there is one. The grid, the sidebar,
    /// the filter bar and the other parts of the window all show their menus
    /// through this one host, which draws the menu above the rest of the
    /// window. When the menu closes, keyboard focus goes back to the shell's
    /// `focus` handle, which the grid also uses.
    menu: ui::menu::Host,
    /// The filter bar. It is drawn in a row between the titlebar and the grid.
    filter_bar: Entity<FilterBar>,
    /// The panels of the view menu while that menu is open. The panels have
    /// switches that hide and show the sidebar and the inspector, so the shell
    /// keeps the panels here to tell them when a rail is hidden or shown.
    /// `_view_panels` holds the subscriptions to the panels' events.
    view_panels: Vec<Entity<view_menu::ViewOptions>>,
    _view_panels: Vec<Subscription>,
    /// The state of the item menus: the items the open menu acts on, the tags
    /// that "Copy Tags" copied, and the confirmation dialog that a menu row
    /// may open.
    items: item_menu::ItemMenus,
    /// Holds the imports, the import dialogs, the task dialog and the task
    /// bar.
    imports: Entity<crate::imports::Imports>,
    /// Holds the exports, the Picto Packs and the dialogs of both.
    exports: Entity<crate::export::Exports>,
    /// The batch rename dialog.
    batch_rename: Entity<batch_rename::BatchRename>,
    /// The library cover dialog.
    library_cover: Entity<library_cover::LibraryCover>,
    /// The update dialog, which is `ApplicationUpdateHost` in production.
    updates: Entity<crate::updates::dialog::UpdateDialog>,
    /// The paths of the files that the user is dragging over the window from
    /// another application. It is `None` again once the files are dropped or
    /// dragged out of the window.
    file_drag: Option<Vec<PathBuf>>,
    /// The state of reviewing Inbox items. `reviewer` holds the reviews in
    /// progress. `review_flash` is the flash drawn on a button of the review
    /// pill after it is pressed. `pending_review` is a review that is waiting
    /// for the next page of items to load. `review_keys` records whether a
    /// review key is being held down.
    reviewer: crate::viewer::review::Reviewer,
    review_flash: Option<crate::viewer::review::Flash>,
    pending_review: Option<review::Pending>,
    review_keys: review::ReviewKeys,
    /// The review button that a click gave keyboard focus to, and whether a
    /// key has been pressed since that click. The button's focus ring is drawn
    /// only after such a key press.
    review_focus: Option<(crate::viewer::review::Decision, bool)>,
    /// The notifications. They are drawn above the rest of the window.
    notifications: Entity<ui::notify::Host>,
    /// The log window while it is open, together with the two subscriptions
    /// that close it: one to the panel's `Closed` event and one that watches
    /// for the Escape key.
    diagnostics: Option<(Entity<crate::diagnostics::panel::Panel>, [Subscription; 2])>,
    /// `cloud` is the library's cloud sync state. The cloud sync bar and the
    /// dialog of a blocking sync are drawn from it. `cloud_focus` is the focus
    /// handle that has keyboard focus while that dialog is open.
    cloud: Entity<crate::cloud::Cloud>,
    cloud_focus: FocusHandle,
    /// The widths of the left and right rails in the last frame that was laid
    /// out. The cloud sync bar is drawn between them so that it does not cover
    /// a rail.
    cloud_rails: (f32, f32),
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// The library's session.
    pub fn session(&self) -> &Entity<Session> {
        &self.session
    }

    /// The library's cloud sync state.
    pub fn cloud(&self) -> &Entity<crate::cloud::Cloud> {
        &self.cloud
    }

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
        // While the frame meter is on, check once a second whether a second
        // has passed with no frame drawn. If so, mark the next frame as the
        // meter's own (`idle_look`) and draw the shell again, so that the
        // meter can show "idle".
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
            // When the system switches between its light and dark appearance,
            // apply the colour scheme from the settings again, so that the
            // "Auto" theme changes with the system.
            cx.observe_window_appearance(window, |_, _, cx| {
                let scheme = crate::settings::current(cx).color_scheme.clone();
                crate::theme::show(&scheme, cx);
            }),
            // The user turned the frosted material behind the sidebar and the
            // inspector on or off (Settings > General > Appearance), or a
            // library with another choice was opened. Make this window show
            // or stop showing the material.
            cx.observe_global_in::<ui::window_chrome::RailMaterial>(window, |_, window, cx| {
                ui::window_chrome::show_rail_material(window, cx)
            }),
            // Whether the system can blur what is behind the window can
            // change while the app runs: on macOS the user can turn Reduce
            // Transparency on or off in System Settings. The user has to leave
            // this window to do that, so check again each time the window
            // becomes the active one.
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    ui::window_chrome::show_rail_material(window, cx);
                }
            }),
            cx.subscribe(&quick_look, |_, _, _: &quick_look::Toggled, cx| cx.notify()),
            cx.subscribe_in(
                &grid,
                window,
                |this, _, event: &grid::Activated, window, cx| {
                    this.activate(event.ordinal, event.middle, window, cx)
                },
            ),
            // The shell decides whether the centre of the window shows the
            // grid, the viewer or a screen, so it is drawn again when the
            // viewer opens or closes and when the destination changes. When
            // the viewer closes because the user navigated to another
            // destination, `viewer_exit` is set so that the viewer stays drawn
            // while the centre fades out.
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
            // A subfolder tile in the grid has the same context menu as that
            // folder has in the sidebar, so the sidebar builds the menu and
            // carries out its rows.
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
            // Pressing Space in the collection editor opens Quick Look on the
            // member of the collection that the event names.
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
            // The sidebar builds its own context menus and carries out their
            // rows. The shell only shows them, through the same menu host that
            // shows the grid's menus.
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
            // The Inbox review pill is drawn for the item that is open in the
            // media view or Quick Look, so the shell is drawn again when the
            // selection or the list of items changes while one of them is
            // open. When the list of items changes, the shell also tells the
            // reviewer, which continues a review that was waiting for more
            // items.
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
        let subscriptions_screen = cx.new(|cx| {
            screens::subscriptions::SubscriptionsScreen::new(
                session.clone(),
                thumbnails.clone(),
                window,
                cx,
            )
        });
        let subscription_rail = screens::subscriptions::rail(&subscriptions_screen, cx);
        let subscription_controls = screens::subscriptions::controls(&subscriptions_screen, cx);
        let subscription_note = screens::subscriptions::note(&subscriptions_screen, cx);
        let toolbar = cx.new(|cx| Toolbar::new(session.clone(), &viewer, cx));
        let filter_bar = cx.new(|cx| FilterBar::new(session.clone(), cx));
        let imports = cx.new(|cx| crate::imports::Imports::new(session.clone(), focus.clone(), cx));
        let exports = cx.new(|cx| crate::export::Exports::new(session.clone(), focus.clone(), cx));
        let batch_rename =
            cx.new(|cx| batch_rename::BatchRename::new(session.clone(), focus.clone(), cx));
        let library_cover =
            cx.new(|cx| library_cover::LibraryCover::new(session.clone(), focus.clone(), cx));
        let updates = cx.new(|cx| crate::updates::dialog::UpdateDialog::new(focus.clone(), cx));
        let mut subscriptions = subscriptions;
        // Start with the sidebar and the inspector hidden or shown as the
        // saved state (`crate::state`) records them, which is how the user
        // last left them.
        let kept = crate::state::get(cx).clone();
        let mut rails = motion::Fades::default();
        rails.set(("sidebar", 0), kept.sidebar_collapsed, Default::default());
        rails.set(
            ("inspector", 0),
            kept.inspector_collapsed,
            Default::default(),
        );
        subscriptions.extend([
            // The shell lays out the filter bar above the grid, so the shell
            // is drawn again when the filter bar changes, because the bar's
            // height may have changed.
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
            // Update the application menu when the selection, the list of
            // items, the destination or the viewer changes, and when the
            // imports change. Which menu rows are enabled depends on the
            // selection, on the destination and on whether an import may
            // start.
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
        let cloud = cx.new(|cx| crate::cloud::Cloud::new(session.clone(), cx));
        session.update(cx, |session, _| session.cloud = Some(cloud.clone()));
        subscriptions.push(cx.observe_in(&cloud, window, |this, cloud, window, cx| {
            // While a blocking cloud sync runs, its dialog has keyboard focus
            // through `cloud_focus`. When the sync is no longer blocking and
            // `cloud_focus` still has focus, give keyboard focus back to the
            // shell.
            let blocking = cloud.read(cx).status.as_ref().is_some_and(|status| {
                status.state == "reconciling" && status.blocking
            });
            if !blocking && this.cloud_focus.is_focused(window) {
                window.focus(&this.focus, cx);
            }
            cx.notify();
        }));
        crate::app_menu::set_main_window(window.window_handle(), cx);
        // Give the toolbar each screen's titlebar controls. While a screen is
        // shown, the toolbar shows that screen's controls where it otherwise
        // shows the grid's controls. The Subscriptions screen also gives the
        // toolbar the count that is shown after its title.
        let controls = duplicates.read(cx).toolbar();
        let tag_controls = tags.read(cx).toolbar();
        toolbar.update(cx, |toolbar, _| {
            toolbar.set_screen_controls(Screen::Duplicates, controls.into());
            toolbar.set_screen_controls(Screen::Tags, tag_controls.into());
            toolbar.set_screen_controls(Screen::Subscriptions, subscription_controls);
            toolbar.set_screen_note(Screen::Subscriptions, subscription_note);
        });
        Self {
            sidebar,
            toolbar,
            inspector,
            viewer,
            quick_look,
            tags,
            subscriptions: subscriptions_screen,
            screen_rails: vec![(Screen::Subscriptions, subscription_rail)],
            duplicates,
            grid,
            session,
            focus: focus.clone(),
            sidebar_collapsed: kept.sidebar_collapsed,
            inspector_collapsed: kept.inspector_collapsed,
            inspector_pinned: false,
            sidebar_width: kept.sidebar_width,
            // In a debug build, setting the environment variable
            // `PICTO_GPUI_INSPECTOR`, for example to `272`, opens the
            // inspector at that width instead of the saved width. It is for
            // comparing this window with an Electron window whose inspector
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
            updates,
            file_drag: None,
            reviewer: Default::default(),
            review_flash: None,
            pending_review: None,
            review_keys: Default::default(),
            review_focus: None,
            notifications: cx.new(ui::notify::Host::new),
            diagnostics: None,
            cloud,
            cloud_focus: cx.focus_handle(),
            cloud_rails: (0., 0.),
            _subscriptions: subscriptions,
        }
    }

    /// Opens the view menu (`GridViewMenu` in production) at `position`, and
    /// tells the toolbar to keep its "View options" button highlighted while
    /// the menu is open.
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

    /// Stores the view panels of `menu` for as long as the menu is open. When
    /// the user uses a panel's sidebar or inspector switch, the shell hides or
    /// shows that rail, and `toggle_rails` then writes the new state of both
    /// rails back to the panels. The panels are dropped when the menu is
    /// released.
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

    /// Hides or shows the sidebar when `sidebar` is true, and the inspector
    /// when `inspector` is true, and saves the new state. Both rails slide
    /// over the same 100 ms, so when they are hidden together they move in
    /// step (in production the sidebar takes 50 ms). The content between the
    /// rails gets its new width immediately and is laid out for that width
    /// once; the rails are drawn over it while they slide. While the window
    /// shows the system's frosted material, the content is not drawn under a
    /// sliding rail (see `Shell::render`), so it is drawn again in each frame
    /// of the slide, at the same width.
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
            self.rails.set(
                ("sidebar", 0),
                self.sidebar_collapsed,
                motion::INSPECTOR_SLIDE,
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
        let (sidebar, inspector) = (self.sidebar_collapsed, self.inspector_collapsed);
        crate::state::update(cx, |state| {
            state.sidebar_collapsed = sidebar;
            state.inspector_collapsed = inspector;
        });
        cx.notify();
    }

    /// Starts a drag of the resize edge of `rail`, with the pointer at the
    /// horizontal position `x`. It records the rail's width at that moment.
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

    /// Ends the drag of a rail's edge, if one is in progress, and saves the
    /// widths of both rails.
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

    /// Reports whether the media view is open, whether Quick Look is open and
    /// whether exactly one item is selected. `trigger` uses these to decide
    /// what a key press or a click does.
    fn trigger_state(&self, cx: &App) -> trigger::State {
        let session = self.session.read(cx);
        trigger::State {
            media_view: session.viewer,
            quick_look: self.quick_look.read(cx).is_open(),
            single: session.selection_count() == 1 && session.selected_item().is_some(),
        }
    }

    /// Handles a double-click on the tile at `ordinal`, or a middle-click when
    /// `middle` is true. What the click does is chosen in the Controls
    /// settings (`onTileDoubleClick` and `onTileMiddleClick` in production).
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
            // A middle-click opens only the clicked item in a detail window,
            // with no other items to step through.
            trigger::Action::OpenWindow => self.open_in_window(vec![ordinal], ordinal, cx),
            trigger::Action::OpenDefault => self.open_default(ordinal, cx),
            action => self.perform(action, window, cx),
        }
    }

    /// Carries out `action`, which `trigger` chose for a key press or a click.
    /// `OpenWindow` and `OpenDefault` do nothing here, because `activate`
    /// handles them.
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

    /// Opens the item at the ordinal `start` in a detail window, as
    /// `openGridItemInWindow` does in production. The detail window can step
    /// through the items at `ordinals`. An ordinal whose item is not among the
    /// items the session has loaded is left out, and nothing opens if `start`
    /// is such an ordinal.
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

    /// Opens the first media file of the item at `ordinal` in the application
    /// that the system uses for that kind of file, as `openDefaultAppForHash`
    /// does in production.
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

    /// Moves the selection in the grid. With Shift held, it extends the
    /// selection instead.
    fn move_selection(&mut self, action: nav::Move, window: &Window, cx: &mut Context<Self>) {
        // Quick Look shows the selected item, so while Quick Look is open a
        // move never extends the selection, even with Shift held.
        let extend = window.modifiers().shift && !self.quick_look.read(cx).is_open();
        self.grid
            .update(cx, |grid, cx| grid.move_selection(action, extend, cx));
    }

    /// The left block of the titlebar. It is as wide as the sidebar even while
    /// the sidebar is hidden, so the toolbar beside it does not move when the
    /// sidebar is hidden or shown. It holds the Settings and Toggle Panels
    /// buttons, and the application menu button where the window has one. Its
    /// left padding (`TITLEBAR_INSET`) keeps the buttons clear of the macOS
    /// window buttons (the traffic lights).
    fn titlebar_left(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let translucent = ui::window_chrome::rails_translucent(cx);
        let rail_fill = ui::window_chrome::rail_fill(cx);
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
            // The block has the app background colour and a 1px right border.
            // While the sidebar is shown, the rail colour fills the area
            // inside the border. The border is therefore drawn over the app
            // background and not over the rail colour. Production's CSS does
            // this with `background-clip: padding-box`. While the window shows
            // the frosted material, the block has no background of its own:
            // `Shell::render` draws the app background under the border and,
            // while the sidebar is hidden, under the whole block.
            .when(!translucent, |block| block.bg(theme.background()))
            .border_r_1()
            .border_color(if self.sidebar_collapsed {
                gpui::transparent_black().into()
            } else {
                theme.border_secondary()
            })
            .children((!self.sidebar_collapsed).then(|| div().absolute().inset_0().bg(rail_fill)))
            // Where the application menu is shown inside the window (Windows
            // and Linux), it is a button at the left of this block
            // (`ApplicationMenuButton` in production). A flexible spacer after
            // it keeps the other buttons at the right.
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
                    .tip(ui::tip(t("Application menu")))
                    .on_click(cx.listener(|this, _, window, cx| this.open_app_menu(window, cx)))
                    .into_any_element()
            }))
            .children(crate::app_menu::in_window().then(|| div().flex_1()))
            .child(
                ui::icon_button("open-settings", "settings")
                    .tip(ui::tip(t("Settings")).shortcut("file.settings", cx))
                    .on_click(|_, _, cx| crate::settings::open(cx)),
            )
            .child(
                ui::icon_button("toggle-panels", "toolbar-panel")
                    .glyph(14.)
                    .tip(ui::tip(t("Toggle Panels")).shortcut("view.toggleBothPanels", cx))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_rails(true, true, cx)))
                    .on_secondary(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        this.open_panels_menu(event.position, window, cx)
                    })),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::meter::frame_started();
        crate::stats::rendered(crate::stats::View::Shell);
        let theme = theme::theme(cx);
        // The grid advances the session's transition between destinations each
        // time it is drawn (`advance_transition`). While a screen is shown, or
        // while the closed viewer is fading out in the centre, the grid is not
        // drawn, so the shell advances the transition here.
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
        // A viewer that was closed by navigating to another destination stays
        // drawn only while the centre fades out, so clear `viewer_exit` once
        // the fade-out is over.
        if !matches!(transition, Transition::FadingOut(_)) {
            self.viewer_exit = false;
        }
        // When a rail is hidden, the centre gets the rail's width immediately.
        // Only the rail's own slide out of the window is animated.
        let sidebar_width = self.sidebar_width;
        let left = if self.sidebar_collapsed {
            0.
        } else {
            sidebar_width
        };
        // The inspector is shown beside the grid and the viewer. A screen uses
        // the window's full width, including the right-hand part of the
        // titlebar (`inspectorAvailable` in production). The exception is a
        // screen that shows a rail of its own, which so far is only a
        // subscription's page: that rail is drawn where the inspector is, with
        // the inspector's width and show/hide toggle. A screen goes on showing
        // the same page while it fades out, so when the user goes between a
        // page with a rail and the grid or the viewer, the rail's area stays
        // where it is and only its content changes.
        let screen_rail = screen
            .filter(|_| !viewer_open)
            .and_then(|screen| self.screen_rails.iter().find(|(owner, _)| *owner == screen))
            .map(|(_, rail)| rail)
            .filter(|rail| (rail.shows)(cx))
            .map(|rail| rail.view.clone());
        let rail_shown = screen_rail.is_some();
        let inspector_available = screen.is_none() || viewer_open || rail_shown;
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
        self.cloud_rails = (left, if inspector_available { right } else { 0. });
        let drop_overlay = self.drop_overlay(cx);
        // While the window shows the system's frosted material
        // (`ui::window_chrome::show_rail_material`), the sidebar and the
        // inspector are filled with a partly transparent colour, and the
        // material is seen through them. Everything else in the window has to
        // be opaque, and nothing may be drawn under a rail, because it would
        // be seen through the rail. `sidebar_visible` and `inspector_visible`
        // are the widths of the parts of the two rails that are inside the
        // window in this frame; they change while a rail slides.
        let translucent = ui::window_chrome::rails_translucent(cx);
        let rail_fill = ui::window_chrome::rail_fill(cx);
        let sidebar_visible = if translucent {
            sidebar_width - sidebar_width * sidebar_hidden
        } else {
            0.
        };
        // The inspector shows the material only while it is fully present.
        // While it fades in or out because the user opened or left a screen,
        // it is drawn with the opaque rail colour over the centre, as it is
        // without the material.
        let inspector_translucent = translucent && inspector_available && inspector_shown >= 1.;
        let inspector_visible = if inspector_translucent {
            self.inspector_width - self.inspector_width * inspector_hidden
        } else {
            0.
        };

        let center = div()
            .absolute()
            .top_0()
            .bottom_0()
            .child(match screen {
                _ if viewer_open => self.viewer.clone().cached(fill()).into_any_element(),
                // The viewer was closed by navigating to another destination.
                // It is still drawn here, at the transition's opacity, so it
                // fades out as the grid would.
                _ if self.viewer_exit => div()
                    .size_full()
                    .opacity(fade)
                    .child(self.viewer.clone().cached(fill()))
                    .into_any_element(),
                // A screen is drawn at the transition's opacity, so it fades
                // out and in when the destination changes, as the grid does.
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
                // While the viewer is open the grid is not drawn, but its view
                // is kept, so it still has its scroll position and thumbnails
                // when the viewer closes. When the filter bar is open, it is
                // drawn in a row above the grid. Its left and right margins
                // keep it between the positions of the sidebar and the
                // inspector even while one of them is hidden, so it stays in
                // line with the toolbar in the titlebar.
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
        // When a rail is hidden, the centre is laid out for its new width at
        // once and the rail slides out over it. With the material shown, the
        // part of the centre that is still under the sliding rail is not
        // drawn: the centre is clipped to the area between the two rails'
        // visible parts.
        let center = if translucent {
            let (clip_left, clip_right) = (left.max(sidebar_visible), right.max(inspector_visible));
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(clip_left))
                .right(px(clip_right))
                .overflow_hidden()
                .child(
                    center
                        .left(px(left - clip_left))
                        .right(px(right - clip_right)),
                )
        } else {
            center.left(px(left)).right(px(right))
        };
        // With the material shown, the root has no background, so the app
        // background is drawn here as two rectangles: one for the titlebar
        // row and one for the body. Each starts 1px inside the sidebar, under
        // the sidebar's right border, and ends where the visible part of the
        // inspector starts. In the titlebar row the sidebar's part is the
        // left block, which does not slide; while the sidebar is hidden the
        // rectangle covers that block too.
        let backgrounds = translucent.then(|| {
            let titlebar_left = if self.sidebar_collapsed {
                0.
            } else {
                sidebar_width - 1.
            };
            [
                div()
                    .absolute()
                    .top_0()
                    .h(px(theme::TITLEBAR_HEIGHT))
                    .left(px(titlebar_left))
                    .right(px(inspector_visible))
                    .bg(theme.background()),
                div()
                    .absolute()
                    .top(px(theme::TITLEBAR_HEIGHT))
                    .bottom_0()
                    .left(px((sidebar_visible - 1.).max(0.)))
                    .right(px(inspector_visible))
                    .bg(theme.background()),
            ]
        });
        let sidebar = (sidebar_hidden < 1.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(-sidebar_width * sidebar_hidden))
                .w(px(sidebar_width))
                .child(self.sidebar.clone().cached(fill()))
        });
        // The inspector rail spans the window's full height, including its
        // strip of the titlebar, and all of it slides together when the user
        // hides or shows it. While it fades out because a screen was opened,
        // it is still drawn at its own width, on top of the centre content,
        // which already uses the window's full width.
        let dragging = |rail| self.resizing.is_some_and(|resize| resize.rail == rail);
        let edge_listeners = |edge: Stateful<Div>, rail, cx: &mut Context<Self>| {
            edge.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.start_resize(rail, event.position.x);
                    cx.stop_propagation();
                }),
            )
            // The resize edge occludes what is under it, so while the pointer
            // is on the edge the root's mouse-up listener does not get the
            // release. The edge therefore ends the drag itself.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_resize(cx)),
            )
        };
        // The sidebar's resize edge spans the window's full height, including
        // the titlebar. It is 5px wide and its right side is at the sidebar's
        // right border.
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
                .bg(if inspector_translucent {
                    rail_fill
                } else {
                    theme.rail()
                })
                .child(
                    ui::drag_region("inspector-titlebar", window, cx)
                        .h(px(theme::TITLEBAR_HEIGHT))
                        .flex_none()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .children((!rail_shown).then(|| {
                            // The Pin button. While the inspector is pinned,
                            // it goes on showing what it showed when it was
                            // pinned and does not change when the selection
                            // changes.
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
                                t("Unpin")
                            } else {
                                t("Pin")
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.inspector_pinned = !this.inspector_pinned;
                                let pinned = this.inspector_pinned;
                                this.inspector
                                    .update(cx, |inspector, cx| inspector.set_pinned(pinned, cx));
                                cx.notify();
                            }))
                        }))
                        .children(ui::window_chrome::caption_buttons(
                            "inspector-caption",
                            window,
                        )),
                )
                .child(div().flex_1().min_h_0().child(match screen_rail {
                    Some(rail) => rail.cached(fill()).into_any_element(),
                    None => self.inspector.clone().cached(fill()).into_any_element(),
                }))
                // The inspector's left edge is its resize handle. It is a 5px
                // strip positioned over the rail's content, so it takes up no
                // width in the layout.
                .child(inspector_edge)
        });
        // On Windows and Linux the app draws the window's minimise, maximise
        // and close buttons. They are in the inspector's titlebar strip while
        // the inspector is drawn, and otherwise in the part of the titlebar
        // that is reserved for the inspector.
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
                // The right-hand part of the titlebar is as wide as the
                // inspector. It stays reserved, and still drags the window,
                // while the inspector is hidden, and it then holds the
                // window's buttons (`.titlebarInspectorHidden` in production).
                // While a screen uses the inspector's width, this part has no
                // fixed width and holds only the window's buttons, at the
                // right end of the toolbar.
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
            .when(!translucent, |root| root.bg(theme.background()))
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            // This is the line height that CSS `line-height: normal` gives for
            // the system font.
            .line_height(px(theme::LINE_HEIGHT))
            .track_focus(&self.focus)
            // The key context includes `Grid` only while the grid itself is
            // shown, because the grid's shortcuts are bound to that context.
            // While Quick Look is open over the grid, `Grid` is left out so
            // that Quick Look gets those keys.
            .key_context(if screen.is_none() && !viewer_open && !quick_look_open {
                "Browser Grid"
            } else {
                "Browser"
            })
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if let Some(resize) = this.resizing {
                    // If the left button is no longer pressed, the window
                    // missed the release event, so end the drag here.
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
            // A review key reviews one item per press and ignores the key's
            // auto-repeat while it is held down, so record here that the key
            // was released.
            .on_key_up(cx.listener(|this, _: &KeyUpEvent, _, _| this.review_keys.released()))
            // This watcher is drawn before every view so that it sees scroll
            // input first. After an Inbox review moves on to the next item, it
            // swallows the scroll events that the system keeps sending from a
            // trackpad flick that began on the previous item.
            .child(ui::momentum::watcher())
            .children(backgrounds.into_iter().flatten())
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
                // The `file.openNewWindow` shortcut opens the focused item in
                // a detail window, or the first selected item when no item has
                // focus. The detail window can step through the rest of the
                // selection.
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
                // Escape closes the open menu. If no menu is open, it closes
                // the viewer. If the viewer is not open and the grid is shown
                // with a selection, it clears the selection. Otherwise the
                // action is passed on.
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
        // Quick Look covers the whole window, including the titlebar and the
        // rails, and is drawn below the context menus (`--layer-overlay` in
        // production).
        if quick_look_open {
            root = root.child(self.quick_look.clone());
        }
        // The log window is drawn along the bottom of the window. It is above
        // Quick Look (in production it has `--z-overlay` and is mounted after
        // Quick Look) and below the menus and dialogs.
        if let Some((panel, _)) = &self.diagnostics {
            root = root.child(panel.clone());
        }
        // The view of the imports in progress is drawn below the menus and
        // dialogs.
        root = root.child(self.imports.clone());
        root = root.child(self.exports.clone());
        root = root.child(self.batch_rename.clone());
        root = root.child(self.library_cover.clone());
        root = root.child(self.updates.clone());
        // The cloud sync bar is drawn along the bottom of the window. While a
        // sync blocks the window, a progress dialog is drawn instead of the
        // bar and has keyboard focus.
        root = root.children(crate::cloud::bar::bar(
            &self.cloud,
            self.cloud_rails,
            &self.cloud_focus,
            window,
            cx,
        ));
        // Handle the actions of the shortcuts in the shortcut registry
        // (`settings::registry`) that act on the window's content.
        root = self.shortcut_actions(root, cx);
        root = self.import_actions(root, cx);
        // Handle the actions of the application menu's rows that act on the
        // window's content.
        root = self.menu_actions(root, cx);
        root = self.export_actions(root, cx);
        root = self.file_drop(root, cx);

        root = root.children(self.menu.layer(window, cx));
        root = root.children(self.group_organizer(window, cx));
        root = root.children(self.confirmation(window, cx));
        // The Inbox review pill is drawn over the media view and Quick Look.
        // It is also drawn over the confirmation dialog that rejecting an item
        // may open, and the review waits for the answer to that dialog (in
        // production the pill has `--layer-modal` and comes after the dialog).
        root = root.children(self.review_controls(cx));
        // The inspector's tag picker and folder picker are added to the root
        // here so that they are drawn over the rest of the window.
        let (tag_picker, folder_picker) = self.inspector.read(cx).pickers();
        root = root
            .child(tag_picker)
            .child(folder_picker)
            .child(self.inspector.read(cx).ai_tagger());
        let (tag_dialog, folder_dialog) = self.inspector.read(cx).dialogs();
        root = root.child(tag_dialog).child(folder_dialog);
        // The filter bar has its own tag picker and folder picker, which work
        // in filter mode. They are added to the root in the same way.
        let (tag_picker, folder_picker) = self.filter_bar.read(cx).pickers();
        root = root.child(tag_picker).child(folder_picker);
        // The notifications are drawn over every other layer
        // (`--layer-notification` in production).
        root = root.child(self.notifications.clone());
        // A video player that is in fullscreen fills the whole window and is
        // drawn over everything else.
        root = root.children(crate::viewer::video::fullscreen_layer(window, cx));
        // On Linux, a window without a system frame gets edges that resize it.
        // They are drawn over the content.
        root = root.children(ui::window_chrome::resize_edges(window));
        // The window's rim is drawn over everything (`.shellMac::after` in
        // production).
        root = root.children(ui::window_chrome::rim(window, cx));
        // The frame meter, which is shown only when debugging, is the last
        // child, so that its paint marks the end of the frame's drawing.
        root = root.children(crate::meter::overlay(cx));
        // Ask for another frame while a rail is still sliding.
        if self.rails.animating() {
            window.request_animation_frame();
        }
        // This outer element is the accessibility node for the shell's `focus`
        // handle, which has keyboard focus whenever no control has it.
        // Assistive technology then treats this node as focused, and the grid
        // can report the tile the keyboard is on as the active one inside it
        // (`aria_active_descendant`).
        div()
            .id("shell")
            .role(Role::Group)
            .a11y_focus(&self.focus)
            .size_full()
            .child(root)
    }
}
