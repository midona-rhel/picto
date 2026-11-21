//! The calculations of the Duplicates screen that need no window: which
//! pair to show after the queue was read again, how the similarity and the
//! dimensions are written, and where each of the two pictures is drawn for
//! the current zoom.
use crate::i18n::t;
use picto_library::FileId;

/// The key that identifies a pair: the IDs of its two files, the smaller
/// one first.
pub type PairKey = (u32, u32);

pub fn pair_key(a: FileId, b: FileId) -> PairKey {
    (a.0.min(b.0), a.0.max(b.0))
}

/// Chooses the pair to show after the queue was read again. `previous` is
/// the old queue, `active` is the pair that was on screen, and `next` is the
/// new queue. The result is `active` if it is still in the new queue.
/// Otherwise it is the first pair after `active` in the old queue that is
/// also in the new queue, or else the last such pair before `active`, or
/// else the first pair of the new queue.
pub fn select_stable(
    previous: &[PairKey],
    active: Option<PairKey>,
    next: &[PairKey],
) -> Option<PairKey> {
    let first = *next.first()?;
    let Some(active) = active else {
        return Some(first);
    };
    if next.contains(&active) {
        return Some(active);
    }
    if let Some(at) = previous.iter().position(|key| *key == active) {
        let after = previous[at + 1..].iter().find(|key| next.contains(key));
        let before = previous[..at].iter().rev().find(|key| next.contains(key));
        if let Some(key) = after.or(before) {
            return Some(*key);
        }
    }
    Some(first)
}

/// Writes the similarity of a pair, for example "98.8% similar". `distance`
/// is the difference between the two files in hundredths of a percent. A
/// difference below a tenth of a percent is written with two decimals. Every
/// other difference is written with one decimal, and a value exactly between
/// two decimals is rounded up. So that this rounding is exact, the function
/// calculates with whole hundredths and not with floating-point numbers.
pub fn similarity_label(distance: u32) -> String {
    if distance == 0 {
        // The locale files have no key for this text, so it is shown in
        // English in every language.
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

/// Writes a width and a height in pixels as the inspector does, for
/// example "1810 × 1280". When either is not known, the result is
/// "Unknown".
pub fn dimensions(width: Option<u32>, height: Option<u32>) -> String {
    match (width, height) {
        (Some(width), Some(height)) => format!("{width} × {height}"),
        _ => t("Unknown").into(),
    }
}

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.;
/// How much one unit of wheel movement changes the zoom. The zoom is
/// multiplied by `exp(-delta_y × 0.003)`, where `delta_y` is the argument of
/// [`LinkedZoom::wheel`].
const WHEEL_RATE: f32 = 0.003;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Each picture is as large as fits inside its pane.
    Fit,
    /// One pixel of the picture is drawn on one point of the screen.
    Actual,
    /// The picture is drawn at a multiple of the size that fits its pane.
    /// The mouse wheel sets this multiple.
    Custom,
}

/// A width and a height.
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

/// The side edge of its pane that a picture narrower than the pane is drawn
/// against. The left pane uses `Right` and the right pane uses `Left`, so
/// two narrow pictures are drawn next to each other in the middle of the
/// screen with only the gap between the panes between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaneEdge {
    /// The picture's right edge is on the pane's right edge.
    Right,
    /// The picture's left edge is on the pane's left edge.
    Left,
}

impl PaneEdge {
    /// The horizontal distance from the pane's centre to the centre of a
    /// picture `width` points wide. It is zero when the picture is at least
    /// as wide as the pane.
    fn shift(self, pane: Extent, width: f32) -> f32 {
        let slack = ((pane.width - width) / 2.).max(0.);
        match self {
            PaneEdge::Right => slack,
            PaneEdge::Left => -slack,
        }
    }
}

