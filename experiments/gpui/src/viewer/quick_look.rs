//! Quick Look (`GridQuickLook.tsx`, `QuickLook.tsx`, `QuickLookHost.tsx`): a
//! peek at the selected item over the whole window, titlebar included, on
//! the app background. The media sits 48px from the top and bottom and 16px
//! from the sides; a close button in the top-right corner and the
//! previous / count / next strip along the bottom. It follows the
//! selection: stepping moves the grid's selection, which keeps it in view.
//! It has the viewer's keys (`Viewer`), so Space, Enter and Escape close it.
use super::{
    ActualSize, Close, FitToWindow, Media, MenuRequested, NextItem, PreviousItem, ZoomIn, ZoomOut,
    browsing_recent, collection_selected, group, options, record_view, show_selection, strip,
    surface::MediaSurface,
};
use crate::session::{Change, Session};
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::{RootId, RootKind};

/// Quick Look opened or closed, so the shell shows or drops it.
pub struct Toggled;

/// `.imageArea`'s margin: 48px above and below, 16px at the sides.
const AREA_VERTICAL: f32 = 48.;
const AREA_SIDE: f32 = 16.;
/// `.imageArea`'s `border-radius: 8px`.
const AREA_RADIUS: f32 = 8.;
/// `.closeButton`: 32px, 12px from the top-right corner.
const CLOSE_SIZE: f32 = 32.;
const CLOSE_INSET: f32 = 12.;

pub struct QuickLook {
    session: Entity<Session>,
    surface: Entity<MediaSurface>,
    /// A collection's members (`GroupQuickLookContent`).
    group: Entity<group::GroupReader>,
    focus: FocusHandle,
    open: bool,
    /// Where the keyboard was before Quick Look opened, to give it back.
    return_focus: Option<FocusHandle>,
    /// The item last recorded as viewed while open.
    recorded: Option<RootId>,
    /// A member of the open collection's editor, peeked at by itself
    /// (`quickLookIndex`): Quick Look steps through the members.
    member: Option<usize>,
    /// A member of the collection Quick Look shows, opened by itself with a
    /// double-click (`viewerIndex`): Escape goes back to the collection,
    /// the arrows step through its members.
    opened: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Toggled> for QuickLook {}
impl EventEmitter<MenuRequested> for QuickLook {}

impl QuickLook {
    pub fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let surface = cx.new(|cx| {
            let mut surface = MediaSurface::default();
            // No navigator in Quick Look.
            surface.set_options(options(cx, false, false), cx);
            surface
        });
        let group = cx.new(|_| group::GroupReader::new(session.clone()));
        let subscriptions = vec![
            cx.subscribe(&group, |this, _, event: &group::MemberMenu, cx| {
                this.session.update(cx, |session, cx| {
                    let delta = event.member as isize - session.member as isize;
                    if delta != 0 {
                        session.step_member(delta, cx);
                    }
                });
                cx.emit(MenuRequested {
                    position: event.position,
                    member: true,
                    members: vec![event.member],
                    ..Default::default()
                });
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
                        // The item it followed is gone, or the selection is
                        // no longer one item: nothing to peek at.
                        let single = {
                            let session = this.session.read(cx);
                            session.selection_count() == 1 && session.selected_item().is_some()
                        };
                        if !single {
                            this.close(window, cx);
                            return;
                        }
                        // Another item: a member opened by itself closes.
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
            cx.subscribe(
                &crate::settings::model(cx),
                |this, _, _: &crate::settings::Applied, cx| {
                    let options = options(cx, false, false);
                    this.surface
                        .update(cx, |surface, cx| surface.set_options(options, cx));
                },
            ),
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
            _subscriptions: subscriptions,
        }
    }

    /// The media surface, for the viewer's context menu.
    pub fn surface(&self) -> Entity<MediaSurface> {
        self.surface.clone()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens on the selected item, which must be the only one selected.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let single = {
            let session = self.session.read(cx);
            session.selection_count() == 1 && session.selected_item().is_some()
        };
        if self.open || !single {
            return;
        }
        self.open = true;
        self.return_focus = window.focused(cx);
        window.focus(&self.focus, cx);
        // Each opening starts fitted, as a fresh `QuickLookContent` does.
        self.surface.update(cx, |surface, _| surface.refit());
        show_selection(&self.session, &self.surface, cx);
        self.present(cx);
        cx.emit(Toggled);
        cx.notify();
    }

    /// Opens on the member at `member` of the selected collection, as the
    /// collection's editor peeks at one; nothing is recorded as viewed.
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

    /// The members of the collection a member is peeked from.
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
        self.open = false;
        self.member = None;
        self.opened = None;
        self.recorded = None;
        self.surface.update(cx, |surface, _| surface.rest());
        self.group.update(cx, |group, cx| group.clear(cx));
        if let Some(focus) = self.return_focus.take() {
            window.focus(&focus, cx);
        }
        cx.emit(Toggled);
        cx.notify();
    }

