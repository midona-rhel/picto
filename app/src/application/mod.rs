//! Runs the application layer (`picto_core`) inside this app. The app sends
//! commands to it (`dispatch("command", {json})`) and receives its events.
//! The layer runs the background workers: imports, derivatives, duplicate
//! scans and subscription runners. The library is a disposable copy that
//! `run.sh` or `--make-copy` made, or a library the user keeps that
//! `crate::libraries` admitted.
//!
//! The library is opened with `picto_core::state::open_library_reporting`
//! on its root. The library thread of [`crate::backend`] uses the
//! application's own `Library` and does not open the database a second
//! time.
use crate::libraries::Kind;
use futures::channel::oneshot;
use picto_core::dto::{LIBRARY_CHANGED_EVENT, LibraryChanged};
use picto_library::Library;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

/// The error text that a caller receives when the application ended before
/// it answered.
pub const STOPPED: &str = "The application has stopped";

/// Waits for the answer on `receiver`. Returns [`STOPPED`] as the error if
/// the sender was dropped before it answered.
pub async fn answer<T>(receiver: oneshot::Receiver<Result<T, String>>) -> Result<T, String> {
    receiver.await.unwrap_or_else(|_| Err(STOPPED.to_owned()))
}

mod copy;
pub(crate) use copy::copy_tree;
pub use copy::{MARKER, detach, guard, make_copy};

/// An event that the application layer sends.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The `library/changed` event. It names what a published write changed:
    /// the revision, the resources and the items.
    Changed(LibraryChanged),
    /// The `picto:thumbnail-changed` event. A thumbnail was written again.
    /// The string is the hash of the thumbnail's file.
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

/// Returns whether `changed` names `resource`.
pub fn touches(changed: &LibraryChanged, resource: &str) -> bool {
    changed.resources.iter().any(|name| name == resource)
}

/// Merges `events` into one invalidation that has the highest revision and
/// every resource and item that any of the events names. Also returns the
/// hashes of the thumbnails that changed, each hash once. The session calls
/// this with the events that arrived since its previous call.
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

/// Returns the folder in which the core keeps its application data (the AI
/// models, in `models/`).
///
/// A release build uses `picto` in the platform's configuration folder for
/// the current user (`~/Library/Application Support` on macOS, `%APPDATA%`
/// on Windows, `$XDG_CONFIG_HOME` or `~/.config` on Linux). Picto 0.6.19
/// and earlier used the same folder, so the models that it downloaded are
/// found.
///
/// A debug build never uses that folder. For a copy it uses `app-data` in
/// the folder that holds the copy. For a library the user keeps it uses
/// `Picto GPUI/app-data` in the configuration folder, so that it writes
/// nothing beside that library. In a debug build `PICTO_GPUI_APP_DATA` can
/// name another folder, and that folder is then used for every library.
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

/// Holds the application layer for one library: the runtime that its
/// commands run on, the library's folder and kind, and the state of opening
/// and closing the library.
pub struct Application {
    runtime: tokio::runtime::Runtime,
    root: PathBuf,
    kind: Kind,
    /// Set once the library is being closed. Commands are refused from then
    /// on, so that no command reaches the library that is opened after this
    /// one.
    retired: std::sync::atomic::AtomicBool,
    /// Holds `None` while the library is opening. Afterwards it holds whether
    /// the library opened, with the error message if it did not.
    opened: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    report: tokio::sync::watch::Sender<Option<Result<(), String>>>,
}

impl Application {
    /// Prepares the application layer for the library at `root`. The
    /// library is a copy that [`guard`] accepted, or a library the user
    /// keeps that `crate::libraries` admitted. From this call on, the
    /// layer's events are sent to `events`, and an `Application` that was
    /// made earlier receives no more events. Nothing is opened until
    /// [`Application::open`] is called.
    pub fn new(root: PathBuf, kind: Kind, events: mpsc::Sender<Event>) -> Arc<Self> {
        // Tell the core where it keeps the AI models, which are outside the
        // library. The core accepts this once per process, so the first
        // library that is opened decides the folder.
        if let Some(data) = application_data(&root, kind) {
            let _ = picto_core::state::set_application_data_root(data);
        }
        picto_core::state::init_tracing();
        let events = std::sync::Mutex::new(events);
        picto_core::events::set_event_callback(move |name, payload| {
            // The core's log lines are shown in the log window.
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
        })
    }

    /// Returns the library's folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns whether the library is a disposable copy or a library the
    /// user keeps.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Returns a receiver that holds `None` until the open has finished, and
    /// then whether the library opened.
    pub fn opened(&self) -> tokio::sync::watch::Receiver<Option<Result<(), String>>> {
        self.opened.clone()
    }

    /// Opens the library and starts the application's workers. A copy is
    /// detached first ([`detach`]). A library the user keeps is opened as it
    /// is. Blocks the calling thread until the library is open, and returns
    /// its `Library`. `readable` is called with that `Library` as soon as
    /// the database is open, before the application's recovery passes run,
    /// so that results can be read during those passes. On a library of a
    /// million items the passes take many seconds.
    pub fn open(
        &self,
        readable: impl FnOnce(Arc<Library>) + Send + 'static,
    ) -> Result<Arc<Library>, String> {
        let detached = match self.kind {
            Kind::Copy => detach(&self.root),
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

    /// Sends a command with [`Application::dispatch`] and returns a future
    /// of its answer.
    pub fn call<C: Into<String>>(
        &self,
        command: C,
        args: Value,
    ) -> impl std::future::Future<Output = Result<Value, String>> + use<C> {
        answer(self.dispatch(command, args))
    }

    /// Sends a command to the application layer and returns a receiver for
    /// its JSON answer. A command that is sent before the library has opened
    /// waits until it has opened. A command that is sent after
    /// [`Application::retire`] is answered with an error.
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
        self.runtime.spawn(async move {
            let ready = match opened.wait_for(Option::is_some).await {
                Ok(state) => state.clone().unwrap_or(Ok(())),
                Err(_) => Err(STOPPED.to_owned()),
            };
            // The clock starts when the library is open, so that the time in
            // the log is the command's own and holds no wait for the library.
            let started = std::time::Instant::now();
            let result = match ready {
                Ok(()) => picto_core::state::invoke(&command, &args.to_string())
                    .await
                    .and_then(|json| {
                        serde_json::from_str(&json).map_err(|error| error.to_string())
                    }),
                Err(error) => Err(error),
            };
            // Record how long the command took, for the log window.
            crate::diagnostics::record_ipc(
                &command,
                started.elapsed().as_secs_f64() * 1000.,
                result.as_ref().err().map(String::as_str),
            );
            let _ = answer.send(result);
        });
        receiver
    }

    /// Makes [`Application::dispatch`] refuse commands from now on.
    pub fn retire(&self) {
        self.retired
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Stops the workers and checkpoints the library, and refuses commands
    /// from then on. The app calls this before it quits and before it opens
    /// another library.
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
    fn events_are_read_as_the_earlier_versions_window_read_them() {
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
        assert!(touches(&merged, "subscriptions") && !touches(&merged, "folders"));
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
        // This command is sent before the library has opened, so it waits
        // for the open.
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
        // An unknown command returns an error and does not panic.
        assert!(
            block_on(application.dispatch("no.such.command", serde_json::json!({})))
                .unwrap()
                .is_err()
        );
        // A write sends a `library/changed` event that names what it changed.
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
        // Keeping one file of a pair frees the other file, and the
        // application deletes the freed file from the copy's blobs. The
        // library that the copy was made from keeps its file.
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
