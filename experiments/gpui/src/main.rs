mod ai_tagger;
mod app_menu;
mod application;
mod assets;
mod atlas;
mod backend;
mod cover;
mod covered;
mod desktop;
mod diagnostics;
mod export;
mod grid;
mod history;
mod i18n;
mod imports;
mod inspector;
mod keymap;
mod libraries;
mod logins;
mod meter;
mod motion;
// Pictures of the app drawn off-screen, for checks without a screen.
#[cfg(test)]
mod offscreen;
mod opening;
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
mod zoom;

use gpui::*;
use shell::*;

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
    zoom::bind(cx);
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

/// What the main window opens first.
enum Initial {
    /// A disposable copy from the command line, with this process holding
    /// its instance.
    Copy(std::path::PathBuf, opening::instance::Instance),
    /// A library named on the command line with `--allow-real-libraries`.
    Named(std::path::PathBuf),
    /// No library named: the one this app had open last, if it may be
    /// opened, else the gate.
    Last,
}

fn main() {
    logins::keep_credentials_in_process();
    diagnostics::install_logger();
    #[cfg(target_os = "macos")]
    match_chromium_text_weight();
    let working_directory = std::env::current_dir().unwrap_or_default();
    let mut arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `--allow-real-libraries` first: a debug or test build may then open
    // libraries the user keeps (`libraries::admission`).
    let flag = arguments
        .first()
        .is_some_and(|first| first == libraries::admission::FLAG);
    if flag {
        arguments.remove(0);
    }
    libraries::set_flag(flag);
    // The first argument is the root of a disposable copy that run.sh or
    // `--make-copy` made; files after it (or, with no copy, files a file
    // manager opens with the app) are opened as production opens them
    // (`opening`). With no argument the app opens the library it had open
    // last, or shows the gate.
    let launch = if arguments.is_empty() {
        None
    } else {
        match opening::parse(arguments, application::guard) {
            Ok(launch) => Some(launch),
            Err(error) => {
                eprintln!("picto: {error}");
                std::process::exit(2);
            }
        }
    };
    // What the app is asked to open, from this launch, later launches on
    // the same library, and the system (macOS `application:openURLs:`).
    let (incoming, arrived) = futures::channel::mpsc::unbounded::<opening::Incoming>();
    let (initial, rest) = match launch {
        None => (Initial::Last, Vec::new()),
        // `--make-copy <library>`: make a disposable copy of the library,
        // print where it is, and stop; the copy is then opened by passing
        // that path. The library is only read.
        Some(opening::Launch::MakeCopy(source)) => {
            match application::make_copy(&source) {
                Ok(copy) => println!("{}", desktop::system_path(&copy).display()),
                Err(error) => {
                    eprintln!("picto: {error}");
                    std::process::exit(2);
                }
            }
            return;
        }
        // `--detach` after the copy: cut it loose from the real library's
        // cloud sync, watched folders and queued runs, and stop. For opening
        // the copy with another app (the Electron reference) that would
        // otherwise resume that work. A folder without the marker is
        // refused (`application::detach`).
        Some(opening::Launch::Open {
            copy, detach: true, ..
        }) => {
            if let Err(error) = application::detach(&copy) {
                eprintln!("picto: {error}");
                std::process::exit(2);
            }
            return;
        }
        Some(opening::Launch::Open { copy, rest, .. }) => {
            let request = opening::instance::Request {
                cwd: working_directory.clone(),
                args: rest.clone(),
            };
            let instance = copy_instance(&copy, &request, &incoming);
            (Initial::Copy(copy, instance), rest)
        }
        // A library the user keeps, named with the flag: opened if the
        // rules let it, after the user's consent.
        Some(opening::Launch::HandOver { files, .. })
            if flag && files.len() == 1 && files[0].to_lowercase().ends_with(".library") =>
        {
            (
                Initial::Named(working_directory.join(&files[0])),
                Vec::new(),
            )
        }
        // Files and no copy: they go to the instance most recently active.
        Some(opening::Launch::HandOver { files, refused }) => {
            let request = opening::instance::Request {
                cwd: working_directory,
                args: files,
            };
            match opening::instance::hand_to_latest(&request) {
                Ok(copy) => {
                    eprintln!("picto: handed to the app open on {}", copy.display());
                    return;
                }
                Err(error) => {
                    eprintln!("picto: {refused}");
                    eprintln!("picto: and the files could not be handed over: {error}");
                    std::process::exit(2);
                }
            }
        }
    };
    run(initial, rest, working_directory, incoming, arrived);
}

