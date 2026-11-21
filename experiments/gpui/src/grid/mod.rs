//! The grid: the scrolling area in the centre of the window that shows the
//! items of the open destination as tiles, with the subfolder header above
//! them.
//!
//! The grid is a GPUI view of its own (`GridView`). When the grid scrolls,
//! when a thumbnail fades in, or when the results fade in or out, GPUI builds
//! the grid and the shell view that contains it again. It reuses what it drew
//! before for the sidebar, the toolbar and the inspector.
//!
//! This file holds the view's state, its handlers for the pointer, the
//! rubber band selection, the handing of dragged files to the operating
//! system, the zoom with the mouse wheel, and `GridView::build`, which builds
//! one frame of the grid for the size the grid has in that frame. `frame.rs`
//! computes what a frame draws, and `body.rs` builds the elements.
pub mod art;
mod body;
pub mod drag;
pub mod empty;
pub mod filter_bar;
pub mod filter_editors;
pub mod filters;
pub mod format;
mod frame;
pub mod layout;
pub mod marquee;
pub mod menu;
pub mod nav;
pub mod originals;
pub mod pointer;
pub mod prefs;
pub mod preview;
pub mod rendered;
pub mod reveal;
pub mod selection;
pub mod subfolders;
pub mod thumbnails;
pub mod tile;
pub mod window;
mod zoom_animation;

use crate::session::{Change, Session};
use crate::ui;
use gpui::prelude::FluentBuilder;
use gpui::*;
use layout::{Layout, ViewMode};
use std::path::PathBuf;
use thumbnails::Thumbnails;

/// The event the grid emits when a tile's image is right-clicked, or
/// Control-clicked on macOS. The value is the pointer's position in window
/// coordinates. The shell opens the tile's context menu at that position.
pub struct MenuRequested(pub Point<Pixels>);

/// The event the grid emits when empty space in the grid is right-clicked,
/// or Control-clicked on macOS. Empty space is the space between tiles and
/// the text rows under a tile's image. The event is also emitted for a
/// right-click anywhere on the grid's empty state. The value is the
/// pointer's position in window coordinates. The shell opens the grid's own
/// context menu at that position.
pub struct EmptyMenuRequested(pub Point<Pixels>);

/// The event the grid emits when a tile's image is double-clicked or
/// middle-clicked. A double-click has selected the tile before the event is
/// emitted. A middle-click does not change the selection. The shell does
/// what the Controls settings say for a double-click or a middle-click.
/// `ordinal` is the item's position in the destination's order, and
/// `middle` is true for a middle-click.
pub struct Activated {
    pub ordinal: usize,
    pub middle: bool,
}

