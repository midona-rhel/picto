//! The application layer the Electron frontend talks to (`picto_core`), run
//! inside this app: the same commands (`dispatch("command", {json})`), the
//! same background workers (imports, derivatives, duplicate scans,
//! subscription runners) and the same events, on a disposable copy of a
//! library that `run.sh` made.
//!
//! The copy is opened exactly as the Electron app opens a library
//! (`picto_core::state::open_library` on its root), and the library thread
//! of [`crate::backend`] borrows the application's own `Library` rather than
//! opening the database a second time.
use futures::channel::oneshot;
use picto_core::dto::{LIBRARY_CHANGED_EVENT, LibraryChanged};
use picto_library::Library;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

/// The file `run.sh` writes into a copy, naming the library it copied.
pub const MARKER: &str = ".gpui-disposable";
/// What the folder holding a copy is called (`run.sh`, `million.sh`).
const SCRATCH_PREFIX: &str = "picto-gpui-";
/// Written once the copy has been cut loose the first time.
const DETACHED: &str = ".gpui-detached";

/// What the application says happened, as the Electron renderer receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `library/changed`: what a published write touched.
    Changed(LibraryChanged),
    /// `picto:thumbnail-changed`: a thumbnail was written again.
    Thumbnail(String),
}

impl Event {
    fn parse(name: &str, payload: &str) -> Option<Self> {
        match name {
            LIBRARY_CHANGED_EVENT => serde_json::from_str(payload).ok().map(Event::Changed),
            "picto:thumbnail-changed" => serde_json::from_str::<Value>(payload)
                .ok()?
                .get("fileHash")?
                .as_str()
                .map(|hash| Event::Thumbnail(hash.to_owned())),
            _ => None,
        }
    }
}

/// Whether an invalidation names `resource`.
pub fn touches(changed: &LibraryChanged, resource: &str) -> bool {
    changed.resources.iter().any(|name| name == resource)
}

/// Merges the invalidations that arrived since the last look into one, as
/// the renderer's registry delivers one per frame: the latest revision, every
/// resource and every item named. Thumbnails are listed once each.
pub fn coalesce(events: impl IntoIterator<Item = Event>) -> (Option<LibraryChanged>, Vec<String>) {
    let mut changed: Option<LibraryChanged> = None;
    let mut thumbnails = Vec::new();
    for event in events {
        match event {
            Event::Changed(next) => match &mut changed {
                None => changed = Some(next),
                Some(merged) => {
                    merged.revision = merged.revision.max(next.revision);
                    for resource in next.resources {
                        if !merged.resources.contains(&resource) {
                            merged.resources.push(resource);
                        }
                    }
                    for item in next.item_ids {
                        if !merged.item_ids.contains(&item) {
                            merged.item_ids.push(item);
                        }
                    }
                }
            },
            Event::Thumbnail(hash) => {
                if !thumbnails.contains(&hash) {
                    thumbnails.push(hash);
                }
            }
        }
    }
    (changed, thumbnails)
}

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
        detach(&copy)
    })();
    if let Err(error) = made {
        // `scratch` removes the new folder as it drops.
        return Err(error);
    }
    let kept = scratch.keep();
    guard(&kept.join(name))
}

/// The database's own files, which the online backup has already copied.
fn is_database_file(name: &str) -> bool {
    matches!(
        name,
        "library.sqlite" | "library.sqlite-wal" | "library.sqlite-shm" | "library.sqlite-journal"
    )
}

