//! Cloud sync: sync keys, synced values, recording the library's changes as
//! change records, and writing synced values from other computers into the
//! library.
//!
//! Each entity of the library (item, collection, tag, tag group, folder,
//! smart folder, the library's settings, the settings of one view and a "Not
//! duplicate" decision) has a sync key that is the same on every computer,
//! and fields. A change to a field is a change record with a hybrid logical
//! clock timestamp. For each field the record with the latest timestamp
//! wins. The sync file (`SyncStore`) holds those records in `sync_value`,
//! and the library is a projection of them: `values` computes what the
//! synced values show for an entity, `local` reads what the library holds,
//! `record` compares the two for what changed in the library since its last
//! pass and records what differs, and `apply` writes what the synced values
//! show into the library.
//! `cloud` reads and writes the files of the cloud folder through which the
//! computers exchange their change records, and `snapshot` what a snapshot
//! of the synced values holds and how a library adopts snapshots.
//!
//! Library writes do no sync work apart from the triggers of `MARKS` and
//! `ROW_MARKS`. The recorder runs in the background after writes, on the
//! sync file's own connection, and reads the library in a read transaction.

mod apply;
pub(crate) mod cloud;
mod local;
mod order;
mod record;
pub(crate) mod snapshot;
#[cfg(test)]
mod tests;
mod values;

use std::path::{Path, PathBuf};

use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

pub(crate) use apply::{finish_projection, merge_remote, pending_projection, project, synced_for_projection};
pub use cloud::{SyncComputer, SyncReport};
#[cfg(test)]
pub(crate) use record::differences;
pub(crate) use record::{record, record_full};

/// The kinds of entity that sync. Adding a kind means adding its fields to
/// `local` (what the library holds) and `values` (what the synced values
/// show), and writing them in `apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncKind {
    Item,
    Collection,
    Tag,
    TagGroup,
    Folder,
    SmartFolder,
    /// The library's settings: the synced fields of the `application`
    /// setting. A library has one, with the empty key.
    Library,
    /// The settings of one view (a row of `view_pref`), keyed by the view's
    /// scope with the local id in it replaced by a sync key: `folder:<uuid>`,
    /// `smart:<uuid>`, or a scope without an id such as `grid:defaults`.
    ViewSettings,
    /// A "Not duplicate" decision about two items, keyed by their content
    /// hashes, sorted and joined with `:` (`decision_key`).
    DuplicateDecision,
}

impl SyncKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Collection => "collection",
            Self::Tag => "tag",
            Self::TagGroup => "tag_group",
            Self::Folder => "folder",
            Self::SmartFolder => "smart_folder",
            Self::Library => "library",
            Self::ViewSettings => "view_settings",
            Self::DuplicateDecision => "duplicate_decision",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "item" => Self::Item,
            "collection" => Self::Collection,
            "tag" => Self::Tag,
            "tag_group" => Self::TagGroup,
            "folder" => Self::Folder,
            "smart_folder" => Self::SmartFolder,
            "library" => Self::Library,
            "view_settings" => Self::ViewSettings,
            "duplicate_decision" => Self::DuplicateDecision,
            _ => return None,
        })
    }

    /// Whether the library holds an entity of this kind in a row with a
    /// local id. The library's settings, view settings and duplicate
    /// decisions are rows without one, which `local` finds by their sync key.
    pub(crate) fn has_local_id(self) -> bool {
        !matches!(self, Self::Library | Self::ViewSettings | Self::DuplicateDecision)
    }
}

/// The key of the "Not duplicate" decision about the items with the content
/// hashes `a` and `b`.
pub(crate) fn decision_key(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a}:{b}")
    } else {
        format!("{b}:{a}")
    }
}

/// The folder or smart folder whose view the view settings with the sync key
/// `key` belong to. The other views (`grid:defaults`, `system:trash` and so
/// on) belong to none.
pub(crate) fn view_owner(key: &str) -> Option<(SyncKind, &str)> {
    if let Some(folder) = key.strip_prefix("folder:") {
        Some((SyncKind::Folder, folder))
    } else {
        key.strip_prefix("smart:").map(|smart| (SyncKind::SmartFolder, smart))
    }
}

/// A hybrid logical clock timestamp. Timestamps compare by wall time, then
/// counter, then device id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SyncTimestamp {
    pub wall_ms: i64,
    pub counter: i64,
    pub device: String,
}

