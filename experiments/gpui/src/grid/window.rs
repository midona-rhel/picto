//! The grid's window over a destination, as the Electron grid keeps it
//! (`gridWindow.ts`, `gridLayoutModel.ts`, `gridController.requestWindowAt`):
//! only a window of at most 1,500 items, read from the library at an exact
//! ordinal of the destination's order, is ever held and laid out. The
//! scrollable height stands for every item of the destination, estimated
//! from how tall the first items lay out, and the window is placed in it
//! where its ordinals fall. Items outside the window are not laid out and
//! nothing is drawn for them until their window arrives.
use std::time::Duration;

/// The first read of a destination, and the window's alignment.
pub const PAGE: usize = 500;
/// `GRID_WINDOW_SIZE`: the most items a window holds.
pub const WINDOW: usize = 1500;
/// `GRID_SCROLL_REQUEST_INTERVAL_MS`: a scroll asks for the window it needs
/// at most this often, for where it is when the interval ends.
pub const REQUEST_INTERVAL: Duration = Duration::from_millis(40);
/// `MAX_GRID_SCROLL_HEIGHT`: the scrollable height stops here, however many
/// items there are; tiles of the window keep their real sizes.
pub const MAX_HEIGHT: f32 = 8_000_000.;
/// `GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT`: how many of the first items the
/// height per item is measured on.
pub const SAMPLE: usize = 500;

/// `windowStartAt`: the window for showing `ordinal`. Starts are multiples
/// of a page, one page before it, and the window never runs past the end.
pub fn start_at(ordinal: usize, total: usize) -> usize {
    let aligned = (ordinal / PAGE).saturating_sub(1) * PAGE;
    aligned.min(total.saturating_sub(WINDOW))
}

/// Whether a window starting at `start` with `count` items is the one
/// `start_at` asks for already, so no read is needed.
pub fn holds(start: usize, count: usize, wanted: usize, total: usize) -> bool {
    start == wanted && count >= WINDOW.min(total)
}

/// The height each item takes on average, measured on the first items of a
/// layout (`scrollEstimateSampleHeight / scrollEstimateSampleCount`).
pub fn density(sample_height: f32, sample_count: usize) -> Option<f32> {
    (sample_count > 0).then(|| sample_height / sample_count as f32)
}

/// Where the window lies in the whole destination's scrollable height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The scrollable height of every item.
    pub height: f32,
    /// Where the window's laid-out height begins in it.
    pub top: f32,
    /// Where it ends.
    pub bottom: f32,
}

impl Placement {
    /// A destination held whole: the window is all of it.
    pub fn whole(local_height: f32) -> Self {
        Self {
            height: local_height,
            top: 0.,
            bottom: local_height,
        }
    }
}

/// `placeGridWindow`: the window of `count` items at `start`, laid out
/// `local_height` tall, in a destination of `total` items of `per_item`
/// height each. Its top is where its ordinals fall in proportion, or, with
/// an `anchor` (an ordinal and where it should be), where that ordinal's
/// tile lands there; `anchor_y` gives an ordinal's tile top in the window's
/// own layout. The first window starts at the top and the last ends at the
/// bottom, whatever the estimate says.
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

/// `gridWindowDestination`: the ordinal whose window the viewport needs, or
/// `None` while the window covers it with room to spare. A viewport inside
/// the window within three viewports of an end that has more beyond asks
/// for the ordinal nearest its middle; one outside the window asks for the
/// ordinal at the scroll's share of the whole. `tops` are the window's tile
/// tops, placed.
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

/// The ordinal at the scroll's share of the scrollable height.
pub fn ordinal_at(scroll_top: f32, height: f32, viewport: f32, total: usize) -> usize {
    let progress = (scroll_top / (height - viewport).max(1.)).clamp(0., 1.);
    ((progress as f64 * total as f64).floor() as usize).min(total.saturating_sub(1))
}

