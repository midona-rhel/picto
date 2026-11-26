//! Recording: after a write, the entities that the write touched are
//! compared with what the synced values show, and every field that differs
//! is recorded with a new timestamp, in the same transaction as the write.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};

use super::local::{lifecycle_name, Local};
use super::order::{self, Member};
use super::values::{Synced, View, ROOT_FIELDS};
use super::{recording, Clock, SyncKind};
use crate::model::{FolderId, Lifecycle, Rating, RootId, TagId};
use crate::projection::{ProjectionSnapshot, SharedBitmap};
use crate::Result;

/// A second comparison runs at most this many times. Each one compares the
/// entities that refer to a key whose visibility the records before it
/// changed; in practice the second one finds nothing.
const SECOND_PASSES: usize = 8;

type Records = BTreeMap<(SyncKind, String, String), Value>;

/// What a write touched: local entities that are compared one by one, the
/// lists whose order is compared, and the roots that entered or left a
/// lifecycle, rating, tag or folder. A list marked `true` is compared as a
/// whole; otherwise only the part that differs from the list before the
/// write.
#[derive(Default)]
struct Touched {
    ids: BTreeSet<u32>,
    folder_lists: BTreeMap<FolderId, bool>,
    collection_lists: BTreeMap<RootId, bool>,
    folder_tree: bool,
    changes: Vec<Change>,
}

/// The roots that entered or left one lifecycle, rating, tag or folder.
struct Change {
    membership: Membership,
    added: RoaringBitmap,
    removed: RoaringBitmap,
}

#[derive(Clone, Copy)]
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

/// Records the changes of the write that turned `before` into `after`.
pub(crate) fn record(
    transaction: &Transaction<'_>,
    before: &ProjectionSnapshot,
    after: &ProjectionSnapshot,
) -> Result<()> {
    if !recording(transaction)? {
        return Ok(());
    }
    let mut touched = touched(transaction, before, after)?;
    if touched.ids.is_empty()
        && touched.folder_lists.is_empty()
        && touched.collection_lists.is_empty()
        && touched.changes.is_empty()
    {
        return Ok(());
    }
    let mut clock = Clock::load(transaction)?;
    let mut recorder = Recorder::new(transaction, before, after)?;
    recorder.merges = transaction
        .prepare_cached("SELECT source, destination FROM temp.sync_tag_merge")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    transaction.execute("DELETE FROM temp.sync_tag_merge", [])?;
    let sets = recorder.split_sets(&mut touched)?;
    recorder.run(touched, &sets, &mut clock)?;
    clock.save(transaction)
}

/// How many entities turning sync on compares at a time, so that the
/// records of a large library are not all held in memory at once.
const FULL_STATE_CHUNK: usize = 20_000;

/// Records the full state of the library into empty synced values when sync
/// is turned on. The records go into the synced values only, not into the
/// outbox. The position keys of every list are written first; the entities
/// follow in chunks and take their positions from the synced values. With
/// empty synced values nothing forwards and nothing is hidden, so no second
/// comparison is needed.
pub(crate) fn record_full(transaction: &Transaction<'_>, snapshot: &ProjectionSnapshot) -> Result<()> {
    let mut clock = Clock::load(transaction)?;
    let mut touched = everything(transaction, snapshot)?;
    let ids = std::mem::take(&mut touched.ids).into_iter().collect::<Vec<_>>();
    let mut recorder = Recorder::new(transaction, snapshot, snapshot)?;
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
        let mut recorder = Recorder::new(transaction, snapshot, snapshot)?;
        recorder.written_positions = true;
        let entities = recorder.compare(Touched {
            ids: chunk.iter().copied().collect(),
            ..Touched::default()
        })?;
        let records = std::mem::take(&mut recorder.records);
        recorder.write(records, &mut clock, false)?;
        recorder.update_identities(&entities)?;
    }
    clock.save(transaction)
}

