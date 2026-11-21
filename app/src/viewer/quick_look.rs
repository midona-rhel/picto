//! Quick Look: a preview of the selected item that covers the whole window,
//! the titlebar included, on the app's background colour. The media is 48px
//! from the top and the bottom of the window and 16px from the sides. A
//! close button is in the top-right corner, and the strip with the previous
//! button, the count and the next button is along the bottom. Quick Look
//! follows the selection: a step moves the grid's selection, and the grid
//! scrolls to keep the selected item in view. It uses the viewer's key
//! bindings (the `Viewer` key context), so Space, Enter and Escape close it.
use super::{
    Close, Media, MenuRequested, NextItem, PreviousItem, browsing_recent, collection_selected,
    group, point_at_member, record_view, show_selection, step_within, strip,
    surface::{self, MediaSurface},
};
use crate::i18n::t;
use crate::session::{Change, Session};
use crate::{theme, ui};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::{RootId, RootKind};

/// Emitted when Quick Look opens or closes, so that the shell adds it to the
/// window or removes it.
pub struct Toggled;

/// The distance of the media from the top and from the bottom of the window.
/// `AREA_SIDE` is its distance from the left and the right side.
const AREA_VERTICAL: f32 = 48.;
const AREA_SIDE: f32 = 16.;
/// The corner radius of the area that the media is shown in.
const AREA_RADIUS: f32 = 8.;
/// The width and the height of the close button. `CLOSE_INSET` is its
/// distance from the top and from the right edge of the window.
const CLOSE_SIZE: f32 = 32.;
const CLOSE_INSET: f32 = 12.;

