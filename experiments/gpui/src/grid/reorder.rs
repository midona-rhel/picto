//! Putting a folder's items in an order of its own by dragging them
//! (`CanvasGrid.tsx`'s reorder drag): where the dragged items would go
//! (`computeReorderTarget`), and the order that sends (`planFolderReorder`).
use super::layout::Tile;
use std::collections::HashSet;

/// Which side of the tile the items go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Beside which tile of the window's layout the dragged items would go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub index: usize,
    pub side: Side,
}

fn side_of(tile: &Tile, x: f32) -> Side {
    if x < tile.x + tile.w / 2. {
        Side::Left
    } else {
        Side::Right
    }
}

/// `computeReorderTarget`: the image under the pointer, by the half it is
/// in; else, in a row the pointer is level with, the tile before or after
/// the gap or edge it is in; else the nearest image's middle. The dragged
/// tiles (`skip`) are never a target. Positions are in the layout's own
/// coordinates; only images take the pointer, not the text rows below.
pub fn target(
    tiles: &[Tile],
    x: f32,
    y: f32,
    text_height: f32,
    skip: &HashSet<usize>,
) -> Option<Target> {
    let image = |tile: &Tile| tile.h - text_height;
    let candidates = || {
        tiles
            .iter()
            .enumerate()
            .filter(|(index, _)| !skip.contains(index))
    };
    if let Some((index, tile)) = candidates().find(|(_, tile)| {
        x >= tile.x && x <= tile.x + tile.w && y >= tile.y && y <= tile.y + image(tile)
    }) {
        return Some(Target {
            index,
            side: side_of(tile, x),
        });
    }
    let mut band = candidates()
        .filter(|(_, tile)| y >= tile.y && y <= tile.y + image(tile))
        .collect::<Vec<_>>();
    band.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
    if let (Some(first), Some(last)) = (band.first(), band.last()) {
        if x < first.1.x {
            return Some(Target {
                index: first.0,
                side: Side::Left,
            });
        }
        if x > last.1.x + last.1.w {
            return Some(Target {
                index: last.0,
                side: Side::Right,
            });
        }
        for pair in band.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if x >= left.1.x + left.1.w && x <= right.1.x {
                let middle = (left.1.x + left.1.w + right.1.x) / 2.;
                return Some(if x < middle {
                    Target {
                        index: left.0,
                        side: Side::Right,
                    }
                } else {
                    Target {
                        index: right.0,
                        side: Side::Left,
                    }
                });
            }
        }
    }
    let mut best: Option<(f32, Target)> = None;
    for (index, tile) in candidates() {
        let (cx, cy) = (tile.x + tile.w / 2., tile.y + image(tile) / 2.);
        let distance = (x - cx).powi(2) + (y - cy).powi(2);
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((
                distance,
                Target {
                    index,
                    side: side_of(tile, x),
                },
            ));
        }
    }
    best.map(|(_, target)| target)
}

/// `planFolderReorder`: the order with the dragged items, in their own
/// order, moved to the target. Nothing when that changes nothing.
pub fn plan(order: &[u32], dragged: &HashSet<u32>, target: Target) -> Vec<u32> {
    let moving = order
        .iter()
        .copied()
        .filter(|id| dragged.contains(id))
        .collect::<Vec<_>>();
    if moving.is_empty() {
        return Vec::new();
    }
    let at = match target.side {
        Side::Right => target.index + 1,
        Side::Left => target.index,
    };
    let before = order[..at.min(order.len())]
        .iter()
        .filter(|id| dragged.contains(id))
        .count();
    let remaining = order
        .iter()
        .copied()
        .filter(|id| !dragged.contains(id))
        .collect::<Vec<_>>();
    let insert = at.saturating_sub(before).min(remaining.len());
    let planned = remaining[..insert]
        .iter()
        .chain(&moving)
        .chain(&remaining[insert..])
        .copied()
        .collect::<Vec<_>>();
    if planned == order {
        Vec::new()
    } else {
        planned
    }
}

#[cfg(test)]
mod tests {
    use super::{Side, Target, plan, target};
    use crate::grid::layout::Tile;
    use std::collections::HashSet;

    fn set(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn a_selected_block_moves_whole() {
        let right = |index| Target {
            index,
            side: Side::Right,
        };
        assert_eq!(
            plan(&[1, 2, 3, 4, 5], &set(&[2, 3]), right(4)),
            [1, 4, 5, 2, 3]
        );
        // A drop inside the block changes nothing, so nothing is sent.
        assert!(plan(&[1, 2, 3, 4], &set(&[2, 3]), right(2)).is_empty());
        // The first item after the third, as the reference did it.
        assert_eq!(plan(&[1, 2, 3, 4], &set(&[1]), right(2)), [2, 3, 1, 4]);
        assert_eq!(
            plan(
                &[1, 2, 3, 4],
                &set(&[4]),
                Target {
                    index: 0,
                    side: Side::Left
                }
            ),
            [4, 1, 2, 3]
        );
    }

    #[test]
    fn the_target_is_the_image_gap_or_nearest_tile() {
        let tile = |x, y| Tile {
            x,
            y,
            w: 100.,
            h: 120.,
        };
        // Two rows of three, 20px text rows, 10px gaps.
        let tiles = [
            tile(0., 0.),
            tile(110., 0.),
            tile(220., 0.),
            tile(0., 130.),
            tile(110., 130.),
            tile(220., 130.),
        ];
        let none = HashSet::new();
        let at = |x, y, skip: &HashSet<usize>| target(&tiles, x, y, 20., skip);
        // On an image: its half decides the side.
        assert_eq!(
            at(130., 50., &none),
            Some(Target {
                index: 1,
                side: Side::Left
            })
        );
        assert_eq!(
            at(190., 50., &none),
            Some(Target {
                index: 1,
                side: Side::Right
            })
        );
        // In the gap between two images of a row.
        assert_eq!(
            at(104., 50., &none),
            Some(Target {
                index: 0,
                side: Side::Right
            })
        );
        assert_eq!(
            at(106., 50., &none),
            Some(Target {
                index: 1,
                side: Side::Left
            })
        );
        // Past the row's end.
        assert_eq!(
            at(400., 160., &none),
            Some(Target {
                index: 5,
                side: Side::Right
            })
        );
        // Over a name row: the nearest image.
        assert_eq!(
            at(40., 115., &none),
            Some(Target {
                index: 0,
                side: Side::Left
            })
        );
        // A dragged tile is never the target.
        let skip = [1].into_iter().collect();
        assert_eq!(
            at(130., 50., &skip),
            Some(Target {
                index: 0,
                side: Side::Right
            })
        );
    }
}
