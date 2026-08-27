//! Replacement mutations over library roots and media assets.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use rand::RngCore;
use roaring::RoaringBitmap;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, ItemTarget, Lifecycle, MutationReceipt};
use crate::projection_v2::{
    timestamp_ms, FolderProjectionChange, GroupOrderProjectionChange, ItemProjectionChange,
    LifecycleSummaryDelta, MembershipProjectionChange, RootProjectionChange,
    RootSummaryProjectionChange, StructureProjectionDelta,
};
use crate::store::history::{
    HistoryDescriptor, SemanticGroupDelta, SemanticGroupFolder, SemanticGroupMember,
    SemanticGroupRoot, SemanticGroupTag, SemanticHistoryPayload, SemanticHistoryRecord,
    SemanticLifecycleDelta, SemanticMembershipDelta, SemanticRatingDelta,
};

const RANK_GAP: i64 = 1024;
const MAX_RECEIPT_ITEM_IDS: usize = 256;

#[derive(Default)]
pub(crate) struct BulkTagProjectionDelta {
    pub(crate) changes: Vec<(i64, RoaringBitmap)>,
    pub(crate) history_changes: Vec<SemanticMembershipDelta>,
    pub(crate) canonical_changed: bool,
}

#[derive(Default)]
struct BulkFolderProjectionDelta {
    roots: RoaringBitmap,
    tags: BulkTagProjectionDelta,
}

struct BulkLifecycleProjectionDelta {
    roots: RoaringBitmap,
    lifecycle: Lifecycle,
}

#[derive(Default)]
struct GroupProjectionDelta {
    structure: StructureProjectionDelta,
    summaries: Vec<RootSummaryProjectionChange>,
    tag_changes: Vec<SemanticMembershipDelta>,
    shared_tag_sets: Vec<(RoaringBitmap, Vec<i64>)>,
    rating_changes: SemanticRatingDelta,
}

