//! Cloud sync through a cloud folder: turning sync on with a new cloud
//! library, opening a cloud library in a new library on another computer,
//! pausing, and a sync, which adopts the base snapshots, reads the change
//! files of the other devices, writes this device's outbox as its next
//! change files, and writes snapshots and deletes old files now and then.

use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use crate::sync::cloud::{self, CloudFolder, CloudState, Manifest, SyncStop, CHANGE_FILE_RECORDS};
use crate::sync::snapshot::{self, KEEP_MS};
use crate::sync::{now_ms, SyncComputer, SyncReport};

impl Library {
    /// Turns sync on with a new cloud library in `folder`, the folder of the
    /// Google Drive or Dropbox app: makes the library id, records the full
    /// state of the library (unless the library already records its changes
    /// without a cloud folder), writes `picto/<library id>/library.json` with
    /// the name `name`, and writes the first snapshot, which covers nothing of
    /// any device's change files. Returns the library id.
    pub fn turn_on_sync(&self, folder: &Path, name: &str, computer: &SyncComputer) -> Result<String> {
        if !folder.is_dir() {
            return Err(LibraryError::InvalidInput(format!("the cloud folder {} does not exist", folder.display())));
        }
        let started = !self.sync.on();
        if started {
            self.start_sync_recording()?;
        }
        let library_id = uuid::Uuid::new_v4().to_string();
        let turned_on = (|| {
            self.connect_sync_folder(folder, &library_id, name, computer)?;
            let mut guard = self.sync.lock(false)?;
            let connection = guard.as_mut().expect("sync is on");
            let state = cloud::state(connection)?.expect("the library was just connected");
            let cloud_folder = CloudFolder::open(folder, &library_id)?;
            let covers = BTreeMap::from([(state.device_id.clone(), 0)]);
            let now = now_ms();
            let manifest = snapshot::write(connection, &cloud_folder, &state.device_id, &computer.name, covers.clone(), now)?;
            self.sync.verified.lock().insert(manifest.snapshot_id.clone());
            let transaction = connection.transaction()?;
            cloud::set_adopted(&transaction, &[manifest.snapshot_id], &covers, &BTreeMap::new())?;
            transaction.commit()?;
            cloud_folder.write_device(&state.device_id, computer, now)
        })();
        if turned_on.is_err() && started {
            let _ = self.stop_sync_recording();
        }
        turned_on.map(|()| library_id)
    }

