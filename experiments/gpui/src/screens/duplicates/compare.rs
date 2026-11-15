//! The arithmetic of Compare: how a file's facts are written, and the zoom
//! and pan that stay the same while the set's files flip in place
//! (`useLinkedComparisonZoom.ts`): the zoom is relative to each picture's
//! fitted size and the centre a fraction of each picture.
use crate::i18n::t;

/// `similarityLabel`. The distance is in hundredths of a percent; JavaScript's
/// `toFixed` rounds a tie up, so the arithmetic is done in whole hundredths.
pub fn similarity_label(distance: u32) -> String {
    if distance == 0 {
        // Production writes the exact match in English.
        return crate::i18n::en("100% similar").into();
    }
    let hundredths = 10_000u32.saturating_sub(distance);
    if distance < 10 {
        crate::tf!(
            "{value0}% similar",
            format!("{}.{:02}", hundredths / 100, hundredths % 100)
        )
    } else {
        let tenths = (hundredths + 5) / 10;
        crate::tf!(
            "{value0}% similar",
            format!("{}.{}", tenths / 10, tenths % 10)
        )
    }
}

/// `W × H`, as the inspector writes them, or Unknown.
pub fn dimensions(width: Option<u32>, height: Option<u32>) -> String {
    match (width, height) {
        (Some(width), Some(height)) => format!("{width} × {height}"),
        _ => t("Unknown").into(),
    }
}

/// `toLocaleDateString()` in the en-US locale the Electron app runs in:
/// month/day/year without leading zeros, in local time.
pub fn short_date(milliseconds: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(milliseconds)
        .single()
        .map_or_else(
            || t("Unknown").into(),
            |date| crate::i18n::format::date(date.date_naive()),
        )
}

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.;
/// How strongly a wheel step zooms: `exp(-deltaY × 0.003)`.
const WHEEL_RATE: f32 = 0.003;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Each image fitted to its pane.
    Fit,
    /// One image pixel to one screen pixel.
    Actual,
    /// A multiple of the fitted size, from the wheel.
    Custom,
}

/// A width and height in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extent {
    pub width: f32,
    pub height: f32,
}

impl Extent {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    fn usable(&self) -> bool {
        self.width > 0. && self.height > 0.
    }
}

/// Where an image is drawn inside its pane, relative to the pane's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// One zoom and one centre for both previews. The zoom is relative to each
/// image's fitted size and the centre is a fraction of each image, so the
/// two panes show the same part of their images at the same relative scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkedZoom {
    pub mode: Mode,
    zoom: f32,
    center: (f32, f32),
}

impl Default for LinkedZoom {
    fn default() -> Self {
        Self {
            mode: Mode::Fit,
            zoom: 1.,
            center: (0.5, 0.5),
        }
    }
}

fn fit_scale(pane: Extent, image: Extent) -> f32 {
    if pane.usable() && image.usable() {
        (pane.width / image.width).min(pane.height / image.height)
    } else {
        1.
    }
}

impl LinkedZoom {
    pub fn fit(&mut self) {
        *self = Self::default();
    }

    pub fn actual(&mut self) {
        self.mode = Mode::Actual;
        self.center = (0.5, 0.5);
    }

    pub fn center(&self) -> (f32, f32) {
        self.center
    }

    /// Image pixels to points for one pane.
    fn scale(&self, pane: Extent, image: Extent) -> f32 {
        match self.mode {
            Mode::Actual => 1.,
            _ => fit_scale(pane, image) * self.zoom,
        }
    }

