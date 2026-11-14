//! Plays a script into the whole app in an off-screen window and saves
//! pictures of it, so any view can be checked without a screen:
//!
//!     PICTO_GPUI_COPY=/tmp/…/Name.library PICTO_GPUI_SCRIPT=steps.txt \
//!     PICTO_GPUI_OUT=/tmp/shots cargo test -- --ignored plays_a_script
//!
//! The copy is one `run.sh` made. A script line is a playback line (see
//! `pointer.rs`; `dragfiles`, `drop` and `dragexit` bring files from the
//! system over the window and drop them, as Finder does, which imports
//! them into the copy), `wait <milliseconds>`, `shot <name>`, which saves
//! `<name>.png`, or `window <n>`, which sends what follows to the app's nth
//! window (0 is the main one; Settings and a detail window come after it in
//! the order they opened), or `pace <milliseconds>`, how long the app runs
//! after each input line (200 unless set), for pictures taken soon after
//! one, or `lazy` / `eager`: from there on, draw a frame only when a real
//! window would (see `settle`) / redraw every tick, the default, or
//! `notify <tone> <title> | <message>`, which shows a notification
//! (`error`, `warning`, `info`, `success`; `info+` adds a Redo button), or
//! `action <name>` (`action app_menu::Accept`, `action app_menu::SetRating
//! 3`), which chooses an application menu row, or `invoke <command>
//! <json>`, which sends an application command (`folders.create`, …) and
//! prints its answer, so a script can set a copy up as it sets up the
//! reference's, or `focused`, which prints the control with the keyboard
//! and whether Tab put it there (its focus ring shows).
//! `PICTO_GPUI_LAZY=1` starts a script lazy. `PICTO_GPUI_WINDOW=1213x865`
//! sets the main window's size. `PICTO_GPUI_STATE=<folder>` keeps what the
//! script arranges (rail widths, folds, filters, shortcuts…) in that folder
//! and starts from what it holds; without it every run starts from the
//! defaults and keeps nothing.
//!
//! Nothing here reaches the desktop: opening a file in another
//! application, showing it in Finder and putting files on the clipboard are
//! written down instead (`crate::desktop`). A `desktop` line appends what
//! was asked since the last one, and the clipboard's text, to
//! `desktop.log` in the output folder.
//!
//! A system file, folder or save dialog cannot be driven here, so
//! `answer <path>` answers the next one the app asks for with that path
//! without opening it, and `answer` alone cancels the next one
//! (`crate::prompt`). Give it before the line that opens the dialog; the
//! answers are used in order.
use crate::pointer;
use crate::shell::Shell;
use gpui::{AnyWindowHandle, AppContext, HeadlessAppContext, PlatformTextSystem, px, size};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// One frame's worth of time.
const TICK: Duration = Duration::from_millis(16);

/// Lets the app work for a while: draws, runs what became ready, and moves
/// its timers along with the real clock that its animations read.
///
/// Eager (the default) redraws the whole window every tick. Lazy draws a
/// frame only when a real window would: after something invalidated it
/// outside a draw, or asked for the next animation frame. A view that is
/// right eagerly and wrong lazily waits for a frame nothing schedules, the
/// kind of fault that on screen "fixes itself when the mouse moves".
fn settle(cx: &mut HeadlessAppContext, window: AnyWindowHandle, time: Duration, lazy: bool) {
    let started = std::time::Instant::now();
    loop {
        // A window the script closed (a detail window's Escape) is gone:
        // the app still runs, and `window 0` goes back to the main one.
        let drawn = cx.update_window(window, |_, window, cx| {
            if lazy {
                // The platform's frame callback: the frames asked for run
                // first, then the window is drawn if anything dirtied it.
                let asked = window.simulate_next_frame(cx);
                if window.is_dirty() {
                    if std::env::var_os("PICTO_GPUI_LAZY_DEBUG").is_some() {
                        eprintln!("lazy draw (asked {asked})");
                    }
                    window.draw(cx).clear(cx);
                }
            } else {
                window.refresh();
                window.draw(cx).clear(cx);
            }
        });
        if drawn.is_err() {
            cx.run_until_parked();
            break;
        }
        cx.run_until_parked();
        if started.elapsed() >= time {
            break;
        }
        std::thread::sleep(TICK);
        cx.advance_clock(TICK);
    }
}

