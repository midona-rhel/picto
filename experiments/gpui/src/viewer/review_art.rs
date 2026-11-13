//! The bitmaps of the Inbox review controls (`InboxReviewControls.module.css`)
//! that GPUI cannot draw itself: each half's light (`.glow`: two radial
//! gradients, three inset shadows and a sideways fade mask, clipped to the
//! half's rounded shape), drawn at every step of the press flash's
//! brightness, and the drop shadows under each glyph (`.action svg`'s
//! `filter`), drawn at every step between the resting and the lit shadows.
//! The glyphs themselves are drawn over them as SVGs. All are computed once
//! per display scale, as straight-alpha BGRA, which is what GPUI's images
//! take.
use gpui::RenderImage;
use image::{ImageBuffer, Rgba};
use std::sync::Arc;

type Buffer = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// `.action`: each half of the pill, 64×54.
pub const HALF_WIDTH: f32 = 64.;
pub const HEIGHT: f32 = 54.;
/// `.glow`'s `inset: 1px`.
pub const GLOW_INSET: f32 = 1.;
/// `<IconCheck size={28}>` / `<IconX size={28}>`, centred in the half.
pub const GLYPH: f32 = 28.;
/// Room around the glyph for its widest shadow (8px blur).
pub const SHADOW_MARGIN: f32 = 16.;
/// How many brightness steps the flash is drawn at, from `brightness(1)` to
/// `brightness(2)`.
pub const FLASH_STEPS: usize = 16;
/// How many steps the glyph's shadow is drawn at between rest and lit.
pub const SHADOW_STEPS: usize = 9;

/// Which half: the accept half's light rises at 58% across and fades to the
/// right, the reject half's at 42% and fades to the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    Accept,
    Reject,
}

/// Everything drawn for one display scale.
pub struct Art {
    pub scale: f32,
    /// By half, then by flash step (0 is `brightness(1)`).
    pub glow: [Vec<Arc<RenderImage>>; 2],
    /// By half, then by step from the resting shadow (0) to the lit one.
    pub shadow: [Vec<Arc<RenderImage>>; 2],
}

impl Art {
    /// Draws every bitmap for the display `scale`. `colors` are the two
    /// halves' `--glow` colours.
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

    /// Every image, for dropping them from the sprite atlas.
    pub fn images(&self) -> impl Iterator<Item = &Arc<RenderImage>> {
        self.glow.iter().chain(&self.shadow).flatten()
    }
}

/// The flash step for a `brightness()` amount between 1 and 2.
pub fn flash_step(brightness: f32) -> usize {
    (((brightness - 1.) * (FLASH_STEPS - 1) as f32).round() as usize).min(FLASH_STEPS - 1)
}

/// The shadow step for how far the glyph's shadow is lit, 0 to 1.
pub fn shadow_step(lit: f32) -> usize {
    ((lit.clamp(0., 1.) * (SHADOW_STEPS - 1) as f32).round() as usize).min(SHADOW_STEPS - 1)
}

fn wrap(buffer: Buffer) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        buffer
    )]))
}

/// A premultiplied RGBA canvas in device pixels.
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

    /// Lays a layer of colour `rgb` at the alphas `alpha` over the canvas.
    fn lay(&mut self, rgb: [f32; 3], alpha: &[f32]) {
        for (pixel, &a) in self.pixels.iter_mut().zip(alpha) {
            for channel in 0..3 {
                pixel[channel] = rgb[channel] * a + pixel[channel] * (1. - a);
            }
            pixel[3] = a + pixel[3] * (1. - a);
        }
    }

    /// Lays premultiplied pixels under the canvas.
    fn under(&mut self, below: &Canvas) {
        for (pixel, low) in self.pixels.iter_mut().zip(&below.pixels) {
            let keep = 1. - pixel[3];
            for channel in 0..4 {
                pixel[channel] += low[channel] * keep;
            }
        }
    }

    /// Scales every pixel's alpha (and so its premultiplied colour).
    fn mask(&mut self, alpha: &[f32]) {
        for (pixel, &a) in self.pixels.iter_mut().zip(alpha) {
            for channel in pixel.iter_mut() {
                *channel *= a;
            }
        }
    }

    /// Straight-alpha BGRA bytes, the colour at `brightness()` (CSS
    /// multiplies the unpremultiplied colour and clamps it).
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

/// A rounded rectangle with a radius per corner (top-left, top-right,
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
    /// Signed distance from the edge: negative inside.
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

    /// How much of the device pixel centred at (`x`, `y`) points is inside.
    fn coverage(&self, x: f32, y: f32, scale: f32) -> f32 {
        (0.5 - self.distance(x, y) * scale).clamp(0., 1.)
    }

    /// The shape moved by (`dx`, `dy`) and grown by `grow` on every side,
    /// its radii with it (CSS spreads a shadow's corners the same way).
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

/// A Gaussian blur of `values` (`width` × `height`) with a standard deviation
/// of `sigma` device pixels, run separably; outside the grid counts as
/// `outside`.
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

