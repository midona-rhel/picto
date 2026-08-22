//! Replacement mutations over library roots and media assets.

use std::collections::BTreeSet;

use rand::RngCore;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, ItemTarget, Lifecycle, MutationReceipt};
use crate::projection_v2::{
    FolderProjectionChange, ItemProjectionChange, MembershipProjectionChange, RootProjectionChange,
    StructureProjectionDelta,
};

const RANK_GAP: i64 = 1024;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct GroupItemsInput {
    pub item_ids: Vec<ItemId>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct GroupItemsResult {
    pub collection_id: ItemId,
    pub receipt: MutationReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct DetachItemsInput {
    pub collection_id: ItemId,
    pub media_item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ReorderCollectionInput {
    pub collection_id: ItemId,
    pub media_item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MediaMetadataPatch {
    pub rating: Option<Option<i64>>,
    pub notes: Option<Option<String>>,
    pub source_urls: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct DeleteItemsResult {
    pub receipt: MutationReceipt,
    pub freed_file_hashes: Vec<String>,
}

impl Application {
    pub fn set_lifecycle(
        &self,
        target: &ItemTarget,
        lifecycle: Lifecycle,
    ) -> Result<MutationReceipt, String> {
        let (item_ids, revision) = self.transaction(
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                for item_id in &item_ids {
                    let changed = transaction.execute(
                        "UPDATE library_root SET lifecycle = ?1 WHERE item_id = ?2",
                        params![lifecycle.as_str(), item_id],
                    )?;
                    if changed != 1 {
                        return Err(invalid(format!("Item {item_id} is not a library root")));
                    }
                }
                Ok((item_ids.clone(), item_ids))
            },
            |projections, changed_ids| {
                for item_id in changed_ids {
                    projections.apply_lifecycle_delta(item_id, lifecycle)?;
                }
                Ok(())
            },
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR],
            &item_ids,
        ))
    }

    pub fn set_folder_membership(
        &self,
        target: &ItemTarget,
        folder_id: i64,
        present: bool,
    ) -> Result<MutationReceipt, String> {
        let (item_ids, revision) = self.transaction(
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                require_folder(transaction, folder_id)?;
                for item_id in &item_ids {
                    require_root(transaction, *item_id)?;
                    if present {
                        transaction.execute(
                            "INSERT INTO folder_item (folder_id, item_id)
                         VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                            params![folder_id, item_id],
                        )?;
                    } else {
                        transaction.execute(
                            "DELETE FROM folder_item WHERE folder_id = ?1 AND item_id = ?2",
                            params![folder_id, item_id],
                        )?;
                    }
                }
                Ok((item_ids.clone(), item_ids))
            },
            |projections, changed_ids| {
                for item_id in changed_ids {
                    projections.apply_folder_delta(folder_id, item_id, present)?;
                }
                Ok(())
            },
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &item_ids,
        ))
    }

    pub fn group_items(&self, input: GroupItemsInput) -> Result<GroupItemsResult, String> {
        let item_ids = unique_ids(&input.item_ids)?;
        if item_ids.len() < 2 {
            return Err("A collection requires at least two media items".to_string());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let key = new_key("collection");
        let ((collection_id, affected), revision) = self.transaction(
            |transaction| {
                let lifecycle = require_same_root_lifecycle(transaction, &item_ids)?;
                let projected_lifecycle = parse_lifecycle(&lifecycle)?;
                let folders = folder_ids_for_roots(transaction, &item_ids)?;
                for item_id in &item_ids {
                    require_standalone_media_root(transaction, *item_id)?;
                }

                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, label, created_at, updated_at)
                 VALUES (?1, 'collection', ?2, ?3, ?3)",
                    params![key, input.label, now],
                )?;
                let collection_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
                    params![collection_id, lifecycle],
                )?;

                for (index, item_id) in item_ids.iter().enumerate() {
                    transaction
                        .execute("DELETE FROM library_root WHERE item_id = ?1", [item_id])?;
                    transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (?1, ?2, ?3)",
                    params![collection_id, item_id, (index as i64 + 1) * RANK_GAP],
                )?;
                }
                for folder_id in &folders {
                    transaction.execute(
                        "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)",
                        params![folder_id, collection_id],
                    )?;
                }
                transaction.execute(
                    "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                    params![item_ids[0], collection_id],
                )?;

                let mut affected = item_ids.clone();
                affected.push(collection_id);
                let mut delta = StructureProjectionDelta::default();
                delta.items.push(ItemProjectionChange {
                    item_id: collection_id,
                    kind: crate::app::ItemKind::Collection,
                    present: true,
                });
                delta.roots.push(RootProjectionChange {
                    item_id: collection_id,
                    lifecycle: Some(projected_lifecycle),
                });
                for item_id in &item_ids {
                    delta.roots.push(RootProjectionChange {
                        item_id: *item_id,
                        lifecycle: None,
                    });
                    delta.memberships.push(MembershipProjectionChange {
                        collection_id,
                        media_id: *item_id,
                        present: true,
                    });
                    delta
                        .folders
                        .extend(folders.iter().map(|folder_id| FolderProjectionChange {
                            folder_id: *folder_id,
                            item_id: *item_id,
                            present: false,
                        }));
                }
                delta
                    .folders
                    .extend(folders.iter().map(|folder_id| FolderProjectionChange {
                        folder_id: *folder_id,
                        item_id: collection_id,
                        present: true,
                    }));
                Ok(((collection_id, affected), delta))
            },
            |projections, delta| projections.apply_structure_delta(delta),
        )?;
        let receipt = receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &affected,
        );
        Ok(GroupItemsResult {
            collection_id: ItemId(collection_id),
            receipt,
        })
    }

    pub fn detach_items(&self, input: DetachItemsInput) -> Result<MutationReceipt, String> {
        let media_ids = unique_ids(&input.media_item_ids)?;
        if media_ids.is_empty() {
            return Err("No collection members were selected".to_string());
        }
        let (affected, revision) = self.transaction_rebuilding(|transaction| {
            let lifecycle = require_collection_root(transaction, input.collection_id.0)?;
            let folders = folder_ids_for_roots(transaction, &[input.collection_id.0])?;
            for media_id in &media_ids {
                let removed = transaction.execute(
                    "DELETE FROM collection_member
                     WHERE collection_id = ?1 AND media_item_id = ?2",
                    params![input.collection_id.0, media_id],
                )?;
                if removed != 1 {
                    return Err(invalid(format!(
                        "Media item {media_id} is not attached to collection {}",
                        input.collection_id.0
                    )));
                }
                create_root_with_folders(transaction, *media_id, &lifecycle, &folders)?;
            }

            let mut affected = media_ids.clone();
            affected.push(input.collection_id.0);
            dissolve_small_collection(transaction, input.collection_id.0, &mut affected)?;
            Ok(affected)
        })?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &affected,
        ))
    }

    pub fn ungroup_collection(&self, collection_id: ItemId) -> Result<MutationReceipt, String> {
        let (affected, revision) = self.transaction_rebuilding(|transaction| {
            let lifecycle = require_collection_root(transaction, collection_id.0)?;
            let folders = folder_ids_for_roots(transaction, &[collection_id.0])?;
            let members = collection_members(transaction, collection_id.0)?;
            for member in &members {
                create_root_with_folders(transaction, *member, &lifecycle, &folders)?;
            }
            transaction.execute(
                "DELETE FROM library_item WHERE item_id = ?1",
                [collection_id.0],
            )?;
            let mut affected = members;
            affected.push(collection_id.0);
            Ok(affected)
        })?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &affected,
        ))
    }

    pub fn reorder_collection(
        &self,
        input: ReorderCollectionInput,
    ) -> Result<MutationReceipt, String> {
        let media_ids = unique_ids(&input.media_item_ids)?;
        let (_, revision) = self.transaction(
            |transaction| {
                require_collection_root(transaction, input.collection_id.0)?;
                let existing = collection_members(transaction, input.collection_id.0)?;
                if existing.len() != media_ids.len()
                    || existing.iter().copied().collect::<BTreeSet<_>>()
                        != media_ids.iter().copied().collect::<BTreeSet<_>>()
                {
                    return Err(invalid(
                        "Reorder must contain every collection member exactly once",
                    ));
                }
                for (index, media_id) in media_ids.iter().enumerate() {
                    transaction.execute(
                        "UPDATE collection_member SET position_rank = ?1
                     WHERE collection_id = ?2 AND media_item_id = ?3",
                        params![
                            (index as i64 + 1) * RANK_GAP,
                            input.collection_id.0,
                            media_id
                        ],
                    )?;
                }
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, &resources::item(input.collection_id.0)],
            &[input.collection_id.0],
        ))
    }

    pub fn set_collection_cover(
        &self,
        collection_id: ItemId,
        media_item_id: ItemId,
    ) -> Result<MutationReceipt, String> {
        let (_, revision) = self.transaction(
            |transaction| {
                require_collection_root(transaction, collection_id.0)?;
                let member = transaction
                    .query_row(
                        "SELECT 1 FROM collection_member
                     WHERE collection_id = ?1 AND media_item_id = ?2",
                        params![collection_id.0, media_item_id.0],
                        |_| Ok(()),
                    )
                    .optional()?;
                if member.is_none() {
                    return Err(invalid("The selected cover is not a collection member"));
                }
                transaction.execute(
                    "UPDATE library_item SET cover_media_item_id = ?1, updated_at = ?2
                 WHERE item_id = ?3",
                    params![
                        media_item_id.0,
                        chrono::Utc::now().to_rfc3339(),
                        collection_id.0
                    ],
                )?;
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, &resources::item(collection_id.0)],
            &[collection_id.0],
        ))
    }

    pub fn apply_tags(
        &self,
        target: &ItemTarget,
        tags: &[String],
        add: bool,
        provenance_mask: i64,
    ) -> Result<MutationReceipt, String> {
        let tags = tags
            .iter()
            .filter_map(|tag| parse_tag(tag))
            .collect::<Vec<_>>();
        if tags.is_empty() {
            return Err("No valid tags were provided".to_string());
        }
        let (item_ids, revision) = self.transaction(
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                let media_ids = media_items_for_roots(transaction, &item_ids)?;
                let mut changed_tags = Vec::new();
                for (namespace, subtag) in &tags {
                    transaction.execute(
                        "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)
                     ON CONFLICT(namespace, subtag) DO NOTHING",
                        params![namespace, subtag],
                    )?;
                    let tag_id: i64 = transaction.query_row(
                        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                        params![namespace, subtag],
                        |row| row.get(0),
                    )?;
                    for media_id in &media_ids {
                        if add {
                            transaction.execute(
                                "INSERT INTO media_tag
                                 (media_item_id, tag_id, source, provenance_mask)
                             VALUES (?1, ?2, 'local', ?3)
                             ON CONFLICT(media_item_id, tag_id, source)
                             DO UPDATE SET provenance_mask =
                                 media_tag.provenance_mask | excluded.provenance_mask",
                                params![media_id, tag_id, provenance_mask],
                            )?;
                        } else {
                            transaction.execute(
                                "DELETE FROM media_tag WHERE media_item_id = ?1 AND tag_id = ?2",
                                params![media_id, tag_id],
                            )?;
                        }
                        changed_tags.push((*media_id, tag_id));
                    }
                }
                Ok((item_ids, changed_tags))
            },
            |projections, changed_tags| {
                for (media_id, tag_id) in changed_tags {
                    projections.apply_tag_delta(media_id, tag_id, add)?;
                }
                Ok(())
            },
        )?;
        Ok(receipt(
            revision,
            &[
                resources::LIBRARY,
                resources::SIDEBAR,
                resources::TAGS,
                resources::SMART_FOLDERS,
            ],
            &item_ids,
        ))
    }

    pub fn patch_metadata(
        &self,
        target: &ItemTarget,
        patch: &MediaMetadataPatch,
    ) -> Result<MutationReceipt, String> {
        let source_urls_json = patch
            .source_urls
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| error.to_string())?;
        let (item_ids, revision) = self.transaction(
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                let media_ids = media_items_for_roots(transaction, &item_ids)?;
                let now = chrono::Utc::now().to_rfc3339();
                for media_id in media_ids {
                    if let Some(rating) = patch.rating {
                        transaction.execute(
                        "UPDATE media_asset SET rating = ?1, updated_at = ?2 WHERE item_id = ?3",
                        params![rating, now, media_id],
                    )?;
                    }
                    if let Some(notes) = &patch.notes {
                        transaction.execute(
                            "UPDATE media_asset SET notes = ?1, updated_at = ?2 WHERE item_id = ?3",
                            params![notes, now, media_id],
                        )?;
                    }
                    if patch.source_urls.is_some() {
                        transaction.execute(
                            "UPDATE media_asset SET source_urls_json = ?1, updated_at = ?2
                         WHERE item_id = ?3",
                            params![source_urls_json, now, media_id],
                        )?;
                    }
                }
                Ok((item_ids, ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(receipt(
            revision,
            &[
                resources::LIBRARY,
                resources::SIDEBAR,
                resources::SMART_FOLDERS,
            ],
            &item_ids,
        ))
    }

    pub fn delete_items(&self, target: &ItemTarget) -> Result<DeleteItemsResult, String> {
        let ((freed_file_hashes, item_ids), revision) =
            self.transaction_rebuilding(|transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                let mut delete_items = BTreeSet::new();
                for item_id in &item_ids {
                    require_root(transaction, *item_id)?;
                    delete_items.insert(*item_id);
                    for member in collection_members(transaction, *item_id)? {
                        delete_items.insert(member);
                    }
                }
                for item_id in delete_items {
                    transaction.execute(
                        "UPDATE source_item
                     SET state = 'deleted', media_item_id = NULL, updated_at = ?1
                     WHERE media_item_id = ?2",
                        params![chrono::Utc::now().to_rfc3339(), item_id],
                    )?;
                    transaction
                        .execute("DELETE FROM library_item WHERE item_id = ?1", [item_id])?;
                }
                let mut stmt = transaction.prepare(
                    "SELECT mf.file_hash
                 FROM media_file mf
                 WHERE NOT EXISTS (
                     SELECT 1 FROM media_asset ma WHERE ma.file_id = mf.file_id
                 )",
                )?;
                let hashes = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(stmt);
                let now = chrono::Utc::now().to_rfc3339();
                for hash in &hashes {
                    crate::workers_v2::enqueue_blob_delete_in(transaction, hash, &now)?;
                }
                transaction.execute(
                    "DELETE FROM media_file
                 WHERE NOT EXISTS (
                     SELECT 1 FROM media_asset ma WHERE ma.file_id = media_file.file_id
                 )",
                    [],
                )?;
                Ok((hashes, item_ids))
            })?;
        Ok(DeleteItemsResult {
            receipt: receipt(
                revision,
                &[
                    resources::LIBRARY,
                    resources::SIDEBAR,
                    resources::FOLDERS,
                    resources::DUPLICATES,
                ],
                &item_ids,
            ),
            freed_file_hashes,
        })
    }
}

