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
                    flash: false,
                    member: true,
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
                            session.selection.len() == 1 && session.selected_item().is_some()
                        };
                        if !single {
                            this.close(window, cx);
                            return;
                        }
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
            session.selection.len() == 1 && session.selected_item().is_some()
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

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.recorded = None;
        self.surface.update(cx, |surface, _| surface.stop());
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
            self.surface.update(cx, |surface, _| surface.stop());
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
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let (index, total, loaded) = {
            let session = self.session.read(cx);
            let Some(index) = session.focus else {
                return;
            };
            (index, session.total as usize, session.items.len())
        };
        let target = index as isize + delta;
        if target < 0 || target as usize >= total {
            return;
        }
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
    }
}

impl Render for QuickLook {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (index, total, collection) = {
            let session = self.session.read(cx);
            (
                session.focus.unwrap_or(0),
                session.total as usize,
                session
                    .selected_item()
                    .is_some_and(|item| item.kind == RootKind::Collection),
            )
        };
        let frame = div()
            .absolute()
            .top(px(AREA_VERTICAL))
            .bottom(px(AREA_VERTICAL))
            .overflow_hidden();
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
        .on_action(cx.listener(|this, _: &Close, window, cx| this.close(window, cx)))
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
        .map(|pane| super::document::handle_actions(pane, &self.surface))
        // The viewer's context menu (`useViewerEntityContextMenu`), which
        // the shell shows.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
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
        // TODO: the copy shortcut (`edit.copy`) puts the file on the
        // clipboard.
        .child(frame.child(if collection {
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
