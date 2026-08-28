//! SQLite ownership for the replacement backend.
//!
//! The store exposes direct read and transaction boundaries. Domain behavior
//! stays in application modules rather than growing another database facade.

pub mod history;
pub mod schema;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rusqlite::functions::{Aggregate, Context, FunctionFlags};
use rusqlite::{Connection, OpenFlags, Transaction};

pub const DATABASE_FILE: &str = "library.sqlite";
const MAX_IDLE_READERS: usize = 8;
const BACKGROUND_WRITER_QUIET_PERIOD: Duration = Duration::from_millis(5);

pub struct Store {
    root: PathBuf,
    path: PathBuf,
    writer: Mutex<Connection>,
    checkpoint: Mutex<Connection>,
    readers: Mutex<Vec<Connection>>,
    history: Mutex<history::HistoryBuffer>,
    publication_gate: Mutex<()>,
    writer_admission: WriterAdmission,
    publication_samples: AtomicU64,
    publication_wait_micros: AtomicU64,
    publication_hold_micros: AtomicU64,
    publication_max_wait_micros: AtomicU64,
    publication_max_hold_micros: AtomicU64,
}

/// A projection settlement prepared before commit may persist the durable
/// representation it is about to publish. This runs inside the same SQLite
/// transaction and before the publication gate is acquired.
pub(crate) trait PreparedSettlement {
    fn persist(&mut self, transaction: &Transaction<'_>, revision: u64) -> Result<(), String>;
}

impl PreparedSettlement for () {
    fn persist(&mut self, _transaction: &Transaction<'_>, _revision: u64) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicationGateStats {
    pub samples: u64,
    pub total_wait_micros: u64,
    pub total_hold_micros: u64,
    pub max_wait_micros: u64,
    pub max_hold_micros: u64,
}

impl PublicationGateStats {
    pub fn average_hold_micros(self) -> u64 {
        self.total_hold_micros
            .checked_div(self.samples)
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WritePriority {
    Foreground,
    Maintenance,
    Background,
    SearchMaintenance,
    Cloud,
}

struct WriterAdmissionState {
    active: bool,
    foreground_waiters: usize,
    maintenance_waiters: usize,
    background_waiters: usize,
    search_waiters: usize,
    cloud_waiters: usize,
    last_higher_priority_activity: Instant,
}

impl Default for WriterAdmissionState {
    fn default() -> Self {
        Self {
            active: false,
            foreground_waiters: 0,
            maintenance_waiters: 0,
            background_waiters: 0,
            search_waiters: 0,
            cloud_waiters: 0,
            last_higher_priority_activity: Instant::now() - BACKGROUND_WRITER_QUIET_PERIOD,
        }
    }
}

#[derive(Default)]
struct WriterAdmission {
    state: Mutex<WriterAdmissionState>,
    ready: Condvar,
}

struct WriterPermit<'a> {
    admission: &'a WriterAdmission,
    priority: WritePriority,
}

impl WriterAdmission {
    fn acquire(&self, priority: WritePriority) -> Result<WriterPermit<'_>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Store writer admission lock poisoned".to_string())?;
        match priority {
            WritePriority::Foreground => state.foreground_waiters += 1,
            WritePriority::Maintenance => state.maintenance_waiters += 1,
            WritePriority::Background => state.background_waiters += 1,
            WritePriority::SearchMaintenance => state.search_waiters += 1,
            WritePriority::Cloud => state.cloud_waiters += 1,
        }
        loop {
            let higher_priority_waiting = match priority {
                WritePriority::Foreground => false,
                WritePriority::Maintenance => state.foreground_waiters > 0,
                WritePriority::Background => {
                    state.foreground_waiters > 0 || state.maintenance_waiters > 0
                }
                WritePriority::SearchMaintenance => {
                    state.foreground_waiters > 0
                        || state.maintenance_waiters > 0
                        || state.background_waiters > 0
                }
                WritePriority::Cloud => {
                    state.foreground_waiters > 0
                        || state.maintenance_waiters > 0
                        || state.background_waiters > 0
                        || state.search_waiters > 0
                }
            };
            let background_quiet = matches!(
                priority,
                WritePriority::Background | WritePriority::SearchMaintenance | WritePriority::Cloud
            ) && state.last_higher_priority_activity.elapsed()
                < BACKGROUND_WRITER_QUIET_PERIOD;
            if !state.active && !higher_priority_waiting && !background_quiet {
                break;
            }
            if background_quiet && !state.active && !higher_priority_waiting {
                let remaining = BACKGROUND_WRITER_QUIET_PERIOD
                    .saturating_sub(state.last_higher_priority_activity.elapsed());
                let (next, _) = self
                    .ready
                    .wait_timeout(state, remaining)
                    .map_err(|_| "Store writer admission lock poisoned".to_string())?;
                state = next;
            } else {
                state = self
                    .ready
                    .wait(state)
                    .map_err(|_| "Store writer admission lock poisoned".to_string())?;
            }
        }
        match priority {
            WritePriority::Foreground => state.foreground_waiters -= 1,
            WritePriority::Maintenance => state.maintenance_waiters -= 1,
            WritePriority::Background => state.background_waiters -= 1,
            WritePriority::SearchMaintenance => state.search_waiters -= 1,
            WritePriority::Cloud => state.cloud_waiters -= 1,
        }
        state.active = true;
        Ok(WriterPermit {
            admission: self,
            priority,
        })
    }
}

impl Drop for WriterPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.admission.state.lock() {
            state.active = false;
            if self.priority != WritePriority::Cloud {
                state.last_higher_priority_activity = Instant::now();
            }
            self.admission.ready.notify_all();
        }
    }
}

