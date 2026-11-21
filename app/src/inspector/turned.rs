//! A turned card of the stacked preview (`.stackItemLeft`,
//! `.stackItemRight`: `rotate(±5deg) scale(0.93)`). GPUI cannot rotate an
//! image, so the card is drawn once, off the drawing thread, into a bitmap
//! of its own: the cover with its rounded corners, the glass ring
//! (`inset 0 0 0 1px rgba(0,0,0,.1)`) and shadow (`0 0 5px rgba(0,0,0,.1)`),
//! turned and scaled as the CSS transform does. A second bitmap holds the
//! card's shape in the rail colour, which is drawn over it at the card's
//! shade (`.stackShade`) so the shade can still fade.
use gpui::RenderImage;
use image::{ImageBuffer, Rgba, imageops};
use std::sync::Arc;

type Buffer = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// The glass ring and shadow are black at 10%.
const GLASS: f32 = 0.1;
/// `box-shadow: 0 0 5px`: a blur radius of 5px is a Gaussian of σ 2.5px.
const SHADOW_SIGMA: f32 = 2.5;
/// How far past the card the shadow is drawn, in CSS pixels.
const SHADOW_REACH: f32 = 6.;

/// The card's shape for one draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// The card's frame before the transform, in CSS pixels.
    pub width: f32,
    pub height: f32,
    /// `scale(…)` of the transform.
    pub scale: f32,
    /// `rotate(…)`, clockwise.
    pub degrees: f32,
    /// `border-radius`, in CSS pixels.
    pub radius: f32,
    /// Device pixels per CSS pixel.
    pub device: f32,
}

impl Shape {
    /// The turned card's bitmap size in device pixels, shadow included.
    pub fn bitmap(&self) -> (u32, u32) {
        let (half_width, half_height) = self.half();
        let (sin, cos) = self.degrees.to_radians().sin_cos();
        let reach = SHADOW_REACH * self.scale * self.device;
        let extent_x = half_width * cos.abs() + half_height * sin.abs() + reach;
        let extent_y = half_width * sin.abs() + half_height * cos.abs() + reach;
        ((2. * extent_x).ceil() as u32, (2. * extent_y).ceil() as u32)
    }

    /// Half the scaled card, in device pixels.
    fn half(&self) -> (f32, f32) {
        let factor = self.scale * self.device / 2.;
        (self.width * factor, self.height * factor)
    }
}

/// The two bitmaps of a turned card and their size in CSS pixels.
pub struct Turned {
    pub card: Arc<RenderImage>,
    pub mask: Arc<RenderImage>,
    pub size: (f32, f32),
}

/// The distance from `(x, y)` to a rounded rectangle centred on the origin,
/// negative inside.
fn rounded_distance(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let radius = radius.min(half_width).min(half_height);
    let qx = x.abs() - half_width + radius;
    let qy = y.abs() - half_height + radius;
    let outside = (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt();
    outside + qx.max(qy).min(0.) - radius
}

/// The complementary error function (Abramowitz and Stegun 7.1.26), close
/// enough for a shadow's falloff.
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1. / (1. + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
    let value = poly * (-z * z).exp();
    if x >= 0. { value } else { 2. - value }
}

/// How much of a pixel at signed distance `distance` lies inside.
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0., 1.)
}

/// One channel of the source, sampled between pixels.
fn sample(source: &Buffer, x: f32, y: f32) -> [f32; 4] {
    let (width, height) = source.dimensions();
    let x = x.clamp(0., (width - 1) as f32);
    let y = y.clamp(0., (height - 1) as f32);
    let (left, top) = (x.floor() as u32, y.floor() as u32);
    let (right, bottom) = ((left + 1).min(width - 1), (top + 1).min(height - 1));
    let (fx, fy) = (x - left as f32, y - top as f32);
    let mut out = [0.; 4];
    // Mixed premultiplied, so a transparent neighbour lends no colour.
    for (px, py, weight) in [
        (left, top, (1. - fx) * (1. - fy)),
        (right, top, fx * (1. - fy)),
        (left, bottom, (1. - fx) * fy),
        (right, bottom, fx * fy),
    ] {
        let pixel = source.get_pixel(px, py).0;
        let alpha = pixel[3] as f32 / 255.;
        for channel in 0..3 {
            out[channel] += pixel[channel] as f32 / 255. * alpha * weight;
        }
        out[3] += alpha * weight;
    }
    out
}

/// Draws the turned card from a decoded cover (GPUI's BGRA). `rail` is the
/// rail colour in BGR order, as the bitmaps are.
pub fn turn(cover: &RenderImage, shape: Shape, rail: [u8; 3]) -> Option<Turned> {
    let size = cover.size(0);
    let source = Buffer::from_raw(
        size.width.0 as u32,
        size.height.0 as u32,
        cover.as_bytes(0)?.to_vec(),
    )?;
    let (card, mask) = draw(&source, shape, rail)?;
    let (width, height) = card.dimensions();
    let wrap = |buffer: Buffer| {
        Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            buffer
        )]))
    };
    Some(Turned {
        card: wrap(card),
        mask: wrap(mask),
        size: (width as f32 / shape.device, height as f32 / shape.device),
    })
}

