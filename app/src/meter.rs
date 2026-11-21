//! A frame meter for debugging. When the environment variable
//! `PICTO_GPUI_FPS` is set, the window's bottom-left corner shows how many
//! frames were drawn in the last second, the time between the last two, and
//! how long the app spent drawing the last one. The drawing time runs from
//! the start of the shell's render to the end of its paint; the work the GPU
//! does afterwards is not included.
//!
//! `stats.rs` also measures the time between frames from the end of each
//! frame. So when either the meter or the statistics are on, the window gets
//! an invisible element that is painted last and records the end of the
//! frame. When both are off, nothing is added to the window. The meter asks
//! for no frames of its own, except one a second while nothing else is
//! drawing, so that it can show `idle`.
use crate::theme::{self, Text as _};
use gpui::*;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PICTO_GPUI_FPS").is_some())
}

/// The colour of the rectangle behind the meter's text: black at 70%
/// opacity, so that the text can be read over any picture.
const BACKING: u32 = 0x000000b3;

#[derive(Default)]
struct Meter {
    /// When the frame being drawn began.
    started: Option<Instant>,
    /// Whether the frame being drawn is one the meter asked for itself,
    /// once a second while nothing else draws. Such a frame is not counted.
    idle_look: bool,
    /// When each frame of the last second ended, and how long it took to draw.
    frames: VecDeque<(Instant, Duration)>,
}

fn meter() -> &'static Mutex<Meter> {
    static METER: OnceLock<Mutex<Meter>> = OnceLock::new();
    METER.get_or_init(Default::default)
}

/// Holds what the meter shows at a given moment for the frames it has
/// recorded.
#[derive(Debug, PartialEq)]
struct Reading {
    frames_per_second: usize,
    /// The time between the ends of the last two frames.
    interval: Option<Duration>,
    /// How long drawing the last frame took, and the longest drawing time
    /// of a frame in the last second.
    draw: Option<(Duration, Duration)>,
}

fn reading(frames: &VecDeque<(Instant, Duration)>, now: Instant) -> Reading {
    let recent = || {
        frames
            .iter()
            .filter(move |(ended, _)| now.duration_since(*ended) <= Duration::from_secs(1))
    };
    let mut last_two = recent().rev().take(2);
    let last = last_two.next();
    let before = last_two.next();
    Reading {
        frames_per_second: recent().count(),
        interval: last
            .zip(before)
            .map(|((ended, _), (previous, _))| ended.duration_since(*previous)),
        draw: last.map(|(_, draw)| {
            (
                *draw,
                recent().map(|(_, draw)| *draw).max().unwrap_or_default(),
            )
        }),
    }
}

/// Records that the shell's render began, which is the start of a frame.
pub fn frame_started() {
    if enabled()
        && let Ok(mut meter) = meter().lock()
    {
        meter.started = Some(Instant::now());
    }
}

/// Marks the next frame as one the meter asked for itself while nothing
/// else draws, so that it is not counted.
pub fn idle_look() {
    if let Ok(mut meter) = meter().lock() {
        meter.idle_look = true;
    }
}

/// Returns whether no frame has been drawn for a second. The meter then
/// has to be drawn again so that it shows `idle`.
pub fn idle() -> bool {
    meter().lock().is_ok_and(|meter| {
        meter
            .frames
            .back()
            .is_none_or(|(ended, _)| ended.elapsed() >= Duration::from_secs(1))
    })
}

fn milliseconds(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1000.)
}

/// Records that a frame of the window has been painted.
fn frame_ended() {
    let now = Instant::now();
    crate::stats::frame_drawn(now);
    if !enabled() {
        return;
    }
    if let Ok(mut meter) = meter().lock() {
        let started = meter.started.take();
        if !std::mem::take(&mut meter.idle_look)
            && let Some(started) = started
        {
            meter.frames.push_back((now, now.duration_since(started)));
        }
        while meter
            .frames
            .front()
            .is_some_and(|(ended, _)| now.duration_since(*ended) > Duration::from_secs(2))
        {
            meter.frames.pop_front();
        }
    }
}

/// Returns the meter as an element to put over the window as its last
/// child, when the meter is on. Returns only the invisible element that
/// records the end of the frame when only the statistics of `stats.rs` are
/// on, and `None` when both are off.
pub fn overlay(cx: &App) -> Option<AnyElement> {
    // This element is painted last, so the moment it is painted is the end
    // of the frame's drawing.
    let end = || {
        canvas(|_, _, _| {}, |_, _, _, _| frame_ended())
            .absolute()
            .size_0()
    };
    if !enabled() {
        return crate::stats::enabled().then(|| end().into_any_element());
    }
    let shown = meter()
        .lock()
        .map(|meter| reading(&meter.frames, Instant::now()))
        .ok()?;
    let text = match (shown.frames_per_second, shown.interval, shown.draw) {
        (0, _, _) => "idle".to_owned(),
        (frames, interval, draw) => format!(
            "{frames} fps · {} ms · draw {} ms (max {})",
            interval.map_or("–".into(), milliseconds),
            draw.map_or("–".into(), |(last, _)| milliseconds(last)),
            draw.map_or("–".into(), |(_, longest)| milliseconds(longest)),
        ),
    };
    let theme = theme::theme(cx);
    Some(
        div()
            .absolute()
            .left(px(8.))
            .bottom(px(8.))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgba(BACKING))
            .font_family(theme::FONT_INSPECTOR_MONO)
            .text(theme::TEXT_XS)
            .text_color(theme.text())
            .child(text)
            .child(end())
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::reading;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    #[test]
    fn the_meter_reads_the_last_second_of_frames() {
        let now = Instant::now();
        let ago = |milliseconds: u64| now - Duration::from_millis(milliseconds);
        let draw = Duration::from_millis;
        // Three frames ended in the last second and one ended before it.
        let frames = VecDeque::from([
            (ago(1500), draw(9)),
            (ago(40), draw(2)),
            (ago(24), draw(5)),
            (ago(8), draw(3)),
        ]);
        let shown = reading(&frames, now);
        assert_eq!(shown.frames_per_second, 3);
        assert_eq!(shown.interval, Some(Duration::from_millis(16)));
        assert_eq!(shown.draw, Some((draw(3), draw(5))));
        // Two seconds later no frame ended in the last second, so the meter
        // shows `idle`.
        let shown = reading(&frames, now + Duration::from_secs(2));
        assert_eq!(shown.frames_per_second, 0);
        assert_eq!(shown.draw, None);
    }
}
