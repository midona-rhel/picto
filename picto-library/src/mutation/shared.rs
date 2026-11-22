//! The entries that the devices of one library share through cloud sync.
//!
//! For every key that a shared change touched, the library keeps one entry:
//! what was done to the key last, the time of the library's hybrid logical
//! clock at which it was done, and the device that did it. A sync writes the
//! entries that this device made into the cloud folder and merges the entries
//! that are there ([`Library::apply_shared_entries`]).
//!
//! The entries are the rows of `shared_entry`.
//!
//! | Kind | Key | States |
//! | --- | --- | --- |
//! | `item` | stable key of an item or a collection | `add` with a value: the item was imported, and the value is its record ([`SharedItem`]): its file or, for a collection, its files, with their names and stored facts, and its name, notes, source links, rating, status, dates, tags and folders. `add` without a value: the item's tombstone was removed, by a subscription or by the undo of a deletion in a library that records no records. `delete`: deleted permanently; the value is `{"file": "<content hash>"}` for an item with a file, and there is none for a collection. |
//! | `status` | stable key of an item or a collection | `trash`, `active`, `inbox`: where the last move into or out of the Trash put it. |
//! | `tag`, `folder`, `smart_folder` | stable key | `delete`, and `add` when the deletion was undone. |
//!
//! One rule merges everything: per key, the entry with the later clock time
//! wins, and of two entries with the same time the one of the device with
//! the greater id. An entry that does not win changes nothing. So the same
//! entries give the same library in every order and however often they are
//! merged.
//!
//! The clock ([`tick`]) is the later of the device's wall clock and the
//! latest time the library has seen, and merging an entry moves it forward
//! to that entry's time. What the user does after a merge is therefore later
//! than everything that was merged, whatever the device's wall clock says.
//!
//! # Which library records what
//!
//! Every library records the entries of deletions, because a deletion
//! leaves a tombstone that an import respects. A library records the record
//! of an imported item only once it has synced: from the first time it is
//! connected to a cloud folder, or from the moment it was made from the
//! cloud data, and from then on also while its sync is paused or switched
//! off. The mark for that is the device's own row in
//! `cloud_device_frontier` ([`records_items`]). A library that never synced
//! has no such row. The items that a library holds when it first syncs
//! reach the other devices in its snapshot, and no entry is written for
//! them.
//!
//! # Merged additions
//!
//! A merged `add` with a record does nothing when the library holds an item
//! with the entry's key. When the library holds the record's file under
//! another key, the entry's key becomes an alias of that item
//! (`library_item_alias`), because one file is one item. Otherwise a job is
//! put into the ingest queue (`ingest_job`, source kind `cloud`), not
//! prepared. The cloud worker copies the job's files from the cloud folder
//! into the library and marks the job prepared, and the ingest worker
//! imports it ([`Library::ingest_shared`]). An entry of another kind that
//! names an item which is not here yet is stored, and the import reads it.
//! An import that was merged records no entry of its own.
//!
//! # Merged deletions
//!
//! A merged `delete` deletes the item through the existing delete code.
//! Three things are left alone. An item whose file has another content hash
//! than the entry names is another item under the same stable key: before
//! stable keys of manual imports held the device's id, two devices could
//! give the same key to different files. Such an item stays, as if the
//! library did not hold an item with that key. An item of which the library
//! holds no entry yet, and which was imported here after the deletion was
//! made, was added again: it stays and gets an `add` entry. That is the rule
//! of an import, which removes a tombstone that is earlier than the import.
//! A folder or smart folder with a folder below it that has no `delete`
//! entry stays until that folder is deleted too, so a merge deletes no
//! folder that the other device did not delete.

use serde::{Deserialize, Serialize};

use super::*;

/// What kind of thing a shared entry is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Item,
    Status,
    Tag,
    Folder,
    SmartFolder,
}

impl EntryKind {
    /// The value of `shared_entry.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Status => "status",
            Self::Tag => "tag",
            Self::Folder => "folder",
            Self::SmartFolder => "smart_folder",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Item, Self::Status, Self::Tag, Self::Folder, Self::SmartFolder]
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }
}

/// What was done last to the key of a shared entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// Deleted.
    Delete,
    /// Added: the item was imported, or the deletion of the key was taken
    /// back.
    Add,
    /// Moved to the Trash.
    Trash,
    /// Taken out of the Trash into All.
    Active,
    /// Taken out of the Trash into the Inbox.
    Inbox,
}

impl EntryState {
    /// The value of `shared_entry.state`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Add => "add",
            Self::Trash => "trash",
            Self::Active => "active",
            Self::Inbox => "inbox",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Delete, Self::Add, Self::Trash, Self::Active, Self::Inbox]
            .into_iter()
            .find(|state| state.as_str() == text)
    }

    fn lifecycle(self) -> Option<Lifecycle> {
        match self {
            Self::Trash => Some(Lifecycle::Trash),
            Self::Active => Some(Lifecycle::Active),
            Self::Inbox => Some(Lifecycle::Inbox),
            Self::Delete | Self::Add => None,
        }
    }
}

/// One shared entry, as it is written to the cloud folder. The device that
/// made it is the device whose file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedEntry {
    pub kind: EntryKind,
    pub key: String,
    pub state: EntryState,
    /// The time of the hybrid logical clock: milliseconds, and a counter
    /// that orders the entries of one millisecond.
    pub at_ms: i64,
    pub logical: i64,
    /// What the state carries. An `add` entry of the kind `item` carries the
    /// record of the item ([`SharedItem`]). A `delete` entry of the kind
    /// `item` carries `{"file": "<content hash>"}` for an item that has a
    /// file, and nothing for a collection: a device that merges the entry
    /// deletes its item with the same stable key only when that item's file
    /// has this hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

impl SharedEntry {
    /// The content hash that a `delete` entry names.
    fn deleted_file(&self) -> Option<&str> {
        (self.state == EntryState::Delete)
            .then(|| self.value.as_ref()?.get("file")?.as_str())
            .flatten()
    }

    /// The record of the item that an `add` entry of the kind `item`
    /// carries. An entry without a value, or with a value that this version
    /// cannot read, has none.
    fn record(&self) -> Option<SharedItem> {
        if self.kind != EntryKind::Item || self.state != EntryState::Add {
            return None;
        }
        serde_json::from_value(self.value.clone()?).ok()
    }
}

/// The record of an imported item or collection, as an `add` entry of the
/// kind `item` carries it. It holds what another device needs to import the
/// same item: the files with the facts that were stored for them, and what
/// the item was given at its import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    pub status: Lifecycle,
    pub rating: Rating,
    pub imported_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<SharedName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<SharedName>,
    /// The file of an item, or the files of a collection in their order.
    pub files: Vec<SharedFile>,
    /// For a collection, the index of its cover among `files`. An item that
    /// is one file has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<usize>,
}

/// A tag or a folder that a record names: its stable key, and its name, with
/// which a device that does not hold it creates it. The name of a tag is its
/// whole name with its namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedName {
    pub key: String,
    pub name: String,
}

/// One file of a record: the stable key of its media item, its name and
/// note, the facts that were stored for the file, and the source it was
/// downloaded from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedFile {
    pub key: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub facts: crate::model::ImmutableMediaFacts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SharedSource>,
}

/// The source of a file, as `source_provenance` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedSource {
    pub source_key: String,
    pub source_item_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
}

/// An item of another device that waits in the ingest queue: the key of its
/// entry, its record, and, once the cloud worker has copied them into the
/// library, the paths of its files in the order of the record's files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedImport {
    pub key: String,
    pub record: SharedItem,
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The state of this library's exchange of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryClock {
    /// This device's id.
    pub device_id: String,
    /// The time of the library's hybrid logical clock.
    pub now: (i64, i64),
    /// The clock time up to which this device's entries are in the cloud
    /// folder, or `None` when none were written yet.
    pub published: Option<(i64, i64)>,
}