fn unique_ids(item_ids: &[ItemId]) -> Result<Vec<i64>, String> {
    let ids = item_ids.iter().map(|id| id.0).collect::<BTreeSet<_>>();
    if ids.len() != item_ids.len() {
        return Err("Item selection contains duplicate IDs".to_string());
    }
    Ok(ids.into_iter().collect())
}

fn receipt(revision: u64, resources: &[&str], item_ids: &[i64]) -> MutationReceipt {
    MutationReceipt {
        revision,
        resources: resources.iter().map(|value| (*value).to_string()).collect(),
        item_ids: item_ids.iter().copied().map(ItemId).collect(),
    }
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn new_key(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}:{}", hex::encode(bytes))
}

fn require_root(transaction: &Transaction<'_>, item_id: i64) -> rusqlite::Result<String> {
    transaction
        .query_row(
            "SELECT lifecycle FROM library_root WHERE item_id = ?1",
            [item_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| invalid(format!("Item {item_id} is not a library root")))
}

fn require_collection_root(
    transaction: &Transaction<'_>,
    item_id: i64,
) -> rusqlite::Result<String> {
    transaction
        .query_row(
            "SELECT lr.lifecycle
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             WHERE lr.item_id = ?1 AND li.kind = 'collection'",
            [item_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| invalid(format!("Item {item_id} is not a collection root")))
}

fn require_standalone_media_root(
    transaction: &Transaction<'_>,
    item_id: i64,
) -> rusqlite::Result<()> {
    let found = transaction
        .query_row(
            "SELECT 1
             FROM library_root lr
             JOIN media_asset ma ON ma.item_id = lr.item_id
             WHERE lr.item_id = ?1",
            [item_id],
            |_| Ok(()),
        )
        .optional()?;
    found.ok_or_else(|| invalid(format!("Item {item_id} is not standalone media")))
}

fn require_same_root_lifecycle(
    transaction: &Transaction<'_>,
    item_ids: &[i64],
) -> rusqlite::Result<String> {
    let mut lifecycle = None;
    for item_id in item_ids {
        let current = require_root(transaction, *item_id)?;
        if lifecycle.as_ref().is_some_and(|value| value != &current) {
            return Err(invalid("Collection members must share one lifecycle"));
        }
        lifecycle = Some(current);
    }
    lifecycle.ok_or_else(|| invalid("No items selected"))
}

fn require_folder(transaction: &Transaction<'_>, folder_id: i64) -> rusqlite::Result<()> {
    let found = transaction
        .query_row(
            "SELECT 1 FROM folder WHERE folder_id = ?1",
            [folder_id],
            |_| Ok(()),
        )
        .optional()?;
    found.ok_or_else(|| invalid(format!("Folder {folder_id} does not exist")))
}

fn folder_ids_for_roots(
    transaction: &Transaction<'_>,
    item_ids: &[i64],
) -> rusqlite::Result<Vec<i64>> {
    let mut folders = BTreeSet::new();
    let mut stmt = transaction.prepare("SELECT folder_id FROM folder_item WHERE item_id = ?1")?;
    for item_id in item_ids {
        for row in stmt.query_map([item_id], |row| row.get::<_, i64>(0))? {
            folders.insert(row?);
        }
    }
    Ok(folders.into_iter().collect())
}

fn collection_members(
    transaction: &Transaction<'_>,
    collection_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = transaction.prepare(
        "SELECT media_item_id FROM collection_member
         WHERE collection_id = ?1 ORDER BY position_rank, media_item_id",
    )?;
    let members = stmt.query_map([collection_id], |row| row.get(0))?.collect();
    members
}

fn create_root_with_folders(
    transaction: &Transaction<'_>,
    item_id: i64,
    lifecycle: &str,
    folder_ids: &[i64],
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
        params![item_id, lifecycle],
    )?;
    for folder_id in folder_ids {
        transaction.execute(
            "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)
             ON CONFLICT DO NOTHING",
            params![folder_id, item_id],
        )?;
    }
    Ok(())
}

fn dissolve_small_collection(
    transaction: &Transaction<'_>,
    collection_id: i64,
    affected: &mut Vec<i64>,
) -> rusqlite::Result<()> {
    let members = collection_members(transaction, collection_id)?;
    if members.len() > 1 {
        if let Some(cover) = transaction
            .query_row(
                "SELECT cover_media_item_id FROM library_item WHERE item_id = ?1",
                [collection_id],
                |row| row.get::<_, Option<i64>>(0),
            )?
            .filter(|cover| members.contains(cover))
        {
            let _ = cover;
        } else {
            transaction.execute(
                "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                params![members[0], collection_id],
            )?;
        }
        return Ok(());
    }

    let lifecycle = require_collection_root(transaction, collection_id)?;
    let folders = folder_ids_for_roots(transaction, &[collection_id])?;
    if let Some(member) = members.first().copied() {
        transaction.execute(
            "DELETE FROM collection_member WHERE collection_id = ?1 AND media_item_id = ?2",
            params![collection_id, member],
        )?;
        create_root_with_folders(transaction, member, &lifecycle, &folders)?;
        affected.push(member);
    }
    transaction.execute(
        "DELETE FROM library_item WHERE item_id = ?1",
        [collection_id],
    )?;
    Ok(())
}

fn media_items_for_roots(
    transaction: &Transaction<'_>,
    root_ids: &[i64],
) -> rusqlite::Result<Vec<i64>> {
    let mut media_ids = BTreeSet::new();
    for root_id in root_ids {
        require_root(transaction, *root_id)?;
        if transaction
            .query_row(
                "SELECT 1 FROM media_asset WHERE item_id = ?1",
                [root_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            media_ids.insert(*root_id);
        } else {
            media_ids.extend(collection_members(transaction, *root_id)?);
        }
    }
    Ok(media_ids.into_iter().collect())
}

fn parse_tag(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (namespace, subtag) = trimmed
        .split_once(':')
        .map(|(namespace, subtag)| (namespace.trim(), subtag.trim()))
        .unwrap_or(("general", trimmed));
    if namespace.is_empty() || subtag.is_empty() {
        return None;
    }
    Some((namespace.to_lowercase(), subtag.to_lowercase()))
}

fn parse_lifecycle(value: &str) -> rusqlite::Result<Lifecycle> {
    match value {
        "inbox" => Ok(Lifecycle::Inbox),
        "active" => Ok(Lifecycle::Active),
        "trash" => Ok(Lifecycle::Trash),
        _ => Err(invalid(format!("Unknown lifecycle: {value}"))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rusqlite::params;

    use super::{DetachItemsInput, GroupItemsInput};
    use crate::app::{
        Application, ItemFilters, ItemId, ItemQuery, ItemScope, ItemSort, ItemTarget, Lifecycle,
    };
    use crate::store::Store;

    fn fixture() -> (tempfile::TempDir, Application, Vec<ItemId>) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let (ids, _) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, created_at, updated_at)
                     VALUES ('folder:a', 'A', 'now', 'now')",
                    [],
                )?;
                let folder_id = transaction.last_insert_rowid();
                let mut ids = Vec::new();
                for index in 0..3 {
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_hash, mime_type, size_bytes, created_at)
                         VALUES (?1, 'image/png', ?2, 'now')",
                        params![format!("hash-{index}"), index + 1],
                    )?;
                    let file_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO library_item
                             (item_key, kind, created_at, updated_at)
                         VALUES (?1, 'media', 'now', 'now')",
                        [format!("item-{index}")],
                    )?;
                    let item_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO media_asset
                             (item_id, file_id, imported_at, updated_at)
                         VALUES (?1, ?2, 'now', 'now')",
                        params![item_id, file_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                        [item_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)",
                        params![folder_id, item_id],
                    )?;
                    ids.push(ItemId(item_id));
                }
                Ok(ids)
            })
            .unwrap();
        (directory, Application::new(store), ids)
    }

    #[test]
    fn grouping_replaces_roots_and_detach_inherits_state_and_folder() {
        let (_directory, app, ids) = fixture();
        let grouped = app
            .group_items(GroupItemsInput {
                item_ids: ids[..2].to_vec(),
                label: Some("Post".to_string()),
            })
            .unwrap();

        app.store()
            .read(|connection| {
                let roots: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE item_id IN (?1, ?2)",
                    params![ids[0].0, ids[1].0],
                    |row| row.get(0),
                )?;
                assert_eq!(roots, 0);
                let collection_folders: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(collection_folders, 1);
                Ok(())
            })
            .unwrap();

        app.set_lifecycle(
            &ItemTarget::Explicit {
                item_ids: vec![grouped.collection_id],
            },
            Lifecycle::Trash,
        )
        .unwrap();
        app.detach_items(DetachItemsInput {
            collection_id: grouped.collection_id,
            media_item_ids: vec![ids[0]],
        })
        .unwrap();

        app.store()
            .read(|connection| {
                for id in &ids[..2] {
                    let lifecycle: String = connection.query_row(
                        "SELECT lifecycle FROM library_root WHERE item_id = ?1",
                        [id.0],
                        |row| row.get(0),
                    )?;
                    assert_eq!(lifecycle, "trash");
                    let folders: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                        [id.0],
                        |row| row.get(0),
                    )?;
                    assert_eq!(folders, 1);
                }
                let collection_exists: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(collection_exists, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn deleting_collection_deletes_members_but_only_unreferenced_files() {
        let (_directory, app, ids) = fixture();
        let grouped = app
            .group_items(GroupItemsInput {
                item_ids: ids[..2].to_vec(),
                label: None,
            })
            .unwrap();
        let result = app
            .delete_items(&ItemTarget::Explicit {
                item_ids: vec![grouped.collection_id],
            })
            .unwrap();
        assert_eq!(result.freed_file_hashes.len(), 2);

        app.store()
            .read(|connection| {
                let items: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM library_item", [], |row| row.get(0))?;
                let files: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?;
                let blob_deletes: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM work_item WHERE work_type = 'blob_delete'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(items, 1);
                assert_eq!(files, 1);
                assert_eq!(blob_deletes, 2);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn query_target_mutation_uses_active_scope_and_exclusions_atomically() {
        let (_directory, app, ids) = fixture();
        app.set_lifecycle(
            &ItemTarget::Query {
                query: ItemQuery {
                    scope: ItemScope::All,
                    filters: ItemFilters::default(),
                    sort: ItemSort::default(),
                },
                excluded_item_ids: vec![ids[0]],
            },
            Lifecycle::Trash,
        )
        .unwrap();

        app.store()
            .read(|connection| {
                let active: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE lifecycle = 'active'",
                    [],
                    |row| row.get(0),
                )?;
                let trash: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE lifecycle = 'trash'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(active, 1);
                assert_eq!(trash, 2);
                Ok(())
            })
            .unwrap();
    }
}
