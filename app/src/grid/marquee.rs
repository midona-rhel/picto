//! The rubber band selection, and the pointer geometry it shares with
//! hovering: which image is under a point, and where a tile's magnifier
//! corner is. Coordinates are layout pixels: x is measured from the grid's
//! left edge and y from the top of the scrolled content.
use super::layout::{Layout, Tile};
use std::collections::BTreeSet;

/// The rubber band's rectangle is drawn, and selects images, once its width
/// or its height is at least this many pixels.
pub const THRESHOLD: f32 = 5.;
/// The grid scrolls while the pointer of a rubber band is within this many
/// pixels of the grid's top or bottom edge.
pub const EDGE_ZONE: f32 = 50.;
/// The number of pixels the grid scrolls in each frame when the pointer is
/// exactly on the edge.
pub const MAX_SPEED: f32 = 12.;
/// The width and the height of the magnifier corner, which is the area at
/// the bottom right of a hovered image that holds the magnifier icon.
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
    /// Returns the rectangle between the point of the press, `start`, and
    /// the point where the pointer is, `end`.
    pub fn spanning(start: (f32, f32), end: (f32, f32)) -> Self {
        Self {
            left: start.0.min(end.0),
            top: start.1.min(end.1),
            width: (end.0 - start.0).abs(),
            height: (end.1 - start.1).abs(),
        }
    }

    /// Whether the rectangle's width or height is at least `THRESHOLD`.
    pub fn reaches_threshold(&self) -> bool {
        self.width >= THRESHOLD || self.height >= THRESHOLD
    }
}

/// A rubber band selection in progress.
pub struct Marquee {
    /// The point of the press, in layout pixels.
    pub start: (f32, f32),
    /// The pointer's position measured from the grid's top-left corner. The
    /// distance the grid is scrolled is not added to it.
    pub pointer: (f32, f32),
    /// The indexes of the tiles that were selected before the press, when
    /// the rubber band adds to the selection. It is empty when the rubber
    /// band replaces the selection.
    pub base: BTreeSet<usize>,
    /// The indexes of the tiles whose images the rectangle overlaps.
    pub hits: BTreeSet<usize>,
    /// The rectangle, once it has reached `THRESHOLD`. It keeps its last
    /// size when the pointer comes back closer to the press than that.
    pub rect: Option<Rect>,
    /// The number of pixels the grid scrolls in each frame, which depends
    /// on the pointer's distance from the grid's top or bottom edge.
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

    /// Computes the rectangle and the tiles it overlaps again, for the
    /// stored pointer position and the distance `scroll_top` that the grid
    /// is scrolled.
    pub fn track(&mut self, scroll_top: f32, layout: &Layout, text_height: f32) {
        let rect = Rect::spanning(self.start, (self.pointer.0, self.pointer.1 + scroll_top));
        if rect.reaches_threshold() && self.rect != Some(rect) {
            self.hits = hits(layout, rect, text_height);
            self.rect = Some(rect);
        }
    }

    /// Whether the tile at `index` is drawn as selected during the rubber
    /// band. Once the rectangle is drawn, these are the tiles of `base` and
    /// the tiles the rectangle overlaps. Before that, these are the tiles
    /// of `selection`, the selection from before the press.
    pub fn shows_selected(&self, index: usize, selection: &BTreeSet<usize>) -> bool {
        if self.rect.is_some() {
            self.base.contains(&index) || self.hits.contains(&index)
        } else {
            selection.contains(&index)
        }
    }

    /// Returns the selection that the release of the button leaves: the
    /// tiles of `base` and the tiles the rectangle overlaps. Returns `None`
    /// when the rectangle never reached `THRESHOLD`.
    pub fn committed(self) -> Option<BTreeSet<usize>> {
        self.rect?;
        let mut selection = self.base;
        selection.extend(self.hits);
        Some(selection)
    }
}

/// Returns the indexes of the tiles whose image the rectangle overlaps. The
/// text rows under an image are not part of the image.
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

/// Returns the index of the tile whose image is under the point (`x`, `y`).
/// A point on the text rows under an image is on no image.
pub fn image_at(layout: &Layout, x: f32, y: f32, text_height: f32) -> Option<usize> {
    layout.visible(y, y).into_iter().find(|&index| {
        let tile = &layout.tiles[index];
        x >= tile.x && x <= tile.x + tile.w && y >= tile.y && y <= tile.y + tile.h - text_height
    })
}

/// Whether the point (`x`, `y`) is on the magnifier corner of `tile`.
pub fn on_magnifier(tile: &Tile, x: f32, y: f32, text_height: f32) -> bool {
    let left = tile.x + tile.w - MAGNIFIER_WIDTH;
    let top = tile.y + tile.h - text_height - MAGNIFIER_HEIGHT;
    x >= left && x < left + MAGNIFIER_WIDTH && y >= top && y < top + MAGNIFIER_HEIGHT
}

/// Returns how many pixels the grid scrolls in each frame for a pointer `y`
/// pixels below the grid's top edge. The speed is 0 further than `EDGE_ZONE`
/// from both edges and grows the nearer the pointer is to an edge. It keeps
/// growing when the pointer is outside the grid.
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

    /// Returns the layout of twelve square items in the grid mode, four in a
    /// row, each with a text row of 20 px under the image.
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
        // A thin rectangle across the text rows of the first row selects
        // nothing.
        let text_rows = Rect {
            left: 0.,
            top: image_bottom + 1.,
            width: 840.,
            height: 15.,
        };
        assert!(hits(&layout, text_rows, 20.).is_empty());
        // A rectangle that reaches one pixel into the images selects the
        // whole row.
        let into_images = Rect {
            top: image_bottom - 1.,
            ..text_rows
        };
        let row = hits(&layout, into_images, 20.);
        assert_eq!(
            row.len(),
            layout.tiles.iter().filter(|t| t.y == first.y).count()
        );
        // A rectangle that only touches an image's edge does not overlap it.
        let edge = Rect {
            left: first.x + first.w,
            top: first.y,
            width: 1.,
            height: 10.,
        };
        assert!(!hits(&layout, edge, 20.).contains(&0));
        // A rectangle in the gap between two images selects neither.
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
        // The speed has no upper limit when the pointer is outside the grid.
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
        // After a press without a move, the selection is shown as it was.
        marquee.pointer = (first.x - 2., first.y - 2.);
        marquee.track(0., &layout, 20.);
        assert!(marquee.rect.is_none());
        assert!(marquee.shows_selected(3, &selection));
        // A move into the first image selects it together with the kept tile.
        marquee.pointer = (first.x + 10., first.y + 10.);
        marquee.track(0., &layout, 20.);
        assert!(marquee.shows_selected(0, &selection));
        assert!(marquee.shows_selected(11, &selection));
        assert!(!marquee.shows_selected(3, &selection));
        // When the grid scrolls down while the pointer stays where it is, the
        // rectangle reaches the next row.
        marquee.track(first.h + 20., &layout, 20.);
        let next_row = layout
            .tiles
            .iter()
            .position(|tile| tile.y > first.y && tile.x == first.x)
            .unwrap();
        assert!(marquee.shows_selected(next_row, &selection));
        assert_eq!(marquee.committed(), Some(BTreeSet::from([0, next_row, 11])));
        // A rubber band whose pointer never moved leaves no selection.
        let still = Marquee::new((0., 0.), (0., 0.), BTreeSet::from([1]));
        assert_eq!(still.committed(), None);
    }
}