pub struct GridView {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    originals: Entity<originals::Originals>,
    /// The focus handle of the shell's root element, which the shell passes
    /// to `GridView::new`. The shell's keyboard shortcuts work while it has
    /// the keyboard focus. The grid gives it the keyboard focus after a
    /// click on a tile or on empty space, when a rubber band selection
    /// ends, and when it asks the shell for a context menu.
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// The positions and sizes of the tiles of the window of results, which
    /// is the items the session holds (at most 1,500 of the destination's
    /// items). The tiles are laid out without the rest of the destination:
    /// `y` = 0 is the top of the window's first row.
    layout: Layout,
    /// Where `layout` lies in the scrollable height that stands for every
    /// item of the destination: that height, and the positions in it where
    /// the window's tiles begin and end. A tile is drawn at its `y` in
    /// `layout` plus `placement.top`.
    placement: window::Placement,
    /// What the layout was last computed for. The layout is computed again
    /// when this changes.
    layout_key: Option<LayoutKey>,
    /// The zoom animation in progress, if the zoom changed less than 150 ms
    /// ago.
    zoom_animation: Option<zoom_animation::ZoomAnimation>,
    /// The tile the last zoom step kept at the same height on screen.
    zoom_anchor: Option<zoom_animation::Anchor>,
    /// The height of the subfolder header in the last frame that was drawn.
    header_height_drawn: f32,
    /// The positions of the subfolder header's tiles in the last frame that
    /// was drawn.
    header_geometry_drawn: subfolders::Geometry,
    /// With the Controls setting "Mouse wheel: Adjust thumbnail size", the
    /// distance in pixels a trackpad has scrolled since the last zoom step
    /// it caused. Positive is scrolling down.
    wheel_distance: f32,
    /// Set when the grid asks the session for another window of results
    /// (`ask_for_window`). It holds the ordinal that was asked for, and the
    /// position of the middle of the viewport at that moment, in pixels
    /// from the top of the items' scrollable height. When the window
    /// arrives, it is placed so that the top of that item's tile is at that
    /// position (production's `windowAnchorRef`). It is cleared when the
    /// grid scrolls after the request, because the position was measured
    /// for the scroll position at the time of the request. It is also
    /// cleared when the results become a different set of items.
    window_anchor: Option<(usize, f32)>,
    /// The timer that `schedule_window` starts. When it ends, after
    /// `window::REQUEST_INTERVAL` (40 ms), the grid asks the session for
    /// the window of results the viewport needs at that moment. While the
    /// timer runs, `schedule_window` does not start another one, so the
    /// grid asks at most once every 40 ms however often it scrolls
    /// (production's `CoalescedGridRequest`).
    window_timer: Option<Task<()>>,
    reveal: reveal::RevealTracker,
    /// The moment this grid was created. The grid measures time in
    /// milliseconds since this moment (`now` in `build`). The fade-in of
    /// thumbnails (`reveal`) and the zoom animation use that time.
    clock: std::time::Instant,
    /// The grid's position and size in window coordinates in the frame
    /// that was built last. `build` sets it at its start, from the bounds
    /// GPUI laid the grid out with in that frame. The handlers of pointer
    /// and key events read it between frames.
    bounds: Bounds<Pixels>,
    /// The value the session's `results_epoch` had in the last frame the
    /// grid built. When the session's value is different, the results are a
    /// different set of items. The grid then computes its layout from the
    /// start and takes its scroll position from the session.
    epoch: u64,
    /// The index in `layout` of the tile whose image the pointer is over,
    /// or `None` when the pointer is over no image. The magnifier icon is
    /// drawn on that tile, unless its item is a video, or the grid scrolled
    /// or its zoom changed less than 80 ms ago.
    hovered: Option<usize>,
    /// When the grid last scrolled or its zoom last changed. The magnifier
    /// icon is not drawn on the hovered tile until 80 ms
    /// (`motion::MAGNIFIER_RESUME`) have passed since then.
    scrolled: std::time::Instant,
    /// The timer that finds the tile under the pointer again after the grid
    /// has scrolled (after 80 ms) or its zoom has changed (after 150 ms).
    /// Both clear `hovered` when they happen. Starting a new timer cancels
    /// the one before it.
    settle: Option<Task<()>>,
    /// How far the items were scrolled, in pixels, in the last frame that
    /// was drawn. `follow_scroll` compares it with the current scroll
    /// position to find out whether the grid scrolled.
    last_top: f32,
    /// The pointer's last position over the grid, in window coordinates. It
    /// is `None` after the pointer has left the grid.
    pointer: Option<Point<Pixels>>,
    /// The height in pixels of the text rows under each image: the name
    /// row and the resolution row, for each of the two that the view
    /// preferences show. It is copied from the session's view preferences
    /// each time the grid is built.
    text_height: f32,
    /// The rubber band selection in progress, from the press on empty space
    /// until the left button is released. It is `None` at other times.
    marquee: Option<marquee::Marquee>,
    /// Set when a rubber band selection ends. The mouse release that ends
    /// it also produces a click event, and this flag makes the grid ignore
    /// that click so it does not clear the selection.
    swallow_click: bool,
    /// Set from a left press on a tile's image until the button is released
    /// or the drag leaves the window. It holds the ordinal of the tile's
    /// item and the position of the press in window coordinates. When the
    /// pointer has moved 5 px (`drag::THRESHOLD`) from that position with
    /// the button held, a drag of items starts.
    press: Option<(usize, Point<Pixels>)>,
    /// True when the rubber band selection in progress adds to the
    /// selection instead of replacing it. That is the case when Shift,
    /// Command on macOS, or Control on Windows and Linux was held at the
    /// press that started it.
    marquee_keep: bool,
    /// True while items are being dragged: from the moment the pointer has
    /// moved 5 px from the press on a tile, until the button is released or
    /// the drag leaves the window.
    dragging: bool,
    /// Set when a drag of items ends with the release of the button. The
    /// release can also produce a click event on a tile, and this flag makes
    /// the tile ignore that click so it does not change the selection.
    swallow_tile_click: bool,
    preview: preview::HoverPreview,
    /// The two pictures that are drawn in place of a thumbnail: the torn
    /// page for a thumbnail that failed to load, and the lettered card for
    /// a font file. They are built when first needed, for the theme's
    /// placeholder colour, and built again when that colour changes.
    art: Option<art::Art>,
    /// The root id of the item that each tile in `layout` was laid out for,
    /// in the order of `layout.tiles`.
    layout_ids: Vec<u32>,
    /// The ordinal of the item of the first tile in `layout`, which is its
    /// position in the destination's order. The ordinal of any tile's item
    /// is `layout_start` plus the tile's index in `layout`.
    layout_start: usize,
    /// The subfolder header: the subfolder tiles drawn above the items when
    /// the open destination is a folder that has subfolders.
    subfolders: subfolders::Header,
    /// Set while items are dragged over the grid of a folder whose items
    /// can be reordered by dragging. It is the place where a drop would
    /// put the dragged items (production's `reorderDropRef`): the index in
    /// `layout` of a tile, and the side of that tile they would go to. The
    /// reorder marker is drawn there.
    reorder: Option<(usize, drag::Side)>,
    /// The paths of the files of the dragged items. The grid gives them to
    /// the operating system if the drag leaves the window (production's
    /// `resolveNativeDragHashes`). The outer `Option` is `None` when no
    /// drag is in progress. The inner `Option` is `None` from the start of
    /// a drag until the session has answered with the paths.
    drag_files: Option<Option<Vec<PathBuf>>>,
    /// True when the drag left the window before the paths in `drag_files`
    /// arrived. When they arrive, `resolve_drag_files` calls `hand_out`,
    /// which gives the files to the operating system (production's
    /// `nativeDragRequestRef.pending`). It is set back to false if the
    /// pointer comes back into the window before the paths arrive.
    drag_out_waiting: bool,
    /// Numbers the requests for the files of dragged items. Each drag makes
    /// one request. When an answer arrives, the grid uses it only if it is
    /// the answer to the latest request, so the answer for an earlier drag
    /// is ignored.
    drag_request: Latest,
    _session: Subscription,
    _thumbnails: Subscription,
}

