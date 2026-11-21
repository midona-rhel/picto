//! The three ways to look at an item, each a host of the one media surface
//! ([`surface::MediaSurface`]): the media view that replaces the grid while
//! an item is open (`MediaView.tsx`, this module), Quick Look over the whole
//! window ([`quick_look`]), and the detail window ([`detail`]). The media
//! view's controls are in the titlebar, where [`toolbar::ViewerToolbar`]
//! takes the grid toolbar's place.
pub mod camera;
pub mod capture;
pub mod detail;
pub mod document;
pub mod flash;
mod flash_controls;
pub mod group;
pub mod group_editor;
pub mod quick_look;
pub mod review;
mod review_art;
pub mod scaled;
mod strip;
pub mod surface;
pub mod toolbar;
pub mod trigger;
pub mod video;

use crate::i18n::t;
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

/// The viewer was right-clicked at this window position; `member` says a
/// collection's member was.
#[derive(Default)]
pub struct MenuRequested {
    pub position: Point<Pixels>,
    pub member: bool,
    /// The members a member's menu acts on, by their place in the
    /// collection: the editor's selection, or the one right-clicked.
    pub members: Vec<usize>,
    /// The collection's editor opened it (`selectionEnabled`): its contents
    /// can be sorted and the view rows show.
    pub editor: bool,
    /// The space between a collection's members: the collection's own menu
    /// (`openEmptyMenu`).
    pub collection: bool,
    /// A member open by itself: `MediaView`'s menu for a file with no item
    /// to edit (`ratingItemId={null}`).
    pub alone: bool,
}

impl EventEmitter<MenuRequested> for Viewer {}

/// Quick Look on a member of the collection the editor shows.
pub struct MemberQuickLook(pub usize);

impl EventEmitter<MemberQuickLook> for Viewer {}

impl MenuRequested {
    /// The menu of a collection's member: `member` is the one it acts on.
    fn member(position: Point<Pixels>, member: usize) -> Self {
        Self {
            position,
            member: true,
            members: vec![member],
            ..Default::default()
        }
    }

    /// A right click on a host at `position`: on a member open by itself,
    /// `MediaView`'s menu for a file with no item to edit; on a
    /// collection's reader none (its members and the space between them
    /// have their own); otherwise the item's.
    fn on_host(position: Point<Pixels>, alone: Option<usize>, session: &Session) -> Option<Self> {
        match alone {
            Some(member) => Some(Self {
                alone: true,
                ..Self::member(position, member)
            }),
            None if collection_selected(session) => None,
            None => Some(Self {
                position,
                ..Default::default()
            }),
        }
    }
}

/// Which end of the results a step ran into.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum End {
    First,
    Last,
}

/// Where a step of `delta` from `current` lands among `count` places, or
/// the end it ran into.
pub fn step_within(current: usize, delta: isize, count: usize) -> Result<usize, End> {
    match current as isize + delta {
        target if target < 0 => Err(End::First),
        target if target as usize >= count => Err(End::Last),
        target => Ok(target as usize),
    }
}

