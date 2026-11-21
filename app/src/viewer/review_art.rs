//! The bitmaps of the Inbox review controls that GPUI cannot draw itself.
//! The first kind is the glow of each half of the pill: two radial
//! gradients, three inset shadows and a fade towards the divider, clipped to
//! the half's rounded shape. It is drawn at every step of brightness that
//! the flash of a press goes through. The second kind is the two drop
//! shadows under each glyph, drawn at every step between the resting shadows
//! and the lit ones. The glyphs themselves are drawn over the bitmaps as SVG
//! icons. All bitmaps are computed once for each display scale, as
//! straight-alpha BGRA, which is the pixel format that GPUI's images take.
use gpui::RenderImage;
use image::{ImageBuffer, Rgba};
use std::sync::Arc;

type Buffer = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// The width of each half of the pill. `HEIGHT` is the height of the pill.
pub const HALF_WIDTH: f32 = 64.;
pub const HEIGHT: f32 = 54.;
/// The distance between a half's edge and its glow, on every side.
pub const GLOW_INSET: f32 = 1.;
/// The size of the check mark and of the cross. Each is centred in its half.
pub const GLYPH: f32 = 28.;
/// The room left around the glyph for its widest shadow, which has a blur
/// radius of 8px.
pub const SHADOW_MARGIN: f32 = 16.;
/// How many steps of brightness the flash is drawn at, from the normal
/// brightness to twice the normal brightness.
pub const FLASH_STEPS: usize = 16;
/// How many steps the glyph's shadow is drawn at, from the resting shadow to
/// the lit one.
pub const SHADOW_STEPS: usize = 9;

/// One half of the pill. The glow of the accept half is centred at 58% of
/// the half's width and fades out towards the right. The glow of the reject
/// half is centred at 42% and fades out towards the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    Accept,
    Reject,
}

/// All the bitmaps drawn for one display scale.
pub struct Art {
    pub scale: f32,
    /// The glow, indexed by half and then by flash step. Step 0 is the
    /// normal brightness.
    pub glow: [Vec<Arc<RenderImage>>; 2],
    /// The glyph's shadow, indexed by half and then by step, from the
    /// resting shadow (step 0) to the lit one.
    pub shadow: [Vec<Arc<RenderImage>>; 2],
}

impl Art {
    /// Draws every bitmap for the display `scale`. `colors` are the glow
    /// colours of the accept half and of the reject half.
    pub fn draw(scale: f32, colors: [[f32; 3]; 2]) -> Self {
        let half = |index: usize| {
            if index == 0 {
                Half::Accept
            } else {
                Half::Reject
            }
        };
        let glow = std::array::from_fn(|index| {
            let base = glow(half(index), colors[index], scale);
            (0..FLASH_STEPS)
                .map(|step| {
                    let brightness = 1. + step as f32 / (FLASH_STEPS - 1) as f32;
                    wrap(base.encode(brightness))
                })
                .collect()
        });
        let shadow = std::array::from_fn(|index| {
            (0..SHADOW_STEPS)
                .map(|step| {
                    let lit = step as f32 / (SHADOW_STEPS - 1) as f32;
                    wrap(glyph_shadow(half(index), colors[index], lit, scale).encode(1.))
                })
                .collect()
        });
        Self {
            scale,
            glow,
            shadow,
        }
    }
}

/// Returns the flash step for a brightness between 1, which is the normal
/// brightness, and 2, which is twice the normal brightness.
pub fn flash_step(brightness: f32) -> usize {
    (((brightness - 1.) * (FLASH_STEPS - 1) as f32).round() as usize).min(FLASH_STEPS - 1)
}

/// Returns the shadow step for `lit`, which says how far the glyph's shadow
/// is from the resting shadow (0) to the lit one (1).
pub fn shadow_step(lit: f32) -> usize {
    ((lit.clamp(0., 1.) * (SHADOW_STEPS - 1) as f32).round() as usize).min(SHADOW_STEPS - 1)
}

fn wrap(buffer: Buffer) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        buffer
    )]))
}

