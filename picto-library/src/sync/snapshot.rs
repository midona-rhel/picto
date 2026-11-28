//! Snapshots: what a snapshot of the synced values holds, which snapshots
//! are the base snapshots, how a library adopts them, and which snapshots
//! and change files the cleanup deletes.
//!
//! A snapshot holds the synced values of the cloud library up to its
//! `covers`: for each device, the last change file whose records it holds.
//! Snapshots rank by the sum of their `covers`, then by their id, which
//! begins with the time it was written. A snapshot covers another when its
//! `covers` is at least the other's for every device. The base snapshots
//! are the snapshots that no higher-ranked snapshot covers; the synced
//! values are their merge, every change file after their `covers`, and the
//! outbox. Every computer that sees the same snapshots finds the same base
//! snapshots.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction};

use super::cloud::{snapshot_id, CloudFolder, Manifest};
use super::{merge_remote, SyncKind, SyncRecord};
use crate::Result;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// How long a covered snapshot, an incomplete snapshot and a covered change
/// file stay in the cloud folder, and how old the newest base snapshot is
/// before a computer writes a new one.
pub(crate) const KEEP_MS: i64 = 7 * DAY_MS;
/// How long after its last record a deleted entity keeps its fields in the
/// snapshots, and a reference to it is kept.
const PRUNE_MS: i64 = 30 * DAY_MS;
/// About how many records one shard of a snapshot holds. A snapshot has at
/// most 100 shards, because their names have two digits.
const SHARD_RECORDS: i64 = 200_000;
/// How many steps a tag that was renamed is followed to the tag it went to,
/// as the projection follows it.
const FORWARD_STEPS: usize = 8;

fn rank(manifest: &Manifest) -> (i64, &str) {
    (manifest.covers.values().sum(), &manifest.snapshot_id)
}

/// Whether `a` covers `b`: it holds at least as much of every device as `b`.
pub(crate) fn covers(a: &Manifest, b: &Manifest) -> bool {
    b.covers.iter().all(|(device, seq)| a.covers.get(device).copied().unwrap_or(0) >= *seq)
}

/// The base snapshots among the valid snapshots `valid`, in the order of
/// their ids.
pub(crate) fn base_snapshots(valid: &[Manifest]) -> Vec<&Manifest> {
    valid
        .iter()
        .filter(|snapshot| !valid.iter().any(|other| rank(other) > rank(snapshot) && covers(other, snapshot)))
        .collect()
}

/// For each device, the greatest of the `covers` of `snapshots`.
pub(crate) fn greatest_covers(snapshots: &[&Manifest]) -> BTreeMap<String, i64> {
    let mut greatest = BTreeMap::new();
    for snapshot in snapshots {
        for (device, seq) in &snapshot.covers {
            let entry = greatest.entry(device.clone()).or_insert(0);
            *entry = (*entry).max(*seq);
        }
    }
    greatest
}

/// The snapshots among `valid` that the cleanup deletes at `now_ms`, lowest
/// ranked first: each one that a higher-ranked snapshot, which is not
/// deleted and was written more than `KEEP_MS` ago, covers. The rule is
/// applied to one snapshot at a time against those that remain, so that two
/// snapshots that cover each other are never both deleted.
pub(crate) fn covered_snapshots(valid: &[Manifest], now_ms: i64) -> Vec<String> {
    let mut ordered = valid.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| rank(a).cmp(&rank(b)));
    let mut deleted = HashSet::new();
    for snapshot in &ordered {
        let covered = ordered.iter().any(|other| {
            !deleted.contains(&other.snapshot_id)
                && rank(other) > rank(snapshot)
                && covers(other, snapshot)
                && now_ms - other.written_at_ms > KEEP_MS
        });
        if covered {
            deleted.insert(snapshot.snapshot_id.clone());
        }
    }
    ordered.into_iter().filter(|snapshot| deleted.contains(&snapshot.snapshot_id)).map(|snapshot| snapshot.snapshot_id.clone()).collect()
}

/// The shard of the entity `kind:key` in a snapshot with `shards` shards.
fn shard_of(kind: SyncKind, key: &str, shards: usize) -> usize {
    // FNV-1a, which is the same on every computer and in every version.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in kind.as_str().bytes().chain([0]).chain(key.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash % shards as u64) as usize
}

/// A stored record: value as JSON text, wall time, counter and device number.
type Stamped = (String, i64, i64, i64);