pub struct QuickLook {
    session: Entity<Session>,
    surface: Entity<MediaSurface>,
    /// Shows the members of a collection, when the selected item is a
    /// collection.
    group: Entity<group::GroupReader>,
    focus: FocusHandle,
    open: bool,
    /// The element that had keyboard focus before Quick Look opened. Focus
    /// returns to it when Quick Look closes.
    return_focus: Option<FocusHandle>,
    /// The item that was last recorded as viewed while Quick Look was open.
    recorded: Option<RootId>,
    /// Set when Quick Look was opened from the collection's editor on one
    /// member of the collection: the member's index. Quick Look then steps
    /// through the collection's members.
    member: Option<usize>,
    /// Set when the user double-clicked a member of the collection that
    /// Quick Look shows, which opens that member by itself: the member's
    /// index. Escape then goes back to the collection, and the arrow keys
    /// step through its members.
    opened: Option<usize>,
    /// Why the last change to a collection shown here failed, and which
    /// collection that was.
    error: Option<(RootId, SharedString)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Toggled> for QuickLook {}
impl EventEmitter<MenuRequested> for QuickLook {}

impl QuickLook {
    pub fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Quick Look shows no navigator.
        let surface = cx.new(|cx| MediaSurface::for_host(false, false, cx));
        let group = cx.new(|_| group::GroupReader::new(session.clone()));
        let subscriptions = vec![
            cx.subscribe(&group, |this, _, event: &group::MemberMenu, cx| {
                point_at_member(&this.session, event.member, cx);
                cx.emit(MenuRequested::member(event.position, event.member));
            }),
            cx.subscribe(&group, |this, _, event: &group::MemberOpened, cx| {
                this.opened = Some(event.0);
                this.surface.update(cx, |surface, _| surface.refit());
                this.present(cx);
                cx.notify();
            }),
            cx.subscribe(&group, |_, _, event: &group::CollectionMenu, cx| {
                cx.emit(MenuRequested {
                    position: event.0,
                    collection: true,
                    ..Default::default()
                });
            }),
            cx.subscribe_in(&session, window, |this, _, change: &Change, window, cx| {
                if !this.open {
                    return;
                }
                match change {
                    Change::Selection => {
                        // Quick Look closes when the item it followed is gone,
                        // or when the selection is no longer exactly one item,
                        // because there is then nothing to preview.
                        let single = {
                            let session = this.session.read(cx);
                            session.selection_count() == 1 && session.selected_item().is_some()
                        };
                        if !single {
                            this.close(window, cx);
                            return;
                        }
                        // The selection is now another item, so a member that
                        // was opened by itself is closed.
                        this.opened = None;
                        show_selection(&this.session, &this.surface, cx);
                        this.present(cx);
                        cx.notify();
                    }
                    Change::Details | Change::Viewer => {
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
            focus: cx.focus_handle(),
            open: false,
            return_focus: None,
            recorded: None,
            member: None,
            opened: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Returns the media surface. The shell needs it to build the viewer's
    /// context menu.
    pub fn surface(&self) -> Entity<MediaSurface> {
        self.surface.clone()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Records the result of a change to the collection `root` (a sort, or
    /// the removal of members). A failure is shown above the members, as in
    /// the media view. A success removes the message of an earlier failure.
    pub fn collection_edited(
        &mut self,
        root: RootId,
        failure: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.error = failure.map(|failure| (root, failure));
        cx.notify();
    }

    /// Opens Quick Look on the selected item. Does nothing unless exactly
    /// one item is selected.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let single = {
            let session = self.session.read(cx);
            session.selection_count() == 1 && session.selected_item().is_some()
        };
        if self.open || !single {
            return;
        }
        // A scroll in the grid that is still moving after a trackpad flick
        // must not go on to scroll what Quick Look shows.
        crate::ui::momentum::suppress();
        self.open = true;
        self.return_focus = window.focused(cx);
        window.focus(&self.focus, cx);
        // The media is fitted to the area each time Quick Look opens.
        self.surface.update(cx, |surface, _| surface.refit());
        show_selection(&self.session, &self.surface, cx);
        self.present(cx);
        cx.emit(Toggled);
        cx.notify();
    }

    /// Opens Quick Look on the member at index `member` of the selected
    /// collection. The collection's editor uses this. Nothing is recorded as
    /// viewed.
    pub fn open_member(&mut self, member: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.open || !collection_selected(self.session.read(cx)) {
            return;
        }
        self.member = Some(member);
        self.open = true;
        self.return_focus = window.focused(cx);
        window.focus(&self.focus, cx);
        self.surface.update(cx, |surface, _| surface.refit());
        self.present(cx);
        cx.emit(Toggled);
        cx.notify();
    }

    /// Returns the number of members of the selected collection.
    fn member_count(&self, cx: &App) -> usize {
        self.session
            .read(cx)
            .selected_details()
            .map_or(0, |details| details.media.len())
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        // A scroll that is still moving after a trackpad flick must not go on
        // to scroll the grid that Quick Look was covering.
        crate::ui::momentum::suppress();
        self.open = false;
        self.member = None;
        self.opened = None;
        self.recorded = None;
        self.error = None;
        self.surface.update(cx, |surface, _| surface.rest());
        self.group.update(cx, |group, cx| group.clear(cx));
        if let Some(focus) = self.return_focus.take() {
            window.focus(&focus, cx);
        }
        cx.emit(Toggled);
        cx.notify();
    }

    /// Opens the selected item's file once its details have arrived, and
    /// records the item as viewed, except while the grid shows Recently
    /// Viewed. A collection is shown as its members. While one member is
    /// shown by itself, that member's file is opened instead.
    fn present(&mut self, cx: &mut Context<Self>) {
        if let Some(member) = self.member.or(self.opened) {
            let media = {
                let session = self.session.read(cx);
                session
                    .selected_details()
                    .filter(|details| details.root.kind == RootKind::Collection)
                    .and_then(|details| Media::of(session, details, member))
            };
            if let Some(media) = media {
                self.surface
                    .update(cx, |surface, cx| media.open(surface, cx));
            }
            return;
        }
        let (media, root, collection) = {
            let session = self.session.read(cx);
            let root = session.selected_item().map(|item| item.root_id);
            if let Some(root) = root.filter(|root| self.recorded != Some(*root))
                && !browsing_recent(session)
            {
                record_view(session, root);
            }
            let media = session
                .selected_details()
                .and_then(|details| Media::of(session, details, session.member));
            let collection = collection_selected(session)
                .then(|| session.selected_details().cloned())
                .flatten();
            (media, root, collection)
        };
        self.recorded = root;
        if let Some(details) = collection {
            self.surface.update(cx, |surface, _| surface.rest());
            self.group.update(cx, |group, cx| group.show(&details, cx));
            return;
        }
        if let Some(media) = media {
            self.surface
                .update(cx, |surface, cx| media.open(surface, cx));
        }
    }

    /// Handles Escape, Space and Enter. A member that was opened by itself
    /// is closed, and its collection is shown again. Otherwise Quick Look
    /// closes.
    fn close_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opened.take().is_some() {
            self.present(cx);
            cx.notify();
            return;
        }
        self.close(window, cx);
    }

    /// Handles the arrow keys. While a member is opened by itself, they step
    /// through the collection's members and stop at the first and at the
    /// last. Otherwise they do what the strip's previous and next buttons
    /// do.
    fn arrow(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opened) = self.opened {
            if let Ok(target) = step_within(opened, delta, self.member_count(cx)) {
                self.opened = Some(target);
                self.present(cx);
                cx.notify();
            }
            return;
        }
        self.step(delta, window, cx);
    }

    /// Goes to the previous or the next result by moving the grid's
    /// selection. A step past the first or the last result does nothing.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        // When Quick Look was opened on a member from the collection's
        // editor, the step goes through the collection's members and stops
        // at the first and at the last.
        if let Some(member) = self.member {
            if let Ok(target) = step_within(member, delta, self.member_count(cx)) {
                self.member = Some(target);
                self.present(cx);
                cx.notify();
            }
            return;
        }
        let (index, total) = {
            let session = self.session.read(cx);
            let Some(index) = session.focus_ordinal() else {
                return;
            };
            (index, session.total as usize)
        };
        if step_within(index, delta, total).is_err() {
            return;
        }
        // When the item to move to is not loaded, the shell first reads the
        // window of results that holds it. What is shown stays on screen
        // until that window has arrived.
        if delta < 0 {
            window.dispatch_action(Box::new(crate::shell::Previous), cx);
        } else {
            window.dispatch_action(Box::new(crate::shell::Next), cx);
        }
    }
}

impl Render for QuickLook {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (index, total, collection) = match self.member {
            Some(member) => (member, self.member_count(cx), None),
            None => {
                let session = self.session.read(cx);
                (
                    session.focus_ordinal().unwrap_or(0),
                    session.total as usize,
                    session
                        .selected_item()
                        .filter(|item| item.kind == RootKind::Collection)
                        .map(|item| item.root_id),
                )
            }
        };
        // The failure is shown only above the collection that the failed
        // change was made to.
        let error = self
            .error
            .as_ref()
            .filter(|(root, _)| collection == Some(*root))
            .map(|(_, error)| error.clone());
        let collection = collection.is_some();
        let frame = div()
            .absolute()
            .top(px(AREA_VERTICAL))
            .bottom(px(AREA_VERTICAL))
            .overflow_hidden();
        // A member that is opened by itself is shown in the same frame as its
        // collection.
        let frame = if collection {
            // The space at each side of a collection is 16% of the window's
            // width, but at least 32px and at most 280px.
            let side = (f32::from(window.viewport_size().width) * 0.16).clamp(32., 280.);
            frame.left(px(side)).right(px(side))
        } else {
            // TODO: round the corners of a zoomed image. GPUI clips to
            // rectangles only, so an image that is zoomed past the edges of
            // the area has square corners.
            frame
                .left(px(AREA_SIDE))
                .right(px(AREA_SIDE))
                .rounded(px(AREA_RADIUS))
        };
        let step = |id: &'static str,
                    icon: &'static str,
                    label: &'static str,
                    shortcut: &'static str,
                    delta: isize,
                    enabled: bool,
                    cx: &mut Context<Self>| {
            strip::strip_button(id, icon, ui::tip(label).shortcut(shortcut, cx))
                .disabled(!enabled)
                .on_click(cx.listener(move |this, _, window, cx| this.step(delta, window, cx)))
        };
        // Quick Look uses the viewer's key bindings. It does not ask for the
        // keys that scroll a document's pages, so Space closes it even while
        // a paged document is shown.
        let context = self.surface.read(cx).key_context("Viewer", false);
        // For assistive technology, Quick Look is a modal dialog. It has
        // keyboard focus while it is open.
        let quick_look = div()
            .id("quick-look")
            .role(Role::Dialog)
            .aria_modal(true)
            .aria_label(t("Quick Look"))
            .track_focus(&self.focus)
            .key_context(context);
        surface::host_actions(quick_look, &self.surface)
            .occlude()
            .absolute()
            .inset_0()
            .bg(theme.background())
            // Space, Enter and Escape are the viewer's close keys.
            .on_action(cx.listener(|this, _: &Close, window, cx| this.close_key(window, cx)))
            .on_action(cx.listener(|this, _: &PreviousItem, window, cx| this.arrow(-1, window, cx)))
            .on_action(cx.listener(|this, _: &NextItem, window, cx| this.arrow(1, window, cx)))
            // A right-click asks for the viewer's context menu, which the shell
            // shows. For a member that is shown by itself, the menu is the one
            // for a file that has no item to edit.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let alone = this.member.or(this.opened);
                    let session = this.session.read(cx);
                    if let Some(menu) = MenuRequested::on_host(event.position, alone, session) {
                        cx.emit(menu);
                    }
                }),
            )
            .child(frame.child(if collection && self.opened.is_none() {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .when_some(error, |column, error| {
                        column.child(group::inline_error(error, cx))
                    })
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(self.group.clone()),
                    )
                    .into_any_element()
            } else {
                self.surface.clone().into_any_element()
            }))
            .child(
                div()
                    .absolute()
                    .top(px(CLOSE_INSET))
                    .right(px(CLOSE_INSET))
                    .child(
                        strip::strip_button(
                            "quick-look-close",
                            "toolbar-close",
                            ui::tip(t("Close")).shortcut("view.quicklook", cx),
                        )
                        .width(CLOSE_SIZE)
                        .glyph(rgb(theme::QUICK_LOOK_GLYPH))
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            )
            .child(
                strip::strip()
                    .child(step(
                        "quick-look-previous",
                        "toolbar-chevron-left",
                        t("Previous"),
                        "view.prevImage",
                        -1,
                        index > 0,
                        cx,
                    ))
                    .child(strip::counter(index + 1, total, cx))
                    .child(step(
                        "quick-look-next",
                        "toolbar-chevron-right",
                        t("Next"),
                        "view.nextImage",
                        1,
                        index + 1 < total,
                        cx,
                    )),
            )
    }
}
