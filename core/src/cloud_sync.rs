//! What cloud sync needs to know about this computer. The sync itself is
//! `picto_library::Library::sync_with_cloud`; the worker `library-sync` in
//! `library_runtime` runs it in the background.

use picto_library::SyncComputer;

/// This computer: its machine id, its name and the version of Picto.
/// Returns `None` when the app has no data folder, or the machine id cannot
/// be read from it or written into it.
pub fn this_computer() -> Option<SyncComputer> {
    Some(SyncComputer {
        machine_id: machine_id()?,
        name: computer_name(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// A random id that is made once and kept in the file `machine-id` of the
/// app's data folder. The data folder belongs to one user on one computer,
/// so a library folder that is copied to another computer finds another id
/// there.
fn machine_id() -> Option<String> {
    let path = crate::state::application_data_root()?.join("machine-id");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let id = text.trim();
        if !id.is_empty() {
            return Some(id.to_owned());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, &id).ok()?;
    Some(id)
}

/// The name of the computer on the network, or `Computer` when the system
/// gives none.
fn computer_name() -> String {
    #[cfg(unix)]
    {
        let mut buffer = [0_u8; 256];
        // SAFETY: `gethostname` writes at most `buffer.len()` bytes into
        // `buffer`.
        if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0 {
            let length = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
            let name = String::from_utf8_lossy(&buffer[..length]).into_owned();
            if !name.is_empty() {
                return name;
            }
        }
    }
    #[cfg(windows)]
    if let Some(name) = std::env::var("COMPUTERNAME").ok().filter(|name| !name.is_empty()) {
        return name;
    }
    "Computer".to_owned()
}