/// Writes a snapshot of the synced values into `folder` as written by the
/// device `device_id` on the computer `computer` at `now_ms`, with the
/// `covers` that the caller gives. The caller makes sure that the synced
/// values are the cloud data up to `covers`: the outbox is empty and every
/// change file up to them was read. While writing, the snapshot leaves out:
///
/// - every field of a deleted entity whose newest record is more than 30
///   days old, except `deleted` and `renamed_to`, which are kept for good so
///   that a computer that was away for weeks cannot bring the entity back;
/// - every reference to such an entity, and to an entity that the synced
///   values do not know, whose record is more than 30 days old. `place` is
///   kept, because without it a folder whose parent was deleted would move
///   to the start of the top level, and so are the lists of tags in folders'
///   auto-tags and smart folders' rules.
///
/// A `tag:<old>` field that names such a tag, when the tag was renamed to a
/// tag that the synced values show, is moved: the same record, with its
/// timestamp, is written as `tag:<new>` when it is newer than that field.
/// The projection follows `renamed_to` in the same way, so what the library
/// shows does not change.
pub(crate) fn write(
    connection: &Connection,
    folder: &CloudFolder,
    device_id: &str,
    computer: &str,
    covers: BTreeMap<String, i64>,
    now_ms: i64,
) -> Result<Manifest> {
    let cutoff = now_ms - PRUNE_MS;
    let devices = connection
        .prepare("SELECT device, device_id FROM sync_device")?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    // The deleted entities whose fields are left out.
    let pruned = connection
        .prepare(
            "SELECT deleted.entity FROM sync_value deleted
             WHERE deleted.field = 'deleted' AND deleted.target = 0 AND deleted.value = 'true'
               AND (SELECT MAX(value.clock_ms) FROM sync_value value WHERE value.entity = deleted.entity) < ?1",
        )?
        .query_map([cutoff], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    let moves = tag_moves(connection, &pruned)?;
    let moved = moved_tags(connection, &moves, &devices)?;

    let rows = connection.query_row("SELECT COUNT(*) FROM sync_value", [], |row| row.get::<_, i64>(0))?;
    let mut writer = folder.snapshot_writer(&snapshot_id(now_ms), ((rows + SHARD_RECORDS - 1) / SHARD_RECORDS).clamp(1, 100) as usize)?;
    let shards = writer.shards();
    let mut known = HashMap::new();
    let mut pruned_originals = Vec::new();
    let mut statement = connection.prepare(
        "SELECT entity.kind, entity.key, value.field, target.key, value.value, value.clock_ms, value.clock_counter,
                value.device, value.entity, value.target, value.reference
         FROM sync_value value
         CROSS JOIN sync_identity entity ON entity.entity = value.entity
         LEFT JOIN sync_identity target ON target.entity = value.target",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let Some(kind) = SyncKind::parse(&row.get::<_, String>(0)?) else { continue };
        let key = row.get::<_, String>(1)?;
        let field = row.get::<_, String>(2)?;
        let (wall_ms, counter, device) = (row.get::<_, i64>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?);
        let (entity, target, reference) = (row.get::<_, i64>(8)?, row.get::<_, i64>(9)?, row.get::<_, Option<i64>>(10)?);
        if pruned.contains(&entity) {
            if field != "deleted" && field != "renamed_to" {
                if kind == SyncKind::Item && field == "file" {
                    pruned_originals.push(key);
                }
                continue;
            }
        } else if field == "tag" && (moves.contains_key(&target) || moved.contains_key(&(entity, target))) {
            // The record goes out as a moved tag below.
            continue;
        } else if wall_ms < cutoff && field != "place" {
            let named = match (kind, field.as_str()) {
                (_, "tag" | "folder") => Some(target),
                (SyncKind::Item, "collection") | (SyncKind::Collection | SyncKind::Folder, "cover") => reference,
                _ => None,
            };
            if let Some(named) = named {
                if pruned.contains(&named) || !known_entity(connection, &mut known, named)? {
                    continue;
                }
            }
        }
        let field = match row.get::<_, Option<String>>(3)? {
            Some(target_key) if target != 0 => format!("{field}:{target_key}"),
            _ => field,
        };
        let device = devices.get(&device).map_or("", String::as_str);
        writer.add(shard_of(kind, &key, shards), kind, &key, &field, &row.get::<_, String>(4)?, wall_ms, counter, device)?;
    }
    drop(rows);
    drop(statement);
    for ((entity, target), (value, wall_ms, counter, device)) in &moved {
        if pruned.contains(entity) {
            continue;
        }
        let (Some((kind, key)), Some((_, target_key))) = (identity(connection, *entity)?, identity(connection, *target)?) else { continue };
        let device = devices.get(device).map_or("", String::as_str);
        writer.add(shard_of(kind, &key, shards), kind, &key, &format!("tag:{target_key}"), value, *wall_ms, *counter, device)?;
    }
    pruned_originals.sort();
    writer.finish(Manifest {
        snapshot_id: String::new(),
        device_id: device_id.to_owned(),
        computer: computer.to_owned(),
        written_at_ms: now_ms,
        covers,
        shards: Vec::new(),
        pruned_originals,
    })
}

/// For each tag among `pruned` that was renamed, the tag it went to that
/// the synced values show, following `renamed_to` at most `FORWARD_STEPS`
/// times.
fn tag_moves(connection: &Connection, pruned: &HashSet<i64>) -> Result<HashMap<i64, i64>> {
    let renamed = connection
        .prepare("SELECT entity, reference FROM sync_value WHERE field = 'renamed_to' AND target = 0 AND reference IS NOT NULL")?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    let mut visible = connection.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM sync_value WHERE field = 'deleted' AND target = 0 AND entity = ?1 AND value = 'false')",
    )?;
    let mut moves = HashMap::new();
    for (source, first) in &renamed {
        if !pruned.contains(source) {
            continue;
        }
        let mut target = *first;
        for _ in 0..FORWARD_STEPS {
            if visible.query_row([target], |row| row.get::<_, bool>(0))? {
                moves.insert(*source, target);
                break;
            }
            match renamed.get(&target) {
                Some(next) => target = *next,
                None => break,
            }
        }
    }
    Ok(moves)
}