/// One change record: the value of one field of one entity at one time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncRecord {
    pub kind: SyncKind,
    pub key: String,
    pub field: String,
    pub value: Value,
    pub timestamp: SyncTimestamp,
}

/// The sync file: a SQLite database next to the library database that holds
/// the synced values, the outbox and copies of what the recorder last
/// recorded. It has its own connection, which attaches the library database
/// as `library` and only reads it; library writes never write the sync file
/// and never wait for it. The file exists while sync is on.
///
/// The connection is opened for each use and closed after it. SQLite, as
/// this crate builds it, keeps the cached pages of all connections of the
/// process in one pool, and a connection whose cache is full reuses the
/// pages that the process used least recently, whichever connection they
/// belong to. A connection to the sync file that stayed open had a full
/// cache after its first large pass, and every later pass took the cached
/// pages of the library's writer, which every library write then read
/// from the file again. A connection that is closed frees its own pages, and
/// a new one has an empty cache, so a pass takes no other connection's pages
/// unless it reads more than its cache holds.
pub(crate) struct SyncStore {
    path: PathBuf,
    library: PathBuf,
    lock: Mutex<()>,
    /// The snapshots of the cloud library whose shards this process has
    /// read with the right checksums. A snapshot is written once, so it is
    /// checked once in each run of the app.
    pub(crate) verified: Mutex<std::collections::HashSet<String>>,
}

/// The connection to the sync file for one use; `None` while sync is off.
/// It closes when this is dropped.
pub(crate) struct SyncConnection<'a> {
    // The connection closes before the lock is released.
    connection: Option<Connection>,
    _lock: MutexGuard<'a, ()>,
}

impl std::ops::Deref for SyncConnection<'_> {
    type Target = Option<Connection>;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl std::ops::DerefMut for SyncConnection<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

impl SyncStore {
    pub(crate) fn new(library: &Path) -> Self {
        Self {
            path: library.with_file_name("sync.sqlite"),
            library: library.to_owned(),
            lock: Mutex::new(()),
            verified: Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub(crate) fn on(&self) -> bool {
        self.path.exists()
    }

    /// Opens the connection to the sync file, and creates the file when
    /// `create` is true. Holding the connection keeps every other user of
    /// the sync file waiting.
    pub(crate) fn lock(&self, create: bool) -> Result<SyncConnection<'_>> {
        let lock = self.lock.lock();
        let connection = if create || self.on() { Some(self.open()?) } else { None };
        Ok(SyncConnection { connection, _lock: lock })
    }

    /// Deletes the sync file.
    pub(crate) fn remove(&self) -> Result<()> {
        let _lock = self.lock.lock();
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.clone().into_os_string();
            path.push(suffix);
            match std::fs::remove_file(path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA journal_size_limit = 67108864;")?;
        connection.execute_batch(include_str!("../sync_schema.sql"))?;
        // The records that wait to be written (`flush`).
        connection.execute_batch(
            "CREATE TEMP TABLE sync_new (
                 entity INTEGER NOT NULL, field TEXT NOT NULL, target INTEGER NOT NULL,
                 value TEXT NOT NULL, reference INTEGER
             );",
        )?;
        // The fields that the cloud data hold while base snapshots are
        // adopted (`snapshot::begin_adoption`).
        connection.execute_batch(
            "CREATE TEMP TABLE sync_adopted (
                 field TEXT NOT NULL, target INTEGER NOT NULL, entity INTEGER NOT NULL,
                 PRIMARY KEY(field, target, entity)
             ) WITHOUT ROWID;",
        )?;
        connection.execute("ATTACH DATABASE ?1 AS library", [self.library.to_string_lossy()])?;
        // The caches of the library database's own connections have the
        // same sizes.
        connection.execute_batch("PRAGMA main.cache_size = -65536; PRAGMA library.cache_size = -16384;")?;
        Ok(connection)
    }
}

