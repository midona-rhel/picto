//! The grid of results. Its own view, so scrolling, thumbnail reveals and
//! fades repaint the grid and nothing else.
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

use crate::session::{Change, Session};
use crate::ui;
use gpui::prelude::FluentBuilder;
use gpui::*;
use layout::{Layout, ViewMode};
use std::path::PathBuf;
use thumbnails::Thumbnails;

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
        if pointer::hover(&mut self.hovered, hit) {
            cx.notify();
        }
        if let Some(index) = pointer::magnifier(hit, &self.layout, self.text_height) {
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
        // Only the copy's own files leave the window, by the paths the
        // system shows (no `\\?\` prefix on Windows). The vendored GPUI
        // drags them natively on every platform: AppKit, OLE, XDND and
        // Wayland.
        let files = if files.is_empty() {
            files
        } else {
            crate::desktop::drag_out(&files, cx)
                .iter()
                .map(|path| crate::desktop::system_path(path))
                .collect()
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
        let restored_top = self.prepare(window, cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        // The scrollbar's gutter is not grid: tiles, and the subfolder
        // header above them, lay out beside it.
        let width = self.grid_width();
        self.sync_subfolders(width, window, cx);
        let viewport = f32::from(self.bounds.size.height);
        let top = self.scroll_top();
        let moved = self.follow_scroll(top, cx);

        // Everything the frame needs from the session, copied for the visible
        // tiles only, so the listeners below can borrow the context.
        let (top, frame) = self.snapshot(width, viewport, top, restored_top, cx);
        let frame::Frame {
            fade,
            animating,
            prefs,
            surface,
            windowed,
            mut tiles,
        } = frame;
        // `CanvasGrid`'s window requests: after a scroll, and whenever the
        // viewport's middle has no tile of the window (a restored or
        // revealed scroll position, a thumb dragged far away).
        if windowed
            && (moved || window::misses(self.placement, self.layout.tiles.len(), top, viewport))
        {
            self.schedule_window(cx);
        }

        let tile_count = tiles.len();
        self.load_pictures(
            &mut tiles, &prefs, top, viewport, now, animating, window, cx,
        );

        let measure = pointer::measure(cx.entity().downgrade(), |this: &mut Self| &mut this.bounds)
            .absolute()
            .size_full();

        // Only the tiles scroll: an empty or failed grid has no scrollbar,
        // whatever the last results left in the scroll handle.
        let scrolls = matches!(surface, Surface::Tiles);
        let body = self.body(surface, tiles, &prefs, fade, now, window, cx);
        crate::stats::grid_built(build_started, tile_count);
        let root = div()
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
        self.overlays(root, scrolls, window, cx)
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
