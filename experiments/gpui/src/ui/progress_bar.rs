//! `ProgressBar.tsx`: a thin bar on the subtle overlay, filled to the share
//! of done over total, or, with `indeterminate`, swept by a 35% primary
//! segment every 900 ms. Views that restyle a progress track (the import
//! task's, a model's download, a subscription's run) give it their colours,
//! corners and sweep.
use crate::{motion, theme};
use gpui::*;
use std::time::{Duration, Instant};

/// The indeterminate segment: its width as a share of the track, one
/// sweep's time, and how far it travels (`translateX(-110%)` to
/// `translateX(-110% + travel)` of its own width, on `ease-in-out`).
#[derive(Clone, Copy)]
pub struct Sweep {
    pub segment: f32,
    pub period: Duration,
    pub travel: f32,
}

/// `.indeterminate { width: 35% }`, to `translateX(310%)` every 900 ms.
pub const SWEEP: Sweep = Sweep {
    segment: 0.35,
    period: motion::PROGRESS_SWEEP,
    travel: 4.2,
};

impl Sweep {
    /// Where the segment's left edge is, as a share of the track, over and
    /// over.
    fn offset(&self, elapsed: Duration) -> f32 {
        let along = (elapsed.as_secs_f32() % self.period.as_secs_f32()) / self.period.as_secs_f32();
        self.segment * (-1.1 + self.travel * motion::EASE_IN_OUT.ease(along))
    }
}

/// The share `done / total` fills, from 0 to 1; nothing while the total is
/// not known.
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
    /// A known share, and the id its fill's movement is kept under.
    filled: Option<(ElementId, f32)>,
    /// A rounder track and its own colour, where a caller restyles
    /// `[role='progressbar']` (the cloud bar and the progress dialog).
    radius: f32,
    track: Option<Rgba>,
    fill: Option<Rgba>,
    sweep: Sweep,
}

/// A bar of unknown progress (`<ProgressBar indeterminate />`).
pub fn progress_bar() -> ProgressBar {
    // `height = 3` by default.
    ProgressBar {
        height: 3.,
        filled: None,
        radius: 2.,
        track: None,
        fill: None,
        sweep: SWEEP,
    }
}

/// Where a fill is moving from and to, and since when.
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
    /// The track's and the fill's corners.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }
    /// The track's colour instead of the subtle overlay.
    pub fn track(mut self, color: Rgba) -> Self {
        self.track = Some(color);
        self
    }
    /// The fill's colour instead of the primary one.
    pub fn fill_color(mut self, color: Rgba) -> Self {
        self.fill = Some(color);
        self
    }
    /// The indeterminate segment's own size and timing.
    pub fn sweep(mut self, sweep: Sweep) -> Self {
        self.sweep = sweep;
        self
    }
    /// A bar of known progress (`done` and `total`), filled to `share`; a
    /// change of share slides the fill there.
    pub fn filled(mut self, id: impl Into<ElementId>, share: f32) -> Self {
        self.filled = Some((id.into(), share.clamp(0., 1.)));
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // `role="progressbar"`: its percent, or none while indeterminate.
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
        // Halfway through a sweep, ease-in-out is halfway too.
        assert!((SWEEP.offset(Duration::from_millis(450)) - segment * 1.).abs() < 1e-3);
        // It starts again from the left.
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
