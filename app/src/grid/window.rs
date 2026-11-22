//! The window: the part of a destination's items that the grid holds.
//!
//! The grid never holds or lays out more than 1,500 items of a destination
//! (`WINDOW`). They are read from the library starting at an exact ordinal.
//! An item's ordinal is its position in the destination's order. The
//! scrollable height stands for every item of the destination. It is
//! estimated from the average height of the first items that were laid out.
//! The window's tiles are placed in that height where the window's ordinals
//! are in the destination. Items outside the window are not laid out, and
//! nothing is drawn for them until the window that holds them has been read.
use std::time::Duration;

/// The number of items in the first read of a destination. The first
/// ordinal of a window is a multiple of this number, except for the window
/// that ends at the last item.
pub const PAGE: usize = 500;
/// The largest number of items a window holds.
pub const WINDOW: usize = 1500;
/// The shortest time between two requests for a window while the grid
/// scrolls. When this time has passed, the grid asks for the window that the
/// viewport needs at that moment.
pub const REQUEST_INTERVAL: Duration = Duration::from_millis(40);
/// The largest scrollable height in pixels, however many items the
/// destination has. The tiles of the window keep their real sizes.
pub const MAX_HEIGHT: f32 = 8_000_000.;
/// The number of the first items of a layout on which the average height of
/// an item is measured.
pub const SAMPLE: usize = 500;

/// Returns the first ordinal of the window that shows the item at `ordinal`
/// in a destination of `total` items. The window starts one page before the
/// page that holds `ordinal`, and it never ends after the last item.
pub fn start_at(ordinal: usize, total: usize) -> usize {
    let aligned = (ordinal / PAGE).saturating_sub(1) * PAGE;
    aligned.min(total.saturating_sub(WINDOW))
}

/// Returns whether the window that is held is already the wanted one, so
/// that no read is needed. The window that is held starts at the ordinal
/// `start` and has `count` items. `wanted` is the first ordinal that
/// `start_at` returned, and `total` is the number of items in the
/// destination.
pub fn holds(start: usize, count: usize, wanted: usize, total: usize) -> bool {
    start == wanted && count >= WINDOW.min(total)
}

/// Returns the average height of one item in pixels: `sample_height`, the
/// height of the first `sample_count` items of a layout, divided by
/// `sample_count`. Returns `None` when `sample_count` is 0.
pub fn density(sample_height: f32, sample_count: usize) -> Option<f32> {
    (sample_count > 0).then(|| sample_height / sample_count as f32)
}

/// Where the window's tiles are in the scrollable height that stands for
/// every item of the destination.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The scrollable height that stands for every item of the destination,
    /// in pixels.
    pub height: f32,
    /// The distance in pixels from the top of that height to the top of the
    /// window's first row of tiles.
    pub top: f32,
    /// The distance in pixels from the top of that height to the bottom of
    /// the window's last row of tiles.
    pub bottom: f32,
}

impl Placement {
    /// Returns the placement of a window that holds every item of its
    /// destination. The scrollable height is the height of the window's
    /// layout, `local_height`.
    pub fn whole(local_height: f32) -> Self {
        Self {
            height: local_height,
            top: 0.,
            bottom: local_height,
        }
    }
}

/// Returns where the window's tiles go in the scrollable height.
///
/// The window holds `count` items starting at the ordinal `start`, and its
/// layout is `local_height` pixels tall. The destination has `total` items,
/// and `per_item` is the estimated average height of one of them.
///
/// Without an `anchor`, the window's top is `start / (total - count)` of
/// the way down the distance the window can move, which is the scrollable
/// height less `local_height`. An `anchor` is an ordinal and the distance
/// from the top of the scrollable height at which the top of that ordinal's
/// tile has to be. With an anchor whose ordinal is in the window, the
/// window is placed so that the tile is there.
/// `anchor_y` takes the index of a tile in the window and returns the top of
/// that tile in the window's own layout.
///
/// A window that starts at the first item is always placed at the top of
/// the scrollable height, and a window that ends at the last item is always
/// placed at its bottom, whatever the estimate and the anchor give.
pub fn place(
    local_height: f32,
    start: usize,
    count: usize,
    total: usize,
    per_item: f32,
    anchor: Option<(usize, f32)>,
    anchor_y: impl Fn(usize) -> Option<f32>,
) -> Placement {
    let height = if total <= count {
        local_height
    } else {
        local_height.max(MAX_HEIGHT.min(total as f32 * per_item))
    };
    let remaining = total.saturating_sub(count);
    let span = height - local_height;
    let mut top = if remaining > 0 {
        start as f32 / remaining as f32 * span
    } else {
        0.
    };
    if let Some((ordinal, at)) = anchor
        && let Some(y) = ordinal.checked_sub(start).and_then(&anchor_y)
    {
        top = at - y;
    }
    if start == 0 {
        top = 0.;
    } else if start + count >= total {
        top = span;
    }
    let top = top.clamp(0., span.max(0.));
    Placement {
        height,
        top,
        bottom: top + local_height,
    }
}

