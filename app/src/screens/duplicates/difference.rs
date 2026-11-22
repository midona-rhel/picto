//! Computes the difference picture of two images and counts the pixels that
//! differ.
//!
//! The difference picture is black where the two images are equal, and
//! brighter the more they differ. GPUI has no blend mode that draws one
//! image over another in this way, so this module computes the picture from
//! the two decoded images. The screen runs it on a background thread.
//!
//! While the picture is computed, two numbers are counted: the share of
//! pixels that differ at all, and the share of pixels that differ enough to
//! be visible. [`Measure::sentence`] writes them as text. The text states
//! only what was counted. It never says that two pictures are the same.
use crate::i18n::{en, t};
use gpui::RenderImage;
use image::{ImageBuffer, Rgba, imageops};
use std::sync::Arc;

/// The largest width or height, in pixels, of the difference picture. Two
/// larger images are scaled down to this size before they are compared.
/// This limits the time and the memory that the comparison takes, and the
/// picture is still larger than a pane of the screen.
const LIMIT: u32 = 4096;

type Buffer = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// A pixel counts as visibly different when one of its colour channels
/// differs by more than this number, on the scale from 0 to 255. Smaller
/// differences are typical of the compression of a lossy format such as
/// JPEG.
const VISIBLE: u8 = 24;

/// The result of comparing two pictures pixel by pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    /// The sizes in pixels of the left and the right picture, when the two
    /// sizes are different. The right picture was then scaled to the size
    /// of the left picture before the comparison. This is `None` when the
    /// two pictures have the same size.
    pub sizes: Option<((u32, u32), (u32, u32))>,
    /// The share of pixels that differ at all, from 0 to 1.
    pub changed: f32,
    /// The share of pixels that differ visibly (by more than [`VISIBLE`]),
    /// from 0 to 1.
    pub visible: f32,
}

/// Writes a share from 0 to 1 as a percentage: "0%" for zero, "<0.1%" for
/// a share below a tenth of a percent, "100%" for a share that rounds to
/// 100.0%, and otherwise one decimal, for example "0.5%".
fn share(value: f32) -> String {
    let percent = value * 100.;
    if percent <= 0. {
        "0%".to_owned()
    } else if percent < 0.1 {
        "<0.1%".to_owned()
    } else if percent >= 99.95 {
        "100%".to_owned()
    } else {
        format!("{percent:.1}%")
    }
}

impl Measure {
    /// Writes the result as two pieces of text. The first says what share
    /// of the pixels differ, for example "0% of pixels differ" or "<0.1% of
    /// pixels differ visibly". The second is `None` when there is nothing
    /// to add. Otherwise it says what share of the pixels differ at all, if
    /// that is more than the share that differ visibly, and it says that the
    /// two pictures had different sizes, if they had.
    pub fn sentence(&self) -> (String, Option<String>) {
        let counted = if self.visible > 0. {
            format!("{} {}", share(self.visible), en("of pixels differ visibly"))
        } else if self.changed > 0. {
            format!(
                "{} {}",
                share(self.changed),
                en("of pixels differ, none visibly")
            )
        } else {
            format!("{} {}", share(0.), en("of pixels differ"))
        };
        let mut detail = Vec::new();
        if self.visible > 0. && self.changed > self.visible {
            detail.push(format!("{} {}", share(self.changed), en("differ at all")));
        }
        if let Some(((aw, ah), (bw, bh))) = self.sizes {
            detail.push(format!(
                "{} {aw} × {ah} {} {bw} × {bh}, {}",
                en("sizes differ,"),
                t("and"),
                en("compared stretched")
            ));
        }
        (counted, (!detail.is_empty()).then(|| detail.join(" · ")))
    }
}

/// Computes the difference picture of two decoded images and counts the
/// pixels that differ. The picture has the proportions of the left image.
/// The right image is scaled to the size of the left image, and its own
/// proportions are not kept. The result is `None` when either image has no
/// pixels.
pub fn compute(left: &RenderImage, right: &RenderImage) -> Option<(Arc<RenderImage>, Measure)> {
    let buffer = |image: &RenderImage| -> Option<Buffer> {
        let size = image.size(0);
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        Buffer::from_raw(width, height, image.as_bytes(0)?.to_vec())
    };
    let (left, right) = (buffer(left)?, buffer(right)?);
    let sizes =
        (left.dimensions() != right.dimensions()).then(|| (left.dimensions(), right.dimensions()));
    let pixels = difference(&left, &right)?;
    let measure = measure(&pixels, sizes);
    Some((
        Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            pixels
        )])),
        measure,
    ))
}

/// Counts the pixels of a difference picture that are not black, and the
/// pixels of which a colour channel is greater than [`VISIBLE`].
fn measure(pixels: &Buffer, sizes: Option<((u32, u32), (u32, u32))>) -> Measure {
    let (mut changed, mut visible) = (0u64, 0u64);
    for pixel in pixels.pixels() {
        let most = pixel.0[0].max(pixel.0[1]).max(pixel.0[2]);
        changed += (most > 0) as u64;
        visible += (most > VISIBLE) as u64;
    }
    let total = (pixels.width() as u64 * pixels.height() as u64).max(1) as f32;
    Measure {
        sizes,
        changed: changed as f32 / total,
        visible: visible as f32 / total,
    }
}

/// Computes the difference picture. Each image is first drawn on black, so
/// a transparent pixel counts as black. Each colour channel of the result
/// is the absolute difference of that channel in the two images, and the
/// result is fully opaque. The calculation is the same for every colour
/// channel, so it gives the right result for GPUI's BGRA pixels as well as
/// for RGBA pixels.
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

/// The size at which two images are compared: the size of the left image,
/// scaled down so that neither side is longer than [`LIMIT`]. The result is
/// `None` when the width or the height is zero.
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
        // A fully transparent pixel counts as black, whatever its colour.
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