/// While sync is on, the library has the table `changed_entity` and these
/// triggers, which give every local id whose row changed the revision of
/// the write that changed it. That is the only sync work in a library write:
/// one row for each item, collection, tag, tag group, folder or smart folder
/// whose own row a write changes, and for each item whose sources change,
/// and nothing for the bulk changes of tags, ratings, lifecycles and lists,
/// which the recorder finds from the `revision` of the `canonical_bitmap`
/// rows (through the index `sync_bitmap_revision`) and the checksums of the
/// `ordered_membership_chunk` rows. An item that enters or leaves a
/// collection changes the collection's list, so media rows are marked only
/// when they are updated. Viewing an item writes `recent_view` and has no
/// trigger: the recorder finds the views from their times
/// (`record::viewed_changes`). A write sets `library_meta.revision` to the
/// next revision after these triggers have run, so they take that next
/// revision.
const MARKS: [(&str, &str, &str); 7] = [
    ("library_root", "root_id", "OF name, notes, source_urls_json, cover_media_id, imported_at_ms, captured_at_ms, modified_at_ms"),
    ("media_item", "media_id", "OF media_name, media_notes, file_id"),
    ("tag_definition", "tag_id", ""),
    ("tag_namespace", "namespace_id", ""),
    ("folder_definition", "folder_id", ""),
    ("smart_folder_definition", "smart_folder_id", ""),
    ("source_provenance", "media_id", ""),
];

/// The rows that hold an entity without a local id, which the triggers of
/// `row_triggers` note in `changed_row` by kind and the text that
/// identifies the row: (table, the condition under which a row is such an
/// entity, the kind, the text). `{row}` stands for `NEW` or `OLD`. The
/// `application` setting is the library's settings, each row of `view_pref`
/// is the settings of one view, and a duplicate pair with status 2 (Not
/// duplicate) is a decision, identified by its two file ids.
const ROW_MARKS: [(&str, &str, &str, &str); 3] = [
    ("setting", "{row}.key = 'application'", "library", "''"),
    ("view_pref", "true", "view_settings", "{row}.scope"),
    ("duplicate_pair", "{row}.status = 2", "duplicate_decision", "{row}.file_id_a || ':' || {row}.file_id_b"),
];

/// The triggers of `ROW_MARKS`, as (name, event, condition, kind, text). An
/// update of a duplicate pair is noted only when its status changes, so that
/// a scan that updates the distances of found pairs does no sync work.
fn row_triggers() -> Vec<(String, String, String, &'static str, String)> {
    let mut triggers = Vec::new();
    for (table, condition, kind, text) in ROW_MARKS {
        let new = |value: &str| value.replace("{row}", "NEW");
        let old = |value: &str| value.replace("{row}", "OLD");
        let updated = if table == "duplicate_pair" { "UPDATE OF status" } else { "UPDATE" };
        triggers.push((format!("{table}_insert"), format!("INSERT ON {table}"), new(condition), kind, new(text)));
        triggers.push((format!("{table}_delete"), format!("DELETE ON {table}"), old(condition), kind, old(text)));
        triggers.push((
            format!("{table}_update"),
            format!("{updated} ON {table}"),
            format!("({}) OR ({})", old(condition), new(condition)),
            kind,
            new(text),
        ));
    }
    triggers
}

/// The triggers of `MARKS`, and the one that marks the items of a file whose
/// facts change, as (name, event, the select of the marked ids).
fn mark_triggers() -> Vec<(String, String, String)> {
    let mut triggers = Vec::new();
    for (table, id, columns) in MARKS {
        if table != "media_item" {
            triggers.push((format!("{table}_insert"), format!("INSERT ON {table}"), format!("SELECT NEW.{id} AS id")));
            triggers.push((format!("{table}_delete"), format!("DELETE ON {table}"), format!("SELECT OLD.{id} AS id")));
        }
        let mut ids = format!("SELECT NEW.{id} AS id");
        // Renaming a tag group renames every tag in it.
        if table == "tag_namespace" {
            ids.push_str(" UNION ALL SELECT tag_id FROM tag_definition WHERE namespace_id = NEW.namespace_id");
        }
        triggers.push((format!("{table}_update"), format!("UPDATE {columns} ON {table}"), ids));
    }
    triggers.push((
        "media_file_update".into(),
        "UPDATE OF content_hash, mime, size_bytes, width, height, duration_ms, frame_count ON media_file".into(),
        "SELECT media_id AS id FROM media_item WHERE file_id = NEW.file_id".into(),
    ));
    triggers
}

