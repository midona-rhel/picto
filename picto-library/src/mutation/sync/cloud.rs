//! Cloud sync through a cloud folder: connecting the library to a cloud
//! library, pausing, and a sync, which reads the change files of the other
//! devices and writes this device's outbox as its next change files.

use std::path::Path;

use super::*;
use crate::sync::cloud::{self, CloudFolder, SyncStop, CHANGE_FILE_RECORDS};
use crate::sync::{SyncComputer, SyncReport};

impl Library {
    /// Connects the library, whose sync is on, to the cloud library
    /// `library_id` in `folder`, the folder of the Google Drive or Dropbox
    /// app. Writes `picto/<library id>/library.json` with the name `name`
    /// when the cloud library has none yet. `computer` is the computer this
    /// runs on; a later sync on another computer finds the database to be a
    /// copy. Connecting again to the same cloud library changes only the
    /// path of the folder.
    pub fn connect_sync_folder(&self, folder: &Path, library_id: &str, name: &str, computer: &SyncComputer) -> Result<()> {
        let folder_text = folder
            .to_str()
            .ok_or_else(|| LibraryError::InvalidInput(format!("the path {} is not valid Unicode", folder.display())))?;
        let guard = self.sync.lock(false)?;
        let Some(connection) = guard.as_ref() else {
            return Err(LibraryError::InvalidState("sync is off".into()));
        };
        CloudFolder::create(folder, library_id, name)?;
        cloud::connect(connection, library_id, folder_text, &computer.machine_id)
    }

    /// Pauses or resumes sync with the cloud folder. While sync is paused the
    /// library records its changes but reads and writes no files.
    pub fn set_sync_paused(&self, paused: bool) -> Result<()> {
        let guard = self.sync.lock(false)?;
        let Some(connection) = guard.as_ref() else {
            return Err(LibraryError::InvalidState("sync is off".into()));
        };
        connection.execute("UPDATE sync_state SET paused = ?1", [paused])?;
        Ok(())
    }

    /// Whether the outbox holds records that no change file holds yet.
    pub fn has_unsent_sync_changes(&self) -> Result<bool> {
        match self.sync.lock(false)?.as_ref() {
            Some(connection) => Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox)", [], |row| row.get(0))?),
            None => Ok(false),
        }
    }

    /// Syncs the library with its cloud folder. Returns `None` without
    /// reading or writing anything when sync is off, the library is not
    /// connected to a cloud folder, or sync is paused. A sync does this, in
    /// order:
    ///
    /// 1. It checks whether this database is a copy of the library. It is a
    ///    copy when the machine id of `computer` is not the one in the sync
    ///    state (the library was copied to another computer), or when this
    ///    device's folder holds a change file after the last one the
    ///    database noted as written (the database was restored from a
    ///    backup, or a sync stopped after writing a change file and before
    ///    noting it). A copy takes a new device id
    ///    (`cloud::take_new_device_id`), so that no two databases write
    ///    change files under the same device id.
    /// 2. It reads the new change files of every other device in sequence
    ///    order and stops at the first one that is missing or cannot be read
    ///    yet. Each file is merged into the synced values in one transaction
    ///    of the sync file, which also notes the file as read, and what it
    ///    changed is then written into the library.
    /// 3. It writes the outbox as this device's next change files, at most
    ///    `CHANGE_FILE_RECORDS` records in each, and after each file removes
    ///    what it holds from the outbox and notes its sequence number.
    /// 4. When it read or wrote a change file, or this device has no
    ///    `device.json` yet, it writes `device.json`.
    ///
    /// The sync file's connection is held for the whole sync, so the
    /// recorder waits for it. Library writes do not wait: the library is
    /// written only in the small transactions of `catch_up`.
    pub fn sync_with_cloud(&self, computer: &SyncComputer) -> Result<Option<SyncReport>> {
        self.sync_with_cloud_until(computer, None)
    }

    /// `sync_with_cloud`, which returns at `stop` without committing or
    /// writing anything after it.
    pub(crate) fn sync_with_cloud_until(&self, computer: &SyncComputer, stop: Option<SyncStop>) -> Result<Option<SyncReport>> {
        let mut guard = self.sync.lock(false)?;
        let Some(connection) = guard.as_mut() else { return Ok(None) };
        let Some(mut state) = cloud::state(connection)? else { return Ok(None) };
        if state.paused {
            return Ok(None);
        }
        let folder = CloudFolder::open(&state.folder, &state.library_id)?;
        let mut report = SyncReport::default();

        if state.machine_id != computer.machine_id || folder.has_change_file(&state.device_id, state.written_seq + 1) {
            let transaction = connection.transaction()?;
            state = cloud::take_new_device_id(&transaction, &computer.machine_id)?;
            transaction.commit()?;
            report.new_device = true;
        }

        let read = cloud::read_positions(connection)?;
        for device in folder.devices()? {
            if device == state.device_id {
                continue;
            }
            let mut seq = read.get(&device).copied().unwrap_or(0);
            while let Some(records) = folder.read_changes(&device, seq + 1) {
                seq += 1;
                // The recorder runs first, so that the library's own changes
                // are in the synced values before the records are merged.
                self.record_pass(connection, None)?;
                let transaction = connection.transaction()?;
                crate::sync::merge_remote(&transaction, &records)?;
                cloud::set_read_position(&transaction, &device, seq)?;
                if stop == Some(SyncStop::BeforeReadIsSaved) {
                    return Ok(Some(report));
                }
                transaction.commit()?;
                report.files_read += 1;
                self.catch_up(connection)?;
            }
        }

        // The recorder runs before the outbox is read, so that the changes
        // made while the files were read go out with this sync.
        self.record_pass(connection, None)?;
        loop {
            let page = cloud::outbox_page(connection, CHANGE_FILE_RECORDS)?;
            if page.records.is_empty() {
                break;
            }
            let seq = state.written_seq + 1;
            folder.write_changes(&state.device_id, seq, &page.records)?;
            report.files_written += 1;
            if stop == Some(SyncStop::AfterChangeFileWritten) {
                return Ok(Some(report));
            }
            let transaction = connection.transaction()?;
            cloud::remove_written(&transaction, &page, seq)?;
            transaction.commit()?;
            state.written_seq = seq;
        }

        if report.files_read + report.files_written > 0 || !folder.has_device_file(&state.device_id) {
            let now = chrono::Utc::now().timestamp_millis();
            folder.write_device(&state.device_id, computer, now)?;
            connection.execute("UPDATE sync_state SET synced_at_ms = ?1", [now])?;
        }
        Ok(Some(report))
    }

    /// The device id of this copy of the library.
    #[cfg(test)]
    pub(crate) fn sync_device_id(&self) -> Result<String> {
        let guard = self.sync.lock(false)?;
        let connection = guard.as_ref().expect("sync is on");
        Ok(connection.query_row("SELECT device_id FROM sync_state", [], |row| row.get(0))?)
    }
}
