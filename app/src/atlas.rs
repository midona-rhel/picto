//! Pictures the app decodes itself (thumbnails, originals, page drawings)
//! are drawn from GPUI's sprite atlas, where every window keeps each one
//! until it is told to let go. This is the one way to let go.
use gpui::{App, RenderImage};
use std::sync::Arc;

/// Takes a picture nothing draws any more out of the sprite atlas of every
/// window. Done once no window is being updated: the window that is drawing
/// or handling input is out of the app's list meanwhile, so a release made
/// there would not reach it and the picture would stay.
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

/// Whether any window's atlas still holds the picture. The atlases answer
/// this only in test builds, where the off-screen tool runs.
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

    /// A picture let go of while its own window is being updated (from a
    /// listener, or while the window draws) leaves that window's atlas, as
    /// one let go of from outside any window does.
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

    /// Why `release` waits: GPUI's own `drop_image` without the window,
    /// called while that window is updated, misses it.
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