/// A grid of premultiplied RGBA pixels, one for each device pixel.
struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![[0.; 4]; width * height],
        }
    }

    /// Draws a layer over the canvas. The layer has the colour `rgb`
    /// everywhere, and `alpha` gives its opacity at each pixel.
    fn lay(&mut self, rgb: [f32; 3], alpha: &[f32]) {
        for (pixel, &a) in self.pixels.iter_mut().zip(alpha) {
            for channel in 0..3 {
                pixel[channel] = rgb[channel] * a + pixel[channel] * (1. - a);
            }
            pixel[3] = a + pixel[3] * (1. - a);
        }
    }

    /// Draws the premultiplied pixels of `below` under what the canvas
    /// already holds.
    fn under(&mut self, below: &Canvas) {
        for (pixel, low) in self.pixels.iter_mut().zip(&below.pixels) {
            let keep = 1. - pixel[3];
            for channel in 0..4 {
                pixel[channel] += low[channel] * keep;
            }
        }
    }

    /// Multiplies each pixel's alpha, and with it the pixel's premultiplied
    /// colour, by the value that `alpha` holds for that pixel.
    fn mask(&mut self, alpha: &[f32]) {
        for (pixel, &a) in self.pixels.iter_mut().zip(alpha) {
            for channel in pixel.iter_mut() {
                *channel *= a;
            }
        }
    }

    /// Returns the canvas as straight-alpha BGRA bytes, with each colour
    /// channel multiplied by `brightness`. The colour is multiplied after
    /// the alpha has been divided out of it, and the result is limited to
    /// the range from 0 to 1.
    fn encode(&self, brightness: f32) -> Buffer {
        let mut bytes = Vec::with_capacity(self.width * self.height * 4);
        for pixel in &self.pixels {
            let a = pixel[3].clamp(0., 1.);
            let straight = |channel: usize| {
                if a <= 0. {
                    0.
                } else {
                    (pixel[channel] / a * brightness).clamp(0., 1.)
                }
            };
            let byte = |value: f32| (value * 255.).round() as u8;
            bytes.extend([
                byte(straight(2)),
                byte(straight(1)),
                byte(straight(0)),
                byte(a),
            ]);
        }
        Buffer::from_raw(self.width as u32, self.height as u32, bytes)
            .expect("the canvas holds width × height pixels")
    }
}

/// A rounded rectangle with one radius for each corner (top-left, top-right,
/// bottom-right, bottom-left), in points.
#[derive(Clone, Copy)]
struct Rounded {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    radii: [f32; 4],
}

impl Rounded {
    /// Returns the signed distance of the point from the shape's edge. It is
    /// negative inside the shape.
    fn distance(&self, x: f32, y: f32) -> f32 {
        let (half_w, half_h) = (self.width / 2., self.height / 2.);
        let (px, py) = (x - self.left - half_w, y - self.top - half_h);
        let radius = match (px >= 0., py >= 0.) {
            (false, false) => self.radii[0],
            (true, false) => self.radii[1],
            (true, true) => self.radii[2],
            (false, true) => self.radii[3],
        }
        .min(half_w)
        .min(half_h);
        let qx = px.abs() - half_w + radius;
        let qy = py.abs() - half_h + radius;
        let outside = (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt();
        outside + qx.max(qy).min(0.) - radius
    }

    /// Returns how much of the device pixel whose centre is at (`x`, `y`)
    /// points is inside the shape, from 0 to 1.
    fn coverage(&self, x: f32, y: f32, scale: f32) -> f32 {
        (0.5 - self.distance(x, y) * scale).clamp(0., 1.)
    }

    /// Returns the shape moved by (`dx`, `dy`) and enlarged by `grow` on
    /// every side. The radius of each rounded corner is enlarged by `grow`
    /// too, and a square corner stays square.
    fn moved(self, dx: f32, dy: f32, grow: f32) -> Self {
        Self {
            left: self.left + dx - grow,
            top: self.top + dy - grow,
            width: self.width + 2. * grow,
            height: self.height + 2. * grow,
            radii: self.radii.map(|radius| {
                if radius > 0. {
                    (radius + grow).max(0.)
                } else {
                    0.
                }
            }),
        }
    }
}

/// Returns `values`, a grid of `width` × `height`, blurred with a Gaussian
/// whose standard deviation is `sigma` device pixels. The blur is run as a
/// horizontal pass and then a vertical pass. A position outside the grid
/// counts as having the value `outside`.
fn blur(values: &[f32], width: usize, height: usize, sigma: f32, outside: f32) -> Vec<f32> {
    if sigma <= 0. {
        return values.to_vec();
    }
    let reach = (sigma * 3.).ceil() as isize;
    let kernel = (-reach..=reach)
        .map(|offset| (-(offset * offset) as f32 / (2. * sigma * sigma)).exp())
        .collect::<Vec<_>>();
    let total: f32 = kernel.iter().sum();
    let kernel = kernel
        .iter()
        .map(|weight| weight / total)
        .collect::<Vec<_>>();
    let pass = |source: &[f32], horizontal: bool| {
        let mut out = vec![0.; source.len()];
        for y in 0..height as isize {
            for x in 0..width as isize {
                let mut sum = 0.;
                for (index, weight) in kernel.iter().enumerate() {
                    let offset = index as isize - reach;
                    let (sx, sy) = if horizontal {
                        (x + offset, y)
                    } else {
                        (x, y + offset)
                    };
                    let value = if sx < 0 || sy < 0 || sx >= width as isize || sy >= height as isize
                    {
                        outside
                    } else {
                        source[sy as usize * width + sx as usize]
                    };
                    sum += weight * value;
                }
                out[y as usize * width + x as usize] = sum;
            }
        }
        out
    };
    pass(&pass(values, true), false)
}

/// Returns a grid with one value for each device pixel of a box that is
/// `width` × `height` device pixels. `value` is called with the centre of
/// each pixel, in points.
fn sample(width: usize, height: usize, scale: f32, value: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            out.push(value((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale));
        }
    }
    out
}

