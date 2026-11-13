//! Plays a script into the whole app in an off-screen window and saves
//! pictures of it, so any view can be checked without a screen:
//!
//!     PICTO_GPUI_COPY=/tmp/…/Name.library PICTO_GPUI_SCRIPT=steps.txt \
//!     PICTO_GPUI_OUT=/tmp/shots cargo test -- --ignored plays_a_script
//!
//! The copy is one `run.sh` made. A script line is a playback line (see
//! `pointer.rs`), `wait <milliseconds>`, `shot <name>`, which saves
//! `<name>.png`, or `window <n>`, which sends what follows to the app's nth
//! window (0 is the main one; Settings and a detail window come after it in
//! the order they opened), or `pace <milliseconds>`, how long the app runs
//! after each input line (200 unless set), for pictures taken soon after
//! one. `PICTO_GPUI_WINDOW=1213x865` sets the main window's size.
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
fn settle(cx: &mut HeadlessAppContext, window: AnyWindowHandle, time: Duration) {
    let started = std::time::Instant::now();
    loop {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
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
    let mut window = main;
    let mut pace = Duration::from_millis(200);
    // The library opens and the first page and its thumbnails arrive.
    settle(&mut cx, window, Duration::from_secs(3));
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
            settle(&mut cx, window, Duration::from_millis(200));
        } else if let Some(time) = line.strip_prefix("pace ") {
            pace = Duration::from_millis(time.trim().parse().expect("pace <milliseconds>"));
        } else if let Some(time) = line.strip_prefix("wait ") {
            let time = time.trim().parse().expect("wait <milliseconds>");
            settle(&mut cx, window, Duration::from_millis(time));
        } else {
            let event = pointer::parse(line).unwrap_or_else(|| panic!("not a line: {line}"));
            cx.update_window(window, |_, window, cx| event.deliver(window, cx))
                .unwrap();
            settle(&mut cx, window, pace);
        }
    }
    closing.close();
}
