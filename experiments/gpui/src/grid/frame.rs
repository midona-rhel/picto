//! The steps that `GridView::render` (in `mod.rs`) runs before it builds the
//! grid's elements for one frame:
//!
//! - `prepare` scrolls the grid while a rubber band is held near the top or
//!   bottom edge, writes the grid's scroll position to the session, and
//!   applies the scroll position of a saved grid that Back or Forward
//!   restored.
//! - `follow_scroll` removes the hover mark and closes the hover preview
//!   when the grid has scrolled since the last frame.
//! - `snapshot` lays out the window of results again when something the
//!   layout depends on changed, and copies the tiles to draw out of the
//!   session.
//! - `load_pictures` asks for the thumbnails and the original files that
//!   those tiles need.
use super::layout::Layout;
use super::prefs::ViewPrefs;
use super::thumbnails::{self, Thumb};
use super::tile::{TileData, TileLook, aspect, media_rect};
use super::{
    GridView, Surface, empty, layout, nav, originals, pointer, reveal, window, zoom_animation,
};
use gpui::*;
use picto_library::query::ItemScope;

/// What `GridView::snapshot` returns for `render` to draw one frame of the
/// grid. `fade` is the opacity, from 0 to 1, that the grid is drawn with
/// while one destination fades out and the next one fades in, and
/// `animating` is true while such a fade is running. `prefs` is a copy of
/// the session's view preferences. `surface` says what fills the grid's
/// area: the tiles, an error message, nothing while the first results are
/// loading, or the empty state. `tiles` holds one entry for each tile that
/// is drawn in this frame.
pub(super) struct Frame {
    pub fade: f32,
    pub animating: bool,
    pub prefs: ViewPrefs,
    pub surface: Surface,
    /// True when the destination has more items than the window of results
    /// holds and no read of results is in progress. `render` asks for
    /// another window of results only when this is true.
    pub windowed: bool,
    /// Whether the zoom's tile size changed since the last frame, with the
    /// same items, width, spacing, layout mode and text rows.
    pub zoom_changed: bool,
    pub tiles: Vec<TileData>,
}

impl GridView {
    /// Runs first in each frame. It does three things:
    ///
    /// 1. While a rubber band is being dragged and the pointer is near the
    ///    top or bottom edge of the grid, it scrolls the grid.
    /// 2. When this grid has already drawn the results the session holds,
    ///    it writes the grid's scroll offset and the top of the window of
    ///    results to the session. The session saves both when the user
    ///    leaves the destination, so that Back and Forward can show the grid
    ///    again as it was.
    /// 3. When the session holds a different set of results from the one
    ///    this grid last drew, it writes neither. It scrolls the grid to the
    ///    offset that the new results start at: the offset of a saved grid
    ///    that Back or Forward restored, or 0, which is the top.
    ///
    /// Returns `Session::restore_window_top` when a saved grid is being
    /// restored: the distance in pixels from the top of the scrollable
    /// height to the top of the window's tiles, as it was when the user left
    /// that grid. `snapshot` places the window there. Returns `None` in
    /// every other frame.
    pub(super) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<f32> {
        // While a rubber band is being dragged and the pointer is near the
        // top or bottom edge of the grid, `pointer::autoscroll` scrolls the
        // grid by the rubber band's speed, once in each frame. Another frame
        // is requested for as long as the grid scrolled. The rubber band's
        // rectangle is updated for the new scroll position later in this
        // frame, in `snapshot` (`marquee.track`).
        if pointer::autoscroll(self.marquee.as_ref(), &self.scroll, self.raw_scroll()) {
            window.request_animation_frame();
        }
        // The session is updated once, below. First `advance_transition`
        // moves the fade between two destinations on to its next stage when
        // a fade-out or a fade-in has lasted 170 ms (`motion::SCOPE_FADE`),
        // and the session is told whether a press, a rubber band or a drag
        // is in progress (`pointer_busy`). What happens next depends on
        // whether this grid has already drawn the session's results, which
        // is when `results_epoch` equals `self.epoch`. If it has, the grid's
        // scroll offset and the top of the window of results are written to
        // the session. If the results are a different set, neither is
        // written. The grid takes from the session the scroll offset that
        // the new results start at (`restore_scroll`, or 0 when there is
        // none) and the saved top of the window (`restore_window_top`).
        let scrolled = self.raw_scroll();
        let epoch = self.epoch;
        let window_top = self.placement.top;
        // `busy` is true while the pointer is pressed on a tile, a rubber
        // band is being dragged, or items are being dragged. The session
        // does not start a background refresh of the results while it is
        // true, because replacing the items would interrupt the gesture.
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
        restored_top
    }

