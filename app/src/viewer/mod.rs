//! The three ways to look at an item. Each of them hosts a media surface
//! ([`surface::MediaSurface`]): the media view, which replaces the grid while
//! an item is open (this module); Quick Look, which covers the whole window
//! ([`quick_look`]); and the detail window ([`detail`]). The media view's
//! controls are in the titlebar, where [`toolbar::ViewerToolbar`] takes the
//! place of the grid's toolbar.
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
// (`crate::keymap`). They apply while the viewer has keyboard focus, and
// they take precedence over the grid's bindings of the same keys. While a
// Flash movie has keyboard focus, the keys go to the movie instead.
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

/// Emitted when the viewer is right-clicked, to ask the shell to open a
/// context menu. `position` is where the pointer was, in window coordinates.
/// `member` is true when a member of a collection was right-clicked.
#[derive(Default)]
pub struct MenuRequested {
    pub position: Point<Pixels>,
    pub member: bool,
    /// The members that a member's menu acts on, as indexes into the
    /// collection's order: the editor's selection, or the one member that
    /// was right-clicked.
    pub members: Vec<usize>,
    /// True when the collection's editor asked for the menu. The menu then
    /// has the rows that sort the collection's contents and the view rows.
    pub editor: bool,
    /// True when the space between a collection's members was
    /// right-clicked. The menu is then the collection's own menu.
    pub collection: bool,
    /// True when a member that is opened by itself was right-clicked. The
    /// menu is then the one for a file that has no item to edit.
    pub alone: bool,
}

impl EventEmitter<MenuRequested> for Viewer {}

/// Emitted to ask the shell to open Quick Look on a member of the collection
/// that the editor shows. The number is the member's index in the
/// collection's order.
pub struct MemberQuickLook(pub usize);

impl EventEmitter<MemberQuickLook> for Viewer {}

impl MenuRequested {
    /// Returns the request for the menu of a member of a collection.
    /// `member` is the index of the member that the menu acts on.
    fn member(position: Point<Pixels>, member: usize) -> Self {
        Self {
            position,
            member: true,
            members: vec![member],
            ..Default::default()
        }
    }

    /// Returns the request for a right-click on the viewer at `position`.
    /// When a member is opened by itself (`alone` is its index), the menu is
    /// the one for a file that has no item to edit. When a collection is
    /// shown, there is no request, because the members and the space between
    /// them ask for their own menus. Otherwise the menu is the selected
    /// item's.
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

/// The end of the results that a step went past.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum End {
    First,
    Last,
}

/// Returns the index that a step of `delta` from `current` reaches among
/// `count` entries, or the end that the step went past.
pub fn step_within(current: usize, delta: isize, count: usize) -> Result<usize, End> {
    match current as isize + delta {
        target if target < 0 => Err(End::First),
        target if target as usize >= count => Err(End::Last),
        target => Ok(target as usize),
    }
}

/// Makes the member at index `member` the session's shown member of the open
/// collection. The shown member is the one that the menu rows for a single
/// file act on, and the one the surface shows when a member is opened by
/// itself.
fn point_at_member(session: &Entity<Session>, member: usize, cx: &mut App) {
    session.update(cx, |session, cx| {
        let delta = member as isize - session.member as isize;
        if delta != 0 {
            session.step_member(delta, cx);
        }
    });
}

/// The "First item" and "Last item" notices of the media view and the detail
/// window. `notice` counts the notices that were shown, so that only the
/// timer of the latest notice hides it.
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

    /// Shows the notice for `end` for 800 ms. `of` returns this boundary
    /// from the view that owns it.
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

    /// Draws the notice for `end`: a dark rounded label at the middle of the
    /// left side for the first item, or of the right side for the last item.
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

/// What a media surface needs to open one file: the item's name, the file's
/// key (its content hash), the path of its thumbnail, its size in pixels
/// when that is known, the path of the file and its MIME type.
pub struct Media {
    pub name: SharedString,
    pub key: String,
    pub thumbnail: PathBuf,
    pub natural: Option<camera::Size>,
    pub path: PathBuf,
    pub mime: String,
}