/// Returns the ordinal that the next window has to show, or `None` when
/// the window that is held is enough for the viewport.
///
/// - When the middle of the viewport is inside the window's tiles and the
///   viewport is at least three times its own height away from each end of
///   the window that has more items beyond it, the result is `None`.
/// - When the middle of the viewport is inside the window's tiles but the
///   viewport is nearer than that to such an end, the result is the ordinal
///   of the tile whose top is nearest the middle of the viewport.
/// - When the middle of the viewport is outside the window's tiles, the
///   result is the ordinal that `ordinal_at` returns for the scroll
///   position.
///
/// `tops` holds the top of each tile of the window, measured from the top of
/// the scrollable height. `start` is the window's first ordinal, `total` is
/// the number of items in the destination, and `scroll_top` and `viewport`
/// are the scroll position and the height of the viewport in pixels.
pub fn destination(
    placement: Placement,
    start: usize,
    tops: &[f32],
    total: usize,
    scroll_top: f32,
    viewport: f32,
) -> Option<usize> {
    let count = tops.len();
    if total <= count || viewport <= 0. {
        return None;
    }
    let center = scroll_top + viewport / 2.;
    if center >= placement.top && center < placement.bottom {
        let near_start = start > 0 && scroll_top < placement.top + viewport * 3.;
        let near_end =
            start + count < total && scroll_top + viewport > placement.bottom - viewport * 3.;
        if !near_start && !near_end {
            return None;
        }
        let nearest = (0..count).min_by(|a, b| {
            (tops[*a] - center)
                .abs()
                .total_cmp(&(tops[*b] - center).abs())
        })?;
        return Some(start + nearest);
    }
    Some(ordinal_at(scroll_top, placement.height, viewport, total))
}

/// Returns the ordinal that is as far down the destination's `total` items
/// as `scroll_top` is down the distance the grid can scroll, which is
/// `height` less `viewport`.
pub fn ordinal_at(scroll_top: f32, height: f32, viewport: f32, total: usize) -> usize {
    let progress = (scroll_top / (height - viewport).max(1.)).clamp(0., 1.);
    ((progress as f64 * total as f64).floor() as usize).min(total.saturating_sub(1))
}

