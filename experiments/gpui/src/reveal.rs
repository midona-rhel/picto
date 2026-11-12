//! When a grid thumbnail fades in. A port of `thumbnailRevealTracker.ts` and
//! `thumbnailTiming.ts`: the product reveal contract.
//!
//! A tile keeps its reveal state while it stays within the margin around the
//! viewport, even if layout or item order changes. Leaving that region removes
//! the state, so coming back reveals again. Every genuine entry waits 100 ms
//! and reveals over 500 ms, including thumbnails that were already cached:
//! loading and revealing are separate decisions.
use std::collections::HashMap;

/// Layout pixels above and below the real viewport that count as visible.
pub const MARGIN: f32 = 500.;
const DELAY_MS: f64 = 100.;
const DURATION_MS: f64 = 500.;

struct State {
    entered_at: f64,
    started_at: Option<f64>,
}

#[derive(Default)]
pub struct RevealTracker {
    visible: HashMap<u32, State>,
}

impl RevealTracker {
    /// `tiles` are the ids inside the margin and whether each one's thumbnail
    /// is decoded. `now` is milliseconds on any steady clock.
    pub fn update(&mut self, tiles: &[(u32, bool)], now: f64) {
        self.visible
            .retain(|id, _| tiles.iter().any(|(tile, _)| tile == id));
        for &(id, has_bitmap) in tiles {
            match self.visible.get_mut(&id) {
                None => {
                    self.visible.insert(
                        id,
                        State {
                            entered_at: now,
                            started_at: has_bitmap.then_some(now + DELAY_MS),
                        },
                    );
                }
                Some(state) if state.started_at.is_none() && has_bitmap => {
                    state.started_at = Some(now.max(state.entered_at + DELAY_MS));
                }
                Some(_) => {}
            }
        }
    }

    pub fn progress(&self, id: u32, now: f64) -> f64 {
        match self.visible.get(&id).and_then(|state| state.started_at) {
            Some(started_at) => ((now - started_at) / DURATION_MS).clamp(0., 1.),
            None => 0.,
        }
    }

    /// Whether any reveal is waiting out its delay or running, so the grid
    /// needs another frame.
    pub fn active(&self, now: f64) -> bool {
        self.visible
            .values()
            .filter_map(|state| state.started_at)
            .any(|started_at| now - started_at < DURATION_MS)
    }
}

/// The two-phase reveal of `drawBase.ts`: the image fades in during the first
/// half, then the placeholder under it fades out during the second half.
pub fn alphas(progress: f64) -> (f32, f32) {
    let image = (progress * 2.).min(1.);
    let placeholder = if progress < 0.5 {
        1.
    } else {
        (2. - progress * 2.).max(0.)
    };
    (image as f32, placeholder as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_thumbnail_still_waits_then_reveals() {
        let mut tracker = RevealTracker::default();
        tracker.update(&[(1, true)], 1000.);
        assert_eq!(tracker.progress(1, 1000.), 0.);
        assert_eq!(tracker.progress(1, 1100.), 0.);
        assert_eq!(tracker.progress(1, 1350.), 0.5);
        assert_eq!(tracker.progress(1, 1600.), 1.);
        assert!(tracker.active(1050.), "waiting out the delay");
        assert!(tracker.active(1599.));
        assert!(!tracker.active(1600.));
    }

    #[test]
    fn a_slow_decode_starts_when_it_arrives_but_not_before_the_delay() {
        let mut tracker = RevealTracker::default();
        tracker.update(&[(1, false), (2, false)], 0.);
        assert!(
            !tracker.active(50.),
            "nothing to animate until a bitmap exists"
        );
        // Decoded 30 ms after entering: still held to the 100 ms delay.
        tracker.update(&[(1, true), (2, false)], 30.);
        assert_eq!(tracker.progress(1, 100.), 0.);
        assert_eq!(tracker.progress(1, 350.), 0.5);
        // Decoded 400 ms after entering: starts at once.
        tracker.update(&[(1, true), (2, true)], 400.);
        assert_eq!(tracker.progress(2, 400.), 0.);
        assert_eq!(tracker.progress(2, 650.), 0.5);
    }

    #[test]
    fn leaving_the_margin_forgets_the_reveal() {
        let mut tracker = RevealTracker::default();
        tracker.update(&[(1, true)], 0.);
        assert_eq!(tracker.progress(1, 600.), 1.);
        // Still visible after a reorder: stays revealed.
        tracker.update(&[(2, true), (1, true)], 700.);
        assert_eq!(tracker.progress(1, 700.), 1.);
        tracker.update(&[(2, true)], 800.);
        assert_eq!(tracker.progress(1, 800.), 0.);
        tracker.update(&[(1, true), (2, true)], 900.);
        assert_eq!(tracker.progress(1, 900.), 0., "re-entry reveals again");
        assert_eq!(tracker.progress(1, 1250.), 0.5);
    }

    #[test]
    fn image_fades_in_first_then_the_placeholder_fades_out() {
        assert_eq!(alphas(0.), (0., 1.));
        assert_eq!(alphas(0.25), (0.5, 1.));
        assert_eq!(alphas(0.5), (1., 1.));
        assert_eq!(alphas(0.75), (1., 0.5));
        assert_eq!(alphas(1.), (1., 0.));
    }
}
