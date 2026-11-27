//! Recording: a pass of the recorder finds what changed in the library since
//! the revision it last recorded, compares those entities with what the
//! synced values show, and records every field that differs with a new
//! timestamp. It runs on the sync file's connection, reads the library
//! through the attached `library` database in the same transaction, and
//! commits the records, its copies and the revision together.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Cursor;
use std::rc::Rc;

use roaring::RoaringBitmap;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};

use super::local::{lifecycle_name, Local};
use super::order::{self, Member};
use super::values::{Synced, View, ROOT_FIELDS};
use super::{Clock, SyncKind};
use crate::model::{FolderId, Lifecycle, Rating, RootId, TagId};
use crate::projection::ProjectionSnapshot;
use crate::Result;

/// A second comparison runs at most this many times. Each one compares the
/// entities that refer to a key whose visibility the records before it
/// changed; in practice the second one finds nothing.
const SECOND_PASSES: usize = 8;

/// The states of `sync_pending`.
pub(crate) const PROJECT: i64 = 0;
pub(crate) const REVIVE: i64 = 1;
const RECORD: i64 = 2;
pub(crate) const FIELDS: i64 = 3;

type Records = BTreeMap<(SyncKind, String, String), Value>;

/// What changed: local entities that are compared one by one, the lists
/// whose order is compared, and the roots that entered or left a lifecycle,
/// rating, tag or folder. A list marked `true` is compared as a whole;
/// otherwise only the part that differs from the list before.
#[derive(Default)]
struct Touched {
    ids: BTreeSet<u32>,
    folder_lists: BTreeMap<FolderId, bool>,
    collection_lists: BTreeMap<RootId, bool>,
    folder_tree: bool,
    changes: Vec<Change>,
}

/// The lists that changed, as (owner kind, owner id), and their order as
/// the recorder last recorded them.
#[derive(Default)]
struct Before {
    owners: Vec<(u8, u32)>,
    folder_orders: HashMap<FolderId, Vec<u32>>,
    collection_orders: HashMap<RootId, Vec<u32>>,
}

/// The roots that entered or left one lifecycle, rating, tag or folder.
struct Change {
    membership: Membership,
    added: RoaringBitmap,
    removed: RoaringBitmap,
}

#[derive(Clone, Copy, PartialEq)]
enum Membership {
    Lifecycle(Lifecycle),
    Rating(Rating),
    Tag(TagId),
    Folder(FolderId),
}

/// One field recorded for a set of roots: `value` for each root in `roots`
/// whose shown value (the stored one, or `default` when there is none)
/// differs from it, or for every root when `compare` is false.
struct SetRecord {
    roots: RoaringBitmap,
    field: &'static str,
    target: i64,
    value: String,
    default: &'static str,
    compare: bool,
}

