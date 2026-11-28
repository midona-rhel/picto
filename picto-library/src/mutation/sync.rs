//! Cloud sync: turning it on and off, running the recorder, reading the
//! outbox, and applying change records from other computers. `cloud` syncs
//! with the cloud folder.

mod cloud;

use super::*;
use crate::sync::SyncRecord;
use rusqlite::Connection;

/// How many entities one library transaction of `apply_sync_records` writes
/// directly (`FIELDS` in `sync_pending`), so that a write of the user waits
/// for it only a few milliseconds: at 1,000,000 roots, 32 new items take
/// about 7 ms. The others are written one at a time, with the entities
/// whose positions changed in the same folders.
const DIRECT_CHUNK: usize = 32;

impl Library {
    pub fn sync_on(&self) -> bool {
        self.sync.on()
    }

    /// Turns sync on for this library: creates the sync file with a new
    /// device id and records the library's full state into it. From then on
    /// the recorder records what changes (`record_sync_changes`).
    pub fn start_sync_recording(&self) -> Result<()> {
        self.start_recording(true)
    }

    /// `start_sync_recording`, which records the full state only when `full`
    /// is true. A new library that is opened from the cloud records nothing,
    /// so that the defaults it was created with do not win over the cloud
    /// library's values.
    pub(super) fn start_recording(&self, full: bool) -> Result<()> {
        if self.sync.on() {
            return Err(LibraryError::InvalidState("sync is already on".into()));
        }
        self.database.maintenance_write(WorkPriority::ForegroundMutation, crate::sync::start_marking)?;
        let started = (|| {
            let mut guard = self.sync.lock(true)?;
            let connection = guard.as_mut().expect("the sync file was just created");
            self.consistent(connection, |transaction, snapshot, revision| {
                transaction.execute(
                    "INSERT INTO sync_state(singleton, device_id, clock_ms, clock_counter, recorded_revision)
                     VALUES (1, ?1, 0, 0, ?2)",
                    rusqlite::params![uuid::Uuid::new_v4().to_string(), revision as i64],
                )?;
                if full {
                    crate::sync::record_full(transaction, snapshot)?;
                }
                Ok(())
            })
        })();
        if started.is_err() {
            let _ = self.stop_sync_recording();
        }
        started
    }

    /// Turns sync off: the sync file is deleted and the library stops
    /// noting its changes.
    pub fn stop_sync_recording(&self) -> Result<()> {
        self.sync.remove()?;
        self.database.maintenance_write(WorkPriority::ForegroundMutation, crate::sync::stop_marking)
    }

    /// Records what changed in the library since the last pass, and writes
    /// the synced values that wait to be written. The sync worker calls it
    /// shortly after writes.
    pub fn record_sync_changes(&self) -> Result<()> {
        let mut guard = self.sync.lock(false)?;
        let Some(connection) = guard.as_mut() else { return Ok(()) };
        self.catch_up(connection)
    }

