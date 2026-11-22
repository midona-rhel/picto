//! The grid's hover preview. When the pointer rests on a tile's magnifier,
//! the original is shown over the whole window. Nothing is drawn until the
//! original has decoded and 150 ms have passed since the preview opened.
//! Then a dark backdrop and the image fade in together. The preview takes no
//! pointer events.
use crate::{motion, theme};
use gpui::*;
use std::{
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// The image stays at least this many pixels from each edge of the window.
const INSET: f32 = 48.;
const RADIUS: f32 = 8.;

/// An open preview: the content hash of its file, the time it opened, and
/// its decoded original with the time at which the decode finished.
pub struct Shown {
    pub hash: String,
    opened: Instant,
    image: Option<(Arc<RenderImage>, Instant)>,
    _load: Task<()>,
}

/// The timers and the open preview of a view that shows tiles, which is the
/// library's grid or the collection editor. The view tells this struct where
/// the pointer rests and how to find the path of the hovered file. `of`
/// returns this struct from the view. It is used when a timer fires and when
/// the file has loaded.
#[derive(Default)]
pub struct HoverPreview {
    show: Option<Task<()>>,
    hide: Option<Task<()>>,
    pub shown: Option<Shown>,
}

impl HoverPreview {
    /// Called when the pointer rests on the magnifier of the file `hash`.
    /// `hash` is `None` for a file that gets no preview, such as a video.
    /// The preview opens after `motion::HOVER_PREVIEW_DELAY`, unless that
    /// timer is already running. `resolve` is then called to find the file's
    /// path.
    pub fn rest_on<V: 'static, F>(
        &mut self,
        hash: Option<String>,
        resolve: impl FnOnce(&mut Context<V>) -> F + 'static,
        of: fn(&mut V) -> &mut Self,
        cx: &mut Context<V>,
    ) where
        F: Future<Output = Option<PathBuf>> + 'static,
    {
        self.hide = None;
        if self.show.is_some() {
            return;
        }
        let Some(hash) = hash else {
            return;
        };
        self.show = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(motion::HOVER_PREVIEW_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                of(this).show = None;
                let path = resolve(cx);
                of(this).open(hash, path, of, cx);
            });
        }));
    }

    /// Called when the pointer is over the view but not on a magnifier. The
    /// timer that would open a preview is cancelled, and an open preview
    /// closes after `motion::HOVER_PREVIEW_HIDE`.
    pub fn away<V: 'static>(&mut self, of: fn(&mut V) -> &mut Self, cx: &mut Context<V>) {
        self.show = None;
        if self.shown.is_none() || self.hide.is_some() {
            return;
        }
        self.hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(motion::HOVER_PREVIEW_HIDE)
                .await;
            let _ = this.update(cx, |this, cx| {
                let open = of(this).close(cx);
                if open {
                    cx.notify();
                }
            });
        }));
    }

    /// Called when the pointer leaves the view. An open preview closes
    /// after `motion::HOVER_PREVIEW_HIDE`, counted from this call.
    pub fn left<V: 'static>(&mut self, of: fn(&mut V) -> &mut Self, cx: &mut Context<V>) {
        self.hide = None;
        self.away(of, cx);
    }

    /// Closes the preview at once and cancels both timers. A view calls
    /// this when it scrolls. Returns whether a preview was open.
    pub fn close(&mut self, cx: &mut App) -> bool {
        self.show = None;
        self.hide = None;
        let shown = self.shown.take();
        let open = shown.is_some();
        if let Some((image, _)) = shown.and_then(|shown| shown.image) {
            crate::atlas::release(image, cx);
        }
        open
    }

    /// Opens the preview of the file `hash`. `path` resolves to the file's
    /// path, and the file is then decoded on a background thread. Does
    /// nothing when the preview of that file is already open.
    fn open<V: 'static>(
        &mut self,
        hash: String,
        path: impl Future<Output = Option<PathBuf>> + 'static,
        of: fn(&mut V) -> &mut Self,
        cx: &mut Context<V>,
    ) {
        if self.shown.as_ref().is_some_and(|shown| shown.hash == hash) {
            return;
        }
        if let Some((image, _)) = self.shown.take().and_then(|shown| shown.image) {
            crate::atlas::release(image, cx);
        }
        let expected = hash.clone();
        let load = cx.spawn(async move |this, cx| {
            let Some(path) = path.await else {
                return;
            };
            let load = cx.update(|cx| {
                let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
                cx.background_executor().spawn(load)
            });
            let Ok(image) = load.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match &mut of(this).shown {
                Some(shown) if shown.hash == expected => {
                    shown.image = Some((image, Instant::now()));
                    cx.notify();
                }
                _ => crate::atlas::release(image, cx),
            });
        });
        self.shown = Some(Shown {
            hash,
            opened: Instant::now(),
            image: None,
            _load: load,
        });
        cx.notify();
    }
}

