//! Rubber-band selection and the pointer geometry it shares with hovering, as
//! `CanvasGrid.tsx` and `hitTesting.ts` define them. Coordinates are layout
//! pixels: x from the grid's left edge, y from the top of the scrolled content.
use super::layout::{Layout, Tile};
use std::collections::BTreeSet;

/// The rectangle shows, and selects, once either side is this long.
pub const THRESHOLD: f32 = 5.;
/// Within this distance of the grid's top or bottom edge the grid scrolls.
pub const EDGE_ZONE: f32 = 50.;
/// The scroll step, in pixels per frame, at the very edge.
pub const MAX_SPEED: f32 = 12.;
/// The magnifier corner of a hovered image (`ZOOM_BTN_SIZE` + 4 by + 2).
pub const MAGNIFIER_WIDTH: f32 = 28.;
pub const MAGNIFIER_HEIGHT: f32 = 26.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// The rectangle between where the press started and where the pointer is.
    pub fn spanning(start: (f32, f32), end: (f32, f32)) -> Self {
        Self {
            left: start.0.min(end.0),
            top: start.1.min(end.1),
            width: (end.0 - start.0).abs(),
            height: (end.1 - start.1).abs(),
        }
    }

    /// Whether the pointer has moved far enough for the rectangle to count.
    pub fn reaches_threshold(&self) -> bool {
        self.width >= THRESHOLD || self.height >= THRESHOLD
    }
}

/// A rubber band in progress.
pub struct Marquee {
    /// Where the press landed, in layout pixels.
    pub start: (f32, f32),
    /// The pointer relative to the grid's top-left corner, not scrolled.
    pub pointer: (f32, f32),
    /// The selection kept from before the press (Shift or Cmd held).
    pub base: BTreeSet<usize>,
    /// The images under the rectangle.
    pub hits: BTreeSet<usize>,
    /// The rectangle once it has reached the threshold. It stays at its last
    /// size if the pointer comes back closer than that.
    pub rect: Option<Rect>,
    /// The auto-scroll step for the pointer's distance from an edge.
    pub speed: f32,
}

impl Marquee {
    pub fn new(start: (f32, f32), pointer: (f32, f32), base: BTreeSet<usize>) -> Self {
        Self {
            start,
            pointer,
            base,
            hits: BTreeSet::new(),
            rect: None,
            speed: 0.,
        }
    }

    /// Follows the pointer at the grid's current scroll position.
    pub fn track(&mut self, scroll_top: f32, layout: &Layout, text_height: f32) {
        let rect = Rect::spanning(self.start, (self.pointer.0, self.pointer.1 + scroll_top));
        if rect.reaches_threshold() && self.rect != Some(rect) {
            self.hits = hits(layout, rect, text_height);
            self.rect = Some(rect);
        }
    }

    /// Whether the tile shows as selected while the band is out: the kept
    /// selection and what the rectangle covers. Before the rectangle shows,
    /// the selection as it was.
    pub fn shows_selected(&self, index: usize, selection: &BTreeSet<usize>) -> bool {
        if self.rect.is_some() {
            self.base.contains(&index) || self.hits.contains(&index)
        } else {
            selection.contains(&index)
        }
    }

    /// What the selection becomes on release, if the rectangle ever showed.
    pub fn committed(self) -> Option<BTreeSet<usize>> {
        self.rect?;
        let mut selection = self.base;
        selection.extend(self.hits);
        Some(selection)
    }
}

/// The images whose rect (not the text rows under them) the rectangle
/// overlaps, as `collectMarqueeHits` tests them.
pub fn hits(layout: &Layout, rect: Rect, text_height: f32) -> BTreeSet<usize> {
    layout
        .visible(rect.top, rect.top + rect.height)
        .into_iter()
        .filter(|&index| {
            let tile = &layout.tiles[index];
            let image_height = tile.h - text_height;
            tile.x + tile.w > rect.left
                && tile.x < rect.left + rect.width
                && tile.y + image_height > rect.top
                && tile.y < rect.top + rect.height
        })
        .collect()
}

