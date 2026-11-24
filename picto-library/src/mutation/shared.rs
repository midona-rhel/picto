//! The entries that the computers of one library send each other through
//! cloud sync, and how this computer writes its own.
//!
//! An entry says what one computer did to one picture, tag or folder, and
//! when. The library keeps one entry per key in `shared_entry`. When two
//! entries have the same key, the later one stays. For example, if this
//! computer rated a picture 3 stars at 10:00 and another computer rated it
//! 5 stars at 10:05, the library keeps the 5-star entry, whichever of the two
//! arrived first. `shared_view.rs` works out what a picture looks like from
//! its entries, and `shared_merge.rs` stores the entries of other computers
//! and changes the library to match.
//!
//! A picture is keyed by the content hash of its file, so two computers that
//! import the same file have the same picture.
//!
//! A library records entries once `Library::start_recording` has been called.
//! After that, every published write compares the library before and after
//! it (`record_changes`) and stores one row in `pending_entry` per kind of
//! change, holding the changed roots as a bitmap. Tagging a million pictures
//! therefore costs one row while the user waits. `Library::settle_entries`
//! turns the rows into entries later, a batch at a time.
//!
//! Deletions of files, tags and folders are written as entries straight away,
//! because the hash or name is gone once the rows are deleted. The pending
//! changes of a deleted picture become entries just before, for the same
//! reason (`settle_roots`).
//!
//! The undo of a deleted file is written into the file's `delete` entry
//! (`write_undos`). It moves the file's cut back to where it was before the
//! deletion, so every entry that the deletion had made obsolete counts again
//! (`shared_view.rs`). Seven days after a cut, `compact_in` drops the entries
//! before it and writes a `compacted` entry, so that every computer drops the
//! same ones and no later undo can bring them back on one computer only.
//!
//! When recording starts, every file the library already holds gets an `add`
//! entry. Those are written in batches by `Library::settle_entries` too, and
//! their record is marked as written at the start, so a tag in it counts as
//! older than any deletion of that tag.
//!
//! Only pictures that are not in a collection are recorded. A picture inside a
//! collection, and the collection itself, get no entries, and a merge leaves
//! them alone.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::shared_view::{cut_of, time_text, Cut, EntryTime, FileRecord, HeldEntry};
use super::*;

/// What an entry says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A computer holds the file. The value is the picture's record.
    Add,
    /// The file was deleted. When the value is set, the deletion was undone:
    /// the value is an `UndoRecord`.
    Delete,
    Name,
    Notes,
    Rating,
    Status,
    /// A tag was put on or taken off a picture.
    ItemTag,
    /// A picture was put into or taken out of a folder.
    FolderItem,
    /// A tag was deleted.
    TagDelete,
    /// A folder was deleted.
    FolderDelete,
    /// The entries of the file at or before the time in the value were
    /// dropped by the computer that wrote it, and no computer counts them
    /// again (`compact_in`).
    Compacted,
}

impl EntryKind {
    pub const ALL: [Self; 11] = [
        Self::Add,
        Self::Delete,
        Self::Name,
        Self::Notes,
        Self::Rating,
        Self::Status,
        Self::ItemTag,
        Self::FolderItem,
        Self::TagDelete,
        Self::FolderDelete,
        Self::Compacted,
    ];

    /// The value of `shared_entry.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Delete => "delete",
            Self::Name => "name",
            Self::Notes => "notes",
            Self::Rating => "rating",
            Self::Status => "status",
            Self::ItemTag => "item_tag",
            Self::FolderItem => "folder_item",
            Self::TagDelete => "tag_delete",
            Self::FolderDelete => "folder_delete",
            Self::Compacted => "compacted",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// One entry, as it is written to the cloud folder and as a merge takes it.
/// The fields are the columns of `shared_entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedEntry {
    pub kind: EntryKind,
    /// The content hash of the file. For `tag_delete` it's the tag's name
    /// and for `folder_delete` the folder's stable key.
    pub subject_key: String,
    /// The computer's id for `add`, the tag's name for `item_tag`, the
    /// folder's stable key for `folder_item`, and empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub second_key: String,
    /// The picture's record as JSON for `add`, the text for `name` and
    /// `notes`, the stars for `rating`, `inbox`, `active` or `trash` for
    /// `status`, `add` or `remove` for `item_tag` and `folder_item`, an
    /// `UndoRecord` as JSON for a `delete` that undoes a deletion, and a time
    /// as `time_text` writes it for `compacted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub clock_ms: i64,
    pub clock_counter: i64,
    /// The computer that wrote the entry.
    pub device_id: String,
}

impl SharedEntry {
    pub fn time(&self) -> EntryTime {
        (self.clock_ms, self.clock_counter, self.device_id.clone())
    }
}

/// The state of this library's exchange of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryClock {
    /// This computer's id.
    pub device_id: String,
    /// The time of the library's clock.
    pub now: (i64, i64),
    /// The clock time up to which this computer's entries are in the cloud
    /// folder, or `None` when none were written yet.
    pub published: Option<(i64, i64)>,
}

/// A file that the entries say exists and that this library doesn't hold
/// yet. The next step of a sync fetches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedFile {
    pub content_hash: String,
    /// The record of the earliest `add` entry that counts, or of the undo
    /// when no add counts.
    pub record: FileRecord,
}

const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// How many files one write of `Library::compact_entries` compacts.
const COMPACT_BATCH: usize = 5_000;

/// The end of every statement that stores an entry. The new entry replaces
/// the stored one only when it is later, so storing the same entries again or
/// in another order gives the same table. A `compacted` entry replaces the
/// stored one when its time value is later, because the later floor is the
/// one every computer has to keep.
pub(super) const KEEP_LATER: &str = "ON CONFLICT(subject_key, kind, second_key) DO UPDATE SET
     value = excluded.value,
     clock_ms = excluded.clock_ms,
     clock_counter = excluded.clock_counter,
     device_id = excluded.device_id
 WHERE CASE WHEN excluded.kind = 'compacted'
     THEN (excluded.value, excluded.clock_ms, excluded.clock_counter, excluded.device_id)
          > (shared_entry.value, shared_entry.clock_ms, shared_entry.clock_counter,
             shared_entry.device_id)
     ELSE (excluded.clock_ms, excluded.clock_counter, excluded.device_id)
          > (shared_entry.clock_ms, shared_entry.clock_counter, shared_entry.device_id)
     END";

/// How long the entries that a deletion made obsolete are kept before
/// `compact_in` drops them, in milliseconds. It's the default of "Keep
/// deleted files", 7 days.
pub(crate) const COMPACT_AFTER_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

/// The SQL expression for the time of an entry in the table `entry` as
/// `time_text` writes it.
macro_rules! time_text_sql {
    ($entry:literal) => {
        concat!(
            "printf('%016d:%010d:%s', ", $entry, ".clock_ms, ", $entry, ".clock_counter, ",
            $entry, ".device_id)"
        )
    };
}

/// The SQL expression for the cut of the file `?hash` as `time_text` writes
/// it, the same cut that `cut_of` returns, and the empty text for none.
fn cut_sql(hash: &str) -> String {
    format!(
        "MAX(
             COALESCE((SELECT floor.value FROM shared_entry floor
                       WHERE floor.subject_key = {hash} AND floor.kind = 'compacted'
                         AND floor.second_key = ''), ''),
             COALESCE((SELECT CASE WHEN json_extract(deleted.value, '$.record') IS NULL
                                   THEN {deleted_time}
                                   ELSE COALESCE(json_extract(deleted.value, '$.before'), '') END
                       FROM shared_entry deleted
                       WHERE deleted.subject_key = {hash} AND deleted.kind = 'delete'
                         AND deleted.second_key = ''), ''))",
        deleted_time = time_text_sql!("deleted"),
    )
}

