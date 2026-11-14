//! Items dragged out of the grid (`dragState.ts`): one model the grid
//! drives and the views that take drops (the sidebar's rows) read, and the
//! ghost that follows the pointer (`DragGhost.tsx`).
use super::thumbnails::Thumbnails;
use crate::theme;
use gpui::*;
use picto_library::RootId;
use picto_library::query::ItemScope;
use std::path::PathBuf;

/// A press on a tile becomes a drag once the pointer has moved this far.
pub const THRESHOLD: f32 = 5.;

/// `dragGhostSpec.ts`.
const THUMB: f32 = 44.;
const STACK_OFFSET: f32 = 3.;
const RADIUS: f32 = 4.;
const BORDER: u32 = 0xffffff33;
const BADGE_HEIGHT: f32 = 20.;
const BADGE_MIN_WIDTH: f32 = 20.;
/// The ghost is drawn at 85%.
const OPACITY: f32 = 0.85;

/// One of the first three dragged items, as the ghost shows it.
#[derive(Clone)]
pub struct Cover {
    pub thumbnail: PathBuf,
    /// The item's dominant colour, behind its thumbnail.
    pub matte: Rgba,
}

/// The items being dragged.
#[derive(Clone)]
pub struct ItemDrag {
    pub roots: Vec<RootId>,
    /// The scope they were dragged from: a drop into a folder from the Inbox
    /// or the Trash also makes them active.
    pub source: ItemScope,
    pub covers: Vec<Cover>,
    pub position: Point<Pixels>,
}

pub enum DragEvent {
    /// The pointer moved with the items.
    Moved,
    /// The button came up; whoever is under the pointer takes them.
    Dropped(ItemDrag),
    /// The drag ended without a drop: it left the window and the system
    /// carries the files on (`cancelDrag` after `startNativeDrag`).
    Cancelled,
}

/// How long files dragged out of the window are not taken back as an import
/// when they come over it again (`startNativeDrag`'s 3 s
/// `internalDragOrigin`).
pub const OUT_GUARD: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Default)]
pub struct Drags {
    pub items: Option<ItemDrag>,
    /// When the grid last handed its items to the system as files.
    pub out_since: Option<std::time::Instant>,
}

impl Drags {
    /// Whether files over the window are the grid's own, dragged out just
    /// now (`isInternalDragOrigin`), which a drop must not import again.
    pub fn dragging_out(&self) -> bool {
        self.out_since
            .is_some_and(|since| since.elapsed() < OUT_GUARD)
    }
}

impl EventEmitter<DragEvent> for Drags {}

struct DragsGlobal(Entity<Drags>);

impl Global for DragsGlobal {}

/// The one drag model, made on first use.
pub fn model(cx: &mut App) -> Entity<Drags> {
    if !cx.has_global::<DragsGlobal>() {
        let drags = cx.new(|_| Drags::default());
        cx.set_global(DragsGlobal(drags));
    }
    cx.global::<DragsGlobal>().0.clone()
}

/// Whether the pointer has left the window's viewport, where the system
/// takes a drag over (`clientX <= 0 || … >= innerWidth`).
pub fn left_window(position: Point<Pixels>, viewport: Size<Pixels>) -> bool {
    position.x <= px(0.)
        || position.y <= px(0.)
        || position.x >= viewport.width
        || position.y >= viewport.height
}

/// Whether the pointer has gone far enough from the press to drag.
pub fn past_threshold(from: Point<Pixels>, to: Point<Pixels>) -> bool {
    let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
    dx.hypot(dy) >= THRESHOLD
}

