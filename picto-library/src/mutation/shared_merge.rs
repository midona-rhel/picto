//! Merging the entries of other computers into this library.
//!
//! A merge has two steps. `Library::store_entries` keeps each entry that is
//! later than the one stored for its key, and notes in `merge_touched` which
//! files, tags and folders those entries are about. `Library::apply_merged`
//! then works out each noted picture from all its entries (`shared_view.rs`)
//! and changes the library to match, through the same functions that Undo and
//! Redo use, in one published write without an undo step. A merge writes no
//! entry of its own and only moves the clock up to the newest entry it saw.
//!
//! What happens to a picture depends on what its entries say and on what the
//! library holds:
//!
//! - It exists and the library has it outside a collection: its name, notes,
//!   rating, status, tags and folders are set to what the entries say. A
//!   missing tag is created. A folder this library doesn't have is skipped,
//!   because folder definitions don't sync yet.
//! - It exists and the library doesn't have the file: the file is wanted
//!   (`Library::wanted_files`), and the next step of a sync fetches it. When
//!   this computer still has the file because a merge deleted the picture
//!   less than "Keep deleted files" ago, the picture is imported again from
//!   that file instead.
//! - It doesn't exist and the library has it outside a collection: it's
//!   deleted, as a deletion that came from another computer. Its file stays
//!   on this computer for "Keep deleted files" (`kept_deleted_file`).
//! - The library has it inside a collection: nothing changes, because
//!   collections don't sync yet.
//!
//! A deleted tag is checked on every picture that has it here, and the tag
//! itself goes once no picture has it. A deleted folder is removed once every
//! folder below it is deleted too; until then it's only emptied.
//!
//! Before storing anything, all of this computer's pending changes become
//! entries (`settle_in`), so that a merge never works from an incomplete
//! picture of what this computer did.
//!
//! An entry of a file at or before the file's floor (its `compacted` entry)
//! isn't stored, and a stored floor drops the entries at or before it, as
//! `compact_in` does on the computer that wrote it.

use std::collections::{BTreeMap, BTreeSet};

use super::shared_view::{
    file_view, parse_time_text, EntryTime, FileRecord, FileView, HeldEntry, UndoRecord,
};
use super::*;

/// How many roots become entries in one transaction before a merge, and how
/// many pictures one write of `Library::merge_entries` changes.
const BATCH: usize = 5_000;

/// What one call of `Library::apply_merged` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppliedMerge {
    /// Whether pictures, tags or folders are left for another call.
    pub more: bool,
    /// How many pictures were changed to match their entries.
    pub changed: u64,
    /// How many pictures were deleted.
    pub deleted: u64,
    /// `None` when the call changed nothing in the library.
    pub receipt: Option<MutationReceipt>,
}

/// Reads all stored entries of the file `content_hash`.
pub(super) fn held_entries(
    connection: &rusqlite::Connection,
    content_hash: &str,
) -> Result<Vec<HeldEntry>> {
    let mut statement = connection.prepare_cached(
        "SELECT kind, second_key, value, clock_ms, clock_counter, device_id
         FROM shared_entry WHERE subject_key = ?1",
    )?;
    let mut rows = statement.query([content_hash])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        let second_key = row.get::<_, String>(1)?;
        let value = row.get::<_, Option<String>>(2)?.unwrap_or_default();
        let time: EntryTime = (row.get(3)?, row.get(4)?, row.get(5)?);
        let kind: String = row.get(0)?;
        let entry = match EntryKind::parse(&kind) {
            Some(EntryKind::Add) => HeldEntry::Add { time, record: FileRecord::parse(&value) },
            Some(EntryKind::Delete) => {
                HeldEntry::Delete { time, undo: UndoRecord::parse(&value) }
            }
            Some(EntryKind::Compacted) => HeldEntry::Compacted { through: parse_time_text(&value) },
            Some(EntryKind::Name) => HeldEntry::Name { time, value },
            Some(EntryKind::Notes) => HeldEntry::Notes { time, value },
            Some(EntryKind::Rating) => HeldEntry::Rating {
                time,
                value: value.parse::<u8>().unwrap_or(0).min(5),
            },
            Some(EntryKind::Status) => HeldEntry::Status {
                time,
                value: Lifecycle::ALL
                    .into_iter()
                    .find(|lifecycle| lifecycle_name(*lifecycle) == value)
                    .unwrap_or(Lifecycle::Active),
            },
            Some(EntryKind::ItemTag) => HeldEntry::ItemTag { time, tag: second_key, on: value == "add" },
            Some(EntryKind::FolderItem) => {
                HeldEntry::FolderItem { time, folder: second_key, on: value == "add" }
            }
            _ => continue,
        };
        entries.push(entry);
    }
    Ok(entries)
}

/// The `tag_delete` and `folder_delete` entries that a batch has looked up,
/// so each is read only once.
#[derive(Default)]
pub(super) struct DeletedDefinitions {
    tags: HashMap<String, Option<EntryTime>>,
    folders: HashMap<String, bool>,
}

