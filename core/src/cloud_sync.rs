//! Cloud sync in the application core: what sync needs to know about this
//! computer, and turning sync on, opening a library from the cloud, pausing
//! and turning sync off. The sync itself is
//! `picto_library::Library::sync_with_cloud`; the worker `library-sync` in
//! `library_runtime` runs it in the background.

use std::path::Path;

use picto_library::SyncComputer;

use crate::library_application::LibraryApplication;

/// Turns sync on for the open library with a new cloud library named `name`
/// in `folder`, the folder of the Google Drive or Dropbox app. Returns the
/// id of the cloud library.
pub fn turn_on(application: &LibraryApplication, folder: &Path, name: &str) -> Result<String, String> {
    let computer = this_computer().ok_or("this computer has no machine id")?;
    application.library().turn_on_sync(folder, name, &computer).map_err(|error| error.to_string())
}

/// Creates a new library in the folder `root` and opens the cloud library
/// `library_id` in `folder` in it: the library reads the cloud library's
/// snapshots and change files and gets everything they hold, except the
/// original files.
pub fn open_from_cloud(root: &Path, folder: &Path, library_id: &str) -> Result<LibraryApplication, String> {
    let computer = this_computer().ok_or("this computer has no machine id")?;
    let application = LibraryApplication::create(root)?;
    application.library().open_from_cloud(folder, library_id, &computer).map_err(|error| error.to_string())?;
    Ok(application)
}

/// Pauses or resumes sync: while it is paused, the library records its
/// changes but reads and writes nothing in the cloud folder.
pub fn set_paused(application: &LibraryApplication, paused: bool) -> Result<(), String> {
    application.library().set_sync_paused(paused).map_err(|error| error.to_string())
}

/// Turns sync off: the sync file is deleted, and the cloud library stays in
/// the cloud folder until the user deletes it.
pub fn turn_off(application: &LibraryApplication) -> Result<(), String> {
    application.library().stop_sync_recording().map_err(|error| error.to_string())
}

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
