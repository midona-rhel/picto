//! The animation the grid plays when its zoom changes.
//!
//! When the zoom changes, every tile that is in the viewport before the
//! change or after it is drawn moving in a straight line from its old
//! position and size to its new position and size. The animation lasts
//! 150 ms ([`crate::motion::ZOOM_GLIDE`]). A tile that enters the viewport
//! starts at the position the old layout gave it, and a tile that leaves the
//! viewport ends at the position the new layout gives it.
//!
//! The scroll position is changed at the same time so that one tile, the
//! anchor, stays at the same height on screen: the tile under the pointer
//! when the pointer is over the grid, otherwise the tile nearest the centre
//! of the viewport ([`nearest_tile`], [`Anchor`]).
//!
//! If the zoom changes again while tiles are still moving, the new animation
//! starts from the positions the tiles have at that moment.
//!
//! Only tiles in the viewport take part, so the cost does not grow with the
//! number of items in the destination. The animation is used only for a
//! change of zoom. A change of the grid's width, spacing, layout mode or
//! results is drawn at its final positions in one frame.
use super::layout::Tile;
use std::collections::HashMap;
use std::time::Duration;

/// A zoom animation in progress: where the animated tiles started, and when.
pub(super) struct ZoomAnimation {
    /// The time the animation started, in milliseconds on the grid's clock
    /// (`GridView::clock`).
    started: f64,
    /// The position and size each animated item tile had when the animation
    /// started, keyed by the item's root id. The coordinates are the ones
    /// tiles are drawn in after the change: `y` is measured from the top of
    /// the items' content, below the subfolder header, with the window of
    /// results placed in the destination's full height.
    start_tiles: HashMap<u32, Tile>,
    /// The position and size each subfolder tile of the header had when the
    /// animation started, in the header's order. `y` is measured from the
    /// top of the scrolled content. It is empty when the header was outside
    /// the viewport before and after the change.
    start_subfolder_tiles: Vec<Tile>,
}

impl ZoomAnimation {
    pub fn new(
        started: f64,
        start_tiles: HashMap<u32, Tile>,
        start_subfolder_tiles: Vec<Tile>,
    ) -> Self {
        Self {
            started,
            start_tiles,
            start_subfolder_tiles,
        }
    }

    fn elapsed(&self, now: f64) -> Duration {
        Duration::from_secs_f64((now - self.started).max(0.) / 1000.)
    }

    /// Whether 150 ms have passed since the animation started, so every tile
    /// is at its final position.
    pub fn finished(&self, now: f64) -> bool {
        self.elapsed(now) >= crate::motion::ZOOM_GLIDE
    }

    /// Whether the tile of the item with this root id is animated.
    pub fn includes(&self, id: u32) -> bool {
        self.start_tiles.contains_key(&id)
    }

    /// The position and size to draw an item's tile with at the time `now`.
    /// `target` is the tile's position and size in the current layout. A
    /// tile that is not animated is drawn at `target`.
    pub fn tile_at(&self, id: u32, target: Tile, now: f64) -> Tile {
        match self.start_tiles.get(&id) {
            Some(start) => self.between(start, target, now),
            None => target,
        }
    }

    /// The position and size to draw the header's subfolder tile number
    /// `index` with at the time `now`. `target` is its position and size in
    /// the current header.
    pub fn subfolder_tile_at(&self, index: usize, target: Tile, now: f64) -> Tile {
        match self.start_subfolder_tiles.get(index) {
            Some(start) => self.between(start, target, now),
            None => target,
        }
    }

    /// The rectangle between `start` and `target` for the time `now`. Each of
    /// x, y, width and height moves by the same share of its distance, so a
    /// picture keeps its aspect ratio while its tile changes size.
    fn between(&self, start: &Tile, target: Tile, now: f64) -> Tile {
        let progress = crate::motion::zoom_glide_progress(self.elapsed(now));
        let between = |start: f32, target: f32| start + (target - start) * progress;
        Tile {
            x: between(start.x, target.x),
            y: between(start.y, target.y),
            w: between(start.w, target.w),
            h: between(start.h, target.h),
        }
    }

    /// Moves every start position down by `by` pixels. The grid calls this
    /// when another window of the same results arrives during the animation
    /// and the scroll position changes by `by` to keep the tiles where they
    /// are on screen. After the shift the animated tiles still start from
    /// the same places on screen.
    pub fn shift_start_positions(&mut self, by: f32) {
        let tiles = self.start_tiles.values_mut();
        for tile in tiles.chain(&mut self.start_subfolder_tiles) {
            tile.y += by;
        }
    }
}

/// The tile that the last zoom step kept at the same height on screen.
///
/// The next zoom step uses the same tile again if the pointer and the scroll
/// position have not changed since. Without this, each step would pick the
/// tile that is under the pointer after the step before it, which is a
/// different tile because tiles change column. Zooming in three steps and
/// out three steps would then end at a different scroll position than it
/// started from.
pub(super) struct Anchor {
    /// The root id of the tile's item.
    pub id: u32,
    /// How far down the tile the fixed point is: 0 is the tile's top edge
    /// and 1 its bottom edge.
    pub fraction_down: f32,
    /// The point the tile was chosen for, in pixels from the grid's top left
    /// corner: the pointer's position, or the centre of the viewport.
    pub point: (f32, f32),
    /// How far the items were scrolled after the step, in pixels.
    pub scroll_top: f32,
}

/// Chooses the tile a zoom step keeps at the same height on screen. `tiles`
/// are candidate tiles with their index in the layout, in the same
/// coordinates as the point (`x`, `y`). Returns the index of the tile that
/// contains the point, or of the nearest tile when the point is in a gap,
/// and how far down that tile the point is (0 is its top edge, 1 its bottom
/// edge). Returns `None` when there are no tiles.
pub(super) fn nearest_tile(
    tiles: impl Iterator<Item = (usize, Tile)>,
    x: f32,
    y: f32,
) -> Option<(usize, f32)> {
    let squared_distance = |tile: &Tile| {
        let across = (tile.x - x).max(x - (tile.x + tile.w)).max(0.);
        let down = (tile.y - y).max(y - (tile.y + tile.h)).max(0.);
        across * across + down * down
    };
    tiles
        .map(|(index, tile)| (index, tile, squared_distance(&tile)))
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(index, tile, _)| (index, ((y - tile.y) / tile.h.max(1.)).clamp(0., 1.)))
}
