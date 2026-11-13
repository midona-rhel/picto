//! Zoom and pan of an image in the viewer, as `useImageZoom.ts` keeps it: a
//! scale, and an offset of the image's centre from the pane's centre. The
//! image is drawn at its natural size times the scale, so 100% is one image
//! pixel per point.
use std::time::{Duration, Instant};

/// `MIN_SCALE` and `MAX_SCALE`.
pub const MIN_SCALE: f32 = 0.05;
pub const MAX_SCALE: f32 = 8.0;
/// Keyboard and wheel-notch zoom multiply or divide by this.
pub const STEP: f32 = 1.25;
/// A pixel wheel delta at least this large is a wheel notch, not a trackpad.
pub const NOTCH: f32 = 40.;
/// Wheel notches within this time of each other zoom from where the last one
/// was headed, not from where the glide has got to.
pub const NOTCH_RUN: Duration = Duration::from_millis(200);
/// `FIT_ROUNDING_TOLERANCE_PX`: an image within this of fitting counts as
/// fitted, so the navigator does not flicker while the window is resized.
const FIT_TOLERANCE: f32 = 3.;

pub type Size = (f32, f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scale: f32,
    /// The image centre's offset from the pane's centre, in points.
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

/// The scale at which the whole image fits the pane. An empty pane or image
/// leaves it at 1.
pub fn fit_scale(image: Size, pane: Size) -> f32 {
    if pane.0 <= 0. || image.0 <= 0. || image.1 <= 0. {
        return 1.;
    }
    (pane.0 / image.0).min(pane.1 / image.1)
}

impl Camera {
    /// `fitToWindow`: the whole image, centred. Not clamped, as in Electron.
    pub fn fit(image: Size, pane: Size) -> Self {
        Self {
            scale: fit_scale(image, pane),
            tx: 0.,
            ty: 0.,
        }
    }

    /// `fitActual`: one image pixel per point, centred.
    pub fn actual() -> Self {
        Self {
            scale: 1.,
            tx: 0.,
            ty: 0.,
        }
    }

    /// The zoom as the toolbar shows it.
    pub fn percent(&self) -> f32 {
        (self.scale * 100.).round()
    }

    /// `zoomTo`: a new scale. With a focal point (relative to the pane's
    /// centre) that point of the image stays where it is; without one the
    /// offset is kept as it is.
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

    /// The target of `animateZoomTo` without a focal point: the offset grows
    /// with the scale, so the point at the pane's centre stays there.
    pub fn zoomed_about_centre(self, scale: f32) -> Self {
        let scale = clamp_scale(scale);
        let ratio = scale / self.scale;
        Self {
            scale,
            tx: self.tx * ratio,
            ty: self.ty * ratio,
        }
    }

    /// The camera a drag that began at `self` has moved by `(dx, dy)`.
    pub fn panned(self, dx: f32, dy: f32) -> Self {
        Self {
            tx: self.tx + dx,
            ty: self.ty + dy,
            ..self
        }
    }

    /// The detail window's resize: the scale follows the fit scale's change
    /// and the offset the pane's, so a fitted image stays fitted and a
    /// zoomed one keeps its framing (`DetailWindow`'s `ResizeObserver`).
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

    /// `panToNormalized`: centres the pane on a point of the image given as
    /// fractions of its width and height.
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

/// The zoom factor for one smooth (trackpad) wheel event; `delta` is the
/// browser's `deltaY`, positive when scrolling down, which zooms out.
pub fn wheel_factor(delta: f32) -> f32 {
    (-delta * 0.004).exp()
}

/// The part of the image the pane shows, as fractions of the image
/// (`computeNavigatorRect`): `(x, y, width, height)`. `None` while the whole
/// image fits, when there is no navigator.
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

/// How the image sits in the square navigator: `(left, top, width, height)`,
/// letterboxed to fit (`useNavigatorRenderer.ts`).
pub fn navigator_image(image: Size, side: f32) -> (f32, f32, f32, f32) {
    let aspect = image.0 / image.1;
    if aspect > 1. {
        let height = side / aspect;
        (0., (side - height) / 2., side, height)
    } else {
        let width = side * aspect;
        ((side - width) / 2., 0., width, side)
    }
}

/// The point of the image under a press in the navigator, as fractions of
/// the image (`useNavigatorDrag.ts`). `local` is relative to the navigator.
pub fn navigator_point(local: (f32, f32), image: Size, side: f32) -> (f32, f32) {
    let (left, top, width, height) = navigator_image(image, side);
    (
        ((local.0 - left) / width).clamp(0., 1.),
        ((local.1 - top) / height).clamp(0., 1.),
    )
}

/// `LERP_SPEED`: while the navigator is dragged, the view moves this share
/// of the remaining way each frame.
pub const NAVIGATOR_FOLLOW: f32 = 0.3;

/// One frame of the navigator's follow: the next point, or the target once
/// within a thousandth of it.
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

/// A camera move that glides over [`crate::motion::ZOOM_GLIDE`] on an ease-out cubic.
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    pub from: Camera,
    pub to: Camera,
    pub started: Instant,
}

impl Glide {
    /// Where the glide is `elapsed` after it started, and whether it is over.
    pub fn at(&self, elapsed: Duration) -> (Camera, bool) {
        let t = (elapsed.as_secs_f32() / crate::motion::ZOOM_GLIDE.as_secs_f32()).clamp(0., 1.);
        let eased = 1. - (1. - t).powi(3);
        if t >= 1. {
            (self.to, true)
        } else {
            (self.from.lerp(self.to, eased), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let fitted = Camera::fit(IMAGE, PANE);
        assert_eq!(fitted.image_rect(IMAGE, PANE), (0., 100., 800., 400.));
        assert_eq!(fitted.percent(), 40.);
    }

    #[test]
    fn a_resized_pane_keeps_the_framing() {
        // Fitted stays fitted.
        let fitted = Camera::fit(IMAGE, PANE);
        let bigger = (1200., 900.);
        assert!(close(
            fitted.resized(IMAGE, PANE, bigger).scale,
            fit_scale(IMAGE, bigger)
        ));
        // A zoomed, panned view scales with the pane.
        let zoomed = Camera {
            scale: 0.8,
            tx: 40.,
            ty: -30.,
        };
        let resized = zoomed.resized(IMAGE, PANE, bigger);
        assert!(close(resized.scale, 1.2));
        assert!(close(resized.tx, 60.));
        assert!(close(resized.ty, -45.));
        // Nothing measured yet: unchanged.
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
        // The image point under the focal point, before and after.
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
        // A few points over still counts as fitted.
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
        // Panned to the left edge.
        let left = Camera::actual().panned(600., 0.);
        let part = visible_part(left, IMAGE, PANE).unwrap();
        assert!(close(part.0, 0.) && close(part.2, 0.4));
    }

    #[test]
    fn navigator_points_map_back_to_the_image() {
        // A wide image is letterboxed top and bottom in the 120px square.
        assert_eq!(navigator_image(IMAGE, 120.), (0., 30., 120., 60.));
        assert_eq!(navigator_point((60., 60.), IMAGE, 120.), (0.5, 0.5));
        assert_eq!(navigator_point((0., 0.), IMAGE, 120.), (0., 0.));
        // Centring on a point puts it in the middle of the pane.
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
        // Ease-out: past halfway at half time.
        assert!(!done && half.scale > 1.5);
        assert_eq!(glide.at(crate::motion::ZOOM_GLIDE), (glide.to, true));
    }
}
