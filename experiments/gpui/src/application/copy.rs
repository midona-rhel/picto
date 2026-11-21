//! The disposable-copy rules: which folder this app opens as a copy
//! ([`guard`]), how a copy is made ([`make_copy`]) and how it is cut loose
//! from the library it was copied from ([`detach`]).
use std::path::{Path, PathBuf};

/// The file `run.sh` writes into a copy, naming the library it copied.
pub const MARKER: &str = ".gpui-disposable";
/// What the folder holding a copy is called (`run.sh`, `million.sh`).
const SCRATCH_PREFIX: &str = "picto-gpui-";
/// Written once the copy has been cut loose the first time.
const DETACHED: &str = ".gpui-detached";
/// Written once the copy has a cloud identity of its own.
const CLOUD_IDENTITY: &str = ".gpui-cloud-identity";

/// Checks that `root` is a marked disposable copy and not the library it was
/// copied from, nor inside it, and returns its canonical path. This app
/// writes, imports and deletes files, so it opens nothing else.
pub fn guard(root: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("{}: {error}", root.display()))?;
    let marker = std::fs::read_to_string(root.join(MARKER)).map_err(|_| {
        format!(
            "{} is not a disposable copy made by run.sh (no {MARKER}); it is not opened",
            root.display()
        )
    })?;
    let source = PathBuf::from(marker.trim());
    if source.as_os_str().is_empty() {
        return Err(format!("{MARKER} in {} names no library", root.display()));
    }
    let source = source.canonicalize().unwrap_or(source);
    if root.starts_with(&source) || source.starts_with(&root) {
        return Err(format!(
            "{} is, or is inside, the library it copies ({}); it is not opened",
            root.display(),
            source.display()
        ));
    }
    // A marker is only a file: one copied along with a library, or left in
    // a folder that was moved, would open something the user keeps. A
    // disposable copy is also where the scripts put it: in a folder of its
    // own named `picto-gpui-…`, directly in a temporary folder.
    if !in_scratch(&root) {
        return Err(format!(
            "{} is not in a picto-gpui-… folder of a temporary folder, where run.sh and million.sh make copies; it is not opened",
            root.display()
        ));
    }
    // A link in the copy leads out of it: what is written or deleted through
    // it lands wherever it points, which may be the real library.
    if let Some(link) = first_link(&root) {
        return Err(format!(
            "{} is a symbolic link inside the copy; it is not opened",
            link.display()
        ));
    }
    // The same database file reached another way (a hard link) is the real
    // library, whatever the folder is called.
    let (copy, real) = (root.join("library.sqlite"), source.join("library.sqlite"));
    if same_file::is_same_file(&copy, &real).unwrap_or(false) {
        return Err(format!(
            "{} shares its database with {}; it is not opened",
            root.display(),
            source.display()
        ));
    }
    Ok(root)
}

/// The temporary folders a copy may live in: the system's, and `/tmp` where
/// the scripts make theirs.
fn scratch_folders() -> Vec<PathBuf> {
    let mut folders = vec![std::env::temp_dir()];
    if cfg!(unix) {
        folders.extend([PathBuf::from("/tmp"), PathBuf::from("/var/tmp")]);
    }
    folders
        .into_iter()
        .filter_map(|folder| folder.canonicalize().ok())
        .collect()
}

/// Whether the (canonical) `root` is `<temporary folder>/picto-gpui-…/<name>`.
fn in_scratch(root: &Path) -> bool {
    let Some(holder) = root.parent() else {
        return false;
    };
    let named = holder
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(SCRATCH_PREFIX));
    named
        && holder
            .parent()
            .is_some_and(|parent| scratch_folders().iter().any(|folder| folder == parent))
}

