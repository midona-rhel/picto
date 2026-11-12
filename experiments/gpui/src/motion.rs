//! Motion tokens from `src/shared/styles/tokens.css` and the CSS
//! `cubic-bezier()` timing function they are written in.
use std::time::Duration;

/// `--ease-default`
pub const EASE_DEFAULT: CubicBezier = CubicBezier::new(0.4, 0., 0.22, 1.);
/// The grid and scope title fade for this long on each side of a scope change.
pub const SCOPE_FADE: Duration = Duration::from_millis(170);

#[derive(Clone, Copy, Debug)]
pub struct CubicBezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl CubicBezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn axis(t: f32, p1: f32, p2: f32) -> f32 {
        let inverse = 1. - t;
        3. * inverse * inverse * t * p1 + 3. * inverse * t * t * p2 + t * t * t
    }

    /// Output progress for input progress `x`, both in `0..=1`.
    pub fn ease(&self, x: f32) -> f32 {
        if x <= 0. {
            return 0.;
        }
        if x >= 1. {
            return 1.;
        }
        // x(t) is monotonic for control points inside 0..=1, so bisection finds
        // the curve parameter for this x.
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..24 {
            let middle = (low + high) / 2.;
            if Self::axis(middle, self.x1, self.x2) < x {
                low = middle;
            } else {
                high = middle;
            }
        }
        Self::axis((low + high) / 2., self.y1, self.y2)
    }
}

/// Progress of an animation that began `elapsed` ago, in `0..=1`.
pub fn progress(elapsed: Duration, duration: Duration) -> f32 {
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_control_points_are_the_identity() {
        let linear = CubicBezier::new(0.25, 0.25, 0.75, 0.75);
        for step in 0..=20 {
            let x = step as f32 / 20.;
            assert!((linear.ease(x) - x).abs() < 1e-3, "{x}");
        }
    }

    #[test]
    fn default_ease_matches_the_sampled_curve() {
        // Walk the parametric curve finely and check the solver against it.
        for step in 1..2000 {
            let t = step as f32 / 2000.;
            let x = CubicBezier::axis(t, 0.4, 0.22);
            let y = CubicBezier::axis(t, 0., 1.);
            assert!((EASE_DEFAULT.ease(x) - y).abs() < 2e-3, "t={t}");
        }
        assert_eq!(EASE_DEFAULT.ease(0.), 0.);
        assert_eq!(EASE_DEFAULT.ease(1.), 1.);
        // Slow start, fast finish: under the diagonal early, over it late.
        assert!(EASE_DEFAULT.ease(0.1) < 0.1);
        assert!(EASE_DEFAULT.ease(0.6) > 0.6);
    }

    #[test]
    fn progress_is_clamped() {
        let duration = Duration::from_millis(170);
        assert_eq!(progress(Duration::ZERO, duration), 0.);
        assert_eq!(progress(Duration::from_millis(85), duration), 0.5);
        assert_eq!(progress(Duration::from_secs(1), duration), 1.);
    }
}
