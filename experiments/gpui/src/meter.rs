//! Opt-in frame meter for debugging: with `PICTO_GPUI_FPS=1` the window's
//! bottom-left corner shows how many frames were drawn in the last second,
//! the time between the last two, and how long the app spent drawing one
//! (from the start of the shell's render to the end of its paint; what the
//! GPU does afterwards is not in it). Costs nothing when the variable is
//! unset. The meter asks for no frames of its own except one a second while
//! nothing else is drawing, to say so.
use crate::theme::{self, Text as _};
use gpui::*;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PICTO_GPUI_FPS").is_some())
}

/// The meter's backing, dark enough to read over any picture.
const BACKING: u32 = 0x000000b3;

#[derive(Default)]
struct Meter {
    /// When the frame being drawn began.
    started: Option<Instant>,
    /// The frame being drawn is the meter's own once-a-second look, which
    /// does not count as a frame of the app's.
    idle_look: bool,
    /// When each frame of the last second ended, and how long it took to draw.
    frames: VecDeque<(Instant, Duration)>,
}

fn meter() -> &'static Mutex<Meter> {
    static METER: OnceLock<Mutex<Meter>> = OnceLock::new();
    METER.get_or_init(Default::default)
}

/// What the meter shows for the frames it holds, at `now`.
#[derive(Debug, PartialEq)]
struct Reading {
    frames_per_second: usize,
    /// Between the last two frames.
    interval: Option<Duration>,
    /// Drawing the last frame, and the longest in the last second.
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

/// The shell's render began: a frame is being drawn.
pub fn frame_started() {
    if enabled()
        && let Ok(mut meter) = meter().lock()
    {
        meter.started = Some(Instant::now());
    }
}

/// The next frame is only the meter looking again while nothing draws.
pub fn idle_look() {
    if let Ok(mut meter) = meter().lock() {
        meter.idle_look = true;
    }
}

/// Whether nothing has been drawn for a second, so the meter should look.
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

/// The meter, to lay over the window as its last child, when it is on.
pub fn overlay(cx: &App) -> Option<AnyElement> {
    if !enabled() {
        return None;
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
    // Painted last, so its paint is the end of the frame's drawing.
    let end =
        canvas(
            |_, _, _| {},
            |_, _, _, _| {
                if let Ok(mut meter) = meter().lock() {
                    let now = Instant::now();
                    let started = meter.started.take();
                    if !std::mem::take(&mut meter.idle_look)
                        && let Some(started) = started
                    {
                        meter.frames.push_back((now, now.duration_since(started)));
                    }
                    while meter.frames.front().is_some_and(|(ended, _)| {
                        now.duration_since(*ended) > Duration::from_secs(2)
                    }) {
                        meter.frames.pop_front();
                    }
                }
            },
        )
        .absolute()
        .size_0();
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
            .child(end)
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
        // Three frames in the last second, one before it.
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
        // Nothing drawn for a second: idle.
        let shown = reading(&frames, now + Duration::from_secs(2));
        assert_eq!(shown.frames_per_second, 0);
        assert_eq!(shown.draw, None);
    }
}