/// Records what changed in the library between the revision in
/// `sync_state.recorded_revision` and `revision`, which `snapshot` shows.
/// `transaction` is on the sync file's connection and reads the library at
/// `revision`. `projected` is the revision of the last write of synced
/// values into the library, with whether it changed the structure of the
/// library, when that write came right after the previous pass.
pub(crate) fn record(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    revision: u64,
    projected: Option<(u64, bool)>,
) -> Result<()> {
    let since = transaction
        .query_row("SELECT recorded_revision FROM sync_state", [], |row| row.get::<_, i64>(0))
        .optional()?;
    let Some(since) = since else { return Ok(()) };
    // Whether an entity waits in one of two states, read through the index
    // on `sync_pending(state)`.
    let pending_in = |first: i64, second: i64| -> Result<bool> {
        Ok(transaction
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM sync_pending WHERE state IN (?1, ?2))")?
            .query_row([first, second], |row| row.get(0))?)
    };
    if since as u64 == revision && !pending_in(RECORD, RECORD)? {
        return Ok(());
    }
    // When the only write since the last pass is the projection of synced
    // values (`projected`), the library shows what the synced values show
    // for everything that write changed, so there is nothing to record. The
    // pass then only stores the keys of the entities that the write created
    // or renamed and brings the copies up to the library; the lists are
    // compared with their copies only when the write changed the structure.
    // Without this, every small transaction of a large projection would be
    // followed by a full pass that reads every waiting entity. Entities to
    // revive or to record need the full pass.
    let projection_only = match projected {
        Some((projected, structure))
            if projected == revision && since as u64 + 1 == revision && !pending_in(REVIVE, RECORD)? =>
        {
            Some(structure)
        }
        _ => None,
    };
    if let Some(structure) = projection_only {
        let mut touched = Touched { ids: changed_ids(transaction, since)?, ..Touched::default() };
        let mut recorder = Recorder::new(transaction, Before::default(), snapshot);
        let entities = recorder.entities(&mut touched)?;
        recorder.update_identities(&entities)?;
        drop(recorder);
        let owners = if structure { changed_lists(transaction)?.into_iter().collect() } else { Vec::new() };
        copy(transaction, since, &owners)?;
        transaction.execute("UPDATE sync_state SET recorded_revision = ?1", [revision as i64])?;
        return Ok(());
    }
    // The entities to revive or to record are few and are read here. The
    // entities that wait to be written can be many, and are read only where
    // the pass asks about them (`Waiting`).
    let pending = transaction
        .prepare_cached(
            "SELECT pending.entity, identity.kind, identity.key, identity.local_id, pending.state
             FROM sync_pending pending JOIN sync_identity identity ON identity.entity = pending.entity
             WHERE pending.state IN (?1, ?2)",
        )?
        .query_map([REVIVE, RECORD], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<u32>>(3)?, row.get::<_, i64>(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let (mut touched, mut before) = touched(transaction, snapshot, since)?;
    let owners = std::mem::take(&mut before.owners);
    let mut recorder = Recorder::new(transaction, before, snapshot);
    // An entity whose synced values wait to be written is compared like any
    // other. `write` leaves out the values that are no change of the
    // library's own, such as a field that still has the value it had before
    // the records from other computers (`sync_pending_value`). Its other
    // changes are recorded with a later timestamp, so the synced values that
    // are written later hold them.
    recorder.waiting = Waiting::load(transaction)?;
    let mut done = Vec::new();
    for (entity, kind, key, local_id, state) in pending {
        let Some(kind) = SyncKind::parse(&kind) else { continue };
        if state == RECORD {
            touched.ids.extend(local_id);
            recorder.recording.insert((kind, key));
            done.push(entity);
        } else {
            recorder.skipped.insert((kind, key));
            recorder.skipped_roots.extend(local_id);
        }
    }
    recorder.merges(since)?;
    let sets = recorder.split_sets(&mut touched)?;
    let mut clock = Clock::load(transaction)?;
    recorder.run(touched, &sets, &mut clock)?;
    clock.save(transaction)?;
    let mut statement = transaction.prepare_cached("DELETE FROM sync_pending WHERE entity = ?1")?;
    for entity in done {
        statement.execute([entity])?;
    }
    // A list that was not compared loses its copy, so that a later pass
    // compares it as a whole. A member that left it was recorded already.
    let held = std::mem::take(&mut recorder.held);
    drop(recorder);
    let mut remove = transaction.prepare_cached("DELETE FROM recorded_order WHERE owner_kind = ?1 AND owner_id = ?2")?;
    for owner in &held {
        remove.execute(*owner)?;
    }
    let owners = owners.into_iter().filter(|owner| !held.contains(owner)).collect::<Vec<_>>();
    copy(transaction, since, &owners)?;
    transaction.execute("UPDATE sync_state SET recorded_revision = ?1", [revision as i64])?;
    Ok(())
}

/// Brings the copies of the bitmaps and lists up to the library: every
/// bitmap row changed after `since`, and every row of the lists of `owners`.
fn copy(transaction: &Transaction<'_>, since: i64, owners: &[(u8, u32)]) -> Result<()> {
    transaction.execute(
        "INSERT OR REPLACE INTO recorded_bitmap(domain, key_id, high_bits, checksum, payload)
         SELECT domain, key_id, high_bits, checksum, payload FROM library.canonical_bitmap WHERE revision > ?1",
        [since],
    )?;
    let mut remove = transaction.prepare_cached("DELETE FROM recorded_order WHERE owner_kind = ?1 AND owner_id = ?2")?;
    let mut insert = transaction.prepare_cached(
        "INSERT INTO recorded_order(owner_kind, owner_id, chunk_index, checksum, payload)
         SELECT owner_kind, owner_id, chunk_index, checksum, payload FROM library.ordered_membership_chunk
         WHERE owner_kind = ?1 AND owner_id = ?2",
    )?;
    for owner in owners {
        remove.execute(*owner)?;
        insert.execute(*owner)?;
    }
    Ok(())
}

/// How many entities turning sync on compares at a time, so that the
/// records of a large library are not all held in memory at once.
const FULL_STATE_CHUNK: usize = 20_000;

/// Records the full state of the library into empty synced values when sync
/// is turned on, and copies its bitmaps and lists. The records go into the
/// synced values only, not into the outbox. The position keys of every list
/// are written first; the entities follow in chunks and take their positions
/// from the synced values. With empty synced values nothing forwards and
/// nothing is hidden, so no second comparison is needed.
pub(crate) fn record_full(transaction: &Transaction<'_>, snapshot: &ProjectionSnapshot) -> Result<()> {
    let mut clock = Clock::load(transaction)?;
    let mut touched = everything(transaction, snapshot)?;
    let ids = std::mem::take(&mut touched.ids).into_iter().collect::<Vec<_>>();
    let mut recorder = Recorder::new(transaction, Before::default(), snapshot);
    recorder.fresh = true;
    let owners = touched
        .folder_lists
        .keys()
        .map(|folder| (crate::ordering::OrderOwnerKind::Folder as u8, folder.0))
        .chain(touched.collection_lists.keys().map(|root| (crate::ordering::OrderOwnerKind::Collection as u8, root.0)))
        .collect::<Vec<_>>();
    // Each list is written before the next is compared, so that the
    // positions of a large library are not all held in memory at once.
    let lists = touched
        .folder_lists
        .into_keys()
        .map(|folder| Touched { folder_lists: [(folder, true)].into(), ..Touched::default() })
        .chain(touched.collection_lists.into_keys().map(|collection| Touched {
            collection_lists: [(collection, true)].into(),
            ..Touched::default()
        }))
        .chain([Touched { folder_tree: true, ..Touched::default() }]);
    for list in lists {
        recorder.lists(list)?;
        let positions = std::mem::take(&mut recorder.positions);
        recorder.write(positions, &mut clock, false)?;
    }
    for chunk in ids.chunks(FULL_STATE_CHUNK) {
        let mut recorder = Recorder::new(transaction, Before::default(), snapshot);
        recorder.written_positions = true;
        let entities = recorder.compare(Touched {
            ids: chunk.iter().copied().collect(),
            ..Touched::default()
        })?;
        let records = std::mem::take(&mut recorder.records);
        recorder.write(records, &mut clock, false)?;
        recorder.update_identities(&entities)?;
    }
    clock.save(transaction)?;
    copy(transaction, -1, &owners)
}

/// The records that a full comparison of the library with the synced values
/// would make, without making them. Once the recorder has caught up it is
/// empty.
#[cfg(test)]
pub(crate) fn differences(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
) -> Result<Vec<(SyncKind, String, String, Value)>> {
    let touched = everything(transaction, snapshot)?;
    let mut recorder = Recorder::new(transaction, Before::default(), snapshot);
    recorder.compare(touched)?;
    // Entities that the synced values show but the library lacks.
    let mut statement = transaction.prepare(
        "SELECT entity.kind, entity.key FROM sync_value value
         JOIN sync_identity entity ON entity.entity = value.entity
         WHERE value.field = 'deleted' AND value.target = 0 AND value.value = 'false'",
    )?;
    let alive = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (kind, key) in alive {
        let Some(kind) = SyncKind::parse(&kind) else { continue };
        if recorder.synced.visible(kind, &key)? && recorder.local.find(kind, &key)?.is_none() {
            recorder.put(kind, &key, "deleted", json!(true));
        }
    }
    Ok(recorder
        .records
        .into_iter()
        .map(|((kind, key, field), value)| (kind, key, field, value))
        .collect())
}

fn everything(connection: &Connection, snapshot: &ProjectionSnapshot) -> Result<Touched> {
    let mut touched = Touched {
        folder_tree: true,
        ..Touched::default()
    };
    for sql in [
        "SELECT media_id FROM media_item",
        "SELECT local_id FROM library_item WHERE item_kind = 2",
        "SELECT tag_id FROM tag_definition",
        "SELECT namespace_id FROM tag_namespace",
        "SELECT folder_id FROM folder_definition",
    ] {
        let mut statement = connection.prepare(sql)?;
        for id in statement.query_map([], |row| row.get::<_, u32>(0))? {
            touched.ids.insert(id?);
        }
    }
    touched.folder_lists = snapshot.folder_orders.keys().map(|id| (*id, true)).collect();
    touched.collection_lists = snapshot.collection_orders.keys().map(|id| (*id, true)).collect();
    Ok(touched)
}

fn decode_order(payloads: impl IntoIterator<Item = Vec<u8>>) -> Vec<u32> {
    payloads
        .into_iter()
        .flat_map(|payload| {
            payload.as_chunks::<4>().0.iter().map(|bytes| u32::from_le_bytes(*bytes)).collect::<Vec<_>>()
        })
        .collect()
}

/// Finds what changed in the library after the revision `since`: the local
/// ids in `changed_entity`, the bitmap rows whose revision is later and
/// whose checksum differs from the copy, and the lists whose chunks differ
/// from the copy. Returns the changes and the lists as they were.
fn touched(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    since: i64,
) -> Result<(Touched, Before)> {
    let mut touched = Touched { ids: changed_ids(transaction, since)?, ..Touched::default() };
    let mut before = Before::default();
    let mut statement = transaction.prepare_cached(
        "SELECT now.domain, now.key_id, now.high_bits, now.payload, recorded.payload
         FROM library.canonical_bitmap now
         LEFT JOIN recorded_bitmap recorded ON recorded.domain = now.domain
             AND recorded.key_id = now.key_id AND recorded.high_bits = now.high_bits
         WHERE now.revision > ?1 AND recorded.checksum IS NOT now.checksum",
    )?;
    let rows = statement.query_map([since], |row| {
        Ok((row.get::<_, u8>(0)?, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?, row.get::<_, Vec<u8>>(3)?, row.get::<_, Option<Vec<u8>>>(4)?))
    })?;
    let mut bitmaps: BTreeMap<(u8, u32), (RoaringBitmap, RoaringBitmap)> = BTreeMap::new();
    for row in rows {
        let (domain, key, high, now, recorded) = row?;
        let decode = |payload: &[u8]| -> Result<RoaringBitmap> {
            let shard = RoaringBitmap::deserialize_from(&mut Cursor::new(payload))?;
            Ok(shard.iter().map(|low| high << 16 | low).collect())
        };
        let now = decode(&now)?;
        let recorded = recorded.map(|payload| decode(&payload)).transpose()?.unwrap_or_default();
        let entry = bitmaps.entry((domain, key)).or_default();
        entry.0 |= &now - &recorded;
        entry.1 |= &recorded - &now;
    }
    for ((domain, key), (added, removed)) in bitmaps {
        let membership = match domain {
            1 => Lifecycle::ALL.into_iter().find(|value| value.bitmap_key() == key).map(Membership::Lifecycle),
            2 => Rating::ALL.into_iter().find(|value| value.bitmap_key() == key).map(Membership::Rating),
            _ => Some(Membership::Tag(TagId(key))),
        };
        if let Some(membership) = membership.filter(|_| !added.is_empty() || !removed.is_empty()) {
            touched.changes.push(Change { membership, added, removed });
        }
    }
    let owners = changed_lists(transaction)?;
    let mut statement = transaction.prepare_cached(
        "SELECT payload FROM recorded_order WHERE owner_kind = ?1 AND owner_id = ?2 ORDER BY chunk_index",
    )?;
    for (kind, owner) in &owners {
        let old = decode_order(statement.query_map((kind, owner), |row| row.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
        let folder = *kind == crate::ordering::OrderOwnerKind::Folder as u8;
        let now = if folder {
            snapshot.folder_orders.get(&FolderId(*owner)).map(|order| order.iter().map(|root| root.0).collect::<Vec<_>>())
        } else {
            snapshot.collection_orders.get(&RootId(*owner)).map(|order| order.iter().map(|media| media.0).collect())
        };
        let Some(now) = now else {
            // The folder or collection is gone; its members are compared.
            touched.ids.extend(old);
            if folder {
                touched.folder_tree = true;
            } else {
                touched.ids.insert(*owner);
            }
            continue;
        };
        let (old_set, new_set) = (old.iter().copied().collect::<RoaringBitmap>(), now.iter().copied().collect::<RoaringBitmap>());
        if folder {
            touched.folder_lists.insert(FolderId(*owner), false);
            if old_set != new_set {
                let (added, removed) = (&new_set - &old_set, &old_set - &new_set);
                touched.changes.push(Change { membership: Membership::Folder(FolderId(*owner)), added, removed });
            }
            before.folder_orders.insert(FolderId(*owner), old);
        } else {
            touched.collection_lists.insert(RootId(*owner), false);
            touched.ids.insert(*owner);
            touched.ids.extend((&old_set ^ &new_set).iter());
            before.collection_orders.insert(RootId(*owner), old);
        }
    }
    before.owners = owners.into_iter().collect();
    Ok((touched, before))
}

/// The local ids whose own row a write changed after the revision `since`.
fn changed_ids(transaction: &Transaction<'_>, since: i64) -> Result<BTreeSet<u32>> {
    let mut statement = transaction.prepare_cached("SELECT local_id FROM library.changed_entity WHERE revision > ?1")?;
    let ids = statement.query_map([since], |row| row.get::<_, u32>(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// The lists, as (owner kind, owner id), that differ from their copies in
/// `recorded_order`: their chunks are compared with the copies by checksum.
fn changed_lists(transaction: &Transaction<'_>) -> Result<BTreeSet<(u8, u32)>> {
    let chunks = |sql: &str| -> Result<BTreeMap<(u8, u32, i64), Vec<u8>>> {
        let mut statement = transaction.prepare_cached(sql)?;
        let rows = statement.query_map([], |row| Ok(((row.get(0)?, row.get(1)?, row.get(2)?), row.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let now = chunks("SELECT owner_kind, owner_id, chunk_index, checksum FROM library.ordered_membership_chunk")?;
    let recorded = chunks("SELECT owner_kind, owner_id, chunk_index, checksum FROM recorded_order")?;
    Ok(now
        .iter()
        .filter(|(key, checksum)| recorded.get(*key) != Some(*checksum))
        .map(|(key, _)| key)
        .chain(recorded.keys().filter(|key| !now.contains_key(*key)))
        .map(|(kind, owner, _)| (*kind, *owner))
        .collect())
}

/// One local entity that changed: the key the synced values knew it by, and
/// the key the library gives it now (`None` when it is gone).
struct Entity {
    kind: SyncKind,
    local_id: u32,
    old: Option<String>,
    new: Option<String>,
}

/// The entities whose synced values wait to be written (`sync_pending` in
/// the states `PROJECT` and `FIELDS`), each with the values its fields had
/// before the records from other computers that changed them
/// (`sync_pending_value`). A pass reads them only for the entities and roots
/// it asks about, so that its cost follows what changed in the library and
/// not the number of entities that wait: a large batch of records from
/// another computer can leave hundreds of thousands of them.
/// The values from before of the fields of a waiting entity, by field.
type OldValues = Rc<HashMap<String, Value>>;

#[derive(Default)]
struct Waiting {
    /// Whether any entity waits. When none does, nothing is read.
    any: bool,
    /// The entities read so far: for each, the values from before when it
    /// waits, and `None` when it does not.
    entities: RefCell<HashMap<(SyncKind, String), Option<OldValues>>>,
    /// The roots that waited when the pass began and that the pass took out
    /// of the waiting entities (`take`). They still count as waiting roots.
    taken_roots: RoaringBitmap,
}

impl Waiting {
    fn load(connection: &Connection) -> Result<Self> {
        let any = connection
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM sync_pending WHERE state IN (?1, ?2))")?
            .query_row([PROJECT, FIELDS], |row| row.get(0))?;
        Ok(Self { any, ..Self::default() })
    }

    /// The values from before of the entity `kind:key` when it waits, and
    /// `None` when it does not.
    fn get(&self, connection: &Connection, kind: SyncKind, key: &str) -> Result<Option<OldValues>> {
        if !self.any {
            return Ok(None);
        }
        let entity_key = (kind, key.to_owned());
        if let Some(found) = self.entities.borrow().get(&entity_key) {
            return Ok(found.clone());
        }
        let entity = connection
            .prepare_cached(
                "SELECT pending.entity FROM sync_identity identity
                 CROSS JOIN sync_pending pending ON pending.entity = identity.entity
                 WHERE identity.kind = ?1 AND identity.key = ?2 AND pending.state IN (?3, ?4)",
            )?
            .query_row(rusqlite::params![kind.as_str(), key, PROJECT, FIELDS], |row| row.get::<_, i64>(0))
            .optional()?;
        let found = match entity {
            None => None,
            Some(entity) => {
                let mut statement = connection.prepare_cached(
                    "SELECT CASE WHEN old.target = 0 THEN old.field ELSE old.field || ':' || target.key END, old.value
                     FROM sync_pending_value old
                     LEFT JOIN sync_identity target ON target.entity = old.target
                     WHERE old.entity = ?1",
                )?;
                let mut fields = HashMap::new();
                for row in statement.query_map([entity], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
                    let (field, value) = row?;
                    fields.insert(field, serde_json::from_str(&value)?);
                }
                Some(Rc::new(fields))
            }
        };
        self.entities.borrow_mut().insert(entity_key, found.clone());
        Ok(found)
    }

    fn contains(&self, connection: &Connection, kind: SyncKind, key: &str) -> Result<bool> {
        Ok(self.get(connection, kind, key)?.is_some())
    }

    /// Takes the entity `kind:key`, whose local id is `local_id`, out of the
    /// waiting entities for the rest of the pass, and returns its values
    /// from before when it waited.
    fn take(&mut self, connection: &Connection, kind: SyncKind, key: &str, local_id: u32) -> Result<Option<OldValues>> {
        let found = self.get(connection, kind, key)?;
        if found.is_some() {
            self.taken_roots.insert(local_id);
        }
        self.entities.borrow_mut().insert((kind, key.to_owned()), None);
        Ok(found)
    }

    /// Drops the value from before of `field` of the waiting entity
    /// `kind:key`. Returns whether it had one.
    fn forget(&self, connection: &Connection, kind: SyncKind, key: &str, field: &str) -> Result<bool> {
        let Some(mut fields) = self.get(connection, kind, key)? else { return Ok(false) };
        if !fields.contains_key(field) {
            return Ok(false);
        }
        Rc::make_mut(&mut fields).remove(field);
        self.entities.borrow_mut().insert((kind, key.to_owned()), Some(fields));
        Ok(true)
    }

    /// The roots among `roots` that wait.
    fn roots(&self, connection: &Connection, roots: &RoaringBitmap) -> Result<RoaringBitmap> {
        if !self.any || roots.is_empty() {
            return Ok(RoaringBitmap::new());
        }
        let mut waiting = connection
            .prepare_cached(
                "SELECT identity.local_id FROM json_each(?1) id
                 CROSS JOIN sync_identity identity ON identity.local_id = id.value
                 CROSS JOIN sync_pending pending ON pending.entity = identity.entity
                 WHERE pending.state IN (?2, ?3)",
            )?
            .query_map(rusqlite::params![ids_json(roots)?, PROJECT, FIELDS], |row| row.get::<_, u32>(0))?
            .collect::<rusqlite::Result<RoaringBitmap>>()?;
        waiting |= &(&self.taken_roots & roots);
        Ok(waiting)
    }
}

struct Recorder<'a> {
    transaction: &'a Transaction<'a>,
    before: Before,
    local: Local<'a>,
    synced: Synced<'a>,
    /// Tags merged since the last pass: source name to destination name.
    merges: HashMap<String, String>,
    /// Entities that are not compared and get no records in this pass: the
    /// items and collections that wait to be written as visible.
    skipped: HashSet<(SyncKind, String)>,
    skipped_roots: RoaringBitmap,
    /// The entities whose synced values wait to be written.
    waiting: Waiting,
    /// For each waiting root, the tags it entered or left in the library
    /// since the last pass, and the tags of it that were renamed, by their
    /// old and new names.
    waiting_tags: HashMap<u32, HashSet<String>>,
    /// The lists that were not compared because a member waits to be
    /// written, as (owner kind, owner id).
    held: Vec<(u8, u32)>,
    /// Entities whose synced values were written as visible although the
    /// synced values delete them; their full state is recorded now.
    recording: HashSet<(SyncKind, String)>,
    records: Records,
    /// Position keys that the comparison of lists gave, for members whose
    /// key changed.
    positions: Records,
    /// Whether the positions of every list are already in the synced
    /// values, as when sync is turned on; a full state then leaves them out.
    written_positions: bool,
    /// Whether the synced values hold no position keys yet, as when sync is
    /// turned on; the comparison of lists then reads none.
    fresh: bool,
    /// The clock before the first record of this pass.
    written_after: Option<(i64, i64)>,
    /// The tags that the last records written made visible.
    appeared_tags: BTreeSet<String>,
}

impl<'a> Recorder<'a> {
    fn new(transaction: &'a Transaction<'a>, before: Before, snapshot: &'a ProjectionSnapshot) -> Self {
        Self {
            transaction,
            before,
            local: Local::new(transaction, snapshot),
            synced: Synced::new(transaction),
            merges: HashMap::new(),
            skipped: HashSet::new(),
            skipped_roots: RoaringBitmap::new(),
            waiting: Waiting::default(),
            waiting_tags: HashMap::new(),
            held: Vec::new(),
            recording: HashSet::new(),
            records: Records::new(),
            positions: Records::new(),
            written_positions: false,
            fresh: false,
            written_after: None,
            appeared_tags: BTreeSet::new(),
        }
    }

    /// Reads the tags merged into another after the revision `since` from
    /// `tag_merge`, by the names the synced values know them by. A
    /// destination that was itself merged later is followed to the tag the
    /// library has.
    fn merges(&mut self, since: i64) -> Result<()> {
        let rows = self
            .transaction
            .prepare_cached("SELECT source_tag_id, destination_tag_id FROM library.tag_merge WHERE revision > ?1")?
            .query_map([since], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?)))?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        for (source, destination) in &rows {
            let Some((_, source)) = self.identity(SyncKind::Tag, *source)? else { continue };
            let mut destination = *destination;
            for _ in 0..rows.len() + 1 {
                if let Some(name) = self.local.key(SyncKind::Tag, destination)? {
                    self.merges.insert(source.clone(), name);
                    break;
                }
                match rows.get(&destination) {
                    Some(next) => destination = *next,
                    None => break,
                }
            }
        }
        Ok(())
    }

    fn put(&mut self, kind: SyncKind, key: &str, field: &str, value: Value) {
        self.records.insert((kind, key.to_owned(), field.to_owned()), value);
    }

    /// Compares one by one what needs it, records `sets`, writes everything
    /// together and runs the second comparison, which sees all of it.
    fn run(&mut self, touched: Touched, sets: &[SetRecord], clock: &mut Clock) -> Result<()> {
        let entities = self.compare(touched)?;
        self.update_identities(&entities)?;
        self.record_sets(sets)?;
        self.written_after = Some(clock.position());
        for pass in 0..SECOND_PASSES {
            if pass > 0 && self.records.is_empty() {
                break;
            }
            let records = std::mem::take(&mut self.records);
            self.positions.clear();
            let moved = records
                .keys()
                .any(|(kind, _, field)| *kind == SyncKind::Folder && matches!(field.as_str(), "place" | "deleted"));
            let changed = self.write(records, clock, true)?;
            if changed.is_empty() && !moved {
                break;
            }
            self.second_pass(changed, moved)?;
        }
        Ok(())
    }

    /// Compares the changed entities and lists and fills `records`. Returns
    /// the entities, so that their keys can be stored.
    fn compare(&mut self, mut touched: Touched) -> Result<Vec<Entity>> {
        let mut entities = self.entities(&mut touched)?;
        // An item or collection that the library has under the local id the
        // synced values knew it by, while the synced values delete it, came
        // back, as an undo of Delete Permanently brings it back. The synced
        // values are written over it first, so that what other computers
        // changed since is kept, and it is recorded after that.
        for entity in &entities {
            let Some(key) = entity.new.clone().filter(|key| entity.old.as_ref() == Some(key)) else { continue };
            if !matches!(entity.kind, SyncKind::Item | SyncKind::Collection)
                || self.recording.contains(&(entity.kind, key.clone()))
                || self.skipped.contains(&(entity.kind, key.clone()))
            {
                continue;
            }
            let deleted = self.synced.raw(entity.kind, &key)?.get("deleted").is_some_and(|stored| stored.value == json!(true));
            if deleted {
                // A deletion from another computer that waits to be written
                // is not a revival: the library had the entity before it.
                let entity_key = (entity.kind, key);
                let waiting = self.waiting.take(self.transaction, entity_key.0, &entity_key.1, entity.local_id)?;
                if waiting.as_ref().and_then(|fields| fields.get("deleted")) != Some(&json!(false)) {
                    self.transaction
                        .prepare_cached("INSERT OR REPLACE INTO sync_pending(entity, state) VALUES (?1, ?2)")?
                        .execute(rusqlite::params![super::entity(self.transaction, entity_key.0, &entity_key.1)?, REVIVE])?;
                }
                self.skipped_roots.insert(entity.local_id);
                self.skipped.insert(entity_key);
            }
        }
        entities.retain(|entity| {
            [&entity.old, &entity.new]
                .into_iter()
                .flatten()
                .all(|key| !self.skipped.contains(&(entity.kind, key.clone())))
        });
        let roots = entities
            .iter()
            .filter(|entity| matches!(entity.kind, SyncKind::Item | SyncKind::Collection))
            .map(|entity| entity.local_id)
            .collect::<RoaringBitmap>();
        self.local.prepare(&roots)?;
        let mut views = Vec::with_capacity(entities.len());
        for entity in &entities {
            let view = match entity.new {
                Some(_) => self.local.view(entity.kind, entity.local_id)?,
                None => None,
            };
            views.push(view);
        }
        // An entity the synced values do not show is recorded with all its
        // fields, including its position in each list that holds it. A list
        // that the write changed is compared where it changed, which takes
        // in the new entity. Any other list that holds such an entity
        // without a position key in the synced values is compared as a
        // whole.
        for (entity, view) in entities.iter().zip(&views) {
            let (Some(key), Some(view)) = (&entity.new, view) else { continue };
            if self.synced.visible(entity.kind, key)? && entity.old.as_deref() == Some(key) {
                continue;
            }
            let raw = self.synced.raw(entity.kind, key)?;
            match entity.kind {
                SyncKind::Item | SyncKind::Collection => {
                    for folder in &view.folders {
                        if !raw.contains_key(&format!("folder:{folder}")) {
                            if let Some(id) = self.local.find(SyncKind::Folder, folder)? {
                                touched.folder_lists.entry(FolderId(id)).or_insert(true);
                            }
                        }
                    }
                    if let Some(collection) = &view.owner {
                        if !raw.contains_key("collection") {
                            if let Some(id) = self.local.find(SyncKind::Collection, collection)? {
                                touched.collection_lists.entry(RootId(id)).or_insert(true);
                            }
                        }
                    }
                }
                SyncKind::Folder => touched.folder_tree = true,
                SyncKind::Tag | SyncKind::TagGroup => {}
            }
        }
        self.lists(touched)?;
        for kind in [
            SyncKind::TagGroup,
            SyncKind::Tag,
            SyncKind::Folder,
            SyncKind::Collection,
            SyncKind::Item,
        ] {
            for (entity, view) in entities.iter().zip(&views) {
                if entity.kind == kind {
                    self.entity(entity, view.as_ref())?;
                }
            }
        }
        let positions = std::mem::take(&mut self.positions);
        for (record, value) in positions {
            self.records.entry(record).or_insert(value);
        }
        Ok(entities)
    }

    /// Compares the lists in `touched` and fills `positions`.
    fn lists(&mut self, touched: Touched) -> Result<()> {
        for (folder, whole) in touched.folder_lists {
            self.folder_list(folder, whole)?;
        }
        for (collection, whole) in touched.collection_lists {
            self.collection_list(collection, whole)?;
        }
        if touched.folder_tree {
            self.folder_tree()?;
        }
        Ok(())
    }

    /// The changed local ids as entities with their old and new keys. A key
    /// change touches what refers to the entity: the items and folders of a
    /// renamed tag, and the lists and covers that hold an item whose hash
    /// was repaired.
    fn entities(&mut self, touched: &mut Touched) -> Result<Vec<Entity>> {
        let mut entities = Vec::new();
        let mut queue = touched.ids.iter().copied().collect::<Vec<_>>();
        let mut seen = touched.ids.clone();
        while let Some(local_id) = queue.pop() {
            let identity = self
                .transaction
                .prepare_cached("SELECT kind, key FROM sync_identity WHERE local_id = ?1")?
                .query_row([local_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .optional()?;
            let (kind, old) = match identity {
                Some((kind, key)) => (SyncKind::parse(&kind), Some(key)),
                None => (self.local.kind(local_id)?, None),
            };
            let Some(kind) = kind else { continue };
            let new = self.local.key(kind, local_id)?;
            if old.is_none() && new.is_none() {
                continue;
            }
            let mut referring = Vec::new();
            if old.is_some() && new.is_some() && old != new {
                match kind {
                    SyncKind::Tag => {
                        if let Some(members) = self.local.snapshot.tags.get(&crate::TagId(local_id)) {
                            referring.extend(members.iter());
                            let waiting = self.waiting.roots(self.transaction, members)?;
                            if !waiting.is_empty() {
                                self.note_waiting_tag(local_id, &waiting)?;
                            }
                        }
                        referring.extend(folders_with_auto_tag(self.transaction, local_id)?);
                    }
                    SyncKind::Item => {
                        let owner = self.local.snapshot.media_owner.get(local_id).copied();
                        if let Some(owner) = owner.filter(|owner| owner.0 != local_id) {
                            touched.collection_lists.insert(owner, true);
                            referring.push(owner.0);
                        }
                        for (folder, members) in self.local.snapshot.folders.iter() {
                            if members.contains(local_id) {
                                touched.folder_lists.insert(*folder, true);
                            }
                        }
                        let mut statement = self
                            .transaction
                            .prepare_cached("SELECT folder_id FROM folder_definition WHERE cover_root_id = ?1")?;
                        for folder in statement.query_map([local_id], |row| row.get::<_, u32>(0))? {
                            referring.push(folder?);
                        }
                    }
                    _ => {}
                }
            }
            for id in referring {
                if seen.insert(id) {
                    queue.push(id);
                }
            }
            if kind == SyncKind::Folder {
                touched.folder_tree = true;
            }
            entities.push(Entity {
                kind,
                local_id,
                old,
                new,
            });
        }
        Ok(entities)
    }

    /// Decides which roots of the membership changes are recorded as a set:
    /// those that changed only through memberships. The others are
    /// compared one by one, and so are the roots of a tag that was renamed or
    /// that another key forwards to, because what the synced values show for
    /// those needs the rules of projection. Returns the fields to record for
    /// the set.
    fn split_sets(&mut self, touched: &mut Touched) -> Result<Vec<SetRecord>> {
        let ids = touched.ids.iter().copied().collect::<RoaringBitmap>();
        let mut single = RoaringBitmap::new();
        let mut sets = Vec::new();
        for change in std::mem::take(&mut touched.changes) {
            let set = |roots, field, target, value: String, default| SetRecord {
                roots,
                field,
                target,
                value,
                default,
                compare: true,
            };
            match change.membership {
                Membership::Lifecycle(lifecycle) => sets.push(set(
                    change.added,
                    "lifecycle",
                    0,
                    json!(lifecycle_name(lifecycle)).to_string(),
                    "\"inbox\"",
                )),
                Membership::Rating(rating) => {
                    sets.push(set(change.added, "rating", 0, rating.bitmap_key().to_string(), "0"));
                }
                Membership::Tag(tag) => {
                    let waiting = self.waiting.roots(self.transaction, &(&change.added | &change.removed))?;
                    if !waiting.is_empty() {
                        self.note_waiting_tag(tag.0, &waiting)?;
                    }
                    match self.plain_tag(tag)? {
                        Some((target, shown)) => {
                            // A tag that the synced values do not show is not
                            // shown under its own name on any root, so every
                            // root that gets it records it. Roots added to the
                            // bitmap of a tag that the library no longer has, as
                            // an undo can add them after the tag was deleted,
                            // do not have the tag.
                            if self.local.key(SyncKind::Tag, tag.0)?.is_some() {
                                sets.push(SetRecord { compare: shown, ..set(change.added, "tag", target, "true".into(), "false") });
                            }
                            if shown {
                                sets.push(set(change.removed, "tag", target, "false".into(), "false"));
                            }
                        }
                        None => {
                            single |= &change.added;
                            single |= &change.removed;
                        }
                    }
                }
                // A root added to a folder gets its position from the
                // comparison of the folder's list.
                Membership::Folder(folder) => {
                    let entered = self.waiting.roots(self.transaction, &change.added)?;
                    if !entered.is_empty() {
                        self.forget_old_positions(folder, &entered)?;
                    }
                    if let Some(target) = self.shown_folder(folder)? {
                        sets.push(set(change.removed, "folder", target, "null".into(), "null"));
                    }
                }
            }
        }
        // A waiting root is compared on its own, where `write` sees the
        // values its fields had before the records from other computers.
        for set in &sets {
            single |= &self.waiting.roots(self.transaction, &set.roots)?;
        }
        single -= &ids;
        single -= &self.skipped_roots;
        for set in &mut sets {
            set.roots -= &self.skipped_roots;
            set.roots -= &ids;
            set.roots -= &single;
        }
        touched.ids.extend(single.iter());
        Ok(sets)
    }

    /// The entity of a tag whose roots can be recorded as a set, and whether
    /// the synced values show it. They can when no other key forwards to the
    /// tag and the tag kept its name: a root then shows the tag exactly when
    /// the synced values show the tag and the root's stored `tag:` value is
    /// `true`.
    fn plain_tag(&mut self, tag: TagId) -> Result<Option<(i64, bool)>> {
        let known = self.identity(SyncKind::Tag, tag.0)?;
        let key = match (&known, self.local.key(SyncKind::Tag, tag.0)?) {
            (Some((_, old)), Some(now)) if *old != now => return Ok(None),
            (Some((_, old)), _) => old.clone(),
            (None, Some(now)) => now,
            (None, None) => return Ok(None),
        };
        let target = match known {
            Some((entity, _)) => entity,
            None => super::entity(self.transaction, SyncKind::Tag, &key)?,
        };
        let forwarded = self
            .transaction
            .prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM sync_value WHERE reference = ?1 AND field = 'renamed_to')",
            )?
            .query_row([target], |row| row.get::<_, bool>(0))?;
        if forwarded {
            return Ok(None);
        }
        Ok(Some((target, self.synced.visible(SyncKind::Tag, &key)?)))
    }

    /// Notes that the waiting `roots` entered or left the tag `tag`, under
    /// its name in the library and the name the synced values know it by.
    fn note_waiting_tag(&mut self, tag: u32, roots: &RoaringBitmap) -> Result<()> {
        let names = [self.local.key(SyncKind::Tag, tag)?, self.identity(SyncKind::Tag, tag)?.map(|(_, key)| key)];
        for root in roots {
            self.waiting_tags.entry(root).or_default().extend(names.iter().flatten().cloned());
        }
        Ok(())
    }

    /// Drops the positions from before the records of other computers that
    /// the waiting `roots` had in `folder`, which they entered since the last
    /// pass: the library no longer holds them where those positions put
    /// them. They entered it as a write of synced values put them there, at
    /// the position the synced values hold, or as the user added them.
    fn forget_old_positions(&mut self, folder: FolderId, roots: &RoaringBitmap) -> Result<()> {
        let Some(folder_key) = self.local.folder_key(folder.0)? else { return Ok(()) };
        let field = format!("folder:{folder_key}");
        let target = super::entity(self.transaction, SyncKind::Folder, &folder_key)?;
        for root in roots {
            let Some((kind, key)) = self.local.root_key(RootId(root))? else { continue };
            if self.waiting.forget(self.transaction, kind, &key, &field)? {
                self.transaction
                    .prepare_cached("DELETE FROM sync_pending_value WHERE entity = ?1 AND field = 'folder' AND target = ?2")?
                    .execute(rusqlite::params![super::entity(self.transaction, kind, &key)?, target])?;
            }
        }
        Ok(())
    }

    /// The entity of a folder that the library still has and the synced
    /// values show.
    fn shown_folder(&mut self, folder: FolderId) -> Result<Option<i64>> {
        if !self.local.snapshot.folder_orders.contains_key(&folder) {
            return Ok(None);
        }
        let Some((entity, key)) = self.identity(SyncKind::Folder, folder.0)? else { return Ok(None) };
        Ok(self.synced.visible(SyncKind::Folder, &key)?.then_some(entity))
    }

    fn identity(&self, kind: SyncKind, local_id: u32) -> Result<Option<(i64, String)>> {
        Ok(self
            .transaction
            .prepare_cached("SELECT entity, key FROM sync_identity WHERE local_id = ?1 AND kind = ?2")?
            .query_row(rusqlite::params![local_id, kind.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?)
    }

    /// Records the fields of roots as sets: one statement for each field
    /// reads the stored values of the whole set and adds a record to
    /// `temp.sync_new` for each root whose shown value differs. A root that
    /// is in a set existed before the write, so the synced values know it.
    /// The records are written with the next records that `write` writes.
    fn record_sets(&mut self, sets: &[SetRecord]) -> Result<()> {
        let mut statement = self.transaction.prepare_cached(
            "INSERT INTO temp.sync_new(entity, field, target, value, reference)
             SELECT identity.entity, ?2, ?3, ?4, NULL
             FROM json_each(?1) changed CROSS JOIN sync_identity identity
                 ON identity.local_id = changed.value
             WHERE NOT ?6 OR COALESCE(NULLIF((SELECT stored.value FROM sync_value stored
                                     WHERE stored.entity = identity.entity AND stored.field = ?2
                                       AND stored.target = ?3), 'null'), ?5) <> ?4",
        )?;
        // A tag added to a root is recorded after the tags removed from it,
        // so that when a merge forwards the removed tag to the added one,
        // the later record, which decides, is the added tag.
        let (added, others): (Vec<_>, Vec<_>) = sets.iter().partition(|set| set.value == "true");
        for set in others.into_iter().chain(added).filter(|set| !set.roots.is_empty()) {
            statement.execute(rusqlite::params![
                ids_json(&set.roots)?,
                set.field,
                set.target,
                set.value,
                set.default,
                set.compare
            ])?;
        }
        Ok(())
    }

    /// Stores the key of each entity whose key the synced values now know.
    /// A deleted entity keeps its local id.
    fn update_identities(&self, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            if entity.old == entity.new {
                continue;
            }
            if let Some(key) = &entity.new {
                self.transaction
                    .execute("UPDATE sync_identity SET local_id = NULL WHERE local_id = ?1", [entity.local_id])?;
                self.transaction.execute(
                    "INSERT INTO sync_identity(kind, key, local_id) VALUES (?1, ?2, ?3)
                     ON CONFLICT(kind, key) DO UPDATE SET local_id = excluded.local_id",
                    rusqlite::params![entity.kind.as_str(), key, entity.local_id],
                )?;
            }
        }
        Ok(())
    }

    fn entity(&mut self, entity: &Entity, view: Option<&View>) -> Result<()> {
        let kind = entity.kind;
        if let Some(old) = entity.old.as_deref().filter(|old| Some(*old) != entity.new.as_deref()) {
            if self.synced.visible(kind, old)? {
                self.put(kind, old, "deleted", json!(true));
            }
            // A renamed or merged tag forwards to its new name.
            if kind == SyncKind::Tag {
                let target = entity.new.clone().or_else(|| self.merges.get(old).cloned());
                if let Some(target) = target {
                    self.forward(old, target)?;
                }
            }
        }
        if let Some(key) = &entity.new {
            self.diff(kind, key, view)?;
        }
        Ok(())
    }

    /// Records that the tag `key` forwards to `target`, unless the synced
    /// values already say so.
    fn forward(&mut self, key: &str, target: String) -> Result<()> {
        let stored = self.synced.raw(SyncKind::Tag, key)?.get("renamed_to").map(|stored| stored.value.clone());
        if stored.as_ref().and_then(Value::as_str) != Some(target.as_str()) {
            self.put(SyncKind::Tag, key, "renamed_to", json!(target));
        }
        Ok(())
    }

    /// Compares one entity of the library with the synced values.
    fn diff(&mut self, kind: SyncKind, key: &str, view: Option<&View>) -> Result<()> {
        let shown = self.synced.view(kind, key)?;
        match (view, shown) {
            (None, Some(_)) => {
                self.put(kind, key, "deleted", json!(true));
                if let Some(target) = self.merges.get(key).filter(|_| kind == SyncKind::Tag).cloned() {
                    self.forward(key, target)?;
                }
            }
            // An entity that waits to be written while the synced values do
            // not show it is removed from the library when it is written.
            (Some(_), None) if self.waiting.contains(self.transaction, kind, key)? => {}
            (Some(view), None) => self.full_state(kind, key, view)?,
            (Some(view), Some(shown)) => self.changes(kind, key, view, &shown)?,
            (None, None) => {}
        }
        Ok(())
    }

    /// Records every field of an entity that the synced values do not show,
    /// and `null` or `false` for each field they hold that the library does
    /// not have.
    fn full_state(&mut self, kind: SyncKind, key: &str, view: &View) -> Result<()> {
        let raw = self.synced.raw(kind, key)?;
        let mut fields = view.fields.clone();
        fields.insert("deleted".into(), json!(false));
        match kind {
            SyncKind::Item => {
                let collection = match &view.owner {
                    Some(collection) => self.position(kind, key, "collection", Some(json!(collection)), || {
                        json!([collection, ""])
                    })?,
                    None => Value::Null,
                };
                fields.insert("collection".into(), collection);
            }
            SyncKind::Folder => {
                let place = self.position(kind, key, "place", Some(json!(view.owner)), || json!([view.owner, ""]))?;
                fields.insert("place".into(), place);
            }
            _ => {}
        }
        for tag in &view.tags {
            fields.insert(format!("tag:{tag}"), json!(true));
        }
        for folder in &view.folders {
            let field = format!("folder:{folder}");
            let position = self.position(kind, key, &field, None, || json!(""))?;
            fields.insert(field, position);
        }
        for field in raw.keys() {
            if !fields.contains_key(field) {
                let empty = if field.starts_with("tag:") { json!(false) } else { Value::Null };
                fields.insert(field.clone(), empty);
            }
        }
        for (field, value) in fields {
            let empty = value.is_null() || value == json!(false) && field.starts_with("tag:");
            if empty && !raw.contains_key(&field) {
                continue;
            }
            let position = field.starts_with("folder:") || field == "collection" || field == "place";
            if self.written_positions && position && raw.get(&field).is_some_and(|stored| stored.value == value) {
                continue;
            }
            self.put(kind, key, &field, value);
        }
        Ok(())
    }

    /// The position key of a member: the one the comparison of its list
    /// gave, or the one the synced values hold.
    /// For `collection` and `place`, whose value names the list as well,
    /// the stored value counts only when it names `list`.
    fn position(&mut self, kind: SyncKind, key: &str, field: &str, list: Option<Value>, fallback: impl FnOnce() -> Value) -> Result<Value> {
        if let Some(value) = self.positions.remove(&(kind, key.to_owned(), field.to_owned())) {
            return Ok(value);
        }
        let old = self.old_value(kind, key, field)?.filter(|old| list.as_ref().is_none_or(|list| old.get(0) == Some(list)));
        Ok(        old.or_else(|| self.synced.raw(kind, key).ok().and_then(|raw| raw.get(field).map(|stored| stored.value.clone())))
            .filter(|value| !value.is_null())
            .filter(|value| list.as_ref().is_none_or(|list| value.get(0) == Some(list)))
            .unwrap_or_else(fallback))
    }

    fn changes(&mut self, kind: SyncKind, key: &str, view: &View, shown: &View) -> Result<()> {
        for (field, value) in &view.fields {
            if ROOT_FIELDS.contains(&field.as_str()) {
                continue;
            }
            if shown.fields.get(field) != Some(value) {
                self.put(kind, key, field, value.clone());
            }
        }
        match kind {
            SyncKind::Item => {
                let field = (kind, key.to_owned(), "collection".to_owned());
                if view.owner != shown.owner || self.positions.contains_key(&field) {
                    let value = match &view.owner {
                        Some(collection) => {
                            self.position(kind, key, "collection", Some(json!(collection)), || json!([collection, ""]))?
                        }
                        None => Value::Null,
                    };
                    self.put(kind, key, "collection", value);
                }
                if view.owner.is_none() {
                    self.root_changes(kind, key, view)?;
                }
            }
            SyncKind::Collection => self.root_changes(kind, key, view)?,
            SyncKind::Folder => {
                let field = (kind, key.to_owned(), "place".to_owned());
                if view.owner != shown.owner || self.positions.contains_key(&field) {
                    let value = self.position(kind, key, "place", Some(json!(view.owner)), || json!([view.owner, ""]))?;
                    self.put(kind, key, "place", value);
                }
            }
            SyncKind::Tag | SyncKind::TagGroup => {}
        }
        Ok(())
    }

    /// Compares the fields of an item outside a collection, or of a
    /// collection, with what the synced values hold for them.
    fn root_changes(&mut self, kind: SyncKind, key: &str, view: &View) -> Result<()> {
        let mut expected = View::default();
        self.synced.root_view(kind, key, &mut expected)?;
        for field in ROOT_FIELDS {
            if view.fields.get(field) != expected.fields.get(field) {
                let value = view.fields.get(field).cloned().unwrap_or(Value::Null);
                self.put(kind, key, field, value);
            }
        }
        for tag in view.tags.difference(&expected.tags) {
            self.put(kind, key, &format!("tag:{tag}"), json!(true));
        }
        for tag in expected.tags.difference(&view.tags) {
            self.put(kind, key, &format!("tag:{tag}"), json!(false));
            // A tag shown through a key that forwards to it is removed from
            // that key too.
            for other in self.synced.forwarded_to(kind, key, tag)? {
                self.put(kind, key, &format!("tag:{other}"), json!(false));
            }
        }
        for folder in expected.folders.difference(&view.folders) {
            if self.local.find(SyncKind::Folder, folder)?.is_some() {
                self.put(kind, key, &format!("folder:{folder}"), Value::Null);
            }
        }
        Ok(())
    }

    /// Whether a member of `list` waits for its synced values to be written.
    /// The list is then not in the order the synced values show yet, so it
    /// is compared as a whole in a pass after that member is written
    /// (`held`), which also takes in what else changed in it meanwhile.
    fn waits(&self, list: &[u32]) -> bool {
        !self.skipped_roots.is_empty() && list.iter().any(|id| self.skipped_roots.contains(*id))
    }

    /// The value a field of a waiting entity had before the records from
    /// other computers changed it, when it had one. The library still holds
    /// the entity in the place that value gives it, so the comparison of
    /// lists reads it instead of the synced value.
    fn old_value(&self, kind: SyncKind, key: &str, field: &str) -> Result<Option<Value>> {
        Ok(self.waiting.get(self.transaction, kind, key)?.and_then(|old| old.get(field).filter(|value| !value.is_null()).cloned()))
    }

    /// Compares the item list of a folder with the position keys of its
    /// members. A member that left the folder is touched itself, and the
    /// comparison of its fields (`root_changes`) records `null` for it.
    fn folder_list(&mut self, folder: FolderId, whole: bool) -> Result<()> {
        let Some(folder_key) = self.local.folder_key(folder.0)? else {
            return Ok(());
        };
        let field = format!("folder:{folder_key}");
        let after_ids = self.local.snapshot.folder_orders.get(&folder).map_or_else(Vec::new, |order| order.iter().map(|root| root.0).collect());
        if self.waits(&after_ids) {
            self.held.push((crate::ordering::OrderOwnerKind::Folder as u8, folder.0));
            return Ok(());
        }
        let window = match self.before.folder_orders.get(&folder) {
            Some(before) if !whole => changed_window(before, &after_ids),
            _ => (0, after_ids.len()),
        };
        let mut lookup = |recorder: &mut Self, root: u32| -> Result<Option<(SyncKind, String, Option<String>)>> {
            let Some((kind, key)) = recorder.local.root_key(RootId(root))? else { return Ok(None) };
            if recorder.fresh {
                return Ok(Some((kind, key, None)));
            }
            let value = match recorder.old_value(kind, &key, &field)? {
                Some(old) => Some(old),
                None => recorder.synced.raw(kind, &key)?.get(&field).map(|stored| stored.value.clone()),
            };
            let position = value.and_then(|value| value.as_str().map(str::to_owned));
            Ok(Some((kind, key, position)))
        };
        for (kind, key, position) in self.order(&after_ids, window, &mut lookup)? {
            self.positions.insert((kind, key, field.clone()), json!(position));
        }
        Ok(())
    }

    fn collection_list(&mut self, collection: RootId, whole: bool) -> Result<()> {
        let Some(collection_key) = self.local.key(SyncKind::Collection, collection.0)? else {
            return Ok(());
        };
        let after_ids = self.local.snapshot.collection_orders.get(&collection).map_or_else(Vec::new, |order| order.iter().map(|media| media.0).collect());
        if self.waits(&after_ids) {
            self.held.push((crate::ordering::OrderOwnerKind::Collection as u8, collection.0));
            return Ok(());
        }
        let window = match self.before.collection_orders.get(&collection) {
            Some(before) if !whole => changed_window(before, &after_ids),
            _ => (0, after_ids.len()),
        };
        let mut lookup = |recorder: &mut Self, media: u32| -> Result<Option<(SyncKind, String, Option<String>)>> {
            let Some(key) = recorder.local.key(SyncKind::Item, media)? else { return Ok(None) };
            if recorder.fresh {
                return Ok(Some((SyncKind::Item, key, None)));
            }
            let value = match recorder.old_value(SyncKind::Item, &key, "collection")? {
                Some(old) => Some(old),
                None => recorder.synced.raw(SyncKind::Item, &key)?.get("collection").map(|stored| stored.value.clone()),
            };
            let position = value
                .as_ref()
                .filter(|value| value.get(0).and_then(Value::as_str) == Some(collection_key.as_str()))
                .and_then(|value| value.get(1).and_then(Value::as_str).map(str::to_owned));
            Ok(Some((SyncKind::Item, key, position)))
        };
        for (kind, key, position) in self.order(&after_ids, window, &mut lookup)? {
            self.positions.insert((kind, key, "collection".into()), json!([collection_key, position]));
        }
        Ok(())
    }

    /// Compares the children of every folder with their position keys.
    fn folder_tree(&mut self) -> Result<()> {
        let mut statement = self.transaction.prepare_cached(
            "SELECT folder_id, parent_id FROM folder_definition ORDER BY parent_id, display_order, folder_id",
        )?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, Option<u32>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let mut children: BTreeMap<Option<u32>, Vec<u32>> = BTreeMap::new();
        for (folder, parent) in rows {
            children.entry(parent).or_default().push(folder);
        }
        let tree = self.synced.folder_tree()?;
        for (parent, list) in children {
            if self.waits(&list) {
                continue;
            }
            let parent_key = match parent {
                Some(parent) => self.local.folder_key(parent)?,
                None => None,
            };
            let mut lookup = |recorder: &mut Self, folder: u32| -> Result<Option<(SyncKind, String, Option<String>)>> {
                let Some(key) = recorder.local.folder_key(folder)? else { return Ok(None) };
                // A folder that waits to be written keeps the position key
                // it had before the records from other computers, also
                // where its parent was gone and the folder was shown at the
                // top, and also when the records delete it. A folder whose
                // stored parent is the one it is in keeps its stored key,
                // also while that parent waits to be removed; one shown at
                // the top because its parent is gone keeps the key it is
                // shown with.
                let waiting = recorder.waiting.contains(recorder.transaction, SyncKind::Folder, &key)?;
                let stored = recorder.synced.raw(SyncKind::Folder, &key)?.get("place").map(|stored| stored.value.clone());
                let stored_key = stored.as_ref().and_then(|place| place.get(1)).and_then(Value::as_str).map(str::to_owned);
                let stored_parent = stored.as_ref().and_then(|place| place.get(0)).and_then(Value::as_str).map(str::to_owned);
                let position = match recorder.old_value(SyncKind::Folder, &key, "place")? {
                    Some(old) => old.get(1).and_then(Value::as_str).map(str::to_owned),
                    None if waiting || stored_parent == parent_key => stored_key,
                    None => tree.place.get(&key).filter(|place| place.0 == parent_key).map(|place| place.1.clone()),
                };
                Ok(Some((SyncKind::Folder, key, position)))
            };
            for (kind, key, position) in self.order(&list, (0, list.len()), &mut lookup)? {
                self.positions.insert((kind, key, "place".into()), json!([parent_key, position]));
            }
        }
        Ok(())
    }

    /// Gives new position keys to the members of `list` that are out of
    /// order. Only the members in `window` are compared; the members before
    /// and after it keep their keys and bound the new ones. When no key fits
    /// between the bounds, the window grows. Returns (kind, key, position)
    /// for each member that needs a new key.
    #[allow(clippy::type_complexity)]
    fn order(
        &mut self,
        list: &[u32],
        window: (usize, usize),
        lookup: &mut dyn FnMut(&mut Self, u32) -> Result<Option<(SyncKind, String, Option<String>)>>,
    ) -> Result<Vec<(SyncKind, String, String)>> {
        let (mut start, mut end) = window;
        if start >= end {
            return Ok(Vec::new());
        }
        let mut cache: HashMap<u32, Option<(SyncKind, String, Option<String>)>> = HashMap::new();
        let mut get = |recorder: &mut Self, index: usize| -> Result<Option<(SyncKind, String, Option<String>)>> {
            let id = list[index];
            if let Some(value) = cache.get(&id) {
                return Ok(value.clone());
            }
            let value = lookup(recorder, id)?;
            cache.insert(id, value.clone());
            Ok(value)
        };
        loop {
            let mut members = Vec::with_capacity(end - start);
            let mut kinds = Vec::with_capacity(end - start);
            for index in start..end {
                let (kind, key, position) = get(self, index)?.unwrap_or((SyncKind::Item, String::new(), None));
                kinds.push(kind);
                members.push(Member { key, position });
            }
            let bound = |value: Option<(SyncKind, String, Option<String>)>| {
                value.and_then(|(_, key, position)| position.map(|position| (position, key)))
            };
            let lower = if start > 0 { bound(get(self, start - 1)?) } else { None };
            let upper = if end < list.len() { bound(get(self, end)?) } else { None };
            let whole = start == 0 && end == list.len();
            let assigned = order::assign(
                &members,
                lower.as_ref().map(|(position, key)| (position.as_str(), key.as_str())),
                upper.as_ref().map(|(position, key)| (position.as_str(), key.as_str())),
            );
            let assigned = match assigned {
                Some(assigned) => assigned,
                None if whole => fallback(&members),
                None => {
                    let grow = (end - start).max(1);
                    start = start.saturating_sub(grow);
                    end = (end + grow).min(list.len());
                    continue;
                }
            };
            return Ok(assigned
                .into_iter()
                .filter(|(index, _)| !members[*index].key.is_empty())
                .map(|(index, position)| (kinds[index], members[index].key.clone(), position))
                .collect());
        }
    }

    /// Writes `records` and returns the entities whose visibility, or for
    /// tags whose forwarding, they may have changed.
    fn write(&mut self, mut records: Records, clock: &mut Clock, outbox: bool) -> Result<Vec<(SyncKind, String)>> {
        // Not recorded: the values of skipped entities; a value that is no
        // change of the library's own; and a value that names, as the old or
        // the new value, a waiting or skipped entity that the library does
        // not have yet, or not in the place the value needs, which the
        // library can only show once that entity is written.
        //
        // A value of a waiting entity is no change of the library's own when
        // it is still the one the field had before the records from other
        // computers. A value of a waiting entity, or one that names a
        // waiting entity, is not either when it is the value the synced
        // values store but do not show: the entity was compared because
        // something else about it changed, and what the synced values show
        // differs only because entities that wait to be written are still
        // as they were, such as a cover or a parent folder that is deleted.
        // A tag of a waiting root is recorded only when the root entered or
        // left that tag in the library since the last pass, because a tag
        // that another tag now forwards to can be shown differently for the
        // same reason.
        if !self.skipped.is_empty() || self.waiting.any {
            let mut dropped = Vec::new();
            for ((kind, key, field), value) in &records {
                let entity = (*kind, key.clone());
                let stored = self.synced.raw(*kind, key)?.get(field.as_str()).map(|stored| stored.value.clone());
                let mut names_waiting = false;
                for named in references(*kind, field, value) {
                    names_waiting |= self.waiting.contains(self.transaction, named.0, &named.1)?;
                }
                let waiting = self.waiting.get(self.transaction, *kind, key)?;
                let mut unchanged = false;
                if let Some(tag) = field.strip_prefix("tag:") {
                    if waiting.is_some() {
                        let root = self.local.find(*kind, key)?;
                        unchanged = !root.and_then(|root| self.waiting_tags.get(&root)).is_some_and(|tags| tags.contains(tag));
                    }
                } else if waiting.is_some() || names_waiting {
                    unchanged = stored.as_ref() == Some(value);
                }
                unchanged |= waiting.is_some_and(|old| old.get(field) == Some(value));
                let mut missing = false;
                for named in references(*kind, field, value).into_iter().chain(stored.iter().flat_map(|stored| references(*kind, field, stored))) {
                    if !self.waiting.contains(self.transaction, named.0, &named.1)? && !self.skipped.contains(&named) {
                        continue;
                    }
                    match self.local.find(named.0, &named.1)? {
                        None => missing = true,
                        // An item that the library holds in a collection
                        // cannot be a folder's cover until it is written.
                        Some(id)
                            if *kind == SyncKind::Folder
                                && field == "cover"
                                && named.0 == SyncKind::Item
                                && self.local.snapshot.media_owner.get(id).is_some_and(|owner| owner.0 != id) =>
                        {
                            missing = true
                        }
                        Some(_) => {}
                    }
                }
                if self.skipped.contains(&entity) || unchanged || missing {
                    dropped.push((*kind, key.clone(), field.clone()));
                }
            }
            for record in dropped {
                records.remove(&record);
            }
        }
        let mut watched = BTreeSet::new();
        for ((kind, key, field), value) in &records {
            match field.as_str() {
                "deleted" | "file" | "renamed_to" => {
                    watched.insert((*kind, key.clone()));
                    // A tag group is also shown while a visible tag is in it.
                    if *kind == SyncKind::Tag {
                        let group = key.split_once(':').map_or("", |(group, _)| group);
                        watched.insert((SyncKind::TagGroup, group.to_owned()));
                    }
                }
                "collection" => {
                    // A folder whose cover is the item shows it only while
                    // it is outside a collection.
                    watched.insert((*kind, key.clone()));
                    let raw = self.synced.raw(*kind, key)?;
                    for value in [raw.get(field).map(|stored| &stored.value), Some(value)].into_iter().flatten() {
                        if let Some(collection) = value.get(0).and_then(Value::as_str) {
                            watched.insert((SyncKind::Collection, collection.to_owned()));
                        }
                    }
                }
                _ => {}
            }
        }
        let mut before = Vec::with_capacity(watched.len());
        for (kind, key) in &watched {
            before.push(self.synced.state(*kind, key)?);
        }
        let rows = records.into_iter().map(|((kind, key, field), value)| (kind, key, field, value));
        super::write_local(self.transaction, clock, rows, outbox)?;
        self.synced.forget();
        let mut changed = Vec::new();
        self.appeared_tags.clear();
        for ((kind, key), before) in watched.into_iter().zip(before) {
            let after = self.synced.state(kind, &key)?;
            if after != before {
                // A tag that forwarded to another before showed its values
                // as that tag, so it does not count as appearing.
                if kind == SyncKind::Tag && after.0 && !before.0 && before.1.is_none() {
                    self.appeared_tags.insert(key.clone());
                }
                changed.push((kind, key));
            }
        }
        Ok(changed)
    }

    /// The second comparison: every entity that refers to a key whose
    /// visibility changed is compared again with the new synced values.
    /// When a folder moved or appeared, every folder is compared again: the
    /// parent that the synced values show for a folder in a cycle depends on
    /// the `place` of the other folders in it.
    fn second_pass(&mut self, changed: Vec<(SyncKind, String)>, moved: bool) -> Result<()> {
        // The entity itself is compared again too: its full state can hold
        // `false` for a tag key that forwards to a tag it has.
        let mut referring = changed.iter().cloned().collect::<BTreeSet<_>>();
        if moved {
            let mut statement = self.transaction.prepare_cached("SELECT stable_key FROM folder_definition")?;
            for key in statement.query_map([], |row| row.get::<_, String>(0))? {
                referring.insert((SyncKind::Folder, key?));
            }
        }
        let mut keys = changed;
        let mut seen = keys.iter().cloned().collect::<BTreeSet<_>>();
        while let Some((kind, key)) = keys.pop() {
            // A tag that appears was shown on no entity before this write,
            // so a `tag:` value that this write recorded for it holds what
            // the library has, and as the latest record it decides what is
            // shown. Its entity is not compared again for it.
            let until = (kind == SyncKind::Tag && self.appeared_tags.contains(&key))
                .then_some(self.written_after)
                .flatten();
            for (referrer_kind, referrer, field) in self.synced.referrers(kind, &key, until)? {
                // A tag renamed into this one forwards to it, so what refers
                // to that tag is compared too.
                if referrer_kind == SyncKind::Tag && field == "renamed_to" {
                    if seen.insert((referrer_kind, referrer.clone())) {
                        keys.push((referrer_kind, referrer));
                    }
                    continue;
                }
                referring.insert((referrer_kind, referrer));
            }
            if kind == SyncKind::Tag {
                for folder in self.synced.folder_keys()? {
                    let raw = self.synced.raw(SyncKind::Folder, &folder)?;
                    let names = raw
                        .get("auto_tags")
                        .and_then(|stored| stored.value.as_array())
                        .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some(key.as_str())));
                    if names {
                        referring.insert((SyncKind::Folder, folder));
                    }
                }
            }
        }
        let mut roots = RoaringBitmap::new();
        let mut found = Vec::new();
        for (kind, key) in referring {
            if self.skipped.contains(&(kind, key.clone())) {
                continue;
            }
            if let Some(local_id) = self.local.find(kind, &key)? {
                if matches!(kind, SyncKind::Item | SyncKind::Collection) {
                    roots.insert(local_id);
                }
                found.push((kind, key, local_id));
            }
        }
        self.local.prepare(&roots)?;
        if moved {
            self.folder_tree()?;
        }
        for (kind, key, local_id) in found {
            let view = self.local.view(kind, local_id)?;
            self.diff(kind, &key, view.as_ref())?;
        }
        let positions = std::mem::take(&mut self.positions);
        for (record, value) in positions {
            self.records.entry(record).or_insert(value);
        }
        Ok(())
    }
}

/// The entities that a value of the field `field` of a `kind` entity names.
fn references(kind: SyncKind, field: &str, value: &Value) -> Vec<(SyncKind, String)> {
    let key = |kind: SyncKind, value: Option<&Value>| value.and_then(Value::as_str).map(|key| (kind, key.to_owned()));
    if let Some(tag) = field.strip_prefix("tag:") {
        return vec![(SyncKind::Tag, tag.to_owned())];
    }
    if let Some(folder) = field.strip_prefix("folder:") {
        return vec![(SyncKind::Folder, folder.to_owned())];
    }
    match (kind, field) {
        (SyncKind::Item, "collection") => key(SyncKind::Collection, value.get(0)).into_iter().collect(),
        (SyncKind::Folder, "place") => key(SyncKind::Folder, value.get(0)).into_iter().collect(),
        (SyncKind::Tag, "renamed_to") => key(SyncKind::Tag, Some(value)).into_iter().collect(),
        (SyncKind::Collection, "cover") => key(SyncKind::Item, Some(value)).into_iter().collect(),
        (SyncKind::Folder, "cover") => value
            .get(0)
            .and_then(Value::as_str)
            .and_then(SyncKind::parse)
            .and_then(|kind| key(kind, value.get(1)))
            .into_iter()
            .collect(),
        (SyncKind::Folder, "auto_tags") => {
            value.as_array().into_iter().flatten().filter_map(|tag| key(SyncKind::Tag, Some(tag))).collect()
        }
        _ => Vec::new(),
    }
}

/// The part of `after` that differs from `before`: everything between their
/// common start and their common end.
fn changed_window(before: &[u32], after: &[u32]) -> (usize, usize) {
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let limit = before.len().min(after.len()) - prefix;
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(limit)
        .take_while(|(a, b)| a == b)
        .count();
    (prefix, after.len() - suffix)
}

/// Keys for a whole list when the members that keep their keys leave no
/// room: each member that is not in order gets a key after the one before
/// it.
fn fallback(members: &[Member]) -> Vec<(usize, String)> {
    let mut previous: Option<String> = None;
    let mut assigned = Vec::new();
    for (index, member) in members.iter().enumerate() {
        let in_order = match (&member.position, &previous) {
            (Some(position), Some(previous)) => position > previous,
            (Some(_), None) => true,
            _ => false,
        };
        if in_order {
            previous = member.position.clone();
        } else {
            let key = order::key_after(previous.as_deref());
            assigned.push((index, key.clone()));
            previous = Some(key);
        }
    }
    assigned
}

fn ids_json(ids: &RoaringBitmap) -> Result<String> {
    Ok(serde_json::to_string(&ids.iter().collect::<Vec<_>>())?)
}

fn folders_with_auto_tag(connection: &Connection, tag_id: u32) -> Result<Vec<u32>> {
    let mut statement = connection
        .prepare_cached("SELECT folder_id, auto_tag_ids FROM folder_definition WHERE length(auto_tag_ids) > 0")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
    let mut folders = Vec::new();
    for row in rows {
        let (folder, payload) = row?;
        if RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(payload))?.contains(tag_id) {
            folders.push(folder);
        }
    }
    Ok(folders)
}
