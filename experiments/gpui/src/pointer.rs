//! Opt-in pointer playback, so hover and pressed states can be checked in the
//! running app without taking the real pointer. Set `PICTO_GPUI_POINTER` to a
//! FIFO (a plain file is read again from its start each time it ends); each
//! line written to it is replayed into the window. Debug builds only (see
//! `seams.rs`); the off-screen tool plays the same lines (`offscreen.rs`):
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
//!     dragfiles 600 400 /path/one.png /path/two
//!     drop 600 400 /path/one.png /path/two
//!     dragexit
//!     window 1
//!
//! `window <n>` sends the lines after it to the app's nth window (0 is the
//! main one, the others in the order they opened, as the off-screen tool
//! counts them); when that window closes they go to the main one again.
//! Coordinates are window points (the system's, which a zoomed window
//! divides by its interface zoom). A fourth number is the click count for
//! `down` and `up`, and the distance in points for `scroll`. `key` takes a
//! keystroke as GPUI writes them (`secondary-` is Command on macOS and
//! Control elsewhere, so one script serves every platform) and `type` the text to enter in the focused
//! field. `dragfiles` brings files from the system over the window at a
//! point, as a drag from Finder does (the platform's file-drop entered and
//! pending events), `drop` also lets them go there (submit, then exited),
//! and `dragexit` takes them out of the window again; the paths are
//! separated by spaces, so a path cannot hold one. Nothing is read when the
//! variable is unset.
use gpui::*;
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::time::Duration;

/// One line of playback.
pub enum Line {
    Input(PlatformInput),
    /// Several events in a row, as the platform sends them for one gesture.
    Inputs(Vec<PlatformInput>),
    /// Pressed as though typed: bindings first, then the focused text field.
    Keys(Vec<Keystroke>),
    /// The lines after it go to this window (`window <n>`).
    Window(usize),
}

impl Line {
    /// Gives the line to the window as though it came from the system.
    pub fn deliver(self, window: &mut Window, cx: &mut App) {
        match self {
            // Positions are the window's screen points, which the window
            // divides by the interface zoom as it does the system's.
            Line::Input(event) => {
                window.dispatch_platform_input(event, cx);
            }
            Line::Inputs(events) => {
                for event in events {
                    window.dispatch_platform_input(event, cx);
                }
            }
            Line::Window(_) => {}
            Line::Keys(keys) => {
                for key in keys {
                    // The system reports held modifiers before the key and
                    // their release after it, which `Window::modifiers`
                    // follows (Shift with an arrow extends the selection).
                    let modifiers = key.modifiers;
                    let held = modifiers != Modifiers::default();
                    if held {
                        window.dispatch_event(modifiers_changed(modifiers), cx);
                    }
                    window.dispatch_keystroke(key, cx);
                    if held {
                        window.dispatch_event(modifiers_changed(Modifiers::default()), cx);
                    }
                }
            }
        }
    }
}

fn modifiers_changed(modifiers: Modifiers) -> PlatformInput {
    PlatformInput::ModifiersChanged(ModifiersChangedEvent {
        modifiers,
        capslock: Capslock::default(),
    })
}

pub fn parse(line: &str) -> Option<Line> {
    let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
    let keystroke = |text: String| Keystroke::parse(interned(text)).ok();
    match kind {
        "key" => {
            let platform = if cfg!(target_os = "macos") {
                "cmd-"
            } else {
                "ctrl-"
            };
            let written = rest.trim().replace("secondary-", platform);
            return Some(Line::Keys(vec![keystroke(written)?]));
        }
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
        "window" => return rest.trim().parse().ok().map(Line::Window),
        "dragexit" => return Some(Line::Input(PlatformInput::FileDrop(FileDropEvent::Exited))),
        "dragfiles" | "drop" => return file_drop(kind == "drop", rest).map(Line::Inputs),
        _ => {}
    }
    pointer(line).map(Line::Input)
}

/// GPUI parses keystrokes from static text: each distinct one is kept for
/// the rest of the run, once.
fn interned(text: String) -> &'static str {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(kept) = seen.iter().find(|kept| **kept == text) {
        return kept;
    }
    let kept: &'static str = String::leak(text);
    seen.push(kept);
    kept
}

/// Files from the system over the window at a point (`dragfiles`), and let
/// go there (`drop`): the events a drag from Finder makes.
fn file_drop(submit: bool, rest: &str) -> Option<Vec<PlatformInput>> {
    let mut parts = rest.split_whitespace();
    let position = point(
        px(parts.next()?.parse().ok()?),
        px(parts.next()?.parse().ok()?),
    );
    let paths = ExternalPaths(parts.map(std::path::PathBuf::from).collect());
    if paths.paths().is_empty() {
        return None;
    }
    let mut events = vec![
        PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }),
        PlatformInput::FileDrop(FileDropEvent::Pending { position }),
    ];
    if submit {
        events.push(PlatformInput::FileDrop(FileDropEvent::Submit { position }));
        events.push(PlatformInput::FileDrop(FileDropEvent::Exited));
    }
    Some(events)
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

/// Plays the lines written to `PICTO_GPUI_POINTER` into `window`, if it
/// is set.
#[cfg(debug_assertions)]
pub fn start(window: AnyWindowHandle, cx: &mut App) {
    let Some(path) = crate::seams::pointer_file() else {
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
        let main = window;
        let mut target = main;
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            while let Ok(event) = receiver.try_recv() {
                if let Line::Window(index) = event {
                    // The main window first, the others in the order they
                    // opened.
                    let mut windows = cx.update(|cx| cx.windows());
                    windows.sort_by_key(|handle| (*handle != main, handle.window_id().as_u64()));
                    target = windows.get(index).copied().unwrap_or(main);
                    continue;
                }
                let mut delivered = target.update(cx, |_, window, cx| event.deliver(window, cx));
                if delivered.is_err() && target != main {
                    target = main;
                    delivered = Ok(());
                }
                if delivered.is_err() {
                    return;
                }
            }
        }
    })
    .detach();
}
