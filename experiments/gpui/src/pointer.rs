//! Opt-in pointer playback, so hover and pressed states can be checked in the
//! running app without taking the real pointer. Set `PICTO_GPUI_POINTER` to a
//! file or FIFO; each line written to it is replayed into the window:
//!
//!     move 320 24
//!     down 320 24
//!     up 320 24
//!     rdown 320 24
//!     rup 320 24
//!     down 320 24 2
//!     scroll 600 400 -900
//!     key space
//!     key cmd-shift-j
//!     type some text
//!
//! Coordinates are window points. A fourth number is the click count for
//! `down` and `up`, and the distance in points for `scroll`. `key` takes a
//! keystroke as GPUI writes them and `type` the text to enter in the focused
//! field. Nothing is read when the variable is unset.
use gpui::*;
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::time::Duration;

/// One line of playback.
pub enum Line {
    Input(PlatformInput),
    /// Pressed as though typed: bindings first, then the focused text field.
    Keys(Vec<Keystroke>),
}

impl Line {
    /// Gives the line to the window as though it came from the system.
    pub fn deliver(self, window: &mut Window, cx: &mut App) {
        match self {
            Line::Input(event) => {
                window.dispatch_event(event, cx);
            }
            Line::Keys(keys) => {
                for key in keys {
                    window.dispatch_keystroke(key, cx);
                }
            }
        }
    }
}

pub fn parse(line: &str) -> Option<Line> {
    let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
    // GPUI parses static text; a playback line lives for the whole run.
    let keystroke = |text: String| Keystroke::parse(String::leak(text)).ok();
    match kind {
        "key" => return Some(Line::Keys(vec![keystroke(rest.trim().to_string())?])),
        "type" => {
            let keys = rest
                .chars()
                .filter_map(|character| match character {
                    ' ' => keystroke("space-> ".to_string()),
                    character => keystroke(format!("{character}->{character}")),
                })
                .collect();
            return Some(Line::Keys(keys));
        }
        _ => {}
    }
    pointer(line).map(Line::Input)
}

fn pointer(line: &str) -> Option<PlatformInput> {
    let mut parts = line.split_whitespace();
    let kind = parts.next()?;
    let position = point(
        px(parts.next()?.parse().ok()?),
        px(parts.next()?.parse().ok()?),
    );
    let extra = parts.next().and_then(|value| value.parse::<f32>().ok());
    let click_count = extra.map_or(1, |count| count as usize);
    let modifiers = Modifiers::default();
    Some(match kind {
        "move" => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        "drag" => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        "down" => PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count,
            first_mouse: false,
        }),
        "up" => PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count,
        }),
        "rdown" => PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Right,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        "rup" => PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Right,
            position,
            modifiers,
            click_count: 1,
        }),
        "scroll" => PlatformInput::Scroll(ScrollEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(extra?))),
            modifiers,
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        }),
        _ => return None,
    })
}

pub fn start(window: AnyWindowHandle, cx: &mut App) {
    let Ok(path) = std::env::var("PICTO_GPUI_POINTER") else {
        return;
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        // A FIFO reports end of file each time its writer closes; reopen it.
        loop {
            let Ok(file) = std::fs::File::open(&path) else {
                return;
            };
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if let Some(event) = parse(&line) {
                    if sender.send(event).is_err() {
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            while let Ok(event) = receiver.try_recv() {
                let delivered = window.update(cx, |_, window, cx| event.deliver(window, cx));
                if delivered.is_err() {
                    return;
                }
            }
        }
    })
    .detach();
}