/// Points the session at the open collection's member at `member`, the one
/// the single-file rows and a member opened by itself show.
fn point_at_member(session: &Entity<Session>, member: usize, cx: &mut App) {
    session.update(cx, |session, cx| {
        let delta = member as isize - session.member as isize;
        if delta != 0 {
            session.step_member(delta, cx);
        }
    });
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
            End::First => (&self.first, t("First item")),
            End::Last => (&self.last, t("Last item")),
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
                .line_height(px(theme::TEXT_MD * theme::LINE_HEIGHT_BODY))
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
            thumbnail: crate::grid::thumbnails::existing(
                session.thumbnail(&media.facts.content_hash),
            ),
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

/// What the session's viewer shows: the shown member of the selected item,
/// once its details have arrived.
fn session_media(session: &Session) -> Option<Media> {
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
                crate::grid::thumbnails::existing(session.thumbnail(&item.content_hash)),
                size_of(item.width, item.height),
            )
        })
    };
    if let Some((key, thumbnail, natural)) = shown {
        surface.update(cx, |surface, cx| surface.show(key, thumbnail, natural, cx));
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
        transparency_grid: settings.show_transparency_grid,
        pixelated: settings.image_rendering == "pixelated",
    }
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
    /// The same members as a grid to arrange, in the editor.
    editor: Entity<group_editor::GroupEditor>,
    /// How the open collection shows, and the collection that is for:
    /// another one opens in the reader.
    mode: group::Mode,
    mode_root: Option<RootId>,
    /// A member opened by itself from the collection (`viewerIndex`): the
    /// media view shows it, "Back to group" returning to the collection.
    member_open: bool,
    /// Why the last edit of the collection failed (`.inlineError`).
    error: Option<SharedString>,
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
        let surface = cx.new(|cx| MediaSurface::for_host(true, false, cx));
        let viewer = cx.entity().downgrade();
        let toolbar = cx.new(|cx| ViewerToolbar::new(session.clone(), surface.clone(), viewer, cx));
        let group = cx.new(|_| group::GroupReader::new(session.clone()));
        let editor = cx.new(|cx| group_editor::GroupEditor::new(session.clone(), cx));
        let subscriptions = vec![
            cx.subscribe(&group, |this, _, event: &group::MemberMenu, cx| {
                this.member_menu(event, cx)
            }),
            cx.subscribe_in(
                &group,
                window,
                |this, _, event: &group::MemberOpened, window, cx| {
                    this.open_member(event.0, window, cx)
                },
            ),
            cx.subscribe(&group, |_, _, event: &group::CollectionMenu, cx| {
                cx.emit(MenuRequested {
                    position: event.0,
                    collection: true,
                    ..Default::default()
                });
            }),
            cx.subscribe_in(
                &editor,
                window,
                |this, _, event: &group::MemberOpened, window, cx| {
                    this.open_member(event.0, window, cx)
                },
            ),
            cx.subscribe(&editor, |_, _, event: &group::CollectionMenu, cx| {
                cx.emit(MenuRequested {
                    position: event.0,
                    collection: true,
                    editor: true,
                    ..Default::default()
                });
            }),
            cx.subscribe(&editor, |this, _, event: &group_editor::MemberMenu, cx| {
                // The member the single-file rows act on is the first.
                if let Some(first) = event.members.first() {
                    point_at_member(&this.session, *first, cx);
                }
                cx.emit(MenuRequested {
                    position: event.position,
                    member: true,
                    members: event.members.clone(),
                    editor: true,
                    ..Default::default()
                });
            }),
            cx.subscribe(&editor, |this, _, event: &group_editor::Reordered, cx| {
                this.error = event.0.clone();
                cx.notify();
            }),
            cx.subscribe(
                &editor,
                |_, _, event: &group_editor::QuickLookRequested, cx| {
                    cx.emit(MemberQuickLook(event.0));
                },
            ),
            cx.subscribe(
                &editor,
                |this, _, event: &group_editor::RemoveRequested, cx| {
                    this.detach_members(event.0.clone(), false, cx);
                },
            ),
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
                        if !open {
                            // Closed, the next collection opens in the
                            // reader.
                            this.mode = group::Mode::Reader;
                            this.mode_root = None;
                            this.member_open = false;
                            this.error = None;
                        }
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
        ];
        Self {
            session,
            surface,
            group,
            editor,
            mode: group::Mode::Reader,
            mode_root: None,
            member_open: false,
            error: None,
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
        point_at_member(&self.session, event.member, cx);
        cx.emit(MenuRequested::member(event.position, event.member));
    }

    /// The toolbar shows the collection's controls: it draws again once
    /// whatever changed them, which may be the toolbar itself, is done.
    fn refresh_toolbar(&self, cx: &mut Context<Self>) {
        let toolbar = self.toolbar.downgrade();
        cx.defer(move |cx| {
            let _ = toolbar.update(cx, |_, cx| cx.notify());
        });
    }

    /// How the open collection shows, and whether a member of it is open
    /// by itself.
    pub fn collection_view(&self) -> (group::Mode, bool) {
        (self.mode, self.member_open)
    }

    /// "Edit group" and the item menu's "Edit Group": the collection opens,
    /// or switches, to the editor (`setMode('editor')`).
    pub fn edit_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self
            .session
            .read(cx)
            .selected_item()
            .map(|item| item.root_id);
        self.mode = group::Mode::Editor;
        self.mode_root = root;
        self.member_open = false;
        self.present(cx);
        if self.session.read(cx).viewer {
            self.take_keyboard(window, cx);
        }
        self.refresh_toolbar(cx);
        cx.notify();
    }

    /// A member opens by itself (`setViewerIndex`).
    fn open_member(&mut self, member: usize, window: &mut Window, cx: &mut Context<Self>) {
        point_at_member(&self.session, member, cx);
        self.member_open = true;
        self.boundary.clear();
        self.surface.update(cx, |surface, _| surface.refit());
        self.present(cx);
        self.take_keyboard(window, cx);
        self.refresh_toolbar(cx);
        cx.notify();
    }

    /// "Back to group": the member closes and the collection shows again,
    /// as it was.
    pub fn close_member(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.member_open {
            return;
        }
        self.member_open = false;
        self.boundary.clear();
        self.present(cx);
        self.take_keyboard(window, cx);
        self.refresh_toolbar(cx);
        cx.notify();
    }

    /// Steps through the members while one is open by itself; a step past
    /// either end shows the notice instead.
    pub fn step_member(&mut self, delta: isize, cx: &mut Context<Self>) {
        let (member, count) = {
            let session = self.session.read(cx);
            (
                session.member,
                session
                    .selected_details()
                    .map_or(0, |details| details.media.len()),
            )
        };
        match step_within(member, delta, count) {
            Ok(target) => {
                self.boundary.clear();
                point_at_member(&self.session, target, cx);
                self.refresh_toolbar(cx);
            }
            Err(end) => self.boundary.show(end, cx, |this| &mut this.boundary),
        }
    }

    /// `detachMembers`: the members at these places leave the collection,
    /// into the Trash when `trash`. With one member or none left the
    /// collection is gone, and the media view closes; otherwise the
    /// selection clears. A failure shows over the members.
    pub fn detach_members(&mut self, members: Vec<usize>, trash: bool, cx: &mut Context<Self>) {
        let Some(details) = shown_collection(self.session.read(cx)).cloned() else {
            return;
        };
        let ids: Vec<_> = members
            .iter()
            .filter_map(|index| details.media.get(*index))
            .map(|media| media.media_id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let closes = details.media.len() - ids.len() <= 1;
        let root = details.root.root_id;
        let answer = group::detach(&self.session, root, ids, trash, cx);
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                // The viewer has moved on to another item since.
                if !this.shows_collection(root, cx) {
                    return;
                }
                match result {
                    Err(error) => this.error = Some(error.to_string().into()),
                    Ok(()) if closes => {
                        this.session
                            .update(cx, |session, cx| session.close_viewer(cx));
                    }
                    Ok(()) => {
                        this.error = None;
                        this.member_open = false;
                        this.editor.update(cx, |editor, cx| {
                            editor.deselect(cx);
                        });
                        this.refresh_toolbar(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Sorts the open collection's contents by `field` (`saveOrder`).
    pub fn sort_collection(
        &mut self,
        field: picto_library::ContentSortField,
        cx: &mut Context<Self>,
    ) {
        let Some(details) = shown_collection(self.session.read(cx)).cloned() else {
            return;
        };
        let answer = group::sort(&self.session, &details, field, cx);
        let root = details.root.root_id;
        // A failed sort shows over the members, while they are still
        // shown; one that went through takes the last failure away.
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.shows_collection(root, cx) {
                    this.error = result.err().map(|error| error.to_string().into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Whether the viewer still shows the collection `root`, for a reply to
    /// an edit of it.
    fn shows_collection(&self, root: RootId, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.viewer
            && shown_collection(session).is_some_and(|details| details.root.root_id == root)
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
        // The editor's grid has the keyboard while it shows, so its keys
        // (select all, remove, Quick Look) apply; the viewer's own keys
        // still reach the viewer around it.
        if open {
            let editing = self.mode == group::Mode::Editor
                && !self.member_open
                && collection_selected(self.session.read(cx));
            let editor = self.editor.read(cx).focus_handle().clone();
            if editing && !editor.is_focused(window) {
                window.focus(&editor, cx);
            } else if !editing && editor.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
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
                .filter(|_| session.viewer);
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
            // Another collection opens in the reader, with nothing open by
            // itself.
            if self.mode_root != Some(details.root.root_id) {
                self.mode = group::Mode::Reader;
                self.mode_root = Some(details.root.root_id);
                self.member_open = false;
                self.error = None;
            }
            if self.member_open {
                // A member by itself: the surface shows it, the collection
                // keeps its place underneath.
                let media = Media::of(
                    self.session.read(cx),
                    &details,
                    self.session.read(cx).member,
                );
                self.surface.update(cx, |surface, cx| match media {
                    Some(media) => media.open(surface, cx),
                    None => surface.rest(),
                });
                return;
            }
            self.surface.update(cx, |surface, _| surface.rest());
            match self.mode {
                group::Mode::Reader => {
                    self.editor.update(cx, |editor, cx| {
                        if editor.collection().is_some() {
                            editor.clear(cx)
                        }
                    });
                    self.group.update(cx, |group, cx| group.show(&details, cx));
                }
                group::Mode::Editor => {
                    self.group.update(cx, |group, cx| {
                        if group.root().is_some() {
                            group.clear(cx)
                        }
                    });
                    self.editor
                        .update(cx, |editor, cx| editor.show(&details, cx));
                }
            }
            return;
        }
        // A collection whose details are on their way keeps its mode.
        if !collection_selected(self.session.read(cx)) {
            self.mode_root = None;
            self.member_open = false;
        }
        self.editor.update(cx, |editor, cx| {
            if editor.collection().is_some() {
                editor.clear(cx)
            }
        });
        self.group.update(cx, |group, cx| {
            if group.root().is_some() {
                group.clear(cx)
            }
        });
        self.surface.update(cx, |surface, cx| match target {
            Some(media) => media.open(surface, cx),
            None => surface.rest(),
        });
    }

    /// Escape, Enter and Space: a member open by itself goes back to its
    /// collection; the editor clears its selection first; anything else
    /// closes the media view.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.member_open {
            self.close_member(window, cx);
            return;
        }
        if self.editing(cx) && self.editor.update(cx, |editor, cx| editor.deselect(cx)) {
            return;
        }
        self.session
            .update(cx, |session, cx| session.close_viewer(cx));
    }

    /// Whether the editor shows.
    fn editing(&self, cx: &App) -> bool {
        self.mode == group::Mode::Editor
            && !self.member_open
            && collection_selected(self.session.read(cx))
    }

    /// The arrow keys: through the members of one open by itself, through
    /// the results otherwise; the editor's grid does not move with them.
    fn arrow(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.member_open {
            self.step_member(delta, cx);
        } else if !self.editing(cx) {
            self.step(delta, window, cx);
        }
    }

    /// `navigate` in `MediaView.tsx`: a step past either end of the results
    /// shows a notice for 800 ms instead; otherwise the shell moves the
    /// selection, as the arrow keys in the grid do.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let (index, total) = {
            let session = self.session.read(cx);
            let Some(index) = session.focus_ordinal() else {
                return;
            };
            (index, session.total as usize)
        };
        if let Err(end) = step_within(index, delta, total) {
            self.boundary.show(end, cx, |this| &mut this.boundary);
            return;
        }
        self.boundary.clear();
        // The grid's move reads the item's window first when it is not
        // held; the item shown stays until it arrives.
        if delta < 0 {
            window.dispatch_action(Box::new(crate::shell::Previous), cx);
        } else {
            window.dispatch_action(Box::new(crate::shell::Next), cx);
        }
        cx.notify();
    }
}

/// Whether the selected item is a collection.
pub fn collection_selected(session: &Session) -> bool {
    session
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
        // A collection's reader is not a document: its pages do not scroll
        // by the keys.
        let context = self
            .surface
            .read(cx)
            .key_context("Viewer", !collection || self.member_open);
        let pane = div()
            .id("viewer")
            .track_focus(&self.focus)
            .key_context(context)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme::theme(cx).background())
            .on_action(cx.listener(|this, _: &Close, window, cx| this.close(window, cx)))
            .on_action(cx.listener(|this, _: &PreviousItem, window, cx| this.arrow(-1, window, cx)))
            .on_action(cx.listener(|this, _: &NextItem, window, cx| this.arrow(1, window, cx)))
            .map(|pane| surface::host_actions(pane, &self.surface))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let session = this.session.read(cx);
                    let alone = this.member_open.then_some(session.member);
                    if let Some(menu) = MenuRequested::on_host(event.position, alone, session) {
                        cx.emit(menu);
                    }
                }),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .when_some(
                        self.error
                            .clone()
                            .filter(|_| collection && !self.member_open),
                        |column, error| column.child(group::inline_error(error, cx)),
                    )
                    .child(div().relative().flex_1().min_h_0().child(
                        if collection && !self.member_open {
                            match self.mode {
                                group::Mode::Reader => self.group.clone().into_any_element(),
                                group::Mode::Editor => self.editor.clone().into_any_element(),
                            }
                        } else {
                            self.surface.clone().into_any_element()
                        },
                    )),
            )
            .child(self.boundary.notice(End::First, cx))
            .child(self.boundary.notice(End::Last, cx));
        pane
    }
}

#[cfg(test)]
mod tests {
    use super::{End, step_within};

    #[test]
    fn a_step_lands_inside_or_names_the_end_it_ran_into() {
        assert_eq!(step_within(0, 1, 3), Ok(1));
        assert_eq!(step_within(2, -2, 3), Ok(0));
        assert_eq!(step_within(0, -1, 3), Err(End::First));
        assert_eq!(step_within(2, 1, 3), Err(End::Last));
        // Nothing to step through: every step runs into an end.
        assert_eq!(step_within(0, 1, 0), Err(End::Last));
    }
}
