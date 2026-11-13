mod application;
mod assets;
mod backend;
mod cover;
mod covered;
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
            items: vec![
                MenuItem::action("SVG", DemoSvg),
                MenuItem::action("Flash (Ruffle)", DemoFlash),
            ],
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

fn main() {
    #[cfg(target_os = "macos")]
    match_chromium_text_weight();
    // The one argument is the root of a disposable copy that run.sh made.
    // Anything else, and the library it was copied from, is refused.
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("Pass the disposable library copy that run.sh made"),
    );
    let root = match application::guard(&root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("picto: {error}");
            std::process::exit(2);
        }
    };
    let (events, delivered) = std::sync::mpsc::channel();
    let application = application::Application::new(root.clone(), events);
    let closing = application.clone();
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
            ui::overlay_shell::bind(cx);
            screens::subscriptions::bind(cx);
            ui::popover::bind(cx);
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            // The registry's shortcuts, at their defaults until the
            // settings are read and applied.
            keymap::install(
                settings::registry::KeyboardPreset::Us,
                &settings::registry::Overrides::new(),
                cx,
            );
            cx.on_action(|_: &Quit, cx| cx.quit());
            // Stop the workers and checkpoint the copy before the process
            // ends, as the Electron app closes its library before quitting.
            cx.on_app_quit(move |_| {
                closing.close();
                async {}
            })
            .detach();
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
            // keyboard from whatever the user is working in, and keeps it
            // drawing while it is covered.
            let background = covered::enabled();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(size(px(1000.), px(600.))),
                        titlebar: Some(TitlebarOptions {
                            title: Some("Picto GPUI · Library copy".into()),
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
                    move |window, cx| {
                        covered::keep_painting(window);
                        cx.new(|cx| Shell::new(root, application, delivered, window, cx))
                    },
                )
                .unwrap();
            pointer::start(window.into(), cx);
            if !background {
                cx.activate(true);
            }
        });
}