/// `DragGhost`: up to three thumbnails stacked 3px apart, centred on the
/// pointer, with the count on a primary badge when there is more than one.
pub fn ghost(drag: &ItemDrag, thumbnails: &Entity<Thumbnails>, cx: &App) -> impl IntoElement {
    let theme = theme::theme(cx);
    let stack = drag.covers.len().min(3);
    let side = THUMB + (stack.max(1) - 1) as f32 * STACK_OFFSET;
    let mut frame = div().relative().w(px(side)).h(px(side)).opacity(OPACITY);
    for (index, cover) in drag.covers.iter().take(3).enumerate() {
        let offset = index as f32 * STACK_OFFSET;
        frame = frame.child(
            div()
                .absolute()
                .left(px(offset))
                .top(px(offset))
                .size(px(THUMB))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(rgba(BORDER))
                .bg(cover.matte)
                .shadow(vec![
                    BoxShadow::new(px(0.), px(2.), rgba(theme::BLACK_30).into())
                        .blur_radius(px(8.)),
                ])
                .child(
                    img(cover.thumbnail.clone())
                        .image_cache(thumbnails)
                        .size_full()
                        .rounded(px(RADIUS))
                        .object_fit(ObjectFit::Cover),
                ),
        );
    }
    let count = drag.roots.len();
    if count > 1 {
        frame = frame.child(
            div()
                .absolute()
                .top(px(-6.))
                .right(px(-6.))
                .h(px(BADGE_HEIGHT))
                .min_w(px(BADGE_MIN_WIDTH))
                .px(px(5.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.))
                .bg(theme.primary())
                .text_size(px(10.))
                .line_height(px(BADGE_HEIGHT))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.on_primary())
                .child(count.to_string()),
        );
    }
    deferred(
        anchored()
            .position(drag.position - point(px(24.), px(24.)))
            .child(frame),
    )
    .with_priority(30)
}

/// Which side of a tile dragged items go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// `computeReorderTarget`: where items dragged to a layout point (`x`, `y`)
/// go, beside the tile whose image is under it, else beside the nearest in
/// its row, else beside the nearest tile of all. The dragged tiles (`skip`)
/// are never a target.
pub fn reorder_target(
    tiles: &[super::layout::Tile],
    x: f32,
    y: f32,
    text_height: f32,
    skip: &std::collections::HashSet<usize>,
) -> Option<(usize, Side)> {
    let side = |tile: &super::layout::Tile| {
        if x < tile.x + tile.w / 2. {
            Side::Left
        } else {
            Side::Right
        }
    };
    let candidates = || {
        tiles
            .iter()
            .enumerate()
            .filter(|(index, _)| !skip.contains(index))
    };
    let in_band = |tile: &super::layout::Tile| y >= tile.y && y <= tile.y + tile.h - text_height;
    if let Some((index, tile)) =
        candidates().find(|(_, tile)| in_band(tile) && x >= tile.x && x <= tile.x + tile.w)
    {
        return Some((index, side(tile)));
    }
    let mut band: Vec<(usize, &super::layout::Tile)> =
        candidates().filter(|(_, tile)| in_band(tile)).collect();
    band.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
    if let (Some(first), Some(last)) = (band.first(), band.last()) {
        if x < first.1.x {
            return Some((first.0, Side::Left));
        }
        if x > last.1.x + last.1.w {
            return Some((last.0, Side::Right));
        }
        for pair in band.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if x >= left.1.x + left.1.w && x <= right.1.x {
                let middle = (left.1.x + left.1.w + right.1.x) / 2.;
                return Some(if x < middle {
                    (left.0, Side::Right)
                } else {
                    (right.0, Side::Left)
                });
            }
        }
    }
    let mut best: Option<(f32, usize, Side)> = None;
    for (index, tile) in candidates() {
        let (cx, cy) = (tile.x + tile.w / 2., tile.y + (tile.h - text_height) / 2.);
        let distance = (x - cx).powi(2) + (y - cy).powi(2);
        if best.is_none_or(|(nearest, _, _)| distance < nearest) {
            best = Some((distance, index, side(tile)));
        }
    }
    best.map(|(_, index, side)| (index, side))
}

/// `CanvasGrid`'s reorder drop indicator: a 2px line in the gap at `side`
/// of the tile's image, with a triangle at its top, in the coordinates the
/// tile is laid out in.
pub fn reorder_marker(tile: &super::layout::Tile, side: Side, gap: f32, text_height: f32) -> Div {
    let x = match side {
        Side::Left => tile.x - gap / 2.,
        Side::Right => tile.x + tile.w + gap / 2.,
    };
    let color = rgb(theme::REORDER_INDICATOR);
    div()
        .absolute()
        .left_0()
        .top_0()
        .child(
            div()
                .absolute()
                .left(px(x - 1.))
                .top(px(tile.y))
                .w(px(2.))
                .h(px(tile.h - text_height))
                .bg(color),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let origin = bounds.origin;
                    let mut path = PathBuilder::fill();
                    path.move_to(origin);
                    path.line_to(origin + point(px(10.), px(0.)));
                    path.line_to(origin + point(px(5.), px(7.)));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .absolute()
            .left(px(x - 5.))
            .top(px(tile.y))
            .w(px(10.))
            .h(px(7.)),
        )
}

