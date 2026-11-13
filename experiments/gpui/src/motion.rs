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
/// `--duration-menu-in`: a popover's and a context menu's scale-in.
pub const MENU_IN: Duration = Duration::from_millis(70);
/// `menuIn` and `librarySwitcherIn`: the scale a menu or popover grows from.
pub const MENU_IN_SCALE: f32 = 0.96;
/// `menuOut`: the scale a dismissed context menu shrinks to as it fades.
pub const MENU_OUT_SCALE: f32 = 0.95;
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
/// The detail window's toolbar: `transition: opacity 250ms ease`, and
/// `TOOLBAR_HIDE_DELAY`, how long it stays after the pointer last moved.
pub const DETAIL_TOOLBAR_FADE: Duration = Duration::from_millis(250);
pub const DETAIL_TOOLBAR_HIDE: Duration = Duration::from_millis(1000);
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
/// `GlassModal`: `modalIn` and `backdropIn`.
pub const MODAL_IN: Duration = Duration::from_millis(150);
/// `GlassModal`: `EXIT_MS`, `modalOut` and `backdropOut`.
pub const MODAL_OUT: Duration = Duration::from_millis(120);
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
/// `.conditionButton`: a smart folder rule's minus and plus buttons, on
/// `ease`.
pub const CONDITION_BUTTON: Duration = Duration::from_millis(120);
/// `.tagAddBtn`: the "+" after a row of chips brightening, on
/// `--ease-default`.
pub const TAG_ADD_BUTTON: Duration = Duration::from_millis(200);
/// `.followCover`: a subscription card's border on hover, on `ease`.
pub const CARD_HOVER: Duration = Duration::from_millis(120);
/// Smart merge's survivor ring fading in and out (`.mediaCard::after`).
pub const SURVIVOR_RING: Duration = Duration::from_millis(140);
/// `subscriptionStatusPulse`: one breath of a syncing badge, on `ease-in-out`.
pub const STATUS_PULSE: Duration = Duration::from_millis(2400);

/// Duplicate review: an original fades in over its thumbnail (`.fullImage`).
pub const FULL_IMAGE_FADE: Duration = Duration::from_millis(130);
/// `LOADING_MESSAGE_DELAY_MS`: a loading message waits this long, so a
/// quick load shows none.
pub const LOADING_MESSAGE_DELAY: Duration = Duration::from_millis(200);

/// How long the viewer keeps the previous item on screen while a video
/// opening waits for its first frame, or a document for its first page,
/// before showing the new item without it.
pub const MEDIA_HOLD: Duration = Duration::from_millis(1500);

/// `--duration-normal`: disclosure arrows, chips and field borders.
pub const NORMAL: Duration = Duration::from_millis(200);
/// A settings row's or check row's highlight (`background 100ms ease`), and
/// a theme swatch's outline.
pub const ROW_HOVER: Duration = Duration::from_millis(100);
/// `CompactNumberInput`'s stepper arrows brightening (`opacity 120ms`), and
/// `GlassInput`'s border on focus.
pub const STEPPER: Duration = Duration::from_millis(120);
/// The search field's border and background on focus.
pub const FIELD_FOCUS: Duration = Duration::from_millis(160);
/// `fieldExpand`: an inspector field opening over what follows.
pub const FIELD_EXPAND: Duration = Duration::from_millis(100);
/// `OverlayShell`'s `.pinBtn`: its overlay under the pointer.
pub const PIN_BUTTON: Duration = Duration::from_millis(120);
/// `TagSelectPanel.module.css` `.sidebar`: the tag picker's group rail
/// fading in (150 ms) and out (100 ms).
pub const PICKER_RAIL_IN: Duration = Duration::from_millis(150);
pub const PICKER_RAIL_OUT: Duration = Duration::from_millis(100);
/// `GlassInput`: a dialog field's border on focus.
pub const FIELD_BORDER: Duration = Duration::from_millis(120);
/// `KbdTooltip`: `openDelay`, the `pop` transition, and `offset`.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
pub const TOOLTIP_FADE: Duration = Duration::from_millis(150);
pub const TOOLTIP_OFFSET: f32 = 6.;
/// Mantine's `pop` transition, which `KbdTooltip` uses: hidden, the tooltip
/// is at `scale(.9) translateY(10px)` about its centre.
pub const TOOLTIP_POP_SCALE: f32 = 0.9;
pub const TOOLTIP_POP_RISE: f32 = 10.;