/// The records that a full comparison of the library with the synced values
/// would make, without making them. After any write it must be empty.
#[cfg(test)]
pub(crate) fn differences(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
) -> Result<Vec<(SyncKind, String, String, Value)>> {
    let touched = everything(transaction, snapshot)?;
    let mut recorder = Recorder::new(transaction, snapshot, snapshot)?;
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

/// Finds what a write touched: the ids that the triggers noted, and the
/// differences between the projections before and after the write.
fn touched(
    transaction: &Transaction<'_>,
    before: &ProjectionSnapshot,
    after: &ProjectionSnapshot,
) -> Result<Touched> {
    let mut touched = Touched::default();
    {
        let mut statement = transaction.prepare_cached("SELECT local_id FROM temp.sync_touched")?;
        for id in statement.query_map([], |row| row.get::<_, u32>(0))? {
            touched.ids.insert(id?);
        }
    }
    transaction.execute("DELETE FROM temp.sync_touched", [])?;
    let mut change = |membership, before: Option<&SharedBitmap>, after: Option<&SharedBitmap>| {
        if let (Some(before), Some(after)) = (before, after) {
            if before.allocation_id() == after.allocation_id() {
                return;
            }
        }
        let empty = RoaringBitmap::new();
        let before: &RoaringBitmap = before.map_or(&empty, |value| value);
        let after: &RoaringBitmap = after.map_or(&empty, |value| value);
        if before != after {
            touched.changes.push(Change { membership, added: after - before, removed: before - after });
        }
    };
    if !Arc::ptr_eq(&before.lifecycle, &after.lifecycle) {
        for (key, value) in after.lifecycle.iter() {
            change(Membership::Lifecycle(*key), before.lifecycle.get(key), Some(value));
        }
    }
    if !Arc::ptr_eq(&before.ratings, &after.ratings) {
        for (key, value) in after.ratings.iter() {
            change(Membership::Rating(*key), before.ratings.get(key), Some(value));
        }
    }
    if !Arc::ptr_eq(&before.tags, &after.tags) {
        for (key, value) in after.tags.iter() {
            change(Membership::Tag(*key), before.tags.get(key), Some(value));
        }
        for (key, value) in before.tags.iter() {
            if !after.tags.contains_key(key) {
                change(Membership::Tag(*key), Some(value), None);
            }
        }
    }
    if !Arc::ptr_eq(&before.folder_orders, &after.folder_orders) {
        for (folder, order) in after.folder_orders.iter() {
            let previous = before.folder_orders.get(folder);
            if previous.is_none_or(|previous| !Arc::ptr_eq(previous, order)) {
                touched.folder_lists.insert(*folder, false);
                let old = previous
                    .map(|order| order.iter().map(|root| root.0).collect::<RoaringBitmap>())
                    .unwrap_or_default();
                let new = order.iter().map(|root| root.0).collect::<RoaringBitmap>();
                if old != new {
                    touched.changes.push(Change {
                        membership: Membership::Folder(*folder),
                        added: &new - &old,
                        removed: &old - &new,
                    });
                }
            }
        }
        for (folder, order) in before.folder_orders.iter() {
            if !after.folder_orders.contains_key(folder) {
                touched.ids.extend(order.iter().map(|root| root.0));
                touched.folder_tree = true;
            }
        }
    }
    if !Arc::ptr_eq(&before.collection_orders, &after.collection_orders) {
        for (collection, order) in after.collection_orders.iter() {
            let previous = before.collection_orders.get(collection);
            if previous.is_none_or(|previous| !Arc::ptr_eq(previous, order)) {
                touched.collection_lists.insert(*collection, false);
                touched.ids.insert(collection.0);
                let old = previous.map(|order| order.iter().map(|media| media.0).collect::<RoaringBitmap>());
                let new = order.iter().map(|media| media.0).collect::<RoaringBitmap>();
                touched.ids.extend((&old.unwrap_or_default() ^ &new).iter());
            }
        }
        for (collection, order) in before.collection_orders.iter() {
            if !after.collection_orders.contains_key(collection) {
                touched.ids.insert(collection.0);
                touched.ids.extend(order.iter().map(|media| media.0));
            }
        }
    }
    if !Arc::ptr_eq(&before.root_kinds, &after.root_kinds) {
        for (kind, value) in after.root_kinds.iter() {
            let empty = SharedBitmap::from(RoaringBitmap::new());
            let previous = before.root_kinds.get(kind).unwrap_or(&empty);
            if previous.allocation_id() != value.allocation_id() {
                touched.ids.extend((previous ^ value).iter());
            }
        }
    }
    Ok(touched)
}

/// One local entity that a write touched: the key the synced values knew it
/// by, and the key the library gives it now (`None` when it is gone).
struct Entity {
    kind: SyncKind,
    local_id: u32,
    old: Option<String>,
    new: Option<String>,
}

struct Recorder<'a> {
    transaction: &'a Transaction<'a>,
    before: &'a ProjectionSnapshot,
    local: Local<'a>,
    synced: Synced<'a>,
    /// Tags merged in this write: source name to destination name.
    merges: HashMap<String, String>,
    records: Records,
    /// Position keys that the comparison of lists gave, for members whose
    /// key changed.
    positions: Records,
    /// Whether the positions of every list are already in the synced
    /// values, as when sync is turned on; a full state then leaves them out.
    written_positions: bool,
    /// The clock before the first record of this write.
    written_after: Option<(i64, i64)>,
    /// The tags that the last records written made visible.
    appeared_tags: BTreeSet<String>,
}