/// Adds `changed_entity`, `changed_row`, their triggers and the revision
/// index of `canonical_bitmap` to the library.
pub(crate) fn start_marking(transaction: &Transaction<'_>) -> Result<()> {
    let mut sql = String::from(
        "CREATE TABLE IF NOT EXISTS changed_entity (local_id INTEGER PRIMARY KEY, revision INTEGER NOT NULL) STRICT;
         CREATE INDEX IF NOT EXISTS changed_entity_revision ON changed_entity(revision);
         CREATE TABLE IF NOT EXISTS changed_row (
             kind TEXT NOT NULL, row TEXT NOT NULL, revision INTEGER NOT NULL, PRIMARY KEY(kind, row)
         ) WITHOUT ROWID, STRICT;
         CREATE INDEX IF NOT EXISTS changed_row_revision ON changed_row(revision);
         CREATE INDEX IF NOT EXISTS sync_bitmap_revision ON canonical_bitmap(revision);
         DELETE FROM tag_merge;",
    );
    // The upsert keeps one row for each local id; it is not overridden by
    // the conflict policy of the statement that fires the trigger.
    for (name, event, ids) in mark_triggers() {
        sql.push_str(&format!(
            "CREATE TRIGGER IF NOT EXISTS sync_{name} AFTER {event} BEGIN
                 INSERT INTO changed_entity(local_id, revision)
                 SELECT id, (SELECT revision + 1 FROM library_meta) FROM ({ids}) WHERE true
                 ON CONFLICT(local_id) DO UPDATE SET revision = excluded.revision;
             END;"
        ));
    }
    for (name, event, condition, kind, text) in row_triggers() {
        sql.push_str(&format!(
            "CREATE TRIGGER IF NOT EXISTS sync_{name} AFTER {event} WHEN {condition} BEGIN
                 INSERT INTO changed_row(kind, row, revision)
                 VALUES ('{kind}', {text}, (SELECT revision + 1 FROM library_meta))
                 ON CONFLICT(kind, row) DO UPDATE SET revision = excluded.revision;
             END;"
        ));
    }
    Ok(transaction.execute_batch(&sql)?)
}

pub(crate) fn stop_marking(transaction: &Transaction<'_>) -> Result<()> {
    let mut sql = String::from(
        "DROP TABLE IF EXISTS changed_entity; DROP TABLE IF EXISTS changed_row;
         DROP INDEX IF EXISTS sync_bitmap_revision;",
    );
    for name in mark_triggers().into_iter().map(|trigger| trigger.0).chain(row_triggers().into_iter().map(|trigger| trigger.0)) {
        sql.push_str(&format!("DROP TRIGGER IF EXISTS sync_{name};"));
    }
    Ok(transaction.execute_batch(&sql)?)
}

/// Notes in `changed_entity` that the projection changed the view time of
/// the root `root_id` in a library write of `revision`. Writing
/// `recent_view` fires no trigger, and a view time that is not the latest
/// one is found only from this note.
pub(crate) fn mark_viewed(transaction: &Transaction<'_>, root_id: u32, revision: u64) -> Result<()> {
    transaction
        .prepare_cached(
            "INSERT INTO changed_entity(local_id, revision) VALUES (?1, ?2)
             ON CONFLICT(local_id) DO UPDATE SET revision = excluded.revision",
        )?
        .execute(rusqlite::params![root_id, revision as i64])?;
    Ok(())
}

/// The hybrid logical clock of this library, read once for a pass of the
/// recorder or a merge and saved at its end. Each timestamp is above every
/// timestamp the library has seen and above the wall clock.
pub(crate) struct Clock {
    /// The number that stands for this library's device id in `sync_value`.
    device_number: i64,
    wall_ms: i64,
    counter: i64,
}

impl Clock {
    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        let (device, wall_ms, counter) = connection.query_row(
            "SELECT device_id, clock_ms, clock_counter FROM sync_state",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(Self {
            device_number: device_number(connection, &device)?,
            wall_ms,
            counter,
        })
    }

    /// Takes `count` timestamps in a row and returns the wall time and the
    /// counter of the first; the others have the counters after it.
    fn reserve(&mut self, count: i64) -> (i64, i64) {
        let now = now_ms();
        if now > self.wall_ms {
            self.wall_ms = now;
            self.counter = 0;
        } else {
            self.counter += 1;
        }
        let first = self.counter;
        self.counter += count - 1;
        (self.wall_ms, first)
    }

    /// The wall time and counter of the latest timestamp taken or seen.
    pub(crate) fn position(&self) -> (i64, i64) {
        (self.wall_ms, self.counter)
    }

