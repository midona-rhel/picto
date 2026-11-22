//! The animation of a scroll that is made with a mouse wheel.
//!
//! On Windows and Linux a mouse wheel reports how far it turned in lines
//! ([`ScrollDelta::Lines`]). A scroll container does not move its content by
//! the distance of such an event in one frame. The content moves to the new
//! position over a short time. An event that arrives during that time moves
//! the end position by its own distance, and the content goes on from the
//! position and at the speed it has at that moment. A device that reports
//! pixels, such as a trackpad, is not animated, and nothing is animated on
//! macOS ([`animates_scroll`]).
//!
//! The rules and the constants are the ones Chromium uses for a mouse wheel
//! on Windows and Linux. They were read from Chromium's source at commit
//! 6abf85f06b87956aee3143a87cf8f5e4e2457e89:
//!
//! - `cc/animation/scroll_offset_animation_curve_factory.cc` says that a
//!   mouse wheel gets an ease-in-out curve whose duration is "inverse delta".
//! - `cc/animation/scroll_offset_animation_curve.cc` has the duration
//!   (`EaseInOutSegmentDuration`), the position at a time (`GetValue`), the
//!   speed at a time (`CalculateVelocity`) and the change of the end position
//!   of a running animation (`UpdateTarget`, `VelocityBasedDurationBound`,
//!   `EaseInOutWithInitialSlope`).
//! - `ui/gfx/geometry/cubic_bezier.cc` has the curve's arithmetic.
//! - `cc/trees/layer_tree_host_impl.cc` (`ScrollAnimationCreate`) and
//!   `cc/animation/scroll_offset_animations_impl.cc`
//!   (`ScrollAnimationUpdateTarget`) say that the end position never lies
//!   beyond the end of the content, and that a new event adds its distance to
//!   the end position of the running animation.
//! - `cc/input/input_handler.cc` says that an event with a distance in
//!   precise pixels is not animated, and that the distances are in device
//!   pixels.
//!
//! This differs from Chromium in one way. Chromium draws the first frame of an
//! animation at the position the curve has after one frame interval. Here the
//! first frame is drawn at the position the curve has for the time that has
//! passed since the wheel event, which is less than one frame interval.
use crate::{EntityId, Pixels, Point, ScrollDelta, Window, point, px};
use scheduler::Instant;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

/// Chromium writes the durations of a scroll animation in sixtieths of a
/// second (`kDurationDivisor`).
const DURATION_DIVISOR: f64 = 60.;
/// The animation of a distance up to this many device pixels lasts
/// [`LONGEST_DURATION`] (`kInverseDeltaRampStartPx`).
const RAMP_START: f64 = 120.;
/// The animation of a distance from this many device pixels lasts
/// [`SHORTEST_DURATION`] (`kInverseDeltaRampEndPx`). Between [`RAMP_START`]
/// and this distance the duration falls in a straight line.
const RAMP_END: f64 = 480.;
/// The shortest duration of an animation, in sixtieths of a second: 100 ms
/// (`kInverseDeltaMinDuration`).
const SHORTEST_DURATION: f64 = 6.;
/// The longest duration of an animation, in sixtieths of a second: 200 ms
/// (`kInverseDeltaMaxDuration`).
const LONGEST_DURATION: f64 = 12.;
/// The x of the first control point of the CSS `ease-in-out` curve,
/// `cubic-bezier(0.42, 0, 0.58, 1)` (`kEaseInOutControlPoints`). Its y is 0
/// for an animation that starts at rest. A curve that continues a running
/// animation has another y ([`Easing::starting_at_slope`]).
const EASE_X1: f64 = 0.42;
/// The x of the second control point of the `ease-in-out` curve. Its y is 1.
const EASE_X2: f64 = 0.58;
/// The steepest slope a curve may start with (`EaseInOutWithInitialSlope`).
const STEEPEST_START: f64 = 1000.;
/// A distance in pixels, or a duration in seconds, below this counts as
/// nothing (`kEpsilon` in `scroll_offset_animation_curve.cc`).
const EPSILON: f64 = 0.01;
/// How many times longer than the time the content would need at its present
/// speed an animation to a new end position may last
/// (`VelocityBasedDurationBound`).
const SPEED_BOUND: f64 = 2.5;
/// A wheel event that asks for no more than this many pixels along each axis
/// moves the content at once (`kEpsilon` in `ScrollAnimationCreate`).
const SMALLEST_ANIMATED_DISTANCE: f32 = 0.1;

