//! Items dragged out of the grid (`dragState.ts`): one model the grid
//! drives and the views that take drops (the sidebar's rows) read, and the
//! ghost that follows the pointer (`DragGhost.tsx`).
use super::prefs::ViewPrefs;
use super::thumbnails::Thumbnails;
use super::{drag, format, subfolders};
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

/// The thumbnail cache's holders for the covers the ghost shows.
const GHOST_HOLDERS: [&str; 3] = ["drag-ghost-0", "drag-ghost-1", "drag-ghost-2"];

/// Keeps the dragged items' covers decoded while the ghost shows them;
/// `&[]` lets them go.
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

/// Whether the destination (`entry`, the session's current one) is still
/// the grid of `scope`, the one a reply was asked for.
fn shows(entry: Option<&crate::backend::Navigation>, scope: &ItemScope) -> bool {
    entry.is_some_and(|entry| entry.screen.is_none() && entry.scope == *scope)
}

impl super::GridView {
    /// The pointer moved with the button held after a press on a tile:
    /// past 5px the selection, or that tile if it is not selected, is
    /// dragged (`CanvasGrid.tsx`), and the ghost follows the pointer.
    pub(super) fn drag_moved(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drags = drag::model(cx);
        // Out of the window the system takes the files over
        // (`startNativeDrag`).
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
            // The selection, loaded or not, when the tile is in it; the
            // ghost shows the loaded ones.
            let (roots, chosen) = if session.is_selected(ordinal) {
                (session.selected_ids(), session.selected_loaded())
            } else {
                let item = session.item_at(ordinal);
                (
                    item.iter().map(|item| item.root_id).collect(),
                    item.into_iter().collect(),
                )
            };
            drag::ItemDrag {
                roots,
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
        if items.roots.is_empty() {
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

    /// The button came up after a press on a tile: a drag hands its items
    /// to whatever is under the pointer, and the click it makes selects
    /// nothing.
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

    /// While items are dragged over the grid: the subfolder tile they would
    /// go into, or, in a folder showing its own items alone, where among
    /// them they would be put (`isOverGridItems`, `computeReorderTarget`).
    fn track_drop(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let inside = self.bounds.contains(&position);
        let tile = inside
            .then(|| self.subfolder_at(position))
            .flatten()
            .map(|index| self.subfolders.children[index].node.clone());
        let reorder = if tile.is_none() && inside && self.reorders(cx) {
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

    /// Whether dragging items within this grid puts them in an order of
    /// their own: a folder showing only its own items. (A folder showing
    /// its subfolders' items too holds items that are not its members, which
    /// the library cannot order in it.)
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

    /// `dragController.executeDrop`'s reorder: the window's order with the
    /// dragged items moved (`folders.items.reorder`), then the folder shown
    /// in that order (`useManualFolderOrder`), the dragged items selected.
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
        let dragged = items.roots.iter().map(|root| root.0).collect::<Vec<_>>();
        let order = drag::plan_reorder(&self.layout_ids, &dragged, target, side);
        if order.is_empty() {
            return;
        }
        let reselect = items.roots.clone();
        let source = items.source.clone();
        let answer = self.session.update(cx, |session, _| {
            session.dispatch(
                "folders.items.reorder",
                serde_json::json!({ "folder_id": folder.0, "root_ids": order }),
            )
        });
        let session = self.session.clone();
        cx.spawn(async move |_, cx| {
            match answer.await {
                Ok(Ok(_)) => {
                    let _ = session.update(cx, |session, cx| {
                        // The folder is shown in its new order only while it
                        // is still the destination: the view preferences
                        // written are the destination's.
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
                }
                // Production leaves a failed drop to the console.
                Ok(Err(error)) => eprintln!("picto: [drag] drop failed: {error}"),
                Err(_) => {}
            }
        })
        .detach();
    }

    /// The reorder marker beside the target tile, placed in the whole
    /// destination's height.
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
