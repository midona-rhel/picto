//! Computes the position and size of each tile of the grid in the three
//! layout modes: grid, waterfall and justified.
//!
//! The number of columns is counted from the zoom's tile size as it is,
//! without rounding the size first, and one step of the zoom adds or removes
//! exactly one column ([`zoom`], [`zoom_step`]). If the tile size were
//! rounded to a multiple of 50 px before the columns are counted, some steps
//! of the zoom would not change the layout and other steps would skip a
//! number of columns.

/// The selection stroke around a selected tile extends this many pixels beyond
/// the picture's rectangle on every side.
pub const SELECTION_EXTENT: f32 = 3.;
pub const TEXT_ROW_HEIGHT: f32 = 20.;
/// `SELECTION_EXTENT` as an `f64`, the type the layout computes in.
const EXTENT: f64 = SELECTION_EXTENT as f64;
const PADDING_TOP: f32 = 16.;
const PADDING_BOTTOM: f32 = 16.;
/// The layout divides its height into horizontal bands of this many pixels and
/// records which tiles touch each band. [`Layout::visible`] then looks only at
/// the bands the viewport covers, and not at every tile.
const BAND: f32 = 256.;
/// The smallest tile size the zoom allows, in pixels. The largest number of
/// columns at a given width is the number of columns that tiles of this size
/// give.
pub const SMALLEST_TILE: f32 = 150.;
/// The largest row height the zoom allows in the justified layout, in
/// pixels. The justified layout has a target row height instead of columns.
pub const TALLEST_ROW: f32 = 900.;
/// In the justified layout, one step of the zoom multiplies or divides the
/// row height by this factor.
const ROW_HEIGHT_STEP_FACTOR: f32 = 1.1;

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
    /// Builds a tile from `f64` values and stores them as `f32`. The layout
    /// computes in `f64` so that rounding errors do not decide which of two
    /// waterfall columns of almost the same height a tile goes into.
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
    /// The number of columns the layout was computed with. Only the tests in
    /// this file read it. Keyboard movement gets the number from
    /// [`keyboard_step`].
    #[cfg(test)]
    pub column_count: usize,
    bands: Vec<Vec<u32>>,
}

struct Geometry {
    used_width: f64,
    column_count: usize,
    column_width: f64,
    offset_x: f64,
    /// The zoom's tile size minus the 3 px of selection stroke on each side:
    /// the width, or in the justified layout the row height, that the
    /// picture itself is asked to have.
    target_media_size: f64,
    item_gap: f64,
}

pub fn safe_aspect_ratio(value: f64) -> f64 {
    if !value.is_finite() || value <= 0. {
        return 1.5;
    }
    value.clamp(0.125, 8.)
}

/// Rounds `value` to the nearest whole number. A value exactly halfway
/// between two whole numbers is rounded up, toward positive infinity, as
/// JavaScript's `Math.round` does. `f64::round` rounds such a value away
/// from zero instead.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// Returns the number of columns whose tiles are closest to `target_size` in
/// a grid `container_width` wide. The result is at least one and is not
/// limited by [`most_columns`]. Each column takes `target_size + gap` pixels,
/// because `target_size` includes the selection stroke around the picture.
fn columns_that_fit(container_width: f64, target_size: f64, gap: f64) -> f64 {
    js_round((container_width - gap) / (target_size + gap)).max(1.)
}

/// Returns the largest number of columns the zoom allows in a grid
/// `container_width` wide, which is the number that tiles of
/// [`SMALLEST_TILE`] give.
pub fn most_columns(container_width: f32, gap: f32) -> usize {
    columns_that_fit(container_width.into(), SMALLEST_TILE.into(), gap.into()) as usize
}

/// Returns the number of columns the grid and waterfall layouts have in a
/// grid `container_width` wide at the tile size `target_size`. The result is
/// between one and [`most_columns`].
pub fn columns(container_width: f32, target_size: f32, gap: f32) -> usize {
    (columns_that_fit(container_width.into(), target_size.into(), gap.into()) as usize)
        .min(most_columns(container_width, gap))
}

