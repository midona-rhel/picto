// A release build on Windows is a program without a console. Without this
// attribute Windows opens a console window beside the app every time the app
// is started from the Start menu or from Explorer. A debug build keeps the
// console, where its messages are read. Text the program prints still
// reaches a caller that reads the program's output, which is how the
// packaging workflow reads what `--version` prints.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ai_tagger;
mod app_menu;
mod application;
mod assets;
mod atlas;
mod backend;
mod cover;
mod cloud;
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
mod net;
#[cfg(debug_assertions)]
mod net_check;
// The off-screen tool draws the whole app without a screen and saves
// pictures of it. Only a test build holds it.
#[cfg(test)]
mod offscreen;
mod opening;
mod platform;
// This module plays recorded input into the app's windows. It is a test
// seam, and a release build does not hold it (`seams.rs`).
#[cfg(any(test, debug_assertions))]
mod pointer;
mod prompt;
mod screens;
mod search_by_image;
mod seams;
mod session;
mod settings;
mod shell;
mod site_icons;
mod state;
mod stats;
mod tags;
mod text;
mod theme;
mod ui;
mod updates;
mod viewer;
mod zoom;

use gpui::*;
use shell::*;

/// Returns the current time in milliseconds since 1970, which is how the
/// library stamps its records.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Sets the application menu (`app_menu`). The menu's rows show the keys
/// that are bound to their actions, so this is called again whenever the
/// user changes a shortcut.
pub fn set_app_menus(cx: &mut App) {
    app_menu::set_menus(cx);
}

/// Sets the app's own `AppleFontSmoothing` preference to 0. Unless this
/// preference is 0, GPUI draws light text on a dark background with thicker
/// strokes than the font has, so it is set before any text is drawn.
#[cfg(target_os = "macos")]
fn turn_off_font_smoothing() {
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

/// Sets up what the app needs before its window opens: the bundled fonts,
/// the theme, and the key bindings of every component.
fn prepare(cx: &mut App) {
    // The state the user left last time is read first, because the keymap
    // and the views start from it.
    state::init(cx);
    cx.text_system()
        .add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/Roboto.ttf")),
            // Roboto is a variable font, and a variable font that is loaded
            // from memory is available at its default weight only. The two
            // files below are the medium and semi-bold weights cut from it.
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
    // Quit and Settings… work in every window of the app, as their menu
    // rows do (in the menu bar or in the window's application menu).
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &settings::OpenSettings, cx| settings::open(cx));
    app_menu::bind(cx);
    imports::bind(cx);
    viewer::document::bind(cx);
    viewer::video::bind(cx);
    zoom::bind(cx);
    // The shortcuts of the registry are installed with the keyboard preset
    // and with the keys that the user changed.
    let (preset, overrides) = settings::keyboard(cx);
    keymap::install(preset, &overrides, cx);
}

