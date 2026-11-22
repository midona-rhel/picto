//! Removes pictures from GPUI's sprite atlas. The pictures the app decodes
//! itself (thumbnails, originals, drawings of document pages) are drawn from
//! that atlas, and every window keeps each picture there until it is told to
//! remove it. All code that removes one calls `release` in this file.
use gpui::{App, RenderImage};
use std::sync::Arc;

/// Removes a picture that nothing draws any more from the sprite atlas of
/// every window. The removal is deferred until no window is being updated:
/// while a window draws or handles input, it is not in the app's list of
/// windows, so a removal made at that moment would not reach it and the
/// picture would stay in its atlas.
pub fn release(image: Arc<RenderImage>, cx: &mut App) {
    cx.defer(move |cx| {
        #[cfg(test)]
        let released = image.clone();
        cx.drop_image(image, None);
        #[cfg(test)]
        crate::stats::image_released(held(&released, cx));
        #[cfg(not(test))]
        crate::stats::image_released(false);
    });
}

/// Returns whether any window's atlas still holds the picture. GPUI can
/// answer this only in test builds, which is where the off-screen tool runs.
#[cfg(test)]
fn held(image: &RenderImage, cx: &mut App) -> bool {
    cx.windows().into_iter().any(|window| {
        window
            .update(cx, |_, window, _| window.has_image_atlas_entry(image))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::release;
    use gpui::{
        AppContext as _, Context, IntoElement, ParentElement as _, Render, RenderImage,
        Styled as _, TestAppContext, Window, div, img, px,
    };
    use std::sync::Arc;

    struct Shows(Option<Arc<RenderImage>>);

    impl Render for Shows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(self.0.clone().map(|image| img(image).w(px(40.)).h(px(30.))))
        }
    }

    fn picture() -> Arc<RenderImage> {
        let pixels = image::RgbaImage::from_pixel(40, 30, image::Rgba([10, 20, 30, 255]));
        Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            pixels
        )]))
    }

    /// Checks that a picture released while its own window is being updated
    /// (from a listener, or while the window draws) is removed from that
    /// window's atlas, as a picture released outside any window is.
    #[test]
    fn a_picture_released_inside_its_window_leaves_the_atlas() {
        let mut cx = TestAppContext::single();
        let window = cx.add_window(|_, _| Shows(Some(picture())));
        let handle: gpui::AnyWindowHandle = window.into();
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .unwrap();
            cx.run_until_parked();
        };
        draw(&mut cx);
        let shown = window
            .read_with(&cx, |shows, _| shows.0.clone().unwrap())
            .unwrap();
        let held = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, _| window.has_image_atlas_entry(&shown))
                .unwrap()
        };
        assert!(held(&mut cx));
        window
            .update(&mut cx, |shows, _, cx| {
                let image = shows.0.take().unwrap();
                release(image, cx);
            })
            .unwrap();
        cx.run_until_parked();
        assert!(!held(&mut cx));
        draw(&mut cx);
        assert!(!held(&mut cx));
    }

    /// Shows why `release` defers the removal: GPUI's `drop_image`, called
    /// without a window while the picture's window is being updated, leaves
    /// the picture in that window's atlas.
    #[test]
    fn dropping_a_picture_inside_its_window_without_it_misses_it() {
        let mut cx = TestAppContext::single();
        let window = cx.add_window(|_, _| Shows(Some(picture())));
        let handle: gpui::AnyWindowHandle = window.into();
        cx.update_window(handle, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
        let shown = window
            .read_with(&cx, |shows, _| shows.0.clone().unwrap())
            .unwrap();
        window
            .update(&mut cx, |shows, _, cx| {
                let image = shows.0.take().unwrap();
                cx.drop_image(image, None);
            })
            .unwrap();
        cx.run_until_parked();
        let held = cx
            .update_window(handle, |_, window, _| window.has_image_atlas_entry(&shown))
            .unwrap();
        assert!(held);
    }
}
