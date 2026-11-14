//! The grid of results. Its own view, so scrolling, thumbnail reveals and
//! fades repaint the grid and nothing else.
pub mod art;
pub mod drag;
pub mod empty;
pub mod filter_bar;
pub mod filter_editors;
pub mod filters;
pub mod format;
pub mod layout;
pub mod marquee;
pub mod menu;
pub mod nav;
pub mod originals;
pub mod prefs;
pub mod preview;
pub mod rendered;
pub mod reveal;
pub mod selection;
pub mod subfolders;
pub mod thumbnails;
pub mod tile;
pub mod window;

use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder;
use gpui::*;
use layout::{Layout, ViewMode};
use picto_library::query::ItemScope;
use std::path::PathBuf;
use thumbnails::{Thumb, Thumbnails};
use tile::{TileData, TileLook, aspect, media_rect};

/// A tile was right-clicked at this window position.
pub struct MenuRequested(pub Point<Pixels>);

/// Empty grid space was right-clicked at this window position.
pub struct EmptyMenuRequested(pub Point<Pixels>);

/// A tile was double-clicked (after selecting it) or middle-clicked; the
/// shell opens it as the Controls settings say. `ordinal` is the item's
/// place in the destination.
pub struct Activated {
    pub ordinal: usize,
    pub middle: bool,
}

pub struct GridView {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    originals: Entity<originals::Originals>,
    /// Keyboard focus returns here after a click on a tile.
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// The window's items laid out by themselves, from the top.
    layout: Layout,
    /// Where that layout lies in the whole destination's height.
    placement: window::Placement,
    /// Width, zoom, spacing, mode, text rows, and the session's window.
    layout_key: Option<(u32, u32, u32, ViewMode, u32, u64)>,
    /// The ordinal a scroll last asked for and where the viewport's middle
    /// was then: the window that arrives puts that item's tile there
    /// (`windowAnchorRef`). Any later scroll drops it.
    window_anchor: Option<(usize, f32)>,
    /// A scroll's ask for the window it needs, made at most once per
    /// `window::REQUEST_INTERVAL` (`CoalescedGridRequest`).
    window_timer: Option<Task<()>>,
    reveal: reveal::RevealTracker,
    /// The steady clock the reveal timings are measured on.
    clock: std::time::Instant,
    /// Where the grid was last painted.
    bounds: Bounds<Pixels>,
    /// The session's result set this grid last laid out.
    epoch: u64,
    /// The tile whose image the pointer is over.
    hovered: Option<usize>,
    /// When the grid last scrolled; hover drawing waits 80 ms after it.
    scrolled: std::time::Instant,
    settle: Option<Task<()>>,
    /// The scroll position the last frame was painted at.
    last_top: f32,
    /// Where the pointer last was over the grid, in window coordinates.
    pointer: Option<Point<Pixels>>,
    /// The text rows' height under each image, from the view preferences.
    text_height: f32,
    /// A rubber band being dragged out.
    marquee: Option<marquee::Marquee>,
    /// The click that ends a rubber band selects nothing.
    swallow_click: bool,
    /// A press on a tile that becomes a drag after 5px: the item's ordinal,
    /// and where.
    press: Option<(usize, Point<Pixels>)>,
    /// The rubber band adds to the selection (Shift or the selection key,
    /// Command on macOS and Control elsewhere, held).
    marquee_keep: bool,
    /// The tile's selected items are being dragged.
    dragging: bool,
    /// The click that ends a drag selects nothing.
    swallow_tile_click: bool,
    preview: preview::HoverPreview,
    /// The broken-thumbnail and font artwork.
    art: Option<art::Art>,
    /// The items the layout's tiles are for, by id, and the ordinal of the
    /// first.
    layout_ids: Vec<u32>,
    layout_start: usize,
    /// The subfolder tiles above the items, in a folder that has children.
    subfolders: subfolders::Header,
    /// Where items dragged within a folder would be put back
    /// (`reorderDropRef`): beside this tile of the window's layout.
    reorder: Option<(usize, drag::Side)>,
    /// The dragged items' files, resolved for the system in case the drag
    /// leaves the window (`resolveNativeDragHashes`); `None` inside while
    /// they are being resolved.
    drag_files: Option<Option<Vec<PathBuf>>>,
    /// The drag left the window before its files were resolved; it is
    /// handed to the system when they are (`nativeDragRequestRef.pending`).
    drag_out_waiting: bool,
    /// The drag whose files are being resolved: a reply for an earlier drag
    /// is dropped.
    drag_request: Latest,
    _session: Subscription,
    _thumbnails: Subscription,
}

/// "Return to Top" appears once the grid has scrolled this far.
const RETURN_TO_TOP_AFTER: f32 = 200.;

/// The files `media.resolve_target_paths` answered with, each once, in
/// its order (`resolveNativeDragHashes` keeps the first of each file).
fn drag_out_paths(found: &serde_json::Value) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    found
        .as_array()
        .into_iter()
        .flatten()
        .filter(|file| {
            let hash = file.get("file_hash").and_then(serde_json::Value::as_str);
            hash.is_none_or(|hash| seen.insert(hash.to_owned()))
        })
        .filter_map(|file| file.get("path")?.as_str().map(PathBuf::from))
        .collect()
}

/// Numbers the requests of one kind, so that a reply can tell whether it
/// answers the latest one: anything asked since makes it stale.
#[derive(Default)]
pub(crate) struct Latest(u64);

