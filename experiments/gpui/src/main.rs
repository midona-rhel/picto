mod assets;
mod backend;
mod cover;
mod demo;
mod grid;
mod inspector;
mod keymap;
mod motion;
mod pointer;
mod screens;
mod session;
mod settings;
mod shell;
mod stats;
mod theme;
mod ui;
mod viewer;

use gpui::*;
use shell::*;
use std::path::PathBuf;

/// The application menu. Its rows show the keys bound to their actions, so
/// it is set again whenever the shortcuts are rebound.
pub fn set_app_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Picto GPUI".into(),
            items: vec![
                MenuItem::action("Settings…", settings::OpenSettings),
                MenuItem::action("Quit", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "Media Examples".into(),
            items: [
                MenuItem::action("SVG", DemoSvg),
                MenuItem::action("Flash (Ruffle)", DemoFlash),
            ]
            .into_iter()
            .chain(viewer::document::demos::menu_items())
            .collect(),
            disabled: false,
        },
    ]);
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

/// Whether the database is a disposable copy the launcher made: its folder
/// holds the snapshot marker. The app opens nothing else, and the edits that
/// cannot be undone check it again before they run.
pub fn marked_disposable(database: &std::path::Path) -> bool {
    database
        .parent()
        .is_some_and(|folder| folder.join(".gpui-snapshot").is_file())
}

fn main() {
    #[cfg(target_os = "macos")]
    match_chromium_text_weight();
    let mut args = std::env::args().skip(1);
    let database = PathBuf::from(args.next().expect("Pass a snapshot database from run.sh"));
    let blobs = PathBuf::from(args.next().expect("Pass the source blobs directory"));
    assert!(
        marked_disposable(&database),
        "Only a marked disposable snapshot is accepted"
    );
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
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
            screens::subscriptions::bind(cx);
            ui::popover::bind(cx);
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            viewer::document::bind(cx);
            // The registry's shortcuts, at their defaults until the
            // settings are read and applied.
            keymap::install(
                settings::registry::KeyboardPreset::Us,
                &settings::registry::Overrides::new(),
                cx,
            );
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &settings::OpenSettings, cx| settings::open(cx));
            // `PICTO_GPUI_WINDOW=1213x865` opens at an exact size, so a capture
            // can be compared pixel for pixel with the Electron window's.
            let (width, height) = std::env::var("PICTO_GPUI_WINDOW")
                .ok()
                .and_then(|value| {
                    let (width, height) = value.split_once('x')?;
                    Some((width.parse().ok()?, height.parse().ok()?))
                })
                .unwrap_or((1320., 850.));
            let mut bounds = Bounds::centered(None, size(px(width), px(height)), cx);
            // A centred origin can land on a half point, which makes the
            // system round the window a point larger.
            bounds.origin = bounds.origin.map(|at| at.floor());
            // `PICTO_GPUI_BACKGROUND=1` opens the window without taking the
            // keyboard from whatever the user is working in.
            let background = std::env::var_os("PICTO_GPUI_BACKGROUND").is_some();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(size(px(1000.), px(600.))),
                        titlebar: Some(TitlebarOptions {
                            title: Some("Picto GPUI · Library snapshot".into()),
                            appears_transparent: true,
                            traffic_light_position: Some(point(px(12.), px(15.))),
                        }),
                        // The toolbar sits in the titlebar. AppKit would drag the
                        // window from anywhere in that zone, controls included, so
                        // the drag regions are declared by the shell instead.
                        app_owns_titlebar_drag: true,
                        focus: !background,
                        ..Default::default()
                    },
                    move |window, cx| cx.new(|cx| Shell::new(database, blobs, window, cx)),
                )
                .unwrap();
            pointer::start(window.into(), cx);
            if !background {
                cx.activate(true);
            }
        });
}