/// The "Return to Top" button is shown when the grid is scrolled at least
/// this many pixels from its top.
const RETURN_TO_TOP_AFTER: f32 = 200.;
/// With the Controls setting "Mouse wheel: Adjust thumbnail size", the
/// distance in pixels a trackpad scrolls for one step of the zoom.
const WHEEL_STEP: f32 = 60.;

/// What a layout is computed for, in this order: the grid's width in whole
/// pixels, the zoom's tile size (the bits of the `f32`, so two sizes are
/// equal only when they are exactly the same), the spacing in pixels, the
/// layout mode, the height of the text rows under each picture in pixels,
/// and the session's `window_epoch`, which changes when the window of
/// results holds other items.
type LayoutKey = (u32, u32, u32, ViewMode, u32, u64);

/// Returns the paths from an answer of [`Session::resolve_files`], which is
/// a list of pairs of a content hash and a path. When the same content hash
/// is in the list more than once, only the first path for it is returned, as
/// in production's `resolveNativeDragHashes`. The paths are returned in the
/// order of the list.
fn drag_out_paths(found: Vec<(String, PathBuf)>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .filter(|(hash, _)| seen.insert(hash.clone()))
        .map(|(_, path)| path)
        .collect()
}

/// A counter that numbers the requests of one kind, for example the requests
/// for the files of dragged items. The code that makes a request keeps the
/// number that `next` returns. When the answer arrives, it calls `is` with
/// that number to find out whether another request has been made since. If
/// one has, the answer is out of date and the code ignores it.
#[derive(Default)]
pub(crate) struct Latest(u64);