/// Makes a disposable copy of the library at `source` and returns its root,
/// for opening with this app: what `run.sh` does on macOS, done here so
/// that every system has it (`picto-gpui-spike --make-copy <library>`).
///
/// The copy goes in a new `picto-gpui-library.…` folder of the system's
/// temporary folder, where [`guard`] allows one. The database is copied with
/// SQLite's online backup from a read-only connection, which includes
/// committed WAL data and never writes the live files; it is copied first,
/// so every file it names is already there when the rest is copied.
/// Everything else in the library folder is copied with the system's own
/// file copy, which clones where the file system can (APFS, btrfs, XFS,
/// ReFS) and copies in full elsewhere. A symbolic link anywhere in the
/// library is refused, as the guard would refuse it in the copy. The marker
/// names the source, and the copy is detached as every launch detaches it.
/// The source is only read; on any failure the new folder is removed.
pub fn make_copy(source: &Path) -> Result<PathBuf, String> {
    let source = source
        .canonicalize()
        .map_err(|error| format!("{}: {error}", source.display()))?;
    if !source.join("library.sqlite").is_file() {
        return Err(format!("{} has no library.sqlite", source.display()));
    }
    let name = source
        .file_name()
        .ok_or_else(|| format!("{} has no name", source.display()))?;
    let scratch = tempfile::Builder::new()
        .prefix(&format!("{SCRATCH_PREFIX}library."))
        .tempdir()
        .map_err(|error| format!("make a temporary folder: {error}"))?;
    let copy = scratch.path().join(name);
    let made = (|| -> Result<(), String> {
        std::fs::create_dir(&copy).map_err(|error| format!("{}: {error}", copy.display()))?;
        let database = rusqlite::Connection::open_with_flags(
            source.join("library.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| format!("open the library's database: {error}"))?;
        database
            .backup(rusqlite::MAIN_DB, copy.join("library.sqlite"), None)
            .map_err(|error| format!("copy the library's database: {error}"))?;
        drop(database);
        let entries =
            std::fs::read_dir(&source).map_err(|error| format!("{}: {error}", source.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_name = entry.file_name();
            if is_database_file(&file_name.to_string_lossy()) {
                continue;
            }
            copy_tree(&entry.path(), &copy.join(&file_name))?;
        }
        std::fs::write(copy.join(MARKER), format!("{}\n", source.display()))
            .map_err(|error| format!("write the marker: {error}"))?;
        detach(&copy, false)
    })();
    if let Err(error) = made {
        // `scratch` removes the new folder as it drops.
        return Err(error);
    }
    let kept = scratch.keep();
    guard(&kept.join(name))
}

/// The database's own files, which the online backup has already copied,
/// and the lock an application holding the library open keeps (a locked
/// file cannot be read on Windows, and a copy is not open anywhere).
fn is_database_file(name: &str) -> bool {
    matches!(
        name,
        "library.sqlite" | "library.sqlite-wal" | "library.sqlite-shm" | "library.sqlite-journal"
    ) || name == picto_core::state::LIBRARY_LOCK_FILE
}

/// Copies `from` to `to`, folders and all, refusing symbolic links.
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let kind = from
        .symlink_metadata()
        .map_err(|error| format!("{}: {error}", from.display()))?
        .file_type();
    if kind.is_symlink() {
        return Err(format!(
            "{} is a symbolic link; a copy with one would lead out of itself, so none was made",
            from.display()
        ));
    }
    if kind.is_dir() {
        std::fs::create_dir(to).map_err(|error| format!("{}: {error}", to.display()))?;
        for entry in
            std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)
            .map(drop)
            .map_err(|error| format!("copy {}: {error}", from.display()))
    }
}

/// The first symbolic link among the copy's own entries and its blob
/// folders, where the application writes and deletes.
fn first_link(root: &Path) -> Option<PathBuf> {
    let folders = [
        root.to_path_buf(),
        root.join("blobs"),
        root.join("blobs").join("f"),
        root.join("blobs").join("t"),
    ];
    for folder in &folders[..2] {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                return Some(entry.path());
            }
        }
    }
    folders[2..]
        .iter()
        .find(|folder| {
            folder
                .symlink_metadata()
                .is_ok_and(|data| data.file_type().is_symlink())
        })
        .cloned()
}