struct TrackedPublicationWrite<'a> {
    _guard: MutexGuard<'a, ()>,
    caller: &'static std::panic::Location<'static>,
    wait: Duration,
    acquired_at: Instant,
    samples: &'a AtomicU64,
    total_wait_micros: &'a AtomicU64,
    total_hold_micros: &'a AtomicU64,
    max_wait_micros: &'a AtomicU64,
    max_hold_micros: &'a AtomicU64,
}

impl Drop for TrackedPublicationWrite<'_> {
    fn drop(&mut self) {
        let held = self.acquired_at.elapsed();
        let wait_micros = self.wait.as_micros().min(u64::MAX as u128) as u64;
        let hold_micros = held.as_micros().min(u64::MAX as u128) as u64;
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.total_wait_micros
            .fetch_add(wait_micros, Ordering::Relaxed);
        self.total_hold_micros
            .fetch_add(hold_micros, Ordering::Relaxed);
        self.max_wait_micros
            .fetch_max(wait_micros, Ordering::Relaxed);
        self.max_hold_micros
            .fetch_max(hold_micros, Ordering::Relaxed);
        if self.wait.as_millis() >= 50 || held.as_millis() >= 16 {
            tracing::warn!(
                target: "picto::store",
                caller_file = self.caller.file(),
                caller_line = self.caller.line(),
                write_wait_ms = self.wait.as_secs_f64() * 1_000.0,
                write_hold_ms = held.as_secs_f64() * 1_000.0,
                "Slow store write ownership"
            );
        }
    }
}

impl Store {
    pub fn open(library_root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(library_root)
            .map_err(|error| format!("Failed to create library directory: {error}"))?;
        let path = library_root.join(DATABASE_FILE);
        let existed = path.exists();
        if existed {
            let validation = open_connection(&path, true)?;
            schema::validate(&validation)?;
        }

        let mut writer = open_connection(&path, false)?;
        if existed {
            writer
                .execute_batch(
                    "PRAGMA analysis_limit=1000;
                     ANALYZE;
                     PRAGMA optimize;",
                )
                .map_err(|error| {
                    format!("Failed to optimize SQLite planner statistics: {error}")
                })?;
        } else {
            schema::create_canonical_v1(&mut writer)?;
        }

        let checkpoint = open_connection(&path, false)?;

        Ok(Self {
            root: library_root.to_path_buf(),
            path,
            writer: Mutex::new(writer),
            checkpoint: Mutex::new(checkpoint),
            readers: Mutex::new(Vec::new()),
            history: Mutex::new(history::HistoryBuffer::default()),
            publication_gate: Mutex::new(()),
            writer_admission: WriterAdmission::default(),
            publication_samples: AtomicU64::new(0),
            publication_wait_micros: AtomicU64::new(0),
            publication_hold_micros: AtomicU64::new(0),
            publication_max_wait_micros: AtomicU64::new(0),
            publication_max_hold_micros: AtomicU64::new(0),
        })
    }

    pub fn library_root(&self) -> &Path {
        &self.root
    }

    pub fn publication_gate_stats(&self) -> PublicationGateStats {
        PublicationGateStats {
            samples: self.publication_samples.load(Ordering::Relaxed),
            total_wait_micros: self.publication_wait_micros.load(Ordering::Relaxed),
            total_hold_micros: self.publication_hold_micros.load(Ordering::Relaxed),
            max_wait_micros: self.publication_max_wait_micros.load(Ordering::Relaxed),
            max_hold_micros: self.publication_max_hold_micros.load(Ordering::Relaxed),
        }
    }