/// Returns where the main window opens. With `PICTO_GPUI_WINDOW=1213x865`
/// the window is centred at exactly that size, so that every picture taken
/// of it has the same size. Otherwise the window opens where it was last, if
/// that place is on a display that is still connected, and else centred at
/// the default size.
fn main_window_bounds(cx: &App) -> WindowBounds {
    let exact = seams::exact_window();
    let centred = |width: f32, height: f32| {
        let mut bounds = Bounds::centered(None, size(px(width), px(height)), cx);
        // The origin of a centred window can be at a half point, and the
        // system then makes the window one point larger. Rounding the origin
        // down prevents that.
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

/// Saves the main window's normal bounds and whether it is maximised. For
/// a window in full screen, the bounds saved are the ones it returns to when
/// full screen ends, and the window opens at those bounds next time.
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

/// Returns the folder that holds this program when `ffmpeg` is in that folder
/// too, as it is in a packaged app (`package.sh` puts `ffmpeg` and `ffprobe`
/// beside the program). Returns `None` for a program without `ffmpeg` beside
/// it, such as one run from Cargo's build folder.
fn folder_with_bundled_ffmpeg() -> Option<std::path::PathBuf> {
    let program = std::env::current_exe().ok()?;
    let folder = program.parent()?;
    let ffmpeg = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    folder.join(ffmpeg).is_file().then(|| folder.to_path_buf())
}

/// Handles `--version` as the only argument: prints the app's name, its
/// version and the folder it takes `ffmpeg` from, and returns `true`. The
/// packaging workflow runs the packaged program with this argument to prove
/// that it starts. Nothing else of the app has run at this point: no state
/// folder, no keychain, no library and no application core has been touched.
fn printed_version(bundled_ffmpeg: Option<&std::path::Path>) -> bool {
    let mut arguments = std::env::args_os().skip(1);
    if !(arguments.next().is_some_and(|first| first == "--version") && arguments.next().is_none()) {
        return false;
    }
    println!("{} {}", desktop::APP_NAME, updates::VERSION);
    match std::env::var_os("PICTO_FFMPEG_DIR") {
        Some(folder) => println!(
            "ffmpeg: {} (PICTO_FFMPEG_DIR)",
            std::path::Path::new(&folder).display()
        ),
        None => match bundled_ffmpeg {
            Some(folder) => println!("ffmpeg: {} (beside the program)", folder.display()),
            None => println!("ffmpeg: not beside the program"),
        },
    }
    true
}

/// Ends this process with exit code 1 when another Picto is running
/// (`libraries::platform::picto_process`, which leaves this process out).
/// When the list of processes cannot be read, the app starts; a library the
/// user keeps is then still refused (`libraries::admission`).
///
/// An update on Linux starts the new AppImage while the old app is still
/// quitting, and sets `APPIMAGE_SILENT_INSTALL` for it
/// (`desktop::install_update`; the updater of Picto 0.6.19 and earlier, which
/// electron-updater provided, does the same). A launch with that variable
/// waits up to 30 seconds for the old app to end before it gives up.
fn exit_if_another_picto_runs() {
    let patience = if std::env::var_os("APPIMAGE_SILENT_INSTALL").is_some() {
        std::time::Duration::from_secs(30)
    } else {
        std::time::Duration::ZERO
    };
    let started = std::time::Instant::now();
    while let Ok(Some(process)) = libraries::platform::picto_process() {
        if started.elapsed() >= patience {
            eprintln!("picto: Picto is already running ({process}), so this one does not start.");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn main() {
    let bundled_ffmpeg = folder_with_bundled_ffmpeg();
    if printed_version(bundled_ffmpeg.as_deref()) {
        return;
    }
    // The application core and the source providers look for `ffmpeg` and
    // `ffprobe` in the folder `PICTO_FFMPEG_DIR` names before they look
    // anywhere else. A packaged app has both programs beside itself, so it
    // names its own folder. A value the user set is kept.
    if let Some(folder) = &bundled_ffmpeg
        && std::env::var_os("PICTO_FFMPEG_DIR").is_none()
    {
        // SAFETY: nothing else runs yet, so no thread reads the environment
        // while it changes.
        unsafe { std::env::set_var("PICTO_FFMPEG_DIR", folder) };
    }
    // Only one Picto runs at a time. A release build that finds another
    // Picto running does not start. That includes Picto 0.6.19 and earlier,
    // whose program has the same name. A debug build does not look: several
    // debug builds run at once on purpose, each on its own disposable copy.
    if !cfg!(debug_assertions) {
        exit_if_another_picto_runs();
    }
    // Only a release build keeps logins in the system keychain. Every other
    // build keeps them in memory (`logins::uses_keychain`).
    if !logins::uses_keychain() {
        logins::keep_credentials_in_process();
    }
    diagnostics::install_logger();
    #[cfg(target_os = "macos")]
    turn_off_font_smoothing();
    let working_directory = std::env::current_dir().unwrap_or_default();
    let mut arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // With `--allow-real-libraries` as the first argument, a debug or test
    // build may open libraries the user keeps (`libraries::admission`).
    let flag = arguments
        .first()
        .is_some_and(|first| first == libraries::admission::FLAG);
    if flag {
        arguments.remove(0);
    }
    libraries::set_flag(flag);
    // The first argument is the root of a disposable copy that `run.sh` or
    // `--make-copy` made. The files after it are opened (`opening`), and so
    // are the files a file manager starts the app with when there is no
    // copy. With no argument the app opens the library it had open last, or
    // shows the gate.
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
    // This channel carries what the app is asked to open. The requests come
    // from this launch, from later launches on the same library, and from
    // the system (`application:openURLs:` on macOS).
    let (incoming, arrived) = futures::channel::mpsc::unbounded::<opening::Incoming>();
    let (initial, rest) = match launch {
        None => (libraries::host::Launch::Last, Vec::new()),
        // `--make-copy <library>` makes a disposable copy of the library,
        // prints the copy's path and stops. The copy is then opened by
        // passing that path. The library itself is only read.
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
        Some(opening::Launch::Open { copy, rest }) => {
            let request = opening::instance::Request {
                cwd: working_directory.clone(),
                args: rest.clone(),
            };
            let instance = copy_instance(&copy, &request, &incoming);
            (
                libraries::host::Launch::Copy(copy, Some(std::sync::Arc::new(instance))),
                rest,
            )
        }
        // A library the user keeps was named on the command line together
        // with the flag. It is opened if the rules allow it, after the user
        // has agreed.
        Some(opening::Launch::HandOver { files, .. })
            if flag && files.len() == 1 && files[0].to_lowercase().ends_with(".library") =>
        {
            (
                libraries::host::Launch::Named(working_directory.join(&files[0])),
                Vec::new(),
            )
        }
        // Files and no copy, in a release build: a file manager started
        // the app with a file. No other Picto runs (that was checked above),
        // so this launch starts as one without arguments does and takes the
        // files itself.
        Some(opening::Launch::HandOver { files, .. }) if !cfg!(debug_assertions) => {
            (libraries::host::Launch::Last, files)
        }
        // Files and no copy, in a debug build: they go to the instance most
        // recently active.
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

/// Makes this process the one instance for `copy`, and returns the
/// instance. When another process already has the copy open, this launch
/// hands its files to that process, whose window comes forward, and this
/// process ends.
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

/// Runs the app. It opens the main window and shows in it the library that
/// `initial` names, or the gate when there is none.
fn run(
    initial: libraries::host::Launch,
    rest: Vec<String>,
    working_directory: std::path::PathBuf,
    incoming: futures::channel::mpsc::UnboundedSender<opening::Incoming>,
    mut arrived: futures::channel::mpsc::UnboundedReceiver<opening::Incoming>,
) {
    let _ = incoming.unbounded_send(opening::Incoming {
        entries: opening::from_arguments(&rest, &working_directory),
        forward: false,
    });
    // From here on a panic is recorded for the log window. The panics of
    // the previous session are shown there too.
    diagnostics::crash::install();
    #[cfg(target_os = "macos")]
    platform::macos::one_full_screen_row();
    let app = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon shows the main window again after Close
    // Window has hidden it.
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
        // The Dock badge and the system notifications are switched on here,
        // which only the app itself does and the off-screen tool does not.
        desktop::enable(cx);
        // On Windows and Linux the system lists the app's notifications
        // under this id and name. macOS takes both from the app's bundle.
        if !cfg!(target_os = "macos") {
            cx.set_app_identity(desktop::APP_ID, desktop::APP_NAME);
        }
        app_menu::quit_with_main_window(cx);
        let window_bounds = main_window_bounds(cx);
        // With `PICTO_GPUI_BACKGROUND=1` the window opens without taking
        // the keyboard from the application the user is working in, and it
        // keeps drawing while another window covers it.
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
                    // The toolbar is in the titlebar. AppKit would let the
                    // user drag the window from any point of the titlebar,
                    // including the toolbar's controls, so the shell says
                    // itself which parts drag the window.
                    ..ui::window_chrome::frameless(desktop::APP_NAME, Some(point(px(12.), px(15.))))
                },
                move |window, cx| {
                    covered::keep_painting(window);
                    cx.new(|cx| {
                        // The window's place and size are saved whenever
                        // they change.
                        cx.observe_window_bounds(window, |_, window, cx| {
                            remember_window(window, cx)
                        })
                        .detach();
                        // When the window becomes active, this instance
                        // is noted as the one active last, which a debug
                        // launch with files and no copy hands its files to.
                        // The app also looks again for another running
                        // Picto while a library the user keeps is open.
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
        libraries::host::boot(window, incoming, initial, cx);
        // Before the process ends, the workers are stopped and the library
        // is checkpointed. This is registered after the window opened, so
        // that the session's own quit handler, which saves a pending view
        // change, runs first.
        cx.on_app_quit(move |cx| {
            state::flush(cx);
            libraries::host::close_for_quit(cx);
            async {}
        })
        .detach();
        // Only a debug build plays recorded input (`seams.rs`).
        #[cfg(debug_assertions)]
        pointer::start(window.into(), cx);
        // Only a debug build runs the check of the sign-in window
        // (`seams.rs`).
        #[cfg(debug_assertions)]
        logins::check::start(cx);
        // This shows the release notes of a version that is started for the
        // first time, and starts the automatic update checks.
        updates::start(cx);
        // Only a debug build runs the check of the network features
        // (`seams.rs`).
        #[cfg(debug_assertions)]
        net_check::start(cx);
        // The files the app was asked to open are opened once the window
        // exists.
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