/// What merging the entries of another device changed in this library.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppliedEntries {
    /// The number of entries that were later than what the library held.
    pub won: u64,
    pub trashed: u64,
    pub restored: u64,
    pub deleted: u64,
    pub tags: u64,
    pub folders: u64,
    pub smart_folders: u64,
    /// The number of items and collections that were put into the ingest
    /// queue.
    pub queued: u64,
    /// The number of keys that became an alias of an item with the same
    /// file.
    pub aliased: u64,
    /// `None` when no entry won, and nothing was written.
    pub receipt: Option<MutationReceipt>,
}

const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// The end of an `INSERT INTO shared_entry` that replaces the entry which the
/// library holds for the same key.
const UPSERT: &str = "ON CONFLICT(kind, key) DO UPDATE SET
     state = excluded.state,
     value = excluded.value,
     clock_ms = excluded.clock_ms,
     clock_counter = excluded.clock_counter,
     device_id = excluded.device_id";

/// The source kind of the ingest jobs that import the items of other
/// devices.
pub const SHARED_JOB_KIND: &str = "cloud";

/// The key of the ingest job that imports the item with the stable key
/// `key`.
fn shared_job_key(key: &str) -> String {
    format!("cloud:{key}")
}

/// Advances the library's hybrid logical clock and returns its new time. The
/// time is later than every time the clock returned or saw before, also when
/// the device's wall clock was set back.
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

/// Moves the library's clock forward to `time` when `time` is later, so that
/// the next [`tick`] is later than an entry of that time.
fn observe(transaction: &rusqlite::Transaction<'_>, time: (i64, i64)) -> Result<()> {
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

fn own_device_id(transaction: &rusqlite::Transaction<'_>) -> Result<String> {
    Ok(transaction.query_row(
        "SELECT device_id FROM cloud_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?)
}

/// Records that this device put `keys` of `kind` into `state` now. The
/// entries get one new clock time, which is later than every entry the
/// library holds, so they replace what was stored for the keys.
pub(super) fn record_entries(
    transaction: &rusqlite::Transaction<'_>,
    kind: EntryKind,
    state: EntryState,
    keys: &[String],
) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let (at_ms, logical) = tick(transaction)?;
    transaction.execute(
        // `WHERE true` ends the SELECT: after `FROM json_each(…)` SQLite
        // would otherwise read `ON` as a join constraint.
        &format!(
            "INSERT INTO shared_entry
                 (kind, key, state, value, clock_ms, clock_counter, device_id)
             SELECT ?1, keys.value, ?2, NULL, ?3, ?4,
                    (SELECT device_id FROM cloud_state WHERE singleton = 1)
             FROM json_each(?5) keys WHERE true
             {UPSERT}"
        ),
        rusqlite::params![
            kind.as_str(),
            state.as_str(),
            at_ms,
            logical,
            serde_json::to_string(keys)?
        ],
    )?;
    Ok(())
}

/// Records that the deletion of `keys` of `kind` was taken back now: every
/// `delete` entry among them becomes an `add` entry with a new clock time. A
/// key without a `delete` entry was never shared as deleted and gets none.
pub(super) fn record_added_again(
    transaction: &rusqlite::Transaction<'_>,
    kind: EntryKind,
    keys: &[String],
) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let (at_ms, logical) = tick(transaction)?;
    transaction.execute(
        "UPDATE shared_entry SET
             state = 'add', value = NULL, clock_ms = ?2, clock_counter = ?3,
             device_id = (SELECT device_id FROM cloud_state WHERE singleton = 1)
         WHERE kind = ?1 AND state = 'delete'
           AND key IN (SELECT value FROM json_each(?4))",
        rusqlite::params![kind.as_str(), at_ms, logical, serde_json::to_string(keys)?],
    )?;
    Ok(())
}

/// Removes the tombstones of the items `keys`, inside the caller's
/// transaction, and records the removal for the other devices
/// ([`record_added_again`]). An import of a deleted item, a subscription
/// that brings an item again, a reset of a subscription and the undo of a
/// permanent deletion all remove tombstones through this function. Returns
/// the number of tombstones that were removed.
pub fn remove_item_tombstones(
    transaction: &rusqlite::Transaction<'_>,
    keys: &[String],
) -> Result<usize> {
    if keys.is_empty() {
        return Ok(0);
    }
    let removed = transaction.execute(
        "DELETE FROM deletion_tombstone
         WHERE stable_key IN (SELECT value FROM json_each(?1))",
        [serde_json::to_string(keys)?],
    )?;
    // A subscription run calls this for every item it records. Most of them
    // have no tombstone, and then nothing is recorded and the clock stays.
    if removed != 0 {
        record_added_again(transaction, EntryKind::Item, keys)?;
    }
    Ok(removed)
}

/// Called by `delete_roots` while the ids of the roots and of their media
/// are staged in `temp.delete_root` and `temp.delete_item` and their rows
/// still exist. For a deletion that the user made it records a `delete`
/// entry for every item that gets a tombstone, with the content hash of the
/// item's file when the item has one. For every deletion it drops
/// the `status` entries of the deleted roots, which no longer mean anything.
pub(super) fn record_deleted_roots(
    transaction: &rusqlite::Transaction<'_>,
    origin: DeletionOrigin,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM shared_entry
         WHERE kind = 'status' AND key IN (
             SELECT item.stable_key FROM temp.delete_root selected
             CROSS JOIN library_item item ON item.local_id = selected.local_id)",
        [],
    )?;
    if origin != DeletionOrigin::User {
        return Ok(());
    }
    let (at_ms, logical) = tick(transaction)?;
    // An item is deleted under its stable key and under every alias it has.
    // A device that holds the same file under one of the aliases deletes it
    // by that entry.
    for names in ["library_item", "library_item_alias"] {
        transaction.execute(
            &format!(
                "INSERT INTO shared_entry
                     (kind, key, state, value, clock_ms, clock_counter, device_id)
                 SELECT 'item', named.stable_key, 'delete',
                        CASE WHEN file.content_hash IS NULL THEN NULL
                             ELSE json_object('file', file.content_hash) END,
                        ?1, ?2, (SELECT device_id FROM cloud_state WHERE singleton = 1)
                 FROM temp.delete_item selected
                 CROSS JOIN {names} named ON named.local_id = selected.local_id
                 LEFT JOIN media_item media ON media.media_id = selected.local_id
                 LEFT JOIN media_file file ON file.file_id = media.file_id
                 WHERE true
                 {UPSERT}"
            ),
            [at_ms, logical],
        )?;
    }
    Ok(())
}

/// Called inside the transaction that connects the library to a cloud
/// folder, makes it from the cloud data, or replaces its database by a
/// snapshot. From then on the library records the items that it imports
/// ([`records_items`]). Its next sync writes all of this device's entries
/// into the cloud folder and merges every entry file that is there, because
/// what the library wrote and merged before belongs to the cloud data or to
/// the database that it had before.
pub fn reset_published_entries(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM cloud_device_frontier", [])?;
    transaction.execute(
        &format!(
            "INSERT INTO cloud_device_frontier
                 (device_id, hlc_physical_ms, hlc_logical, updated_at)
             SELECT device_id, 0, 0, {NOW_SQL} FROM cloud_state WHERE singleton = 1"
        ),
        [],
    )?;
    transaction.execute("DELETE FROM merged_entry_file", [])?;
    Ok(())
}

/// Returns whether the library records the items that it imports: whether it
/// has synced before. The mark is the device's own row in
/// `cloud_device_frontier`, which [`reset_published_entries`] writes.
pub fn records_items(connection: &rusqlite::Connection) -> Result<bool> {
    Ok(connection
        .prepare_cached(
            "SELECT EXISTS (
                 SELECT 1 FROM cloud_device_frontier frontier
                 JOIN cloud_state state ON state.device_id = frontier.device_id)",
        )?
        .query_row([], |row| row.get(0))?)
}

