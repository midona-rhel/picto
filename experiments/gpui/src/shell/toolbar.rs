//! This file draws the content of the window's titlebar (`.titlebarContent`
//! in production's `AppShell.tsx`).
//!
//! While the grid or a screen is shown, the content is the grid toolbar
//! (`GridToolbar.tsx`): the Back and Forward buttons, the scope title, and
//! then either the grid's controls (zoom, view options, filter, search) or
//! the screen's own controls. While an item is open in the viewer, the
//! content is the viewer toolbar instead.
use crate::backend::Screen;
use crate::grid::layout;
use crate::i18n::t;
use crate::session::{Change, Session, Transition};
use crate::ui::{
    self,
    field::Look,
    menu::Menu,
    text_input::{Kind, TextInput},
    zoom,
};
use crate::viewer::{Viewer, toolbar::ViewerToolbar};
use crate::{theme, ui::zoom::Zoom};
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// The toolbar emits this event when the "View options" button is clicked.
/// The shell opens the view menu at the point the event carries, which is
/// at the button's left edge, 4 px below the button's bottom edge.
pub struct ViewMenuRequested(pub Point<Pixels>);

impl EventEmitter<ViewMenuRequested> for Toolbar {}

pub struct Toolbar {
    session: Entity<Session>,
    search: Entity<TextInput>,
    zoom: Entity<Zoom>,
    viewer: Entity<ViewerToolbar>,
    /// True from the moment the viewer closes because the session went to
    /// another destination, until that destination has finished fading in.
    /// It is set when the viewer closes while the session's transition is
    /// running, and cleared when the transition is idle again. While it is
    /// true, the whole titlebar content is drawn at the transition's
    /// opacity, as the grid is: the viewer toolbar fades out, then the
    /// destination's toolbar fades in. Production marks the same state
    /// with `data-viewer-exit-transition`.
    viewer_exit: bool,
    /// For each screen that has its own titlebar controls, the view that
    /// draws them. While that screen is shown, the titlebar shows this view
    /// in place of the grid's zoom, view options, filter and search
    /// controls.
    screen_controls: Vec<(Screen, AnyView)>,
    /// For each screen that shows a note after its name in the scope title,
    /// the view that draws the note. The Subscriptions screen's note is the
    /// number of subscriptions.
    screen_notes: Vec<(Screen, AnyView)>,
    /// The toolbar's width in pixels in the frame that is being built.
    /// `build` sets it at its start. The scope title ends at the toolbar's
    /// middle, so this width decides whether the path to a place open
    /// inside a screen fits in the title or has to be shortened.
    width: f32,
    /// The bounds of the "View options" button the last time it was
    /// painted. The point the view menu opens at is taken from them.
    view_button: Rc<Cell<Bounds<Pixels>>>,
    /// The view menu, and the subscription that observes it, from the time
    /// the menu opens until it has finished fading out. While this is set,
    /// the "View options" button is drawn in its toggled state, as
    /// production's `active={viewMenu.state != null}` does.
    view_menu: Option<(Entity<Menu>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl Toolbar {
    /// Returns the focus handle of the grid's search field. The `nav.search`
    /// shortcut uses it to put the keyboard focus in the field.
    pub fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }

    pub fn new(session: Entity<Session>, viewer: &Entity<Viewer>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(Kind::Search, cx).placeholder("Search"));
        let zoom = cx.new(|_| Zoom::new());
        let subscriptions = vec![
            cx.observe(&search, |this, search, cx| {
                let text = search.read(cx).value();
                this.session
                    .update(cx, |session, cx| session.set_search(text, cx));
            }),
            cx.subscribe(&zoom, |this, _, event: &zoom::Changed, cx| {
                this.session.update(cx, |session, cx| match *event {
                    zoom::Changed::Columns(columns) => session.zoom_to_columns(columns, cx),
                    zoom::Changed::Height(height) => session.zoom_to_height(height, cx),
                    zoom::Changed::Step { larger } => session.zoom_step(larger, cx),
                    // Only the viewer toolbar's zoom control emits a
                    // percentage. The grid's control never does.
                    zoom::Changed::Percent(_) => {}
                });
            }),
            cx.subscribe(
                &session,
                |this, session, change: &Change, cx| match change {
                    Change::Navigation => {
                        // The session clears its search text every time a
                        // destination opens. The search field holds its own
                        // copy of the text, so the field is cleared here
                        // when the session's text is empty and the field's
                        // is not.
                        if session.read(cx).search_text().is_empty()
                            && !this.search.read(cx).is_empty()
                        {
                            this.search.update(cx, |search, cx| search.clear(cx));
                        }
                        cx.notify()
                    }
                    Change::Viewer => {
                        let session = session.read(cx);
                        this.viewer_exit =
                            !session.viewer && !matches!(session.transition, Transition::Idle);
                        cx.notify();
                    }
                    Change::Results => {
                        // The session emits `Change::Results` when the
                        // items, the view preferences or the zoom change.
                        // That includes a destination opening with the zoom
                        // saved for it, so the zoom control is set to the
                        // grid's current zoom each time.
                        this.show_zoom(cx);
                        cx.notify();
                    }
                    // At another width the same tile size gives other column
                    // numbers, so the zoom control is given the grid's zoom
                    // again.
                    Change::GridWidth => this.show_zoom(cx),
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
            screen_notes: Vec::new(),
            width: 0.,
            view_button: Rc::new(Cell::new(Bounds::default())),
            view_menu: None,
            _subscriptions: subscriptions,
        }
    }

    /// Sets the toolbar's zoom control to the grid's current zoom: the
    /// number of columns in the grid and waterfall layouts, or the target
    /// row height in the justified layout.
    fn show_zoom(&mut self, cx: &mut Context<Self>) {
        let value = match self.session.read(cx).zoom() {
            layout::Zoom::Columns {
                count,
                most,
                exact,
                span,
            } => zoom::Value::Columns {
                count,
                most,
                exact,
                span,
            },
            layout::Zoom::Height(height) => zoom::Value::Height {
                height,
                least: layout::SMALLEST_TILE,
                most: layout::TALLEST_ROW,
            },
        };
        self.zoom.update(cx, |zoom, cx| zoom.show(value, cx));
    }

    /// The shell calls this when it opens the view menu. The toolbar keeps
    /// `menu` so that the "View options" button is drawn in its toggled
    /// state until the menu has been dismissed and has finished fading out.
    pub fn watch_view_menu(&mut self, menu: Entity<Menu>, cx: &mut Context<Self>) {
        let observed = cx.observe(&menu, |_, menu, cx| {
            // A dismissed menu takes 80 ms (`motion::MENU_OUT`) to fade
            // out, and `render` drops the menu once `finished()` is true.
            // The menu notifies its observers when it is dismissed, which
            // is when the fade starts, not when it ends. So each time the
            // menu notifies, the toolbar is drawn again 80 ms later to make
            // that check.
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

    /// Registers `controls` as the titlebar controls of `screen`. While
    /// `screen` is the destination shown, the toolbar draws them to the
    /// right of the Back and Forward buttons, in place of the grid's
    /// controls. A second call for the same screen replaces the first.
    pub fn set_screen_controls(&mut self, screen: Screen, controls: AnyView) {
        self.screen_controls.retain(|(owner, _)| *owner != screen);
        self.screen_controls.push((screen, controls));
    }

    /// Registers `note` as the view drawn after the scope title while
    /// `screen` is the destination shown. A second call for the same screen
    /// replaces the first. The toolbar also draws the note while a place is
    /// open inside the screen, so the note's own view has to draw nothing
    /// then.
    pub fn set_screen_note(&mut self, screen: Screen, note: AnyView) {
        self.screen_notes.retain(|(owner, _)| *owner != screen);
        self.screen_notes.push((screen, note));
    }
}

impl Toolbar {
    /// Builds the scope title (production's `.scopeTitle`), drawn at
    /// `opacity`. It is a path: the names of the destination's ancestors,
    /// each a link to that ancestor and followed by "/", and then the
    /// destination's own name as plain text.
    ///
    /// While a search or a filter is limiting the items in the grid, the
    /// destination's own name is a link as well. Clicking it clears the
    /// search and the filters. It is followed by "/" and
    /// "Search results · N", where N is the number of items found.
    ///
    /// The title starts 80 px from the toolbar's left edge (the toolbar's
    /// 8 px of padding plus 72 px), which is to the right of the Back and
    /// Forward buttons, and it ends at the toolbar's middle. The last 40 px
    /// of that space are covered by a gradient from transparent to the
    /// background colour, so a title too long for the space fades out there
    /// instead of being cut off with an ellipsis. The one exception is the
    /// name of a place open inside a screen: the comments in the function
    /// say how that name is shortened.
    fn title(&self, opacity: f32, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let (mut crumbs, results, place, note) = {
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
            // When a place is open inside a screen (one subscription inside
            // the Subscriptions screen), the place's name is added to the
            // path after the screen's name. The screen's name is then no
            // longer last, so it is drawn as a link, and clicking it closes
            // the place and shows the screen's top.
            if let Some(label) = session.place().and(session.place_label.clone()) {
                crumbs.push((usize::MAX, label.to_string()));
            }
            let note = session.screen().and_then(|screen| {
                self.screen_notes
                    .iter()
                    .find(|(owner, _)| *owner == screen)
                    .map(|(_, note)| note.clone())
            });
            (crumbs, results, session.place().is_some(), note)
        };
        // `room` is the width the title has, in pixels: from 80 px to the
        // toolbar's middle. When a place is open inside a screen and the
        // whole path (the names and the "/" separators between them) is
        // wider than `room`, the title is shortened. The names before the
        // place's name are removed and the place's name is shown alone, cut
        // off with an ellipsis if it is still too wide. This shows as much
        // of the place's name as fits.
        let room = (self.width * 0.5 - (8. + 72.)).max(0.);
        let shortened = place && crumbs.len() > 1 && {
            let width = |text: &str| {
                f32::from(
                    ui::shape_line(
                        crate::i18n::t(text).to_owned(),
                        font(theme::FONT_UI),
                        theme::TEXT_MD,
                        theme.text(),
                        window,
                    )
                    .width,
                )
            };
            let path: f32 = crumbs.iter().map(|(_, label)| width(label)).sum::<f32>()
                + (crumbs.len() - 1) as f32 * (10. + width("/"));
            path > room
        };
        // When the title has less than 40 px of room, the place's name is
        // not shown at all. So little room would show an ellipsis with no
        // letters before it.
        if shortened && room < 40. {
            crumbs.clear();
        }
        if shortened && !crumbs.is_empty() {
            crumbs.drain(..crumbs.len() - 1);
        }
        let last = crumbs.len().saturating_sub(1);
        let mut path = div().flex().items_center().whitespace_nowrap();
        let separator = || div().mx(px(5.)).opacity(0.4).child("/");
        for (position, (index, label)) in crumbs.into_iter().enumerate() {
            // The name is translated into the app's current language here,
            // each time the title is drawn, as production's
            // `translateMessage(label)` does. A name that has no
            // translation, such as a folder's, is shown as it is.
            let label = crate::i18n::t(&label).to_owned();
            if position == last && results.is_none() {
                path = path.child(if shortened {
                    div().min_w_0().truncate().child(label)
                } else {
                    div().child(label)
                });
                continue;
            }
            let clears = position == last;
            path = path.child(ui::crumb(("scope-crumb", index), label, true, cx).on_click(
                cx.listener(move |this, _, _, cx| {
                    if clears {
                        // As production's `clearSearchResults` does: empty
                        // the search field, clear the session's search
                        // text, and show this destination with no filters.
                        this.search.update(cx, |search, cx| search.clear(cx));
                        this.session.update(cx, |session, cx| {
                            session.set_search(String::new(), cx);
                            session.navigate_with_filters(index, Default::default(), cx);
                        });
                    } else {
                        this.session
                            .update(cx, |session, cx| session.navigate(index, cx));
                    }
                }),
            ));
            if !clears {
                path = path.child(separator());
            }
        }
        if let Some(results) = results {
            path = path.child(separator()).child(div().child(results));
        }
        if let Some(note) = note {
            path = path.child(
                div()
                    .ml(px(8.))
                    .text_color(theme.text_secondary())
                    .child(note),
            );
        }
        if shortened {
            path = path.min_w_0().overflow_hidden();
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
            .children((!shortened).then(|| {
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
                    ))
            }))
    }
}

/// Returns the text "Search results · N" for the scope title. N is `total`,
/// the number of items found, written with a comma between each group of
/// three digits (2,673).
fn search_results_label(total: u64) -> String {
    format!(
        "{} · {}",
        crate::i18n::en("Search results"),
        crate::i18n::format::grouped(total)
    )
}

impl Render for Toolbar {
    /// The scope title is shortened when the toolbar is too narrow for it,
    /// so `render` returns an element that fills the toolbar's area and
    /// builds the toolbar for that element's bounds in the frame that lays
    /// it out (`ui::sized_view`).
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::sized_view(cx.weak_entity(), Self::build).size_full()
    }
}

impl Toolbar {
    /// Builds one frame of the toolbar for `bounds`, the toolbar's position
    /// and size in window coordinates in this frame.
    fn build(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.width = f32::from(bounds.size.width);
        crate::stats::rendered(crate::stats::View::Toolbar);
        let (viewing, transition) = {
            let session = self.session.read(cx);
            (session.viewer, session.transition)
        };
        if self.viewer_exit && matches!(transition, Transition::Idle) {
            self.viewer_exit = false;
        }
        // The whole titlebar content is faded only while `viewer_exit` is
        // true, which is while the viewer is closing into another
        // destination. It is then drawn at the transition's opacity. At all
        // other times it is fully opaque.
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
        // The scope title is drawn at the transition's opacity, so it fades
        // out and in together with the grid. While the fade is running,
        // another frame is requested so that the title is drawn again at
        // the next opacity.
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
            // The padding is 8 px on the left and 16 px on the right. In
            // production the titlebar has 8 px of padding on each side, and
            // the toolbar adds 8 px more on its right.
            .pl(px(8.))
            .pr(px(16.))
            .flex()
            .items_center()
            .child(self.title(opacity, window, cx))
            .child(history(
                "history-back",
                "history-back",
                -1,
                back,
                ui::tip(t("Back")).shortcut("nav.back", cx),
                cx,
            ))
            .child(history(
                "history-forward",
                "history-forward",
                1,
                forward,
                ui::tip(t("Forward")).shortcut("nav.forward", cx),
                cx,
            ));
        // A screen (Tags, Subscriptions or Duplicates) has no grid, so the
        // zoom, view options, filter and search controls do not apply to
        // it. As in production, the titlebar shows those controls only
        // while the grid is shown. While a screen is shown, the titlebar
        // shows the controls the screen registered with
        // `set_screen_controls` instead, or no controls if it registered
        // none.
        if let Some(screen) = screen {
            let controls = self
                .screen_controls
                .iter()
                .find(|(owner, _)| *owner == screen)
                .map(|(_, controls)| controls.clone());
            // As production's `.titlebarToolbar` does, the screen's controls
            // are given all of the row to the right of the Back and Forward
            // buttons and are aligned to its right end, away from the scope
            // title on the left. They are drawn at the transition's
            // opacity, so they fade out and in with the screen they belong
            // to, as the scope title does.
            return toolbar
                .child(
                    div()
                        .opacity(opacity)
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
        // The grid's controls take all of the row to the right of the Back
        // and Forward buttons. Which of them are shown depends on that
        // row's width, so the row is built for its own bounds in the frame
        // that lays it out (`ui::sized_view`).
        let controls = ui::sized_view(cx.weak_entity(), move |this, bounds, _, cx| {
            this.grid_controls(
                bounds.size.width,
                view_menu_open,
                bar_open,
                filter_count,
                cx,
            )
        })
        .flex_1()
        .min_w_0()
        .h_full();
        toolbar.child(controls).into_any_element()
    }

    /// Builds the row of the grid's controls: the zoom control, the "View
    /// options" button, the "Filter library" button and the search field.
    /// `width` is the row's width in this frame.
    ///
    /// The controls are hidden when there is too little room for them, as
    /// production's `.toolbar` container queries (`@container (max-width:
    /// …)`) do. The zoom control is hidden when the row is 350 px wide or
    /// less, and the search field when it is 240 px wide or less.
    ///
    /// `view_menu_open` is true while the view menu is open, `bar_open`
    /// while the filter bar is open, and `filter_count` is the number of
    /// filters that are set.
    fn grid_controls(
        &mut self,
        width: Pixels,
        view_menu_open: bool,
        bar_open: bool,
        filter_count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let mut controls = div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .child(
                // The zoom control is centred in the space between the Back
                // and Forward buttons and the "View options" button.
                // `zoom::section` gives it the app's background colour and a
                // 20 px gradient on its left side, so a scope title long
                // enough to reach the zoom control is covered by it.
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(
                        (width > px(350.)).then(|| zoom::section(cx).child(self.zoom.clone())),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        ui::icon_button("view-options", "toolbar-layout")
                            .tip(ui::tip(t("View options")))
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
                // The "Filter library" button is drawn in its toggled state
                // while the filter bar is open or at least one filter is
                // set. Its badge shows the number of filters that are set,
                // and is hidden when none are.
                ui::icon_button("filter-library", "toolbar-filter")
                    .tip(ui::tip(t("Filter library")))
                    .toggled(bar_open || filter_count > 0)
                    .badge(filter_count)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.set_filter_bar_open(!bar_open, cx));
                    })),
            );
        if width > px(240.) {
            controls = controls.child(div().ml(px(4.)).w(px(148.)).flex_none().child(
                ui::field("search", &self.search, Look::Search).icon(
                    "search",
                    14.,
                    8.,
                    theme.text_tertiary(),
                ),
            ));
        }
        controls.into_any_element()
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
