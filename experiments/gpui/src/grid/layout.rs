//! Grid geometry. A direct port of `src/features/grid/layout/layoutMath.ts`, so
//! the same library, window width and view preferences place every tile where
//! the Electron grid places it.

/// The outer selection stroke reaches three pixels beyond the media rect.
pub const SELECTION_EXTENT: f32 = 3.;
pub const TEXT_ROW_HEIGHT: f32 = 20.;
/// The selection extent in the layout's arithmetic.
const EXTENT: f64 = SELECTION_EXTENT as f64;
const PADDING_TOP: f32 = 16.;
const PADDING_BOTTOM: f32 = 16.;
/// Tiles are looked up by the horizontal bands they touch.
const BAND: f32 = 256.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    Waterfall,
    Justified,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Tile {
    /// A tile from the layout's double-precision arithmetic, which is
    /// `layoutMath.ts`'s own: a column chosen by a near tie lands where
    /// JavaScript's numbers put it.
    fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            x: x as f32,
            y: y as f32,
            w: w as f32,
            h: h as f32,
        }
    }
}

#[derive(Default)]
pub struct Layout {
    pub tiles: Vec<Tile>,
    pub total_height: f32,
    /// Read by the geometry tests; navigation computes its own step.
    #[cfg(test)]
    pub column_count: usize,
    bands: Vec<Vec<u32>>,
}

struct Geometry {
    used_width: f64,
    column_count: usize,
    column_width: f64,
    offset_x: f64,
    snapped_size: f64,
    item_gap: f64,
}

pub fn safe_aspect_ratio(value: f64) -> f64 {
    if !value.is_finite() || value <= 0. {
        return 1.5;
    }
    value.clamp(0.125, 8.)
}

/// JavaScript's `Math.round`: halves round toward positive infinity.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn geometry(container_width: f64, target_size: f64, gap: f64) -> Geometry {
    let snapped_size = (js_round(target_size / 50.) * 50.).max(50.) - 2. * EXTENT;
    // Positions describe only the clickable media. The selection stroke lives
    // outside that rect, so reserve its full extent on both sides of every tile.
    let outer_inset = gap + EXTENT;
    let item_gap = gap + 2. * EXTENT;
    let min_inner_width = container_width - 2. * outer_inset;
    let column_count =
        js_round((min_inner_width + item_gap) / (snapped_size + item_gap)).max(1.) as usize;
    let columns = column_count as f64;
    let column_width = ((min_inner_width - (columns - 1.) * item_gap) / columns).floor();
    let used_width = columns * column_width + (columns - 1.) * item_gap;
    Geometry {
        used_width,
        column_count,
        column_width,
        offset_x: ((container_width - used_width) / 2.).floor(),
        snapped_size,
        item_gap,
    }
}

impl Layout {
    pub fn compute(
        aspect_ratios: &[f64],
        container_width: f32,
        target_size: f32,
        gap: f32,
        mode: ViewMode,
        text_height: f32,
    ) -> Self {
        if aspect_ratios.is_empty() || container_width <= 0. {
            return Self::default();
        }
        let geometry = geometry(container_width.into(), target_size.into(), gap.into());
        // A pane narrower than the gaps around one column has no room for a
        // tile.
        if geometry.column_width <= 0. {
            return Self::default();
        }
        let text_height = f64::from(text_height);
        let (mut tiles, height) = match mode {
            ViewMode::Grid => grid(aspect_ratios.len(), &geometry, text_height),
            ViewMode::Justified => justified(aspect_ratios, &geometry, text_height),
            ViewMode::Waterfall => waterfall(aspect_ratios, &geometry, text_height),
        };
        for tile in &mut tiles {
            tile.x += geometry.offset_x as f32;
            tile.y += PADDING_TOP;
        }
        let total_height = height as f32 + PADDING_TOP + PADDING_BOTTOM;
        let mut bands = vec![Vec::new(); (total_height / BAND) as usize + 1];
        for (index, tile) in tiles.iter().enumerate() {
            let first = (tile.y / BAND).max(0.) as usize;
            let last = (((tile.y + tile.h) / BAND) as usize)
                .min(bands.len() - 1)
                .max(first);
            for band in &mut bands[first..=last] {
                band.push(index as u32);
            }
        }
        Self {
            tiles,
            total_height,
            #[cfg(test)]
            column_count: geometry.column_count,
            bands,
        }
    }

