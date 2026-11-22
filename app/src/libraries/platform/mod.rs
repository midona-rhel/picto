//! What the libraries code needs from the system, with one file per system.
//! The first thing is whether another Picto is running: a packaged release
//! build of this app, or an Electron-based Picto (version 0.6.19 or
//! earlier), which holds no lock that this app can see. The second thing is
//! a text that identifies a database file on disk. The third thing is the
//! icon that a library's folder shows in the file manager, which only
//! Windows needs set by the app.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{file_identity, picto_process};
#[cfg(target_os = "macos")]
pub use macos::{file_identity, picto_process};
#[cfg(windows)]
pub use windows::{file_identity, mark_library_folder, picto_process};

/// Gives the folder of the library at `root` Picto's library icon in the
/// file manager. Only Windows needs this. On macOS the Finder shows a
/// `.library` folder with the icon that the app's `Info.plist` declares, and
/// the Linux file managers have no icon for a single folder that an app
/// could set in the same way. So this does nothing on those two systems.
#[cfg(not(windows))]
pub fn mark_library_folder(_root: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// Returns the file's device, its inode and the time it was created, as one
/// text. All three stay the same while the file is renamed or its folder is
/// moved on the same volume, and the text changes when the file is replaced.
/// The creation time is needed because some file systems (ext4 is one) give
/// the inode of a deleted file to the next file that is created. Where the
/// system does not record a creation time, the text holds 0 in its place.
#[cfg(unix)]
fn unix_identity(path: &std::path::Path) -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::metadata(path)?;
    let made = meta
        .created()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    Ok(format!("{}:{}:{made}", meta.dev(), meta.ino()))
}

#[cfg(test)]
mod tests {
    use super::file_identity;
    use std::path::{Path, PathBuf};

    fn database_in(library: &Path) -> PathBuf {
        std::fs::create_dir_all(library).unwrap();
        let database = library.join("library.sqlite");
        std::fs::write(&database, b"a database").unwrap();
        database
    }

    /// Returns whether the file system records the time a file was created.
    /// Without that time a new file can be told from the file it replaced
    /// only by its inode, and the new file may have the old file's inode.
    fn keeps_creation_time(file: &Path) -> bool {
        std::fs::metadata(file).is_ok_and(|meta| meta.created().is_ok())
    }

    #[test]
    fn a_moved_library_keeps_its_identity_and_a_replaced_one_gets_another() {
        let folder = tempfile::tempdir().unwrap();
        let library = folder.path().join("Kept.library");
        let first = file_identity(&database_in(&library)).unwrap();
        assert_eq!(
            file_identity(&library.join("library.sqlite")).unwrap(),
            first
        );

        let moved = folder.path().join("Moved.library");
        std::fs::rename(&library, &moved).unwrap();
        assert_eq!(file_identity(&moved.join("library.sqlite")).unwrap(), first);

        // A second library is made at the path of the first. File systems
        // round a file's creation time to the tick of the clock (up to
        // 10 ms on Linux), and the new file may get the old file's inode.
        // The test waits 50 ms first, so the two creation times differ.
        std::fs::remove_dir_all(&moved).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let replacement = database_in(&library);
        if keeps_creation_time(&replacement) {
            assert_ne!(file_identity(&replacement).unwrap(), first);
        }
    }
}