/// Returns the alpha of a radial gradient at the distance `t` from its
/// centre, where 1 is the gradient's radius. `stops` are pairs of position
/// and alpha. The alpha is interpolated between two stops, and past the last
/// stop it is the alpha of that stop.
fn radial_alpha(t: f32, stops: &[(f32, f32)]) -> f32 {
    let Some(&(first, first_alpha)) = stops.first() else {
        return 0.;
    };
    if t <= first {
        return first_alpha;
    }
    for pair in stops.windows(2) {
        let ((from, a), (to, b)) = (pair[0], pair[1]);
        if t <= to {
            return a + (b - a) * (t - from) / (to - from);
        }
    }
    stops.last().map_or(0., |stop| stop.1)
}

/// Draws the glow of one half at the normal brightness.
fn glow(half: Half, color: [f32; 3], scale: f32) -> Canvas {
    let (box_w, box_h) = (HALF_WIDTH - 2. * GLOW_INSET, HEIGHT - 2. * GLOW_INSET);
    let width = (box_w * scale).round() as usize;
    let height = (box_h * scale).round() as usize;
    // The glow has the half's shape: fully rounded on the outer side and
    // square on the side of the divider. The radius is half of the box's
    // height.
    let round = box_h / 2.;
    let shape = Rounded {
        left: 0.,
        top: 0.,
        width: box_w,
        height: box_h,
        radii: match half {
            Half::Accept => [round, 0., 0., round],
            Half::Reject => [0., round, round, 0.],
        },
    };
    let across = match half {
        Half::Accept => 0.58,
        Half::Reject => 0.42,
    };
    let mut canvas = Canvas::new(width, height);
    // The lower layer is a radial gradient in the shape of an ellipse. Its
    // radii are 50% of the box's width and 55% of its height, and its centre
    // is at `across` of the width and half of the height. Its alpha goes
    // from 0.28 at the centre to 0 at 75% of the radius.
    let core = sample(width, height, scale, |x, y| {
        let (dx, dy) = (
            (x - across * box_w) / (0.5 * box_w),
            (y - 0.5 * box_h) / (0.55 * box_h),
        );
        radial_alpha((dx * dx + dy * dy).sqrt(), &[(0., 0.28), (0.75, 0.)])
    });
    canvas.lay(color, &core);
    // The upper layer is a radial gradient whose centre is below the bottom
    // edge. Its radii are 110% of the box's width and 95% of its height, and
    // its centre is at `across` of the width and 118% of the height. Its
    // alpha goes from 0.9 at the centre through 0.45 at 38% and 0.12 at 64%
    // to 0 at 80% of the radius.
    let wash = sample(width, height, scale, |x, y| {
        let (dx, dy) = (
            (x - across * box_w) / (1.1 * box_w),
            (y - 1.18 * box_h) / (0.95 * box_h),
        );
        radial_alpha(
            (dx * dx + dy * dy).sqrt(),
            &[(0., 0.9), (0.38, 0.45), (0.64, 0.12), (0.8, 0.)],
        )
    });
    canvas.lay(color, &wash);
    // The three inset shadows, each drawn over the one before: a white line
    // of 6% opacity along the top edge, a shadow in the glow colour of 22%
    // opacity with an 8px blur, and a shadow in the glow colour of 95%
    // opacity along the bottom edge (moved up by 3px, 7px blur, spread of
    // -3px).
    let white = [1., 1., 1.];
    for (rgb, alpha, dy, blur_radius, spread) in [
        (white, 0.06, 1., 0., 0.),
        (color, 0.22, 0., 8., 0.),
        (color, 0.95, -3., 7., -3.),
    ] {
        let shadow = inset_shadow(shape, dy, blur_radius, spread, width, height, scale);
        let layer = shadow.iter().map(|value| value * alpha).collect::<Vec<_>>();
        canvas.lay(rgb, &layer);
    }
    // The glow is clipped to the half's rounded shape. It also fades out
    // towards the divider: it has its full strength over the outer 70% of
    // the width and falls to nothing at the divider.
    let clip = sample(width, height, scale, |x, y| {
        let across = match half {
            Half::Accept => x / box_w,
            Half::Reject => 1. - x / box_w,
        };
        let fade = if across <= 0.7 {
            1.
        } else {
            (1. - (across - 0.7) / 0.3).clamp(0., 1.)
        };
        shape.coverage(x, y, scale) * fade
    });
    canvas.mask(&clip);
    canvas
}

