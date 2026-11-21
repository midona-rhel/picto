//! The drag of items that starts on a tile of the grid. This file holds the
//! state of that drag, `Drags`, which the grid writes and which the views
//! that accept a drop read; the sidebar is such a view. It also holds the
//! ghost, which is the small picture of the dragged items that follows the
//! pointer, the functions that decide where a drop reorders the items of a
//! folder, and the grid's own handling of the drag.
use super::prefs::ViewPrefs;
use super::thumbnails::Thumbnails;
use super::{drag, format, subfolders};
use crate::theme;
use gpui::*;
use picto_library::RootId;
use picto_library::query::ItemScope;
use picto_library::selection::SelectionTarget;
use std::path::PathBuf;

/// A press on a tile becomes a drag of items once the pointer is this many
/// pixels from the place of the press.
pub const THRESHOLD: f32 = 5.;

/// The ghost's sizes and its border colour. `THUMB` is the side of one
/// thumbnail in pixels.
/// `STACK_OFFSET` is how many pixels each thumbnail is placed to the right of
/// and below the one before it. `RADIUS` is a thumbnail's corner radius.
/// `BORDER` is the colour of a thumbnail's border, white at 20% opacity.
/// `BADGE_HEIGHT` and `BADGE_MIN_WIDTH` are the height and the smallest width
/// of the badge that shows the number of dragged items.
const THUMB: f32 = 44.;
const STACK_OFFSET: f32 = 3.;
const RADIUS: f32 = 4.;
const BORDER: u32 = 0xffffff33;
const BADGE_HEIGHT: f32 = 20.;
const BADGE_MIN_WIDTH: f32 = 20.;
/// The ghost is drawn at 85% opacity.
const OPACITY: f32 = 0.85;

/// What the ghost needs to draw one dragged item. The ghost draws the first
/// three dragged items.
#[derive(Clone)]
pub struct Cover {
    pub thumbnail: PathBuf,
    /// The colour drawn behind the thumbnail. It is the first colour of the
    /// item's palette, or the theme's background colour when the item has no
    /// palette.
    pub matte: Rgba,
}

/// The names under which the ghost pins its thumbnails in the thumbnail cache
/// (`Thumbnails::pin`), one name for each of the three thumbnails.
const GHOST_HOLDERS: [&str; 3] = ["drag-ghost-0", "drag-ghost-1", "drag-ghost-2"];

/// Pins the thumbnails of `covers` in the thumbnail cache, so that the cache
/// does not remove them while the ghost shows them. A call with an empty
/// `covers` removes the three pins.
pub fn pin_covers(thumbnails: &Entity<Thumbnails>, covers: &[Cover], cx: &mut App) {
    thumbnails.update(cx, |thumbnails, _| {
        for (at, holder) in GHOST_HOLDERS.into_iter().enumerate() {
            let cover = covers.get(at);
            thumbnails.pin(
                holder,
                cover.map(|cover| Resource::Path(cover.thumbnail.clone().into())),
            );
        }
    });
}

/// A drag of items that is in progress: which items are dragged, where they
/// were dragged from, and where the pointer is.
#[derive(Clone)]
pub struct ItemDrag {
    /// The ids of the dragged items, in the grid's order. For a query-wide
    /// selection (Select All or a range) this lists only the selected items
    /// that the grid has loaded. A reorder within a folder moves these items.
    pub roots: Vec<RootId>,
    /// The description of the dragged items that a drop sends to the library.
    /// When the pressed tile was selected, it describes the grid's whole
    /// selection, including selected items that the grid has not loaded. When
    /// the pressed tile was not selected, it describes that tile's item alone.
    pub target: SelectionTarget,
    /// The number of items that `target` describes.
    pub count: u64,
    /// The scope of the destination that the items were dragged from. When it
    /// is the Inbox or the Trash, a drop on a folder also makes the items
    /// active.
    pub source: ItemScope,
    pub covers: Vec<Cover>,
    pub position: Point<Pixels>,
}

impl ItemDrag {
    /// Whether every dragged item is among the items the grid has loaded.
    /// A selection made over the whole query, for example with Select All,
    /// can hold items that the grid has not loaded, and `roots` then lists
    /// fewer items than `count`.
    ///
    /// A reorder within a folder moves only the items in `roots`. With items
    /// missing from `roots`, it would move one part of the selection and
    /// leave the other part where it was, so such a drag does not reorder.
    pub fn all_loaded(&self) -> bool {
        self.roots.len() as u64 == self.count
    }
}