impl DeletedDefinitions {
    fn tag(&mut self, connection: &rusqlite::Connection, name: &str) -> Result<Option<EntryTime>> {
        use rusqlite::OptionalExtension;
        if let Some(held) = self.tags.get(name) {
            return Ok(held.clone());
        }
        let held = connection
            .prepare_cached(
                "SELECT clock_ms, clock_counter, device_id FROM shared_entry
                 WHERE subject_key = ?1 AND kind = 'tag_delete' AND second_key = ''",
            )?
            .query_row([name], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .optional()?;
        self.tags.insert(name.to_owned(), held.clone());
        Ok(held)
    }

    fn folder(&mut self, connection: &rusqlite::Connection, key: &str) -> Result<bool> {
        if let Some(held) = self.folders.get(key) {
            return Ok(*held);
        }
        let held = connection
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM shared_entry
                 WHERE subject_key = ?1 AND kind = 'folder_delete' AND second_key = '')",
            )?
            .query_row([key], |row| row.get(0))?;
        self.folders.insert(key.to_owned(), held);
        Ok(held)
    }
}

/// Returns the picture that the stored entries of `content_hash` describe,
/// or `None` when it doesn't exist.
pub(super) fn view_of(
    connection: &rusqlite::Connection,
    deleted: &mut DeletedDefinitions,
    content_hash: &str,
) -> Result<Option<FileView>> {
    let entries = held_entries(connection, content_hash)?;
    // The view function takes lookups that cannot fail, so the tags and
    // folders that the entries name are looked up first.
    for entry in &entries {
        match entry {
            HeldEntry::Add { record, .. }
            | HeldEntry::Delete { undo: Some(UndoRecord { record, .. }), .. } => {
                for tag in &record.tags {
                    deleted.tag(connection, tag)?;
                }
                for folder in &record.folders {
                    deleted.folder(connection, folder)?;
                }
            }
            HeldEntry::ItemTag { tag, .. } => {
                deleted.tag(connection, tag)?;
            }
            HeldEntry::FolderItem { folder, .. } => {
                deleted.folder(connection, folder)?;
            }
            _ => {}
        }
    }
    let DeletedDefinitions { tags, folders } = deleted;
    Ok(file_view(
        &entries,
        &mut |tag| tags.get(tag).cloned().flatten(),
        &mut |folder| folders.get(folder).copied().unwrap_or(false),
    ))
}

/// Notes the file of each of `roots` that isn't in a collection in
/// `merge_touched`, so the next apply checks it.
fn touch_files_of_roots(
    transaction: &rusqlite::Transaction<'_>,
    roots: &RoaringBitmap,
) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    transaction.execute(
        "INSERT OR IGNORE INTO merge_touched (subject_kind, subject_key)
         SELECT 'file', file.content_hash
         FROM json_each(?1) ids
         CROSS JOIN library_root root ON root.root_id = ids.value
         JOIN media_item media ON media.media_id = root.root_id
         JOIN media_file file ON file.file_id = media.file_id",
        [serde_json::to_string(&roots.iter().collect::<Vec<_>>())?],
    )?;
    Ok(())
}