    /// Waits until the recorder has recorded every write and nothing waits
    /// to be written, or until `timeout` passes. Returns whether it caught up.
    pub fn wait_for_sync_recording(&self, timeout: std::time::Duration) -> Result<bool> {
        let started = std::time::Instant::now();
        loop {
            let caught_up = match self.sync.lock(false)?.as_ref() {
                None => true,
                Some(connection) => connection.query_row(
                    "SELECT recorded_revision = (SELECT revision FROM library.library_meta)
                         AND NOT EXISTS (SELECT 1 FROM sync_pending) FROM sync_state",
                    [],
                    |row| row.get::<_, bool>(0),
                )?,
            };
            if caught_up || started.elapsed() >= timeout {
                return Ok(caught_up);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The change records of this library that no change file holds yet.
    pub fn sync_outbox(&self) -> Result<Vec<SyncRecord>> {
        match self.sync.lock(false)?.as_ref() {
            Some(connection) => crate::sync::outbox(connection),
            None => Ok(Vec::new()),
        }
    }

    /// Removes `records` from the outbox once a change file holds them. A
    /// record of the same field that the library made since stays.
    pub fn remove_from_sync_outbox(&self, records: &[SyncRecord]) -> Result<()> {
        let mut guard = self.sync.lock(false)?;
        let Some(connection) = guard.as_mut() else { return Ok(()) };
        let transaction = connection.transaction()?;
        crate::sync::remove_from_outbox(&transaction, records)?;
        Ok(transaction.commit()?)
    }

    /// Merges change records from other computers into the synced values and
    /// writes what they change into the library. The recorder runs first, so
    /// that the library's own changes are in the synced values before the
    /// records are merged. The writes have no undo entry.
    pub fn apply_sync_records(&self, records: &[SyncRecord]) -> Result<()> {
        let mut guard = self.sync.lock(false)?;
        let Some(connection) = guard.as_mut() else {
            return Err(LibraryError::InvalidState("sync is off".into()));
        };
        self.record_pass(connection, None)?;
        let transaction = connection.transaction()?;
        crate::sync::merge_remote(&transaction, records)?;
        transaction.commit()?;
        self.catch_up(connection)
    }

    /// Runs the recorder, then writes the pending synced values into the
    /// library in small transactions at background priority
    /// (`pending_projection`). A transaction is written only when
    /// the library has not changed since the recorder's last pass; otherwise
    /// the recorder runs again first and records the change, so that the
    /// synced values that are written hold it.
    fn catch_up(&self, connection: &mut Connection) -> Result<()> {
        let mut projected = None;
        loop {
            self.record_pass(connection, projected)?;
            match self.project_next(connection)? {
                None => return Ok(()),
                Some((chunk, written, revision)) => {
                    crate::sync::finish_projection(connection, &chunk, &written)?;
                    projected = revision;
                }
            }
        }
    }

    /// Writes the next entities of `sync_pending` into the library in one
    /// transaction, when the library has not changed since the recorder's
    /// last pass. Returns the entities of `sync_pending` it took, all
    /// entities it wrote in full, and the revision of the write with whether
    /// it changed the structure of the library; nothing of these when the
    /// library had changed, and `None` when nothing waits.
    #[allow(clippy::type_complexity)]
    pub(crate) fn project_next(
        &self,
        connection: &Connection,
    ) -> Result<Option<(Vec<(i64, crate::sync::SyncKind, String, bool)>, BTreeSet<(crate::sync::SyncKind, String)>, Option<(u64, bool)>)>> {
        let (chunk, direct) = crate::sync::pending_projection(connection, DIRECT_CHUNK)?;
        if chunk.is_empty() {
            return Ok(None);
        }
        let recorded = connection.query_row("SELECT recorded_revision FROM sync_state", [], |row| row.get::<_, i64>(0))?;
        let keys = chunk.iter().map(|(_, kind, key, _)| (*kind, key.clone())).collect();
        let revived = chunk.iter().filter(|row| row.3).map(|(_, kind, key, _)| (*kind, key.clone())).collect();
        let mut synced = crate::sync::synced_for_projection(connection, &keys, revived, direct)?;
        let protected = self.history.protected_cleanup_files();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let written = self.database.published_write_if_changed(
            WorkPriority::Maintenance,
            |revision| self.capture_revision(revision),
            |transaction, base, revision, snapshot| {
                if base as i64 != recorded {
                    return Ok(None);
                }
                let mut next = (*snapshot).clone();
                let projected =
                    crate::sync::project(transaction, revision, &mut next, &mut synced, keys, direct, &protected)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(revision, projected.resources, projected.roots.iter().map(RootId));
                Ok(Some(((projected.written, projected.structure), PublishedDelta { snapshot: next, receipt, history: None })))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok(Some(match written {
            Some(((written, structure), revision, _)) => (chunk, written, Some((revision, structure))),
            None => (Vec::new(), BTreeSet::new(), None),
        }))
    }

    /// One pass of the recorder. `projected` is the revision of the last
    /// write of synced values into the library, with whether it changed the
    /// structure of the library, when that write came right after the
    /// previous pass.
    fn record_pass(&self, connection: &mut Connection, projected: Option<(u64, bool)>) -> Result<()> {
        self.consistent(connection, |transaction, snapshot, revision| {
            crate::sync::record(transaction, snapshot, revision, projected)
        })
    }

    /// Runs `operation` in a transaction of the sync file that reads the
    /// library at the revision of the published projection, which it gets
    /// with the revision.
    fn consistent<T>(
        &self,
        connection: &mut Connection,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, &ProjectionSnapshot, u64) -> Result<T>,
    ) -> Result<T> {
        loop {
            let transaction = connection.transaction()?;
            let revision = transaction
                .query_row("SELECT revision FROM library.library_meta", [], |row| row.get::<_, i64>(0))?
                as u64;
            let snapshot = self.projections.snapshot();
            // A write that committed after the read began, or whose
            // projection is not published yet, gives another revision.
            if snapshot.revision == revision {
                let output = operation(&transaction, &snapshot, revision)?;
                transaction.commit()?;
                return Ok(output);
            }
            drop(transaction);
            std::thread::yield_now();
        }
    }

    /// The records that a full comparison of the library with the synced
    /// values would make. It is empty once the recorder has caught up.
    #[cfg(test)]
    pub(crate) fn sync_differences(
        &self,
    ) -> Result<Vec<(crate::sync::SyncKind, String, String, serde_json::Value)>> {
        let mut guard = self.sync.lock(false)?;
        let connection = guard.as_mut().expect("sync is on");
        self.consistent(connection, |transaction, snapshot, _| crate::sync::differences(transaction, snapshot))
    }

    /// Merges `records` and writes the first `writes` library transactions
    /// of them. With `crash`, the sync file does not note that the last one
    /// was written, as a crash right after that transaction leaves the two
    /// files.
    #[cfg(test)]
    pub(crate) fn apply_sync_records_partly(&self, records: &[SyncRecord], writes: usize, crash: bool) -> Result<()> {
        let mut guard = self.sync.lock(false)?;
        let connection = guard.as_mut().expect("sync is on");
        self.record_pass(connection, None)?;
        let transaction = connection.transaction()?;
        crate::sync::merge_remote(&transaction, records)?;
        transaction.commit()?;
        for write in 1..=writes {
            self.record_pass(connection, None)?;
            let Some((chunk, written, _)) = self.project_next(connection)? else { break };
            if !(crash && write == writes) {
                crate::sync::finish_projection(connection, &chunk, &written)?;
            }
        }
        Ok(())
    }

    /// Every synced value as a change record.
    #[cfg(test)]
    pub(crate) fn synced_records(&self) -> Result<Vec<SyncRecord>> {
        crate::sync::all_records(self.sync.lock(false)?.as_ref().expect("sync is on"))
    }
}
