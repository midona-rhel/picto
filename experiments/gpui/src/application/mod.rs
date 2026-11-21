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
use crate::libraries::Kind;
use futures::channel::oneshot;
use picto_core::dto::{LIBRARY_CHANGED_EVENT, LibraryChanged};
use picto_library::Library;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

/// What a caller is told when the application ended before it answered.
pub const STOPPED: &str = "The application has stopped";

/// The answer `receiver` brings, or [`STOPPED`] if its sender went first.
pub async fn answer<T>(receiver: oneshot::Receiver<Result<T, String>>) -> Result<T, String> {
    receiver.await.unwrap_or_else(|_| Err(STOPPED.to_owned()))
}

mod copy;
pub(crate) use copy::copy_tree;
pub use copy::{MARKER, detach, guard, make_copy};

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

/// The folder the core keeps its application data in (the AI models, in
/// `models/`). A release build uses the one production uses, `picto` in the
/// platform's per-user application data folder (`app.getPath('appData')`:
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows,
/// `$XDG_CONFIG_HOME` or `~/.config` on Linux), so the models production
/// downloaded are found. A debug build keeps it beside the copy, never in
/// the user's own folder, unless `PICTO_GPUI_APP_DATA` names one.
///
/// A debug build opening a library the user keeps never writes beside it:
/// it uses `app-data` in this app's own folder.
pub fn application_data(root: &Path, kind: Kind) -> Option<PathBuf> {
    if let Some(folder) = crate::seams::app_data() {
        return Some(folder);
    }
    if cfg!(test) && kind == Kind::Real {
        return Some(std::env::temp_dir().join("picto-gpui-test-app-data"));
    }
    if cfg!(debug_assertions) || cfg!(test) {
        return match kind {
            Kind::Copy => root.parent().map(|scratch| scratch.join("app-data")),
            Kind::Real => crate::state::config_folder()
                .map(|folder| folder.join("Picto GPUI").join("app-data")),
        };
    }
    crate::state::config_folder().map(|folder| folder.join("picto"))
}

/// The open application, its runtime, and where its events go.
pub struct Application {
    runtime: tokio::runtime::Runtime,
    root: PathBuf,
    kind: Kind,
    /// Set once the library is being closed: commands are refused from then
    /// on, so none reaches the library opened after it.
    retired: std::sync::atomic::AtomicBool,
    /// `None` while the library opens, then whether it opened.
    opened: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    report: tokio::sync::watch::Sender<Option<Result<(), String>>>,
    /// Set once this app holds none of the library's connections any more
    /// (the library's thread has ended): a restore may then replace its
    /// database.
    released: tokio::sync::watch::Sender<bool>,
    /// A copy opened again right after a restore keeps the cloud folder the
    /// user connected it to in this run (`detach`).
    keep_cloud: std::sync::atomic::AtomicBool,
}

