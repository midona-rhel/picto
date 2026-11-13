//! The viewer that replaces the grid while an item is open (`MediaView.tsx`).
//! The media fills the pane; its controls are in the titlebar, where
//! [`toolbar::ViewerToolbar`] takes the grid toolbar's place.
pub mod camera;
pub mod flash;
mod strip;
pub mod surface;
pub mod toolbar;

use crate::session::{Change, Session, Transition};
use crate::theme::Text as _;
use crate::{motion, theme, ui};
use gpui::*;
use std::path::PathBuf;
use surface::MediaSurface;
use toolbar::ViewerToolbar;

actions!(
    viewer,
    [
        PreviousItem,
        NextItem,
        FitToWindow,
        ActualSize,
        ZoomIn,
        ZoomOut
    ]
);

/// The viewer's shortcuts from the View group of `shortcuts.ts`. They apply
/// while the viewer has the keyboard and win over the grid's bindings of the
/// same keys; Flash keeps its keys for the movie.
pub fn bind(cx: &mut App) {
    let viewer = Some("Viewer && !Flash");
    cx.bind_keys([
        // view.prevImage / view.nextImage
        KeyBinding::new("left", PreviousItem, viewer),
        KeyBinding::new("a", PreviousItem, viewer),
        KeyBinding::new("right", NextItem, viewer),
        KeyBinding::new("d", NextItem, viewer),
        // view.fitWindow, with Shift+F for keyboards without a backtick.
        KeyBinding::new("`", FitToWindow, viewer),
        KeyBinding::new("shift-f", FitToWindow, viewer),
        // view.actualSize
        KeyBinding::new("cmd-0", ActualSize, viewer),
        // view.zoomIn / view.zoomOut
        KeyBinding::new("+", ZoomIn, viewer),
        KeyBinding::new("=", ZoomIn, viewer),
        KeyBinding::new("-", ZoomOut, viewer),
    ]);
}

/// Which end of the results a step ran into.
#[derive(Clone, Copy, PartialEq)]
enum End {
    First,
    Last,
}

pub struct Viewer {
    session: Entity<Session>,
    surface: Entity<MediaSurface>,
    toolbar: Entity<ViewerToolbar>,
    focus: FocusHandle,
    /// Whether the viewer was open at the last change.
    open: bool,
    /// Closed by going to another destination: the shell keeps the viewer
    /// in the pane while it fades out, so what it shows stays until then.
    leaving: bool,
    /// Where the keyboard was before the viewer opened, to give it back.
    return_focus: Option<FocusHandle>,
    /// The "First item" and "Last item" notices, and a count that lets only
    /// the latest notice's timer hide it.
    first: motion::Fade,
    last: motion::Fade,
    notice: u64,
    _subscriptions: Vec<Subscription>,
}