impl Latest {
    /// A new request, which every earlier one's reply now answers late.
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }

    /// Whether a reply to `request` still answers the latest request.
    pub fn is(&self, request: u64) -> bool {
        self.0 == request
    }
}

impl EventEmitter<MenuRequested> for GridView {}
impl EventEmitter<EmptyMenuRequested> for GridView {}

impl EventEmitter<Activated> for GridView {}

impl GridView {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&session, |this, _, change: &Change, cx| {
            // A folder changed: the subfolder tiles' covers are read again.
            if matches!(change, Change::Navigation) {
                this.subfolders.stale = true;
                this.subfolders.relist = true;
            }
            if !matches!(change, Change::Viewer) {
                cx.notify();
            }
        });
        // A thumbnail written since it was read is read again.
        let changed = cx.subscribe(
            &session,
            |this, session, event: &crate::application::Event, cx| {
                if let crate::application::Event::Thumbnail(hash) = event {
                    let jpeg = session.read(cx).thumbnail(hash);
                    this.thumbnails
                        .update(cx, |thumbnails, cx| thumbnails.forget(&jpeg, cx));
                    cx.notify();
                }
            },
        );
        let originals = cx.new(originals::Originals::new);
        // An original that finished decoding, or a missing thumbnail that
        // was made, is drawn on the next frame.
        cx.observe(&originals, |_, _, cx| cx.notify()).detach();
        cx.observe(&thumbnails, |_, _, cx| cx.notify()).detach();
        Self {
            session,
            thumbnails,
            originals,
            focus,
            scroll: ScrollHandle::new(),
            layout: Layout::default(),
            placement: window::Placement::whole(0.),
            layout_key: None,
            window_anchor: None,
            window_timer: None,
            reveal: reveal::RevealTracker::default(),
            clock: std::time::Instant::now(),
            bounds: Bounds::default(),
            epoch: 0,
            hovered: None,
            scrolled: std::time::Instant::now(),
            settle: None,
            last_top: 0.,
            pointer: None,
            text_height: 0.,
            marquee: None,
            swallow_click: false,
            press: None,
            marquee_keep: false,
            dragging: false,
            swallow_tile_click: false,
            preview: preview::HoverPreview::default(),
            art: None,
            layout_ids: Vec::new(),
            layout_start: 0,
            subfolders: Default::default(),
            reorder: None,
            drag_files: None,
            drag_out_waiting: false,
            drag_request: Latest::default(),
            _session: subscription,
            _thumbnails: changed,
        }
    }

    /// The width tiles lay out in, beside the scrollbar's gutter.
    fn grid_width(&self) -> f32 {
        (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.)
    }

    /// How far the grid's content is scrolled, header and all.
    fn raw_scroll(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    /// How far the items are scrolled: the content's scroll less the
    /// subfolder header above them, so negative while the header shows
    /// (`visibleScrollTop`, `toLayoutCoords`).
    fn scroll_top(&self) -> f32 {
        self.raw_scroll() - self.subfolders.height()
    }

    /// Scrolls so the items' layout is `top` from the viewport's top.
    fn set_scroll_top(&mut self, top: f32) {
        let raw = (top + self.subfolders.height()).max(0.);
        self.scroll.set_offset(point(px(0.), px(-raw)));
    }

    /// A window position relative to the grid's top-left corner.
    fn local(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.bounds.origin.x),
            f32::from(position.y - self.bounds.origin.y),
        )
    }

    /// The image under a window position, if any: its tile in the window's
    /// layout, and the position in that layout.
    fn image_at(&self, position: Point<Pixels>) -> Option<(usize, f32, f32)> {
        let (x, y) = self.local(position);
        let y = y + self.scroll_top() - self.placement.top;
        if x < 0. || x >= self.grid_width() {
            return None;
        }
        marquee::image_at(&self.layout, x, y, self.text_height).map(|index| (index, x, y))
    }

    /// The tile at `index` of the window's layout, placed in the whole
    /// destination's height.
    fn placed(&self, index: usize) -> Option<layout::Tile> {
        let tile = self.layout.tiles.get(index)?;
        Some(layout::Tile {
            y: tile.y + self.placement.top,
            ..*tile
        })
    }

    /// The scroll moved, or the window changed: asks, at most once per
    /// interval, for the window the viewport then needs.
    fn schedule_window(&mut self, cx: &mut Context<Self>) {
        if self.window_timer.is_some() {
            return;
        }
        self.window_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(window::REQUEST_INTERVAL)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.window_timer = None;
                this.ask_for_window(cx);
            });
        }));
    }

    /// `gridWindowDestination`, for where the grid is scrolled now.
    fn ask_for_window(&mut self, cx: &mut Context<Self>) {
        let top = self.scroll_top();
        let viewport = f32::from(self.bounds.size.height);
        let (start, total) = {
            let session = self.session.read(cx);
            // Only the window this layout was made for.
            if self.layout_key.map(|key| key.5) != Some(session.window_epoch) {
                return;
            }
            (session.start, session.total as usize)
        };
        let tops = self
            .layout
            .tiles
            .iter()
            .map(|tile| tile.y + self.placement.top)
            .collect::<Vec<_>>();
        let Some(ordinal) = window::destination(self.placement, start, &tops, total, top, viewport)
        else {
            return;
        };
        self.window_anchor = Some((ordinal, top + viewport / 2.));
        self.session
            .update(cx, |session, cx| session.request_window_at(ordinal, cx));
    }

    /// Hover tracking: which image the pointer is over, and the hover
    /// preview's timers for its magnifier.
    fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        if self.marquee.is_some() {
            return;
        }
        let hit = self.image_at(position);
        let hovered = hit.map(|(index, _, _)| index);
        if hovered != self.hovered {
            self.hovered = hovered;
            cx.notify();
        }
        let magnifier = hit.filter(|&(index, x, y)| {
            marquee::on_magnifier(&self.layout.tiles[index], x, y, self.text_height)
        });
        if let Some((index, _, _)) = magnifier {
            // The item under the magnifier when the pointer arrived is the
            // one previewed, as the Electron grid's timer captures it: the
            // item the tile was laid out for, if the session still holds it.
            let laid_out = self.layout_ids.get(index).copied();
            let target = self
                .session
                .read(cx)
                .items
                .get(index)
                .filter(|item| Some(item.root_id.0) == laid_out && !item.mime.starts_with("video/"))
                .map(|item| (item.root_id, item.content_hash.clone()));
            let hash = target.as_ref().map(|(_, hash)| hash.clone());
            let session = self.session.clone();
            // The file's path from the library, off the drawing thread.
            let resolve = move |cx: &mut Context<Self>| {
                let path = target.map(|(id, wanted)| {
                    session.read(cx).run(move |library| {
                        let details = library.details(id).ok()?;
                        // A collection shows the media its tile shows.
                        details
                            .media
                            .iter()
                            .find(|media| media.facts.content_hash == wanted)
                            .or(details.media.first())
                            .map(|media| PathBuf::from(&media.file_path))
                    })
                });
                async move { path?.await.ok().flatten() }
            };
            self.preview
                .rest_on(hash, resolve, |this: &mut Self| &mut this.preview, cx);
        } else {
            self.preview.away(|this: &mut Self| &mut this.preview, cx);
        }
    }

    /// The pointer left the grid.
    fn pointer_left(&mut self, cx: &mut Context<Self>) {
        self.pointer = None;
        if self.marquee.is_none() && self.hovered.take().is_some() {
            cx.notify();
        }
        self.preview.left(|this: &mut Self| &mut this.preview, cx);
    }

    /// A left press on empty space starts a rubber band. Shift, or the
    /// platform's selection key, held keeps what is selected; a secondary
    /// click (Control on macOS) is a right-click.
    fn pressed(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.swallow_click = false;
        self.swallow_tile_click = false;
        let (x, y) = self.local(event.position);
        if x < 0. || x >= self.grid_width() {
            return;
        }
        let press = selection::Press::here(event.modifiers);
        if press == selection::Press::Menu {
            self.empty_menu(event.position, window, cx);
            return;
        }
        // A press on an image may become a drag of the items.
        if let Some((index, _, _)) = self.image_at(event.position) {
            self.press = Some((self.layout_start + index, event.position));
            cx.notify();
            return;
        }
        let keep = press.keeps();
        // The band works in the window's own layout.
        let base = if keep {
            let session = self.session.read(cx);
            (0..self.layout.tiles.len())
                .filter(|index| session.is_selected(self.layout_start + index))
                .collect()
        } else {
            Default::default()
        };
        self.marquee_keep = keep;
        self.marquee = Some(marquee::Marquee::new(
            (x, y + self.scroll_top() - self.placement.top),
            (x, y),
            base,
        ));
        cx.notify();
    }

    /// `media.resolve_target_paths` for the dragged items, loaded or not,
    /// so the files are ready if the drag leaves the window.
    fn resolve_drag_files(
        &mut self,
        items: &drag::ItemDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.session.read(cx).target_for(items.roots.clone());
        let Ok(target) = serde_json::to_value(&target) else {
            return;
        };
        self.drag_files = Some(None);
        self.drag_out_waiting = false;
        let request = self.drag_request.next();
        let answer = self.session.read(cx).dispatch(
            "media.resolve_target_paths",
            serde_json::json!({ "target": target }),
        );
        cx.spawn_in(window, async move |this, cx| {
            let resolved = answer.await;
            let _ = this.update_in(cx, |this, window, cx| {
                // Only for the drag going on now.
                if !this.drag_request.is(request) {
                    return;
                }
                let files = match resolved {
                    Ok(Ok(found)) => drag_out_paths(&found),
                    Ok(Err(error)) => {
                        eprintln!("[drag] native drag failed: {error}");
                        Vec::new()
                    }
                    Err(_) => Vec::new(),
                };
                if this.drag_files == Some(None) {
                    this.drag_files = Some(Some(files));
                    if this.drag_out_waiting && this.dragging {
                        this.hand_out(window, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// The drag left the window: its files go to the system as a drag of
    /// its own, and the grid's drag ends without a drop. Until the files are
    /// resolved it waits; a drag with no files is let go.
    fn hand_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let files = match &self.drag_files {
            Some(Some(files)) => files.clone(),
            Some(None) => {
                self.drag_out_waiting = true;
                return;
            }
            None => Vec::new(),
        };
        self.drag_out_waiting = false;
        self.reorder = None;
        self.subfolders.drop = None;
        // TODO: Windows and X11 have no native file drag in GPUI yet; there
        // the grid's drag simply ends at the window's edge.
        let payload = ExternalDragPayload::Files(FileDragPaths::new(
            files.into_iter().map(|path| (path, false)),
        ));
        let handed = !matches!(&payload, ExternalDragPayload::Files(paths) if paths.entries().is_empty())
            && window.start_platform_drag(&payload);
        if handed {
            drag::model(cx).update(cx, |drags, _| {
                drags.out_since = Some(std::time::Instant::now())
            });
        }
        // `cancelDrag`: the grid's drag is over, handed on or not.
        self.press = None;
        self.dragging = false;
        self.drag_files = None;
        for holder in ["drag-ghost-0", "drag-ghost-1", "drag-ghost-2"] {
            self.thumbnails
                .update(cx, |thumbnails, _| thumbnails.pin(holder, None));
        }
        drag::model(cx).update(cx, |drags, cx| {
            if drags.items.take().is_some() {
                cx.emit(drag::DragEvent::Cancelled);
            }
        });
        cx.notify();
    }

    /// The pointer moved while a rubber band is out, anywhere in the window.
    fn marquee_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let local = self.local(position);
        let top = self.scroll_top() - self.placement.top;
        let viewport = f32::from(self.bounds.size.height);
        if let Some(marquee) = &mut self.marquee {
            marquee.pointer = local;
            marquee.speed = marquee::auto_scroll_speed(local.1, viewport);
            marquee.track(top, &self.layout, self.text_height);
            cx.notify();
        }
    }

    /// The button came up: what the rectangle covers becomes the selection,
    /// and the click this release makes is swallowed.
    fn marquee_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.take() else {
            return;
        };
        // A band over subfolder tiles selects those folders, not items
        // (`collectHeaderMarqueeHits`); a band that keeps the selection adds
        // them to folders already selected.
        let folders = marquee
            .rect
            .map(|rect| self.subfolders_in(rect))
            .unwrap_or_default();
        if !folders.is_empty() {
            self.swallow_click = true;
            let keep = self.marquee_keep;
            self.session.update(cx, |session, cx| {
                let mut chosen = if keep && session.selection.is_empty() {
                    session.folder_selection.clone()
                } else {
                    Vec::new()
                };
                for node in folders {
                    if !chosen.contains(&node) {
                        chosen.push(node);
                    }
                }
                session.select_folders(chosen, false, cx)
            });
        } else if let Some(selection) = marquee.committed() {
            self.swallow_click = true;
            let ordinals = selection
                .into_iter()
                .map(|index| self.layout_start + index)
                .collect();
            let keep = self.marquee_keep;
            self.session
                .update(cx, |session, cx| session.set_selection(ordinals, keep, cx));
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Scrolls down by nine tenths of the visible height, as Space does when
    /// the Controls setting has it scroll (`spaceKeyAction: 'scroll'`).
    pub fn scroll_by_page(&mut self, cx: &mut Context<Self>) {
        let height = f32::from(self.bounds.size.height);
        let bottom = -f32::from(self.scroll.max_offset().y);
        let top = (self.raw_scroll() + height * 0.9).clamp(0., bottom.max(0.));
        self.scroll.set_offset(point(px(0.), px(-top)));
        cx.notify();
    }

    /// Moves the selection as the Electron grid's keyboard navigation does and
    /// scrolls the new selection into view.
    pub fn move_selection(&mut self, action: nav::Move, extend: bool, cx: &mut Context<Self>) {
        let height = f32::from(self.bounds.size.height);
        let target = self
            .session
            .read(cx)
            .move_target(action, self.grid_width(), height);
        let Some(target) = target else {
            return;
        };
        // Shift extends a range from where the selection started. The grid
        // brings the item into view once it is laid out, its window read
        // first when it is not held.
        self.session
            .update(cx, |session, cx| session.move_to(target, extend, cx));
        cx.notify();
    }

    /// The menu of the grid's empty space, at a window position.
    fn empty_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EmptyMenuRequested(position));
        window.focus(&self.focus, cx);
    }
}

/// Follows the pointer anywhere in the window until the left button comes
/// up, as pointer capture does: `moved` while it is held, `released` when it
/// comes up (or when a move shows it came up unheard). Capture runs before
/// the click the release makes, so that click can be swallowed.
fn capture_pointer(
    grid: WeakEntity<GridView>,
    moved: fn(&mut GridView, Point<Pixels>, &mut Window, &mut Context<GridView>),
    released: fn(&mut GridView, &mut Window, &mut Context<GridView>),
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let moving = grid.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let _ = moving.update(cx, |this, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        moved(this, event.position, window, cx);
                    } else {
                        released(this, window, cx);
                    }
                });
            });
            let grid = grid.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    let _ = grid.update(cx, |this, cx| released(this, window, cx));
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// What fills the grid's place.
enum Surface {
    Tiles,
    /// The first page is on its way: nothing yet.
    Loading,
    Error(String),
    Empty(empty::EmptyCopy),
}

impl Render for GridView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Grid);
        let build_started = crate::stats::Started::now();
        // A rubber band near the top or bottom edge scrolls the grid each
        // frame; the rectangle follows below.
        if let Some(speed) = self
            .marquee
            .as_ref()
            .map(|marquee| marquee.speed)
            .filter(|speed| *speed != 0.)
        {
            let current = self.raw_scroll();
            let limit = f32::from(self.scroll.max_offset().y).max(0.);
            let next = (current + speed).clamp(0., limit);
            if next != current {
                self.scroll.set_offset(point(px(0.), px(-next)));
            }
            window.request_animation_frame();
        }
        // The session moves its scope change along, hears how far this grid
        // is scrolled and where its window lies, and says where a restored
        // grid should open.
        let scrolled = self.raw_scroll();
        let epoch = self.epoch;
        let window_top = self.placement.top;
        // A press, rubber band or drag in progress holds back a refresh of
        // the results, which would move what it is on.
        let busy = self.marquee.is_some() || self.dragging || self.press.is_some();
        let restore = self.session.update(cx, |session, cx| {
            session.advance_transition(cx);
            session.pointer_busy = busy;
            if session.results_epoch == epoch {
                session.grid_scroll = scrolled;
                session.window_top = window_top;
                None
            } else {
                Some((
                    session.restore_scroll.take().unwrap_or(0.),
                    session.restore_window_top.take(),
                ))
            }
        });
        let mut restored_top = None;
        if let Some((offset, top)) = restore {
            self.scroll.set_offset(point(px(0.), px(-offset)));
            restored_top = top;
        }
        let theme = theme::theme(cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        // The scrollbar's gutter is not grid: tiles, and the subfolder
        // header above them, lay out beside it.
        let width = self.grid_width();
        self.sync_subfolders(width, window, cx);
        let viewport = f32::from(self.bounds.size.height);
        let mut top = self.scroll_top();
        // Any scroll ends the hover and closes the preview at once; the
        // hover comes back 80 ms after the last one, where the pointer is.
        let moved = top != self.last_top;
        if moved {
            self.last_top = top;
            self.hovered = None;
            // The anchor was for where the scroll was when it asked.
            self.window_anchor = None;
            self.preview.close(cx);
            self.scrolled = std::time::Instant::now();
            self.settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::motion::MAGNIFIER_RESUME)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(pointer) = this.pointer {
                        this.pointer_moved(pointer, cx);
                    }
                    cx.notify();
                });
            }));
        }

        // Everything the frame needs from the session, copied for the visible
        // tiles only, so the listeners below can borrow the context.
        let mut measured = None;
        let mut revealed = None;
        let (fade, animating, prefs, surface, windowed, mut tiles) = {
            let session = self.session.read(cx);
            let fresh = session.results_epoch != self.epoch;
            if fresh {
                self.epoch = session.results_epoch;
                self.layout_key = None;
                self.window_anchor = None;
                // Positions now mean other items.
                self.marquee = None;
                self.hovered = None;
            }
            let prefs = session.prefs.clone();
            let text_height = prefs.text_height();
            self.text_height = text_height;
            let geometry = (
                width as u32,
                session.tile as u32,
                prefs.gap as u32,
                prefs.mode,
                text_height as u32,
            );
            let key = Some((
                geometry.0,
                geometry.1,
                geometry.2,
                geometry.3,
                geometry.4,
                session.window_epoch,
            ));
            if key != self.layout_key {
                let laying_out = crate::stats::Started::now();
                let window_changed = self.layout_key.map(|key| key.5) != key.map(|key| key.5);
                if window_changed {
                    // Tiles of the window before mean other items now.
                    self.marquee = None;
                    self.hovered = None;
                    self.reorder = None;
                }
                let aspects = session
                    .items
                    .iter()
                    // `gridLayoutModel.ts`: an unknown shape lays out as 3:2.
                    .map(|item| aspect(item).unwrap_or(1.5))
                    .collect::<Vec<_>>();
                let compute = |aspects: &[f64]| {
                    Layout::compute(
                        aspects,
                        width,
                        session.tile,
                        prefs.gap,
                        prefs.mode,
                        text_height,
                    )
                };
                let next = compute(&aspects);
                // The height per item, measured on the first items of the
                // window the destination opened with (or of the one held
                // when the geometry changed), as `densityRef` keeps it.
                let density = match session.density {
                    Some((measured_for, density)) if measured_for == geometry => density,
                    _ => {
                        let count = aspects.len().min(window::SAMPLE);
                        let height = if count == aspects.len() {
                            next.total_height
                        } else {
                            compute(&aspects[..count]).total_height
                        };
                        let density = window::density(height, count).unwrap_or(1.);
                        if count > 0 {
                            measured = Some((geometry, density));
                        }
                        density
                    }
                };
                let anchor = self.window_anchor.filter(|_| window_changed);
                let mut placement = window::place(
                    next.total_height,
                    session.start,
                    session.items.len(),
                    session.total as usize,
                    density,
                    anchor,
                    |offset| next.tiles.get(offset).map(|tile| tile.y),
                );
                // A restored window lies where it lay when it was left.
                if let Some(restored) = restored_top {
                    let span = (placement.height - next.total_height).max(0.);
                    placement.top = restored.clamp(0., span);
                    placement.bottom = placement.top + next.total_height;
                }
                let previous = std::mem::replace(&mut self.layout, next);
                let previous_top = std::mem::replace(&mut self.placement, placement).top;
                // The same results laid out anew (zoom, spacing, layout mode,
                // labels, width), another window of them, or the window read
                // again in place: the topmost visible tile, followed by its
                // item, stays where it was on screen, as `CanvasGrid` keeps it.
                let ids = session
                    .items
                    .iter()
                    .map(|item| item.root_id.0)
                    .collect::<Vec<_>>();
                if self.layout_key.is_some() && !fresh {
                    let now = ids
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (*id, index))
                        .collect::<std::collections::HashMap<_, _>>();
                    let shift = |tiles: &[layout::Tile], by: f32| {
                        tiles
                            .iter()
                            .map(|tile| layout::Tile {
                                y: tile.y + by,
                                ..*tile
                            })
                            .collect::<Vec<_>>()
                    };
                    // In the content's own scroll, below the subfolder
                    // header, so a header in view stays where it is.
                    let header = self.subfolders.height();
                    let before = shift(&previous.tiles, previous_top + header);
                    let after = shift(&self.layout.tiles, self.placement.top + header);
                    let selected = self
                        .layout_ids
                        .iter()
                        .enumerate()
                        .filter(|(index, id)| {
                            session
                                .selection
                                .contains(picto_library::RootId(**id), self.layout_start + index)
                        })
                        .map(|(index, _)| index)
                        .collect();
                    let moved = |index: usize| {
                        self.layout_ids
                            .get(index)
                            .and_then(|id| now.get(id).copied())
                    };
                    let raw = top + header;
                    if let Some(anchored) =
                        layout::scroll_anchor(&before, &after, &selected, raw, viewport, moved)
                        && anchored != raw
                    {
                        top = anchored - header;
                        self.last_top = top;
                        self.set_scroll_top(top);
                    }
                }
                self.layout_ids = ids;
                self.layout_start = session.start;
                self.layout_key = key;
                crate::stats::grid_part(crate::stats::GridPart::Layout, laying_out);
            }
            // An item moved to (a key, the viewer) comes into view once its
            // window is laid out.
            if let Some(ordinal) = session.reveal
                && let Some(tile) = ordinal
                    .checked_sub(session.start)
                    .and_then(|index| self.placed(index))
            {
                if let Some(offset) = nav::scroll_into_view(&tile, top, viewport, prefs.gap) {
                    top = offset.max(-self.subfolders.height());
                    self.last_top = top;
                    self.set_scroll_top(top);
                }
                revealed = Some(ordinal);
            }
            if let Some(marquee) = &mut self.marquee {
                marquee.track(top - self.placement.top, &self.layout, text_height);
            }
            // `GridScreen.tsx`: an error first, then the tiles, and only a
            // scope known to be empty says so; a loading one shows nothing.
            let surface = if let Some(error) = &session.error {
                Surface::Error(error.clone())
            } else if !session.items.is_empty()
                || (!self.subfolders.children.is_empty() && !session.loading)
            {
                Surface::Tiles
            } else if session.loading && !session.refreshing {
                Surface::Loading
            } else {
                let scope = session
                    .nav
                    .get(session.displayed)
                    .map_or(ItemScope::All, |entry| entry.scope.clone());
                Surface::Empty(empty::copy(
                    &scope,
                    session.searching(),
                    session.filter.is_some(),
                ))
            };
            // Only the window's tiles are laid out; the rest of the height
            // is the destination's other items, drawn once their window
            // arrives.
            let local_top = top - self.placement.top;
            let marquee_base = self.marquee.as_ref().map(|_| {
                (0..self.layout.tiles.len())
                    .filter(|index| session.is_selected(session.start + index))
                    .collect::<std::collections::BTreeSet<_>>()
            });
            let tiles = self
                .layout
                .visible(
                    local_top - reveal::MARGIN,
                    local_top + viewport + reveal::MARGIN,
                )
                .into_iter()
                .filter_map(|index| {
                    let item = session.items.get(index)?;
                    let ordinal = session.start + index;
                    let selected = match (&self.marquee, &marquee_base) {
                        (Some(marquee), Some(base)) => marquee.shows_selected(index, base),
                        _ => session.selection.contains(item.root_id, ordinal),
                    };
                    Some(TileData {
                        index,
                        ordinal,
                        look: TileLook {
                            tile: self.placed(index)?,
                            item: item.clone(),
                            thumbnail: session.thumbnail(&item.content_hash),
                            selected,
                            thumb: Thumb::Pending,
                            original: None,
                            // Set as the tile is drawn.
                            alphas: (0., 0.),
                            magnifier: false,
                            grayscale: false,
                        },
                    })
                })
                .collect::<Vec<_>>();
            (
                session.transition.opacity(),
                session.transition.animating(),
                prefs,
                surface,
                (session.total as usize) > session.items.len() && !session.loading,
                tiles,
            )
        };
        if measured.is_some() || revealed.is_some() {
            self.session.update(cx, |session, _| {
                if measured.is_some() {
                    session.density = measured;
                }
                if revealed.is_some() && session.reveal == revealed {
                    session.reveal = None;
                }
            });
        }
        // `CanvasGrid`'s window requests: after a scroll, and whenever the
        // viewport's middle has no tile of the window (a restored or
        // revealed scroll position, a thumb dragged far away).
        if windowed
            && (moved || window::misses(self.placement, self.layout.tiles.len(), top, viewport))
        {
            self.schedule_window(cx);
        }

        let tile_count = tiles.len();
        // Thumbnails load from the middle of the viewport outward.
        let spans = tiles
            .iter()
            .map(|data| (data.look.tile.y, data.look.tile.h))
            .collect::<Vec<_>>();
        // The subfolder covers first while the header is in view.
        let header_wanted = if self.raw_scroll() < self.subfolders.height() {
            self.subfolders.wanted()
        } else {
            Vec::new()
        };
        let wanted = header_wanted
            .into_iter()
            .chain(
                thumbnails::load_order(&spans, top, viewport)
                    .into_iter()
                    .map(|at| Resource::Path(tiles[at].look.thumbnail.clone().into())),
            )
            .collect();
        self.thumbnails
            .update(cx, |thumbnails, cx| thumbnails.plan(wanted, window, cx));
        let mut missing = Vec::new();
        let decoded = {
            let thumbnails = self.thumbnails.read(cx);
            tiles
                .iter_mut()
                .map(|data| {
                    let resource = Resource::Path(data.look.thumbnail.clone().into());
                    data.look.thumb = thumbnails.thumb(&resource, &data.look.item.mime);
                    if data.look.thumb == Thumb::Failed {
                        missing.push(thumbnails::Missing {
                            hash: data.look.item.content_hash.clone(),
                            mime: data.look.item.mime.clone(),
                            jpeg: data.look.thumbnail.clone(),
                        });
                    }
                    (data.look.item.root_id.0, data.look.thumb == Thumb::Loaded)
                })
                .collect::<Vec<_>>()
        };
        let session = self.session.clone();
        self.thumbnails.update(cx, |thumbnails, cx| {
            thumbnails.make_missing(&session, missing, cx)
        });
        // Tiles in the viewport that draw more pixels than their thumbnail
        // holds ask for their original, which arrives after a short dwell.
        let scale = window.scale_factor();
        let wants = {
            let session = self.session.read(cx);
            tiles
                .iter()
                .filter(|data| {
                    data.look.thumb == Thumb::Loaded
                        && data.look.tile.y + data.look.tile.h > top
                        && data.look.tile.y < top + viewport
                })
                .filter_map(|data| {
                    let (contain, (_, _, w, h)) =
                        media_rect(&data.look.tile, &data.look.item, &prefs);
                    let source = data.look.item.width.zip(data.look.item.height);
                    originals::wants_original(&data.look.item.mime, (w, h), source, contain, scale)
                        .then(|| originals::Want {
                            hash: data.look.item.content_hash.clone(),
                            path: session
                                .original(&data.look.item.content_hash, &data.look.item.mime),
                            size: originals::decode_size((w, h), source, contain, scale),
                        })
                })
                .collect::<Vec<_>>()
        };
        self.originals
            .update(cx, |originals, cx| originals.plan(wants, cx));
        {
            let originals = self.originals.read(cx);
            for data in &mut tiles {
                data.look.original = originals.get(&data.look.item.content_hash);
            }
        }
        self.reveal.update(&decoded, now);
        // Keep painting while a fade or a thumbnail reveal is in motion.
        if animating || self.reveal.active(now) {
            window.request_animation_frame();
        }

        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.bounds != bounds {
                        this.bounds = bounds;
                        cx.notify();
                        // A notification made while a frame is being drawn
                        // does not by itself bring another frame.
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        // Only the tiles scroll: an empty or failed grid has no scrollbar,
        // whatever the last results left in the scroll handle.
        let scrolls = matches!(surface, Surface::Tiles);
        let body = match surface {
            Surface::Error(error) => div()
                .id("grid-error")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .opacity(fade)
                .text(theme::TEXT_MD)
                .text_color(theme.negative())
                .child(error)
                .child(
                    div()
                        .id("grid-retry")
                        .px(px(12.))
                        .py(px(4.))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme.border())
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_secondary())
                        .cursor_pointer()
                        .hover(move |style| {
                            style.text_color(theme.text()).bg(theme.surface_hover())
                        })
                        .child("Retry")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.update(cx, |session, cx| session.reload(cx));
                        })),
                ),
            Surface::Loading => div().id("grid-loading").size_full(),
            Surface::Empty(copy) => {
                let actions = if copy.import {
                    // `chooseAndImportFiles` / `chooseAndImportFolder` into
                    // this destination, held while an import runs.
                    let destination = {
                        let session = self.session.read(cx);
                        session
                            .nav
                            .get(session.displayed)
                            .map(|entry| crate::imports::plan::Destination::for_scope(&entry.scope))
                            .unwrap_or_default()
                    };
                    let blocked = crate::imports::blocked(cx);
                    let start = move |folder: bool| {
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if let Some(imports) = crate::imports::get(cx) {
                                imports.update(cx, |imports, cx| {
                                    if folder {
                                        imports.choose_folder(destination, window, cx)
                                    } else {
                                        imports.choose_files(destination, window, cx)
                                    }
                                });
                            }
                        }
                    };
                    vec![
                        ui::empty_state_action("grid-import-files", "upload", "Import Files")
                            .disabled(blocked.is_some())
                            .on_click(start(false))
                            .into_any_element(),
                        ui::empty_state_action(
                            "grid-import-folder",
                            "folder-plus",
                            "Import Folder",
                        )
                        .disabled(blocked.is_some())
                        .on_click(start(true))
                        .into_any_element(),
                    ]
                } else {
                    Vec::new()
                };
                div()
                    .id("grid-empty")
                    .size_full()
                    .opacity(fade)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, event: &MouseDownEvent, _, cx| {
                            cx.emit(EmptyMenuRequested(event.position))
                        }),
                    )
                    .child(ui::empty_state(
                        "empty-photo",
                        copy.title,
                        copy.description,
                        actions,
                        cx,
                    ))
            }
            Surface::Tiles => {
                // The whole destination's height, so the scrollbar stands
                // for every item, loaded or not.
                // The subfolder header, then the whole destination's height
                // below it.
                let header = self.subfolders.height();
                let mut content = div()
                    .absolute()
                    .left_0()
                    .top(px(header))
                    .w_full()
                    .h(px(self.placement.height));
                let building = crate::stats::Started::now();
                for data in tiles {
                    content = content.child(self.tile(data, &prefs, now, window, cx));
                }
                crate::stats::grid_part(crate::stats::GridPart::Tiles, building);
                if let Some(marker) = self.reorder_marker(&prefs) {
                    content = content.child(marker);
                }
                // `.marquee`: square corners, over the tiles.
                if let Some(rect) = self.marquee.as_ref().and_then(|marquee| marquee.rect) {
                    content = content.child(
                        div()
                            .absolute()
                            .left(px(rect.left))
                            .top(px(rect.top + self.placement.top))
                            .w(px(rect.width))
                            .h(px(rect.height))
                            .bg(rgba(theme::MARQUEE_FILL))
                            .border_1()
                            .border_color(rgba(theme::MARQUEE_BORDER)),
                    );
                }
                div()
                    .id("media-grid")
                    .size_full()
                    .opacity(fade)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // The frame that follows a scroll notices the new position.
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.pointer_moved(event.position, cx)
                    }))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.pointer_left(cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.pressed(event, window, cx)
                        }),
                    )
                    // A right-click no image took is on empty space.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.empty_menu(event.position, window, cx)
                        }),
                    )
                    // A click that no image took is a click on empty space,
                    // unless it ends a rubber band or was a secondary click.
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        if std::mem::take(&mut this.swallow_click)
                            || selection::Press::here(event.modifiers()) == selection::Press::Menu
                        {
                            return;
                        }
                        this.session
                            .update(cx, |session, cx| session.clear_selection(cx));
                        window.focus(&this.focus, cx);
                    }))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(header + self.placement.height))
                            .when(header > 0., |all| {
                                all.child(self.subfolder_header(window, cx))
                            })
                            .child(content),
                    )
            }
        };
        crate::stats::grid_built(build_started, tile_count);
        let mut root = div()
            .relative()
            .size_full()
            .child(measure)
            .child(body)
            .when(scrolls, |root| {
                root.child(ui::scrollbar("grid-scrollbar", &self.scroll, window, cx))
            })
            .children(ui::scroll_to_top(
                "grid-return-to-top",
                &self.scroll,
                RETURN_TO_TOP_AFTER,
                12.,
                window,
                cx,
            ));
        // While a rubber band is out, or after a press on a tile (which can
        // become a drag), the pointer is followed and the release heard
        // anywhere in the window, as pointer capture does.
        if self.marquee.is_some() {
            root = root.child(capture_pointer(
                cx.entity().downgrade(),
                |this, position, _, cx| this.marquee_moved(position, cx),
                |this, window, cx| this.marquee_released(window, cx),
            ));
        }
        if self.press.is_some() || self.dragging {
            root = root.child(capture_pointer(
                cx.entity().downgrade(),
                |this, position, window, cx| this.drag_moved(position, window, cx),
                |this, _, cx| this.drag_released(cx),
            ));
        }
        // Settings › Controls "Mouse wheel: Zoom" (`onGridWheel`): the wheel
        // over the tiles changes the tile size instead of scrolling. The
        // setting is read when the wheel turns, so a change applies at once.
        if scrolls {
            let entity = cx.entity().downgrade();
            root = root.child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |_, hitbox, window, _| {
                        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture
                                || !hitbox.is_hovered_at(event.position, window)
                                || crate::settings::current(cx).grid_wheel_action != "zoom"
                            {
                                return;
                            }
                            // The browser's `deltaY` is positive scrolling
                            // down; GPUI's is the other way round.
                            let delta_y =
                                -f32::from(event.delta.pixel_delta(window.line_height()).y);
                            cx.stop_propagation();
                            let _ = entity.update(cx, |this, cx| {
                                this.session.update(cx, |session, cx| {
                                    if let Some(size) = nav::wheel_zoom(session.tile, delta_y) {
                                        session.set_tile(size, cx);
                                    }
                                })
                            });
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
        if self.dragging
            && let Some(items) = drag::model(cx).read(cx).items.clone()
        {
            root = root.child(drag::ghost(&items, &self.thumbnails, cx));
        }
        if let Some(shown) = &self.preview.shown {
            let (overlay, fading) = preview::overlay(shown, window);
            if fading {
                window.request_animation_frame();
            }
            root = root.children(overlay);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::{Latest, drag_out_paths};
    use std::path::PathBuf;

    #[test]
    fn a_reply_is_stale_once_anything_was_asked_since() {
        let mut latest = Latest::default();
        let first = latest.next();
        assert!(latest.is(first));
        let second = latest.next();
        assert!(!latest.is(first));
        assert!(latest.is(second));
    }

    #[test]
    fn dragged_out_files_are_each_file_once() {
        let found = serde_json::json!([
            { "file_hash": "a", "path": "/library/a.jpg" },
            { "file_hash": "b", "path": "/library/b.png" },
            { "file_hash": "a", "path": "/library/elsewhere/a.jpg" },
        ]);
        assert_eq!(
            drag_out_paths(&found),
            [
                PathBuf::from("/library/a.jpg"),
                PathBuf::from("/library/b.png")
            ]
        );
        assert!(drag_out_paths(&serde_json::json!(null)).is_empty());
    }
}