    /// Moves the clock above a timestamp read from another computer.
    pub(crate) fn observe(&mut self, timestamp: &SyncTimestamp) {
        if (timestamp.wall_ms, timestamp.counter) > (self.wall_ms, self.counter) {
            self.wall_ms = timestamp.wall_ms;
            self.counter = timestamp.counter;
        }
    }

    pub(crate) fn save(&self, connection: &Connection) -> Result<()> {
        connection.execute(
            "UPDATE sync_state SET clock_ms = ?1, clock_counter = ?2",
            rusqlite::params![self.wall_ms, self.counter],
        )?;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    /// How far the tests of this thread have moved the time forward, in
    /// milliseconds, to check what sync does after days and weeks.
    pub(crate) static TIME_SHIFT_MS: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// The time in milliseconds since 1970 that sync works with: the wall
/// clock, moved forward in tests by `TIME_SHIFT_MS`.
pub(crate) fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64);
    #[cfg(test)]
    let now = now + TIME_SHIFT_MS.with(std::cell::Cell::get);
    now
}

/// The number that stands for a device id in `sync_value`, added when new.
fn device_number(connection: &Connection, device: &str) -> Result<i64> {
    connection
        .prepare_cached("INSERT OR IGNORE INTO sync_device(device_id) VALUES (?1)")?
        .execute([device])?;
    Ok(connection
        .prepare_cached("SELECT device FROM sync_device WHERE device_id = ?1")?
        .query_row([device], |row| row.get(0))?)
}

/// The number that stands for an entity in `sync_value`: its row in
/// `sync_identity`, added when the key is new.
pub(crate) fn entity(connection: &Connection, kind: SyncKind, key: &str) -> Result<i64> {
    connection
        .prepare_cached("INSERT OR IGNORE INTO sync_identity(kind, key) VALUES (?1, ?2)")?
        .execute([kind.as_str(), key])?;
    Ok(connection
        .prepare_cached("SELECT entity FROM sync_identity WHERE kind = ?1 AND key = ?2")?
        .query_row([kind.as_str(), key], |row| row.get(0))?)
}

/// One record as `sync_value` stores it. A field `tag:<name>` or
/// `folder:<uuid>` is stored as `tag` or `folder` with the tag or folder as
/// its target; every other field has the target 0. `reference` is the
/// entity that the value of a field without a target names, or that the
/// entity is shown only with: the folder or smart folder of a view's
/// settings, and the two items of a duplicate decision, the first named by
/// its field `deleted` and the second by its field `decision`, so that each
/// item finds the decision. The tags and folders that a smart folder's rules
/// name are not references; they are found by reading the rules.
struct StoredRow {
    entity: i64,
    field: String,
    target: i64,
    value: String,
    reference: Option<i64>,
}

fn stored_row(connection: &Connection, kind: SyncKind, key: &str, field: &str, value: &Value) -> Result<StoredRow> {
    let first = value.get(0).and_then(Value::as_str);
    let (name, target) = if let Some(tag) = field.strip_prefix("tag:") {
        ("tag", Some((SyncKind::Tag, tag)))
    } else if let Some(folder) = field.strip_prefix("folder:") {
        ("folder", Some((SyncKind::Folder, folder)))
    } else {
        (field, None)
    };
    let referred = match (kind, field) {
        (_, _) if target.is_some() => None,
        (SyncKind::Item, "collection") => first.map(|key| (SyncKind::Collection, key)),
        (SyncKind::Folder, "place") => first.map(|key| (SyncKind::Folder, key)),
        (SyncKind::SmartFolder, "place") => first.map(|key| (SyncKind::SmartFolder, key)),
        (SyncKind::ViewSettings, _) => view_owner(key),
        (SyncKind::DuplicateDecision, "deleted") => key.split_once(':').map(|(a, _)| (SyncKind::Item, a)),
        (SyncKind::DuplicateDecision, "decision") => key.split_once(':').map(|(_, b)| (SyncKind::Item, b)),
        (SyncKind::Tag, "renamed_to") => value.as_str().map(|key| (SyncKind::Tag, key)),
        (SyncKind::Collection, "cover") => value.as_str().map(|key| (SyncKind::Item, key)),
        (SyncKind::Folder, "cover") => first
            .and_then(SyncKind::parse)
            .zip(value.get(1).and_then(Value::as_str)),
        _ => None,
    };
    let target = match target {
        Some((kind, key)) => entity(connection, kind, key)?,
        None => 0,
    };
    let reference = match referred {
        Some((kind, key)) => Some(entity(connection, kind, key)?),
        None => None,
    };
    Ok(StoredRow {
        entity: entity(connection, kind, key)?,
        field: name.to_owned(),
        target,
        value: serde_json::to_string(value)?,
        reference,
    })
}