impl Viewer {
    pub fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let surface = cx.new(|_| MediaSurface::default());
        let viewer = cx.entity().downgrade();
        let toolbar = cx.new(|cx| ViewerToolbar::new(session.clone(), surface.clone(), viewer, cx));
        let subscriptions =
            vec![
                cx.subscribe_in(&session, window, |this, _, change: &Change, window, cx| {
                    match change {
                        Change::Selection => {
                            // Show the thumbnail at once; the original follows when open.
                            let shown = {
                                let session = this.session.read(cx);
                                session.selected_item().map(|item| {
                                    (
                                        item.content_hash.clone(),
                                        session.thumbnail(&item.content_hash),
                                        size_of(item.width, item.height),
                                    )
                                })
                            };
                            if let Some((key, thumbnail, natural)) = shown {
                                this.surface.update(cx, |surface, cx| {
                                    surface.show(key, thumbnail, natural, None, cx)
                                });
                            }
                            this.present(cx);
                            cx.notify();
                        }
                        Change::Viewer => {
                            this.take_keyboard(window, cx);
                            // Each opening starts fitted, as a fresh
                            // `MediaView` does.
                            let (open, fading_out) = {
                                let session = this.session.read(cx);
                                (
                                    session.viewer,
                                    matches!(session.transition, Transition::FadingOut(_)),
                                )
                            };
                            if open && !this.open {
                                this.surface.update(cx, |surface, _| surface.refit());
                            }
                            this.leaving = !open && this.open && fading_out;
                            this.open = open;
                            if !this.leaving {
                                this.present(cx);
                            }
                            cx.notify();
                        }
                        Change::Navigation if this.leaving => {
                            let fading_out = matches!(
                                this.session.read(cx).transition,
                                Transition::FadingOut(_)
                            );
                            if !fading_out {
                                this.leaving = false;
                                this.present(cx);
                                cx.notify();
                            }
                        }
                        Change::Details => {
                            this.present(cx);
                            cx.notify();
                        }
                        _ => {}
                    }
                }),
            ];
        Self {
            session,
            surface,
            toolbar,
            focus: cx.focus_handle(),
            open: false,
            leaving: false,
            return_focus: None,
            first: motion::Fade::default(),
            last: motion::Fade::default(),
            notice: 0,
            _subscriptions: subscriptions,
        }
    }

    /// The toolbar the titlebar shows while the viewer is open.
    pub fn toolbar(&self) -> Entity<ViewerToolbar> {
        self.toolbar.clone()
    }

    /// The viewer has the keyboard while it is open, so its shortcuts apply,
    /// and gives it back when it closes.
    fn take_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.session.read(cx).viewer;
        if open && !self.focus.contains_focused(window, cx) {
            self.return_focus = window.focused(cx);
            window.focus(&self.focus, cx);
        } else if !open && let Some(focus) = self.return_focus.take() {
            window.focus(&focus, cx);
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
                    None,
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
                            size_of(media.facts.width, media.facts.height),
                            PathBuf::from(&media.file_path),
                            media.facts.mime.clone(),
                        )
                    })
            }
        };
        self.surface.update(cx, |surface, cx| match target {
            Some((key, thumbnail, natural, path, mime)) => {
                surface.open(key, thumbnail, natural, path, &mime, cx)
            }
            None => surface.stop(),
        });
    }

    /// `navigate` in `MediaView.tsx`: a step past either end of the results
    /// shows a notice for 800 ms instead; otherwise the shell moves the
    /// selection, as the arrow keys in the grid do.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let (index, total, loaded) = {
            let session = self.session.read(cx);
            if session.demo.is_some() {
                return;
            }
            let Some(index) = session.focus else {
                return;
            };
            (index, session.total as usize, session.items.len())
        };
        let target = index as isize + delta;
        if target < 0 || target as usize >= total {
            self.notify_end(if target < 0 { End::First } else { End::Last }, cx);
            return;
        }
        self.first.set(false, motion::BOUNDARY_FADE);
        self.last.set(false, motion::BOUNDARY_FADE);
        // Fetch the next page before the step reaches the end of what is
        // loaded.
        if target as usize + 1 >= loaded {
            self.session.update(cx, |session, cx| session.load_more(cx));
        }
        if (target as usize) < loaded {
            if delta < 0 {
                window.dispatch_action(Box::new(crate::shell::Previous), cx);
            } else {
                window.dispatch_action(Box::new(crate::shell::Next), cx);
            }
        }
        cx.notify();
    }

    fn notify_end(&mut self, end: End, cx: &mut Context<Self>) {
        self.first.set(end == End::First, motion::BOUNDARY_FADE);
        self.last.set(end == End::Last, motion::BOUNDARY_FADE);
        self.notice += 1;
        let notice = self.notice;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(motion::BOUNDARY_HOLD).await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if this.notice == notice {
                    this.first.set(false, motion::BOUNDARY_FADE);
                    this.last.set(false, motion::BOUNDARY_FADE);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// `.boundaryLeft` / `.boundaryRight`: a dark pill at the middle of
    /// either side.
    fn notice(&self, end: End, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let (fade, label) = match end {
            End::First => (&self.first, "First item"),
            End::Last => (&self.last, "Last item"),
        };
        let side = div().absolute().top_0().bottom_0().flex().items_center();
        let side = match end {
            End::First => side.left(px(16.)),
            End::Last => side.right(px(16.)),
        };
        side.child(
            div()
                .px(px(14.))
                .py(px(8.))
                .rounded(px(theme::RADIUS_MD))
                .bg(rgba(theme::VIEWER_NOTICE_BACKGROUND))
                .text(theme::TEXT_MD)
                .text_color(theme.text_secondary())
                .whitespace_nowrap()
                .opacity(fade.value_with(motion::EASE))
                .child(label),
        )
    }
}

/// An item's size in image pixels, when the library knows it.
fn size_of(width: Option<u32>, height: Option<u32>) -> Option<camera::Size> {
    Some((width? as f32, height? as f32)).filter(|size| size.0 > 0. && size.1 > 0.)
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Viewer);
        if self.first.animating() || self.last.animating() {
            window.request_animation_frame();
        }
        let (members, member) = {
            let session = self.session.read(cx);
            let members = if session.demo.is_some() {
                1
            } else {
                session
                    .details
                    .as_ref()
                    .map_or(1, |details| details.media.len())
            };
            (members, session.member)
        };
        let mut pane = div()
            .id("viewer")
            .track_focus(&self.focus)
            .key_context("Viewer")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme::theme(cx).background())
            .on_action(cx.listener(|this, _: &PreviousItem, window, cx| this.step(-1, window, cx)))
            .on_action(cx.listener(|this, _: &NextItem, window, cx| this.step(1, window, cx)))
            .on_action(cx.listener(|this, _: &FitToWindow, _, cx| {
                this.surface.update(cx, |surface, cx| surface.fit(cx));
            }))
            .on_action(cx.listener(|this, _: &ActualSize, _, cx| {
                this.surface
                    .update(cx, |surface, cx| surface.actual_size(cx));
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| {
                this.surface
                    .update(cx, |surface, cx| surface.step_zoom(true, cx));
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| {
                this.surface
                    .update(cx, |surface, cx| surface.step_zoom(false, cx));
            }))
            .child(self.surface.clone())
            .child(self.notice(End::First, cx))
            .child(self.notice(End::Last, cx));
        if members > 1 {
            let member_step = |id: &'static str,
                               icon: &'static str,
                               label: &'static str,
                               delta: isize,
                               enabled: bool,
                               cx: &mut Context<Self>| {
                strip::strip_button(id, icon, ui::tip(label))
                    .disabled(!enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.step_member(delta, cx));
                    }))
            };
            pane = pane.child(
                strip::strip()
                    .child(member_step(
                        "member-previous",
                        "toolbar-chevron-left",
                        "Previous",
                        -1,
                        member > 0,
                        cx,
                    ))
                    .child(strip::counter(member + 1, members, cx))
                    .child(member_step(
                        "member-next",
                        "toolbar-chevron-right",
                        "Next",
                        1,
                        member + 1 < members,
                        cx,
                    )),
            );
        }
        pane
    }
}
