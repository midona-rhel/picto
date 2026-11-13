//! The difference view of two images. The Electron screen stacks the right
//! image over the left with `mix-blend-mode: difference` on black; GPUI has
//! no blend modes, so the same picture is computed once per pair, off the
//! drawing thread, from the decoded images.
use gpui::RenderImage;
use image::{ImageBuffer, Rgba, imageops};
use std::sync::Arc;

/// The longest side of the computed image. A larger original is compared at
/// this size, which is plenty for a pane and keeps the work bounded.
const LIMIT: u32 = 4096;

type Buffer = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// The difference of two decoded images, at the left image's proportions:
/// the right one is stretched onto it, as `object-fit: fill` does in the
/// composite. `None` if either image is empty.
pub fn compute(left: &RenderImage, right: &RenderImage) -> Option<Arc<RenderImage>> {
    let buffer = |image: &RenderImage| -> Option<Buffer> {
        let size = image.size(0);
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        Buffer::from_raw(width, height, image.as_bytes(0)?.to_vec())
    };
    let pixels = difference(&buffer(left)?, &buffer(right)?)?;
    Some(Arc::new(RenderImage::new(smallvec::smallvec![
        image::Frame::new(pixels)
    ])))
}

/// Each channel is `|left − right|` of the two images laid on black, and the
/// result is opaque. Channel order does not matter, so this works on GPUI's
/// BGRA as well as on RGBA.
fn difference(left: &Buffer, right: &Buffer) -> Option<Buffer> {
    let (width, height) = bounded(left.dimensions())?;
    let fitted = |image: &Buffer| -> Option<Buffer> {
        if image.width() == 0 || image.height() == 0 {
            return None;
        }
        Some(if image.dimensions() == (width, height) {
            image.clone()
        } else {
            imageops::resize(image, width, height, imageops::FilterType::Triangle)
        })
    };
    let (left, right) = (fitted(left)?, fitted(right)?);
    let mut out = Buffer::new(width, height);
    for ((a, b), pixel) in left.pixels().zip(right.pixels()).zip(out.pixels_mut()) {
        let on_black = |p: &Rgba<u8>, channel: usize| p.0[channel] as u32 * p.0[3] as u32 / 255;
        for channel in 0..3 {
            pixel.0[channel] = on_black(a, channel).abs_diff(on_black(b, channel)) as u8;
        }
        pixel.0[3] = 255;
    }
    Some(out)
}

/// The size to compare at: the left image's, scaled down to the limit.
fn bounded((width, height): (u32, u32)) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let longest = width.max(height);
    if longest <= LIMIT {
        return Some((width, height));
    }
    let scale = LIMIT as f64 / longest as f64;
    Some((
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    ))
}

#[cfg(test)]
mod tests {
    use super::{Buffer, LIMIT, bounded, difference};
    use image::Rgba;

    #[test]
    fn identical_images_are_black() {
        let image = Buffer::from_pixel(4, 3, Rgba([200, 100, 50, 255]));
        let out = difference(&image, &image).unwrap();
        assert_eq!(out.dimensions(), (4, 3));
        assert!(out.pixels().all(|p| p.0 == [0, 0, 0, 255]));
    }

    #[test]
    fn channels_differ_by_their_distance_on_black() {
        let left = Buffer::from_pixel(2, 2, Rgba([200, 100, 50, 255]));
        let right = Buffer::from_pixel(2, 2, Rgba([50, 150, 50, 255]));
        let out = difference(&left, &right).unwrap();
        assert_eq!(out.get_pixel(0, 0).0, [150, 50, 0, 255]);
        // A transparent pixel counts as black.
        let clear = Buffer::from_pixel(2, 2, Rgba([255, 255, 255, 0]));
        assert_eq!(
            difference(&left, &clear).unwrap().get_pixel(1, 1).0,
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn the_right_image_is_stretched_onto_the_left() {
        let left = Buffer::from_pixel(8, 4, Rgba([10, 10, 10, 255]));
        let right = Buffer::from_pixel(3, 3, Rgba([10, 10, 10, 255]));
        let out = difference(&left, &right).unwrap();
        assert_eq!(out.dimensions(), (8, 4));
        assert!(out.pixels().all(|p| p.0 == [0, 0, 0, 255]));
        assert!(difference(&left, &Buffer::new(0, 0)).is_none());
    }

    #[test]
    fn large_images_are_compared_at_the_limit() {
        assert_eq!(bounded((800, 600)), Some((800, 600)));
        assert_eq!(bounded((LIMIT * 2, LIMIT)), Some((LIMIT, LIMIT / 2)));
        assert_eq!(bounded((0, 10)), None);
    }
}
