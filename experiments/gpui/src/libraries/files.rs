//! What the Library Manager's Rename…, Move… and Delete… do to a library's
//! folder on disk. Move… and Delete… do what the Electron app's main process
//! does (`libraryHostService.mjs`, `libraryFilesystem.mjs`).
//!
//! Delete… removes the folder and everything in it (`deleteLibrary`, which
//! calls `fs.rm` recursively). The folder is not moved to the system's Trash.
//!
//! Move… puts the folder into another folder (`relocateLibrary`,
//! `moveLibraryDirectory`). On the same volume this is a rename. Across
//! volumes the folder is copied, every file of the copy is compared with its
//! original, the copy is renamed to the library's name, and only then is the
//! original removed.
//!
//! Rename… gives the folder another name in the same parent folder.
//!
//! All three act only on a folder that `ownedLibraryPath` accepts: a real
//! folder, not a link, whose name ends in `.library` and which holds a file
//! named `library.sqlite`. Delete… refuses the library that is open in this
//! app. For Rename… and Move…, the caller closes the library first if it is
//! open. None of the three acts on a library that another process has locked.
//! Each takes the core's lock on `.picto-library.lock` and keeps it until its
//! work on the folder is done, so that no other instance can open the library
//! in the middle of it ([`hold`]).
//!
//! Before these functions are called, `host::permit` has checked that this app
//! may change the library (`admission::admit`), which includes the user's
//! consent for that library. The Library Manager (`manager`) has also had the
//! user confirm a Delete…, choose the folder for a Move…, or type the new name
//! for a Rename….
use picto_core::state::{LIBRARY_LOCK_FILE, LibraryLock};
use std::path::{Path, PathBuf};

/// Returns the canonical form of `path` if it is a library folder that this
/// app may change on disk (`ownedLibraryPath` in the Electron app): a real
/// folder, not a link, whose name ends in `.library` in any letter case and
/// which holds a file named `library.sqlite`. Otherwise it returns the
/// Electron app's error message for a path that is not a library.
pub fn owned(path: &Path) -> Result<PathBuf, String> {
    let refuse = || "Refusing to modify a path that is no longer a Picto library.".to_owned();
    let meta = path.symlink_metadata().map_err(|_| refuse())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(refuse());
    }
    let root = path.canonicalize().map_err(|_| refuse())?;
    let named = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_lowercase().ends_with(".library"));
    let database = root.join("library.sqlite").symlink_metadata();
    if !named || !database.is_ok_and(|meta| meta.is_file()) {
        return Err(refuse());
    }
    Ok(root)
}

/// Refuses the library open in this app (`open`, canonical) and one that
/// another holder has locked (another instance of this app, or anything
/// else that takes the core's lock). Otherwise it takes that lock and
/// returns it. While the caller keeps the returned lock, no other instance
/// can open the library.
fn hold(root: &Path, open: Option<&Path>) -> Result<LibraryLock, String> {
    if open.is_some_and(|open| open.canonicalize().is_ok_and(|open| open == root)) {
        return Err(
            "Cannot delete the currently open library. Switch to a different library first."
                .to_owned(),
        );
    }
    picto_core::state::lock_library(root).map_err(|error| format!("{}: {error}", root.display()))
}

/// Removes the library's folder `root` and everything in it, while `lock`
/// is held. Everything except the lock file is removed first. Then the lock
/// is released and the lock file and the folder are removed. Between those
/// two steps another instance could take the lock, but the folder then
/// holds no database, so that instance cannot open a library there.
fn remove_held(root: &Path, lock: LibraryLock) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_name() == LIBRARY_LOCK_FILE {
            continue;
        }
        // `file_type` does not follow a link, so a link is never treated as
        // a folder here, and `remove_dir_all` removes the links it finds
        // inside a folder, never what they lead to. On Windows a link to a
        // folder cannot be removed as a file, so when that fails it is
        // removed as an empty folder, which removes the link and leaves
        // what it leads to.
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else if let Err(error) = std::fs::remove_file(&path) {
            std::fs::remove_dir(&path).map_err(|_| error)?;
        }
    }
    drop(lock);
    std::fs::remove_dir_all(root)
}

