//! Replacement folder hierarchy operations.
//!
//! Folders organize library roots only. They never own or delete media, and
//! hierarchy changes are settled through the application transaction boundary.

use std::collections::BTreeSet;

use chrono::Utc;
use rand::RngCore;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::cloud::capture::{record_folder_created, record_folder_delete, record_folder_upsert};
use crate::store::history::HistoryDescriptor;

const RANK_GAP: i64 = 1024;
const APPLICATION_SETTINGS_KEY: &str = "application";
const FOLDER_AUTO_TAGS_KEY: &str = "folderAutoTags";
const FOLDER_COVERS_KEY: &str = "folderCovers";
const MAX_FOLDER_RESOURCE_HINTS: usize = 256;
const MAX_RECEIPT_ITEM_IDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct FolderId(#[ts(type = "number")] pub i64);

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreateFolderInput {
    pub name: String,
    pub parent_id: Option<FolderId>,
    #[serde(default)]
    pub folder_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ReorderFolderChildrenInput {
    pub parent_id: Option<FolderId>,
    pub folder_ids: Vec<FolderId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ReorderFolderItemsInput {
    pub folder_id: FolderId,
    pub item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SortFolderTreeInput {
    pub folder_id: FolderId,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SetFolderAutoTagsInput {
    pub folder_id: FolderId,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SetFolderCoverInput {
    pub folder_id: FolderId,
    pub item_id: ItemId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderCover {
    pub entity_hash: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderWatchInput {
    pub folder_id: FolderId,
    pub path: String,
    #[serde(default)]
    pub include_subfolders: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderMetadataInput {
    pub folder_id: FolderId,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
}

/// Folder IDs are explicit because `MutationReceipt.item_ids` is reserved for
/// media/library roots. This keeps folder invalidation truthful and typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderMutationReceipt {
    pub receipt: MutationReceipt,
    pub folder_ids: Vec<FolderId>,
    pub deleted_folder_ids: Vec<FolderId>,
    pub fallback_folder_id: Option<FolderId>,
}

impl Application {
    pub fn create_folder(
        &self,
        input: &CreateFolderInput,
    ) -> Result<(FolderId, FolderMutationReceipt), String> {
        let name = non_empty("Folder name", &input.name)?;
        let folder_key = match input.folder_key.as_deref() {
            Some(key) => non_empty("Folder key", key)?,
            None => new_folder_key(),
        };
        let parent_id = input.parent_id.map(|id| id.0);
        let now = Utc::now().to_rfc3339();

        let (folder_id, revision, _) = self.undoable_transaction(
            folder_history("folders.create", "Create folder", &[]),
            |transaction| {
                if let Some(parent_id) = parent_id {
                    require_folder(transaction, parent_id)?;
                }
                let sort_rank = next_sibling_rank(transaction, parent_id, None)?;
                transaction.execute(
                    "INSERT INTO folder
                        (folder_key, name, parent_id, sort_rank, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    params![folder_key, name, parent_id, sort_rank, now],
                )?;
                let folder_id = transaction.last_insert_rowid();
                record_folder_created(transaction, &[folder_id])?;
                Ok((folder_id, ()))
            },
            |_, ()| Ok(()),
        )?;

        let folder_id = FolderId(folder_id);
        Ok((
            folder_id,
            folder_receipt(revision, vec![folder_id], Vec::new(), None),
        ))
    }

    pub fn rename_folder(
        &self,
        folder_id: FolderId,
        name: &str,
    ) -> Result<FolderMutationReceipt, String> {
        let name = non_empty("Folder name", name)?;
        let now = Utc::now().to_rfc3339();
        let ((), revision, _) = self.undoable_transaction(
            folder_history("folders.rename", "Rename folder", &[folder_id]),
            |transaction| {
                require_folder(transaction, folder_id.0)?;
                transaction.execute(
                    "UPDATE folder SET name = ?1, updated_at = ?2 WHERE folder_id = ?3",
                    params![name, now, folder_id.0],
                )?;
                record_folder_upsert(transaction, &[folder_id.0], &["name"])?;
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;

        Ok(folder_receipt(revision, vec![folder_id], Vec::new(), None))
    }

    /// Clone a folder hierarchy without copying media memberships or watches.
    pub fn duplicate_folder(
        &self,
        folder_id: FolderId,
    ) -> Result<(FolderId, FolderMutationReceipt), String> {
        let now = Utc::now().to_rfc3339();
        let ((duplicate_id, created_ids), revision, _) = self.undoable_transaction(
            folder_history("folders.duplicate", "Duplicate folder", &[folder_id]),
            |transaction| {
                require_folder(transaction, folder_id.0)?;
                let (name, parent_id): (String, Option<i64>) = transaction.query_row(
                    "SELECT name, parent_id FROM folder WHERE folder_id = ?1",
                    [folder_id.0],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                let (duplicate_id, created_ids, cloned_pairs) = clone_folder_tree_setwise(
                    transaction,
                    folder_id.0,
                    parent_id,
                    &format!("{name} copy"),
                    &now,
                )?;

                let mut settings = read_application_settings(transaction)?;
                let mut copied_auto_tags = false;
                for (source_id, clone_id) in cloned_pairs {
                    let tags = exact_folder_auto_tag_names(&settings, source_id);
                    if !tags.is_empty() {
                        write_exact_folder_auto_tags(&mut settings, clone_id, &tags)?;
                        copied_auto_tags = true;
                    }
                }
                if copied_auto_tags {
                    write_application_settings(transaction, &settings)?;
                }

                let mut siblings = child_folder_ids(transaction, parent_id)?;
                siblings.retain(|candidate| *candidate != duplicate_id);
                let source_index = siblings
                    .iter()
                    .position(|candidate| *candidate == folder_id.0)
                    .ok_or_else(|| invalid("Source folder is missing from its parent"))?;
                siblings.insert(source_index + 1, duplicate_id);
                stage_ordered_ids(transaction, "picto_folder_order", &siblings)?;
                transaction.execute(
                    "UPDATE folder
                     SET sort_rank = (
                             SELECT staged.ordinal * ?1
                             FROM picto_folder_order staged
                             WHERE staged.item_id = folder.folder_id
                         ),
                         updated_at = ?2
                     WHERE folder_id IN (SELECT item_id FROM picto_folder_order)",
                    params![RANK_GAP, now],
                )?;
                record_folder_created(transaction, &created_ids)?;
                record_folder_upsert(transaction, &siblings, &["sort_rank"])?;
                Ok(((duplicate_id, created_ids.clone()), created_ids))
            },
            |_, _| Ok(()),
        )?;
        let created_ids = created_ids.into_iter().map(FolderId).collect::<Vec<_>>();
        Ok((
            FolderId(duplicate_id),
            folder_receipt(revision, created_ids, Vec::new(), None),
        ))
    }

    pub fn set_folder_metadata(
        &self,
        input: &FolderMetadataInput,
    ) -> Result<FolderMutationReceipt, String> {
        let icon = normalized_optional(input.icon.as_deref());
        let color = normalized_optional(input.color.as_deref());
        let notes = normalized_optional(input.notes.as_deref());
        let now = Utc::now().to_rfc3339();
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            folder_history("folders.set_metadata", "Edit folder", &[input.folder_id]),
            |transaction| {
                require_folder(transaction, input.folder_id.0)?;
                let previous: (Option<String>, Option<String>, Option<String>) = transaction
                    .query_row(
                        "SELECT icon, color, notes FROM folder WHERE folder_id = ?1",
                        [input.folder_id.0],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                let next = (icon.clone(), color.clone(), notes.clone());
                let changed = previous != next;
                if changed {
                    let changed_fields = [
                        ("icon", previous.0 != next.0),
                        ("color", previous.1 != next.1),
                        ("notes", previous.2 != next.2),
                    ]
                    .into_iter()
                    .filter_map(|(field, changed)| changed.then_some(field))
                    .collect::<Vec<_>>();
                    transaction.execute(
                        "UPDATE folder
                         SET icon = ?1, color = ?2, notes = ?3, updated_at = ?4
                         WHERE folder_id = ?5",
                        params![icon, color, notes, now, input.folder_id.0],
                    )?;
                    record_folder_upsert(transaction, &[input.folder_id.0], &changed_fields)?;
                }
                Ok(((), (), changed))
            },
            |_, ()| Ok(()),
        )?;
        Ok(folder_receipt(
            revision,
            vec![input.folder_id],
            Vec::new(),
            None,
        ))
    }

    pub fn folder_auto_tags(&self, folder_id: FolderId) -> Result<Vec<String>, String> {
        self.store().read(|connection| {
            connection
                .query_row(
                    "SELECT 1 FROM folder WHERE folder_id = ?1",
                    [folder_id.0],
                    |_| Ok(()),
                )
                .optional()?
                .ok_or_else(|| invalid(format!("Folder {} does not exist", folder_id.0)))?;
            let settings = read_application_settings(connection)?;
            Ok(exact_folder_auto_tag_names(&settings, folder_id.0))
        })
    }

    pub fn set_folder_auto_tags(
        &self,
        input: &SetFolderAutoTagsInput,
    ) -> Result<FolderMutationReceipt, String> {
        let mut normalized = input
            .tags
            .iter()
            .map(|tag| crate::tag_name_v2::parse_local(tag))
            .collect::<Result<Vec<_>, _>>()?;
        normalized.sort();
        normalized.dedup();
        let names = normalized
            .iter()
            .map(|(namespace, subtag)| canonical_tag_name(namespace, subtag))
            .collect::<Vec<_>>();
        let history = HistoryDescriptor::new(
            "folders.set_auto_tags",
            "Set folder auto tags",
            vec![
                resources::FOLDERS.to_string(),
                resources::SIDEBAR.to_string(),
                resources::LIBRARY.to_string(),
                resources::TAGS.to_string(),
                resources::SMART_FOLDERS.to_string(),
                resources::SETTINGS.to_string(),
                format!("folder:{}", input.folder_id.0),
            ],
            Vec::new(),
        );

        let (root_ids, revision, _, _) = self.undoable_transaction_if_changed(
            history,
            |transaction| {
                require_folder(transaction, input.folder_id.0)?;
                let mut settings = read_application_settings(transaction)?;
                let previous = exact_folder_auto_tag_names(&settings, input.folder_id.0);
                let previous_set = previous.iter().cloned().collect::<BTreeSet<_>>();
                let added = normalized
                    .iter()
                    .filter(|(namespace, subtag)| {
                        !previous_set.contains(&canonical_tag_name(namespace, subtag))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                write_exact_folder_auto_tags(&mut settings, input.folder_id.0, &names)?;
                write_application_settings(transaction, &settings)?;

                crate::operations_v2::stage_folder_subtree_selection(
                    transaction,
                    &self.projections().selection_snapshot(),
                    input.folder_id.0,
                )?;
                let root_ids = crate::operations_v2::staged_root_hints(transaction)?;
                let changed_tags = if added.is_empty() {
                    crate::operations_v2::BulkTagProjectionDelta::default()
                } else {
                    crate::operations_v2::apply_tags_to_selection(
                        transaction,
                        self.projections(),
                        &added,
                        true,
                    )?
                };
                let changed = previous != names || changed_tags.canonical_changed;
                Ok((root_ids, changed_tags, changed))
            },
            |projections, changed_tags| {
                for (tag_id, root_ids) in changed_tags.changes {
                    projections.apply_root_tag_bitmap(tag_id, &root_ids, true)?;
                }
                Ok(())
            },
        )?;

        let mut receipt = folder_receipt(revision, vec![input.folder_id], Vec::new(), None);
        receipt.receipt.resources.extend([
            resources::TAGS.to_string(),
            resources::SMART_FOLDERS.to_string(),
            resources::SETTINGS.to_string(),
        ]);
        set_receipt_item_ids(
            &mut receipt.receipt,
            root_ids.into_iter().map(ItemId).collect(),
        );
        Ok(receipt)
    }

    pub fn move_folder(
        &self,
        folder_id: FolderId,
        parent_id: Option<FolderId>,
    ) -> Result<FolderMutationReceipt, String> {
        let parent_id = parent_id.map(|id| id.0);
        let now = Utc::now().to_rfc3339();
        let ((), revision, _) = self.undoable_transaction(
            folder_history("folders.move", "Move folder", &[folder_id]),
            |transaction| {
                require_folder(transaction, folder_id.0)?;
                if let Some(parent_id) = parent_id {
                    require_folder(transaction, parent_id)?;
                    if parent_id == folder_id.0
                        || is_descendant(transaction, folder_id.0, parent_id)?
                    {
                        return Err(invalid(
                            "Cannot move a folder below itself or its descendant",
                        ));
                    }
                }

                let sort_rank = next_sibling_rank(transaction, parent_id, Some(folder_id.0))?;
                transaction.execute(
                    "UPDATE folder
                 SET parent_id = ?1, sort_rank = ?2, updated_at = ?3
                 WHERE folder_id = ?4",
                    params![parent_id, sort_rank, now, folder_id.0],
                )?;
                record_folder_upsert(transaction, &[folder_id.0], &["parent", "sort_rank"])?;
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;

        Ok(folder_receipt(revision, vec![folder_id], Vec::new(), None))
    }

    pub fn reorder_folder_children(
        &self,
        input: &ReorderFolderChildrenInput,
    ) -> Result<FolderMutationReceipt, String> {
        let folder_ids = unique_folder_ids(&input.folder_ids)?;
        let parent_id = input.parent_id.map(|id| id.0);
        let now = Utc::now().to_rfc3339();

        let ((), revision, _) = self.undoable_transaction(
            folder_history("folders.reorder", "Reorder folders", &folder_ids),
            |transaction| {
                if let Some(parent_id) = parent_id {
                    require_folder(transaction, parent_id)?;
                }
                let expected = child_folder_ids(transaction, parent_id)?;
                let requested = folder_ids.iter().map(|id| id.0).collect::<BTreeSet<_>>();
                if expected.len() != requested.len()
                    || expected.into_iter().collect::<BTreeSet<_>>() != requested
                {
                    return Err(invalid(
                        "Folder reorder must contain every sibling exactly once",
                    ));
                }
                let ids = folder_ids
                    .iter()
                    .map(|folder_id| folder_id.0)
                    .collect::<Vec<_>>();
                stage_ordered_ids(transaction, "picto_folder_order", &ids)?;
                transaction.execute(
                    "UPDATE folder
                     SET sort_rank = (
                             SELECT staged.ordinal * ?1
                             FROM picto_folder_order staged
                             WHERE staged.item_id = folder.folder_id
                         ),
                         updated_at = ?2
                     WHERE folder_id IN (SELECT item_id FROM picto_folder_order)",
                    params![RANK_GAP, now],
                )?;
                record_folder_upsert(transaction, &ids, &["sort_rank"])?;
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;

        Ok(folder_receipt(revision, folder_ids, Vec::new(), None))
    }

    pub fn sort_folder_tree(
        &self,
        input: &SortFolderTreeInput,
    ) -> Result<FolderMutationReceipt, String> {
        let now = Utc::now().to_rfc3339();
        let (changed_ids, revision, _) = self.undoable_transaction(
            folder_history("folders.sort_tree", "Sort folders", &[input.folder_id]),
            |transaction| {
                require_folder(transaction, input.folder_id.0)?;
                stage_sorted_folder_tree(
                    transaction,
                    input.folder_id.0,
                    input.recursive,
                    input.descending,
                )?;
                transaction.execute(
                    "UPDATE folder
                     SET sort_rank = (
                             SELECT staged.sort_rank
                             FROM picto_sorted_folder staged
                             WHERE staged.folder_id = folder.folder_id
                         ),
                         updated_at = ?1
                     WHERE folder_id IN (SELECT folder_id FROM picto_sorted_folder)",
                    [now.as_str()],
                )?;
                let changed_ids = transaction
                    .prepare(
                        "SELECT folder_id FROM picto_sorted_folder
                         ORDER BY folder_id",
                    )?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                record_folder_upsert(transaction, &changed_ids, &["sort_rank"])?;
                Ok((changed_ids.clone(), changed_ids))
            },
            |_, _| Ok(()),
        )?;
        Ok(folder_receipt(
            revision,
            changed_ids.into_iter().map(FolderId).collect(),
            Vec::new(),
            None,
        ))
    }

    pub fn reorder_folder_items(
        &self,
        input: &ReorderFolderItemsInput,
    ) -> Result<FolderMutationReceipt, String> {
        let requested = input
            .item_ids
            .iter()
            .map(|item_id| item_id.0)
            .collect::<BTreeSet<_>>();
        if requested.len() != input.item_ids.len() {
            return Err("Folder item reorder must contain unique item IDs".to_string());
        }

        let ((), revision, _) = self.undoable_transaction(
            folder_history(
                "folders.reorder_items",
                "Reorder folder items",
                &[input.folder_id],
            ),
            |transaction| {
                require_folder(transaction, input.folder_id.0)?;
                let visible = transaction
                    .prepare(
                        "SELECT fi.item_id
                         FROM folder_item fi
                         JOIN library_root lr ON lr.item_id = fi.item_id
                         WHERE fi.folder_id = ?1 AND lr.lifecycle = 'active'
                         ORDER BY fi.position_rank, fi.item_id",
                    )?
                    .query_map([input.folder_id.0], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<BTreeSet<_>>>()?;
                if visible != requested {
                    return Err(invalid(
                        "Folder item reorder must contain every active folder item exactly once",
                    ));
                }
                let item_ids = input.item_ids.iter().map(|item| item.0).collect::<Vec<_>>();
                stage_ordered_ids(transaction, "picto_folder_item_order", &item_ids)?;
                transaction.execute(
                    "UPDATE folder_item
                     SET position_rank = (
                         SELECT staged.ordinal * ?1
                         FROM picto_folder_item_order staged
                         WHERE staged.item_id = folder_item.item_id
                     )
                     WHERE folder_id = ?2
                       AND item_id IN (SELECT item_id FROM picto_folder_item_order)",
                    params![RANK_GAP, input.folder_id.0],
                )?;
                transaction.execute(
                    "WITH hidden AS (
                         SELECT fi.item_id,
                                ROW_NUMBER() OVER (ORDER BY fi.position_rank, fi.item_id) AS offset
                         FROM folder_item fi
                         JOIN library_root lr ON lr.item_id = fi.item_id
                         WHERE fi.folder_id = ?1 AND lr.lifecycle != 'active'
                     )
                     UPDATE folder_item
                     SET position_rank = (?2 + (
                         SELECT offset FROM hidden WHERE hidden.item_id = folder_item.item_id
                     )) * ?3
                     WHERE folder_id = ?1
                       AND item_id IN (SELECT item_id FROM hidden)",
                    params![input.folder_id.0, input.item_ids.len() as i64, RANK_GAP],
                )?;
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;
        let mut receipt = folder_receipt(revision, vec![input.folder_id], Vec::new(), None);
        set_receipt_item_ids(&mut receipt.receipt, input.item_ids.clone());
        Ok(receipt)
    }

    pub fn sort_folder_items_by_name(
        &self,
        folder_id: FolderId,
    ) -> Result<FolderMutationReceipt, String> {
        let (item_ids, revision, _) = self.undoable_transaction(
            folder_history("folders.sort_items", "Sort folder items", &[folder_id]),
            |transaction| {
                require_folder(transaction, folder_id.0)?;
                transaction.execute(
                    "WITH ranked AS (
                         SELECT fi.item_id,
                                ROW_NUMBER() OVER (
                                    ORDER BY lower(COALESCE(summary.sort_name, '')), fi.item_id
                                ) AS ordinal
                         FROM folder_item fi
                         JOIN library_root lr ON lr.item_id = fi.item_id
                         JOIN root_summary summary ON summary.root_item_id = fi.item_id
                         WHERE fi.folder_id = ?1 AND lr.lifecycle = 'active'
                     )
                     UPDATE folder_item
                     SET position_rank = (
                         SELECT ranked.ordinal * ?2 FROM ranked
                         WHERE ranked.item_id = folder_item.item_id
                     )
                     WHERE folder_id = ?1 AND item_id IN (SELECT item_id FROM ranked)",
                    params![folder_id.0, RANK_GAP],
                )?;
                let item_ids = transaction
                    .prepare(
                        "SELECT fi.item_id
                         FROM folder_item fi
                         JOIN library_root lr ON lr.item_id = fi.item_id
                         WHERE fi.folder_id = ?1 AND lr.lifecycle = 'active'
                         ORDER BY fi.position_rank, fi.item_id",
                    )?
                    .query_map([folder_id.0], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((item_ids.clone(), item_ids))
            },
            |_, _| Ok(()),
        )?;
        let mut receipt = folder_receipt(revision, vec![folder_id], Vec::new(), None);
        set_receipt_item_ids(
            &mut receipt.receipt,
            item_ids.into_iter().map(ItemId).collect(),
        );
        Ok(receipt)
    }

    pub fn delete_folder(&self, folder_id: FolderId) -> Result<FolderMutationReceipt, String> {
        self.delete_folders(&[folder_id])
    }

    pub fn delete_folders(&self, folder_ids: &[FolderId]) -> Result<FolderMutationReceipt, String> {
        let folder_ids = folder_ids.iter().copied().collect::<BTreeSet<_>>();
        if folder_ids.is_empty() {
            return Err("At least one folder is required".to_string());
        }
        let history_folder_ids = folder_ids.iter().copied().collect::<Vec<_>>();
        let ((deleted_folder_ids, fallback_folder_id), revision, _) = self.undoable_transaction(
            folder_history("folders.delete", "Delete folders", &history_folder_ids),
            |transaction| {
                let selected = folder_ids.iter().map(|folder| folder.0).collect::<Vec<_>>();
                stage_folder_deletion(transaction, &selected)?;
                let existing: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM folder
                     WHERE folder_id IN (SELECT folder_id FROM picto_selected_folder)",
                    [],
                    |row| row.get(0),
                )?;
                if existing != selected.len() as i64 {
                    return Err(invalid("One or more selected folders do not exist"));
                }
                let delete_root_count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM picto_delete_folder_root",
                    [],
                    |row| row.get(0),
                )?;
                let fallback_folder_id = if delete_root_count == 1 {
                    transaction.query_row(
                        "SELECT parent_id FROM folder
                         WHERE folder_id = (SELECT folder_id FROM picto_delete_folder_root)",
                        [],
                        |row| row.get::<_, Option<i64>>(0),
                    )?
                } else {
                    None
                };
                let deleted_folder_keys = transaction
                    .prepare(
                        "SELECT folder_key FROM folder
                         WHERE folder_id IN (SELECT folder_id FROM picto_deleted_folder)
                         ORDER BY folder_id",
                    )?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let deleted_folder_ids = transaction
                    .prepare("SELECT folder_id FROM picto_deleted_folder ORDER BY folder_id")?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                transaction.execute(
                    "DELETE FROM folder
                     WHERE folder_id IN (SELECT folder_id FROM picto_delete_folder_root)",
                    [],
                )?;
                record_folder_delete(transaction, deleted_folder_keys)?;
                Ok((
                    (deleted_folder_ids.clone(), fallback_folder_id),
                    deleted_folder_ids,
                ))
            },
            |projections, deleted_folder_ids| projections.remove_folders(&deleted_folder_ids),
        )?;

        let deleted_folder_ids = deleted_folder_ids
            .into_iter()
            .map(FolderId)
            .collect::<Vec<_>>();
        Ok(folder_receipt(
            revision,
            deleted_folder_ids.clone(),
            deleted_folder_ids,
            fallback_folder_id.map(FolderId),
        ))
    }

    pub fn set_folder_watch(
        &self,
        input: &FolderWatchInput,
    ) -> Result<FolderMutationReceipt, String> {
        let path = std::fs::canonicalize(input.path.trim())
            .map_err(|error| format!("Failed to resolve watched folder: {error}"))?;
        if !path.is_dir() {
            return Err(format!(
                "Watched path is not a directory: {}",
                path.display()
            ));
        }
        let library_root = std::fs::canonicalize(self.store().library_root())
            .unwrap_or_else(|_| self.store().library_root().to_path_buf());
        if path.starts_with(&library_root) {
            return Err("A watched folder cannot be inside the Picto library".to_string());
        }
        let path = path.to_string_lossy().into_owned();
        let now = Utc::now().to_rfc3339();
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            folder_history(
                "folders.set_watch",
                "Set watched folder",
                &[input.folder_id],
            ),
            |transaction| {
                require_folder(transaction, input.folder_id.0)?;
                let previous: (Option<String>, bool, bool) = transaction.query_row(
                    "SELECT watch_path, watch_enabled, watch_subfolders
                     FROM folder WHERE folder_id = ?1",
                    [input.folder_id.0],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                let changed = previous != (Some(path.clone()), true, input.include_subfolders);
                if changed {
                    transaction.execute(
                        "UPDATE folder
                         SET watch_path = ?1, watch_enabled = 1,
                             watch_subfolders = ?2, updated_at = ?3
                         WHERE folder_id = ?4",
                        params![path, input.include_subfolders, now, input.folder_id.0],
                    )?;
                }
                Ok(((), (), changed))
            },
            |_, ()| Ok(()),
        )?;
        Ok(folder_receipt(
            revision,
            vec![input.folder_id],
            Vec::new(),
            None,
        ))
    }

    pub fn clear_folder_watch(&self, folder_id: FolderId) -> Result<FolderMutationReceipt, String> {
        let now = Utc::now().to_rfc3339();
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            folder_history("folders.clear_watch", "Clear watched folder", &[folder_id]),
            |transaction| {
                require_folder(transaction, folder_id.0)?;
                let changed = transaction.execute(
                    "UPDATE folder
                     SET watch_path = NULL, watch_enabled = 0,
                         watch_subfolders = 0, updated_at = ?1
                     WHERE folder_id = ?2
                       AND (watch_path IS NOT NULL OR watch_enabled != 0 OR watch_subfolders != 0)",
                    params![now, folder_id.0],
                )? != 0;
                Ok(((), (), changed))
            },
            |_, ()| Ok(()),
        )?;
        Ok(folder_receipt(revision, vec![folder_id], Vec::new(), None))
    }

    pub fn folder_cover(&self, folder_id: FolderId) -> Result<Option<FolderCover>, String> {
        let folder_members = self
            .projections()
            .selection_snapshot()
            .folder_bitmap(folder_id.0);
        self.store().read(|connection| {
            require_folder(connection, folder_id.0)?;
            let settings = read_application_settings(connection)?;
            let Some(item_id) = exact_folder_cover_item_id(&settings, folder_id.0) else {
                return Ok(None);
            };
            if u32::try_from(item_id)
                .ok()
                .is_none_or(|item_id| !folder_members.contains(item_id))
            {
                return Ok(None);
            }
            resolve_folder_cover(connection, item_id).map_err(Into::into)
        })
    }

    pub fn set_folder_cover(
        &self,
        input: &SetFolderCoverInput,
    ) -> Result<FolderMutationReceipt, String> {
        let belongs_to_folder = u32::try_from(input.item_id.0).ok().is_some_and(|item_id| {
            self.projections()
                .selection_snapshot()
                .folder_bitmap(input.folder_id.0)
                .contains(item_id)
        });
        if !belongs_to_folder {
            return Err("Folder cover item must belong to the folder".to_string());
        }
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            folder_history("folders.set_cover", "Set folder cover", &[input.folder_id]),
            |transaction| {
                require_folder(transaction, input.folder_id.0)?;
                resolve_folder_cover(transaction, input.item_id.0)?
                    .ok_or_else(|| invalid("Folder cover item is unavailable"))?;
                let mut settings = read_application_settings(transaction)?;
                let previous = exact_folder_cover_item_id(&settings, input.folder_id.0);
                if previous == Some(input.item_id.0) {
                    return Ok(((), (), false));
                }
                write_exact_folder_cover_item_id(
                    &mut settings,
                    input.folder_id.0,
                    input.item_id.0,
                )?;
                write_application_settings(transaction, &settings)?;
                Ok(((), (), true))
            },
            |_, ()| Ok(()),
        )?;
        let mut receipt = folder_receipt(revision, vec![input.folder_id], Vec::new(), None);
        set_receipt_item_ids(&mut receipt.receipt, vec![input.item_id]);
        Ok(receipt)
    }
}

fn set_receipt_item_ids(receipt: &mut MutationReceipt, item_ids: Vec<ItemId>) {
    receipt.item_ids = if item_ids.len() <= MAX_RECEIPT_ITEM_IDS {
        item_ids
    } else {
        Vec::new()
    };
}

fn folder_history(command: &str, label: &str, folder_ids: &[FolderId]) -> HistoryDescriptor {
    let mut resources = vec![
        resources::FOLDERS.to_string(),
        resources::SIDEBAR.to_string(),
        resources::LIBRARY.to_string(),
    ];
    if folder_ids.len() <= MAX_FOLDER_RESOURCE_HINTS {
        resources.extend(
            folder_ids
                .iter()
                .map(|folder_id| format!("folder:{}", folder_id.0)),
        );
    }
    HistoryDescriptor::new(command, label, resources, Vec::new())
}

fn folder_receipt(
    revision: u64,
    folder_ids: Vec<FolderId>,
    deleted_folder_ids: Vec<FolderId>,
    fallback_folder_id: Option<FolderId>,
) -> FolderMutationReceipt {
    let mut resources = vec![
        resources::FOLDERS.to_string(),
        resources::SIDEBAR.to_string(),
        resources::LIBRARY.to_string(),
    ];
    if folder_ids.len() <= MAX_FOLDER_RESOURCE_HINTS {
        resources.extend(
            folder_ids
                .iter()
                .map(|folder_id| format!("folder:{}", folder_id.0)),
        );
    }
    FolderMutationReceipt {
        receipt: MutationReceipt {
            revision,
            resources,
            item_ids: Vec::new(),
        },
        folder_ids,
        deleted_folder_ids,
        fallback_folder_id,
    }
}

fn canonical_tag_name(namespace: &str, subtag: &str) -> String {
    if namespace == "general" {
        subtag.to_string()
    } else {
        crate::tag_name_v2::format(namespace, subtag)
    }
}

fn read_application_settings(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<serde_json::Value> {
    connection
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [APPLICATION_SETTINGS_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|encoded| {
            serde_json::from_str(&encoded).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
        .map(|value| value.unwrap_or_else(|| serde_json::json!({})))
}

fn write_application_settings(
    transaction: &Transaction<'_>,
    settings: &serde_json::Value,
) -> rusqlite::Result<()> {
    let encoded = serde_json::to_string(settings)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        params![APPLICATION_SETTINGS_KEY, encoded],
    )?;
    Ok(())
}

fn exact_folder_auto_tag_names(settings: &serde_json::Value, folder_id: i64) -> Vec<String> {
    settings
        .get(FOLDER_AUTO_TAGS_KEY)
        .and_then(serde_json::Value::as_object)
        .and_then(|folders| folders.get(&folder_id.to_string()))
        .and_then(serde_json::Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn write_exact_folder_auto_tags(
    settings: &mut serde_json::Value,
    folder_id: i64,
    tags: &[String],
) -> rusqlite::Result<()> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| invalid("Application settings must be an object"))?;
    let folders = root
        .entry(FOLDER_AUTO_TAGS_KEY)
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("Folder auto tags setting must be an object"))?;
    if tags.is_empty() {
        folders.remove(&folder_id.to_string());
    } else {
        folders.insert(folder_id.to_string(), serde_json::json!(tags));
    }
    Ok(())
}

fn exact_folder_cover_item_id(settings: &serde_json::Value, folder_id: i64) -> Option<i64> {
    settings
        .get(FOLDER_COVERS_KEY)
        .and_then(serde_json::Value::as_object)
        .and_then(|folders| folders.get(&folder_id.to_string()))
        .and_then(serde_json::Value::as_i64)
}

fn write_exact_folder_cover_item_id(
    settings: &mut serde_json::Value,
    folder_id: i64,
    item_id: i64,
) -> rusqlite::Result<()> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| invalid("Application settings must be an object"))?;
    let folders = root
        .entry(FOLDER_COVERS_KEY)
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("Folder covers setting must be an object"))?;
    folders.insert(folder_id.to_string(), serde_json::json!(item_id));
    Ok(())
}

fn resolve_folder_cover(
    connection: &rusqlite::Connection,
    item_id: i64,
) -> rusqlite::Result<Option<FolderCover>> {
    connection
        .query_row(
            "SELECT mf.file_hash, mf.mime_type
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             JOIN media_asset ma ON ma.item_id = CASE
                 WHEN li.kind = 'collection' THEN (
                     SELECT cm.media_item_id
                     FROM collection_member cm
                     WHERE cm.collection_id = li.item_id
                     ORDER BY cm.position_rank ASC, cm.media_item_id ASC
                     LIMIT 1
                 )
                 ELSE li.item_id
             END
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE lr.lifecycle = 'active' AND lr.item_id = ?1",
            [item_id],
            |row| {
                Ok(FolderCover {
                    entity_hash: row.get(0)?,
                    mime_type: row.get(1)?,
                })
            },
        )
        .optional()
}

pub(crate) fn inherited_folder_auto_tags(
    transaction: &Transaction<'_>,
    folder_id: i64,
) -> rusqlite::Result<Vec<(String, String)>> {
    let settings = read_application_settings(transaction)?;
    let folder_ids = transaction
        .prepare(
            "WITH RECURSIVE ancestors(folder_id, parent_id) AS (
                 SELECT folder_id, parent_id FROM folder WHERE folder_id = ?1
                 UNION ALL
                 SELECT parent.folder_id, parent.parent_id
                 FROM folder parent
                 JOIN ancestors child ON child.parent_id = parent.folder_id
             )
             SELECT folder_id FROM ancestors",
        )?
        .query_map([folder_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut tags = BTreeSet::new();
    for folder_id in folder_ids {
        for tag in exact_folder_auto_tag_names(&settings, folder_id) {
            tags.insert(crate::tag_name_v2::parse_local(&tag).map_err(invalid)?);
        }
    }
    Ok(tags.into_iter().collect())
}

fn require_folder(connection: &rusqlite::Connection, folder_id: i64) -> rusqlite::Result<()> {
    connection
        .query_row(
            "SELECT 1 FROM folder WHERE folder_id = ?1",
            [folder_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| invalid(format!("Folder {folder_id} does not exist")))
}

fn is_descendant(
    transaction: &Transaction<'_>,
    ancestor_id: i64,
    candidate_id: i64,
) -> rusqlite::Result<bool> {
    transaction.query_row(
        "WITH RECURSIVE descendants(folder_id) AS (
             SELECT folder_id FROM folder WHERE folder_id = ?1
             UNION ALL
             SELECT child.folder_id
             FROM folder child
             JOIN descendants parent ON child.parent_id = parent.folder_id
         )
         SELECT EXISTS(
             SELECT 1 FROM descendants WHERE folder_id = ?2
         )",
        params![ancestor_id, candidate_id],
        |row| row.get(0),
    )
}

fn next_sibling_rank(
    transaction: &Transaction<'_>,
    parent_id: Option<i64>,
    excluding_folder_id: Option<i64>,
) -> rusqlite::Result<i64> {
    let rank = match (parent_id, excluding_folder_id) {
        (Some(parent_id), Some(excluding_folder_id)) => transaction.query_row(
            "SELECT COALESCE(MAX(sort_rank), 0)
             FROM folder WHERE parent_id = ?1 AND folder_id <> ?2",
            params![parent_id, excluding_folder_id],
            |row| row.get::<_, i64>(0),
        )?,
        (Some(parent_id), None) => transaction.query_row(
            "SELECT COALESCE(MAX(sort_rank), 0) FROM folder WHERE parent_id = ?1",
            [parent_id],
            |row| row.get::<_, i64>(0),
        )?,
        (None, Some(excluding_folder_id)) => transaction.query_row(
            "SELECT COALESCE(MAX(sort_rank), 0)
             FROM folder WHERE parent_id IS NULL AND folder_id <> ?1",
            [excluding_folder_id],
            |row| row.get::<_, i64>(0),
        )?,
        (None, None) => transaction.query_row(
            "SELECT COALESCE(MAX(sort_rank), 0) FROM folder WHERE parent_id IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )?,
    };
    Ok(rank.saturating_add(RANK_GAP))
}

fn child_folder_ids(
    transaction: &Transaction<'_>,
    parent_id: Option<i64>,
) -> rusqlite::Result<Vec<i64>> {
    transaction
        .prepare(
            "SELECT folder_id FROM folder
             WHERE (?1 IS NULL AND parent_id IS NULL) OR parent_id = ?1
             ORDER BY sort_rank, folder_id",
        )?
        .query_map([parent_id], |row| row.get(0))?
        .collect()
}

fn stage_ordered_ids(
    transaction: &Transaction<'_>,
    table: &str,
    item_ids: &[i64],
) -> rusqlite::Result<()> {
    let table = match table {
        "picto_folder_order" => "picto_folder_order",
        "picto_folder_item_order" => "picto_folder_item_order",
        _ => return Err(invalid("Unsupported folder order staging table")),
    };
    transaction.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table} (
             item_id INTEGER PRIMARY KEY,
             ordinal INTEGER NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM {table};"
    ))?;
    let encoded = serde_json::to_string(item_ids)
        .map_err(|error| invalid(format!("Could not encode folder order: {error}")))?;
    transaction.execute(
        &format!(
            "INSERT INTO {table}(item_id, ordinal)
             SELECT CAST(value AS INTEGER), CAST(key AS INTEGER) + 1
             FROM json_each(?1)"
        ),
        [encoded],
    )?;
    Ok(())
}

fn stage_sorted_folder_tree(
    transaction: &Transaction<'_>,
    folder_id: i64,
    recursive: bool,
    descending: bool,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_sorted_folder (
             folder_id INTEGER PRIMARY KEY,
             sort_rank INTEGER NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM picto_sorted_folder;",
    )?;
    let direction = if descending { "DESC" } else { "ASC" };
    let target_parents = if recursive {
        "WITH RECURSIVE descendants(folder_id) AS (
             SELECT ?1
             UNION ALL
             SELECT child.folder_id
             FROM folder child
             JOIN descendants parent ON child.parent_id = parent.folder_id
         ), target_parents(parent_key) AS (
             SELECT COALESCE(parent_id, -1) FROM folder WHERE folder_id = ?1
             UNION
             SELECT folder_id FROM descendants
         )"
    } else {
        "WITH target_parents(parent_key) AS (
             SELECT COALESCE(parent_id, -1) FROM folder WHERE folder_id = ?1
         )"
    };
    let sql = format!(
        "{target_parents}, ranked AS (
             SELECT child.folder_id,
                    ROW_NUMBER() OVER (
                        PARTITION BY COALESCE(child.parent_id, -1)
                        ORDER BY lower(child.name) {direction}, child.folder_id {direction}
                    ) AS ordinal
             FROM folder child
             JOIN target_parents target
               ON target.parent_key = COALESCE(child.parent_id, -1)
         )
         INSERT INTO picto_sorted_folder(folder_id, sort_rank)
         SELECT folder_id, ordinal * ?2 FROM ranked"
    );
    transaction.execute(&sql, params![folder_id, RANK_GAP])?;
    Ok(())
}

fn stage_folder_deletion(
    transaction: &Transaction<'_>,
    folder_ids: &[i64],
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_selected_folder (
             folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_delete_folder_root (
             folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_deleted_folder (
             folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_selected_folder;
         DELETE FROM picto_delete_folder_root;
         DELETE FROM picto_deleted_folder;",
    )?;
    let encoded = serde_json::to_string(folder_ids)
        .map_err(|error| invalid(format!("Could not encode selected folders: {error}")))?;
    transaction.execute(
        "INSERT INTO picto_selected_folder(folder_id)
         SELECT CAST(value AS INTEGER) FROM json_each(?1)",
        [encoded],
    )?;
    transaction.execute_batch(
        "WITH RECURSIVE selected_tree(root_id, folder_id) AS (
             SELECT folder.folder_id, folder.folder_id
             FROM folder
             JOIN picto_selected_folder selected
               ON selected.folder_id = folder.folder_id
             UNION ALL
             SELECT tree.root_id, child.folder_id
             FROM folder child
             JOIN selected_tree tree ON child.parent_id = tree.folder_id
         )
         INSERT INTO picto_delete_folder_root(folder_id)
         SELECT selected.folder_id
         FROM picto_selected_folder selected
         WHERE NOT EXISTS (
             SELECT 1 FROM selected_tree tree
             WHERE tree.folder_id = selected.folder_id
               AND tree.root_id <> selected.folder_id
         );

         WITH RECURSIVE deleted_tree(folder_id) AS (
             SELECT folder.folder_id
             FROM folder
             JOIN picto_delete_folder_root root
               ON root.folder_id = folder.folder_id
             UNION ALL
             SELECT child.folder_id
             FROM folder child
             JOIN deleted_tree parent ON child.parent_id = parent.folder_id
         )
         INSERT INTO picto_deleted_folder(folder_id)
         SELECT DISTINCT folder_id FROM deleted_tree;",
    )?;
    Ok(())
}

fn clone_folder_tree_setwise(
    transaction: &Transaction<'_>,
    source_id: i64,
    parent_id: Option<i64>,
    root_name: &str,
    now: &str,
) -> rusqlite::Result<(i64, Vec<i64>, Vec<(i64, i64)>)> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_folder_clone (
             source_id INTEGER PRIMARY KEY,
             clone_id INTEGER NOT NULL UNIQUE,
             parent_source_id INTEGER,
             depth INTEGER NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM picto_folder_clone;",
    )?;
    let maximum_id: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(folder_id), 0) FROM folder",
        [],
        |row| row.get(0),
    )?;
    transaction.execute(
        "WITH RECURSIVE tree(source_id, parent_source_id, depth) AS (
             SELECT folder_id, parent_id, 0 FROM folder WHERE folder_id = ?1
             UNION ALL
             SELECT child.folder_id, child.parent_id, parent.depth + 1
             FROM folder child
             JOIN tree parent ON child.parent_id = parent.source_id
         ), numbered AS (
             SELECT source_id, parent_source_id, depth,
                    ROW_NUMBER() OVER (ORDER BY depth, source_id) AS ordinal
             FROM tree
         )
         INSERT INTO picto_folder_clone(source_id, clone_id, parent_source_id, depth)
         SELECT source_id, ?2 + ordinal, parent_source_id, depth FROM numbered",
        params![source_id, maximum_id],
    )?;
    let root_rank = next_sibling_rank(transaction, parent_id, None)?;
    transaction.execute(
        "INSERT INTO folder(
             folder_id, folder_key, name, parent_id, icon, color, notes,
             sort_rank, created_at, updated_at
         )
         SELECT mapping.clone_id,
                'folder:' || lower(hex(randomblob(16))),
                CASE WHEN mapping.depth = 0 THEN ?1 ELSE source.name END,
                CASE WHEN mapping.depth = 0 THEN ?2 ELSE parent.clone_id END,
                source.icon, source.color, source.notes,
                CASE WHEN mapping.depth = 0 THEN ?3 ELSE source.sort_rank END,
                ?4, ?4
         FROM picto_folder_clone mapping
         JOIN folder source ON source.folder_id = mapping.source_id
         LEFT JOIN picto_folder_clone parent
           ON parent.source_id = mapping.parent_source_id
         ORDER BY mapping.depth, mapping.source_id",
        params![root_name, parent_id, root_rank, now],
    )?;
    let cloned_pairs = transaction
        .prepare(
            "SELECT source_id, clone_id FROM picto_folder_clone
             ORDER BY depth, source_id",
        )?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let created_ids = cloned_pairs
        .iter()
        .map(|(_, clone)| *clone)
        .collect::<Vec<_>>();
    let duplicate_id = transaction.query_row(
        "SELECT clone_id FROM picto_folder_clone WHERE source_id = ?1",
        [source_id],
        |row| row.get(0),
    )?;
    Ok((duplicate_id, created_ids, cloned_pairs))
}

fn unique_folder_ids(folder_ids: &[FolderId]) -> Result<Vec<FolderId>, String> {
    let mut unique = BTreeSet::new();
    for folder_id in folder_ids {
        if !unique.insert(folder_id.0) {
            return Err(format!(
                "Folder reorder contains duplicate ID {}",
                folder_id.0
            ));
        }
    }
    Ok(folder_ids.to_vec())
}

fn non_empty(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    Ok(value.to_string())
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn new_folder_key() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("folder:{}", hex::encode(bytes))
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        folder_receipt, set_receipt_item_ids, CreateFolderInput, FolderId, FolderMetadataInput,
        FolderWatchInput, ReorderFolderChildrenInput, ReorderFolderItemsInput,
        SetFolderAutoTagsInput, SetFolderCoverInput, SortFolderTreeInput, MAX_RECEIPT_ITEM_IDS,
        RANK_GAP,
    };
    use crate::app::{Application, ItemId, ItemTarget};
    use crate::store::Store;

    #[test]
    fn folder_receipt_item_ids_are_bounded() {
        let folder_id = FolderId(1);
        let mut at_limit = folder_receipt(1, vec![folder_id], Vec::new(), None);
        set_receipt_item_ids(
            &mut at_limit.receipt,
            (1..=MAX_RECEIPT_ITEM_IDS as i64).map(ItemId).collect(),
        );
        assert_eq!(at_limit.receipt.item_ids.len(), MAX_RECEIPT_ITEM_IDS);

        let mut above_limit = folder_receipt(2, vec![folder_id], Vec::new(), None);
        set_receipt_item_ids(
            &mut above_limit.receipt,
            (1..=MAX_RECEIPT_ITEM_IDS as i64 + 1).map(ItemId).collect(),
        );
        assert!(above_limit.receipt.item_ids.is_empty());
        assert!(above_limit
            .receipt
            .resources
            .contains(&crate::app::resources::FOLDERS.to_string()));
        assert!(above_limit
            .receipt
            .resources
            .contains(&crate::app::resources::LIBRARY.to_string()));
    }

    fn fixture() -> (tempfile::TempDir, Application, i64) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let (media_id, _) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES ('folder-test-hash', 'image/png', 10, 'now')",
                    [],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES ('folder-test-item', 'media', 'now', 'now')",
                    [],
                )?;
                let media_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, imported_at, updated_at)
                     VALUES (?1, ?2, 'now', 'now')",
                    rusqlite::params![media_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                    [media_id],
                )?;
                crate::canonical_bitmap::seed_test_state(transaction)?;
                Ok(media_id)
            })
            .unwrap();
        (directory, Application::new(store), media_id)
    }

    fn create(app: &Application, name: &str, parent_id: Option<FolderId>) -> FolderId {
        app.create_folder(&CreateFolderInput {
            name: name.to_string(),
            parent_id,
            folder_key: None,
        })
        .unwrap()
        .0
    }

    fn assigned_tag_names(app: &Application, media_id: i64) -> Vec<String> {
        let roots = roaring::RoaringBitmap::from_iter([media_id as u32]);
        let tag_ids = app
            .projections()
            .tag_memberships_for_roots(&roots)
            .into_iter()
            .map(|(tag_id, _)| tag_id)
            .collect::<Vec<_>>();
        app.store()
            .read(|connection| {
                let encoded = serde_json::to_string(&tag_ids)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
                connection
                    .prepare(
                        "SELECT CASE WHEN tag.namespace = 'general' THEN tag.subtag
                                     ELSE tag.namespace || ':' || tag.subtag END
                         FROM tag
                         JOIN json_each(?1) selected
                           ON tag.tag_id = CAST(selected.value AS INTEGER)
                         ORDER BY tag.namespace, tag.subtag",
                    )?
                    .query_map([encoded], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
    }

    #[test]
    fn deleting_descendants_preserves_media_and_returns_parent_fallback() {
        let (_directory, app, media_id) = fixture();
        let root = create(&app, "Root", None);
        let child = create(&app, "Child", Some(root));
        let grandchild = create(&app, "Grandchild", Some(child));

        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)",
                    rusqlite::params![root.0, media_id],
                )?;
                Ok(())
            })
            .unwrap();
        let result = app.delete_folder(child).unwrap();

        assert_eq!(result.deleted_folder_ids, vec![child, grandchild]);
        assert_eq!(result.fallback_folder_id, Some(root));
        assert_eq!(result.receipt.item_ids, Vec::new());
        assert_eq!(
            result.receipt.resources,
            vec![
                "folders".to_string(),
                "sidebar".to_string(),
                "library".to_string(),
                format!("folder:{}", child.0),
                format!("folder:{}", grandchild.0),
            ]
        );
        app.store()
            .read(|connection| {
                let media_exists: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_asset WHERE item_id = ?1",
                    [media_id],
                    |row| row.get(0),
                )?;
                let item_exists: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_id = ?1",
                    [media_id],
                    |row| row.get(0),
                )?;
                let file_exists: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_file WHERE file_hash = 'folder-test-hash'",
                    [],
                    |row| row.get(0),
                )?;
                let root_folder_items: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id = ?1 AND item_id = ?2",
                    rusqlite::params![root.0, media_id],
                    |row| row.get(0),
                )?;
                assert_eq!(media_exists, 1);
                assert_eq!(item_exists, 1);
                assert_eq!(file_exists, 1);
                assert_eq!(root_folder_items, 1);
                Ok(())
            })
            .unwrap();

        app.undo().unwrap();
        app.store()
            .read(|connection| {
                let restored: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder WHERE folder_id IN (?1, ?2)",
                    rusqlite::params![child.0, grandchild.0],
                    |row| row.get(0),
                )?;
                assert_eq!(restored, 2);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn deleting_multiple_selected_hierarchies_is_one_operation() {
        let (_directory, app, _media_id) = fixture();
        let first = create(&app, "First", None);
        let child = create(&app, "Child", Some(first));
        let second = create(&app, "Second", None);

        let result = app.delete_folders(&[child, second, first, first]).unwrap();

        assert_eq!(result.deleted_folder_ids, vec![first, child, second]);
        assert_eq!(result.fallback_folder_id, None);
        app.store()
            .read(|connection| {
                let remaining: i64 =
                    connection.query_row("SELECT COUNT(*) FROM folder", [], |row| row.get(0))?;
                assert_eq!(remaining, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn bulk_deletion_validates_every_folder_before_deleting_any() {
        let (_directory, app, _media_id) = fixture();
        let existing = create(&app, "Existing", None);

        assert!(app.delete_folders(&[existing, FolderId(i64::MAX)]).is_err());
        app.store()
            .read(|connection| {
                let remaining: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder WHERE folder_id = ?1",
                    [existing.0],
                    |row| row.get(0),
                )?;
                assert_eq!(remaining, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn moving_folder_below_descendant_is_rejected_without_mutation() {
        let (_directory, app, _media_id) = fixture();
        let root = create(&app, "Root", None);
        let child = create(&app, "Child", Some(root));
        let grandchild = create(&app, "Grandchild", Some(child));

        let error = app.move_folder(root, Some(grandchild)).unwrap_err();
        assert!(error.contains("itself or its descendant"));
        app.store()
            .read(|connection| {
                let parent: Option<i64> = connection.query_row(
                    "SELECT parent_id FROM folder WHERE folder_id = ?1",
                    [root.0],
                    |row| row.get(0),
                )?;
                assert_eq!(parent, None);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn reordering_updates_sibling_order() {
        let (_directory, app, _media_id) = fixture();
        let first = create(&app, "First", None);
        let second = create(&app, "Second", None);
        let third = create(&app, "Third", None);

        app.reorder_folder_children(&ReorderFolderChildrenInput {
            parent_id: None,
            folder_ids: vec![third, first, second],
        })
        .unwrap();

        app.store()
            .read(|connection| {
                let mut statement = connection.prepare(
                    "SELECT folder_id FROM folder
                     WHERE parent_id IS NULL ORDER BY sort_rank, folder_id",
                )?;
                let ids = statement
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(ids, vec![third.0, first.0, second.0]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn metadata_round_trips_as_one_folder_update() {
        let (_directory, app, _media_id) = fixture();
        let folder = create(&app, "Styled", None);

        app.set_folder_metadata(&FolderMetadataInput {
            folder_id: folder,
            icon: Some("star".to_string()),
            color: Some("#ff8800".to_string()),
            notes: Some("Reference images".to_string()),
        })
        .unwrap();

        app.store()
            .read(|connection| {
                let metadata: (Option<String>, Option<String>, Option<String>) = connection
                    .query_row(
                        "SELECT icon, color, notes FROM folder WHERE folder_id = ?1",
                        [folder.0],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                assert_eq!(
                    metadata,
                    (
                        Some("star".to_string()),
                        Some("#ff8800".to_string()),
                        Some("Reference images".to_string()),
                    )
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn duplicating_folder_clones_structure_metadata_and_auto_tags_only() {
        let (_directory, app, media_id) = fixture();
        let source = create(&app, "Source", None);
        let child = create(&app, "Child", Some(source));
        app.set_folder_metadata(&FolderMetadataInput {
            folder_id: source,
            icon: Some("star".to_string()),
            color: Some("#ff8800".to_string()),
            notes: Some("Reference images".to_string()),
        })
        .unwrap();
        app.set_folder_auto_tags(&SetFolderAutoTagsInput {
            folder_id: child,
            tags: vec!["artist:melon".to_string()],
        })
        .unwrap();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)",
                    rusqlite::params![source.0, media_id],
                )?;
                transaction.execute(
                    "UPDATE folder SET watch_path = '/tmp/source', watch_enabled = 1,
                     watch_subfolders = 1 WHERE folder_id = ?1",
                    [source.0],
                )?;
                Ok(())
            })
            .unwrap();

        let (duplicate, receipt) = app.duplicate_folder(source).unwrap();
        assert_eq!(receipt.folder_ids.len(), 2);
        let duplicate_child = app
            .store()
            .read(|connection| {
                let duplicate_row: (String, Option<String>, Option<String>, Option<String>, bool) =
                    connection.query_row(
                        "SELECT name, icon, color, notes, watch_enabled
                         FROM folder WHERE folder_id = ?1",
                        [duplicate.0],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                            ))
                        },
                    )?;
                assert_eq!(
                    duplicate_row,
                    (
                        "Source copy".to_string(),
                        Some("star".to_string()),
                        Some("#ff8800".to_string()),
                        Some("Reference images".to_string()),
                        false,
                    )
                );
                let duplicate_child: i64 = connection.query_row(
                    "SELECT folder_id FROM folder WHERE parent_id = ?1 AND name = 'Child'",
                    [duplicate.0],
                    |row| row.get(0),
                )?;
                let copied_memberships: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id IN (?1, ?2)",
                    rusqlite::params![duplicate.0, duplicate_child],
                    |row| row.get(0),
                )?;
                assert_eq!(copied_memberships, 0);
                Ok(duplicate_child)
            })
            .unwrap();
        assert_eq!(
            app.folder_auto_tags(FolderId(duplicate_child)).unwrap(),
            vec!["artist:melon".to_string()]
        );
    }

    #[test]
    fn folder_auto_tags_apply_to_descendants_and_future_members_without_removal() {
        let (_directory, app, media_id) = fixture();
        let parent = create(&app, "Parent", None);
        let child = create(&app, "Child", Some(parent));
        app.set_folder_membership(
            &ItemTarget::Explicit {
                item_ids: vec![ItemId(media_id)],
            },
            child.0,
            true,
        )
        .unwrap();

        app.set_folder_auto_tags(&SetFolderAutoTagsInput {
            folder_id: parent,
            tags: vec!["artist:melon".to_string()],
        })
        .unwrap();
        assert_eq!(assigned_tag_names(&app, media_id), vec!["artist:melon"]);

        app.set_folder_auto_tags(&SetFolderAutoTagsInput {
            folder_id: parent,
            tags: Vec::new(),
        })
        .unwrap();
        assert_eq!(assigned_tag_names(&app, media_id), vec!["artist:melon"]);

        app.set_folder_membership(
            &ItemTarget::Explicit {
                item_ids: vec![ItemId(media_id)],
            },
            child.0,
            false,
        )
        .unwrap();

        app.set_folder_auto_tags(&SetFolderAutoTagsInput {
            folder_id: parent,
            tags: vec!["species:slime".to_string()],
        })
        .unwrap();
        assert_eq!(assigned_tag_names(&app, media_id), vec!["artist:melon"]);
        app.set_folder_membership(
            &ItemTarget::Explicit {
                item_ids: vec![ItemId(media_id)],
            },
            child.0,
            true,
        )
        .unwrap();
        assert_eq!(
            assigned_tag_names(&app, media_id),
            vec!["artist:melon", "species:slime"]
        );
    }

    #[test]
    fn sorting_folder_tree_reorders_the_selected_sibling_level() {
        let (_directory, app, _media_id) = fixture();
        let zulu = create(&app, "Zulu", None);
        let alpha = create(&app, "alpha", None);
        let mike = create(&app, "Mike", None);

        app.sort_folder_tree(&SortFolderTreeInput {
            folder_id: zulu,
            descending: false,
            recursive: false,
        })
        .unwrap();

        app.store()
            .read(|connection| {
                let ids = connection
                    .prepare(
                        "SELECT folder_id FROM folder WHERE parent_id IS NULL
                         ORDER BY sort_rank, folder_id",
                    )?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(ids, vec![alpha.0, mike.0, zulu.0]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn sorting_folder_items_by_name_writes_canonical_order() {
        let (_directory, app, zulu_id) = fixture();
        let folder = create(&app, "Sorted", None);
        let ((alpha_id, mike_id), _) = app
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO root_metadata (root_item_id, name, updated_at)
                     VALUES (?1, 'Zulu', 'now')
                     ON CONFLICT(root_item_id) DO UPDATE SET name = excluded.name,
                         updated_at = excluded.updated_at",
                    [zulu_id],
                )?;
                let create_root = |key: &str, label: &str| {
                    transaction.execute(
                        "INSERT INTO library_item
                             (item_key, kind, created_at, updated_at)
                         VALUES (?1, 'media', 'now', 'now')",
                        [key],
                    )?;
                    let item_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                        [item_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO root_metadata (root_item_id, name, updated_at)
                         VALUES (?1, ?2, 'now')",
                        rusqlite::params![item_id, label],
                    )?;
                    Ok::<_, rusqlite::Error>(item_id)
                };
                let alpha_id = create_root("sort-alpha", "Alpha")?;
                let mike_id = create_root("sort-mike", "mike")?;
                for (item_id, rank) in [(zulu_id, 10), (mike_id, 20), (alpha_id, 30)] {
                    transaction.execute(
                        "INSERT INTO folder_item (folder_id, item_id, position_rank)
                         VALUES (?1, ?2, ?3)",
                        rusqlite::params![folder.0, item_id, rank],
                    )?;
                }
                Ok((alpha_id, mike_id))
            })
            .unwrap();

        app.sort_folder_items_by_name(folder).unwrap();

        app.store()
            .read(|connection| {
                let ids = connection
                    .prepare(
                        "SELECT item_id FROM folder_item
                         WHERE folder_id = ?1 ORDER BY position_rank, item_id",
                    )?
                    .query_map([folder.0], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(ids, vec![alpha_id, mike_id, zulu_id]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn reordering_folder_items_covers_active_items_and_leaves_hidden_items_last() {
        let (_directory, app, first_id) = fixture();
        let folder = create(&app, "Ordered", None);
        let ((second_id, hidden_id), _) = app
            .store()
            .transaction(|transaction| {
                let create_media = |key: &str, lifecycle: &str| {
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_hash, mime_type, size_bytes, created_at)
                         VALUES (?1, 'image/png', 10, 'now')",
                        [format!("{key}-hash")],
                    )?;
                    let file_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                         VALUES (?1, 'media', 'now', 'now')",
                        [key],
                    )?;
                    let item_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO media_asset
                             (item_id, file_id, imported_at, updated_at)
                         VALUES (?1, ?2, 'now', 'now')",
                        rusqlite::params![item_id, file_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
                        rusqlite::params![item_id, lifecycle],
                    )?;
                    Ok::<_, rusqlite::Error>(item_id)
                };
                let second_id = create_media("folder-second-item", "active")?;
                let hidden_id = create_media("folder-hidden-item", "trash")?;
                for (item_id, rank) in [(first_id, 100), (second_id, 200), (hidden_id, 50)] {
                    transaction.execute(
                        "INSERT INTO folder_item (folder_id, item_id, position_rank)
                         VALUES (?1, ?2, ?3)",
                        rusqlite::params![folder.0, item_id, rank],
                    )?;
                }
                Ok((second_id, hidden_id))
            })
            .unwrap();

        let incomplete = app
            .reorder_folder_items(&ReorderFolderItemsInput {
                folder_id: folder,
                item_ids: vec![ItemId(first_id)],
            })
            .unwrap_err();
        assert!(incomplete.contains("every active folder item"));

        let receipt = app
            .reorder_folder_items(&ReorderFolderItemsInput {
                folder_id: folder,
                item_ids: vec![ItemId(second_id), ItemId(first_id)],
            })
            .unwrap();
        assert_eq!(
            receipt.receipt.item_ids,
            vec![ItemId(second_id), ItemId(first_id)]
        );
        assert!(receipt
            .receipt
            .resources
            .contains(&format!("folder:{}", folder.0)));

        app.store()
            .read(|connection| {
                let rows = connection
                    .prepare(
                        "SELECT item_id, position_rank FROM folder_item
                         WHERE folder_id = ?1 ORDER BY position_rank, item_id",
                    )?
                    .query_map([folder.0], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(
                    rows,
                    vec![
                        (second_id, RANK_GAP),
                        (first_id, RANK_GAP * 2),
                        (hidden_id, RANK_GAP * 3),
                    ]
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn watch_configuration_is_external_idempotent_and_clearable() {
        let (library, app, _media_id) = fixture();
        let watched = tempfile::tempdir().unwrap();
        let folder = create(&app, "Watched", None);
        let input = FolderWatchInput {
            folder_id: folder,
            path: watched.path().display().to_string(),
            include_subfolders: true,
        };

        let first = app.set_folder_watch(&input).unwrap();
        let repeated = app.set_folder_watch(&input).unwrap();
        assert_eq!(first.receipt.revision, repeated.receipt.revision);
        app.store()
            .read(|connection| {
                let value: (Option<String>, bool, bool) = connection.query_row(
                    "SELECT watch_path, watch_enabled, watch_subfolders
                     FROM folder WHERE folder_id = ?1",
                    [folder.0],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(
                    value,
                    (
                        Some(
                            std::fs::canonicalize(watched.path())
                                .unwrap()
                                .display()
                                .to_string()
                        ),
                        true,
                        true,
                    )
                );
                Ok(())
            })
            .unwrap();

        app.clear_folder_watch(folder).unwrap();
        let rejected = app
            .set_folder_watch(&FolderWatchInput {
                folder_id: folder,
                path: library.path().display().to_string(),
                include_subfolders: false,
            })
            .unwrap_err();
        assert!(rejected.contains("inside the Picto library"));
    }

    #[test]
    fn explicit_folder_cover_is_persistent_and_undoable() {
        let (_directory, app, media_id) = fixture();
        let folder = create(&app, "Covered", None);
        app.set_folder_membership(
            &ItemTarget::Explicit {
                item_ids: vec![ItemId(media_id)],
            },
            folder.0,
            true,
        )
        .unwrap();

        app.set_folder_cover(&SetFolderCoverInput {
            folder_id: folder,
            item_id: ItemId(media_id),
        })
        .unwrap();
        assert_eq!(
            app.folder_cover(folder).unwrap().unwrap().entity_hash,
            "folder-test-hash"
        );

        app.undo().unwrap();
        assert!(app.folder_cover(folder).unwrap().is_none());
        app.redo().unwrap();
        assert_eq!(
            app.folder_cover(folder).unwrap().unwrap().entity_hash,
            "folder-test-hash"
        );
    }

    #[test]
    fn folder_cover_rejects_items_outside_the_folder() {
        let (_directory, app, media_id) = fixture();
        let folder = create(&app, "Covered", None);
        let error = app
            .set_folder_cover(&SetFolderCoverInput {
                folder_id: folder,
                item_id: ItemId(media_id),
            })
            .unwrap_err();
        assert!(error.contains("must belong to the folder"));
    }
}
