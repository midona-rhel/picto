//! The audio view's visualizer (`AudioVisualizer.tsx`): a dark stage with
//! the spectrum, oscilloscope or orbit of what is playing, and its soft
//! reflection below. The numbers come from a port of the Web Audio
//! analyser the production canvas reads (`fftSize` 256 for the spectrum and
//! orbit, 1024 for the oscilloscope, smoothing 0.78, −100 to −30 dB), fed
//! by the samples the sound card is playing.
//!
//! The canvas draws in device pixels; so does this, dividing by the scale
//! factor (at most 2, as `sizeCanvas` caps it) when it paints.
use super::audio::Analysis;
use gpui::*;
use rustfft::{FftPlanner, num_complex::Complex};

const BAR_COUNT: usize = 64;
const SMOOTHING: f32 = 0.78;
const MIN_DECIBELS: f32 = -100.;
const MAX_DECIBELS: f32 = -30.;

/// `AudioVisualizationMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    None,
    Spectrum,
    Oscilloscope,
    Orbit,
}

impl Mode {
    /// The setting's stored value; anything unknown is the default.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "none" => Self::None,
            "oscilloscope" => Self::Oscilloscope,
            "orbit" => Self::Orbit,
            _ => Self::Spectrum,
        }
    }

    fn fft_size(self) -> usize {
        if self == Self::Oscilloscope {
            1024
        } else {
            256
        }
    }
}

/// The analyser's state between frames: the smoothed magnitudes.
pub struct Analyser {
    smoothed: Vec<f32>,
    written: Option<u64>,
    planner: FftPlanner<f32>,
    data: Vec<u8>,
}

impl Default for Analyser {
    fn default() -> Self {
        Self {
            smoothed: Vec::new(),
            written: None,
            planner: FftPlanner::new(),
            data: Vec::new(),
        }
    }
}

/// The Blackman window the analyser applies before its transform.
pub fn blackman(index: usize, size: usize) -> f32 {
    let (a0, a1, a2) = (0.42, 0.5, 0.08);
    let x = index as f32 / size as f32;
    a0 - a1 * (std::f32::consts::TAU * x).cos() + a2 * (2. * std::f32::consts::TAU * x).cos()
}

/// `getByteFrequencyData`'s conversion of a smoothed magnitude.
pub fn magnitude_byte(magnitude: f32) -> u8 {
    let decibels = if magnitude > 0. {
        20. * magnitude.log10()
    } else {
        f32::NEG_INFINITY
    };
    let scaled = 255. / (MAX_DECIBELS - MIN_DECIBELS) * (decibels - MIN_DECIBELS);
    scaled.floor().clamp(0., 255.) as u8
}

/// `getByteTimeDomainData`'s conversion of a sample.
pub fn sample_byte(sample: f32) -> u8 {
    (128. * (sample + 1.)).floor().clamp(0., 255.) as u8
}

impl Analyser {
    /// The bytes the production canvas reads this frame: frequencies for the
    /// spectrum and orbit, the waveform for the oscilloscope.
    pub fn read(&mut self, mode: Mode, analysis: &Analysis) -> &[u8] {
        let size = mode.fft_size();
        let samples = analysis.latest(size);
        if mode == Mode::Oscilloscope {
            self.data = samples.iter().map(|sample| sample_byte(*sample)).collect();
            return &self.data;
        }
        // The analyser smooths once per new block of sound, not per read.
        let written = analysis.written();
        if self.smoothed.len() != size / 2 {
            self.smoothed = vec![0.; size / 2];
            self.written = None;
        }
        if self.written != Some(written) {
            self.written = Some(written);
            let mut buffer: Vec<Complex<f32>> = samples
                .iter()
                .enumerate()
                .map(|(i, sample)| Complex::new(sample * blackman(i, size), 0.))
                .collect();
            self.planner.plan_fft_forward(size).process(&mut buffer);
            for (bin, smoothed) in self.smoothed.iter_mut().enumerate() {
                let magnitude = buffer[bin].norm() / size as f32;
                *smoothed = SMOOTHING * *smoothed + (1. - SMOOTHING) * magnitude;
            }
        }
        self.data = self.smoothed.iter().map(|m| magnitude_byte(*m)).collect();
        &self.data
    }
}

/// `frequencyBin`: bars sample the bins on a curve that favours the lows.
pub fn frequency_bin(data: &[u8], index: usize) -> f32 {
    if data.is_empty() {
        return 0.;
    }
    let normalized = index as f32 / (BAR_COUNT - 1) as f32;
    let bin = ((normalized.powf(1.7) * data.len() as f32).floor() as usize).min(data.len() - 1);
    data[bin] as f32 / 255.
}

