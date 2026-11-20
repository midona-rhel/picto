//! The zoom and pan of an image in the viewer. [`Camera`] holds them the way
//! production's `useImageZoom.ts` does: a scale, and the offset of the
//! image's centre from the pane's centre. The pane is the area of the viewer
//! that the image is shown in. The image is drawn at its natural size times
//! the scale, so at 100% one image pixel takes one point on screen.
use std::time::{Duration, Instant};

/// `MIN_SCALE`: the smallest zoom the viewer allows, 5%.
pub const MIN_SCALE: f32 = 0.05;
/// `MAX_SCALE`: the largest zoom the viewer allows, 800%.
pub const MAX_SCALE: f32 = 8.0;
/// One zoom step. A zoom key or one notch of the mouse wheel multiplies the
/// scale by this to zoom in, and divides the scale by it to zoom out.
pub const STEP: f32 = 1.25;
/// A wheel event whose pixel delta is at least this large is treated as one
/// notch of a mouse wheel. A smaller one is treated as a trackpad scroll.
/// When a scroll pans the image and its delta is given in lines, one line
/// also counts as this many pixels.
pub const NOTCH: f32 = 40.;
/// When two wheel notches arrive within this time of each other, the second
/// one zooms from the scale the first one was animating towards, not from
/// the scale the animation has reached so far. Several quick notches
/// therefore each add a full step.
pub const NOTCH_RUN: Duration = Duration::from_millis(200);
/// `FIT_ROUNDING_TOLERANCE_PX`: an image that is larger than the pane by no
/// more than this many points still counts as fitting in it. Without this,
/// rounding while the window is resized would make the navigator appear and
/// disappear repeatedly.
const FIT_TOLERANCE: f32 = 3.;

pub type Size = (f32, f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scale: f32,
    /// The offset of the image's centre from the pane's centre, in points:
    /// `tx` to the right and `ty` downward.
    pub tx: f32,
    pub ty: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self::actual()
    }
}

fn clamp_scale(scale: f32) -> f32 {
    scale.clamp(MIN_SCALE, MAX_SCALE)
}

/// The largest scale at which the whole image fits in the pane. Returns 1
/// when the image or the pane has no width or no height.
pub fn fit_scale(image: Size, pane: Size) -> f32 {
    if pane.0 <= 0. || pane.1 <= 0. || image.0 <= 0. || image.1 <= 0. {
        return 1.;
    }
    (pane.0 / image.0).min(pane.1 / image.1)
}

/// The rectangle the image takes when it is scaled to fit in the pane whole
/// and is centred in it, as `(left, top, width, height)` relative to the
/// pane's top-left corner. Returns `None` when the image or the pane has no
/// width or no height. It is used wherever a picture is fitted into a box:
/// for example Flash's stage in the viewer, the image in the navigator, and
/// a thumbnail in a grid tile.
pub fn contain(image: Size, pane: Size) -> Option<(f32, f32, f32, f32)> {
    if pane.0 <= 0. || pane.1 <= 0. || image.0 <= 0. || image.1 <= 0. {
        return None;
    }
    let scale = fit_scale(image, pane);
    let (width, height) = (image.0 * scale, image.1 * scale);
    Some(((pane.0 - width) / 2., (pane.1 - height) / 2., width, height))
}

impl Camera {
    /// `fitToWindow`: the camera that shows the whole image, centred in the
    /// pane. The scale is not limited to the range from [`MIN_SCALE`] to
    /// [`MAX_SCALE`] here, as in production.
    pub fn fit(image: Size, pane: Size) -> Self {
        Self {
            scale: fit_scale(image, pane),
            tx: 0.,
            ty: 0.,
        }
    }

    /// `fitActual`: the camera that shows the image at 100%, which is one
    /// image pixel per point, centred in the pane.
    pub fn actual() -> Self {
        Self {
            scale: 1.,
            tx: 0.,
            ty: 0.,
        }
    }

    /// The scale as a percentage rounded to a whole number, which is how the
    /// toolbar shows the zoom.
    pub fn percent(&self) -> f32 {
        (self.scale * 100.).round()
    }