pub enum DragEvent {
    /// Emitted when a drag starts and each time the pointer moves during it.
    /// `ItemDrag::position` holds the pointer's position.
    Moved,
    /// The mouse button was released during the drag. The value is the drag as
    /// it was at that moment. Each view that accepts drops checks whether the
    /// pointer is over one of its drop targets and, if it is, carries out the
    /// drop.
    Dropped(ItemDrag),
    /// The drag ended without a drop, because the pointer left the window. The
    /// grid then gives the items' files to the operating system as a drag of
    /// files, when there are files to give.
    Cancelled,
}

/// For this long after the grid has given its items' files to the operating
/// system as a drag of files, files dragged over the window are not accepted
/// for an import, because they are taken to be those same files.
pub const OUT_GUARD: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Default)]
pub struct Drags {
    pub items: Option<ItemDrag>,
    /// The time at which the grid last gave its items' files to the operating
    /// system as a drag of files. It is `None` if the grid has never done so.
    pub out_since: Option<std::time::Instant>,
}

impl Drags {
    /// Whether the grid gave its items' files to the operating system as a
    /// drag of files less than 3 seconds ago (`OUT_GUARD`). While this is
    /// true, files dragged over the window are taken to be those files. They
    /// are already in the library, so a drop must not import them.
    pub fn dragging_out(&self) -> bool {
        self.out_since
            .is_some_and(|since| since.elapsed() < OUT_GUARD)
    }
}

impl EventEmitter<DragEvent> for Drags {}

struct DragsGlobal(Entity<Drags>);

impl Global for DragsGlobal {}

/// Returns the app's one `Drags` entity. The first call creates it.
pub fn model(cx: &mut App) -> Entity<Drags> {
    if !cx.has_global::<DragsGlobal>() {
        let drags = cx.new(|_| Drags::default());
        cx.set_global(DragsGlobal(drags));
    }
    cx.global::<DragsGlobal>().0.clone()
}

/// Whether `position` is on or outside an edge of the window's viewport. When
/// the pointer gets there during a drag, the grid ends its own drag and gives
/// the files to the operating system.
pub fn left_window(position: Point<Pixels>, viewport: Size<Pixels>) -> bool {
    position.x <= px(0.)
        || position.y <= px(0.)
        || position.x >= viewport.width
        || position.y >= viewport.height
}

/// Whether the pointer at `to` is at least `THRESHOLD` (5) pixels, in a
/// straight line, from the press at `from`.
pub fn past_threshold(from: Point<Pixels>, to: Point<Pixels>) -> bool {
    let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
    dx.hypot(dy) >= THRESHOLD
}

/// Draws the ghost: the thumbnails of up to three dragged items, each 44 px
/// square, each placed 3 px to the right of and below the one before it. The
/// ghost's top-left corner is 24 px to the left of and 24 px above the
/// pointer. When more than one item is dragged, a badge in the theme's
/// primary colour at the ghost's top-right corner shows the number of items.
/// The ghost is drawn on the layer `ui::layer::DRAG`, which is above dialogs,
/// menus and tooltips.
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
    let count = drag.count;
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
    .with_priority(crate::ui::layer::DRAG)
}

/// The side of a tile on which dragged items are put: `Left` puts them before
/// the tile's item in the folder's order, and `Right` puts them after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Finds where a drop at the point (`x`, `y`) would put the dragged items.
/// The point is in the coordinates of `tiles`. Returns the index of a tile
/// and the side of that tile, or `None` when there is no tile to choose.
/// `text_height` is the height of the text rows under a tile's image; those
/// rows are not part of the image. The tiles whose indexes are in `skip` are
/// the dragged ones, and they are never chosen.
///
/// If the point is on a tile's image, that tile is chosen, on its left side
/// when the point is in the left half of the image and on its right side
/// otherwise.
///
/// Otherwise, if `y` is between the top and the bottom of one or more images,
/// the tile is chosen among the tiles of those images. To the left of the
/// leftmost image, that tile's left side is chosen. To the right of the
/// rightmost image, that tile's right side is chosen. In the gap between two
/// neighbouring images, the right side of the left tile is chosen when the
/// point is in the left half of the gap, and the left side of the right tile
/// otherwise.
///
/// Otherwise the tile whose image has its centre nearest to the point is
/// chosen, on its left side when the point is to the left of that centre and
/// on its right side otherwise.
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