fn color(hex: u32) -> Rgba {
    rgb(hex)
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// A colour on a gradient of evenly listed stops at `t` in 0..1.
fn along(stops: &[(f32, Rgba)], t: f32) -> Rgba {
    let t = t.clamp(0., 1.);
    for pair in stops.windows(2) {
        let ((from, a), (to, b)) = (pair[0], pair[1]);
        if t <= to {
            return mix(
                a,
                b,
                if to > from {
                    (t - from) / (to - from)
                } else {
                    0.
                },
            );
        }
    }
    stops.last().map_or(color(0), |stop| stop.1)
}

const CYAN: u32 = 0x31d7ff;
const VIOLET: u32 = 0x735cff;
const PINK: u32 = 0xff4fa3;

/// The scene, in device pixels, mapped to window points when painted.
struct Canvas<'a> {
    origin: Point<Pixels>,
    /// Device pixels per point.
    scale: f32,
    window: &'a mut Window,
}

impl Canvas<'_> {
    fn point(&self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.origin.x + px(x / self.scale),
            self.origin.y + px(y / self.scale),
        )
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(
            self.point(x, y),
            size(px(w / self.scale), px(h / self.scale)),
        )
    }
}

/// `drawStage`'s radial glow: violet at 18% in the middle, pink at 8% a
/// third of the way out, clear at the edge.
const GLOW: [(f32, u32); 3] = [(0., 0x735cff2e), (0.34, 0xff4fa314), (1., 0x05050b00)];

/// `drawStage`: the night sky and the violet glow about the middle. GPUI
/// draws two-stop linear gradients only, so the sky is two bands and the
/// radial glow is a run of thin rings.
fn draw_stage(canvas: &mut Canvas, width: f32, height: f32) {
    let split = height * 0.58;
    let top = canvas.rect(0., 0., width, split);
    let bottom = canvas.rect(0., split, width, height - split);
    canvas.window.paint_quad(fill(
        top,
        linear_gradient(
            180.,
            linear_color_stop(color(0x05050b), 0.),
            linear_color_stop(color(0x0d0920), 1.),
        ),
    ));
    canvas.window.paint_quad(fill(
        bottom,
        linear_gradient(
            180.,
            linear_color_stop(color(0x0d0920), 0.),
            linear_color_stop(color(0x04040a), 1.),
        ),
    ));
    let stops = GLOW.map(|(at, colour)| (at, rgba(colour)));
    // On whole device pixels, so that each ring meets the next without a
    // seam of partly covered pixels between them.
    let (centre_x, centre_y) = ((width / 2.).round(), (height * 0.54).round());
    let radius = (width * 0.58).max(height);
    // Rings that do not overlap, each drawn once in the gradient's colour
    // at its middle: discs laid one over another would keep the outer pink
    // under the violet middle, where the gradient has none.
    const RINGS: usize = 96;
    let step = (radius / RINGS as f32).ceil();
    for ring in 0..RINGS {
        let outer = step * (ring + 1) as f32;
        let colour = along(&stops, (ring as f32 + 0.5) / RINGS as f32);
        let bounds = canvas.rect(centre_x - outer, centre_y - outer, 2. * outer, 2. * outer);
        canvas.window.paint_quad(quad(
            bounds,
            px(outer / canvas.scale),
            transparent_black(),
            px(step / canvas.scale),
            colour,
            BorderStyle::Solid,
        ));
    }
}

/// The geometry each mode draws, in device pixels, so the reflection can
/// draw the same.
enum Shape {
    /// Rounded bars: x, y, width, height, radius, colour.
    Bars(Vec<(f32, f32, f32, f32, f32, Rgba)>),
    /// Stroked polylines with their width and colour stops along x.
    Lines(Vec<(Vec<(f32, f32)>, Rgba)>, f32),
}

fn spectrum(data: &[u8], width: f32, height: f32) -> Shape {
    let gap = (width * 0.003).max(2.);
    let usable = width * 0.96;
    let bar = ((usable - gap * (BAR_COUNT - 1) as f32) / BAR_COUNT as f32).max(2.);
    let left = (width - usable) / 2.;
    let (middle, amplitude) = (height * 0.42, height * 0.25);
    let stops = [(0., color(CYAN)), (0.48, color(VIOLET)), (1., color(PINK))];
    // The gradient runs from (0, middle − amplitude) to (width, middle +
    // amplitude); each bar takes the colour where its middle projects.
    let (dx, dy) = (width, 2. * amplitude);
    let length = dx * dx + dy * dy;
    Shape::Bars(
        (0..BAR_COUNT)
            .map(|index| {
                let value = frequency_bin(data, index).max(0.025);
                let bar_height = (value * amplitude).max(2.);
                let x = left + index as f32 * (bar + gap);
                let (cx, cy) = (x + bar / 2., middle);
                let t = ((cx - 0.) * dx + (cy - (middle - amplitude)) * dy) / length;
                (
                    x,
                    middle - bar_height,
                    bar,
                    bar_height * 2.,
                    bar / 2.,
                    along(&stops, t),
                )
            })
            .collect(),
    )
}