/// `planFolderReorder`: the order with the dragged items moved, as one block
/// in their own order, to `side` of the item at `target`. Empty when nothing
/// moves.
pub fn plan_reorder<T: Copy + PartialEq>(
    ordered: &[T],
    dragged: &[T],
    target: usize,
    side: Side,
) -> Vec<T> {
    let moved: Vec<T> = ordered
        .iter()
        .copied()
        .filter(|item| dragged.contains(item))
        .collect();
    if moved.is_empty() {
        return Vec::new();
    }
    let at = if side == Side::Right {
        target + 1
    } else {
        target
    };
    let before = ordered[..at.min(ordered.len())]
        .iter()
        .filter(|item| dragged.contains(item))
        .count();
    let rest: Vec<T> = ordered
        .iter()
        .copied()
        .filter(|item| !dragged.contains(item))
        .collect();
    let insert = at.saturating_sub(before).min(rest.len());
    let mut next = rest[..insert].to_vec();
    next.extend(moved);
    next.extend_from_slice(&rest[insert..]);
    if next == ordered { Vec::new() } else { next }
}

#[cfg(test)]
mod tests {
    use super::{Side, left_window, past_threshold, plan_reorder, reorder_target};
    use crate::grid::layout::Tile;
    use gpui::{point, px, size};
    use std::collections::HashSet;

    #[test]
    fn a_drag_leaves_the_window_at_its_edges() {
        let viewport = size(px(1213.), px(865.));
        assert!(!left_window(point(px(600.), px(400.)), viewport));
        assert!(left_window(point(px(0.), px(400.)), viewport));
        assert!(left_window(point(px(600.), px(-3.)), viewport));
        assert!(left_window(point(px(1213.), px(400.)), viewport));
        assert!(left_window(point(px(600.), px(900.)), viewport));
    }

    #[test]
    fn a_dragged_block_moves_whole_and_in_order() {
        assert_eq!(
            plan_reorder(&[1, 2, 3, 4, 5], &[2, 3], 4, Side::Right),
            vec![1, 4, 5, 2, 3]
        );
        assert_eq!(
            plan_reorder(&[1, 2, 3, 4, 5], &[4], 0, Side::Left),
            vec![4, 1, 2, 3, 5]
        );
    }

    #[test]
    fn a_drop_inside_the_dragged_block_changes_nothing() {
        assert!(plan_reorder(&[1, 2, 3, 4], &[2, 3], 2, Side::Right).is_empty());
        assert!(plan_reorder(&[1, 2, 3], &[9], 0, Side::Left).is_empty());
    }

    fn tile(x: f32, y: f32) -> Tile {
        Tile {
            x,
            y,
            w: 100.,
            h: 120.,
        }
    }

    #[test]
    fn the_target_is_the_image_under_the_pointer_by_its_half() {
        let tiles = [tile(0., 0.), tile(110., 0.), tile(0., 130.)];
        let none = HashSet::new();
        assert_eq!(
            reorder_target(&tiles, 140., 50., 20., &none),
            Some((1, Side::Left))
        );
        assert_eq!(
            reorder_target(&tiles, 190., 50., 20., &none),
            Some((1, Side::Right))
        );
        // In the gap between two images of a row: the nearer side of it.
        assert_eq!(
            reorder_target(&tiles, 103., 50., 20., &none),
            Some((0, Side::Right))
        );
        assert_eq!(
            reorder_target(&tiles, 108., 50., 20., &none),
            Some((1, Side::Left))
        );
        // Past the row's end, after its last image.
        assert_eq!(
            reorder_target(&tiles, 400., 50., 20., &none),
            Some((1, Side::Right))
        );
        // Under the name row: the nearest image of all.
        assert_eq!(
            reorder_target(&tiles, 20., 115., 20., &none),
            Some((0, Side::Left))
        );
        // A dragged tile is never the target.
        let skip = HashSet::from([1]);
        assert_eq!(
            reorder_target(&tiles, 140., 50., 20., &skip),
            Some((0, Side::Right))
        );
    }

    #[test]
    fn a_press_drags_only_after_five_pixels() {
        let from = point(px(10.), px(10.));
        assert!(!past_threshold(from, point(px(13.), px(13.))));
        assert!(past_threshold(from, point(px(14.), px(13.))));
    }
}
