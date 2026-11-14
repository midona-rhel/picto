mod app_menu;
mod application;
mod assets;
mod atlas;
mod backend;
mod cover;
mod covered;
mod desktop;
mod export;
mod grid;
mod history;
mod imports;
mod inspector;
mod keymap;
mod meter;
mod motion;
// Pictures of the app drawn off-screen, for checks without a screen.
#[cfg(test)]
mod offscreen;
mod platform;
// Input playback, a test seam a release build does not hold (`seams.rs`).
#[cfg(any(test, debug_assertions))]
mod pointer;
mod prompt;
mod screens;
mod seams;
mod session;
mod settings;
mod shell;
mod state;
mod stats;
mod tags;
mod text;
mod theme;
mod ui;
mod viewer;

use gpui::*;
use shell::*;
use std::path::PathBuf;

/// Now, in milliseconds since 1970, as the library stamps its records.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The application menu (`app_menu`). Its rows show the keys bound to
/// their actions, so it is set again whenever the shortcuts are rebound.
pub fn set_app_menus(cx: &mut App) {
    app_menu::set_menus(cx);
}

/// Chromium draws Picto's text with `-webkit-font-smoothing: antialiased`,
/// which does not thicken strokes. GPUI thickens light text on dark unless the
/// app's own `AppleFontSmoothing` preference is 0, so set that before any
/// text is drawn.
#[cfg(target_os = "macos")]
fn match_chromium_text_weight() {
    use core_foundation::{base::TCFType, number::CFNumber, string::CFString};
    use core_foundation_sys::preferences::{
        CFPreferencesSetAppValue, kCFPreferencesCurrentApplication,
    };
    let key = CFString::new("AppleFontSmoothing");
    let value = CFNumber::from(0i32);
    unsafe {
        CFPreferencesSetAppValue(
            key.as_concrete_TypeRef(),
            value.as_CFTypeRef(),
            kCFPreferencesCurrentApplication,
        );
    }
}

/// What the app needs before its window opens: the bundled fonts, the
/// theme, and every component's key bindings.
fn prepare(cx: &mut App) {
    // What the user arranged last time, which the keymap and views start from.
    state::init(cx);
    cx.text_system()
        .add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/Roboto.ttf")),
            // Static weights cut from the same variable font, which
            // loads from memory at its default weight only.
            std::borrow::Cow::Borrowed(include_bytes!("../assets/Roboto-Medium.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/Roboto-SemiBold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/FiraMono.ttf")),
        ])
        .expect("load bundled inspector font");
    cx.set_global(theme::DARK);
    stats::start_reporting(cx);
    ui::text_input::bind(cx);
    ui::menu::bind(cx);
    ui::modal::bind(cx);
    ui::tag_token_input::bind(cx);
    shell::sidebar::bind(cx);
    ui::overlay_shell::bind(cx);
    ui::selection_modal::bind(cx);
    screens::subscriptions::bind(cx);
    ui::popover::bind(cx);
    ui::focus::bind(cx);
    if let Some(keys) = app_menu::Platform::CURRENT.quit_keys() {
        keymap::bind_written(keys, Box::new(Quit), None, cx);
    }
    // Quit and Settings… are app-wide, as their menu rows (in the menu bar
    // or the window's application menu) are.
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &settings::OpenSettings, cx| settings::open(cx));
    app_menu::bind(cx);
    imports::bind(cx);
    viewer::document::bind(cx);
    viewer::video::bind(cx);
    // The registry's shortcuts, with the preset and the bindings the user
    // changed.
    let (preset, overrides) = settings::keyboard(cx);
    keymap::install(preset, &overrides, cx);
}

/// Where the main window opens: `PICTO_GPUI_WINDOW=1213x865` centres it at
/// an exact size, so a capture can be compared pixel for pixel with the
/// Electron window's; otherwise where it was last, on a display that is still
/// there (`getSavedMainWindowState`), else centred at the default size.
fn main_window_bounds(cx: &App) -> WindowBounds {
    let exact = seams::exact_window();
    let centred = |width: f32, height: f32| {
        let mut bounds = Bounds::centered(None, size(px(width), px(height)), cx);
        // A centred origin can land on a half point, which makes the
        // system round the window a point larger.
        bounds.origin = bounds.origin.map(|at| at.floor());
        bounds
    };
    if let Some((width, height)) = exact {
        return WindowBounds::Windowed(centred(width, height));
    }
    let Some(saved) = state::get(cx).window.clone() else {
        return WindowBounds::Windowed(centred(1320., 850.));
    };
    let displays = cx
        .displays()
        .iter()
        .map(|display| {
            let area = display.visible_bounds();
            state::Rect {
                x: f32::from(area.origin.x),
                y: f32::from(area.origin.y),
                width: f32::from(area.size.width),
                height: f32::from(area.size.height),
            }
        })
        .collect::<Vec<_>>();
    let place = saved.on_displays(&displays);
    let bounds = match (place.x, place.y) {
        (Some(x), Some(y)) => {
            Bounds::new(point(px(x), px(y)), size(px(place.width), px(place.height)))
        }
        _ => centred(place.width, place.height),
    };
    if place.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

/// Keeps the main window's normal bounds and whether it is maximised. A
/// window in full screen keeps the bounds it will return to, as production's
/// `getNormalBounds` does, and opens at them next time.
fn remember_window(window: &Window, cx: &mut App) {
    if seams::exact_window().is_some() {
        return;
    }
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Fullscreen(bounds) => (bounds, false),
    };
    let place = state::WindowPlace {
        x: Some(f32::from(bounds.origin.x)),
        y: Some(f32::from(bounds.origin.y)),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    };
    state::update(cx, |state| state.window = Some(place));
}

