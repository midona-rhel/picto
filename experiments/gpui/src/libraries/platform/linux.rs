//! Whether the Electron app runs, on Linux: every process in `/proc`, by its
//! executable, its name and its first argument, looked at by [`is_picto`].
use std::path::Path;

/// A description of the first process of the Electron app found, if any.
pub fn picto_process() -> Result<Option<String>, String> {
    let entries = std::fs::read_dir("/proc").map_err(|error| format!("/proc: {error}"))?;
    let own = std::process::id().to_string();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str() else {
            continue;
        };
        if !pid.bytes().all(|byte| byte.is_ascii_digit()) || pid == own {
            continue;
        }
        let folder = entry.path();
        // Each may be unreadable (another user's process, or one that
        // ended); what can be read is enough.
        let executable = std::fs::read_link(folder.join("exe"))
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let comm = std::fs::read_to_string(folder.join("comm")).unwrap_or_default();
        let first = std::fs::read(folder.join("cmdline"))
            .ok()
            .and_then(|line| {
                line.split(|byte| *byte == 0)
                    .next()
                    .map(|first| String::from_utf8_lossy(first).into_owned())
            })
            .unwrap_or_default();
        if is_picto(&executable, comm.trim_end(), &first) {
            return Ok(Some(format!("{pid} {executable}")));
        }
    }
    Ok(None)
}

/// Whether a process is the Electron app's: an executable named `picto`
/// (electron-builder names it after the package, as an AppImage's mounted
/// binary and a packaged install are), a process named `picto`, or one
/// started from a `Picto…AppImage`. This app (`picto-gpui-spike`, named
/// `picto-gpui-spik` by the kernel) is not.
pub fn is_picto(executable: &str, comm: &str, first_argument: &str) -> bool {
    let base = |path: &str| {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    };
    let first = base(first_argument);
    base(executable) == "picto"
        || comm == "picto"
        || first == "picto"
                // i18n: a process's name
        || (first.starts_with("Picto") && first.to_lowercase().ends_with(".appimage"))
}

/// The database file's identity: its device and inode.
pub fn file_identity(path: &Path) -> std::io::Result<String> {
    super::unix_identity(path)
}

#[cfg(test)]
mod tests {
    use super::is_picto;

    #[test]
    fn the_electron_app_is_known_by_its_executable_name_or_image() {
        assert!(is_picto(
            "/tmp/.mount_PictoX/picto",
            "picto",
            "/tmp/.mount_PictoX/picto"
        ));
        assert!(is_picto("/opt/Picto/picto", "picto", "/opt/Picto/picto"));
        assert!(is_picto("", "", "/home/me/Picto-1.0.0.AppImage"));
        assert!(!is_picto(
            "/home/me/gpui/target/debug/picto-gpui-spike",
            "picto-gpui-spik",
            "target/debug/picto-gpui-spike"
        ));
        assert!(!is_picto("/usr/bin/bash", "bash", "bash"));
        assert!(super::picto_process().is_ok());
    }
}
