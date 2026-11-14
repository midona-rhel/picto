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

#[cfg(test)]
mod tests {
    use super::{left_window, past_threshold};
    use gpui::{point, px, size};

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
    fn a_press_drags_only_after_five_pixels() {
        let from = point(px(10.), px(10.));
        assert!(!past_threshold(from, point(px(13.), px(13.))));
        assert!(past_threshold(from, point(px(14.), px(13.))));
    }
}
