//! Stops the rest of a scroll gesture from reaching a view that has replaced
//! the view the gesture started on.
//!
//! After a flick on a trackpad the system keeps sending scroll events
//! (momentum) for a second or more, to whatever is under the pointer. When the
//! view under the pointer is replaced (for example by the next item after an
//! Inbox review), these events would pan or zoom the new view. So after a call
//! to [`suppress`], scroll events that keep arriving without a pause, in the
//! same direction and not clearly stronger, are taken to belong to the old
//! gesture and are swallowed: they are stopped before any view receives them.
//! This ends at a pause of 90 ms, at a clearly stronger event or an event in
//! the opposite direction (either one is a new gesture), or after 1.2 seconds.
//! The rule reads only the deltas and their timing, so it is the same for the
//! wheel and the trackpad of every platform.
use gpui::*;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// When no scroll event has arrived for this long, the old gesture has ended.
const QUIET: Duration = Duration::from_millis(90);
/// Scroll events are never swallowed for longer than this.
const LIMIT: Duration = Duration::from_millis(1_200);
/// An event that is more than this many times as strong as the last one starts
/// a new gesture.
const NEW_GESTURE_RATIO: f32 = 1.6;
/// The number of pixels that one line of a wheel's delta counts as, so that
/// its strength can be compared with a trackpad's delta in pixels.
const LINE: f32 = 16.;

/// The state while scroll events are being swallowed.
#[derive(Clone, Copy)]
struct Swallow {
    started: Instant,
    /// The time of the last event that was swallowed, or of the call that
    /// started the swallowing.
    last_at: Instant,
    last_magnitude: f32,
    direction: i8,
}

/// Holds the last scroll event and, while scroll events are being swallowed,
/// the state of that.
#[derive(Default)]
pub struct Momentum {
    /// The last scroll event: its time, its strength and its direction. With
    /// it, the first event after a call to `suppress` is compared with the old
    /// gesture, and is not simply assumed to belong to it.
    last: Option<(Instant, f32, i8)>,
    swallow: Option<Swallow>,
}

/// Returns the direction of a delta along its main axis: -1, 0 or 1.
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
    /// Starts swallowing what is left of the scroll gesture that is in
    /// progress at `now`. Does nothing when no gesture is in progress.
    pub fn suppress(&mut self, now: Instant) {
        self.swallow = None;
        let Some((at, magnitude, direction)) = self.last else {
            return;
        };
        // When no scroll event arrived within the last `QUIET`, no gesture is
        // in progress and there is nothing to swallow.
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

    /// Records a scroll event of `dx`, `dy` pixels at `now`, and returns
    /// whether it is swallowed.
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

/// Starts swallowing the rest of the scroll gesture that is in progress; see
/// the module's description.
pub fn suppress() {
    MOMENTUM.with(|momentum| momentum.borrow_mut().suppress(Instant::now()));
}

/// Returns an element that reads the window's scroll events before any view
/// receives them, and stops the ones that are swallowed after a call to
/// [`suppress`]. A window draws it once, before its content.
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
        // The rest of the gesture: weaker events in the same direction.
        assert!(momentum.scrolled(0., -30., at(start, 32)));
        assert!(momentum.scrolled(0., -20., at(start, 48)));
        // A pause ends the swallowing. The event after it belongs to a new
        // gesture.
        assert!(!momentum.scrolled(0., -20., at(start, 200)));
    }

    #[test]
    fn a_new_gesture_is_not_swallowed() {
        let start = Instant::now();
        let mut momentum = Momentum::default();
        momentum.scrolled(0., -40., at(start, 0));
        momentum.suppress(at(start, 16));
        // A clearly stronger event is a new gesture.
        assert!(!momentum.scrolled(0., -80., at(start, 32)));
        let mut reversed = Momentum::default();
        reversed.scrolled(0., -40., at(start, 0));
        reversed.suppress(at(start, 16));
        assert!(!reversed.scrolled(0., 10., at(start, 32)));
        // No gesture was in progress when the view changed.
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