/// Returns the record of the root `root_id`, with its stable key, as the
/// library holds the root now. `tags` are the root's tags when the caller
/// knows them; without them every tag of the library is asked whether it
/// holds the root.
fn item_record(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    root_id: RootId,
    tags: Option<&[crate::TagId]>,
) -> Result<Option<(String, SharedItem)>> {
    use rusqlite::OptionalExtension;
    let Some((key, name, notes, urls, imported_at_ms, captured_at_ms, cover)) = transaction
        .prepare_cached(
            "SELECT item.stable_key, root.name, root.notes, root.source_urls_json,
                    root.imported_at_ms, root.captured_at_ms, root.cover_media_id
             FROM library_root root
             JOIN library_item item ON item.local_id = root.root_id
             WHERE root.root_id = ?1",
        )?
        .query_row([root_id.0], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, u32>(6)?,
            ))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let members = snapshot.collection_orders.get(&root_id);
    let media_ids = match members {
        Some(members) => members.iter().map(|media| media.0).collect::<Vec<_>>(),
        None => vec![root_id.0],
    };
    let mut files = Vec::with_capacity(media_ids.len());
    for media_id in &media_ids {
        let mut statement = transaction.prepare_cached(
            "SELECT item.stable_key, media.media_name, media.media_notes,
                    file.content_hash, file.mime, file.size_bytes, file.width, file.height,
                    file.duration_ms, file.frame_count, file.perceptual_hash, file.palette_json
             FROM media_item media
             JOIN library_item item ON item.local_id = media.media_id
             JOIN media_file file ON file.file_id = media.file_id
             WHERE media.media_id = ?1",
        )?;
        let (mut file, palette) = statement.query_row([media_id], |row| {
            Ok((
                SharedFile {
                    key: row.get(0)?,
                    name: row.get(1)?,
                    notes: row.get(2)?,
                    facts: crate::model::ImmutableMediaFacts {
                        content_hash: row.get(3)?,
                        mime: row.get(4)?,
                        size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
                        width: row.get(6)?,
                        height: row.get(7)?,
                        duration_ms: row.get::<_, Option<i64>>(8)?.map(|value| value.max(0) as u64),
                        frame_count: row.get(9)?,
                        perceptual_hash: row.get(10)?,
                        palette: Vec::new(),
                    },
                    source: None,
                },
                row.get::<_, String>(11)?,
            ))
        })?;
        file.facts.palette = serde_json::from_str(&palette)?;
        file.source = transaction
            .prepare_cached(
                "SELECT source_key, source_item_key, source_text FROM source_provenance
                 WHERE media_id = ?1 ORDER BY source_key, source_item_key LIMIT 1",
            )?
            .query_row([media_id], |row| {
                Ok(SharedSource {
                    source_key: row.get(0)?,
                    source_item_key: row.get(1)?,
                    source_text: row.get(2)?,
                })
            })
            .optional()?;
        files.push(file);
    }
    let tag_ids = match tags {
        Some(tags) => tags.to_vec(),
        None => {
            let mut held = snapshot
                .tags
                .iter()
                .filter(|(_, roots)| roots.contains(root_id.0))
                .map(|(tag, _)| *tag)
                .collect::<Vec<_>>();
            held.sort_unstable_by_key(|tag| tag.0);
            held
        }
    };
    let mut tags = Vec::with_capacity(tag_ids.len());
    for tag_id in tag_ids {
        let named = transaction
            .prepare_cached(
                "SELECT tag.stable_key, namespace.display_name, tag.subname
                 FROM tag_definition tag
                 JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
                 WHERE tag.tag_id = ?1",
            )?
            .query_row([tag_id.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .optional()?;
        if let Some((key, namespace, subname)) = named {
            tags.push(SharedName {
                key,
                name: whole_tag_name(&namespace, &subname),
            });
        }
    }
    let mut folder_ids = snapshot
        .folders
        .iter()
        .filter(|(_, roots)| roots.contains(root_id.0))
        .map(|(folder, _)| *folder)
        .collect::<Vec<_>>();
    folder_ids.sort_unstable_by_key(|folder| folder.0);
    let mut folders = Vec::with_capacity(folder_ids.len());
    for folder_id in folder_ids {
        let named = transaction
            .prepare_cached("SELECT stable_key, name FROM folder_definition WHERE folder_id = ?1")?
            .query_row([folder_id.0], |row| {
                Ok(SharedName {
                    key: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .optional()?;
        folders.extend(named);
    }
    let status = Lifecycle::ALL
        .into_iter()
        .find(|value| snapshot.lifecycle(*value).contains(root_id.0))
        .unwrap_or(Lifecycle::Active);
    let rating = Rating::ALL
        .into_iter()
        .find(|value| snapshot.rating(*value).contains(root_id.0))
        .unwrap_or(Rating::Unrated);
    Ok(Some((
        key,
        SharedItem {
            name,
            notes: notes.filter(|notes| !notes.is_empty()),
            urls: serde_json::from_str(&urls)?,
            status,
            rating,
            imported_at_ms,
            captured_at_ms,
            tags,
            folders,
            cover: members
                .map(|_| media_ids.iter().position(|media| *media == cover).unwrap_or(0)),
            files,
        },
    )))
}

/// The whole name of a tag, as an import names it: the name alone for a tag
/// of the namespace without a name, and otherwise the namespace, a colon and
/// the name.
fn whole_tag_name(namespace: &str, subname: &str) -> String {
    if namespace.is_empty() {
        subname.to_owned()
    } else {
        format!("{namespace}:{subname}")
    }
}

/// Records that this device imported `roots` now, for a library that records
/// items ([`records_items`]): every root gets an `add` entry with its record.
/// With a root come its tags when the caller knows them ([`item_record`]).
/// `snapshot` holds the roots as the import leaves them.
pub(super) fn record_imported_roots(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    roots: &[(RootId, Option<Vec<crate::TagId>>)],
) -> Result<()> {
    if roots.is_empty() || !records_items(transaction)? {
        return Ok(());
    }
    let (at_ms, logical) = tick(transaction)?;
    for (root_id, tags) in roots {
        let Some((key, record)) = item_record(transaction, snapshot, *root_id, tags.as_deref())?
        else {
            continue;
        };
        transaction
            .prepare_cached(&format!(
                "INSERT INTO shared_entry
                     (kind, key, state, value, clock_ms, clock_counter, device_id)
                 VALUES ('item', ?1, 'add', ?2, ?3, ?4,
                         (SELECT device_id FROM cloud_state WHERE singleton = 1))
                 {UPSERT}"
            ))?
            .execute(rusqlite::params![
                key,
                serde_json::to_string(&record)?,
                at_ms,
                logical
            ])?;
    }
    Ok(())
}

/// Returns the clock time and the device of the `item` entry that the
/// library holds for each of `keys`.
fn held_item_times(
    transaction: &rusqlite::Transaction<'_>,
    keys: &[&str],
) -> Result<HashMap<String, (i64, i64, String)>> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mut statement = transaction.prepare_cached(
        "SELECT stored.key, stored.clock_ms, stored.clock_counter, stored.device_id
         FROM json_each(?1) keys
         CROSS JOIN shared_entry stored ON stored.kind = 'item' AND stored.key = keys.value",
    )?;
    let rows = statement.query_map([serde_json::to_string(keys)?], |row| {
        Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Returns the status that the library holds a `status` entry of for each of
/// `keys`, when that entry is later than the `item` entry of the same key.
/// The import of an item of another device gives the item that status,
/// because it is later than the status in the item's record.
pub(super) fn held_statuses(
    transaction: &rusqlite::Transaction<'_>,
    keys: &[&str],
) -> Result<HashMap<String, Lifecycle>> {
    let mut statement = transaction.prepare_cached(
        "SELECT stored.key, stored.state FROM json_each(?1) keys
         CROSS JOIN shared_entry stored ON stored.kind = 'status' AND stored.key = keys.value
         WHERE NOT EXISTS (
             SELECT 1 FROM shared_entry item
             WHERE item.kind = 'item' AND item.key = stored.key
               AND (item.clock_ms, item.clock_counter, item.device_id)
                   >= (stored.clock_ms, stored.clock_counter, stored.device_id))",
    )?;
    let rows = statement
        .query_map([serde_json::to_string(keys)?], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(key, state)| Some((key, EntryState::parse(&state)?.lifecycle()?)))
        .collect())
}

/// Turns the roots of pending `root.trash` journal rows into `status`
/// entries of this device, at most `limit` roots, and returns whether rows
/// are left. A row holds the roots that one change moved into the Trash or
/// out of it, and the clock time of the change. The rows are written by the
/// change itself, which stays as fast as it was for any number of roots;
/// the entries are written here, on the cloud queue. An entry is not stored
/// over a later one.
fn settle_trash_moves_in(transaction: &rusqlite::Transaction<'_>, limit: usize) -> Result<bool> {
    use rusqlite::OptionalExtension;
    let device_id = own_device_id(transaction)?;
    let mut room = limit;
    loop {
        let row = transaction
            .prepare_cached(
                "SELECT journal_id, target_bitmap, payload_json, created_at_ms
                 FROM cloud_journal WHERE operation_kind = 'root.trash'
                 ORDER BY journal_id LIMIT 1",
            )?
            .query_row([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .optional()?;
        let Some((journal_id, targets, payload, created_at_ms)) = row else {
            return Ok(false);
        };
        if room == 0 {
            return Ok(true);
        }
        let payload: serde_json::Value = serde_json::from_str(&payload)?;
        let state = match payload["lifecycle"].as_u64() {
            Some(key) if key == u64::from(Lifecycle::Trash.bitmap_key()) => EntryState::Trash,
            Some(key) if key == u64::from(Lifecycle::Inbox.bitmap_key()) => EntryState::Inbox,
            _ => EntryState::Active,
        };
        let at_ms = payload["at_ms"].as_i64().unwrap_or(created_at_ms);
        let logical = payload["logical"].as_i64().unwrap_or(0);
        let mut roots = targets
            .map(|bytes| RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(bytes)))
            .transpose()?
            .unwrap_or_default();
        let taken = roots.iter().take(room).collect::<Vec<_>>();
        room -= taken.len();
        transaction.execute(
            "INSERT INTO shared_entry
                 (kind, key, state, value, clock_ms, clock_counter, device_id)
             SELECT 'status', item.stable_key, ?2, NULL, ?3, ?4, ?5
             FROM json_each(?1) ids
             CROSS JOIN library_item item ON item.local_id = ids.value
             JOIN library_root root ON root.root_id = item.local_id
             WHERE true
             ON CONFLICT(kind, key) DO UPDATE SET
                 state = excluded.state,
                 value = excluded.value,
                 clock_ms = excluded.clock_ms,
                 clock_counter = excluded.clock_counter,
                 device_id = excluded.device_id
             WHERE (excluded.clock_ms, excluded.clock_counter, excluded.device_id)
                 > (shared_entry.clock_ms, shared_entry.clock_counter,
                    shared_entry.device_id)",
            rusqlite::params![
                serde_json::to_string(&taken)?,
                state.as_str(),
                at_ms,
                logical,
                device_id
            ],
        )?;
        for root in &taken {
            roots.remove(*root);
        }
        if roots.is_empty() {
            transaction.execute("DELETE FROM cloud_journal WHERE journal_id = ?1", [journal_id])?;
        } else {
            transaction.execute(
                "UPDATE cloud_journal SET target_bitmap = ?2 WHERE journal_id = ?1",
                rusqlite::params![journal_id, crate::bitmap::encode(&roots)?],
            )?;
        }
    }
}

impl Library {
    /// Writes the `status` entries of the moves into and out of the Trash
    /// that this device made since the last call, for at most `limit` roots
    /// ([`settle_trash_moves_in`]). Returns whether moves are left.
    pub fn settle_trash_moves(&self, limit: usize) -> Result<bool> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                settle_trash_moves_in(transaction, limit)
            })
    }

    /// Returns this device's id, the time of the library's clock, and the
    /// clock time up to which this device's entries are in the cloud folder.
    pub fn entry_clock(&self) -> Result<EntryClock> {
        self.database.read(WorkPriority::Cloud, |connection| {
            use rusqlite::OptionalExtension;
            let (device_id, now) = connection.query_row(
                "SELECT device_id, hlc_physical_ms, hlc_logical
                 FROM cloud_state WHERE singleton = 1",
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
            Ok(EntryClock {
                device_id,
                now,
                published,
            })
        })
    }

    /// Calls `visit` with every entry that this device made, in the order of
    /// the kinds and keys. With `later_than`, only with the entries whose
    /// clock time is later than that time.
    pub fn visit_own_entries(
        &self,
        later_than: Option<(i64, i64)>,
        visit: &mut dyn FnMut(SharedEntry),
    ) -> Result<()> {
        self.database.read(WorkPriority::Cloud, |connection| {
            let (at_ms, logical) = later_than.unwrap_or((i64::MIN, 0));
            let mut statement = connection.prepare(
                "SELECT kind, key, state, clock_ms, clock_counter, value
                 FROM shared_entry
                 WHERE device_id = (SELECT device_id FROM cloud_state WHERE singleton = 1)
                   AND (clock_ms, clock_counter) > (?1, ?2)
                 ORDER BY kind, key",
            )?;
            let mut rows = statement.query([at_ms, logical])?;
            while let Some(row) = rows.next()? {
                let kind = EntryKind::parse(&row.get::<_, String>(0)?);
                let state = EntryState::parse(&row.get::<_, String>(2)?);
                if let (Some(kind), Some(state)) = (kind, state) {
                    visit(SharedEntry {
                        kind,
                        key: row.get(1)?,
                        state,
                        at_ms: row.get(3)?,
                        logical: row.get(4)?,
                        value: row
                            .get::<_, Option<String>>(5)?
                            .map(|value| serde_json::from_str(&value))
                            .transpose()?,
                    });
                }
            }
            Ok(())
        })
    }

    /// Records that this device's entries up to the clock time `through` are
    /// in the cloud folder.
    pub fn set_entries_published(&self, through: (i64, i64)) -> Result<()> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                transaction.execute(
                    &format!(
                        "INSERT INTO cloud_device_frontier
                             (device_id, hlc_physical_ms, hlc_logical, updated_at)
                         VALUES ((SELECT device_id FROM cloud_state WHERE singleton = 1),
                                 ?1, ?2, {NOW_SQL})
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

    /// Merges `entries`, which the device `device_id` made, into this
    /// library, in one publication. Nothing is recorded in the undo history.
    ///
    /// An entry wins when the library holds no entry for its key, or an
    /// earlier one, or one of the same time from a device with a smaller id.
    /// An entry that does not win changes nothing. An entry that wins is
    /// stored, and:
    ///
    /// - `item`, `delete`: the item gets its tombstone. If it is an item or
    ///   a collection of this library, it is deleted, whatever its status
    ///   is. An entry that names the content hash of a file deletes the
    ///   item only when the item's file here has that hash. An item with
    ///   another file is another item under the same stable key: it stays,
    ///   and the entry is handled as if the library held no item with that
    ///   key. One more case is different: the library holds no entry for the item,
    ///   and the item was imported here after the time of the deletion. Then
    ///   the item stays, gets no tombstone, and gets an `add` entry of this
    ///   device.
    /// - `item`, `add`: the item's tombstone is removed. An entry with a
    ///   record does nothing more when the library holds an item with the
    ///   entry's key, or, for a collection, items with the keys of all its
    ///   files. When the library holds the record's one file under another
    ///   key, the entry's key becomes an alias of that item. Otherwise the
    ///   item is put into the ingest queue.
    /// - `status`: an item of this library is moved to the Trash, or, when
    ///   it is in the Trash, out of it. An entry for an item that this
    ///   library does not hold is stored, and the import of the item reads
    ///   it. An entry that is earlier than the `item` entry of the same key
    ///   is dropped.
    /// - `tag`, `delete`: the tag is deleted.
    /// - `folder` and `smart_folder`, `delete`: every folder that has a
    ///   `delete` entry is deleted once every folder below it has one too.
    ///   The items in a deleted folder stay in the library.
    ///
    /// The moves into and out of the Trash that this device made and has not
    /// turned into entries yet are settled first, so that an earlier entry
    /// of another device does not win over them.
    pub fn apply_shared_entries(
        &self,
        device_id: &str,
        entries: &[SharedEntry],
    ) -> Result<AppliedEntries> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let protected = self.history.protected_cleanup_files();
        let published = self.database.published_write_if_changed(
            WorkPriority::Cloud,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                settle_trash_moves_in(transaction, usize::MAX)?;
                let held = held_entries(transaction, entries)?;
                let mut winners = entries
                    .iter()
                    .filter(|entry| {
                        held.get(&(entry.kind, entry.key.as_str())).is_none_or(|held| {
                            (entry.at_ms, entry.logical, device_id)
                                > (held.0, held.1, held.2.as_str())
                        })
                    })
                    .collect::<Vec<_>>();
                if winners.is_empty() {
                    return Ok(None);
                }
                if let Some(latest) = winners.iter().map(|e| (e.at_ms, e.logical)).max() {
                    observe(transaction, latest)?;
                }
                let mut next = (*snapshot).clone();
                let mut applied = AppliedEntries::default();
                let mut resources = BTreeSet::from(["cloud".to_string()]);
                let mut affected = RoaringBitmap::new();
                let keys_of = |winners: &[&SharedEntry], kind: EntryKind, state: EntryState| {
                    winners
                        .iter()
                        .filter(|entry| entry.kind == kind && entry.state == state)
                        .map(|entry| entry.key.clone())
                        .collect::<Vec<_>>()
                };

                // The roots of this library that the item and status
                // entries name.
                let root_keys = winners
                    .iter()
                    .filter(|entry| matches!(entry.kind, EntryKind::Item | EntryKind::Status))
                    .map(|entry| entry.key.as_str())
                    .collect::<BTreeSet<_>>();
                let roots = local_roots(transaction, &root_keys)?;
                // The keys under which this library holds an item with
                // another file than a `delete` entry names. Such an item is
                // not the deleted one, and the entry does not touch it.
                let other_file = winners
                    .iter()
                    .filter(|entry| entry.kind == EntryKind::Item)
                    .filter(|entry| {
                        entry.deleted_file().is_some_and(|named| {
                            roots
                                .get(entry.key.as_str())
                                .is_some_and(|(_, _, held)| held.as_deref() != Some(named))
                        })
                    })
                    .map(|entry| entry.key.as_str())
                    .collect::<HashSet<_>>();

                // An item of which the library holds no entry, and which was
                // imported here after it was deleted elsewhere, was added
                // again. It stays. When the library holds an entry for the
                // item, the clock times alone decide.
                let mut added_again = Vec::new();
                winners.retain(|entry| {
                    let later_import = entry.kind == EntryKind::Item
                        && entry.state == EntryState::Delete
                        && !held.contains_key(&(EntryKind::Item, entry.key.as_str()))
                        && !other_file.contains(entry.key.as_str())
                        && roots
                            .get(entry.key.as_str())
                            .is_some_and(|(_, imported_at_ms, _)| *imported_at_ms > entry.at_ms);
                    if later_import {
                        added_again.push(entry.key.clone());
                    }
                    !later_import
                });
                // A status entry that is earlier than the entry of its item
                // says where the item was before it was deleted or imported
                // again, and is dropped.
                let status_keys = winners
                    .iter()
                    .filter(|entry| entry.kind == EntryKind::Status)
                    .map(|entry| entry.key.as_str())
                    .collect::<Vec<_>>();
                let mut item_times = held_item_times(transaction, &status_keys)?;
                for entry in winners.iter().filter(|entry| entry.kind == EntryKind::Item) {
                    let time = (entry.at_ms, entry.logical, device_id.to_owned());
                    match item_times.get_mut(&entry.key) {
                        Some(held) if *held >= time => {}
                        Some(held) => *held = time,
                        None => {
                            item_times.insert(entry.key.clone(), time);
                        }
                    }
                }
                winners.retain(|entry| {
                    entry.kind != EntryKind::Status
                        || item_times.get(&entry.key).is_none_or(|item| {
                            (entry.at_ms, entry.logical, device_id)
                                > (item.0, item.1, item.2.as_str())
                        })
                });
                if winners.is_empty() && added_again.is_empty() {
                    return Ok(None);
                }

                // Moves into and out of the Trash.
                let lifecycle_keys = Lifecycle::ALL
                    .iter()
                    .map(|value| value.bitmap_key())
                    .collect::<Vec<_>>();
                for destination in Lifecycle::ALL {
                    let mut moved = winners
                        .iter()
                        .filter(|entry| {
                            entry.kind == EntryKind::Status
                                && entry.state.lifecycle() == Some(destination)
                        })
                        .filter_map(|entry| roots.get(entry.key.as_str()).map(|(root, _, _)| *root))
                        .collect::<RoaringBitmap>();
                    if destination == Lifecycle::Trash {
                        moved -= next.lifecycle(Lifecycle::Trash);
                    } else {
                        // A restore moves only an item that is in the Trash.
                        moved &= next.lifecycle(Lifecycle::Trash);
                    }
                    if moved.is_empty() {
                        continue;
                    }
                    move_to_partition(
                        transaction,
                        revision,
                        &mut next,
                        BitmapDomain::Lifecycle,
                        destination.bitmap_key(),
                        &lifecycle_keys,
                        &moved,
                    )?;
                    if destination == Lifecycle::Trash {
                        applied.trashed += moved.len();
                    } else {
                        applied.restored += moved.len();
                    }
                    affected |= &moved;
                    resources.extend(["roots", "sidebar", "tags", "folders"].map(str::to_owned));
                }

                // Permanent deletions.
                let deleted_items = winners
                    .iter()
                    .filter(|entry| {
                        entry.kind == EntryKind::Item && entry.state == EntryState::Delete
                    })
                    .map(|entry| (entry.key.as_str(), entry.at_ms))
                    .collect::<Vec<_>>();
                let deleted = deleted_items
                    .iter()
                    .filter(|(key, _)| !other_file.contains(key))
                    .filter_map(|(key, _)| roots.get(key).map(|(root, _, _)| *root))
                    .collect::<RoaringBitmap>();
                if !deleted.is_empty() {
                    let (after, _, receipt) = delete_roots(
                        transaction,
                        revision,
                        &next,
                        &deleted,
                        now_ms(),
                        &protected,
                        DeletionOrigin::OtherDevice,
                    )?;
                    next = after;
                    applied.deleted = deleted.len();
                    affected |= &deleted;
                    resources.extend(receipt.resources);
                }
                if !deleted_items.is_empty() {
                    // Every deleted item gets its tombstone with the time of
                    // the deletion, also an item that is not here, so that
                    // an earlier import of it is refused here too.
                    transaction.execute(
                        "INSERT INTO deletion_tombstone(stable_key, revision, deleted_at_ms)
                         SELECT json_extract(entry.value, '$[0]'), ?2,
                                json_extract(entry.value, '$[1]')
                         FROM json_each(?1) entry WHERE true
                         ON CONFLICT(stable_key) DO UPDATE SET
                             revision = excluded.revision,
                             deleted_at_ms = excluded.deleted_at_ms",
                        rusqlite::params![
                            serde_json::to_string(&deleted_items)?,
                            revision as i64
                        ],
                    )?;
                }
                let added = keys_of(&winners, EntryKind::Item, EntryState::Add);
                if !added.is_empty() {
                    transaction.execute(
                        "DELETE FROM deletion_tombstone
                         WHERE stable_key IN (SELECT value FROM json_each(?1))",
                        [serde_json::to_string(&added)?],
                    )?;
                }

                // Items that another device imported.
                let records = winners
                    .iter()
                    .filter_map(|entry| Some((entry.key.as_str(), entry.record()?)))
                    .collect::<Vec<_>>();
                let (queued, aliased) = queue_shared_items(transaction, &records)?;
                applied.queued = queued;
                applied.aliased = aliased;
                if queued != 0 {
                    resources.extend(["ingest", "tasks"].map(str::to_owned));
                }
                if !deleted_items.is_empty() {
                    // A deleted item is not imported any more.
                    let jobs = deleted_items
                        .iter()
                        .map(|(key, _)| shared_job_key(key))
                        .collect::<Vec<_>>();
                    transaction.execute(
                        "DELETE FROM ingest_job
                         WHERE source_kind = ?2 AND status != 'running'
                           AND job_key IN (SELECT value FROM json_each(?1))",
                        rusqlite::params![serde_json::to_string(&jobs)?, SHARED_JOB_KIND],
                    )?;
                }

                // The winning entries are stored before the folders are
                // looked at, because a folder is deleted by the entries that
                // the library holds and not by this call's entries alone.
                store_entries(transaction, device_id, &winners)?;
                record_entries(transaction, EntryKind::Item, EntryState::Add, &added_again)?;
                if !deleted_items.is_empty() {
                    // A status entry of an item that these entries delete
                    // means nothing any more, whichever of the two came
                    // first in the list.
                    let keys = deleted_items.iter().map(|(key, _)| *key).collect::<Vec<_>>();
                    transaction.execute(
                        "DELETE FROM shared_entry
                         WHERE kind = 'status'
                           AND key IN (SELECT value FROM json_each(?1))",
                        [serde_json::to_string(&keys)?],
                    )?;
                }

                // Tags.
                let tag_keys = keys_of(&winners, EntryKind::Tag, EntryState::Delete);
                for tag_id in definition_ids(transaction, "tag_definition", "tag_id", &tag_keys)? {
                    let tag_id = crate::TagId(tag_id);
                    let (members, _) =
                        remove_tag_in_transaction(transaction, &mut next, tag_id, None, revision)?;
                    crate::smart::settle_affected_for(
                        transaction,
                        &mut next,
                        &members,
                        &[crate::predicate::DependencyChange::Tag(tag_id)],
                    )?;
                    applied.tags += 1;
                    affected |= &members;
                }
                if applied.tags != 0 {
                    resources
                        .extend(["roots", "tags", "navigation", "smart-folders"].map(str::to_owned));
                    if prune_deleted_starred_tags(transaction, &next)?.is_some() {
                        resources.insert("settings".into());
                    }
                }

                // Folders.
                if winners.iter().any(|entry| entry.kind == EntryKind::Folder) {
                    let named = tombstoned(transaction, "folder_definition", "folder_id", "folder")?;
                    let parents = parents_of(transaction, "folder_definition", "folder_id")?;
                    let requested = whole_subtrees(&named, &parents)
                        .into_iter()
                        .map(FolderId)
                        .collect::<Vec<_>>();
                    if !requested.is_empty() {
                        let removed =
                            delete_folder_subtrees(transaction, revision, &next, &requested)?;
                        applied.folders = removed.folder_ids.len() as u64;
                        affected |= &removed.affected;
                        next = removed.next;
                        resources.extend(
                            ["folders", "navigation", "sidebar", "smart-folders"]
                                .map(str::to_owned),
                        );
                    }
                }

                // Smart folders.
                if winners.iter().any(|entry| entry.kind == EntryKind::SmartFolder) {
                    let named = tombstoned(
                        transaction,
                        "smart_folder_definition",
                        "smart_folder_id",
                        "smart_folder",
                    )?;
                    let parents =
                        parents_of(transaction, "smart_folder_definition", "smart_folder_id")?;
                    for smart_folder_id in whole_subtrees(&named, &parents) {
                        let removed = delete_smart_folder_subtree(
                            transaction,
                            &mut next,
                            SmartFolderId(smart_folder_id),
                        )?;
                        applied.smart_folders += removed.smart_folder_ids.len() as u64;
                    }
                    if applied.smart_folders != 0 {
                        resources.extend(["smart-folders", "navigation"].map(str::to_owned));
                    }
                }

                applied.won = (winners.len() + added_again.len()) as u64;
                if applied.trashed
                    + applied.restored
                    + applied.deleted
                    + applied.tags
                    + applied.folders
                    + applied.smart_folders
                    + applied.queued
                    + applied.aliased
                    != 0
                {
                    // The row makes the next snapshot due, so that this
                    // device's copy in the cloud folder holds the change.
                    insert_cloud_journal(
                        transaction,
                        revision,
                        "sync.entries_applied",
                        None,
                        serde_json::json!({ "device_id": device_id }),
                        now_ms(),
                    )?;
                }
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    affected.iter().map(RootId),
                );
                applied.receipt = Some(receipt.clone());
                Ok(Some((
                    applied,
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                )))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok(published.map(|(applied, _, _)| applied).unwrap_or_default())
    }
}

/// An item of another device that waits in the ingest queue for its files.
#[derive(Debug, Clone, PartialEq)]
pub struct AwaitedImport {
    pub ingest_job_id: i64,
    pub import: SharedImport,
}

impl Library {
    /// Returns at most `limit` items of other devices that wait for their
    /// files and whose time to be looked at has come, the longest waiting
    /// first.
    pub fn awaited_shared_imports(&self, limit: usize, now: &str) -> Result<Vec<AwaitedImport>> {
        self.database.read(WorkPriority::Cloud, |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT ingest_job_id, payload_json FROM ingest_job
                 WHERE status = 'pending' AND prepared = 0 AND available_at <= ?1
                   AND source_kind = ?2
                 ORDER BY available_at, ingest_job_id LIMIT ?3",
            )?;
            let rows = statement
                .query_map(rusqlite::params![now, SHARED_JOB_KIND, limit as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .filter_map(|(ingest_job_id, payload)| {
                    match serde_json::from_str(&payload).ok()? {
                        crate::model::PreparedIngestPayload::Shared(import) => Some(AwaitedImport {
                            ingest_job_id,
                            import,
                        }),
                        _ => None,
                    }
                })
                .collect())
        })
    }

    /// Records that the files of the waiting item of the job `ingest_job_id`
    /// are in the library at `paths`, in the order of the record's files.
    /// The ingest worker imports the item from then on.
    pub fn prepare_shared_import(
        &self,
        awaited: &AwaitedImport,
        paths: Vec<String>,
        now: &str,
    ) -> Result<()> {
        let payload = crate::model::PreparedIngestPayload::Shared(SharedImport {
            paths,
            ..awaited.import.clone()
        });
        let source_path = match &payload {
            crate::model::PreparedIngestPayload::Shared(import) => {
                import.paths.first().cloned().unwrap_or_default()
            }
            _ => String::new(),
        };
        let payload = serde_json::to_string(&payload)?;
        self.auxiliary_write_if_changed(
            WorkPriority::Cloud,
            ["ingest".to_owned()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE ingest_job
                     SET payload_json = ?2, source_path = ?3, prepared = 1,
                         available_at = ?4, updated_at = ?4
                     WHERE ingest_job_id = ?1 AND status = 'pending' AND prepared = 0",
                    rusqlite::params![awaited.ingest_job_id, payload, source_path, now],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )?;
        Ok(())
    }

    /// Fails the job of a waiting item whose files cannot be imported, with
    /// the reason `error`.
    pub fn fail_shared_import(&self, ingest_job_id: i64, error: &str, now: &str) -> Result<()> {
        self.auxiliary_write_if_changed(
            WorkPriority::Cloud,
            ["ingest".to_owned()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE ingest_job SET status = 'failed', last_error = ?2, updated_at = ?3
                     WHERE ingest_job_id = ?1 AND status = 'pending' AND prepared = 0",
                    rusqlite::params![ingest_job_id, error, now],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )?;
        Ok(())
    }

    /// Makes the waiting items of the jobs `ingest_job_ids` be looked at
    /// again at `available_at` and not before.
    pub fn postpone_shared_imports(&self, ingest_job_ids: &[i64], available_at: &str) -> Result<()> {
        if ingest_job_ids.is_empty() {
            return Ok(());
        }
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                transaction.execute(
                    "UPDATE ingest_job SET available_at = ?2
                     WHERE status = 'pending' AND prepared = 0
                       AND ingest_job_id IN (SELECT value FROM json_each(?1))",
                    rusqlite::params![serde_json::to_string(ingest_job_ids)?, available_at],
                )?;
                Ok(())
            })
    }

    /// Imports an item or a collection that another device imported, from
    /// the files at `import.paths`. The item gets the stable key, the name,
    /// the notes, the source links, the rating, the dates, the tags and the
    /// folders of its record, and the status of the `status` entry that the
    /// library holds for it, or else of the record. A tag or a folder that
    /// the library does not hold under the record's key is looked for or
    /// created by its name. Nothing is recorded in the undo history, and no
    /// entry is recorded.
    pub fn ingest_shared(&self, import: &SharedImport) -> Result<(RootId, MutationReceipt)> {
        let record = &import.record;
        if record.files.is_empty() || import.paths.len() != record.files.len() {
            return Err(LibraryError::InvalidInput(
                "a shared item needs a path for each of its files".into(),
            ));
        }
        let mut folders = Vec::with_capacity(record.folders.len());
        for folder in &record.folders {
            folders.push(self.shared_folder(folder)?);
        }
        let tags = self.database.read(WorkPriority::CanonicalIngest, |connection| {
            use rusqlite::OptionalExtension;
            record
                .tags
                .iter()
                .map(|tag| {
                    let held = connection
                        .prepare_cached(
                            "SELECT namespace.display_name, tag.subname
                             FROM tag_definition tag
                             JOIN tag_namespace namespace
                               ON namespace.namespace_id = tag.namespace_id
                             WHERE tag.stable_key = ?1",
                        )?
                        .query_row([&tag.key], |row| {
                            Ok(whole_tag_name(
                                &row.get::<_, String>(0)?,
                                &row.get::<_, String>(1)?,
                            ))
                        })
                        .optional()?;
                    Ok(held.unwrap_or_else(|| tag.name.clone()))
                })
                .collect::<Result<Vec<_>>>()
        })?;
        let collection = record.cover.is_some();
        let members = record
            .files
            .iter()
            .zip(&import.paths)
            .map(|(file, path)| PreparedImport {
                stable_key: if collection {
                    file.key.clone()
                } else {
                    import.key.clone()
                },
                media_name: if collection {
                    file.name.clone()
                } else {
                    record.name.clone()
                },
                file_path: path.clone(),
                facts: file.facts.clone(),
                lifecycle: record.status,
                rating: record.rating,
                notes: if collection {
                    file.notes.clone()
                } else {
                    record.notes.clone()
                },
                tags: tags.clone(),
                folders: folders.clone(),
                source_urls: record.urls.clone(),
                source_identity: file.source.as_ref().map(|source| crate::model::SourceIdentity {
                    source_key: source.source_key.clone(),
                    source_item_key: source.source_item_key.clone(),
                    source_text: source.source_text.clone(),
                    source_attempt_id: None,
                }),
                imported_at_ms: record.imported_at_ms,
                captured_at_ms: record.captured_at_ms,
            })
            .collect::<Vec<_>>();
        if !collection {
            let mut outputs = self.ingest_batch_with(&members[..1], false, None, &|_| {
                ingest::Held::Shared
            })?;
            return Ok(outputs.remove(0));
        }
        self.ingest_collection_with(
            &PreparedCollectionImport {
                cover_index: record.cover.unwrap_or(0).min(members.len() - 1),
                members,
                existing_root_id: None,
                name: Some(record.name.clone()),
                modified_at_ms: record.imported_at_ms,
            },
            false,
            None,
            Some(&record.notes),
            Some(&import.key),
        )
    }

    /// Returns the folder of this library with the stable key of `folder`,
    /// and creates it at the top level with the name of `folder` when the
    /// library does not hold it.
    fn shared_folder(&self, folder: &SharedName) -> Result<FolderId> {
        let held = |library: &Self| {
            library.database.read(WorkPriority::CanonicalIngest, |connection| {
                use rusqlite::OptionalExtension;
                Ok(connection
                    .prepare_cached("SELECT folder_id FROM folder_definition WHERE stable_key = ?1")?
                    .query_row([&folder.key], |row| row.get::<_, u32>(0))
                    .optional()?
                    .map(FolderId))
            })
        };
        if let Some(folder_id) = held(self)? {
            return Ok(folder_id);
        }
        let created = self.history.without_recording(|| {
            self.create_folder_with_stable_key(&folder.name, None, folder.key.clone())
        });
        match created {
            Ok((folder_id, _)) => Ok(folder_id),
            // Another import created the folder meanwhile.
            Err(error) => held(self)?.ok_or(error),
        }
    }

    /// Returns the entry files that this library has merged, each as the id
    /// of the device that wrote it and its file name.
    pub fn merged_entry_files(&self) -> Result<HashSet<(String, String)>> {
        self.database.read(WorkPriority::Cloud, |connection| {
            let mut statement =
                connection.prepare_cached("SELECT device_id, file_name FROM merged_entry_file")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Records that this library has merged the file `file_name` of the
    /// device `device_id`, which holds the entries of the shard `shard`.
    pub fn set_entry_file_merged(&self, device_id: &str, shard: &str, file_name: &str) -> Result<()> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                transaction.execute(
                    "INSERT INTO merged_entry_file (device_id, shard, file_name)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(device_id, shard) DO UPDATE SET file_name = excluded.file_name",
                    [device_id, shard, file_name],
                )?;
                Ok(())
            })
    }
}

/// The clock time (milliseconds and counter) and the device of the entries
/// that the library holds, by kind and key.
type HeldEntries<'a> = HashMap<(EntryKind, &'a str), (i64, i64, String)>;

/// Returns the clock time and the device of the entry that the library holds
/// for each key of `entries`.
fn held_entries<'a>(
    transaction: &rusqlite::Transaction<'_>,
    entries: &'a [SharedEntry],
) -> Result<HeldEntries<'a>> {
    let wanted = entries
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.key.as_str()))
        .collect::<Vec<_>>();
    let mut statement = transaction.prepare_cached(
        "SELECT stored.kind, stored.key, stored.clock_ms, stored.clock_counter,
                stored.device_id
         FROM json_each(?1) wanted
         CROSS JOIN shared_entry stored
           ON stored.kind = json_extract(wanted.value, '$[0]')
          AND stored.key = json_extract(wanted.value, '$[1]')",
    )?;
    let rows = statement
        .query_map([serde_json::to_string(&wanted)?], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                (row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, String>(4)?),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let by_key = entries
        .iter()
        .map(|entry| ((entry.kind, entry.key.as_str()), entry.key.as_str()))
        .collect::<HashMap<_, _>>();
    Ok(rows
        .into_iter()
        .filter_map(|(kind, key, held)| {
            let kind = EntryKind::parse(&kind)?;
            let key = by_key.get(&(kind, key.as_str()))?;
            Some(((kind, *key), held))
        })
        .collect())
}

/// Stores `entries` as entries of the device `device_id`, over what the
/// library holds for their keys.
fn store_entries(
    transaction: &rusqlite::Transaction<'_>,
    device_id: &str,
    entries: &[&SharedEntry],
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let rows = entries
        .iter()
        .map(|entry| {
            serde_json::json!([
                entry.kind.as_str(),
                entry.key,
                entry.state.as_str(),
                entry.at_ms,
                entry.logical,
                entry.value.as_ref().map(|value| value.to_string())
            ])
        })
        .collect::<Vec<_>>();
    transaction.execute(
        &format!(
            "INSERT INTO shared_entry
                 (kind, key, state, value, clock_ms, clock_counter, device_id)
             SELECT json_extract(entry.value, '$[0]'), json_extract(entry.value, '$[1]'),
                    json_extract(entry.value, '$[2]'), json_extract(entry.value, '$[5]'),
                    json_extract(entry.value, '$[3]'), json_extract(entry.value, '$[4]'), ?2
             FROM json_each(?1) entry WHERE true
             {UPSERT}"
        ),
        rusqlite::params![serde_json::to_string(&rows)?, device_id],
    )?;
    Ok(())
}

/// A root of this library: its id, the time of its import, and the content
/// hash of its file. A collection has no file of its own.
type LocalRoot = (u32, i64, Option<String>);

/// Returns, for each of `keys` that is the stable key of an item or a
/// collection of this library, or an alias of one, that root.
fn local_roots<'a>(
    transaction: &rusqlite::Transaction<'_>,
    keys: &BTreeSet<&'a str>,
) -> Result<HashMap<&'a str, LocalRoot>> {
    let mut roots = HashMap::new();
    if keys.is_empty() {
        return Ok(roots);
    }
    let wanted = serde_json::to_string(keys)?;
    // The two tables are read one after the other: a join on their union
    // would read both of them whole.
    for names in ["library_item_alias", "library_item"] {
        let mut statement = transaction.prepare_cached(&format!(
            "SELECT keys.value, root.root_id, root.imported_at_ms, file.content_hash
             FROM json_each(?1) keys
             CROSS JOIN {names} named ON named.stable_key = keys.value
             JOIN library_root root ON root.root_id = named.local_id
             LEFT JOIN media_item media ON media.media_id = root.root_id
             LEFT JOIN media_file file ON file.file_id = media.file_id"
        ))?;
        let rows = statement
            .query_map([&wanted], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, u32>(1)?, row.get::<_, i64>(2)?, row.get(3)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<(String, LocalRoot)>>>()?;
        for (key, root) in rows {
            if let Some(key) = keys.get(key.as_str()) {
                roots.insert(*key, root);
            }
        }
    }
    Ok(roots)
}