/// Copies `from` to `to`, folders and all, refusing symbolic links.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
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
/// Each step is idempotent; the copy is detached again on every launch.
pub fn detach(root: &Path) -> Result<(), String> {
    let mut connection = rusqlite::Connection::open(root.join("library.sqlite"))
        .map_err(|error| format!("open the copy's database: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let first = !root.join(DETACHED).exists();
    let source = std::fs::read_to_string(root.join(MARKER))
        .ok()
        .map(|marker| marker.trim().to_owned())
        .filter(|source| Path::new(source).is_absolute());
    let detached = (|| -> rusqlite::Result<()> {
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            "UPDATE cloud_state
                SET provider = NULL, account_label = NULL, remote_root = NULL,
                    paused = 1, state = 'disabled', phase = 'idle', blocking = 0,
                    message = ''
              WHERE provider IS NOT NULL OR remote_root IS NOT NULL;
             UPDATE folder_definition SET watch_enabled = 0 WHERE watch_enabled = 1;
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
    if first {
        std::fs::write(root.join(DETACHED), format!("{now}\n"))
            .map_err(|error| format!("mark the copy detached: {error}"))?;
    }
    Ok(())
}

/// The open application, its runtime, and where its events go.
pub struct Application {
    runtime: tokio::runtime::Runtime,
    root: PathBuf,
    /// `None` while the library opens, then whether it opened.
    opened: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    report: tokio::sync::watch::Sender<Option<Result<(), String>>>,
}

impl Application {
    /// Prepares the application for a guarded copy and starts delivering its
    /// events to `events`. Nothing is opened until [`Application::open`].
    pub fn new(root: PathBuf, events: mpsc::Sender<Event>) -> Arc<Self> {
        // Models, credentials caches and other data the application keeps
        // outside a library go beside the copy, never into the user's own
        // application folder. This can be set once per process.
        if let Some(scratch) = root.parent() {
            let _ = picto_core::state::set_application_data_root(scratch.join("app-data"));
        }
        picto_core::state::init_tracing();
        let events = std::sync::Mutex::new(events);
        picto_core::events::set_event_callback(move |name, payload| {
            // The core's tracing, for the log window (`diagnosticsRuntime`).
            if name == picto_core::events::event_names::LOG {
                crate::diagnostics::accept_core(payload);
                return;
            }
            if let Some(event) = Event::parse(name, payload)
                && let Ok(events) = events.lock()
            {
                let _ = events.send(event);
            }
        });
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("picto-core")
            .build()
            .expect("start the application runtime");
        let (report, opened) = tokio::sync::watch::channel(None);
        Arc::new(Self {
            runtime,
            root,
            opened,
            report,
        })
    }

    /// Detaches the copy and opens it as the Electron app opens a library,
    /// starting the application's workers. Blocks the calling thread until
    /// it is open, and hands back its `Library`. `readable` is handed that
    /// `Library` as soon as the database is open, before the application's
    /// recovery passes run (on a library of a million items they take many
    /// seconds), so results can be read meanwhile.
    pub fn open(
        &self,
        readable: impl FnOnce(Arc<Library>) + Send + 'static,
    ) -> Result<Arc<Library>, String> {
        let opened = detach(&self.root).and_then(|()| {
            self.runtime
                .block_on(picto_core::state::open_library_reporting(
                    self.root.clone(),
                    readable,
                ))
        });
        let result = opened.map(|state| Arc::clone(state.application().library()));
        let _ = self
            .report
            .send(Some(result.as_ref().map(|_| ()).map_err(Clone::clone)));
        result
    }

    /// Sends a command to the application, as the Electron renderer's
    /// `invoke(command, args)` does, and hands back its JSON answer. Commands
    /// sent before the library has opened wait for it.
    pub fn dispatch(
        &self,
        command: impl Into<String>,
        args: Value,
    ) -> oneshot::Receiver<Result<Value, String>> {
        let (answer, receiver) = oneshot::channel();
        let command = command.into();
        let mut opened = self.opened.clone();
        let started = std::time::Instant::now();
        self.runtime.spawn(async move {
            let ready = match opened.wait_for(Option::is_some).await {
                Ok(state) => state.clone().unwrap_or(Ok(())),
                Err(_) => Err("The application has stopped".to_owned()),
            };
            let result = match ready {
                Ok(()) => picto_core::state::invoke(&command, &args.to_string())
                    .await
                    .and_then(|json| {
                        serde_json::from_str(&json).map_err(|error| error.to_string())
                    }),
                Err(error) => Err(error),
            };
            // The round trip, for the log window (`recordIpcCall`).
            crate::diagnostics::record_ipc(
                &command,
                started.elapsed().as_secs_f64() * 1000.,
                result.as_ref().err().map(String::as_str),
            );
            let _ = answer.send(result);
        });
        receiver
    }

    /// Stops the workers and checkpoints the copy, as the Electron app does
    /// before it quits.
    pub fn close(&self) {
        if let Err(error) = self.runtime.block_on(picto_core::state::close_library()) {
            eprintln!("picto: closing the library: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, MARKER, coalesce, detach, guard, touches};
    use std::path::{Path, PathBuf};

    use picto_core::dto::LibraryChanged;

    fn changed(revision: u64, resources: &[&str], items: &[u32]) -> Event {
        Event::Changed(LibraryChanged {
            revision,
            resources: resources.iter().map(|name| name.to_string()).collect(),
            item_ids: items.to_vec(),
        })
    }

    #[test]
    fn events_are_read_as_the_renderer_reads_them() {
        assert_eq!(
            Event::parse(
                "library/changed",
                r#"{"revision":7,"resources":["tags"],"item_ids":[3]}"#
            ),
            Some(changed(7, &["tags"], &[3]))
        );
        assert_eq!(
            Event::parse("picto:thumbnail-changed", r#"{"fileHash":"ab12"}"#),
            Some(Event::Thumbnail("ab12".into()))
        );
        assert_eq!(Event::parse("log", r#"{"message":"x"}"#), None);
        assert_eq!(Event::parse("library/changed", "not json"), None);
    }

    #[test]
    fn invalidations_in_one_frame_become_one() {
        let (merged, thumbnails) = coalesce([
            changed(4, &["library", "tags"], &[1, 2]),
            Event::Thumbnail("a".into()),
            changed(6, &["tags", "subscriptions"], &[2, 3]),
            Event::Thumbnail("a".into()),
            changed(5, &["sidebar"], &[]),
        ]);
        let merged = merged.unwrap();
        assert_eq!(merged.revision, 6);
        assert_eq!(
            merged.resources,
            ["library", "tags", "subscriptions", "sidebar"]
        );
        assert_eq!(merged.item_ids, [1, 2, 3]);
        assert!(touches(&merged, "subscriptions") && !touches(&merged, "cloud"));
        assert_eq!(thumbnails, ["a"]);
        assert_eq!(coalesce([]), (None, vec![]));
    }

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
    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy made by run.sh"]
    fn real_copy_answers_commands_and_reports_changes() {
        use futures::executor::block_on;
        let root = std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap();
        let root = guard(std::path::Path::new(&root)).unwrap();
        let (events, received) = std::sync::mpsc::channel();
        let application = super::Application::new(root.clone(), events);
        // Sent before the library has opened: it waits for the open.
        let early = application.dispatch("sidebar.counts", serde_json::json!({}));
        let started = std::time::Instant::now();
        let library = application.open(|_| {}).unwrap();
        eprintln!("opened in {:?}", started.elapsed());
        let counts = block_on(early).unwrap().unwrap();
        assert_eq!(
            counts["all"].as_u64().unwrap(),
            library.sidebar_counts().unwrap().all
        );
        let list = block_on(application.dispatch("subscriptions.list", serde_json::json!({})))
            .unwrap()
            .unwrap();
        assert!(list["subscriptions"].is_array());
        let pairs =
            block_on(application.dispatch("duplicates.list", serde_json::json!({ "limit": 500 })))
                .unwrap()
                .unwrap();
        eprintln!(
            "{} subscriptions, {} duplicate pairs",
            list["subscriptions"].as_array().unwrap().len(),
            pairs.as_array().map_or(0, Vec::len)
        );
        // An unknown command is an error, not a panic.
        assert!(
            block_on(application.dispatch("no.such.command", serde_json::json!({})))
                .unwrap()
                .is_err()
        );
        // A write reports what it touched, as `library/changed`.
        let settings = block_on(application.dispatch("settings.get", serde_json::json!({})))
            .unwrap()
            .unwrap();
        let spacing = settings["gridSpacing"].as_f64().unwrap_or(8.);
        // Two different values, so at least one write changes something.
        for value in [spacing + 1., spacing] {
            block_on(application.dispatch(
                "settings.patch",
                serde_json::json!({ "value": { "gridSpacing": value } }),
            ))
            .unwrap()
            .unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut seen = false;
        while !seen && std::time::Instant::now() < deadline {
            if let Ok(event) = received.recv_timeout(std::time::Duration::from_millis(100)) {
                if let Event::Changed(changed) = event {
                    seen = touches(&changed, "settings");
                }
            }
        }
        assert!(seen, "no settings invalidation arrived");
        // The duplicate scanner runs through the application layer.
        let started = std::time::Instant::now();
        let scan = block_on(application.dispatch(
            "duplicates.scan",
            serde_json::json!({ "distance_threshold": 16 }),
        ))
        .unwrap()
        .unwrap();
        eprintln!("scan {scan} in {:?}", started.elapsed());
        // Keeping one file of a pair frees the other, which the application
        // deletes from the copy's blobs; the library it was copied from
        // keeps its file.
        let pairs =
            block_on(application.dispatch("duplicates.list", serde_json::json!({ "limit": 500 })))
                .unwrap()
                .unwrap();
        if let Some(pair) = pairs.as_array().and_then(|pairs| pairs.first()) {
            let resolved = block_on(application.dispatch(
                "duplicates.resolve",
                serde_json::json!({
                    "file_id_a": pair["file_id_a"],
                    "file_id_b": pair["file_id_b"],
                    "choice": { "KeepFile": { "winner_file_id": pair["file_id_a"] } },
                }),
            ))
            .unwrap()
            .unwrap();
            eprintln!("resolved {resolved}");
            if let Some(hash) = resolved["freed_file_hash"].as_str() {
                let source = std::fs::read_to_string(root.join(super::MARKER)).unwrap();
                let blob_in = |library: &std::path::Path| {
                    let folder = library.join("blobs/f").join(&hash[..2]).join(&hash[2..4]);
                    std::fs::read_dir(folder).ok().and_then(|entries| {
                        entries
                            .flatten()
                            .find(|entry| entry.file_name().to_string_lossy().starts_with(hash))
                            .map(|entry| entry.path())
                    })
                };
                let real = std::path::PathBuf::from(source.trim());
                let before = blob_in(&real);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while blob_in(&root).is_some() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                eprintln!(
                    "freed {hash}: gone from the copy: {}, still in the real library: {}",
                    blob_in(&root).is_none(),
                    blob_in(&real).is_some()
                );
                assert_eq!(blob_in(&real), before, "the real library changed");
            }
        }
        application.close();
    }

    #[test]
    fn a_copy_keeps_no_cloud_watch_or_queued_work() {
        let scratch = tempfile::tempdir().unwrap();
        // The copy's marker names the library it was made from.
        // An absolute path of this platform's own form.
        let source = PathBuf::from(if cfg!(windows) {
            r"C:\Users\me\Pictures\Real.library"
        } else {
            "/Users/me/Pictures/Real.library"
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
                        remote_root = '/Users/me/Dropbox', state = 'idle', paused = 0;
                 INSERT INTO folder_definition (folder_id, stable_key, name, watch_path, watch_enabled, display_order)
                 VALUES (1, 'f1', 'Watched', '/Users/me/Downloads', 1, 0);
                 INSERT INTO subscription (subscription_id, subscription_key, name, schedule, next_run_at, created_at)
                 VALUES (1, 's1', 'Daily', 'daily', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO subscription_run (run_id, subscription_id, requested_by, status, created_at)
                 VALUES (1, 1, 'scheduled', 'pending', '2026-01-01T00:00:00Z'),
                        (2, 1, 'user', 'succeeded', '2026-01-01T00:00:00Z');
                 INSERT INTO ingest_job (ingest_job_id, job_key, source_kind, source_path, payload_json,
                                         delete_after_ingest, status, available_at, created_at, updated_at)
                 VALUES (1, 'moving', 'file', '/Users/me/Downloads/only-copy.png', '{}', 1, 'pending', 'now', 'now', 'now'),
                        (2, 'claimed', 'file', '/Users/me/Downloads/claimed.png', '{}', 1, 'running', 'now', 'now', 'now'),
                        (3, 'done', 'file', '/Users/me/Downloads/done.png', '{}', 1, 'succeeded', 'now', 'now', 'now');",
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
        detach(scratch.path()).unwrap();
        // An import started in the copy and cut short by quitting is the
        // user's own: the next launch leaves it queued, but still deletes
        // nothing it imports from.
        rusqlite::Connection::open(scratch.path().join("library.sqlite"))
            .unwrap()
            .execute_batch(
                "INSERT INTO ingest_job (ingest_job_id, job_key, source_kind, source_path, payload_json,
                                         delete_after_ingest, status, available_at, created_at, updated_at)
                 VALUES (4, 'own', 'file', '/Users/me/Desktop/own.png', '{}', 1, 'pending', 'now', 'now', 'now');",
            )
            .unwrap();
        // Again, on the next launch.
        detach(scratch.path()).unwrap();
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

#[cfg(test)]
mod open_stages {
    use std::time::Instant;

    /// Times each stage of opening a library in the order `open_library`
    /// runs them, and the parts of the media recovery pass, printing one
    /// line each. The recovery passes write to the copy, as an open does.
    ///
    /// ```sh
    /// PICTO_GPUI_TEST_LIBRARY=<copy> cargo test --offline time_each_stage \
    ///     -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy"]
    fn time_each_stage() {
        fn timed<T>(stage: &str, work: impl FnOnce() -> T) -> T {
            let started = Instant::now();
            let done = work();
            eprintln!("{stage}: {:?}", started.elapsed());
            done
        }
        let root = std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap();
        let root = super::guard(std::path::Path::new(&root)).unwrap();
        timed("detach", || super::detach(&root).unwrap());
        let opening = Instant::now();
        let application = timed("open the database and its resident indexes", || {
            picto_core::library_application::LibraryApplication::open_with(
                &root,
                &picto_library::OpenCancellation::default(),
                &mut |stage| eprintln!("  {stage:?} from {:?}", opening.elapsed()),
            )
            .unwrap()
        });
        let library = application.library();
        timed("navigation, as the grid first reads it", || {
            crate::backend::navigation(library).unwrap()
        });
        timed("subscription recovery", || {
            picto_core::library_subscription_state::recover(&application, "2026-01-01T00:00:00Z")
                .unwrap()
        });
        timed("ingest recovery", || {
            picto_core::library_ingest_runtime::recover(&application).unwrap()
        });
        let originals = timed("media recovery, its parts: list the originals", || {
            application.blobs().list_originals()
        });
        timed("  look for each one's thumbnail", || {
            originals
                .iter()
                .filter(|(hash, _)| {
                    application
                        .blobs()
                        .find_thumbnail_path(hash)
                        .unwrap()
                        .is_none()
                })
                .count()
        });
        timed("  list the thumbnails instead", || {
            application.blobs().list_thumbnails().len()
        });
        eprintln!("  ({} originals)", originals.len());
        timed("media recovery, whole", || {
            picto_core::library_media_runtime::recover(&application).unwrap()
        });
        timed("cloud retention", || {
            picto_core::cloud::normalize_retention_library(&application).unwrap()
        });
    }
}
