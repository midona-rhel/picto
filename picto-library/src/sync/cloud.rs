//! The cloud folder of a library that syncs, and the rows of the sync file
//! that belong to it.
//!
//! A library that syncs keeps its files in `picto/<library id>/` inside a
//! folder that the Google Drive or Dropbox desktop app copies to the other
//! computers. The folder holds:
//!
//! - `library.json`: the library id, the version of the format of the files
//!   and the name of the library. It is written once, when the library is
//!   first connected to the folder.
//! - `devices/<device id>/device.json`: the name of the computer, the version
//!   of Picto and the time of the device's last sync that read or wrote
//!   change files. It is the only file that is ever written over.
//! - `devices/<device id>/changes/<seq>.jsonl.zst`: the change records that
//!   one sync of the device wrote, at most `CHANGE_FILE_RECORDS` of them. The
//!   sequence numbers of a device start at 1 and have no gaps.
//!
//! A change file is JSON Lines compressed with zstd. Each line is one record,
//! written as the array `[kind, key, field, value, wall ms, counter]`. The
//! device id of the record's timestamp is the name of the device's folder.
//!
//! Every file is written under a temporary name in its folder, flushed to
//! disk and then renamed, so that this computer never leaves part of a file
//! under the file's real name. The cloud app can still put part of another
//! computer's file into the folder while it downloads it, and on Windows the
//! app can hold a file open so that it cannot be read. A change file that
//! cannot be read, decompressed or parsed is therefore treated as not there
//! yet: reading that device's files stops at it until a later sync.
//!
//! Device ids and library ids are lowercase UUIDs. A folder whose name is
//! not a lowercase UUID is not read. Neither is a file whose name the sync
//! does not write, such as the copy that a cloud app makes when two
//! computers write the same file. So the same files are read on file systems
//! that ignore the case of names and on those that do not.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{SyncKind, SyncRecord, SyncTimestamp};
use crate::{LibraryError, Result};

/// The largest number of change records in one change file.
pub(crate) const CHANGE_FILE_RECORDS: usize = 100_000;

/// The version of the layout and the format of the files in the cloud
/// folder. A library whose `library.json` names a later version is not
/// synced.
const FORMAT: u32 = 1;

/// The computer that a sync runs on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncComputer {
    /// An id of this computer that stays the same while the app runs on it.
    /// A library whose sync state names another machine id was copied from
    /// another computer, and takes a new device id.
    pub machine_id: String,
    /// The name of the computer, written into `device.json`.
    pub name: String,
    /// The version of Picto, written into `device.json`.
    pub app_version: String,
}

/// What one sync with the cloud folder did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// The number of change files of other devices that were read and
    /// merged.
    pub files_read: usize,
    /// The number of change files that this sync wrote.
    pub files_written: usize,
    /// Whether the database turned out to be a copy of the library and took
    /// a new device id.
    pub new_device: bool,
}

/// A point at which a test stops a sync, as a crash of the app there would
/// stop it.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncStop {
    /// After the first change file of another device is merged, before the
    /// transaction that merges it and notes it as read is committed.
    BeforeReadIsSaved,
    /// After the first change file is written, before the outbox is cleared
    /// of what it holds.
    AfterChangeFileWritten,
}

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    library_id: String,
    format: u32,
    name: String,
}

#[derive(Serialize)]
struct DeviceFile<'a> {
    computer: &'a str,
    app_version: &'a str,
    last_sync_ms: i64,
}

/// The folder `picto/<library id>/` of one cloud library.
pub(crate) struct CloudFolder {
    path: PathBuf,
}

impl CloudFolder {
    /// Makes the folder of the cloud library `library_id` in `root`, the
    /// folder of the Google Drive or Dropbox app, and writes its
    /// `library.json` with the name `name` when it has none yet.
    pub(crate) fn create(root: &Path, library_id: &str, name: &str) -> Result<()> {
        if !root.is_dir() {
            return Err(LibraryError::InvalidInput(format!("the cloud folder {} does not exist", root.display())));
        }
        let path = library_path(root, library_id)?;
        let library_file = path.join("library.json");
        if !library_file.exists() {
            let file = LibraryFile { library_id: library_id.to_owned(), format: FORMAT, name: name.to_owned() };
            write_atomically(&library_file, &serde_json::to_vec_pretty(&file)?, false)?;
        }
        Ok(())
    }