impl Application {
    /// Prepares the application for a guarded copy, or for a library the
    /// user keeps that `crate::libraries` admitted, and starts delivering
    /// its events to `events` (the events of an application made before go
    /// nowhere from then on). Nothing is opened until [`Application::open`].
    pub fn new(root: PathBuf, kind: Kind, events: mpsc::Sender<Event>) -> Arc<Self> {
        // Where the AI models the application keeps outside a library are.
        // This can be set once per process: the first library decides.
        if let Some(data) = application_data(&root, kind) {
            let _ = picto_core::state::set_application_data_root(data);
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
            kind,
            retired: std::sync::atomic::AtomicBool::new(false),
            opened,
            report,
            released: tokio::sync::watch::channel(false).0,
            keep_cloud: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Opened again after a restore: a copy keeps its cloud folder.
    pub fn keep_cloud(&self) {
        self.keep_cloud
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The library's thread has let the library go.
    pub fn release(&self) {
        self.released.send_replace(true);
    }

    /// Waits until this app holds none of the library's connections, at
    /// most `limit`; whether it does. Blocks the calling thread.
    pub fn wait_released(&self, limit: std::time::Duration) -> bool {
        let mut released = self.released.subscribe();
        self.runtime.block_on(async move {
            tokio::time::timeout(limit, released.wait_for(|released| *released))
                .await
                .is_ok_and(|result| result.is_ok())
        })
    }

    /// Runs one command on a library that is not open here, and closes
    /// whatever the command left open in the core, on this application's
    /// runtime (`cloud.library.join`, which opens the library it makes).
    /// Blocks the calling thread.
    pub fn run_unopened(&self, command: &str, args: Value) -> Result<Value, String> {
        let result = self.run_blocking(command, args);
        if let Err(error) = self.runtime.block_on(picto_core::state::close_library()) {
            eprintln!("picto: closing after {command}: {error}");
        }
        result
    }

    /// Runs one command on this application's runtime and waits for its
    /// answer, on a thread that may block (a restore).
    pub fn run_blocking(&self, command: &str, args: Value) -> Result<Value, String> {
        let started = std::time::Instant::now();
        let result = self.runtime.block_on(async {
            picto_core::state::invoke(command, &args.to_string())
                .await
                .and_then(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
        });
        crate::diagnostics::record_ipc(
            command,
            started.elapsed().as_secs_f64() * 1000.,
            result.as_ref().err().map(String::as_str),
        );
        result
    }

    /// The library's folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether it is a disposable copy or a library the user keeps.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Whether the library opened: `None` until the open has finished.
    pub fn opened(&self) -> tokio::sync::watch::Receiver<Option<Result<(), String>>> {
        self.opened.clone()
    }

    /// Opens the library as the Electron app opens one, a copy detached
    /// first and a library the user keeps as it is,
    /// starting the application's workers. Blocks the calling thread until
    /// it is open, and hands back its `Library`. `readable` is handed that
    /// `Library` as soon as the database is open, before the application's
    /// recovery passes run (on a library of a million items they take many
    /// seconds), so results can be read meanwhile.
    pub fn open(
        &self,
        readable: impl FnOnce(Arc<Library>) + Send + 'static,
    ) -> Result<Arc<Library>, String> {
        let detached = match self.kind {
            Kind::Copy => detach(
                &self.root,
                self.keep_cloud.load(std::sync::atomic::Ordering::SeqCst),
            ),
            Kind::Real => Ok(()),
        };
        let opened = detached.and_then(|()| {
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

    /// [`Application::dispatch`]'s answer, as a future of its own.
    pub fn call<C: Into<String>>(
        &self,
        command: C,
        args: Value,
    ) -> impl std::future::Future<Output = Result<Value, String>> + use<C> {
        answer(self.dispatch(command, args))
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
        if self.retired.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = answer.send(Err(format!(
                "{} was closed; {command} was not sent",
                self.root.display()
            )));
            return receiver;
        }
        let mut opened = self.opened.clone();
        let started = std::time::Instant::now();
        self.runtime.spawn(async move {
            let ready = match opened.wait_for(Option::is_some).await {
                Ok(state) => state.clone().unwrap_or(Ok(())),
                Err(_) => Err(STOPPED.to_owned()),
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

    /// Refuses commands from now on.
    pub fn retire(&self) {
        self.retired
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Stops the workers and checkpoints the library, as the Electron app
    /// does before it quits or switches, and refuses commands from then on.
    pub fn close(&self) {
        self.retire();
        if let Err(error) = self.runtime.block_on(picto_core::state::close_library()) {
            eprintln!("picto: closing the library: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, coalesce, guard, touches};

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
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy made by run.sh"]
    fn real_copy_answers_commands_and_reports_changes() {
        use futures::executor::block_on;
        let root = std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap();
        let root = guard(std::path::Path::new(&root)).unwrap();
        let (events, received) = std::sync::mpsc::channel();
        let application =
            super::Application::new(root.clone(), crate::libraries::Kind::Copy, events);
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
}
