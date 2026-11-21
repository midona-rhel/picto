//! The viewer toolbar (`ViewerToolbar` in `GridToolbar.tsx`). While an item
//! is open in the viewer, it is shown in the titlebar where the grid toolbar
//! normally is.
//!
//! On the left are the back button, the breadcrumb (only while a collection
//! is being edited) and the item's position in the results, for example
//! "3 / 120". In the centre are the zoom percentage and the zoom slider,
//! whose track is logarithmic. On the right are the "Fit to window", "Actual
//! size", "Previous" and "Next" buttons, and the "Edit group" button while a
//! collection is shown in its reader.
use super::surface::{self, MediaSurface};
use crate::i18n::t;
use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::ui::menu::{self, Menu};
use crate::ui::zoom::{self, Zoom};
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

/// The zoom levels offered in the menu that opens when the zoom percentage
/// is clicked. Each entry is the name of the action its menu row reports,
/// and the level in percent.
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
    /// The viewer. The toolbar's buttons call it to step to the previous or
    /// next item, to close a member that is open by itself, and to switch a
    /// collection to its editor.
    viewer: WeakEntity<super::Viewer>,
    surface: Entity<MediaSurface>,
    zoom: Entity<Zoom>,
    /// The host of the menu that opens when the zoom percentage is clicked.
    /// When the menu closes, keyboard focus returns to whatever had it
    /// before the menu opened.
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
                this.zoom
                    .update(cx, |zoom, cx| zoom.show(zoom::Value::Percent(percent), cx));
                cx.notify();
            }),
            cx.subscribe(&zoom, |this, _, event: &zoom::Changed, cx| {
                if let zoom::Changed::Percent(percent) = *event {
                    this.surface
                        .update(cx, |surface, cx| surface.set_percent(percent, cx));
                }
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

    /// Opens the zoom menu at `position`, which is where the pointer was
    /// when the zoom percentage was clicked. The menu lists the zoom levels
    /// in [`LEVELS`], with a check mark on the one that equals the current
    /// zoom, then a separator, then "Actual size" and "Fit to window".
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
        // This menu has no search row. Production's `GridToolbar.tsx` opens
        // it with `showSearch: false`.
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

    /// `.viewerBreadcrumb`: the name of the place the collection was opened
    /// from (`parent`), a slash, and the collection's name (`current`). A
    /// click on the parent's name closes the viewer. The breadcrumb is at
    /// most `min(420px, 42vw)` wide, and a name that does not fit is cut off
    /// with an ellipsis.
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
        // What the toolbar shows for a collection depends on how the
        // collection is displayed (`GroupSurface`'s display controls in
        // production). In the reader, the toolbar has the "Edit group"
        // button. In the editor, it has the breadcrumb, and it has no
        // position counter and no "Previous" and "Next" buttons. In both,
        // the zoom controls are left out. When one member is open by itself
        // (production's `MediaView` over the members), "Previous" and "Next"
        // step through the members and the back button returns to the
        // collection.
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
                (session.focus_ordinal().filter(|_| !editing), session.total)
            }
        };
        let (zoomable, percent) = {
            let surface = self.surface.read(cx);
            (surface.zoomable() && !shown, surface.percent())
        };
        let can_previous = index.is_some_and(|index| index > 0);
        let can_next = index.is_some_and(|index| (index as u64) + 1 < total);
        // The breadcrumb, which is shown only in the editor. Its first part
        // is the name of the place the collection was opened from, or
        // "Collections" when that place has no name. Its second part is the
        // collection's name, or "Untitled" when it has none.
        let breadcrumb: Option<(SharedString, SharedString)> = editing.then(|| {
            let session = self.session.read(cx);
            let parent = session
                .nav
                .get(session.displayed)
                .map(|entry| entry.label.clone())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| t("Collections").to_owned());
            let name = session
                .selected_details()
                .map(|details| details.root.name.clone())
                .or_else(|| session.selected_item().map(|item| item.name.clone()))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Untitled".to_owned());
            (parent.into(), name.into())
        });

        let back = if member_open {
            crate::i18n::en("Back to group")
        } else {
            crate::i18n::en("Back to grid")
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
            // The position counter, for example "3 / 120"
            // (`TitlebarCounter` in production).
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
                        // The zoom percentage (`.zoomLabel`). A click opens
                        // the zoom menu. A right-click sets the zoom to "Fit
                        // to window" when it is at 100%, and to 100%
                        // otherwise. Production's element is a
                        // `<span role="button">`, so assistive technology
                        // reads it as a button named by the percentage.
                        div()
                            .id("viewer-zoom-label")
                            .role(Role::Button)
                            .aria_label(format!("{percent}%"))
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
                        .tip(ui::tip(t("Fit to window")).shortcut("view.fitWindow", cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.surface.update(cx, |surface, cx| surface.fit(cx));
                        })),
                )
                .child(
                    ui::icon_button("viewer-actual-size", "toolbar-actual-size")
                        .tip(ui::tip(t("Actual size")).shortcut("view.actualSize", cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.surface
                                .update(cx, |surface, cx| surface.actual_size(cx));
                        })),
                );
        }
        if shown && !editing {
            right = right.child(
                ui::icon_button("viewer-edit-group", "menu-group-edit")
                    .tip(ui::tip(t("Edit group")))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let _ = this
                            .viewer
                            .update(cx, |viewer, cx| viewer.edit_collection(window, cx));
                    })),
            );
        }
        // The "Previous" and "Next" buttons are left out in the editor,
        // which shows all the members at once.
        right = right.when(!editing, |right| {
            right.child(
                // The group that holds the "Previous" and "Next" buttons
                // (`TitlebarControlGroup` in production).
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        ui::icon_button("viewer-previous", "toolbar-chevron-left")
                            .disabled(!can_previous)
                            .tip(ui::tip(t("Previous")).shortcut("view.prevImage", cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let _ = this
                                    .viewer
                                    .update(cx, |viewer, cx| viewer.arrow(-1, window, cx));
                            })),
                    )
                    .child(
                        ui::icon_button("viewer-next", "toolbar-chevron-right")
                            .disabled(!can_next)
                            .tip(ui::tip(t("Next")).shortcut("view.nextImage", cx))
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
            // The same padding as the grid toolbar: 8px on the left and
            // 16px on the right. In production the titlebar gives 8px on
            // each side and the toolbar itself adds 8px more on the right.
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