/// The copy's one instance: a launch on a copy already open hands its
/// files to that instance, which comes forward, and this launch ends.
fn copy_instance(
    copy: &std::path::Path,
    request: &opening::instance::Request,
    incoming: &futures::channel::mpsc::UnboundedSender<opening::Incoming>,
) -> opening::instance::Instance {
    let from_launches = incoming.clone();
    match opening::instance::start(copy, request, move |request| {
        let _ = from_launches.unbounded_send(opening::Incoming {
            entries: opening::from_arguments(&request.args, &request.cwd),
            forward: true,
        });
    }) {
        Ok(opening::instance::Started::Primary(instance)) => instance,
        Ok(opening::instance::Started::HandedOver) => {
            eprintln!(
                "picto: handed to the app already open on {}",
                copy.display()
            );
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("picto: {error}");
            std::process::exit(2);
        }
    }
}

/// Runs the app: the main window, the gate or the first library in it.
fn run(
    initial: Initial,
    rest: Vec<String>,
    working_directory: std::path::PathBuf,
    incoming: futures::channel::mpsc::UnboundedSender<opening::Incoming>,
    mut arrived: futures::channel::mpsc::UnboundedReceiver<opening::Incoming>,
) {
    let _ = incoming.unbounded_send(opening::Incoming {
        entries: opening::from_arguments(&rest, &working_directory),
        forward: false,
    });
    // Panics are recorded for the log window from here on, and those of
    // the previous session are shown there.
    diagnostics::crash::install();
    #[cfg(target_os = "macos")]
    platform::macos::one_full_screen_row();
    let app = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon brings back the main window Close Window hid.
    app.on_reopen(app_menu::reopen);
    let from_system = incoming.clone();
    app.on_open_urls(move |urls| {
        let _ = from_system.unbounded_send(opening::Incoming {
            entries: opening::from_arguments(&urls, &working_directory),
            forward: true,
        });
    });
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
                    window_min_size: Some(size(
                        px(state::WINDOW_MIN_WIDTH),
                        px(state::WINDOW_MIN_HEIGHT),
                    )),
                    focus: !background,
                    // The toolbar sits in the titlebar. AppKit would drag the
                    // window from anywhere in that zone, controls included, so
                    // the drag regions are declared by the shell instead.
                    ..ui::window_chrome::frameless(
                        i18n::en("Picto GPUI"),
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
                        // A launch with files and no copy goes to the
                        // instance whose window was active last; a library
                        // the user keeps is checked against the Electron
                        // app again.
                        cx.observe_window_activation(window, move |_, window, cx| {
                            if window.is_window_active() {
                                cx.defer(libraries::host::activated);
                            }
                        })
                        .detach();
                        libraries::host::Main::new(cx)
                    })
                },
            )
            .unwrap();
        app_menu::set_main_window(window.into(), cx);
        libraries::host::attach(window, incoming, cx);
        app_menu::set_menus(cx);
        match initial {
            Initial::Copy(root, instance) => libraries::host::start(
                root,
                libraries::Kind::Copy,
                Some(std::sync::Arc::new(instance)),
                window,
                cx,
            ),
            Initial::Named(path) => {
                libraries::host::request(&path, libraries::Origin::CommandLine { flag: true }, cx)
            }
            // Only a library this process may open is even looked at.
            Initial::Last => libraries::host::start_last(cx),
        }
        libraries::host::launched(cx);
        // Stop the workers and checkpoint the library before the process
        // ends, as the Electron app closes its library before quitting.
        // Registered after the window opened, so the session's own quit
        // hook (a view change still to save) runs first.
        cx.on_app_quit(move |cx| {
            state::flush(cx);
            libraries::host::close_for_quit(cx);
            async {}
        })
        .detach();
        // Input playback is a debug build's only (`seams.rs`).
        #[cfg(debug_assertions)]
        pointer::start(window.into(), cx);
        // The sign-in window's check, a debug build's only (`seams.rs`).
        #[cfg(debug_assertions)]
        logins::check::start(cx);
        // Opened as production's `drainAssociatedFiles` opens them, once
        // the window is up.
        cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while let Some(incoming) = arrived.next().await {
                cx.update(|cx| opening::accept(incoming, cx));
            }
        })
        .detach();
        if !background {
            cx.activate(true);
        }
    });
}
