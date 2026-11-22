//! Deletions that cloud sync carries between the devices of one library.
//!
//! A device writes the deletions that its user made into the cloud folder,
//! and every other device of the library makes the same deletions. This file
//! owns both halves on the library's side:
//!
//! - [`Library::outgoing_deletions`] turns the rows that the local deletions
//!   left in `cloud_journal` into [`SyncedDeletion`] records.
//! - [`Library::apply_synced_deletions`] makes the deletions of one file
//!   that another device wrote, in one publication and with no history
//!   entry.
//!
//! A record names what it deletes by a stable key. An item record also holds
//! the content hashes of the item's files, and it changes the local item
//! with that key only when every file of the local item is among those
//! hashes. Two devices can give the same key to different files, and one
//! device can add a file to a collection that the other device deleted, so
//! the key alone does not prove that the local item is the deleted one.
//!
//! Two tables hold the state of the exchange. `cloud_device_frontier` holds,
//! for each device, the number of the last deletion file of that device that
//! this library holds: applied for another device, written for this one.
//! `cloud_tombstone` holds, under the object kind `lifecycle`, the time and
//! the device of the last synced move of an item into or out of the Trash,
//! so that the later of two moves wins on every device.
//!
//! The time of a move is the time of the clock of the device that made it,
//! with one correction. A move that the user makes after this library has
//! stored a move of the same item is the later of the two, whatever the
//! clocks say, so it gets a time that is not before that move
//! ([`StoredMove::time_of`]). Two moves that were made before either device
//! knew of the other's are ordered by the clocks, and at the same time by
//! the device ids.

use serde::{Deserialize, Serialize};

use super::*;

/// The journal rows that hold local deletions for the other devices.
const OUTGOING_KINDS: &str = "'root.trash', 'root.permanent_delete', 'tag.delete',
     'tag.delete_unused', 'folder.delete', 'smart_folder.delete'";

/// One deletion, as it is written to the cloud folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncedDeletion {
    /// The item was moved to the Trash.
    ItemTrash {
        key: String,
        files: Vec<String>,
        at_ms: i64,
    },
    /// The item was taken out of the Trash, into the Inbox when `inbox` is
    /// set and into All otherwise.
    ItemRestore {
        key: String,
        files: Vec<String>,
        inbox: bool,
        at_ms: i64,
    },
    /// The item was deleted permanently.
    ItemDelete {
        key: String,
        files: Vec<String>,
        at_ms: i64,
    },
    TagDelete {
        key: String,
        at_ms: i64,
    },
    FolderDelete {
        key: String,
        at_ms: i64,
    },
    SmartFolderDelete {
        key: String,
        at_ms: i64,
    },
}

/// Where [`Library::outgoing_deletions`] continues: the journal row, and the
/// number of entries of that row that were already returned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutgoingCursor {
    journal_id: u64,
    offset: usize,
}

/// One page of the local deletions that wait to be written to the cloud
/// folder. `next` is `None` when no deletion is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingDeletions {
    pub deletions: Vec<SyncedDeletion>,
    pub next: Option<OutgoingCursor>,
}

/// One deletion file of another device, as [`Library::apply_synced_deletions`]
/// takes it.
#[derive(Debug, Clone, Copy)]
pub struct ReceivedDeletions<'a> {
    /// The device that wrote the file.
    pub device_id: &'a str,
    /// The file's number among that device's files, starting at 1.
    pub sequence: u32,
    pub created_at_ms: i64,
    pub deletions: &'a [SyncedDeletion],
}

/// What applying one deletion file changed in this library.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppliedDeletions {
    pub trashed: u64,
    pub restored: u64,
    pub deleted: u64,
    pub tags: u64,
    pub folders: u64,
    pub smart_folders: u64,
    /// `None` when the file had been applied before.
    pub receipt: Option<MutationReceipt>,
}

