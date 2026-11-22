//! The tombstones that the devices of one library share through cloud sync.
//!
//! A permanent deletion writes a tombstone for every deleted item
//! (`deletion_tombstone`), and an import refuses an item whose tombstone is
//! later than the import. This file makes that model shared. For every key
//! that a deletion or its reversal touched, the library keeps one entry: what
//! was done last, the time of the library's hybrid logical clock at which it
//! was done, and the device that did it. A sync writes the entries that this
//! device made into the cloud folder and merges the entries that are there
//! ([`Library::apply_shared_entries`]).
//!
//! The entries are rows of `cloud_tombstone`. `object_kind` and `object_key`
//! name the key, `mutation_id` holds the state, `hlc_physical_ms` and
//! `hlc_logical` the clock time, and `device_id` the device. The rows of the
//! kind `blob` in the same table are the originals that wait to be removed
//! from the cloud folder; they are not entries.
//!
//! | Kind | Key | States |
//! | --- | --- | --- |
//! | `item` | stable key of an item or a collection | `delete`: deleted permanently. `add`: the tombstone was removed, by an import of the item, a subscription or an undo. |
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
//! A merged `delete` deletes the item through the existing delete code. Two
//! things are left alone. An item of which the library holds no entry yet,
//! and which was imported here after the deletion was made, was added again:
//! it stays and gets an `add` entry. That is the rule of an import, which
//! removes a tombstone that is earlier than the import. A folder or
//! smart folder with a folder below it that has no `delete` entry stays until
//! that folder is deleted too, so a merge deletes no folder that the other
//! device did not delete.

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
    /// The value of `cloud_tombstone.object_kind`.
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
    /// The deletion was taken back: the item was imported again, or the
    /// deletion was undone.
    Add,
    /// Moved to the Trash.
    Trash,
    /// Taken out of the Trash into All.
    Active,
    /// Taken out of the Trash into the Inbox.
    Inbox,
}

impl EntryState {
    /// The value of `cloud_tombstone.mutation_id`.
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
    /// `None` when no entry won, and nothing was written.
    pub receipt: Option<MutationReceipt>,
}

const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

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
            "INSERT INTO cloud_tombstone
                 (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
                  device_id, causal_frontier_json, deleted_at, purge_after)
             SELECT ?1, keys.value, ?2, ?3, ?4,
                    (SELECT device_id FROM cloud_state WHERE singleton = 1),
                    '{{}}', {NOW_SQL}, NULL
             FROM json_each(?5) keys WHERE true
             ON CONFLICT(object_kind, object_key) DO UPDATE SET
                 mutation_id = excluded.mutation_id,
                 hlc_physical_ms = excluded.hlc_physical_ms,
                 hlc_logical = excluded.hlc_logical,
                 device_id = excluded.device_id,
                 deleted_at = excluded.deleted_at"
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
        &format!(
            "UPDATE cloud_tombstone SET
                 mutation_id = 'add', hlc_physical_ms = ?2, hlc_logical = ?3,
                 device_id = (SELECT device_id FROM cloud_state WHERE singleton = 1),
                 deleted_at = {NOW_SQL}
             WHERE object_kind = ?1 AND mutation_id = 'delete'
               AND object_key IN (SELECT value FROM json_each(?4))"
        ),
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
/// entry for every item that gets a tombstone. For every deletion it drops
/// the `status` entries of the deleted roots, which no longer mean anything.
pub(super) fn record_deleted_roots(
    transaction: &rusqlite::Transaction<'_>,
    origin: DeletionOrigin,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM cloud_tombstone
         WHERE object_kind = 'status' AND object_key IN (
             SELECT item.stable_key FROM temp.delete_root selected
             CROSS JOIN library_item item ON item.local_id = selected.local_id)",
        [],
    )?;
    if origin != DeletionOrigin::User {
        return Ok(());
    }
    let (at_ms, logical) = tick(transaction)?;
    transaction.execute(
        &format!(
            "INSERT INTO cloud_tombstone
                 (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
                  device_id, causal_frontier_json, deleted_at, purge_after)
             SELECT 'item', item.stable_key, 'delete', ?1, ?2,
                    (SELECT device_id FROM cloud_state WHERE singleton = 1),
                    '{{}}', {NOW_SQL}, NULL
             FROM temp.delete_item selected
             CROSS JOIN library_item item ON item.local_id = selected.local_id
             WHERE true
             ON CONFLICT(object_kind, object_key) DO UPDATE SET
                 mutation_id = excluded.mutation_id,
                 hlc_physical_ms = excluded.hlc_physical_ms,
                 hlc_logical = excluded.hlc_logical,
                 device_id = excluded.device_id,
                 deleted_at = excluded.deleted_at"
        ),
        [at_ms, logical],
    )?;
    Ok(())
}

