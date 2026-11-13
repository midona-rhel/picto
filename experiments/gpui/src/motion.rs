//! Motion tokens from `src/shared/styles/tokens.css` and the CSS
//! `cubic-bezier()` timing function they are written in.
use std::time::Duration;

/// `--ease-default`
pub const EASE_DEFAULT: CubicBezier = CubicBezier::new(0.4, 0., 0.22, 1.);
/// CSS `ease-in`.
pub const EASE_IN: CubicBezier = CubicBezier::new(0.42, 0., 1., 1.);
/// CSS `ease-out`.
pub const EASE_OUT: CubicBezier = CubicBezier::new(0., 0., 0.58, 1.);
/// CSS `ease-in-out`.
pub const EASE_IN_OUT: CubicBezier = CubicBezier::new(0.42, 0., 0.58, 1.);
/// `--ease-scale-in`: popovers and menus growing in.
pub const EASE_SCALE_IN: CubicBezier = CubicBezier::new(0., 0.89, 0.44, 1.);
/// `--duration-menu-in`: a popover's scale-in.
pub const MENU_IN: Duration = Duration::from_millis(70);
/// CSS `ease`.
pub const EASE: CubicBezier = CubicBezier::new(0.25, 0.1, 0.25, 1.);
/// The viewer's "First item" / "Last item" notice: its fade (`200ms ease`)
/// and how long it stays up.
pub const BOUNDARY_FADE: Duration = Duration::from_millis(200);
pub const BOUNDARY_HOLD: Duration = Duration::from_millis(800);
/// `NAVIGATOR_SHOW_DELAY_MS`: the viewer's navigator appears this long after
/// the image first overflows the pane.
pub const NAVIGATOR_DELAY: Duration = Duration::from_millis(80);
/// `QuickLookNavigation`'s buttons: `transition: background 200ms ease`.
pub const STRIP_BUTTON: Duration = Duration::from_millis(200);
/// `--ease-standard`
pub const EASE_STANDARD: CubicBezier = CubicBezier::new(0.4, 0., 0.2, 1.);
/// `ScrollToTopButton`: its rise and fade.
pub const RETURN_TO_TOP: Duration = Duration::from_millis(150);
/// A context-menu row's highlight.
pub const MENU_ROW: Duration = Duration::from_millis(30);
/// `menuOut`: a dismissed context menu fades for this long.
pub const MENU_OUT: Duration = Duration::from_millis(80);
/// `handleSubmenuIntent`: an open submenu stays this long after the pointer
/// moves onto another row of its menu.
pub const MENU_SUBMENU_GRACE: Duration = Duration::from_millis(150);
/// The grid and scope title fade for this long on each side of a scope change.
pub const SCOPE_FADE: Duration = Duration::from_millis(170);
/// `HoverPreviewPortal.tsx`: the backdrop and the original fade in together.
pub const HOVER_PREVIEW_FADE: Duration = Duration::from_millis(150);
/// `EmptyState.module.css` `.action`: its background on hover.
pub const EMPTY_ACTION_HOVER: Duration = Duration::from_millis(250);

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

/// `--sidebar-list-highlight-transition`: a sidebar row's highlight fade.
pub const SIDEBAR_HOVER: Duration = Duration::from_millis(50);
/// `iconButton.module.css`: an icon button's hover fade.
pub const BUTTON_HOVER: Duration = Duration::from_millis(150);
/// `.sidebar { transition: transform var(--duration-fast) }`.
pub const SIDEBAR_SLIDE: Duration = Duration::from_millis(50);
/// `inspectorRailIn` / `inspectorRailOut`.
pub const INSPECTOR_SLIDE: Duration = Duration::from_millis(100);

/// `.querySmallBtn`: a row button's opacity and fill, on `ease`.
pub const SMALL_BUTTON: Duration = Duration::from_millis(100);
/// `.followCover`: a subscription card's border on hover, on `ease`.
pub const CARD_HOVER: Duration = Duration::from_millis(120);
/// `subscriptionStatusPulse`: one breath of a syncing badge, on `ease-in-out`.
pub const STATUS_PULSE: Duration = Duration::from_millis(2400);

/// Duplicate review: an original fades in over its thumbnail (`.fullImage`).
pub const FULL_IMAGE_FADE: Duration = Duration::from_millis(130);
/// `LOADING_MESSAGE_DELAY_MS`: a loading message waits this long, so a
/// quick load shows none.
pub const LOADING_MESSAGE_DELAY: Duration = Duration::from_millis(200);

/// `--duration-normal`: disclosure arrows, chips and field borders.
pub const NORMAL: Duration = Duration::from_millis(200);
/// The search field's border and background on focus.
pub const FIELD_FOCUS: Duration = Duration::from_millis(160);
/// `KbdTooltip`: `openDelay`, the `pop` transition, and `offset`.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
pub const TOOLTIP_FADE: Duration = Duration::from_millis(150);
pub const TOOLTIP_OFFSET: f32 = 6.;