/// Returns whether a scroll container animates a scroll event with this
/// distance: on Windows and Linux when the distance is in lines, which is
/// what a mouse wheel reports. On macOS no scroll event is animated.
pub(crate) fn animates_scroll(delta: &ScrollDelta) -> bool {
    !cfg!(target_os = "macos") && matches!(delta, ScrollDelta::Lines(_))
}

/// Returns the component of `delta` that is further from 0, with its sign.
/// Chromium measures the distance of a scroll along that axis only
/// (`MaximumDimension`).
fn longer_axis(delta: Point<Pixels>) -> f64 {
    let (x, y) = (f32::from(delta.x) as f64, f32::from(delta.y) as f64);
    if x.abs() > y.abs() { x } else { y }
}

/// Returns how many seconds the animation of `distance` device pixels lasts:
/// 200 ms up to 120 pixels, 100 ms from 480 pixels, and in between a time
/// that falls in a straight line.
fn duration_for(distance: f64) -> f64 {
    let slope = (SHORTEST_DURATION - LONGEST_DURATION) / (RAMP_END - RAMP_START);
    let at_zero = LONGEST_DURATION - RAMP_START * slope;
    (at_zero + distance.abs() * slope).clamp(SHORTEST_DURATION, LONGEST_DURATION) / DURATION_DIVISOR
}

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function, held as the
/// coefficients of its two polynomials in the curve's parameter.
#[derive(Clone, Copy, Debug)]
struct Easing {
    ax: f64,
    bx: f64,
    cx: f64,
    ay: f64,
    by: f64,
    cy: f64,
}

impl Easing {
    fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        let (cx, cy) = (3. * x1, 3. * y1);
        let (bx, by) = (3. * (x2 - x1) - cx, 3. * (y2 - y1) - cy);
        Self {
            ax: 1. - cx - bx,
            bx,
            cx,
            ay: 1. - cy - by,
            by,
            cy,
        }
    }

    /// The CSS `ease-in-out` curve, which starts and ends at rest.
    fn ease_in_out() -> Self {
        Self::new(EASE_X1, 0., EASE_X2, 1.)
    }

    /// The `ease-in-out` curve with its first control point moved so that
    /// the curve starts with the slope `slope` and not at rest.
    fn starting_at_slope(slope: f64) -> Self {
        let slope = slope.clamp(-STEEPEST_START, STEEPEST_START);
        Self::new(EASE_X1, EASE_X1 * slope, EASE_X2, 1.)
    }

    fn x(&self, parameter: f64) -> f64 {
        ((self.ax * parameter + self.bx) * parameter + self.cx) * parameter
    }

    fn y(&self, parameter: f64) -> f64 {
        ((self.ay * parameter + self.by) * parameter + self.cy) * parameter
    }

    fn x_derivative(&self, parameter: f64) -> f64 {
        (3. * self.ax * parameter + 2. * self.bx) * parameter + self.cx
    }

    fn y_derivative(&self, parameter: f64) -> f64 {
        (3. * self.ay * parameter + 2. * self.by) * parameter + self.cy
    }

    /// Returns the parameter at which the curve's x is `x`, for an `x` from 0
    /// to 1. The curve's x rises with the parameter everywhere, because both
    /// control points have an x between 0 and 1. Newton's method finds the
    /// parameter in a few steps; halving the interval is the fallback.
    fn parameter_at(&self, x: f64) -> f64 {
        const PRECISION: f64 = 1e-7;
        let mut parameter = x;
        for _ in 0..8 {
            let error = self.x(parameter) - x;
            if error.abs() < PRECISION {
                return parameter;
            }
            let derivative = self.x_derivative(parameter);
            if derivative.abs() < PRECISION {
                break;
            }
            parameter -= error / derivative;
        }
        let (mut low, mut high) = (0., 1.);
        parameter = x;
        for _ in 0..64 {
            let value = self.x(parameter);
            if (value - x).abs() < PRECISION {
                break;
            }
            if x > value {
                low = parameter;
            } else {
                high = parameter;
            }
            parameter = (low + high) / 2.;
        }
        parameter
    }

    /// Returns the share of the distance that is covered when the share `x`
    /// of the time has passed.
    fn value(&self, x: f64) -> f64 {
        self.y(self.parameter_at(x.clamp(0., 1.)))
    }

    /// Returns the curve's slope where the share `x` of the time has passed.
    fn slope(&self, x: f64) -> f64 {
        let parameter = self.parameter_at(x.clamp(0., 1.));
        let (dx, dy) = (self.x_derivative(parameter), self.y_derivative(parameter));
        if dx == 0. && dy == 0. { 0. } else { dy / dx }
    }
}