    /// Indexes of the tiles touching `top..=bottom`, in item order.
    pub fn visible(&self, top: f32, bottom: f32) -> Vec<usize> {
        if self.bands.is_empty() || bottom < 0. {
            return Vec::new();
        }
        let first = ((top / BAND).max(0.) as usize).min(self.bands.len() - 1);
        let last = ((bottom / BAND).max(0.) as usize).min(self.bands.len() - 1);
        let mut visible = self.bands[first..=last]
            .iter()
            .flatten()
            .map(|&index| index as usize)
            .filter(|&index| {
                let tile = &self.tiles[index];
                tile.y + tile.h >= top && tile.y <= bottom
            })
            .collect::<Vec<_>>();
        visible.sort_unstable();
        visible.dedup();
        visible
    }
}

fn waterfall(aspect_ratios: &[f64], geometry: &Geometry, text_height: f64) -> (Vec<Tile>, f64) {
    let width = geometry.column_width;
    let gap = geometry.item_gap;
    let mut heights = vec![0f64; geometry.column_count];
    let mut tiles = Vec::with_capacity(aspect_ratios.len());
    for &aspect in aspect_ratios {
        let mut shortest = 0;
        for column in 1..heights.len() {
            if heights[column] < heights[shortest] {
                shortest = column;
            }
        }
        let y = heights[shortest];
        let h = width / safe_aspect_ratio(aspect) + text_height;
        tiles.push(Tile::new(shortest as f64 * (width + gap), y, width, h));
        heights[shortest] = y + h + gap;
    }
    let tallest = heights.iter().copied().fold(0., f64::max);
    (tiles, (tallest - gap).max(0.))
}

fn grid(count: usize, geometry: &Geometry, text_height: f64) -> (Vec<Tile>, f64) {
    let size = geometry.column_width;
    let gap = geometry.item_gap;
    let cell_height = size + text_height;
    let tiles = (0..count)
        .map(|index| {
            Tile::new(
                (index % geometry.column_count) as f64 * (size + gap),
                (index / geometry.column_count) as f64 * (cell_height + gap),
                size,
                cell_height,
            )
        })
        .collect();
    let rows = count.div_ceil(geometry.column_count) as f64;
    let height = if rows > 0. {
        rows * cell_height + (rows - 1.) * gap
    } else {
        0.
    };
    (tiles, height)
}

fn justified(aspect_ratios: &[f64], geometry: &Geometry, text_height: f64) -> (Vec<Tile>, f64) {
    let container_width = geometry.used_width;
    let target_row_height = geometry.snapped_size;
    let gap = geometry.item_gap;
    let mut tiles = Vec::with_capacity(aspect_ratios.len());
    let mut row_start = 0;
    let mut y = 0f64;
    while row_start < aspect_ratios.len() {
        let mut row_end = row_start;
        let mut total_aspect = 0f64;
        while row_end < aspect_ratios.len() {
            total_aspect += safe_aspect_ratio(aspect_ratios[row_end]);
            row_end += 1;
            let row_width =
                total_aspect * target_row_height + (row_end - row_start - 1) as f64 * gap;
            if row_width >= container_width {
                break;
            }
        }
        // An incomplete final row keeps the target height. If its final tile is
        // also the tile that crosses the available width, that rule would make it
        // paint beyond the grid. Finish the preceding row and let the tile begin
        // a new final row instead.
        let target_width =
            total_aspect * target_row_height + (row_end - row_start - 1) as f64 * gap;
        if row_end == aspect_ratios.len()
            && row_end - row_start > 1
            && target_width > container_width
        {
            row_end -= 1;
            total_aspect -= safe_aspect_ratio(aspect_ratios[row_end]);
        }
        let last_row = row_end == aspect_ratios.len();
        let gap_space = (row_end - row_start - 1) as f64 * gap;
        let final_height = if last_row {
            target_row_height
        } else {
            ((container_width - gap_space) / total_aspect).min(target_row_height * 1.5)
        };
        let cell_height = final_height + text_height;
        let mut x = 0f64;
        for &aspect in &aspect_ratios[row_start..row_end] {
            let aspect = safe_aspect_ratio(aspect);
            let mut w = final_height * aspect;
            let mut h = cell_height;
            if w > container_width {
                w = container_width;
                h = w / aspect + text_height;
            }
            tiles.push(Tile::new(x, y, w, h));
            x += w + gap;
        }
        y += cell_height + gap;
        row_start = row_end;
    }
    (tiles, (y - gap).max(0.))
}

