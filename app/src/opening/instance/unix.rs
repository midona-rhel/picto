//! The transport between a launch and an instance on macOS and Linux: a Unix
//! socket in a folder that only the user can reach.
use super::Request;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Returns the folder for the lock files and the sockets, and creates it if
/// it does not exist. The folder is `$XDG_RUNTIME_DIR/picto-gpui` where the
/// system sets that variable (Linux). Otherwise it is
/// `picto-gpui-instances-<uid>` in the temporary folder, which on macOS
/// belongs to the user. The folder is created with access for the user
/// only. Returns an error for a folder that another user owns or that other
/// users can reach.
pub fn folder() -> std::io::Result<PathBuf> {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let folder = match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(runtime) => PathBuf::from(runtime).join("picto-gpui"),
        None => std::env::temp_dir().join(format!("picto-gpui-instances-{uid}")),
    };
    match std::fs::DirBuilder::new().mode(0o700).create(&folder) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = std::fs::symlink_metadata(&folder)?;
    if !meta.is_dir() || meta.uid() != uid || meta.permissions().mode() & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not a private folder of this user", folder.display()),
        ));
    }
    Ok(folder)
}

/// Returns the device and the inode of the copy's folder, as bytes.
pub fn identity(copy: &Path) -> std::io::Result<Vec<u8>> {
    let meta = std::fs::metadata(copy)?;
    let mut identity = meta.dev().to_le_bytes().to_vec();
    identity.extend(meta.ino().to_le_bytes());
    Ok(identity)
}

fn socket(folder: &Path, key: &str) -> PathBuf {
    folder.join(format!("{key}.sock"))
}

/// Starts a thread that listens for launches on the copy and answers each
/// one on a thread of its own. The caller holds the copy's lock, so a socket
/// file that exists belongs to an instance that crashed, and it is removed
/// first.
pub fn serve(
    folder: &Path,
    key: &str,
    deliver: Arc<dyn Fn(Request) + Send + Sync>,
) -> std::io::Result<()> {
    let path = socket(folder, key);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let deliver = deliver.clone();
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(super::EXCHANGE));
                    let _ = stream.set_write_timeout(Some(super::EXCHANGE));
                    super::answer(stream, &*deliver);
                });
            }
        })?;
    Ok(())
}

pub fn connect(folder: &Path, key: &str) -> std::io::Result<UnixStream> {
    let stream = UnixStream::connect(socket(folder, key))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    Ok(stream)
}

/// Does nothing. On macOS and Linux a process needs no permission from
/// another process to bring its own window to the front.
pub fn allow_foreground(_pid: u32) {}