/// Returns whether the middle of the viewport is outside the window's
/// tiles, so that the grid has no tile to draw there. It returns false for
/// a window that holds every item of its destination, also when the tiles
/// end above the middle of the viewport.
pub fn misses(placement: Placement, count: usize, scroll_top: f32, viewport: f32) -> bool {
    if viewport <= 0. || count == 0 {
        return false;
    }
    if placement.top == 0. && placement.bottom == placement.height {
        return false;
    }
    let center = scroll_top + viewport / 2.;
    center < placement.top || center >= placement.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_start_one_page_early_on_a_page_and_stop_at_the_end() {
        assert_eq!(start_at(0, 2673), 0);
        assert_eq!(start_at(499, 2673), 0);
        assert_eq!(start_at(999, 2673), 0);
        assert_eq!(start_at(1000, 2673), 500);
        // The window never ends after the last item, so the window for the
        // last item holds the last 1,500 items.
        assert_eq!(start_at(2672, 2673), 1173);
        assert_eq!(start_at(500_000, 1_000_000), 499_500);
        assert_eq!(start_at(999_999, 1_000_000), 998_500);
        // A destination with fewer items than a window holds has one
        // window, which starts at the first item.
        assert_eq!(start_at(800, 1000), 0);
    }

    #[test]
    fn a_window_already_held_needs_no_read() {
        assert!(holds(500, 1500, 500, 2673));
        // The first page of 500 items is not the whole first window.
        assert!(!holds(0, 500, 0, 2673));
        // A destination of 300 items is held whole.
        assert!(holds(0, 300, 0, 300));
        assert!(!holds(0, 1500, 500, 2673));
    }

    #[test]
    fn the_height_stands_for_every_item_and_is_capped() {
        let none = |_| None;
        let first = place(20_000., 0, 500, 2673, 40., None, none);
        assert_eq!(first.height, 2673. * 40.);
        assert_eq!((first.top, first.bottom), (0., 20_000.));
        // A destination that is held whole has the height of its layout.
        assert_eq!(place(5_000., 0, 300, 300, 40., None, none).height, 5_000.);
        // The height for a million items is limited to `MAX_HEIGHT`.
        let million = place(60_000., 0, 1500, 1_000_000, 40., None, none);
        assert_eq!(million.height, MAX_HEIGHT);
        // The height is never less than the height of the window's layout.
        assert_eq!(
            place(90_000., 0, 1500, 1600, 40., None, none).height,
            90_000.
        );
    }

    #[test]
    fn a_window_lies_where_its_ordinals_fall_and_the_ends_are_exact() {
        let none = |_| None;
        // The destination has 1,000,000 items of 40 px each. The window
        // holds 1,500 of them, and its layout is 60,000 px tall.
        let middle = place(60_000., 499_500, 1500, 1_000_000, 40., None, none);
        let span = MAX_HEIGHT - 60_000.;
        assert!((middle.top - 499_500. / 998_500. * span).abs() < 1.);
        // The last window ends at the bottom of the scrollable height, and
        // the first window starts at its top.
        let last = place(60_000., 998_500, 1500, 1_000_000, 40., None, none);
        assert_eq!(last.bottom, MAX_HEIGHT);
        let first = place(60_000., 0, 1500, 1_000_000, 40., None, none);
        assert_eq!(first.top, 0.);
    }

    #[test]
    fn an_anchor_puts_its_tile_where_it_was_asked_for() {
        // The top of the tile of ordinal 600 is 4,000 px below the top of
        // the window that starts at ordinal 500.
        let y = |offset: usize| (offset == 100).then_some(4_000.);
        let placed = place(60_000., 500, 1500, 10_000, 40., Some((600, 150_000.)), y);
        assert_eq!(placed.top, 146_000.);
        // An anchor whose ordinal is not in the window is ignored.
        let unanchored = place(60_000., 500, 1500, 10_000, 40., None, |_| None);
        let ignored = place(60_000., 500, 1500, 10_000, 40., Some((10, 150_000.)), y);
        assert_eq!(ignored, unanchored);
        // The window is never placed above the top of the scrollable
        // height.
        let low = place(60_000., 500, 1500, 10_000, 40., Some((600, 10.)), y);
        assert_eq!(low.top, 0.);
    }

    #[test]
    fn the_viewport_asks_for_a_window_only_near_an_edge_or_outside() {
        // The window holds the ordinals 500 to 1,999, and the tops of its
        // tiles are 40 px apart. Its top is 100,000 px down a scrollable
        // height of 400,000 px that stands for 10,000 items.
        let tops = (0..1500)
            .map(|i| 100_000. + i as f32 * 40.)
            .collect::<Vec<_>>();
        let placed = Placement {
            height: 400_000.,
            top: 100_000.,
            bottom: 160_000.,
        };
        // A viewport far from both ends of the window asks for nothing.
        assert_eq!(
            destination(placed, 500, &tops, 10_000, 130_000., 800.),
            None
        );
        // A viewport less than three times its height from the window's
        // end asks for the ordinal at the middle of the viewport.
        let near_end = destination(placed, 500, &tops, 10_000, 158_000., 800.);
        assert_eq!(near_end, Some(500 + (158_400 - 100_000) / 40));
        // A viewport near the start of a window that starts at the first
        // item asks for nothing, because no items come before the window.
        assert_eq!(destination(placed, 0, &tops, 10_000, 100_100., 800.), None);
        // A viewport outside the window asks for the ordinal that is as
        // far down the 10,000 items as the scroll position is down the
        // distance the grid can scroll: 299,600 of 399,200 px.
        assert_eq!(
            destination(placed, 500, &tops, 10_000, 299_600., 800.),
            Some(7_505)
        );
        // A destination held whole asks for nothing.
        assert_eq!(destination(placed, 0, &tops, 1500, 300_000., 800.), None);
    }

    #[test]
    fn a_dragged_thumb_lands_on_its_share_of_the_items() {
        // The scrollbar's thumb is dragged to the middle of a destination
        // of 1,000,000 items.
        let height = MAX_HEIGHT;
        let viewport = 800.;
        let middle = (height - viewport) / 2.;
        let ordinal = ordinal_at(middle, height, viewport, 1_000_000);
        assert_eq!(ordinal, 500_000);
        assert_eq!(start_at(ordinal, 1_000_000), 499_500);
        // The top of the scrollable height gives the first ordinal, and
        // its bottom gives the last.
        assert_eq!(ordinal_at(0., height, viewport, 1_000_000), 0);
        assert_eq!(ordinal_at(height, height, viewport, 1_000_000), 999_999);
    }

    #[test]
    fn only_a_viewport_off_the_window_misses_it() {
        let placed = Placement {
            height: 400_000.,
            top: 100_000.,
            bottom: 160_000.,
        };
        assert!(!misses(placed, 1500, 120_000., 800.));
        assert!(misses(placed, 1500, 200_000., 800.));
        assert!(misses(placed, 1500, 0., 800.));
        // A window that holds its whole destination never misses.
        assert!(!misses(Placement::whole(3_000.), 20, 0., 800.));
    }

    /// Checks that the work of one frame of the grid depends on the window
    /// and the viewport, and not on the number of items in the destination.
    /// It counts the items in the window, the tiles laid out and the tiles
    /// drawn at the top, the middle and the end of destinations of 3,000, a
    /// million and fifty million items. The three numbers have to be the
    /// same for a million and for fifty million items, and at every size
    /// the window has to hold at most 1,500 items and fewer than 100 tiles
    /// may be drawn.
    #[test]
    fn a_frame_does_the_same_work_whatever_the_library_size() {
        use crate::grid::layout::{Layout, ViewMode};
        let viewport = 800.;
        let mut work = Vec::new();
        for total in [3_000, 1_000_000, 50_000_000] {
            for share in [0., 0.5, 1.] {
                let ordinal = ((total - 1) as f64 * share) as usize;
                let start = start_at(ordinal, total);
                let count = WINDOW.min(total - start);
                // The waterfall layout is used because it does the most
                // work per item.
                let aspects = (0..count)
                    .map(|index| [0.5, 1.5, 0.75, 2.][index % 4])
                    .collect::<Vec<_>>();
                let layout = Layout::compute(&aspects, 600., 200., 8., ViewMode::Waterfall, 20.);
                let placed = place(layout.total_height, start, count, total, 40., None, |_| {
                    None
                });
                assert!(placed.height <= MAX_HEIGHT.max(layout.total_height));
                let tops = layout
                    .tiles
                    .iter()
                    .map(|tile| tile.y + placed.top)
                    .collect::<Vec<_>>();
                let middle = placed.top + layout.total_height / 2.;
                destination(placed, start, &tops, total, middle, viewport);
                let local = layout.total_height / 2.;
                let drawn = layout.visible(local, local + viewport).len();
                work.push((count, layout.tiles.len(), drawn));
            }
        }
        // The frames of the destinations of a million and of fifty million
        // items do the same work.
        assert!(work[3..].iter().all(|frame| *frame == work[3]), "{work:?}");
        assert!(
            work.iter()
                .all(|(count, laid, drawn)| { *count <= WINDOW && laid == count && *drawn < 100 })
        );
    }

    #[test]
    fn the_height_per_item_is_the_sample_shared_out() {
        assert_eq!(density(20_000., 500), Some(40.));
        assert_eq!(density(0., 0), None);
    }
}