/// Where the content is at each moment of one animation. Times are in seconds
/// since the wheel event that started the animation. The animation consists
/// of one segment at a time: from `initial` at `segment_start` to `target` at
/// `end`. A wheel event during the animation replaces the segment
/// ([`Curve::retarget`]).
#[derive(Clone, Copy, Debug)]
struct Curve {
    initial: Point<Pixels>,
    target: Point<Pixels>,
    easing: Easing,
    segment_start: f64,
    end: f64,
}

impl Curve {
    /// A curve from `initial` at time 0 to `target`. `scale_factor` is the
    /// number of device pixels in one pixel of the offsets.
    fn new(initial: Point<Pixels>, target: Point<Pixels>, scale_factor: f32) -> Self {
        Self {
            initial,
            target,
            easing: Easing::ease_in_out(),
            segment_start: 0.,
            end: duration_for(longer_axis(target - initial) * scale_factor as f64),
        }
    }

    /// Returns the position at the time `time`.
    fn value(&self, time: f64) -> Point<Pixels> {
        let duration = self.end - self.segment_start;
        let time = time - self.segment_start;
        if duration <= 0. || time >= duration {
            return self.target;
        }
        if time <= 0. {
            return self.initial;
        }
        let progress = self.easing.value(time / duration) as f32;
        self.initial + (self.target - self.initial) * progress
    }

    /// Returns the speed at the time `time`, in pixels per second along the
    /// axis of the longer distance.
    fn velocity(&self, time: f64) -> f64 {
        let duration = self.end - self.segment_start;
        let slope = self.easing.slope((time - self.segment_start) / duration);
        slope * (longer_axis(self.target - self.initial) / duration)
    }

    /// Makes the animation end at `new_target`. `time` is the time of the
    /// frame that was drawn last, and it is before `end`. The new segment
    /// starts at the position and with the speed the content has at `time`,
    /// so the movement goes on without a jump and without a change of speed.
    fn retarget(&mut self, time: f64, new_target: Point<Pixels>, scale_factor: f32) {
        let time = time.max(self.segment_start);
        // The end position has not moved, which happens at the end of the
        // content. The animation goes on as it is. Starting a new segment
        // would make it last longer.
        if longer_axis(self.target - new_target).abs() < EPSILON {
            self.target = new_target;
            return;
        }
        let position = self.value(time);
        let distance = longer_axis(new_target - position);
        // The content is already at the new end position, so the animation
        // ends.
        if distance.abs() < EPSILON {
            self.segment_start = time;
            self.end = time;
            self.target = new_target;
            return;
        }
        let velocity = self.velocity(time);
        // The time the content would need for the distance at its present
        // speed, times `SPEED_BOUND`, limits the duration. Without this
        // limit, fast content with a short distance left would run past the
        // end position and come back. There is no such limit when the
        // content is at rest or moves away from the new end position.
        let time_at_speed = distance / velocity * SPEED_BOUND;
        let bound = if velocity.abs() < EPSILON || time_at_speed < 0. {
            f64::INFINITY
        } else {
            time_at_speed
        };
        let duration = duration_for(distance * scale_factor as f64).min(bound);
        // The new segment would last no time, so the content moves to the
        // new end position in the next frame.
        if duration < EPSILON {
            self.target = new_target;
            self.end = time;
            return;
        }
        self.easing = Easing::starting_at_slope(velocity * (duration / distance));
        self.initial = position;
        self.target = new_target;
        self.segment_start = time;
        self.end = time + duration;
    }
}

/// An animation that is running.
#[derive(Clone, Copy, Debug)]
struct Running {
    curve: Curve,
    /// The time of the wheel event that started the animation. The curve's
    /// times are measured from it.
    started: Instant,
    /// The offset the animation gave the content in the frame that was drawn
    /// last, and the curve's time for that frame. Before the animation's
    /// first frame these are the offset the animation started from and 0.
    shown: Point<Pixels>,
    shown_at: f64,
}

/// The state of the animated wheel scroll of one scroll container.
///
/// The container's scroll offset is the distance from the top left of the
/// container to the top left of its content, so it becomes more negative as
/// the content is scrolled further. [`WheelScroll::scroll_by`] starts an
/// animation or moves the end position of the running one, and
/// [`WheelScroll::advance`] moves the offset once in each frame.
///
/// The animation ends by itself when the offset it reads is not the offset
/// it wrote last, because then something else has moved the content: the
/// application set the offset, a trackpad scrolled, or the content became
/// shorter. The animation does not write the offset in that case.
#[derive(Debug, Default)]
pub struct WheelScroll {
    /// How far the content can be scrolled along each axis, as a positive
    /// distance. The end position of an animation stays within it.
    max: Point<Pixels>,
    running: Option<Running>,
    /// Whether [`step_each_frame`] has a call of `advance` waiting for the
    /// next frame.
    stepping: bool,
}