    /// The folder of the cloud library `library_id` in `root`. Its
    /// `library.json` must be there and name the library. When it is not,
    /// the cloud folder is not available (the cloud app is not running, the
    /// drive is not connected, or the folder was deleted), and nothing is
    /// read or written.
    pub(crate) fn open(root: &Path, library_id: &str) -> Result<Self> {
        let path = library_path(root, library_id)?;
        let unavailable = |reason: String| {
            LibraryError::NotFound(format!("the cloud library {library_id} in {}: {reason}", root.display()))
        };
        let bytes = std::fs::read(path.join("library.json")).map_err(|error| unavailable(error.to_string()))?;
        let file = serde_json::from_slice::<LibraryFile>(&bytes)
            .map_err(|_| unavailable("its library.json cannot be read".into()))?;
        if file.library_id != library_id {
            return Err(unavailable("its library.json names another library".into()));
        }
        if file.format > FORMAT {
            return Err(LibraryError::Incompatible(format!(
                "the cloud library {library_id} was written by a newer version of Picto"
            )));
        }
        Ok(Self { path })
    }

    fn device(&self, device: &str) -> PathBuf {
        self.path.join("devices").join(device)
    }

    fn change_file(&self, device: &str, seq: i64) -> PathBuf {
        self.device(device).join("changes").join(format!("{seq}.jsonl.zst"))
    }

    /// The device ids that have a folder in `devices/`.
    pub(crate) fn devices(&self) -> Result<Vec<String>> {
        let listing = match std::fs::read_dir(self.path.join("devices")) {
            Ok(listing) => listing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut devices = Vec::new();
        for entry in listing {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if is_id(name) && entry.file_type()?.is_dir() {
                    devices.push(name.to_owned());
                }
            }
        }
        devices.sort();
        Ok(devices)
    }

    /// Whether the change file `seq` of `device` is in the folder, whole or
    /// in part.
    pub(crate) fn has_change_file(&self, device: &str, seq: i64) -> bool {
        self.change_file(device, seq).exists()
    }

    /// Whether `device` has written its `device.json`.
    pub(crate) fn has_device_file(&self, device: &str) -> bool {
        self.device(device).join("device.json").exists()
    }

    /// The records of the change file `seq` of `device`, or `None` when the
    /// file is missing or cannot be read, decompressed or parsed yet.
    pub(crate) fn read_changes(&self, device: &str, seq: i64) -> Option<Vec<SyncRecord>> {
        let bytes = std::fs::read(self.change_file(device, seq)).ok()?;
        decode_change_file(&bytes, device)
    }

    /// Writes `records` as the change file `seq` of `device`. A change file
    /// is written once; the write fails when the file exists already.
    pub(crate) fn write_changes(&self, device: &str, seq: i64, records: &[SyncRecord]) -> Result<()> {
        write_atomically(&self.change_file(device, seq), &encode_change_file(records)?, false)
    }

    /// Writes `device.json` of `device`, over the one that is there.
    pub(crate) fn write_device(&self, device: &str, computer: &SyncComputer, last_sync_ms: i64) -> Result<()> {
        let file = DeviceFile { computer: &computer.name, app_version: &computer.app_version, last_sync_ms };
        write_atomically(&self.device(device).join("device.json"), &serde_json::to_vec_pretty(&file)?, true)
    }
}

/// The folder of the cloud library `library_id` in `root`. The id must be a
/// lowercase UUID, so that the path stays inside `root`.
fn library_path(root: &Path, library_id: &str) -> Result<PathBuf> {
    if !is_id(library_id) {
        return Err(LibraryError::InvalidInput(format!("{library_id} is not the id of a cloud library")));
    }
    Ok(root.join("picto").join(library_id))
}

/// Whether `name` is a UUID in lowercase with hyphens, the form in which
/// library ids and device ids are written.
fn is_id(name: &str) -> bool {
    uuid::Uuid::parse_str(name).is_ok_and(|id| id.to_string() == name)
}

/// One line of a change file: kind, key, field, value, wall time and
/// counter.
type ChangeLine = (SyncKind, String, String, Value, i64, i64);

fn encode_change_file(records: &[SyncRecord]) -> Result<Vec<u8>> {
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 3)?;
    // The checksum makes a file that was changed or cut short fail to
    // decompress.
    encoder.include_checksum(true)?;
    for record in records {
        serde_json::to_writer(
            &mut encoder,
            &(record.kind, &record.key, &record.field, &record.value, record.timestamp.wall_ms, record.timestamp.counter),
        )?;
        encoder.write_all(b"\n")?;
    }
    Ok(encoder.finish()?)
}

