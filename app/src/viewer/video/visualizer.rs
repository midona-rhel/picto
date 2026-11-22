//! The audio visualizer, which an audio player draws in the place of a
//! video's picture. It draws a dark background and, on it, one of three
//! visualizations of the sound that is playing: the spectrum, the
//! oscilloscope or the orbit. Below the visualization it draws a faint,
//! blurred reflection of it.
//!
//! The numbers that are drawn are computed from the samples that the output
//! device is playing, in the way the `AnalyserNode` of the Web Audio API
//! computes them. The spectrum and the orbit use a Fourier transform of 256
//! samples, and the oscilloscope uses 1024 samples. The magnitudes are
//! smoothed over time with the factor 0.78, and levels from −100 dB to
//! −30 dB are mapped onto the range that is drawn.
//!
//! Positions and sizes are computed in device pixels and are divided by the
//! window's scale factor when they are painted. A scale factor above 2
//! counts as 2.
use super::audio::Analysis;
use gpui::*;
use rustfft::{FftPlanner, num_complex::Complex};

/// The number of bars of the spectrum and of spokes of the orbit.
const BAR_COUNT: usize = 64;
/// How much of the previous smoothed magnitude is kept each time new
/// samples have been played. The rest comes from the new magnitude.
const SMOOTHING: f32 = 0.78;
/// The level in decibels that `magnitude_byte` turns into 0.
const MIN_DECIBELS: f32 = -100.;
/// The level in decibels that `magnitude_byte` turns into 255.
const MAX_DECIBELS: f32 = -30.;

/// The kind of audio visualization, which the user chooses in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Nothing is drawn.
    None,
    /// 64 vertical bars, one for each band of frequencies.
    Spectrum,
    /// The waveform, drawn as a line.
    Oscilloscope,
    /// 64 spokes around a circle, one for each band of frequencies.
    Orbit,
}

impl Mode {
    /// Returns the mode for the value that the settings store. A value that
    /// is not known gives the spectrum, which is the default.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "none" => Self::None,
            "oscilloscope" => Self::Oscilloscope,
            "orbit" => Self::Orbit,
            _ => Self::Spectrum,
        }
    }

    /// Returns how many samples the mode reads each time: 1024 for the
    /// oscilloscope, and 256 for the spectrum and the orbit.
    fn fft_size(self) -> usize {
        if self == Self::Oscilloscope {
            1024
        } else {
            256
        }
    }
}

