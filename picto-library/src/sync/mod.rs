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
/// new one. The table and triggers exist only while sync is on, so they add
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
         CREATE TEMP TABLE IF NOT EXISTS sync_tag_merge (source TEXT PRIMARY KEY, destination TEXT NOT NULL);",
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
    sql.push_str("DROP TABLE IF EXISTS temp.sync_touched; DROP TABLE IF EXISTS temp.sync_tag_merge;");
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
    device: String,
    wall_ms: i64,
    counter: i64,
}

impl Clock {
    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        Ok(connection.query_row(
            "SELECT device_id, clock_ms, clock_counter FROM sync_state",
            [],
            |row| {
                Ok(Self {
                    device: row.get(0)?,
                    wall_ms: row.get(1)?,
                    counter: row.get(2)?,
                })
            },
        )?)
    }

    pub(crate) fn tick(&mut self) -> SyncTimestamp {
        let now = now_ms();
        if now > self.wall_ms {
            self.wall_ms = now;
            self.counter = 0;
        } else {
            self.counter += 1;
        }
        SyncTimestamp {
            wall_ms: self.wall_ms,
            counter: self.counter,
            device: self.device.clone(),
        }
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

/// The entity that a field's value refers to, as `<kind>:<key>`, for the
/// `reference` column of `sync_value`.
fn reference(kind: SyncKind, field: &str, value: &Value) -> Option<String> {
    if let Some(tag) = field.strip_prefix("tag:") {
        return Some(format!("tag:{tag}"));
    }
    if let Some(folder) = field.strip_prefix("folder:") {
        return Some(format!("folder:{folder}"));
    }
    let first = || value.get(0).and_then(Value::as_str);
    match (kind, field) {
        (SyncKind::Item, "collection") => first().map(|key| format!("collection:{key}")),
        (SyncKind::Folder, "place") => first().map(|key| format!("folder:{key}")),
        (SyncKind::Tag, "renamed_to") => value.as_str().map(|key| format!("tag:{key}")),
        (SyncKind::Collection, "cover") => value.as_str().map(|key| format!("item:{key}")),
        (SyncKind::Folder, "cover") => {
            let kind = first()?;
            let key = value.get(1).and_then(Value::as_str)?;
            Some(format!("{kind}:{key}"))
        }
        _ => None,
    }
}

/// Writes change records that this library made, each with a new timestamp:
/// into the synced values, and into the outbox when `outbox` is true.
/// `records` holds (kind, key, field, value).
pub(crate) fn write_local(
    transaction: &Transaction<'_>,
    clock: &mut Clock,
    records: impl IntoIterator<Item = (SyncKind, String, String, Value)>,
    outbox: bool,
) -> Result<usize> {
    let mut value_statement = transaction.prepare_cached(
        "INSERT INTO sync_value(kind, key, field, value, clock_ms, clock_counter, device_id, reference)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(kind, key, field) DO UPDATE SET
             value = excluded.value, clock_ms = excluded.clock_ms,
             clock_counter = excluded.clock_counter, device_id = excluded.device_id,
             reference = excluded.reference",
    )?;
    let mut outbox_statement = transaction.prepare_cached(
        "INSERT OR REPLACE INTO sync_outbox(kind, key, field, value, clock_ms, clock_counter, device_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut count = 0;
    for (kind, key, field, value) in records {
        let stamp = clock.tick();
        let text = serde_json::to_string(&value)?;
        value_statement.execute(rusqlite::params![
            kind.as_str(),
            key,
            field,
            text,
            stamp.wall_ms,
            stamp.counter,
            stamp.device,
            reference(kind, &field, &value),
        ])?;
        if !outbox {
            count += 1;
            continue;
        }
        outbox_statement.execute(rusqlite::params![
            kind.as_str(),
            key,
            field,
            text,
            stamp.wall_ms,
            stamp.counter,
            stamp.device
        ])?;
        count += 1;
    }
    Ok(count)
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
        "INSERT INTO sync_value(kind, key, field, value, clock_ms, clock_counter, device_id, reference)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(kind, key, field) DO UPDATE SET
             value = excluded.value, clock_ms = excluded.clock_ms,
             clock_counter = excluded.clock_counter, device_id = excluded.device_id,
             reference = excluded.reference
         WHERE (excluded.clock_ms, excluded.clock_counter, excluded.device_id)
             > (sync_value.clock_ms, sync_value.clock_counter, sync_value.device_id)",
    )?;
    let mut merged = Vec::new();
    for record in records {
        clock.observe(&record.timestamp);
        let changed = statement.execute(rusqlite::params![
            record.kind.as_str(),
            record.key,
            record.field,
            serde_json::to_string(&record.value)?,
            record.timestamp.wall_ms,
            record.timestamp.counter,
            record.timestamp.device,
            reference(record.kind, &record.field, &record.value),
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
    let mut statement = connection.prepare(
        "SELECT kind, key, field, value, clock_ms, clock_counter, device_id
         FROM sync_outbox ORDER BY clock_ms, clock_counter",
    )?;
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

