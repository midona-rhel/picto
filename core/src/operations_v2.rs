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
use crate::store::history::HistoryDescriptor;

const RANK_GAP: i64 = 1024;
const MAX_RECEIPT_ITEM_IDS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct OrganizeIntoCollectionInput {
    pub target: ItemTarget,
    pub label: Option<String>,
    pub winning_collection_id: Option<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct OrganizeIntoCollectionResult {
    pub collection_id: ItemId,
    pub receipt: MutationReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct DetachItemsInput {
    pub collection_id: ItemId,
    pub media_item_ids: Vec<ItemId>,
    pub target_lifecycle: Option<Lifecycle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ReorderCollectionInput {
    pub collection_id: ItemId,
    pub media_item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemRename {
    pub item_id: ItemId,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MediaMetadataPatch {
    #[ts(type = "number | null")]
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
    /// Rename one visible root. Collection names are structural; standalone
    /// media names remain media-owned.
    pub fn rename_item(&self, item_id: ItemId, name: &str) -> Result<MutationReceipt, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("An item name cannot be empty".to_string());
        }
        let item_resource = resources::item(item_id.0);
        let (_, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "items.rename",
                "Rename item",
                vec![resources::LIBRARY.to_string(), item_resource.clone()],
                vec![item_id.0],
            ),
            |transaction| {
                let kind: String = transaction
                    .query_row(
                        "SELECT li.kind FROM library_root lr
                         JOIN library_item li ON li.item_id = lr.item_id
                         WHERE lr.item_id = ?1",
                        [item_id.0],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| invalid(format!("Item {} is not a library root", item_id.0)))?;
                let now = chrono::Utc::now().to_rfc3339();
                match kind.as_str() {
                    "collection" => {
                        transaction.execute(
                            "UPDATE library_item SET label = ?1, updated_at = ?2 WHERE item_id = ?3",
                            params![name, now, item_id.0],
                        )?;
                    }
                    "media" => {
                        transaction.execute(
                            "UPDATE media_asset SET name = ?1, updated_at = ?2 WHERE item_id = ?3",
                            params![name, now, item_id.0],
                        )?;
                    }
                    other => return Err(invalid(format!("Unsupported item kind '{other}'"))),
                }
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, &item_resource],
            &[item_id.0],
        ))
    }

    /// Rename an explicit set of visible roots as one atomic, undoable action.
    pub fn rename_items(&self, renames: &[ItemRename]) -> Result<MutationReceipt, String> {
        if renames.len() < 2 {
            return Err("Batch rename requires at least two items".to_string());
        }
        let mut item_ids = BTreeSet::new();
        let mut normalized = Vec::with_capacity(renames.len());
        for rename in renames {
            if !item_ids.insert(rename.item_id.0) {
                return Err(format!("Item {} appears more than once", rename.item_id.0));
            }
            let name = rename.name.trim();
            if name.is_empty() {
                return Err("An item name cannot be empty".to_string());
            }
            if name.chars().count() > 255 {
                return Err("An item name cannot exceed 255 characters".to_string());
            }
            normalized.push((rename.item_id, name.to_string()));
        }
        let resources_for_history = std::iter::once(resources::LIBRARY.to_string())
            .chain(item_ids.iter().map(|item_id| resources::item(*item_id)))
            .collect();
        let history_ids = item_ids.iter().copied().collect::<Vec<_>>();
        let (_, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "items.rename_many",
                format!("Rename {} items", normalized.len()),
                resources_for_history,
                history_ids.clone(),
            ),
            |transaction| {
                let now = chrono::Utc::now().to_rfc3339();
                for (item_id, name) in &normalized {
                    let kind: String = transaction
                        .query_row(
                            "SELECT li.kind FROM library_root lr
                             JOIN library_item li ON li.item_id = lr.item_id
                             WHERE lr.item_id = ?1",
                            [item_id.0],
                            |row| row.get(0),
                        )
                        .optional()?
                        .ok_or_else(|| invalid(format!("Item {} is not a library root", item_id.0)))?;
                    match kind.as_str() {
                        "collection" => transaction.execute(
                            "UPDATE library_item SET label = ?1, updated_at = ?2 WHERE item_id = ?3",
                            params![name, now, item_id.0],
                        )?,
                        "media" => transaction.execute(
                            "UPDATE media_asset SET name = ?1, updated_at = ?2 WHERE item_id = ?3",
                            params![name, now, item_id.0],
                        )?,
                        other => return Err(invalid(format!("Unsupported item kind '{other}'"))),
                    };
                }
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(receipt(revision, &[resources::LIBRARY], &history_ids))
    }

    pub fn set_lifecycle(
        &self,
        target: &ItemTarget,
        lifecycle: Lifecycle,
    ) -> Result<MutationReceipt, String> {
        let (item_ids, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "items.set_lifecycle",
                match lifecycle {
                    Lifecycle::Trash => "Move to Trash",
                    _ => "Change item status",
                },
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                ],
                vec![],
            )
            .rebuilding_projections(),
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
        let ((item_ids, _changed_tags), revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "items.set_folder",
                if present {
                    "Add to folder"
                } else {
                    "Remove from folder"
                },
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                vec![],
            )
            .rebuilding_projections(),
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
                let changed_tags = if present {
                    let tags =
                        crate::folders_v2::inherited_folder_auto_tags(transaction, folder_id)?;
                    if tags.is_empty() {
                        Vec::new()
                    } else {
                        let media_ids = media_items_for_roots(transaction, &item_ids)?;
                        apply_tags_in(transaction, &media_ids, &tags, true, 1, "folder")?
                    }
                } else {
                    Vec::new()
                };
                Ok((
                    (item_ids.clone(), changed_tags.clone()),
                    (item_ids, changed_tags),
                ))
            },
            |projections, (changed_ids, changed_tags)| {
                for item_id in changed_ids {
                    projections.apply_folder_delta(folder_id, item_id, present)?;
                }
                projections.apply_tag_changes(&changed_tags, true)?;
                Ok(())
            },
        )?;
        Ok(receipt(
            revision,
            &[
                resources::LIBRARY,
                resources::SIDEBAR,
                resources::FOLDERS,
                resources::TAGS,
                resources::SMART_FOLDERS,
            ],
            &item_ids,
        ))
    }

    pub fn organize_into_collection(
        &self,
        input: OrganizeIntoCollectionInput,
    ) -> Result<OrganizeIntoCollectionResult, String> {
        let label = input
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_owned);
        let now = chrono::Utc::now().to_rfc3339();
        let key = new_key("collection");
        let ((collection_id, affected), revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "collections.organize",
                "Create or merge group",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                Vec::new(),
            )
            .rebuilding_projections(),
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, &input.target)?;
                let mut collection_ids = Vec::new();
                let mut standalone_media_ids = Vec::new();
                let mut members_by_collection = Vec::new();
                for item_id in &item_ids {
                    let kind: String = transaction.query_row(
                        "SELECT kind FROM library_item WHERE item_id = ?1",
                        [item_id],
                        |row| row.get(0),
                    )?;
                    match kind.as_str() {
                        "media" => {
                            require_standalone_media_root(transaction, *item_id)?;
                            standalone_media_ids.push(*item_id);
                        }
                        "collection" => {
                            let members = collection_members(transaction, *item_id)?;
                            collection_ids.push(*item_id);
                            members_by_collection.push((*item_id, members));
                        }
                        other => return Err(invalid(format!("Unsupported item kind '{other}'"))),
                    }
                }
                let mut member_groups = standalone_media_ids
                    .iter()
                    .map(|media_item_id| vec![*media_item_id])
                    .collect::<Vec<_>>();
                member_groups.extend(
                    members_by_collection
                        .iter()
                        .map(|(_, members)| members.clone()),
                );
                require_no_collection_file_overlap(transaction, &member_groups)?;
                if collection_ids.is_empty() && item_ids.len() < 2 {
                    return Err(invalid(
                        "Creating a group requires at least two standalone media items",
                    ));
                }
                if collection_ids.is_empty() && input.winning_collection_id.is_some() {
                    return Err(invalid(
                        "A winning group can only be selected when the target contains a group",
                    ));
                }
                let lifecycle = require_same_root_lifecycle(transaction, &item_ids)?;
                let projected_lifecycle = parse_lifecycle(&lifecycle)?;
                let mut delta = StructureProjectionDelta::default();
                let collection_id = if collection_ids.is_empty() {
                    let label = label
                        .as_deref()
                        .ok_or_else(|| invalid("A new group requires a non-empty label"))?;
                    let folders = folder_ids_for_roots(transaction, &item_ids)?;
                    transaction.execute(
                        "INSERT INTO library_item (item_key, kind, label, created_at, updated_at)
                         VALUES (?1, 'collection', ?2, ?3, ?3)",
                        params![key, label, now],
                    )?;
                    let collection_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
                        params![collection_id, lifecycle],
                    )?;
                    for folder_id in &folders {
                        transaction.execute(
                            "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)",
                            params![folder_id, collection_id],
                        )?;
                    }
                    delta.items.push(ItemProjectionChange {
                        item_id: collection_id,
                        kind: crate::app::ItemKind::Collection,
                        present: true,
                    });
                    delta.roots.push(RootProjectionChange {
                        item_id: collection_id,
                        lifecycle: Some(projected_lifecycle),
                    });
                    delta.folders.extend(folders.into_iter().map(|folder_id| {
                        FolderProjectionChange {
                            folder_id,
                            item_id: collection_id,
                            present: true,
                        }
                    }));
                    collection_id
                } else {
                    let collection_id = match input.winning_collection_id {
                        Some(winner) if !collection_ids.contains(&winner.0) => {
                            return Err(invalid(
                                "The winning group must be one of the selected groups",
                            ));
                        }
                        Some(winner) => winner.0,
                        None if collection_ids.len() == 1 => collection_ids[0],
                        None => {
                            return Err(invalid(
                                "Select which group should own the merged members",
                            ));
                        }
                    };
                    if standalone_media_ids.is_empty()
                        && collection_ids.iter().all(|id| *id == collection_id)
                    {
                        return Err(invalid(
                            "Organizing a group requires at least one additional item",
                        ));
                    }
                    collection_id
                };

                let mut next_position: i64 = transaction.query_row(
                    "SELECT COALESCE(MAX(position_rank), 0) + ?1
                     FROM collection_member WHERE collection_id = ?2",
                    params![RANK_GAP, collection_id],
                    |row| row.get(0),
                )?;
                let mut affected = vec![collection_id];
                for item_id in &item_ids {
                    affected.push(*item_id);
                    if *item_id == collection_id {
                        continue;
                    }
                    let folders = folder_ids_for_roots(transaction, &[*item_id])?;
                    let kind: String = transaction.query_row(
                        "SELECT kind FROM library_item WHERE item_id = ?1",
                        [item_id],
                        |row| row.get(0),
                    )?;
                    if kind == "media" {
                        transaction
                            .execute("DELETE FROM library_root WHERE item_id = ?1", [item_id])?;
                        transaction.execute(
                            "INSERT INTO collection_member
                             (collection_id, media_item_id, position_rank)
                             VALUES (?1, ?2, ?3)",
                            params![collection_id, item_id, next_position],
                        )?;
                        delta.roots.push(RootProjectionChange {
                            item_id: *item_id,
                            lifecycle: None,
                        });
                        delta.memberships.push(MembershipProjectionChange {
                            collection_id,
                            media_id: *item_id,
                            present: true,
                        });
                        delta.folders.extend(folders.into_iter().map(|folder_id| {
                            FolderProjectionChange {
                                folder_id,
                                item_id: *item_id,
                                present: false,
                            }
                        }));
                        affected.push(*item_id);
                        next_position += RANK_GAP;
                        continue;
                    }

                    let members = members_by_collection
                        .iter()
                        .find(|(id, _)| id == item_id)
                        .map(|(_, members)| members.clone())
                        .unwrap_or_default();
                    if *item_id != collection_id {
                        for media_id in members {
                            transaction.execute(
                                "UPDATE collection_member
                                 SET collection_id = ?1, position_rank = ?2
                                 WHERE collection_id = ?3 AND media_item_id = ?4",
                                params![collection_id, next_position, item_id, media_id],
                            )?;
                            delta.memberships.push(MembershipProjectionChange {
                                collection_id: *item_id,
                                media_id,
                                present: false,
                            });
                            delta.memberships.push(MembershipProjectionChange {
                                collection_id,
                                media_id,
                                present: true,
                            });
                            affected.push(media_id);
                            next_position += RANK_GAP;
                        }
                        transaction
                            .execute("DELETE FROM library_item WHERE item_id = ?1", [item_id])?;
                        delta.items.push(ItemProjectionChange {
                            item_id: *item_id,
                            kind: crate::app::ItemKind::Collection,
                            present: false,
                        });
                        delta.roots.push(RootProjectionChange {
                            item_id: *item_id,
                            lifecycle: None,
                        });
                        delta.folders.extend(folders.into_iter().map(|folder_id| {
                            FolderProjectionChange {
                                folder_id,
                                item_id: *item_id,
                                present: false,
                            }
                        }));
                    }
                }

                sync_collection_cover(transaction, collection_id)?;
                affected.sort_unstable();
                affected.dedup();
                Ok(((collection_id, affected), delta))
            },
            |projections, delta| projections.apply_structure_delta(delta),
        )?;
        let receipt = receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &affected,
        );
        Ok(OrganizeIntoCollectionResult {
            collection_id: ItemId(collection_id),
            receipt,
        })
    }

    pub fn detach_items(&self, input: DetachItemsInput) -> Result<MutationReceipt, String> {
        let media_ids = unique_ids(&input.media_item_ids)?;
        if media_ids.is_empty() {
            return Err("No group members were selected".to_string());
        }
        let (affected, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "collections.detach",
                "Remove from group",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                Vec::new(),
            )
            .rebuilding_projections(),
            |transaction| {
                let lifecycle = require_collection_root(transaction, input.collection_id.0)?;
                let projected_lifecycle = parse_lifecycle(&lifecycle)?;
                let detached_lifecycle = input.target_lifecycle.unwrap_or(projected_lifecycle);
                let detached_lifecycle_name = detached_lifecycle.as_str();
                let folders = folder_ids_for_roots(transaction, &[input.collection_id.0])?;
                let mut delta = StructureProjectionDelta::default();
                for media_id in &media_ids {
                    let removed = transaction.execute(
                        "DELETE FROM collection_member
                         WHERE collection_id = ?1 AND media_item_id = ?2",
                        params![input.collection_id.0, media_id],
                    )?;
                    if removed != 1 {
                        return Err(invalid(format!(
                            "Media item {media_id} is not attached to group {}",
                            input.collection_id.0
                        )));
                    }
                    create_root_with_folders(
                        transaction,
                        *media_id,
                        detached_lifecycle_name,
                        &folders,
                    )?;
                    project_detached_root(
                        &mut delta,
                        input.collection_id.0,
                        *media_id,
                        detached_lifecycle,
                        &folders,
                    );
                }

                let mut affected = media_ids.clone();
                affected.push(input.collection_id.0);
                let remaining = collection_members(transaction, input.collection_id.0)?;
                if remaining.len() > 1 {
                    sync_collection_cover(transaction, input.collection_id.0)?;
                } else {
                    if let Some(media_id) = remaining.first().copied() {
                        transaction.execute(
                            "DELETE FROM collection_member
                             WHERE collection_id = ?1 AND media_item_id = ?2",
                            params![input.collection_id.0, media_id],
                        )?;
                        create_root_with_folders(transaction, media_id, &lifecycle, &folders)?;
                        project_detached_root(
                            &mut delta,
                            input.collection_id.0,
                            media_id,
                            projected_lifecycle,
                            &folders,
                        );
                        affected.push(media_id);
                    }
                    transaction.execute(
                        "DELETE FROM library_item WHERE item_id = ?1",
                        [input.collection_id.0],
                    )?;
                    project_removed_collection(&mut delta, input.collection_id.0, &folders);
                }
                Ok((affected, delta))
            },
            |projections, delta| projections.apply_structure_delta(delta),
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &affected,
        ))
    }

    pub fn ungroup_collection(&self, collection_id: ItemId) -> Result<MutationReceipt, String> {
        let (affected, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "collections.ungroup",
                "Ungroup",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                vec![collection_id.0],
            )
            .rebuilding_projections(),
            |transaction| {
                let lifecycle = require_collection_root(transaction, collection_id.0)?;
                let projected_lifecycle = parse_lifecycle(&lifecycle)?;
                let folders = folder_ids_for_roots(transaction, &[collection_id.0])?;
                let members = collection_members(transaction, collection_id.0)?;
                let mut delta = StructureProjectionDelta::default();
                for member in &members {
                    create_root_with_folders(transaction, *member, &lifecycle, &folders)?;
                    project_detached_root(
                        &mut delta,
                        collection_id.0,
                        *member,
                        projected_lifecycle,
                        &folders,
                    );
                }
                transaction.execute(
                    "DELETE FROM library_item WHERE item_id = ?1",
                    [collection_id.0],
                )?;
                project_removed_collection(&mut delta, collection_id.0, &folders);
                let mut affected = members;
                affected.push(collection_id.0);
                Ok((affected, delta))
            },
            |projections, delta| projections.apply_structure_delta(delta),
        )?;
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
        let (_, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "collections.reorder",
                "Reorder group",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::item(input.collection_id.0),
                ],
                vec![input.collection_id.0],
            ),
            |transaction| {
                require_collection_root(transaction, input.collection_id.0)?;
                let existing = collection_members(transaction, input.collection_id.0)?;
                if existing.len() != media_ids.len()
                    || existing.iter().copied().collect::<BTreeSet<_>>()
                        != media_ids.iter().copied().collect::<BTreeSet<_>>()
                {
                    return Err(invalid(
                        "Reorder must contain every group member exactly once",
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
                sync_collection_cover(transaction, input.collection_id.0)?;
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

    pub fn apply_tags(
        &self,
        target: &ItemTarget,
        tags: &[String],
        add: bool,
        provenance_mask: i64,
    ) -> Result<MutationReceipt, String> {
        let tags = tags
            .iter()
            .filter_map(|tag| crate::tag_name_v2::parse_local(tag).ok())
            .collect::<Vec<_>>();
        if tags.is_empty() {
            return Err("No valid tags were provided".to_string());
        }
        let (item_ids, revision, _, _) = self.undoable_transaction_if_changed(
            HistoryDescriptor::new(
                "items.apply_tags",
                if add { "Add tags" } else { "Remove tags" },
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::TAGS.to_string(),
                    resources::SMART_FOLDERS.to_string(),
                ],
                vec![],
            )
            .rebuilding_projections(),
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                let media_ids = media_items_for_roots(transaction, &item_ids)?;
                let changed_tags = apply_tags_in(
                    transaction,
                    &media_ids,
                    &tags,
                    add,
                    provenance_mask,
                    "local",
                )?;
                let changed = !changed_tags.is_empty();
                Ok((item_ids, changed_tags, changed))
            },
            |projections, changed_tags| projections.apply_tag_changes(&changed_tags, add),
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

    /// Internal media-owned write used by per-media workers. User collection
    /// writes continue to use `apply_tags`, which intentionally fans out.
    pub(crate) fn apply_media_tags(
        &self,
        media_item_id: ItemId,
        tags: &[String],
        provenance_mask: i64,
    ) -> Result<MutationReceipt, String> {
        let tags = tags
            .iter()
            .filter_map(|tag| crate::tag_name_v2::parse_external(tag).ok())
            .collect::<Vec<_>>();
        if tags.is_empty() {
            return Err("No valid tags were provided".to_string());
        }
        let (root_id, revision, _) = self.transaction_if_changed(
            |transaction| {
                let root_id = root_for_media(transaction, media_item_id.0)?;
                let changed_tags = apply_tags_in(
                    transaction,
                    &[media_item_id.0],
                    &tags,
                    true,
                    provenance_mask,
                    "ai",
                )?;
                let changed = !changed_tags.is_empty();
                Ok((root_id, changed_tags, changed))
            },
            |projections, changed_tags| projections.apply_tag_changes(&changed_tags, true),
        )?;
        Ok(receipt(
            revision,
            &[
                resources::LIBRARY,
                resources::SIDEBAR,
                resources::TAGS,
                resources::SMART_FOLDERS,
            ],
            &[root_id],
        ))
    }

    pub(crate) fn apply_media_tag_assignments(
        &self,
        assignments: &[(ItemId, Vec<String>)],
        provenance_mask: i64,
    ) -> Result<MutationReceipt, String> {
        if assignments.is_empty() {
            return Err("At least one AI tag assignment is required".to_string());
        }
        let normalized = assignments
            .iter()
            .map(|(media_item_id, tags)| {
                let tags = tags
                    .iter()
                    .map(|tag| {
                        crate::tag_name_v2::parse_external(tag).map_err(|error| {
                            format!(
                                "Invalid AI tag '{tag}' for media item {}: {error}",
                                media_item_id.0
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if tags.is_empty() {
                    return Err(format!(
                        "Media item {} has no valid AI tags",
                        media_item_id.0
                    ));
                }
                Ok((*media_item_id, tags))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let (root_ids, revision, _, _) = self.undoable_transaction_if_changed(
            HistoryDescriptor::new(
                "items.apply_ai_tags",
                "Apply AI tags",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::TAGS.to_string(),
                    resources::SMART_FOLDERS.to_string(),
                ],
                vec![],
            )
            .rebuilding_projections(),
            |transaction| {
                let mut root_ids = BTreeSet::new();
                let mut changed_tags = Vec::new();
                for (media_item_id, tags) in &normalized {
                    root_ids.insert(root_for_media(transaction, media_item_id.0)?);
                    changed_tags.extend(apply_tags_in(
                        transaction,
                        &[media_item_id.0],
                        tags,
                        true,
                        provenance_mask,
                        "ai",
                    )?);
                }
                let changed = !changed_tags.is_empty();
                Ok((
                    root_ids.into_iter().collect::<Vec<_>>(),
                    changed_tags,
                    changed,
                ))
            },
            |projections, changed_tags| projections.apply_tag_changes(&changed_tags, true),
        )?;
        Ok(receipt(
            revision,
            &[
                resources::LIBRARY,
                resources::SIDEBAR,
                resources::TAGS,
                resources::SMART_FOLDERS,
            ],
            &root_ids,
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
        let label = if patch.rating.is_some()
            && patch.notes.is_none()
            && patch.source_urls.is_none()
        {
            "Change rating"
        } else if patch.rating.is_none() && patch.notes.is_some() && patch.source_urls.is_none() {
            "Edit notes"
        } else if patch.rating.is_none() && patch.notes.is_none() && patch.source_urls.is_some() {
            "Edit source"
        } else {
            "Edit metadata"
        };
        let (item_ids, revision, _) = self.undoable_transaction(
            HistoryDescriptor::new(
                "items.patch_metadata",
                label,
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::SMART_FOLDERS.to_string(),
                ],
                vec![],
            ),
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
        // Permanent media deletion is deliberately outside application
        // history. It removes unreferenced blobs and must never imply a
        // recoverable Undo action.
        let ((freed_file_hashes, item_ids), revision) = self.transaction(
            |transaction| {
                let item_ids = crate::query_v2::resolve_target_ids(transaction, target)?;
                let roots_json = serde_json::to_string(&item_ids).map_err(|error| {
                    invalid(format!("Could not encode deleted root IDs: {error}"))
                })?;
                let mut delete_items = BTreeSet::new();
                let mut delta = StructureProjectionDelta::default();
                {
                    let mut statement = transaction.prepare(
                        "SELECT lr.item_id, li.kind, cm.media_item_id
                         FROM json_each(?1) requested
                         JOIN library_root lr
                           ON lr.item_id = CAST(requested.value AS INTEGER)
                         JOIN library_item li ON li.item_id = lr.item_id
                         LEFT JOIN collection_member cm ON cm.collection_id = lr.item_id
                         ORDER BY CAST(requested.key AS INTEGER), cm.position_rank, cm.media_item_id",
                    )?;
                    let rows = statement.query_map([&roots_json], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    })?;
                    let mut projected_roots = BTreeSet::new();
                    for row in rows {
                        let (root_id, kind, member_id) = row?;
                        delete_items.insert(root_id);
                        if projected_roots.insert(root_id) {
                            let kind = match kind.as_str() {
                                "media" => crate::app::ItemKind::Media,
                                "collection" => crate::app::ItemKind::Collection,
                                other => {
                                    return Err(invalid(format!(
                                        "Unsupported item kind '{other}'"
                                    )))
                                }
                            };
                            delta.items.push(ItemProjectionChange {
                                item_id: root_id,
                                kind,
                                present: false,
                            });
                            delta.roots.push(RootProjectionChange {
                                item_id: root_id,
                                lifecycle: None,
                            });
                        }
                        if let Some(member_id) = member_id {
                            delete_items.insert(member_id);
                            delta.memberships.push(MembershipProjectionChange {
                                collection_id: root_id,
                                media_id: member_id,
                                present: false,
                            });
                            delta.items.push(ItemProjectionChange {
                                item_id: member_id,
                                kind: crate::app::ItemKind::Media,
                                present: false,
                            });
                        }
                    }
                    if projected_roots.len() != item_ids.len() {
                        return Err(invalid("A targeted item is not a library root"));
                    }
                }
                {
                    let mut statement = transaction.prepare(
                        "SELECT fi.item_id, fi.folder_id
                         FROM folder_item fi
                         JOIN json_each(?1) requested
                           ON fi.item_id = CAST(requested.value AS INTEGER)
                         ORDER BY CAST(requested.key AS INTEGER), fi.folder_id",
                    )?;
                    let rows = statement.query_map([&roots_json], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                    })?;
                    for row in rows {
                        let (item_id, folder_id) = row?;
                        delta.folders.push(FolderProjectionChange {
                            folder_id,
                            item_id,
                            present: false,
                        });
                    }
                }
                let affected_item_ids = delete_items.iter().copied().collect::<Vec<_>>();
                let affected_json = serde_json::to_string(&affected_item_ids).map_err(|error| {
                    invalid(format!("Could not encode deleted item IDs: {error}"))
                })?;
                let candidate_file_ids = {
                    let mut statement = transaction.prepare(
                        "SELECT DISTINCT mf.file_id
                         FROM media_asset ma
                         JOIN media_file mf ON mf.file_id = ma.file_id
                         WHERE ma.item_id IN (
                             SELECT CAST(value AS INTEGER) FROM json_each(?1)
                         )",
                    )?;
                    let file_ids = statement
                        .query_map([&affected_json], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    file_ids
                };
                let deleted_source_item_ids = {
                    let mut statement = transaction.prepare(
                        "SELECT source_item_id FROM source_item
                         WHERE media_item_id IN (
                             SELECT CAST(value AS INTEGER) FROM json_each(?1)
                         )",
                    )?;
                    let source_item_ids = statement
                        .query_map([&affected_json], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    source_item_ids
                };
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "UPDATE source_item
                     SET state = 'deleted', media_item_id = NULL, updated_at = ?1
                     WHERE media_item_id IN (
                         SELECT CAST(value AS INTEGER) FROM json_each(?2)
                     )",
                    params![&now, &affected_json],
                )?;
                crate::cloud::capture::record_source_item_deletes(
                    transaction,
                    &deleted_source_item_ids,
                )?;
                transaction.execute(
                    "DELETE FROM library_item
                     WHERE item_id IN (
                         SELECT CAST(value AS INTEGER) FROM json_each(?1)
                     )",
                    [&affected_json],
                )?;

                let candidate_files_json = serde_json::to_string(&candidate_file_ids)
                    .map_err(|error| invalid(format!("Could not encode deleted files: {error}")))?;
                let hashes = {
                    let mut statement = transaction.prepare(
                        "DELETE FROM media_file
                         WHERE file_id IN (
                             SELECT CAST(value AS INTEGER) FROM json_each(?1)
                         )
                           AND NOT EXISTS (
                               SELECT 1 FROM media_asset
                               WHERE media_asset.file_id = media_file.file_id
                           )
                         RETURNING file_hash",
                    )?;
                    let hashes = statement
                        .query_map([candidate_files_json], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    hashes
                };
                if !hashes.is_empty() {
                    let hashes_json = serde_json::to_string(&hashes).map_err(|error| {
                        invalid(format!("Could not encode deleted file hashes: {error}"))
                    })?;
                    transaction.execute(
                        "INSERT INTO work_item (
                             file_hash, work_type, status, attempt_count,
                             available_at, created_at, updated_at
                         )
                         SELECT value, 'blob_delete', 'pending', 0, ?2, ?2, ?2
                         FROM json_each(?1) WHERE 1
                         ON CONFLICT(file_hash, work_type) WHERE file_hash IS NOT NULL
                         DO NOTHING",
                        params![hashes_json, now],
                    )?;
                }
                Ok(((hashes, affected_item_ids), delta))
            },
            |projections, delta| projections.apply_structure_delta(delta),
        )?;
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
    let ids = item_ids.iter().map(|id| id.0).collect::<Vec<_>>();
    if ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err("Item selection contains duplicate IDs".to_string());
    }
    Ok(ids)
}

fn receipt(revision: u64, resources: &[&str], item_ids: &[i64]) -> MutationReceipt {
    MutationReceipt {
        revision,
        resources: resources.iter().map(|value| (*value).to_string()).collect(),
        item_ids: if item_ids.len() <= MAX_RECEIPT_ITEM_IDS {
            item_ids.iter().copied().map(ItemId).collect()
        } else {
            Vec::new()
        },
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
        .ok_or_else(|| invalid(format!("Item {item_id} is not a group root")))
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
            return Err(invalid("Group members must share one lifecycle"));
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

fn require_no_collection_file_overlap(
    transaction: &Transaction<'_>,
    member_groups: &[Vec<i64>],
) -> rusqlite::Result<()> {
    let mut file_ids = BTreeSet::new();
    for members in member_groups {
        let mut group_file_ids = BTreeSet::new();
        for media_item_id in members {
            let file_id: i64 = transaction.query_row(
                "SELECT file_id FROM media_asset WHERE item_id = ?1",
                [media_item_id],
                |row| row.get(0),
            )?;
            group_file_ids.insert(file_id);
        }
        for file_id in group_file_ids {
            if !file_ids.insert(file_id) {
                return Err(invalid(
                    "A group cannot contain the same physical file more than once",
                ));
            }
        }
    }
    Ok(())
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

pub(crate) fn sync_collection_cover(
    transaction: &Transaction<'_>,
    collection_id: i64,
) -> rusqlite::Result<()> {
    let first_member = transaction
        .query_row(
            "SELECT media_item_id FROM collection_member
             WHERE collection_id = ?1
             ORDER BY position_rank, media_item_id
             LIMIT 1",
            [collection_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    transaction.execute(
        "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
        params![first_member, collection_id],
    )?;
    Ok(())
}

fn project_detached_root(
    delta: &mut StructureProjectionDelta,
    collection_id: i64,
    media_id: i64,
    lifecycle: Lifecycle,
    folder_ids: &[i64],
) {
    delta.memberships.push(MembershipProjectionChange {
        collection_id,
        media_id,
        present: false,
    });
    delta.roots.push(RootProjectionChange {
        item_id: media_id,
        lifecycle: Some(lifecycle),
    });
    delta
        .folders
        .extend(folder_ids.iter().map(|folder_id| FolderProjectionChange {
            folder_id: *folder_id,
            item_id: media_id,
            present: true,
        }));
}

fn project_removed_collection(
    delta: &mut StructureProjectionDelta,
    collection_id: i64,
    folder_ids: &[i64],
) {
    delta.roots.push(RootProjectionChange {
        item_id: collection_id,
        lifecycle: None,
    });
    delta.items.push(ItemProjectionChange {
        item_id: collection_id,
        kind: crate::app::ItemKind::Collection,
        present: false,
    });
    delta
        .folders
        .extend(folder_ids.iter().map(|folder_id| FolderProjectionChange {
            folder_id: *folder_id,
            item_id: collection_id,
            present: false,
        }));
}

pub(crate) fn media_items_for_roots(
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

pub(crate) fn apply_tags_in(
    transaction: &Transaction<'_>,
    media_ids: &[i64],
    tags: &[(String, String)],
    add: bool,
    provenance_mask: i64,
    source: &str,
) -> rusqlite::Result<Vec<(i64, i64)>> {
    let mut changed_tags = Vec::new();
    let media_ids_json = serde_json::to_string(media_ids)
        .map_err(|error| invalid(format!("Could not encode tag targets: {error}")))?;
    for (namespace, subtag) in tags {
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
        let changed_media = if add {
            let mut statement = transaction.prepare(
                "INSERT INTO media_tag (media_item_id, tag_id, source, provenance_mask)
                 SELECT CAST(value AS INTEGER), ?2, ?3, ?4 FROM json_each(?1) WHERE 1
                 ON CONFLICT(media_item_id, tag_id, source) DO UPDATE SET
                     provenance_mask = media_tag.provenance_mask | excluded.provenance_mask
                 WHERE media_tag.provenance_mask <>
                     (media_tag.provenance_mask | excluded.provenance_mask)
                 RETURNING media_item_id",
            )?;
            let media_ids = statement
                .query_map(
                    params![media_ids_json, tag_id, source, provenance_mask],
                    |row| row.get::<_, i64>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            media_ids
        } else {
            let mut statement = transaction.prepare(
                "DELETE FROM media_tag
                 WHERE tag_id = ?2
                   AND media_item_id IN (
                       SELECT CAST(value AS INTEGER) FROM json_each(?1)
                   )
                 RETURNING media_item_id",
            )?;
            let media_ids = statement
                .query_map(params![media_ids_json, tag_id], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            media_ids
        };
        changed_tags.extend(changed_media.into_iter().map(|media_id| (media_id, tag_id)));
    }
    Ok(changed_tags)
}

fn root_for_media(transaction: &Transaction<'_>, media_item_id: i64) -> rusqlite::Result<i64> {
    transaction.query_row(
        "SELECT COALESCE(cm.collection_id, lr.item_id)
         FROM media_asset ma
         LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
         LEFT JOIN library_root lr ON lr.item_id = ma.item_id
         WHERE ma.item_id = ?1 AND (cm.collection_id IS NOT NULL OR lr.item_id IS NOT NULL)",
        [media_item_id],
        |row| row.get(0),
    )
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
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use rusqlite::params;

    use super::{
        DetachItemsInput, ItemRename, OrganizeIntoCollectionInput, OrganizeIntoCollectionResult,
        ReorderCollectionInput,
    };
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

    fn organize(
        app: &Application,
        item_ids: &[ItemId],
        label: Option<&str>,
        winning_collection_id: Option<ItemId>,
    ) -> OrganizeIntoCollectionResult {
        app.organize_into_collection(OrganizeIntoCollectionInput {
            target: ItemTarget::Explicit {
                item_ids: item_ids.to_vec(),
            },
            label: Some(label.unwrap_or("Test").to_string()),
            winning_collection_id,
        })
        .unwrap()
    }

    fn add_media_root(app: &Application, key: &str) -> ItemId {
        let (item_id, _) = app
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES (?1, 'image/png', 1, 'now')",
                    [format!("hash-{key}")],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES (?1, 'media', 'now', 'now')",
                    [format!("item-{key}")],
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
                Ok(item_id)
            })
            .unwrap();
        ItemId(item_id)
    }

    #[test]
    fn grouping_replaces_roots_and_detach_inherits_state_and_folder() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], Some("Post"), None);

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
        let detached = app
            .detach_items(DetachItemsInput {
                collection_id: grouped.collection_id,
                media_item_ids: vec![ids[0]],
                target_lifecycle: None,
            })
            .unwrap();
        assert_eq!(
            detached.item_ids,
            vec![ids[0], grouped.collection_id, ids[1]]
        );
        let trash = app.projections().trash_bitmap();
        assert!(trash.contains(ids[0].0 as u32));
        assert!(trash.contains(ids[1].0 as u32));
        assert!(!trash.contains(grouped.collection_id.0 as u32));

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
    fn grouping_round_trips_through_application_history() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], Some("Post"), None);

        app.undo().unwrap();
        app.store()
            .read(|connection| {
                let roots: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE item_id IN (?1, ?2)",
                    params![ids[0].0, ids[1].0],
                    |row| row.get(0),
                )?;
                let collection: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(roots, 2);
                assert_eq!(collection, 0);
                Ok(())
            })
            .unwrap();

        app.redo().unwrap();
        app.store()
            .read(|connection| {
                let members: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM collection_member WHERE collection_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(members, 2);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn detaching_to_trash_only_changes_the_selected_members_lifecycle() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids, Some("Post"), None);

        app.detach_items(DetachItemsInput {
            collection_id: grouped.collection_id,
            media_item_ids: vec![ids[0]],
            target_lifecycle: Some(Lifecycle::Trash),
        })
        .unwrap();

        app.store()
            .read(|connection| {
                let detached_lifecycle: String = connection.query_row(
                    "SELECT lifecycle FROM library_root WHERE item_id = ?1",
                    [ids[0].0],
                    |row| row.get(0),
                )?;
                assert_eq!(detached_lifecycle, "trash");

                let collection_lifecycle: String = connection.query_row(
                    "SELECT lifecycle FROM library_root WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(collection_lifecycle, "active");

                let remaining_members: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM collection_member WHERE collection_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(remaining_members, 2);
                Ok(())
            })
            .unwrap();

        assert!(app.projections().trash_bitmap().contains(ids[0].0 as u32));
        assert!(app
            .projections()
            .active_bitmap()
            .contains(grouped.collection_id.0 as u32));
    }

    #[test]
    fn organizing_media_into_existing_collection_preserves_winner_structure() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], Some("Post"), None);
        let result = app
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: ItemTarget::Explicit {
                    item_ids: vec![grouped.collection_id, ids[2]],
                },
                label: Some("Ignored label".to_string()),
                winning_collection_id: None,
            })
            .unwrap();

        assert_eq!(result.collection_id, grouped.collection_id);
        app.store()
            .read(|connection| {
                let label: String = connection.query_row(
                    "SELECT label FROM library_item WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(label, "Post");
                let members: Vec<i64> = connection
                    .prepare(
                        "SELECT media_item_id FROM collection_member
                         WHERE collection_id = ?1 ORDER BY position_rank",
                    )?
                    .query_map([grouped.collection_id.0], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(members, vec![ids[0].0, ids[1].0, ids[2].0]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn user_cannot_create_collection_with_repeated_physical_file() {
        let (_directory, app, ids) = fixture();
        app.store()
            .transaction(|transaction| {
                let first_file_id: i64 = transaction.query_row(
                    "SELECT file_id FROM media_asset WHERE item_id = ?1",
                    [ids[0].0],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "UPDATE media_asset SET file_id = ?1 WHERE item_id = ?2",
                    params![first_file_id, ids[1].0],
                )?;
                Ok(())
            })
            .unwrap();

        let error = app
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: ItemTarget::Explicit {
                    item_ids: ids[..2].to_vec(),
                },
                label: Some("Repeated".to_string()),
                winning_collection_id: None,
            })
            .unwrap_err();
        assert!(error.contains("same physical file more than once"));
    }

    #[test]
    fn user_cannot_add_a_repeated_physical_file_to_a_collection() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], Some("Existing"), None);
        app.store()
            .transaction(|transaction| {
                let member_file_id: i64 = transaction.query_row(
                    "SELECT file_id FROM media_asset WHERE item_id = ?1",
                    [ids[0].0],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "UPDATE media_asset SET file_id = ?1 WHERE item_id = ?2",
                    params![member_file_id, ids[2].0],
                )?;
                Ok(())
            })
            .unwrap();

        let error = app
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: ItemTarget::Explicit {
                    item_ids: vec![grouped.collection_id, ids[2]],
                },
                label: None,
                winning_collection_id: None,
            })
            .unwrap_err();
        assert!(error.contains("same physical file more than once"));
    }

    #[test]
    fn existing_source_repetition_does_not_block_adding_a_unique_item() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], Some("Source post"), None);
        app.store()
            .transaction(|transaction| {
                let first_file_id: i64 = transaction.query_row(
                    "SELECT file_id FROM media_asset WHERE item_id = ?1",
                    [ids[0].0],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "UPDATE media_asset SET file_id = ?1 WHERE item_id = ?2",
                    params![first_file_id, ids[1].0],
                )?;
                Ok(())
            })
            .unwrap();

        let result = app
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: ItemTarget::Explicit {
                    item_ids: vec![grouped.collection_id, ids[2]],
                },
                label: None,
                winning_collection_id: None,
            })
            .unwrap();
        assert_eq!(result.collection_id, grouped.collection_id);
    }

    #[test]
    fn collection_reorder_persists_the_complete_member_order() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids, Some("Post"), None);
        let reordered = vec![ids[2], ids[0], ids[1]];

        app.reorder_collection(ReorderCollectionInput {
            collection_id: grouped.collection_id,
            media_item_ids: reordered.clone(),
        })
        .unwrap();

        let details = crate::query_v2::details(app.store(), grouped.collection_id).unwrap();
        assert_eq!(
            details
                .media
                .into_iter()
                .map(|media| media.media_item_id)
                .collect::<Vec<_>>(),
            reordered
        );
        assert_eq!(details.cover_media_item_id, Some(reordered[0]));
        let cached_cover = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT cover_media_item_id FROM library_item WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get::<_, Option<i64>>(0),
                )
            })
            .unwrap();
        assert_eq!(cached_cover, Some(reordered[0].0));
    }

    #[test]
    fn organizing_multiple_collections_requires_winner_and_keeps_members_flat() {
        let (_directory, app, ids) = fixture();
        let extra = add_media_root(&app, "extra");
        let left = organize(&app, &ids[..2], Some("Left"), None);
        let right = organize(&app, &[ids[2], extra], Some("Right"), None);

        let missing_winner = app.organize_into_collection(OrganizeIntoCollectionInput {
            target: ItemTarget::Explicit {
                item_ids: vec![left.collection_id, right.collection_id],
            },
            label: None,
            winning_collection_id: None,
        });
        assert!(missing_winner
            .unwrap_err()
            .to_string()
            .contains("which group should own"));

        let merged = app
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: ItemTarget::Explicit {
                    item_ids: vec![left.collection_id, right.collection_id],
                },
                label: Some("Ignored label".to_string()),
                winning_collection_id: Some(right.collection_id),
            })
            .unwrap();
        assert_eq!(merged.collection_id, right.collection_id);

        app.store()
            .read(|connection| {
                let label: String = connection.query_row(
                    "SELECT label FROM library_item WHERE item_id = ?1",
                    [right.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(label, "Right");
                let members: Vec<i64> = connection
                    .prepare(
                        "SELECT cm.media_item_id FROM collection_member cm
                         WHERE cm.collection_id = ?1 ORDER BY cm.position_rank",
                    )?
                    .query_map([right.collection_id.0], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(members, vec![ids[2].0, extra.0, ids[0].0, ids[1].0]);
                let nested: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM collection_member cm
                     JOIN library_item li ON li.item_id = cm.media_item_id
                     WHERE cm.collection_id = ?1 AND li.kind = 'collection'",
                    [right.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(nested, 0);
                let losing_root: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE item_id = ?1",
                    [left.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(losing_root, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn organizing_rejects_invalid_winner_and_lifecycle_mismatch() {
        let (_directory, app, ids) = fixture();
        let missing_label = app.organize_into_collection(OrganizeIntoCollectionInput {
            target: ItemTarget::Explicit {
                item_ids: ids[..2].to_vec(),
            },
            label: Some("  ".to_string()),
            winning_collection_id: None,
        });
        assert!(missing_label
            .unwrap_err()
            .to_string()
            .contains("non-empty label"));

        let grouped = organize(&app, &ids[..2], Some("Post"), None);
        let invalid_winner = app.organize_into_collection(OrganizeIntoCollectionInput {
            target: ItemTarget::Explicit {
                item_ids: vec![grouped.collection_id, ids[2]],
            },
            label: None,
            winning_collection_id: Some(ItemId(9999)),
        });
        assert!(invalid_winner.unwrap_err().to_string().contains("selected"));

        app.set_lifecycle(
            &ItemTarget::Explicit {
                item_ids: vec![ids[2]],
            },
            Lifecycle::Inbox,
        )
        .unwrap();
        let mismatch = app.organize_into_collection(OrganizeIntoCollectionInput {
            target: ItemTarget::Explicit {
                item_ids: vec![grouped.collection_id, ids[2]],
            },
            label: None,
            winning_collection_id: None,
        });
        assert!(mismatch
            .unwrap_err()
            .to_string()
            .contains("share one lifecycle"));
    }

    #[test]
    fn worker_tags_only_the_analyzed_collection_member() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], None, None);
        let first = app
            .apply_media_tags(ids[0], &["general:predicted".to_string()], 2)
            .unwrap();
        let repeated = app
            .apply_media_tags(ids[0], &["general:predicted".to_string()], 2)
            .unwrap();
        assert_eq!(first.item_ids, vec![grouped.collection_id]);
        assert_eq!(first.revision, repeated.revision);
        app.store()
            .read(|connection| {
                let tagged: Vec<i64> = {
                    let mut statement = connection.prepare(
                        "SELECT mt.media_item_id FROM media_tag mt
                         JOIN tag t ON t.tag_id = mt.tag_id
                         WHERE t.namespace = 'general' AND t.subtag = 'predicted'
                         ORDER BY mt.media_item_id",
                    )?;
                    let rows = statement
                        .query_map([], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    rows
                };
                assert_eq!(tagged, vec![ids[0].0]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn ai_review_assignments_commit_once_and_report_affected_roots() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], None, None);
        let before = app.store().revision().unwrap();

        let receipt = app
            .apply_media_tag_assignments(
                &[
                    (ids[0], vec!["general:first".to_string()]),
                    (ids[1], vec!["general:second".to_string()]),
                ],
                2,
            )
            .unwrap();

        assert_eq!(receipt.revision, before + 1);
        assert_eq!(receipt.item_ids, vec![grouped.collection_id]);
        let assignments: Vec<(i64, String)> = app
            .store()
            .read(|connection| {
                connection
                    .prepare(
                        "SELECT mt.media_item_id, t.subtag
                         FROM media_tag mt JOIN tag t ON t.tag_id = mt.tag_id
                         WHERE mt.source = 'ai'
                         ORDER BY mt.media_item_id",
                    )?
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect()
            })
            .unwrap();
        assert_eq!(
            assignments,
            vec![
                (ids[0].0, "first".to_string()),
                (ids[1].0, "second".to_string())
            ]
        );

        app.undo().unwrap();
        let remaining = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM media_tag WHERE source = 'ai'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn collection_tag_write_updates_members_with_one_projection_batch() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], None, None);
        let target = ItemTarget::Explicit {
            item_ids: vec![grouped.collection_id],
        };
        app.apply_tags(&target, &["general:shared".to_string()], true, 1)
            .unwrap();
        let tag_id = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'general' AND subtag = 'shared'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(
            app.projections().direct_tag_bitmap(tag_id),
            roaring::RoaringBitmap::from_iter([grouped.collection_id.0 as u32])
        );
        app.store()
            .read(|connection| {
                let count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_tag WHERE tag_id = ?1",
                    [tag_id],
                    |row| row.get(0),
                )?;
                assert_eq!(count, 2);
                Ok(())
            })
            .unwrap();

        app.apply_tags(&target, &["general:shared".to_string()], false, 1)
            .unwrap();
        assert!(app.projections().direct_tag_bitmap(tag_id).is_empty());
    }

    #[test]
    fn deleting_collection_deletes_members_but_only_unreferenced_files() {
        let (_directory, app, ids) = fixture();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES ('preexisting-orphan', 'image/png', 1, 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let grouped = organize(&app, &ids[..2], None, None);
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET provider = 'dropbox' WHERE singleton = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let result = app
            .delete_items(&ItemTarget::Explicit {
                item_ids: vec![grouped.collection_id],
            })
            .unwrap();
        assert_eq!(result.freed_file_hashes.len(), 2);
        assert_eq!(
            result
                .receipt
                .item_ids
                .iter()
                .map(|id| id.0)
                .collect::<BTreeSet<_>>(),
            [grouped.collection_id.0, ids[0].0, ids[1].0]
                .into_iter()
                .collect()
        );
        let active = app.projections().active_bitmap();
        assert!(!active.contains(grouped.collection_id.0 as u32));
        assert!(!active.contains(ids[0].0 as u32));
        assert!(!active.contains(ids[1].0 as u32));

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
                assert_eq!(files, 2);
                assert_eq!(blob_deletes, 2);
                let unrelated_orphan: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_file WHERE file_hash = 'preexisting-orphan'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(unrelated_orphan, 1);
                let tombstones: (i64, i64) = connection.query_row(
                    "SELECT COUNT(*), COUNT(DISTINCT mutation_id)
                     FROM cloud_tombstone WHERE object_kind = 'item'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(tombstones, (3, 1));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn deleting_media_tombstones_its_source_but_retains_a_shared_file() {
        let (_directory, app, ids) = fixture();
        let ((shared_item_id, shared_file_id), _) = app
            .store()
            .transaction(|transaction| {
                let file_id: i64 = transaction.query_row(
                    "SELECT file_id FROM media_asset WHERE item_id = ?1",
                    [ids[0].0],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES ('shared-file-item', 'media', 'now', 'now')",
                    [],
                )?;
                let shared_item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, imported_at, updated_at)
                     VALUES (?1, ?2, 'now', 'now')",
                    params![shared_item_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (?1, 'active')",
                    [shared_item_id],
                )?;
                transaction.execute(
                    "INSERT INTO source_post
                         (site_id, post_key, created_at, updated_at)
                     VALUES ('test', 'delete-source', 'now', 'now')",
                    [],
                )?;
                let source_post_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO source_item
                         (source_post_id, item_key, position, media_item_id,
                          state, created_at, updated_at)
                     VALUES (?1, 'source-media', 0, ?2, 'ingested', 'now', 'now')",
                    params![source_post_id, ids[0].0],
                )?;
                Ok((shared_item_id, file_id))
            })
            .unwrap();

        let result = app
            .delete_items(&ItemTarget::Explicit {
                item_ids: vec![ids[0]],
            })
            .unwrap();

        assert!(result.freed_file_hashes.is_empty());
        app.store()
            .read(|connection| {
                let source: (String, Option<i64>) = connection.query_row(
                    "SELECT state, media_item_id FROM source_item
                     WHERE item_key = 'source-media'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(source, ("deleted".to_string(), None));
                let shared_item: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_asset
                     WHERE item_id = ?1 AND file_id = ?2",
                    params![shared_item_id, shared_file_id],
                    |row| row.get(0),
                )?;
                assert_eq!(shared_item, 1);
                let blob_deletes: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM work_item
                     WHERE work_type = 'blob_delete'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(blob_deletes, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn deleting_an_empty_collection_removes_the_collection_projection() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let (collection_id, _) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES ('empty-collection', 'collection', 'now', 'now')",
                    [],
                )?;
                let collection_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (?1, 'active')",
                    [collection_id],
                )?;
                Ok(collection_id)
            })
            .unwrap();
        let app = Application::new(store);
        assert!(app
            .projections()
            .active_bitmap()
            .contains(collection_id as u32));

        let result = app
            .delete_items(&ItemTarget::Explicit {
                item_ids: vec![ItemId(collection_id)],
            })
            .unwrap();

        assert!(result.freed_file_hashes.is_empty());
        assert!(!app
            .projections()
            .active_bitmap()
            .contains(collection_id as u32));
    }

    #[test]
    fn query_target_delete_respects_excluded_roots() {
        let (_directory, app, ids) = fixture();
        let result = app
            .delete_items(&ItemTarget::Query {
                query: ItemQuery {
                    scope: ItemScope::All,
                    filters: ItemFilters::default(),
                    sort: ItemSort::default(),
                },
                excluded_item_ids: vec![ids[0]],
            })
            .unwrap();

        assert_eq!(result.freed_file_hashes.len(), 2);
        app.store()
            .read(|connection| {
                let surviving_roots: Vec<i64> = connection
                    .prepare("SELECT item_id FROM library_root ORDER BY item_id")?
                    .query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(surviving_roots, vec![ids[0].0]);
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

    #[test]
    fn bulk_receipts_invalidate_resources_without_transmitting_every_item_id() {
        let ids = (1..=257).collect::<Vec<_>>();
        let receipt = super::receipt(4, &[crate::app::resources::LIBRARY], &ids);
        assert_eq!(receipt.revision, 4);
        assert_eq!(receipt.resources, vec!["library"]);
        assert!(receipt.item_ids.is_empty());
    }

    #[test]
    fn batch_rename_is_one_atomic_undoable_action() {
        let (_directory, app, ids) = fixture();
        app.rename_items(&[
            ItemRename {
                item_id: ids[0],
                name: "First renamed".to_string(),
            },
            ItemRename {
                item_id: ids[1],
                name: "Second renamed".to_string(),
            },
        ])
        .unwrap();

        let names = || {
            app.store()
                .read(|connection| {
                    ids[..2]
                        .iter()
                        .map(|item_id| {
                            connection.query_row(
                                "SELECT name FROM media_asset WHERE item_id = ?1",
                                [item_id.0],
                                |row| {
                                    row.get::<_, Option<String>>(0)
                                        .map(Option::unwrap_or_default)
                                },
                            )
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
        };
        assert_eq!(names(), vec!["First renamed", "Second renamed"]);
        app.undo().unwrap();
        assert_eq!(names(), vec!["", ""]);
        app.redo().unwrap();
        assert_eq!(names(), vec!["First renamed", "Second renamed"]);
    }
}