/// Draws the reorder marker, which shows where a drop would put the dragged
/// items. It is a vertical line 2 px wide and as tall as the tile's image, in
/// the middle of the gap on the `side` of `tile`. At the top of the line is a
/// triangle that points down, 10 px wide and 7 px tall. `gap` is the space
/// between two tiles in pixels, and `text_height` is the height of the text
/// rows under the image. The marker is placed with the tile's own `x` and
/// `y`, so it must be a child of the element that the tiles are placed in.
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

/// Returns `ordered` with the items of `dragged` moved to the `side` of the
/// item at index `target`. The moved items are put next to each other and
/// keep the order they had in `ordered`. Returns an empty list when the
/// result would be the same as `ordered`, and when `ordered` holds none of
/// the dragged items.
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

/// Whether the open destination `entry` is the grid of items of `scope`. A
/// reply from the library is applied only if this is true for the scope the
/// request was made for, because the user may have opened another destination
/// before the reply arrived.
fn shows(entry: Option<&crate::backend::Navigation>, scope: &ItemScope) -> bool {
    entry.is_some_and(|entry| entry.screen.is_none() && entry.scope == *scope)
}

impl super::GridView {
    /// Called when the pointer moves while the button is held down after a
    /// press on a tile.
    ///
    /// Before a drag has started, it starts one once the pointer is 5 px from
    /// the press (`THRESHOLD`). The drag carries the selection when the
    /// pressed tile is selected, and only that tile's item when it is not.
    /// Starting a drag pins the ghost's thumbnails, asks the session for the
    /// paths of the dragged items' files (`resolve_drag_files`), and stores
    /// the drag in the `Drags` entity.
    ///
    /// During a drag, it stores the pointer's position, which is where the
    /// ghost is drawn, and works out where a drop would go (`track_drop`).
    /// When the pointer reaches an edge of the window, it ends the grid's own
    /// drag and gives the files to the operating system (`hand_out`).
    pub(super) fn drag_moved(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drags = drag::model(cx);
        // When the pointer reaches an edge of the window during a drag, the
        // grid gives the items' files to the operating system as a drag of
        // files and ends its own drag.
        if self.dragging && drag::left_window(position, window.viewport_size()) {
            self.hand_out(window, cx);
            return;
        }
        self.drag_out_waiting = false;
        if self.dragging {
            drags.update(cx, |drags, cx| {
                if let Some(items) = &mut drags.items {
                    items.position = position;
                    cx.emit(drag::DragEvent::Moved);
                }
            });
            self.track_drop(position, cx);
            cx.notify();
            return;
        }
        let Some((ordinal, from)) = self.press else {
            return;
        };
        if !drag::past_threshold(from, position) {
            return;
        }
        let items = {
            let session = self.session.read(cx);
            let theme = theme::theme(cx);
            // When the pressed tile is selected, the drag carries the whole
            // selection, including selected items that the grid has not
            // loaded. The ghost shows thumbnails of loaded items only.
            let selection = session
                .selection_target()
                .filter(|_| session.is_selected(ordinal));
            let (roots, chosen, target, count) = match selection {
                Some(target) => {
                    let chosen = session.selected_loaded();
                    (
                        session.selected_ids(),
                        chosen,
                        target,
                        session.selection_count(),
                    )
                }
                None => {
                    let item = session.item_at(ordinal);
                    let roots: Vec<RootId> = item.iter().map(|item| item.root_id).collect();
                    let count = roots.len() as u64;
                    let target = SelectionTarget::Explicit {
                        root_ids: roots.clone(),
                    };
                    (roots, item.into_iter().collect(), target, count)
                }
            };
            drag::ItemDrag {
                roots,
                target,
                count,
                source: session
                    .nav
                    .get(session.displayed)
                    .map(|entry| entry.scope.clone())
                    .unwrap_or(picto_library::query::ItemScope::All),
                covers: chosen
                    .iter()
                    .take(3)
                    .map(|item| drag::Cover {
                        thumbnail: session.thumbnail(&item.content_hash),
                        matte: item
                            .palette
                            .first()
                            .map(|color| rgb(format::lab_to_rgb(color.l, color.a, color.b)))
                            .unwrap_or(theme.background()),
                    })
                    .collect(),
                position,
            }
        };
        if items.count == 0 {
            self.press = None;
            return;
        }
        drag::pin_covers(&self.thumbnails, &items.covers, cx);
        self.dragging = true;
        self.pointer_left(cx);
        self.resolve_drag_files(&items, window, cx);
        drags.update(cx, |drags, cx| {
            drags.items = Some(items);
            cx.emit(drag::DragEvent::Moved);
        });
        cx.notify();
    }