/// Cuts the copy loose from work the real library has going on elsewhere,
/// before the application starts its workers on it:
/// - cloud sync is disconnected, so no snapshot of the copy is ever written
///   to the user's cloud folder;
/// - watched folders are switched off, so nothing outside the copy is
///   imported (or deleted after import);
/// - runs that were queued or running in the real library are cancelled, and
///   schedules have no next run, so no subscription downloads unless the user
///   starts it. Choosing a schedule again sets its next run;
/// - no import job deletes the file it imports: a job queued in the real
///   library by a moving import, a subscription or a watched folder names a
///   file outside the copy, and would remove the user's only original once
///   the copy had it;
/// - the first time, import jobs still queued from the real library are
///   marked failed rather than run, and the stored file paths, which the
///   database backup carries over pointing into the real library, are
///   pointed at the copy's own files, so that nothing opened, revealed,
///   copied or dragged out before the application repairs them is a file of
///   the real library.
///
/// - the copy has a cloud identity of its own (a new library and device
///   id, once, kept in `.gpui-cloud-identity`), so a copy connected to a
///   cloud folder never writes into, or purges from, the folder of the
///   library it was copied from.
///
/// Each step is idempotent; the copy is detached again on every launch.
/// `keep_cloud` keeps the cloud folder the user connected the copy to in
/// this run: a copy opened again after a restore (`host::restore`).
/// A folder without the marker is a library the user keeps: it is refused
/// before anything is opened.
pub fn detach(root: &Path, keep_cloud: bool) -> Result<(), String> {
    if !root.join(MARKER).is_file() {
        return Err(format!(
            "{} is not a disposable copy (no {MARKER}); it is not detached",
            root.display()
        ));
    }
    let mut connection = rusqlite::Connection::open(root.join("library.sqlite"))
        .map_err(|error| format!("open the copy's database: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let first = !root.join(DETACHED).exists();
    let source = std::fs::read_to_string(root.join(MARKER))
        .ok()
        .map(|marker| marker.trim().to_owned())
        .filter(|source| Path::new(source).is_absolute());
    let identified = root.join(CLOUD_IDENTITY).is_file();
    let detached = (|| -> rusqlite::Result<()> {
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if !keep_cloud {
            transaction.execute_batch(
                "UPDATE cloud_state
                    SET provider = NULL, account_label = NULL, remote_root = NULL,
                        paused = 1, state = 'disabled', phase = 'idle', blocking = 0,
                        message = ''
                  WHERE provider IS NOT NULL OR remote_root IS NOT NULL;",
            )?;
        }
        if !identified {
            transaction.execute(
                "UPDATE cloud_state SET library_id = ?1, device_id = ?2 WHERE singleton = 1",
                [
                    uuid::Uuid::new_v4().to_string(),
                    uuid::Uuid::new_v4().to_string(),
                ],
            )?;
        }
        transaction.execute_batch(
            "UPDATE folder_definition SET watch_enabled = 0 WHERE watch_enabled = 1;
             UPDATE subscription SET next_run_at = NULL WHERE next_run_at IS NOT NULL;
             UPDATE ingest_job SET delete_after_ingest = 0 WHERE delete_after_ingest = 1;",
        )?;
        for table in ["subscription_run_query", "subscription_run"] {
            transaction.execute(
                &format!(
                    "UPDATE {table} SET status = 'cancelled', finished_at = ?1
                      WHERE status IN ('pending', 'running')"
                ),
                [&now],
            )?;
        }
        if first {
            transaction.execute(
                "UPDATE ingest_job
                    SET status = 'failed', updated_at = ?1,
                        last_error = 'Queued in the library this copy was made from; not run in a copy'
                  WHERE status IN ('pending', 'running')",
                [&now],
            )?;
            if let Some(source) = &source {
                // `substr` rather than `LIKE`: a path may hold `%` or `_`.
                let copy = root.to_string_lossy();
                transaction.execute(
                    "UPDATE media_file
                        SET file_path = ?2 || substr(file_path, length(?1) + 1)
                      WHERE substr(file_path, 1, length(?1) + 1) IN (?1 || '/', ?1 || char(92))",
                    [source.as_str(), copy.as_ref()],
                )?;
            }
        }
        transaction.commit()
    })();
    detached.map_err(|error| format!("detach the copy: {error}"))?;
    if !identified {
        std::fs::write(root.join(CLOUD_IDENTITY), format!("{now}\n"))
            .map_err(|error| format!("mark the copy's cloud identity: {error}"))?;
    }
    if first {
        std::fs::write(root.join(DETACHED), format!("{now}\n"))
            .map_err(|error| format!("mark the copy detached: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MARKER, detach, guard};
    use std::path::{Path, PathBuf};

    /// A folder as `run.sh` makes one: `picto-gpui-library.…` in the
    /// temporary folder.
    fn scratch() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("picto-gpui-library.")
            .tempdir()
            .unwrap()
    }

    /// `--make-copy`: a marked, detached copy in a folder the guard allows,
    /// with the database backed up and the rest copied; the source is left
    /// as it was, and a library holding a link is refused.
    #[test]
    fn make_copy_makes_an_openable_copy_and_leaves_the_source_alone() {
        let elsewhere = tempfile::tempdir().unwrap();
        let source = elsewhere.path().join("Real.library");
        std::fs::create_dir_all(source.join("blobs").join("f").join("ab")).unwrap();
        std::fs::write(source.join("blobs/f/ab/one.jpg"), b"picture").unwrap();
        let mut database = rusqlite::Connection::open(source.join("library.sqlite")).unwrap();
        picto_library::schema::create(&mut database).unwrap();
        database
            .execute_batch("CREATE TABLE note(text); INSERT INTO note VALUES ('kept');")
            .unwrap();
        drop(database);

        let copy = super::make_copy(&source).unwrap();
        let holder = copy.parent().unwrap().to_path_buf();
        assert_eq!(guard(&copy).unwrap(), copy);
        assert_eq!(
            std::fs::read(copy.join("blobs/f/ab/one.jpg")).unwrap(),
            b"picture"
        );
        let copied = rusqlite::Connection::open(copy.join("library.sqlite")).unwrap();
        let text: String = copied
            .query_row("SELECT text FROM note", [], |row| row.get(0))
            .unwrap();
        assert_eq!(text, "kept");
        assert!(!source.join(MARKER).exists());
        // Windows keeps an open database's files from being removed.
        drop(copied);
        std::fs::remove_dir_all(&holder).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(elsewhere.path(), source.join("blobs/out")).unwrap();
            assert!(super::make_copy(&source).is_err());
        }
    }

    #[test]
    fn only_a_marked_copy_outside_its_source_is_opened() {
        let elsewhere = tempfile::tempdir().unwrap();
        let scratch = scratch();
        let source = elsewhere.path().join("Real.library");
        let copy = scratch.path().join("Real.library");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(source.join("library.sqlite"), b"real").unwrap();
        // Unmarked: refused.
        assert!(guard(&copy).is_err());
        assert!(guard(&source).is_err());
        // Marked, naming its source: opened.
        std::fs::write(copy.join(MARKER), format!("{}\n", source.display())).unwrap();
        std::fs::write(copy.join("library.sqlite"), b"copy").unwrap();
        assert_eq!(guard(&copy).unwrap(), copy.canonicalize().unwrap());
        // A marker planted in the real library, or in a folder inside it.
        std::fs::write(source.join(MARKER), format!("{}\n", source.display())).unwrap();
        assert!(guard(&source).is_err());
        let inside = source.join("blobs");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(inside.join(MARKER), format!("{}\n", source.display())).unwrap();
        assert!(guard(&inside).is_err());
        // A copy whose database is the real one under another name.
        std::fs::remove_file(copy.join("library.sqlite")).unwrap();
        std::fs::hard_link(source.join("library.sqlite"), copy.join("library.sqlite")).unwrap();
        assert!(guard(&copy).is_err());
        // An empty marker.
        std::fs::remove_file(copy.join("library.sqlite")).unwrap();
        std::fs::write(copy.join(MARKER), "\n").unwrap();
        assert!(guard(&copy).is_err());
    }

    /// A marker travels with the files: a marked copy that was moved or
    /// copied out of the scratch folders is something the user keeps.
    #[test]
    fn a_marked_folder_outside_the_scratch_folders_is_refused() {
        let elsewhere = tempfile::tempdir().unwrap();
        let source = elsewhere.path().join("Real.library");
        std::fs::create_dir_all(&source).unwrap();
        let mark = |folder: &std::path::Path| {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("library.sqlite"), b"copy").unwrap();
            std::fs::write(folder.join(MARKER), format!("{}\n", source.display())).unwrap();
        };
        // Named as a copy's folder is, but not in a temporary folder.
        let moved = elsewhere
            .path()
            .join("picto-gpui-library.moved")
            .join("Kept.library");
        mark(&moved);
        assert!(guard(&moved).is_err());
        // In a temporary folder, but not in a folder of the scripts' naming.
        let stray = tempfile::tempdir().unwrap();
        let unnamed = stray.path().join("Kept.library");
        mark(&unnamed);
        assert!(guard(&unnamed).is_err());
        // Deeper than the scripts put it.
        let scratch = scratch();
        let deep = scratch.path().join("more").join("Kept.library");
        mark(&deep);
        assert!(guard(&deep).is_err());
        let placed = scratch.path().join("Kept.library");
        mark(&placed);
        assert!(guard(&placed).is_ok());
    }

    /// What is written through a link lands where the link points.
    #[cfg(unix)]
    #[test]
    fn a_copy_with_a_link_in_it_is_refused() {
        let elsewhere = tempfile::tempdir().unwrap();
        let source = elsewhere.path().join("Real.library");
        std::fs::create_dir_all(source.join("blobs").join("f")).unwrap();
        let link = std::os::unix::fs::symlink;
        for linked in ["blobs", "blobs/f", "blobs/t", "thumbnails"] {
            let scratch = scratch();
            let copy = scratch.path().join("Real.library");
            std::fs::create_dir_all(&copy).unwrap();
            std::fs::write(copy.join("library.sqlite"), b"copy").unwrap();
            std::fs::write(copy.join(MARKER), format!("{}\n", source.display())).unwrap();
            assert!(guard(&copy).is_ok());
            if let Some((folder, _)) = linked.split_once('/') {
                std::fs::create_dir_all(copy.join(folder)).unwrap();
            }
            link(source.join("blobs"), copy.join(linked)).unwrap();
            assert!(guard(&copy).is_err(), "{linked}");
        }
    }

    /// Opens the application on a real copy, sends commands as the Electron
    /// renderer does, and sees the invalidation of a write arrive.

    /// `--detach` and every launch detach only a marked copy: a library
    /// the user keeps is refused before its database is opened, and left
    /// exactly as it was.
    #[test]
    fn detach_never_touches_a_library_without_the_marker() {
        let folder = tempfile::tempdir().unwrap();
        let root = crate::libraries::admission::tests::real_library(folder.path(), "Kept");
        rusqlite::Connection::open(root.join("library.sqlite"))
            .unwrap()
            .execute_batch(
                "INSERT INTO folder_definition (folder_id, stable_key, name, watch_path, watch_enabled, display_order)
                 VALUES (1, 'f1', 'Watched', '/home/me/Downloads', 1, 0);",
            )
            .unwrap();
        let before = crate::libraries::admission::tests::snapshot(&root);
        assert!(detach(&root, false).is_err());
        assert_eq!(crate::libraries::admission::tests::snapshot(&root), before);
        assert!(!root.join(super::DETACHED).exists());
    }

    /// A copy synced to a cloud folder writes, and purges, only under an
    /// identity of its own: never the library's it was copied from, whose
    /// snapshots and originals in that folder it could otherwise replace or
    /// remove. The identity stays across launches, and a copy opened again
    /// after a restore keeps the folder the user connected it to.
    #[test]
    fn a_copy_never_syncs_as_the_library_it_was_copied_from() {
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(scratch.path().join(MARKER), "synthetic:test\n").unwrap();
        let mut connection =
            rusqlite::Connection::open(scratch.path().join("library.sqlite")).unwrap();
        picto_library::schema::create(&mut connection).unwrap();
        let identity = |connection: &rusqlite::Connection| -> (String, String) {
            connection
                .query_row("SELECT library_id, device_id FROM cloud_state", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .unwrap()
        };
        let source = identity(&connection);
        drop(connection);
        detach(scratch.path(), false).unwrap();
        let connection = rusqlite::Connection::open(scratch.path().join("library.sqlite")).unwrap();
        let own = identity(&connection);
        assert_ne!(own.0, source.0);
        assert_ne!(own.1, source.1);
        connection
            .execute_batch(
                "UPDATE cloud_state SET provider = 'dropbox', account_label = 'Dropbox',
                        remote_root = '/fake/Dropbox', state = 'idle', paused = 0;",
            )
            .unwrap();
        drop(connection);
        detach(scratch.path(), true).unwrap();
        let connection = rusqlite::Connection::open(scratch.path().join("library.sqlite")).unwrap();
        assert_eq!(identity(&connection), own);
        let root: Option<String> = connection
            .query_row("SELECT remote_root FROM cloud_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(root.as_deref(), Some("/fake/Dropbox"));
        drop(connection);
        detach(scratch.path(), false).unwrap();
        let connection = rusqlite::Connection::open(scratch.path().join("library.sqlite")).unwrap();
        assert_eq!(identity(&connection), own);
        let root: Option<String> = connection
            .query_row("SELECT remote_root FROM cloud_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(root, None);
    }

    #[test]
    fn a_copy_keeps_no_cloud_watch_or_queued_work() {
        let scratch = tempfile::tempdir().unwrap();
        // The copy's marker names the library it was made from.
        // An absolute path of this platform's own form.
        let source = PathBuf::from(if cfg!(windows) {
            r"C:\Work\Pictures\Real.library"
        } else {
            "/home/me/Pictures/Real.library"
        });
        let stored = |library: &Path, hash: &str| {
            library
                .join("blobs")
                .join("f")
                .join(hash)
                .join(format!("{hash}.png"))
                .to_string_lossy()
                .into_owned()
        };
        // A folder that only starts with the source's name is not the source.
        let neighbour = PathBuf::from(format!("{}.bak", source.display()));
        std::fs::write(
            scratch.path().join(MARKER),
            format!("{}\n", source.display()),
        )
        .unwrap();
        let mut connection =
            rusqlite::Connection::open(scratch.path().join("library.sqlite")).unwrap();
        picto_library::schema::create(&mut connection).unwrap();
        connection
            .execute_batch(
                "UPDATE cloud_state SET provider = 'dropbox', account_label = 'me',
                        remote_root = '/home/me/Dropbox', state = 'idle', paused = 0;
                 INSERT INTO folder_definition (folder_id, stable_key, name, watch_path, watch_enabled, display_order)
                 VALUES (1, 'f1', 'Watched', '/home/me/Downloads', 1, 0);
                 INSERT INTO subscription (subscription_id, subscription_key, name, schedule, next_run_at, created_at)
                 VALUES (1, 's1', 'Daily', 'daily', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO subscription_run (run_id, subscription_id, requested_by, status, created_at)
                 VALUES (1, 1, 'scheduled', 'pending', '2026-01-01T00:00:00Z'),
                        (2, 1, 'user', 'succeeded', '2026-01-01T00:00:00Z');
                 INSERT INTO ingest_job (ingest_job_id, job_key, source_kind, source_path, payload_json,
                                         delete_after_ingest, status, available_at, created_at, updated_at)
                 VALUES (1, 'moving', 'file', '/home/me/Downloads/only-copy.png', '{}', 1, 'pending', 'now', 'now', 'now'),
                        (2, 'claimed', 'file', '/home/me/Downloads/claimed.png', '{}', 1, 'running', 'now', 'now', 'now'),
                        (3, 'done', 'file', '/home/me/Downloads/done.png', '{}', 1, 'succeeded', 'now', 'now', 'now');",
            )
            .unwrap();
        for (file, hash, library) in [(1, "aa", &source), (2, "bb", &neighbour)] {
            connection
                .execute(
                    "INSERT INTO media_file (file_id, content_hash, file_path, mime, size_bytes)
                     VALUES (?1, ?2, ?3, 'image/png', 1)",
                    rusqlite::params![file, hash, stored(library, hash)],
                )
                .unwrap();
        }
        drop(connection);
        detach(scratch.path(), false).unwrap();
        // An import started in the copy and cut short by quitting is the
        // user's own: the next launch leaves it queued, but still deletes
        // nothing it imports from.
        rusqlite::Connection::open(scratch.path().join("library.sqlite"))
            .unwrap()
            .execute_batch(
                "INSERT INTO ingest_job (ingest_job_id, job_key, source_kind, source_path, payload_json,
                                         delete_after_ingest, status, available_at, created_at, updated_at)
                 VALUES (4, 'own', 'file', '/home/me/Desktop/own.png', '{}', 1, 'pending', 'now', 'now', 'now');",
            )
            .unwrap();
        // Again, on the next launch.
        detach(scratch.path(), false).unwrap();
        let connection = rusqlite::Connection::open(scratch.path().join("library.sqlite")).unwrap();
        let one = |sql: &str| -> String {
            connection
                .query_row(sql, [], |row| row.get::<_, Option<String>>(0))
                .unwrap()
                .unwrap_or_else(|| "NULL".into())
        };
        assert_eq!(one("SELECT provider FROM cloud_state"), "NULL");
        assert_eq!(one("SELECT remote_root FROM cloud_state"), "NULL");
        assert_eq!(
            one("SELECT CAST(watch_enabled AS TEXT) FROM folder_definition"),
            "0"
        );
        assert_eq!(one("SELECT next_run_at FROM subscription"), "NULL");
        assert_eq!(
            one("SELECT group_concat(status) FROM subscription_run ORDER BY run_id"),
            "cancelled,succeeded"
        );
        // No job deletes its source, and what the real library had queued
        // is not run.
        assert_eq!(
            one("SELECT CAST(sum(delete_after_ingest) AS TEXT) FROM ingest_job"),
            "0"
        );
        assert_eq!(
            one(
                "SELECT group_concat(status) FROM (SELECT status FROM ingest_job ORDER BY ingest_job_id)"
            ),
            "failed,failed,succeeded,pending"
        );
        // Stored paths lead into the copy; the neighbour's are left alone.
        assert_eq!(
            one("SELECT file_path FROM media_file WHERE file_id = 1"),
            stored(scratch.path(), "aa")
        );
        assert_eq!(
            one("SELECT file_path FROM media_file WHERE file_id = 2"),
            stored(&neighbour, "bb")
        );
    }
}
