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
    // The same database file reached another way (a hard link) is the real
    // library, whatever the folder is called.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let identity = |path: &Path| {
            std::fs::metadata(path)
                .ok()
                .map(|meta| (meta.dev(), meta.ino()))
        };
        let copy = identity(&root.join("library.sqlite"));
        if copy.is_some() && copy == identity(&source.join("library.sqlite")) {
            return Err(format!(
                "{} shares its database with {}; it is not opened",
                root.display(),
                source.display()
            ));
        }
    }
    Ok(root)
}

/// Cuts the copy loose from work the real library has going on elsewhere,
/// before the application starts its workers on it:
/// - cloud sync is disconnected, so no snapshot of the copy is ever written
///   to the user's cloud folder;
/// - watched folders are switched off, so nothing outside the copy is
///   imported (or deleted after import);
/// - runs that were queued or running in the real library are cancelled, and
///   schedules have no next run, so no subscription downloads unless the user
///   starts it. Choosing a schedule again sets its next run.
///
/// Each step is idempotent; the copy is detached again on every launch.
pub fn detach(root: &Path) -> Result<(), String> {
    let connection = rusqlite::Connection::open(root.join("library.sqlite"))
        .map_err(|error| format!("open the copy's database: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    connection
        .execute_batch(&format!(
            "BEGIN IMMEDIATE;
             UPDATE cloud_state
                SET provider = NULL, account_label = NULL, remote_root = NULL,
                    paused = 1, state = 'disabled', phase = 'idle', blocking = 0,
                    message = ''
              WHERE provider IS NOT NULL OR remote_root IS NOT NULL;
             UPDATE folder_definition SET watch_enabled = 0 WHERE watch_enabled = 1;
             UPDATE subscription_run_query
                SET status = 'cancelled', finished_at = '{now}'
              WHERE status IN ('pending', 'running');
             UPDATE subscription_run
                SET status = 'cancelled', finished_at = '{now}'
              WHERE status IN ('pending', 'running');
             UPDATE subscription SET next_run_at = NULL WHERE next_run_at IS NOT NULL;
             COMMIT;"
        ))
        .map_err(|error| format!("detach the copy: {error}"))
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
    /// starting the application's workers. Blocks the calling thread (the
    /// library thread) until it is open, and hands back its `Library`.
    pub fn open(&self) -> Result<Arc<Library>, String> {
        let opened = detach(&self.root).and_then(|()| {
            self.runtime
                .block_on(picto_core::state::open_library(self.root.clone()))
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

    #[test]
    fn only_a_marked_copy_outside_its_source_is_opened() {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("Real.library");
        let copy = scratch.path().join("copy").join("Real.library");
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
        let library = application.open().unwrap();
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
                        (2, 1, 'user', 'succeeded', '2026-01-01T00:00:00Z');",
            )
            .unwrap();
        drop(connection);
        detach(scratch.path()).unwrap();
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
    }
}