#[test]
#[ignore = "takes a disposable library copy and a script"]
fn plays_a_script() {
    let variable =
        |name: &str| PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")));
    let root = crate::application::guard(&variable("PICTO_GPUI_COPY")).unwrap();
    let script = std::fs::read_to_string(variable("PICTO_GPUI_SCRIPT")).unwrap();
    let out = variable("PICTO_GPUI_OUT");
    std::fs::create_dir_all(&out).unwrap();
    let (width, height) = std::env::var("PICTO_GPUI_WINDOW")
        .ok()
        .and_then(|value| {
            let (width, height) = value.split_once('x')?;
            Some((width.parse().ok()?, height.parse().ok()?))
        })
        .unwrap_or((1213., 865.));

    let (events, delivered) = std::sync::mpsc::channel();
    let application = crate::application::Application::new(root.clone(), events);
    let closing = application.clone();
    let text_system: Arc<dyn PlatformTextSystem> = Arc::new(gpui_apple::AppleTextSystem::new());
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(crate::assets::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.allow_parking();
    cx.update(crate::prepare);
    let window = cx
        .open_window(size(px(width), px(height)), move |window, cx| {
            cx.new(|cx| Shell::new(root, application, delivered, window, cx))
        })
        .unwrap();
    let main: AnyWindowHandle = window.into();
    // A window on screen is the active one, which is what makes focus
    // changes report blurs to the views that lose it.
    cx.update_window(main, |_, window, _| window.activate_window())
        .unwrap();
    let mut window = main;
    let mut pace = Duration::from_millis(200);
    let mut lazy = std::env::var_os("PICTO_GPUI_LAZY").is_some_and(|value| value != "0");
    // The library opens and the first page and its thumbnails arrive.
    settle(&mut cx, window, Duration::from_secs(3), lazy);
    for line in script.lines().map(str::trim) {
        // `size`, `js` and `reload` are for the Electron reference
        // (`reference.mjs`), which takes the same scripts.
        if line.is_empty()
            || line.starts_with('#')
            || line == "reload"
            || line.starts_with("size ")
            || line.starts_with("js ")
        {
            continue;
        }
        if let Some(name) = line.strip_prefix("shot ") {
            let picture = cx.capture_screenshot(window).unwrap();
            picture
                .save(out.join(format!("{}.png", name.trim())))
                .unwrap();
        } else if let Some(index) = line.strip_prefix("window ") {
            let index: usize = index.trim().parse().expect("window <n>");
            // The main window first, the others in the order they opened.
            let mut windows = cx.update(|cx| cx.windows());
            windows.sort_by_key(|handle| (*handle != main, handle.window_id().as_u64()));
            window = *windows
                .get(index)
                .unwrap_or_else(|| panic!("there are {} windows", windows.len()));
            settle(&mut cx, window, Duration::from_millis(200), lazy);
        } else if line == "focused" {
            // Prints the control with the keyboard, the off-screen side of
            // the reference's `js document.activeElement`.
            let focused = cx
                .update_window(window, |_, window, cx| {
                    crate::ui::focus::names::focused(window, cx)
                })
                .unwrap();
            println!("focused: {focused}");
        } else if line == "lazy" || line == "eager" {
            lazy = line == "lazy";
        } else if let Some(rest) = line.strip_prefix("notify ") {
            notify_line(&mut cx, rest);
        } else if let Some(name) = line.strip_prefix("action ") {
            // An application menu row chosen: its action, sent to where the
            // keyboard is, as AppKit sends it.
            let action = cx
                .update(|cx| crate::app_menu::named(name.trim(), cx))
                .unwrap_or_else(|| panic!("no action {name}"));
            cx.update_window(window, |_, window, cx| window.dispatch_action(action, cx))
                .unwrap();
            settle(&mut cx, window, pace, lazy);
        } else if line == "desktop" {
            let mut log = crate::desktop::requests();
            let text = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
            log.push(format!("clipboard {}", text.unwrap_or_default()));
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(out.join("desktop.log"))
                .unwrap();
            std::io::Write::write_all(&mut file, format!("{}\n", log.join("\n")).as_bytes())
                .unwrap();
        } else if let Some(rest) = line.strip_prefix("invoke ") {
            // An application command, as the renderer's `invoke` sends it;
            // its answer is printed, and the script goes on once it came.
            let (command, args) = rest.trim().split_once(' ').unwrap_or((rest.trim(), "{}"));
            let args: serde_json::Value =
                serde_json::from_str(args).unwrap_or_else(|error| panic!("{line}: {error}"));
            let mut answer = closing.dispatch(command.to_owned(), args);
            let started = std::time::Instant::now();
            let result = loop {
                settle(&mut cx, window, Duration::from_millis(50), lazy);
                if let Ok(Some(result)) = answer.try_recv() {
                    break result;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(30),
                    "{command} did not answer"
                );
            };
            println!("{command} -> {result:?}");
            settle(&mut cx, window, pace, lazy);
        } else if line == "answer" {
            crate::prompt::answer_next(None);
        } else if let Some(path) = line.strip_prefix("answer ") {
            crate::prompt::answer_next(Some(PathBuf::from(path.trim())));
        } else if let Some(time) = line.strip_prefix("pace ") {
            pace = Duration::from_millis(time.trim().parse().expect("pace <milliseconds>"));
        } else if let Some(time) = line.strip_prefix("wait ") {
            let time = time.trim().parse().expect("wait <milliseconds>");
            settle(&mut cx, window, Duration::from_millis(time), lazy);
        } else {
            let event = pointer::parse(line).unwrap_or_else(|| panic!("not a line: {line}"));
            cx.update_window(window, |_, window, cx| event.deliver(window, cx))
                .unwrap();
            settle(&mut cx, window, pace, lazy);
        }
    }
    // What the script arranged is kept, as when the app quits, so a second
    // run with the same `PICTO_GPUI_STATE` starts from it.
    cx.update(crate::state::flush);
    closing.close();
}

/// `notify <tone> <title> | <message>`: shows a notification as the app
/// would, to photograph it.
fn notify_line(cx: &mut HeadlessAppContext, line: &str) {
    use crate::ui::notify::{self, Options, Tone};
    let (tone, text) = line.split_once(' ').unwrap_or((line, ""));
    let (title, message) = text.split_once(" | ").unwrap_or((text, ""));
    let (tone, action) = match tone.strip_suffix('+') {
        Some(tone) => (tone, true),
        None => (tone, false),
    };
    let tone = Tone::from_name(tone).expect("notify error|warning|info|success");
    let mut options = Options::new(title.to_owned(), message.to_owned());
    if action {
        options = options.action("Redo", |_, _| {});
    }
    cx.update(|cx| notify::show(tone, options, cx));
}
