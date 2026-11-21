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
        .map(|mut input| input.write_all(&list));
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

/// `$APPIMAGE`: the AppImage this process runs from, as its runtime sets
/// it; only an absolute path without NUL is taken (`AppImageUpdater`).
pub fn app_image() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPIMAGE")?);
    (path.is_absolute() && !path.as_os_str().as_encoded_bytes().contains(&0)).then_some(path)
}

/// Whether a file name holds a version (`/\d+\.\d+\.\d+/`).
fn has_version(name: &str) -> bool {
    let bytes = name.as_bytes();
    (0..bytes.len()).any(|start| {
        let mut at = start;
        for part in 0..3 {
            let digits = bytes[at..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits == 0 {
                return false;
            }
            at += digits;
            if part < 2 {
                if bytes.get(at) != Some(&b'.') {
                    return false;
                }
                at += 1;
            }
        }
        true
    })
}

/// `AppImageUpdater.doInstall`: the running AppImage (`$APPIMAGE`) removed
/// and the new one, made executable, moved into its place (`mv -f`), under
/// the old name when that had no version or is the new one's, else under
/// the new one's name beside it; then started with
/// `APPIMAGE_SILENT_INSTALL=true`: detached, to run the app again
/// (`--force-run`), or, when the app quits with the update waiting, waited
/// for with `APPIMAGE_EXIT_AFTER_INSTALL=true`.
pub fn install_update(
    installer: &Path,
    _silent: bool,
    run_after: bool,
    _admin: bool,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};
    let Some(current) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        return Err("APPIMAGE env is not defined".to_owned());
    };
    if app_image().is_none() {
        return Err(format!(
            "APPIMAGE env is not a valid absolute path: \"{}\"",
            current.display()
        ));
    }
    // `chmod(updateFile, 0o755)`, which production does once downloaded.
    std::fs::set_permissions(installer, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("{}: {error}", installer.display()))?;
    std::fs::remove_file(&current).map_err(|error| format!("{}: {error}", current.display()))?;
    let existing = current
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let destination = if installer.file_name() == current.file_name() || !has_version(&existing) {
        current.clone()
    } else {
        current
            .parent()
            .unwrap_or(Path::new("/"))
            .join(installer.file_name().unwrap_or_default())
    };
    let moved = Command::new("mv")
        .arg("-f")
        .arg(installer)
        .arg(&destination)
        .status()
        .map_err(|error| format!("mv: {error}"))?;
    if !moved.success() {
        return Err(format!(
            "Command failed: mv -f {} {}",
            installer.display(),
            destination.display()
        ));
    }
    let mut command = Command::new(&destination);
    command.env("APPIMAGE_SILENT_INSTALL", "true");
    if run_after {
        // `spawn(destination, [], { detached: true, stdio: 'ignore' })`
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: `setsid` is async-signal-safe and touches no memory.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        command
            .spawn()
            .map_err(|error| format!("spawn {} {error}", destination.display()))?;
    } else {
        // `execFileSync(destination, [], { env })`
        let status = command
            .env("APPIMAGE_EXIT_AFTER_INSTALL", "true")
            .status()
            .map_err(|error| format!("spawnSync {} {error}", destination.display()))?;
        if !status.success() {
            return Err(format!("Command failed: {}", destination.display()));
        }
    }
    Ok(())
}
