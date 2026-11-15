//! The pointer over a grid of tiles, as the grid and the collection editor
//! both follow it: the tile under it, the hover preview of the tile whose
//! magnifier it rests on, the rubber band and its scrolling at the edges,
//! the view's measured bounds and the preview drawn over the window. Each
//! view keeps what it selects and how it finds a tile's file.
use super::layout::Layout;
use super::marquee::{self, Marquee};
use super::preview::{self, HoverPreview};
use gpui::*;

/// The tile under the pointer is now `hit`'s: whether that changed it.
pub fn hover(hovered: &mut Option<usize>, hit: Option<(usize, f32, f32)>) -> bool {
    let index = hit.map(|(index, _, _)| index);
    if index == *hovered {
        return false;
    }
    *hovered = index;
    true
}

/// The tile whose magnifier `hit` is on, if it is on one.
pub fn magnifier(hit: Option<(usize, f32, f32)>, layout: &Layout, text_height: f32) -> Option<usize> {
    hit.filter(|&(index, x, y)| marquee::on_magnifier(&layout.tiles[index], x, y, text_height))
        .map(|(index, _, _)| index)
}

/// The pointer left the view: it is forgotten, the hovered tile too unless
/// a rubber band is out, and the preview closes. Whether to draw again.
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

/// A rubber band out follows the pointer at `local` (from the view's
/// top-left corner), with the items scrolled `top` and the view
/// `viewport` tall: its speed at the edges and what it covers. Whether one
/// is out.
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

/// A rubber band near the top or bottom edge scrolls the view, which is
/// scrolled `current`, each frame until it can go no further: whether it
/// moved, and so needs the next frame.
pub fn autoscroll(marquee: Option<&Marquee>, scroll: &ScrollHandle, current: f32) -> bool {
    let Some(speed) = marquee.map(|marquee| marquee.speed).filter(|speed| *speed != 0.) else {
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

/// Keeps the view's bounds (`bounds` of the view) as laid out, drawing
/// again when they change.
pub fn measure<V: 'static>(
    entity: WeakEntity<V>,
    bounds: fn(&mut V) -> &mut Bounds<Pixels>,
) -> impl IntoElement + Styled {
    canvas(
        move |laid_out, window, cx| {
            let _ = entity.update(cx, |this, cx| {
                let kept = bounds(this);
                if *kept != laid_out {
                    *kept = laid_out;
                    cx.notify();
                    // A notification made while a frame is being drawn
                    // does not by itself bring another frame.
                    window.request_animation_frame();
                }
            });
        },
        |_, _, _, _| {},
    )
}

/// The hover preview over the window, while one shows.
pub fn preview_overlay(preview: &HoverPreview, window: &mut Window) -> Option<AnyElement> {
    let shown = preview.shown.as_ref()?;
    let (overlay, fading) = preview::overlay(shown, window);
    if fading {
        window.request_animation_frame();
    }
    overlay
}
