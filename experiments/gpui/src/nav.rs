//! Keyboard movement through the grid. A port of `useGridArrowNav.ts`: the
//! same steps, the same column and page arithmetic, the same rule for
//! scrolling the target into view.
use crate::layout::Tile;

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

fn js_round(value: f32) -> f32 {
    (value + 0.5).floor()
}

fn snapped(target_size: f32) -> f32 {
    (js_round(target_size / 50.) * 50.).max(50.)
}

/// How many items Up and Down step over.
pub fn column_count(container_width: f32, target_size: f32, gap: f32) -> usize {
    let min_inner_width = container_width - 2. * gap;
    js_round((min_inner_width + gap) / (snapped(target_size) + gap)).max(1.) as usize
}

/// How many rows Page Up and Page Down step over.
pub fn visible_rows(viewport_height: f32, target_size: f32, gap: f32) -> usize {
    ((viewport_height / (snapped(target_size) + gap)).floor() as usize).max(1)
}

/// The item a move lands on, or `None` when it would not leave `current`.
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

/// The scroll offset that brings `tile` into view with one gap of margin, or
/// `None` when it is already there.
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
    fn columns_and_rows_follow_the_snapped_size() {
        // 720px wide, 260 snaps to 250, tight spacing.
        assert_eq!(column_count(720., 260., 8.), 3);
        assert_eq!(column_count(100., 440., 16.), 1);
        assert_eq!(visible_rows(770., 260., 8.), 2);
        assert_eq!(visible_rows(100., 260., 8.), 1);
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
        // Above the viewport: its top lands one gap below the edge.
        assert_eq!(scroll_into_view(&tile, 1100., 600., 8.), Some(992.));
        // Below it: its bottom lands one gap above the edge.
        assert_eq!(scroll_into_view(&tile, 500., 600., 8.), Some(708.));
    }
}