    /// `zoomTo`: this camera at a new scale, which is limited to the range
    /// from [`MIN_SCALE`] to [`MAX_SCALE`]. With a focal point (given
    /// relative to the pane's centre), the part of the image under that
    /// point stays under it. Without one, the offset is left unchanged.
    pub fn zoomed(self, scale: f32, focal: Option<(f32, f32)>) -> Self {
        let scale = clamp_scale(scale);
        match focal {
            Some((x, y)) => {
                let ratio = scale / self.scale;
                Self {
                    scale,
                    tx: x - ratio * (x - self.tx),
                    ty: y - ratio * (y - self.ty),
                }
            }
            None => Self { scale, ..self },
        }
    }

    /// The camera that `animateZoomTo` animates to when it has no focal
    /// point. The offset is multiplied by the same ratio as the scale, so
    /// the part of the image at the pane's centre stays there.
    pub fn zoomed_about_centre(self, scale: f32) -> Self {
        let scale = clamp_scale(scale);
        let ratio = scale / self.scale;
        Self {
            scale,
            tx: self.tx * ratio,
            ty: self.ty * ratio,
        }
    }

    /// This camera with the image moved by `(dx, dy)` points: to the right
    /// for a positive `dx` and downward for a positive `dy`. During a drag,
    /// `self` is the camera the drag began with and `(dx, dy)` is how far
    /// the pointer has moved since.
    pub fn panned(self, dx: f32, dy: f32) -> Self {
        Self {
            tx: self.tx + dx,
            ty: self.ty + dy,
            ..self
        }
    }

    /// The camera after the pane changes size from `from` to `to`, as the
    /// detail window does it (`DetailWindow`'s `ResizeObserver`). The scale
    /// is multiplied by the ratio of the new fit scale to the old one, and
    /// the offset by the ratio of the new pane size to the old one. An image
    /// that was fitted therefore stays fitted, and a zoomed image keeps
    /// showing the same part. Returns the camera unchanged when the old
    /// pane has no width or no height.
    pub fn resized(self, image: Size, from: Size, to: Size) -> Self {
        let (old, new) = (fit_scale(image, from), fit_scale(image, to));
        if from.0 <= 0. || from.1 <= 0. || old <= 0. || new <= 0. {
            return self;
        }
        Self {
            scale: self.scale * new / old,
            tx: self.tx * to.0 / from.0,
            ty: self.ty * to.1 / from.1,
        }
    }

    /// Where the image is drawn, relative to the pane's top-left corner:
    /// `(left, top, width, height)`.
    pub fn image_rect(&self, image: Size, pane: Size) -> (f32, f32, f32, f32) {
        let (width, height) = (image.0 * self.scale, image.1 * self.scale);
        (
            pane.0 / 2. + self.tx - width / 2.,
            pane.1 / 2. + self.ty - height / 2.,
            width,
            height,
        )
    }

    /// `panToNormalized`: this camera moved so that one point of the image
    /// is at the pane's centre. The point is given as fractions of the
    /// image's width (`nx`) and height (`ny`).
    pub fn centred_on(self, nx: f32, ny: f32, image: Size) -> Self {
        Self {
            tx: (0.5 - nx) * image.0 * self.scale,
            ty: (0.5 - ny) * image.1 * self.scale,
            ..self
        }
    }

    fn lerp(self, to: Self, amount: f32) -> Self {
        Self {
            scale: self.scale + (to.scale - self.scale) * amount,
            tx: self.tx + (to.tx - self.tx) * amount,
            ty: self.ty + (to.ty - self.ty) * amount,
        }
    }
}

/// The factor the scale is multiplied by for one trackpad scroll event.
/// `delta` has the sign of a browser's `deltaY`: it is positive when
/// scrolling down, which zooms out.
pub fn wheel_factor(delta: f32) -> f32 {
    (-delta * 0.004).exp()
}

/// `computeNavigatorRect`: the part of the image that is visible in the
/// pane, as `(x, y, width, height)` in fractions of the image's width and
/// height. Returns `None` while the whole image fits in the pane, or is
/// larger than it by no more than `FIT_TOLERANCE`. The navigator is not
/// shown then.
pub fn visible_part(camera: Camera, image: Size, pane: Size) -> Option<(f32, f32, f32, f32)> {
    let (left, top, width, height) = camera.image_rect(image, pane);
    if width <= pane.0 + FIT_TOLERANCE && height <= pane.1 + FIT_TOLERANCE {
        return None;
    }
    let visible_left = (-left).max(0.);
    let visible_top = (-top).max(0.);
    let visible_right = width.min(pane.0 - left);
    let visible_bottom = height.min(pane.1 - top);
    let fraction = |pixels: f32, extent: f32| (pixels / camera.scale / extent).clamp(0., 1.);
    Some((
        fraction(visible_left, image.0),
        fraction(visible_top, image.1),
        fraction((visible_right - visible_left).max(0.), image.0),
        fraction((visible_bottom - visible_top).max(0.), image.1),
    ))
}