/// `inspectorFadeIn` / `inspectorFadeOut`: the inspector rail fades rather
/// than slides when it becomes available or unavailable (a screen opened
/// or left), not when it is toggled.
pub const INSPECTOR_FADE: Duration = Duration::from_millis(170);

/// `modalIn` / `modalOut`: the panel's scale and drop when hidden.
pub const MODAL_SCALE: f32 = 0.97;
pub const MODAL_DROP: f32 = 4.;

/// `ScrollToTopButton`: its rise and fade wait this long before starting.
pub const RETURN_TO_TOP_DELAY: Duration = Duration::from_millis(30);

/// The tag manager (`TagManagerScreen.module.css`): `.viewLeaving`, the
/// list fading out before another group shows; `.viewEntering`, the new
/// list fading in; `.tagListInner`, the rows fading in each time the list
/// is replaced; and `tagSkeleton`, one pass of the placeholder shimmer.
pub const TAG_VIEW_LEAVE: Duration = Duration::from_millis(100);
pub const TAG_VIEW_ENTER: Duration = Duration::from_millis(140);
pub const TAG_LIST_FADE: Duration = Duration::from_millis(90);
pub const TAG_SHIMMER: Duration = Duration::from_millis(800);

/// The inspector's preview stack (`@keyframes stackPreviewLeft`,
/// `stackPreviewRight`): a card entering from the left or the right, and
/// one leaving, which fades over 200 ms and is gone at 220 ms.
pub const STACK_ENTER_LEFT: Duration = Duration::from_millis(200);
pub const STACK_ENTER_RIGHT: Duration = Duration::from_millis(250);
pub const STACK_EXIT_FADE: Duration = Duration::from_millis(200);
pub const STACK_EXIT_REMOVE: Duration = Duration::from_millis(220);

/// The grid's hover preview (`HoverPreviewPortal.tsx`):
/// `HOVER_PREVIEW_DELAY_MS`, how long the pointer rests on the magnifier;
/// `HOVER_HIDE_DELAY_MS`, how long after leaving it the preview stays; and
/// `MIN_WAIT_MS`, the preview never appearing sooner than this after it
/// opens.
pub const HOVER_PREVIEW_DELAY: Duration = Duration::from_millis(200);
pub const HOVER_PREVIEW_HIDE: Duration = Duration::from_millis(90);
pub const HOVER_PREVIEW_MIN_WAIT: Duration = Duration::from_millis(150);
/// `HOVER_RESUME_DELAY_MS`: the grid's magnifier comes back this long after
/// scrolling.
pub const MAGNIFIER_RESUME: Duration = Duration::from_millis(80);
/// `thumbnailTiming.ts`: a thumbnail entering the viewport waits 100 ms,
/// then fades in over 500 ms.
pub const THUMBNAIL_REVEAL_DELAY: Duration = Duration::from_millis(100);
pub const THUMBNAIL_REVEAL: Duration = Duration::from_millis(500);

/// The viewer's `ANIM_DURATION_MS`: a keyboard or wheel-notch zoom glides
/// for this long.
pub const ZOOM_GLIDE: Duration = Duration::from_millis(150);

/// A text field's caret: on and off for this long each.
pub const CARET_BLINK: Duration = Duration::from_millis(500);
/// `ToggleSwitch`: `transition: 0.2s ease`.
pub const TOGGLE_SWITCH: Duration = Duration::from_millis(200);
/// `progress-indeterminate`: one sweep of an indeterminate progress bar.
pub const PROGRESS_SWEEP: Duration = Duration::from_millis(900);
/// A Flash movie is advanced and redrawn this often (about 60 frames a
/// second).
pub const FLASH_FRAME: Duration = Duration::from_millis(16);
/// Settings shows "Saving…" only for a save that takes longer than this.
pub const SAVING_MESSAGE_DELAY: Duration = Duration::from_millis(160);