/// Returns which of `keys` are the stable key of an item of this library or
/// an alias of one. A file of a collection is an item too.
fn held_item_keys(
    transaction: &rusqlite::Transaction<'_>,
    keys: &[&str],
) -> Result<HashSet<String>> {
    let mut held = HashSet::new();
    let wanted = serde_json::to_string(keys)?;
    for names in ["library_item_alias", "library_item"] {
        let mut statement = transaction.prepare_cached(&format!(
            "SELECT keys.value FROM json_each(?1) keys
             CROSS JOIN {names} named ON named.stable_key = keys.value"
        ))?;
        let rows = statement.query_map([&wanted], |row| row.get::<_, String>(0))?;
        for key in rows {
            held.insert(key?);
        }
    }
    Ok(held)
}

/// Handles the records of the items that another device imported, each with
/// the key of its entry. A record whose keys the library holds needs nothing.
/// A record of one file that the library holds under another key makes its
/// key an alias of that item. Every other record is put into the ingest
/// queue as a job that is not prepared: the cloud worker copies its files
/// from the cloud folder first. Returns the number of jobs and of aliases.
fn queue_shared_items(
    transaction: &rusqlite::Transaction<'_>,
    records: &[(&str, SharedItem)],
) -> Result<(u64, u64)> {
    use rusqlite::OptionalExtension;
    if records.is_empty() {
        return Ok((0, 0));
    }
    let keys = records
        .iter()
        .flat_map(|(key, record)| {
            std::iter::once(*key).chain(record.files.iter().map(|file| file.key.as_str()))
        })
        .collect::<Vec<_>>();
    let held = held_item_keys(transaction, &keys)?;
    let now = chrono::Utc::now().to_rfc3339();
    let (mut queued, mut aliased) = (0, 0);
    for (key, record) in records {
        if record.files.is_empty() {
            continue;
        }
        let collection = record.cover.is_some();
        let complete = if collection {
            held.contains(*key) && record.files.iter().all(|file| held.contains(&file.key))
        } else {
            held.contains(*key)
        };
        if complete {
            continue;
        }
        if !collection {
            let same_file = transaction
                .prepare_cached(
                    "SELECT media.media_id FROM media_file file
                     JOIN media_item media ON media.file_id = file.file_id
                     WHERE file.content_hash = ?1 ORDER BY media.media_id LIMIT 1",
                )?
                .query_row([&record.files[0].facts.content_hash], |row| {
                    row.get::<_, u32>(0)
                })
                .optional()?;
            if let Some(media_id) = same_file {
                transaction
                    .prepare_cached(
                        "INSERT OR IGNORE INTO library_item_alias(stable_key, local_id)
                         VALUES (?1, ?2)",
                    )?
                    .execute(rusqlite::params![key, media_id])?;
                aliased += 1;
                continue;
            }
        }
        let payload = crate::model::PreparedIngestPayload::Shared(SharedImport {
            key: (*key).to_owned(),
            record: record.clone(),
            paths: Vec::new(),
        });
        // A job that is running is left alone. Its import finds the item
        // by its key, and the next entry for the key queues it again.
        transaction
            .prepare_cached(
                "INSERT INTO ingest_job
                     (job_key, source_kind, source_path, payload_json, delete_after_ingest,
                      status, attempt_count, available_at, created_at, updated_at, units,
                      prepared)
                 VALUES (?1, ?2, '', ?3, 0, 'pending', 0, ?4, ?4, ?4, 1, 0)
                 ON CONFLICT(job_key) DO UPDATE SET
                     payload_json = excluded.payload_json, status = 'pending', prepared = 0,
                     attempt_count = 0, available_at = excluded.available_at,
                     last_error = NULL, updated_at = excluded.updated_at
                 WHERE ingest_job.status != 'running'",
            )?
            .execute(rusqlite::params![
                shared_job_key(key),
                SHARED_JOB_KIND,
                serde_json::to_string(&payload)?,
                now
            ])?;
        queued += 1;
    }
    Ok((queued, aliased))
}