impl Media {
    /// Returns the file at index `member` of the item in `details`, or
    /// `None` when the item has no file at that index.
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

/// Returns what the media view shows: the shown member of the selected item.
/// Returns `None` until the item's details have arrived.
fn session_media(session: &Session) -> Option<Media> {
    Media::of(session, session.details.as_ref()?, session.member)
}

/// Shows the selected item's thumbnail on `surface` at once. The original
/// file is opened afterwards, by the `present` function of the host that
/// owns the surface.
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

/// Returns the options of a media surface: the preview settings, which every
/// host follows, and `navigator` and `follows_resize`, which each host
/// chooses for itself.
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

/// Records that the item was opened, so that it appears in Recently Viewed.
pub fn record_view(session: &Session, root: RootId) {
    let now = crate::now_ms();
    // Nothing waits for the result. A record that fails changes nothing on
    // screen.
    drop(session.run(move |library| library.record_recent_view(root, now)));
}

/// Returns whether the grid shows Recently Viewed. The viewers that are
/// opened from that grid do not record views, so that the order of the list
/// does not change while the user browses it.
pub fn browsing_recent(session: &Session) -> bool {
    session
        .nav
        .get(session.displayed)
        .is_some_and(|entry| matches!(entry.scope, picto_library::query::ItemScope::RecentlyViewed))
}

pub struct Viewer {
    session: Entity<Session>,
    surface: Entity<MediaSurface>,
    /// Shows the members of a collection for reading, while a collection is
    /// open.
    group: Entity<group::GroupReader>,
    /// Shows the same members as a grid in which they can be rearranged.
    editor: Entity<group_editor::GroupEditor>,
    /// Whether the open collection is shown in the reader or in the editor.
    /// `mode_root` is the collection that this applies to. Another
    /// collection opens in the reader.
    mode: group::Mode,
    mode_root: Option<RootId>,
    /// Whether a member of the collection is opened by itself. The media
    /// view then shows that member, and "Back to group" returns to the
    /// collection.
    member_open: bool,
    /// Why the last change to the collection failed. It is shown in a red
    /// strip above the members.
    error: Option<SharedString>,
    toolbar: Entity<ViewerToolbar>,
    focus: FocusHandle,
    /// Whether the viewer was open when the session last reported a change
    /// of the viewer.
    open: bool,
    /// True when the viewer was closed by going to another destination. The
    /// shell keeps the viewer in the pane while it fades out, so the viewer
    /// goes on showing the same media until the fade has ended.
    leaving: bool,
    /// The element that had keyboard focus before the viewer opened. Focus
    /// returns to it when the viewer closes.
    return_focus: Option<FocusHandle>,
    boundary: Boundary,
    /// The item that was last recorded as viewed while the viewer was open.
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
                // The menu's rows for a single file act on the session's shown
                // member, so the first of the members is made the shown member.
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
                        // The media is fitted to the pane each time the viewer
                        // opens.
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
                            // After the viewer has closed, the next collection
                            // opens in the reader.
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

    /// Returns the toolbar that the titlebar shows while the viewer is open.
    pub fn toolbar(&self) -> Entity<ViewerToolbar> {
        self.toolbar.clone()
    }

    /// Returns the media surface. The shell needs it to build the viewer's
    /// context menu.
    pub fn surface(&self) -> Entity<MediaSurface> {
        self.surface.clone()
    }

    /// Handles a right-click on a member in the reader: makes it the
    /// session's shown member and asks for the member's menu.
    fn member_menu(&mut self, event: &group::MemberMenu, cx: &mut Context<Self>) {
        point_at_member(&self.session, event.member, cx);
        cx.emit(MenuRequested::member(event.position, event.member));
    }

    /// Draws the toolbar again. The toolbar shows the collection's controls,
    /// which depend on the viewer's state. The redraw is deferred until the
    /// current update has finished, because the toolbar itself may be what
    /// called the viewer.
    fn refresh_toolbar(&self, cx: &mut Context<Self>) {
        let toolbar = self.toolbar.downgrade();
        cx.defer(move |cx| {
            let _ = toolbar.update(cx, |_, cx| cx.notify());
        });
    }

    /// Returns whether the open collection is shown in the reader or in the
    /// editor, and whether a member of it is opened by itself.
    pub fn collection_view(&self) -> (group::Mode, bool) {
        (self.mode, self.member_open)
    }

    /// Shows the selected collection in the editor, whether it is already
    /// open in the reader or not open yet. The toolbar's "Edit group" button
    /// and the item menu's "Edit Group" row run this.
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

    /// Opens the member at index `member` by itself in the media view.
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

    /// Closes the member that is opened by itself and shows the collection
    /// again, as it was before the member was opened. "Back to group" runs
    /// this.
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

    /// Goes to the previous or the next member while a member is opened by
    /// itself. A step past the first or the last member shows the notice
    /// instead.
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