    /// Called when the button is released after a press on a tile. If no drag
    /// had started, it only forgets the press. If a drag was in progress, it
    /// ends the drag and emits `DragEvent::Dropped`, so that the sidebar takes
    /// the items if the pointer is over one of its rows. If the pointer is
    /// over a subfolder tile of the grid, the items are put into that folder.
    /// Otherwise, if the grid shows a reorder marker, the items are moved to
    /// the marked place. The release also produces a click event on a tile;
    /// `swallow_tile_click` makes the tile ignore that click, so that it does
    /// not change the selection.
    pub(super) fn drag_released(&mut self, cx: &mut Context<Self>) {
        self.press = None;
        self.drag_files = None;
        self.drag_out_waiting = false;
        if !std::mem::take(&mut self.dragging) {
            return;
        }
        self.swallow_tile_click = true;
        drag::pin_covers(&self.thumbnails, &[], cx);
        let mut dropped = None;
        drag::model(cx).update(cx, |drags, cx| {
            if let Some(items) = drags.items.take() {
                dropped = Some(items.clone());
                cx.emit(drag::DragEvent::Dropped(items));
            }
        });
        let into = self.subfolders.drop.take();
        let reorder = self.reorder.take();
        if let Some(items) = dropped {
            match (into.as_deref().and_then(subfolders::folder_of), reorder) {
                (Some(folder), _) => {
                    crate::shell::sidebar::drop_into_folder(&self.session, items, folder, cx)
                }
                (None, Some(target)) => self.reorder_dropped(&items, target, cx),
                _ => {}
            }
        }
        cx.notify();
    }

