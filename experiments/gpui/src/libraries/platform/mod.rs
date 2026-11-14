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