/// The records that the moves give the tags they move to: for each
/// (entity, tag moved to), the latest of the entity's records of the tags
/// moved there and of that tag itself.
fn moved_tags(
    connection: &Connection,
    moves: &HashMap<i64, i64>,
    devices: &HashMap<i64, String>,
) -> Result<HashMap<(i64, i64), Stamped>> {
    let mut moved: HashMap<(i64, i64), Stamped> = HashMap::new();
    let later = |a: &Stamped, b: &Stamped| {
        let device = |stamped: &Stamped| devices.get(&stamped.3).cloned().unwrap_or_default();
        (a.1, a.2, device(a)) > (b.1, b.2, device(b))
    };
    let mut records = connection.prepare_cached(
        "SELECT entity, value, clock_ms, clock_counter, device FROM sync_value WHERE field = 'tag' AND target = ?1",
    )?;
    for (source, target) in moves {
        let rows = records
            .query_map([source], |row| Ok((row.get::<_, i64>(0)?, (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))))?
            .collect::<rusqlite::Result<Vec<(i64, Stamped)>>>()?;
        for (entity, stamped) in rows {
            match moved.get(&(entity, *target)) {
                Some(current) if !later(&stamped, current) => {}
                _ => {
                    moved.insert((entity, *target), stamped);
                }
            }
        }
    }
    let mut existing = connection.prepare_cached(
        "SELECT value, clock_ms, clock_counter, device FROM sync_value WHERE field = 'tag' AND target = ?2 AND entity = ?1",
    )?;
    for ((entity, target), stamped) in moved.iter_mut() {
        let current = existing
            .query_row([*entity, *target], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .optional()?;
        if let Some(current) = current.filter(|current| !later(stamped, current)) {
            *stamped = current;
        }
    }
    Ok(moved)
}

/// Whether the synced values hold any record of the entity `entity`.
fn known_entity(connection: &Connection, known: &mut HashMap<i64, bool>, entity: i64) -> Result<bool> {
    if let Some(found) = known.get(&entity) {
        return Ok(*found);
    }
    let found = connection
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM sync_value WHERE entity = ?1)")?
        .query_row([entity], |row| row.get::<_, bool>(0))?;
    known.insert(entity, found);
    Ok(found)
}

fn identity(connection: &Connection, entity: i64) -> Result<Option<(SyncKind, String)>> {
    Ok(connection
        .prepare_cached("SELECT kind, key FROM sync_identity WHERE entity = ?1")?
        .query_row([entity], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .optional()?
        .and_then(|(kind, key)| Some((SyncKind::parse(&kind)?, key))))
}

/// Starts to adopt base snapshots in `transaction`: from now on the fields
/// that the merged records hold are noted (`merge_adopted`), beginning with
/// those of the outbox.
pub(crate) fn begin_adoption(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM temp.sync_adopted", [])?;
    transaction.execute(
        "INSERT OR IGNORE INTO temp.sync_adopted(field, target, entity) SELECT field, target, entity FROM sync_outbox",
        [],
    )?;
    Ok(())
}

/// Merges `records` of a base snapshot or a change file as records from
/// other computers are merged, and notes the fields they hold.
pub(crate) fn merge_adopted(transaction: &Transaction<'_>, records: &[SyncRecord]) -> Result<()> {
    merge_remote(transaction, records)?;
    let mut note = transaction.prepare_cached("INSERT OR IGNORE INTO temp.sync_adopted(field, target, entity) VALUES (?1, ?2, ?3)")?;
    for record in records {
        let row = super::stored_row(transaction, record.kind, &record.key, &record.field, &record.value)?;
        note.execute(rusqlite::params![row.field, row.target, row.entity])?;
    }
    Ok(())
}

/// Finishes adopting base snapshots: removes the synced values of the
/// fields that neither the base snapshots, nor the change files after them,
/// nor the outbox hold, which are the fields that the snapshots left out.
/// They belong to entities that are not shown, or name entities that are
/// not shown, so the library does not change. Returns how many were removed.
pub(crate) fn finish_adoption(transaction: &Transaction<'_>) -> Result<usize> {
    let removed = transaction.execute(
        "DELETE FROM sync_value WHERE NOT EXISTS (
             SELECT 1 FROM temp.sync_adopted adopted
             WHERE adopted.field = sync_value.field AND adopted.target = sync_value.target
               AND adopted.entity = sync_value.entity)",
        [],
    )?;
    transaction.execute("DELETE FROM temp.sync_adopted", [])?;
    Ok(removed)
}
