//! The titlebar's content (`.titlebarContent` in `AppShell.tsx`): the grid
//! toolbar (`GridToolbar.tsx`) with Back, Forward and the scope title, or the
//! viewer toolbar while an item is open.
use crate::backend::Screen;
use crate::session::{Change, Session, Transition};
use crate::ui::{self, field::Look, menu::Menu, text_input::TextInput, zoom};
use crate::viewer::{Viewer, toolbar::ViewerToolbar};
use crate::{theme, ui::zoom::Zoom};
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// The "View options" button was pressed: the view menu opens at this
/// point, the button's left edge 4px below it.
pub struct ViewMenuRequested(pub Point<Pixels>);

impl EventEmitter<ViewMenuRequested> for Toolbar {}

pub struct Toolbar {
    session: Entity<Session>,
    search: Entity<TextInput>,
    zoom: Entity<Zoom>,
    viewer: Entity<ViewerToolbar>,
    /// Set when the viewer closed by going to another destination: the
    /// titlebar's content then fades with the grid (`data-viewer-exit-transition`),
    /// the viewer toolbar out and the destination's toolbar in.
    viewer_exit: bool,
    /// The controls a screen puts in the titlebar in place of the grid's.
    screen_controls: Vec<(Screen, AnyView)>,
    /// Where the "View options" button was painted.
    view_button: Rc<Cell<Bounds<Pixels>>>,
    /// The view menu while it is open or fading, which keeps the button
    /// lit (`active={viewMenu.state != null}`).
    view_menu: Option<(Entity<Menu>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl Toolbar {
    /// The search field, which `nav.search` gives the keyboard to.
    pub fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }

    pub fn new(session: Entity<Session>, viewer: &Entity<Viewer>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(TextInput::new);
        let zoom = cx.new(|_| Zoom::new());
        let subscriptions = vec![
            cx.observe(&search, |this, search, cx| {
                let text = search.read(cx).value();
                this.session
                    .update(cx, |session, cx| session.set_search(text, cx));
            }),
            cx.subscribe(&zoom, |this, _, event: &zoom::Changed, cx| {
                this.session
                    .update(cx, |session, cx| session.set_tile(event.0, cx));
            }),
            cx.subscribe(
                &session,
                |this, session, change: &Change, cx| match change {
                    Change::Navigation => cx.notify(),
                    Change::Viewer => {
                        let session = session.read(cx);
                        this.viewer_exit =
                            !session.viewer && !matches!(session.transition, Transition::Idle);
                        cx.notify();
                    }
                    Change::Results => {
                        // A destination opens at its saved zoom.
                        let tile = session.read(cx).tile;
                        this.zoom.update(cx, |zoom, cx| zoom.show(tile, cx));
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ];
        Self {
            viewer: viewer.read(cx).toolbar(),
            viewer_exit: false,
            session,
            search,
            zoom,
            screen_controls: Vec::new(),
            view_button: Rc::new(Cell::new(Bounds::default())),
            view_menu: None,
            _subscriptions: subscriptions,
        }
    }

    /// Keeps the "View options" button lit while this menu is open, until
    /// it has faded out.
    pub fn watch_view_menu(&mut self, menu: Entity<Menu>, cx: &mut Context<Self>) {
        let observed = cx.observe(&menu, |_, menu, cx| {
            // A dismissed menu fades for 80 ms; look again once it is gone.
            if !menu.read(cx).finished() {
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(crate::motion::MENU_OUT)
                        .await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        });
        self.view_menu = Some((menu, observed));
        cx.notify();
    }

    /// Shows `controls` after the history buttons while `screen` is the
    /// destination shown.
    pub fn set_screen_controls(&mut self, screen: Screen, controls: AnyView) {
        self.screen_controls.retain(|(owner, _)| *owner != screen);
        self.screen_controls.push((screen, controls));
    }
}

impl Toolbar {
    /// The scope title (`.scopeTitle`): the destination's ancestors as links,
    /// then its own name. While a search or a filter narrows the grid, the
    /// name is a link too, back to the destination unnarrowed, followed by
    /// "Search results · N". It runs from 72px in to the toolbar's middle and
    /// fades out over its last 40px rather than ending in an ellipsis.
    fn title(&self, opacity: f32, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let (crumbs, results) = {
            let session = self.session.read(cx);
            let results = (session.place().is_none()
                && (!session.search_text().is_empty() || session.filters.count() > 0))
                .then(|| search_results_label(session.total));
            let mut crumbs = Vec::new();
            let mut at = session
                .nav
                .get(session.displayed)
                .map(|_| session.displayed);
            while let Some(index) = at {
                crumbs.push((index, session.nav[index].label.clone()));
                at = session.nav[index].parent;
            }
            crumbs.reverse();
            // A screen's open place follows the screen, which links back to
            // the screen's top.
            if let Some(label) = session.place().and(session.place_label.clone()) {
                crumbs.push((usize::MAX, label.to_string()));
            }
            (crumbs, results)
        };
        let last = crumbs.len().saturating_sub(1);
        let mut path = div().flex().items_center().whitespace_nowrap();
        let separator = || div().mx(px(5.)).opacity(0.4).child("/");
        for (position, (index, label)) in crumbs.into_iter().enumerate() {
            if position == last && results.is_none() {
                path = path.child(div().child(label));
                continue;
            }
            let clears = position == last;
            path = path.child(
                div()
                    .id(("scope-crumb", index))
                    .occlude()
                    .cursor_pointer()
                    .text_color(theme.text_secondary())
                    .hover(move |style| style.text_color(theme.text()).underline())
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if clears {
                            // `clearSearchResults`: no search, no filters.
                            this.search.update(cx, |search, cx| search.clear(cx));
                            this.session.update(cx, |session, cx| {
                                session.set_search(String::new(), cx);
                                session.navigate_with_filters(index, Default::default(), cx);
                            });
                        } else {
                            this.session
                                .update(cx, |session, cx| session.navigate(index, cx));
                        }
                    })),
            );
            if !clears {
                path = path.child(separator());
            }
        }
        if let Some(results) = results {
            path = path.child(separator()).child(div().child(results));
        }
        let background = theme.background();
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(8. + 72.))
            .right(relative(0.5))
            .flex()
            .items_center()
            .overflow_hidden()
            .opacity(opacity)
            .child(path)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(40.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(theme::clear(background), 0.),
                        linear_color_stop(background, 1.),
                    )),
            )
    }
}

