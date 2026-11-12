//! Grid geometry. A direct port of `src/features/grid/layout/layoutMath.ts`, so
//! the same library, window width and view preferences place every tile where
//! the Electron grid places it.

/// The outer selection stroke reaches three pixels beyond the media rect.
pub const SELECTION_EXTENT: f32 = 3.;
pub const TEXT_ROW_HEIGHT: f32 = 20.;
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

#[derive(Default)]
pub struct Layout {
    pub tiles: Vec<Tile>,
    pub total_height: f32,
    pub column_count: usize,
    bands: Vec<Vec<u32>>,
}

struct Geometry {
    used_width: f32,
    column_count: usize,
    column_width: f32,
    offset_x: f32,
    snapped_size: f32,
    item_gap: f32,
}

pub fn safe_aspect_ratio(value: f32) -> f32 {
    if !value.is_finite() || value <= 0. {
        return 1.5;
    }
    value.clamp(0.125, 8.)
}

/// JavaScript's `Math.round`: halves round toward positive infinity.
fn js_round(value: f32) -> f32 {
    (value + 0.5).floor()
}

fn geometry(container_width: f32, target_size: f32, gap: f32) -> Geometry {
    let snapped_size = (js_round(target_size / 50.) * 50.).max(50.) - 2. * SELECTION_EXTENT;
    // Positions describe only the clickable media. The selection stroke lives
    // outside that rect, so reserve its full extent on both sides of every tile.
    let outer_inset = gap + SELECTION_EXTENT;
    let item_gap = gap + 2. * SELECTION_EXTENT;
    let min_inner_width = container_width - 2. * outer_inset;
    let column_count =
        js_round((min_inner_width + item_gap) / (snapped_size + item_gap)).max(1.) as usize;
    let columns = column_count as f32;
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
        aspect_ratios: &[f32],
        container_width: f32,
        target_size: f32,
        gap: f32,
        mode: ViewMode,
        text_height: f32,
    ) -> Self {
        if aspect_ratios.is_empty() || container_width <= 0. {
            return Self::default();
        }
        let geometry = geometry(container_width, target_size, gap);
        let (mut tiles, height) = match mode {
            ViewMode::Grid => grid(aspect_ratios.len(), &geometry, text_height),
            ViewMode::Justified => justified(aspect_ratios, &geometry, text_height),
            ViewMode::Waterfall => waterfall(aspect_ratios, &geometry, text_height),
        };
        for tile in &mut tiles {
            tile.x += geometry.offset_x;
            tile.y += PADDING_TOP;
        }
        let total_height = height + PADDING_TOP + PADDING_BOTTOM;
        let mut bands = vec![Vec::new(); (total_height / BAND) as usize + 1];
        for (index, tile) in tiles.iter().enumerate() {
            let first = (tile.y / BAND).max(0.) as usize;
            let last = (((tile.y + tile.h) / BAND) as usize).min(bands.len() - 1);
            for band in &mut bands[first..=last] {
                band.push(index as u32);
            }
        }
        Self {
            tiles,
            total_height,
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

fn waterfall(aspect_ratios: &[f32], geometry: &Geometry, text_height: f32) -> (Vec<Tile>, f32) {
    let width = geometry.column_width;
    let gap = geometry.item_gap;
    let mut heights = vec![0f32; geometry.column_count];
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
        tiles.push(Tile {
            x: shortest as f32 * (width + gap),
            y,
            w: width,
            h,
        });
        heights[shortest] = y + h + gap;
    }
    let tallest = heights.iter().copied().fold(0., f32::max);
    (tiles, (tallest - gap).max(0.))
}

fn grid(count: usize, geometry: &Geometry, text_height: f32) -> (Vec<Tile>, f32) {
    let size = geometry.column_width;
    let gap = geometry.item_gap;
    let cell_height = size + text_height;
    let tiles = (0..count)
        .map(|index| Tile {
            x: (index % geometry.column_count) as f32 * (size + gap),
            y: (index / geometry.column_count) as f32 * (cell_height + gap),
            w: size,
            h: cell_height,
        })
        .collect();
    let rows = count.div_ceil(geometry.column_count) as f32;
    let height = if rows > 0. {
        rows * cell_height + (rows - 1.) * gap
    } else {
        0.
    };
    (tiles, height)
}

fn justified(aspect_ratios: &[f32], geometry: &Geometry, text_height: f32) -> (Vec<Tile>, f32) {
    let container_width = geometry.used_width;
    let target_row_height = geometry.snapped_size;
    let gap = geometry.item_gap;
    let mut tiles = Vec::with_capacity(aspect_ratios.len());
    let mut row_start = 0;
    let mut y = 0f32;
    while row_start < aspect_ratios.len() {
        let mut row_end = row_start;
        let mut total_aspect = 0f32;
        while row_end < aspect_ratios.len() {
            total_aspect += safe_aspect_ratio(aspect_ratios[row_end]);
            row_end += 1;
            let row_width =
                total_aspect * target_row_height + (row_end - row_start - 1) as f32 * gap;
            if row_width >= container_width {
                break;
            }
        }
        // An incomplete final row keeps the target height. If its final tile is
        // also the tile that crosses the available width, that rule would make it
        // paint beyond the grid. Finish the preceding row and let the tile begin
        // a new final row instead.
        let target_width =
            total_aspect * target_row_height + (row_end - row_start - 1) as f32 * gap;
        if row_end == aspect_ratios.len()
            && row_end - row_start > 1
            && target_width > container_width
        {
            row_end -= 1;
            total_aspect -= safe_aspect_ratio(aspect_ratios[row_end]);
        }
        let last_row = row_end == aspect_ratios.len();
        let gap_space = (row_end - row_start - 1) as f32 * gap;
        let final_height = if last_row {
            target_row_height
        } else {
            ((container_width - gap_space) / total_aspect).min(target_row_height * 1.5)
        };
        let cell_height = final_height + text_height;
        let mut x = 0f32;
        for &aspect in &aspect_ratios[row_start..row_end] {
            let aspect = safe_aspect_ratio(aspect);
            let mut w = final_height * aspect;
            let mut h = cell_height;
            if w > container_width {
                w = container_width;
                h = w / aspect + text_height;
            }
            tiles.push(Tile { x, y, w, h });
            x += w + gap;
        }
        y += cell_height + gap;
        row_start = row_end;
    }
    (tiles, (y - gap).max(0.))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aspects(count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| [0.125, 8., 1., 0.5, 1.5][i % 5])
            .collect()
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
        assert_eq!(safe_aspect_ratio(f32::NAN), 1.5);
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
}