    /// Zooms about the pointer, `offset` from the pane's centre, keeping the
    /// image point under it in place. `delta_y` is the DOM's: positive when
    /// the wheel turns toward the user, which zooms out.
    pub fn wheel(&mut self, pane: Extent, image: Extent, offset: (f32, f32), delta_y: f32) {
        if !image.usable() {
            return;
        }
        let fit = fit_scale(pane, image);
        let relative = match self.mode {
            Mode::Actual => 1. / fit,
            _ => self.zoom,
        };
        let shown = fit * relative;
        let next = (relative * (-delta_y * WHEEL_RATE).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
        let next_shown = fit * next;
        let focus = (
            self.center.0 + offset.0 / (image.width * shown),
            self.center.1 + offset.1 / (image.height * shown),
        );
        self.center = (
            (focus.0 - offset.0 / (image.width * next_shown)).clamp(0., 1.),
            (focus.1 - offset.1 / (image.height * next_shown)).clamp(0., 1.),
        );
        self.zoom = next;
        self.mode = Mode::Custom;
    }

    /// Moves the view with a drag that began at `start` and has since moved
    /// `moved` points.
    pub fn pan(&mut self, pane: Extent, image: Extent, start: (f32, f32), moved: (f32, f32)) {
        if !image.usable() {
            return;
        }
        let shown = self.scale(pane, image);
        self.center = (
            (start.0 - moved.0 / (image.width * shown)).clamp(0., 1.),
            (start.1 - moved.1 / (image.height * shown)).clamp(0., 1.),
        );
    }

    /// The image's place in its pane. The offset from the centre is clamped
    /// so a zoomed image never leaves a gap at an edge it could cover.
    pub fn frame(&self, pane: Extent, image: Extent) -> Frame {
        let scale = self.scale(pane, image);
        let (width, height) = (image.width * scale, image.height * scale);
        let reach_x = ((width - pane.width) / 2.).max(0.);
        let reach_y = ((height - pane.height) / 2.).max(0.);
        let tx = ((0.5 - self.center.0) * width).clamp(-reach_x, reach_x);
        let ty = ((0.5 - self.center.1) * height).clamp(-reach_y, reach_y);
        Frame {
            left: pane.width / 2. + tx - width / 2.,
            top: pane.height / 2. + ty - height / 2.,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Extent, LinkedZoom, Mode, dimensions, similarity_label};

    #[test]
    fn similarity_reads_as_the_electron_label() {
        assert_eq!(similarity_label(0), "100% similar");
        assert_eq!(similarity_label(5), "99.95% similar");
        assert_eq!(similarity_label(120), "98.8% similar");
        // A tie rounds up, as `toFixed` does.
        assert_eq!(similarity_label(75), "99.3% similar");
        assert_eq!(similarity_label(20_000), "0.0% similar");
    }

    #[test]
    fn sizes_and_dimensions_read_as_the_electron_rows() {
        assert_eq!(dimensions(Some(640), Some(480)), "640 × 480");
        assert_eq!(dimensions(None, Some(480)), "Unknown");
    }

    #[test]
    fn fit_centres_the_whole_image() {
        let zoom = LinkedZoom::default();
        let frame = zoom.frame(Extent::new(400., 300.), Extent::new(800., 400.));
        assert_eq!((frame.width, frame.height), (400., 200.));
        assert_eq!((frame.left, frame.top), (0., 50.));
    }

    #[test]
    fn actual_pixels_shows_the_middle_at_one_to_one() {
        let mut zoom = LinkedZoom::default();
        zoom.actual();
        let frame = zoom.frame(Extent::new(400., 300.), Extent::new(800., 400.));
        assert_eq!((frame.width, frame.height), (800., 400.));
        assert_eq!((frame.left, frame.top), (-200., -50.));
    }

    #[test]
    fn the_wheel_zooms_about_the_pointer() {
        let pane = Extent::new(400., 400.);
        let image = Extent::new(400., 400.);
        let mut zoom = LinkedZoom::default();
        // Zoom in with the pointer 100pt right of centre: the image point
        // under it stays under it.
        let before = zoom.frame(pane, image);
        let under = (300. - before.left) / before.width;
        zoom.wheel(pane, image, (100., 0.), -200.);
        assert_eq!(zoom.mode, Mode::Custom);
        let after = zoom.frame(pane, image);
        assert!(after.width > before.width);
        let still = (300. - after.left) / after.width;
        assert!((still - under).abs() < 1e-4, "{under} {still}");
        // The zoom stops at eight times the fitted size.
        for _ in 0..100 {
            zoom.wheel(pane, image, (0., 0.), -500.);
        }
        assert!((zoom.frame(pane, image).width - 3200.).abs() < 1e-2);
    }

    #[test]
    fn a_drag_moves_both_images_by_the_same_fraction() {
        let mut zoom = LinkedZoom::default();
        zoom.actual();
        let pane = Extent::new(200., 200.);
        let small = Extent::new(400., 400.);
        let large = Extent::new(800., 800.);
        zoom.pan(pane, small, (0.5, 0.5), (-100., 0.));
        // A quarter of the small image to the right...
        assert!((zoom.center().0 - 0.75).abs() < 1e-6);
        // ...which is a quarter of the large one too.
        let frame = zoom.frame(pane, large);
        assert_eq!(frame.left, 100. - 600.);
        // It never leaves an edge uncovered.
        zoom.pan(pane, small, (0.5, 0.5), (-1000., 0.));
        assert_eq!(zoom.frame(pane, small).left, -200.);
    }
}
