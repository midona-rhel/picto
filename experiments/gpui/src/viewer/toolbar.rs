//! The viewer toolbar (`ViewerToolbar` in `GridToolbar.tsx`), which takes the
//! titlebar's place of the grid toolbar while an item is open. Left: back to
//! the grid, the breadcrumb and the item's place in the results. Centre: the
//! zoom percentage and a logarithmic zoom slider. Right: fit, actual size,
//! previous and next.
use super::surface::{self, MediaSurface};
use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::ui::menu::{self, Menu};
use crate::ui::zoom::{self, Zoom};
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

/// The zoom menu's levels, in percent, with the action each reports.
const LEVELS: [(&str, f32); 11] = [
    ("zoom-5", 5.),
    ("zoom-10", 10.),
    ("zoom-25", 25.),
    ("zoom-50", 50.),
    ("zoom-100", 100.),
    ("zoom-125", 125.),
    ("zoom-150", 150.),
    ("zoom-200", 200.),
    ("zoom-300", 300.),
    ("zoom-400", 400.),
    ("zoom-800", 800.),
];

pub struct ViewerToolbar {
    session: Entity<Session>,
    /// The viewer, which steps to the previous or next item.
    viewer: WeakEntity<super::Viewer>,
    surface: Entity<MediaSurface>,
    zoom: Entity<Zoom>,
    /// The zoom percentage's menu. The keyboard goes back where it was
    /// before it opened.
    menu: menu::Host,
    _subscriptions: Vec<Subscription>,
}

impl ViewerToolbar {
    pub fn new(
        session: Entity<Session>,
        surface: Entity<MediaSurface>,
        viewer: WeakEntity<super::Viewer>,
        cx: &mut Context<Self>,
    ) -> Self {
        let zoom = cx.new(|_| Zoom::percent());
        let subscriptions = vec![
            cx.subscribe(&session, |_, _, change: &Change, cx| {
                if matches!(
                    change,
                    Change::Selection | Change::Results | Change::Viewer | Change::Details
                ) {
                    cx.notify();
                }
            }),
            cx.subscribe(&surface, |this, surface, _: &surface::Event, cx| {
                let percent = surface.read(cx).percent();
                this.zoom.update(cx, |zoom, cx| zoom.show(percent, cx));
                cx.notify();
            }),
            cx.subscribe(&zoom, |this, _, event: &zoom::Changed, cx| {
                let percent = event.0;
                this.surface
                    .update(cx, |surface, cx| surface.set_percent(percent, cx));
            }),
        ];
        Self {
            session,
            viewer,
            surface,
            zoom,
            menu: menu::Host::deferred(crate::ui::layer::MENU, menu::Return::Opener),
            _subscriptions: subscriptions,
        }
    }

    /// The zoom percentage's menu at the pointer: the common levels, the
    /// current one checked, then actual size and fit.
    fn open_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.surface.read(cx).percent();
        let mut entries: Vec<menu::Entry> = LEVELS
            .iter()
            .map(|(action, percent)| {
                menu::item(action, format!("{percent}%"))
                    .checked(current == *percent)
                    .into()
            })
            .collect();
        entries.push(menu::Entry::Separator);
        entries.push(
            menu::item("actual-size", "Actual size")
                .shortcut_for("view.actualSize", cx)
                .into(),
        );
        entries.push(
            menu::item("fit-window", "Fit to window")
                .shortcut_for("view.fitWindow", cx)
                .into(),
        );
        // `showSearch: false`, as `GridToolbar.tsx` opens it.
        let menu = cx.new(|cx| Menu::new(entries, position, cx).search(false));
        self.menu.open(menu, window, cx, |this, _, action, _, cx| {
            this.surface.update(cx, |surface, cx| match action {
                "actual-size" => surface.actual_size(cx),
                "fit-window" => surface.fit(cx),
                action => {
                    if let Some((_, percent)) = LEVELS.iter().find(|(name, _)| *name == action) {
                        surface.set_percent(*percent, cx);
                    }
                }
            });
        });
    }

    /// `.viewerBreadcrumb`: the parent, which goes back, a slash, and the
    /// current name, ellipsised, within `min(420px, 42vw)`.
    fn breadcrumb(
        &self,
        parent: SharedString,
        current: SharedString,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let widest = (f32::from(window.viewport_size().width) * 0.42).min(420.);
        div()
            .min_w_0()
            .max_w(px(widest))
            .ml(px(6.))
            .flex()
            .items_center()
            .gap(px(5.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .child(
                ui::crumb("viewer-breadcrumb-parent", parent, false, cx)
                    .flex_shrink(1.)
                    .min_w_0()
                    .truncate()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.close_viewer(cx));
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.text_tertiary())
                    .child("/"),
            )
            .child(div().min_w_0().truncate().child(current))
    }
}