/// Renames the library's folder `root` to `target`. On macOS and Linux the
/// lock is held until the rename is done. On failure the error is returned
/// with the lock, if it is still held.
///
/// Windows does not rename a folder while a file inside it is open, and the
/// lock is an open file inside it, so on Windows the lock is released just
/// before the rename. An instance that has the library open keeps its
/// database file open, and Windows then refuses the rename for the same
/// reason.
fn rename_held(
    root: &Path,
    target: &Path,
    lock: LibraryLock,
) -> Result<(), (std::io::Error, Option<LibraryLock>)> {
    #[cfg(windows)]
    let lock = {
        drop(lock);
        None
    };
    #[cfg(not(windows))]
    let lock = Some(lock);
    std::fs::rename(root, target).map_err(|error| (error, lock))
}

/// Does the Library Manager's Delete…: removes the library's folder at `path`
/// and everything in it. The folder is not moved to the system's Trash. `open`
/// is the folder of the library that is open in this app, if one is open.
/// Deleting that library is refused, and so is deleting a library whose lock
/// another process holds.
pub fn delete(path: &Path, open: Option<&Path>) -> Result<(), String> {
    let root = owned(path)?;
    let lock = hold(&root, open)?;
    remove_held(&root, lock).map_err(|error| format!("{}: {error}", root.display()))
}

/// Rename…: the library's folder at `path` becomes `target`, which is in
/// the same parent folder. The open library is closed by the caller first.
pub fn rename_to(path: &Path, target: &Path) -> Result<(), String> {
    let root = owned(path)?;
    let lock = hold(&root, None)?;
    rename_held(&root, target, lock)
        .map_err(|(error, _)| format!("Failed to rename library: {error}"))
}

/// The answer of [`destination`]: whether a Move… into the chosen folder would
/// move the library, and to which path.
#[derive(Debug, PartialEq)]
pub enum Moved {
    /// The library would be moved. The value is the path that its folder would
    /// have after the move.
    To(PathBuf),
    /// The chosen folder is the folder that the library is already in, so
    /// there is nothing to move.
    Unchanged,
}

/// Works out where Move… would put the library at `path` when the user chooses
/// `folder`: the path in `folder` with the library's own folder name. Returns
/// `Moved::Unchanged` when the library is already at that path. Returns the
/// Electron app's error message in two cases: when `folder` is a library
/// folder or is inside one (a folder whose name ends in `.library`), and when
/// something with the library's folder name already exists in `folder`.
pub fn destination(path: &Path, folder: &Path) -> Result<Moved, String> {
    let inside_library = |folder: &Path| {
        folder.ancestors().any(|folder| {
            folder
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().ends_with(".library"))
        })
    };
    let refuse_inside = || "A library cannot be moved inside another Picto library.".to_owned();
    if inside_library(folder) {
        return Err(refuse_inside());
    }
    let folder = folder
        .canonicalize()
        .map_err(|error| format!("{}: {error}", folder.display()))?;
    if inside_library(&folder) {
        return Err(refuse_inside());
    }
    let name = path.file_name().ok_or_else(|| path.display().to_string())?;
    let target = folder.join(name);
    if path.canonicalize().is_ok_and(|path| path == target) {
        return Ok(Moved::Unchanged);
    }
    if target.symlink_metadata().is_ok() {
        // The Electron app shows this message under the title "Already
        // Exists".
        return Err(format!(
            "\"{}\" already exists at that location.",
            name.to_string_lossy()
        ));
    }
    Ok(Moved::To(target))
}

/// Does the Library Manager's Move…: moves the library's folder at `path` to
/// `target`, which is the path that [`destination`] returned. It first tries a
/// rename, which works when both paths are on the same volume. If the system
/// answers that they are on different volumes, it copies the folder instead
/// ([`move_across`]). If the library is open, the caller closes it before
/// calling this.
pub fn move_to(path: &Path, target: &Path) -> Result<(), String> {
    let root = owned(path)?;
    let lock = hold(&root, None)?;
    match rename_held(&root, target, lock) {
        Ok(()) => Ok(()),
        Err((error, lock)) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            // On Windows the rename released the lock, so it is taken again
            // for the copy.
            let lock = match lock {
                Some(lock) => lock,
                None => hold(&root, None)?,
            };
            move_across(&root, target, lock)
        }
        Err((error, _)) => Err(format!("Failed to move library: {error}")),
    }
}