/// Creates the temporary tables and triggers that the writes of the library
/// fill, on the connection that writes the library.
///
/// `entry_text_change` is filled by a trigger. Names and notes aren't in the
/// resident projection, so a trigger is the cheap way to learn which pictures
/// a write renamed or gave other notes. Missing notes and empty notes count
/// as the same notes.
///
/// `entry_restored_root` holds the pictures that an undo brought back
/// (`mark_restored_files`).
///
/// `entry_deletion_cut` holds, for each file this computer deleted since the
/// library was opened, the cut the file had just before the deletion, as
/// `time_text` writes it. The undo of the deletion moves the cut back to it.
/// The undo history lasts as long as the library is open, and so does this
/// table.
///
/// The two `entry_compaction_due_on_` triggers note the file of every
/// `delete` entry that is stored or replaced in `entry_compaction_due`, so
/// that `compact_in` looks at the file 7 days after the time of the entry.
pub(crate) fn install_recording_triggers(connection: &rusqlite::Connection) -> Result<()> {
    connection.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS entry_restored_root (root_id INTEGER PRIMARY KEY);
         CREATE TEMP TABLE IF NOT EXISTS entry_deletion_cut (
             content_hash TEXT PRIMARY KEY,
             cut TEXT NOT NULL
         );
         CREATE TEMP TRIGGER IF NOT EXISTS entry_compaction_due_on_insert
         AFTER INSERT ON main.shared_entry WHEN NEW.kind = 'delete'
         BEGIN
             INSERT INTO main.entry_compaction_due (content_hash, due_ms)
             VALUES (NEW.subject_key, NEW.clock_ms + {COMPACT_AFTER_MS})
             ON CONFLICT(content_hash) DO UPDATE SET due_ms = MAX(due_ms, excluded.due_ms);
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS entry_compaction_due_on_update
         AFTER UPDATE ON main.shared_entry WHEN NEW.kind = 'delete'
         BEGIN
             INSERT INTO main.entry_compaction_due (content_hash, due_ms)
             VALUES (NEW.subject_key, NEW.clock_ms + {COMPACT_AFTER_MS})
             ON CONFLICT(content_hash) DO UPDATE SET due_ms = MAX(due_ms, excluded.due_ms);
         END;
         CREATE TEMP TABLE IF NOT EXISTS entry_text_change (
             root_id INTEGER PRIMARY KEY,
             name INTEGER NOT NULL,
             notes INTEGER NOT NULL
         );
         CREATE TEMP TRIGGER IF NOT EXISTS entry_text_changed
         AFTER UPDATE OF name, notes ON main.library_root
         WHEN (OLD.name <> NEW.name OR COALESCE(OLD.notes, '') <> COALESCE(NEW.notes, ''))
              AND EXISTS (SELECT 1 FROM main.entry_recording)
         BEGIN
             INSERT INTO entry_text_change (root_id, name, notes)
             VALUES (NEW.root_id, OLD.name <> NEW.name,
                     COALESCE(OLD.notes, '') <> COALESCE(NEW.notes, ''))
             ON CONFLICT(root_id) DO UPDATE SET
                 name = MAX(name, excluded.name),
                 notes = MAX(notes, excluded.notes);
         END;"
    ))?;
    Ok(())
}