    /// Start a fresh diagnostics window without changing store behavior.
    pub fn reset_publication_gate_stats(&self) {
        self.publication_samples.store(0, Ordering::Relaxed);
        self.publication_wait_micros.store(0, Ordering::Relaxed);
        self.publication_hold_micros.store(0, Ordering::Relaxed);
        self.publication_max_wait_micros.store(0, Ordering::Relaxed);
        self.publication_max_hold_micros.store(0, Ordering::Relaxed);
    }

    #[track_caller]
    pub fn checkpoint(&self) -> Result<(), String> {
        let connection = self
            .checkpoint
            .lock()
            .map_err(|_| "Store checkpoint lock poisoned".to_string())?;
        connection
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))
            .map_err(|error| error.to_string())
    }

    pub fn read<T>(
        &self,
        operation: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        self.read_snapshot(operation)
    }

    /// Pin one WAL snapshot without entering the projection publication gate.
    /// SQLite-only views do not combine database and in-memory projection state.
    pub fn read_snapshot<T>(
        &self,
        operation: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        self.with_published_snapshot(|connection, _revision| {
            operation(connection).map_err(|error| error.to_string())
        })
    }

    pub fn read_result<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        self.read_snapshot_result(operation)
    }

    /// Fallible SQLite-only read over one pinned published WAL snapshot.
    pub fn read_snapshot_result<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_published_snapshot(|connection, _revision| operation(connection))
    }

    /// Capture an owned projection view and pin the matching SQLite snapshot
    /// under the publication gate, then release the gate before query work.
    /// The captured value must not borrow mutable projection state.
    pub fn read_snapshot_captured<C, T>(
        &self,
        capture: impl FnOnce() -> C,
        operation: impl FnOnce(&Connection, u64, C) -> Result<T, String>,
    ) -> Result<T, String> {
        let connection = self
            .readers
            .lock()
            .map_err(|_| "Store reader pool lock poisoned".to_string())?
            .pop()
            .map(Ok)
            .unwrap_or_else(|| open_connection(&self.path, true))?;
        let result = (|| {
            let publication = self
                .publication_gate
                .lock()
                .map_err(|_| "Store publication gate poisoned".to_string())?;
            connection
                .execute_batch("BEGIN DEFERRED")
                .map_err(|error| error.to_string())?;
            let revision = match schema::revision(&connection) {
                Ok(revision) => revision,
                Err(error) => {
                    let _ = connection.execute_batch("ROLLBACK");
                    return Err(error.to_string());
                }
            };
            let captured = capture();
            drop(publication);

            let result = operation(&connection, revision, captured);
            let finish = if result.is_ok() { "COMMIT" } else { "ROLLBACK" };
            match connection.execute_batch(finish) {
                Ok(()) => result,
                Err(error) => {
                    let _ = connection.execute_batch("ROLLBACK");
                    Err(error.to_string())
                }
            }
        })();
        let mut readers = self
            .readers
            .lock()
            .map_err(|_| "Store reader pool lock poisoned".to_string())?;
        if readers.len() < MAX_IDLE_READERS {
            readers.push(connection);
        }
        result
    }

    fn with_published_snapshot<T>(
        &self,
        operation: impl FnOnce(&Connection, u64) -> Result<T, String>,
    ) -> Result<T, String> {
        let connection = self
            .readers
            .lock()
            .map_err(|_| "Store reader pool lock poisoned".to_string())?
            .pop()
            .map(Ok)
            .unwrap_or_else(|| open_connection(&self.path, true))?;

        let snapshot = (|| {
            connection
                .execute_batch("BEGIN DEFERRED")
                .map_err(|error| error.to_string())?;
            let revision = match schema::revision(&connection) {
                Ok(revision) => revision,
                Err(error) => {
                    let _ = connection.execute_batch("ROLLBACK");
                    return Err(error.to_string());
                }
            };
            let result = operation(&connection, revision);
            let finish = if result.is_ok() { "COMMIT" } else { "ROLLBACK" };
            match connection.execute_batch(finish) {
                Ok(()) => result,
                Err(error) => {
                    let _ = connection.execute_batch("ROLLBACK");
                    Err(error.to_string())
                }
            }
        })();

        let mut readers = self
            .readers
            .lock()
            .map_err(|_| "Store reader pool lock poisoned".to_string())?;
        if readers.len() < MAX_IDLE_READERS {
            readers.push(connection);
        }
        snapshot
    }

    #[track_caller]
    pub fn transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<(T, u64), String> {
        self.transaction_with_priority(WritePriority::Foreground, operation)
    }

    #[track_caller]
    pub(crate) fn transaction_cloud<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<(T, u64), String> {
        self.transaction_with_priority(WritePriority::Cloud, operation)
    }

    #[track_caller]
    fn transaction_with_priority<T>(
        &self,
        priority: WritePriority,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<(T, u64), String> {
        let _permit = self.writer_admission.acquire(priority)?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let cloud_capture = crate::cloud::capture::SemanticCapture::start(&transaction)
            .map_err(|error| error.to_string())?;
        let value = operation(&transaction).map_err(|error| error.to_string())?;
        schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
        cloud_capture
            .finish(&transaction)
            .map_err(|error| error.to_string())?;
        let revision =
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?;
        let _publication = self.consistency_write(std::panic::Location::caller())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok((value, revision))
    }

    #[track_caller]
    pub fn transaction_if_changed<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, bool)>,
    ) -> Result<(T, u64, bool), String> {
        self.transaction_if_changed_with_priority(WritePriority::Foreground, operation)
    }

    #[track_caller]
    pub(crate) fn transaction_if_changed_background<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, bool)>,
    ) -> Result<(T, u64, bool), String> {
        self.transaction_if_changed_with_priority(WritePriority::Background, operation)
    }

    #[track_caller]
    pub(crate) fn transaction_if_changed_cloud<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, bool)>,
    ) -> Result<(T, u64, bool), String> {
        self.transaction_if_changed_with_priority(WritePriority::Cloud, operation)
    }

    #[track_caller]
    fn transaction_if_changed_with_priority<T>(
        &self,
        priority: WritePriority,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, bool)>,
    ) -> Result<(T, u64, bool), String> {
        let _permit = self.writer_admission.acquire(priority)?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let cloud_capture = crate::cloud::capture::SemanticCapture::start(&transaction)
            .map_err(|error| error.to_string())?;
        let (value, changed) = operation(&transaction).map_err(|error| error.to_string())?;
        schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
        let revision = if changed {
            cloud_capture
                .finish(&transaction)
                .map_err(|error| error.to_string())?;
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            drop(cloud_capture);
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };
        let _publication = self.consistency_write(std::panic::Location::caller())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok((value, revision, changed))
    }

    /// Commit SQLite and settle derived state under the brief publication gate.
    /// Settlement must be bounded; callers decide how to recover a failed
    /// rebuildable component after this method releases the gate.
    #[track_caller]
    pub(crate) fn transaction_settled<T, D, P: PreparedSettlement>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64), String> {
        self.transaction_settled_captured(
            || (),
            |transaction, ()| operation(transaction),
            prepare,
            publish,
        )
    }

    /// Capture immutable projection state after writer admission so SQL and
    /// bitmap-owned organization are read from one published revision.
    #[track_caller]
    pub(crate) fn transaction_settled_captured<T, D, P: PreparedSettlement, C>(
        &self,
        capture: impl FnOnce() -> C,
        operation: impl FnOnce(&Transaction<'_>, C) -> rusqlite::Result<(T, D)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64), String> {
        let _permit = self.writer_admission.acquire(WritePriority::Foreground)?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let cloud_capture = crate::cloud::capture::SemanticCapture::start(&transaction)
            .map_err(|error| error.to_string())?;
        let captured = capture();
        let total_started = std::time::Instant::now();
        let (value, delta) =
            operation(&transaction, captured).map_err(|error| error.to_string())?;
        let operation_elapsed = total_started.elapsed();
        let read_models_started = std::time::Instant::now();
        schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
        let read_models_elapsed = read_models_started.elapsed();
        let cloud_started = std::time::Instant::now();
        cloud_capture
            .finish(&transaction)
            .map_err(|error| error.to_string())?;
        let cloud_elapsed = cloud_started.elapsed();
        let revision =
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?;
        let prepare_started = std::time::Instant::now();
        let mut prepared = prepare(delta)?;
        let prepare_elapsed = prepare_started.elapsed();
        let persist_started = std::time::Instant::now();
        prepared.persist(&transaction, revision)?;
        let persist_elapsed = persist_started.elapsed();
        let commit_started = std::time::Instant::now();
        let _publication = self.consistency_write(std::panic::Location::caller())?;
        transaction.commit().map_err(|error| error.to_string())?;
        let commit_elapsed = commit_started.elapsed();
        publish(prepared);
        if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some()
            && total_started.elapsed() >= std::time::Duration::from_millis(100)
        {
            eprintln!(
                "settled_store_stages total_ms={:.2} operation_ms={:.2} read_models_ms={:.2} cloud_ms={:.2} prepare_ms={:.2} persist_ms={:.2} commit_ms={:.2}",
                total_started.elapsed().as_secs_f64() * 1_000.0,
                operation_elapsed.as_secs_f64() * 1_000.0,
                read_models_elapsed.as_secs_f64() * 1_000.0,
                cloud_elapsed.as_secs_f64() * 1_000.0,
                prepare_elapsed.as_secs_f64() * 1_000.0,
                persist_elapsed.as_secs_f64() * 1_000.0,
                commit_elapsed.as_secs_f64() * 1_000.0,
            );
        }
        Ok((value, revision))
    }

    #[track_caller]
    pub(crate) fn transaction_if_changed_settled<T, D, P: PreparedSettlement>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, bool), String> {
        self.transaction_if_changed_settled_inner(
            WritePriority::Foreground,
            operation,
            prepare,
            publish,
            true,
        )
    }

    /// Visible ingest publishes exact canonical and projection state, but it
    /// yields writer admission to direct user mutations.
    #[track_caller]
    pub(crate) fn transaction_if_changed_settled_maintenance<T, D, P: PreparedSettlement>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, bool), String> {
        self.transaction_if_changed_settled_inner(
            WritePriority::Maintenance,
            operation,
            prepare,
            publish,
            true,
        )
    }

    /// Remote mutations are durable maintenance work. They yield writer
    /// admission to interactive mutations while retaining the same atomic
    /// commit/projection publication boundary.
    #[track_caller]
    pub(crate) fn transaction_if_changed_settled_without_cloud_maintenance<
        T,
        D,
        P: PreparedSettlement,
    >(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, bool), String> {
        self.transaction_if_changed_settled_inner(
            WritePriority::Cloud,
            operation,
            prepare,
            publish,
            false,
        )
    }

    #[track_caller]
    fn transaction_if_changed_settled_inner<T, D, P: PreparedSettlement>(
        &self,
        priority: WritePriority,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
        capture_cloud: bool,
    ) -> Result<(T, u64, bool), String> {
        let _permit = self.writer_admission.acquire(priority)?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let mut cloud_capture = capture_cloud
            .then(|| crate::cloud::capture::SemanticCapture::start(&transaction))
            .transpose()
            .map_err(|error| error.to_string())?;
        let total_started = std::time::Instant::now();
        let (value, delta, changed) = operation(&transaction).map_err(|error| error.to_string())?;
        let operation_elapsed = total_started.elapsed();
        let read_models_started = std::time::Instant::now();
        schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
        let read_models_elapsed = read_models_started.elapsed();
        let cloud_started = std::time::Instant::now();
        let revision = if changed {
            if let Some(cloud_capture) = cloud_capture.take() {
                cloud_capture
                    .finish(&transaction)
                    .map_err(|error| error.to_string())?;
            }
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };
        drop(cloud_capture);
        let cloud_elapsed = cloud_started.elapsed();
        let prepare_started = std::time::Instant::now();
        let mut prepared = changed.then(|| prepare(delta)).transpose()?;
        let prepare_elapsed = prepare_started.elapsed();
        let persist_started = std::time::Instant::now();
        if let Some(prepared) = prepared.as_mut() {
            prepared.persist(&transaction, revision)?;
        }
        let persist_elapsed = persist_started.elapsed();
        let commit_started = std::time::Instant::now();
        let _publication = self.consistency_write(std::panic::Location::caller())?;
        transaction.commit().map_err(|error| error.to_string())?;
        let commit_elapsed = commit_started.elapsed();
        if let Some(prepared) = prepared {
            publish(prepared);
        }
        if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some()
            && total_started.elapsed() >= std::time::Duration::from_millis(100)
        {
            eprintln!(
                "settled_store_stages total_ms={:.2} operation_ms={:.2} read_models_ms={:.2} cloud_ms={:.2} prepare_ms={:.2} persist_ms={:.2} commit_ms={:.2}",
                total_started.elapsed().as_secs_f64() * 1_000.0,
                operation_elapsed.as_secs_f64() * 1_000.0,
                read_models_elapsed.as_secs_f64() * 1_000.0,
                cloud_elapsed.as_secs_f64() * 1_000.0,
                prepare_elapsed.as_secs_f64() * 1_000.0,
                persist_elapsed.as_secs_f64() * 1_000.0,
                commit_elapsed.as_secs_f64() * 1_000.0,
            );
        }
        Ok((value, revision, changed))
    }

    pub fn revision(&self) -> Result<u64, String> {
        self.read(schema::revision)
    }

    /// Materialize every dirty FTS row. This explicit path is reserved for
    /// conversion, repair, and focused verification; normal runtime work uses
    /// the bounded maintenance method below.
    pub fn refresh_search_indexes(&self) -> Result<Option<u64>, String> {
        let (_, revision, changed) = self.transaction_if_changed_settled_inner(
            WritePriority::SearchMaintenance,
            |transaction| {
                let changed = schema::search_indexes_dirty(transaction)?;
                if changed {
                    schema::refresh_search_indexes(transaction)?;
                }
                Ok(((), (), changed))
            },
            |()| Ok(()),
            |()| {},
            false,
        )?;
        Ok(changed.then_some(revision))
    }

    /// Materialize a small FTS batch as lowest-priority rebuildable work. The
    /// canonical mutation has already committed, so foreground work may
    /// interleave between these transactions.
    pub fn maintain_search_indexes(&self, limit: usize) -> Result<Option<u64>, String> {
        let (_, revision, changed) = self.transaction_if_changed_settled_inner(
            WritePriority::SearchMaintenance,
            |transaction| {
                // One category can remain continuously dirty during ingest.
                // Give every category an explicit bounded turn rather than
                // repeatedly selecting the first non-empty queue.
                let category_allowance = limit.max(1).div_ceil(3);
                let mut processed = 0;
                for category in [
                    schema::SearchCategory::Name,
                    schema::SearchCategory::Notes,
                    schema::SearchCategory::Source,
                ] {
                    let batch = schema::refresh_search_indexes_category_batch(
                        transaction,
                        category,
                        category_allowance,
                    )?;
                    processed += batch.processed;
                }
                let changed = processed > 0;
                Ok(((), (), changed))
            },
            |()| Ok(()),
            |()| {},
            false,
        )?;
        Ok(changed.then_some(revision))
    }

    fn consistency_write(
        &self,
        caller: &'static std::panic::Location<'static>,
    ) -> Result<TrackedPublicationWrite<'_>, String> {
        let started = Instant::now();
        let guard = self
            .publication_gate
            .lock()
            .map_err(|_| "Store publication gate poisoned".to_string())?;
        Ok(TrackedPublicationWrite {
            _guard: guard,
            caller,
            wait: started.elapsed(),
            acquired_at: Instant::now(),
            samples: &self.publication_samples,
            total_wait_micros: &self.publication_wait_micros,
            total_hold_micros: &self.publication_hold_micros,
            max_wait_micros: &self.publication_max_wait_micros,
            max_hold_micros: &self.publication_max_hold_micros,
        })
    }
}

