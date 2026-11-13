//! The hover preview of `HoverPreviewPortal.tsx`: resting the pointer on a
//! tile's magnifier shows the original over the whole window. Nothing appears
//! until it has decoded and 150 ms have passed; then the backdrop darkens and
//! the image fades in together. It takes no pointer events.
use crate::{motion, theme};
use gpui::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// `HOVER_PREVIEW_DELAY_MS`: how long the pointer rests on the magnifier.
pub const SHOW_DELAY: Duration = Duration::from_millis(200);
/// `HOVER_HIDE_DELAY_MS`: how long after leaving it the preview stays.
pub const HIDE_DELAY: Duration = Duration::from_millis(90);
/// `MIN_WAIT_MS`: the preview never appears sooner than this after it opens.
const MIN_WAIT: Duration = Duration::from_millis(150);
/// `PREVIEW_INSET`: the image keeps this far from each edge of the window.
const INSET: f32 = 48.;
const RADIUS: f32 = 8.;

/// An open preview: which file, since when, and its decoded original.
pub struct Shown {
    pub hash: String,
    opened: Instant,
    image: Option<(Arc<RenderImage>, Instant)>,
    _load: Task<()>,
}

impl Shown {
    pub fn new(hash: String, load: Task<()>) -> Self {
        Self {
            hash,
            opened: Instant::now(),
            image: None,
            _load: load,
        }
    }

    pub fn decoded(&mut self, image: Arc<RenderImage>) {
        self.image = Some((image, Instant::now()));
    }

    /// The decoded original, to release when the preview closes.
    pub fn take_image(&mut self) -> Option<Arc<RenderImage>> {
        self.image.take().map(|(image, _)| image)
    }
}

/// The timers and the open preview.
#[derive(Default)]
pub struct HoverPreview {
    pub show: Option<Task<()>>,
    pub hide: Option<Task<()>>,
    pub shown: Option<Shown>,
}

impl HoverPreview {
    /// Closes the preview at once and forgets both timers, handing back the
    /// original to release.
    pub fn clear(&mut self) -> Option<Arc<RenderImage>> {
        self.show = None;
        self.hide = None;
        self.shown.take().and_then(|mut shown| shown.take_image())
    }
}

/// How far the fade has gone, 0 to 1, or `None` before anything shows. The
/// fade starts once the image has decoded and the minimum wait has passed.
pub fn strength(since_opened: Duration, since_decoded: Option<Duration>) -> Option<f32> {
    let since_decoded = since_decoded?;
    let waited = since_opened.checked_sub(MIN_WAIT)?;
    let since_visible = waited.min(since_decoded);
    Some(motion::EASE.ease(motion::progress(since_visible, motion::HOVER_PREVIEW_FADE)))
}

/// The image's size: its own, scaled down only as far as needed to fit the
/// window less the inset, keeping its shape (`max-width`/`max-height`).
pub fn fitted(natural: (f32, f32), window: (f32, f32)) -> (f32, f32) {
    let (width, height) = natural;
    if width <= 0. || height <= 0. {
        return (0., 0.);
    }
    let room = (
        (window.0 - 2. * INSET).max(0.),
        (window.1 - 2. * INSET).max(0.),
    );
    let scale = (room.0 / width).min(room.1 / height).min(1.);
    (width * scale, height * scale)
}

/// The overlay for the open preview, if it is showing yet, and whether its
/// fade still needs frames.
pub fn overlay(shown: &Shown, window: &Window) -> (Option<AnyElement>, bool) {
    let now = Instant::now();
    let since_decoded = shown
        .image
        .as_ref()
        .map(|(_, decoded)| now.duration_since(*decoded));
    let Some(strength) = strength(now.duration_since(shown.opened), since_decoded) else {
        // Waiting out the minimum: paint again until it has passed.
        return (None, shown.image.is_some());
    };
    let Some((image, _)) = &shown.image else {
        return (None, false);
    };
    let viewport = window.viewport_size();
    let natural = image.size(0);
    let (width, height) = fitted(
        (
            i32::from(natural.width) as f32,
            i32::from(natural.height) as f32,
        ),
        (f32::from(viewport.width), f32::from(viewport.height)),
    );
    let backdrop = Rgba {
        a: rgba(theme::PREVIEW_BACKDROP).a * strength,
        ..rgba(theme::PREVIEW_BACKDROP)
    };
    let element = deferred(
        anchored()
            .position_mode(AnchoredPositionMode::Window)
            .position(point(px(0.), px(0.)))
            .child(
                div()
                    .w(viewport.width)
                    .h(viewport.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(backdrop)
                    .child(
                        div()
                            .w(px(width))
                            .h(px(height))
                            .flex_none()
                            .rounded(px(RADIUS))
                            .shadow(vec![
                                BoxShadow::new(px(0.), px(8.), rgba(theme::PREVIEW_SHADOW).into())
                                    .blur_radius(px(48.)),
                            ])
                            .opacity(strength)
                            .child(
                                img(image.clone())
                                    .size_full()
                                    .rounded(px(RADIUS))
                                    .object_fit(ObjectFit::Contain),
                            ),
                    ),
            ),
    )
    // Over everything in the window but its rim.
    .with_priority(50)
    .into_any_element();
    (Some(element), strength < 1.)
}

#[cfg(test)]
mod tests {
    use super::{fitted, strength};
    use std::time::Duration;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn nothing_shows_before_the_wait_or_the_decode() {
        assert_eq!(strength(ms(300), None), None);
        assert_eq!(strength(ms(149), Some(ms(149))), None);
        assert_eq!(strength(ms(150), Some(ms(100))), Some(0.));
    }

    #[test]
    fn the_fade_starts_at_whichever_comes_later() {
        // Decoded at once: the fade starts at 150 ms and ends at 300 ms.
        assert_eq!(strength(ms(300), Some(ms(300))), Some(1.));
        let early = strength(ms(225), Some(ms(225))).unwrap();
        // Decoded at 250 ms: the fade starts then.
        let late = strength(ms(325), Some(ms(75))).unwrap();
        assert!((early - late).abs() < 1e-6);
        assert!(early > 0. && early < 1.);
    }

    #[test]
    fn the_original_fits_inside_the_window_less_the_inset() {
        // Small images keep their own size.
        assert_eq!(fitted((400., 300.), (1200., 800.)), (400., 300.));
        // Large ones scale down by the tighter side, keeping their shape.
        assert_eq!(fitted((4000., 2000.), (1196., 896.)), (1100., 550.));
        assert_eq!(fitted((1000., 2000.), (1196., 896.)), (400., 800.));
        assert_eq!(fitted((0., 100.), (1196., 896.)), (0., 0.));
    }
}
