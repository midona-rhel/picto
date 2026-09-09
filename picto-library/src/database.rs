use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex, RwLock};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::query_request::{OpenCancellation, OpenStage};
use crate::schema;
use crate::{LibraryError, Result};

const DEFAULT_READERS: usize = 8;
const GATE_HISTOGRAM_BUCKETS: usize = 32;

/// A prepared publication settles its resident derived state in the same
/// transaction as canonical data, before either becomes visible to readers.
pub trait PublicationDelta {
    fn settle(&mut self, transaction: &Transaction<'_>) -> Result<()>;
}

/// Raw canonical writes publish no projection and settle nothing; only
/// tests use them, against databases no library has open.
impl PublicationDelta for () {
    fn settle(&mut self, _transaction: &Transaction<'_>) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicationGateStats {
    pub samples: u64,
    pub p95_micros: u64,
    pub max_micros: u64,
}

struct PublicationGateMetrics {
    samples: AtomicU64,
    max_micros: AtomicU64,
    histogram: [AtomicU64; GATE_HISTOGRAM_BUCKETS],
}

impl Default for PublicationGateMetrics {
    fn default() -> Self {
        Self {
            samples: AtomicU64::new(0),
            max_micros: AtomicU64::new(0),
            histogram: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl PublicationGateMetrics {
    fn record(&self, micros: u64) {
        let bucket = if micros <= 1 {
            0
        } else {
            (u64::BITS - (micros - 1).leading_zeros()) as usize
        }
        .min(GATE_HISTOGRAM_BUCKETS - 1);
        self.histogram[bucket].fetch_add(1, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.max_micros.fetch_max(micros, Ordering::Relaxed);
    }

    fn snapshot(&self) -> PublicationGateStats {
        let samples = self.samples.load(Ordering::Relaxed);
        if samples == 0 {
            return PublicationGateStats::default();
        }
        let target = samples.saturating_mul(95).div_ceil(100);
        let mut cumulative = 0;
        let mut p95_micros = 0;
        for (bucket, count) in self.histogram.iter().enumerate() {
            cumulative += count.load(Ordering::Relaxed);
            if cumulative >= target {
                p95_micros = 1u64 << bucket;
                break;
            }
        }
        PublicationGateStats {
            samples,
            p95_micros,
            max_micros: self.max_micros.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkPriority {
    ForegroundMutation = 1,
    VisibleRead = 2,
    CanonicalIngest = 3,
    CorrectnessRecovery = 4,
    Maintenance = 5,
    Fts = 6,
    Cloud = 7,
}

impl WorkPriority {
    const COUNT: usize = 7;

    const fn index(self) -> usize {
        self as usize - 1
    }
}

#[derive(Default)]
struct SchedulerState {
    active: bool,
    waiting: [usize; WorkPriority::COUNT],
}

#[derive(Default)]
struct WriterScheduler {
    state: Mutex<SchedulerState>,
    available: Condvar,
}

impl WriterScheduler {
    fn acquire(self: &Arc<Self>, priority: WorkPriority) -> WriterLease {
        let index = priority.index();
        let mut state = self.state.lock();
        state.waiting[index] += 1;
        while state.active || state.waiting[..index].iter().any(|count| *count > 0) {
            self.available.wait(&mut state);
        }
        state.waiting[index] -= 1;
        state.active = true;
        WriterLease {
            scheduler: self.clone(),
        }
    }

    fn has_higher_priority_waiter(&self, priority: WorkPriority) -> bool {
        self.state.lock().waiting[..priority.index()]
            .iter()
            .any(|count| *count > 0)
    }
}

struct WriterLease {
    scheduler: Arc<WriterScheduler>,
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        let mut state = self.scheduler.state.lock();
        state.active = false;
        self.scheduler.available.notify_all();
    }
}

struct ReadPool {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
    permits: Mutex<usize>,
    available: Condvar,
    maximum: usize,
}

impl ReadPool {
    fn acquire(self: &Arc<Self>) -> Result<ReadLease> {
        let mut permits = self.permits.lock();
        while *permits == 0 {
            crate::query_request::check()?;
            self.available
                .wait_for(&mut permits, std::time::Duration::from_millis(5));
        }
        crate::query_request::check()?;
        *permits -= 1;
        drop(permits);
        // Take the idle connection in its own lock scope. Opening a connection
        // and restoring a permit must both run with no pool lock held: the
        // idle lock must never be held while the permits lock is taken.
        let idle = {
            let mut idle = self.idle.lock();
            idle.pop()
        };
        let connection = match idle {
            Some(connection) => connection,
            None => match open_connection(&self.path, false) {
                Ok(connection) => connection,
                Err(error) => {
                    self.restore_permit();
                    return Err(error);
                }
            },
        };
        Ok(ReadLease {
            connection: Some(connection),
            pool: self.clone(),
        })
    }

    fn restore_permit(&self) {
        let mut permits = self.permits.lock();
        *permits = (*permits + 1).min(self.maximum);
        self.available.notify_one();
    }
}

struct ReadLease {
    connection: Option<Connection>,
    pool: Arc<ReadPool>,
}

impl Drop for ReadLease {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.idle.lock().push(connection);
        }
        let mut permits = self.pool.permits.lock();
        *permits = (*permits + 1).min(self.pool.maximum);
        self.pool.available.notify_one();
    }
}

pub struct LibraryDatabase {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: Arc<ReadPool>,
    publication_gate: RwLock<()>,
    publication_gate_metrics: PublicationGateMetrics,
    scheduler: Arc<WriterScheduler>,
}

impl LibraryDatabase {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(LibraryError::InvalidInput(format!(
                "refusing to replace existing database {}",
                path.display()
            )));
        }
        let mut writer = open_connection(&path, true)?;
        schema::create(&mut writer)?;
        Self::from_writer(path, writer)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, &OpenCancellation::default(), &mut |_| {})
    }

    /// Open, migrating a supported predecessor schema first. The migration
    /// reports `OpenStage::Migrating` and honours the cancellation handle.
    pub fn open_with(
        path: impl AsRef<Path>,
        cancellation: &OpenCancellation,
        progress: &mut dyn FnMut(OpenStage),
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut writer = open_connection(&path, false)?;
        schema::migrate_for_open(&mut writer, &path, cancellation, progress)?;
        schema::validate(&writer)?;
        Self::from_writer(path, writer)
    }

    fn from_writer(path: PathBuf, writer: Connection) -> Result<Self> {
        Ok(Self {
            path: path.clone(),
            writer: Mutex::new(writer),
            readers: Arc::new(ReadPool {
                path,
                idle: Mutex::new(Vec::new()),
                permits: Mutex::new(DEFAULT_READERS),
                available: Condvar::new(),
                maximum: DEFAULT_READERS,
            }),
            publication_gate: RwLock::new(()),
            publication_gate_metrics: PublicationGateMetrics::default(),
            scheduler: Arc::new(WriterScheduler::default()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn revision(&self) -> Result<u64> {
        self.read(WorkPriority::VisibleRead, |connection| {
            schema::validate(connection)
        })
    }

    pub fn publication_gate_stats(&self) -> PublicationGateStats {
        self.publication_gate_metrics.snapshot()
    }

    pub fn read<T>(
        &self,
        _priority: WorkPriority,
        operation: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        let lease = self.readers.acquire()?;
        operation(
            lease
                .connection
                .as_ref()
                .expect("read lease owns connection"),
        )
    }

    pub fn read_consistent<P, T>(
        &self,
        _priority: WorkPriority,
        capture: impl FnOnce(u64) -> Result<P>,
        operation: impl FnOnce(&Transaction<'_>, P) -> Result<T>,
    ) -> Result<T> {
        let lease = self.readers.acquire()?;
        let connection = lease
            .connection
            .as_ref()
            .expect("read lease owns connection");
        let (transaction, projection) = {
            let _gate = self.publication_gate.read();
            let transaction = connection.unchecked_transaction()?;
            let revision = transaction.query_row(
                "SELECT revision FROM library_meta WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )?;
            (transaction, capture(revision)?)
        };
        let output = operation(&transaction, projection)?;
        transaction.commit()?;
        Ok(output)
    }

    pub fn write<T>(
        &self,
        _priority: WorkPriority,
        operation: impl FnOnce(&Transaction<'_>, u64) -> Result<T>,
    ) -> Result<(T, u64)> {
        let (output, revision, ()) = self.published_write(
            _priority,
            |_| Ok(()),
            |transaction, _, revision, ()| {
                operation(transaction, revision).map(|value| (value, ()))
            },
            |_, ()| {},
        )?;
        Ok((output, revision))
    }

    pub fn published_write<P, T, D: PublicationDelta, A>(
        &self,
        priority: WorkPriority,
        capture: impl FnOnce(u64) -> Result<P>,
        operation: impl FnOnce(&Transaction<'_>, u64, u64, P) -> Result<(T, D)>,
        publish: impl FnOnce(u64, D) -> A,
    ) -> Result<(T, u64, A)> {
        let _scheduler_lease = self.scheduler.acquire(priority);
        let mut writer = self.writer.lock();
        let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let base_revision = transaction.query_row(
            "SELECT revision FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0).map(|value| value as u64),
        )?;
        let revision = base_revision + 1;
        let prepared = capture(base_revision)?;
        let (output, mut delta) = operation(&transaction, base_revision, revision, prepared)?;
        delta.settle(&transaction)?;
        transaction.execute(
            "UPDATE library_meta SET revision = ?1 WHERE singleton = 1",
            [revision as i64],
        )?;
        let (after_publication, gate_micros) = {
            let _gate = self.publication_gate.write();
            let started = Instant::now();
            transaction.commit()?;
            let output = publish(revision, delta);
            (output, started.elapsed().as_micros() as u64)
        };
        self.publication_gate_metrics.record(gate_micros);
        Ok((output, revision, after_publication))
    }

    /// Run a publication only when the prepared operation reports canonical
    /// work. An empty background poll rolls its transaction back and leaves
    /// both the database revision and projection untouched.
    pub fn published_write_if_changed<P, T, D: PublicationDelta, A>(
        &self,
        priority: WorkPriority,
        capture: impl FnOnce(u64) -> Result<P>,
        operation: impl FnOnce(&Transaction<'_>, u64, u64, P) -> Result<Option<(T, D)>>,
        publish: impl FnOnce(u64, D) -> A,
    ) -> Result<Option<(T, u64, A)>> {
        let _scheduler_lease = self.scheduler.acquire(priority);
        let mut writer = self.writer.lock();
        let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let base_revision = transaction.query_row(
            "SELECT revision FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0).map(|value| value as u64),
        )?;
        let revision = base_revision + 1;
        let prepared = capture(base_revision)?;
        let Some((output, mut delta)) = operation(&transaction, base_revision, revision, prepared)?
        else {
            transaction.rollback()?;
            return Ok(None);
        };
        delta.settle(&transaction)?;
        transaction.execute(
            "UPDATE library_meta SET revision = ?1 WHERE singleton = 1",
            [revision as i64],
        )?;
        let (after_publication, gate_micros) = {
            let _gate = self.publication_gate.write();
            let started = Instant::now();
            transaction.commit()?;
            let output = publish(revision, delta);
            (output, started.elapsed().as_micros() as u64)
        };
        self.publication_gate_metrics.record(gate_micros);
        Ok(Some((output, revision, after_publication)))
    }

    pub fn has_higher_priority_waiter(&self, priority: WorkPriority) -> bool {
        self.scheduler.has_higher_priority_waiter(priority)
    }

    pub fn maintenance_write<T>(
        &self,
        priority: WorkPriority,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let _scheduler_lease = self.scheduler.acquire(priority);
        let mut writer = self.writer.lock();
        let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let output = operation(&transaction)?;
        // Maintenance writes publish no snapshot, so they must leave every
        // fact the resident model mirrors untouched. The journal proves it;
        // returning here rolls the transaction back.
        if crate::browsing::DrawingIndex::journal_is_dirty(&transaction)? {
            return Err(LibraryError::InvalidState(
                "maintenance write changed browsing facts without publishing a projection".into(),
            ));
        }
        transaction.commit()?;
        Ok(output)
    }

    pub fn checkpoint_wal(&self) -> Result<()> {
        let _scheduler_lease = self.scheduler.acquire(WorkPriority::Maintenance);
        let writer = self.writer.lock();
        writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    pub fn allocate_id(transaction: &Transaction<'_>) -> Result<u32> {
        transaction
            .prepare_cached(
                "UPDATE library_meta
                 SET next_local_id = next_local_id + 1
                 WHERE singleton = 1 AND next_local_id < 4294967295
                 RETURNING next_local_id - 1",
            )?
            .query_row([], |row| row.get::<_, u32>(0))
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    LibraryError::InvalidState("local ID space is exhausted".into())
                }
                error => error.into(),
            })
    }
}

fn open_connection(path: &Path, create: bool) -> Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let connection = Connection::open_with_flags(path, flags)?;
    // Canonical ingestion and projection settlement share this connection.
    // Keep their bounded statement working set resident between small batches.
    connection.set_prepared_statement_cache_capacity(64);
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA wal_autocheckpoint = 0;",
    )?;
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query_request::{QueryRequest, WindowRequests};

    fn unopenable_pool() -> Arc<ReadPool> {
        // A file in a directory that does not exist can neither be opened nor
        // created: every open attempt fails deterministically on every platform.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing").join("library.sqlite");
        drop(directory);
        Arc::new(ReadPool {
            path,
            idle: Mutex::new(Vec::new()),
            permits: Mutex::new(DEFAULT_READERS),
            available: Condvar::new(),
            maximum: DEFAULT_READERS,
        })
    }

    #[test]
    fn acquire_holds_no_pool_lock_while_opening_and_restores_the_permit_on_failure() {
        let pool = unopenable_pool();
        assert!(pool.acquire().is_err());
        // Both locks are free again and the permit came back.
        assert!(pool.idle.try_lock().is_some());
        assert_eq!(*pool.permits.try_lock().unwrap(), DEFAULT_READERS);
        // A permit restored by a failed open never exceeds the pool maximum.
        pool.restore_permit();
        assert_eq!(*pool.permits.lock(), DEFAULT_READERS);
    }

    #[test]
    fn a_cancelled_open_escapes_an_exhausted_pool_with_open_cancelled() {
        let directory = tempfile::tempdir().unwrap();
        let database = LibraryDatabase::create(directory.path().join("library.sqlite")).unwrap();
        let leases = (0..DEFAULT_READERS)
            .map(|_| database.readers.acquire().unwrap())
            .collect::<Vec<_>>();
        let handle = OpenCancellation::new();
        let token = crate::query_request::Cancellation::for_open(&handle);
        {
            let _scope = token.enter();
            handle.cancel();
            assert!(matches!(
                database.readers.acquire(),
                Err(LibraryError::OpenCancelled)
            ));
            assert_eq!(*database.readers.permits.lock(), 0);
        }
        drop(leases);
        assert_eq!(*database.readers.permits.lock(), DEFAULT_READERS);
    }

    #[test]
    fn superseded_reads_escape_an_exhausted_pool_without_leaking_a_permit() {
        let directory = tempfile::tempdir().unwrap();
        let database = LibraryDatabase::create(directory.path().join("library.sqlite")).unwrap();
        let leases = (0..DEFAULT_READERS)
            .map(|_| database.readers.acquire().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(*database.readers.permits.lock(), 0);
        let requests = WindowRequests::default();
        let request = QueryRequest {
            channel: "grid".into(),
            generation: 1,
        };
        let token = requests.begin(&request).unwrap();
        {
            let _scope = token.enter();
            requests
                .begin(&QueryRequest {
                    generation: 2,
                    ..request
                })
                .unwrap();
            assert!(matches!(
                database.readers.acquire(),
                Err(LibraryError::QuerySuperseded)
            ));
            assert_eq!(*database.readers.permits.lock(), 0);
        }
        drop(leases);
        assert_eq!(*database.readers.permits.lock(), DEFAULT_READERS);
        assert_eq!(
            database
                .read(WorkPriority::VisibleRead, |connection| {
                    Ok(connection.query_row("SELECT 42", [], |row| row.get::<_, i64>(0))?)
                })
                .unwrap(),
            42
        );
        assert_eq!(*database.readers.permits.lock(), DEFAULT_READERS);
    }
}
