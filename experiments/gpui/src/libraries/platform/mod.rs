//! What libraries need from the system, one leaf per platform: whether the
//! Electron app is running (production's process, which holds no lock this
//! app can see), and a database file's identity on disk.
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
pub use windows::{file_identity, picto_process};

/// Device, inode and the time the file was made, which stay with the file
/// while it is renamed or its folder moved on the same volume, and change
/// when it is replaced. The time is what tells a replacement apart where
/// the file system hands a freed inode to the next file made (ext4 does);
/// where the system does not keep it, device and inode are all there is.
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

    /// Whether the file system records when a file was made. Without it a
    /// replacement can only be told by its inode, which may be the old one.
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

        // Another library made where the first was. File systems round a
        // file's creation time to the clock's tick (up to 10 ms on Linux),
        // and the new file may be handed the old one's inode, so the
        // replacement is made a few ticks later, as any real one is.
        std::fs::remove_dir_all(&moved).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let replacement = database_in(&library);
        if keeps_creation_time(&replacement) {
            assert_ne!(file_identity(&replacement).unwrap(), first);
        }
    }
}