impl AppliedDeletions {
    fn changed(&self) -> bool {
        self.trashed + self.restored + self.deleted + self.tags + self.folders + self.smart_folders
            != 0
    }
}

/// The local root that an item record names, when it may be changed.
struct MatchedRoot {
    root_id: u32,
    imported_at_ms: i64,
}

/// The last synced move of an item into or out of the Trash that this
/// library holds.
#[derive(Debug, Clone)]
struct StoredMove {
    /// The time of the move.
    at_ms: i64,
    /// The device that made the move.
    device_id: String,
    /// The time, by this device's clock, at which this library stored the
    /// move.
    stored_ms: i64,
}

impl StoredMove {
    /// Returns the time to give a move that the device `own_device` made at
    /// `made_at_ms` and has not written to the cloud folder yet.
    ///
    /// Such a move is later than every move that the same device has
    /// written, so it does not get an earlier time than this one when this
    /// one is the device's own. It is also later than another device's
    /// move that was stored here before it was made, so it then gets a
    /// later time than this one.
    fn time_of(&self, made_at_ms: i64, own_device: &str) -> i64 {
        if self.device_id == own_device {
            made_at_ms.max(self.at_ms)
        } else if made_at_ms > self.stored_ms {
            made_at_ms.max(self.at_ms + 1)
        } else {
            made_at_ms
        }
    }

    /// Returns whether a move that `device_id` made at `at_ms` is later
    /// than this one. Of two moves at the same time, the one of the device
    /// with the greater id is the later one, and a device's own moves come
    /// in the order in which it made them.
    fn is_before(&self, at_ms: i64, device_id: &str) -> bool {
        at_ms > self.at_ms || (at_ms == self.at_ms && device_id >= self.device_id.as_str())
    }
}