/// The image under a point, as `hitTestTile` finds it: the text rows under an
/// image are empty space.
pub fn image_at(layout: &Layout, x: f32, y: f32, text_height: f32) -> Option<usize> {
    layout.visible(y, y).into_iter().find(|&index| {
        let tile = &layout.tiles[index];
        x >= tile.x && x <= tile.x + tile.w && y >= tile.y && y <= tile.y + tile.h - text_height
    })
}

/// Whether a point on a tile is on its magnifier corner (`isZoomButtonHit`).
pub fn on_magnifier(tile: &Tile, x: f32, y: f32, text_height: f32) -> bool {
    let left = tile.x + tile.w - MAGNIFIER_WIDTH;
    let top = tile.y + tile.h - text_height - MAGNIFIER_HEIGHT;
    x >= left && x < left + MAGNIFIER_WIDTH && y >= top && y < top + MAGNIFIER_HEIGHT
}

/// How far the grid scrolls each frame for a pointer `y` pixels below the
/// grid's top edge: faster the nearer the edge, and faster still past it.
pub fn auto_scroll_speed(y: f32, viewport_height: f32) -> f32 {
    if y < EDGE_ZONE {
        -MAX_SPEED * (1. - y / EDGE_ZONE)
    } else if y > viewport_height - EDGE_ZONE {
        MAX_SPEED * (1. - (viewport_height - y) / EDGE_ZONE)
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::super::layout::{Layout, ViewMode};
    use super::{Marquee, Rect, Tile, auto_scroll_speed, hits, image_at, on_magnifier};
    use std::collections::BTreeSet;

    /// Twelve square items in the grid mode, four to a row, each with a 20px
    /// text row under the image.
    fn grid() -> Layout {
        Layout::compute(&[1.; 12], 840., 200., 14., ViewMode::Grid, 20.)
    }

    #[test]
    fn the_rectangle_spans_the_press_and_the_pointer_either_way_round() {
        let rect = Rect::spanning((100., 300.), (40., 250.));
        assert_eq!(
            rect,
            Rect {
                left: 40.,
                top: 250.,
                width: 60.,
                height: 50.
            }
        );
        assert!(!Rect::spanning((10., 10.), (14., 14.)).reaches_threshold());
        assert!(Rect::spanning((10., 10.), (15., 11.)).reaches_threshold());
        assert!(Rect::spanning((10., 10.), (10., 5.)).reaches_threshold());
    }

    #[test]
    fn only_the_image_rect_is_hit_not_the_text_row_or_the_gap() {
        let layout = grid();
        let first = layout.tiles[0];
        let image_bottom = first.y + first.h - 20.;
        // A thin band across the first row's text rows selects nothing.
        let text_rows = Rect {
            left: 0.,
            top: image_bottom + 1.,
            width: 840.,
            height: 15.,
        };
        assert!(hits(&layout, text_rows, 20.).is_empty());
        // Reaching one pixel into the image selects the whole row.
        let into_images = Rect {
            top: image_bottom - 1.,
            ..text_rows
        };
        let row = hits(&layout, into_images, 20.);
        assert_eq!(
            row.len(),
            layout.tiles.iter().filter(|t| t.y == first.y).count()
        );
        // Touching an edge exactly is not overlapping.
        let edge = Rect {
            left: first.x + first.w,
            top: first.y,
            width: 1.,
            height: 10.,
        };
        assert!(!hits(&layout, edge, 20.).contains(&0));
        // The gap between two images selects neither.
        let second = layout.tiles[1];
        let gap = Rect {
            left: first.x + first.w + 1.,
            top: first.y,
            width: second.x - first.x - first.w - 2.,
            height: 50.,
        };
        assert!(hits(&layout, gap, 20.).is_empty());
    }

    #[test]
    fn a_tall_rectangle_selects_every_row_it_crosses() {
        let layout = grid();
        let first = layout.tiles[0];
        let column: BTreeSet<usize> = layout
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, tile)| tile.x == first.x)
            .map(|(index, _)| index)
            .collect();
        let rect = Rect {
            left: first.x + 10.,
            top: 0.,
            width: 10.,
            height: layout.total_height,
        };
        assert_eq!(hits(&layout, rect, 20.), column);
    }

    #[test]
    fn a_press_on_a_text_row_is_on_empty_space() {
        let layout = grid();
        let tile = layout.tiles[0];
        assert_eq!(image_at(&layout, tile.x + 5., tile.y + 5., 20.), Some(0));
        assert_eq!(
            image_at(&layout, tile.x + 5., tile.y + tile.h - 20., 20.),
            Some(0)
        );
        assert_eq!(
            image_at(&layout, tile.x + 5., tile.y + tile.h - 10., 20.),
            None
        );
        assert_eq!(image_at(&layout, tile.x - 2., tile.y + 5., 20.), None);
    }

    #[test]
    fn the_magnifier_is_the_bottom_right_corner_of_the_image() {
        let tile = Tile {
            x: 100.,
            y: 50.,
            w: 200.,
            h: 220.,
        };
        // The image ends 20px above the tile's bottom.
        assert!(on_magnifier(&tile, 299., 249., 20.));
        assert!(on_magnifier(&tile, 272., 224., 20.));
        assert!(!on_magnifier(&tile, 271., 240., 20.));
        assert!(!on_magnifier(&tile, 290., 223., 20.));
        assert!(!on_magnifier(&tile, 290., 250., 20.));
    }

    #[test]
    fn auto_scroll_quickens_toward_each_edge() {
        assert_eq!(auto_scroll_speed(300., 600.), 0.);
        assert_eq!(auto_scroll_speed(50., 600.), 0.);
        assert_eq!(auto_scroll_speed(550., 600.), 0.);
        assert_eq!(auto_scroll_speed(25., 600.), -6.);
        assert_eq!(auto_scroll_speed(0., 600.), -12.);
        assert_eq!(auto_scroll_speed(575., 600.), 6.);
        assert_eq!(auto_scroll_speed(600., 600.), 12.);
        // Past the edge, as the Electron grid does, it does not cap.
        assert_eq!(auto_scroll_speed(-50., 600.), -24.);
        assert_eq!(auto_scroll_speed(650., 600.), 24.);
    }

    #[test]
    fn the_rectangle_follows_the_scroll_and_keeps_the_base_selection() {
        let layout = grid();
        let first = layout.tiles[0];
        let base = BTreeSet::from([11]);
        let mut marquee = Marquee::new((first.x - 4., first.y - 4.), (0., 0.), base);
        let selection = BTreeSet::from([11, 3]);
        // A press without moving shows the selection as it was.
        marquee.pointer = (first.x - 2., first.y - 2.);
        marquee.track(0., &layout, 20.);
        assert!(marquee.rect.is_none());
        assert!(marquee.shows_selected(3, &selection));
        // Moving into the first image selects it alongside the kept one.
        marquee.pointer = (first.x + 10., first.y + 10.);
        marquee.track(0., &layout, 20.);
        assert!(marquee.shows_selected(0, &selection));
        assert!(marquee.shows_selected(11, &selection));
        assert!(!marquee.shows_selected(3, &selection));
        // Scrolling down with the pointer still reaches the next row.
        marquee.track(first.h + 20., &layout, 20.);
        let next_row = layout
            .tiles
            .iter()
            .position(|tile| tile.y > first.y && tile.x == first.x)
            .unwrap();
        assert!(marquee.shows_selected(next_row, &selection));
        assert_eq!(marquee.committed(), Some(BTreeSet::from([0, next_row, 11])));
        // A press that never moved commits nothing.
        let still = Marquee::new((0., 0.), (0., 0.), BTreeSet::from([1]));
        assert_eq!(still.committed(), None);
    }
}
