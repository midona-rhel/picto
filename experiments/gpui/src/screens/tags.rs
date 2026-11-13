//! The Tags screen.
use crate::grid::thumbnails::Thumbnails;
use crate::session::Session;
use crate::theme;
use gpui::*;

pub struct TagsScreen {
    #[allow(dead_code)]
    session: Entity<Session>,
    #[allow(dead_code)]
    thumbnails: Entity<Thumbnails>,
}

impl TagsScreen {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            session,
            thumbnails,
        }
    }
}

impl Render for TagsScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // TODO: the Tags screen.
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme::theme(cx).text_tertiary())
            .child("Tags is not part of this prototype yet")
    }
}