/// `Search results · N`, the count grouped by thousands.
fn search_results_label(total: u64) -> String {
    format!("Search results · {}", crate::grid::format::grouped(total))
}

impl Render for Toolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Toolbar);
        let theme = theme::theme(cx);
        let (viewing, transition) = {
            let session = self.session.read(cx);
            (session.viewer, session.transition)
        };
        if self.viewer_exit && matches!(transition, Transition::Idle) {
            self.viewer_exit = false;
        }
        // The content as a whole fades only on the way out of the viewer.
        let content = if self.viewer_exit {
            if transition.animating() {
                window.request_animation_frame();
            }
            transition.opacity()
        } else {
            1.
        };
        if viewing || (self.viewer_exit && matches!(transition, Transition::FadingOut(_))) {
            return div()
                .size_full()
                .opacity(content)
                .child(self.viewer.clone())
                .into_any_element();
        }
        let (opacity, animating, back, forward, filter_count, bar_open, screen) = {
            let session = self.session.read(cx);
            (
                session.transition.opacity(),
                session.transition.animating(),
                session.can_step_history(-1),
                session.can_step_history(1),
                session.filters.count(),
                session.filter_bar_open,
                session.screen(),
            )
        };
        if self
            .view_menu
            .as_ref()
            .is_some_and(|(menu, _)| menu.read(cx).finished())
        {
            self.view_menu = None;
        }
        let view_menu_open = self.view_menu.is_some();
        // The scope title fades with the grid.
        if animating {
            window.request_animation_frame();
        }
        let history = |id: &'static str,
                       icon,
                       delta: isize,
                       enabled: bool,
                       tip: ui::tooltip::Tip,
                       cx: &mut Context<Self>| {
            ui::icon_button(id, icon)
                .disabled(!enabled)
                .tip(tip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session
                        .update(cx, |session, cx| session.step_history(delta, cx));
                }))
        };
        let toolbar = ui::drag_region("toolbar", window, cx)
            .relative()
            .size_full()
            .opacity(content)
            // 8px either side from the titlebar, and 8 more on the right
            // from the toolbar itself.
            .pl(px(8.))
            .pr(px(16.))
            .flex()
            .items_center()
            .child(self.title(opacity, cx))
            .child(history(
                "history-back",
                "history-back",
                -1,
                back,
                ui::tip("Back").shortcut("nav.back", cx),
                cx,
            ))
            .child(history(
                "history-forward",
                "history-forward",
                1,
                forward,
                ui::tip("Forward").shortcut("nav.forward", cx),
                cx,
            ));
        // A screen has no grid to zoom, filter or search: the Electron
        // titlebar shows the grid's controls only over the grid, and a
        // screen's own controls, if it has any, in their place.
        if let Some(screen) = screen {
            let controls = self
                .screen_controls
                .iter()
                .find(|(owner, _)| *owner == screen)
                .map(|(_, controls)| controls.clone());
            // `.titlebarToolbar`: the controls take the rest of the row and
            // sit at its right end, clear of the scope title.
            return toolbar
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_end()
                        .children(controls),
                )
                .into_any_element();
        }
        toolbar
            .child(
                // The zoom is centred in the space between the history
                // buttons and the right-hand controls. It has the app
                // background, with a short fade on its left, so a long
                // scope title disappears under it.
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(zoom::section(cx).child(self.zoom.clone())),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        ui::icon_button("view-options", "toolbar-layout")
                            .tip(ui::tip("View options"))
                            .toggled(view_menu_open)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let button = this.view_button.get();
                                cx.emit(ViewMenuRequested(point(
                                    button.left(),
                                    button.bottom() + px(4.),
                                )));
                            })),
                    )
                    .child({
                        let bounds = self.view_button.clone();
                        canvas(move |painted, _, _| bounds.set(painted), |_, _, _, _| {})
                            .absolute()
                            .inset_0()
                    }),
            )
            .child(div().w(px(4.)).flex_none())
            .child(
                // Lit while the filter bar is open or any filter is set,
                // with the number set.
                ui::icon_button("filter-library", "toolbar-filter")
                    .tip(ui::tip("Filter library"))
                    .toggled(bar_open || filter_count > 0)
                    .badge(filter_count)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.set_filter_bar_open(!bar_open, cx));
                    })),
            )
            .child(div().ml(px(4.)).w(px(148.)).flex_none().child(
                ui::field("search", &self.search, Look::Search).icon(
                    "search",
                    14.,
                    8.,
                    theme.text_tertiary(),
                ),
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::search_results_label;

    #[test]
    fn search_results_count_their_items() {
        assert_eq!(search_results_label(0), "Search results · 0");
        assert_eq!(search_results_label(2673), "Search results · 2,673");
    }
}
