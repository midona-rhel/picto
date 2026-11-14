//! What the Library Manager does to a library's folder on disk, as
//! production's main process does it (`libraryHostService.mjs`,
//! `libraryFilesystem.mjs`): Delete… removes the folder for good
//! (`deleteLibrary`, `fs.rm` recursive: not to the system's Trash), and
//! Move… puts it in another folder (`relocateLibrary`,
//! `moveLibraryDirectory`: a rename, or across volumes a copy that is
//! verified, published under the new name and only then the original
//! removed).
//!
//! Both act only on what `ownedLibraryPath` accepts (a real folder, not a
//! link, named `.library`, holding a `library.sqlite` file), never on the
//! library open in this app, and never on one another process holds: the
//! core's `.picto-library.lock` must be free. The caller has already had
//! `admission::admit` let the library through, the user's consent
//! included (`manager`), and asked the user.
use std::path::{Path, PathBuf};

/// `ownedLibraryPath`: `path` made canonical if it is a library this app
/// may change on disk, or production's words for why not.
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
/// else that takes the core's lock).
fn free(root: &Path, open: Option<&Path>) -> Result<(), String> {
    if open.is_some_and(|open| open.canonicalize().is_ok_and(|open| open == root)) {
        return Err(
            "Cannot delete the currently open library. Switch to a different library first."
                .to_owned(),
        );
    }
    picto_core::state::lock_library(root)
        .map(drop)
        .map_err(|error| format!("{}: {error}", root.display()))
}

/// Delete…: the library's folder and everything in it, for good.
pub fn delete(path: &Path, open: Option<&Path>) -> Result<(), String> {
    let root = owned(path)?;
    free(&root, open)?;
    // `remove_dir_all` removes links themselves, never what they lead to.
    std::fs::remove_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))
}

/// What a Move… came to.
#[derive(Debug, PartialEq)]
pub enum Moved {
    /// Moved: the library's new folder.
    To(PathBuf),
    /// The folder chosen is where it already is.
    Unchanged,
}

/// Where Move… would put `path` in `folder`, or production's words for why
/// it cannot go there: not inside a library, and nothing of that name
/// there already.
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
        // Production shows it as "Already Exists".
        return Err(format!(
            "\"{}\" already exists at that location.",
            name.to_string_lossy()
        ));
    }
    Ok(Moved::To(target))
}

/// Move…: the library at `path` into `target` (from [`destination`]). The
/// open library is closed by the caller first.
pub fn move_to(path: &Path, target: &Path) -> Result<(), String> {
    let root = owned(path)?;
    free(&root, None)?;
    match std::fs::rename(&root, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            move_across(&root, target)
        }
        Err(error) => Err(format!("Failed to move library: {error}")),
    }
}

/// `moveLibraryDirectory` across volumes: copied beside the target under a
/// hidden name, every file compared with its original, renamed into place,
/// and only then the original removed. A copy that fails or differs is
/// removed and the original left as it was. A library holding a symbolic
/// link is not moved this way (production copies the link as it is; this
/// app's copy refuses links).
fn move_across(root: &Path, target: &Path) -> Result<(), String> {
    let folder = target
        .parent()
        .ok_or_else(|| target.display().to_string())?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let staging = folder.join(format!(".{name}.picto-moving-{}", std::process::id()));
    let published = crate::application::copy_tree(root, &staging)
        .and_then(|()| same_tree(root, &staging))
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
    if let Err(error) = std::fs::remove_dir_all(root) {
        eprintln!(
            "picto: unable to remove the original after a verified cross-volume copy {}: {error}",
            root.display()
        );
    }
    Ok(())
}

/// `verifyDirectoryCopy`: `copy` holds exactly what `original` holds, file
/// by file and byte by byte.
pub fn same_tree(original: &Path, copy: &Path) -> Result<(), String> {
    let failed = || "Copied library verification failed".to_owned();
    let mut count = 0;
    for entry in std::fs::read_dir(original).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
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
        // The open one.
        assert!(delete(&kept, Some(&kept)).is_err());
        assert!(kept.join("library.sqlite").is_file());
        // Held by another holder of the core's lock.
        let held = picto_core::state::lock_library(&kept).unwrap();
        assert!(delete(&kept, None).is_err());
        assert!(kept.join("library.sqlite").is_file());
        drop(held);
        // Not a library.
        let plain = folder.path().join("Plain");
        std::fs::create_dir(&plain).unwrap();
        assert!(delete(&plain, None).is_err());
        assert!(plain.exists());
        // Deleted, and nothing else touched.
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
        // Something of that name there already.
        let again = real_library(folder.path(), "Kept");
        assert!(destination(&again, &into).is_err());
    }

    /// The copy a move across volumes makes is checked file by file
    /// before the original goes.
    #[test]
    fn a_copy_across_volumes_is_verified_before_the_original_goes() {
        let folder = tempfile::tempdir().unwrap();
        let kept = real_library(folder.path(), "Kept");
        std::fs::write(kept.join("blobs/one"), b"one").unwrap();
        let target = folder.path().join("Moved.library");
        move_across(&kept, &target).unwrap();
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
