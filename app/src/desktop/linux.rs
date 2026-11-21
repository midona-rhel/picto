use super::Application;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Returns the sender through which the badge's counts go to a thread that
/// holds the connection to the D-Bus session bus. The connection is made on
/// that thread, because connecting can take a long time or fail and must not
/// delay the drawing of a frame. Returns `None` if the thread could not be
/// started.
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

/// Puts the files on the clipboard as a `text/uri-list`. GPUI's clipboard
/// holds only text and images, so the list is given to the desktop's own
/// clipboard tool, which offers it to other applications: `wl-copy` under
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
    // Once the tool has read the list, it offers the list to other
    // applications from a background process, and the process started here
    // ends. This thread waits for that end, so that the ended process does not
    // stay in the process table.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    match written {
        Some(Ok(())) => Ok(()),
        _ => Err("Could not write files to the system clipboard".into()),
    }
}

/// Returns no applications: the app has no list of applications on Linux.
pub fn applications(_file: &Path) -> Vec<Application> {
    Vec::new()
}

/// Returns the path of the AppImage that this process runs from. The
/// AppImage's runtime sets it in the environment variable `APPIMAGE`. The path
/// is returned only if it is absolute and contains no NUL byte.
pub fn app_image() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPIMAGE")?);
    (path.is_absolute() && !path.as_os_str().as_encoded_bytes().contains(&0)).then_some(path)
}

/// Returns whether a file name contains a version: three numbers with a dot
/// between them, such as `0.7.1`.
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

/// Replaces the running AppImage with the downloaded one and starts the new
/// one. The updater of an Electron-based Picto (version 0.6.19 or earlier)
/// also performs these steps.
///
/// The downloaded AppImage is made executable, and the running one (the path
/// in `APPIMAGE`) is removed. The downloaded one is moved with `mv -f` to the
/// removed one's path, if the two have the same file name or the removed one's
/// name contains no version. Otherwise it is moved into the same folder under
/// its own name.
///
/// The new AppImage is then started with `APPIMAGE_SILENT_INSTALL=true`. If
/// `run_after` is set, it is started as a detached process, so that the app
/// runs again. If `run_after` is not set, which is the case when the app quits
/// while an update waits to be installed, it is started with
/// `APPIMAGE_EXIT_AFTER_INSTALL=true` as well, and this function waits until
/// it has ended.
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
    // The downloaded file is made executable, because it is started below.
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
        // The new AppImage gets a session of its own and none of this
        // process's standard streams, so it keeps running after this process
        // has ended.
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
        // This call waits until the new AppImage has ended, and an exit status
        // other than success is returned as an error.
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