impl Latest {
    /// Counts a new request and returns its number. From now on, `is`
    /// returns false for the number of every earlier request.
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }

    /// Returns true when `request` is the number of the latest request, so
    /// the answer to it is not out of date.
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
            // The session emits `Change::Navigation` when, among other
            // things, the list of destinations or the open destination
            // has changed. The subfolder header then computes its list of
            // subfolders again (`relist`) and reads the subfolders' covers
            // from the library again (`stale`).
            if matches!(change, Change::Navigation) {
                this.subfolders.stale = true;
                this.subfolders.relist = true;
            }
            // `Change::GridWidth` is emitted by this grid's own `build`,
            // which has already drawn with the new width.
            if !matches!(change, Change::Viewer | Change::GridWidth) {
                cx.notify();
            }
        });
        // When the application reports that a thumbnail file was written
        // again (`Event::Thumbnail`), remove that thumbnail from the cache
        // and draw the grid again, so that the new file is read.
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
        // Draw the grid again when `originals` or `thumbnails` notifies.
        // `originals` notifies when the decoding of an original has ended,
        // and when a tile has been in the viewport long enough for its
        // original to be decoded. `thumbnails` notifies when an attempt to
        // make a missing thumbnail has ended.
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
            zoom_animation: None,
            zoom_anchor: None,
            header_height_drawn: 0.,
            header_geometry_drawn: Default::default(),
            wheel_distance: 0.,
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

    /// The width in pixels that the tiles are laid out in: the grid's width
    /// less the 8 px at its right edge that are kept free for the scrollbar
    /// (`ui::scrollbar::GUTTER`).
    fn grid_width(&self) -> f32 {
        (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.)
    }

    /// How far the grid's content is scrolled, in pixels. The content is
    /// the subfolder header with the items below it, so 0 means that the
    /// top of the header is at the top of the grid.
    fn raw_scroll(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    /// How far the items are scrolled, in pixels: `raw_scroll` less the
    /// height of the subfolder header above the items. 0 means that the top
    /// of the items is at the top of the grid. The value is negative while
    /// the top of the items is below the top of the grid, which is while
    /// part of the header is in view (production's `visibleScrollTop` and
    /// `toLayoutCoords`).
    fn scroll_top(&self) -> f32 {
        self.raw_scroll() - self.subfolders.height()
    }

    /// Scrolls the grid so that the items are scrolled `top` pixels, which
    /// is the value `scroll_top` then returns. The grid is not scrolled
    /// above the top of the subfolder header.
    fn set_scroll_top(&mut self, top: f32) {
        let raw = (top + self.subfolders.height()).max(0.);
        self.scroll.set_offset(point(px(0.), px(-raw)));
    }

    /// Converts a position in window coordinates to (x, y) in pixels from
    /// the grid's top-left corner.
    fn local(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.bounds.origin.x),
            f32::from(position.y - self.bounds.origin.y),
        )
    }

    /// Finds the tile whose image is under `position`, which is in window
    /// coordinates. Returns the tile's index in `layout`, and the position
    /// in the coordinates of `layout`: x from the grid's left edge and y
    /// from the top of the window's first row. Returns `None` when the
    /// position is over no image. The text rows under an image and the
    /// scrollbar's gutter are not part of an image.
    fn image_at(&self, position: Point<Pixels>) -> Option<(usize, f32, f32)> {
        let (x, y) = self.local(position);
        let y = y + self.scroll_top() - self.placement.top;
        if x < 0. || x >= self.grid_width() {
            return None;
        }
        marquee::image_at(&self.layout, x, y, self.text_height).map(|index| (index, x, y))
    }

    /// Returns the tile at `index` of `layout` with `placement.top` added
    /// to its `y`. Its `y` is then measured from the top of the scrollable
    /// height that stands for every item of the destination, which is where
    /// the tile is drawn.
    fn placed(&self, index: usize) -> Option<layout::Tile> {
        let tile = self.layout.tiles.get(index)?;
        Some(layout::Tile {
            y: tile.y + self.placement.top,
            ..*tile
        })
    }

    /// Starts a timer of `window::REQUEST_INTERVAL` (40 ms), unless one is
    /// already running. When the timer ends, `ask_for_window` asks the
    /// session for the window of results that the viewport needs at that
    /// moment. `build` calls this after a scroll, after a change of zoom,
    /// and while the middle of the viewport is outside the window's tiles.
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

    /// Works out which window of results the viewport needs at the current
    /// scroll position (`window::destination`, production's
    /// `gridWindowDestination`) and asks the session for it. It asks for
    /// nothing when the session holds every item of the destination, or
    /// when the middle of the viewport is inside the window's tiles and
    /// the viewport is at least three times its own height away from each
    /// end of the window that has more items beyond it.
    ///
    /// It also sets `window_anchor`, so that the window that arrives is
    /// placed where the viewport is.
    fn ask_for_window(&mut self, cx: &mut Context<Self>) {
        let top = self.scroll_top();
        let viewport = f32::from(self.bounds.size.height);
        let (start, total) = {
            let session = self.session.read(cx);
            // `layout` and `placement` describe the window of results the
            // grid last laid out. If the session holds another window by
            // now, they do not describe the session's items, so nothing
            // is asked for here. The grid lays out the new window when it
            // is next built.
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

    /// Called when the pointer moves over the grid, with its position in
    /// window coordinates. It is also called 80 ms after a scroll and 150 ms
    /// after a change of zoom, with the pointer's last position, to find
    /// the tile that is under the pointer then.
    ///
    /// It records the position and the tile whose image the pointer is over
    /// (`hovered`). When the pointer is on that tile's magnifier icon, it
    /// starts the timer that opens the hover preview. Otherwise it starts
    /// the timer that closes an open hover preview. During a rubber band
    /// selection it records the position and does nothing else.
    fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        if self.marquee.is_some() {
            return;
        }
        let hit = self.image_at(position);
        if pointer::hover(&mut self.hovered, hit) {
            cx.notify();
        }
        if let Some(index) = pointer::magnifier(hit, &self.layout, self.text_height) {
            // As in production's grid, the hover preview shows the item
            // that is under the magnifier when the preview's timer starts,
            // even if another item is there when the timer ends 200 ms
            // later. The item is read from the session's items at the
            // tile's index, and used only if it is the item the tile was
            // laid out for (`layout_ids`). It can be another item when the
            // session already holds a window of results that the grid has
            // not laid out yet. A video has no hover preview.
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
            // `resolve` is called when the preview's timer ends. It asks
            // the library for the path of the item's file. The library is
            // read on its own thread, not on the thread that draws.
            let resolve = move |cx: &mut Context<Self>| {
                let path = target.map(|(id, wanted)| {
                    session.read(cx).run(move |library| {
                        let details = library.details(id).ok()?;
                        // A collection has more than one media file. The
                        // preview shows the one whose content hash is the
                        // one the tile shows, or the first one when none
                        // has that hash.
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

    /// Called when the pointer leaves the grid, and when a drag of items
    /// starts. It forgets the pointer's position, cancels the timer that
    /// would open the hover preview, and starts the timer that closes an
    /// open one. It also clears `hovered`, unless a rubber band selection
    /// is in progress.
    fn pointer_left(&mut self, cx: &mut Context<Self>) {
        let banding = self.marquee.is_some();
        if pointer::left(
            &mut self.pointer,
            &mut self.hovered,
            banding,
            &mut self.preview,
            |this: &mut Self| &mut this.preview,
            cx,
        ) {
            cx.notify();
        }
    }

    /// Handles a press of the left button on the scrolling list of tiles.
    ///
    /// - A Control-press on macOS is a secondary click. It opens the grid's
    ///   own context menu, as a right-click on empty space does.
    /// - A press on a tile's image is recorded in `press`, because it
    ///   becomes a drag of items if the pointer then moves 5 px.
    /// - Any other press is on empty space and starts a rubber band
    ///   selection. With Shift, Command on macOS, or Control on Windows and
    ///   Linux held, the rubber band adds to the selection. Without them it
    ///   replaces the selection.
    ///
    /// A press over the scrollbar's gutter does none of these.
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
        // Record a press on a tile's image, with the ordinal of the tile's
        // item. `drag_moved` starts a drag of items when the pointer has
        // moved 5 px from the press.
        if let Some((index, _, _)) = self.image_at(event.position) {
            self.press = Some((self.layout_start + index, event.position));
            cx.notify();
            return;
        }
        let keep = press.keeps();
        // A rubber band selection works with the indexes and coordinates of
        // `layout`, not with ordinals. `base` is the indexes of the tiles
        // that are selected now. The rubber band keeps them selected when
        // it adds to the selection, and `base` is empty when it does not.
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

    /// Called when a drag of items starts. It asks the session for the
    /// paths of the files of all the dragged items
    /// (`Session::resolve_files`, which calls `media.resolve_target_paths`).
    /// That includes selected items that are not in the window of results
    /// the session holds. The answer is stored in `drag_files`, so that the
    /// files can be given to the operating system if the drag leaves the
    /// window.
    fn resolve_drag_files(
        &mut self,
        items: &drag::ItemDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_files = Some(None);
        self.drag_out_waiting = false;
        let request = self.drag_request.next();
        let answer = self.session.read(cx).resolve_files(&items.target);
        cx.spawn_in(window, async move |this, cx| {
            let resolved = answer.await;
            let _ = this.update_in(cx, |this, window, cx| {
                // Ignore the answer if another drag has started since this
                // request was made.
                if !this.drag_request.is(request) {
                    return;
                }
                let files = match resolved {
                    Ok(found) => drag_out_paths(found),
                    Err(error) => {
                        eprintln!("[drag] native drag failed: {error}");
                        Vec::new()
                    }
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

    /// Called when a drag of items leaves the window. It starts a drag of
    /// the items' files in the operating system, so that the files can be
    /// dropped outside the app, and ends the grid's own drag without a
    /// drop.
    ///
    /// If the paths in `drag_files` have not arrived yet, it sets
    /// `drag_out_waiting` and returns. `resolve_drag_files` calls it again
    /// when the paths arrive. If there are no files to give, no drag is
    /// started in the operating system, and the grid's own drag still ends.
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
        // `desktop::drag_out` checks that every file exists and is inside
        // the folder of the open library copy. If one is not, it shows an
        // error notification and returns no files. It returns the paths as
        // the operating system shows them, which on Windows is without the
        // `\\?\` prefix. The vendored GPUI then starts a native drag of the
        // files on every platform: with AppKit on macOS, OLE on Windows,
        // and XDND or Wayland on Linux.
        let files = if files.is_empty() {
            files
        } else {
            crate::desktop::drag_out(&files, cx)
        };
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
        // End the grid's own drag, as production's `cancelDrag` does,
        // whether or not the operating system started a drag of the files.
        // This forgets the press, releases the thumbnails that were pinned
        // for the picture of the dragged items, and tells the views that
        // accept drops that the drag was cancelled.
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

    /// Called when the pointer moves during a rubber band selection, with
    /// the pointer anywhere in the window, also outside the grid. It
    /// updates the rubber band's rectangle and the tiles the rectangle
    /// covers. It also sets how fast the grid scrolls on each frame, which
    /// is not 0 while the pointer is less than 50 px from the grid's top or
    /// bottom edge or beyond that edge.
    fn marquee_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let local = self.local(position);
        let top = self.scroll_top() - self.placement.top;
        let viewport = f32::from(self.bounds.size.height);
        if pointer::band_moved(
            self.marquee.as_mut(),
            local,
            top,
            viewport,
            &self.layout,
            self.text_height,
        ) {
            cx.notify();
        }
    }

    /// Called when the left button is released during a rubber band
    /// selection. If the rectangle covers subfolder tiles, those folders
    /// become the selection. Otherwise the items whose images the rectangle
    /// covers become the selection, or are added to it when the rubber band
    /// adds to the selection. In both cases `swallow_click` is set, so that
    /// the click event this release produces does not clear the selection.
    ///
    /// If the rectangle never became 5 px wide or 5 px tall
    /// (`marquee::THRESHOLD`), it was never shown. The selection is then
    /// not changed here, and the click event that follows is handled as a
    /// click on empty space.
    ///
    /// In every case `focus` gets the keyboard focus.
    fn marquee_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.take() else {
            return;
        };
        // A rubber band whose rectangle covers subfolder tiles selects
        // those folders and no items (production's
        // `collectHeaderMarqueeHits`). When the rubber band adds to the
        // selection and no items are selected, the folders are added to
        // the folders that are already selected. Otherwise they replace
        // the selection.
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

    /// Called in the frame in which the zoom changed. The tiles move, so the
    /// tile that was under the pointer may no longer be there. This removes
    /// the hover mark and closes the hover preview. After 150 ms, when the
    /// zoom animation has finished, it marks the tile that is then under the
    /// pointer.
    fn reset_hover_after_zoom(&mut self, cx: &mut Context<Self>) {
        self.hovered = None;
        self.preview.close(cx);
        self.scrolled = std::time::Instant::now();
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::motion::ZOOM_GLIDE)
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(pointer) = this.pointer {
                    this.pointer_moved(pointer, cx);
                }
                cx.notify();
            });
        }));
    }

    /// Zooms the grid from a scroll event over the tiles. This runs only
    /// when the Controls setting "Mouse wheel" is "Adjust thumbnail size"
    /// (production's `onGridWheel`).
    ///
    /// `delta_y` is the scrolled distance with the browser's sign: positive
    /// is scrolling down, which zooms out. `notch` is true for a mouse
    /// wheel, which reports lines, and false for a trackpad, which reports
    /// pixels.
    ///
    /// - A mouse wheel event is one zoom step.
    /// - A trackpad makes one zoom step for every 60 px it scrolls
    ///   ([`WHEEL_STEP`]).
    /// - In the justified layout a trackpad changes the row height by the
    ///   number of pixels it scrolled.
    fn wheel_zoom(&mut self, delta_y: f32, notch: bool, cx: &mut Context<Self>) {
        if delta_y == 0. {
            return;
        }
        let larger = delta_y < 0.;
        let mut steps = 0;
        let mut height = None;
        if notch {
            steps = 1;
        } else if let layout::Zoom::Height(rows) = self.session.read(cx).zoom() {
            height = Some(rows - delta_y);
        } else {
            // When the scroll direction reverses, the distance scrolled in
            // the old direction does not count toward a step in the new one.
            if (self.wheel_distance < 0.) != larger {
                self.wheel_distance = 0.;
            }
            self.wheel_distance += delta_y;
            steps = (self.wheel_distance.abs() / WHEEL_STEP) as usize;
            self.wheel_distance %= WHEEL_STEP;
        }
        self.session.update(cx, |session, cx| {
            if let Some(height) = height {
                session.zoom_to_height(height, cx);
            }
            for _ in 0..steps {
                session.zoom_step(larger, cx);
            }
        });
    }

    /// Is meant to scroll the grid down by nine tenths of its visible
    /// height, and not past the end of its content. The shell calls this
    /// when Space is pressed and the Controls setting for the Space key is
    /// set to scroll (`spaceKeyAction: 'scroll'`).
    ///
    /// TODO: this scrolls the grid to its top instead.
    /// `ScrollHandle::max_offset` is 0 or positive, so `bottom` below is 0
    /// or negative, and the new scroll position is limited to the range
    /// from 0 to 0. `bottom` has to be the value without the minus sign, as
    /// `limit` is in `pointer::autoscroll`.
    pub fn scroll_by_page(&mut self, cx: &mut Context<Self>) {
        let height = f32::from(self.bounds.size.height);
        let bottom = -f32::from(self.scroll.max_offset().y);
        let top = (self.raw_scroll() + height * 0.9).clamp(0., bottom.max(0.));
        self.scroll.set_offset(point(px(0.), px(-top)));
        cx.notify();
    }

    /// Moves the selection with the keyboard, as production's grid does
    /// (`useGridArrowNav.ts`). `action` is the move, for example one item
    /// to the right or one page down. With `extend` set, which the shell
    /// does when Shift is held, the range from the item where the selection
    /// started to the new item is selected. Without it the new item is
    /// selected alone. The grid then scrolls the new item into view.
    pub fn move_selection(&mut self, action: nav::Move, extend: bool, cx: &mut Context<Self>) {
        let height = f32::from(self.bounds.size.height);
        let target = self
            .session
            .read(cx)
            .move_target(action, self.grid_width(), height);
        let Some(target) = target else {
            return;
        };
        // `move_to` selects the item, or with `extend` the range from the
        // item where the selection started to it, and sets
        // `session.reveal`. The next frame in which the item's tile is
        // laid out scrolls the tile into view. If the session's window of
        // results does not hold the item, `move_to` requests the window
        // that does, and the item is selected and scrolled into view when
        // that window arrives.
        self.session
            .update(cx, |session, cx| session.move_to(target, extend, cx));
        cx.notify();
    }

    /// Emits `EmptyMenuRequested`, which asks the shell to open the grid's
    /// own context menu at `position` (in window coordinates), and gives
    /// `focus` the keyboard focus. It is called for a right-click, or a
    /// Control-click on macOS, on empty space in the list of tiles.
    fn empty_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EmptyMenuRequested(position));
        window.focus(&self.focus, cx);
    }
}

/// Returns an element that, while it is drawn, listens for mouse events in
/// the whole window. This is what pointer capture does in a browser. The
/// grid draws it during a rubber band selection, and from a press on a tile
/// until the end of the drag that may follow, so that both keep working
/// while the pointer is outside the grid.
///
/// `moved` is called for each move of the pointer while the left button is
/// held. `released` is called when the left button is released. `released`
/// is also called when a move arrives with the left button not held, which
/// means that the button was released without the window receiving the
/// release.
///
/// The listeners run in GPUI's capture phase, which is before the click
/// event that the release produces is handled. `released` can therefore set
/// a flag such as `swallow_click` before the handler of that click reads it.
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

/// What the grid's area shows in a frame. `GridView::snapshot` chooses it
/// and `GridView::body` builds its elements.
enum Surface {
    Tiles,
    /// The first page of results has been asked for and has not arrived
    /// yet. The grid's area is left empty.
    Loading,
    Error(String),
    Empty(empty::EmptyCopy),
}

impl Render for GridView {
    /// The tiles' layout depends on the grid's width and the tiles that are
    /// drawn depend on its height, so `render` returns an element that
    /// fills the grid's area and builds the grid for that element's bounds
    /// in the frame that lays it out (`ui::sized_view`). Every frame is
    /// therefore built for the size the grid has in that frame, also while
    /// the window is resized or the sidebar and the inspector are hidden.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::sized_view(cx.weak_entity(), Self::build).size_full()
    }
}

impl GridView {
    /// Builds one frame of the grid for `bounds`, the grid's position and
    /// size in window coordinates in this frame. It stores them in
    /// `self.bounds` first, and everything it calls reads them from there.
    fn build(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.bounds = bounds;
        crate::stats::rendered(crate::stats::View::Grid);
        let build_started = crate::stats::Started::now();
        let restored_top = self.prepare(window, cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        // The tiles, and the subfolder header above them, are laid out in
        // the grid's width less the scrollbar's gutter, so that nothing is
        // drawn under the scrollbar.
        let width = self.grid_width();
        // The session needs the grid's width to turn the zoom's tile size
        // into a number of columns for the toolbar's zoom control and for
        // zoom steps.
        self.session
            .update(cx, |session, cx| session.set_grid_width(width, cx));
        self.sync_subfolders(width, window, cx);
        let viewport = f32::from(self.bounds.size.height);
        let top = self.scroll_top();
        let moved = self.follow_scroll(top, cx);
        if self
            .zoom_animation
            .as_ref()
            .is_some_and(|animation| animation.finished(now))
        {
            self.zoom_animation = None;
        }

        // `snapshot` computes the layout if it is out of date and copies
        // from the session everything this frame draws. It copies items
        // only for the tiles that are drawn. The session is not borrowed
        // after it returns, so the code below can use `cx` to build
        // elements and listeners. The `top` it returns is the scroll
        // position after any change it made to keep a tile in place or to
        // scroll an item into view.
        let pointer = window.mouse_position();
        let (top, frame) = self.snapshot(width, viewport, top, restored_top, now, pointer, cx);
        let frame::Frame {
            fade,
            animating,
            prefs,
            surface,
            windowed,
            zoom_changed,
            mut tiles,
        } = frame;
        // `windowed` is true when the destination has more items than the
        // session holds and the results are not loading. The grid then
        // asks for the window of results the viewport needs, as
        // production's `CanvasGrid` does, in three cases. The first is
        // after a scroll. The second is while the middle of the viewport
        // is outside the window's tiles, which happens when a saved scroll
        // position was restored, an item was scrolled into view, or the
        // scrollbar's thumb was dragged far. The third is after a change
        // of zoom, which also puts other items in the viewport without a
        // scroll.
        if windowed
            && (moved
                || zoom_changed
                || window::misses(self.placement, self.layout.tiles.len(), top, viewport))
        {
            self.schedule_window(cx);
        }

        let tile_count = tiles.len();
        // `load_pictures` runs while each tile still has its final position
        // and size, so thumbnails and originals are requested for the size
        // the tiles will have when a zoom animation ends, not for each
        // size in between.
        self.load_pictures(
            &mut tiles, &prefs, top, viewport, now, animating, window, cx,
        );
        // During a zoom animation, each tile is drawn at its position and
        // size for this moment, and another frame is requested.
        if let Some(animation) = &self.zoom_animation {
            for data in &mut tiles {
                let id = data.look.item.root_id.0;
                data.look.tile = animation.tile_at(id, data.look.tile, now);
            }
            window.request_animation_frame();
        }

        // The scrollbar is drawn only when the grid shows tiles. The error
        // message, the loading state and the empty state do not scroll.
        // The scroll handle can still hold the scrollable height of the
        // tiles that were shown before, and without this check a scrollbar
        // would be drawn for it.
        let scrolls = matches!(surface, Surface::Tiles);
        let body = self.body(surface, tiles, &prefs, fade, now, window, cx);
        crate::stats::grid_built(build_started, tile_count);
        let root = div()
            .relative()
            .size_full()
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
        self.overlays(root, scrolls, window, cx).into_any_element()
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
        let file = |hash: &str, path: &str| (hash.to_owned(), PathBuf::from(path));
        let found = vec![
            file("a", "/library/a.jpg"),
            file("b", "/library/b.png"),
            file("a", "/library/elsewhere/a.jpg"),
        ];
        assert_eq!(
            drag_out_paths(found),
            [
                PathBuf::from("/library/a.jpg"),
                PathBuf::from("/library/b.png")
            ]
        );
    }
}