/// Makes the library write all of this device's entries into the cloud
/// folder again at its next sync, inside the transaction that connects the
/// library to a cloud folder. What was written before is in the cloud data
/// that the library synced with before.
pub fn reset_published_entries(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM cloud_device_frontier", [])?;
    Ok(())
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
            &format!(
                "INSERT INTO cloud_tombstone
                     (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
                      device_id, causal_frontier_json, deleted_at, purge_after)
                 SELECT 'status', item.stable_key, ?2, ?3, ?4, ?5, '{{}}', {NOW_SQL}, NULL
                 FROM json_each(?1) ids
                 CROSS JOIN library_item item ON item.local_id = ids.value
                 JOIN library_root root ON root.root_id = item.local_id
                 WHERE true
                 ON CONFLICT(object_kind, object_key) DO UPDATE SET
                     mutation_id = excluded.mutation_id,
                     hlc_physical_ms = excluded.hlc_physical_ms,
                     hlc_logical = excluded.hlc_logical,
                     device_id = excluded.device_id,
                     deleted_at = excluded.deleted_at
                 WHERE (excluded.hlc_physical_ms, excluded.hlc_logical, excluded.device_id)
                     > (cloud_tombstone.hlc_physical_ms, cloud_tombstone.hlc_logical,
                        cloud_tombstone.device_id)"
            ),
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
                "SELECT object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical
                 FROM cloud_tombstone
                 WHERE object_kind != 'blob'
                   AND device_id = (SELECT device_id FROM cloud_state WHERE singleton = 1)
                   AND (hlc_physical_ms, hlc_logical) > (?1, ?2)
                 ORDER BY object_kind, object_key",
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
    ///   is. One case is different: the library holds no entry for the item,
    ///   and the item was imported here after the time of the deletion. Then
    ///   the item stays, gets no tombstone, and gets an `add` entry of this
    ///   device.
    /// - `item`, `add`: the item's tombstone is removed.
    /// - `status`: an item of this library is moved to the Trash, or, when
    ///   it is in the Trash, out of it. An entry for an item that this
    ///   library does not hold is dropped.
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

                // An item of which the library holds no entry, and which was
                // imported here after it was deleted elsewhere, was added
                // again. It stays. When the library holds an entry for the
                // item, the clock times alone decide.
                let mut added_again = Vec::new();
                winners.retain(|entry| {
                    let later_import = entry.kind == EntryKind::Item
                        && entry.state == EntryState::Delete
                        && !held.contains_key(&(EntryKind::Item, entry.key.as_str()))
                        && roots
                            .get(entry.key.as_str())
                            .is_some_and(|(_, imported_at_ms)| *imported_at_ms > entry.at_ms);
                    if later_import {
                        added_again.push(entry.key.clone());
                    }
                    !later_import
                });
                // A status entry for an item that is not here is dropped.
                winners.retain(|entry| {
                    entry.kind != EntryKind::Status || roots.contains_key(entry.key.as_str())
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
                        .filter_map(|entry| roots.get(entry.key.as_str()).map(|(root, _)| *root))
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
                    .filter_map(|(key, _)| roots.get(key).map(|(root, _)| *root))
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
                        "DELETE FROM cloud_tombstone
                         WHERE object_kind = 'status'
                           AND object_key IN (SELECT value FROM json_each(?1))",
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
        "SELECT stored.object_kind, stored.object_key, stored.hlc_physical_ms,
                stored.hlc_logical, stored.device_id
         FROM json_each(?1) wanted
         CROSS JOIN cloud_tombstone stored
           ON stored.object_kind = json_extract(wanted.value, '$[0]')
          AND stored.object_key = json_extract(wanted.value, '$[1]')",
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
                entry.logical
            ])
        })
        .collect::<Vec<_>>();
    transaction.execute(
        &format!(
            "INSERT INTO cloud_tombstone
                 (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
                  device_id, causal_frontier_json, deleted_at, purge_after)
             SELECT json_extract(entry.value, '$[0]'), json_extract(entry.value, '$[1]'),
                    json_extract(entry.value, '$[2]'), json_extract(entry.value, '$[3]'),
                    json_extract(entry.value, '$[4]'), ?2, '{{}}', {NOW_SQL}, NULL
             FROM json_each(?1) entry WHERE true
             ON CONFLICT(object_kind, object_key) DO UPDATE SET
                 mutation_id = excluded.mutation_id,
                 hlc_physical_ms = excluded.hlc_physical_ms,
                 hlc_logical = excluded.hlc_logical,
                 device_id = excluded.device_id,
                 deleted_at = excluded.deleted_at"
        ),
        rusqlite::params![serde_json::to_string(&rows)?, device_id],
    )?;
    Ok(())
}

/// Returns, for each of `keys` that is the stable key of an item or a
/// collection of this library, its root id and the time of its import.
fn local_roots<'a>(
    transaction: &rusqlite::Transaction<'_>,
    keys: &BTreeSet<&'a str>,
) -> Result<HashMap<&'a str, (u32, i64)>> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mut statement = transaction.prepare_cached(
        "SELECT keys.value, root.root_id, root.imported_at_ms FROM json_each(?1) keys
         CROSS JOIN library_item item ON item.stable_key = keys.value
         JOIN library_root root ON root.root_id = item.local_id",
    )?;
    let rows = statement
        .query_map([serde_json::to_string(keys)?], |row| {
            Ok((row.get::<_, String>(0)?, (row.get::<_, u32>(1)?, row.get::<_, i64>(2)?)))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(key, root)| Some((*keys.get(key.as_str())?, root)))
        .collect())
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
        "SELECT definition.{id_column} FROM cloud_tombstone stored
         CROSS JOIN {table} definition ON definition.stable_key = stored.object_key
         WHERE stored.object_kind = '{kind}' AND stored.mutation_id = 'delete'
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
