//! Plays recorded input into the app's windows, so that hover and pressed
//! states can be checked in the running app without moving the real pointer.
//! Set `PICTO_GPUI_POINTER` to a FIFO or to a file. Each line written to it
//! is played into the window. A plain file is read again from its start each
//! time its end is reached. Only debug builds read the variable (see
//! `seams.rs`). The off-screen tool plays the same lines (`offscreen.rs`):
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
//!     dropdata 600 400 image=/path/photo.png uri=https://e.test/photo.png
//!     window 1
//!
//! `window <n>` sends the lines after it to the nth window of the app. Window
//! 0 is the main window, and the others are numbered in the order they
//! opened, as the off-screen tool numbers them. When that window closes, the
//! lines go to the main window again.
//!
//! Coordinates are in window points as the system counts them; a zoomed
//! window divides them by its interface zoom. A fourth number is the click
//! count for `down` and `up`, and the distance in points for `scroll`. `key`
//! takes a keystroke in GPUI's notation. In it, `secondary-` stands for
//! Command on macOS and for Control elsewhere, so that one script works on
//! every platform. `type` takes the text to enter in the focused field.
//!
//! `dragfiles` holds files from the system over the window at a point, as a
//! drag from Finder does: it sends the platform's file-drop events `Entered`
//! and `Pending`. `drop` does the same and then lets the files go at that
//! point. It sends a mouse release and then `Exited`, and not the platform's
//! drop event, which would bring the app to the front. `dragexit` takes the
//! files out of the window again. The paths are separated by spaces, so a
//! path cannot contain a space.
//!
//! `dropdata` drops what a web browser drags, which holds no files. Each
//! `image=<path>` is a file in memory that holds the bytes of that file.
//! `uri=`, `text=` and `download=` give an address list, a text and a
//! Chromium `DownloadURL`. `html=<path>` gives the text of that file as HTML.
//! This data is let go with a mouse release too.
//!
//! Nothing is read when the variable is not set.
use gpui::*;
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::time::Duration;

/// One line of the input.
pub enum Line {
    Input(PlatformInput),
    /// Several events that are delivered one after the other, as the platform
    /// sends them for one gesture.
    Inputs(Vec<PlatformInput>),
    /// Keystrokes that are delivered as typed keys are: first to the key
    /// bindings, then to the focused text field.
    Keys(Vec<Keystroke>),
    /// A drop of data that is not a file. The events in `over` bring the drag
    /// over the window, `data` is then set as what the platform read from the
    /// drag, and the events in `release` let it go.
    DropData {
        over: Vec<PlatformInput>,
        data: ExternalDropData,
        release: Vec<PlatformInput>,
    },
    /// The lines after this one go to this window (`window <n>`).
    Window(usize),
}

impl Line {
    /// Delivers the line to the window as though it came from the system.
    pub fn deliver(self, window: &mut Window, cx: &mut App) {
        match self {
            // Positions are in the window's points as the system counts them.
            // The window divides them by the interface zoom, as it does for
            // the system's own events.
            Line::Input(event) => {
                window.dispatch_platform_input(event, cx);
            }
            Line::Inputs(events) => {
                for event in events {
                    window.dispatch_platform_input(event, cx);
                }
            }
            Line::DropData {
                over,
                data,
                release,
            } => {
                for event in over {
                    window.dispatch_event(event, cx);
                }
                set_external_drop_data(data);
                for event in release {
                    window.dispatch_event(event, cx);
                }
            }
            Line::Window(_) => {}
            Line::Keys(keys) => {
                for key in keys {
                    // The system reports the held modifiers before the key
                    // and their release after it, and `Window::modifiers`
                    // follows these reports. A view reads it, for example to
                    // extend the selection when Shift is held with an arrow
                    // key.
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
        "dropdata" => return drop_data(rest),
        _ => {}
    }
    pointer(line).map(Line::Input)
}

/// Returns `text` as static text, which is the only kind of text that GPUI
/// parses a keystroke from. Each distinct text is kept once, for the rest of
/// the run.
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

/// Returns the events for files from the system held over the window at a
/// point (`dragfiles`) and, with `submit`, also let go there (`drop`). They
/// are the events a drag from Finder makes, except that the files are let go
/// with a mouse release and not with the platform's drop event. GPUI answers
/// the platform's drop event by bringing the app to the front, over whatever
/// the user is working in.
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
        events.push(PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }));
        events.push(PlatformInput::FileDrop(FileDropEvent::Exited));
    }
    Some(events)
}

/// Returns the line for a drop, at a point, of what a web browser drags
/// (`dropdata`).
fn drop_data(rest: &str) -> Option<Line> {
    let mut parts = rest.split_whitespace();
    let position = point(
        px(parts.next()?.parse().ok()?),
        px(parts.next()?.parse().ok()?),
    );
    let mut data = ExternalDropData::default();
    for part in parts {
        let (kind, value) = part.split_once('=')?;
        match kind {
            "image" => data.files.push(ExternalDropFile {
                bytes: std::fs::read(value).ok()?,
                name: std::path::Path::new(value)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                media_type: None,
            }),
            "uri" => data.uri_lists.push(value.to_owned()),
            "text" => data.text.push(value.to_owned()),
            "download" => data.download_urls.push(value.to_owned()),
            "html" => data.html.push(std::fs::read_to_string(value).ok()?),
            _ => return None,
        }
    }
    let paths = ExternalPaths(Default::default());
    Some(Line::DropData {
        over: vec![
            PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }),
            PlatformInput::FileDrop(FileDropEvent::Pending { position }),
        ],
        data,
        release: vec![
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            PlatformInput::FileDrop(FileDropEvent::Exited),
        ],
    })
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

/// Plays the lines written to the file that `PICTO_GPUI_POINTER` names into
/// `window`. Does nothing when the variable is not set.
#[cfg(debug_assertions)]
pub fn start(window: AnyWindowHandle, cx: &mut App) {
    let Some(path) = crate::seams::pointer_file() else {
        return;
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        // A FIFO reports the end of the file each time its writer closes it,
        // so the file is opened again.
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
                    // The main window is first, and the others follow in the
                    // order they opened.
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