/// Returns the strength (0 to 1), at each device pixel, of an inset shadow
/// inside `shape`. The shadow is everything outside the shape after the
/// shape has been moved down by `dy` and shrunk by `spread` on every side
/// (enlarged when `spread` is negative), blurred with a Gaussian whose
/// standard deviation is half of `blur_radius`.
fn inset_shadow(
    shape: Rounded,
    dy: f32,
    blur_radius: f32,
    spread: f32,
    width: usize,
    height: usize,
    scale: f32,
) -> Vec<f32> {
    let hole = shape.moved(0., dy, -spread);
    // The shadow is computed on a grid that is larger than the box by a
    // border on every side. The border is wide enough to hold the moved
    // shape and the reach of the blur. Everything beyond the border is
    // outside the shape.
    let pad = (blur_radius * 1.5 + dy.abs() + spread.abs() + 1.).ceil();
    let border = (pad * scale).round() as usize;
    let (wide, tall) = (width + 2 * border, height + 2 * border);
    let outside = sample(wide, tall, scale, |x, y| {
        1. - hole.coverage(x - pad, y - pad, scale)
    });
    let blurred = blur(&outside, wide, tall, blur_radius / 2. * scale, 1.);
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = (y + border) * wide + border;
        out.extend_from_slice(&blurred[row..row + width]);
    }
    out
}