#[derive(Clone, Default)]
struct CapturedGroupState {
    item_ids: BTreeSet<i64>,
    roots: BTreeMap<i64, SemanticGroupRoot>,
    members: BTreeMap<(i64, i64), i64>,
    tag_sets: BTreeMap<i64, RoaringBitmap>,
}

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
    /// Rename one visible root without changing the media-owned source name.
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
                    "collection" | "media" => {
                        transaction.execute(
                            "INSERT INTO root_metadata (
                                 root_item_id, name, source_urls_json, updated_at
                             ) VALUES (?1, ?2, '[]', ?3)
                             ON CONFLICT(root_item_id) DO UPDATE SET
                                 name = excluded.name,
                                 updated_at = excluded.updated_at",
                            params![item_id.0, name, now],
                        )?;
                    }
                    other => return Err(invalid(format!("Unsupported item kind '{other}'"))),
                }
                crate::smart_v2::refresh_impacted_roots(
                    transaction,
                    &RoaringBitmap::from_iter([root_id_u32(item_id.0)?]),
                    &["name"],
                    &[],
                )?;
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
        let history_ids = capped_ids(item_ids.iter().copied());
        let resources_for_history = capped_item_resources(resources::LIBRARY, &history_ids);
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
                        .ok_or_else(|| {
                            invalid(format!("Item {} is not a library root", item_id.0))
                        })?;
                    match kind.as_str() {
                        "collection" | "media" => {
                            transaction.execute(
                                "INSERT INTO root_metadata (
                                     root_item_id, name, source_urls_json, updated_at
                                 ) VALUES (?1, ?2, '[]', ?3)
                                 ON CONFLICT(root_item_id) DO UPDATE SET
                                     name = excluded.name,
                                     updated_at = excluded.updated_at",
                                params![item_id.0, name, now],
                            )?;
                        }
                        other => return Err(invalid(format!("Unsupported item kind '{other}'"))),
                    };
                }
                crate::smart_v2::refresh_impacted_roots(
                    transaction,
                    &bitmap_from_i64s(normalized.iter().map(|(item_id, _)| item_id.0))?,
                    &["name"],
                    &[],
                )?;
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
        let item_ids = mutation_item_hints(target);
        let (_, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
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
            ),
            |projections| projections.selection_snapshot(),
            |transaction, projection| {
                let operation_started = Instant::now();
                let mut stage_started = operation_started;
                stage_root_selection_projected(transaction, target, &projection)?;
                transaction.execute_batch(
                    "CREATE TEMP TABLE IF NOT EXISTS picto_changed_root (
                         item_id INTEGER PRIMARY KEY
                     ) WITHOUT ROWID;
                     DELETE FROM picto_changed_root;",
                )?;
                let selected =
                    selected_id_bitmap(transaction, "SELECT item_id FROM picto_selected_root")?;
                let changed_ids = &selected - &projection.lifecycle_bitmap(lifecycle);
                let encoded = serde_json::to_string(&changed_ids.iter().collect::<Vec<_>>())
                    .map_err(|error| invalid(format!("Could not encode changed roots: {error}")))?;
                transaction.execute(
                    "INSERT INTO picto_changed_root(item_id)
                     SELECT CAST(value AS INTEGER) FROM json_each(?1)",
                    [encoded],
                )?;
                let undo = lifecycle_delta_for_projection(&projection, &changed_ids);
                trace_bulk_stage("items.lifecycle", "stage_selection", stage_started);
                stage_started = Instant::now();
                if !changed_ids.is_empty() {
                    let summary_delta = projection
                        .lifecycle_summary_delta(&changed_ids, lifecycle)
                        .map_err(invalid)?;
                    begin_bulk_lifecycle_settlement(transaction, &summary_delta)?;
                    trace_bulk_stage("items.lifecycle", "prepare_summaries", stage_started);
                    stage_started = Instant::now();
                    transaction.execute(
                        "UPDATE library_root
                         SET lifecycle = ?1
                         WHERE item_id IN (SELECT item_id FROM picto_changed_root)",
                        [lifecycle.as_str()],
                    )?;
                    trace_bulk_stage("items.lifecycle", "canonical_roots", stage_started);
                    stage_started = Instant::now();
                    finish_bulk_lifecycle_settlement(transaction, lifecycle)?;
                    trace_bulk_stage("items.lifecycle", "exact_summaries", stage_started);
                }
                trace_bulk_stage("items.lifecycle", "closure_total", operation_started);
                let changed = !changed_ids.is_empty();
                let redo = lifecycle_delta_for_target(&changed_ids, lifecycle);
                Ok((
                    (),
                    BulkLifecycleProjectionDelta {
                        roots: changed_ids,
                        lifecycle,
                    },
                    changed.then(|| {
                        SemanticHistoryRecord::new(
                            SemanticHistoryPayload::Lifecycle(undo),
                            SemanticHistoryPayload::Lifecycle(redo),
                        )
                    }),
                    changed,
                ))
            },
            |projections, delta| projections.apply_lifecycle_bitmap(&delta.roots, delta.lifecycle),
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
        let item_ids = mutation_item_hints(target);
        let (_, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
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
            ),
            |projections| projections.selection_snapshot(),
            |transaction, projection| {
                require_folder(transaction, folder_id)?;
                stage_root_selection_projected(transaction, target, &projection)?;
                let changed_roots =
                    apply_folder_to_selection(transaction, &projection, folder_id, present)?;
                let changed_tags = if present && !changed_roots.is_empty() {
                    let tags =
                        crate::folders_v2::inherited_folder_auto_tags(transaction, folder_id)?;
                    if tags.is_empty() {
                        BulkTagProjectionDelta::default()
                    } else {
                        apply_tags_to_selection(transaction, self.projections(), &tags, true)?
                    }
                } else {
                    BulkTagProjectionDelta::default()
                };
                let changed = !changed_roots.is_empty() || changed_tags.canonical_changed;
                let undo = SemanticHistoryPayload::Composite(vec![
                    semantic_membership(folder_id, &changed_roots, !present, false),
                    semantic_tag_memberships(&changed_tags, false),
                ]);
                let redo = SemanticHistoryPayload::Composite(vec![
                    semantic_membership(folder_id, &changed_roots, present, false),
                    semantic_tag_memberships(&changed_tags, true),
                ]);
                Ok((
                    (),
                    BulkFolderProjectionDelta {
                        roots: changed_roots,
                        tags: changed_tags,
                    },
                    changed.then(|| SemanticHistoryRecord::new(undo, redo)),
                    changed,
                ))
            },
            |projections, delta| {
                projections.apply_folder_bitmap(folder_id, &delta.roots, present)?;
                for (tag_id, root_ids) in delta.tags.changes {
                    projections.apply_root_tag_bitmap(tag_id, &root_ids, true)?;
                }
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
        let ((collection_id, affected), revision, _) = self.semantic_undoable_transaction(
            HistoryDescriptor::new(
                "collections.organize",
                "Create or merge group",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                Vec::new(),
            ),
            |transaction| {
                let operation_started = Instant::now();
                let mut stage_started = operation_started;
                let item_ids = crate::query_v2::resolve_target_ids(transaction, &input.target)?;
                let projection = self.projections().selection_snapshot();
                stage_root_ids(transaction, &item_ids, &projection)?;
                let before_universe = group_history_universe(&projection, &item_ids)?;
                let before = capture_group_state_from_projection(
                    transaction,
                    &before_universe,
                    &projection,
                    false,
                )?;
                let selected_roots = bitmap_from_i64s(item_ids.iter().copied())?;
                let inherited_folder_ids = folder_ids_for_roots(&projection, &item_ids);
                let inherited_tag_ids = before
                    .tag_sets
                    .iter()
                    .filter_map(|(tag_id, roots)| {
                        (roots.intersection_len(&selected_roots) > 0).then_some(*tag_id)
                    })
                    .collect::<Vec<_>>();
                trace_bulk_stage("groups.organize", "capture_before", stage_started);
                stage_started = Instant::now();
                let (selected_count, media_count, collection_count): (i64, i64, i64) = transaction
                    .query_row(
                        "SELECT COUNT(*),
                                COALESCE(SUM(item.kind = 'media'), 0),
                                COALESCE(SUM(item.kind = 'collection'), 0)
                         FROM picto_selected_root selected
                         JOIN library_root root ON root.item_id = selected.item_id
                         JOIN library_item item ON item.item_id = selected.item_id",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                if selected_count != item_ids.len() as i64
                    || media_count + collection_count != selected_count
                {
                    return Err(invalid("A targeted item is not a supported library root"));
                }
                let standalone_media_ids = selected_root_ids_of_kind(transaction, "media")?;
                let collection_ids = selected_root_ids_of_kind(transaction, "collection")?;
                require_no_selected_file_overlap(transaction, &projection, &item_ids)?;
                trace_bulk_stage("groups.organize", "validate_selection", stage_started);
                stage_started = Instant::now();
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
                let creating_collection = collection_ids.is_empty();
                if creating_collection {
                    begin_group_create_summary_batch(transaction)?;
                } else {
                    begin_structural_summary_batch(transaction, &before_universe)?;
                }
                let mut delta = StructureProjectionDelta::default();
                let collection_id = if creating_collection {
                    let label = label
                        .as_deref()
                        .ok_or_else(|| invalid("A new group requires a non-empty label"))?;
                    transaction.execute(
                        "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                         VALUES (?1, 'collection', ?2, ?2)",
                        params![key, now],
                    )?;
                    let collection_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
                        params![collection_id, lifecycle],
                    )?;
                    let cover_root_id = item_ids[0];
                    transaction.execute(
                        "INSERT INTO root_metadata (
                             root_item_id, name, rating, notes, source_urls_json, updated_at
                         )
                         SELECT ?1, ?2, metadata.rating, metadata.notes,
                                COALESCE(metadata.source_urls_json, '[]'), ?3
                         FROM library_item cover
                         LEFT JOIN root_metadata metadata
                           ON metadata.root_item_id = cover.item_id
                         WHERE cover.item_id = ?4",
                        params![collection_id, label, now, cover_root_id],
                    )?;
                    delta.items.push(ItemProjectionChange {
                        item_id: collection_id,
                        kind: crate::app::ItemKind::Collection,
                        present: true,
                    });
                    delta.roots.push(RootProjectionChange {
                        item_id: collection_id,
                        lifecycle: Some(projected_lifecycle),
                    });
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
                trace_bulk_stage("groups.organize", "create_winner", stage_started);
                stage_started = Instant::now();

                let encoded_roots = serde_json::to_string(&item_ids)
                    .map_err(|error| invalid(format!("Could not encode group roots: {error}")))?;
                trace_bulk_stage("groups.organize.structure", "union_tags", stage_started);
                stage_started = Instant::now();
                delta
                    .folders
                    .extend(inherited_folder_ids.iter().copied().map(|folder_id| {
                        FolderProjectionChange {
                            folder_id,
                            item_id: collection_id,
                            present: true,
                        }
                    }));
                trace_bulk_stage("groups.organize.structure", "union_folders", stage_started);
                stage_started = Instant::now();
                transaction.execute(
                    "UPDATE root_metadata
                     SET source_urls_json = COALESCE((
                           SELECT json_group_array(url)
                           FROM (
                               SELECT DISTINCT CAST(url.value AS TEXT) AS url
                               FROM root_metadata source
                               JOIN json_each(?1) selected
                                 ON source.root_item_id = CAST(selected.value AS INTEGER)
                               JOIN json_each(source.source_urls_json) url
                               WHERE CAST(url.value AS TEXT) <> ''
                               ORDER BY url
                           )
                         ), '[]'),
                         updated_at = ?2
                     WHERE root_item_id = ?3",
                    params![encoded_roots, now, collection_id],
                )?;
                trace_bulk_stage("groups.organize.structure", "union_sources", stage_started);
                stage_started = Instant::now();
                let mut affected = vec![collection_id];
                let final_order;
                if creating_collection {
                    let folder_rows = item_ids
                        .iter()
                        .flat_map(|item_id| {
                            projection
                                .folder_ids_for_root(*item_id)
                                .into_iter()
                                .map(move |folder_id| (*item_id, folder_id))
                        })
                        .collect::<Vec<_>>();
                    remove_staged_roots_for_collection(transaction)?;
                    trace_bulk_stage(
                        "groups.organize.structure",
                        "remove_member_roots",
                        stage_started,
                    );
                    transaction.execute(
                        "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                        params![item_ids.first(), collection_id],
                    )?;
                    stage_started = Instant::now();
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
                        affected.push(*item_id);
                    }
                    delta
                        .folders
                        .extend(folder_rows.into_iter().map(|(item_id, folder_id)| {
                            FolderProjectionChange {
                                folder_id,
                                item_id,
                                present: false,
                            }
                        }));
                    delta.group_orders.push(GroupOrderProjectionChange {
                        collection_id,
                        media_ids: item_ids.clone(),
                    });
                    final_order = item_ids.clone();
                } else {
                    let members_by_collection = collection_ids
                        .iter()
                        .map(|group_id| {
                            projection
                                .group_order(*group_id)
                                .map(|members| (*group_id, members))
                                .ok_or_else(|| {
                                    invalid(format!(
                                        "Group {group_id} has no canonical member order"
                                    ))
                                })
                        })
                        .collect::<Result<BTreeMap<_, _>, _>>()?;
                    let mut folders_by_root = item_ids
                        .iter()
                        .filter(|item_id| **item_id != collection_id)
                        .map(|item_id| (*item_id, projection.folder_ids_for_root(*item_id)))
                        .collect::<BTreeMap<_, _>>();
                    let mut merged_order = members_by_collection
                        .get(&collection_id)
                        .cloned()
                        .unwrap_or_default();
                    for item_id in &item_ids {
                        if *item_id == collection_id {
                            continue;
                        }
                        if standalone_media_ids.binary_search(item_id).is_ok() {
                            merged_order.push(*item_id);
                        } else if let Some(members) = members_by_collection.get(item_id) {
                            merged_order.extend(members.iter().copied());
                        }
                    }
                    transaction.execute(
                        "DELETE FROM library_root
                         WHERE item_id IN (
                             SELECT selected.item_id
                             FROM picto_selected_root selected
                             JOIN library_item item ON item.item_id = selected.item_id
                             WHERE item.kind = 'media' AND selected.item_id <> ?1
                         )",
                        [collection_id],
                    )?;
                    transaction.execute(
                        "DELETE FROM library_item
                         WHERE item_id IN (
                             SELECT selected.item_id
                             FROM picto_selected_root selected
                             JOIN library_item item ON item.item_id = selected.item_id
                             WHERE item.kind = 'collection' AND selected.item_id <> ?1
                         )",
                        [collection_id],
                    )?;
                    for item_id in &item_ids {
                        affected.push(*item_id);
                        if *item_id == collection_id {
                            continue;
                        }
                        let folders = folders_by_root.remove(item_id).unwrap_or_default();
                        if standalone_media_ids.binary_search(item_id).is_ok() {
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
                            continue;
                        }

                        let members = members_by_collection
                            .get(item_id)
                            .cloned()
                            .unwrap_or_default();
                        if *item_id != collection_id {
                            for media_id in members {
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
                            }
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
                    transaction.execute(
                        "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                        params![merged_order.first(), collection_id],
                    )?;
                    delta.group_orders.push(GroupOrderProjectionChange {
                        collection_id,
                        media_ids: merged_order.clone(),
                    });
                    final_order = merged_order;
                }

                trace_bulk_stage("groups.organize", "canonical_structure", stage_started);
                stage_started = Instant::now();
                affected.sort_unstable();
                affected.dedup();
                let mut after_universe = before_universe;
                after_universe.extend(affected.iter().copied());
                after_universe.push(collection_id);
                after_universe.sort_unstable();
                after_universe.dedup();
                if creating_collection {
                    finish_group_create_summary_batch(transaction, collection_id, &item_ids)?;
                } else {
                    upsert_group_root_summary(transaction, collection_id, &final_order)?;
                    finish_structural_summary_batch(transaction, &after_universe)?;
                }
                trace_bulk_stage("groups.organize", "summary_settlement", stage_started);
                stage_started = Instant::now();
                let mut after = capture_group_state_after_projection(
                    transaction,
                    &after_universe,
                    &projection,
                    &delta,
                    &delta.folders,
                )?;
                let collection_root = RoaringBitmap::from_iter([root_id_u32(collection_id)?]);
                for tag_id in &inherited_tag_ids {
                    after.tag_sets.insert(*tag_id, collection_root.clone());
                }
                let (history, forward) = group_history_record(&before, &after)?;
                let summaries = root_summary_changes_for_roots(transaction, &after_universe)?;
                trace_bulk_stage("groups.organize", "capture_after_history", stage_started);
                trace_bulk_stage("groups.organize", "closure_total", operation_started);
                Ok((
                    (collection_id, affected),
                    GroupProjectionDelta {
                        structure: delta,
                        summaries,
                        tag_changes: Vec::new(),
                        shared_tag_sets: vec![(collection_root, inherited_tag_ids)],
                        rating_changes: forward.rating_changes,
                    },
                    history,
                ))
            },
            apply_group_projection_delta,
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
        let (affected, revision, _) = self.semantic_undoable_transaction(
            HistoryDescriptor::new(
                "collections.detach",
                "Remove from group",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                Vec::new(),
            ),
            |transaction| {
                let mut seeds = media_ids.clone();
                seeds.push(input.collection_id.0);
                let projection = self.projections().selection_snapshot();
                let before_universe = group_history_universe(&projection, &seeds)?;
                let before = capture_group_state_from_projection(
                    transaction,
                    &before_universe,
                    &projection,
                    true,
                )?;
                let lifecycle = require_collection_root(transaction, input.collection_id.0)?;
                let projected_lifecycle = parse_lifecycle(&lifecycle)?;
                let detached_lifecycle = input.target_lifecycle.unwrap_or(projected_lifecycle);
                let detached_lifecycle_name = detached_lifecycle.as_str();
                let folders = folder_ids_for_roots(
                    &self.projections().selection_snapshot(),
                    &[input.collection_id.0],
                );
                let current_order = projection
                    .group_order(input.collection_id.0)
                    .ok_or_else(|| invalid("Group has no canonical member order"))?;
                let selected = media_ids.iter().copied().collect::<BTreeSet<_>>();
                if let Some(media_id) = media_ids
                    .iter()
                    .find(|media_id| !current_order.contains(media_id))
                {
                    return Err(invalid(format!(
                        "Media item {media_id} is not attached to group {}",
                        input.collection_id.0
                    )));
                }
                let remaining = current_order
                    .into_iter()
                    .filter(|media_id| !selected.contains(media_id))
                    .collect::<Vec<_>>();
                let removes_group = remaining.len() <= 1;
                begin_structural_summary_batch(transaction, &before_universe)?;
                stage_media_ids(transaction, &media_ids)?;
                let mut delta = StructureProjectionDelta::default();
                create_staged_roots_with_metadata(
                    transaction,
                    detached_lifecycle_name,
                    input.collection_id.0,
                )?;
                for media_id in &media_ids {
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
                if !removes_group {
                    transaction.execute(
                        "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                        params![remaining.first(), input.collection_id.0],
                    )?;
                    delta.group_orders.push(GroupOrderProjectionChange {
                        collection_id: input.collection_id.0,
                        media_ids: remaining.clone(),
                    });
                    upsert_group_root_summary(transaction, input.collection_id.0, &remaining)?;
                } else {
                    if let Some(media_id) = remaining.first().copied() {
                        stage_media_ids(transaction, &[media_id])?;
                        create_staged_roots_with_metadata(
                            transaction,
                            &lifecycle,
                            input.collection_id.0,
                        )?;
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
                affected.sort_unstable();
                affected.dedup();
                let mut after_universe = before_universe;
                after_universe.extend(affected.iter().copied());
                after_universe.sort_unstable();
                after_universe.dedup();
                finish_structural_summary_batch(transaction, &after_universe)?;
                let after = capture_group_state_after_projection(
                    transaction,
                    &after_universe,
                    &projection,
                    &delta,
                    &delta.folders,
                )?;
                let detached_roots = bitmap_from_i64s(
                    affected
                        .iter()
                        .copied()
                        .filter(|item_id| *item_id != input.collection_id.0),
                )?;
                let (folder_changes, tag_changes) = inherited_group_membership_changes(
                    before.roots.get(&input.collection_id.0).ok_or_else(|| {
                        invalid(format!(
                            "Group {} has no root metadata",
                            input.collection_id.0
                        ))
                    })?,
                    &detached_roots,
                    removes_group,
                )?;
                let (history, forward) = group_history_record_with_memberships(
                    &before,
                    &after,
                    folder_changes,
                    tag_changes,
                )?;
                let summaries = root_summary_changes_for_roots(transaction, &after_universe)?;
                Ok((
                    affected,
                    GroupProjectionDelta {
                        structure: delta,
                        summaries,
                        tag_changes: Vec::new(),
                        shared_tag_sets: vec![(
                            detached_roots,
                            before
                                .roots
                                .get(&input.collection_id.0)
                                .map(|root| root.tags.iter().map(|tag| tag.tag_id).collect())
                                .unwrap_or_default(),
                        )],
                        rating_changes: forward.rating_changes,
                    },
                    history,
                ))
            },
            apply_group_projection_delta,
        )?;
        Ok(receipt(
            revision,
            &[resources::LIBRARY, resources::SIDEBAR, resources::FOLDERS],
            &affected,
        ))
    }

    pub fn ungroup_collection(&self, collection_id: ItemId) -> Result<MutationReceipt, String> {
        let (affected, revision, _) = self.semantic_undoable_transaction(
            HistoryDescriptor::new(
                "collections.ungroup",
                "Ungroup",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::FOLDERS.to_string(),
                ],
                vec![collection_id.0],
            ),
            |transaction| {
                let operation_started = Instant::now();
                let mut stage_started = operation_started;
                let projection = self.projections().selection_snapshot();
                let before_universe = group_history_universe(&projection, &[collection_id.0])?;
                let before = capture_group_state_from_projection(
                    transaction,
                    &before_universe,
                    &projection,
                    true,
                )?;
                trace_bulk_stage("groups.ungroup", "capture_before", stage_started);
                stage_started = Instant::now();
                let lifecycle = require_collection_root(transaction, collection_id.0)?;
                let projected_lifecycle = parse_lifecycle(&lifecycle)?;
                let folders = folder_ids_for_roots(
                    &self.projections().selection_snapshot(),
                    &[collection_id.0],
                );
                begin_structural_summary_batch(transaction, &before_universe)?;
                let members = projection
                    .group_order(collection_id.0)
                    .ok_or_else(|| invalid("Group has no canonical member order"))?;
                stage_media_ids(transaction, &members)?;
                let mut delta = StructureProjectionDelta::default();
                create_staged_roots_with_metadata(transaction, &lifecycle, collection_id.0)?;
                trace_bulk_stage("groups.ungroup", "create_roots", stage_started);
                stage_started = Instant::now();
                for member in &members {
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
                trace_bulk_stage("groups.ungroup", "delete_collection", stage_started);
                project_removed_collection(&mut delta, collection_id.0, &folders);
                let mut affected = members;
                affected.push(collection_id.0);
                affected.sort_unstable();
                affected.dedup();
                let mut after_universe = before_universe;
                after_universe.extend(affected.iter().copied());
                after_universe.sort_unstable();
                after_universe.dedup();
                let summary_started = Instant::now();
                finish_structural_summary_batch(transaction, &after_universe)?;
                trace_bulk_stage("groups.ungroup", "summary_settlement", summary_started);
                stage_started = Instant::now();
                let after = capture_group_state_after_projection(
                    transaction,
                    &after_universe,
                    &projection,
                    &delta,
                    &delta.folders,
                )?;
                let detached_roots = bitmap_from_i64s(
                    affected
                        .iter()
                        .copied()
                        .filter(|item_id| *item_id != collection_id.0),
                )?;
                let (folder_changes, tag_changes) = inherited_group_membership_changes(
                    before.roots.get(&collection_id.0).ok_or_else(|| {
                        invalid(format!("Group {} has no root metadata", collection_id.0))
                    })?,
                    &detached_roots,
                    true,
                )?;
                let (history, forward) = group_history_record_with_memberships(
                    &before,
                    &after,
                    folder_changes,
                    tag_changes,
                )?;
                let summaries = root_summary_changes_for_roots(transaction, &after_universe)?;
                trace_bulk_stage("groups.ungroup", "capture_after_history", stage_started);
                trace_bulk_stage("groups.ungroup", "closure_total", operation_started);
                Ok((
                    affected,
                    GroupProjectionDelta {
                        structure: delta,
                        summaries,
                        tag_changes: Vec::new(),
                        shared_tag_sets: vec![(
                            detached_roots,
                            before
                                .roots
                                .get(&collection_id.0)
                                .map(|root| root.tags.iter().map(|tag| tag.tag_id).collect())
                                .unwrap_or_default(),
                        )],
                        rating_changes: forward.rating_changes,
                    },
                    history,
                ))
            },
            apply_group_projection_delta,
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
        let (_, revision, _) = self.semantic_undoable_transaction(
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
                let projection = self.projections().selection_snapshot();
                let universe = group_history_universe(&projection, &[input.collection_id.0])?;
                let before =
                    capture_group_state_from_projection(transaction, &universe, &projection, true)?;
                let current_order = projection
                    .group_order(input.collection_id.0)
                    .ok_or_else(|| invalid("Group has no canonical member order"))?;
                if current_order.len() != media_ids.len()
                    || current_order.iter().copied().collect::<BTreeSet<_>>()
                        != media_ids.iter().copied().collect::<BTreeSet<_>>()
                {
                    return Err(invalid(
                        "Reorder must contain every group member exactly once",
                    ));
                }
                transaction.execute(
                    "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                    params![media_ids.first(), input.collection_id.0],
                )?;
                transaction.execute(
                    "UPDATE root_summary SET cover_media_item_id = ?1
                     WHERE root_item_id = ?2",
                    params![media_ids.first(), input.collection_id.0],
                )?;
                let structure = StructureProjectionDelta {
                    group_orders: vec![GroupOrderProjectionChange {
                        collection_id: input.collection_id.0,
                        media_ids: media_ids.clone(),
                    }],
                    ..StructureProjectionDelta::default()
                };
                let after = capture_group_state_after_projection(
                    transaction,
                    &universe,
                    &projection,
                    &structure,
                    &[],
                )?;
                let (history, forward) = group_history_record(&before, &after)?;
                Ok((
                    (),
                    GroupProjectionDelta {
                        structure,
                        rating_changes: forward.rating_changes,
                        ..GroupProjectionDelta::default()
                    },
                    history,
                ))
            },
            apply_group_projection_delta,
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
    ) -> Result<MutationReceipt, String> {
        let tags = tags
            .iter()
            .filter_map(|tag| crate::tag_name_v2::parse_local(tag).ok())
            .collect::<Vec<_>>();
        if tags.is_empty() {
            return Err("No valid tags were provided".to_string());
        }
        let item_ids = mutation_item_hints(target);
        let (_, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
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
            ),
            |projections| projections.selection_snapshot(),
            |transaction, projection| {
                stage_root_selection_projected(transaction, target, &projection)?;
                let delta = apply_tags_to_selection(transaction, self.projections(), &tags, add)?;
                let changed = delta.canonical_changed;
                let undo = semantic_tag_memberships(&delta, !add);
                let redo = semantic_tag_memberships(&delta, add);
                Ok((
                    (),
                    delta,
                    changed.then(|| SemanticHistoryRecord::new(undo, redo)),
                    changed,
                ))
            },
            |projections, delta| {
                for (tag_id, root_ids) in delta.changes {
                    projections.apply_root_tag_bitmap(tag_id, &root_ids, add)?;
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

    /// Worker tags resolve to the owning root so attached group members never
    /// acquire independent organization state.
    pub(crate) fn apply_media_tags(
        &self,
        media_item_id: ItemId,
        tags: &[String],
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
                let root_id = self
                    .projections()
                    .root_for_media(media_item_id.0)
                    .ok_or_else(|| {
                        invalid(format!(
                            "Media item {} has no library root",
                            media_item_id.0
                        ))
                    })?;
                let roots = RoaringBitmap::from_iter([root_id_u32(root_id)?]);
                let delta =
                    apply_tags_to_roots(transaction, self.projections(), &roots, &tags, true)?;
                let changed = delta.canonical_changed;
                Ok((root_id, delta, changed))
            },
            |projections, delta| {
                for (tag_id, roots) in delta.changes {
                    projections.apply_root_tag_bitmap(tag_id, &roots, true)?;
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
            &[root_id],
        ))
    }

    pub(crate) fn apply_media_tag_assignments(
        &self,
        assignments: &[(ItemId, Vec<String>)],
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
        let (root_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed(
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
            ),
            |transaction| {
                let mut root_ids = BTreeSet::new();
                let mut roots_by_tag = BTreeMap::<(String, String), RoaringBitmap>::new();
                for (media_item_id, tags) in &normalized {
                    let root_id = self
                        .projections()
                        .root_for_media(media_item_id.0)
                        .ok_or_else(|| {
                            invalid(format!(
                                "Media item {} has no library root",
                                media_item_id.0
                            ))
                        })?;
                    root_ids.insert(root_id);
                    let root_id = root_id_u32(root_id)?;
                    for tag in tags {
                        roots_by_tag.entry(tag.clone()).or_default().insert(root_id);
                    }
                }
                let delta =
                    apply_tag_assignments(transaction, self.projections(), &roots_by_tag, true)?;
                let changed = delta.canonical_changed;
                let history = changed.then(|| {
                    SemanticHistoryRecord::new(
                        semantic_tag_memberships(&delta, false),
                        semantic_tag_memberships(&delta, true),
                    )
                });
                Ok((
                    root_ids.into_iter().collect::<Vec<_>>(),
                    delta,
                    history,
                    changed,
                ))
            },
            |projections, delta| {
                for (tag_id, root_ids) in delta.changes {
                    projections.apply_root_tag_bitmap(tag_id, &root_ids, true)?;
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
            &root_ids,
        ))
    }

    pub fn patch_metadata(
        &self,
        target: &ItemTarget,
        patch: &MediaMetadataPatch,
    ) -> Result<MutationReceipt, String> {
        let projection_rating = patch
            .rating
            .map(|rating| {
                rating
                    .map(u8::try_from)
                    .transpose()
                    .map_err(|_| "Rating must be between 0 and 5".to_string())
                    .and_then(|rating| {
                        if rating.is_some_and(|rating| rating > 5) {
                            Err("Rating must be between 0 and 5".to_string())
                        } else {
                            Ok(rating)
                        }
                    })
            })
            .transpose()?;
        if let Some(rating) = patch.rating {
            if patch.notes.is_none() && patch.source_urls.is_none() {
                return self.patch_rating(target, rating);
            }
        }
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
                stage_root_selection(transaction, target)?;
                let item_ids = limited_staged_hints(
                    transaction,
                    "picto_selected_root",
                    "item_id",
                )?;
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "INSERT INTO root_metadata (
                         root_item_id, name, rating, notes, source_urls_json, updated_at
                     )
                     SELECT selected.item_id, metadata.name, ?1, ?2, COALESCE(?3, '[]'), ?4
                     FROM picto_selected_root selected
                     LEFT JOIN root_metadata metadata
                       ON metadata.root_item_id = selected.item_id
                     WHERE TRUE
                     ON CONFLICT(root_item_id) DO UPDATE SET
                         rating = CASE WHEN ?5 THEN excluded.rating ELSE root_metadata.rating END,
                         notes = CASE WHEN ?6 THEN excluded.notes ELSE root_metadata.notes END,
                         source_urls_json = CASE WHEN ?7 THEN excluded.source_urls_json ELSE root_metadata.source_urls_json END,
                         updated_at = excluded.updated_at",
                    params![
                        patch.rating.flatten(),
                        patch.notes.as_ref().and_then(|value| value.clone()),
                        source_urls_json,
                        now,
                        patch.rating.is_some(),
                        patch.notes.is_some(),
                        patch.source_urls.is_some(),
                    ],
                )?;
                let changed_fields = [
                    patch.rating.is_some().then_some("rating"),
                    patch.notes.is_some().then_some("notes"),
                    patch.source_urls.is_some().then_some("source_urls"),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                let roots = selected_id_bitmap(
                    transaction,
                    "SELECT item_id FROM picto_selected_root",
                )?;
                crate::smart_v2::refresh_impacted_roots(
                    transaction,
                    &roots,
                    &changed_fields,
                    &[],
                )?;
                Ok((item_ids, roots))
            },
            |projections, changed_roots| {
                if let Some(rating) = projection_rating {
                    projections.apply_rating_bitmap(&changed_roots, rating)?;
                }
                Ok(())
            },
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

    fn patch_rating(
        &self,
        target: &ItemTarget,
        rating: Option<i64>,
    ) -> Result<MutationReceipt, String> {
        if rating.is_some_and(|rating| !(0..=5).contains(&rating)) {
            return Err("Rating must be between 0 and 5".to_string());
        }
        let item_ids = mutation_item_hints(target);
        let (_, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
            HistoryDescriptor::new(
                "items.patch_rating",
                "Change rating",
                vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::SMART_FOLDERS.to_string(),
                ],
                vec![],
            ),
            |projections| projections.selection_snapshot(),
            |transaction, projection| {
                let operation_started = Instant::now();
                let mut stage_started = Instant::now();
                stage_root_selection_projected(transaction, target, &projection)?;
                trace_bulk_stage("items.patch_rating", "stage_selection", stage_started);
                stage_started = Instant::now();
                let selected =
                    selected_id_bitmap(transaction, "SELECT item_id FROM picto_selected_root")?;
                let changed_roots = &selected - &projection.rating_value_bitmap(rating);
                transaction.execute("DELETE FROM picto_selected_root", [])?;
                let encoded = serde_json::to_string(&changed_roots.iter().collect::<Vec<_>>())
                    .map_err(|error| invalid(format!("Could not encode changed roots: {error}")))?;
                transaction.execute(
                    "INSERT INTO picto_selected_root(item_id)
                     SELECT CAST(value AS INTEGER) FROM json_each(?1)",
                    [encoded],
                )?;
                trace_bulk_stage("items.patch_rating", "stage_changes", stage_started);
                stage_started = Instant::now();
                let undo = rating_delta_for_projection(&projection, &changed_roots);
                trace_bulk_stage("items.patch_rating", "capture_history", stage_started);
                if !changed_roots.is_empty() {
                    stage_started = Instant::now();
                    let now = chrono::Utc::now().to_rfc3339();
                    transaction.execute(
                        "UPDATE projection_write_control
                         SET suppress_root_summary = 1
                         WHERE singleton = 1",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO root_metadata (
                             root_item_id, rating, source_urls_json, updated_at
                         )
                         SELECT item_id, ?1, '[]', ?2
                         FROM picto_selected_root
                         WHERE TRUE
                         ON CONFLICT(root_item_id) DO UPDATE SET
                             rating = excluded.rating,
                             updated_at = excluded.updated_at",
                        params![rating, now],
                    )?;
                    transaction.execute(
                        "UPDATE root_summary
                         SET sort_rating = ?1
                         WHERE root_item_id IN (
                             SELECT item_id FROM picto_selected_root
                        )",
                        [rating],
                    )?;
                    transaction.execute(
                        "UPDATE projection_write_control
                         SET suppress_root_summary = 0
                         WHERE singleton = 1",
                        [],
                    )?;
                    trace_bulk_stage("items.patch_rating", "canonical_update", stage_started);
                }
                let changed = !changed_roots.is_empty();
                let redo = rating_delta_for_target(&changed_roots, rating);
                trace_bulk_stage("items.patch_rating", "closure_total", operation_started);
                Ok((
                    (),
                    changed_roots,
                    changed.then(|| {
                        SemanticHistoryRecord::new(
                            SemanticHistoryPayload::Ratings(undo),
                            SemanticHistoryPayload::Ratings(redo),
                        )
                    }),
                    changed,
                ))
            },
            |projections, changed_roots| {
                let rating = rating
                    .map(u8::try_from)
                    .transpose()
                    .map_err(|_| "Rating is outside the projection range".to_string())?;
                projections.apply_rating_bitmap(&changed_roots, rating)
            },
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
                let operation_started = Instant::now();
                let mut stage_started = operation_started;
                let projection = self.projections().selection_snapshot();
                stage_mutation_selection(transaction, target, &projection)?;
                let selected_root_ids = transaction
                    .prepare("SELECT item_id FROM picto_selected_root ORDER BY item_id")?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut delete_ids = selected_root_ids.clone();
                for root_id in &selected_root_ids {
                    if let Some(members) = projection.group_order(*root_id) {
                        delete_ids.extend(members);
                    }
                }
                delete_ids.sort_unstable();
                delete_ids.dedup();
                let encoded_delete_ids = serde_json::to_string(&delete_ids)
                    .map_err(|error| invalid(format!("Could not encode delete target: {error}")))?;
                transaction.execute_batch(
                    "CREATE TEMP TABLE IF NOT EXISTS picto_delete_item (
                         item_id INTEGER PRIMARY KEY
                     ) WITHOUT ROWID;
                     CREATE TEMP TABLE IF NOT EXISTS picto_candidate_file (
                         file_id INTEGER PRIMARY KEY
                     ) WITHOUT ROWID;
                     DELETE FROM picto_delete_item;
                     DELETE FROM picto_candidate_file;",
                )?;
                transaction.execute(
                    "INSERT INTO picto_delete_item(item_id)
                     SELECT CAST(value AS INTEGER) FROM json_each(?1)",
                    [encoded_delete_ids],
                )?;
                transaction.execute_batch(
                    "INSERT INTO picto_candidate_file(file_id)
                     SELECT DISTINCT asset.file_id
                     FROM media_asset asset
                     JOIN picto_delete_item deleted ON deleted.item_id = asset.item_id;",
                )?;
                let item_ids = limited_staged_hints(transaction, "picto_delete_item", "item_id")?;
                let selected_roots =
                    selected_id_bitmap(transaction, "SELECT item_id FROM picto_selected_root")?;
                begin_structural_summary_batch_from_staged_roots(transaction)?;
                trace_bulk_stage("items.delete", "stage_selection", stage_started);
                stage_started = Instant::now();
                let delete_count: i64 =
                    transaction.query_row("SELECT COUNT(*) FROM picto_delete_item", [], |row| {
                        row.get(0)
                    })?;
                let delete_count = usize::try_from(delete_count)
                    .map_err(|_| invalid("Delete selection exceeds addressable memory"))?;
                let mut delta = StructureProjectionDelta {
                    items: Vec::with_capacity(delete_count),
                    roots: Vec::with_capacity(selected_roots.len() as usize),
                    ..StructureProjectionDelta::default()
                };
                for collection_id in &selected_root_ids {
                    if let Some(members) = projection.group_order(*collection_id) {
                        for media_id in members {
                            delta.memberships.push(MembershipProjectionChange {
                                collection_id: *collection_id,
                                media_id,
                                present: false,
                            });
                        }
                    }
                    for folder_id in projection.folder_ids_for_root(*collection_id) {
                        delta.folders.push(FolderProjectionChange {
                            folder_id,
                            item_id: *collection_id,
                            present: false,
                        });
                    }
                }
                trace_bulk_stage("items.delete", "prepare_projection", stage_started);
                stage_started = Instant::now();
                let cloud_configured: bool = transaction.query_row(
                    "SELECT provider IS NOT NULL FROM cloud_state WHERE singleton = 1",
                    [],
                    |row| row.get(0),
                )?;
                let deleted_source_item_ids = if cloud_configured {
                    let mut statement = transaction.prepare(
                        "SELECT source_item_id FROM source_item
                         WHERE media_item_id IN (SELECT item_id FROM picto_delete_item)",
                    )?;
                    let source_item_ids = statement
                        .query_map([], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    source_item_ids
                } else {
                    Vec::new()
                };
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "UPDATE source_item
                     SET state = 'deleted', media_item_id = NULL, updated_at = ?1
                     WHERE media_item_id IN (SELECT item_id FROM picto_delete_item)",
                    [&now],
                )?;
                if cloud_configured {
                    crate::cloud::capture::record_source_item_deletes(
                        transaction,
                        &deleted_source_item_ids,
                    )?;
                }
                trace_bulk_stage("items.delete", "source_tombstones", stage_started);
                stage_started = Instant::now();
                {
                    let mut statement = transaction.prepare(
                        "DELETE FROM library_item
                         WHERE item_id IN (SELECT item_id FROM picto_delete_item)
                         RETURNING item_id, kind",
                    )?;
                    let mut rows = statement.query([])?;
                    while let Some(row) = rows.next()? {
                        let item_id = row.get::<_, i64>(0)?;
                        let kind = match row.get::<_, String>(1)?.as_str() {
                            "media" => crate::app::ItemKind::Media,
                            "collection" => crate::app::ItemKind::Collection,
                            other => {
                                return Err(invalid(format!("Unsupported item kind '{other}'")))
                            }
                        };
                        delta.items.push(ItemProjectionChange {
                            item_id,
                            kind,
                            present: false,
                        });
                        if selected_roots.contains(root_id_u32(item_id)?) {
                            delta.roots.push(RootProjectionChange {
                                item_id,
                                lifecycle: None,
                            });
                        }
                    }
                }
                trace_bulk_stage("items.delete", "canonical_delete", stage_started);
                stage_started = Instant::now();

                let hashes = {
                    let mut statement = transaction.prepare(
                        "DELETE FROM media_file
                         WHERE file_id IN (SELECT file_id FROM picto_candidate_file)
                           AND NOT EXISTS (
                               SELECT 1 FROM media_asset
                               WHERE media_asset.file_id = media_file.file_id
                           )
                         RETURNING file_hash",
                    )?;
                    let hashes = statement
                        .query_map([], |row| row.get::<_, String>(0))?
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
                trace_bulk_stage("items.delete", "blob_work", stage_started);
                stage_started = Instant::now();
                finish_structural_summary_batch_from_staged_roots(transaction)?;
                trace_bulk_stage("items.delete", "summary_settlement", stage_started);
                trace_bulk_stage("items.delete", "closure_total", operation_started);
                Ok(((hashes, item_ids), delta))
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

fn remove_staged_roots_for_collection(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    let mut stage_started;
    for (stage, statement) in [
        (
            "smart_memberships",
            "DELETE FROM smart_folder_membership
             WHERE root_item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
        (
            "recently_viewed",
            "DELETE FROM media_view
             WHERE item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
        (
            "folders",
            "DELETE FROM folder_item
             WHERE item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
        (
            "tags",
            "DELETE FROM root_tag
             WHERE root_item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
        (
            "metadata",
            "DELETE FROM root_metadata
             WHERE root_item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
        (
            "summaries",
            "DELETE FROM root_summary
             WHERE root_item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
        (
            "roots",
            "DELETE FROM library_root
             WHERE item_id IN (SELECT item_id FROM picto_selected_root)",
        ),
    ] {
        stage_started = Instant::now();
        transaction.execute(statement, [])?;
        trace_bulk_stage("groups.remove_roots", stage, stage_started);
    }
    Ok(())
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

fn group_history_universe(
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    seed_ids: &[i64],
) -> rusqlite::Result<Vec<i64>> {
    let mut item_ids = seed_ids.iter().copied().collect::<BTreeSet<_>>();
    for seed_id in seed_ids {
        if let Some(members) = projection.group_order(*seed_id) {
            item_ids.extend(members);
        }
        if let Some(root_id) = projection.root_for_media(*seed_id) {
            item_ids.insert(root_id);
            if root_id != *seed_id {
                item_ids.extend(projection.group_order(root_id).unwrap_or_default());
            }
        }
    }
    Ok(item_ids.into_iter().collect())
}

fn capture_group_state_after_projection(
    transaction: &Transaction<'_>,
    universe_ids: &[i64],
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    structure: &StructureProjectionDelta,
    folder_changes: &[FolderProjectionChange],
) -> rusqlite::Result<CapturedGroupState> {
    let mut state = capture_group_state_internal(transaction, universe_ids)?;
    populate_group_folders_from_projection(&mut state, projection);
    populate_group_members_from_projection(&mut state, projection);
    apply_structure_to_captured_group_state(&mut state, structure);
    for change in folder_changes {
        let Some(root) = state.roots.get_mut(&change.item_id) else {
            continue;
        };
        if change.present {
            if !root
                .folders
                .iter()
                .any(|folder| folder.folder_id == change.folder_id)
            {
                root.folders.push(SemanticGroupFolder {
                    folder_id: change.folder_id,
                    position_rank: None,
                });
            }
        } else {
            root.folders
                .retain(|folder| folder.folder_id != change.folder_id);
        }
        root.folders.sort_by_key(|folder| folder.folder_id);
    }
    Ok(state)
}

fn capture_group_state_from_projection(
    transaction: &Transaction<'_>,
    universe_ids: &[i64],
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    include_root_tags: bool,
) -> rusqlite::Result<CapturedGroupState> {
    let mut state = capture_group_state_internal(transaction, universe_ids)?;
    populate_group_folders_from_projection(&mut state, projection);
    populate_group_members_from_projection(&mut state, projection);
    let roots = bitmap_from_i64s(universe_ids.iter().copied())?;
    for (tag_id, tagged_roots) in projection.tag_memberships_for_roots(&roots) {
        if include_root_tags {
            for root_id in tagged_roots.iter().map(i64::from) {
                if let Some(root) = state.roots.get_mut(&root_id) {
                    root.tags.push(SemanticGroupTag { tag_id });
                }
            }
        }
        state.tag_sets.insert(tag_id, tagged_roots);
    }
    Ok(state)
}

fn populate_group_members_from_projection(
    state: &mut CapturedGroupState,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
) {
    for collection_id in state.item_ids.iter().copied() {
        let Some(order) = projection.group_order(collection_id) else {
            continue;
        };
        for (position, media_item_id) in order.into_iter().enumerate() {
            state.members.insert(
                (collection_id, media_item_id),
                i64::try_from(position + 1)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(RANK_GAP),
            );
        }
    }
}

fn apply_structure_to_captured_group_state(
    state: &mut CapturedGroupState,
    structure: &StructureProjectionDelta,
) {
    for change in &structure.items {
        if !change.present && change.kind == crate::app::ItemKind::Collection {
            state
                .members
                .retain(|(collection_id, _), _| *collection_id != change.item_id);
        }
    }
    for change in &structure.memberships {
        if change.present {
            let next_rank = state
                .members
                .range((change.collection_id, i64::MIN)..=(change.collection_id, i64::MAX))
                .map(|(_, rank)| *rank)
                .max()
                .unwrap_or_default()
                .saturating_add(RANK_GAP);
            state
                .members
                .insert((change.collection_id, change.media_id), next_rank);
        } else {
            state
                .members
                .remove(&(change.collection_id, change.media_id));
        }
    }
    for order in &structure.group_orders {
        state
            .members
            .retain(|(collection_id, _), _| *collection_id != order.collection_id);
        for (position, media_item_id) in order.media_ids.iter().copied().enumerate() {
            state.members.insert(
                (order.collection_id, media_item_id),
                i64::try_from(position + 1)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(RANK_GAP),
            );
        }
    }
}

fn populate_group_folders_from_projection(
    state: &mut CapturedGroupState,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
) {
    for root in state.roots.values_mut() {
        root.folders = projection
            .folder_ids_for_root(root.item_id)
            .into_iter()
            .map(|folder_id| SemanticGroupFolder {
                folder_id,
                position_rank: None,
            })
            .collect();
    }
}

fn capture_group_state_internal(
    transaction: &Transaction<'_>,
    universe_ids: &[i64],
) -> rusqlite::Result<CapturedGroupState> {
    let encoded = serde_json::to_string(universe_ids)
        .map_err(|error| invalid(format!("Could not encode group history state: {error}")))?;
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_group_history_universe (
             item_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_group_history_universe;",
    )?;
    transaction.execute(
        "INSERT INTO picto_group_history_universe(item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)",
        [encoded],
    )?;

    let mut state = CapturedGroupState::default();
    {
        let mut statement = transaction.prepare(
            "SELECT item.item_id
             FROM library_item item
             JOIN picto_group_history_universe selected
               ON selected.item_id = item.item_id
             ORDER BY item.item_id",
        )?;
        state.item_ids = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<_>>()?;
    }
    {
        let mut statement = transaction.prepare(
            "SELECT item.item_id, item.item_key, item.kind,
                    item.cover_media_item_id, root.lifecycle, root.sort_rank,
                    metadata.name, metadata.rating, metadata.notes,
                    COALESCE(metadata.source_urls_json, '[]'),
                    item.created_at, item.updated_at
             FROM picto_group_history_universe selected
             JOIN library_item item ON item.item_id = selected.item_id
             JOIN library_root root ON root.item_id = item.item_id
             LEFT JOIN root_metadata metadata
               ON metadata.root_item_id = item.item_id
             ORDER BY item.item_id",
        )?;
        let roots = statement
            .query_map([], |row| {
                let kind = match row.get::<_, String>(2)?.as_str() {
                    "media" => crate::app::ItemKind::Media,
                    "collection" => crate::app::ItemKind::Collection,
                    other => return Err(invalid(format!("Unsupported item kind '{other}'"))),
                };
                let lifecycle = parse_lifecycle(&row.get::<_, String>(4)?)?;
                Ok(SemanticGroupRoot {
                    item_id: row.get(0)?,
                    item_key: row.get(1)?,
                    kind,
                    cover_media_item_id: row.get(3)?,
                    lifecycle,
                    sort_rank: row.get(5)?,
                    name: row.get(6)?,
                    rating: row.get(7)?,
                    notes: row.get(8)?,
                    source_urls_json: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    folders: Vec::new(),
                    tags: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        state.roots = roots.into_iter().map(|root| (root.item_id, root)).collect();
    }
    Ok(state)
}

fn group_history_record(
    before: &CapturedGroupState,
    after: &CapturedGroupState,
) -> rusqlite::Result<(SemanticHistoryRecord, SemanticGroupDelta)> {
    let undo = group_delta_between(after, before)?;
    let redo = group_delta_between(before, after)?;
    Ok((
        SemanticHistoryRecord::new(
            compact_group_history_payload(undo),
            compact_group_history_payload(redo.clone()),
        ),
        redo,
    ))
}

fn group_history_record_with_memberships(
    before: &CapturedGroupState,
    after: &CapturedGroupState,
    forward_folders: Vec<SemanticMembershipDelta>,
    forward_tags: Vec<SemanticMembershipDelta>,
) -> rusqlite::Result<(SemanticHistoryRecord, SemanticGroupDelta)> {
    let mut undo = group_delta_between(after, before)?;
    undo.folder_changes = reverse_membership_changes(&forward_folders);
    undo.tag_changes = reverse_membership_changes(&forward_tags);

    let mut redo = group_delta_between(before, after)?;
    redo.folder_changes = forward_folders;
    redo.tag_changes = forward_tags;

    Ok((
        SemanticHistoryRecord::new(
            compact_group_history_payload(undo),
            compact_group_history_payload(redo.clone()),
        ),
        redo,
    ))
}

fn compact_group_history_payload(mut delta: SemanticGroupDelta) -> SemanticHistoryPayload {
    let folders = std::mem::take(&mut delta.folder_changes);
    let tags = std::mem::take(&mut delta.tag_changes);
    for root in &mut delta.roots {
        root.folders.clear();
        root.tags.clear();
    }
    SemanticHistoryPayload::Composite(vec![
        SemanticHistoryPayload::Group(delta),
        SemanticHistoryPayload::Folders(folders),
        SemanticHistoryPayload::Tags(tags),
    ])
}

fn reverse_membership_changes(changes: &[SemanticMembershipDelta]) -> Vec<SemanticMembershipDelta> {
    changes
        .iter()
        .map(|change| SemanticMembershipDelta {
            relation_id: change.relation_id,
            add: change.remove.clone(),
            remove: change.add.clone(),
        })
        .collect()
}

fn inherited_group_membership_changes(
    source: &SemanticGroupRoot,
    detached_roots: &RoaringBitmap,
    collection_removed: bool,
) -> rusqlite::Result<(Vec<SemanticMembershipDelta>, Vec<SemanticMembershipDelta>)> {
    let collection_root = root_id_u32(source.item_id)?;
    let folders = source
        .folders
        .iter()
        .map(|folder| SemanticMembershipDelta {
            relation_id: folder.folder_id,
            add: detached_roots.clone(),
            remove: collection_removed
                .then(|| RoaringBitmap::from_iter([collection_root]))
                .unwrap_or_default(),
        })
        .collect();
    let tags = source
        .tags
        .iter()
        .map(|tag| SemanticMembershipDelta {
            relation_id: tag.tag_id,
            add: detached_roots.clone(),
            remove: collection_removed
                .then(|| RoaringBitmap::from_iter([collection_root]))
                .unwrap_or_default(),
        })
        .collect();
    Ok((folders, tags))
}

fn group_delta_between(
    current: &CapturedGroupState,
    desired: &CapturedGroupState,
) -> rusqlite::Result<SemanticGroupDelta> {
    let remove_root_ids = bitmap_from_i64s(
        current
            .roots
            .keys()
            .filter(|item_id| !desired.roots.contains_key(item_id))
            .copied(),
    )?;
    let remove_item_ids =
        bitmap_from_i64s(current.item_ids.difference(&desired.item_ids).copied())?;
    let member_keys = current
        .members
        .keys()
        .chain(desired.members.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let members = member_keys
        .into_iter()
        .map(|(collection_id, media_item_id)| SemanticGroupMember {
            collection_id,
            media_item_id,
            position_rank: desired
                .members
                .get(&(collection_id, media_item_id))
                .copied()
                .or_else(|| {
                    current
                        .members
                        .get(&(collection_id, media_item_id))
                        .copied()
                })
                .unwrap_or_default(),
            present: desired
                .members
                .contains_key(&(collection_id, media_item_id)),
        })
        .collect();
    Ok(SemanticGroupDelta {
        remove_root_ids,
        remove_item_ids,
        roots: desired.roots.values().cloned().collect(),
        members,
        folder_changes: group_folder_changes(current, desired)?,
        tag_changes: group_tag_changes(current, desired)?,
        rating_changes: group_rating_changes(current, desired)?,
    })
}

fn group_folder_changes(
    current: &CapturedGroupState,
    desired: &CapturedGroupState,
) -> rusqlite::Result<Vec<SemanticMembershipDelta>> {
    let current = group_folder_memberships(current)?;
    let desired = group_folder_memberships(desired)?;
    Ok(membership_difference(&current, &desired))
}

fn group_tag_changes(
    current: &CapturedGroupState,
    desired: &CapturedGroupState,
) -> rusqlite::Result<Vec<SemanticMembershipDelta>> {
    Ok(current
        .tag_sets
        .keys()
        .chain(desired.tag_sets.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|relation_id| {
            let current = current
                .tag_sets
                .get(&relation_id)
                .cloned()
                .unwrap_or_default();
            let desired = desired
                .tag_sets
                .get(&relation_id)
                .cloned()
                .unwrap_or_default();
            let add = &desired - &current;
            let remove = &current - &desired;
            (!add.is_empty() || !remove.is_empty()).then_some(SemanticMembershipDelta {
                relation_id,
                add,
                remove,
            })
        })
        .collect())
}

fn group_folder_memberships(
    state: &CapturedGroupState,
) -> rusqlite::Result<BTreeMap<i64, RoaringBitmap>> {
    let mut values = BTreeMap::new();
    for root in state.roots.values() {
        let root_id = root_id_u32(root.item_id)?;
        for folder in &root.folders {
            values
                .entry(folder.folder_id)
                .or_insert_with(RoaringBitmap::new)
                .insert(root_id);
        }
    }
    Ok(values)
}

fn membership_difference(
    current: &BTreeMap<i64, RoaringBitmap>,
    desired: &BTreeMap<i64, RoaringBitmap>,
) -> Vec<SemanticMembershipDelta> {
    current
        .keys()
        .chain(desired.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|relation_id| {
            let current = current.get(&relation_id).cloned().unwrap_or_default();
            let desired = desired.get(&relation_id).cloned().unwrap_or_default();
            let add = &desired - &current;
            let remove = &current - &desired;
            (!add.is_empty() || !remove.is_empty()).then_some(SemanticMembershipDelta {
                relation_id,
                add,
                remove,
            })
        })
        .collect()
}

fn group_rating_changes(
    current: &CapturedGroupState,
    desired: &CapturedGroupState,
) -> rusqlite::Result<SemanticRatingDelta> {
    let mut delta = SemanticRatingDelta::default();
    let mut rated = BTreeMap::<i64, RoaringBitmap>::new();
    for root in desired.roots.values() {
        if current.roots.get(&root.item_id).map(|value| value.rating) == Some(root.rating) {
            continue;
        }
        let root_id = root_id_u32(root.item_id)?;
        if let Some(rating) = root.rating {
            rated.entry(rating).or_default().insert(root_id);
        } else {
            delta.unrated.insert(root_id);
        }
    }
    delta.rated = rated.into_iter().collect();
    Ok(delta)
}

fn bitmap_from_i64s(ids: impl IntoIterator<Item = i64>) -> rusqlite::Result<RoaringBitmap> {
    ids.into_iter()
        .try_fold(RoaringBitmap::new(), |mut bitmap, item_id| {
            bitmap.insert(root_id_u32(item_id)?);
            Ok(bitmap)
        })
}

fn root_id_u32(item_id: i64) -> rusqlite::Result<u32> {
    u32::try_from(item_id)
        .map_err(|_| invalid(format!("Item ID {item_id} exceeds projection capacity")))
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn new_key(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}:{}", hex::encode(bytes))
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

fn require_same_root_lifecycle(
    transaction: &Transaction<'_>,
    item_ids: &[i64],
) -> rusqlite::Result<String> {
    if item_ids.is_empty() {
        return Err(invalid("No items selected"));
    }
    let encoded = serde_json::to_string(item_ids)
        .map_err(|error| invalid(format!("Could not encode root selection: {error}")))?;
    let (count, minimum, maximum): (i64, Option<String>, Option<String>) = transaction.query_row(
        "SELECT COUNT(*), MIN(root.lifecycle), MAX(root.lifecycle)
         FROM json_each(?1) selected
         JOIN library_root root
           ON root.item_id = CAST(selected.value AS INTEGER)",
        [encoded],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if count != item_ids.len() as i64 {
        return Err(invalid("A targeted item is not a library root"));
    }
    if minimum != maximum {
        return Err(invalid("Group members must share one lifecycle"));
    }
    minimum.ok_or_else(|| invalid("No items selected"))
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
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    item_ids: &[i64],
) -> Vec<i64> {
    item_ids
        .iter()
        .flat_map(|item_id| projection.folder_ids_for_root(*item_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn selected_root_ids_of_kind(
    transaction: &Transaction<'_>,
    kind: &str,
) -> rusqlite::Result<Vec<i64>> {
    transaction
        .prepare(
            "SELECT selected.item_id
             FROM picto_selected_root selected
             JOIN library_item item ON item.item_id = selected.item_id
             WHERE item.kind = ?1
             ORDER BY selected.item_id",
        )?
        .query_map([kind], |row| row.get(0))?
        .collect()
}

fn require_no_selected_file_overlap(
    transaction: &Transaction<'_>,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    root_ids: &[i64],
) -> rusqlite::Result<()> {
    let candidates = root_ids
        .iter()
        .flat_map(|root_id| {
            projection
                .group_order(*root_id)
                .unwrap_or_else(|| vec![*root_id])
                .into_iter()
                .map(move |media_id| (*root_id, media_id))
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&candidates)
        .map_err(|error| invalid(format!("Could not encode group candidates: {error}")))?;
    let repeated: Option<i64> = transaction
        .query_row(
            "WITH candidate(origin_id, media_item_id) AS (
                 SELECT CAST(json_extract(value, '$[0]') AS INTEGER),
                        CAST(json_extract(value, '$[1]') AS INTEGER)
                 FROM json_each(?1)
             )
             SELECT asset.file_id
             FROM candidate
             JOIN media_asset asset ON asset.item_id = candidate.media_item_id
             GROUP BY asset.file_id
             HAVING COUNT(DISTINCT candidate.origin_id) > 1
             LIMIT 1",
            [encoded],
            |row| row.get(0),
        )
        .optional()?;
    if repeated.is_some() {
        return Err(invalid(
            "A group cannot contain the same physical file more than once",
        ));
    }
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

fn mutation_item_hints(target: &ItemTarget) -> Vec<i64> {
    match target {
        ItemTarget::Explicit { item_ids } => capped_ids(item_ids.iter().map(|item_id| item_id.0)),
        ItemTarget::Query { .. } | ItemTarget::Range { .. } => Vec::new(),
    }
}

fn capped_ids(ids: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut capped = ids
        .into_iter()
        .take(MAX_RECEIPT_ITEM_IDS + 1)
        .collect::<Vec<_>>();
    if capped.len() > MAX_RECEIPT_ITEM_IDS {
        capped.clear();
    }
    capped
}

fn capped_item_resources(base: &str, item_ids: &[i64]) -> Vec<String> {
    let mut values = Vec::with_capacity(item_ids.len().saturating_add(1));
    values.push(base.to_string());
    values.extend(item_ids.iter().map(|item_id| resources::item(*item_id)));
    values
}

fn semantic_membership(
    relation_id: i64,
    roots: &RoaringBitmap,
    present: bool,
    tags: bool,
) -> SemanticHistoryPayload {
    let change = SemanticMembershipDelta {
        relation_id,
        add: present.then(|| roots.clone()).unwrap_or_default(),
        remove: (!present).then(|| roots.clone()).unwrap_or_default(),
    };
    if tags {
        SemanticHistoryPayload::Tags(vec![change])
    } else {
        SemanticHistoryPayload::Folders(vec![change])
    }
}

fn semantic_tag_memberships(
    delta: &BulkTagProjectionDelta,
    present: bool,
) -> SemanticHistoryPayload {
    SemanticHistoryPayload::Tags(
        delta
            .history_changes
            .iter()
            .map(|change| SemanticMembershipDelta {
                relation_id: change.relation_id,
                add: present.then(|| change.add.clone()).unwrap_or_default(),
                remove: (!present).then(|| change.add.clone()).unwrap_or_default(),
            })
            .collect(),
    )
}

fn lifecycle_delta_for_projection(
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    roots: &RoaringBitmap,
) -> SemanticLifecycleDelta {
    SemanticLifecycleDelta {
        inbox: roots & &projection.lifecycle_bitmap(Lifecycle::Inbox),
        active: roots & &projection.lifecycle_bitmap(Lifecycle::Active),
        trash: roots & &projection.lifecycle_bitmap(Lifecycle::Trash),
    }
}

fn rating_delta_for_projection(
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    roots: &RoaringBitmap,
) -> SemanticRatingDelta {
    SemanticRatingDelta {
        unrated: roots & &projection.rating_value_bitmap(None),
        rated: (0..=5)
            .filter_map(|rating| {
                let matching = roots & &projection.rating_value_bitmap(Some(rating));
                (!matching.is_empty()).then_some((rating, matching))
            })
            .collect(),
    }
}

fn rating_delta_for_target(roots: &RoaringBitmap, rating: Option<i64>) -> SemanticRatingDelta {
    match rating {
        Some(rating) => SemanticRatingDelta {
            rated: vec![(rating, roots.clone())],
            ..SemanticRatingDelta::default()
        },
        None => SemanticRatingDelta {
            unrated: roots.clone(),
            ..SemanticRatingDelta::default()
        },
    }
}

fn lifecycle_delta_for_target(
    roots: &RoaringBitmap,
    lifecycle: Lifecycle,
) -> SemanticLifecycleDelta {
    let mut delta = SemanticLifecycleDelta::default();
    match lifecycle {
        Lifecycle::Inbox => delta.inbox = roots.clone(),
        Lifecycle::Active => delta.active = roots.clone(),
        Lifecycle::Trash => delta.trash = roots.clone(),
    }
    delta
}

fn apply_group_projection_delta(
    projections: &crate::projection_v2::ProjectionStore,
    delta: GroupProjectionDelta,
) -> Result<(), String> {
    projections.apply_structure_delta(delta.structure)?;
    projections.apply_root_summary_changes(&delta.summaries, &RoaringBitmap::new())?;
    for (roots, tag_ids) in delta.shared_tag_sets {
        projections.apply_shared_root_tag_set(&roots, &tag_ids)?;
    }
    for change in delta.tag_changes {
        projections.apply_root_tag_bitmap(change.relation_id, &change.remove, false)?;
        projections.apply_root_tag_bitmap(change.relation_id, &change.add, true)?;
    }
    projections.apply_rating_bitmap(&delta.rating_changes.unrated, None)?;
    for (rating, roots) in delta.rating_changes.rated {
        projections.apply_rating_bitmap(
            &roots,
            Some(
                u8::try_from(rating)
                    .map_err(|_| format!("rating {rating} is outside the projection range"))?,
            ),
        )?;
    }
    Ok(())
}

pub(crate) fn root_summary_changes_for_roots(
    transaction: &Transaction<'_>,
    root_ids: &[i64],
) -> rusqlite::Result<Vec<RootSummaryProjectionChange>> {
    if root_ids.is_empty() {
        return Ok(Vec::new());
    }
    let encoded = serde_json::to_string(root_ids)
        .map_err(|error| invalid(format!("Could not encode summary roots: {error}")))?;
    transaction
        .prepare(
            "SELECT summary.root_item_id, summary.total_size_bytes,
                    summary.media_count, summary.sort_rating,
                    file.duration_ms, file.pixel_width, file.pixel_height,
                    summary.imported_at, summary.updated_at
             FROM root_summary summary
             LEFT JOIN media_asset cover
               ON cover.item_id = summary.cover_media_item_id
             LEFT JOIN media_file file ON file.file_id = cover.file_id
             JOIN json_each(?1) selected
               ON CAST(selected.value AS INTEGER) = summary.root_item_id",
        )?
        .query_map([encoded], |row| {
            let total_size_bytes = u64::try_from(row.get::<_, i64>(1)?)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, 0))?;
            let media_count = u64::try_from(row.get::<_, i64>(2)?)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, 0))?;
            let rating = row
                .get::<_, Option<i64>>(3)?
                .map(u8::try_from)
                .transpose()
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, 0))?;
            Ok(RootSummaryProjectionChange {
                item_id: row.get(0)?,
                total_size_bytes,
                media_count,
                rating,
                display_duration_ms: optional_u64(row, 4)?,
                display_width: optional_u64(row, 5)?,
                display_height: optional_u64(row, 6)?,
                imported_at_ms: optional_timestamp_ms(row, 7)?,
                modified_at_ms: optional_timestamp_ms(row, 8)?,
            })
        })?
        .collect()
}

fn optional_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

fn optional_timestamp_ms(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<i64>> {
    Ok(row
        .get::<_, Option<String>>(index)?
        .as_deref()
        .and_then(timestamp_ms))
}

fn limited_staged_hints(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
) -> rusqlite::Result<Vec<i64>> {
    let sql = format!(
        "SELECT {column} FROM {table} LIMIT {}",
        MAX_RECEIPT_ITEM_IDS + 1
    );
    let mut statement = transaction.prepare(&sql)?;
    let hints = statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(if hints.len() > MAX_RECEIPT_ITEM_IDS {
        Vec::new()
    } else {
        hints
    })
}

fn stage_mutation_selection(
    transaction: &Transaction<'_>,
    target: &ItemTarget,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
) -> rusqlite::Result<()> {
    stage_root_selection(transaction, target)?;
    populate_staged_media(transaction, projection)
}

fn stage_root_selection(
    transaction: &Transaction<'_>,
    target: &ItemTarget,
) -> rusqlite::Result<()> {
    prepare_mutation_selection_tables(transaction)?;
    let selection = crate::query_v2::target_selection_sql(transaction, target)?;
    let sql = format!(
        "{}
         INSERT INTO picto_selected_root(item_id)
         SELECT item_id FROM selected_roots",
        selection.with_clause
    );
    transaction.execute(&sql, selection.parameters().as_slice())?;
    Ok(())
}

fn stage_root_selection_projected(
    transaction: &Transaction<'_>,
    target: &ItemTarget,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
) -> rusqlite::Result<()> {
    if let ItemTarget::Query {
        query,
        excluded_item_ids,
    } = target
    {
        if let Some(mut roots) =
            crate::predicate_v2::compile_item_query(transaction, projection, query)?
        {
            for item_id in excluded_item_ids {
                if let Ok(item_id) = u32::try_from(item_id.0) {
                    roots.remove(item_id);
                }
            }
            return stage_root_bitmap(transaction, &roots);
        }
    }
    stage_root_selection(transaction, target)
}

fn stage_root_bitmap(transaction: &Transaction<'_>, roots: &RoaringBitmap) -> rusqlite::Result<()> {
    prepare_mutation_selection_tables(transaction)?;
    let encoded = serde_json::to_string(&roots.iter().collect::<Vec<_>>())
        .map_err(|error| invalid(format!("Could not encode root selection: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_selected_root(item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)",
        [encoded],
    )?;
    Ok(())
}

fn stage_root_ids(
    transaction: &Transaction<'_>,
    item_ids: &[i64],
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
) -> rusqlite::Result<()> {
    prepare_mutation_selection_tables(transaction)?;
    let encoded = serde_json::to_string(item_ids)
        .map_err(|error| invalid(format!("Could not encode root selection: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_selected_root(item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)",
        [encoded],
    )?;
    let media_ids = item_ids
        .iter()
        .flat_map(|item_id| {
            projection
                .group_order(*item_id)
                .unwrap_or_else(|| vec![*item_id])
        })
        .collect::<Vec<_>>();
    let encoded_media_ids = serde_json::to_string(&media_ids)
        .map_err(|error| invalid(format!("Could not encode media selection: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_selected_media(media_item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)
         WHERE TRUE ON CONFLICT DO NOTHING",
        [encoded_media_ids],
    )?;
    Ok(())
}

fn prepare_mutation_selection_tables(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_selected_root (
             item_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_selected_media (
             media_item_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_selected_root;
         DELETE FROM picto_selected_media;",
    )
}

fn populate_staged_media(
    transaction: &Transaction<'_>,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
) -> rusqlite::Result<()> {
    let root_ids = transaction
        .prepare("SELECT item_id FROM picto_selected_root ORDER BY item_id")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let media_ids = root_ids
        .into_iter()
        .flat_map(|item_id| {
            projection
                .group_order(item_id)
                .unwrap_or_else(|| vec![item_id])
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&media_ids)
        .map_err(|error| invalid(format!("Could not encode media selection: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_selected_media(media_item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)
         WHERE TRUE ON CONFLICT DO NOTHING",
        [encoded],
    )?;
    Ok(())
}

fn stage_media_ids(transaction: &Transaction<'_>, media_ids: &[i64]) -> rusqlite::Result<()> {
    prepare_mutation_selection_tables(transaction)?;
    let encoded = serde_json::to_string(media_ids)
        .map_err(|error| invalid(format!("Could not encode media selection: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_selected_media(media_item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)",
        [encoded],
    )?;
    Ok(())
}

fn create_staged_roots_with_metadata(
    transaction: &Transaction<'_>,
    lifecycle: &str,
    source_root_id: i64,
) -> rusqlite::Result<()> {
    let operation_started = Instant::now();
    let mut stage_started = operation_started;
    transaction.execute(
        "INSERT INTO library_root (item_id, lifecycle)
         SELECT media_item_id, ?1 FROM picto_selected_media",
        [lifecycle],
    )?;
    trace_bulk_stage(
        "groups.create_detached_roots",
        "library_root",
        stage_started,
    );
    stage_started = Instant::now();
    let now = chrono::Utc::now().to_rfc3339();
    transaction.execute(
        "INSERT INTO root_metadata (
             root_item_id, name, rating, notes, source_urls_json, updated_at
         )
         SELECT selected.media_item_id, asset.name, source.rating, source.notes,
                COALESCE(source.source_urls_json, '[]'), ?2
         FROM picto_selected_media selected
         JOIN media_asset asset ON asset.item_id = selected.media_item_id
         LEFT JOIN root_metadata source ON source.root_item_id = ?1
         WHERE TRUE
         ORDER BY selected.media_item_id
         ON CONFLICT(root_item_id) DO UPDATE SET
             name = excluded.name,
             rating = excluded.rating,
             notes = excluded.notes,
             source_urls_json = excluded.source_urls_json,
             updated_at = excluded.updated_at",
        params![source_root_id, now],
    )?;
    trace_bulk_stage("groups.create_detached_roots", "metadata", stage_started);
    trace_bulk_stage("groups.create_detached_roots", "total", operation_started);
    Ok(())
}

fn prepare_structural_summary_tables(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_structural_root (
             item_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_folder (
             folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_tag (
             tag_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_old_summary (
             root_item_id INTEGER PRIMARY KEY,
             lifecycle TEXT NOT NULL,
             media_count INTEGER NOT NULL,
             total_size_bytes INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_lifecycle_before (
             lifecycle TEXT PRIMARY KEY,
             root_count INTEGER NOT NULL,
             media_count INTEGER NOT NULL,
             total_size_bytes INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_old_folder (
             root_item_id INTEGER NOT NULL,
             folder_id INTEGER NOT NULL,
             PRIMARY KEY (root_item_id, folder_id)
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_old_tag (
             root_item_id INTEGER NOT NULL,
             tag_id INTEGER NOT NULL,
             PRIMARY KEY (root_item_id, tag_id)
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_folder_before (
             folder_id INTEGER PRIMARY KEY,
             visible_root_count INTEGER NOT NULL,
             media_count INTEGER NOT NULL,
             total_size_bytes INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_structural_tag_before (
             tag_id INTEGER PRIMARY KEY,
             visible_root_count INTEGER NOT NULL,
             assignment_count INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS temp.picto_structural_old_folder_by_folder
             ON picto_structural_old_folder(folder_id, root_item_id);
         CREATE INDEX IF NOT EXISTS temp.picto_structural_old_tag_by_tag
             ON picto_structural_old_tag(tag_id, root_item_id);
         DELETE FROM picto_structural_root;
         DELETE FROM picto_structural_folder;
         DELETE FROM picto_structural_tag;
         DELETE FROM picto_structural_old_summary;
         DELETE FROM picto_structural_lifecycle_before;
         DELETE FROM picto_structural_old_folder;
         DELETE FROM picto_structural_old_tag;
         DELETE FROM picto_structural_folder_before;
         DELETE FROM picto_structural_tag_before;",
    )
}

fn stage_structural_summary_dependencies(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "INSERT INTO picto_structural_folder(folder_id)
         SELECT DISTINCT membership.folder_id
         FROM folder_item membership
         JOIN picto_structural_root changed ON changed.item_id = membership.item_id
         WHERE TRUE
         ON CONFLICT DO NOTHING;
         INSERT INTO picto_structural_tag(tag_id)
         SELECT DISTINCT relation.tag_id
         FROM root_tag relation
         JOIN picto_structural_root changed ON changed.item_id = relation.root_item_id
         WHERE TRUE
         ON CONFLICT DO NOTHING;",
    )
}

fn stage_structural_summary_roots(
    transaction: &Transaction<'_>,
    root_ids: &[i64],
) -> rusqlite::Result<()> {
    if root_ids.is_empty() {
        return Ok(());
    }
    let encoded = serde_json::to_string(root_ids)
        .map_err(|error| invalid(format!("Could not encode structural roots: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_structural_root(item_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)
         WHERE TRUE
         ON CONFLICT DO NOTHING",
        [encoded],
    )?;
    Ok(())
}

fn suppress_structural_summaries(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE projection_write_control
         SET suppress_root_summary = 1,
             suppress_folder_summary = 1,
             suppress_tag_summary = 1,
             suppress_smart_dirty = 1
         WHERE singleton = 1",
        [],
    )?;
    Ok(())
}

fn begin_group_create_summary_batch(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_group_create_root (
             lifecycle TEXT PRIMARY KEY,
             root_count INTEGER NOT NULL,
             media_count INTEGER NOT NULL,
             total_size_bytes INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_group_create_folder (
             folder_id INTEGER PRIMARY KEY,
             visible_root_count INTEGER NOT NULL,
             media_count INTEGER NOT NULL,
             total_size_bytes INTEGER NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM picto_group_create_root;
         DELETE FROM picto_group_create_folder;

         INSERT INTO picto_group_create_root (
             lifecycle, root_count, media_count, total_size_bytes
         )
         SELECT summary.lifecycle, COUNT(*), SUM(summary.media_count),
                SUM(summary.total_size_bytes)
         FROM root_summary summary
         JOIN picto_selected_root selected
           ON selected.item_id = summary.root_item_id
         GROUP BY summary.lifecycle;

         INSERT INTO picto_group_create_folder (
             folder_id, visible_root_count, media_count, total_size_bytes
         )
         SELECT membership.folder_id,
                SUM(summary.lifecycle = 'active'),
                COALESCE(SUM(CASE WHEN summary.lifecycle = 'active'
                                  THEN summary.media_count ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN summary.lifecycle = 'active'
                                  THEN summary.total_size_bytes ELSE 0 END), 0)
         FROM folder_item membership
         JOIN picto_selected_root selected ON selected.item_id = membership.item_id
         JOIN root_summary summary ON summary.root_item_id = membership.item_id
         GROUP BY membership.folder_id;",
    )?;
    suppress_structural_summaries(transaction)
}

fn finish_group_create_summary_batch(
    transaction: &Transaction<'_>,
    collection_id: i64,
    media_ids: &[i64],
) -> rusqlite::Result<()> {
    upsert_group_root_summary(transaction, collection_id, media_ids)?;
    let (lifecycle, media_count, total_size_bytes): (String, i64, i64) = transaction.query_row(
        "SELECT lifecycle, media_count, total_size_bytes
         FROM root_summary WHERE root_item_id = ?1",
        [collection_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    transaction.execute(
        "UPDATE lifecycle_summary
         SET root_count = root_count
                 - COALESCE((SELECT root_count FROM picto_group_create_root old
                             WHERE old.lifecycle = lifecycle_summary.lifecycle), 0)
                 + CASE WHEN lifecycle = ?1 THEN 1 ELSE 0 END,
             media_count = media_count
                 - COALESCE((SELECT media_count FROM picto_group_create_root old
                             WHERE old.lifecycle = lifecycle_summary.lifecycle), 0)
                 + CASE WHEN lifecycle = ?1 THEN ?2 ELSE 0 END,
             total_size_bytes = total_size_bytes
                 - COALESCE((SELECT total_size_bytes FROM picto_group_create_root old
                             WHERE old.lifecycle = lifecycle_summary.lifecycle), 0)
                 + CASE WHEN lifecycle = ?1 THEN ?3 ELSE 0 END",
        params![lifecycle, media_count, total_size_bytes],
    )?;
    transaction.execute(
        "UPDATE folder_summary
         SET visible_root_count = visible_root_count
                 - (SELECT old.visible_root_count FROM picto_group_create_folder old
                    WHERE old.folder_id = folder_summary.folder_id)
                 + CASE WHEN ?1 = 'active' THEN 1 ELSE 0 END,
             media_count = media_count
                 - (SELECT old.media_count FROM picto_group_create_folder old
                    WHERE old.folder_id = folder_summary.folder_id)
                 + CASE WHEN ?1 = 'active' THEN ?2 ELSE 0 END,
             total_size_bytes = total_size_bytes
                 - (SELECT old.total_size_bytes FROM picto_group_create_folder old
                    WHERE old.folder_id = folder_summary.folder_id)
                 + CASE WHEN ?1 = 'active' THEN ?3 ELSE 0 END
         WHERE folder_id IN (SELECT folder_id FROM picto_group_create_folder)",
        params![lifecycle, media_count, total_size_bytes],
    )?;
    transaction.execute(
        "UPDATE projection_write_control
         SET suppress_root_summary = 0,
             suppress_folder_summary = 0,
             suppress_tag_summary = 0,
             suppress_smart_dirty = 0
         WHERE singleton = 1",
        [],
    )?;
    Ok(())
}

pub(crate) fn upsert_group_root_summary(
    transaction: &Transaction<'_>,
    collection_id: i64,
    media_ids: &[i64],
) -> rusqlite::Result<()> {
    let encoded_media_ids = serde_json::to_string(media_ids)
        .map_err(|error| invalid(format!("Could not encode group members: {error}")))?;
    transaction.execute(
        "INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
         VALUES (?1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
         ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms",
        [collection_id],
    )?;
    transaction.execute(
        "INSERT INTO root_summary (
             root_item_id, lifecycle, kind, cover_media_item_id, media_count,
             total_size_bytes, imported_at, captured_at, sort_rating,
             sort_name, updated_at
         )
         SELECT item.item_id, root.lifecycle, item.kind,
                item.cover_media_item_id,
                COUNT(*),
                COALESCE(SUM(file.size_bytes), 0),
                MAX(asset.imported_at), MAX(asset.captured_at), metadata.rating,
                metadata.name, COALESCE(metadata.updated_at, item.updated_at)
         FROM library_item item
         JOIN library_root root ON root.item_id = item.item_id
         JOIN json_each(?2) member
         JOIN media_asset asset ON asset.item_id = CAST(member.value AS INTEGER)
         JOIN media_file file ON file.file_id = asset.file_id
         LEFT JOIN root_metadata metadata ON metadata.root_item_id = item.item_id
         WHERE item.item_id = ?1
         GROUP BY item.item_id
         ON CONFLICT(root_item_id) DO UPDATE SET
             lifecycle = excluded.lifecycle,
             kind = excluded.kind,
             cover_media_item_id = excluded.cover_media_item_id,
             media_count = excluded.media_count,
             total_size_bytes = excluded.total_size_bytes,
             imported_at = excluded.imported_at,
             captured_at = excluded.captured_at,
             sort_rating = excluded.sort_rating,
             sort_name = excluded.sort_name,
             updated_at = excluded.updated_at",
        params![collection_id, encoded_media_ids],
    )?;
    Ok(())
}

fn begin_bulk_lifecycle_settlement(
    transaction: &Transaction<'_>,
    summary_delta: &LifecycleSummaryDelta,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_lifecycle_old_root (
             root_item_id INTEGER PRIMARY KEY,
             lifecycle TEXT NOT NULL,
             media_count INTEGER NOT NULL,
             total_size_bytes INTEGER NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM picto_lifecycle_old_root;
         INSERT INTO picto_lifecycle_old_root (
             root_item_id, lifecycle, media_count, total_size_bytes
         )
         SELECT summary.root_item_id, summary.lifecycle,
                summary.media_count, summary.total_size_bytes
         FROM root_summary summary
         JOIN picto_changed_root changed ON changed.item_id = summary.root_item_id;

         UPDATE projection_write_control
         SET suppress_root_summary = 1,
             suppress_folder_summary = 1,
             suppress_tag_summary = 1
         WHERE singleton = 1;

        ",
    )?;
    summary_delta.stage(transaction)
}

fn finish_bulk_lifecycle_settlement(
    transaction: &Transaction<'_>,
    lifecycle: Lifecycle,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE root_summary
         SET lifecycle = ?1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE root_item_id IN (SELECT root_item_id FROM picto_lifecycle_old_root)",
        [lifecycle.as_str()],
    )?;
    transaction.execute(
        "UPDATE lifecycle_summary
         SET root_count = root_count
                 - COALESCE((
                     SELECT COUNT(*) FROM picto_lifecycle_old_root old
                     WHERE old.lifecycle = lifecycle_summary.lifecycle
                 ), 0)
                 + CASE WHEN lifecycle_summary.lifecycle = ?1
                     THEN (SELECT COUNT(*) FROM picto_lifecycle_old_root)
                     ELSE 0 END,
             media_count = media_count
                 - COALESCE((
                     SELECT SUM(old.media_count) FROM picto_lifecycle_old_root old
                     WHERE old.lifecycle = lifecycle_summary.lifecycle
                 ), 0)
                 + CASE WHEN lifecycle_summary.lifecycle = ?1
                     THEN COALESCE((SELECT SUM(media_count) FROM picto_lifecycle_old_root), 0)
                     ELSE 0 END,
             total_size_bytes = total_size_bytes
                 - COALESCE((
                     SELECT SUM(old.total_size_bytes) FROM picto_lifecycle_old_root old
                     WHERE old.lifecycle = lifecycle_summary.lifecycle
                 ), 0)
                 + CASE WHEN lifecycle_summary.lifecycle = ?1
                     THEN COALESCE((SELECT SUM(total_size_bytes)
                                    FROM picto_lifecycle_old_root), 0)
                     ELSE 0 END",
        [lifecycle.as_str()],
    )?;
    transaction.execute(
        "UPDATE folder_summary
         SET visible_root_count = visible_root_count + (
                 SELECT delta.root_count FROM picto_lifecycle_folder_delta delta
                 WHERE delta.folder_id = folder_summary.folder_id
             ),
             media_count = media_count + (
                 SELECT delta.media_count FROM picto_lifecycle_folder_delta delta
                 WHERE delta.folder_id = folder_summary.folder_id
             ),
             total_size_bytes = total_size_bytes + (
                 SELECT delta.total_size_bytes FROM picto_lifecycle_folder_delta delta
                 WHERE delta.folder_id = folder_summary.folder_id
             )
         WHERE folder_id IN (SELECT folder_id FROM picto_lifecycle_folder_delta)",
        [],
    )?;
    transaction.execute(
        "UPDATE tag_summary
         SET visible_root_count = visible_root_count + (
             SELECT delta.visible_root_count FROM picto_lifecycle_tag_delta delta
             WHERE delta.tag_id = tag_summary.tag_id
         )
         WHERE tag_id IN (SELECT tag_id FROM picto_lifecycle_tag_delta)",
        [],
    )?;
    transaction.execute_batch(
        "UPDATE projection_write_control
         SET suppress_root_summary = 0,
             suppress_folder_summary = 0,
             suppress_tag_summary = 0
         WHERE singleton = 1;",
    )?;
    Ok(())
}

fn capture_structural_summary_baseline(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "INSERT INTO picto_structural_old_summary (
             root_item_id, lifecycle, media_count, total_size_bytes
         )
         SELECT summary.root_item_id, summary.lifecycle,
                summary.media_count, summary.total_size_bytes
         FROM root_summary summary
         JOIN picto_structural_root changed ON changed.item_id = summary.root_item_id;
         INSERT INTO picto_structural_lifecycle_before (
             lifecycle, root_count, media_count, total_size_bytes
         )
         SELECT lifecycle, root_count, media_count, total_size_bytes
         FROM lifecycle_summary;
         INSERT INTO picto_structural_old_folder(root_item_id, folder_id)
         SELECT membership.item_id, membership.folder_id
         FROM folder_item membership
         JOIN picto_structural_root changed ON changed.item_id = membership.item_id;
         INSERT INTO picto_structural_old_tag(root_item_id, tag_id)
         SELECT relation.root_item_id, relation.tag_id
         FROM root_tag relation
         JOIN picto_structural_root changed ON changed.item_id = relation.root_item_id;
         INSERT INTO picto_structural_folder_before (
             folder_id, visible_root_count, media_count, total_size_bytes
         )
         SELECT summary.folder_id, summary.visible_root_count,
                summary.media_count, summary.total_size_bytes
         FROM folder_summary summary
         JOIN picto_structural_folder changed ON changed.folder_id = summary.folder_id;
         INSERT INTO picto_structural_tag_before (
             tag_id, visible_root_count, assignment_count
         )
         SELECT summary.tag_id, summary.visible_root_count, summary.assignment_count
         FROM tag_summary summary
         JOIN picto_structural_tag changed ON changed.tag_id = summary.tag_id;",
    )
}

fn begin_structural_summary_batch(
    transaction: &Transaction<'_>,
    root_ids: &[i64],
) -> rusqlite::Result<()> {
    prepare_structural_summary_tables(transaction)?;
    stage_structural_summary_roots(transaction, root_ids)?;
    stage_structural_summary_dependencies(transaction)?;
    capture_structural_summary_baseline(transaction)?;
    suppress_structural_summaries(transaction)
}

fn begin_structural_summary_batch_from_staged_roots(
    transaction: &Transaction<'_>,
) -> rusqlite::Result<()> {
    prepare_structural_summary_tables(transaction)?;
    transaction.execute_batch(
        "INSERT INTO picto_structural_root(item_id)
         SELECT item_id FROM picto_selected_root
         WHERE TRUE
         ON CONFLICT DO NOTHING;",
    )?;
    stage_structural_summary_dependencies(transaction)?;
    capture_structural_summary_baseline(transaction)?;
    suppress_structural_summaries(transaction)
}

fn finish_structural_summary_batch(
    transaction: &Transaction<'_>,
    root_ids: &[i64],
) -> rusqlite::Result<()> {
    stage_structural_summary_roots(transaction, root_ids)?;
    finish_structural_summary_batch_inner(transaction)
}

fn finish_structural_summary_batch_from_staged_roots(
    transaction: &Transaction<'_>,
) -> rusqlite::Result<()> {
    finish_structural_summary_batch_inner(transaction)
}

fn finish_structural_summary_batch_inner(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    stage_structural_summary_dependencies(transaction)?;
    transaction.execute(
        "DELETE FROM root_summary
         WHERE root_item_id IN (SELECT item_id FROM picto_structural_root)
           AND NOT EXISTS (
               SELECT 1 FROM library_root root
               WHERE root.item_id = root_summary.root_item_id
           )",
        [],
    )?;
    transaction.execute(
        "INSERT INTO root_summary (
             root_item_id, lifecycle, kind, cover_media_item_id, media_count,
             total_size_bytes, imported_at, captured_at, sort_rating,
             sort_name, updated_at
         )
         SELECT item.item_id,
                root.lifecycle,
                item.kind,
                COALESCE(
                    item.cover_media_item_id,
                    CASE WHEN item.kind = 'media' THEN item.item_id END,
                    (SELECT member.media_item_id
                     FROM collection_member member
                     WHERE member.collection_id = item.item_id
                     ORDER BY member.position_rank, member.media_item_id
                     LIMIT 1)
                ),
                CASE WHEN item.kind = 'media' THEN 1 ELSE (
                    SELECT COUNT(*) FROM collection_member member
                    WHERE member.collection_id = item.item_id
                ) END,
                CASE WHEN item.kind = 'media' THEN COALESCE(file.size_bytes, 0)
                     ELSE COALESCE((
                         SELECT SUM(member_file.size_bytes)
                         FROM collection_member member
                         JOIN media_asset member_asset
                           ON member_asset.item_id = member.media_item_id
                         JOIN media_file member_file
                           ON member_file.file_id = member_asset.file_id
                         WHERE member.collection_id = item.item_id
                     ), 0) END,
                CASE WHEN item.kind = 'media' THEN asset.imported_at ELSE (
                    SELECT MAX(member_asset.imported_at)
                    FROM collection_member member
                    JOIN media_asset member_asset
                      ON member_asset.item_id = member.media_item_id
                    WHERE member.collection_id = item.item_id
                ) END,
                CASE WHEN item.kind = 'media' THEN asset.captured_at ELSE (
                    SELECT MAX(member_asset.captured_at)
                    FROM collection_member member
                    JOIN media_asset member_asset
                      ON member_asset.item_id = member.media_item_id
                    WHERE member.collection_id = item.item_id
                ) END,
                metadata.rating,
                COALESCE(metadata.name, asset.name),
                COALESCE(metadata.updated_at, item.updated_at)
         FROM picto_structural_root changed
         JOIN library_root root ON root.item_id = changed.item_id
         JOIN library_item item ON item.item_id = root.item_id
         LEFT JOIN media_asset asset ON asset.item_id = COALESCE(
             item.cover_media_item_id,
             CASE WHEN item.kind = 'media' THEN item.item_id END,
             (SELECT member.media_item_id
              FROM collection_member member
              WHERE member.collection_id = item.item_id
              ORDER BY member.position_rank, member.media_item_id
              LIMIT 1)
         )
         LEFT JOIN media_file file ON file.file_id = asset.file_id
         LEFT JOIN root_metadata metadata ON metadata.root_item_id = item.item_id
         WHERE item.kind = 'media'
            OR EXISTS (
                SELECT 1 FROM collection_member member
                WHERE member.collection_id = item.item_id
            )
         ON CONFLICT(root_item_id) DO UPDATE SET
             lifecycle = excluded.lifecycle,
             kind = excluded.kind,
             cover_media_item_id = excluded.cover_media_item_id,
             media_count = excluded.media_count,
             total_size_bytes = excluded.total_size_bytes,
             imported_at = excluded.imported_at,
             captured_at = excluded.captured_at,
             sort_rating = excluded.sort_rating,
             sort_name = excluded.sort_name,
             updated_at = excluded.updated_at",
        [],
    )?;
    transaction.execute_batch(
        "UPDATE lifecycle_summary
         SET root_count = (
                 SELECT baseline.root_count
                 FROM picto_structural_lifecycle_before baseline
                 WHERE baseline.lifecycle = lifecycle_summary.lifecycle
             )
             - (SELECT COUNT(*) FROM picto_structural_old_summary old
                WHERE old.lifecycle = lifecycle_summary.lifecycle)
             + (SELECT COUNT(*)
                FROM root_summary summary
                JOIN picto_structural_root changed
                  ON changed.item_id = summary.root_item_id
                WHERE summary.lifecycle = lifecycle_summary.lifecycle),
             media_count = (
                 SELECT baseline.media_count
                 FROM picto_structural_lifecycle_before baseline
                 WHERE baseline.lifecycle = lifecycle_summary.lifecycle
             )
             - COALESCE((SELECT SUM(old.media_count)
                         FROM picto_structural_old_summary old
                         WHERE old.lifecycle = lifecycle_summary.lifecycle), 0)
             + COALESCE((SELECT SUM(summary.media_count)
                         FROM root_summary summary
                         JOIN picto_structural_root changed
                           ON changed.item_id = summary.root_item_id
                         WHERE summary.lifecycle = lifecycle_summary.lifecycle), 0),
             total_size_bytes = (
                 SELECT baseline.total_size_bytes
                 FROM picto_structural_lifecycle_before baseline
                 WHERE baseline.lifecycle = lifecycle_summary.lifecycle
             )
             - COALESCE((SELECT SUM(old.total_size_bytes)
                         FROM picto_structural_old_summary old
                         WHERE old.lifecycle = lifecycle_summary.lifecycle), 0)
             + COALESCE((SELECT SUM(summary.total_size_bytes)
                         FROM root_summary summary
                         JOIN picto_structural_root changed
                           ON changed.item_id = summary.root_item_id
                         WHERE summary.lifecycle = lifecycle_summary.lifecycle), 0);

         INSERT INTO folder_summary (
             folder_id, visible_root_count, media_count, total_size_bytes
         )
         SELECT folder.folder_id,
                COALESCE(baseline.visible_root_count, 0)
                    - (SELECT COUNT(*)
                       FROM picto_structural_old_folder old_membership
                       JOIN picto_structural_old_summary old_summary
                         ON old_summary.root_item_id = old_membership.root_item_id
                       WHERE old_membership.folder_id = folder.folder_id
                         AND old_summary.lifecycle = 'active')
                    + (SELECT COUNT(*)
                       FROM folder_item membership
                       JOIN picto_structural_root changed_root
                         ON changed_root.item_id = membership.item_id
                       JOIN root_summary summary
                         ON summary.root_item_id = membership.item_id
                       WHERE membership.folder_id = folder.folder_id
                         AND summary.lifecycle = 'active'),
                COALESCE(baseline.media_count, 0)
                    - COALESCE((
                        SELECT SUM(old_summary.media_count)
                        FROM picto_structural_old_folder old_membership
                        JOIN picto_structural_old_summary old_summary
                          ON old_summary.root_item_id = old_membership.root_item_id
                        WHERE old_membership.folder_id = folder.folder_id
                          AND old_summary.lifecycle = 'active'
                    ), 0)
                    + COALESCE((
                        SELECT SUM(summary.media_count)
                        FROM folder_item membership
                        JOIN picto_structural_root changed_root
                          ON changed_root.item_id = membership.item_id
                        JOIN root_summary summary
                          ON summary.root_item_id = membership.item_id
                        WHERE membership.folder_id = folder.folder_id
                          AND summary.lifecycle = 'active'
                    ), 0),
                COALESCE(baseline.total_size_bytes, 0)
                    - COALESCE((
                        SELECT SUM(old_summary.total_size_bytes)
                        FROM picto_structural_old_folder old_membership
                        JOIN picto_structural_old_summary old_summary
                          ON old_summary.root_item_id = old_membership.root_item_id
                        WHERE old_membership.folder_id = folder.folder_id
                          AND old_summary.lifecycle = 'active'
                    ), 0)
                    + COALESCE((
                        SELECT SUM(summary.total_size_bytes)
                        FROM folder_item membership
                        JOIN picto_structural_root changed_root
                          ON changed_root.item_id = membership.item_id
                        JOIN root_summary summary
                          ON summary.root_item_id = membership.item_id
                        WHERE membership.folder_id = folder.folder_id
                          AND summary.lifecycle = 'active'
                    ), 0)
         FROM picto_structural_folder changed
         JOIN folder ON folder.folder_id = changed.folder_id
         LEFT JOIN picto_structural_folder_before baseline
           ON baseline.folder_id = folder.folder_id
         ON CONFLICT(folder_id) DO UPDATE SET
             visible_root_count = excluded.visible_root_count,
             media_count = excluded.media_count,
             total_size_bytes = excluded.total_size_bytes;

         INSERT INTO tag_summary (
             tag_id, visible_root_count, assignment_count
         )
         SELECT tag.tag_id,
                COALESCE(baseline.visible_root_count, 0)
                    - (SELECT COUNT(*)
                       FROM picto_structural_old_tag old_relation
                       JOIN picto_structural_old_summary old_summary
                         ON old_summary.root_item_id = old_relation.root_item_id
                       WHERE old_relation.tag_id = tag.tag_id
                         AND old_summary.lifecycle = 'active')
                    + (SELECT COUNT(*)
                       FROM root_tag relation
                       JOIN picto_structural_root changed_root
                         ON changed_root.item_id = relation.root_item_id
                       JOIN root_summary summary
                         ON summary.root_item_id = relation.root_item_id
                       WHERE relation.tag_id = tag.tag_id
                         AND summary.lifecycle = 'active'),
                COALESCE(baseline.assignment_count, 0)
                    - (SELECT COUNT(*) FROM picto_structural_old_tag old_relation
                       WHERE old_relation.tag_id = tag.tag_id)
                    + (SELECT COUNT(*)
                       FROM root_tag relation
                       JOIN picto_structural_root changed_root
                         ON changed_root.item_id = relation.root_item_id
                       WHERE relation.tag_id = tag.tag_id)
         FROM picto_structural_tag changed
         JOIN tag ON tag.tag_id = changed.tag_id
         LEFT JOIN picto_structural_tag_before baseline ON baseline.tag_id = tag.tag_id
         ON CONFLICT(tag_id) DO UPDATE SET
             visible_root_count = excluded.visible_root_count,
             assignment_count = excluded.assignment_count;

         UPDATE projection_write_control
         SET suppress_root_summary = 0,
             suppress_folder_summary = 0,
             suppress_tag_summary = 0,
             suppress_smart_dirty = 0
         WHERE singleton = 1;",
    )?;
    Ok(())
}

fn trace_bulk_stage(operation: &str, stage: &str, started: Instant) {
    if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some() {
        eprintln!(
            "bulk_operation_stage operation={operation} stage={stage} elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
}

pub(crate) fn stage_folder_subtree_selection(
    transaction: &Transaction<'_>,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    folder_id: i64,
) -> rusqlite::Result<()> {
    let folder_ids = transaction
        .prepare(
            "WITH RECURSIVE descendants(folder_id) AS (
             SELECT ?1
             UNION ALL
             SELECT folder.folder_id
             FROM folder
             JOIN descendants parent ON folder.parent_id = parent.folder_id
         )
         SELECT folder_id FROM descendants ORDER BY folder_id",
        )?
        .query_map([folder_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let roots = folder_ids
        .into_iter()
        .fold(RoaringBitmap::new(), |mut roots, folder_id| {
            roots |= projection.folder_bitmap(folder_id);
            roots
        });
    stage_root_bitmap(transaction, &roots)?;
    populate_staged_media(transaction, projection)
}

pub(crate) fn staged_root_hints(transaction: &Transaction<'_>) -> rusqlite::Result<Vec<i64>> {
    limited_staged_hints(transaction, "picto_selected_root", "item_id")
}

fn apply_folder_to_selection(
    transaction: &Transaction<'_>,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    folder_id: i64,
    present: bool,
) -> rusqlite::Result<RoaringBitmap> {
    let selected = selected_id_bitmap(transaction, "SELECT item_id FROM picto_selected_root")?;
    let existing = projection.folder_bitmap(folder_id);
    Ok(if present {
        &selected - &existing
    } else {
        &selected & &existing
    })
}

pub(crate) fn apply_tags_to_selection(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    tags: &[(String, String)],
    add: bool,
) -> rusqlite::Result<BulkTagProjectionDelta> {
    let selected = selected_id_bitmap(transaction, "SELECT item_id FROM picto_selected_root")?;
    apply_tags_to_roots(transaction, projections, &selected, tags, add)
}

fn apply_tags_to_roots(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    roots: &RoaringBitmap,
    tags: &[(String, String)],
    add: bool,
) -> rusqlite::Result<BulkTagProjectionDelta> {
    let assignments = tags
        .iter()
        .cloned()
        .map(|tag| (tag, roots.clone()))
        .collect::<BTreeMap<_, _>>();
    apply_tag_assignments(transaction, projections, &assignments, add)
}

fn apply_tag_assignments(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    assignments: &BTreeMap<(String, String), RoaringBitmap>,
    add: bool,
) -> rusqlite::Result<BulkTagProjectionDelta> {
    let mut delta = BulkTagProjectionDelta::default();
    for ((namespace, subtag), requested_roots) in assignments {
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
        let existing = projections.direct_tag_bitmap(tag_id);
        let changed_root_ids = if add {
            requested_roots - &existing
        } else {
            requested_roots & &existing
        };
        if changed_root_ids.is_empty() {
            continue;
        }
        delta.canonical_changed = true;
        delta.history_changes.push(SemanticMembershipDelta {
            relation_id: tag_id,
            add: changed_root_ids.clone(),
            remove: RoaringBitmap::new(),
        });
        delta.changes.push((tag_id, changed_root_ids));
    }
    Ok(delta)
}

fn selected_id_bitmap(transaction: &Transaction<'_>, sql: &str) -> rusqlite::Result<RoaringBitmap> {
    let mut statement = transaction.prepare(sql)?;
    let mut rows = statement.query([])?;
    let mut ids = RoaringBitmap::new();
    while let Some(row) = rows.next()? {
        let item_id: i64 = row.get(0)?;
        let item_id = u32::try_from(item_id)
            .map_err(|_| invalid(format!("Item ID {item_id} exceeds projection capacity")))?;
        ids.insert(item_id);
    }
    Ok(ids)
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

    use roaring::RoaringBitmap;
    use rusqlite::params;

    use super::{
        DetachItemsInput, ItemRename, MediaMetadataPatch, OrganizeIntoCollectionInput,
        OrganizeIntoCollectionResult, ReorderCollectionInput,
    };
    use crate::app::{
        Application, ItemFilters, ItemId, ItemQuery, ItemScope, ItemSort, ItemTarget, Lifecycle,
    };
    use crate::canonical_bitmap::{
        load_bitmap, load_order, rating_key, BitmapDomain, LIFECYCLE_ACTIVE_KEY,
        LIFECYCLE_TRASH_KEY,
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
                crate::canonical_bitmap::seed_test_state(transaction)?;
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
                let legacy_collection_folders: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(legacy_collection_folders, 0);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections()
                .folder_ids_for_root(grouped.collection_id.0),
            vec![1]
        );

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
            vec![ids[0], ids[1], grouped.collection_id]
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
                    let legacy_folders: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                        [id.0],
                        |row| row.get(0),
                    )?;
                    assert_eq!(legacy_folders, 0);
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
        for id in &ids[..2] {
            assert_eq!(app.projections().folder_ids_for_root(id.0), vec![1]);
        }
    }

    #[test]
    fn grouping_round_trips_through_application_history() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], Some("Post"), None);
        assert_eq!(
            app.projections()
                .folder_ids_for_root(grouped.collection_id.0),
            vec![1]
        );
        for id in &ids[..2] {
            assert!(app.projections().folder_ids_for_root(id.0).is_empty());
        }

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
        for id in &ids[..2] {
            assert_eq!(app.projections().folder_ids_for_root(id.0), vec![1]);
        }
        assert!(app
            .projections()
            .folder_ids_for_root(grouped.collection_id.0)
            .is_empty());

        app.redo().unwrap();
        app.store()
            .read(|connection| {
                let legacy_members: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM collection_member WHERE collection_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(legacy_members, 0);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections()
                .group_order(grouped.collection_id.0)
                .unwrap(),
            vec![ids[0].0, ids[1].0]
        );
        assert_eq!(
            app.projections()
                .folder_ids_for_root(grouped.collection_id.0),
            vec![1]
        );
        for id in &ids[..2] {
            assert!(app.projections().folder_ids_for_root(id.0).is_empty());
        }
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

                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections()
                .group_order(grouped.collection_id.0)
                .unwrap()
                .len(),
            2
        );

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
                    "SELECT name FROM root_metadata WHERE root_item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(label, "Post");
                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections()
                .group_order(grouped.collection_id.0)
                .unwrap(),
            vec![ids[0].0, ids[1].0, ids[2].0]
        );
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
        let legacy_row_order = || {
            app.store()
                .read(|connection| {
                    connection
                        .prepare(
                            "SELECT media_item_id FROM collection_member
                             WHERE collection_id = ?1 ORDER BY position_rank",
                        )?
                        .query_map([grouped.collection_id.0], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
        };
        let unchanged_legacy_order = legacy_row_order();

        app.reorder_collection(ReorderCollectionInput {
            collection_id: grouped.collection_id,
            media_item_ids: reordered.clone(),
        })
        .unwrap();

        let details = crate::query_v2::details(&app, grouped.collection_id).unwrap();
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
        assert_eq!(legacy_row_order(), unchanged_legacy_order);

        let canonical_order = app
            .store()
            .read(|connection| load_order(connection, "group", grouped.collection_id.0))
            .unwrap()
            .unwrap();
        assert_eq!(
            canonical_order,
            reordered
                .iter()
                .map(|item_id| item_id.0 as u32)
                .collect::<Vec<_>>()
        );

        app.undo().unwrap();
        let original_order = app
            .store()
            .read(|connection| load_order(connection, "group", grouped.collection_id.0))
            .unwrap()
            .unwrap();
        assert_eq!(
            original_order,
            ids.iter()
                .map(|item_id| item_id.0 as u32)
                .collect::<Vec<_>>()
        );
        assert_eq!(legacy_row_order(), unchanged_legacy_order);

        app.redo().unwrap();
        let redone_order = app
            .store()
            .read(|connection| load_order(connection, "group", grouped.collection_id.0))
            .unwrap()
            .unwrap();
        assert_eq!(redone_order, canonical_order);
        assert_eq!(legacy_row_order(), unchanged_legacy_order);
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
                    "SELECT name FROM root_metadata WHERE root_item_id = ?1",
                    [right.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(label, "Right");
                let losing_root: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE item_id = ?1",
                    [left.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(losing_root, 0);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections()
                .group_order(right.collection_id.0)
                .unwrap(),
            vec![ids[2].0, extra.0, ids[0].0, ids[1].0]
        );
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
    fn worker_tags_the_analyzed_members_owning_collection_root() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], None, None);
        let first = app
            .apply_media_tags(ids[0], &["general:predicted".to_string()])
            .unwrap();
        let repeated = app
            .apply_media_tags(ids[0], &["general:predicted".to_string()])
            .unwrap();
        assert_eq!(first.item_ids, vec![grouped.collection_id]);
        assert_eq!(first.revision, repeated.revision);
        let tag_id = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag
                     WHERE namespace = 'general' AND subtag = 'predicted'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(
            app.projections().direct_tag_bitmap(tag_id),
            RoaringBitmap::from_iter([grouped.collection_id.0 as u32])
        );
    }

    #[test]
    fn ai_review_assignments_commit_once_and_report_affected_roots() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], None, None);
        let before = app.store().revision().unwrap();

        let receipt = app
            .apply_media_tag_assignments(&[
                (ids[0], vec!["general:first".to_string()]),
                (ids[1], vec!["general:second".to_string()]),
            ])
            .unwrap();

        assert_eq!(receipt.revision, before + 1);
        assert_eq!(receipt.item_ids, vec![grouped.collection_id]);
        let tag_ids: Vec<i64> = app
            .store()
            .read(|connection| {
                connection
                    .prepare(
                        "SELECT tag_id FROM tag
                         WHERE namespace = 'general' AND subtag IN ('first', 'second')
                         ORDER BY subtag",
                    )?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect()
            })
            .unwrap();
        assert_eq!(tag_ids.len(), 2);
        let expected = RoaringBitmap::from_iter([grouped.collection_id.0 as u32]);
        for tag_id in &tag_ids {
            assert_eq!(app.projections().direct_tag_bitmap(*tag_id), expected);
        }

        app.undo().unwrap();
        for tag_id in tag_ids {
            assert!(app.projections().direct_tag_bitmap(tag_id).is_empty());
        }
    }

    #[test]
    fn collection_tag_write_updates_members_with_one_projection_batch() {
        let (_directory, app, ids) = fixture();
        let grouped = organize(&app, &ids[..2], None, None);
        let target = ItemTarget::Explicit {
            item_ids: vec![grouped.collection_id],
        };
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (99, 'smart:shared', 'Shared', ?1, 'now', 'now')",
                    [serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [{
                                "field": "tags",
                                "op": "include",
                                "values": ["general:shared"]
                            }]
                        }]
                    })
                    .to_string()],
                )?;
                crate::smart_v2::refresh_materialized(transaction)?;
                Ok(())
            })
            .unwrap();
        app.apply_tags(&target, &["general:shared".to_string()], true)
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
                let legacy_row_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM root_tag WHERE tag_id = ?1",
                    [tag_id],
                    |row| row.get(0),
                )?;
                assert_eq!(legacy_row_count, 0);
                let smart_count: i64 = connection.query_row(
                    "SELECT COUNT(*)
                     FROM smart_folder_generation generation
                     JOIN smart_folder_membership membership
                       ON membership.generation_id = generation.generation_id
                     WHERE generation.smart_folder_id = 99
                       AND generation.state = 'active'
                       AND membership.root_item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(smart_count, 1);
                Ok(())
            })
            .unwrap();

        app.apply_tags(&target, &["general:shared".to_string()], false)
            .unwrap();
        assert!(app.projections().direct_tag_bitmap(tag_id).is_empty());
        let smart_count = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*)
                     FROM smart_folder_generation generation
                     JOIN smart_folder_membership membership
                       ON membership.generation_id = generation.generation_id
                     WHERE generation.smart_folder_id = 99
                       AND generation.state = 'active'
                       AND membership.root_item_id = ?1",
                    [grouped.collection_id.0],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(smart_count, 0);
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
                crate::canonical_bitmap::seed_test_state(transaction)?;
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
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Lifecycle, LIFECYCLE_ACTIVE_KEY)?,
                    RoaringBitmap::from_iter([ids[0].0 as u32])
                );
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Lifecycle, LIFECYCLE_TRASH_KEY)?,
                    RoaringBitmap::from_iter([ids[1].0 as u32, ids[2].0 as u32])
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn prepared_publication_persists_only_canonical_bitmap_components() {
        let (_directory, app, ids) = fixture();
        let target = ItemTarget::Explicit {
            item_ids: ids[..2].to_vec(),
        };
        app.apply_tags(&target, &["creator:test".to_string()], true)
            .unwrap();
        app.set_folder_membership(&target, 1, true).unwrap();
        app.patch_metadata(
            &target,
            &MediaMetadataPatch {
                rating: Some(Some(4)),
                notes: None,
                source_urls: None,
            },
        )
        .unwrap();
        let grouped = organize(&app, &ids[..2], Some("Canonical"), None);

        app.store()
            .read(|connection| {
                let tag_id: i64 = connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'creator' AND subtag = 'test'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Tag, tag_id)?,
                    RoaringBitmap::from_iter([grouped.collection_id.0 as u32])
                );
                let tag_summary: (i64, i64) = connection.query_row(
                    "SELECT visible_root_count, assignment_count
                     FROM tag_summary WHERE tag_id = ?1",
                    [tag_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(tag_summary, (1, 1));
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Folder, 1)?,
                    RoaringBitmap::from_iter([grouped.collection_id.0 as u32, ids[2].0 as u32])
                );
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Rating, rating_key(Some(4)))?,
                    RoaringBitmap::from_iter([grouped.collection_id.0 as u32])
                );
                assert_eq!(
                    load_bitmap(
                        connection,
                        BitmapDomain::GroupMember,
                        grouped.collection_id.0
                    )?,
                    RoaringBitmap::from_iter([ids[0].0 as u32, ids[1].0 as u32])
                );
                assert_eq!(
                    load_order(connection, "group", grouped.collection_id.0)?.unwrap(),
                    vec![ids[0].0 as u32, ids[1].0 as u32]
                );
                Ok(())
            })
            .unwrap();

        app.undo().unwrap();
        app.store()
            .read(|connection| {
                let tag_id: i64 = connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'creator' AND subtag = 'test'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Tag, tag_id)?,
                    RoaringBitmap::from_iter([ids[0].0 as u32, ids[1].0 as u32])
                );
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Folder, 1)?,
                    RoaringBitmap::from_iter(ids.iter().map(|item| item.0 as u32))
                );
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Rating, rating_key(Some(4)))?,
                    RoaringBitmap::from_iter([ids[0].0 as u32, ids[1].0 as u32])
                );
                assert!(load_bitmap(
                    connection,
                    BitmapDomain::GroupMember,
                    grouped.collection_id.0
                )?
                .is_empty());
                assert_eq!(
                    load_order(connection, "group", grouped.collection_id.0)?,
                    None
                );
                Ok(())
            })
            .unwrap();

        app.redo().unwrap();
        app.store()
            .read(|connection| {
                let tag_id: i64 = connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'creator' AND subtag = 'test'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Tag, tag_id)?,
                    RoaringBitmap::from_iter([grouped.collection_id.0 as u32])
                );
                assert_eq!(
                    load_bitmap(
                        connection,
                        BitmapDomain::GroupMember,
                        grouped.collection_id.0
                    )?,
                    RoaringBitmap::from_iter([ids[0].0 as u32, ids[1].0 as u32])
                );
                assert_eq!(
                    load_order(connection, "group", grouped.collection_id.0)?.unwrap(),
                    vec![ids[0].0 as u32, ids[1].0 as u32]
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn folder_membership_and_history_are_canonical_bitmaps_only() {
        let (_directory, app, ids) = fixture();
        let folder_id = app
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder(folder_key, name, created_at, updated_at)
                     VALUES ('bitmap-folder', 'Bitmap folder', 'now', 'now')",
                    [],
                )?;
                Ok(transaction.last_insert_rowid())
            })
            .unwrap()
            .0;
        let selected = RoaringBitmap::from_iter(ids[..2].iter().map(|item| item.0 as u32));
        let target = ItemTarget::Explicit {
            item_ids: ids[..2].to_vec(),
        };

        app.set_folder_membership(&target, folder_id, true).unwrap();
        app.store()
            .read(|connection| {
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Folder, folder_id)?,
                    selected
                );
                let legacy_rows: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id = ?1",
                    [folder_id],
                    |row| row.get(0),
                )?;
                assert_eq!(legacy_rows, 0);
                let summary: (i64, i64, i64) = connection.query_row(
                    "SELECT visible_root_count, media_count, total_size_bytes
                     FROM folder_summary WHERE folder_id = ?1",
                    [folder_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(summary, (2, 2, 3));
                Ok(())
            })
            .unwrap();

        app.undo().unwrap();
        app.store()
            .read(|connection| {
                assert!(load_bitmap(connection, BitmapDomain::Folder, folder_id)?.is_empty());
                Ok(())
            })
            .unwrap();
        app.redo().unwrap();
        app.store()
            .read(|connection| {
                assert_eq!(
                    load_bitmap(connection, BitmapDomain::Folder, folder_id)?,
                    selected
                );
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
    fn mixed_metadata_patch_settles_rating_projection_before_returning() {
        let (_directory, app, ids) = fixture();
        let target = ItemTarget::Explicit {
            item_ids: vec![ids[0]],
        };

        app.patch_metadata(
            &target,
            &MediaMetadataPatch {
                rating: Some(Some(4)),
                notes: Some(Some("Reviewed".to_string())),
                source_urls: Some(vec!["https://example.test/source".to_string()]),
            },
        )
        .unwrap();

        let selected = RoaringBitmap::from_iter([ids[0].0 as u32]);
        let aggregate = app.projections().rating_aggregate(&selected);
        assert_eq!((aggregate.count, aggregate.sum), (1, 4));
        app.store()
            .read(|connection| {
                let metadata: (Option<i64>, Option<String>, String) = connection.query_row(
                    "SELECT rating, notes, source_urls_json
                     FROM root_metadata WHERE root_item_id = ?1",
                    [ids[0].0],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(metadata.0, Some(4));
                assert_eq!(metadata.1.as_deref(), Some("Reviewed"));
                assert_eq!(
                    serde_json::from_str::<Vec<String>>(&metadata.2).unwrap(),
                    vec!["https://example.test/source"]
                );
                Ok(())
            })
            .unwrap();

        app.patch_metadata(
            &target,
            &MediaMetadataPatch {
                rating: Some(None),
                notes: Some(Some("Unrated".to_string())),
                source_urls: None,
            },
        )
        .unwrap();

        let aggregate = app.projections().rating_aggregate(&selected);
        assert_eq!((aggregate.count, aggregate.sum), (0, 0));
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
                                "SELECT COALESCE(metadata.name, asset.name, '')
                                 FROM media_asset asset
                                 LEFT JOIN root_metadata metadata
                                   ON metadata.root_item_id = asset.item_id
                                 WHERE asset.item_id = ?1",
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