/// `resolveGridScrollAnchor`: where to scroll once the layout changes so
/// the topmost visible tile stays where it was on screen. A selected tile in
/// view is preferred. The anchor is followed by its item, so the items may
/// have changed too (a refresh in place): `moved` says where a previous
/// tile's item is now, if it still is (`Some` when the items are the same).
/// `selected` are positions in the previous tiles.
pub fn scroll_anchor(
    previous: &[Tile],
    next: &[Tile],
    selected: &std::collections::BTreeSet<usize>,
    scroll_top: f32,
    viewport: f32,
    moved: impl Fn(usize) -> Option<usize>,
) -> Option<f32> {
    let bottom = scroll_top + viewport;
    let topmost = |only_selected: bool| {
        previous
            .iter()
            .enumerate()
            .filter(|(index, _)| !only_selected || selected.contains(index))
            .filter(|(_, tile)| tile.y + tile.h >= scroll_top && tile.y <= bottom)
            .fold(None::<(usize, f32)>, |best, (index, tile)| match best {
                Some((_, top)) if top <= tile.y => best,
                _ => Some((index, tile.y)),
            })
    };
    let (anchor, top) = (!selected.is_empty())
        .then(|| topmost(true))
        .flatten()
        .or_else(|| topmost(false))?;
    let next = next.get(moved(anchor)?)?;
    Some((next.y - (top - scroll_top)).max(0.))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aspects(count: usize) -> Vec<f64> {
        (0..count)
            .map(|i| [0.125, 8., 1., 0.5, 1.5][i % 5])
            .collect()
    }

    #[test]
    fn a_pane_too_narrow_for_a_tile_lays_out_nothing() {
        for mode in [ViewMode::Grid, ViewMode::Waterfall, ViewMode::Justified] {
            for width in [1., 20., 33.] {
                let layout = Layout::compute(&aspects(500), width, 150., 16., mode, 40.);
                assert!(layout.tiles.iter().all(|tile| tile.w > 0. && tile.h > 0.));
                // Looking tiles up never fails.
                layout.visible(0., 10_000.);
            }
        }
    }

    #[test]
    fn column_count_follows_the_snapped_target_size() {
        // 1000px wide, tight spacing: inset 11, item gap 14, snapped 250 - 6.
        let layout = Layout::compute(&aspects(40), 1000., 260., 8., ViewMode::Waterfall, 0.);
        assert_eq!(layout.column_count, 4);
        let width = layout.tiles[0].w;
        assert_eq!(width, ((1000. - 22. - 3. * 14.) / 4f32).floor());
        // The used width is centred in the container.
        let used = 4. * width + 3. * 14.;
        assert_eq!(layout.tiles[0].x, ((1000. - used) / 2.).floor());
        assert_eq!(layout.tiles[0].y, 16.);
    }

    #[test]
    fn waterfall_fills_the_shortest_column_and_keeps_aspect() {
        let layout = Layout::compute(&[1., 0.5, 2., 1.], 700., 200., 16., ViewMode::Waterfall, 0.);
        assert_eq!(layout.column_count, 3);
        let width = layout.tiles[0].w;
        assert_eq!(layout.tiles[1].h, width / 0.5);
        // The fourth tile goes under the shortest column: the wide third tile.
        assert_eq!(layout.tiles[3].x, layout.tiles[2].x);
        assert_eq!(
            layout.tiles[3].y,
            layout.tiles[2].y + layout.tiles[2].h + 16. + 2. * SELECTION_EXTENT
        );
    }

    #[test]
    fn grid_rows_are_square_cells_plus_text() {
        let layout = Layout::compute(&aspects(7), 700., 200., 16., ViewMode::Grid, 20.);
        assert_eq!(layout.column_count, 3);
        let size = layout.tiles[0].w;
        assert!(
            layout
                .tiles
                .iter()
                .all(|t| t.w == size && t.h == size + 20.)
        );
        assert_eq!(layout.tiles[3].y, layout.tiles[0].y + size + 20. + 22.);
        assert_eq!(layout.total_height, 3. * (size + 20.) + 2. * 22. + 32.);
    }

    #[test]
    fn justified_rows_fill_the_width_except_the_last() {
        let ratios = vec![1.5; 11];
        let layout = Layout::compute(&ratios, 1000., 200., 8., ViewMode::Justified, 0.);
        let first_row = layout
            .tiles
            .iter()
            .filter(|t| t.y == layout.tiles[0].y)
            .collect::<Vec<_>>();
        let right = first_row.last().map(|t| t.x + t.w).unwrap();
        let left = first_row[0].x;
        // Five 184px columns and four 14px gaps: the used width is 976.
        assert!(
            (right - left - 976.).abs() < 0.01,
            "row spans the used width"
        );
        // The incomplete last row keeps the target height: 200 snapped, less the stroke.
        assert_eq!(layout.tiles.last().unwrap().h, 194.);
    }

    #[test]
    fn extreme_and_invalid_aspect_ratios_are_bounded() {
        assert_eq!(safe_aspect_ratio(0.), 1.5);
        assert_eq!(safe_aspect_ratio(f64::NAN), 1.5);
        assert_eq!(safe_aspect_ratio(100.), 8.);
        assert_eq!(safe_aspect_ratio(0.001), 0.125);
    }

    #[test]
    fn banded_visibility_matches_a_full_scan_in_every_mode() {
        let ratios = aspects(20_000);
        for mode in [ViewMode::Waterfall, ViewMode::Grid, ViewMode::Justified] {
            let layout = Layout::compute(&ratios, 1000., 200., 8., mode, 20.);
            for top in (0..100_000).step_by(137) {
                let top = top as f32;
                let expected = layout
                    .tiles
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| (t.y + t.h >= top && t.y <= top + 1800.).then_some(i))
                    .collect::<Vec<_>>();
                assert_eq!(
                    layout.visible(top, top + 1800.),
                    expected,
                    "{mode:?} at {top}"
                );
            }
        }
    }

    #[test]
    fn a_relayout_keeps_the_topmost_visible_tile_in_place() {
        let tile = |y: f32| Tile {
            x: 0.,
            y,
            w: 100.,
            h: 100.,
        };
        let previous = [tile(0.), tile(120.), tile(240.), tile(360.)];
        let next = [tile(0.), tile(200.), tile(400.), tile(600.)];
        let none = std::collections::BTreeSet::new();
        // Scrolled 150px: tile 1 (top 120) is the topmost in view, 30px above
        // the viewport; it stays 30px above it.
        assert_eq!(
            scroll_anchor(&previous, &next, &none, 150., 200., Some),
            Some(230.)
        );
        // A selected tile in view is preferred.
        let selected = [2].into_iter().collect();
        assert_eq!(
            scroll_anchor(&previous, &next, &selected, 150., 200., Some),
            Some(310.)
        );
        // Nothing laid out before: nothing to keep.
        assert_eq!(scroll_anchor(&[], &next, &none, 0., 200., Some), None);
    }

    #[test]
    fn a_refresh_keeps_the_topmost_item_where_it_was_by_following_it() {
        let tile = |y: f32| Tile {
            x: 0.,
            y,
            w: 100.,
            h: 100.,
        };
        // Items 0..3 at 0, 110, 220; two new items arrive above them, so the
        // old item 1 (topmost in view at 150) is now the fourth tile.
        let previous = [tile(0.), tile(110.), tile(220.)];
        let next = [tile(0.), tile(110.), tile(220.), tile(330.), tile(440.)];
        let none = std::collections::BTreeSet::new();
        assert_eq!(
            scroll_anchor(&previous, &next, &none, 150., 200., |index| Some(index + 2)),
            // It was 40px above the viewport's top, and stays so.
            Some(370.)
        );
        // An anchor that is gone keeps nothing.
        assert_eq!(
            scroll_anchor(&previous, &next, &none, 150., 200., |_| None),
            None
        );
    }
}
