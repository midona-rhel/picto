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

/// Device and inode, which stay with the file while it is renamed or its
/// folder moved on the same volume, and change when it is replaced.
#[cfg(unix)]
fn unix_identity(path: &std::path::Path) -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::metadata(path)?;
    Ok(format!("{}:{}", meta.dev(), meta.ino()))
}