/// The records of a change file of `device`, or `None` when the bytes are
/// not a whole change file. A change file holds at least one record and
/// ends with a line break, so an empty file, which a cloud app can show
/// before it has downloaded the file, is not a whole change file either.
fn decode_change_file(bytes: &[u8], device: &str) -> Option<Vec<SyncRecord>> {
    let text = zstd::stream::decode_all(bytes).ok()?;
    if !text.ends_with(b"\n") {
        return None;
    }
    let mut records = Vec::new();
    for line in text.split_inclusive(|byte| *byte == b'\n') {
        let (kind, key, field, value, wall_ms, counter) = serde_json::from_slice::<ChangeLine>(line).ok()?;
        records.push(SyncRecord {
            kind,
            key,
            field,
            value,
            timestamp: SyncTimestamp { wall_ms, counter, device: device.to_owned() },
        });
    }
    Some(records)
}

/// Writes `bytes` to `path`: into a new file beside it, flushed to disk and
/// then renamed to `path`. When `replace` is false and `path` exists, the
/// new file is removed and the write fails.
fn write_atomically(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| LibraryError::InvalidInput(format!("{} has no folder", path.display())))?;
    std::fs::create_dir_all(folder)?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("file");
    let temporary = folder.join(format!("{name}.picto-writing-{}", uuid::Uuid::new_v4()));
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if !replace && path.exists() {
            return Err(LibraryError::InvalidState(format!("{} exists already", path.display())));
        }
        rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written?;
    // The rename is flushed too where the system allows it. Some file
    // systems of cloud apps do not allow it; then only the durability of the
    // rename after a power loss is lost.
    #[cfg(unix)]
    let _ = std::fs::File::open(folder).and_then(|folder| folder.sync_all());
    Ok(())
}

/// Renames `from` to `to`, replacing `to`. On Windows a rename fails while
/// another program, such as the cloud app or a virus scanner, has one of the
/// files open, so a failed rename is tried again 10 times over about three
/// seconds.
fn rename(from: &Path, to: &Path) -> Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 10 && error.kind() != std::io::ErrorKind::NotFound => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50 * attempt));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// The part of `sync_state` that a sync with the cloud folder uses.
pub(crate) struct CloudState {
    pub device_id: String,
    pub library_id: String,
    pub folder: PathBuf,
    pub machine_id: String,
    pub written_seq: i64,
    pub paused: bool,
}

/// The cloud state, or `None` when the library is not connected to a cloud
/// folder.
pub(crate) fn state(connection: &Connection) -> Result<Option<CloudState>> {
    Ok(connection
        .query_row(
            "SELECT device_id, library_id, cloud_folder, machine_id, written_seq, paused
             FROM sync_state WHERE library_id IS NOT NULL",
            [],
            |row| {
                Ok(CloudState {
                    device_id: row.get(0)?,
                    library_id: row.get(1)?,
                    folder: PathBuf::from(row.get::<_, String>(2)?),
                    machine_id: row.get(3)?,
                    written_seq: row.get(4)?,
                    paused: row.get(5)?,
                })
            },
        )
        .optional()?)
}

/// Stores the cloud library that the library syncs with. A library that
/// syncs with another cloud library already is refused. When the library is
/// connected again to the same cloud library, only the path of the folder
/// changes, so that the machine id stays the one of the computer that first
/// connected it.
pub(crate) fn connect(connection: &Connection, library_id: &str, folder: &str, machine_id: &str) -> Result<()> {
    let current = connection.query_row("SELECT library_id FROM sync_state", [], |row| row.get::<_, Option<String>>(0))?;
    match current {
        Some(current) if current != library_id => Err(LibraryError::InvalidState(format!(
            "the library syncs with the cloud library {current} already"
        ))),
        Some(_) => {
            connection.execute("UPDATE sync_state SET cloud_folder = ?1", [folder])?;
            Ok(())
        }
        None => {
            connection.execute(
                "UPDATE sync_state SET library_id = ?1, cloud_folder = ?2, machine_id = ?3, written_seq = 0",
                [library_id, folder, machine_id],
            )?;
            Ok(())
        }
    }
}