/// Returns how far the fade-in has gone, from 0 to 1, or `None` while
/// nothing is drawn. The fade-in starts once the image has decoded and
/// `motion::HOVER_PREVIEW_MIN_WAIT` has passed since the preview opened.
/// `since_opened` is the time since the preview opened. `since_decoded` is
/// the time since the image decoded, or `None` if it has not decoded yet.
pub fn strength(since_opened: Duration, since_decoded: Option<Duration>) -> Option<f32> {
    let since_decoded = since_decoded?;
    let waited = since_opened.checked_sub(motion::HOVER_PREVIEW_MIN_WAIT)?;
    let since_visible = waited.min(since_decoded);
    Some(motion::EASE.ease(motion::progress(since_visible, motion::HOVER_PREVIEW_FADE)))
}

/// Returns the size the image is drawn at. It is the image's own size,
/// `natural`, when that fits in the window with `INSET` left free on each
/// side. Otherwise the image is scaled down, keeping its proportions, until
/// it fits.
pub fn fitted(natural: (f32, f32), window: (f32, f32)) -> (f32, f32) {
    let room = (window.0 - 2. * INSET, window.1 - 2. * INSET);
    if natural.0 <= 0. || natural.1 <= 0. || room.0 <= 0. || room.1 <= 0. {
        return (0., 0.);
    }
    let scale = crate::viewer::camera::fit_scale(natural, room).min(1.);
    (natural.0 * scale, natural.1 * scale)
}

/// Returns the element that draws the open preview, or `None` while nothing
/// is drawn yet. Also returns whether another frame is needed. One is needed
/// while a decoded image waits for the minimum wait to pass, and while the
/// preview fades in.
pub fn overlay(shown: &Shown, window: &Window) -> (Option<AnyElement>, bool) {
    let now = Instant::now();
    let since_decoded = shown
        .image
        .as_ref()
        .map(|(_, decoded)| now.duration_since(*decoded));
    let Some(strength) = strength(now.duration_since(shown.opened), since_decoded) else {
        // The minimum wait has not passed. If the image has decoded, another
        // frame is requested until the wait has passed.
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
    // The preview is drawn over menus, tooltips and the drag ghost, and under
    // notifications and the window's rim.
    .with_priority(crate::ui::layer::PREVIEW)
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
        // The image decoded at once, so the fade-in starts at 150 ms and ends
        // at 300 ms.
        assert_eq!(strength(ms(300), Some(ms(300))), Some(1.));
        let early = strength(ms(225), Some(ms(225))).unwrap();
        // The image decoded at 250 ms, so the fade-in starts then.
        let late = strength(ms(325), Some(ms(75))).unwrap();
        assert!((early - late).abs() < 1e-6);
        assert!(early > 0. && early < 1.);
    }

    #[test]
    fn the_original_fits_inside_the_window_less_the_inset() {
        // A small image keeps its own size.
        assert_eq!(fitted((400., 300.), (1200., 800.)), (400., 300.));
        // A large image is scaled down until both sides fit, and keeps its
        // proportions.
        assert_eq!(fitted((4000., 2000.), (1196., 896.)), (1100., 550.));
        assert_eq!(fitted((1000., 2000.), (1196., 896.)), (400., 800.));
        assert_eq!(fitted((0., 100.), (1196., 896.)), (0., 0.));
    }
}
