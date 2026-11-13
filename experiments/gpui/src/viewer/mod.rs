//! The three ways to look at an item, each a host of the one media surface
//! ([`surface::MediaSurface`]): the media view that replaces the grid while
//! an item is open (`MediaView.tsx`, this module), Quick Look over the whole
//! window ([`quick_look`]), and the detail window ([`detail`]). The media
//! view's controls are in the titlebar, where [`toolbar::ViewerToolbar`]
//! takes the grid toolbar's place.
pub mod camera;
pub mod detail;
pub mod document;
pub mod flash;
mod flash_controls;
pub mod group;
pub mod quick_look;
mod strip;
pub mod surface;
pub mod toolbar;
pub mod trigger;
pub mod video;

use crate::session::{Change, Session, Transition};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::RootId;
use std::path::PathBuf;
use surface::MediaSurface;
use toolbar::ViewerToolbar;

// The viewer's actions. Their keys come from the shortcut registry
// (`crate::keymap`): they apply while the viewer has the keyboard and win
// over the grid's bindings of the same keys; Flash keeps its keys for the
// movie.
actions!(
    viewer,
    [
        PreviousItem,
        NextItem,
        FitToWindow,
        ActualSize,
        ZoomIn,
        ZoomOut,
        Close
    ]
);

/// The viewer was right-clicked at this window position; `flash` says a
/// Flash movie is showing, `member` that a collection's member was.
pub struct MenuRequested {
    pub position: Point<Pixels>,
    pub flash: bool,
    pub member: bool,
}

impl EventEmitter<MenuRequested> for Viewer {}

/// Which end of the results a step ran into.
#[derive(Clone, Copy, PartialEq)]
pub enum End {
    First,
    Last,
}

/// The "First item" and "Last item" notices of the media view and the
/// detail window, and a count that lets only the latest notice's timer hide
/// it.
#[derive(Default)]
pub struct Boundary {
    first: motion::Fade,
    last: motion::Fade,
    notice: u64,
}

impl Boundary {
    pub fn clear(&mut self) {
        self.first.set(false, motion::BOUNDARY_FADE);
        self.last.set(false, motion::BOUNDARY_FADE);
    }

    pub fn animating(&self) -> bool {
        self.first.animating() || self.last.animating()
    }