    /// Removes the members at these indexes from the collection, and moves
    /// them to the Trash when `trash` is true. When one member or none is
    /// left, the collection no longer exists, and the media view closes.
    /// Otherwise the editor's selection is cleared. A failure is shown above
    /// the members.
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
                // Nothing is done when the viewer has gone on to another item
                // in the meantime.
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

    /// Sorts the open collection's members by `field` and saves the order.
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
        // A sort that fails is shown above the members, if the collection is
        // still shown. A sort that succeeds removes the message of an earlier
        // failure.
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

    /// Returns whether the viewer still shows the collection `root`. The
    /// result of a change to a collection is applied only while this is true.
    fn shows_collection(&self, root: RootId, cx: &App) -> bool {
        let session = self.session.read(cx);
        session.viewer
            && shown_collection(session).is_some_and(|details| details.root.root_id == root)
    }

    /// Gives the viewer keyboard focus while it is open, so that its
    /// shortcuts apply, and returns focus to where it was before when the
    /// viewer closes.
    fn take_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.session.read(cx).viewer;
        if open && !self.focus.contains_focused(window, cx) {
            self.return_focus = window.focused(cx);
            window.focus(&self.focus, cx);
        } else if !open && let Some(focus) = self.return_focus.take() {
            window.focus(&focus, cx);
        }
        // While the editor is shown, its grid has keyboard focus, so that its
        // keys (select all, remove, Quick Look) apply. The grid is inside the
        // viewer, so the viewer's own keys still reach the viewer.
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

    /// Shows what the session's selection is, if the viewer is open, and
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
        // A collection is shown by the reader or the editor once its details
        // have arrived. The media surface then shows nothing.
        let collection = {
            let session = self.session.read(cx);
            session
                .viewer
                .then(|| shown_collection(session))
                .flatten()
                .cloned()
        };
        if let Some(details) = collection {
            // A collection other than the previous one opens in the reader,
            // with no member opened by itself.
            if self.mode_root != Some(details.root.root_id) {
                self.mode = group::Mode::Reader;
                self.mode_root = Some(details.root.root_id);
                self.member_open = false;
                self.error = None;
            }
            if self.member_open {
                // A member is opened by itself, so the surface shows it. The
                // reader or the editor keeps its state, so that the collection
                // is as it was when the member is closed.
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
        // While the details of a selected collection have not arrived yet, the
        // mode that was chosen for the collection is kept.
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

    /// Handles Escape, Enter and Space. A member that is opened by itself is
    /// closed, and its collection is shown again. In the editor, a selection
    /// is cleared first. In every other case the media view closes.
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

    /// Returns whether the editor is shown.
    fn editing(&self, cx: &App) -> bool {
        self.mode == group::Mode::Editor
            && !self.member_open
            && collection_selected(self.session.read(cx))
    }

    /// Handles the arrow keys. While a member is opened by itself, they step
    /// through the collection's members. Otherwise they step through the
    /// results, except in the editor, where they do nothing.
    fn arrow(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.member_open {
            self.step_member(delta, cx);
        } else if !self.editing(cx) {
            self.step(delta, window, cx);
        }
    }

    /// Goes to the previous or the next result. A step past the first or the
    /// last result shows a notice for 800 ms instead. Otherwise the shell
    /// moves the selection, as the arrow keys do in the grid.
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
        // When the item to move to is not loaded, the shell first reads the
        // window of results that holds it. The item that is shown stays on
        // screen until that window has arrived.
        if delta < 0 {
            window.dispatch_action(Box::new(crate::shell::Previous), cx);
        } else {
            window.dispatch_action(Box::new(crate::shell::Next), cx);
        }
        cx.notify();
    }
}

/// Returns whether the selected item is a collection.
pub fn collection_selected(session: &Session) -> bool {
    session
        .selected_item()
        .is_some_and(|item| item.kind == picto_library::RootKind::Collection)
}

/// Returns the selected collection's details, or `None` when the selected
/// item is not a collection or its details have not arrived yet.
fn shown_collection(session: &Session) -> Option<&picto_library::RootDetails> {
    if !collection_selected(session) {
        return None;
    }
    session
        .selected_details()
        .filter(|details| details.root.kind == picto_library::RootKind::Collection)
}

/// Returns an item's size in image pixels, or `None` when the library does
/// not know it.
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
        // A collection's reader is not a paged document, so it does not get
        // the keys that scroll a document's pages. A member that is opened by
        // itself does get them.
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
        // With nothing to step through, every step goes past an end.
        assert_eq!(step_within(0, 1, 0), Err(End::Last));
    }
}
