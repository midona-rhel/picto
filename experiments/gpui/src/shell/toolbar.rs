//! The grid toolbar (`GridToolbar.tsx`), which sits in the titlebar row.
use crate::session::{Change, Session};
use crate::ui::{self, text_input::TextInput, zoom};
use crate::{theme, ui::zoom::Zoom};
use gpui::*;

pub struct Toolbar {
    session: Entity<Session>,
    search: Entity<TextInput>,
    zoom: Entity<Zoom>,
    _subscriptions: Vec<Subscription>,
}

impl Toolbar {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
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
            session,
            search,
            zoom,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for Toolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (title, opacity, animating, back, forward, filtered) = {
            let session = self.session.read(cx);
            (
                session
                    .nav
                    .get(session.displayed)
                    .map(|entry| entry.label.clone())
                    .unwrap_or_else(|| "Opening library...".into()),
                session.transition.opacity(),
                session.transition.animating(),
                session.can_step_history(-1),
                session.can_step_history(1),
                session.filter.is_some(),
            )
        };
        // The scope title fades with the grid.
        if animating {
            window.request_animation_frame();
        }
        let history =
            |id: &'static str, icon, delta: isize, enabled: bool, cx: &mut Context<Self>| {
                ui::icon_button(id, icon)
                    .disabled(!enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.step_history(delta, cx));
                    }))
            };
        let mut toolbar = ui::drag_region("toolbar", window, cx)
            .size_full()
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(history("history-back", "history-back", -1, back, cx))
            .child(history(
                "history-forward",
                "history-forward",
                1,
                forward,
                cx,
            ))
            .child(
                div()
                    .ml(px(10.))
                    .w(px(160.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .opacity(opacity)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .child(div().occlude().child(self.zoom.clone())),
            );
        if filtered {
            toolbar = toolbar.child(ui::button("clear-filter", "Clear filter", cx).on_click(
                cx.listener(|this, _, _, cx| {
                    this.session
                        .update(cx, |session, cx| session.set_filter(None, cx));
                }),
            ));
        }
        toolbar
            .child(ui::icon_button("view-options", "toolbar-layout"))
            .child(ui::icon_button("filter-library", "toolbar-filter"))
            .child(
                div()
                    .occlude()
                    .ml(px(4.))
                    .w(px(160.))
                    .h(px(theme::ICON_BUTTON))
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border_secondary())
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .child(div().pl(px(7.)).flex_none().child(ui::icon(
                        "search",
                        14.,
                        theme.text_tertiary(),
                    )))
                    .child(div().flex_1().h_full().child(self.search.clone())),
            )
    }
}