/// Moves the library's clock forward and returns the new time. The time is
/// later than every time the clock has returned or seen, even when the
/// computer's wall clock was set back.
pub(super) fn tick(transaction: &rusqlite::Transaction<'_>) -> Result<(i64, i64)> {
    Ok(transaction.query_row(
        "UPDATE cloud_state SET
             hlc_logical = CASE WHEN ?1 > hlc_physical_ms THEN 0 ELSE hlc_logical + 1 END,
             hlc_physical_ms = MAX(?1, hlc_physical_ms)
         WHERE singleton = 1
         RETURNING hlc_physical_ms, hlc_logical",
        [now_ms()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

/// Moves the library's clock up to `time` when `time` is later, so that what
/// this computer does next is later than an entry it has seen.
pub(super) fn observe(transaction: &rusqlite::Transaction<'_>, time: (i64, i64)) -> Result<()> {
    transaction.execute(
        "UPDATE cloud_state SET
             hlc_logical = CASE WHEN ?1 > hlc_physical_ms THEN ?2
                                WHEN ?1 = hlc_physical_ms THEN MAX(hlc_logical, ?2)
                                ELSE hlc_logical END,
             hlc_physical_ms = MAX(?1, hlc_physical_ms)
         WHERE singleton = 1",
        [time.0, time.1],
    )?;
    Ok(())
}

pub(super) fn own_device_id(connection: &rusqlite::Connection) -> Result<String> {
    Ok(connection
        .prepare_cached("SELECT device_id FROM cloud_state WHERE singleton = 1")?
        .query_row([], |row| row.get(0))?)
}

/// Returns whether the library records entries.
pub(super) fn recording(connection: &rusqlite::Connection) -> Result<bool> {
    Ok(connection
        .prepare_cached("SELECT EXISTS (SELECT 1 FROM entry_recording)")?
        .query_row([], |row| row.get(0))?)
}

/// Returns the clock time of the write with the revision `revision`, or
/// `None` when the library doesn't record entries. Everything one write
/// records gets the same time: the first call in a write moves the clock, and
/// later calls in the same write return that time again.
pub(super) fn action_time(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
) -> Result<Option<(i64, i64)>> {
    use rusqlite::OptionalExtension;
    let held = transaction
        .prepare_cached(
            "SELECT action_revision, action_ms, action_counter FROM entry_recording
             WHERE singleton = 1",
        )?
        .query_row([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })
        .optional()?;
    let Some((held_revision, clock_ms, clock_counter)) = held else {
        return Ok(None);
    };
    if held_revision == revision as i64 {
        return Ok(Some((clock_ms, clock_counter)));
    }
    let time = tick(transaction)?;
    transaction
        .prepare_cached(
            "UPDATE entry_recording
             SET action_revision = ?1, action_ms = ?2, action_counter = ?3
             WHERE singleton = 1",
        )?
        .execute(rusqlite::params![revision as i64, time.0, time.1])?;
    Ok(Some(time))
}

/// The name of a tag as entries carry it: the bare name for a tag without a
/// namespace, and `namespace:name` otherwise. It's the same name that
/// `ProjectionSnapshot::tag_ids_by_name` uses.
pub(super) fn whole_tag_name(namespace: &str, subname: &str) -> String {
    if namespace.is_empty() || namespace.eq_ignore_ascii_case("general") {
        subname.to_owned()
    } else {
        format!("{namespace}:{subname}")
    }
}

/// Returns the name of the tag `tag_id`, or `None` when the library doesn't
/// have the tag.
pub(super) fn tag_name(connection: &rusqlite::Connection, tag_id: u32) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(connection
        .prepare_cached(
            "SELECT namespace.display_name, tag.subname
             FROM tag_definition tag
             JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
             WHERE tag.tag_id = ?1",
        )?
        .query_row([tag_id], |row| {
            Ok(whole_tag_name(&row.get::<_, String>(0)?, &row.get::<_, String>(1)?))
        })
        .optional()?)
}

/// Returns the stable key of the folder `folder_id`, or `None` when the
/// library doesn't have the folder.
pub(super) fn folder_key(
    connection: &rusqlite::Connection,
    folder_id: u32,
) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(connection
        .prepare_cached("SELECT stable_key FROM folder_definition WHERE folder_id = ?1")?
        .query_row([folder_id], |row| row.get(0))
        .optional()?)
}

/// The roots that are pictures of their own, outside any collection.
fn file_roots(snapshot: &ProjectionSnapshot) -> RoaringBitmap {
    snapshot
        .root_kinds
        .get(&RootKind::Media)
        .map(|roots| roots.to_bitmap())
        .unwrap_or_default()
}

/// Stores one row of `pending_entry`. A row without roots isn't stored.
fn write_pending(
    transaction: &rusqlite::Transaction<'_>,
    kind: EntryKind,
    second_key: &str,
    value: Option<&str>,
    time: (i64, i64),
    roots: &RoaringBitmap,
) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    transaction
        .prepare_cached(
            "INSERT INTO pending_entry
                 (kind, second_key, value, clock_ms, clock_counter, root_bitmap)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(rusqlite::params![
            kind.as_str(),
            second_key,
            value,
            time.0,
            time.1,
            crate::bitmap::encode(roots)?
        ])?;
    Ok(())
}

/// Stores an entry of this computer that has no second key and no value,
/// which is a `tag_delete` or a `folder_delete`.
fn write_own_entry(
    transaction: &rusqlite::Transaction<'_>,
    kind: EntryKind,
    subject_key: &str,
    time: (i64, i64),
) -> Result<()> {
    transaction
        .prepare_cached(&format!(
            "INSERT INTO shared_entry
                 (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
             SELECT ?1, ?2, '', NULL, ?3, ?4, device_id FROM cloud_state WHERE singleton = 1
             {KEEP_LATER}"
        ))?
        .execute(rusqlite::params![subject_key, kind.as_str(), time.0, time.1])?;
    Ok(())
}

/// The roots that one tag or folder gained and lost in a write.
struct MemberChange {
    id: u32,
    gained: RoaringBitmap,
    lost: RoaringBitmap,
}

/// Compares the member bitmaps of `before` and `after` and returns what each
/// tag or folder gained and lost among `roots`. A bitmap that is the same
/// object in both snapshots didn't change, so only changed ones are compared.
fn member_changes<K: Copy + Eq + std::hash::Hash>(
    before: &HashMap<K, crate::projection::SharedBitmap>,
    after: &HashMap<K, crate::projection::SharedBitmap>,
    roots: &RoaringBitmap,
    id_of: impl Fn(K) -> u32,
) -> Vec<MemberChange> {
    let mut changes = before
        .iter()
        .filter(|(key, _)| !after.contains_key(key))
        .map(|(key, was)| MemberChange {
            id: id_of(*key),
            gained: RoaringBitmap::new(),
            lost: &**was & roots,
        })
        .filter(|change| !change.lost.is_empty())
        .collect::<Vec<_>>();
    for (key, now) in after {
        let was = before.get(key);
        if was.is_some_and(|was| was.allocation_id() == now.allocation_id()) {
            continue;
        }
        let empty = RoaringBitmap::new();
        let was: &RoaringBitmap = was.map_or(&empty, |was| was);
        let mut gained = &**now - was;
        gained &= roots;
        let mut lost = was - &**now;
        lost &= roots;
        if !gained.is_empty() || !lost.is_empty() {
            changes.push(MemberChange { id: id_of(*key), gained, lost });
        }
    }
    changes
}

/// Records what a write changed, when the library records entries. `before`
/// is the library before the write and `after` the library the write leaves.
/// It's called for every published write except a merge, from
/// `PublishedDelta::settle`.
///
/// New pictures get an `add` row. Pictures that existed before and after get
/// rows for a new status, rating, tag, folder, name or notes. A write that
/// changed none of these stores nothing and doesn't move the clock.
pub(super) fn record_changes(
    transaction: &rusqlite::Transaction<'_>,
    before: &ProjectionSnapshot,
    after: &ProjectionSnapshot,
) -> Result<()> {
    if !recording(transaction)? {
        return Ok(());
    }
    let revision = before.revision + 1;
    // Each row is the kind, the second key, the value and the roots.
    let mut rows = Vec::<(EntryKind, String, Option<String>, RoaringBitmap)>::new();
    let files_after = file_roots(after);
    let files_before = if Arc::ptr_eq(&before.root_kinds, &after.root_kinds) {
        files_after.clone()
    } else {
        file_roots(before)
    };
    let in_both = &files_after & &files_before;
    // An import adds a file only when no computer has added it yet, and
    // becomes an entry later, like the other changes. The undo of a deletion
    // is written in this transaction: it needs the cut in
    // `entry_deletion_cut`, which is gone once the library is closed.
    let restored = transaction
        .prepare_cached("SELECT root_id FROM temp.entry_restored_root")?
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    transaction.execute("DELETE FROM temp.entry_restored_root", [])?;
    let added = &files_after - &files_before;
    let undone = &added & &restored;
    rows.push((EntryKind::Add, String::new(), None, &added - &restored));

    if !Arc::ptr_eq(&before.lifecycle, &after.lifecycle) {
        for lifecycle in Lifecycle::ALL {
            let mut moved = after.lifecycle(lifecycle) - before.lifecycle(lifecycle);
            moved &= &in_both;
            rows.push((
                EntryKind::Status,
                String::new(),
                Some(lifecycle_name(lifecycle).to_owned()),
                moved,
            ));
        }
    }
    if !Arc::ptr_eq(&before.ratings, &after.ratings) {
        for rating in Rating::ALL {
            let mut moved = after.rating(rating) - before.rating(rating);
            moved &= &in_both;
            rows.push((
                EntryKind::Rating,
                String::new(),
                Some(rating.bitmap_key().to_string()),
                moved,
            ));
        }
    }
    if !Arc::ptr_eq(&before.tags, &after.tags) {
        let changes = member_changes(&before.tags, &after.tags, &in_both, |tag| tag.0);
        // A tag that the write removed, for example by merging it into
        // another, is taken off its pictures under the name it had before.
        let gone_names = if changes.iter().any(|change| !after.tags.contains_key(&crate::TagId(change.id))) {
            before
                .tag_ids_by_name
                .iter()
                .filter(|(_, tag_id)| !after.tags.contains_key(tag_id))
                .map(|(name, tag_id)| (tag_id.0, name.clone()))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        for change in changes {
            let name = match gone_names.get(&change.id) {
                Some(name) => Some(name.clone()),
                None => tag_name(transaction, change.id)?,
            };
            if let Some(name) = name {
                rows.push((EntryKind::ItemTag, name.clone(), Some("add".into()), change.gained));
                rows.push((EntryKind::ItemTag, name, Some("remove".into()), change.lost));
            }
        }
    }
    if !Arc::ptr_eq(&before.folders, &after.folders) {
        for change in member_changes(&before.folders, &after.folders, &in_both, |folder| folder.0)
        {
            if let Some(key) = folder_key(transaction, change.id)? {
                rows.push((EntryKind::FolderItem, key.clone(), Some("add".into()), change.gained));
                rows.push((EntryKind::FolderItem, key, Some("remove".into()), change.lost));
            }
        }
    }
    let (mut renamed, mut noted) = (RoaringBitmap::new(), RoaringBitmap::new());
    {
        let mut statement = transaction
            .prepare_cached("SELECT root_id, name, notes FROM temp.entry_text_change")?;
        let mut changed = statement.query([])?;
        while let Some(row) = changed.next()? {
            let root = row.get::<_, u32>(0)?;
            if !in_both.contains(root) {
                continue;
            }
            if row.get::<_, bool>(1)? {
                renamed.insert(root);
            }
            if row.get::<_, bool>(2)? {
                noted.insert(root);
            }
        }
    }
    transaction.execute("DELETE FROM temp.entry_text_change", [])?;
    rows.push((EntryKind::Name, String::new(), None, renamed));
    rows.push((EntryKind::Notes, String::new(), None, noted));

    rows.retain(|row| !row.3.is_empty());
    if rows.is_empty() && undone.is_empty() {
        return Ok(());
    }
    let Some(time) = action_time(transaction, revision)? else {
        return Ok(());
    };
    if !undone.is_empty() {
        let device_id = own_device_id(transaction)?;
        settle_row(
            transaction,
            after,
            EntryKind::Add.as_str(),
            "",
            Some("undo"),
            time,
            &undone,
            &device_id,
        )?;
    }
    for (kind, second_key, value, roots) in rows {
        write_pending(transaction, kind, &second_key, value.as_deref(), time, &roots)?;
    }
    Ok(())
}

/// Drops what the trigger collected during a write that isn't recorded, which
/// is a merge of other computers' entries.
pub(super) fn discard_changes(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM temp.entry_text_change", [])?;
    transaction.execute("DELETE FROM temp.entry_restored_root", [])?;
    Ok(())
}

/// Notes the pictures that are in `after` and not in `before`, for an undo or
/// redo that brings deleted pictures back. `record_changes` writes the undo of
/// the deletion for them in the same write (`write_undos`).
pub(super) fn mark_restored_files(
    transaction: &rusqlite::Transaction<'_>,
    before: &ProjectionSnapshot,
    after: &ProjectionSnapshot,
) -> Result<()> {
    if Arc::ptr_eq(&before.root_kinds, &after.root_kinds) {
        return Ok(());
    }
    let restored = file_roots(after) - file_roots(before);
    if restored.is_empty() {
        return Ok(());
    }
    transaction.execute(
        "INSERT OR IGNORE INTO temp.entry_restored_root (root_id)
         SELECT value FROM json_each(?1)",
        [serde_json::to_string(&restored.iter().collect::<Vec<_>>())?],
    )?;
    Ok(())
}

/// The value of a `status` entry.
pub(super) fn lifecycle_name(lifecycle: Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::Inbox => "inbox",
        Lifecycle::Trash => "trash",
    }
}

/// Records what a change in the undo history did to tag names and folders,
/// when the library records entries. `forward` is `true` when the change is
/// made or redone and `false` when it's undone. `after` is the library the
/// write leaves.
///
/// Entries name a tag by its name, so renaming a tag is recorded as deleting
/// the old name and putting the new name on every picture that has the tag.
/// Renaming a tag group renames every tag in it. A folder that the change
/// removes gets a `folder_delete` entry.
///
/// A deleted folder never holds anything again on any computer, so a folder
/// that an undo brings back gets a new stable key. Its pictures are then
/// recorded under the new key by `record_changes`.
pub(super) fn record_definition_changes(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    after: &ProjectionSnapshot,
    change: &SemanticChange,
    forward: bool,
) -> Result<()> {
    if !recording(transaction)? {
        return Ok(());
    }
    let mut renamed_tags = Vec::<(u32, String, String)>::new();
    let mut removed_folders = Vec::<String>::new();
    let mut returned_folders = Vec::<(FolderId, String)>::new();
    collect_definition_changes(
        transaction,
        change,
        forward,
        &mut renamed_tags,
        &mut removed_folders,
        &mut returned_folders,
    )?;
    let files = file_roots(after);
    for (tag_id, old_name, new_name) in renamed_tags {
        let Some(time) = action_time(transaction, revision)? else {
            return Ok(());
        };
        write_own_entry(transaction, EntryKind::TagDelete, &old_name, time)?;
        if let Some(members) = after.tags.get(&crate::TagId(tag_id)) {
            let members = &**members & &files;
            write_pending(transaction, EntryKind::ItemTag, &new_name, Some("add"), time, &members)?;
        }
    }
    for key in removed_folders {
        let Some(time) = action_time(transaction, revision)? else {
            return Ok(());
        };
        write_own_entry(transaction, EntryKind::FolderDelete, &key, time)?;
    }
    for (folder_id, key) in returned_folders {
        let deleted: bool = transaction
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM shared_entry
                 WHERE subject_key = ?1 AND kind = 'folder_delete' AND second_key = '')",
            )?
            .query_row([&key], |row| row.get(0))?;
        if deleted {
            transaction.execute(
                "UPDATE folder_definition SET stable_key = ?1 WHERE folder_id = ?2",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), folder_id.0],
            )?;
        }
    }
    Ok(())
}