fn draw(source: &Buffer, shape: Shape, rail: [u8; 3]) -> Option<(Buffer, Buffer)> {
    if source.width() == 0 || source.height() == 0 || shape.width <= 0. || shape.height <= 0. {
        return None;
    }
    let (half_width, half_height) = shape.half();
    // The cover at the card's drawn size, so sampling it needs no more
    // than its neighbours.
    let fitted = imageops::resize(
        source,
        (2. * half_width).round().max(1.) as u32,
        (2. * half_height).round().max(1.) as u32,
        imageops::FilterType::Triangle,
    );
    let (step_x, step_y) = (
        fitted.width() as f32 / (2. * half_width),
        fitted.height() as f32 / (2. * half_height),
    );
    let (width, height) = shape.bitmap();
    let (sin, cos) = shape.degrees.to_radians().sin_cos();
    let unit = shape.scale * shape.device;
    let radius = shape.radius * unit;
    let ring = unit;
    let sigma = SHADOW_SIGMA * unit;
    let mut card = Buffer::new(width, height);
    let mut mask = Buffer::new(width, height);
    let (centre_x, centre_y) = (width as f32 / 2., height as f32 / 2.);
    for y in 0..height {
        for x in 0..width {
            let (px, py) = (x as f32 + 0.5 - centre_x, y as f32 + 0.5 - centre_y);
            // Back from the screen into the card's own axes.
            let (qx, qy) = (px * cos + py * sin, -px * sin + py * cos);
            let distance = rounded_distance(qx, qy, half_width, half_height, radius);
            let inside = coverage(distance);
            let shadow = GLASS * 0.5 * erfc(distance / (sigma * std::f32::consts::SQRT_2));
            let (mut colour, mut alpha) = ([0.; 3], 0.);
            if inside > 0. {
                let texel = sample(
                    &fitted,
                    (qx + half_width) * step_x - 0.5,
                    (qy + half_height) * step_y - 0.5,
                );
                colour = [texel[0], texel[1], texel[2]];
                alpha = texel[3];
                // The ring: the band within one pixel of the edge.
                let band = (inside - coverage(distance + ring)) * GLASS;
                for channel in &mut colour {
                    *channel *= 1. - band;
                }
                alpha += band * (1. - alpha);
                for channel in &mut colour {
                    *channel *= inside;
                }
                alpha *= inside;
            }
            // The shadow lies under the card; it is black, so it adds no colour.
            let total = alpha + shadow * (1. - alpha);
            let out = if total > 0. {
                [
                    (colour[0] / total * 255.).round().clamp(0., 255.) as u8,
                    (colour[1] / total * 255.).round().clamp(0., 255.) as u8,
                    (colour[2] / total * 255.).round().clamp(0., 255.) as u8,
                    (total * 255.).round().clamp(0., 255.) as u8,
                ]
            } else {
                [0; 4]
            };
            card.put_pixel(x, y, Rgba(out));
            mask.put_pixel(
                x,
                y,
                Rgba([rail[0], rail[1], rail[2], (inside * 255.).round() as u8]),
            );
        }
    }
    Some((card, mask))
}

#[cfg(test)]
mod tests {
    use super::{Buffer, Shape, draw, rounded_distance};
    use image::Rgba;

    fn shape(degrees: f32) -> Shape {
        Shape {
            width: 100.,
            height: 50.,
            scale: 1.,
            degrees,
            radius: 6.,
            device: 1.,
        }
    }

    #[test]
    fn the_distance_is_negative_inside_and_rounds_the_corners() {
        assert!(rounded_distance(0., 0., 50., 25., 6.) < 0.);
        assert!((rounded_distance(60., 0., 50., 25., 6.) - 10.).abs() < 1e-4);
        // A corner point of the square is outside the rounded corner.
        assert!(rounded_distance(50., 25., 50., 25., 6.) > 0.);
    }

    #[test]
    fn a_straight_card_fits_its_frame_with_room_for_the_shadow() {
        assert_eq!(shape(0.).bitmap(), (112, 62));
        // Turned, it needs more room both ways.
        let (width, height) = shape(5.).bitmap();
        assert!(width > 112 && height > 62);
    }

    #[test]
    fn the_card_holds_the_cover_and_the_mask_its_shape() {
        let cover = Buffer::from_pixel(10, 10, Rgba([200, 100, 50, 255]));
        let (card, mask) = draw(&cover, shape(5.), [1, 2, 3]).unwrap();
        let (width, height) = card.dimensions();
        let middle = card.get_pixel(width / 2, height / 2).0;
        // The middle is the cover, under no ring.
        assert_eq!(middle, [200, 100, 50, 255]);
        assert_eq!(mask.get_pixel(width / 2, height / 2).0, [1, 2, 3, 255]);
        // The corners of the bitmap are clear of the card and its shadow.
        assert_eq!(card.get_pixel(0, 0).0[3], 0);
        assert_eq!(mask.get_pixel(0, 0).0[3], 0);
    }
}