/// Where a fade that set off from `from` toward 0 or 1 has got to.
fn fade_value(from: f32, hovered: bool, elapsed: Duration, duration: Duration) -> f32 {
    let target = if hovered { 1. } else { 0. };
    from + (target - from) * EASE_DEFAULT.ease(progress(elapsed, duration))
}

struct HoverFade {
    hovered: bool,
    since: std::time::Instant,
    from: f32,
    duration: Duration,
}

/// One two-state value that moves as a CSS transition does. Components keep
/// one per element for their hover highlight.
pub struct Fade {
    on: bool,
    since: std::time::Instant,
    from: f32,
    duration: Duration,
}

impl Default for Fade {
    fn default() -> Self {
        Self {
            on: false,
            since: std::time::Instant::now(),
            from: 0.,
            duration: Duration::ZERO,
        }
    }
}

impl Fade {
    /// A fade already at rest in the given state.
    pub fn at(on: bool) -> Self {
        Self {
            on,
            ..Self::default()
        }
    }

    /// Moves toward `on` from wherever the fade is now. Setting the state it
    /// is already in changes nothing.
    pub fn set(&mut self, on: bool, duration: Duration) {
        if self.on == on {
            return;
        }
        self.from = self.value();
        self.on = on;
        self.since = std::time::Instant::now();
        // A transition reversed part-way takes only the time of the distance
        // left to cover, as CSS shortens it.
        let distance = if on { 1. - self.from } else { self.from };
        self.duration = duration.mul_f32(distance.clamp(0., 1.));
    }

    /// 0 at rest, 1 fully on, moving on the default curve.
    pub fn value(&self) -> f32 {
        self.value_with(EASE_DEFAULT)
    }

    /// The same, on another timing function.
    pub fn value_with(&self, curve: CubicBezier) -> f32 {
        let target = if self.on { 1. } else { 0. };
        if self.duration.is_zero() {
            return target;
        }
        self.from + (target - self.from) * curve.ease(progress(self.since.elapsed(), self.duration))
    }

    pub fn on(&self) -> bool {
        self.on
    }

    pub fn animating(&self) -> bool {
        self.since.elapsed() < self.duration
    }
}

/// Two-state values that move as CSS transitions do: toward the new state on
/// the default curve, starting from wherever the previous move had reached.
/// Used for hover highlights and for sliding the rails in and out.
#[derive(Default)]
pub struct Fades {
    fades: std::collections::HashMap<(&'static str, usize), HoverFade>,
}

impl Fades {
    pub fn set(&mut self, key: (&'static str, usize), hovered: bool, duration: Duration) {
        let from = self.value(key);
        self.fades
            .retain(|_, fade| fade.hovered || fade.since.elapsed() < fade.duration);
        self.fades.insert(
            key,
            HoverFade {
                hovered,
                since: std::time::Instant::now(),
                from,
                duration,
            },
        );
    }

    /// The highlight's strength, 0 at rest and 1 fully hovered.
    pub fn value(&self, key: (&'static str, usize)) -> f32 {
        self.fades.get(&key).map_or(0., |fade| {
            fade_value(fade.from, fade.hovered, fade.since.elapsed(), fade.duration)
        })
    }

    pub fn animating(&self) -> bool {
        self.fades
            .values()
            .any(|fade| fade.since.elapsed() < fade.duration)
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
    fn a_reversed_hover_fade_continues_from_where_it_was() {
        let duration = Duration::from_millis(150);
        assert_eq!(fade_value(0., true, Duration::ZERO, duration), 0.);
        assert_eq!(fade_value(0., true, duration, duration), 1.);
        let halfway = fade_value(0., true, Duration::from_millis(75), duration);
        assert!(halfway > 0. && halfway < 1.);
        // The pointer leaves mid-fade: the fade-out starts at that strength.
        assert_eq!(
            fade_value(halfway, false, Duration::ZERO, duration),
            halfway
        );
        assert_eq!(fade_value(halfway, false, duration, duration), 0.);
    }

    #[test]
    fn a_fade_reversed_part_way_takes_less_than_its_full_time() {
        let mut fade = Fade::default();
        fade.set(true, Duration::from_secs(10));
        // Barely begun, so going back covers almost no distance.
        fade.set(false, Duration::from_secs(10));
        assert!(fade.duration < Duration::from_secs(1));
        assert!(!fade.on());
        // Setting the state it is already in starts nothing.
        let since = fade.since;
        fade.set(false, Duration::from_secs(10));
        assert_eq!(fade.since, since);
        assert!(Fade::at(true).value() == 1. && !Fade::at(true).animating());
    }

    #[test]
    fn progress_is_clamped() {
        let duration = Duration::from_millis(170);
        assert_eq!(progress(Duration::ZERO, duration), 0.);
        assert_eq!(progress(Duration::from_millis(85), duration), 0.5);
        assert_eq!(progress(Duration::from_secs(1), duration), 1.);
    }
}