/// The rectangle in which a picture is drawn, in points from the top left
/// corner of its pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// The zoom that the two pictures share. `zoom` is a multiple of the size
/// at which a picture fits its pane. `center` is the point of the picture
/// that is drawn at the centre of the pane, as a fraction of the picture's
/// width and height. Because both values are relative to each picture, the
/// two panes show the same part of their pictures even when the two files
/// have different sizes in pixels.
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

    /// The number of screen points per pixel of the picture, for a picture
    /// of size `image` in a pane of size `pane`.
    fn scale(&self, pane: Extent, image: Extent) -> f32 {
        match self.mode {
            Mode::Actual => 1.,
            _ => fit_scale(pane, image) * self.zoom,
        }
    }

    /// Changes the zoom for a turn of the mouse wheel. `offset` is the
    /// position of the pointer relative to the centre of the pane. The point
    /// of the picture that is under the pointer stays under the pointer.
    /// `delta_y` is positive when the wheel is turned toward the user, and
    /// that zooms out.
    pub fn wheel(
        &mut self,
        pane: Extent,
        image: Extent,
        pane_edge: PaneEdge,
        offset: (f32, f32),
        delta_y: f32,
    ) {
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
        // A picture narrower than the pane is not centred in it, so the
        // pointer's distance from the picture's centre is its distance from
        // the pane's centre less the picture's shift.
        let from = offset.0 - pane_edge.shift(pane, image.width * shown);
        let to = offset.0 - pane_edge.shift(pane, image.width * next_shown);
        let focus = (
            self.center.0 + from / (image.width * shown),
            self.center.1 + offset.1 / (image.height * shown),
        );
        self.center = (
            (focus.0 - to / (image.width * next_shown)).clamp(0., 1.),
            (focus.1 - offset.1 / (image.height * next_shown)).clamp(0., 1.),
        );
        self.zoom = next;
        self.mode = Mode::Custom;
    }

    /// Moves the pictures for a drag. `start` is the value of `center` when
    /// the drag began, and `moved` is how far the pointer has moved since
    /// then, in points.
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

    /// The rectangle in which a picture of size `image` is drawn in a pane
    /// of size `pane`. A picture that is larger than the pane cannot be
    /// moved so far that an empty strip appears between its edge and the
    /// edge of the pane. A picture that is lower than the pane is centred
    /// vertically. A picture that is narrower than the pane is drawn against
    /// the side edge that `pane_edge` names.
    pub fn frame(&self, pane: Extent, image: Extent, pane_edge: PaneEdge) -> Frame {
        let scale = self.scale(pane, image);
        let (width, height) = (image.width * scale, image.height * scale);
        let reach_x = ((width - pane.width) / 2.).max(0.);
        let reach_y = ((height - pane.height) / 2.).max(0.);
        let tx = ((0.5 - self.center.0) * width).clamp(-reach_x, reach_x);
        let ty = ((0.5 - self.center.1) * height).clamp(-reach_y, reach_y);
        Frame {
            left: pane.width / 2. + tx - width / 2. + pane_edge.shift(pane, width),
            top: pane.height / 2. + ty - height / 2.,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Extent, LinkedZoom, Mode, PaneEdge, dimensions, pair_key, select_stable, similarity_label,
    };
    use picto_library::FileId;

    #[test]
    fn a_pair_key_is_the_same_either_way_round() {
        assert_eq!(pair_key(FileId(9), FileId(3)), (3, 9));
        assert_eq!(pair_key(FileId(3), FileId(9)), (3, 9));
    }

    #[test]
    fn the_queue_keeps_its_place_when_fetched_again() {
        let previous = [(1, 2), (3, 4), (5, 6), (7, 8)];
        // The pair on screen is still in the queue, so it stays on screen.
        assert_eq!(
            select_stable(&previous, Some((3, 4)), &[(1, 2), (3, 4)]),
            Some((3, 4))
        );
        // The pair on screen was decided and is gone. The next pair after
        // it that is still in the queue is shown.
        assert_eq!(
            select_stable(&previous, Some((3, 4)), &[(1, 2), (7, 8)]),
            Some((7, 8))
        );
        // No pair after it is still in the queue, so the nearest pair
        // before it is shown.
        assert_eq!(
            select_stable(&previous, Some((5, 6)), &[(1, 2), (3, 4)]),
            Some((3, 4))
        );
        // No pair was on screen, so the first pair of the queue is shown.
        assert_eq!(select_stable(&[], None, &[(9, 10)]), Some((9, 10)));
        assert_eq!(select_stable(&previous, Some((3, 4)), &[]), None);
    }

    #[test]
    fn similarity_reads_as_the_earlier_versions_label() {
        assert_eq!(similarity_label(0), "100% similar");
        assert_eq!(similarity_label(5), "99.95% similar");
        assert_eq!(similarity_label(120), "98.8% similar");
        // 99.25 is exactly between 99.2 and 99.3. It is rounded up.
        assert_eq!(similarity_label(75), "99.3% similar");
        assert_eq!(similarity_label(20_000), "0.0% similar");
    }

    #[test]
    fn sizes_and_dimensions_read_as_the_earlier_versions_rows() {
        assert_eq!(dimensions(Some(640), Some(480)), "640 × 480");
        assert_eq!(dimensions(None, Some(480)), "Unknown");
    }

    #[test]
    fn fit_shows_the_whole_image_leaning_on_its_edge() {
        let zoom = LinkedZoom::default();
        let pane = Extent::new(400., 300.);
        let frame = zoom.frame(pane, Extent::new(800., 400.), PaneEdge::Left);
        assert_eq!((frame.width, frame.height), (400., 200.));
        assert_eq!((frame.left, frame.top), (0., 50.));
        // A picture that is narrower than its pane is drawn against the
        // edge that is next to the other pane.
        let tall = Extent::new(200., 600.);
        assert_eq!(zoom.frame(pane, tall, PaneEdge::Right).left, 300.);
        assert_eq!(zoom.frame(pane, tall, PaneEdge::Left).left, 0.);
    }

    #[test]
    fn actual_pixels_shows_the_middle_at_one_to_one() {
        let mut zoom = LinkedZoom::default();
        zoom.actual();
        let frame = zoom.frame(
            Extent::new(400., 300.),
            Extent::new(800., 400.),
            PaneEdge::Left,
        );
        assert_eq!((frame.width, frame.height), (800., 400.));
        assert_eq!((frame.left, frame.top), (-200., -50.));
    }

    #[test]
    fn the_wheel_zooms_about_the_pointer() {
        let pane = Extent::new(400., 400.);
        let image = Extent::new(400., 400.);
        let mut zoom = LinkedZoom::default();
        // Zooming in with the pointer 100 points to the right of the centre
        // keeps the same point of the picture under the pointer.
        let before = zoom.frame(pane, image, PaneEdge::Left);
        let under = (300. - before.left) / before.width;
        zoom.wheel(pane, image, PaneEdge::Left, (100., 0.), -200.);
        assert_eq!(zoom.mode, Mode::Custom);
        let after = zoom.frame(pane, image, PaneEdge::Left);
        assert!(after.width > before.width);
        let still = (300. - after.left) / after.width;
        assert!((still - under).abs() < 1e-4, "{under} {still}");
        // The zoom does not go beyond eight times the size that fits.
        for _ in 0..100 {
            zoom.wheel(pane, image, PaneEdge::Left, (0., 0.), -500.);
        }
        assert!((zoom.frame(pane, image, PaneEdge::Left).width - 3200.).abs() < 1e-2);
    }

    #[test]
    fn a_drag_moves_both_images_by_the_same_fraction() {
        let mut zoom = LinkedZoom::default();
        zoom.actual();
        let pane = Extent::new(200., 200.);
        let small = Extent::new(400., 400.);
        let large = Extent::new(800., 800.);
        zoom.pan(pane, small, (0.5, 0.5), (-100., 0.));
        // The drag moves the view a quarter of the small picture's width to
        // the right.
        assert!((zoom.center().0 - 0.75).abs() < 1e-6);
        // The large picture also moves by a quarter of its own width.
        let frame = zoom.frame(pane, large, PaneEdge::Left);
        assert_eq!(frame.left, 100. - 600.);
        // A drag that is too long stops when the edge of the picture reaches
        // the edge of the pane.
        zoom.pan(pane, small, (0.5, 0.5), (-1000., 0.));
        assert_eq!(zoom.frame(pane, small, PaneEdge::Left).left, -200.);
    }
}