/// Stores those of `entries` that are later than the stored ones, moves the
/// clock up to the latest of them, and notes what they're about in
/// `merge_touched`. Returns how many were stored.
///
/// An entry of a file at or before the file's floor isn't stored, because
/// it never counts again; a computer that was away for weeks sends such
/// entries. A stored `compacted` entry drops the entries at or before its
/// floor, as on the computer that wrote it.
fn store_in(transaction: &rusqlite::Transaction<'_>, entries: &[SharedEntry]) -> Result<u64> {
    let mut stored = 0;
    let mut latest = None::<(i64, i64)>;
    let mut floors = BTreeMap::<String, String>::new();
    for chunk in entries.chunks(BATCH) {
        let rows = chunk
            .iter()
            .map(|entry| {
                serde_json::json!([
                    entry.subject_key,
                    entry.kind.as_str(),
                    entry.second_key,
                    entry.value,
                    entry.clock_ms,
                    entry.clock_counter,
                    entry.device_id
                ])
            })
            .collect::<Vec<_>>();
        // The statement returns a row for every entry that it stored.
        let mut statement = transaction.prepare_cached(&format!(
            "INSERT INTO shared_entry
                 (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
             SELECT json_extract(row.value, '$[0]'), json_extract(row.value, '$[1]'),
                    json_extract(row.value, '$[2]'), json_extract(row.value, '$[3]'),
                    json_extract(row.value, '$[4]'), json_extract(row.value, '$[5]'),
                    json_extract(row.value, '$[6]')
             FROM json_each(?1) row
             WHERE NOT (json_extract(row.value, '$[1]') IN {DROPPED_KINDS}
                        AND EXISTS (
                            SELECT 1 FROM shared_entry floor
                            WHERE floor.subject_key = json_extract(row.value, '$[0]')
                              AND floor.kind = 'compacted' AND floor.second_key = ''
                              AND floor.value >= printf('%016d:%010d:%s',
                                  json_extract(row.value, '$[4]'),
                                  json_extract(row.value, '$[5]'),
                                  json_extract(row.value, '$[6]'))))
             {KEEP_LATER}
             RETURNING subject_key, kind, clock_ms, clock_counter, value"
        ))?;
        let mut won = statement.query([serde_json::to_string(&rows)?])?;
        let mut touched = BTreeSet::new();
        while let Some(row) = won.next()? {
            let subject_key = row.get::<_, String>(0)?;
            let kind: String = row.get(1)?;
            let kind = match EntryKind::parse(&kind) {
                Some(EntryKind::TagDelete) => "tag",
                Some(EntryKind::FolderDelete) => "folder",
                Some(EntryKind::Compacted) => {
                    let through = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                    floors.insert(subject_key.clone(), through);
                    "file"
                }
                _ => "file",
            };
            latest = latest.max(Some((row.get(2)?, row.get(3)?)));
            touched.insert((kind, subject_key));
            stored += 1;
        }
        drop(won);
        if !touched.is_empty() {
            transaction.execute(
                "INSERT OR IGNORE INTO merge_touched (subject_kind, subject_key)
                 SELECT json_extract(row.value, '$[0]'), json_extract(row.value, '$[1]')
                 FROM json_each(?1) row",
                [serde_json::to_string(&touched)?],
            )?;
        }
    }
    for (content_hash, through) in floors {
        drop_below(transaction, &content_hash, &through)?;
    }
    if let Some(latest) = latest {
        observe(transaction, latest)?;
    }
    Ok(stored)
}

/// The state of a batch of files in [`apply_files`].
struct Applied {
    changed: u64,
    deleted: u64,
    /// Files that the entries delete and that were kept for a later round.
    kept: Vec<String>,
}

/// Changes the library to match the entries of the files `hashes`. The top
/// of this file says what happens to each. `tags_to_check` gets the tags that
/// lost a picture. With `keep_held_files`, a picture that the entries delete
/// is kept and returned in `kept`, because a file of entries that couldn't
/// be read yet may add it again.
#[allow(clippy::too_many_arguments)]
fn apply_files(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    next: &mut ProjectionSnapshot,
    hashes: &[String],
    keep_held_files: bool,
    protected: &BTreeSet<crate::FileId>,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
    tags_to_check: &mut BTreeSet<crate::TagId>,
) -> Result<Applied> {
    use rusqlite::OptionalExtension;
    let mut deleted_definitions = DeletedDefinitions::default();
    let mut wanted = Vec::new();
    let mut not_wanted = Vec::new();
    let mut to_delete = RoaringBitmap::new();
    let mut kept_files = Vec::new();
    let mut views = BTreeMap::<u32, FileView>::new();
    for hash in hashes {
        let view = view_of(transaction, &mut deleted_definitions, hash)?;
        // The media item of the file, and whether it is an item of its own.
        let held = transaction
            .prepare_cached(
                "SELECT media.media_id,
                        EXISTS (SELECT 1 FROM library_root root
                                WHERE root.root_id = media.media_id)
                 FROM media_file file
                 JOIN media_item media ON media.file_id = file.file_id
                 WHERE file.content_hash = ?1",
            )?
            .query_row([hash], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, bool>(1)?)))
            .optional()?;
        match (view, held) {
            (Some(_), None) => wanted.push(hash),
            (None, None) => not_wanted.push(hash),
            (Some(view), Some((root, true))) => {
                not_wanted.push(hash);
                views.insert(root, view);
            }
            (None, Some((_, true))) if keep_held_files => kept_files.push(hash.clone()),
            (None, Some((root, true))) => {
                not_wanted.push(hash);
                to_delete.insert(root);
            }
            // A member of a collection is left as it is.
            (_, Some((_, false))) => not_wanted.push(hash),
        }
    }
    transaction.execute(
        "INSERT OR IGNORE INTO wanted_file (content_hash) SELECT value FROM json_each(?1)",
        [serde_json::to_string(&wanted)?],
    )?;
    transaction.execute(
        "DELETE FROM wanted_file WHERE content_hash IN (SELECT value FROM json_each(?1))",
        [serde_json::to_string(&not_wanted)?],
    )?;

    let roots = views.keys().copied().collect::<RoaringBitmap>();
    let mut changes = Vec::new();
    let mut changed = RoaringBitmap::new();

    // Names and notes.
    let mut kept = Vec::new();
    let mut texts = Vec::new();
    for state in load_root_text(transaction, &roots)? {
        let view = &views[&state.root_id.0];
        let notes = (!view.notes.is_empty()).then(|| view.notes.clone());
        let same_notes = state.notes.clone().filter(|held| !held.is_empty()) == notes;
        if state.name == view.name && same_notes {
            continue;
        }
        let mut after = state.clone();
        if !view.name.trim().is_empty() {
            after.name = view.name.clone();
        }
        if !same_notes {
            after.notes = notes;
        }
        if after == state {
            continue;
        }
        after.modified_at_ms = now_ms();
        kept.push(state);
        texts.push(after);
    }
    if !texts.is_empty() {
        changes.push(SemanticChange::RootText {
            before: Arc::new(kept),
            after: Arc::new(texts),
        });
    }

    // Tags: for each tag, the roots that get it and the roots that lose it.
    let mut wanted_tags = BTreeMap::<&str, RoaringBitmap>::new();
    for (root, view) in &views {
        for tag in &view.tags {
            wanted_tags.entry(tag).or_default().insert(*root);
        }
    }
    let mut tag_changes = BTreeMap::<crate::TagId, (RoaringBitmap, RoaringBitmap)>::new();
    for (tag_id, members) in next.tags.iter() {
        if !members.is_disjoint(&roots) {
            tag_changes.entry(*tag_id).or_default().1 = &**members & &roots;
        }
    }
    for (name, members) in wanted_tags {
        let tag_id = match next.tag_ids_by_name.get(name).copied() {
            Some(tag_id) => tag_id,
            None => {
                let Ok(canonical) = ingest::canonical_tag_name(name) else {
                    continue;
                };
                let tag_id = ingest::ensure_tag(transaction, &canonical)?;
                Arc::make_mut(&mut next.tag_ids_by_name).insert(canonical, tag_id);
                resources.insert("tags".into());
                tag_id
            }
        };
        let change = tag_changes.entry(tag_id).or_default();
        // `change.1` holds the roots that have the tag now. Those that the
        // view gives the tag keep it, and the others lose it.
        change.0 = &members - &change.1;
        change.1 -= &members;
    }
    for (tag_id, (added, removed)) in tag_changes {
        if added.is_empty() && removed.is_empty() {
            continue;
        }
        if !removed.is_empty() {
            tags_to_check.insert(tag_id);
        }
        let key = BitmapKey { domain: BitmapDomain::Tag, key_id: tag_id.0 };
        let before = projection_bitmap(next, key);
        let mut after = &before | &added;
        after -= &removed;
        changes.push(SemanticChange::Bitmap {
            key,
            before: Arc::new(before),
            after: Arc::new(after),
        });
        resources.extend(["tags", "sidebar"].map(str::to_owned));
    }

    // Folders. An item that is added stands at the end of the folder.
    let mut wanted_folders = BTreeMap::<&str, RoaringBitmap>::new();
    for (root, view) in &views {
        for folder in &view.folders {
            wanted_folders.entry(folder).or_default().insert(*root);
        }
    }
    let mut folder_changes = BTreeMap::<FolderId, (RoaringBitmap, RoaringBitmap)>::new();
    for (folder_id, members) in next.folders.iter() {
        if !members.is_disjoint(&roots) {
            folder_changes.entry(*folder_id).or_default().1 = &**members & &roots;
        }
    }
    for (key, members) in wanted_folders {
        let folder_id = transaction
            .prepare_cached("SELECT folder_id FROM folder_definition WHERE stable_key = ?1")?
            .query_row([key], |row| row.get::<_, u32>(0))
            .optional()?;
        let Some(folder_id) = folder_id else {
            continue;
        };
        let change = folder_changes.entry(FolderId(folder_id)).or_default();
        change.0 = &members - &change.1;
        change.1 -= &members;
    }
    for (folder_id, (added, removed)) in folder_changes {
        if added.is_empty() && removed.is_empty() {
            continue;
        }
        let before = next
            .folder_orders
            .get(&folder_id)
            .map(|roots| roots.iter().map(|root| root.0).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut after = before
            .iter()
            .copied()
            .filter(|root| !removed.contains(*root))
            .collect::<Vec<_>>();
        after.extend(added.iter());
        changes.push(SemanticChange::Order {
            owner_kind: OrderOwnerKind::Folder,
            owner_id: folder_id.0,
            before: Arc::new(before),
            after: Arc::new(after),
        });
        resources.extend(["folders", "sidebar"].map(str::to_owned));
    }

    let mut dependencies = Vec::new();
    for change in &changes {
        apply_semantic_change(transaction, revision, next, change, true, &mut changed, resources)?;
        history_dependencies(change, &mut dependencies);
    }

    // Ratings and statuses.
    let rating_keys = Rating::ALL.map(|rating| rating.bitmap_key());
    for rating in Rating::ALL {
        let mut moved = views
            .iter()
            .filter(|(_, view)| u32::from(view.rating) == rating.bitmap_key())
            .map(|(root, _)| *root)
            .collect::<RoaringBitmap>();
        moved -= next.rating(rating);
        if moved.is_empty() {
            continue;
        }
        move_to_partition(
            transaction,
            revision,
            next,
            BitmapDomain::Rating,
            rating.bitmap_key(),
            &rating_keys,
            &moved,
        )?;
        changed |= &moved;
        resources.extend(["roots", "ratings"].map(str::to_owned));
    }
    let lifecycle_keys = Lifecycle::ALL.map(|lifecycle| lifecycle.bitmap_key());
    for lifecycle in Lifecycle::ALL {
        let mut moved = views
            .iter()
            .filter(|(_, view)| view.status == lifecycle)
            .map(|(root, _)| *root)
            .collect::<RoaringBitmap>();
        moved -= next.lifecycle(lifecycle);
        if moved.is_empty() {
            continue;
        }
        move_to_partition(
            transaction,
            revision,
            next,
            BitmapDomain::Lifecycle,
            lifecycle.bitmap_key(),
            &lifecycle_keys,
            &moved,
        )?;
        changed |= &moved;
        resources.extend(["roots", "sidebar", "tags", "folders"].map(str::to_owned));
    }
    if !changes.is_empty() {
        crate::smart::settle_affected_for(transaction, next, &changed, &dependencies)?;
        resources.insert("smart-folders".into());
    }
    *affected |= &changed;

    // Deletions.
    if !to_delete.is_empty() {
        let (after, _, receipt) = delete_roots(
            transaction,
            revision,
            next,
            &to_delete,
            now_ms(),
            protected,
            DeletionOrigin::OtherDevice,
        )?;
        *next = after;
        *affected |= &to_delete;
        resources.extend(receipt.resources);
    }
    Ok(Applied { changed: changed.len(), deleted: to_delete.len(), kept: kept_files })
}