/// Moves the library's folder `root` to `target` on another volume
/// (`moveLibraryDirectory` in the Electron app). The folder is copied into the
/// parent folder of `target` under the temporary name
/// `.<name>.picto-moving-<process id>`. Every file of the copy is compared
/// with its original. The copy is then renamed to `target`, and only after
/// that is the original removed. If the copy fails or differs from the
/// original, or if something has appeared at `target` in the meantime, the
/// copy is removed and the original is left as it was. If the original cannot
/// be removed after the copy has been renamed to `target`, the move still
/// succeeds and the failure is written to the console. A library that holds a
/// symbolic link is not moved this way: the Electron app copies the link as it
/// is, but this app's `copy_tree` returns an error when it finds a link.
///
/// `lock` is the library's lock. It is held until the original has been
/// removed, so no other instance can open and change the original while it
/// is copied, compared and removed. The lock file itself is not copied and
/// not compared, because it is open and locked here. The core makes a new
/// lock file when the library is next opened in its new place.
fn move_across(root: &Path, target: &Path, lock: LibraryLock) -> Result<(), String> {
    let folder = target
        .parent()
        .ok_or_else(|| target.display().to_string())?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let staging = folder.join(format!(".{name}.picto-moving-{}", std::process::id()));
    let published = copy_without_lock_file(root, &staging)
        .and_then(|()| same_tree_except(root, &staging, Some(LIBRARY_LOCK_FILE)))
        .and_then(|()| {
            if target.symlink_metadata().is_ok() {
                return Err("The move destination appeared while copying the library".to_owned());
            }
            std::fs::rename(&staging, target).map_err(|error| error.to_string())
        });
    if let Err(error) = published {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("Failed to move library: {error}"));
    }
    if let Err(error) = remove_held(root, lock) {
        eprintln!(
            "picto: unable to remove the original after a verified cross-volume copy {}: {error}",
            root.display()
        );
    }
    Ok(())
}

/// Copies the library's folder `root` to `staging`, with everything in it
/// except the lock file, which is directly in `root`.
fn copy_without_lock_file(root: &Path, staging: &Path) -> Result<(), String> {
    std::fs::create_dir(staging).map_err(|error| format!("{}: {error}", staging.display()))?;
    for entry in std::fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_name() == LIBRARY_LOCK_FILE {
            continue;
        }
        crate::application::copy_tree(&entry.path(), &staging.join(entry.file_name()))?;
    }
    Ok(())
}

/// Checks that the folder `copy` holds exactly what the folder `original`
/// holds (`verifyDirectoryCopy` in the Electron app): the same file names and
/// folder names at every level, and the same bytes in every file. Returns an
/// error if anything differs, and also if `original` holds something that is
/// neither a file nor a folder, such as a symbolic link.
pub fn same_tree(original: &Path, copy: &Path) -> Result<(), String> {
    same_tree_except(original, copy, None)
}

/// Does what [`same_tree`] does, except that the file named `skip` directly
/// in `original` is not looked at and is not expected in `copy`.
fn same_tree_except(original: &Path, copy: &Path, skip: Option<&str>) -> Result<(), String> {
    let failed = || "Copied library verification failed".to_owned();
    let mut count = 0;
    for entry in std::fs::read_dir(original).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if skip.is_some_and(|skip| entry.file_name() == skip) {
            continue;
        }
        count += 1;
        let copied = copy.join(entry.file_name());
        let (kind, copied_meta) = (
            entry.file_type().map_err(|error| error.to_string())?,
            copied.symlink_metadata().map_err(|_| failed())?,
        );
        if kind.is_dir() {
            if !copied_meta.is_dir() {
                return Err(failed());
            }
            same_tree(&entry.path(), &copied)?;
        } else if !kind.is_file() || !copied_meta.is_file() || !same_file(&entry.path(), &copied)? {
            return Err(failed());
        }
    }
    let copied = std::fs::read_dir(copy)
        .map_err(|error| error.to_string())?
        .count();
    if copied != count {
        return Err(failed());
    }
    Ok(())
}

