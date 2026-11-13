//! The viewer that replaces the grid while an item is open.
pub mod flash;
pub mod surface;

use crate::session::{Change, Session};
use crate::{theme, ui};
use gpui::*;
use std::path::PathBuf;
use surface::MediaSurface;

pub struct Viewer {
    session: Entity<Session>,
    surface: Entity<MediaSurface>,
    _session: Subscription,
}

impl Viewer {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let surface = cx.new(|_| MediaSurface::default());
        let subscription = cx.subscribe(&session, |this, _, change: &Change, cx| match change {
            Change::Selection => {
                // Show the thumbnail at once; the original follows when open.
                let shown = {
                    let session = this.session.read(cx);
                    session.selected_item().map(|item| {
                        (
                            item.content_hash.clone(),
                            session.thumbnail(&item.content_hash),
                        )
                    })
                };
                if let Some((key, thumbnail)) = shown {
                    this.surface
                        .update(cx, |surface, cx| surface.show(key, thumbnail, None, cx));
                }
                this.present(cx);
                cx.notify();
            }
            Change::Details | Change::Viewer => {
                this.present(cx);
                cx.notify();
            }
            _ => {}
        });
        Self {
            session,
            surface,
            _session: subscription,
        }
    }

    /// Opens what the session points at, if the viewer is showing.
    fn present(&mut self, cx: &mut Context<Self>) {
        let target = {
            let session = self.session.read(cx);
            if !session.viewer {
                None
            } else if let Some((name, mime)) = session.demo {
                Some((
                    name.to_owned(),
                    PathBuf::new(),
                    session.fixtures.path().join(name),
                    mime.to_owned(),
                ))
            } else {
                session
                    .details
                    .as_ref()
                    .and_then(|details| details.media.get(session.member))
                    .map(|media| {
                        (
                            media.facts.content_hash.clone(),
                            session.thumbnail(&media.facts.content_hash),
                            PathBuf::from(&media.file_path),
                            media.facts.mime.clone(),
                        )
                    })
            }
        };
        self.surface.update(cx, |surface, cx| match target {
            Some((key, thumbnail, path, mime)) => surface.open(key, thumbnail, path, &mime, cx),
            None => surface.stop(),
        });
    }
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (name, members, member) = {
            let session = self.session.read(cx);
            let name = match session.demo {
                Some((name, _)) => name.to_owned(),
                None => session
                    .selected_item()
                    .map_or_else(|| "No selection".to_owned(), |item| item.name.clone()),
            };
            let members = if session.demo.is_some() {
                1
            } else {
                session
                    .details
                    .as_ref()
                    .map_or(1, |details| details.media.len())
            };
            (name, members, session.member)
        };
        let step = |id: &'static str, label: &'static str, delta: isize, cx: &mut Context<Self>| {
            ui::button(id, label, cx).on_click(cx.listener(move |this, _, _, cx| {
                this.session.update(cx, |session, cx| {
                    let target = session
                        .selected
                        .saturating_add_signed(delta)
                        .min(session.items.len().saturating_sub(1));
                    session.select(target, cx);
                });
            }))
        };
        let mut pane = div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::button("close", "Back to grid", cx).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.session
                                    .update(cx, |session, cx| session.close_viewer(cx));
                            },
                        )),
                    )
                    .child(div().flex_1().overflow_hidden().text_ellipsis().child(name))
                    .child(step("previous", "Previous", -1, cx))
                    .child(step("next", "Next", 1, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.))
                    .child(self.surface.clone()),
            );
        if members > 1 {
            let member_step =
                |id: &'static str, label: &'static str, delta: isize, cx: &mut Context<Self>| {
                    ui::button(id, label, cx).on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.step_member(delta, cx));
                    }))
                };
            pane = pane.child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .text_color(theme.text_secondary())
                    .child(member_step("member-back", "‹", -1, cx))
                    .child(format!("{} / {} in collection", member + 1, members))
                    .child(member_step("member-next", "›", 1, cx)),
            );
        }
        pane
    }
}