/// Gives the library a new device id, because the database is a copy of the
/// library: it was copied to another computer, restored from a backup, or a
/// sync of it stopped after writing a change file and before it noted that
/// file. The copy writes its change files under the new id from sequence
/// number 1. The records in its outbox take the new id in the synced values
/// too, because the device id of a record is the folder of the file it is
/// in. Every other device is read again from its first change file. The old
/// device id is read from the change file after the last one this copy
/// wrote, so that what the original wrote after the copy was made is merged
/// too. Returns the new state.
pub(crate) fn take_new_device_id(transaction: &Transaction<'_>, machine_id: &str) -> Result<CloudState> {
    let (old, written_seq) = transaction.query_row("SELECT device_id, written_seq FROM sync_state", [], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let new = uuid::Uuid::new_v4().to_string();
    let old_number = super::device_number(transaction, &old)?;
    let new_number = super::device_number(transaction, &new)?;
    transaction.execute(
        "UPDATE sync_value SET device = ?2
         FROM sync_outbox outbox
         WHERE sync_value.field = outbox.field AND sync_value.target = outbox.target
           AND sync_value.entity = outbox.entity AND sync_value.clock_ms = outbox.clock_ms
           AND sync_value.clock_counter = outbox.clock_counter AND sync_value.device = ?1",
        [old_number, new_number],
    )?;
    transaction.execute(
        "UPDATE sync_state SET device_id = ?1, machine_id = ?2, written_seq = 0",
        [&new, machine_id],
    )?;
    transaction.execute("DELETE FROM sync_read", [])?;
    transaction.execute(
        "INSERT INTO sync_read(device_id, seq) VALUES (?1, ?2)",
        rusqlite::params![old, written_seq],
    )?;
    state(transaction)?.ok_or_else(|| LibraryError::InvalidState("the library is not connected to a cloud folder".into()))
}

/// For each other device, the last of its change files that the library
/// has read.
pub(crate) fn read_positions(connection: &Connection) -> Result<BTreeMap<String, i64>> {
    Ok(connection
        .prepare("SELECT device_id, seq FROM sync_read")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn set_read_position(transaction: &Transaction<'_>, device: &str, seq: i64) -> Result<()> {
    transaction.execute(
        "INSERT INTO sync_read(device_id, seq) VALUES (?1, ?2)
         ON CONFLICT(device_id) DO UPDATE SET seq = excluded.seq",
        rusqlite::params![device, seq],
    )?;
    Ok(())
}

/// The first records of the outbox, as the next change file holds them.
pub(crate) struct OutboxPage {
    pub records: Vec<SyncRecord>,
    /// The primary key and the clock of each record's row, which find the
    /// row again when the record is removed.
    rows: Vec<(String, i64, i64, i64, i64)>,
}

/// Reads at most `limit` records of the outbox. Records are taken in the
/// order of the primary key and not of their timestamps, because the outbox
/// has no index on its timestamps, and the order of the records of one
/// device does not change how they merge.
pub(crate) fn outbox_page(connection: &Connection, limit: usize) -> Result<OutboxPage> {
    let mut statement = connection.prepare(
        "SELECT entity.kind, entity.key,
                CASE WHEN outbox.target = 0 THEN outbox.field ELSE outbox.field || ':' || target.key END,
                outbox.value, outbox.clock_ms, outbox.clock_counter, state.device_id,
                outbox.field, outbox.target, outbox.entity
         FROM sync_outbox outbox
         CROSS JOIN sync_identity entity ON entity.entity = outbox.entity
         LEFT JOIN sync_identity target ON target.entity = outbox.target
         CROSS JOIN sync_state state
         ORDER BY outbox.field, outbox.target, outbox.entity
         LIMIT ?1",
    )?;
    let mut page = OutboxPage { records: Vec::new(), rows: Vec::new() };
    let mut rows = statement.query([limit as i64])?;
    while let Some(row) = rows.next()? {
        let kind = row.get::<_, String>(0)?;
        let kind = SyncKind::parse(&kind)
            .ok_or_else(|| LibraryError::InvalidState(format!("the sync file names the unknown kind {kind}")))?;
        let timestamp = SyncTimestamp { wall_ms: row.get(4)?, counter: row.get(5)?, device: row.get(6)? };
        page.rows.push((row.get(7)?, row.get(8)?, row.get(9)?, timestamp.wall_ms, timestamp.counter));
        page.records.push(SyncRecord {
            kind,
            key: row.get(1)?,
            field: row.get(2)?,
            value: serde_json::from_str(&row.get::<_, String>(3)?)?,
            timestamp,
        });
    }
    Ok(page)
}

/// Removes the records of `page` from the outbox once the change file `seq`
/// holds them, and notes `seq` as the last change file this device wrote. A
/// record that the library made since for the same field has a later clock
/// and stays.
pub(crate) fn remove_written(transaction: &Transaction<'_>, page: &OutboxPage, seq: i64) -> Result<()> {
    let mut statement = transaction.prepare_cached(
        "DELETE FROM sync_outbox
         WHERE field = ?1 AND target = ?2 AND entity = ?3 AND clock_ms = ?4 AND clock_counter = ?5",
    )?;
    for (field, target, entity, clock_ms, clock_counter) in &page.rows {
        statement.execute(rusqlite::params![field, target, entity, clock_ms, clock_counter])?;
    }
    transaction.execute("UPDATE sync_state SET written_seq = ?1", [seq])?;
    Ok(())
}