/// Returns how much of the device pixel at (`x`, `y`) points the glyph's
/// strokes cover. The glyph is defined in a box of 24 units, which is scaled
/// to `GLYPH` points and placed at (`left`, `top`). Its strokes are 2 units
/// wide and have round ends, as the icons of the Tabler set do.
fn glyph_alpha(half: Half, left: f32, top: f32, x: f32, y: f32, scale: f32) -> f32 {
    let unit = GLYPH / 24.;
    let strokes: &[[(f32, f32); 2]] = match half {
        // The check mark, which is the SVG path `M5 12l5 5l10 -10`.
        Half::Accept => &[[(5., 12.), (10., 17.)], [(10., 17.), (20., 7.)]],
        // The cross, which is the SVG paths `M18 6l-12 12` and `M6 6l12 12`.
        Half::Reject => &[[(18., 6.), (6., 18.)], [(6., 6.), (18., 18.)]],
    };
    let (ux, uy) = ((x - left) / unit, (y - top) / unit);
    let distance = strokes
        .iter()
        .map(|&[(ax, ay), (bx, by)]| {
            let (dx, dy) = (bx - ax, by - ay);
            let along = (((ux - ax) * dx + (uy - ay) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
            ((ux - ax - along * dx).powi(2) + (uy - ay - along * dy).powi(2)).sqrt()
        })
        .fold(f32::MAX, f32::min);
    ((1. - distance) * unit * scale + 0.5).clamp(0., 1.)
}

/// Draws what lies under the glyph of one half: its two drop shadows, at
/// `lit` of the way from the resting shadows to the lit ones. The first
/// shadow is black, 1px below the glyph, with a 2px blur. Its opacity goes
/// from 0.58 at rest to 0.5 when lit. The second shadow is not moved. It
/// goes from white at 8% opacity with a 7px blur at rest to the glow colour
/// at 75% opacity with an 8px blur when lit. The second shadow is cast by
/// the glyph together with the first shadow. The bitmap is the glyph's box
/// with `SHADOW_MARGIN` added on every side.
fn glyph_shadow(half: Half, color: [f32; 3], lit: f32, scale: f32) -> Canvas {
    let side = GLYPH + 2. * SHADOW_MARGIN;
    let size = (side * scale).round() as usize;
    let lerp = |from: f32, to: f32| from + (to - from) * lit;
    let glyph = sample(size, size, scale, |x, y| {
        glyph_alpha(half, SHADOW_MARGIN, SHADOW_MARGIN, x, y, scale)
    });
    // The first shadow is the glyph's shape one point lower, blurred.
    let lowered = sample(size, size, scale, |x, y| {
        glyph_alpha(half, SHADOW_MARGIN, SHADOW_MARGIN + 1., x, y, scale)
    });
    let dark = blur(&lowered, size, size, 2. / 2. * scale, 0.)
        .iter()
        .map(|value| value * lerp(0.58, 0.5))
        .collect::<Vec<_>>();
    // The second shadow is cast by the glyph and the first shadow together.
    let together = glyph
        .iter()
        .zip(&dark)
        .map(|(g, d)| g + d * (1. - g))
        .collect::<Vec<_>>();
    let spread = blur(&together, size, size, lerp(7., 8.) / 2. * scale, 0.);
    // The colour of the second shadow goes from white at 8% opacity to the
    // glow colour at 75% opacity. The two are mixed premultiplied, and the
    // mix is divided by its opacity to get the colour.
    let white_strength = 0.08 * (1. - lit);
    let glow_strength = 0.75 * lit;
    let strength = white_strength + glow_strength;
    let rgb: [f32; 3] = std::array::from_fn(|channel| {
        if strength <= 0. {
            0.
        } else {
            (white_strength + color[channel] * glow_strength) / strength
        }
    });
    let mut light = Canvas::new(size, size);
    light.lay(
        rgb,
        &spread
            .iter()
            .map(|value| value * strength)
            .collect::<Vec<_>>(),
    );
    let mut canvas = Canvas::new(size, size);
    canvas.lay([0., 0., 0.], &dark);
    canvas.under(&light);
    canvas
}

#[cfg(test)]
mod tests {
    use super::{Half, Rounded, blur, flash_step, glyph_alpha, radial_alpha, shadow_step};

    #[test]
    fn gradient_stops_interpolate_and_end_transparent() {
        let stops = [(0., 0.9), (0.38, 0.45), (0.64, 0.12), (0.8, 0.)];
        assert_eq!(radial_alpha(0., &stops), 0.9);
        assert!((radial_alpha(0.19, &stops) - 0.675).abs() < 1e-5);
        assert_eq!(radial_alpha(0.8, &stops), 0.);
        assert_eq!(radial_alpha(2., &stops), 0.);
    }

    #[test]
    fn rounded_halves_are_round_on_their_outer_side_only() {
        let accept = Rounded {
            left: 0.,
            top: 0.,
            width: 62.,
            height: 52.,
            radii: [26., 0., 0., 26.],
        };
        // The outer corner is rounded off, so the point there is outside the
        // shape. The inner corner is square, so the point there is inside.
        assert_eq!(accept.coverage(0.5, 0.5, 2.), 0.);
        assert_eq!(accept.coverage(61.75, 0.25, 2.), 1.);
        assert_eq!(accept.coverage(31., 26., 2.), 1.);
        assert_eq!(accept.coverage(70., 26., 2.), 0.);
    }

    #[test]
    fn a_blur_keeps_a_flat_field_flat() {
        let flat = vec![1.; 25];
        for value in blur(&flat, 5, 5, 1.5, 1.) {
            assert!((value - 1.).abs() < 1e-5);
        }
        let empty = blur(&[0.; 25], 5, 5, 1.5, 1.);
        assert!(empty[0] > 0. && empty[12] < empty[0]);
    }

    #[test]
    fn glyph_strokes_are_covered_on_their_line() {
        // The corner of the check mark is at (10, 17) of the 24 units, in a
        // box of 28 points.
        let unit = 28. / 24.;
        assert_eq!(
            glyph_alpha(Half::Accept, 0., 0., 10. * unit, 17. * unit, 2.),
            1.
        );
        assert_eq!(glyph_alpha(Half::Accept, 0., 0., 2., 2., 2.), 0.);
        assert_eq!(glyph_alpha(Half::Reject, 0., 0., 14., 14., 2.), 1.);
    }

    #[test]
    fn steps_cover_the_ranges() {
        assert_eq!(flash_step(1.), 0);
        assert_eq!(flash_step(2.), 15);
        assert_eq!(flash_step(9.), 15);
        assert_eq!(shadow_step(0.), 0);
        assert_eq!(shadow_step(1.), 8);
        assert_eq!(shadow_step(0.5), 4);
    }
}