fn collect_definition_changes(
    transaction: &rusqlite::Transaction<'_>,
    change: &SemanticChange,
    forward: bool,
    renamed_tags: &mut Vec<(u32, String, String)>,
    removed_folders: &mut Vec<String>,
    returned_folders: &mut Vec<(FolderId, String)>,
) -> Result<()> {
    match change {
        SemanticChange::Compound(changes) => {
            for change in changes {
                collect_definition_changes(
                    transaction,
                    change,
                    forward,
                    renamed_tags,
                    removed_folders,
                    returned_folders,
                )?;
            }
        }
        SemanticChange::TagName { tag_id, before, after } => {
            let (old, new) = if forward { (before, after) } else { (after, before) };
            if old != new {
                renamed_tags.push((tag_id.0, old.clone(), new.clone()));
            }
        }
        SemanticChange::TagNamespaceName { namespace_id, before, after } => {
            let (old, new) = if forward { (before, after) } else { (after, before) };
            let mut statement = transaction.prepare_cached(
                "SELECT tag_id, subname FROM tag_definition WHERE namespace_id = ?1",
            )?;
            let tags = statement
                .query_map([namespace_id.0], |row| {
                    Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (tag_id, subname) in tags {
                let (old, new) = (whole_tag_name(old, &subname), whole_tag_name(new, &subname));
                if old != new {
                    renamed_tags.push((tag_id, old, new));
                }
            }
        }
        SemanticChange::FolderDefinition { folder_id, before, after } => {
            let (old, new) = if forward { (before, after) } else { (after, before) };
            match (old, new) {
                (Some(old), None) => removed_folders.push(old.stable_key.clone()),
                (None, Some(new)) => returned_folders.push((*folder_id, new.stable_key.clone())),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Records that the user deleted the tags `names`, when the library records
/// entries. Merging one tag into another doesn't call this: the pictures of
/// the merged tag get the other tag, and other computers keep the merged tag
/// on pictures they tagged themselves.
pub(super) fn record_deleted_tags(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    names: &[String],
) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let Some(time) = action_time(transaction, revision)? else {
        return Ok(());
    };
    for name in names {
        write_own_entry(transaction, EntryKind::TagDelete, name, time)?;
    }
    Ok(())
}

/// Called by `delete_roots` while the deleted media are listed in
/// `temp.delete_media` and their rows still exist. When the user deleted the
/// pictures, or the library merged a duplicate away, it stores a `delete`
/// entry for each file that no other picture in the library still uses, and
/// notes in `entry_deletion_cut` the cut that the file had before. A picture
/// inside a collection gets no entry, because collections don't sync yet. A
/// deletion that came from another computer, or the undo of an import,
/// stores nothing.
pub(super) fn record_deleted_files(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    origin: DeletionOrigin,
) -> Result<()> {
    if !matches!(origin, DeletionOrigin::User | DeletionOrigin::MergedDuplicate) {
        return Ok(());
    }
    let Some((clock_ms, clock_counter)) = action_time(transaction, revision)? else {
        return Ok(());
    };
    transaction.execute(
        &format!(
            "INSERT OR REPLACE INTO temp.entry_deletion_cut (content_hash, cut)
             SELECT DISTINCT file.content_hash, {cut}
             FROM temp.delete_media selected
             CROSS JOIN media_item media ON media.media_id = selected.local_id
             JOIN media_file file ON file.file_id = media.file_id",
            cut = cut_sql("file.content_hash")
        ),
        [],
    )?;
    transaction.execute(
        &format!(
            "INSERT INTO shared_entry
                 (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
             SELECT DISTINCT file.content_hash, 'delete', '', NULL, ?1, ?2,
                    (SELECT device_id FROM cloud_state WHERE singleton = 1)
             FROM temp.delete_media selected
             CROSS JOIN media_item media ON media.media_id = selected.local_id
             JOIN media_file file ON file.file_id = media.file_id
             WHERE EXISTS (SELECT 1 FROM library_root root WHERE root.root_id = media.media_id)
               AND NOT EXISTS (
                 SELECT 1 FROM media_item other
                 WHERE other.file_id = media.file_id
                   AND other.media_id NOT IN (SELECT local_id FROM temp.delete_media))
             {KEEP_LATER}"
        ),
        [clock_ms, clock_counter],
    )?;
    Ok(())
}

/// Records that a repair replaced the file of the media `media_id`, whose old
/// content hash was `old_hash`: the old file gets a `delete` entry and the
/// new file an `add` entry. A picture inside a collection gets neither,
/// because collections don't sync yet.
pub(super) fn record_replaced_file(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &ProjectionSnapshot,
    old_hash: &str,
    media_id: MediaId,
) -> Result<()> {
    if !file_roots(snapshot).contains(media_id.0) {
        return Ok(());
    }
    let Some(time) = action_time(transaction, revision)? else {
        return Ok(());
    };
    transaction
        .prepare_cached(&format!(
            "INSERT INTO shared_entry
                 (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
             SELECT ?1, 'delete', '', NULL, ?2, ?3, device_id FROM cloud_state WHERE singleton = 1
             {KEEP_LATER}"
        ))?
        .execute(rusqlite::params![old_hash, time.0, time.1])?;
    write_pending(
        transaction,
        EntryKind::Add,
        "",
        Some("always"),
        time,
        &RoaringBitmap::from_iter([media_id.0]),
    )?;
    Ok(())
}

/// Returns those of `roots` whose file has an `add` entry of this computer,
/// which other computers may already have received.
pub(super) fn recorded_roots(
    transaction: &rusqlite::Transaction<'_>,
    roots: &RoaringBitmap,
) -> Result<RoaringBitmap> {
    if roots.is_empty() || !recording(transaction)? {
        return Ok(RoaringBitmap::new());
    }
    let mut statement = transaction.prepare_cached(
        "SELECT ids.value FROM json_each(?1) ids
         CROSS JOIN media_item media ON media.media_id = ids.value
         JOIN media_file file ON file.file_id = media.file_id
         WHERE EXISTS (
             SELECT 1 FROM shared_entry entry
             WHERE entry.subject_key = file.content_hash AND entry.kind = 'add'
               AND entry.second_key = (SELECT device_id FROM cloud_state WHERE singleton = 1))",
    )?;
    let recorded = statement
        .query_map([serde_json::to_string(&roots.iter().collect::<Vec<_>>())?], |row| {
            row.get::<_, u32>(0)
        })?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    Ok(recorded)
}

/// Returns those of `roots` whose file exists by the stored entries. The
/// caller passes roots that have no `add` entry of this computer, so their
/// file exists by what another computer did.
pub(super) fn roots_added_elsewhere(
    transaction: &rusqlite::Transaction<'_>,
    roots: &RoaringBitmap,
) -> Result<RoaringBitmap> {
    if roots.is_empty() || !recording(transaction)? {
        return Ok(RoaringBitmap::new());
    }
    let files = transaction
        .prepare_cached(
            "SELECT ids.value, file.content_hash FROM json_each(?1) ids
             CROSS JOIN media_item media ON media.media_id = ids.value
             JOIN media_file file ON file.file_id = media.file_id",
        )?
        .query_map([serde_json::to_string(&roots.iter().collect::<Vec<_>>())?], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let existing = existing_files(
        transaction,
        &files.iter().map(|(_, hash)| hash.as_str()).collect::<Vec<_>>(),
    )?;
    Ok(files
        .into_iter()
        .filter(|(_, hash)| existing.contains(hash))
        .map(|(root, _)| root)
        .collect())
}

/// Removes the tombstones of the items `keys`, inside the caller's
/// transaction. A tombstone is a row of `deletion_tombstone`; it stops an
/// import from bringing back an item that was deleted after the import was
/// prepared. An import that is later than the deletion, a subscription that
/// brings an item again, resetting a subscription and undoing a permanent
/// deletion remove tombstones through this function. Returns how many were
/// removed.
pub fn remove_item_tombstones(
    transaction: &rusqlite::Transaction<'_>,
    keys: &[String],
) -> Result<usize> {
    if keys.is_empty() {
        return Ok(0);
    }
    Ok(transaction.execute(
        "DELETE FROM deletion_tombstone
         WHERE stable_key IN (SELECT value FROM json_each(?1))",
        [serde_json::to_string(keys)?],
    )?)
}

/// Makes the library write all of this computer's entries into the cloud
/// folder again at its next sync. It's called inside the transaction that
/// connects the library to a cloud folder, because the new folder holds none
/// of them.
pub fn reset_published_entries(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM cloud_device_frontier", [])?;
    Ok(())
}

/// Returns those of `hashes` whose file exists by the stored entries, by the
/// same rule as `file_view`: an `add` entry is later than the file's cut, or
/// the `delete` entry is an undo. It is one query for all of `hashes`.
pub(super) fn existing_files(
    connection: &rusqlite::Connection,
    hashes: &[&str],
) -> Result<HashSet<String>> {
    if hashes.is_empty() {
        return Ok(HashSet::new());
    }
    let existing = connection
        .prepare_cached(&format!(
            "SELECT hashes.value FROM json_each(?1) hashes
             WHERE EXISTS (
                     SELECT 1 FROM shared_entry deleted
                     WHERE deleted.subject_key = hashes.value AND deleted.kind = 'delete'
                       AND deleted.second_key = ''
                       AND json_extract(deleted.value, '$.record') IS NOT NULL)
                OR EXISTS (
                     SELECT 1 FROM shared_entry added
                     WHERE added.subject_key = hashes.value AND added.kind = 'add'
                       AND {added_time} > {cut})",
            added_time = time_text_sql!("added"),
            cut = cut_sql("hashes.value"),
        ))?
        .query_map([serde_json::to_string(hashes)?], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(existing)
}

/// One picture outside any collection, with its file and its record as the
/// library holds it now.
pub(super) struct HeldFile {
    pub root: u32,
    pub content_hash: String,
    pub record: FileRecord,
}

/// Returns the pictures among `roots` that aren't in a collection, each with
/// its record as the library holds it now, ordered by root.
///
/// Tags and folders are found by intersecting each tag's and folder's bitmap
/// with the batch, which is one pass over the tags and folders per batch
/// instead of a query per picture.
pub(super) fn file_records(
    connection: &rusqlite::Connection,
    snapshot: &ProjectionSnapshot,
    roots: &RoaringBitmap,
    start: bool,
) -> Result<Vec<HeldFile>> {
    let mut files = Vec::new();
    {
        let mut statement = connection.prepare_cached(
            "SELECT root.root_id, file.content_hash, root.name, root.notes
             FROM json_each(?1) ids
             CROSS JOIN library_root root ON root.root_id = ids.value
             JOIN media_item media ON media.media_id = root.root_id
             JOIN media_file file ON file.file_id = media.file_id
             ORDER BY root.root_id",
        )?;
        let ids = serde_json::to_string(&roots.iter().collect::<Vec<_>>())?;
        let mut rows = statement.query([ids])?;
        while let Some(row) = rows.next()? {
            let root = row.get::<_, u32>(0)?;
            files.push(HeldFile {
                root,
                content_hash: row.get(1)?,
                record: FileRecord {
                    name: row.get(2)?,
                    notes: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    rating: Rating::ALL
                        .into_iter()
                        .find(|rating| snapshot.rating(*rating).contains(root))
                        .map_or(0, |rating| rating.bitmap_key() as u8),
                    status: Lifecycle::ALL
                        .into_iter()
                        .find(|lifecycle| snapshot.lifecycle(*lifecycle).contains(root))
                        .unwrap_or(Lifecycle::Active),
                    tags: Vec::new(),
                    folders: Vec::new(),
                    start,
                },
            });
        }
    }
    let held = files.iter().map(|file| file.root).collect::<RoaringBitmap>();
    let mut tagged = Vec::new();
    for (tag_id, members) in snapshot.tags.iter() {
        if members.is_disjoint(&held) {
            continue;
        }
        if let Some(name) = tag_name(connection, tag_id.0)? {
            tagged.extend((&**members & &held).iter().map(|root| (root, name.clone())));
        }
    }
    let mut placed = Vec::new();
    for (folder_id, members) in snapshot.folders.iter() {
        if members.is_disjoint(&held) {
            continue;
        }
        if let Some(key) = folder_key(connection, folder_id.0)? {
            placed.extend((&**members & &held).iter().map(|root| (root, key.clone())));
        }
    }
    for (root, name) in tagged {
        if let Ok(index) = files.binary_search_by_key(&root, |file| file.root) {
            files[index].record.tags.push(name);
        }
    }
    for (root, key) in placed {
        if let Ok(index) = files.binary_search_by_key(&root, |file| file.root) {
            files[index].record.folders.push(key);
        }
    }
    for file in &mut files {
        file.record.tags.sort();
        file.record.folders.sort();
    }
    Ok(files)
}

/// Returns those of `hashes` that have a `delete` entry.
fn deleted_files(connection: &rusqlite::Connection, hashes: &[&str]) -> Result<HashSet<String>> {
    if hashes.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(connection
        .prepare_cached(
            "SELECT hashes.value FROM json_each(?1) hashes
             CROSS JOIN shared_entry entry
                 ON entry.subject_key = hashes.value AND entry.kind = 'delete'
                    AND entry.second_key = ''",
        )?
        .query_map([serde_json::to_string(hashes)?], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?)
}

/// Stores the undo of the deletion of each of `files` at `time`, in the
/// `delete` entry of the file. The undo moves the cut back to the one in
/// `entry_deletion_cut`, so every entry that the deletion had made obsolete
/// counts again, including those of other computers that hadn't arrived
/// here. A deletion that this computer made before it recorded entries has no
/// row there; when another computer's `delete` entry for the file arrived
/// since, the cut stays where it is, and the picture comes back with the
/// record of the undo. The next merge makes the picture look the way the
/// entries say.
fn write_undos(
    transaction: &rusqlite::Transaction<'_>,
    files: &[HeldFile],
    time: (i64, i64),
    device_id: &str,
) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut statement = transaction.prepare_cached(&format!(
        "INSERT INTO shared_entry
             (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
         VALUES (?1, 'delete', '', json_object('before', COALESCE(
                     (SELECT cut FROM temp.entry_deletion_cut WHERE content_hash = ?1), {cut}),
                     'record', json(?2)),
                 ?3, ?4, ?5)
         {KEEP_LATER}",
        cut = cut_sql("?1")
    ))?;
    for file in files {
        statement.execute(rusqlite::params![
            file.content_hash,
            serde_json::to_string(&file.record)?,
            time.0,
            time.1,
            device_id
        ])?;
    }
    transaction.execute(
        "INSERT OR IGNORE INTO merge_touched (subject_kind, subject_key)
         SELECT 'file', value FROM json_each(?1)",
        [serde_json::to_string(&files.iter().map(|file| &file.content_hash).collect::<Vec<_>>())?],
    )?;
    Ok(())
}

/// Stores an `add` entry of this computer for each of `files` at `time`.
fn write_adds(
    transaction: &rusqlite::Transaction<'_>,
    files: &[HeldFile],
    time: (i64, i64),
    device_id: &str,
) -> Result<()> {
    let mut statement = transaction.prepare_cached(&format!(
        "INSERT INTO shared_entry
             (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
         VALUES (?1, 'add', ?2, ?3, ?4, ?5, ?2)
         {KEEP_LATER}"
    ))?;
    for file in files {
        statement.execute(rusqlite::params![
            file.content_hash,
            device_id,
            serde_json::to_string(&file.record)?,
            time.0,
            time.1
        ])?;
    }
    Ok(())
}

/// Writes the `add` entries of the start of recording for at most `limit`
/// roots and returns how many roots it looked at, or zero when they're all
/// written.
fn settle_start(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    limit: usize,
    device_id: &str,
) -> Result<usize> {
    use rusqlite::OptionalExtension;
    let start = transaction
        .prepare_cached(
            "SELECT start_next_root, start_end_root, started_ms, started_counter
             FROM entry_recording
             WHERE singleton = 1 AND start_next_root IS NOT NULL",
        )?
        .query_row([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                (row.get::<_, i64>(2)?, row.get::<_, i64>(3)?),
            ))
        })
        .optional()?;
    let Some((next, end, time)) = start else {
        return Ok(0);
    };
    let roots = transaction
        .prepare_cached(
            "SELECT root.root_id FROM library_root root
             JOIN media_item media ON media.media_id = root.root_id
             WHERE root.root_id >= ?1 AND root.root_id < ?2
             ORDER BY root.root_id LIMIT ?3",
        )?
        .query_map(rusqlite::params![next, end, limit as i64], |row| row.get::<_, u32>(0))?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    let files = file_records(transaction, snapshot, &roots, true)?;
    write_adds(transaction, &files, time, device_id)?;
    // Fewer roots than the limit means that none are left below the end.
    let next = roots
        .max()
        .filter(|_| roots.len() as usize == limit)
        .map(|last| i64::from(last) + 1);
    transaction.execute(
        "UPDATE entry_recording SET start_next_root = ?1 WHERE singleton = 1",
        [next],
    )?;
    Ok((roots.len() as usize).max(1))
}

/// Turns the roots `taken` of one row of `pending_entry` into entries.
/// Returns `false` for a kind that is never stored as a row.
#[allow(clippy::too_many_arguments)]
fn settle_row(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    kind: &str,
    second_key: &str,
    value: Option<&str>,
    time: (i64, i64),
    taken: &RoaringBitmap,
    device_id: &str,
) -> Result<bool> {
    match EntryKind::parse(kind) {
        Some(EntryKind::Add) => {
            // An import of a file that already exists by the entries
            // writes nothing. The picture is changed to look the way the
            // entries say instead, by the next merge. A repair (`always`)
            // writes its `add` in any case. An undo (`undo`) of a deletion
            // that has a `delete` entry writes the undo into that entry,
            // and otherwise an `add` like a repair.
            let files = file_records(transaction, snapshot, taken, false)?;
            let (undone, files): (Vec<_>, Vec<_>) = if value == Some("undo") {
                let deleted = deleted_files(
                    transaction,
                    &files.iter().map(|file| file.content_hash.as_str()).collect::<Vec<_>>(),
                )?;
                files.into_iter().partition(|file| deleted.contains(&file.content_hash))
            } else {
                (Vec::new(), files)
            };
            write_undos(transaction, &undone, time, device_id)?;
            let existing = if matches!(value, Some("always" | "undo")) {
                HashSet::new()
            } else {
                existing_files(
                    transaction,
                    &files.iter().map(|file| file.content_hash.as_str()).collect::<Vec<_>>(),
                )?
            };
            let (known, new): (Vec<_>, Vec<_>) =
                files.into_iter().partition(|file| existing.contains(&file.content_hash));
            // Other entries of the file can win over the new record: a
            // rename on another computer after a deletion, or an earlier
            // add when an undo or a repair brings the file back. The
            // next merge brings the picture in line with them.
            transaction.execute(
                "INSERT OR IGNORE INTO merge_touched (subject_kind, subject_key)
                 SELECT 'file', hashes.value FROM json_each(?1) hashes
                 WHERE EXISTS (SELECT 1 FROM shared_entry entry
                               WHERE entry.subject_key = hashes.value
                                 AND (entry.kind <> 'add' OR entry.second_key <> ?2))",
                rusqlite::params![
                    serde_json::to_string(
                        &new.iter().map(|file| &file.content_hash).collect::<Vec<_>>(),
                    )?,
                    device_id
                ],
            )?;
            write_adds(transaction, &new, time, device_id)?;
            if !known.is_empty() {
                transaction.execute(
                    "INSERT OR IGNORE INTO merge_touched (subject_kind, subject_key)
                     SELECT 'file', value FROM json_each(?1)",
                    [serde_json::to_string(
                        &known.iter().map(|file| &file.content_hash).collect::<Vec<_>>(),
                    )?],
                )?;
            }
        }
        Some(kind @ (EntryKind::Name | EntryKind::Notes)) => {
            // The rows hold no text: the text is read now. A later change
            // of the same text has its own, later row, so the last entry
            // always carries the text the picture has.
            let column = if kind == EntryKind::Name {
                "root.name"
            } else {
                "COALESCE(root.notes, '')"
            };
            transaction.execute(
                &format!(
                    "INSERT INTO shared_entry
                         (subject_key, kind, second_key, value, clock_ms, clock_counter,
                          device_id)
                     SELECT file.content_hash, ?2, '', {column}, ?3, ?4, ?5
                     FROM json_each(?1) ids
                     CROSS JOIN library_root root ON root.root_id = ids.value
                     JOIN media_item media ON media.media_id = root.root_id
                     JOIN media_file file ON file.file_id = media.file_id
                     WHERE true
                     {KEEP_LATER}"
                ),
                rusqlite::params![
                    serde_json::to_string(&taken.iter().collect::<Vec<_>>())?,
                    kind.as_str(),
                    time.0,
                    time.1,
                    device_id
                ],
            )?;
        }
        Some(
            kind @ (EntryKind::Rating
            | EntryKind::Status
            | EntryKind::ItemTag
            | EntryKind::FolderItem),
        ) => {
            transaction.execute(
                &format!(
                    "INSERT INTO shared_entry
                         (subject_key, kind, second_key, value, clock_ms, clock_counter,
                          device_id)
                     SELECT file.content_hash, ?2, ?3, ?4, ?5, ?6, ?7
                     FROM json_each(?1) ids
                     CROSS JOIN library_root root ON root.root_id = ids.value
                     JOIN media_item media ON media.media_id = root.root_id
                     JOIN media_file file ON file.file_id = media.file_id
                     WHERE true
                     {KEEP_LATER}"
                ),
                rusqlite::params![
                    serde_json::to_string(&taken.iter().collect::<Vec<_>>())?,
                    kind.as_str(),
                    second_key,
                    value,
                    time.0,
                    time.1,
                    device_id
                ],
            )?;
        }
        // No other kind is stored as a row.
        _ => return Ok(false),
    }
    Ok(true)
}

/// Turns the pending changes of `roots` into entries now, before a deletion
/// removes the roots. Pending changes name a picture by its root and get the
/// file's hash when they become entries, so after the deletion they would be
/// lost. The undo of the deletion brings them back, as it brings back the
/// entries of the other computers.
pub(super) fn settle_roots(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    roots: &RoaringBitmap,
) -> Result<()> {
    if roots.is_empty() || !recording(transaction)? {
        return Ok(());
    }
    let device_id = own_device_id(transaction)?;
    let rows = transaction
        .prepare_cached(
            "SELECT pending_id, kind, second_key, value, clock_ms, clock_counter, root_bitmap
             FROM pending_entry ORDER BY pending_id",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                (row.get::<_, i64>(4)?, row.get::<_, i64>(5)?),
                row.get::<_, Vec<u8>>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (pending_id, kind, second_key, value, time, bitmap) in rows {
        let mut pending = RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(bitmap))?;
        let taken = &pending & roots;
        if taken.is_empty() {
            continue;
        }
        settle_row(
            transaction,
            snapshot,
            &kind,
            &second_key,
            value.as_deref(),
            time,
            &taken,
            &device_id,
        )?;
        pending -= &taken;
        if pending.is_empty() {
            transaction.execute("DELETE FROM pending_entry WHERE pending_id = ?1", [pending_id])?;
        } else {
            transaction.execute(
                "UPDATE pending_entry SET root_bitmap = ?2 WHERE pending_id = ?1",
                rusqlite::params![pending_id, crate::bitmap::encode(&pending)?],
            )?;
        }
    }
    Ok(())
}

/// Turns this computer's pending changes into entries, at most `limit` roots,
/// and returns whether work is left. The `add` entries of the start of
/// recording come first, then the rows of `pending_entry` in the order they
/// were stored. `snapshot` is the library as the transaction sees it.
pub(super) fn settle_in(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    limit: usize,
) -> Result<bool> {
    use rusqlite::OptionalExtension;
    if !recording(transaction)? {
        return Ok(false);
    }
    let device_id = own_device_id(transaction)?;
    let mut room = limit;
    while room > 0 {
        let looked_at = settle_start(transaction, snapshot, room, &device_id)?;
        if looked_at == 0 {
            break;
        }
        room = room.saturating_sub(looked_at);
    }
    loop {
        let row = transaction
            .prepare_cached(
                "SELECT pending_id, kind, second_key, value, clock_ms, clock_counter, root_bitmap
                 FROM pending_entry ORDER BY pending_id LIMIT 1",
            )?
            .query_row([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    (row.get::<_, i64>(4)?, row.get::<_, i64>(5)?),
                    row.get::<_, Vec<u8>>(6)?,
                ))
            })
            .optional()?;
        let Some((pending_id, kind, second_key, value, time, roots)) = row else {
            let starting: bool = transaction.query_row(
                "SELECT EXISTS (SELECT 1 FROM entry_recording
                                WHERE start_next_root IS NOT NULL)",
                [],
                |row| row.get(0),
            )?;
            return Ok(starting);
        };
        if room == 0 {
            return Ok(true);
        }
        let mut roots = RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(roots))?;
        let taken = roots.iter().take(room).collect::<RoaringBitmap>();
        room -= taken.len() as usize;
        if !settle_row(
            transaction,
            snapshot,
            &kind,
            &second_key,
            value.as_deref(),
            time,
            &taken,
            &device_id,
        )? {
            roots.clear();
        }
        roots -= &taken;
        if roots.is_empty() {
            transaction.execute("DELETE FROM pending_entry WHERE pending_id = ?1", [pending_id])?;
        } else {
            transaction.execute(
                "UPDATE pending_entry SET root_bitmap = ?2 WHERE pending_id = ?1",
                rusqlite::params![pending_id, crate::bitmap::encode(&roots)?],
            )?;
        }
    }
}

/// The kinds of entries of a file that its cut makes obsolete. `compact_in`
/// drops them, and a merge doesn't store them, once they are at or before
/// the file's floor.
pub(super) const DROPPED_KINDS: &str =
    "('add', 'name', 'notes', 'rating', 'status', 'item_tag', 'folder_item')";

/// Drops the entries of the file `content_hash` whose kind is one of
/// `DROPPED_KINDS` and whose time is at or before `through`, which is a time
/// as `time_text` writes it. This computer's own entries are dropped only
/// with `own`, and they are noted in `removed_own_entry`, so that the next
/// sync writes their files in the cloud folder again without them.
pub(super) fn drop_below(
    transaction: &rusqlite::Transaction<'_>,
    content_hash: &str,
    through: &str,
    own: bool,
) -> Result<()> {
    let old = format!(
        "subject_key = ?1 AND kind IN {DROPPED_KINDS} AND {time} <= ?2",
        time = time_text_sql!("shared_entry")
    );
    let own_device = "(SELECT device_id FROM cloud_state WHERE singleton = 1)";
    if own {
        transaction
            .prepare_cached(&format!(
                "INSERT OR IGNORE INTO removed_own_entry (subject_key, kind, second_key)
                 SELECT subject_key, kind, second_key FROM shared_entry
                 WHERE {old} AND device_id = {own_device}"
            ))?
            .execute([content_hash, through])?;
        transaction
            .prepare_cached(&format!("DELETE FROM shared_entry WHERE {old}"))?
            .execute([content_hash, through])?;
    } else {
        transaction
            .prepare_cached(&format!(
                "DELETE FROM shared_entry WHERE {old} AND device_id <> {own_device}"
            ))?
            .execute([content_hash, through])?;
    }
    Ok(())
}

/// Compacts the entries of at most `limit` files of `entry_compaction_due`
/// whose time has come by `now_ms`, and returns whether more are due.
///
/// When the cut of a file is later than its floor and at least 7 days old,
/// this computer stores the cut as the file's floor in a `compacted` entry,
/// and then drops the entries at or before it. The `compacted` entry reaches
/// every other computer, which drops the same entries. An undo of the
/// deletion that arrives later can't bring them back on any computer: the cut
/// never goes below the floor, and the undo's record is the picture instead.
/// Without the floor, a computer that had dropped them would show the picture
/// differently from one that still had them.
///
/// The floor is stored only once the `delete` entry that sets the cut is in
/// the cloud folder: it came from another computer, or this computer has
/// written it there. Otherwise a restore or a lost disk could lose the
/// deletion while the floor is already in the cloud folder, and the picture
/// would be gone on every computer although the deletion never reached them.
///
/// This computer's own entries are dropped only once the `compacted` entry is
/// in the cloud folder. A sync writes the cloud files one at a time. If it
/// wrote a file without a dropped entry and stopped before it wrote the file
/// with the `compacted` entry, and this computer then lost its disk, the
/// other computers that never received the entry would show the picture
/// differently from those that did.
///
/// A file that waits for one of these two entries to be in the cloud folder
/// is looked at again at the next call after `now_ms`. A file whose cut is
/// less than 7 days old is looked at again 7 days after its cut. The `delete`
/// entries stay.
pub(super) fn compact_in(
    transaction: &rusqlite::Transaction<'_>,
    now_ms: i64,
    limit: usize,
) -> Result<bool> {
    use rusqlite::OptionalExtension;
    let device_id = own_device_id(transaction)?;
    let published = transaction
        .query_row(
            "SELECT hlc_physical_ms, hlc_logical FROM cloud_device_frontier WHERE device_id = ?1",
            [&device_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let in_cloud = |time: &EntryTime| {
        time.2 != device_id || published.is_some_and(|through| (time.0, time.1) <= through)
    };
    let due = transaction
        .prepare_cached(
            "SELECT content_hash FROM entry_compaction_due WHERE due_ms <= ?1
             ORDER BY due_ms LIMIT ?2",
        )?
        .query_map(rusqlite::params![now_ms, limit.min(i64::MAX as usize) as i64], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut time = None;
    for content_hash in &due {
        let entries = super::shared_merge::held_entries(transaction, content_hash)?;
        let Cut { cut, floor } = cut_of(&entries);
        if cut > floor && cut.0.saturating_add(COMPACT_AFTER_MS) > now_ms {
            transaction.execute(
                "UPDATE entry_compaction_due SET due_ms = ?2 WHERE content_hash = ?1",
                rusqlite::params![content_hash, cut.0.saturating_add(COMPACT_AFTER_MS)],
            )?;
            continue;
        }
        let deletion_in_cloud = entries.iter().all(|entry| match entry {
            HeldEntry::Delete { time, .. } => in_cloud(time),
            _ => true,
        });
        let wait = |transaction: &rusqlite::Transaction<'_>| {
            transaction.execute(
                "UPDATE entry_compaction_due SET due_ms = ?2 WHERE content_hash = ?1",
                rusqlite::params![content_hash, now_ms.saturating_add(1)],
            )
        };
        if cut > floor && !deletion_in_cloud {
            wait(transaction)?;
            continue;
        }
        let floor_in_cloud = if cut > floor {
            let (clock_ms, clock_counter) = match time {
                Some(time) => time,
                None => *time.insert(tick(transaction)?),
            };
            transaction
                .prepare_cached(&format!(
                    "INSERT INTO shared_entry
                         (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
                     SELECT ?1, 'compacted', '', ?2, ?3, ?4, device_id
                     FROM cloud_state WHERE singleton = 1
                     {KEEP_LATER}"
                ))?
                .execute(rusqlite::params![content_hash, time_text(&cut), clock_ms, clock_counter])?;
            false
        } else {
            entries.iter().all(|entry| match entry {
                HeldEntry::Compacted { stored, .. } => in_cloud(stored),
                _ => true,
            })
        };
        drop_below(transaction, content_hash, &time_text(&cut), floor_in_cloud)?;
        if floor_in_cloud {
            transaction.execute(
                "DELETE FROM entry_compaction_due WHERE content_hash = ?1",
                [content_hash],
            )?;
        } else {
            wait(transaction)?;
        }
    }
    Ok(due.len() == limit)
}

impl Library {
    /// Makes the library record entries from now on, and returns `true` when
    /// it didn't record before. The `add` entries of the pictures the library
    /// holds now are written afterwards by `Library::settle_entries`, all with
    /// the clock time of this call. A library that records already is left
    /// alone, which includes a library opened from a copy of one that records.
    pub fn start_recording(&self) -> Result<bool> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                if recording(transaction)? {
                    return Ok(false);
                }
                let (clock_ms, clock_counter) = tick(transaction)?;
                transaction.execute(
                    "INSERT INTO entry_recording
                         (singleton, started_ms, started_counter, start_next_root, start_end_root)
                     SELECT 1, ?1, ?2, 1, next_local_id FROM library_meta WHERE singleton = 1",
                    [clock_ms, clock_counter],
                )?;
                Ok(true)
            })
    }

    /// Returns whether the library records entries.
    pub fn records_entries(&self) -> Result<bool> {
        self.database
            .read(WorkPriority::Cloud, recording)
    }

    /// Turns this computer's pending changes into entries, at most `limit`
    /// roots per call, and returns whether work is left. Each call is one
    /// short write, so the library stays usable while the caller repeats it.
    pub fn settle_entries(&self, limit: usize) -> Result<bool> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                let snapshot = self.projections.snapshot();
                settle_in(transaction, &snapshot, limit)
            })
    }

    /// Returns this computer's id, the time of the library's clock, and the
    /// clock time up to which this computer's entries are in the cloud folder.
    pub fn entry_clock(&self) -> Result<EntryClock> {
        use rusqlite::OptionalExtension;
        self.database.read(WorkPriority::Cloud, |connection| {
            let (device_id, now) = connection.query_row(
                "SELECT device_id, hlc_physical_ms, hlc_logical FROM cloud_state
                 WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?))),
            )?;
            let published = connection
                .query_row(
                    "SELECT hlc_physical_ms, hlc_logical FROM cloud_device_frontier
                     WHERE device_id = ?1",
                    [&device_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            Ok(EntryClock { device_id, now, published })
        })
    }

    /// Calls `visit` with every stored entry that this computer wrote,
    /// ordered by key. With `since`, only entries later than that clock time
    /// are visited.
    pub fn visit_own_entries(
        &self,
        since: Option<(i64, i64)>,
        visit: &mut dyn FnMut(SharedEntry),
    ) -> Result<()> {
        let since = since.unwrap_or((-1, -1));
        self.database.read(WorkPriority::Cloud, |connection| {
            let mut statement = connection.prepare(
                "SELECT entry.kind, entry.subject_key, entry.second_key, entry.value,
                        entry.clock_ms, entry.clock_counter, entry.device_id
                 FROM shared_entry entry
                 WHERE entry.device_id = (SELECT device_id FROM cloud_state WHERE singleton = 1)
                   AND (entry.clock_ms, entry.clock_counter) > (?1, ?2)
                 ORDER BY entry.subject_key, entry.kind, entry.second_key",
            )?;
            let mut rows = statement.query([since.0, since.1])?;
            while let Some(row) = rows.next()? {
                let kind: String = row.get(0)?;
                let Some(kind) = EntryKind::parse(&kind) else {
                    continue;
                };
                visit(SharedEntry {
                    kind,
                    subject_key: row.get(1)?,
                    second_key: row.get(2)?,
                    value: row.get(3)?,
                    clock_ms: row.get(4)?,
                    clock_counter: row.get(5)?,
                    device_id: row.get(6)?,
                });
            }
            Ok(())
        })
    }

    /// Drops the entries that a deletion made obsolete at least 7 days before
    /// `now_ms`, in short writes (`compact_in`).
    pub fn compact_entries(&self, now_ms: i64) -> Result<()> {
        while self
            .database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                compact_in(transaction, now_ms, COMPACT_BATCH)
            })?
        {}
        Ok(())
    }

    /// Returns the keys of this computer's entries that compaction dropped
    /// and whose files in the cloud folder haven't been written again since.
    /// Each key is the kind, the subject key and the second key.
    pub fn removed_own_entries(&self) -> Result<Vec<(EntryKind, String, String)>> {
        self.database.read(WorkPriority::Cloud, |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT kind, subject_key, second_key FROM removed_own_entry",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .filter_map(|(kind, subject_key, second_key)| {
                    Some((EntryKind::parse(&kind)?, subject_key, second_key))
                })
                .collect())
        })
    }

    /// Forgets `keys` from `removed_own_entries`, once the files that held
    /// them have been written again.
    pub fn forget_removed_own_entries(&self, keys: &[(EntryKind, String, String)]) -> Result<()> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                let mut statement = transaction.prepare_cached(
                    "DELETE FROM removed_own_entry
                     WHERE subject_key = ?1 AND kind = ?2 AND second_key = ?3",
                )?;
                for (kind, subject_key, second_key) in keys {
                    statement.execute(rusqlite::params![subject_key, kind.as_str(), second_key])?;
                }
                Ok(())
            })
    }

    /// Records that this computer's entries up to the clock time `through`
    /// are in the cloud folder.
    pub fn set_entries_published(&self, through: (i64, i64)) -> Result<()> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                transaction.execute(
                    &format!(
                        "INSERT INTO cloud_device_frontier
                             (device_id, hlc_physical_ms, hlc_logical, updated_at)
                         SELECT device_id, ?1, ?2, {NOW_SQL} FROM cloud_state WHERE singleton = 1
                         ON CONFLICT(device_id) DO UPDATE SET
                             hlc_physical_ms = excluded.hlc_physical_ms,
                             hlc_logical = excluded.hlc_logical,
                             updated_at = excluded.updated_at"
                    ),
                    [through.0, through.1],
                )?;
                Ok(())
            })
    }
}
