use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{Condvar, Mutex, RwLock};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::schema;
use crate::{LibraryError, Result};

const DEFAULT_READERS: usize = 8;

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
            self.available.wait(&mut permits);
        }
        *permits -= 1;
        drop(permits);
        let connection = if let Some(connection) = self.idle.lock().pop() {
            connection
        } else {
            match open_connection(&self.path, false) {
                Ok(connection) => connection,
                Err(error) => {
                    *self.permits.lock() += 1;
                    self.available.notify_one();
                    return Err(error);
                }
            }
        };
        Ok(ReadLease {
            connection: Some(connection),
            pool: self.clone(),
        })
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
        let path = path.as_ref().to_path_buf();
        let writer = open_connection(&path, true)?;
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

    pub fn published_write<P, T, D, A>(
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
        let (output, delta) = operation(&transaction, base_revision, revision, prepared)?;
        transaction.execute(
            "UPDATE library_meta SET revision = ?1 WHERE singleton = 1",
            [revision as i64],
        )?;
        let after_publication = {
            let _gate = self.publication_gate.write();
            transaction.commit()?;
            publish(revision, delta)
        };
        Ok((output, revision, after_publication))
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
        transaction.commit()?;
        Ok(output)
    }

    pub fn allocate_id(transaction: &Transaction<'_>) -> Result<u32> {
        let next = transaction.query_row(
            "SELECT next_local_id FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0).map(|value| value as u64),
        )?;
        if next == 0 || next > u32::MAX as u64 {
            return Err(LibraryError::InvalidState(
                "local ID space is exhausted".into(),
            ));
        }
        transaction.execute(
            "UPDATE library_meta SET next_local_id = next_local_id + 1 WHERE singleton = 1",
            [],
        )?;
        Ok(next as u32)
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
