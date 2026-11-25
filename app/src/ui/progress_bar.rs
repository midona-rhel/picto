//! The progress bar: a thin bar in the subtle overlay colour. When the
//! progress is known, the bar is filled to the share that is done. When it is
//! not known, a segment in the primary colour, 35% of the bar's width, moves
//! across the bar every 900 ms. A view that needs a different bar (for the
//! import task, the download of a model, the run of a subscription) sets its
//! own colours, corner radius and segment.
use crate::{motion, theme};
use gpui::*;
use std::time::{Duration, Instant};

/// The moving segment of a bar of unknown progress. `segment` is its width as
/// a share of the track. `period` is the time of one crossing. `travel` is how
/// far it moves during one crossing, in multiples of its own width. Its left
/// edge starts 1.1 of its own width to the left of the track's left end, and
/// it moves along `motion::EASE_IN_OUT`.
#[derive(Clone, Copy)]
pub struct Sweep {
    pub segment: f32,
    pub period: Duration,
    pub travel: f32,
}

/// The usual segment: 35% of the track wide. Every 900 ms it moves by 4.2
/// times its own width, from just outside the track's left end to past its
/// right end.
pub const SWEEP: Sweep = Sweep {
    segment: 0.35,
    period: motion::PROGRESS_SWEEP,
    travel: 4.2,
};

impl Sweep {
    /// Returns where the segment's left edge is after `elapsed`, as a share of
    /// the track's width. The movement repeats every `period`.
    fn offset(&self, elapsed: Duration) -> f32 {
        let along = (elapsed.as_secs_f32() % self.period.as_secs_f32()) / self.period.as_secs_f32();
        self.segment * (-1.1 + self.travel * motion::EASE_IN_OUT.ease(along))
    }
}

/// Returns the share that `done` is of `total`, from 0 to 1. Returns 0 while
/// the total is not known, which is when it is 0 or less.
pub fn share(done: i64, total: i64) -> f32 {
    if total > 0 {
        (done as f32 / total as f32).clamp(0., 1.)
    } else {
        0.
    }
}

#[derive(IntoElement)]
pub struct ProgressBar {
    height: f32,
    /// The share that is done, when it is known, and the id under which the
    /// state of the fill's animation is kept.
    filled: Option<(ElementId, f32)>,
    /// The corner radius of the track and of the fill. Some callers, such as
    /// the import task view, set another radius, and their own colours in
    /// `track` and `fill`.
    radius: f32,
    track: Option<Rgba>,
    fill: Option<Rgba>,
    sweep: Sweep,
}

/// Returns a bar of unknown progress. A call to `filled` makes it a bar of
/// known progress.
pub fn progress_bar() -> ProgressBar {
    // The bar is 3px high unless the caller sets another height.
    ProgressBar {
        height: 3.,
        filled: None,
        radius: 2.,
        track: None,
        fill: None,
        sweep: SWEEP,
    }
}

/// The animation of a fill's width: the share it starts from, the share it
/// ends at, and the time it started.
#[derive(Clone, Copy)]
struct Fill {
    from: f32,
    to: f32,
    since: Instant,
}

impl Fill {
    fn at(&self, now: Instant) -> f32 {
        let done =
            motion::EASE_DEFAULT.ease(motion::progress(now - self.since, motion::PROGRESS_FILL));
        self.from + (self.to - self.from) * done
    }
}

impl ProgressBar {
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    /// Sets the corner radius of the track and of the fill.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }
    /// Sets the track's colour, in place of the subtle overlay.
    pub fn track(mut self, color: Rgba) -> Self {
        self.track = Some(color);
        self
    }
    /// Sets the fill's colour, in place of the primary colour.
    pub fn fill_color(mut self, color: Rgba) -> Self {
        self.fill = Some(color);
        self
    }
    /// Sets the width, the period and the travel of the moving segment.
    pub fn sweep(mut self, sweep: Sweep) -> Self {
        self.sweep = sweep;
        self
    }
    /// Makes this a bar of known progress that is filled to `share`. When the
    /// share changes, the fill's width moves to the new share over
    /// `motion::PROGRESS_FILL`. `id` identifies the bar between frames.
    pub fn filled(mut self, id: impl Into<ElementId>, share: f32) -> Self {
        self.filled = Some((id.into(), share.clamp(0., 1.)));
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // Assistive technology is told that this is a progress bar, and the
        // percentage that is done. It is told no value while the progress is
        // not known.
        let (id, percent) = match &self.filled {
            Some((id, share)) => (
                ElementId::NamedChild(std::sync::Arc::new(id.clone()), "progress".into()),
                Some((*share as f64 * 100.).round()),
            ),
            None => ("progress".into(), None),
        };
        let mut root = div().id(id).role(Role::ProgressIndicator);
        if let Some(percent) = percent {
            root = root
                .aria_numeric_value(percent)
                .aria_min_numeric_value(0.)
                .aria_max_numeric_value(100.);
        }
        let fill = match self.filled {
            Some((id, share)) => {
                let now = Instant::now();
                let state = window.use_keyed_state(id, cx, |_, _| Fill {
                    from: share,
                    to: share,
                    since: now,
                });
                let mut fill = *state.read(cx);
                if (fill.to - share).abs() > f32::EPSILON {
                    fill = Fill {
                        from: fill.at(now),
                        to: share,
                        since: now,
                    };
                    state.update(cx, |stored, _| *stored = fill);
                }
                if now - fill.since < motion::PROGRESS_FILL {
                    window.request_animation_frame();
                }
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(relative(fill.at(now)))
                    .rounded(px(self.radius))
                    .bg(self.fill.unwrap_or_else(|| theme.primary()))
            }
            None => {
                motion::loop_frame(window, cx);
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(relative(self.sweep.offset(motion::loop_clock())))
                    .w(relative(self.sweep.segment))
                    .rounded(px(self.radius))
                    .bg(self.fill.unwrap_or_else(|| theme.primary()))
            }
        };
        root.relative()
            .w_full()
            .h(px(self.height))
            .flex_none()
            .rounded(px(self.radius))
            .overflow_hidden()
            .bg(self.track.unwrap_or_else(|| theme.subtle_overlay()))
            .child(fill)
    }
}

#[cfg(test)]
mod tests {
    use super::{SWEEP, share};
    use std::time::Duration;

    #[test]
    fn the_segment_sweeps_from_before_the_track_to_past_it() {
        let segment = SWEEP.segment;
        assert!((SWEEP.offset(Duration::ZERO) - segment * -1.1).abs() < 1e-4);
        // Halfway through a crossing, `EASE_IN_OUT` is halfway too.
        assert!((SWEEP.offset(Duration::from_millis(450)) - segment * 1.).abs() < 1e-3);
        // After a full period the segment starts again from the left.
        assert!((SWEEP.offset(Duration::from_millis(900)) - segment * -1.1).abs() < 1e-4);
        assert!(SWEEP.offset(Duration::from_millis(899)) > 1.);
    }

    #[test]
    fn a_known_total_fills_its_share_and_an_unknown_one_nothing() {
        assert_eq!(share(5, 20), 0.25);
        assert_eq!(share(30, 20), 1.);
        assert_eq!(share(3, 0), 0.);
    }
}