fn oscilloscope(data: &[u8], width: f32, height: f32) -> Shape {
    let line = (height * 0.012).clamp(2., 4.);
    let left = width * 0.02;
    let usable = width * 0.96;
    let count = data.len().max(2);
    let points: Vec<(f32, f32)> = data
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = left + index as f32 / (count - 1) as f32 * usable;
            let y = height * 0.42 + (*value as f32 - 128.) / 128. * height * 0.25;
            (x, y)
        })
        .collect();
    // The stroke's gradient runs from 12% to 88% of the width; each short
    // run of the line takes the colour at its start.
    let stops = [(0., color(CYAN)), (0.5, color(VIOLET)), (1., color(PINK))];
    let runs = points
        .chunks(16)
        .enumerate()
        .map(|(index, chunk)| {
            let mut run = chunk.to_vec();
            if let Some(next) = points.get((index + 1) * 16) {
                run.push(*next);
            }
            let t = (chunk[0].0 - width * 0.12) / (width * 0.76);
            (run, along(&stops, t))
        })
        .collect();
    Shape::Lines(runs, line)
}

fn orbit(data: &[u8], width: f32, height: f32) -> Shape {
    let (centre_x, centre_y) = (width / 2., height * 0.42);
    let radius = width.min(height) * 0.17;
    let reach = width.min(height) * 0.22;
    let line = (radius * 0.055).clamp(2., 5.);
    // The conic gradient starts at the top and goes round clockwise, as the
    // spokes do.
    let stops = [
        (0., color(CYAN)),
        (0.34, color(VIOLET)),
        (0.67, color(PINK)),
        (1., color(CYAN)),
    ];
    let spokes = (0..BAR_COUNT)
        .map(|index| {
            let turn = index as f32 / BAR_COUNT as f32;
            let angle = turn * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let value = frequency_bin(data, index).max(0.035);
            let outer = radius + value * reach;
            let (cos, sin) = (angle.cos(), angle.sin());
            (
                vec![
                    (centre_x + cos * radius, centre_y + sin * radius),
                    (centre_x + cos * outer, centre_y + sin * outer),
                ],
                along(&stops, turn),
            )
        })
        .collect();
    Shape::Lines(spokes, line)
}

/// Paints a shape, mapped by `map` (for the reflection) and at `alpha`, with
/// `blur` in device pixels (0 for none).
fn draw_shape(canvas: &mut Canvas, shape: &Shape, alpha: f32, blur: f32, map: impl Fn(f32) -> f32) {
    match shape {
        Shape::Bars(bars) => {
            for (x, y, w, h, radius, colour) in bars {
                let (top, bottom) = (map(*y), map(y + h));
                let (top, bottom) = (top.min(bottom), top.max(bottom));
                let colour = Rgba {
                    a: colour.a * alpha,
                    ..*colour
                };
                let squash = (bottom - top) / h;
                let bounds = canvas.rect(*x, top, *w, bottom - top);
                let radii = px(radius * squash.min(1.) / canvas.scale);
                if blur > 0. {
                    // A drop shadow is a blurred copy of its rectangle. The
                    // rectangle itself is placed far away so it does not
                    // hide the middle of its own shadow.
                    let away = point(px(100_000.), px(0.));
                    canvas.window.paint_drop_shadows(
                        Bounds::new(bounds.origin + away, bounds.size),
                        radii.into(),
                        &[BoxShadow {
                            color: colour.into(),
                            offset: point(px(-100_000.), px(0.)),
                            blur_radius: px(2. * blur / canvas.scale),
                            spread_radius: px(0.),
                            inset: false,
                        }],
                    );
                } else {
                    canvas
                        .window
                        .paint_quad(fill(bounds, colour).corner_radii(radii));
                }
            }
        }
        Shape::Lines(lines, width) => {
            // Paths cannot be blurred; a blurred stroke is drawn as a few
            // faint copies spread over the blur.
            let copies: &[f32] = if blur > 0. { &[-0.66, 0., 0.66] } else { &[0.] };
            for spread in copies {
                for (points, colour) in lines {
                    let mut builder = PathBuilder::stroke(px(*width / canvas.scale)).with_style(
                        PathStyle::Stroke(
                            StrokeOptions::default()
                                .with_line_width(*width / canvas.scale)
                                .with_line_cap(lyon_cap())
                                .with_line_join(lyon_join()),
                        ),
                    );
                    for (index, (x, y)) in points.iter().enumerate() {
                        let at = canvas.point(*x, map(*y) + spread * blur);
                        if index == 0 {
                            builder.move_to(at);
                        } else {
                            builder.line_to(at);
                        }
                    }
                    if let Ok(path) = builder.build() {
                        let colour = Rgba {
                            a: colour.a * alpha / copies.len() as f32,
                            ..*colour
                        };
                        canvas.window.paint_path(path, colour);
                    }
                }
            }
        }
    }
}

