//! Keyboard movement through the grid. A port of `useGridArrowNav.ts`: the
//! same moves and the same rule for scrolling the target tile into view.
//! The number of columns and the row height come from the layout
//! (`layout::keyboard_step`), so the Up, Down and page keys agree with what
//! is drawn.
use super::layout::Tile;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    First,
    Last,
    PageUp,
    PageDown,
}

/// The number of rows Page Up and Page Down move by: the number of whole
/// rows that fit a viewport `viewport_height` tall, at least one. `row` is
/// the height of one row including the gap below it
/// ([`super::layout::keyboard_step`]).
pub fn visible_rows(viewport_height: f32, row: f32) -> usize {
    ((viewport_height / row.max(1.)).floor() as usize).max(1)
}

/// Returns the index of the item that the move `action` goes to from the
/// item at index `current`. Returns `None` when the move would stay on
/// `current`, or when the grid has no items.
///
/// `len` is the number of items, `columns` the number of columns in the
/// grid, and `rows` the number of rows that Page Up and Page Down move by.
/// Left and Right move by one item. Up and Down move by one row, which is
/// `columns` items. Page Up and Page Down move by `rows` rows. First and
/// Last go to the first and the last item. A move that would go past
/// either end stops at the first or the last item.
pub fn target(
    action: Move,
    current: usize,
    len: usize,
    columns: usize,
    rows: usize,
) -> Option<usize> {
    let last = len.checked_sub(1)?;
    let target = match action {
        Move::Left => current.saturating_sub(1),
        Move::Right => (current + 1).min(last),
        Move::Up => current.saturating_sub(columns),
        Move::Down => (current + columns).min(last),
        Move::First => 0,
        Move::Last => last,
        Move::PageUp => current.saturating_sub(columns * rows),
        Move::PageDown => (current + columns * rows).min(last),
    };
    (target != current).then_some(target)
}

/// Returns the scroll offset to scroll to so that `tile` is fully visible
/// with at least `gap` pixels between it and the top and bottom edges of
/// the viewport. Returns `None` when the tile is already visible that way
/// at the current offset, `scroll_top`, so that no scrolling is needed.
///
/// If the tile's top is higher than the point `gap` pixels below the
/// viewport's top, the offset returned puts the tile's top exactly `gap`
/// pixels below the viewport's top. Otherwise, if the tile's bottom is
/// lower than the point `gap` pixels above the viewport's bottom, the
/// offset returned puts the tile's bottom exactly `gap` pixels above the
/// viewport's bottom. The grid is never scrolled further than that.
pub fn scroll_into_view(
    tile: &Tile,
    scroll_top: f32,
    viewport_height: f32,
    gap: f32,
) -> Option<f32> {
    if tile.y < scroll_top + gap {
        Some(tile.y - gap)
    } else if tile.y + tile.h > scroll_top + viewport_height - gap {
        Some(tile.y + tile.h - viewport_height + gap)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stop_at_the_ends() {
        assert_eq!(target(Move::Left, 0, 10, 4, 3), None);
        assert_eq!(target(Move::Right, 9, 10, 4, 3), None);
        assert_eq!(target(Move::Right, 3, 10, 4, 3), Some(4));
        assert_eq!(target(Move::Up, 2, 10, 4, 3), Some(0));
        assert_eq!(target(Move::Down, 7, 10, 4, 3), Some(9));
        assert_eq!(target(Move::First, 5, 10, 4, 3), Some(0));
        assert_eq!(target(Move::Last, 5, 10, 4, 3), Some(9));
        assert_eq!(target(Move::PageDown, 1, 100, 4, 3), Some(13));
        assert_eq!(target(Move::PageUp, 20, 100, 4, 3), Some(8));
        assert_eq!(target(Move::Down, 0, 0, 4, 3), None);
    }

    #[test]
    fn columns_and_rows_are_the_layouts() {
        use crate::grid::layout::{self, Layout, ViewMode};
        // The grid is 720 px wide with tight spacing and names shown. A
        // 260 px tile size gives three columns, so the fourth item is laid
        // out directly below the first, one row height further down.
        let (columns, row) = layout::keyboard_step(720., 260., 8., ViewMode::Grid, 20.);
        let layout = Layout::compute(&[1.; 8], 720., 260., 8., ViewMode::Grid, 20.);
        assert_eq!(columns, 3);
        assert_eq!(layout.tiles[columns].x, layout.tiles[0].x);
        assert_eq!(layout.tiles[columns].y - layout.tiles[0].y, row);
        assert_eq!(
            layout::keyboard_step(100., 440., 16., ViewMode::Grid, 0.).0,
            1
        );
        // A row is 257 px tall here. Two whole rows fit in 770 px. A viewport
        // shorter than one row still counts as one row.
        assert_eq!(visible_rows(770., row), 2);
        assert_eq!(visible_rows(100., row), 1);
    }

    #[test]
    fn scrolls_only_as_far_as_needed() {
        let tile = Tile {
            x: 0.,
            y: 1000.,
            w: 200.,
            h: 300.,
        };
        assert_eq!(scroll_into_view(&tile, 900., 600., 8.), None);
        // The tile's top, at 1000, is above the viewport's top, at 1100.
        // The new offset, 992, puts the tile's top 8 px below the
        // viewport's top.
        assert_eq!(scroll_into_view(&tile, 1100., 600., 8.), Some(992.));
        // The tile's bottom, at 1300, is below the viewport's bottom, at
        // 1100. The new offset, 708, puts the viewport's bottom at 1308,
        // which is 8 px below the tile's bottom.
        assert_eq!(scroll_into_view(&tile, 500., 600., 8.), Some(708.));
    }
}
