//! Plays a script into the whole app in an off-screen window and saves
//! pictures of the window, so that any view can be checked without a
//! screen:
//!
//!     PICTO_GPUI_COPY=/tmp/…/Name.library PICTO_GPUI_SCRIPT=steps.txt \
//!     PICTO_GPUI_OUT=/tmp/shots cargo test -- --ignored plays_a_script
//!
//! `PICTO_GPUI_COPY` names a disposable copy of a library that `run.sh`
//! made, `PICTO_GPUI_SCRIPT` names the script, and `PICTO_GPUI_OUT` names
//! the folder that the pictures are saved in. An empty script line and a
//! line that starts with `#` are skipped. Every other line is one of these:
//!
//! - A playback line (see `pointer.rs`). Among them, `dragfiles` brings
//!   files from the system over the window, as a drag from Finder does,
//!   `drop` drops them there, and `dragexit` takes them out of the window
//!   again. A drop imports the files into the open library.
//! - `wait <milliseconds>` lets the app run for that long.
//! - `shot <name>` saves a picture of the window as `<name>.png`.
//! - `a11y <name>` writes the window's accessibility tree to
//!   `<name>.a11y.txt`.
//! - `window <n>` sends the lines after it to the app's nth window. Window
//!   0 is the main one. Settings and detail windows come after it in the
//!   order they opened.
//! - `pace <milliseconds>` sets how long the app runs after each input
//!   line. It is 200 unless set. A lower value lets a `shot` line take its
//!   picture sooner after an input line.
//! - `lazy` makes the tool draw a frame only when a real window would (see
//!   `settle`). `eager` makes it redraw on every tick again, which is the
//!   default.
//! - `notify <tone> <title> | <message>` shows a notification. The tone is
//!   `error`, `warning`, `info` or `success`. A `+` after the tone
//!   (`info+`) adds a Redo button.
//! - `update <status> [darwin|win32|linux]` sets the update state that a
//!   check for updates would reach (`downloading` at 42%, `downloaded`,
//!   `available`, …) and shows its notification. `update dialog` opens the
//!   update dialog.
//! - `action <name>` runs the action of an application menu row
//!   (`action app_menu::Accept`, `action app_menu::SetRating 3`).
//! - `invoke <command> <json>` sends an application command
//!   (`folders.create`, …) and prints its answer, so that a script can
//!   prepare the library it works on.
//! - `focused` prints the name of the control that has keyboard focus, and
//!   adds `(ring)` when the control shows its focus ring.
//! - `desktop`, `answer <path>` and `answer` are described below.
//!
//! `PICTO_GPUI_LAZY=1` makes a script start lazy.
//! `PICTO_GPUI_WINDOW=1213x865` sets the main window's size.
//! `PICTO_GPUI_STATE=<folder>` stores the app's state (rail widths, folds,
//! filters, shortcuts…) in that folder, and a run starts from the state
//! that the folder holds. Without it, every run starts from the defaults
//! and stores nothing.
//!
//! The tool sends nothing to the desktop. A request to open a file in
//! another application, to show a file in Finder or to put files on the
//! clipboard is recorded instead (`crate::desktop`). A `desktop` line
//! appends the requests recorded since the previous `desktop` line, and the
//! clipboard's text, to `desktop.log` in the output folder.
//!
//! The tool cannot operate a system file, folder or save dialog. An
//! `answer <path>` line makes the next dialog that the app asks for return
//! that path without opening, and an `answer` line makes the next dialog
//! return as cancelled (`crate::prompt`). Give the line before the line
//! that opens the dialog. Several answers are used in the order they were
//! given.
use crate::pointer;
use gpui::{AnyWindowHandle, AppContext, HeadlessAppContext, PlatformTextSystem, px, size};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Returns the text system that the pictures are drawn with. On macOS it is
/// the system's text system, which can be created off the main thread. On
/// other systems it is the text system that GPUI's Linux platform uses,
/// created without the system's fonts. It then holds only the fonts the app
/// bundles, so a picture does not depend on the fonts installed on the
/// machine.
#[cfg(target_os = "macos")]
fn text_system() -> Arc<dyn PlatformTextSystem> {
    Arc::new(gpui_apple::AppleTextSystem::new())
}

#[cfg(not(target_os = "macos"))]
fn text_system() -> Arc<dyn PlatformTextSystem> {
    Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        crate::theme::FONT_INSPECTOR,
    ))
}

/// Returns the renderer that the pictures are drawn with. On macOS it is
/// GPUI's headless Metal renderer. On other systems it is the wgpu
/// renderer, which uses a software adapter when the machine has no GPU
/// (lavapipe on Linux, WARP on Windows). GPUI's Windows platform has no
/// headless renderer of its own.
#[cfg(target_os = "macos")]
fn renderer() -> anyhow::Result<Option<Box<dyn gpui::PlatformHeadlessRenderer>>> {
    gpui_platform::current_headless_renderer()
}

