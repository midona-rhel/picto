use super::Application;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// The badge's counts go to one thread that holds the session bus, so
/// connecting (which can take a while, or fail) never holds up a frame.
fn launcher() -> Option<&'static mpsc::Sender<u64>> {
    static LAUNCHER: std::sync::OnceLock<Option<mpsc::Sender<u64>>> = std::sync::OnceLock::new();
    LAUNCHER
        .get_or_init(|| {
            let (send, counts) = mpsc::channel::<u64>();
            std::thread::Builder::new()
                .name("launcher-badge".into())
                .spawn(move || {
                    let Ok(bus) = zbus::blocking::Connection::session() else {
                        return;
                    };
                    for count in counts {
                        let mut properties = std::collections::HashMap::new();
                        properties.insert("count", zbus::zvariant::Value::from(count as i64));
                        properties.insert("count-visible", zbus::zvariant::Value::from(count > 0));
                        let _ = bus.emit_signal(
                            None::<&str>,
                            super::LAUNCHER_PATH,
                            "com.canonical.Unity.LauncherEntry",
                            "Update",
                            &(super::launcher_uri(), properties),
                        );
                    }
                })
                .ok()?;
            Some(send)
        })
        .as_ref()
}

pub fn set_badge(count: u64) {
    if let Some(launcher) = launcher() {
        let _ = launcher.send(count);
    }
}

pub fn notify(title: &str, body: &str, cx: &gpui::App) {
    super::system_notification(title, body, cx);
}

/// `writeClipboardFilePaths` on Linux: a `text/uri-list` of the files.
/// GPUI's clipboard holds text and images only, so the desktop's own
/// clipboard tool takes the list and serves it: `wl-copy` under
/// Wayland, `xclip` under X11.
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    use std::io::Write as _;
    let list = super::uri_list(paths);
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let (program, arguments): (&str, &[&str]) = if wayland {
        ("wl-copy", &["--type", "text/uri-list"])
    } else {
        (
            "xclip",
            &["-selection", "clipboard", "-t", "text/uri-list", "-i"],
        )
    };
    let mut child = std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| {
            format!("Copying files needs {program}, which was not found on this system.")
        })?;
    let written = child
        .stdin
        .take()
        .map(|mut input| input.write_all(list.as_bytes()));
    // The tool keeps serving the clipboard in the background once it has
    // read the list; the process started here ends soon after.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    match written {
        Some(Ok(())) => Ok(()),
        _ => Err("Could not write files to the system clipboard".into()),
    }
}

/// Production offers no application list on Linux.
pub fn applications(_file: &Path) -> Vec<Application> {
    Vec::new()
}