/// Writes change records that this library made, each with a new timestamp:
/// into the synced values, and into the outbox when `outbox` is true.
/// `records` holds (kind, key, field, value). Records already waiting in
/// `temp.sync_new` are written first.
pub(crate) fn write_local(
    transaction: &Transaction<'_>,
    clock: &mut Clock,
    records: impl IntoIterator<Item = (SyncKind, String, String, Value)>,
    outbox: bool,
) -> Result<usize> {
    let mut stage = transaction.prepare_cached(
        "INSERT INTO temp.sync_new(entity, field, target, value, reference) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (kind, key, field, value) in records {
        let row = stored_row(transaction, kind, &key, &field, &value)?;
        stage.execute(rusqlite::params![row.entity, row.field, row.target, row.value, row.reference])?;
    }
    drop(stage);
    flush(transaction, clock, outbox)
}

/// Writes the records waiting in `temp.sync_new` into the synced values, and
/// into the outbox when `outbox` is true, each with its own timestamp in the
/// order they were added, and empties the table. Returns how many there
/// were.
pub(crate) fn flush(transaction: &Transaction<'_>, clock: &mut Clock, outbox: bool) -> Result<usize> {
    let count = transaction.query_row("SELECT COUNT(*) FROM temp.sync_new", [], |row| row.get::<_, i64>(0))?;
    if count == 0 {
        return Ok(0);
    }
    // The rowids of the waiting records run from 1, because the table is
    // emptied after every write.
    let (wall_ms, first) = clock.reserve(count);
    transaction
        .prepare_cached(
            "INSERT INTO sync_value(entity, field, target, value, clock_ms, clock_counter, device, reference)
             SELECT entity, field, target, value, ?1, ?2 + rowid - 1, ?3, reference
             FROM temp.sync_new WHERE true
             ON CONFLICT(field, target, entity) DO UPDATE SET
                 value = excluded.value, clock_ms = excluded.clock_ms,
                 clock_counter = excluded.clock_counter, device = excluded.device,
                 reference = excluded.reference",
        )?
        .execute(rusqlite::params![wall_ms, first, clock.device_number])?;
    // The library holds what it recorded, so a value from before the
    // records of other computers no longer tells what it holds.
    if transaction.query_row("SELECT EXISTS(SELECT 1 FROM sync_pending_value)", [], |row| row.get::<_, bool>(0))? {
        transaction.execute(
            "DELETE FROM sync_pending_value
             WHERE (entity, field, target) IN (SELECT entity, field, target FROM temp.sync_new)",
            [],
        )?;
    }
    if outbox {
        transaction
            .prepare_cached(
                "INSERT OR REPLACE INTO sync_outbox(entity, field, target, value, clock_ms, clock_counter)
                 SELECT entity, field, target, value, ?1, ?2 + rowid - 1 FROM temp.sync_new",
            )?
            .execute(rusqlite::params![wall_ms, first])?;
    }
    transaction.execute("DELETE FROM temp.sync_new", [])?;
    Ok(count as usize)
}