/// Returns the tile size at which exactly `columns` columns fill a grid
/// `container_width` wide. The session saves this size after a zoom step to
/// that number of columns. With this size, [`columns`] returns the same
/// number again at the same width, and a larger number at a larger width.
pub fn size_for_columns(container_width: f32, columns: usize, gap: f32) -> f32 {
    let (width, gap) = (f64::from(container_width), f64::from(gap));
    ((width - gap) / columns.max(1) as f64 - gap) as f32
}

/// The current zoom, in the unit the zoom control shows for the layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// The grid and waterfall layouts: the grid has `count` columns, and the
    /// zoom allows at most `most` columns at this width. `exact` and `span`
    /// are the same two numbers before they are rounded to whole columns:
    /// how many tiles of the current size fit in the width, and how many
    /// tiles of [`SMALLEST_TILE`] fit. The zoom control places its knob with
    /// them, because they change evenly while the grid is made wider or
    /// narrower, and the whole numbers change in jumps.
    Columns {
        count: usize,
        most: usize,
        exact: f32,
        span: f32,
    },
    /// The justified layout: the target row height in pixels.
    Height(f32),
}

/// Returns the zoom that the tile size `target_size` gives in a grid
/// `container_width` wide.
pub fn zoom(container_width: f32, target_size: f32, gap: f32, mode: ViewMode) -> Zoom {
    match mode {
        ViewMode::Justified => Zoom::Height(target_size.clamp(SMALLEST_TILE, TALLEST_ROW)),
        ViewMode::Grid | ViewMode::Waterfall => {
            let fit = |size: f32| f64::from(container_width - gap) / f64::from(size + gap);
            let span = fit(SMALLEST_TILE).max(1.);
            Zoom::Columns {
                count: columns(container_width, target_size, gap),
                most: most_columns(container_width, gap),
                exact: fit(target_size.max(SMALLEST_TILE)).clamp(1., span) as f32,
                span: span as f32,
            }
        }
    }
}

/// Returns the tile size after one step of the zoom, or `None` when the zoom
/// is already at that end of its range. In the grid and waterfall layouts a
/// step with `larger` set removes one column and a step without it adds one
/// column. In the justified layout a step multiplies or divides the row
/// height by 1.1.
pub fn zoom_step(
    container_width: f32,
    target_size: f32,
    gap: f32,
    mode: ViewMode,
    larger: bool,
) -> Option<f32> {
    // Before the grid has been drawn once its width is not known, so the
    // number of columns cannot be counted.
    if container_width <= 0. {
        return None;
    }
    match zoom(container_width, target_size, gap, mode) {
        Zoom::Columns { count, most, .. } => {
            let next = if larger { count - 1 } else { count + 1 };
            (1..=most)
                .contains(&next)
                .then(|| size_for_columns(container_width, next, gap))
        }
        Zoom::Height(height) => {
            let next = if larger {
                height * ROW_HEIGHT_STEP_FACTOR
            } else {
                height / ROW_HEIGHT_STEP_FACTOR
            };
            let next = next.round().clamp(SMALLEST_TILE, TALLEST_ROW);
            (next != height).then_some(next)
        }
    }
}

/// Returns the two distances keyboard movement uses. The first is the number
/// of items the Up and Down keys move by, which is the number of columns.
/// The second is the height in pixels of one row including the gap below it,
/// which Page Up and Page Down use to count the rows in the viewport.
///
/// The waterfall layout has no rows of one height, so it uses the height of
/// a square tile. The justified layout has no columns, so it uses the number
/// of square tiles of its row height that fit the width.
pub fn keyboard_step(
    container_width: f32,
    target_size: f32,
    gap: f32,
    mode: ViewMode,
    text_height: f32,
) -> (usize, f32) {
    let geometry = geometry(container_width.into(), target_size.into(), gap.into(), mode);
    let row = match mode {
        ViewMode::Justified => geometry.target_media_size,
        ViewMode::Grid | ViewMode::Waterfall => geometry.column_width.max(0.),
    };
    (
        geometry.column_count,
        (row + f64::from(text_height) + geometry.item_gap) as f32,
    )
}