impl WheelScroll {
    /// Sets how far the content can be scrolled along each axis, as a
    /// positive distance. A scroll container calls this in each frame.
    pub fn set_max(&mut self, max: Point<Pixels>) {
        self.max = max;
    }

    /// Handles a wheel event that asks to move the content by `delta`, at the
    /// time `now`. `offset` is the container's present scroll offset, and
    /// `scale_factor` is the number of device pixels in one pixel.
    ///
    /// If an animation is running, its end position moves by `delta`. If
    /// none is running, one starts from `offset`. In both cases the end
    /// position stays within the content. Returns whether an animation is
    /// running afterwards, which means that `advance` has to be called in
    /// each frame. It returns false when the content cannot move in that
    /// direction, and when `delta` is so small that the offset was changed
    /// at once.
    pub fn scroll_by(
        &mut self,
        offset: &mut Point<Pixels>,
        delta: Point<Pixels>,
        scale_factor: f32,
        now: Instant,
    ) -> bool {
        let max = self.max;
        let within_content = |position: Point<Pixels>| {
            point(
                position.x.clamp(-max.x, px(0.)),
                position.y.clamp(-max.y, px(0.)),
            )
        };
        if let Some(running) = self
            .running
            .as_mut()
            .filter(|running| running.shown == *offset && running.shown_at < running.curve.end)
        {
            let target = within_content(running.curve.target + delta);
            running
                .curve
                .retarget(running.shown_at, target, scale_factor);
            return true;
        }
        self.running = None;
        let target = within_content(*offset + delta);
        if delta.x.abs() <= px(SMALLEST_ANIMATED_DISTANCE)
            && delta.y.abs() <= px(SMALLEST_ANIMATED_DISTANCE)
        {
            *offset = target;
            return false;
        }
        if target == *offset {
            return false;
        }
        self.running = Some(Running {
            curve: Curve::new(*offset, target, scale_factor),
            started: now,
            shown: *offset,
            shown_at: 0.,
        });
        true
    }

    /// Moves `offset` to where the running animation has the content at the
    /// time `now`. Returns whether the animation is still running afterwards.
    ///
    /// When `offset` is not the offset this function wrote last, something
    /// else has moved the content. The animation then ends and `offset` is
    /// left as it is.
    pub fn advance(&mut self, offset: &mut Point<Pixels>, now: Instant) -> bool {
        let Some(mut running) = self.running.take() else {
            return false;
        };
        if running.shown != *offset {
            return false;
        }
        let time = now.duration_since(running.started).as_secs_f64();
        *offset = running.curve.value(time);
        if time >= running.curve.end {
            return false;
        }
        running.shown = *offset;
        running.shown_at = time;
        self.running = Some(running);
        true
    }
}

/// Calls [`WheelScroll::advance`] for `offset` in each frame, from the next
/// one until the animation ends, and asks for the view `view` to be drawn
/// again each time the offset changed. Does nothing when such calls are
/// already scheduled for `wheel`. The calls also end when the scroll
/// container that owns `offset` and `wheel` no longer exists.
pub(crate) fn step_each_frame(
    offset: &Rc<RefCell<Point<Pixels>>>,
    wheel: &Rc<RefCell<WheelScroll>>,
    view: EntityId,
    window: &Window,
) {
    if std::mem::replace(&mut wheel.borrow_mut().stepping, true) {
        return;
    }
    step_in_next_frame(Rc::downgrade(offset), Rc::downgrade(wheel), view, window);
}

/// Schedules one call of [`WheelScroll::advance`] for the start of the next
/// frame, before the frame is drawn. After the call it schedules itself again
/// if the animation is still running.
fn step_in_next_frame(
    offset: Weak<RefCell<Point<Pixels>>>,
    wheel: Weak<RefCell<WheelScroll>>,
    view: EntityId,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let (Some(shared_offset), Some(shared_wheel)) = (offset.upgrade(), wheel.upgrade()) else {
            return;
        };
        let before = *shared_offset.borrow();
        let running = shared_wheel.borrow_mut().advance(
            &mut shared_offset.borrow_mut(),
            cx.background_executor().now(),
        );
        shared_wheel.borrow_mut().stepping = running;
        if *shared_offset.borrow() != before {
            cx.notify(view);
        }
        if running {
            step_in_next_frame(offset, wheel, view, window);
        }
    });
}