/// What the visualizer keeps from one frame to the next.
pub struct Analyser {
    /// The smoothed magnitude of each frequency bin.
    smoothed: Vec<f32>,
    /// The count of `Analysis::written` at which the magnitudes were last
    /// smoothed, or `None` before the first time.
    written: Option<u64>,
    /// Makes the Fourier transforms, and keeps those it has made so that
    /// they are used again.
    planner: FftPlanner<f32>,
    /// The bytes that `read` returned last.
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

/// Returns the weight of the sample `index` out of `size` samples in the
/// Blackman window. The samples are multiplied by these weights before the
/// Fourier transform.
pub fn blackman(index: usize, size: usize) -> f32 {
    let (a0, a1, a2) = (0.42, 0.5, 0.08);
    let x = index as f32 / size as f32;
    a0 - a1 * (std::f32::consts::TAU * x).cos() + a2 * (2. * std::f32::consts::TAU * x).cos()
}

/// Returns a smoothed magnitude as a byte. The magnitude's level in
/// decibels is mapped from the range −100 dB to −30 dB onto 0 to 255, and a
/// level outside that range gives 0 or 255. A magnitude of 0 gives 0. The
/// `AnalyserNode` of the Web Audio API converts its frequency data in the
/// same way.
pub fn magnitude_byte(magnitude: f32) -> u8 {
    let decibels = if magnitude > 0. {
        20. * magnitude.log10()
    } else {
        f32::NEG_INFINITY
    };
    let scaled = 255. / (MAX_DECIBELS - MIN_DECIBELS) * (decibels - MIN_DECIBELS);
    scaled.floor().clamp(0., 255.) as u8
}

/// Returns a sample as a byte: −1 gives 0, 0 gives 128 and 1 gives 255. A
/// sample outside that range gives 0 or 255. The `AnalyserNode` of the Web
/// Audio API converts its waveform data in the same way.
pub fn sample_byte(sample: f32) -> u8 {
    (128. * (sample + 1.)).floor().clamp(0., 255.) as u8
}

impl Analyser {
    /// Returns the bytes to draw in this frame for `mode`. For the
    /// oscilloscope they are the 1024 samples that were played last, each
    /// as a byte (`sample_byte`). For the spectrum and the orbit they are
    /// the smoothed magnitudes of the 128 frequency bins of the 256 samples
    /// that were played last, each as a byte (`magnitude_byte`).
    pub fn read(&mut self, mode: Mode, analysis: &Analysis) -> &[u8] {
        let size = mode.fft_size();
        let samples = analysis.latest(size);
        if mode == Mode::Oscilloscope {
            self.data = samples.iter().map(|sample| sample_byte(*sample)).collect();
            return &self.data;
        }
        // The magnitudes are computed and smoothed only when new samples
        // have been played since the last time, and not each time the
        // player is drawn. Otherwise the amount of smoothing would depend
        // on how often the player is drawn.
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

/// Returns the value, from 0 to 1, of the bar `index` of the 64 bars: the
/// byte of one frequency bin of `data`, divided by 255. The bin is chosen
/// along a curve, which is the bar's position from 0 to 1 raised to the
/// power 1.7. So more of the bars show low frequencies than high ones.
/// Returns 0 when `data` is empty.
pub fn frequency_bin(data: &[u8], index: usize) -> f32 {
    if data.is_empty() {
        return 0.;
    }
    let normalized = index as f32 / (BAR_COUNT - 1) as f32;
    let bin = ((normalized.powf(1.7) * data.len() as f32).floor() as usize).min(data.len() - 1);
    data[bin] as f32 / 255.
}

/// Returns the opaque colour `hex`, written as `0xRRGGBB`.
fn color(hex: u32) -> Rgba {
    rgb(hex)
}

/// Returns the colour that is the fraction `t` of the way from `a` to `b`.
fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Returns the colour at the position `t`, from 0 to 1, of a gradient.
/// `stops` lists the gradient's colours with their positions, in rising
/// order of position.
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

use crate::theme::{VISUALIZER_CYAN as CYAN, VISUALIZER_PINK as PINK, VISUALIZER_VIOLET as VIOLET};

/// Converts positions and sizes in device pixels to window coordinates, for
/// painting in `window`.
struct Canvas<'a> {
    /// The top left corner of the visualizer's area, in window coordinates.
    origin: Point<Pixels>,
    /// The number of device pixels in one point.
    scale: f32,
    window: &'a mut Window,
}

impl Canvas<'_> {
    /// Returns the window position of the point that is `x` and `y` device
    /// pixels from the top left corner of the area.
    fn point(&self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.origin.x + px(x / self.scale),
            self.origin.y + px(y / self.scale),
        )
    }

    /// Returns the window bounds of a rectangle that is given in device
    /// pixels from the top left corner of the area.
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(
            self.point(x, y),
            size(px(w / self.scale), px(h / self.scale)),
        )
    }
}

/// The colours of the round glow on the background, each with its position
/// from the centre of the glow (0) to its edge (1): violet at 18% opacity
/// in the centre, pink at 8% opacity at 0.34, and fully transparent at the
/// edge.
const GLOW: [(f32, u32); 3] = [(0., 0x735cff2e), (0.34, 0xff4fa314), (1., 0x05050b00)];

/// Draws the background: a vertical gradient of three dark blues, and over
/// it a round glow whose centre is at 54% of the height. GPUI draws only
/// linear gradients of two colours. So the vertical gradient is drawn as
/// two rectangles that meet at 58% of the height, and the round glow is
/// drawn as 96 rings.
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
    // The centre of the glow and the width of a ring are whole numbers of
    // device pixels, so that each ring ends exactly where the next begins.
    // Otherwise a line of partly covered pixels would show between two
    // rings.
    let (centre_x, centre_y) = ((width / 2.).round(), (height * 0.54).round());
    let radius = (width * 0.58).max(height);
    // The glow is drawn as rings that do not overlap. Each ring has the
    // colour of the gradient at the middle of the ring. Discs drawn one
    // over the other would not give the gradient, because the pink of the
    // outer discs would show through the violet in the centre.
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

/// What a mode draws, in device pixels. It is kept as data so that the
/// reflection can draw the same shapes a second time.
enum Shape {
    /// Bars with rounded corners. Each has its left edge, its top edge, its
    /// width, its height, its corner radius and its colour.
    Bars(Vec<(f32, f32, f32, f32, f32, Rgba)>),
    /// Lines, each through a series of points and with its own colour, and
    /// the width that all of them are drawn with.
    Lines(Vec<(Vec<(f32, f32)>, Rgba)>, f32),
}