    /// Opens the cloud library `library_id` in `folder` in this library,
    /// which must be new: turns sync on with that cloud library and syncs,
    /// which adopts the base snapshots, reads every change file after them
    /// and writes everything into the library. When the cloud app has not
    /// copied a snapshot yet, nothing is read, and a later sync reads it. A
    /// library that has items, tags, folders or smart folders is never joined
    /// with a cloud library; Merge Library is for that.
    pub fn open_from_cloud(&self, folder: &Path, library_id: &str, computer: &SyncComputer) -> Result<SyncReport> {
        CloudFolder::open(folder, library_id)?;
        let has_content = self.database.read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_root) OR EXISTS(SELECT 1 FROM tag_definition)
                     OR EXISTS(SELECT 1 FROM folder_definition) OR EXISTS(SELECT 1 FROM smart_folder_definition)",
                [],
                |row| row.get::<_, bool>(0),
            )?)
        })?;
        if has_content || self.sync.on() {
            return Err(LibraryError::InvalidState("only a new library can be opened from the cloud".into()));
        }
        self.start_recording(false)?;
        let opened = (|| {
            self.connect_sync_folder(folder, library_id, "", computer)?;
            Ok(self.sync_with_cloud(computer)?.unwrap_or_default())
        })();
        if opened.is_err() {
            let _ = self.stop_sync_recording();
        }
        opened
    }

    /// Connects the library, whose sync is on, to the cloud library
    /// `library_id` in `folder`, and writes `library.json` with the name
    /// `name` when the cloud library has none yet. `computer` is the computer
    /// this runs on; a later sync on another computer finds the database to
    /// be a copy. Connecting again to the same cloud library changes only
    /// the path of the folder.
    fn connect_sync_folder(&self, folder: &Path, library_id: &str, name: &str, computer: &SyncComputer) -> Result<()> {
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
    /// library records its changes but reads and writes no files. A sync
    /// after resuming is an ordinary sync; when the change files it needs
    /// are gone, it adopts the base snapshots.
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
    ///    state (the library was copied to another computer), when this
    ///    device's folder holds a change file after the last one the
    ///    database noted as written (the database was restored from a
    ///    backup, or a sync stopped after writing a change file and before
    ///    noting it), or when a snapshot covers more of this device than
    ///    that last change file (the database was restored from a backup
    ///    older than the cleanup of the change files after it). A copy waits
    ///    until a snapshot is in the folder, then takes a new device id
    ///    (`cloud::take_new_device_id`), so that no two databases write
    ///    change files under the same device id, and adopts the base
    ///    snapshots.
    /// 2. When the base snapshots are not the ones the synced values were
    ///    built from, it adopts them (`adopt`).
    /// 3. It reads the new change files of every other device in sequence
    ///    order and stops at the first one that is missing or cannot be read
    ///    yet. Each file is merged into the synced values in one transaction
    ///    of the sync file, which also notes the file as read, and what it
    ///    changed is then written into the library.
    /// 4. It writes the outbox as this device's next change files, at most
    ///    `CHANGE_FILE_RECORDS` records in each, and after each file removes
    ///    what it holds from the outbox and notes its sequence number.
    /// 5. When it read or wrote a change file, or this device has no
    ///    `device.json` yet, it writes `device.json`.
    /// 6. It writes a snapshot and deletes old snapshots and change files
    ///    (`housekeeping`).
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
        let now = now_ms();
        let mut report = SyncReport::default();
        let (valid, incomplete) = self.checked_snapshots(&folder)?;
        let incomplete = cloud::note_incomplete(connection, &incomplete, now)?;
        let base = snapshot::base_snapshots(&valid);

        let copy = state.machine_id != computer.machine_id
            || folder.has_change_file(&state.device_id, state.written_seq + 1)
            || valid.iter().any(|manifest| manifest.covers.get(&state.device_id).is_some_and(|seq| *seq > state.written_seq));
        if copy {
            if base.is_empty() {
                return Ok(Some(report));
            }
            let transaction = connection.transaction()?;
            state = cloud::take_new_device_id(&transaction, &computer.machine_id)?;
            transaction.commit()?;
            report.new_device = true;
        }
        let base_ids = base.iter().map(|manifest| manifest.snapshot_id.clone()).collect::<Vec<_>>();
        if !base.is_empty() && (copy || base_ids != state.adopted) {
            report.files_read += self.adopt(connection, &folder, &state, &base)?;
            state = cloud::state(connection)?.expect("the library is connected");
        }

        let read = cloud::read_positions(connection)?;
        for device in folder.devices()? {
            if device == state.device_id {
                continue;
            }
            let mut seq = read.get(&device).or_else(|| state.adopted_covers.get(&device)).copied().unwrap_or(0);
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
            folder.write_device(&state.device_id, computer, now)?;
            connection.execute("UPDATE sync_state SET synced_at_ms = ?1", [now])?;
        }
        self.housekeeping(connection, &folder, &state, &valid, &base, &incomplete, computer, now)?;
        Ok(Some(report))
    }

    /// The snapshots of the cloud library whose manifest and shards are all
    /// there with the right checksums, and the ids of the others. The
    /// shards of a snapshot are read once in each run of the app; after
    /// that their sizes are compared with the manifest.
    fn checked_snapshots(&self, folder: &CloudFolder) -> Result<(Vec<Manifest>, Vec<String>)> {
        let (listed, mut incomplete) = folder.snapshots()?;
        let mut valid = Vec::new();
        for manifest in listed {
            let checked = self.sync.verified.lock().contains(&manifest.snapshot_id)
                || manifest.shards.iter().all(|shard| folder.read_shard(&manifest, shard).is_some());
            if checked {
                self.sync.verified.lock().insert(manifest.snapshot_id.clone());
                valid.push(manifest);
            } else {
                incomplete.push(manifest.snapshot_id);
            }
        }
        Ok((valid, incomplete))
    }

    /// Adopts the base snapshots `base`: builds the synced values again from
    /// their merge, every change file after their `covers` (this device's own
    /// up to the last one it wrote), and the outbox, all in one transaction
    /// of the sync file that also notes the snapshots as adopted and the
    /// change files as read. The records merge as records from other
    /// computers do, so the entities whose visible state they change are
    /// then written into the library. Returns the number of change files
    /// read.
    fn adopt(&self, connection: &mut Connection, folder: &CloudFolder, state: &CloudState, base: &[&Manifest]) -> Result<usize> {
        self.record_pass(connection, None)?;
        let covers = snapshot::greatest_covers(base);
        let transaction = connection.transaction()?;
        snapshot::begin_adoption(&transaction)?;
        for manifest in base {
            for shard in &manifest.shards {
                let records = folder.read_shard(manifest, shard).ok_or_else(|| {
                    LibraryError::InvalidState(format!("the snapshot {} cannot be read", manifest.snapshot_id))
                })?;
                snapshot::merge_adopted(&transaction, &records)?;
            }
        }
        let mut files = 0;
        let mut read = BTreeMap::new();
        for device in folder.devices()? {
            let own = device == state.device_id;
            let mut seq = covers.get(&device).copied().unwrap_or(0);
            while !own || seq < state.written_seq {
                let Some(records) = folder.read_changes(&device, seq + 1) else { break };
                snapshot::merge_adopted(&transaction, &records)?;
                seq += 1;
                files += 1;
            }
            if !own {
                read.insert(device, seq);
            }
        }
        snapshot::finish_adoption(&transaction)?;
        let ids = base.iter().map(|manifest| manifest.snapshot_id.clone()).collect::<Vec<_>>();
        cloud::set_adopted(&transaction, &ids, &covers, &read)?;
        transaction.commit()?;
        self.catch_up(connection)?;
        Ok(files)
    }

    /// Writes a new snapshot when the newest base snapshot is more than
    /// `KEEP_MS` old, the outbox is empty and no change file that this
    /// computer needs is missing, and deletes, in this order: the snapshots
    /// that a higher-ranked snapshot written more than `KEEP_MS` ago covers,
    /// the snapshots that have been incomplete for `KEEP_MS` since this
    /// computer first saw them so, and the change files that every valid
    /// snapshot covers and that are more than `KEEP_MS` old. A snapshot is
    /// written only where one is: turning sync on writes the first one.
    #[allow(clippy::too_many_arguments)]
    fn housekeeping(
        &self,
        connection: &Connection,
        folder: &CloudFolder,
        state: &CloudState,
        valid: &[Manifest],
        base: &[&Manifest],
        incomplete: &[(String, i64)],
        computer: &SyncComputer,
        now: i64,
    ) -> Result<()> {
        let read = cloud::read_positions(connection)?;
        let newest = base.iter().map(|manifest| manifest.written_at_ms).max();
        if newest.is_some_and(|newest| now - newest > KEEP_MS) {
            let outbox_empty = !connection.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox)", [], |row| row.get::<_, bool>(0))?;
            let mut covers = state.adopted_covers.clone();
            let mut missing = false;
            for device in folder.devices()? {
                if device == state.device_id {
                    continue;
                }
                let position = read.get(&device).or_else(|| state.adopted_covers.get(&device)).copied().unwrap_or(0);
                missing |= folder.change_files(&device)?.last().is_some_and(|(seq, _)| *seq > position);
                covers.insert(device, position);
            }
            covers.insert(state.device_id.clone(), state.written_seq);
            if outbox_empty && !missing {
                let manifest = snapshot::write(connection, folder, &state.device_id, &computer.name, covers, now)?;
                self.sync.verified.lock().insert(manifest.snapshot_id);
            }
        }
        let covered = snapshot::covered_snapshots(valid, now);
        for id in &covered {
            folder.delete_snapshot(id)?;
        }
        for (id, first_seen) in incomplete {
            if now - first_seen > KEEP_MS {
                folder.delete_snapshot(id)?;
                connection.execute("DELETE FROM sync_incomplete WHERE snapshot_id = ?1", [id])?;
            }
        }
        let remaining = valid.iter().filter(|manifest| !covered.contains(&manifest.snapshot_id)).collect::<Vec<_>>();
        if remaining.is_empty() {
            return Ok(());
        }
        for device in folder.devices()? {
            let every = remaining.iter().map(|manifest| manifest.covers.get(&device).copied().unwrap_or(0)).min().unwrap_or(0);
            for (seq, modified_ms) in folder.change_files(&device)? {
                if seq <= every && now - modified_ms > KEEP_MS {
                    folder.delete_change_file(&device, seq)?;
                }
            }
        }
        Ok(())
    }

    /// The device id of this copy of the library.
    #[cfg(test)]
    pub(crate) fn sync_device_id(&self) -> Result<String> {
        let guard = self.sync.lock(false)?;
        let connection = guard.as_ref().expect("sync is on");
        Ok(connection.query_row("SELECT device_id FROM sync_state", [], |row| row.get(0))?)
    }
}