impl<'a> Recorder<'a> {
    fn new(
        transaction: &'a Transaction<'a>,
        before: &'a ProjectionSnapshot,
        after: &'a ProjectionSnapshot,
    ) -> Result<Self> {
        Ok(Self {
            transaction,
            before,
            local: Local::new(transaction, after)?,
            synced: Synced::new(transaction),
            merges: HashMap::new(),
            records: Records::new(),
            positions: Records::new(),
            written_positions: false,
            written_after: None,
            appeared_tags: BTreeSet::new(),
        })
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

    /// Compares the touched entities and lists and fills `records`. Returns
    /// the entities, so that their keys can be stored.
    fn compare(&mut self, mut touched: Touched) -> Result<Vec<Entity>> {
        let entities = self.entities(&mut touched)?;
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

    /// The touched local ids as entities with their old and new keys. A key
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
    /// those that the write changed only through memberships. The others are
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
                Membership::Tag(tag) => match self.plain_tag(tag)? {
                    Some((target, shown)) => {
                        // A tag that the synced values do not show is not
                        // shown under its own name on any root, so every
                        // root that gets it records it.
                        sets.push(SetRecord { compare: shown, ..set(change.added, "tag", target, "true".into(), "false") });
                        if shown {
                            sets.push(set(change.removed, "tag", target, "false".into(), "false"));
                        }
                    }
                    None => {
                        single |= &change.added;
                        single |= &change.removed;
                    }
                },
                // A root added to a folder gets its position from the
                // comparison of the folder's list.
                Membership::Folder(folder) => {
                    if let Some(target) = self.shown_folder(folder)? {
                        sets.push(set(change.removed, "folder", target, "null".into(), "null"));
                    }
                }
            }
        }
        single -= &ids;
        for set in &mut sets {
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
    fn update_identities(&self, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            if entity.old == entity.new {
                continue;
            }
            self.transaction
                .execute("UPDATE sync_identity SET local_id = NULL WHERE local_id = ?1", [entity.local_id])?;
            if let Some(key) = &entity.new {
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
                    self.put(kind, old, "renamed_to", json!(target));
                }
            }
        }
        if let Some(key) = &entity.new {
            self.diff(kind, key, view)?;
        }
        Ok(())
    }

    /// Compares one entity of the library with the synced values.
    fn diff(&mut self, kind: SyncKind, key: &str, view: Option<&View>) -> Result<()> {
        let shown = self.synced.view(kind, key)?;
        match (view, shown) {
            (None, Some(_)) => {
                self.put(kind, key, "deleted", json!(true));
                if kind == SyncKind::Tag {
                    if let Some(target) = self.merges.get(key).cloned() {
                        self.put(kind, key, "renamed_to", json!(target));
                    }
                }
            }
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
                    }),
                    None => Value::Null,
                };
                fields.insert("collection".into(), collection);
            }
            SyncKind::Folder => {
                let place = self.position(kind, key, "place", Some(json!(view.owner)), || json!([view.owner, ""]));
                fields.insert("place".into(), place);
            }
            _ => {}
        }
        for tag in &view.tags {
            fields.insert(format!("tag:{tag}"), json!(true));
        }
        for folder in &view.folders {
            let field = format!("folder:{folder}");
            let position = self.position(kind, key, &field, None, || json!(""));
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
    fn position(&mut self, kind: SyncKind, key: &str, field: &str, list: Option<Value>, fallback: impl FnOnce() -> Value) -> Value {
        if let Some(value) = self.positions.remove(&(kind, key.to_owned(), field.to_owned())) {
            return value;
        }
        self.synced
            .raw(kind, key)
            .ok()
            .and_then(|raw| raw.get(field).map(|stored| stored.value.clone()))
            .filter(|value| !value.is_null())
            .filter(|value| list.as_ref().is_none_or(|list| value.get(0) == Some(list)))
            .unwrap_or_else(fallback)
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
                            self.position(kind, key, "collection", Some(json!(collection)), || json!([collection, ""]))
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
                    let value = self.position(kind, key, "place", Some(json!(view.owner)), || json!([view.owner, ""]));
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

    /// Compares the item list of a folder with the position keys of its
    /// members. A member that left the folder is touched itself, and the
    /// comparison of its fields (`root_changes`) records `null` for it.
    fn folder_list(&mut self, folder: FolderId, whole: bool) -> Result<()> {
        let Some(folder_key) = self.local.folder_key(folder.0)? else {
            return Ok(());
        };
        let field = format!("folder:{folder_key}");
        let empty = Arc::new(Vec::new());
        let after = self.local.snapshot.folder_orders.get(&folder).cloned().unwrap_or_else(|| empty.clone());
        let before = self.before.folder_orders.get(&folder).cloned().unwrap_or(empty);
        let after_ids = after.iter().map(|root| root.0).collect::<Vec<_>>();
        let before_ids = before.iter().map(|root| root.0).collect::<Vec<_>>();
        let window = if whole { (0, after_ids.len()) } else { changed_window(&before_ids, &after_ids) };
        let mut lookup = |recorder: &mut Self, root: u32| -> Result<Option<(SyncKind, String, Option<String>)>> {
            let Some((kind, key)) = recorder.local.root_key(RootId(root))? else { return Ok(None) };
            let position = recorder
                .synced
                .raw(kind, &key)?
                .get(&field)
                .and_then(|stored| stored.value.as_str().map(str::to_owned));
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
        let empty = Arc::new(Vec::new());
        let after = self.local.snapshot.collection_orders.get(&collection).cloned().unwrap_or_else(|| empty.clone());
        let before = self.before.collection_orders.get(&collection).cloned().unwrap_or(empty);
        let after_ids = after.iter().map(|media| media.0).collect::<Vec<_>>();
        let before_ids = before.iter().map(|media| media.0).collect::<Vec<_>>();
        let window = if whole { (0, after_ids.len()) } else { changed_window(&before_ids, &after_ids) };
        let mut lookup = |recorder: &mut Self, media: u32| -> Result<Option<(SyncKind, String, Option<String>)>> {
            let Some(key) = recorder.local.key(SyncKind::Item, media)? else { return Ok(None) };
            let raw = recorder.synced.raw(SyncKind::Item, &key)?;
            let position = raw
                .get("collection")
                .map(|stored| &stored.value)
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
            let parent_key = match parent {
                Some(parent) => self.local.folder_key(parent)?,
                None => None,
            };
            let mut lookup = |recorder: &mut Self, folder: u32| -> Result<Option<(SyncKind, String, Option<String>)>> {
                let Some(key) = recorder.local.folder_key(folder)? else { return Ok(None) };
                let position = tree
                    .place
                    .get(&key)
                    .filter(|place| place.0 == parent_key)
                    .map(|place| place.1.clone());
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
    fn write(&mut self, records: Records, clock: &mut Clock, outbox: bool) -> Result<Vec<(SyncKind, String)>> {
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