/// `.controls` in `VideoPlayer.module.css`: the media controls fade in and
/// out over 300 ms (`ease`).
pub const MEDIA_CONTROLS_FADE: Duration = Duration::from_millis(300);
/// `.progressTrack`: the scrubber thickens under the pointer over 100 ms.
pub const MEDIA_SCRUBBER_EXPAND: Duration = Duration::from_millis(100);
/// `volumeHudFadeIn`: the volume display fades in over 150 ms.
pub const MEDIA_VOLUME_HUD_IN: Duration = Duration::from_millis(150);
/// `VolumePanel`: the volume slider stays 300 ms after the pointer leaves.
pub const MEDIA_VOLUME_PANEL_GRACE: Duration = Duration::from_millis(300);

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
    /// `transition-delay`: how long the move waits before it starts.
    delay: Duration,
}

impl Default for Fade {
    fn default() -> Self {
        Self {
            on: false,
            since: std::time::Instant::now(),
            from: 0.,
            duration: Duration::ZERO,
            delay: Duration::ZERO,
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
        self.set_after(on, duration, Duration::ZERO);
    }

    /// The same, starting only after `delay`, as a CSS `transition-delay`.
    pub fn set_after(&mut self, on: bool, duration: Duration, delay: Duration) {
        if self.on == on {
            return;
        }
        self.from = self.value();
        self.on = on;
        self.since = std::time::Instant::now();
        self.delay = delay;
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
        let elapsed = self.since.elapsed().saturating_sub(self.delay);
        self.from + (target - self.from) * curve.ease(progress(elapsed, self.duration))
    }

    pub fn on(&self) -> bool {
        self.on
    }

    pub fn animating(&self) -> bool {
        self.since.elapsed() < self.delay + self.duration
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

/// Where a rectangle of `w` × `h` is drawn at `scale` about `origin`, as CSS
/// `transform: scale()` with `transform-origin` places it: its left, top,
/// width and height, relative to the unscaled rectangle's top-left corner.
/// `origin` is a fraction of the width and height (0 the left or top edge,
/// 0.5 the centre, 1 the right or bottom edge).
///
/// GPUI cannot scale an element's contents, so a panel that grows or shrinks
/// draws its frame (surface, border and shadow) in this rectangle and keeps
/// its contents at full size, clipped to it.
pub fn scaled_frame(w: f32, h: f32, scale: f32, origin: (f32, f32)) -> (f32, f32, f32, f32) {
    (
        origin.0 * w * (1. - scale),
        origin.1 * h * (1. - scale),
        w * scale,
        h * scale,
    )
}

/// `menuIn` / `librarySwitcherIn`: the scale `elapsed` after opening, 0.96
/// growing to 1 over `--duration-menu-in` on `--ease-scale-in`.
pub fn menu_in(elapsed: Duration) -> f32 {
    MENU_IN_SCALE + (1. - MENU_IN_SCALE) * EASE_SCALE_IN.ease(progress(elapsed, MENU_IN))
}

/// `menuOut`: the scale and opacity `elapsed` after dismissal, 1 shrinking
/// to 0.95 and fading to 0 over 80 ms on `ease-in`.
pub fn menu_out(elapsed: Duration) -> (f32, f32) {
    let eased = EASE_IN.ease(progress(elapsed, MENU_OUT));
    (1. - (1. - MENU_OUT_SCALE) * eased, 1. - eased)
}

/// Mantine's `pop` at `shown` (0 hidden, 1 shown, already eased): the
/// tooltip's scale about its centre, and how far below its place it is.
/// `scale(s) translateY(t)` moves the centre by `s × t`.
pub fn tooltip_pop(shown: f32) -> (f32, f32) {
    let scale = TOOLTIP_POP_SCALE + (1. - TOOLTIP_POP_SCALE) * shown;
    (scale, scale * TOOLTIP_POP_RISE * (1. - shown))
}

/// `modalIn` / `modalOut` at `shown` (0 hidden, 1 shown, already eased): the
/// panel's scale about its centre and how far below its place it is
/// (`scale(0.97) translateY(4px)`, so the centre drops `0.97 × 4`).
pub fn modal_pop(shown: f32) -> (f32, f32) {
    let scale = MODAL_SCALE + (1. - MODAL_SCALE) * shown;
    (scale, scale * MODAL_DROP * (1. - shown))
}

/// How long the app's loop clock has run. Every looping animation (a
/// progress sweep, a status pulse) reads this one clock, so loops of the
/// same kind keep in step and none restarts when its view repaints.
pub fn loop_clock() -> Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

/// The subscription pulses' timing (`subscriptionStatusPulse`,
/// `subscriptionActivityPulse`): from 0 to 1 and back over
/// [`STATUS_PULSE`], each half on `ease-in-out`.
pub fn pulse(elapsed: Duration) -> f32 {
    let period = STATUS_PULSE.as_secs_f32();
    let phase = (elapsed.as_secs_f32() % period) / (period / 2.);
    EASE_IN_OUT.ease(if phase < 1. { phase } else { 2. - phase })
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
    fn a_scaled_frame_keeps_its_origin_in_place() {
        // About the top-left corner the corner stays and the far edges move in.
        assert_eq!(
            scaled_frame(200., 100., 0.96, (0., 0.)),
            (0., 0., 192., 96.)
        );
        // About the bottom-right corner that corner stays where it was.
        let (x, y, w, h) = scaled_frame(200., 100., 0.96, (1., 1.));
        assert!((x + w - 200.).abs() < 1e-4 && (y + h - 100.).abs() < 1e-4);
        assert!((x - 8.).abs() < 1e-4 && (y - 4.).abs() < 1e-4);
        // About the centre each side moves in by half the shrink.
        let (x, y, w, h) = scaled_frame(200., 100., 0.9, (0.5, 0.5));
        assert!((x - 10.).abs() < 1e-4 && (y - 5.).abs() < 1e-4);
        assert!((x + w / 2. - 100.).abs() < 1e-4 && (y + h / 2. - 50.).abs() < 1e-4);
        // At full scale nothing moves.
        assert_eq!(scaled_frame(200., 100., 1., (1., 0.)), (0., 0., 200., 100.));
    }

    #[test]
    fn a_menu_grows_in_then_shrinks_and_fades_out() {
        assert_eq!(menu_in(Duration::ZERO), MENU_IN_SCALE);
        assert_eq!(menu_in(MENU_IN), 1.);
        // `--ease-scale-in` covers most of the way at once.
        assert!(menu_in(Duration::from_millis(10)) > 0.98);
        assert_eq!(menu_out(Duration::ZERO), (1., 1.));
        assert_eq!(menu_out(MENU_OUT), (MENU_OUT_SCALE, 0.));
        // `ease-in` starts slowly: a quarter of the way, little has changed.
        let (scale, opacity) = menu_out(MENU_OUT / 4);
        assert!(opacity > 0.9 && scale > 0.995, "{scale} {opacity}");
    }

    #[test]
    fn a_tooltip_pops_up_from_below_and_a_modal_rises() {
        assert_eq!(tooltip_pop(1.), (1., 0.));
        let (scale, below) = tooltip_pop(0.);
        assert_eq!(scale, 0.9);
        // The 10px translation is scaled with the tooltip.
        assert!((below - 9.).abs() < 1e-5);
        assert_eq!(modal_pop(1.), (1., 0.));
        let (scale, below) = modal_pop(0.);
        assert_eq!(scale, 0.97);
        assert!((below - 3.88).abs() < 1e-5);
    }

    #[test]
    fn a_delayed_fade_waits_before_it_moves() {
        let mut fade = Fade::default();
        fade.set_after(true, Duration::from_secs(10), Duration::from_secs(10));
        // Still inside its delay: nothing has moved, but it counts as animating.
        assert_eq!(fade.value(), 0.);
        assert!(fade.animating());
    }

    #[test]
    fn progress_is_clamped() {
        let duration = Duration::from_millis(170);
        assert_eq!(progress(Duration::ZERO, duration), 0.);
        assert_eq!(progress(Duration::from_millis(85), duration), 0.5);
        assert_eq!(progress(Duration::from_secs(1), duration), 1.);
    }
}

/// `.inspectorResizeHandle`'s `transition-delay`: the edge waits this long
/// before going back once the pointer leaves it.
pub const RESIZE_EDGE_DELAY: Duration = Duration::from_millis(100);