/// A grid of a value per device pixel of a box of `width` × `height` points.
fn sample(width: usize, height: usize, scale: f32, value: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            out.push(value((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale));
        }
    }
    out
}

/// A CSS radial gradient's alpha at normalized distance `t` from its
/// centre, through `stops` of (position, alpha); transparent past the last.
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

/// One half's `.glow`, at `brightness(1)`.
fn glow(half: Half, color: [f32; 3], scale: f32) -> Canvas {
    let (box_w, box_h) = (HALF_WIDTH - 2. * GLOW_INSET, HEIGHT - 2. * GLOW_INSET);
    let width = (box_w * scale).round() as usize;
    let height = (box_h * scale).round() as usize;
    // `border-radius: inherit` from the half's `999px 0 0 999px` (or its
    // mirror), which the box's height clamps to half of it.
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
    // `radial-gradient(ellipse 50% 55% at var(--glow-x) 50%, 0.28 0%,
    // transparent 75%)`, the lower layer.
    let core = sample(width, height, scale, |x, y| {
        let (dx, dy) = (
            (x - across * box_w) / (0.5 * box_w),
            (y - 0.5 * box_h) / (0.55 * box_h),
        );
        radial_alpha((dx * dx + dy * dy).sqrt(), &[(0., 0.28), (0.75, 0.)])
    });
    canvas.lay(color, &core);
    // `radial-gradient(ellipse 110% 95% at var(--glow-x) 118%, 0.9 0%,
    // 0.45 38%, 0.12 64%, transparent 80%)`: the wash from the bottom edge.
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
    // The inset shadows, the last listed lowest: `inset 0 1px 0 rgba(255,
    // 255, 255, 0.06)`, `inset 0 0 8px rgba(glow, 0.22)` and `inset 0 -3px
    // 7px -3px rgba(glow, 0.95)`.
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
    // Clipped to the rounded half, and faded toward the divider by
    // `mask-image: linear-gradient(to right|left, #000 70%, transparent)`.
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

/// The strength (0 to 1) of an inset box shadow inside `shape`: everything
/// outside the shape moved by `dy` and shrunk by `spread` (grown when it is
/// negative), blurred by the CSS blur radius (a standard deviation of half
/// of it).
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
    // Worked out on a border wide enough to hold the moved hole and the
    // blur's reach, beyond which everything is outside the hole.
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

/// How much of the device pixel at (`x`, `y`) points the glyph's strokes
/// cover. The glyph is drawn in a 24-unit box scaled to `GLYPH` points at
/// (`left`, `top`), with Tabler's 2-unit round-capped strokes.
fn glyph_alpha(half: Half, left: f32, top: f32, x: f32, y: f32, scale: f32) -> f32 {
    let unit = GLYPH / 24.;
    let strokes: &[[(f32, f32); 2]] = match half {
        // `M5 12l5 5l10 -10`
        Half::Accept => &[[(5., 12.), (10., 17.)], [(10., 17.), (20., 7.)]],
        // `M18 6l-12 12`, `M6 6l12 12`
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

/// What lies under one half's glyph: its two drop shadows, `lit` of the
/// way from the resting `drop-shadow(0 1px 2px rgba(0, 0, 0, 0.58))
/// drop-shadow(0 0 7px rgba(255, 255, 255, 0.08))` to the lit
/// `drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5)) drop-shadow(0 0 8px
/// rgba(glow, 0.75))`. The second shadows the glyph with the first under
/// it. The box is the glyph's with `SHADOW_MARGIN` round it.
fn glyph_shadow(half: Half, color: [f32; 3], lit: f32, scale: f32) -> Canvas {
    let side = GLYPH + 2. * SHADOW_MARGIN;
    let size = (side * scale).round() as usize;
    let lerp = |from: f32, to: f32| from + (to - from) * lit;
    let glyph = sample(size, size, scale, |x, y| {
        glyph_alpha(half, SHADOW_MARGIN, SHADOW_MARGIN, x, y, scale)
    });
    // The first shadow: the glyph's shape a point lower, blurred.
    let lowered = sample(size, size, scale, |x, y| {
        glyph_alpha(half, SHADOW_MARGIN, SHADOW_MARGIN + 1., x, y, scale)
    });
    let dark = blur(&lowered, size, size, 2. / 2. * scale, 0.)
        .iter()
        .map(|value| value * lerp(0.58, 0.5))
        .collect::<Vec<_>>();
    // The second shadows the glyph and the first together.
    let together = glyph
        .iter()
        .zip(&dark)
        .map(|(g, d)| g + d * (1. - g))
        .collect::<Vec<_>>();
    let spread = blur(&together, size, size, lerp(7., 8.) / 2. * scale, 0.);
    // Its colour moves from white at 8% to the glow at 75%, premultiplied.
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
        // The outer corner is cut away; the inner one is square.
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
        // The check's corner, at (10, 17) of 24, in a 28-point box.
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
