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
//! because the hash or name is gone once the rows are deleted.
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

use super::shared_view::{EntryTime, FileRecord};
use super::*;

/// What an entry says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A computer holds the file. The value is the picture's record.
    Add,
    /// The file was deleted.
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
}

impl EntryKind {
    pub const ALL: [Self; 10] = [
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
    /// `status`, and `add` or `remove` for `item_tag` and `folder_item`.
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
    /// The record of the earliest `add` entry that still counts.
    pub record: FileRecord,
}

const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// The end of every statement that stores an entry. The new entry replaces
/// the stored one only when it is later, so storing the same entries again or
/// in another order gives the same table.
pub(super) const KEEP_LATER: &str = "ON CONFLICT(subject_key, kind, second_key) DO UPDATE SET
     value = excluded.value,
     clock_ms = excluded.clock_ms,
     clock_counter = excluded.clock_counter,
     device_id = excluded.device_id
 WHERE (excluded.clock_ms, excluded.clock_counter, excluded.device_id)
     > (shared_entry.clock_ms, shared_entry.clock_counter, shared_entry.device_id)";

/// Creates the temporary tables that a write fills for `record_changes` on
/// the connection that writes the library.
///
/// `entry_text_change` is filled by a trigger. Names and notes aren't in the
/// resident projection, so a trigger is the cheap way to learn which pictures
/// a write renamed or gave other notes. Missing notes and empty notes count
/// as the same notes.
///
/// `entry_restored_root` holds the pictures that an undo brought back
/// (`mark_restored_files`).
pub(crate) fn install_recording_triggers(connection: &rusqlite::Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS entry_restored_root (root_id INTEGER PRIMARY KEY);
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
         END;",
    )?;
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
    // An import adds a file only when no computer has added it yet, but an
    // undo of a deletion always adds it again. The value `always` marks the
    // rows of the second kind for `settle_in`.
    let restored = transaction
        .prepare_cached("SELECT root_id FROM temp.entry_restored_root")?
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    transaction.execute("DELETE FROM temp.entry_restored_root", [])?;
    let added = &files_after - &files_before;
    rows.push((EntryKind::Add, String::new(), Some("always".into()), &added & &restored));
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
    if rows.is_empty() {
        return Ok(());
    }
    let Some(time) = action_time(transaction, revision)? else {
        return Ok(());
    };
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
/// redo that brings deleted pictures back. Their `add` entries are written
/// even when another computer has added the file, as an undo of a deletion
/// should.
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
/// entry for each file that no other picture in the library still uses. A
/// deletion that came from another computer, or the undo of an import, stores
/// nothing.
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
            "INSERT INTO shared_entry
                 (subject_key, kind, second_key, value, clock_ms, clock_counter, device_id)
             SELECT DISTINCT file.content_hash, 'delete', '', NULL, ?1, ?2,
                    (SELECT device_id FROM cloud_state WHERE singleton = 1)
             FROM temp.delete_media selected
             CROSS JOIN media_item media ON media.media_id = selected.local_id
             JOIN media_file file ON file.file_id = media.file_id
             WHERE NOT EXISTS (
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
/// content hash was `old_hash`: the old file is deleted, and the picture gets
/// an `add` entry for the new file when it isn't in a collection.
pub(super) fn record_replaced_file(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &ProjectionSnapshot,
    old_hash: &str,
    media_id: MediaId,
) -> Result<()> {
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
    if file_roots(snapshot).contains(media_id.0) {
        write_pending(
            transaction,
            EntryKind::Add,
            "",
            Some("always"),
            time,
            &RoaringBitmap::from_iter([media_id.0]),
        )?;
    }
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

/// Returns those of `roots` whose file exists by an `add` entry of another
/// computer.
pub(super) fn roots_added_elsewhere(
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
             SELECT 1 FROM shared_entry added
             WHERE added.subject_key = file.content_hash AND added.kind = 'add'
               AND added.second_key <> (SELECT device_id FROM cloud_state WHERE singleton = 1)
               AND NOT EXISTS (
                   SELECT 1 FROM shared_entry deleted
                   WHERE deleted.subject_key = added.subject_key AND deleted.kind = 'delete'
                     AND (deleted.clock_ms, deleted.clock_counter, deleted.device_id)
                         >= (added.clock_ms, added.clock_counter, added.device_id)))",
    )?;
    let found = statement
        .query_map([serde_json::to_string(&roots.iter().collect::<Vec<_>>())?], |row| {
            row.get::<_, u32>(0)
        })?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    Ok(found)
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

/// Returns those of `hashes` whose file exists by the stored entries: an
/// `add` entry is later than the `delete` entry, or there's no `delete`.
pub(super) fn existing_files(
    connection: &rusqlite::Connection,
    hashes: &[&str],
) -> Result<HashSet<String>> {
    if hashes.is_empty() {
        return Ok(HashSet::new());
    }
    let mut statement = connection.prepare_cached(
        "SELECT DISTINCT hashes.value FROM json_each(?1) hashes
         CROSS JOIN shared_entry added
             ON added.subject_key = hashes.value AND added.kind = 'add'
         WHERE NOT EXISTS (
             SELECT 1 FROM shared_entry deleted
             WHERE deleted.subject_key = hashes.value AND deleted.kind = 'delete'
               AND deleted.second_key = ''
               AND (deleted.clock_ms, deleted.clock_counter, deleted.device_id)
                   >= (added.clock_ms, added.clock_counter, added.device_id))",
    )?;
    let existing = statement
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
        match EntryKind::parse(&kind) {
            Some(EntryKind::Add) => {
                // An import of a file that already exists by the entries
                // writes nothing. The picture is changed to look the way the
                // entries say instead, by the next merge. An undo or a
                // repair (`always`) writes its entry in any case.
                let files = file_records(transaction, snapshot, &taken, false)?;
                let existing = if value.as_deref() == Some("always") {
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
                write_adds(transaction, &new, time, &device_id)?;
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
            _ => roots.clear(),
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
            .read(WorkPriority::Cloud, |connection| recording(connection))
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
