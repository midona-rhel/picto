//! Cloud sync: sync keys, synced values, recording the library's changes as
//! change records, and writing synced values from other computers into the
//! library.
//!
//! Each entity of the library (item, collection, tag, tag group, folder) has
//! a sync key that is the same on every computer, and fields. A change to a
//! field is a change record with a hybrid logical clock timestamp. For each
//! field the record with the latest timestamp wins. `sync_value` holds those
//! records, and the library is a projection of them: `values` computes what
//! the synced values show for an entity, `local` reads what the library
//! holds, `record` compares the two after every write and records what
//! differs, and `apply` writes what the synced values show into the library.
//!
//! The library records only while sync is on (`sync_state` has its row).
//! While it is off, `record` returns after one query.

mod apply;
mod local;
mod order;
mod record;
#[cfg(test)]
mod tests;
mod values;

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

pub(crate) use apply::{apply, project};
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
}

impl SyncKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Collection => "collection",
            Self::Tag => "tag",
            Self::TagGroup => "tag_group",
            Self::Folder => "folder",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "item" => Self::Item,
            "collection" => Self::Collection,
            "tag" => Self::Tag,
            "tag_group" => Self::TagGroup,
            "folder" => Self::Folder,
            _ => return None,
        })
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

/// Whether this library records its changes, which it does while sync is on.
pub(crate) fn recording(connection: &Connection) -> Result<bool> {
    Ok(connection
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM sync_state)")?
        .query_row([], |row| row.get(0))?)
}

/// The writer connection's temporary table of touched local ids and the
/// triggers that fill it. Every write to a table that holds a synced field
/// names the local id of the entity it changed; `record` reads the ids and
/// clears the table in the same transaction. A merge of one tag into another
/// names both tags in `sync_tag_merge`, so that the old name forwards to the
/// new one. `sync_new` holds the records that wait to be written. The table and triggers exist only while sync is on, so they add
/// no work to a write while it is off.
const TRIGGERS: &[(&str, &str, &str)] = &[
    ("item_insert", "AFTER INSERT ON main.library_item", "NEW.local_id"),
    ("item_delete", "AFTER DELETE ON main.library_item", "OLD.local_id"),
    ("item_update", "AFTER UPDATE ON main.library_item", "OLD.local_id"),
    ("media_insert", "AFTER INSERT ON main.media_item", "NEW.media_id"),
    ("media_delete", "AFTER DELETE ON main.media_item", "OLD.media_id"),
    ("media_update", "AFTER UPDATE ON main.media_item", "OLD.media_id"),
    ("root_insert", "AFTER INSERT ON main.library_root", "NEW.root_id"),
    ("root_delete", "AFTER DELETE ON main.library_root", "OLD.root_id"),
    ("root_update", "AFTER UPDATE ON main.library_root", "OLD.root_id"),
    ("tag_insert", "AFTER INSERT ON main.tag_definition", "NEW.tag_id"),
    ("tag_delete", "AFTER DELETE ON main.tag_definition", "OLD.tag_id"),
    ("tag_update", "AFTER UPDATE ON main.tag_definition", "OLD.tag_id"),
    ("group_insert", "AFTER INSERT ON main.tag_namespace", "NEW.namespace_id"),
    ("group_delete", "AFTER DELETE ON main.tag_namespace", "OLD.namespace_id"),
    ("folder_insert", "AFTER INSERT ON main.folder_definition", "NEW.folder_id"),
    ("folder_delete", "AFTER DELETE ON main.folder_definition", "OLD.folder_id"),
    ("folder_update", "AFTER UPDATE ON main.folder_definition", "OLD.folder_id"),
];

pub(crate) fn install_triggers(connection: &Connection) -> Result<()> {
    let touch = |id: &str| {
        format!(
            "INSERT INTO temp.sync_touched SELECT {id}
             WHERE NOT EXISTS (SELECT 1 FROM temp.sync_touched WHERE local_id = {id});"
        )
    };
    let mut sql = String::from(
        "CREATE TEMP TABLE IF NOT EXISTS sync_touched (local_id INTEGER PRIMARY KEY);
         CREATE TEMP TABLE IF NOT EXISTS sync_tag_merge (source TEXT PRIMARY KEY, destination TEXT NOT NULL);
         CREATE TEMP TABLE IF NOT EXISTS sync_new (
             entity INTEGER NOT NULL, field TEXT NOT NULL, target INTEGER NOT NULL,
             value TEXT NOT NULL, reference INTEGER
         );",
    );
    for (name, event, id) in TRIGGERS {
        sql.push_str(&format!(
            "CREATE TEMP TRIGGER IF NOT EXISTS sync_{name} {event} BEGIN {} END;",
            touch(id)
        ));
    }
    // Renaming a tag group renames every tag in it, and a change to the
    // facts of a file changes the `file` field of the item that uses it.
    sql.push_str(&format!(
        "CREATE TEMP TRIGGER IF NOT EXISTS sync_group_update AFTER UPDATE ON main.tag_namespace BEGIN
             {}
             INSERT INTO temp.sync_touched SELECT tag_id FROM main.tag_definition
             WHERE namespace_id = OLD.namespace_id
               AND tag_id NOT IN (SELECT local_id FROM temp.sync_touched);
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS sync_file_update
         AFTER UPDATE OF content_hash, mime, size_bytes, width, height, duration_ms, frame_count
         ON main.media_file BEGIN
             INSERT INTO temp.sync_touched SELECT media_id FROM main.media_item
             WHERE file_id = OLD.file_id
               AND media_id NOT IN (SELECT local_id FROM temp.sync_touched);
         END;",
        touch("OLD.namespace_id")
    ));
    connection.execute_batch(&sql)?;
    Ok(())
}

pub(crate) fn remove_triggers(connection: &Connection) -> Result<()> {
    let mut sql = String::new();
    for name in TRIGGERS
        .iter()
        .map(|(name, _, _)| *name)
        .chain(["group_update", "file_update"])
    {
        sql.push_str(&format!("DROP TRIGGER IF EXISTS temp.sync_{name};"));
    }
    for table in ["sync_touched", "sync_tag_merge", "sync_new"] {
        sql.push_str(&format!("DROP TABLE IF EXISTS temp.{table};"));
    }
    connection.execute_batch(&sql)?;
    Ok(())
}

/// Notes local ids whose entities changed without a trigger noting them, so
/// that the next write records them. Sync must be on.
pub(crate) fn touch(connection: &Connection, ids: &roaring::RoaringBitmap) -> Result<()> {
    let mut statement =
        connection.prepare_cached("INSERT OR IGNORE INTO temp.sync_touched(local_id) VALUES (?1)")?;
    for id in ids {
        statement.execute([id])?;
    }
    Ok(())
}

/// Notes that the tag `source` was merged into `destination`, when the
/// library records its changes. `record` then records that the old name
/// forwards to the new one.
pub(crate) fn note_tag_merge(connection: &Connection, source: &str, destination: &str) -> Result<()> {
    if recording(connection)? {
        connection
            .prepare_cached(
                "INSERT INTO temp.sync_tag_merge(source, destination) VALUES (?1, ?2)
                 ON CONFLICT(source) DO UPDATE SET destination = excluded.destination",
            )?
            .execute([source, destination])?;
    }
    Ok(())
}

/// The hybrid logical clock of this library, read once for a write and saved
/// at its end. Each timestamp is above every timestamp the library has seen
/// and above the wall clock.
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
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
/// entity that the value of a field without a target names.
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