#[cfg(not(target_os = "macos"))]
fn renderer() -> anyhow::Result<Option<Box<dyn gpui::PlatformHeadlessRenderer>>> {
    let renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    Ok(Some(Box::new(renderer)))
}

/// The time between two frames.
const TICK: Duration = Duration::from_millis(16);

thread_local! {
    /// How long each eager frame took to draw. The test prints a summary of
    /// these times when `PICTO_GPUI_STATS` is set.
    static DRAWS: std::cell::RefCell<Vec<Duration>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Lets the app run for `time`. Each tick it draws the window, runs the
/// tasks that became ready, and advances the app's timers by the time that
/// passed on the real clock, which the app's animations read.
///
/// When `lazy` is false (eager, the default), the whole window is redrawn
/// on every tick. When `lazy` is true, a frame is drawn only when a real
/// window would draw one: after something invalidated the window outside a
/// draw, or after something asked for the next animation frame. A view that
/// is drawn correctly when eager and wrongly when lazy is waiting for a
/// frame that nothing schedules. In the real app such a view stays wrong
/// until something else causes a frame, for example a pointer move.
fn settle(cx: &mut HeadlessAppContext, window: AnyWindowHandle, time: Duration, lazy: bool) {
    let started = std::time::Instant::now();
    loop {
        // Updating a window that the script closed (for example a detail
        // window, with Escape) fails. The app still runs, so the loop ends
        // below, and a `window 0` line selects the main window again.
        let drawn = cx.update_window(window, |_, window, cx| {
            if lazy {
                // This does what the platform's frame callback does: it
                // runs the callbacks that asked for the next frame, then
                // draws the window if anything invalidated it.
                let asked = window.simulate_next_frame(cx);
                if window.is_dirty() {
                    if std::env::var_os("PICTO_GPUI_LAZY_DEBUG").is_some() {
                        eprintln!("lazy draw (asked {asked})");
                    }
                    window.draw(cx).clear(cx);
                }
            } else {
                window.refresh();
                let started = std::time::Instant::now();
                window.draw(cx).clear(cx);
                DRAWS.with(|draws| draws.borrow_mut().push(started.elapsed()));
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
    // The app opens the copy that `PICTO_GPUI_COPY` names. When that is not
    // set and `PICTO_GPUI_TEST_LIBRARIES` is, the app starts as a launch
    // with no library named, and the script opens the generated libraries
    // in that folder (`libraries::test_folder`).
    let root = match std::env::var_os("PICTO_GPUI_COPY") {
        Some(copy) => Some(crate::application::guard(std::path::Path::new(&copy)).unwrap()),
        None if std::env::var_os("PICTO_GPUI_TEST_LIBRARIES").is_some() => None,
        None => panic!("set PICTO_GPUI_COPY"),
    };
    let script = std::fs::read_to_string(variable("PICTO_GPUI_SCRIPT")).unwrap();
    let out = variable("PICTO_GPUI_OUT");
    std::fs::create_dir_all(&out).unwrap();
    let (width, height) = crate::seams::exact_window().unwrap_or((1213., 865.));

    // Installs the panic hook, as the app does at launch. It shows the
    // panics of earlier sessions in the log window (read from `logs/` in
    // the `PICTO_GPUI_STATE` folder) and records every panic from now on.
    crate::diagnostics::crash::install();
    let mut cx =
        HeadlessAppContext::with_platform(text_system(), Arc::new(crate::assets::Assets), renderer);
    cx.allow_parking();
    cx.update(crate::prepare);
    let window = cx
        .open_window(size(px(width), px(height)), |_, cx| {
            cx.new(crate::libraries::host::Main::new)
        })
        .unwrap();
    let main: AnyWindowHandle = window.into();
    let (incoming, _arrived) = futures::channel::mpsc::unbounded();
    cx.update(|cx| {
        let launch = match root {
            Some(root) => crate::libraries::host::Launch::Copy(root, None),
            // This is a launch with no library named. It opens the library
            // that was open last, if there is one.
            None => crate::libraries::host::Launch::Last,
        };
        crate::libraries::host::boot(window, incoming, launch, cx);
    });
    // Asks for the window decorations that the app's main window asks for.
    // On Linux they are client-side decorations, so the app draws the
    // window's buttons and edges, and the pictures show them.
    if let Some(decorations) = crate::ui::window_chrome::DECORATIONS {
        cx.update_window(main, |_, window, _| window.request_decorations(decorations))
            .unwrap();
    }
    // Makes the window the active one, as the app's window on screen is.
    // Only in an active window does a change of focus report a blur to the
    // view that loses focus.
    cx.update_window(main, |_, window, _| window.activate_window())
        .unwrap();
    let mut window = main;
    let mut pace = Duration::from_millis(200);
    let mut lazy = std::env::var_os("PICTO_GPUI_LAZY").is_some_and(|value| value != "0");
    // Waits 3 seconds for the library to open and for the first page and
    // its thumbnails to arrive.
    settle(&mut cx, window, Duration::from_secs(3), lazy);
    for line in script.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix("shot ") {
            let picture = cx.capture_screenshot(window).unwrap();
            picture
                .save(out.join(format!("{}.png", name.trim())))
                .unwrap();
        } else if let Some(index) = line.strip_prefix("window ") {
            let index: usize = index.trim().parse().expect("window <n>");
            // Sorts the main window first and the other windows in the
            // order they opened.
            let mut windows = cx.update(|cx| cx.windows());
            windows.sort_by_key(|handle| (*handle != main, handle.window_id().as_u64()));
            window = *windows
                .get(index)
                .unwrap_or_else(|| panic!("there are {} windows", windows.len()));
            settle(&mut cx, window, Duration::from_millis(200), lazy);
        } else if line == "focused" {
            // Prints the name of the control that has keyboard focus.
            let focused = cx
                .update_window(window, |_, window, cx| {
                    crate::ui::focus::names::focused(window, cx)
                })
                .unwrap();
            println!("focused: {focused}");
        } else if let Some(name) = line.strip_prefix("a11y ") {
            // Writes the window's accessibility tree, as it is reported to
            // assistive technology, as indented text. The window builds the
            // tree from this line on, as it does while a screen reader is
            // attached.
            cx.update_window(window, |_, window, _| window.set_a11y_active_for_test(true))
                .unwrap();
            settle(&mut cx, window, Duration::from_millis(100), false);
            let (update, unexposed) = cx
                .update_window(window, |_, window, _| {
                    (
                        window.last_a11y_tree_update(),
                        window.a11y_unexposed_clickables(),
                    )
                })
                .unwrap();
            let tree = crate::ui::a11y::tree::Tree::new(update.expect("a tree"));
            let mut text = tree.dump();
            if std::env::var_os("PICTO_GPUI_STATS").is_some() {
                eprintln!("a11y {}: {} nodes", name.trim(), tree.all().len());
            }
            for (at, bounds) in unexposed {
                text.push_str(&format!("unexposed click: {at} {bounds:?}\n"));
            }
            std::fs::write(out.join(format!("{}.a11y.txt", name.trim())), text).unwrap();
        } else if line == "lazy" || line == "eager" {
            lazy = line == "lazy";
        } else if let Some(rest) = line.strip_prefix("notify ") {
            notify_line(&mut cx, rest);
        } else if line == "update dialog" {
            cx.update(|cx| crate::updates::show_dialog(cx));
            settle(&mut cx, window, pace, lazy);
        } else if let Some(rest) = line.strip_prefix("update ") {
            // Sets the update state that a check for updates would reach,
            // so that a picture can be taken of it.
            let (status, platform) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
            cx.update(|cx| crate::updates::show_for_pictures(status, platform.trim(), cx));
            settle(&mut cx, window, pace, lazy);
        } else if let Some(name) = line.strip_prefix("action ") {
            // Dispatches the action of an application menu row to the
            // control that has keyboard focus, as AppKit does when the row
            // is chosen.
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
            // Sends an application command and prints its answer. The
            // script continues only after the answer has arrived. The test
            // fails when no answer arrives within 30 seconds.
            let (command, args) = rest.trim().split_once(' ').unwrap_or((rest.trim(), "{}"));
            let args: serde_json::Value =
                serde_json::from_str(args).unwrap_or_else(|error| panic!("{line}: {error}"));
            let application = cx
                .update(|cx| crate::libraries::host::application(cx))
                .expect("a library is open");
            let mut answer = application.dispatch(command.to_owned(), args);
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
    // Writes the app's state to its file, as the app does when it quits, so
    // that a second run with the same `PICTO_GPUI_STATE` starts from it.
    cx.update(crate::state::flush);
    if std::env::var_os("PICTO_GPUI_STATS").is_some() {
        // Prints the number of eager frames and the average, median, 95th
        // percentile and longest time that one took to draw. A frame's time
        // covers layout, prepaint (with the accessibility tree, when it is
        // built) and paint.
        DRAWS.with(|draws| {
            let mut draws = draws.borrow().clone();
            draws.sort();
            if let (Some(max), false) = (draws.last(), draws.is_empty()) {
                let total: Duration = draws.iter().sum();
                eprintln!(
                    "offscreen draws={} avg={:.2}ms median={:.2}ms p95={:.2}ms max={:.2}ms",
                    draws.len(),
                    total.as_secs_f64() * 1000. / draws.len() as f64,
                    draws[draws.len() / 2].as_secs_f64() * 1000.,
                    draws[draws.len() * 95 / 100].as_secs_f64() * 1000.,
                    max.as_secs_f64() * 1000.,
                );
            }
        });
    }
    cx.update(crate::libraries::host::close_for_quit);
}

/// Runs a `notify <tone> <title> | <message>` line: shows the notification
/// as the app would show it, so that a picture can be taken of it. A `+`
/// after the tone adds a Redo button that does nothing when pressed.
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