/// Returns the shape of the spectrum: 64 bars side by side across 96% of
/// the width. Each bar is centred vertically at 42% of the height. It
/// extends up and down from there by the value of its band of frequencies,
/// at most 25% of the height in each direction. The bars' colours go from
/// cyan at the left through violet to pink at the right.
fn spectrum(data: &[u8], width: f32, height: f32) -> Shape {
    let gap = (width * 0.003).max(2.);
    let usable = width * 0.96;
    let bar = ((usable - gap * (BAR_COUNT - 1) as f32) / BAR_COUNT as f32).max(2.);
    let left = (width - usable) / 2.;
    let (middle, amplitude) = (height * 0.42, height * 0.25);
    let stops = [(0., color(CYAN)), (0.48, color(VIOLET)), (1., color(PINK))];
    // The colours follow a gradient along the line from (0, middle −
    // amplitude) to (width, middle + amplitude). Each bar has one colour:
    // the gradient's colour at the point of that line that is nearest to
    // the bar's centre.
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

/// Returns the shape of the oscilloscope: the waveform as a line across
/// 96% of the width, centred vertically at 42% of the height. A sample of
/// the largest size is drawn 25% of the height away from that centre.
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
    // The line's colour follows a gradient from cyan at 12% of the width
    // through violet to pink at 88% of the width. The line is drawn in
    // pieces of 16 points, and each piece has the gradient's colour at its
    // first point. Each piece also ends with the first point of the next
    // piece, so that the pieces join.
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

/// Returns the shape of the orbit: 64 spokes that point outwards from a
/// circle whose centre is at 42% of the height. The circle's radius is 17%
/// of the width or of the height, whichever is smaller. A spoke is as long
/// as the value of its band of frequencies, at most 22% of that smaller
/// side.
fn orbit(data: &[u8], width: f32, height: f32) -> Shape {
    let (centre_x, centre_y) = (width / 2., height * 0.42);
    let radius = width.min(height) * 0.17;
    let reach = width.min(height) * 0.22;
    let line = (radius * 0.055).clamp(2., 5.);
    // The first spoke points up and the others follow clockwise. The
    // colours go round with them: cyan at the top, violet about a third of
    // the way round, pink about two thirds of the way round, and cyan
    // again at the top.
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

/// Paints `shape` with its opacity multiplied by `alpha`. `map` returns the
/// vertical position at which a vertical position of the shape is painted.
/// The reflection uses it to turn the shape upside down. When `blur` is
/// above 0 the shape is painted blurred, and `blur` is the size of the blur
/// in device pixels.
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
                    // The only blurred thing GPUI paints is a drop shadow,
                    // so a blurred bar is painted as the drop shadow of a
                    // rectangle of the bar's size. The rectangle is placed
                    // 100,000 px to the right, and its shadow is offset by
                    // the same distance to the left, so the shadow is where
                    // the bar is. The rectangle is that far away so that it
                    // does not hide the middle of its own shadow.
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
            // GPUI cannot blur a path. A blurred line is painted as three
            // copies of the line, each at a third of the opacity: one in
            // place, one moved up and one moved down by 0.66 times the
            // blur.
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

/// Returns the round cap that the ends of the lines are drawn with.
fn lyon_cap() -> lyon_tessellation::LineCap {
    lyon_tessellation::LineCap::Round
}

/// Returns the round join that the corners of the lines are drawn with.
fn lyon_join() -> lyon_tessellation::LineJoin {
    lyon_tessellation::LineJoin::Round
}

/// Draws the visualizer in `bounds`: the background, the visualization of
/// `mode` and its reflection. For `Mode::None` it draws nothing at all.
/// `data` is what [`Analyser::read`] returned. When `data` is empty, which
/// is the case before any sound has played, the visualization is drawn as
/// it is for silence.
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
    // The reflection is the same shape turned upside down and drawn at 34%
    // of its height: what the shape has at the vertical position `y` is
    // painted at `height * 0.86 - y * 0.34`. It is clipped to the part of
    // the area below 58% of the height. It is painted twice: at 7% opacity
    // with a blur of 3% of the height, and at 9% opacity with a blur of
    // 1.2% of the height. Each blur is at least 8 and at most 30 device
    // pixels.
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
        // A magnitude at −30 dB gives 255, and one at −100 dB gives 0.
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
        // The last bar shows the last bin.
        assert_eq!(frequency_bin(&data, 63), 127. / 255.);
        // The middle bar shows a bin in the lower half of the bins.
        let middle = frequency_bin(&data, 31) * 255.;
        assert!(middle < 64., "{middle}");
        assert_eq!(frequency_bin(&[], 3), 0.);
    }
}