/// Where the image is drawn inside the square navigator, whose sides are
/// `side` long, as `(left, top, width, height)` (`useNavigatorRenderer.ts`).
/// The image is scaled to fit in the square whole and is centred in it. That
/// leaves empty bands above and below a wide image, or to the left and right
/// of a tall one.
pub fn navigator_image(image: Size, side: f32) -> (f32, f32, f32, f32) {
    contain(image, (side, side)).unwrap_or((0., 0., side, side))
}

/// The point of the image that is under the pointer when the pointer is at
/// `local` in the navigator (`useNavigatorDrag.ts`). `local` is relative to
/// the navigator's top-left corner. The result is in fractions of the
/// image's width and height, limited to the range from 0 to 1.
pub fn navigator_point(local: (f32, f32), image: Size, side: f32) -> (f32, f32) {
    let (left, top, width, height) = navigator_image(image, side);
    (
        ((local.0 - left) / width).clamp(0., 1.),
        ((local.1 - top) / height).clamp(0., 1.),
    )
}

/// `LERP_SPEED`: while the user drags in the navigator, the viewer does not
/// jump to the point under the pointer. Each frame it moves this share of
/// the remaining distance towards that point.
pub const NAVIGATOR_FOLLOW: f32 = 0.3;

/// One frame of that movement: the point that is [`NAVIGATOR_FOLLOW`] of the
/// way from `current` to `target`. Returns `target` itself once `current`
/// is within 0.001 of it on both axes. The points are in fractions of the
/// image's width and height.
pub fn follow(current: (f32, f32), target: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (target.0 - current.0, target.1 - current.1);
    if dx.abs() < 0.001 && dy.abs() < 0.001 {
        target
    } else {
        (
            current.0 + dx * NAVIGATOR_FOLLOW,
            current.1 + dy * NAVIGATOR_FOLLOW,
        )
    }
}

/// An animated move of the camera from `from` to `to` that began at
/// `started`. It lasts [`crate::motion::ZOOM_GLIDE`] and follows the curve
/// of [`crate::motion::zoom_glide_progress`].
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    pub from: Camera,
    pub to: Camera,
    pub started: Instant,
}

impl Glide {
    /// The camera `elapsed` after the animation started, and whether the
    /// animation has finished.
    pub fn at(&self, elapsed: Duration) -> (Camera, bool) {
        if elapsed >= crate::motion::ZOOM_GLIDE {
            (self.to, true)
        } else {
            let eased = crate::motion::zoom_glide_progress(elapsed);
            (self.from.lerp(self.to, eased), false)
        }
    }
}