/// Returns the ids of the rows of `table` whose stable key is in `keys`.
fn definition_ids(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    id_column: &'static str,
    keys: &[String],
) -> Result<Vec<u32>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = transaction.prepare_cached(&format!(
        "SELECT definition.{id_column} FROM json_each(?1) keys
         CROSS JOIN {table} definition ON definition.stable_key = keys.value
         ORDER BY definition.{id_column}"
    ))?;
    let rows = statement.query_map([serde_json::to_string(keys)?], |row| row.get::<_, u32>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Returns the ids of the rows of `table` that still exist and for whose
/// stable key the library holds a `delete` entry of `kind`.
fn tombstoned(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    id_column: &'static str,
    kind: &'static str,
) -> Result<Vec<u32>> {
    let mut statement = transaction.prepare_cached(&format!(
        "SELECT definition.{id_column} FROM shared_entry stored
         CROSS JOIN {table} definition ON definition.stable_key = stored.key
         WHERE stored.kind = '{kind}' AND stored.state = 'delete'
         ORDER BY definition.{id_column}"
    ))?;
    let rows = statement.query_map([], |row| row.get::<_, u32>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Returns the parent of every row of `table`, which is the table of the
/// folders or of the smart folders.
fn parents_of(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    id_column: &'static str,
) -> Result<HashMap<u32, Option<u32>>> {
    let mut statement =
        transaction.prepare_cached(&format!("SELECT {id_column}, parent_id FROM {table}"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, Option<u32>>(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Returns the members of `named` that can be deleted without deleting
/// anything outside `named`, leaving out those that lie below another such
/// member. `parents` gives the parent of every folder. A folder with a
/// folder below it that `named` does not hold is left out, together with
/// every folder above it. So is a folder that `parents` does not hold.
fn whole_subtrees(named: &[u32], parents: &HashMap<u32, Option<u32>>) -> Vec<u32> {
    let named = named.iter().copied().collect::<BTreeSet<_>>();
    let mut kept = HashSet::new();
    for folder in parents.keys().filter(|folder| !named.contains(folder)) {
        let mut above = parents.get(folder).copied().flatten();
        while let Some(parent) = above {
            if !kept.insert(parent) {
                break;
            }
            above = parents.get(&parent).copied().flatten();
        }
    }
    let whole = |folder: &u32| {
        named.contains(folder) && parents.contains_key(folder) && !kept.contains(folder)
    };
    named
        .iter()
        .copied()
        .filter(|folder| {
            whole(folder)
                && !parents
                    .get(folder)
                    .copied()
                    .flatten()
                    .is_some_and(|parent| whole(&parent))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::whole_subtrees;
    use std::collections::HashMap;

    /// The folders 1 > 2 > 3 and 1 > 4, and the folder 5 beside them.
    fn tree() -> HashMap<u32, Option<u32>> {
        HashMap::from([(1, None), (2, Some(1)), (3, Some(2)), (4, Some(1)), (5, None)])
    }

    #[test]
    fn a_folder_is_deleted_only_with_everything_below_it() {
        // Every folder below 1 is named, so 1 alone is asked for.
        assert_eq!(whole_subtrees(&[1, 2, 3, 4], &tree()), [1]);
        // 4 is not named, so 1 stays, and the whole branch below 2 goes.
        assert_eq!(whole_subtrees(&[1, 2, 3], &tree()), [2]);
        // 3 is not named, so 2 and 1 stay and only the leaf 4 goes.
        assert_eq!(whole_subtrees(&[1, 2, 4, 5], &tree()), [4, 5]);
        assert!(whole_subtrees(&[1], &tree()).is_empty());
        assert!(whole_subtrees(&[9], &tree()).is_empty());
    }
}