/// Handles a deleted tag from `merge_touched`: every picture that has the tag
/// `name` is noted, so its entries decide whether it keeps the tag. A deleted
/// tag that no picture has any more is removed.
fn apply_deleted_tag(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    next: &mut ProjectionSnapshot,
    name: &str,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<()> {
    let Some(tag_id) = next.tag_ids_by_name.get(name).copied() else {
        return Ok(());
    };
    let members = next
        .tags
        .get(&tag_id)
        .map(|members| members.to_bitmap())
        .unwrap_or_default();
    if members.is_empty() {
        remove_emptied_tags(transaction, revision, next, &BTreeSet::from([tag_id]), affected, resources)
    } else {
        touch_files_of_roots(transaction, &members)
    }
}

/// Removes each of `tags` that no picture has any more and that has a
/// `tag_delete` entry.
fn remove_emptied_tags(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    next: &mut ProjectionSnapshot,
    tags: &BTreeSet<crate::TagId>,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<()> {
    let mut removed = false;
    for tag_id in tags {
        if next.tags.get(tag_id).is_some_and(|members| !members.is_empty()) {
            continue;
        }
        let Some(name) = tag_name(transaction, tag_id.0)? else {
            continue;
        };
        let deleted: bool = transaction
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM shared_entry
                 WHERE subject_key = ?1 AND kind = 'tag_delete' AND second_key = '')",
            )?
            .query_row([&name], |row| row.get(0))?;
        if !deleted {
            continue;
        }
        let (members, _) = remove_tag_in_transaction(transaction, next, *tag_id, None, revision)?;
        crate::smart::settle_affected_for(
            transaction,
            next,
            &members,
            &[crate::predicate::DependencyChange::Tag(*tag_id)],
        )?;
        *affected |= &members;
        removed = true;
    }
    if removed {
        resources.extend(["roots", "tags", "navigation", "smart-folders"].map(str::to_owned));
        if prune_deleted_starred_tags(transaction, next)?.is_some() {
            resources.insert("settings".into());
        }
    }
    Ok(())
}

