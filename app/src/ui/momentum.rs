//! Swallows the tail of a scroll gesture that started on a view now replaced
//! (`scrollMomentum.ts`).
//!
//! After a trackpad flick the system keeps sending scroll events (momentum)
//! for a second or more, aimed at whatever is under the pointer. When the
//! view changes underneath (the next item after an Inbox review), that
//! momentum would pan or zoom the new view. As production does, scroll input
//! that keeps arriving without a pause, in the same direction and no
//! stronger, is taken for the old gesture and swallowed until a short pause,
//! a clearly stronger push or a reversal (a new gesture), or a time limit.
//! The rule reads only the deltas and their timing, so it holds the same
//! for every platform's wheel and trackpad.
use gpui::*;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// No scroll input for this long ends the old gesture.
const QUIET: Duration = Duration::from_millis(90);
/// Input is never swallowed for longer than this.
const LIMIT: Duration = Duration::from_millis(1_200);
/// A push this much stronger than the last event is a new gesture.
const NEW_GESTURE_RATIO: f32 = 1.6;
/// What a line of a wheel's delta counts for, so that its strength compares
/// with a trackpad's pixels.
const LINE: f32 = 16.;

/// The swallowing under way.
#[derive(Clone, Copy)]
struct Swallow {
    started: Instant,
    /// The last event swallowed, or the call that started it.
    last_at: Instant,
    last_magnitude: f32,
    direction: i8,
}

/// The scroll input seen, and the swallowing it is under.
#[derive(Default)]
pub struct Momentum {
    /// The last scroll event: when, how strong and which way, so that the
    /// first event after a switch is judged against the old gesture rather
    /// than assumed to be part of it.
    last: Option<(Instant, f32, i8)>,
    swallow: Option<Swallow>,
}

/// Which way a delta moves along its main axis: -1, 0 or 1.
fn direction(dx: f32, dy: f32) -> i8 {
    let main = if dy.abs() >= dx.abs() { dy } else { dx };
    if main > 0. {
        1
    } else if main < 0. {
        -1
    } else {
        0
    }
}

impl Momentum {
    /// `suppressScrollMomentum`: from now on, swallow what is left of the
    /// gesture under way, if one is.
    pub fn suppress(&mut self, now: Instant) {
        self.swallow = None;
        let Some((at, magnitude, direction)) = self.last else {
            return;
        };
        // Without recent scroll input there is no gesture to swallow.
        if now.duration_since(at) > QUIET {
            return;
        }
        self.swallow = Some(Swallow {
            started: now,
            last_at: now,
            last_magnitude: magnitude,
            direction,
        });
    }

    /// A scroll event of `dx`, `dy` pixels: whether it is swallowed.
    pub fn scrolled(&mut self, dx: f32, dy: f32, now: Instant) -> bool {
        let magnitude = dx.abs() + dy.abs();
        let way = direction(dx, dy);
        self.last = Some((now, magnitude, way));
        let Some(swallow) = &mut self.swallow else {
            return false;
        };
        let reversed = swallow.direction != 0 && way == -swallow.direction;
        if now.duration_since(swallow.started) > LIMIT
            || now.duration_since(swallow.last_at) > QUIET
            || reversed
            || magnitude > swallow.last_magnitude * NEW_GESTURE_RATIO
        {
            self.swallow = None;
            return false;
        }
        swallow.last_magnitude = magnitude;
        swallow.last_at = now;
        true
    }
}

thread_local! {
    static MOMENTUM: RefCell<Momentum> = RefCell::default();
}

/// Swallows the rest of the scroll gesture under way (see the module).
pub fn suppress() {
    MOMENTUM.with(|momentum| momentum.borrow_mut().suppress(Instant::now()));
}

/// Watches the window's scroll input, before any view sees it, and stops
/// what [`suppress`] swallows. Drawn once in a window, before its content.
pub fn watcher() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |_, _, window, _| {
            window.on_mouse_event(|event: &ScrollEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let delta = event.delta.pixel_delta(px(LINE));
                let swallowed = MOMENTUM.with(|momentum| {
                    momentum.borrow_mut().scrolled(
                        f32::from(delta.x),
                        f32::from(delta.y),
                        Instant::now(),
                    )
                });
                if swallowed {
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .size_0()
}

#[cfg(test)]
mod tests {
    use super::Momentum;
    use std::time::{Duration, Instant};

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn the_rest_of_a_flick_is_swallowed_until_it_pauses() {
        let start = Instant::now();
        let mut momentum = Momentum::default();
        assert!(!momentum.scrolled(0., -40., at(start, 0)));
        momentum.suppress(at(start, 16));
        // The tail, weakening, in the same direction.
        assert!(momentum.scrolled(0., -30., at(start, 32)));
        assert!(momentum.scrolled(0., -20., at(start, 48)));
        // A pause ends it; what follows is a new gesture.
        assert!(!momentum.scrolled(0., -20., at(start, 200)));
    }

    #[test]
    fn a_new_gesture_is_not_swallowed() {
        let start = Instant::now();
        let mut momentum = Momentum::default();
        momentum.scrolled(0., -40., at(start, 0));
        momentum.suppress(at(start, 16));
        // A clearly stronger push.
        assert!(!momentum.scrolled(0., -80., at(start, 32)));
        let mut reversed = Momentum::default();
        reversed.scrolled(0., -40., at(start, 0));
        reversed.suppress(at(start, 16));
        assert!(!reversed.scrolled(0., 10., at(start, 32)));
        // No gesture under way when the view changed.
        let mut idle = Momentum::default();
        idle.scrolled(0., -40., at(start, 0));
        idle.suppress(at(start, 500));
        assert!(!idle.scrolled(0., -10., at(start, 510)));
    }

    #[test]
    fn swallowing_stops_at_the_limit() {
        let start = Instant::now();
        let mut momentum = Momentum::default();
        momentum.scrolled(0., -40., at(start, 0));
        momentum.suppress(at(start, 10));
        let mut now = 10;
        while now < 1_200 {
            now += 50;
            assert!(momentum.scrolled(0., -10., at(start, now)), "{now}");
        }
        assert!(!momentum.scrolled(0., -10., at(start, now + 50)));
    }
}