fn open_connection(path: &Path, read_only: bool) -> Result<Connection, String> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
    let pragmas = if read_only {
        "PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -16384;
         PRAGMA mmap_size = 1073741824;
         PRAGMA temp_store = MEMORY;
         PRAGMA query_only = ON;"
    } else {
        "PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -65536;
         PRAGMA mmap_size = 1073741824;
         PRAGMA cache_spill = OFF;
         PRAGMA temp_store = MEMORY;
         PRAGMA wal_autocheckpoint = 0;"
    };
    connection
        .execute_batch(pragmas)
        .map_err(|error| format!("Failed to configure SQLite: {error}"))?;
    connection
        .create_aggregate_function(
            "picto_bit_or",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            BitOr,
        )
        .map_err(|error| format!("Failed to register SQLite helpers: {error}"))?;
    Ok(connection)
}

struct BitOr;

impl Aggregate<i64, i64> for BitOr {
    fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<i64> {
        Ok(0)
    }

    fn step(&self, context: &mut Context<'_>, accumulated: &mut i64) -> rusqlite::Result<()> {
        *accumulated |= context.get::<i64>(0)?;
        Ok(())
    }

    fn finalize(&self, _: &mut Context<'_>, accumulated: Option<i64>) -> rusqlite::Result<i64> {
        Ok(accumulated.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::{Store, WritePriority, DATABASE_FILE};
    use rusqlite::Connection;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn incompatible_database_is_rejected_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_meta (
                     singleton INTEGER PRIMARY KEY,
                     schema_version INTEGER NOT NULL,
                     revision INTEGER NOT NULL
                 );
                 INSERT INTO library_meta VALUES (1, 1, 27);
                 CREATE TABLE foreign_backend_state (value TEXT NOT NULL);
                 INSERT INTO foreign_backend_state VALUES ('preserve me');",
            )
            .unwrap();
        drop(connection);
        let before = fs::read(&path).unwrap();

        let error = match Store::open(directory.path()) {
            Ok(_) => panic!("incompatible database opened successfully"),
            Err(error) => error,
        };

        assert!(error.contains("Invalid Picto library schema"));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!path.with_extension("sqlite-wal").exists());
        assert!(!path.with_extension("sqlite-shm").exists());
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT value FROM foreign_backend_state", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "preserve me"
        );
    }

    #[test]
    fn transaction_commits_one_revision() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        assert_eq!(store.revision().unwrap(), 1);

        let (_, revision) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES ('item-a', 'media', 'now', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(revision, 2);
        assert_eq!(store.revision().unwrap(), 2);
    }

    #[test]
    fn cloud_session_capture_is_not_created_until_sync_is_enabled() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();

        let (disabled_count, _) = store
            .transaction(|transaction| {
                transaction.query_row(
                    "SELECT COUNT(*) FROM sqlite_temp_master
                     WHERE type = 'table' AND name = 'cloud_capture_operation'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(disabled_count, 0);

        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET provider = 'dropbox' WHERE singleton = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let (enabled_count, _) = store
            .transaction(|transaction| {
                transaction.query_row(
                    "SELECT COUNT(*) FROM sqlite_temp_master
                     WHERE type = 'table' AND name = 'cloud_capture_operation'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(enabled_count, 1);
    }

    #[test]
    fn repeated_reads_reuse_a_configured_connection() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();

        assert_eq!(store.revision().unwrap(), 1);
        assert_eq!(store.revision().unwrap(), 1);
        assert_eq!(store.readers.lock().unwrap().len(), 1);
    }

    #[test]
    fn one_bounded_search_batch_settles_all_canonical_text_categories() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|transaction| {
                transaction.execute_batch(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (1, 'hash-1', 'image/png', 1, 'now');
                     INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'item:1', 'media', 'now', 'now');
                     INSERT INTO media_asset
                         (item_id, file_id, name, imported_at, updated_at)
                     VALUES (1, 1, 'Media', 'now', 'now');
                     INSERT INTO library_root (item_id, lifecycle)
                     VALUES (1, 'active');
                     INSERT INTO root_metadata
                         (root_item_id, name, notes, source_urls_json, updated_at)
                     VALUES (1, 'Item', 'Notes', '[\"https://example.test/item\"]', 'now');
                     INSERT INTO source_post
                         (source_post_id, site_id, post_key, title, description, root_item_id,
                          created_at, updated_at)
                     VALUES (1, 'test', 'post:1', 'Source', 'Source text', 1, 'now', 'now');
                     INSERT INTO source_item
                         (source_item_id, source_post_id, item_key, position, media_item_id,
                          state, created_at, updated_at)
                     VALUES (1, 1, 'source:1', 0, 1, 'ingested', 'now', 'now');",
                )?;
                Ok(())
            })
            .unwrap();

        assert!(store.maintain_search_indexes(128).unwrap().is_some());
        let counts = store
            .read_snapshot(|connection| {
                connection.query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM root_name_fts),
                         (SELECT COUNT(*) FROM root_notes_fts),
                         (SELECT COUNT(*) FROM source_text_fts),
                         (SELECT COUNT(*) FROM search_dirty_name) +
                         (SELECT COUNT(*) FROM search_dirty_notes) +
                         (SELECT COUNT(*) FROM search_dirty_source)",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
            })
            .unwrap();
        assert_eq!(counts, (1, 1, 1, 0));
    }

    #[test]
    fn sqlite_snapshot_reads_do_not_wait_for_an_uncommitted_writer() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let writer = Arc::clone(&store);
        let (started_tx, transaction_started) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();

        let handle = std::thread::spawn(move || {
            writer
                .transaction(|transaction| {
                    transaction.execute(
                        "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                         VALUES ('pending-item', 'media', 'now', 'now')",
                        [],
                    )?;
                    started_tx.send(()).unwrap();
                    release.recv().unwrap();
                    Ok(())
                })
                .unwrap();
        });

        transaction_started.recv().unwrap();
        let visible_count = store
            .read_snapshot(|connection| {
                connection.query_row("SELECT COUNT(*) FROM library_item", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .unwrap();
        assert_eq!(visible_count, 0);

        release_tx.send(()).unwrap();
        handle.join().unwrap();
        assert_eq!(
            store
                .read_snapshot(|connection| {
                    connection.query_row("SELECT COUNT(*) FROM library_item", [], |row| {
                        row.get::<_, i64>(0)
                    })
                })
                .unwrap(),
            1,
        );
    }

    #[test]
    fn published_snapshot_releases_gate_before_query_work() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let reader = Arc::clone(&store);
        let (query_started_tx, query_started_rx) = mpsc::channel();
        let (release_query_tx, release_query_rx) = mpsc::channel();

        let read = std::thread::spawn(move || {
            reader
                .read_snapshot_captured(
                    || (),
                    |connection, _revision, ()| {
                        query_started_tx.send(()).unwrap();
                        release_query_rx.recv().unwrap();
                        connection
                            .query_row("SELECT COUNT(*) FROM library_item", [], |row| {
                                row.get::<_, i64>(0)
                            })
                            .map_err(|error| error.to_string())
                    },
                )
                .unwrap()
        });
        query_started_rx.recv().unwrap();

        let writer = Arc::clone(&store);
        let (committed_tx, committed_rx) = mpsc::channel();
        let write = std::thread::spawn(move || {
            writer
                .transaction(|transaction| {
                    transaction.execute(
                        "INSERT INTO library_item
                             (item_key, kind, created_at, updated_at)
                         VALUES ('published-while-reading', 'media', 'now', 'now')",
                        [],
                    )?;
                    Ok(())
                })
                .unwrap();
            committed_tx.send(()).unwrap();
        });

        committed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("query work retained the publication gate");
        release_query_tx.send(()).unwrap();
        assert_eq!(read.join().unwrap(), 0);
        write.join().unwrap();
        assert_eq!(store.revision().unwrap(), 2);
    }

    #[test]
    fn sqlite_only_reads_do_not_enter_the_publication_gate() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let publication = store.publication_gate.lock().unwrap();
        let reader = Arc::clone(&store);
        let (finished_tx, finished_rx) = mpsc::channel();

        let read = std::thread::spawn(move || {
            let revision = reader.revision().unwrap();
            finished_tx.send(revision).unwrap();
        });

        assert_eq!(
            finished_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("SQLite-only read waited for the projection publication gate"),
            1,
        );
        drop(publication);
        read.join().unwrap();
    }

    #[test]
    fn failed_projection_preparation_aborts_before_sqlite_commit() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let published = AtomicBool::new(false);

        let result = store.transaction_if_changed_settled(
            |transaction| {
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES ('must-rollback', 'media', 'now', 'now')",
                    [],
                )?;
                Ok(((), (), true))
            },
            |()| Err::<(), _>("invalid prepared projection".to_string()),
            |()| published.store(true, Ordering::Release),
        );

        assert_eq!(result.unwrap_err(), "invalid prepared projection");
        assert!(!published.load(Ordering::Acquire));
        assert_eq!(store.revision().unwrap(), 1);
        assert_eq!(
            store
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_key = 'must-rollback'",
                    [],
                    |row| row.get::<_, i64>(0),
                ))
                .unwrap(),
            0,
        );
    }

    #[test]
    fn foreground_writer_overtakes_queued_maintenance() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let active = store
            .writer_admission
            .acquire(WritePriority::Maintenance)
            .unwrap();
        let (order_tx, order_rx) = mpsc::channel();

        let foreground_store = Arc::clone(&store);
        let foreground_tx = order_tx.clone();
        let foreground = std::thread::spawn(move || {
            let _permit = foreground_store
                .writer_admission
                .acquire(WritePriority::Foreground)
                .unwrap();
            foreground_tx.send("foreground").unwrap();
        });

        while store
            .writer_admission
            .state
            .lock()
            .unwrap()
            .foreground_waiters
            == 0
        {
            std::thread::yield_now();
        }

        let maintenance_store = Arc::clone(&store);
        let maintenance = std::thread::spawn(move || {
            let _permit = maintenance_store
                .writer_admission
                .acquire(WritePriority::Maintenance)
                .unwrap();
            order_tx.send("maintenance").unwrap();
        });

        drop(active);
        assert_eq!(order_rx.recv().unwrap(), "foreground");
        assert_eq!(order_rx.recv().unwrap(), "maintenance");
        foreground.join().unwrap();
        maintenance.join().unwrap();
    }

    #[test]
    fn writer_admission_orders_foreground_then_maintenance_then_background() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let active = store
            .writer_admission
            .acquire(WritePriority::Background)
            .unwrap();
        let (order_tx, order_rx) = mpsc::channel();

        let spawn_waiter = |priority, name| {
            let store = Arc::clone(&store);
            let order_tx = order_tx.clone();
            std::thread::spawn(move || {
                let _permit = store.writer_admission.acquire(priority).unwrap();
                order_tx.send(name).unwrap();
            })
        };
        let background = spawn_waiter(WritePriority::Background, "background");
        let maintenance = spawn_waiter(WritePriority::Maintenance, "maintenance");
        let foreground = spawn_waiter(WritePriority::Foreground, "foreground");

        loop {
            let state = store.writer_admission.state.lock().unwrap();
            if state.foreground_waiters == 1
                && state.maintenance_waiters == 1
                && state.background_waiters == 1
            {
                break;
            }
            drop(state);
            std::thread::yield_now();
        }

        drop(active);
        assert_eq!(order_rx.recv().unwrap(), "foreground");
        assert_eq!(order_rx.recv().unwrap(), "maintenance");
        assert_eq!(order_rx.recv().unwrap(), "background");
        foreground.join().unwrap();
        maintenance.join().unwrap();
        background.join().unwrap();
    }
}