/// `gridViewportMissesWindow`: the middle of the viewport has no tile of
/// the window to show. A short destination that ends above the middle
/// misses nothing.
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
        // The window never runs past the end: the last 1,500.
        assert_eq!(start_at(2672, 2673), 1173);
        assert_eq!(start_at(500_000, 1_000_000), 499_500);
        assert_eq!(start_at(999_999, 1_000_000), 998_500);
        // Fewer items than a window: always the start.
        assert_eq!(start_at(800, 1000), 0);
    }

    #[test]
    fn a_window_already_held_needs_no_read() {
        assert!(holds(500, 1500, 500, 2673));
        // The first page alone is not yet the first window.
        assert!(!holds(0, 500, 0, 2673));
        // A short destination held whole.
        assert!(holds(0, 300, 0, 300));
        assert!(!holds(0, 1500, 500, 2673));
    }

    #[test]
    fn the_height_stands_for_every_item_and_is_capped() {
        let none = |_| None;
        let first = place(20_000., 0, 500, 2673, 40., None, none);
        assert_eq!(first.height, 2673. * 40.);
        assert_eq!((first.top, first.bottom), (0., 20_000.));
        // Held whole: the layout's own height.
        assert_eq!(place(5_000., 0, 300, 300, 40., None, none).height, 5_000.);
        // A million items do not overflow the cap.
        let million = place(60_000., 0, 1500, 1_000_000, 40., None, none);
        assert_eq!(million.height, MAX_HEIGHT);
        // Never shorter than what is laid out.
        assert_eq!(
            place(90_000., 0, 1500, 1600, 40., None, none).height,
            90_000.
        );
    }

    #[test]
    fn a_window_lies_where_its_ordinals_fall_and_the_ends_are_exact() {
        let none = |_| None;
        // 1,000,000 items of 40px; the window holds 1,500 laid out 60,000 tall.
        let middle = place(60_000., 499_500, 1500, 1_000_000, 40., None, none);
        let span = MAX_HEIGHT - 60_000.;
        assert!((middle.top - 499_500. / 998_500. * span).abs() < 1.);
        // The last window ends at the bottom, the first starts at the top.
        let last = place(60_000., 998_500, 1500, 1_000_000, 40., None, none);
        assert_eq!(last.bottom, MAX_HEIGHT);
        let first = place(60_000., 0, 1500, 1_000_000, 40., None, none);
        assert_eq!(first.top, 0.);
    }

    #[test]
    fn an_anchor_puts_its_tile_where_it_was_asked_for() {
        // Ordinal 600's tile is 4,000px into the window that starts at 500.
        let y = |offset: usize| (offset == 100).then_some(4_000.);
        let placed = place(60_000., 500, 1500, 10_000, 40., Some((600, 150_000.)), y);
        assert_eq!(placed.top, 146_000.);
        // An anchor outside the window is ignored.
        let unanchored = place(60_000., 500, 1500, 10_000, 40., None, |_| None);
        let ignored = place(60_000., 500, 1500, 10_000, 40., Some((10, 150_000.)), y);
        assert_eq!(ignored, unanchored);
        // Never past the ends.
        let low = place(60_000., 500, 1500, 10_000, 40., Some((600, 10.)), y);
        assert_eq!(low.top, 0.);
    }

    #[test]
    fn the_viewport_asks_for_a_window_only_near_an_edge_or_outside() {
        // The window holds ordinals 500..2000, tiles 40px apart, placed at
        // 100,000 in a 400,000px destination of 10,000 items.
        let tops = (0..1500)
            .map(|i| 100_000. + i as f32 * 40.)
            .collect::<Vec<_>>();
        let placed = Placement {
            height: 400_000.,
            top: 100_000.,
            bottom: 160_000.,
        };
        // Well inside: nothing.
        assert_eq!(
            destination(placed, 500, &tops, 10_000, 130_000., 800.),
            None
        );
        // Within three viewports of the window's end: the ordinal in the
        // viewport's middle.
        let near_end = destination(placed, 500, &tops, 10_000, 158_000., 800.);
        assert_eq!(near_end, Some(500 + (158_400 - 100_000) / 40));
        // Near the start of a window that starts at the very first item:
        // nothing before it to read.
        assert_eq!(destination(placed, 0, &tops, 10_000, 100_100., 800.), None);
        // Outside: the scroll's share of the whole, 299,600 of 399,200
        // scrollable pixels.
        assert_eq!(
            destination(placed, 500, &tops, 10_000, 299_600., 800.),
            Some(7_505)
        );
        // A destination held whole asks for nothing.
        assert_eq!(destination(placed, 0, &tops, 1500, 300_000., 800.), None);
    }

    #[test]
    fn a_dragged_thumb_lands_on_its_share_of_the_items() {
        // The thumb dragged to the middle of a 1,000,000-item destination.
        let height = MAX_HEIGHT;
        let viewport = 800.;
        let middle = (height - viewport) / 2.;
        let ordinal = ordinal_at(middle, height, viewport, 1_000_000);
        assert_eq!(ordinal, 500_000);
        assert_eq!(start_at(ordinal, 1_000_000), 499_500);
        // The ends.
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
        // A short destination held whole never misses.
        assert!(!misses(Placement::whole(3_000.), 20, 0., 800.));
    }

    /// A guard that a grid frame's work depends on the window and the
    /// viewport, never on how many items the destination has: at the top,
    /// the middle and the end of 3,000, a million and fifty million items,
    /// the window asked for, the tiles laid out and the tiles drawn are the
    /// same in number.
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
                // Waterfall, the layout that does the most per item.
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
        // Every window but those of the short destination is a full one.
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