fn same_file(left: &Path, right: &Path) -> Result<bool, String> {
    use std::io::Read as _;
    let open = |path: &Path| std::fs::File::open(path).map_err(|error| error.to_string());
    let (mut left, mut right) = (open(left)?, open(right)?);
    let (mut a, mut b) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        let read = left.read(&mut a).map_err(|error| error.to_string())?;
        let mut filled = 0;
        while filled < read {
            let more = right
                .read(&mut b[filled..read])
                .map_err(|error| error.to_string())?;
            if more == 0 {
                return Ok(false);
            }
            filled += more;
        }
        if read == 0 {
            return Ok(right.read(&mut b[..1]).map_err(|error| error.to_string())? == 0);
        }
        if a[..read] != b[..read] {
            return Ok(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::admission::tests::{real_library, snapshot};

    #[test]
    fn only_a_library_folder_is_owned() {
        let folder = tempfile::tempdir().unwrap();
        let kept = real_library(folder.path(), "Kept");
        assert_eq!(owned(&kept).unwrap(), kept);
        let plain = folder.path().join("Pictures");
        std::fs::create_dir(&plain).unwrap();
        std::fs::write(plain.join("library.sqlite"), b"").unwrap();
        let empty = folder.path().join("Empty.library");
        std::fs::create_dir(&empty).unwrap();
        let fake = folder.path().join("Fake.library");
        std::fs::create_dir_all(fake.join("library.sqlite")).unwrap();
        for refused in [plain, empty, fake, folder.path().join("Gone.library")] {
            assert!(owned(&refused).is_err(), "{}", refused.display());
        }
        #[cfg(unix)]
        {
            let link = folder.path().join("Link.library");
            std::os::unix::fs::symlink(&kept, &link).unwrap();
            assert!(owned(&link).is_err());
        }
    }

    #[test]
    fn delete_removes_only_a_library_that_is_neither_open_nor_held() {
        let folder = tempfile::tempdir().unwrap();
        let kept = real_library(folder.path(), "Kept");
        let other = real_library(folder.path(), "Other");
        let before = snapshot(&other);
        // The library that is open in this app is not deleted.
        assert!(delete(&kept, Some(&kept)).is_err());
        assert!(kept.join("library.sqlite").is_file());
        // A library is not deleted while something else holds the core's lock
        // on it.
        let held = picto_core::state::lock_library(&kept).unwrap();
        assert!(delete(&kept, None).is_err());
        assert!(kept.join("library.sqlite").is_file());
        drop(held);
        // A folder that is not a library is not deleted.
        let plain = folder.path().join("Plain");
        std::fs::create_dir(&plain).unwrap();
        assert!(delete(&plain, None).is_err());
        assert!(plain.exists());
        // A library that is neither open nor locked is deleted. On Unix a
        // symbolic link to the other library is put inside it first. The
        // delete removes the link, and it leaves the other library and the
        // plain folder as they were.
        #[cfg(unix)]
        std::os::unix::fs::symlink(&other, kept.join("blobs/leads-out")).unwrap();
        delete(&kept, Some(&other)).unwrap();
        assert!(!kept.exists());
        assert_eq!(snapshot(&other), before);
        assert!(plain.exists());
    }

    #[test]
    fn move_puts_the_library_in_the_folder_chosen_and_nowhere_else() {
        let folder = tempfile::tempdir().unwrap();
        let kept = real_library(folder.path(), "Kept");
        let into = folder.path().join("Archive");
        std::fs::create_dir(&into).unwrap();
        let into = into.canonicalize().unwrap();
        assert_eq!(destination(&kept, folder.path()).unwrap(), Moved::Unchanged);
        assert!(destination(&kept, &kept.join("blobs")).is_err());
        let Moved::To(target) = destination(&kept, &into).unwrap() else {
            panic!("not moved");
        };
        assert_eq!(target, into.join("Kept.library"));
        let database = std::fs::read(kept.join("library.sqlite")).unwrap();
        move_to(&kept, &target).unwrap();
        assert!(!kept.exists());
        assert_eq!(
            std::fs::read(target.join("library.sqlite")).unwrap(),
            database
        );
        assert!(target.join("blobs").is_dir());
        // A library cannot be moved into a folder that already holds something
        // with the library's folder name.
        let again = real_library(folder.path(), "Kept");
        assert!(destination(&again, &into).is_err());
    }

    /// A move across volumes copies the library and compares every file of the
    /// copy with its original before it removes the original. The second half
    /// of the test checks that `same_tree` returns an error when a file's
    /// bytes differ and when the copy holds an extra file.
    #[test]
    fn a_copy_across_volumes_is_verified_before_the_original_goes() {
        let folder = tempfile::tempdir().unwrap();
        let kept = real_library(folder.path(), "Kept");
        std::fs::write(kept.join("blobs/one"), b"one").unwrap();
        let target = folder.path().join("Moved.library");
        let lock = hold(&kept, None).unwrap();
        move_across(&kept, &target, lock).unwrap();
        assert!(!kept.exists());
        assert_eq!(std::fs::read(target.join("blobs/one")).unwrap(), b"one");
        let copy = folder.path().join("Copy.library");
        crate::application::copy_tree(&target, &copy).unwrap();
        same_tree(&target, &copy).unwrap();
        std::fs::write(copy.join("blobs/one"), b"two").unwrap();
        assert!(same_tree(&target, &copy).is_err());
        std::fs::write(copy.join("blobs/one"), b"one").unwrap();
        std::fs::write(copy.join("extra"), b"").unwrap();
        assert!(same_tree(&target, &copy).is_err());
    }
}