fn geometry(container_width: f64, target_size: f64, gap: f64, mode: ViewMode) -> Geometry {
    // A saved tile size can be outside the zoom's range: Settings used to
    // allow 100 px, and a size saved for one wide column is larger than
    // 900 px. Limit it here so the layout never has more columns than
    // `most_columns`, and justified rows are never taller than `TALLEST_ROW`.
    let target_size = match mode {
        ViewMode::Justified => target_size.clamp(SMALLEST_TILE.into(), TALLEST_ROW.into()),
        ViewMode::Grid | ViewMode::Waterfall => target_size.max(SMALLEST_TILE.into()),
    };
    // A tile's position and size are those of its picture. The selection
    // stroke is drawn 3 px outside the picture, so 3 px are left free on
    // each side of every tile.
    let outer_inset = gap + EXTENT;
    let item_gap = gap + 2. * EXTENT;
    let min_inner_width = container_width - 2. * outer_inset;
    let column_count = columns_that_fit(container_width, target_size, gap) as usize;
    let columns = column_count as f64;
    let column_width = ((min_inner_width - (columns - 1.) * item_gap) / columns).floor();
    let used_width = columns * column_width + (columns - 1.) * item_gap;
    Geometry {
        used_width,
        column_count,
        column_width,
        offset_x: ((container_width - used_width) / 2.).floor(),
        target_media_size: target_size - 2. * EXTENT,
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
        let geometry = geometry(container_width.into(), target_size.into(), gap.into(), mode);
        // When the grid is narrower than the gaps on both sides of one column,
        // the column's width is zero or negative. Nothing is laid out then.
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

    /// Returns the indexes of the tiles that touch the vertical range from
    /// `top` to `bottom`, in ascending order. `top` and `bottom` are measured
    /// in pixels from the top of the layout.
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
    // Each tile goes into the shortest column. The column heights that
    // decide this are computed with the zoom's tile size (`reference`), not
    // with the real column width. The text rows and the gaps keep their
    // size when a column gets wider, so with the real width two columns of
    // nearly the same height swap order when the window is resized by a few
    // pixels, and every later tile changes column with them. With the
    // zoom's tile size a tile changes column only when the number of
    // columns changes.
    let reference = geometry.target_media_size;
    let mut decided = vec![0f64; geometry.column_count];
    let mut tiles = Vec::with_capacity(aspect_ratios.len());
    for &aspect in aspect_ratios {
        let aspect = safe_aspect_ratio(aspect);
        let mut shortest = 0;
        for column in 1..decided.len() {
            if decided[column] < decided[shortest] {
                shortest = column;
            }
        }
        let y = heights[shortest];
        let h = width / aspect + text_height;
        tiles.push(Tile::new(shortest as f64 * (width + gap), y, width, h));
        heights[shortest] = y + h + gap;
        decided[shortest] += reference / aspect + text_height + gap;
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
    let target_row_height = geometry.target_media_size;
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
        // The last row of the items can be incomplete. An incomplete row keeps
        // the target row height and is not stretched to the grid's width. If
        // the last item's tile is also the tile that makes the row wider than
        // the grid, the row would be drawn beyond the grid's right edge at
        // that height. In that case the row ends before this tile, and the
        // tile starts a new last row alone.
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

/// Returns the scroll position that keeps one tile at the same place on
/// screen after the layout changed. The tile is the topmost tile in the
/// viewport, or the topmost selected tile in the viewport when there is one.
///
/// `previous` and `next` are the tiles before and after the change. `selected`
/// holds indexes into `previous`. `scroll_top` and `viewport` are the scroll
/// position and the viewport's height before the change. The items can change
/// too, for example when the results are read again: `moved` maps an index in
/// `previous` to the index of the same item in `next`, or to `None` when the
/// item is gone.
///
/// Returns `None` when no tile was in the viewport or the tile's item is gone.
/// The scroll position returned is never less than 0.
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
                // `visible` must not panic when the layout has no tiles.
                layout.visible(0., 10_000.);
            }
        }
    }

    #[test]
    fn column_count_follows_the_target_size() {
        // The grid is 1000 px wide with tight spacing: the inset is 11 px
        // and the gap between tiles is 14 px. A 260 px tile with its 8 px gap
        // takes 268 px, and 992 px are available. 992 / 268 is 3.7, which
        // rounds to four columns.
        let layout = Layout::compute(&aspects(40), 1000., 260., 8., ViewMode::Waterfall, 0.);
        assert_eq!(layout.column_count, 4);
        let width = layout.tiles[0].w;
        assert_eq!(width, ((1000. - 22. - 3. * 14.) / 4f32).floor());
        // The columns and the gaps between them are centred together in the
        // grid's width.
        let used = 4. * width + 3. * 14.;
        assert_eq!(layout.tiles[0].x, ((1000. - used) / 2.).floor());
        assert_eq!(layout.tiles[0].y, 16.);
        // One zoom step toward larger tiles removes one column. The tile
        // size it returns gives three columns at this width and six columns
        // at twice the width. One step toward smaller tiles adds a column.
        let larger = zoom_step(1000., 260., 8., ViewMode::Waterfall, true).unwrap();
        assert_eq!(columns(1000., larger, 8.), 3);
        assert_eq!(columns(2000., larger, 8.), 6);
        let smaller = zoom_step(1000., 260., 8., ViewMode::Waterfall, false).unwrap();
        assert_eq!(columns(1000., smaller, 8.), 5);
    }

    #[test]
    fn waterfall_fills_the_shortest_column_and_keeps_aspect() {
        let layout = Layout::compute(&[1., 0.5, 2., 1.], 700., 200., 16., ViewMode::Waterfall, 0.);
        assert_eq!(layout.column_count, 3);
        let width = layout.tiles[0].w;
        assert_eq!(layout.tiles[1].h, width / 0.5);
        // The fourth tile goes into the shortest column. That is the third
        // column, because its tile is the widest picture and therefore the
        // least tall.
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
        // The row is as wide as five 184 px columns and four 14 px gaps: 976
        // px.
        assert!(
            (right - left - 976.).abs() < 0.01,
            "row spans the used width"
        );
        // The incomplete last row keeps the target row height: 200 px minus
        // 3 px of selection stroke above and below.
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
        // The grid is scrolled 150 px. Tile 1, whose top is at 120, is the
        // topmost tile in the viewport, 30 px above the viewport's top. After
        // the change its top is at 200, so the scroll position becomes 230 to
        // keep it 30 px above.
        assert_eq!(
            scroll_anchor(&previous, &next, &none, 150., 200., Some),
            Some(230.)
        );
        // A selected tile in the viewport is used in preference to the topmost
        // tile.
        let selected = [2].into_iter().collect();
        assert_eq!(
            scroll_anchor(&previous, &next, &selected, 150., 200., Some),
            Some(310.)
        );
        // With no tiles before the change there is no tile to keep in place.
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
        // Items 0 to 2 are at 0, 110 and 220. Two new items are added before
        // them, so old item 1, the topmost tile in the viewport at scroll
        // position 150, becomes the fourth tile.
        let previous = [tile(0.), tile(110.), tile(220.)];
        let next = [tile(0.), tile(110.), tile(220.), tile(330.), tile(440.)];
        let none = std::collections::BTreeSet::new();
        assert_eq!(
            scroll_anchor(&previous, &next, &none, 150., 200., |index| Some(index + 2)),
            // The tile was 40 px above the viewport's top. Its top is now at
            // 330, so the scroll position is 370.
            Some(370.)
        );
        // When the tile's item is gone, no scroll position is returned.
        assert_eq!(
            scroll_anchor(&previous, &next, &none, 150., 200., |_| None),
            None
        );
    }
}
