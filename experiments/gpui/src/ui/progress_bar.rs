//! `ProgressBar.tsx` with `indeterminate`: a thin bar on the subtle overlay
//! across which a 35% primary segment sweeps every 900 ms. The determinate
//! form (a share of done over total) is not ported yet.
use crate::{motion, theme};
use gpui::*;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `progress-indeterminate`: one sweep.
const SWEEP: Duration = Duration::from_millis(900);
/// `.indeterminate { width: 35% }`.
const SEGMENT: f32 = 0.35;

/// Where the indeterminate segment's left edge is, as a share of the track:
/// `translateX(-110%)` to `translateX(310%)` of its own width, on
/// `ease-in-out`, over and over.
pub fn sweep_offset(elapsed: Duration) -> f32 {
    let along = (elapsed.as_secs_f32() % SWEEP.as_secs_f32()) / SWEEP.as_secs_f32();
    let moved = -1.1 + 4.2 * motion::EASE_IN_OUT.ease(along);
    SEGMENT * moved
}

/// One clock for every bar, so they sweep together.
fn clock() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

#[derive(IntoElement)]
pub struct ProgressBar {
    height: f32,
}

/// A bar of unknown progress (`<ProgressBar indeterminate />`).
pub fn progress_bar() -> ProgressBar {
    // `height = 3` by default.
    ProgressBar { height: 3. }
}

impl ProgressBar {
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        window.request_animation_frame();
        let fill = div()
            .absolute()
            .top_0()
            .h_full()
            .left(relative(sweep_offset(clock().elapsed())))
            .w(relative(SEGMENT))
            .rounded(px(2.))
            .bg(theme.primary());
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
    use super::{SEGMENT, sweep_offset};
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
}