/// Merges records from another computer into the synced values. A record
/// replaces the stored one only when its timestamp is later. Returns the
/// records that replaced a stored value.
pub(crate) fn merge<'a>(
    transaction: &Transaction<'_>,
    clock: &mut Clock,
    records: &'a [SyncRecord],
) -> Result<Vec<&'a SyncRecord>> {
    let mut statement = transaction.prepare_cached(
        "INSERT INTO sync_value(entity, field, target, value, clock_ms, clock_counter, device, reference)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(field, target, entity) DO UPDATE SET
             value = excluded.value, clock_ms = excluded.clock_ms,
             clock_counter = excluded.clock_counter, device = excluded.device,
             reference = excluded.reference
         WHERE (excluded.clock_ms, excluded.clock_counter,
                (SELECT device_id FROM sync_device WHERE device = excluded.device))
             > (sync_value.clock_ms, sync_value.clock_counter,
                (SELECT device_id FROM sync_device WHERE device = sync_value.device))",
    )?;
    // The value each field had before the first record merged into it while
    // the entity waits to be written (`sync_pending_value`). A field without
    // a value counts as `null`, and the `deleted` field of an entity that the
    // synced values do not know as `true`.
    let mut keep = transaction.prepare_cached(
        "INSERT OR IGNORE INTO sync_pending_value(entity, field, target, value)
         VALUES (?1, ?2, ?3, COALESCE(
             (SELECT value FROM sync_value WHERE field = ?2 AND target = ?3 AND entity = ?1),
             CASE ?2 WHEN 'deleted' THEN 'true' ELSE 'null' END))",
    )?;
    let mut devices = std::collections::HashMap::new();
    let mut merged = Vec::new();
    for record in records {
        clock.observe(&record.timestamp);
        let device = match devices.get(&record.timestamp.device) {
            Some(device) => *device,
            None => {
                let device = device_number(transaction, &record.timestamp.device)?;
                devices.insert(record.timestamp.device.clone(), device);
                device
            }
        };
        let row = stored_row(transaction, record.kind, &record.key, &record.field, &record.value)?;
        keep.execute(rusqlite::params![row.entity, row.field, row.target])?;
        let changed = statement.execute(rusqlite::params![
            row.entity,
            row.field,
            row.target,
            row.value,
            record.timestamp.wall_ms,
            record.timestamp.counter,
            device,
            row.reference,
        ])?;
        if changed > 0 {
            merged.push(record);
        }
    }
    Ok(merged)
}

/// Reads the outbox: the change records this library made that no change
/// file holds yet.
pub(crate) fn outbox(connection: &Connection) -> Result<Vec<SyncRecord>> {
    read_records(
        connection,
        "SELECT entity.kind, entity.key,
                CASE WHEN outbox.target = 0 THEN outbox.field ELSE outbox.field || ':' || target.key END,
                outbox.value, outbox.clock_ms, outbox.clock_counter, state.device_id
         FROM sync_outbox outbox
         JOIN sync_identity entity ON entity.entity = outbox.entity
         LEFT JOIN sync_identity target ON target.entity = outbox.target
         CROSS JOIN sync_state state
         ORDER BY outbox.clock_ms, outbox.clock_counter",
    )
}

/// Removes `records` from the outbox once a change file holds them. A
/// record of the same field that the library made since stays.
pub(crate) fn remove_from_outbox(connection: &Connection, records: &[SyncRecord]) -> Result<()> {
    let mut statement = connection.prepare_cached(
        "DELETE FROM sync_outbox
         WHERE entity = ?1 AND field = ?2 AND target = ?3 AND clock_ms = ?4 AND clock_counter = ?5",
    )?;
    for record in records {
        let row = stored_row(connection, record.kind, &record.key, &record.field, &record.value)?;
        statement.execute(rusqlite::params![
            row.entity,
            row.field,
            row.target,
            record.timestamp.wall_ms,
            record.timestamp.counter
        ])?;
    }
    Ok(())
}

/// Every synced value as a change record.
#[cfg(test)]
pub(crate) fn all_records(connection: &Connection) -> Result<Vec<SyncRecord>> {
    read_records(
        connection,
        "SELECT entity.kind, entity.key,
                CASE WHEN value.target = 0 THEN value.field ELSE value.field || ':' || target.key END,
                value.value, value.clock_ms, value.clock_counter, device.device_id
         FROM sync_value value
         JOIN sync_identity entity ON entity.entity = value.entity
         LEFT JOIN sync_identity target ON target.entity = value.target
         JOIN sync_device device ON device.device = value.device",
    )
}

/// Reads change records with `sql`, whose columns are kind, key, field,
/// value, wall time, counter and device id.
fn read_records(connection: &Connection, sql: &str) -> Result<Vec<SyncRecord>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            SyncTimestamp {
                wall_ms: row.get(4)?,
                counter: row.get(5)?,
                device: row.get(6)?,
            },
        ))
    })?;
    let mut records = Vec::new();
    for row in rows {
        let (kind, key, field, value, timestamp) = row?;
        let Some(kind) = SyncKind::parse(&kind) else {
            continue;
        };
        records.push(SyncRecord {
            kind,
            key,
            field,
            value: serde_json::from_str(&value)?,
            timestamp,
        });
    }
    Ok(records)
}

/// The latest stored record of one field.
#[derive(Clone, Debug)]
pub(crate) struct Stored {
    pub value: Value,
    pub timestamp: SyncTimestamp,
}