/// Handles the deleted folders from `merge_touched`. A deleted folder is
/// removed once every folder below it is deleted too, so a merge never removes
/// a folder that another computer didn't delete. Until then it's only
/// emptied: its pictures are noted, and their entries take them out.
fn apply_deleted_folders(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    next: &mut ProjectionSnapshot,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<()> {
    let mut statement = transaction.prepare_cached(
        "SELECT folder.folder_id, folder.parent_id,
                EXISTS (SELECT 1 FROM shared_entry entry
                        WHERE entry.subject_key = folder.stable_key
                          AND entry.kind = 'folder_delete' AND entry.second_key = '')
         FROM folder_definition folder",
    )?;
    let folders = statement
        .query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, Option<u32>>(1)?, row.get::<_, bool>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let named = folders
        .iter()
        .filter(|(_, _, deleted)| *deleted)
        .map(|(folder_id, _, _)| *folder_id)
        .collect::<Vec<_>>();
    if named.is_empty() {
        return Ok(());
    }
    let parents = folders
        .iter()
        .map(|(folder_id, parent, _)| (*folder_id, *parent))
        .collect::<HashMap<_, _>>();
    let whole = whole_subtrees(&named, &parents);
    for folder_id in named.iter().filter(|folder_id| !whole.contains(folder_id)) {
        if let Some(members) = next.folders.get(&FolderId(*folder_id)) {
            touch_files_of_roots(transaction, &members.to_bitmap())?;
        }
    }
    if !whole.is_empty() {
        let requested = whole.into_iter().map(FolderId).collect::<Vec<_>>();
        let removed = delete_folder_subtrees(transaction, revision, next, &requested)?;
        *affected |= &removed.affected;
        *next = removed.next;
        resources.extend(["folders", "navigation", "sidebar", "smart-folders"].map(str::to_owned));
    }
    Ok(())
}

/// Handles at most `limit` files of `kept_deleted_file` whose keep time ended
/// before `now_ms`. A file that a picture uses again only loses its row. A
/// file whose picture doesn't exist by the entries goes to
/// `blob_cleanup_queue`. A file whose picture exists by the entries stays,
/// because a merge imports the picture again from it.
pub(super) fn release_kept_files(
    transaction: &rusqlite::Transaction<'_>,
    now_ms: i64,
    limit: usize,
) -> Result<()> {
    let ended = transaction
        .prepare_cached(
            "SELECT kept.content_hash, file.file_id, file.file_path,
                    EXISTS (SELECT 1 FROM media_item media WHERE media.file_id = file.file_id)
             FROM kept_deleted_file kept
             LEFT JOIN media_file file ON file.content_hash = kept.content_hash
             WHERE kept.remove_after_ms <= ?1
               AND NOT EXISTS (SELECT 1 FROM wanted_file wanted
                               WHERE wanted.content_hash = kept.content_hash)
             ORDER BY kept.remove_after_ms LIMIT ?2",
        )?
        .query_map(
            rusqlite::params![now_ms, limit.min(i64::MAX as usize) as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<u32>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if ended.is_empty() {
        return Ok(());
    }
    let revision: i64 =
        transaction.query_row("SELECT revision FROM library_meta WHERE singleton = 1", [], |row| {
            row.get(0)
        })?;
    let mut deleted = DeletedDefinitions::default();
    for (content_hash, file, file_path, used) in ended {
        if let (Some(file_id), Some(file_path), false) = (file, file_path, used) {
            if view_of(transaction, &mut deleted, &content_hash)?.is_some() {
                continue;
            }
            transaction.execute(
                "INSERT INTO blob_cleanup_queue (file_id, file_path, enqueued_revision)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(file_id) DO NOTHING",
                rusqlite::params![file_id, file_path, revision],
            )?;
        }
        transaction
            .execute("DELETE FROM kept_deleted_file WHERE content_hash = ?1", [&content_hash])?;
    }
    Ok(())
}

/// Returns the folders among `named` that can be deleted with everything
/// below them: those whose descendants are all in `named`. `parents` gives
/// the parent of every folder. Of a subtree that can be deleted, only the
/// topmost folder is returned.
fn whole_subtrees(named: &[u32], parents: &HashMap<u32, Option<u32>>) -> Vec<u32> {
    let named = named.iter().copied().collect::<HashSet<_>>();
    let mut children = HashMap::<u32, Vec<u32>>::new();
    for (folder_id, parent) in parents {
        if let Some(parent) = parent {
            children.entry(*parent).or_default().push(*folder_id);
        }
    }
    fn is_whole(
        folder_id: u32,
        named: &HashSet<u32>,
        children: &HashMap<u32, Vec<u32>>,
    ) -> bool {
        named.contains(&folder_id)
            && children
                .get(&folder_id)
                .is_none_or(|below| below.iter().all(|child| is_whole(*child, named, children)))
    }
    let mut whole = named
        .iter()
        .copied()
        .filter(|folder_id| is_whole(*folder_id, &named, &children))
        .filter(|folder_id| {
            // Leave out a folder whose parent is deleted whole: the parent's
            // deletion takes it along.
            parents
                .get(folder_id)
                .copied()
                .flatten()
                .is_none_or(|parent| !is_whole(parent, &named, &children))
        })
        .collect::<Vec<_>>();
    whole.sort_unstable();
    whole
}

impl Library {
    /// Stores those of `entries` that are later than what the library holds
    /// and returns how many were stored. The library itself isn't changed
    /// here; `Library::apply_merged` does that. A library that doesn't record
    /// entries yet starts recording, because merging is part of a sync.
    pub fn store_entries(&self, entries: &[SharedEntry]) -> Result<u64> {
        self.start_recording()?;
        // This computer's changes become entries first, in short
        // transactions. Anything changed between this loop and the
        // transaction below is settled inside that transaction.
        while self.settle_entries(BATCH)? {}
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                let snapshot = self.projections.snapshot();
                settle_in(transaction, &snapshot, usize::MAX)?;
                store_in(transaction, entries)
            })
    }

    /// Changes the library to match the entries that `Library::store_entries`
    /// stored, for at most `limit` pictures, in one published write without an
    /// undo step. The caller repeats the call while `more` is set.
    pub fn apply_merged(&self, limit: usize) -> Result<AppliedMerge> {
        Ok(self.apply_batch(limit, None, false)?.0)
    }

    /// Changes the library to match all stored entries, a batch of pictures
    /// per write. With `keep_held_files`, which a sync sets while a file of
    /// entries couldn't be read, no picture whose file this computer holds is
    /// deleted; those pictures are checked again by the next merge.
    ///
    /// A picture that exists by the entries and whose file this computer kept
    /// after a merge deleted the picture is then imported again from that
    /// file, and changed to match its entries like any other picture.
    pub fn apply_stored(&self, keep_held_files: bool) -> Result<()> {
        self.apply_all(keep_held_files)?;
        if self.import_kept_files()? {
            while self.settle_entries(BATCH)? {}
            self.apply_all(keep_held_files)?;
        }
        Ok(())
    }

    fn apply_all(&self, keep_held_files: bool) -> Result<()> {
        let mut after = None;
        loop {
            let (applied, last) = self.apply_batch(BATCH, after, keep_held_files)?;
            if !applied.more {
                return Ok(());
            }
            after = last;
        }
    }

    /// Imports the files of `wanted_file` that are in `kept_deleted_file`,
    /// from the file on this computer, and returns whether there were any.
    /// The new picture gets the name, notes, rating and status that the
    /// entries say. Its file exists by the entries, so the import writes no
    /// `add` entry. When its pending row becomes an entry, the file is noted
    /// in `merge_touched`, and the next apply gives the picture its tags and
    /// folders.
    fn import_kept_files(&self) -> Result<bool> {
        let inputs = self.database.read(WorkPriority::Cloud, |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT file.file_path, file.mime, file.size_bytes, file.width, file.height,
                        file.duration_ms, file.frame_count, file.content_hash,
                        file.perceptual_hash, file.palette_json
                 FROM kept_deleted_file kept
                 JOIN wanted_file wanted ON wanted.content_hash = kept.content_hash
                 JOIN media_file file ON file.content_hash = kept.content_hash",
            )?;
            let files = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        crate::ImmutableMediaFacts {
                            mime: row.get(1)?,
                            size_bytes: row.get::<_, i64>(2)?.max(0) as u64,
                            width: row.get(3)?,
                            height: row.get(4)?,
                            duration_ms: row
                                .get::<_, Option<i64>>(5)?
                                .map(|value| value.max(0) as u64),
                            frame_count: row.get(6)?,
                            content_hash: row.get(7)?,
                            perceptual_hash: row.get(8)?,
                            palette: Vec::new(),
                        },
                        row.get::<_, String>(9)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut deleted = DeletedDefinitions::default();
            let mut inputs = Vec::new();
            for (file_path, mut facts, palette) in files {
                let Some(view) = view_of(connection, &mut deleted, &facts.content_hash)? else {
                    continue;
                };
                facts.palette = serde_json::from_str(&palette)?;
                inputs.push(PreparedImport {
                    stable_key: uuid::Uuid::new_v4().to_string(),
                    media_name: view.name,
                    file_path,
                    facts,
                    lifecycle: view.status,
                    rating: Rating::ALL
                        .into_iter()
                        .find(|rating| rating.bitmap_key() == u32::from(view.rating))
                        .unwrap_or(Rating::Unrated),
                    notes: (!view.notes.is_empty()).then_some(view.notes),
                    tags: Vec::new(),
                    folders: Vec::new(),
                    source_urls: Vec::new(),
                    source_identity: None,
                    imported_at_ms: now_ms(),
                    captured_at_ms: None,
                });
            }
            Ok(inputs)
        })?;
        for batch in inputs.chunks(ingest::MAX_INGEST_BATCH) {
            self.ingest_batch(batch)?;
        }
        Ok(!inputs.is_empty())
    }

    /// One write of `apply_stored`: the pictures after `after` in
    /// `merge_touched`, at most `limit`. Returns the last one it looked at.
    fn apply_batch(
        &self,
        limit: usize,
        after: Option<String>,
        keep_held_files: bool,
    ) -> Result<(AppliedMerge, Option<String>)> {
        let waiting: bool = self.database.read(WorkPriority::Cloud, |connection| {
            Ok(connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM merge_touched)",
                [],
                |row| row.get(0),
            )?)
        })?;
        if !waiting {
            return Ok((AppliedMerge::default(), None));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let protected = self.history.protected_cleanup_files();
        let ((removed, last, applied), _, ()) = self.database.published_write(
            WorkPriority::Cloud,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                // Changes made on this computer since the entries were
                // stored become entries first, so the merge doesn't undo them.
                settle_in(transaction, &snapshot, usize::MAX)?;
                let mut next = (*snapshot).clone();
                let mut resources = BTreeSet::from(["cloud".to_string()]);
                let mut affected = RoaringBitmap::new();
                let mut tags_to_check = BTreeSet::new();

                let tags = transaction
                    .prepare_cached("SELECT subject_key FROM merge_touched WHERE subject_kind = 'tag'")?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for name in &tags {
                    apply_deleted_tag(
                        transaction,
                        revision,
                        &mut next,
                        name,
                        &mut affected,
                        &mut resources,
                    )?;
                }
                let folders: bool = transaction.query_row(
                    "SELECT EXISTS (SELECT 1 FROM merge_touched WHERE subject_kind = 'folder')",
                    [],
                    |row| row.get(0),
                )?;
                if folders {
                    apply_deleted_folders(
                        transaction,
                        revision,
                        &mut next,
                        &mut affected,
                        &mut resources,
                    )?;
                }
                transaction.execute("DELETE FROM merge_touched WHERE subject_kind IN ('tag', 'folder')", [])?;

                let hashes = transaction
                    .prepare_cached(
                        "SELECT subject_key FROM merge_touched
                         WHERE subject_kind = 'file' AND subject_key > ?2
                         ORDER BY subject_key LIMIT ?1",
                    )?
                    .query_map(
                        rusqlite::params![
                            limit.min(i64::MAX as usize) as i64,
                            after.as_deref().unwrap_or("")
                        ],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let last = hashes.last().cloned().or(after.clone());
                let files = apply_files(
                    transaction,
                    revision,
                    &mut next,
                    &hashes,
                    keep_held_files,
                    &protected,
                    &mut affected,
                    &mut resources,
                    &mut tags_to_check,
                )?;
                let done = hashes.iter().filter(|hash| !files.kept.contains(hash)).collect::<Vec<_>>();
                transaction.execute(
                    "DELETE FROM merge_touched
                     WHERE subject_kind = 'file' AND subject_key IN (SELECT value FROM json_each(?1))",
                    [serde_json::to_string(&done)?],
                )?;
                remove_emptied_tags(
                    transaction,
                    revision,
                    &mut next,
                    &tags_to_check,
                    &mut affected,
                    &mut resources,
                )?;
                let more: bool = transaction.query_row(
                    "SELECT EXISTS (SELECT 1 FROM merge_touched
                                    WHERE subject_kind <> 'file' OR subject_key > ?1)",
                    [last.as_deref().unwrap_or("")],
                    |row| row.get(0),
                )?;
                let changed_library = !affected.is_empty() || resources.len() > 1;
                let removed = files.deleted > 0 || resources.contains("navigation");
                if changed_library {
                    // The row makes the next snapshot due, so that this
                    // device's copy in the cloud folder holds the change.
                    insert_cloud_journal(
                        transaction,
                        revision,
                        "sync.entries_applied",
                        None,
                        serde_json::json!({}),
                        now_ms(),
                    )?;
                }
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    affected.iter().map(RootId),
                );
                Ok((
                    (removed, last, AppliedMerge {
                        more,
                        changed: files.changed,
                        deleted: files.deleted,
                        receipt: changed_library.then(|| receipt.clone()),
                    }),
                    UnrecordedDelta(PublishedDelta { snapshot: next, receipt, history: None }),
                ))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta.0);
            },
        )?;
        // The undo history can't be replayed over pictures, tags or folders
        // that another computer deleted: an undo would put pictures back
        // into a folder that no longer exists.
        if removed {
            self.history.clear();
        }
        Ok((applied, last))
    }

    /// Merges `entries`: stores those that are later than the stored ones and
    /// changes the library to match, a batch of pictures per write. Returns
    /// how many entries were stored.
    pub fn merge_entries(&self, entries: &[SharedEntry]) -> Result<u64> {
        let stored = self.store_entries(entries)?;
        self.apply_stored(false)?;
        Ok(stored)
    }

    /// Returns at most `limit` files that exist by the entries and that this
    /// library doesn't hold, ordered by content hash and starting after
    /// `after`, each with the record a fetch should import it with.
    pub fn wanted_files(&self, after: Option<&str>, limit: usize) -> Result<Vec<WantedFile>> {
        self.database.read(WorkPriority::Cloud, |connection| {
            let hashes = connection
                .prepare_cached(
                    "SELECT content_hash FROM wanted_file WHERE content_hash > ?1
                     ORDER BY content_hash LIMIT ?2",
                )?
                .query_map(
                    rusqlite::params![after.unwrap_or(""), limit.min(i64::MAX as usize) as i64],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut deleted = DeletedDefinitions::default();
            let mut wanted = Vec::new();
            for content_hash in hashes {
                if let Some(view) = view_of(connection, &mut deleted, &content_hash)? {
                    wanted.push(WantedFile { content_hash, record: view.earliest });
                }
            }
            Ok(wanted)
        })
    }

    /// Returns the picture that the entries of `content_hash` describe, or
    /// `None` when it doesn't exist by them.
    pub fn entry_view(&self, content_hash: &str) -> Result<Option<FileView>> {
        self.database.read(WorkPriority::Cloud, |connection| {
            view_of(connection, &mut DeletedDefinitions::default(), content_hash)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> HashMap<u32, Option<u32>> {
        HashMap::from([(1, None), (2, Some(1)), (3, Some(2)), (4, Some(1)), (5, None)])
    }

    #[test]
    fn a_folder_is_deleted_only_with_everything_below_it() {
        // Folder 1 holds 2 and 4, and folder 2 holds 3.
        assert_eq!(whole_subtrees(&[1], &tree()), Vec::<u32>::new());
        assert_eq!(whole_subtrees(&[1, 2, 4], &tree()), vec![4]);
        assert_eq!(whole_subtrees(&[1, 2, 3, 4], &tree()), vec![1]);
        assert_eq!(whole_subtrees(&[2, 3, 5], &tree()), vec![2, 5]);
        assert_eq!(whole_subtrees(&[3], &tree()), vec![3]);
    }
}
