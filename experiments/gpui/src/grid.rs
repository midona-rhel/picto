//! Picto waterfall geometry; recomputed only when viewport, zoom or results change.
pub const GAP: f32 = 14.; // 8px spacing plus selection extents.
pub const INSET: f32 = 11.;
#[derive(Clone, Debug, PartialEq)]
pub struct PositionedTile {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Default)]
pub struct VisibilityIndex {
    columns: Vec<Vec<usize>>,
    pub height: f32,
}

impl VisibilityIndex {
    pub fn new(tiles: &[PositionedTile]) -> Self {
        let mut positions = Vec::new();
        let mut columns: Vec<Vec<usize>> = Vec::new();
        let mut height: f32 = 0.;
        for (index, tile) in tiles.iter().enumerate() {
            let column = positions
                .iter()
                .position(|x| *x == tile.x)
                .unwrap_or_else(|| {
                    positions.push(tile.x);
                    columns.push(Vec::new());
                    columns.len() - 1
                });
            columns[column].push(index);
            height = height.max(tile.y + tile.height);
        }
        Self {
            columns,
            height: height + 16.,
        }
    }

    pub fn visible(&self, tiles: &[PositionedTile], top: f32, bottom: f32) -> Vec<usize> {
        let mut visible = Vec::new();
        for column in &self.columns {
            // Each waterfall column is ordered and non-overlapping, including tall images.
            let start = column.partition_point(|&i| tiles[i].y + tiles[i].height < top);
            for &index in &column[start..] {
                if tiles[index].y > bottom {
                    break;
                }
                visible.push(index);
            }
        }
        visible.sort_unstable();
        visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_visibility_matches_full_scan() {
        let aspects = (0..20_000)
            .map(|i| [0.125, 8., 1., 0.5][i % 4])
            .collect::<Vec<_>>();
        let tiles = waterfall(&aspects, 1000., 200.);
        let index = VisibilityIndex::new(&tiles);
        for top in (0..100_000).step_by(137) {
            let top = top as f32;
            let expected = tiles
                .iter()
                .enumerate()
                .filter_map(|(i, t)| (t.y + t.height >= top && t.y <= top + 1800.).then_some(i))
                .collect::<Vec<_>>();
            assert_eq!(index.visible(&tiles, top, top + 1800.), expected);
        }
    }

    #[test]
    fn pagination_does_not_move_existing_tiles() {
        let aspects = vec![1.25; 1000];
        let first = waterfall(&aspects[..500], 800., 200.);
        let appended = waterfall(&aspects, 800., 200.);
        assert_eq!(first, appended[..500]);
    }

    #[test]
    fn tiles_are_finite_in_bounds_and_do_not_overlap() {
        for width in [100., 500., 1000.] {
            let tiles = waterfall(&[0., f32::NAN, 0.1, 8., 1., 2., 0.5], width, 200.);
            for (index, tile) in tiles.iter().enumerate() {
                assert_eq!(tile.index, index);
                assert!(tile.height.is_finite() && tile.height > 0.);
                assert!(tile.x + tile.width <= width - INSET + 0.1);
                for other in &tiles[..index] {
                    assert!(
                        tile.x >= other.x + other.width
                            || other.x >= tile.x + tile.width
                            || tile.y >= other.y + other.height
                            || other.y >= tile.y + tile.height
                    );
                }
            }
        }
    }
}
pub fn waterfall(aspects: &[f32], width: f32, target: f32) -> Vec<PositionedTile> {
    let inner = (width - 2. * INSET).max(1.);
    let target = (target / 50.).round().max(1.) * 50. - 6.;
    let columns = ((inner + GAP) / (target + GAP)).round().max(1.) as usize;
    let column_width = ((inner - (columns - 1) as f32 * GAP) / columns as f32)
        .floor()
        .max(1.);
    let mut heights = vec![16.; columns];
    aspects
        .iter()
        .enumerate()
        .map(|(index, aspect)| {
            let column = (0..columns)
                .min_by(|a, b| heights[*a].partial_cmp(&heights[*b]).unwrap())
                .unwrap();
            let aspect = if aspect.is_finite() && *aspect > 0. {
                aspect.clamp(0.125, 8.)
            } else {
                1.5
            };
            let tile = PositionedTile {
                index,
                x: INSET + column as f32 * (column_width + GAP),
                y: heights[column],
                width: column_width,
                height: column_width / aspect,
            };
            heights[column] += tile.height + GAP;
            tile
        })
        .collect()
}