    /// Opens the selected item's file once its details are there, and
    /// records it as viewed (not while browsing Recently Viewed).
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

    /// `navigate`: steps through the results by moving the grid's selection;
    /// a step past either end does nothing.
    /// Escape, Space and Enter: a member opened by itself goes back to its
    /// collection; otherwise Quick Look closes.
    fn close_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opened.take().is_some() {
            self.present(cx);
            cx.notify();
            return;
        }
        self.close(window, cx);
    }

    /// The arrow keys: through the members of one opened by itself, as
    /// `MediaView` steps, stopping at either end; otherwise as the strip.
    fn arrow(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opened) = self.opened {
            let target = opened as isize + delta;
            if target >= 0 && (target as usize) < self.member_count(cx) {
                self.opened = Some(target as usize);
                self.present(cx);
                cx.notify();
            }
            return;
        }
        self.step(delta, window, cx);
    }

    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        // Through the members of the collection a member is peeked from,
        // stopping at either end.
        if let Some(member) = self.member {
            let target = member as isize + delta;
            if target >= 0 && (target as usize) < self.member_count(cx) {
                self.member = Some(target as usize);
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
        let target = index as isize + delta;
        if target < 0 || target as usize >= total {
            return;
        }
        // The grid's move reads the item's window first when it is not
        // held; what is shown stays until it arrives.
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
            Some(member) => (member, self.member_count(cx), false),
            None => {
                let session = self.session.read(cx);
                (
                    session.focus_ordinal().unwrap_or(0),
                    session.total as usize,
                    session
                        .selected_item()
                        .is_some_and(|item| item.kind == RootKind::Collection),
                )
            }
        };
        let frame = div()
            .absolute()
            .top(px(AREA_VERTICAL))
            .bottom(px(AREA_VERTICAL))
            .overflow_hidden();
        // A member opened by itself shows in the collection's frame.
        let frame = if collection {
            // `GroupQuickLook.module.css`: `clamp(32px, 16vw, 280px)` at
            // the sides.
            let side = (f32::from(window.viewport_size().width) * 0.16).clamp(32., 280.);
            frame.left(px(side)).right(px(side))
        } else {
            // TODO: the rounded corners clip only the area's rectangle;
            // GPUI clips to rectangles, so a zoomed image shows square
            // corners where production rounds them.
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
        let media = self.surface.read(cx).player().is_some();
        let surface = self.surface.downgrade();
        let quick_look = div()
            .id("quick-look")
            .track_focus(&self.focus)
            .key_context(super::video::key_context("Viewer", media));
        super::video::on_actions(quick_look, move |cx| {
            surface
                .upgrade()
                .and_then(|surface| surface.read(cx).player())
        })
        .occlude()
        .absolute()
        .inset_0()
        .bg(theme.background())
        // Space, Enter and Escape: the viewer's close keys.
        .on_action(cx.listener(|this, _: &Close, window, cx| this.close_key(window, cx)))
        .on_action(cx.listener(|this, _: &PreviousItem, window, cx| this.arrow(-1, window, cx)))
        .on_action(cx.listener(|this, _: &NextItem, window, cx| this.arrow(1, window, cx)))
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
        .map(|pane| super::document::handle_actions(pane, &self.surface))
        // The viewer's context menu (`useViewerEntityContextMenu`), which
        // the shell shows.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                // A member peeked at by itself: `QuickLook`'s menu for a
                // file with no item to edit.
                if let Some(member) = this.member.or(this.opened) {
                    cx.emit(MenuRequested {
                        position: event.position,
                        member: true,
                        members: vec![member],
                        alone: true,
                        ..Default::default()
                    });
                    return;
                }
                if collection_selected(this.session.read(cx)) {
                    return;
                }
                cx.emit(MenuRequested {
                    position: event.position,
                    ..Default::default()
                });
            }),
        )
        .child(frame.child(if collection && self.opened.is_none() {
            self.group.clone().into_any_element()
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
                        ui::tip("Close").shortcut("view.quicklook", cx),
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
                    "Previous",
                    "view.prevImage",
                    -1,
                    index > 0,
                    cx,
                ))
                .child(strip::counter(index + 1, total, cx))
                .child(step(
                    "quick-look-next",
                    "toolbar-chevron-right",
                    "Next",
                    "view.nextImage",
                    1,
                    index + 1 < total,
                    cx,
                )),
        )
    }
}