/// The grey squares of the checkerboard that is drawn behind an image with
/// transparent parts (`.transparencyGrid` in production). The square at the
/// image's top-left corner is white, and the squares alternate between
/// white and grey from there.
///
/// `image` is the image's rectangle on screen and `visible` is the part of
/// the window that is drawn, both as `(left, top, width, height)`. `side` is
/// the width of one square on screen; the caller passes 8 image pixels
/// times the scale. Returns the grey squares that are inside both
/// rectangles, each cut off at the rectangles' edges, as
/// `(left, top, width, height)`. The caller fills the area with white
/// before it draws them.
pub fn checker_squares(
    image: (f32, f32, f32, f32),
    visible: (f32, f32, f32, f32),
    side: f32,
) -> Vec<(f32, f32, f32, f32)> {
    let left = image.0.max(visible.0);
    let top = image.1.max(visible.1);
    let right = (image.0 + image.2).min(visible.0 + visible.2);
    let bottom = (image.1 + image.3).min(visible.1 + visible.3);
    if side <= 0. || right <= left || bottom <= top {
        return Vec::new();
    }
    let first_column = ((left - image.0) / side).floor() as i64;
    let last_column = ((right - image.0) / side).ceil() as i64;
    let first_row = ((top - image.1) / side).floor() as i64;
    let last_row = ((bottom - image.1) / side).ceil() as i64;
    let mut squares = Vec::new();
    for row in first_row..last_row {
        for column in first_column..last_column {
            if (row + column) % 2 == 0 {
                continue;
            }
            let x = (image.0 + column as f32 * side).max(left);
            let y = (image.1 + row as f32 * side).max(top);
            let x_end = (image.0 + (column + 1) as f32 * side).min(right);
            let y_end = (image.1 + (row + 1) as f32 * side).min(bottom);
            if x_end > x && y_end > y {
                squares.push((x, y, x_end - x, y_end - y));
            }
        }
    }
    squares
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checker_squares_alternate_from_a_white_corner_and_stay_visible() {
        // A 32×32 image at 100% has 4 rows of 4 squares, each 8 wide. Half
        // of the 16 squares are grey.
        let squares = checker_squares((0., 0., 32., 32.), (0., 0., 100., 100.), 8.);
        assert_eq!(squares.len(), 8);
        assert_eq!(squares[0], (8., 0., 8., 8.));
        assert!(squares.contains(&(0., 8., 8., 8.)));
        assert!(!squares.contains(&(0., 0., 8., 8.)));
        // Only the squares inside the visible rectangle are returned, and a
        // square that crosses its edge is cut off there.
        let squares = checker_squares((-100., 0., 132., 16.), (0., 0., 10., 100.), 8.);
        assert!(squares.iter().all(|s| s.0 >= 0. && s.0 + s.2 <= 10.));
        assert_eq!(squares.len(), 2);
        // No squares are returned when the image is outside the visible
        // rectangle.
        assert!(checker_squares((200., 0., 8., 8.), (0., 0., 100., 100.), 8.).is_empty());
    }

    const IMAGE: Size = (2000., 1000.);
    const PANE: Size = (800., 600.);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn fit_takes_the_tighter_axis() {
        assert!(close(fit_scale(IMAGE, PANE), 0.4));
        assert!(close(fit_scale((300., 1200.), PANE), 0.5));
        assert_eq!(fit_scale(IMAGE, (0., 0.)), 1.);
        // A pane with no height (a window resized down to a line) gives a
        // fit scale of 1, not 0. A later zoom divides by the current scale,
        // so a scale of 0 would give an offset that is not a finite number.
        assert_eq!(fit_scale(IMAGE, (800., 0.)), 1.);
        assert!(
            Camera::fit(IMAGE, (800., 0.))
                .zoomed(2., Some((1., 1.)))
                .tx
                .is_finite()
        );
        let fitted = Camera::fit(IMAGE, PANE);
        assert_eq!(fitted.image_rect(IMAGE, PANE), (0., 100., 800., 400.));
        assert_eq!(contain(IMAGE, PANE), Some((0., 100., 800., 400.)));
        assert_eq!(
            contain((600., 800.), (1200., 900.)),
            Some((262.5, 0., 675., 900.))
        );
        assert_eq!(contain(IMAGE, (0., 900.)), None);
        assert_eq!(fitted.percent(), 40.);
    }

    #[test]
    fn a_resized_pane_keeps_the_framing() {
        // An image that was fitted in the old pane is fitted in the new one.
        let fitted = Camera::fit(IMAGE, PANE);
        let bigger = (1200., 900.);
        assert!(close(
            fitted.resized(IMAGE, PANE, bigger).scale,
            fit_scale(IMAGE, bigger)
        ));
        // A zoomed and panned camera has its scale and its offset multiplied
        // by 1.5, the ratio of the new pane's size to the old one's.
        let zoomed = Camera {
            scale: 0.8,
            tx: 40.,
            ty: -30.,
        };
        let resized = zoomed.resized(IMAGE, PANE, bigger);
        assert!(close(resized.scale, 1.2));
        assert!(close(resized.tx, 60.));
        assert!(close(resized.ty, -45.));
        // The old pane has no size because it has not been measured yet, so
        // the camera is returned unchanged.
        assert_eq!(zoomed.resized(IMAGE, (0., 0.), bigger), zoomed);
    }

    #[test]
    fn zooming_about_a_point_keeps_that_point_still() {
        let camera = Camera {
            scale: 0.5,
            tx: 30.,
            ty: -20.,
        };
        let focal = (120., 80.);
        // The point of the image that is under the focal point for a given
        // camera, in image pixels from the image's centre.
        let under = |camera: Camera| {
            (
                (focal.0 - camera.tx) / camera.scale,
                (focal.1 - camera.ty) / camera.scale,
            )
        };
        let zoomed = camera.zoomed(2., Some(focal));
        assert_eq!(zoomed.scale, 2.);
        let (before, after) = (under(camera), under(zoomed));
        assert!(close(before.0, after.0) && close(before.1, after.1));
    }

    #[test]
    fn zoom_is_clamped_and_keeps_the_offset_without_a_focal_point() {
        let camera = Camera {
            scale: 1.,
            tx: 10.,
            ty: 5.,
        };
        assert_eq!(camera.zoomed(100., None).scale, MAX_SCALE);
        assert_eq!(camera.zoomed(0.001, None).scale, MIN_SCALE);
        assert_eq!(camera.zoomed(2., None).tx, 10.);
        let centred = camera.zoomed_about_centre(2.);
        assert_eq!((centred.tx, centred.ty), (20., 10.));
    }

    #[test]
    fn panning_moves_the_image_with_the_pointer() {
        let camera = Camera::fit(IMAGE, PANE).panned(15., -25.);
        assert_eq!(camera.image_rect(IMAGE, PANE), (15., 75., 800., 400.));
    }

    #[test]
    fn wheel_factor_zooms_in_when_scrolling_up() {
        assert!(wheel_factor(-10.) > 1.);
        assert!(wheel_factor(10.) < 1.);
        assert!(close(wheel_factor(10.) * wheel_factor(-10.), 1.));
    }

    #[test]
    fn the_navigator_shows_nothing_while_the_image_fits() {
        assert_eq!(visible_part(Camera::fit(IMAGE, PANE), IMAGE, PANE), None);
        // An image that is 2 points wider than the pane still counts as
        // fitting in it.
        let nearly = Camera {
            scale: 0.4 + 2. / 2000.,
            tx: 0.,
            ty: 0.,
        };
        assert_eq!(visible_part(nearly, IMAGE, PANE), None);
    }

    #[test]
    fn the_navigator_frames_the_visible_part() {
        // At 100% the 800×600 pane shows the middle of the 2000×1000 image.
        let part = visible_part(Camera::actual(), IMAGE, PANE).unwrap();
        assert!(close(part.0, 0.3) && close(part.1, 0.2));
        assert!(close(part.2, 0.4) && close(part.3, 0.6));
        // Panned 600 points to the right, which puts the image's left edge
        // at the pane's left edge.
        let left = Camera::actual().panned(600., 0.);
        let part = visible_part(left, IMAGE, PANE).unwrap();
        assert!(close(part.0, 0.) && close(part.2, 0.4));
    }

    #[test]
    fn navigator_points_map_back_to_the_image() {
        // A 2:1 image in the 120px square is drawn 120 wide and 60 tall,
        // with an empty band 30 tall above it and below it.
        assert_eq!(navigator_image(IMAGE, 120.), (0., 30., 120., 60.));
        assert_eq!(navigator_point((60., 60.), IMAGE, 120.), (0.5, 0.5));
        assert_eq!(navigator_point((0., 0.), IMAGE, 120.), (0., 0.));
        // After `centred_on`, the point a quarter of the way across the
        // image is at the horizontal middle of the pane.
        let camera = Camera::actual().centred_on(0.25, 0.5, IMAGE);
        let (left, _, width, _) = camera.image_rect(IMAGE, PANE);
        assert!(close(left + 0.25 * width, PANE.0 / 2.));
    }

    #[test]
    fn the_navigator_follow_closes_in_and_lands() {
        let next = follow((0., 0.), (1., 1.));
        assert!(close(next.0, 0.3));
        assert_eq!(follow((0.9995, 0.5), (1., 0.5)), (1., 0.5));
    }

    #[test]
    fn a_glide_eases_out_and_lands_on_its_target() {
        let glide = Glide {
            from: Camera::actual(),
            to: Camera::actual().zoomed(2., None),
            started: Instant::now(),
        };
        assert_eq!(glide.at(Duration::ZERO).0.scale, 1.);
        let (half, done) = glide.at(crate::motion::ZOOM_GLIDE / 2);
        // The curve is an ease-out, so at half of the time the scale is
        // already more than halfway from 1 to 2.
        assert!(!done && half.scale > 1.5);
        assert_eq!(glide.at(crate::motion::ZOOM_GLIDE), (glide.to, true));
    }
}