impl Render for ViewerToolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        // How a collection shows (`GroupSurface`'s display controls): its
        // reader offers "Edit group"; its editor has the breadcrumb and
        // nothing to step through; a member open by itself (`MediaView`
        // over the members) steps through the members and goes back to
        // the group.
        let (mode, member_open) = self
            .viewer
            .upgrade()
            .map(|viewer| viewer.read(cx).collection_view())
            .unwrap_or_default();
        let collection = super::collection_selected(self.session.read(cx));
        let member_open = collection && member_open;
        let shown = collection && !member_open;
        let editing = shown && mode == super::group::Mode::Editor;
        let (index, total) = {
            let session = self.session.read(cx);
            if member_open {
                (
                    Some(session.member),
                    session
                        .selected_details()
                        .map_or(0, |details| details.media.len() as u64),
                )
            } else {
                (
                    session
                        .focus_ordinal()
                        .filter(|_| session.demo.is_none() && !editing),
                    session.total,
                )
            }
        };
        let (zoomable, percent) = {
            let surface = self.surface.read(cx);
            (surface.zoomable() && !shown, surface.percent())
        };
        let can_previous = index.is_some_and(|index| index > 0);
        let can_next = index.is_some_and(|index| (index as u64) + 1 < total);
        // The editor names where the collection was opened from, or
        // "Collections", and the collection, or "Untitled".
        let breadcrumb: Option<(SharedString, SharedString)> = editing.then(|| {
            let session = self.session.read(cx);
            let parent = session
                .nav
                .get(session.displayed)
                .map(|entry| entry.label.clone())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| "Collections".to_owned());
            let name = session
                .selected_details()
                .map(|details| details.root.name.clone())
                .or_else(|| session.selected_item().map(|item| item.name.clone()))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Untitled".to_owned());
            (parent.into(), name.into())
        });

        let back = if member_open {
            "Back to group"
        } else {
            "Back to grid"
        };
        let mut left = div().flex().items_center().gap(px(4.)).flex_none().child(
            ui::icon_button("viewer-back", "history-back")
                .tip(ui::tip(back).shortcut("view.closeDetail", cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if member_open {
                        let _ = this
                            .viewer
                            .update(cx, |viewer, cx| viewer.close_member(window, cx));
                    } else {
                        this.session
                            .update(cx, |session, cx| session.close_viewer(cx));
                    }
                })),
        );
        if let Some((parent, current)) = breadcrumb {
            left = left.child(self.breadcrumb(parent, current, window, cx));
        }
        if let Some(index) = index {
            // `TitlebarCounter`
            left = left.child(
                div()
                    .ml(px(4.))
                    .whitespace_nowrap()
                    .text(theme::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary())
                    .child(format!("{} / {}", index + 1, total)),
            );
        }

        let mut center = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .justify_center();
        if zoomable {
            center = center.child(
                zoom::section(cx)
                    .child(
                        // `.zoomLabel`: a click opens the levels; a right
                        // click switches between fit and actual size.
                        div()
                            .id("viewer-zoom-label")
                            .min_w(px(40.))
                            .flex()
                            .justify_center()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text_secondary())
                            .child(format!("{percent}%"))
                            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                this.open_menu(event.position(), window, cx)
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    let actual = this.surface.read(cx).percent() == 100.;
                                    this.surface.update(cx, |surface, cx| {
                                        if actual {
                                            surface.fit(cx)
                                        } else {
                                            surface.actual_size(cx)
                                        }
                                    });
                                }),
                            ),
                    )
                    .child(self.zoom.clone()),
            );
        }

        let mut right = div().flex().items_center().gap(px(4.)).flex_none();
        if zoomable {
            right = right
                .child(
                    ui::icon_button("viewer-fit", "toolbar-fit")
                        .tip(ui::tip("Fit to window").shortcut("view.fitWindow", cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.surface.update(cx, |surface, cx| surface.fit(cx));
                        })),
                )
                .child(
                    ui::icon_button("viewer-actual-size", "toolbar-actual-size")
                        .tip(ui::tip("Actual size").shortcut("view.actualSize", cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.surface
                                .update(cx, |surface, cx| surface.actual_size(cx));
                        })),
                );
        }
        if shown && !editing {
            right = right.child(
                ui::icon_button("viewer-edit-group", "menu-group-edit")
                    .tip(ui::tip("Edit group"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let _ = this
                            .viewer
                            .update(cx, |viewer, cx| viewer.edit_collection(window, cx));
                    })),
            );
        }
        // The editor has nothing to step through.
        right = right.when(!editing, |right| {
            right.child(
                // `TitlebarControlGroup`
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        ui::icon_button("viewer-previous", "toolbar-chevron-left")
                            .disabled(!can_previous)
                            .tip(ui::tip("Previous").shortcut("view.prevImage", cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let _ = this
                                    .viewer
                                    .update(cx, |viewer, cx| viewer.arrow(-1, window, cx));
                            })),
                    )
                    .child(
                        ui::icon_button("viewer-next", "toolbar-chevron-right")
                            .disabled(!can_next)
                            .tip(ui::tip("Next").shortcut("view.nextImage", cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let _ = this
                                    .viewer
                                    .update(cx, |viewer, cx| viewer.arrow(1, window, cx));
                            })),
                    ),
            )
        });

        let menu = self.menu.layer(window, cx);

        ui::drag_region("viewer-toolbar", window, cx)
            .relative()
            .size_full()
            // As the grid toolbar: 8px from the titlebar on either side and
            // 8 more on the right from the toolbar itself.
            .pl(px(8.))
            .pr(px(16.))
            .flex()
            .items_center()
            .child(left)
            .child(center)
            .child(right)
            .children(menu)
    }
}
