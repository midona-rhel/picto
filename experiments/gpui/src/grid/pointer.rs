//! The handling of the pointer that the library grid (`GridView`) and the
//! collection editor (`GroupEditor`) share. Both show tiles from a `Layout`
//! and follow the pointer in the same way. The functions here find the tile
//! under the pointer, start and stop the hover preview of the tile whose
//! magnifier icon the pointer is on, update a rubber band selection, scroll
//! the view while a rubber band is held near its top or bottom edge, and
//! build the hover preview that is drawn over the window.
//!
//! Each view keeps its own state and passes the parts a function needs. The
//! selection, and the way a tile's file is found, differ between the two
//! views and stay in them.
use super::layout::Layout;
use super::marquee::{self, Marquee};
use super::preview::{self, HoverPreview};
use gpui::*;

/// Records the tile whose image the pointer is over. `hit` is that tile's
/// index in the layout with the pointer's position in the layout's
/// coordinates, or `None` when the pointer is over no image. `hovered` is
/// the view's record of the tile's index. Returns true when the index
/// changed, so that the view asks to be drawn again.
pub fn hover(hovered: &mut Option<usize>, hit: Option<(usize, f32, f32)>) -> bool {
    let index = hit.map(|(index, _, _)| index);
    if index == *hovered {
        return false;
    }
    *hovered = index;
    true
}

/// Returns the index of the tile in `hit` when the pointer is on that tile's
/// magnifier icon, and `None` otherwise. `text_height` is the height of the
/// text rows under each image.
pub fn magnifier(
    hit: Option<(usize, f32, f32)>,
    layout: &Layout,
    text_height: f32,
) -> Option<usize> {
    hit.filter(|&(index, x, y)| marquee::on_magnifier(&layout.tiles[index], x, y, text_height))
        .map(|(index, _, _)| index)
}

/// Called when the pointer leaves the view. It clears the pointer's
/// position, cancels the timer that would open the hover preview and starts
/// the timer that closes an open one. It also clears the hovered tile,
/// unless a rubber band selection is in progress (`banding`). `of` returns
/// the view's hover preview, which the preview's timers use. Returns true
/// when the hovered tile was cleared, so that the view asks to be drawn
/// again.
pub fn left<V: 'static>(
    pointer: &mut Option<Point<Pixels>>,
    hovered: &mut Option<usize>,
    banding: bool,
    preview: &mut HoverPreview,
    of: fn(&mut V) -> &mut HoverPreview,
    cx: &mut Context<V>,
) -> bool {
    *pointer = None;
    let unhovered = !banding && hovered.take().is_some();
    preview.left(of, cx);
    unhovered
}

/// Updates the rubber band selection in progress for a move of the pointer.
/// `local` is the pointer's position in pixels from the view's top-left
/// corner, `top` is how far the tiles are scrolled, and `viewport` is the
/// view's height. The rubber band's rectangle and the tiles it covers are
/// computed again, and so is the speed at which the view scrolls while the
/// pointer is near the top or the bottom edge. Returns false when no rubber
/// band selection is in progress, and true otherwise.
pub fn band_moved(
    marquee: Option<&mut Marquee>,
    local: (f32, f32),
    top: f32,
    viewport: f32,
    layout: &Layout,
    text_height: f32,
) -> bool {
    let Some(marquee) = marquee else {
        return false;
    };
    marquee.pointer = local;
    marquee.speed = marquee::auto_scroll_speed(local.1, viewport);
    marquee.track(top, layout, text_height);
    true
}

/// Scrolls the view by the rubber band's speed. A view calls this once in
/// each frame. The speed is not 0 while the pointer of a rubber band is near
/// the view's top or bottom edge. `current` is how far the view's content is
/// scrolled, in pixels. Returns true when the view scrolled, so that the
/// view requests another frame. It returns false when there is no rubber
/// band, when its speed is 0, and when the view is already scrolled as far
/// as it goes in that direction.
pub fn autoscroll(marquee: Option<&Marquee>, scroll: &ScrollHandle, current: f32) -> bool {
    let Some(speed) = marquee
        .map(|marquee| marquee.speed)
        .filter(|speed| *speed != 0.)
    else {
        return false;
    };
    let limit = f32::from(scroll.max_offset().y).max(0.);
    let next = (current + speed).clamp(0., limit);
    if next == current {
        return false;
    }
    scroll.set_offset(point(px(0.), px(-next)));
    true
}

/// Returns the element that draws the hover preview over the window, or
/// `None` when no preview is open or the open one shows nothing yet.
/// Another frame is requested while the decoded picture waits for the
/// preview's minimum delay to pass, and while the preview fades in.
pub fn preview_overlay(preview: &HoverPreview, window: &mut Window) -> Option<AnyElement> {
    let shown = preview.shown.as_ref()?;
    let (overlay, fading) = preview::overlay(shown, window);
    if fading {
        window.request_animation_frame();
    }
    overlay
}