    /// Shows the notice for `end` for 800 ms. `of` finds this boundary in
    /// the view that owns it.
    pub fn show<T: 'static>(
        &mut self,
        end: End,
        cx: &mut Context<T>,
        of: fn(&mut T) -> &mut Boundary,
    ) {
        self.first.set(end == End::First, motion::BOUNDARY_FADE);
        self.last.set(end == End::Last, motion::BOUNDARY_FADE);
        self.notice += 1;
        let notice = self.notice;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(motion::BOUNDARY_HOLD).await;
            let _ = this.update(cx, |this: &mut T, cx| {
                let boundary = of(this);
                if boundary.notice == notice {
                    boundary.clear();
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// `.boundaryLeft` / `.boundaryRight`: a dark pill at the middle of
    /// either side.
    pub fn notice(&self, end: End, cx: &App) -> Div {
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
                .text_size(px(theme::TEXT_MD))
                .line_height(px(theme::TEXT_MD * theme::LINE_HEIGHT_INHERITED))
                .text_color(theme.text_secondary())
                .whitespace_nowrap()
                .opacity(fade.value_with(motion::EASE))
                .child(label),
        )
    }
}

/// An item as a surface opens it: its name, key, thumbnail, size, file and
/// type.
pub struct Media {
    pub name: SharedString,
    pub key: String,
    pub thumbnail: PathBuf,
    pub natural: Option<camera::Size>,
    pub path: PathBuf,
    pub mime: String,
}

impl Media {
    /// The media of `details`' member, once its details have arrived.
    pub fn of(
        session: &Session,
        details: &picto_library::RootDetails,
        member: usize,
    ) -> Option<Self> {
        details.media.get(member).map(|media| Self {
            name: details.root.name.clone().into(),
            key: media.facts.content_hash.clone(),
            thumbnail: thumbnail_file(session.thumbnail(&media.facts.content_hash)),
            natural: size_of(media.facts.width, media.facts.height),
            path: PathBuf::from(&media.file_path),
            mime: media.facts.mime.clone(),
        })
    }

    pub fn open(self, surface: &mut MediaSurface, cx: &mut Context<MediaSurface>) {
        surface.set_display_name(self.name);
        surface.open(
            self.key,
            self.thumbnail,
            self.natural,
            self.path,
            &self.mime,
            cx,
        )
    }
}

/// The thumbnail file that exists: the JPEG the library writes, or the PNG
/// written for audio waveforms and captured frames, as the media protocol
/// looks for `jpg` then `png`.
pub fn thumbnail_file(jpeg: PathBuf) -> PathBuf {
    if jpeg.exists() {
        return jpeg;
    }
    let png = jpeg.with_extension("png");
    if png.exists() { png } else { jpeg }
}

/// What the session's viewer shows: the bundled sample, or the shown member
/// of the selected item once its details have arrived.
fn session_media(session: &Session) -> Option<Media> {
    if let Some((name, mime)) = session.demo {
        return Some(Media {
            name: name.into(),
            key: name.to_owned(),
            thumbnail: PathBuf::new(),
            natural: None,
            path: session.scratch.path().join(name),
            mime: mime.to_owned(),
        });
    }
    Media::of(session, session.details.as_ref()?, session.member)
}

/// Shows the selected item's thumbnail at once; the original follows when a
/// viewer is open.
fn show_selection(session: &Entity<Session>, surface: &Entity<MediaSurface>, cx: &mut App) {
    let shown = {
        let session = session.read(cx);
        session.selected_item().map(|item| {
            (
                item.content_hash.clone(),
                thumbnail_file(session.thumbnail(&item.content_hash)),
                size_of(item.width, item.height),
            )
        })
    };
    if let Some((key, thumbnail, natural)) = shown {
        surface.update(cx, |surface, cx| {
            surface.show(key, thumbnail, natural, None, cx)
        });
    }
}

/// The preview settings every host follows (`usePreviewPreferences`), with
/// what the host itself draws.
pub fn options(cx: &App, navigator: bool, follows_resize: bool) -> surface::Options {
    let settings = crate::settings::current(cx);
    surface::Options {
        navigator,
        opens_actual: settings.image_default_zoom == "actual",
        scroll_pans: settings.viewer_trackpad_gestures,
        follows_resize,
        member: false,
    }
    // TODO: `imageRendering: pixelated` draws images with nearest-neighbour
    // sampling and `showTransparencyGrid` draws the checkerboard behind
    // them; GPUI's image element has neither yet.
}

/// `useRecordMediaView`: opening an item records it for Recently Viewed.
pub fn record_view(session: &Session, root: RootId) {
    let now = crate::now_ms();
    // Nothing waits for the answer; a failed record changes nothing shown.
    drop(session.run(move |library| library.record_recent_view(root, now)));
}

/// Whether the grid shows Recently Viewed, where the grid's viewers do not
/// record views, so its order holds while it is browsed.
pub fn browsing_recent(session: &Session) -> bool {
    session
        .nav
        .get(session.displayed)
        .is_some_and(|entry| matches!(entry.scope, picto_library::query::ItemScope::RecentlyViewed))
}

pub struct Viewer {
    session: Entity<Session>,
    surface: Entity<MediaSurface>,
    /// A collection's members, while one is open.
    group: Entity<group::GroupReader>,
    toolbar: Entity<ViewerToolbar>,
    focus: FocusHandle,
    /// Whether the viewer was open at the last change.
    open: bool,
    /// Closed by going to another destination: the shell keeps the viewer
    /// in the pane while it fades out, so what it shows stays until then.
    leaving: bool,
    /// Where the keyboard was before the viewer opened, to give it back.
    return_focus: Option<FocusHandle>,
    boundary: Boundary,
    /// The item last recorded as viewed while open.
    recorded: Option<RootId>,
    _subscriptions: Vec<Subscription>,
}

impl Viewer {
    pub fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let surface = cx.new(|cx| {
            let mut surface = MediaSurface::default();
            surface.set_options(options(cx, true, false), cx);
            surface
        });
        let viewer = cx.entity().downgrade();
        let toolbar = cx.new(|cx| ViewerToolbar::new(session.clone(), surface.clone(), viewer, cx));
        let group = cx.new(|_| group::GroupReader::new(session.clone()));
        let subscriptions = vec![
            cx.subscribe(&group, |this, _, event: &group::MemberMenu, cx| {
                this.member_menu(event, cx)
            }),
            cx.subscribe_in(&session, window, |this, _, change: &Change, window, cx| {
                match change {
                    Change::Selection => {
                        show_selection(&this.session, &this.surface, cx);
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
                        let fading_out =
                            matches!(this.session.read(cx).transition, Transition::FadingOut(_));
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
            cx.subscribe(
                &crate::settings::model(cx),
                |this, _, _: &crate::settings::Applied, cx| {
                    let options = options(cx, true, false);
                    this.surface
                        .update(cx, |surface, cx| surface.set_options(options, cx));
                },
            ),
        ];
        Self {
            session,
            surface,
            group,
            toolbar,
            focus: cx.focus_handle(),
            open: false,
            leaving: false,
            return_focus: None,
            boundary: Boundary::default(),
            recorded: None,
            _subscriptions: subscriptions,
        }
    }

    /// The toolbar the titlebar shows while the viewer is open.
    pub fn toolbar(&self) -> Entity<ViewerToolbar> {
        self.toolbar.clone()
    }

    /// The media surface, for the viewer's context menu.
    pub fn surface(&self) -> Entity<MediaSurface> {
        self.surface.clone()
    }

    /// A collection member was right-clicked: the viewer's menu for it.
    fn member_menu(&mut self, event: &group::MemberMenu, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let delta = event.member as isize - session.member as isize;
            if delta != 0 {
                session.step_member(delta, cx);
            }
        });
        cx.emit(MenuRequested {
            position: event.position,
            flash: false,
            member: true,
        });
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

    /// Opens what the session points at, if the viewer is showing, and
    /// records the item as viewed.
    fn present(&mut self, cx: &mut Context<Self>) {
        let (target, viewing) = {
            let session = self.session.read(cx);
            let viewing = session
                .selected_item()
                .map(|item| item.root_id)
                .filter(|_| session.viewer && session.demo.is_none());
            if let Some(root) = viewing.filter(|root| self.recorded != Some(*root))
                && !browsing_recent(session)
            {
                record_view(session, root);
            }
            (
                session.viewer.then(|| session_media(session)).flatten(),
                viewing,
            )
        };
        self.recorded = viewing;
        // A collection shows its members in the reader once its details
        // are there (`GroupSurface`); the surface then shows nothing.
        let collection = {
            let session = self.session.read(cx);
            session
                .viewer
                .then(|| shown_collection(session))
                .flatten()
                .cloned()
        };
        if let Some(details) = collection {
            self.surface.update(cx, |surface, _| surface.stop());
            self.group.update(cx, |group, cx| group.show(&details, cx));
            return;
        }
        self.group.update(cx, |group, cx| {
            if group.root().is_some() {
                group.clear(cx)
            }
        });
        self.surface.update(cx, |surface, cx| match target {
            Some(media) => media.open(surface, cx),
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
            let end = if target < 0 { End::First } else { End::Last };
            self.boundary.show(end, cx, |this| &mut this.boundary);
            return;
        }
        self.boundary.clear();
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
}

/// Whether the selected item is a collection.
pub fn collection_selected(session: &Session) -> bool {
    session.demo.is_none()
        && session
            .selected_item()
            .is_some_and(|item| item.kind == picto_library::RootKind::Collection)
}

/// The selected collection's details, once they have arrived.
fn shown_collection(session: &Session) -> Option<&picto_library::RootDetails> {
    if !collection_selected(session) {
        return None;
    }
    session
        .selected_details()
        .filter(|details| details.root.kind == picto_library::RootKind::Collection)
}

/// An item's size in image pixels, when the library knows it.
fn size_of(width: Option<u32>, height: Option<u32>) -> Option<camera::Size> {
    Some((width? as f32, height? as f32)).filter(|size| size.0 > 0. && size.1 > 0.)
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Viewer);
        if self.boundary.animating() {
            window.request_animation_frame();
        }
        let collection = collection_selected(self.session.read(cx));
        // "Media" while the player shows, for its keys.
        let media = self.surface.read(cx).player().is_some();
        let pane = div()
            .id("viewer")
            .track_focus(&self.focus)
            .key_context(video::key_context("Viewer", media))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme::theme(cx).background())
            .on_action(cx.listener(|this, _: &Close, _, cx| {
                this.session
                    .update(cx, |session, cx| session.close_viewer(cx));
            }))
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
            .map(|pane| document::handle_actions(pane, &self.surface))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    // A collection's members have their own menu; the space
                    // between them has the collection's, which the reader
                    // does not offer yet.
                    if collection_selected(this.session.read(cx)) {
                        return;
                    }
                    let flash = this.surface.read(cx).flash();
                    cx.emit(MenuRequested {
                        position: event.position,
                        flash,
                        member: false,
                    });
                }),
            )
            .child(if collection {
                self.group.clone().into_any_element()
            } else {
                self.surface.clone().into_any_element()
            });
        let surface = self.surface.downgrade();
        video::on_actions(pane, move |cx| {
            surface
                .upgrade()
                .and_then(|surface| surface.read(cx).player())
        })
        .child(self.boundary.notice(End::First, cx))
        .child(self.boundary.notice(End::Last, cx))
    }
}
