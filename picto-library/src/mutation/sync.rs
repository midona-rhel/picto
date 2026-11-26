//! Cloud sync: turning the recording of changes on and off, reading the
//! outbox, and applying change records from other computers.

use super::*;
use crate::sync::SyncRecord;

impl Library {
    /// Turns sync on for this library: gives it a new device id and records
    /// its full state into the synced values. From then on every write
    /// records what it changed, in the synced values and in the outbox.
    pub fn start_sync_recording(&self) -> Result<MutationReceipt> {
        self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            if crate::sync::recording(transaction)? {
                return Err(LibraryError::InvalidState("sync is already on".into()));
            }
            transaction.execute(
                "INSERT INTO sync_state(singleton, device_id, clock_ms, clock_counter)
                 VALUES (1, ?1, 0, 0)",
                [uuid::Uuid::new_v4().to_string()],
            )?;
            crate::sync::install_triggers(transaction)?;
            crate::sync::record_full(transaction, &snapshot)?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
            Ok((receipt.clone(), PublishedDelta { snapshot: next, receipt, history: None }))
        })
    }

    /// Turns sync off: the library stops recording and its sync tables are
    /// cleared.
    pub fn stop_sync_recording(&self) -> Result<MutationReceipt> {
        self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            crate::sync::remove_triggers(transaction)?;
            transaction.execute_batch(
                "DELETE FROM sync_state; DELETE FROM sync_value; DELETE FROM sync_outbox;
                 DELETE FROM sync_identity; DELETE FROM sync_device;",
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
            Ok((receipt.clone(), PublishedDelta { snapshot: next, receipt, history: None }))
        })
    }

    /// The change records of this library that no change file holds yet.
    pub fn sync_outbox(&self) -> Result<Vec<SyncRecord>> {
        self.database
            .read(WorkPriority::Maintenance, crate::sync::outbox)
    }

    /// Removes `records` from the outbox once a change file holds them. A
    /// record of the same field that the library made since stays.
    pub fn remove_from_sync_outbox(&self, records: &[SyncRecord]) -> Result<()> {
        self.database.maintenance_write(WorkPriority::Maintenance, |transaction| {
            crate::sync::remove_from_outbox(transaction, records)
        })
    }

    /// Merges change records from other computers into the synced values
    /// and writes what they change into the library, in one write. The
    /// write has no undo entry.
    pub fn apply_sync_records(&self, records: &[SyncRecord]) -> Result<MutationReceipt> {
        self.publish(WorkPriority::CanonicalIngest, |transaction, revision, snapshot| {
            if !crate::sync::recording(transaction)? {
                return Err(LibraryError::InvalidState("sync is off".into()));
            }
            let mut next = (*snapshot).clone();
            let protected = self.history.protected_cleanup_files();
            let projected = crate::sync::apply(transaction, revision, &mut next, records, &protected)?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                projected.resources,
                projected.roots.iter().map(RootId),
            );
            Ok((receipt.clone(), PublishedDelta { snapshot: next, receipt, history: None }))
        })
    }

    /// The records that a full comparison of the library with the synced
    /// values would make. It is empty after every write.
    #[cfg(test)]
    pub(crate) fn sync_differences(
        &self,
    ) -> Result<Vec<(crate::sync::SyncKind, String, String, serde_json::Value)>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |transaction, snapshot| crate::sync::differences(transaction, &snapshot),
        )
    }
}