fn lyon_cap() -> lyon_tessellation::LineCap {
    lyon_tessellation::LineCap::Round
}

fn lyon_join() -> lyon_tessellation::LineJoin {
    lyon_tessellation::LineJoin::Round
}

/// Draws the stage and, unless the mode is `none`, the visualization and
/// its reflection. `data` is what [`Analyser::read`] returned, or nothing
/// before any sound has played.
pub fn paint(mode: Mode, data: &[u8], bounds: Bounds<Pixels>, window: &mut Window) {
    if mode == Mode::None {
        return;
    }
    let scale = window.scale_factor().min(2.);
    let (width, height) = (
        (f32::from(bounds.size.width) * scale).round().max(1.),
        (f32::from(bounds.size.height) * scale).round().max(1.),
    );
    let mut canvas = Canvas {
        origin: bounds.origin,
        scale,
        window,
    };
    draw_stage(&mut canvas, width, height);
    let silence;
    let data = if data.is_empty() {
        silence = vec![if mode == Mode::Oscilloscope { 128 } else { 0 }; mode.fft_size()];
        &silence[..]
    } else {
        data
    };
    let shape = match mode {
        Mode::Spectrum => spectrum(data, width, height),
        Mode::Oscilloscope => oscilloscope(data, width, height),
        Mode::Orbit => orbit(data, width, height),
        Mode::None => unreachable!(),
    };
    // `drawReflection`: the same geometry turned over about 86% of the
    // height, squashed to a third, below 58%, in two soft passes.
    let clip = ContentMask {
        bounds: canvas.rect(0., height * 0.58, width, height * 0.42),
    };
    canvas.window.with_content_mask(Some(clip), |window| {
        let mut canvas = Canvas {
            origin: bounds.origin,
            scale,
            window,
        };
        for (alpha, blur) in [(0.07, 0.03), (0.09, 0.012)] {
            let blur = (height * blur).clamp(8., 30.);
            draw_shape(&mut canvas, &shape, alpha, blur, |y| {
                height * 0.86 - y * 0.34
            });
        }
    });
    draw_shape(&mut canvas, &shape, 1., 0., |y| y);
}

#[cfg(test)]
mod tests {
    use super::{Mode, blackman, frequency_bin, magnitude_byte, sample_byte};

    #[test]
    fn settings_values_choose_the_mode() {
        assert_eq!(Mode::from_setting("none"), Mode::None);
        assert_eq!(Mode::from_setting("orbit"), Mode::Orbit);
        assert_eq!(Mode::from_setting("oscilloscope"), Mode::Oscilloscope);
        assert_eq!(Mode::from_setting("spectrum"), Mode::Spectrum);
        assert_eq!(Mode::from_setting("bogus"), Mode::Spectrum);
    }

    #[test]
    fn analyser_conversions_match_web_audio() {
        assert_eq!(sample_byte(0.), 128);
        assert_eq!(sample_byte(1.), 255);
        assert_eq!(sample_byte(-1.), 0);
        // −30 dB is the top of the range, −100 dB the bottom.
        assert_eq!(magnitude_byte(10f32.powf(-30. / 20.)), 255);
        assert_eq!(magnitude_byte(10f32.powf(-100. / 20.)), 0);
        assert_eq!(magnitude_byte(10f32.powf(-65. / 20.)), 127);
        assert_eq!(magnitude_byte(0.), 0);
        assert!(blackman(0, 256).abs() < 1e-6);
        assert!((blackman(128, 256) - 1.).abs() < 1e-6);
    }

    #[test]
    fn bars_sample_low_bins_densely() {
        let data: Vec<u8> = (0..128).map(|i| i as u8).collect();
        assert_eq!(frequency_bin(&data, 0), 0.);
        // The last bar reads the last bin.
        assert_eq!(frequency_bin(&data, 63), 127. / 255.);
        // The middle bar reads well below the middle bin.
        let middle = frequency_bin(&data, 31) * 255.;
        assert!(middle < 64., "{middle}");
        assert_eq!(frequency_bin(&[], 3), 0.);
    }
}