impl Library {
    /// Returns up to `limit` deletions that this device made up to
    /// `through_revision` and has not yet written to the cloud folder,
    /// starting at `after`. A deletion that was undone since is left out: a
    /// deleted item, tag, folder or smart folder that exists again is not
    /// sent.
    pub fn outgoing_deletions(
        &self,
        through_revision: u64,
        after: OutgoingCursor,
        limit: usize,
    ) -> Result<OutgoingDeletions> {
        self.auxiliary_read_consistent(WorkPriority::Cloud, |transaction, snapshot| {
            let own_device = own_device_id(transaction)?;
            let mut deletions = Vec::new();
            let mut cursor = after;
            while deletions.len() < limit {
                let row = {
                    use rusqlite::OptionalExtension;
                    transaction
                        .prepare_cached(&format!(
                            "SELECT journal_id, operation_kind, target_bitmap, payload_json,
                                    created_at_ms
                             FROM cloud_journal
                             WHERE revision <= ?1 AND journal_id >= ?2
                               AND operation_kind IN ({OUTGOING_KINDS})
                             ORDER BY journal_id LIMIT 1"
                        ))?
                        .query_row(
                            rusqlite::params![through_revision as i64, cursor.journal_id as i64],
                            |row| {
                                Ok((
                                    row.get::<_, i64>(0)? as u64,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, Option<Vec<u8>>>(2)?,
                                    row.get::<_, String>(3)?,
                                    row.get::<_, i64>(4)?,
                                ))
                            },
                        )
                        .optional()?
                };
                let Some((journal_id, kind, targets, payload, at_ms)) = row else {
                    return Ok(OutgoingDeletions {
                        deletions,
                        next: None,
                    });
                };
                if journal_id != cursor.journal_id {
                    cursor = OutgoingCursor {
                        journal_id,
                        offset: 0,
                    };
                }
                let payload: serde_json::Value = serde_json::from_str(&payload)?;
                let room = limit - deletions.len();
                let taken = match kind.as_str() {
                    "root.trash" => {
                        let targets = targets
                            .map(|bytes| {
                                RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(bytes))
                            })
                            .transpose()?
                            .unwrap_or_default();
                        let roots = targets
                            .iter()
                            .skip(cursor.offset)
                            .take(room)
                            .collect::<Vec<_>>();
                        let lifecycle = payload["lifecycle"].as_u64().unwrap_or(0) as u32;
                        let identities = local_identities(transaction, snapshot, &roots)?;
                        let stored = stored_trash_moves(
                            transaction,
                            &identities.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
                        )?;
                        for (key, files) in identities {
                            let at_ms = stored
                                .get(&key)
                                .map_or(at_ms, |stored| stored.time_of(at_ms, &own_device));
                            deletions.push(if lifecycle == Lifecycle::Trash.bitmap_key() {
                                SyncedDeletion::ItemTrash { key, files, at_ms }
                            } else {
                                SyncedDeletion::ItemRestore {
                                    key,
                                    files,
                                    inbox: lifecycle == Lifecycle::Inbox.bitmap_key(),
                                    at_ms,
                                }
                            });
                        }
                        roots.len()
                    }
                    "root.permanent_delete" => {
                        let items = payload["items"]
                            .as_array()
                            .map(Vec::as_slice)
                            .unwrap_or_default();
                        let page = items
                            .iter()
                            .skip(cursor.offset)
                            .take(room)
                            .collect::<Vec<_>>();
                        let keys = page
                            .iter()
                            .filter_map(|item| item["key"].as_str())
                            .collect::<Vec<_>>();
                        let existing =
                            existing_keys(transaction, "library_item", &keys)?;
                        for item in &page {
                            let Some(key) = item["key"].as_str() else {
                                continue;
                            };
                            if existing.contains(key) {
                                continue;
                            }
                            deletions.push(SyncedDeletion::ItemDelete {
                                key: key.to_owned(),
                                files: item["files"]
                                    .as_array()
                                    .map(|files| {
                                        files
                                            .iter()
                                            .filter_map(|file| file.as_str().map(str::to_owned))
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                at_ms,
                            });
                        }
                        page.len()
                    }
                    kind => {
                        let table = match kind {
                            "folder.delete" => "folder_definition",
                            "smart_folder.delete" => "smart_folder_definition",
                            _ => "tag_definition",
                        };
                        let all = payload["stable_keys"]
                            .as_array()
                            .map(Vec::as_slice)
                            .unwrap_or_default();
                        let page = all
                            .iter()
                            .skip(cursor.offset)
                            .take(room)
                            .filter_map(|key| key.as_str())
                            .collect::<Vec<_>>();
                        let existing = existing_keys(transaction, table, &page)?;
                        for key in &page {
                            if existing.contains(*key) {
                                continue;
                            }
                            let key = (*key).to_owned();
                            deletions.push(match table {
                                "folder_definition" => SyncedDeletion::FolderDelete { key, at_ms },
                                "smart_folder_definition" => {
                                    SyncedDeletion::SmartFolderDelete { key, at_ms }
                                }
                                _ => SyncedDeletion::TagDelete { key, at_ms },
                            });
                        }
                        all.iter().skip(cursor.offset).take(room).count()
                    }
                };
                cursor = if taken < room {
                    // The row is used up.
                    OutgoingCursor {
                        journal_id: journal_id + 1,
                        offset: 0,
                    }
                } else {
                    OutgoingCursor {
                        journal_id,
                        offset: cursor.offset + taken,
                    }
                };
            }
            Ok(OutgoingDeletions {
                deletions,
                next: Some(cursor),
            })
        })
    }

    /// Records that this device wrote `deletions` to the cloud folder as its
    /// deletion file number `sequence`. It stores the number, the time of
    /// each Trash move unless a later move of the same item is already
    /// stored, and it forgets the Trash moves of the items that the file
    /// deletes permanently.
    pub fn record_written_deletions(
        &self,
        sequence: u32,
        created_at_ms: i64,
        deletions: &[SyncedDeletion],
    ) -> Result<()> {
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                let device_id = own_device_id(transaction)?;
                store_frontier(transaction, &device_id, sequence, created_at_ms)?;
                let moves = deletions
                    .iter()
                    .filter_map(|deletion| match deletion {
                        SyncedDeletion::ItemTrash { key, at_ms, .. } => {
                            Some((key.as_str(), *at_ms, true))
                        }
                        SyncedDeletion::ItemRestore { key, at_ms, .. } => {
                            Some((key.as_str(), *at_ms, false))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                store_trash_moves(transaction, &device_id, &moves)?;
                let deleted = deletions
                    .iter()
                    .filter_map(|deletion| match deletion {
                        SyncedDeletion::ItemDelete { key, .. } => Some(key.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                forget_trash_moves(transaction, &deleted)
            })
    }

    /// Makes the deletions of one deletion file of another device, in one
    /// publication. Nothing is recorded in the undo history.
    ///
    /// - A Trash move or a restore is made unless this library holds a later
    ///   move of the same item, made here or on a third device.
    /// - A permanent deletion deletes the item whatever its status here is,
    ///   unless the item was imported here after the deletion was made.
    /// - An item is changed only when every file it holds here is among the
    ///   files of the record.
    /// - A folder or a smart folder is deleted only when every folder below
    ///   it here is deleted by the same file. The items in a deleted folder
    ///   stay in the library.
    ///
    /// The files of a device are applied in the order of their numbers. A
    /// file that was applied before changes nothing, and a file that skips a
    /// number is refused.
    pub fn apply_synced_deletions(&self, received: &ReceivedDeletions<'_>) -> Result<AppliedDeletions> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let protected = self.history.protected_cleanup_files();
        let published = self.database.published_write_if_changed(
            WorkPriority::Cloud,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let applied_through = transaction
                    .query_row(
                        "SELECT COALESCE((SELECT hlc_logical FROM cloud_device_frontier
                                          WHERE device_id = ?1), 0)",
                        [received.device_id],
                        |row| row.get::<_, i64>(0),
                    )?;
                if i64::from(received.sequence) <= applied_through {
                    return Ok(None);
                }
                if i64::from(received.sequence) != applied_through + 1 {
                    return Err(LibraryError::InvalidInput(format!(
                        "deletion file {} of device {} follows file {applied_through}",
                        received.sequence, received.device_id
                    )));
                }
                let own_device = own_device_id(transaction)?;
                let mut next = (*snapshot).clone();
                let mut applied = AppliedDeletions::default();
                let mut resources = BTreeSet::from(["cloud".to_string()]);
                let mut affected = RoaringBitmap::new();

                let matched = match_items(transaction, &next, received.deletions)?;

                // Trash moves and restores: the later move of an item wins.
                let moves = received
                    .deletions
                    .iter()
                    .filter_map(|deletion| match deletion {
                        SyncedDeletion::ItemTrash { key, at_ms, .. } => {
                            Some((key.as_str(), *at_ms, Lifecycle::Trash))
                        }
                        SyncedDeletion::ItemRestore {
                            key, at_ms, inbox, ..
                        } => Some((
                            key.as_str(),
                            *at_ms,
                            if *inbox {
                                Lifecycle::Inbox
                            } else {
                                Lifecycle::Active
                            },
                        )),
                        _ => None,
                    })
                    .filter(|(key, _, _)| matched.contains_key(*key))
                    .collect::<Vec<_>>();
                if !moves.is_empty() {
                    let keys = moves.iter().map(|(key, _, _)| *key).collect::<Vec<_>>();
                    let mut latest = stored_trash_moves(transaction, &keys)?;
                    let pending = pending_trash_moves(transaction)?;
                    let mut wanted = HashMap::<u32, Lifecycle>::new();
                    let mut accepted = HashMap::<&str, (i64, bool)>::new();
                    let stored_ms = now_ms();
                    for (key, at_ms, lifecycle) in &moves {
                        let root_id = matched[*key].root_id;
                        // The last move that this library knows: the stored
                        // one, or a later one that this device made and has
                        // not written to the cloud folder yet.
                        let mut local = latest.get(*key).cloned();
                        for (roots, made_at_ms) in &pending {
                            if !roots.contains(root_id) {
                                continue;
                            }
                            let at_ms = latest.get(*key).map_or(*made_at_ms, |stored| {
                                stored.time_of(*made_at_ms, &own_device)
                            });
                            if local
                                .as_ref()
                                .is_none_or(|known| known.is_before(at_ms, &own_device))
                            {
                                local = Some(StoredMove {
                                    at_ms,
                                    device_id: own_device.clone(),
                                    stored_ms,
                                });
                            }
                        }
                        if local.is_some_and(|local| !local.is_before(*at_ms, received.device_id)) {
                            continue;
                        }
                        latest.insert(
                            (*key).to_owned(),
                            StoredMove {
                                at_ms: *at_ms,
                                device_id: received.device_id.to_owned(),
                                stored_ms,
                            },
                        );
                        wanted.insert(root_id, *lifecycle);
                        accepted.insert(*key, (*at_ms, *lifecycle == Lifecycle::Trash));
                    }
                    let lifecycle_keys = Lifecycle::ALL
                        .iter()
                        .map(|value| value.bitmap_key())
                        .collect::<Vec<_>>();
                    for destination in Lifecycle::ALL {
                        let mut roots = wanted
                            .iter()
                            .filter(|(_, lifecycle)| **lifecycle == destination)
                            .map(|(root_id, _)| *root_id)
                            .collect::<RoaringBitmap>();
                        if destination == Lifecycle::Trash {
                            roots -= next.lifecycle(Lifecycle::Trash);
                        } else {
                            // A restore moves only an item that is in the
                            // Trash here.
                            roots &= next.lifecycle(Lifecycle::Trash);
                        }
                        if roots.is_empty() {
                            continue;
                        }
                        move_to_partition(
                            transaction,
                            revision,
                            &mut next,
                            BitmapDomain::Lifecycle,
                            destination.bitmap_key(),
                            &lifecycle_keys,
                            &roots,
                        )?;
                        if destination == Lifecycle::Trash {
                            applied.trashed += roots.len();
                        } else {
                            applied.restored += roots.len();
                        }
                        affected |= &roots;
                        resources.extend(
                            ["roots", "sidebar", "tags", "folders"].map(str::to_owned),
                        );
                    }
                    let stored = accepted
                        .iter()
                        .map(|(key, (at_ms, trashed))| (*key, *at_ms, *trashed))
                        .collect::<Vec<_>>();
                    store_trash_moves(transaction, received.device_id, &stored)?;
                }

                // Permanent deletions.
                let mut deleted = RoaringBitmap::new();
                let mut deleted_keys = Vec::new();
                for deletion in received.deletions {
                    let SyncedDeletion::ItemDelete { key, at_ms, .. } = deletion else {
                        continue;
                    };
                    // An item that was imported here after the deletion is
                    // another item with the same key, and it stays.
                    let root = matched
                        .get(key.as_str())
                        .filter(|root| root.imported_at_ms <= *at_ms);
                    if let Some(root) = root {
                        deleted.insert(root.root_id);
                        deleted_keys.push(key.as_str());
                    }
                }
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
                    forget_trash_moves(transaction, &deleted_keys)?;
                    applied.deleted = deleted.len();
                    affected |= &deleted;
                    resources.extend(receipt.resources);
                }

                // Tags.
                let tag_keys = received
                    .deletions
                    .iter()
                    .filter_map(|deletion| match deletion {
                        SyncedDeletion::TagDelete { key, .. } => Some(key.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
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
                    resources.extend(
                        ["roots", "tags", "navigation", "smart-folders"].map(str::to_owned),
                    );
                    if prune_deleted_starred_tags(transaction, &next)?.is_some() {
                        resources.insert("settings".into());
                    }
                }

                // Folders.
                let folder_keys = received
                    .deletions
                    .iter()
                    .filter_map(|deletion| match deletion {
                        SyncedDeletion::FolderDelete { key, .. } => Some(key.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let named = definition_ids(transaction, "folder_definition", "folder_id", &folder_keys)?;
                let requested = if named.is_empty() {
                    Vec::new()
                } else {
                    let parents = parents_of(transaction, "folder_definition", "folder_id")?;
                    whole_subtrees(&named, &parents)
                        .into_iter()
                        .map(FolderId)
                        .collect::<Vec<_>>()
                };
                if !requested.is_empty() {
                    let removed = delete_folder_subtrees(transaction, revision, &next, &requested)?;
                    applied.folders = removed.folder_ids.len() as u64;
                    affected |= &removed.affected;
                    next = removed.next;
                    resources.extend(
                        ["folders", "navigation", "sidebar", "smart-folders"].map(str::to_owned),
                    );
                }

                // Smart folders.
                let smart_keys = received
                    .deletions
                    .iter()
                    .filter_map(|deletion| match deletion {
                        SyncedDeletion::SmartFolderDelete { key, .. } => Some(key.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let named = definition_ids(
                    transaction,
                    "smart_folder_definition",
                    "smart_folder_id",
                    &smart_keys,
                )?;
                if !named.is_empty() {
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

                store_frontier(
                    transaction,
                    received.device_id,
                    received.sequence,
                    received.created_at_ms,
                )?;
                if applied.changed() {
                    // The row makes the next snapshot due, so that this
                    // device's copy in the cloud folder holds the change.
                    insert_cloud_journal(
                        transaction,
                        revision,
                        "sync.deletions_applied",
                        None,
                        serde_json::json!({
                            "device_id": received.device_id,
                            "sequence": received.sequence,
                        }),
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
        Ok(published
            .map(|(applied, _, _)| applied)
            .unwrap_or_default())
    }
}

/// Makes the exchange of deletions start again, inside the transaction that
/// connects the library to a cloud folder. It forgets which deletion files
/// this library holds, because the numbers belong to the cloud data that
/// the library synced with before, and it removes the journal rows of the
/// deletions that were made while sync was off, so that they are not sent.
pub fn reset_synced_deletions(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM cloud_device_frontier", [])?;
    transaction.execute(
        &format!("DELETE FROM cloud_journal WHERE operation_kind IN ({OUTGOING_KINDS})"),
        [],
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

fn store_frontier(
    transaction: &rusqlite::Transaction<'_>,
    device_id: &str,
    sequence: u32,
    created_at_ms: i64,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO cloud_device_frontier
             (device_id, hlc_physical_ms, hlc_logical, updated_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(device_id) DO UPDATE SET
             hlc_physical_ms = excluded.hlc_physical_ms,
             hlc_logical = excluded.hlc_logical,
             updated_at = excluded.updated_at",
        rusqlite::params![device_id, created_at_ms, sequence],
    )?;
    Ok(())
}

/// Returns the keys of `keys` that `table` holds in its column `stable_key`.
fn existing_keys(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    keys: &[&str],
) -> Result<HashSet<String>> {
    if keys.is_empty() {
        return Ok(HashSet::new());
    }
    let mut statement = transaction.prepare_cached(&format!(
        "SELECT keys.value FROM json_each(?1) keys
         CROSS JOIN {table} definition ON definition.stable_key = keys.value"
    ))?;
    let rows = statement.query_map([serde_json::to_string(keys)?], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Returns the ids of the rows of `table` whose stable key is in `keys`.
fn definition_ids(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    id_column: &'static str,
    keys: &[&str],
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

/// Returns the stable key and the sorted content hashes of the files of each
/// root in `roots` that exists, in the order of `roots`.
fn local_identities(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    roots: &[u32],
) -> Result<Vec<(String, Vec<String>)>> {
    let roots = roots
        .iter()
        .copied()
        .filter(|root_id| {
            Lifecycle::ALL
                .iter()
                .any(|lifecycle| snapshot.lifecycle(*lifecycle).contains(*root_id))
        })
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let keys = {
        let mut statement = transaction.prepare_cached(
            "SELECT item.local_id, item.stable_key FROM json_each(?1) ids
             CROSS JOIN library_item item ON item.local_id = ids.value",
        )?;
        let rows = statement.query_map([serde_json::to_string(&roots)?], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()?
    };
    let files = root_files(transaction, snapshot, &roots)?;
    Ok(roots
        .iter()
        .filter_map(|root_id| {
            Some((keys.get(root_id)?.clone(), files.get(root_id)?.clone()))
        })
        .collect())
}

/// Returns the sorted content hashes of the files of each root in `roots`: of
/// the one file of a standalone item, and of the file of every member of a
/// collection.
fn root_files(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    roots: &[u32],
) -> Result<HashMap<u32, Vec<String>>> {
    let mut owner = HashMap::new();
    for root_id in roots {
        match snapshot.collection_orders.get(&RootId(*root_id)) {
            Some(members) => owner.extend(members.iter().map(|media| (media.0, *root_id))),
            None => {
                owner.insert(*root_id, *root_id);
            }
        }
    }
    let media = owner.keys().copied().collect::<Vec<_>>();
    let mut statement = transaction.prepare_cached(
        "SELECT media.media_id, file.content_hash FROM json_each(?1) ids
         CROSS JOIN media_item media ON media.media_id = ids.value
         JOIN media_file file ON file.file_id = media.file_id",
    )?;
    let rows = statement.query_map([serde_json::to_string(&media)?], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut files = HashMap::<u32, Vec<String>>::new();
    for row in rows {
        let (media_id, hash) = row?;
        files.entry(owner[&media_id]).or_default().push(hash);
    }
    for hashes in files.values_mut() {
        hashes.sort_unstable();
    }
    Ok(files)
}

/// Returns, for each item record of `deletions`, the local root with the
/// record's key, when that root may be changed: it is a root of this
/// library, and every file it holds here is among the record's files.
fn match_items<'a>(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    deletions: &'a [SyncedDeletion],
) -> Result<HashMap<&'a str, MatchedRoot>> {
    let records = deletions
        .iter()
        .filter_map(|deletion| match deletion {
            SyncedDeletion::ItemTrash { key, files, .. }
            | SyncedDeletion::ItemRestore { key, files, .. }
            | SyncedDeletion::ItemDelete { key, files, .. } => Some((key.as_str(), files)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(HashMap::new());
    }
    let keys = records.iter().map(|(key, _)| *key).collect::<BTreeSet<_>>();
    let local = {
        let mut statement = transaction.prepare_cached(
            "SELECT keys.value, root.root_id, root.imported_at_ms FROM json_each(?1) keys
             CROSS JOIN library_item item ON item.stable_key = keys.value
             JOIN library_root root ON root.root_id = item.local_id",
        )?;
        let rows = statement.query_map([serde_json::to_string(&keys)?], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, u32>(1)?, row.get::<_, i64>(2)?),
            ))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()?
    };
    let roots = local.values().map(|(root_id, _)| *root_id).collect::<Vec<_>>();
    let held = root_files(transaction, snapshot, &roots)?;
    let mut matched = HashMap::new();
    for (key, files) in records {
        let Some((root_id, imported_at_ms)) = local.get(key).copied() else {
            continue;
        };
        let same = held.get(&root_id).is_some_and(|held| {
            !held.is_empty() && held.iter().all(|hash| files.contains(hash))
        });
        if same {
            matched.insert(
                key,
                MatchedRoot {
                    root_id,
                    imported_at_ms,
                },
            );
        } else {
            matched.remove(key);
        }
    }
    Ok(matched)
}

/// Returns the last synced Trash move that is stored for each of `keys`.
fn stored_trash_moves(
    transaction: &rusqlite::Transaction<'_>,
    keys: &[&str],
) -> Result<HashMap<String, StoredMove>> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }
    // `deleted_at` holds the time at which the row was stored. The Julian
    // day 2440587.5 is the start of 1970.
    let mut statement = transaction.prepare_cached(
        "SELECT stored.object_key, stored.hlc_physical_ms, stored.device_id,
                CAST((julianday(stored.deleted_at) - 2440587.5) * 86400000 AS INTEGER)
         FROM json_each(?1) keys
         CROSS JOIN cloud_tombstone stored
           ON stored.object_kind = 'lifecycle' AND stored.object_key = keys.value",
    )?;
    let rows = statement.query_map([serde_json::to_string(keys)?], |row| {
        Ok((
            row.get::<_, String>(0)?,
            StoredMove {
                at_ms: row.get(1)?,
                device_id: row.get(2)?,
                stored_ms: row.get(3)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Returns the roots and the time of every Trash move that this device made
/// and has not yet written to the cloud folder.
fn pending_trash_moves(transaction: &rusqlite::Transaction<'_>) -> Result<Vec<(RoaringBitmap, i64)>> {
    let mut statement = transaction.prepare_cached(
        "SELECT target_bitmap, created_at_ms FROM cloud_journal
         WHERE operation_kind = 'root.trash' AND target_bitmap IS NOT NULL",
    )?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(bytes, at_ms)| {
            Ok((
                RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(bytes))?,
                at_ms,
            ))
        })
        .collect()
}

/// Stores `moves` (key, time, whether the move was into the Trash) as the
/// last synced Trash moves that `device_id` made. A move that is not later
/// than the stored move of the same item is not stored
/// ([`StoredMove::is_before`]).
fn store_trash_moves(
    transaction: &rusqlite::Transaction<'_>,
    device_id: &str,
    moves: &[(&str, i64, bool)],
) -> Result<()> {
    if moves.is_empty() {
        return Ok(());
    }
    let rows = moves
        .iter()
        .map(|(key, at_ms, trashed)| serde_json::json!([key, at_ms, i64::from(*trashed)]))
        .collect::<Vec<_>>();
    transaction.execute(
        // `WHERE true` ends the SELECT: after `FROM json_each(…)` SQLite
        // would otherwise read `ON` as a join constraint.
        "INSERT INTO cloud_tombstone
             (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
              device_id, causal_frontier_json, deleted_at, purge_after)
         SELECT 'lifecycle', json_extract(entry.value, '$[0]'), 'lifecycle',
                json_extract(entry.value, '$[1]'), json_extract(entry.value, '$[2]'),
                ?2, '{}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), NULL
         FROM json_each(?1) entry WHERE true
         ON CONFLICT(object_kind, object_key) DO UPDATE SET
             hlc_physical_ms = excluded.hlc_physical_ms,
             hlc_logical = excluded.hlc_logical,
             device_id = excluded.device_id,
             deleted_at = excluded.deleted_at
         WHERE excluded.hlc_physical_ms > cloud_tombstone.hlc_physical_ms
            OR (excluded.hlc_physical_ms = cloud_tombstone.hlc_physical_ms
                AND excluded.device_id >= cloud_tombstone.device_id)",
        rusqlite::params![serde_json::to_string(&rows)?, device_id],
    )?;
    Ok(())
}

/// Removes the stored Trash moves of the items `keys`, which no longer exist.
fn forget_trash_moves(transaction: &rusqlite::Transaction<'_>, keys: &[&str]) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    transaction.execute(
        "DELETE FROM cloud_tombstone
         WHERE object_kind = 'lifecycle'
           AND object_key IN (SELECT value FROM json_each(?1))",
        [serde_json::to_string(keys)?],
    )?;
    Ok(())
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