fn main() {
    #[cfg(target_os = "macos")]
    match_chromium_text_weight();
    // `--make-copy <library>`: make a disposable copy of the library, print
    // where it is, and stop; the copy is then opened by passing that path.
    if std::env::args().nth(1).as_deref() == Some("--make-copy") {
        let Some(source) = std::env::args().nth(2) else {
            eprintln!("picto: --make-copy takes the library to copy");
            std::process::exit(2);
        };
        match application::make_copy(std::path::Path::new(&source)) {
            Ok(copy) => println!("{}", desktop::system_path(&copy).display()),
            Err(error) => {
                eprintln!("picto: {error}");
                std::process::exit(2);
            }
        }
        return;
    }
    // The one argument is the root of a disposable copy that run.sh or
    // `--make-copy` made. Anything else, and the library it was copied from,
    // is refused.
    let Some(root) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("picto: pass the disposable library copy that run.sh or --make-copy made");
        std::process::exit(2);
    };
    let root = match application::guard(&root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("picto: {error}");
            std::process::exit(2);
        }
    };
    // `--detach` after the copy: cut it loose from the real library's cloud
    // sync, watched folders and queued runs, and stop. For opening the copy
    // with another app (the Electron reference) that would otherwise resume
    // that work.
    if std::env::args().nth(2).as_deref() == Some("--detach") {
        if let Err(error) = application::detach(&root) {
            eprintln!("picto: {error}");
            std::process::exit(2);
        }
        return;
    }
    let (events, delivered) = std::sync::mpsc::channel();
    let application = application::Application::new(root.clone(), events);
    let closing = application.clone();
    let app = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon brings back the main window Close Window hid.
    app.on_reopen(app_menu::reopen);
    app.run(move |cx: &mut App| {
        prepare(cx);
        // The dock badge and the system notifications, for the app proper
        // only.
        desktop::enable(cx);
        // Off macOS the system files the app's notifications under this
        // identity (the Mac takes it from the bundle).
        if !cfg!(target_os = "macos") {
            cx.set_app_identity(desktop::APP_ID, desktop::APP_NAME);
        }
        app_menu::quit_with_main_window(cx);
        let window_bounds = main_window_bounds(cx);
        // `PICTO_GPUI_BACKGROUND=1` opens the window without taking the
        // keyboard from whatever the user is working in, and keeps it
        // drawing while it is covered.
        let background = seams::background();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    window_min_size: Some(size(px(1000.), px(600.))),
                    focus: !background,
                    // The toolbar sits in the titlebar. AppKit would drag the
                    // window from anywhere in that zone, controls included, so
                    // the drag regions are declared by the shell instead.
                    ..ui::window_chrome::frameless(
                        "Picto GPUI · Library copy",
                        Some(point(px(12.), px(15.))),
                    )
                },
                move |window, cx| {
                    covered::keep_painting(window);
                    cx.new(|cx| {
                        // The window's place is kept as it changes
                        // (`queueSaveMainWindowState`).
                        cx.observe_window_bounds(window, |_, window, cx| {
                            remember_window(window, cx)
                        })
                        .detach();
                        Shell::new(root, application, delivered, window, cx)
                    })
                },
            )
            .unwrap();
        // Stop the workers and checkpoint the copy before the process
        // ends, as the Electron app closes its library before quitting.
        // Registered after the window opened, so the session's own quit
        // hook (a view change still to save) runs first.
        cx.on_app_quit(move |cx| {
            state::flush(cx);
            closing.close();
            async {}
        })
        .detach();
        // Input playback is a debug build's only (`seams.rs`).
        #[cfg(debug_assertions)]
        pointer::start(window.into(), cx);
        #[cfg(not(debug_assertions))]
        let _ = window;
        if !background {
            cx.activate(true);
        }
    });
}