    /// Compares `top`, which is how far the items are scrolled in this
    /// frame, with the scroll position of the last frame, and returns
    /// whether it changed. When it changed, the hover mark is removed from
    /// the tile under the pointer and the hover preview is closed.
    pub(super) fn follow_scroll(&mut self, top: f32, cx: &mut Context<Self>) -> bool {
        // After a scroll the pointer may be over another tile, so the hover
        // mark is removed and the hover preview is closed in this frame.
        // 80 ms after the last scroll (`motion::MAGNIFIER_RESUME`), the
        // timer below marks the tile that is under the pointer at that
        // time. Each scroll replaces the timer, which cancels the one
        // before it.
        let moved = top != self.last_top;
        if moved {
            self.last_top = top;
            self.hovered = None;
            // `window_anchor` holds the ordinal that a window of results
            // was asked for, and the content position where the middle of
            // the viewport was at that time. The grid has scrolled since,
            // so the position no longer applies and the anchor is cleared.
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
        moved
    }

    /// Lays out the window of results again when something the layout
    /// depends on changed, and builds the `Frame` for this draw from the
    /// session.
    ///
    /// `width` is the width in pixels that the tiles are laid out in, and
    /// `viewport` is the height of the grid in pixels. `top` is how far the
    /// items are scrolled (`scroll_top`). `restored_top` is what `prepare`
    /// returned. `now` is the time in milliseconds since the grid was
    /// created (`GridView::clock`). `pointer` is the pointer's position in
    /// window coordinates.
    ///
    /// Returns the scroll position after the call, and the frame. The
    /// scroll position can differ from `top` in three cases:
    ///
    /// - The zoom changed. The tile under the pointer stays at the same
    ///   height on screen. When the pointer is not over the grid, the tile
    ///   nearest the centre of the viewport does.
    /// - The layout changed in another way, or the window of results was
    ///   replaced. The topmost tile in the viewport, or the topmost
    ///   selected tile in the viewport when there is one, stays at the same
    ///   place on screen.
    /// - The session asked for an item to be scrolled into view
    ///   (`Session::reveal`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn snapshot(
        &mut self,
        width: f32,
        viewport: f32,
        mut top: f32,
        restored_top: Option<f32>,
        now: f64,
        pointer: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> (f32, Frame) {
        // The block below reads the session and copies out of it everything
        // the frame needs, for the tiles that are drawn only. `cx` is
        // borrowed for as long as the session is read. The copies are what
        // let the code after the block, and the code that builds the tiles'
        // elements and their listeners, use `cx` again. For the same reason
        // nothing in the block can update the session. `measured` and
        // `revealed` carry what the block wants written to the session, and
        // the update is made after the block: `measured` is a height per
        // item that was measured in this frame, and `revealed` is the
        // ordinal of an item that was scrolled into view.
        let mut measured = None;
        let mut revealed = None;
        let mut zoom_changed = false;
        let (fade, animating, prefs, surface, windowed, tiles) = {
            let session = self.session.read(cx);
            let fresh = session.results_epoch != self.epoch;
            if fresh {
                self.epoch = session.results_epoch;
                self.layout_key = None;
                self.window_anchor = None;
                // The results are different items now, so a zoom animation
                // of the old items stops and the zoom's anchor tile is
                // forgotten.
                self.zoom_animation = None;
                self.zoom_anchor = None;
                // A rubber band and a hovered tile are stored as tile
                // indexes and positions in the layout of the old items, so
                // they are cleared too.
                self.marquee = None;
                self.hovered = None;
            }
            let prefs = session.prefs.clone();
            let text_height = prefs.text_height();
            self.text_height = text_height;
            let geometry = (
                width as u32,
                session.tile.to_bits(),
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
                // Compare the parts of the old and the new layout key
                // (`LayoutKey`): the zoom's tile size, the window of
                // results, and the rest (width, spacing, layout mode and
                // text rows).
                let (zoom_same, window_same, rest_same) = match (&self.layout_key, &key) {
                    (Some(before), Some(after)) => (
                        before.1 == after.1,
                        before.5 == after.5,
                        (before.0, before.2, before.3, before.4)
                            == (after.0, after.2, after.3, after.4),
                    ),
                    _ => (false, false, false),
                };
                // Only the zoom's tile size changed. The key differs, so
                // when the window and the rest are the same the tile size
                // is what differs.
                zoom_changed = !fresh && rest_same && window_same;
                // Only the window of results changed.
                let only_window_changed = rest_same && zoom_same;
                if window_changed {
                    // The window of results holds other items now. A rubber
                    // band, a hovered tile and the place where dragged
                    // items would be put back (`reorder`) are stored as
                    // tile indexes of the window before, so they are
                    // cleared.
                    self.marquee = None;
                    self.hovered = None;
                    self.reorder = None;
                }
                let aspects = session
                    .items
                    .iter()
                    // An item whose width and height are not known is laid
                    // out with an aspect ratio of 3:2, as in
                    // `gridLayoutModel.ts`.
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
                // `density` is the average height in pixels that one item
                // takes in the layout. `window::place` multiplies it by the
                // destination's total to estimate the scrollable height of
                // the whole destination. The session keeps the value that
                // was measured for this geometry (width, tile size,
                // spacing, layout mode and text rows), as production's
                // `densityRef` does, so the estimated height stays the same
                // while other windows of the same results load. When the
                // session has no value for this geometry, the value is
                // measured here on the first 500 items (`window::SAMPLE`)
                // of the window that is held now, or on all of them when
                // the window holds fewer: the height of their layout
                // divided by their number. It is written to the session
                // after the block.
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
                // A saved grid is being restored by Back or Forward. Put the
                // window of results at the distance from the top of the
                // scrollable height that it had when the user left that
                // grid, so that the restored scroll offset shows the same
                // items. The distance is limited to the range the
                // scrollable height allows.
                if let Some(restored) = restored_top {
                    let span = (placement.height - next.total_height).max(0.);
                    placement.top = restored.clamp(0., span);
                    placement.bottom = placement.top + next.total_height;
                }
                let previous = std::mem::replace(&mut self.layout, next);
                let previous_top = std::mem::replace(&mut self.placement, placement).top;
                let ids = session
                    .items
                    .iter()
                    .map(|item| item.root_id.0)
                    .collect::<Vec<_>>();
                let header = self.subfolders.height();
                if zoom_changed {
                    // Only the zoom changed. Scroll so that the anchor tile
                    // stays at the same height on screen, and start the
                    // animation that moves the tiles in the viewport from
                    // their old positions and sizes to their new ones.
                    //
                    // `top` was measured with the subfolder header's new
                    // height, but the header's height can change with the
                    // zoom. `top_before` is how far the items were scrolled
                    // in the frame that was drawn last.
                    let top_before = top + header - self.header_height_drawn;
                    // The position and size a tile had on screen in the last
                    // frame, with `y` measured from the viewport's top. If a
                    // zoom animation is still running, this is the tile's
                    // position part of the way through that animation.
                    let on_screen_before = |index: usize| {
                        let tile = previous.tiles.get(index)?;
                        let placed = layout::Tile {
                            y: tile.y + previous_top,
                            ..*tile
                        };
                        let id = *ids.get(index)?;
                        let mut tile = match &self.zoom_animation {
                            Some(animation) => animation.tile_at(id, placed, now),
                            None => placed,
                        };
                        tile.y -= top_before;
                        Some(tile)
                    };
                    let in_viewport = |layout: &Layout, local_top: f32| {
                        layout.visible(local_top, local_top + viewport)
                    };
                    // The indexes of the tiles to animate: first the tiles
                    // that were in the viewport, and the tiles of an
                    // animation that is still running.
                    let mut animated = in_viewport(&previous, top_before - previous_top);
                    if let Some(animation) = &self.zoom_animation {
                        animated.extend(
                            ids.iter()
                                .enumerate()
                                .filter(|(_, id)| animation.includes(**id))
                                .map(|(index, _)| index),
                        );
                    }
                    // The anchor tile is looked for at the pointer when the
                    // pointer is over the grid, and at the centre of the
                    // viewport otherwise. The pointer's position comes from
                    // the window and not from `self.pointer`: GPUI reports
                    // the pointer as having left the grid as soon as a key
                    // is pressed, and the zoom is usually changed with keys.
                    let point = Some(self.local(pointer))
                        .filter(|(x, y)| (0.0..width).contains(x) && (0.0..viewport).contains(y))
                        .unwrap_or((width / 2., viewport / 2.));
                    // Reuse the last step's anchor tile if the pointer and the
                    // scroll position are the same as after that step
                    // (`zoom_animation::Anchor` explains why). Otherwise
                    // choose the tile at the point.
                    let anchor = self
                        .zoom_anchor
                        .take()
                        .filter(|anchor| {
                            anchor.point == point && (anchor.scroll_top - top_before).abs() < 0.5
                        })
                        .and_then(|anchor| {
                            let index = ids.iter().position(|id| *id == anchor.id)?;
                            Some((index, anchor.fraction_down))
                        })
                        .or_else(|| {
                            zoom_animation::nearest_tile(
                                animated
                                    .iter()
                                    .filter_map(|&index| Some((index, on_screen_before(index)?))),
                                point.0,
                                point.1,
                            )
                        });
                    let top_at_start = top;
                    if let Some((index, fraction_down)) = anchor
                        && let (Some(before), Some(after), Some(&id)) = (
                            on_screen_before(index),
                            self.layout.tiles.get(index),
                            ids.get(index),
                        )
                    {
                        // The point `fraction_down` of the way down the
                        // anchor tile keeps its distance from the viewport's
                        // top. The scroll position is limited to the range
                        // the new content height allows.
                        let fixed_y = before.y + fraction_down * before.h;
                        let wanted =
                            after.y + fraction_down * after.h + self.placement.top - fixed_y;
                        let limit = (self.placement.height + header - viewport).max(0.) - header;
                        top = wanted.clamp(-header, limit);
                        self.zoom_anchor = Some(zoom_animation::Anchor {
                            id,
                            fraction_down,
                            point,
                            scroll_top: top,
                        });
                    }
                    // Also animate the tiles that are in the viewport after
                    // the change.
                    animated.extend(in_viewport(&self.layout, top - self.placement.top));
                    // Each animated tile starts where it was on screen. Adding
                    // the new scroll position turns that into the coordinates
                    // tiles are drawn in from now on.
                    let start_tiles = animated
                        .into_iter()
                        .filter_map(|index| {
                            let mut tile = on_screen_before(index)?;
                            tile.y += top;
                            Some((*ids.get(index)?, tile))
                        })
                        .collect();
                    // The subfolder tiles of the header are animated too when
                    // the header is in the viewport before or after the
                    // change. Their start positions are moved by the change
                    // of the scroll position, so they start where they were
                    // on screen.
                    let scrolled_by = (top + header) - (top_before + self.header_height_drawn);
                    let start_subfolder_tiles = if top_before < 0. || top < 0. {
                        let drawn = &self.header_geometry_drawn;
                        drawn
                            .tiles
                            .iter()
                            .enumerate()
                            .map(|(index, &(x, y, h))| {
                                let w = drawn.tile_width;
                                let mut tile = layout::Tile { x, y, w, h };
                                if let Some(animation) = &self.zoom_animation {
                                    tile = animation.subfolder_tile_at(index, tile, now);
                                }
                                tile.y += scrolled_by;
                                tile
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    // Settings > Controls > "Animate zoom in the grid". The
                    // saved setting is read at each change of zoom, so
                    // turning it on or off applies to the next zoom step.
                    // When it is off, no animation is kept, and the tiles
                    // are drawn at their final positions in this frame. The
                    // scroll position above is set either way.
                    let animate = crate::settings::current(cx).grid_zoom_animation;
                    self.zoom_animation = animate.then(|| {
                        zoom_animation::ZoomAnimation::new(now, start_tiles, start_subfolder_tiles)
                    });
                    // `window_anchor` holds the content position where the
                    // middle of the viewport was when a window of results was
                    // asked for. The content under the viewport has moved,
                    // so move that position by the same distance.
                    if let Some((_, at)) = &mut self.window_anchor {
                        *at += top - top_before;
                    }
                    if top != top_at_start {
                        self.last_top = top;
                        self.set_scroll_top(top);
                    }
                } else if self.layout_key.is_some() && !fresh {
                    // The grid has laid out these results before, and
                    // something other than the zoom alone changed: the
                    // width, the spacing, the layout mode or the text rows,
                    // or the window of results was replaced by another
                    // window or was read again in place. One tile is kept at
                    // the same place on screen, as `CanvasGrid` does: the
                    // topmost tile in the viewport, or the topmost selected
                    // tile in the viewport when there is one
                    // (`layout::scroll_anchor`). The tile's item can be at
                    // another index in the new window, so it is looked up
                    // by its id. `now` maps the id of each item in the new
                    // window to its index there.
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
                    // `scroll_anchor` is given positions measured from the
                    // top of the scrolled content, where the subfolder
                    // header comes first and the items start `header`
                    // pixels down. The tiles of both layouts and the scroll
                    // position (`raw`) are converted to that measure here.
                    // The reason is that `scroll_anchor` never returns a
                    // position less than 0. Measured from the top of the
                    // items, the scroll position is negative while the
                    // header is in the viewport, and that limit would
                    // scroll the header out of the viewport.
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
                    let top_at_start = top;
                    let anchored =
                        layout::scroll_anchor(&before, &after, &selected, raw, viewport, moved);
                    if let Some(anchored) = anchored
                        && anchored != raw
                    {
                        top = anchored - header;
                        self.last_top = top;
                        self.set_scroll_top(top);
                    }
                    // A zoom animation that is still running continues when
                    // only the window of results changed and the scroll
                    // position followed it. Its start positions move by the
                    // same distance as the scroll position, so the tiles
                    // stay where they are on screen. After any other change
                    // the animation stops and the tiles are drawn at their
                    // final positions.
                    match &mut self.zoom_animation {
                        Some(animation) if only_window_changed && anchored.is_some() => {
                            animation.shift_start_positions(top - top_at_start)
                        }
                        _ => self.zoom_animation = None,
                    }
                } else {
                    self.zoom_animation = None;
                }
                self.layout_ids = ids;
                self.layout_start = session.start;
                self.layout_key = key;
                crate::stats::grid_part(crate::stats::GridPart::Layout, laying_out);
            }
            // `Session::reveal` is the ordinal of an item that a keyboard
            // move, or the viewer's next or previous, moved the selection
            // to. When the window that is laid out holds that item, the
            // grid scrolls just far enough to bring its tile into the
            // viewport, with the spacing (`prefs.gap`) between the tile and
            // the viewport's edge, and never above the top of the subfolder
            // header. `reveal` is cleared in the session after the block.
            // When the window does not hold the item, nothing is done here,
            // and this is tried again in each frame until the window that
            // holds the item is laid out.
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
            // Choose what fills the grid's area. The first of these that
            // applies is used, in the order `GridScreen.tsx` uses:
            //
            // 1. The error message, when the session has an error.
            // 2. The tiles, when the window of results holds items, or when
            //    the folder has subfolder tiles and no read of results is
            //    in progress.
            // 3. Nothing, while results are being read, unless that read is
            //    a refresh in place (`session.refreshing`).
            // 4. The empty state in every other case: the results have
            //    arrived and hold no items, or a destination without items
            //    is being refreshed in place.
            //
            // A destination is therefore never shown as empty while its
            // first results are still being read.
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
            // The layout holds only the tiles of the window of results,
            // measured from the window's top, and `local_top` is the scroll
            // position in that measure. The tiles drawn are those that
            // touch the viewport or the 500 pixels (`reveal::MARGIN`) above
            // and below it. The rest of the scrollable height stands for
            // the destination's items outside the window. Nothing is drawn
            // there until the window that holds those items arrives.
            let local_top = top - self.placement.top;
            let mut shown = self.layout.visible(
                local_top - reveal::MARGIN,
                local_top + viewport + reveal::MARGIN,
            );
            // During a zoom animation, the animated tiles are drawn even if
            // their final position is outside the viewport, because they
            // are still crossing it.
            if let Some(animation) = &self.zoom_animation {
                shown.extend(
                    self.layout_ids
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| animation.includes(**id))
                        .map(|(index, _)| index),
                );
                shown.sort_unstable();
                shown.dedup();
            }
            let marquee_base = self.marquee.as_ref().map(|_| {
                (0..self.layout.tiles.len())
                    .filter(|index| session.is_selected(session.start + index))
                    .collect::<std::collections::BTreeSet<_>>()
            });
            let tiles = shown
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
                            // The three values below are placeholders.
                            // `GridView::tile` (in `tile.rs`) sets these
                            // fields when it builds the tile's element.
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
        // Remember the subfolder header as it is drawn in this frame. The
        // next change of zoom needs it to know where things were on screen.
        self.header_height_drawn = self.subfolders.height();
        if self.header_geometry_drawn != self.subfolders.geometry {
            self.header_geometry_drawn = self.subfolders.geometry.clone();
        }
        if zoom_changed {
            self.reset_hover_after_zoom(cx);
        }
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
        (
            top,
            Frame {
                fade,
                animating,
                prefs,
                surface,
                windowed,
                zoom_changed,
                tiles,
            },
        )
    }

    /// Asks for the thumbnails and the original files that `tiles` need,
    /// and writes into each tile how far its pictures have loaded
    /// (`look.thumb` and `look.original`).
    ///
    /// - Thumbnails are asked for in order: the tiles inside the viewport
    ///   first, and of those the tile nearest the vertical middle of the
    ///   viewport first.
    /// - A tile whose thumbnail could not be read or decoded is given to
    ///   `Thumbnails::make_missing`, which makes the thumbnail if no
    ///   thumbnail file exists on disk.
    /// - A tile in the viewport that draws a still image with more pixels
    ///   than its thumbnail has asks for its original file.
    ///
    /// `top` is how far the items are scrolled and `viewport` is the height
    /// of the grid, both in pixels. `now` is the time in milliseconds since
    /// the grid was created. `animating` is true while the fade between two
    /// destinations is running. Another frame is requested while that fade
    /// is running, and while a thumbnail is waiting to fade in or is fading
    /// in.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_pictures(
        &mut self,
        tiles: &mut [TileData],
        prefs: &ViewPrefs,
        top: f32,
        viewport: f32,
        now: f64,
        animating: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `spans` is the top and the height of each tile. From them,
        // `thumbnails::load_order` below gives the order in which the
        // thumbnails are loaded: the tiles inside the viewport first, and
        // within those the tile whose centre is nearest the vertical middle
        // of the viewport first.
        let spans = tiles
            .iter()
            .map(|data| (data.look.tile.y, data.look.tile.h))
            .collect::<Vec<_>>();
        // The cover thumbnails of the subfolder tiles come before the
        // thumbnails of the items in the list, while any part of the
        // subfolder header is in the viewport. That is while the content is
        // scrolled by less than the header's height.
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
        // A tile asks for its original file when its thumbnail is loaded,
        // the tile is at least partly inside the viewport, and its item is
        // a still image, other than a GIF or a JPEG XL, that the tile draws
        // with more pixels than the thumbnail has
        // (`originals::wants_original`; a thumbnail's long edge is 512
        // pixels). `Originals::plan` reads the original once the tile has
        // stayed in the viewport for 200 ms (`originals::DWELL`), and
        // decodes it to the size the tile draws it at. `scale` is the
        // number of device pixels per layout pixel in this window.
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
            for data in tiles.iter_mut() {
                data.look.original = originals.get(&data.look.item.content_hash);
            }
        }
        self.reveal.update(&decoded, now);
        // Request another frame while the fade between two destinations is
        // running, and while a thumbnail is waiting to fade in or is fading
        // in.
        if animating || self.reveal.active(now) {
            window.request_animation_frame();
        }
    }
}
