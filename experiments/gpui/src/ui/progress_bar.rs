//! `ProgressBar.tsx`: a thin bar on the subtle overlay, filled to the share
//! of done over total, or, with `indeterminate`, swept by a 35% primary
//! segment every 900 ms.
use crate::{motion, theme};
use gpui::*;
use std::time::{Duration, Instant};

/// `.fill { transition: width 250ms var(--ease-default) }`.
const FILL_MOVE: Duration = Duration::from_millis(250);

/// `.indeterminate { width: 35% }`.
const SEGMENT: f32 = 0.35;

/// Where the indeterminate segment's left edge is, as a share of the track:
/// `translateX(-110%)` to `translateX(310%)` of its own width, on
/// `ease-in-out`, over and over.
fn sweep_offset(elapsed: Duration) -> f32 {
    let along = (elapsed.as_secs_f32() % motion::PROGRESS_SWEEP.as_secs_f32())
        / motion::PROGRESS_SWEEP.as_secs_f32();
    let moved = -1.1 + 4.2 * motion::EASE_IN_OUT.ease(along);
    SEGMENT * moved
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
}

/// A bar of unknown progress (`<ProgressBar indeterminate />`).
pub fn progress_bar() -> ProgressBar {
    // `height = 3` by default.
    ProgressBar {
        height: 3.,
        filled: None,
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
        let done = motion::EASE_DEFAULT.ease(motion::progress(now - self.since, FILL_MOVE));
        self.from + (self.to - self.from) * done
    }
}

impl ProgressBar {
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
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
                if now - fill.since < FILL_MOVE {
                    window.request_animation_frame();
                }
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(relative(fill.at(now)))
                    .rounded(px(2.))
                    .bg(theme.primary())
            }
            None => {
                window.request_animation_frame();
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(relative(sweep_offset(motion::loop_clock())))
                    .w(relative(SEGMENT))
                    .rounded(px(2.))
                    .bg(theme.primary())
            }
        };
        div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_none()
            .rounded(px(2.))
            .overflow_hidden()
            .bg(theme.subtle_overlay())
            .child(fill)
    }
}

#[cfg(test)]
mod tests {
    use super::{SEGMENT, share, sweep_offset};
    use std::time::Duration;

    #[test]
    fn the_segment_sweeps_from_before_the_track_to_past_it() {
        assert!((sweep_offset(Duration::ZERO) - SEGMENT * -1.1).abs() < 1e-4);
        // Halfway through a sweep, ease-in-out is halfway too.
        assert!((sweep_offset(Duration::from_millis(450)) - SEGMENT * 1.).abs() < 1e-3);
        // It starts again from the left.
        assert!((sweep_offset(Duration::from_millis(900)) - SEGMENT * -1.1).abs() < 1e-4);
        assert!(sweep_offset(Duration::from_millis(899)) > 1.);
    }

    #[test]
    fn a_known_total_fills_its_share_and_an_unknown_one_nothing() {
        assert_eq!(share(5, 20), 0.25);
        assert_eq!(share(30, 20), 1.);
        assert_eq!(share(3, 0), 0.);
    }
}