    /// Works out, while items are dragged, where a drop at `position` would
    /// go, and stores the answer. If the pointer is over a subfolder tile, that
    /// tile is stored in `subfolders.drop`. Otherwise, the tile and side that
    /// `reorder_target` returns are stored in `reorder`, on three conditions:
    /// the grid's items can be reordered by dragging (`reorders`), every
    /// dragged item is loaded, and the pointer is at or below the top of the
    /// items and to the left of the scrollbar's gutter. Both values are
    /// cleared when the pointer is outside the grid.
    fn track_drop(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let inside = self.bounds.contains(&position);
        let tile = inside
            .then(|| self.subfolder_at(position))
            .flatten()
            .map(|index| self.subfolders.children[index].node.clone());
        // A selection with items that are not loaded cannot be reordered as
        // a whole (`ItemDrag::all_loaded`), so it gets no reorder marker.
        let all_loaded = drag::model(cx)
            .read(cx)
            .items
            .as_ref()
            .is_some_and(drag::ItemDrag::all_loaded);
        let reorder = if tile.is_none() && inside && all_loaded && self.reorders(cx) {
            let (x, y) = self.local(position);
            let y = y + self.scroll_top();
            if y >= 0. && x < self.grid_width() {
                let dragged = drag::model(cx)
                    .read(cx)
                    .items
                    .as_ref()
                    .map(|items| {
                        items
                            .roots
                            .iter()
                            .map(|root| root.0)
                            .collect::<std::collections::HashSet<_>>()
                    })
                    .unwrap_or_default();
                let skip = self
                    .layout_ids
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| dragged.contains(id))
                    .map(|(index, _)| index)
                    .collect();
                drag::reorder_target(
                    &self.layout.tiles,
                    x,
                    y - self.placement.top,
                    self.text_height,
                    &skip,
                )
            } else {
                None
            }
        } else {
            None
        };
        if self.subfolders.drop != tile || self.reorder != reorder {
            self.subfolders.drop = tile;
            self.reorder = reorder;
            cx.notify();
        }
    }

    /// Whether a drag within this grid can reorder its items. It can when the
    /// displayed destination is a folder and the grid shows only the folder's
    /// own items: either the view preference `show_subfolders` is off, or the
    /// folder has no subfolders. With `show_subfolders` on, the grid of a
    /// folder that has subfolders also shows the subfolders' items. Those
    /// items are not members of the folder, so the library cannot give them a
    /// position in the folder's order.
    fn reorders(&self, cx: &App) -> bool {
        let session = self.session.read(cx);
        let folder = session.nav.get(session.displayed).is_some_and(|entry| {
            entry.screen.is_none()
                && matches!(
                    entry.scope,
                    ItemScope::Folder { .. } | ItemScope::FolderTree { .. }
                )
        });
        folder && (!session.prefs.show_subfolders || self.subfolders.children.is_empty())
    }

    /// Carries out a drop that reorders the items of a folder. `target` and
    /// `side` are the tile and the side that `reorder_target` chose. It does
    /// nothing unless the items were dragged from a folder and every dragged
    /// item is loaded. It takes the order of the items that the grid has laid
    /// out (`layout_ids`), moves the dragged items to the target with
    /// `plan_reorder`, and asks the library to store that order for the folder
    /// (`reorder_folder_items`). When the library has stored it, the folder's
    /// view preferences are changed to sort by `folder_order`, ascending, with
    /// `show_subfolders` off, so that the grid shows the stored order. The
    /// dragged items are selected again when the results arrive.
    fn reorder_dropped(
        &mut self,
        items: &drag::ItemDrag,
        (target, side): (usize, drag::Side),
        cx: &mut Context<Self>,
    ) {
        let folder = match &items.source {
            ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id } => *folder_id,
            _ => return,
        };
        if !items.all_loaded() {
            return;
        }
        let dragged = items.roots.iter().map(|root| root.0).collect::<Vec<_>>();
        let order = drag::plan_reorder(&self.layout_ids, &dragged, target, side);
        if order.is_empty() {
            return;
        }
        let reselect = items.roots.clone();
        let source = items.source.clone();
        let order = order.into_iter().map(RootId).collect::<Vec<_>>();
        // With `Report::default()`, the user gets no notification if the
        // library refuses the reorder. The error is only written to the
        // console. `Reread::NOTHING` is passed because `update_view` below
        // reads the folder's results again, in the new order.
        crate::session::edit_then(
            &self.session,
            crate::session::Report::default(),
            crate::session::Reread::NOTHING,
            move |library| library.reorder_folder_items(folder, &order),
            cx,
            move |this, reordered, cx| {
                if reordered.is_err() {
                    return;
                }
                this.session.update(cx, |session, cx| {
                    // `update_view` changes the view preferences of the open
                    // destination. So it is called only if the open
                    // destination is still the folder that was reordered. The
                    // user may have opened another destination while the
                    // library was storing the order.
                    if !shows(session.nav.get(session.scope), &source) {
                        return;
                    }
                    session.reselect = Some(reselect);
                    session.update_view(
                        serde_json::json!({
                            "sort_field": "folder_order",
                            "sort_order": "ascending",
                            "show_subfolders": false,
                        }),
                        true,
                        cx,
                    )
                });
            },
        );
    }

    /// Returns the reorder marker for the tile and side in `reorder`, or
    /// `None` when `reorder` is not set. The tile's `y` is measured from the
    /// top of the scrollable height that stands for every item of the
    /// destination (`placed`), because the marker is drawn in the element of
    /// that height.
    pub(super) fn reorder_marker(&self, prefs: &ViewPrefs) -> Option<AnyElement> {
        let (target, side) = self.reorder?;
        let tile = self.placed(target)?;
        Some(drag::reorder_marker(&tile, side, prefs.gap, self.text_height).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::{Side, left_window, past_threshold, plan_reorder, reorder_target, shows};
    use picto_library::FolderId;
    use picto_library::query::ItemScope;

    #[test]
    fn a_reply_applies_only_while_its_destination_is_shown() {
        let folder = |id| ItemScope::Folder {
            folder_id: FolderId(id),
        };
        let entry = crate::backend::Navigation {
            label: "Art".into(),
            count: None,
            scope: folder(1),
            screen: None,
            random: false,
            icon: None,
            tint: None,
            group: false,
            watched: false,
            depth: 0,
            parent: None,
        };
        assert!(shows(Some(&entry), &folder(1)));
        assert!(!shows(Some(&entry), &folder(2)));
        assert!(!shows(None, &folder(1)));
    }

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
        // In the gap between two images of a row, the point gives the tile
        // whose image is nearer, on the side that faces the gap.
        assert_eq!(
            reorder_target(&tiles, 103., 50., 20., &none),
            Some((0, Side::Right))
        );
        assert_eq!(
            reorder_target(&tiles, 108., 50., 20., &none),
            Some((1, Side::Left))
        );
        // A point to the right of the row's last image gives the right side of
        // that tile.
        assert_eq!(
            reorder_target(&tiles, 400., 50., 20., &none),
            Some((1, Side::Right))
        );
        // The `y` of a point in the text rows under an image is between the
        // top and the bottom of no image, so the point gives the tile whose
        // image has its centre nearest to it.
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
