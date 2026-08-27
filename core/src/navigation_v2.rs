//! Canonical folder and smart-folder navigation for the replacement backend.

use std::collections::BTreeSet;

use chrono::Utc;
use rand::RngCore;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};
use crate::cloud::capture::{
    record_smart_folder_created, record_smart_folder_delete, record_smart_folder_upsert,
};
use crate::smart_v2::SmartFolderPredicate;
use crate::store::history::HistoryDescriptor;

const RANK_GAP: i64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderNavigationItem {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    #[ts(type = "number")]
    pub sort_rank: i64,
    pub watch_path: Option<String>,
    pub watch_enabled: bool,
    pub watch_subfolders: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SmartFolderNavigationItem {
    #[ts(type = "number")]
    pub smart_folder_id: i64,
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub predicate: SmartFolderPredicate,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    #[ts(type = "number")]
    pub display_order: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct NavigationSnapshot {
    pub folders: Vec<FolderNavigationItem>,
    pub smart_folders: Vec<SmartFolderNavigationItem>,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreateSmartFolderInput {
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    pub predicate: SmartFolderPredicate,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SmartFolderMutationReceipt {
    pub receipt: MutationReceipt,
    #[ts(type = "number[]")]
    pub smart_folder_ids: Vec<i64>,
    #[ts(type = "number[]")]
    pub deleted_smart_folder_ids: Vec<i64>,
    #[ts(type = "number | null")]
    pub fallback_smart_folder_id: Option<i64>,
}

pub fn navigation(application: &Application) -> Result<NavigationSnapshot, String> {
    application.store().read(|connection| {
        let folders = connection
            .prepare(
                "SELECT folder_id, name, parent_id, icon, color, notes,
                        COALESCE(sort_rank, 0), watch_path, watch_enabled, watch_subfolders
                 FROM folder ORDER BY COALESCE(sort_rank, 0), folder_id",
            )?
            .query_map([], |row| {
                Ok(FolderNavigationItem {
                    folder_id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    icon: row.get(3)?,
                    color: row.get(4)?,
                    notes: row.get(5)?,
                    sort_rank: row.get(6)?,
                    watch_path: row.get(7)?,
                    watch_enabled: row.get(8)?,
                    watch_subfolders: row.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let raw_smart_folders = connection
            .prepare(
                "SELECT smart_folder_id, name, parent_id, icon, color, notes,
                        predicate_json, sort_field, sort_order, COALESCE(display_order, 0)
                 FROM smart_folder ORDER BY COALESCE(display_order, 0), smart_folder_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let smart_folders = raw_smart_folders
            .into_iter()
            .map(
                |(
                    smart_folder_id,
                    name,
                    parent_id,
                    icon,
                    color,
                    notes,
                    predicate_json,
                    sort_field,
                    sort_order,
                    display_order,
                )| {
                    let predicate = serde_json::from_str(&predicate_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(SmartFolderNavigationItem {
                        smart_folder_id,
                        name,
                        parent_id,
                        icon,
                        color,
                        notes,
                        predicate,
                        sort_field,
                        sort_order,
                        display_order,
                    })
                },
            )
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(NavigationSnapshot {
            folders,
            smart_folders,
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

impl Application {
    pub fn create_smart_folder_v2(
        &self,
        input: &CreateSmartFolderInput,
    ) -> Result<(i64, SmartFolderMutationReceipt), String> {
        let input = PreparedSmartFolder::from_input(input)?;
        let projection = self.projections().selection_snapshot();
        let active_roots = projection.lifecycle_bitmap(crate::app::Lifecycle::Active);
        let now = Utc::now().to_rfc3339();
        let key = new_key();
        let (smart_folder_id, revision, _) = self.undoable_transaction(
            smart_folder_history("smart_folders.create", "Create smart folder"),
            |transaction| {
                if let Some(parent_id) = input.parent_id {
                    require_smart_folder(transaction, parent_id)?;
                }
                let display_order = next_order(transaction, input.parent_id, None)?;
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_key, name, parent_id, icon, color, notes,
                         predicate_json, sort_field, sort_order, display_order,
                         created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                    params![
                        key,
                        input.name,
                        input.parent_id,
                        input.icon,
                        input.color,
                        input.notes,
                        input.predicate_json,
                        input.sort_field,
                        input.sort_order,
                        display_order,
                        now,
                    ],
                )?;
                let smart_folder_id = transaction.last_insert_rowid();
                crate::smart_v2::rebuild_generations(
                    transaction,
                    &[smart_folder_id],
                    &active_roots,
                    |tag_id| projection.tag_bitmap(tag_id),
                )?;
                record_smart_folder_created(transaction, &[smart_folder_id])?;
                Ok((smart_folder_id, ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok((
            smart_folder_id,
            smart_receipt(revision, vec![smart_folder_id], Vec::new(), None),
        ))
    }

    pub fn update_smart_folder_v2(
        &self,
        smart_folder_id: i64,
        input: &CreateSmartFolderInput,
    ) -> Result<SmartFolderMutationReceipt, String> {
        let input = PreparedSmartFolder::from_input(input)?;
        let projection = self.projections().selection_snapshot();
        let active_roots = projection.lifecycle_bitmap(crate::app::Lifecycle::Active);
        let now = Utc::now().to_rfc3339();
        let (affected, revision, _) = self.undoable_transaction(
            smart_folder_history("smart_folders.update", "Edit smart folder"),
            |transaction| {
                require_smart_folder(transaction, smart_folder_id)?;
                validate_parent(transaction, smart_folder_id, input.parent_id)?;
                let previous: (
                    String,
                    Option<i64>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    String,
                    Option<String>,
                    Option<String>,
                ) = transaction.query_row(
                    "SELECT name, parent_id, icon, color, notes, predicate_json,
                            sort_field, sort_order
                     FROM smart_folder WHERE smart_folder_id = ?1",
                    [smart_folder_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )?;
                let changed_fields = [
                    ("name", previous.0 != input.name),
                    ("parent", previous.1 != input.parent_id),
                    ("icon", previous.2 != input.icon),
                    ("color", previous.3 != input.color),
                    ("notes", previous.4 != input.notes),
                    ("predicate_json", previous.5 != input.predicate_json),
                    ("sort_field", previous.6 != input.sort_field),
                    ("sort_order", previous.7 != input.sort_order),
                ]
                .into_iter()
                .filter_map(|(field, changed)| changed.then_some(field))
                .collect::<Vec<_>>();
                let definition_changed = changed_fields
                    .iter()
                    .any(|field| matches!(*field, "parent" | "predicate_json"));
                if definition_changed {
                    transaction.execute(
                        "UPDATE smart_folder
                         SET name = ?1, parent_id = ?2, icon = ?3, color = ?4, notes = ?5,
                             predicate_json = ?6, sort_field = ?7, sort_order = ?8,
                             updated_at = ?9
                         WHERE smart_folder_id = ?10",
                        params![
                            input.name,
                            input.parent_id,
                            input.icon,
                            input.color,
                            input.notes,
                            input.predicate_json,
                            input.sort_field,
                            input.sort_order,
                            now,
                            smart_folder_id,
                        ],
                    )?;
                } else {
                    transaction.execute(
                        "UPDATE smart_folder
                         SET name = ?1, icon = ?2, color = ?3, notes = ?4,
                             sort_field = ?5, sort_order = ?6, updated_at = ?7
                         WHERE smart_folder_id = ?8",
                        params![
                            input.name,
                            input.icon,
                            input.color,
                            input.notes,
                            input.sort_field,
                            input.sort_order,
                            now,
                            smart_folder_id,
                        ],
                    )?;
                }
                let affected = descendant_ids(transaction, smart_folder_id)?;
                if definition_changed {
                    crate::smart_v2::rebuild_generations(
                        transaction,
                        &affected,
                        &active_roots,
                        |tag_id| projection.tag_bitmap(tag_id),
                    )?;
                }
                record_smart_folder_upsert(transaction, &[smart_folder_id], &changed_fields)?;
                Ok((affected, ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(smart_receipt(revision, affected, Vec::new(), None))
    }

    pub fn move_smart_folder_v2(
        &self,
        smart_folder_id: i64,
        parent_id: Option<i64>,
    ) -> Result<SmartFolderMutationReceipt, String> {
        let projection = self.projections().selection_snapshot();
        let active_roots = projection.lifecycle_bitmap(crate::app::Lifecycle::Active);
        let now = Utc::now().to_rfc3339();
        let (affected, revision, _) = self.undoable_transaction(
            smart_folder_history("smart_folders.move", "Move smart folder"),
            |transaction| {
                require_smart_folder(transaction, smart_folder_id)?;
                validate_parent(transaction, smart_folder_id, parent_id)?;
                let previous_parent_id: Option<i64> = transaction.query_row(
                    "SELECT parent_id FROM smart_folder WHERE smart_folder_id = ?1",
                    [smart_folder_id],
                    |row| row.get(0),
                )?;
                let display_order = next_order(transaction, parent_id, Some(smart_folder_id))?;
                let parent_changed = previous_parent_id != parent_id;
                if parent_changed {
                    transaction.execute(
                        "UPDATE smart_folder
                         SET parent_id = ?1, display_order = ?2, updated_at = ?3
                         WHERE smart_folder_id = ?4",
                        params![parent_id, display_order, now, smart_folder_id],
                    )?;
                } else {
                    transaction.execute(
                        "UPDATE smart_folder
                         SET display_order = ?1, updated_at = ?2
                         WHERE smart_folder_id = ?3",
                        params![display_order, now, smart_folder_id],
                    )?;
                }
                let affected = descendant_ids(transaction, smart_folder_id)?;
                if parent_changed {
                    crate::smart_v2::rebuild_generations(
                        transaction,
                        &affected,
                        &active_roots,
                        |tag_id| projection.tag_bitmap(tag_id),
                    )?;
                }
                let changed_fields = if parent_changed {
                    &["parent", "display_order"][..]
                } else {
                    &["display_order"][..]
                };
                record_smart_folder_upsert(transaction, &[smart_folder_id], changed_fields)?;
                Ok((affected, ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(smart_receipt(revision, affected, Vec::new(), None))
    }

    pub fn reorder_smart_folder_children_v2(
        &self,
        parent_id: Option<i64>,
        smart_folder_ids: &[i64],
    ) -> Result<SmartFolderMutationReceipt, String> {
        let requested = smart_folder_ids.iter().copied().collect::<BTreeSet<_>>();
        if requested.len() != smart_folder_ids.len() {
            return Err("Smart-folder reorder contains duplicate IDs".to_string());
        }
        let now = Utc::now().to_rfc3339();
        let (_, revision, _) = self.undoable_transaction(
            smart_folder_history("smart_folders.reorder", "Reorder smart folders"),
            |transaction| {
                if let Some(parent_id) = parent_id {
                    require_smart_folder(transaction, parent_id)?;
                }
                let expected = child_ids(transaction, parent_id)?;
                if expected.len() != requested.len()
                    || expected.into_iter().collect::<BTreeSet<_>>() != requested
                {
                    return Err(invalid(
                        "Smart-folder reorder must contain every sibling exactly once",
                    ));
                }
                for (index, smart_folder_id) in smart_folder_ids.iter().enumerate() {
                    transaction.execute(
                        "UPDATE smart_folder SET display_order = ?1, updated_at = ?2
                     WHERE smart_folder_id = ?3",
                        params![(index as i64 + 1) * RANK_GAP, now, smart_folder_id],
                    )?;
                }
                record_smart_folder_upsert(transaction, smart_folder_ids, &["display_order"])?;
                Ok(((), ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(smart_receipt(
            revision,
            smart_folder_ids.to_vec(),
            Vec::new(),
            None,
        ))
    }

    pub fn delete_smart_folder_v2(
        &self,
        smart_folder_id: i64,
    ) -> Result<SmartFolderMutationReceipt, String> {
        let ((deleted_ids, fallback), revision, _) = self.undoable_transaction(
            smart_folder_history("smart_folders.delete", "Delete smart folder"),
            |transaction| {
                let fallback = transaction
                    .query_row(
                        "SELECT parent_id FROM smart_folder WHERE smart_folder_id = ?1",
                        [smart_folder_id],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .optional()?
                    .ok_or_else(|| invalid("smart folder does not exist"))?;
                let deleted_ids = descendant_ids(transaction, smart_folder_id)?;
                crate::smart_v2::stage_projection_changes(transaction, &deleted_ids)?;
                let deleted_keys = deleted_ids
                    .iter()
                    .map(|deleted_id| {
                        transaction.query_row(
                            "SELECT smart_folder_key FROM smart_folder
                             WHERE smart_folder_id = ?1",
                            [deleted_id],
                            |row| row.get::<_, String>(0),
                        )
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                transaction.execute(
                    "DELETE FROM smart_folder WHERE smart_folder_id = ?1",
                    [smart_folder_id],
                )?;
                record_smart_folder_delete(transaction, deleted_keys)?;
                Ok(((deleted_ids, fallback), ()))
            },
            |_, ()| Ok(()),
        )?;
        Ok(smart_receipt(
            revision,
            deleted_ids.clone(),
            deleted_ids,
            fallback,
        ))
    }
}

fn smart_folder_history(command: &str, label: &str) -> HistoryDescriptor {
    HistoryDescriptor::new(
        command,
        label,
        vec![
            resources::SMART_FOLDERS.to_string(),
            resources::SIDEBAR.to_string(),
            resources::LIBRARY.to_string(),
        ],
        Vec::new(),
    )
}

struct PreparedSmartFolder {
    name: String,
    parent_id: Option<i64>,
    predicate_json: String,
    icon: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    sort_field: Option<String>,
    sort_order: Option<String>,
}

impl PreparedSmartFolder {
    fn from_input(input: &CreateSmartFolderInput) -> Result<Self, String> {
        let name = required("Smart-folder name", &input.name)?;
        let sort_order = optional(input.sort_order.as_deref());
        if sort_order
            .as_deref()
            .is_some_and(|value| !matches!(value, "ascending" | "descending"))
        {
            return Err("Smart-folder sort order must be ascending or descending".to_string());
        }
        Ok(Self {
            name,
            parent_id: input.parent_id,
            predicate_json: serde_json::to_string(&input.predicate)
                .map_err(|error| error.to_string())?,
            icon: optional(input.icon.as_deref()),
            color: optional(input.color.as_deref()),
            notes: optional(input.notes.as_deref()),
            sort_field: optional(input.sort_field.as_deref()),
            sort_order,
        })
    }
}

fn validate_parent(
    transaction: &Transaction<'_>,
    smart_folder_id: i64,
    parent_id: Option<i64>,
) -> rusqlite::Result<()> {
    if let Some(parent_id) = parent_id {
        require_smart_folder(transaction, parent_id)?;
        if parent_id == smart_folder_id
            || descendant_ids(transaction, smart_folder_id)?.contains(&parent_id)
        {
            return Err(invalid(
                "Cannot move a smart folder below itself or its descendant",
            ));
        }
    }
    Ok(())
}

fn require_smart_folder(
    transaction: &Transaction<'_>,
    smart_folder_id: i64,
) -> rusqlite::Result<()> {
    transaction
        .query_row(
            "SELECT 1 FROM smart_folder WHERE smart_folder_id = ?1",
            [smart_folder_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| invalid(format!("Smart folder {smart_folder_id} does not exist")))
}

fn descendant_ids(
    transaction: &Transaction<'_>,
    smart_folder_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    transaction
        .prepare(
            "WITH RECURSIVE descendants(smart_folder_id) AS (
                 SELECT smart_folder_id FROM smart_folder WHERE smart_folder_id = ?1
                 UNION
                 SELECT child.smart_folder_id
                 FROM smart_folder child
                 JOIN descendants parent ON child.parent_id = parent.smart_folder_id
             )
             SELECT smart_folder_id
             FROM descendants
             ORDER BY smart_folder_id != ?1, smart_folder_id",
        )?
        .query_map([smart_folder_id], |row| row.get(0))?
        .collect()
}

fn child_ids(transaction: &Transaction<'_>, parent_id: Option<i64>) -> rusqlite::Result<Vec<i64>> {
    transaction
        .prepare(
            "SELECT smart_folder_id FROM smart_folder
             WHERE (?1 IS NULL AND parent_id IS NULL) OR parent_id = ?1
             ORDER BY COALESCE(display_order, 0), smart_folder_id",
        )?
        .query_map([parent_id], |row| row.get(0))?
        .collect()
}

fn next_order(
    transaction: &Transaction<'_>,
    parent_id: Option<i64>,
    excluding_id: Option<i64>,
) -> rusqlite::Result<i64> {
    let current = transaction.query_row(
        "SELECT COALESCE(MAX(display_order), 0) FROM smart_folder
         WHERE ((?1 IS NULL AND parent_id IS NULL) OR parent_id = ?1)
           AND (?2 IS NULL OR smart_folder_id != ?2)",
        params![parent_id, excluding_id],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(current.saturating_add(RANK_GAP))
}

fn smart_receipt(
    revision: u64,
    smart_folder_ids: Vec<i64>,
    deleted_smart_folder_ids: Vec<i64>,
    fallback_smart_folder_id: Option<i64>,
) -> SmartFolderMutationReceipt {
    SmartFolderMutationReceipt {
        receipt: MutationReceipt {
            revision,
            resources: vec![
                resources::SMART_FOLDERS.to_string(),
                resources::SIDEBAR.to_string(),
                resources::LIBRARY.to_string(),
            ],
            item_ids: Vec::new(),
        },
        smart_folder_ids,
        deleted_smart_folder_ids,
        fallback_smart_folder_id,
    }
}

fn required(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    Ok(value.to_string())
}

fn optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn new_key() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("smart:{}", hex::encode(bytes))
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::canonical_bitmap::{load_bitmap, BitmapDomain};
    use crate::smart_v2::{MatchMode, PredicateRule, SmartRuleGroup};
    use crate::store::schema::create_canonical_v1;
    use crate::store::Store;
    use roaring::RoaringBitmap;
    use rusqlite::Connection;

    fn fixture() -> (tempfile::TempDir, Application) {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        (directory, application)
    }

    fn input(name: &str, parent_id: Option<i64>, minimum_rating: i64) -> CreateSmartFolderInput {
        CreateSmartFolderInput {
            name: name.to_string(),
            parent_id,
            predicate: SmartFolderPredicate {
                groups: vec![SmartRuleGroup {
                    match_mode: MatchMode::All,
                    negate: false,
                    rules: vec![PredicateRule {
                        field: "rating".to_string(),
                        op: "gte".to_string(),
                        value: Some(serde_json::json!(minimum_rating)),
                        value2: None,
                        values: None,
                    }],
                }],
            },
            icon: None,
            color: None,
            notes: None,
            sort_field: None,
            sort_order: None,
        }
    }

    fn seed_media(application: &Application) {
        application
            .store()
            .transaction(|transaction| {
                transaction.execute_batch(
                    "INSERT INTO media_file (
                         file_id, file_hash, mime_type, size_bytes, created_at
                     ) VALUES
                         (1, 'hash-1', 'image/png', 10, 'now'),
                         (2, 'hash-2', 'image/png', 10, 'now');
                     INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES
                         (1, 'item-1', 'media', 'now', 'now'),
                         (2, 'item-2', 'media', 'now', 'now');
                     INSERT INTO library_root(item_id, lifecycle) VALUES
                         (1, 'active'), (2, 'active');
                     INSERT INTO media_asset (
                         item_id, file_id, name, imported_at, updated_at
                     ) VALUES
                         (1, 1, 'one', 'now', 'now'),
                         (2, 2, 'two', 'now', 'now');
                     INSERT INTO root_metadata (
                         root_item_id, name, rating, notes, source_urls_json, updated_at
                     ) VALUES
                         (1, 'one', 5, NULL, '[]', 'now'),
                         (2, 'two', 2, NULL, '[]', 'now');",
                )?;
                Ok(())
            })
            .unwrap();
        let mut structure = crate::projection_v2::StructureProjectionDelta::default();
        for item_id in [1, 2] {
            structure
                .items
                .push(crate::projection_v2::ItemProjectionChange {
                    item_id,
                    kind: crate::app::ItemKind::Media,
                    present: true,
                });
            structure.media_classifications.push(
                crate::projection_v2::MediaClassificationProjectionChange {
                    media_id: item_id,
                    is_image: true,
                    mime_type: "image/png".to_string(),
                },
            );
            structure
                .roots
                .push(crate::projection_v2::RootProjectionChange {
                    item_id,
                    lifecycle: Some(crate::app::Lifecycle::Active),
                });
        }
        application
            .projections()
            .apply_structure_delta(structure)
            .unwrap();
        application
            .projections()
            .apply_root_summary_changes(
                &[
                    crate::projection_v2::RootSummaryProjectionChange {
                        item_id: 1,
                        total_size_bytes: 10,
                        media_count: 1,
                        rating: Some(5),
                    },
                    crate::projection_v2::RootSummaryProjectionChange {
                        item_id: 2,
                        total_size_bytes: 10,
                        media_count: 1,
                        rating: Some(2),
                    },
                ],
                &RoaringBitmap::new(),
            )
            .unwrap();
    }

    fn generation_state(
        application: &Application,
        smart_folder_id: i64,
    ) -> (i64, i64, i64, i64, i64) {
        application
            .store()
            .read(|connection| {
                let active = connection.query_row(
                    "SELECT generation_id, database_revision, member_count
                     FROM smart_folder_generation
                     WHERE smart_folder_id = ?1 AND state = 'active'",
                    [smart_folder_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )?;
                let actual_members: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM smart_folder_membership
                     WHERE generation_id = ?1",
                    [active.0],
                    |row| row.get(0),
                )?;
                let building: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM smart_folder_generation
                     WHERE smart_folder_id = ?1 AND state = 'building'",
                    [smart_folder_id],
                    |row| row.get(0),
                )?;
                Ok((active.0, active.1, active.2, actual_members, building))
            })
            .unwrap()
    }

    fn assert_exact_active(
        application: &Application,
        smart_folder_id: i64,
        revision: u64,
        expected_members: i64,
    ) -> i64 {
        let state = generation_state(application, smart_folder_id);
        assert_eq!(state.1, revision as i64);
        assert_eq!(state.2, expected_members);
        assert_eq!(state.3, expected_members);
        assert_eq!(state.4, 0);
        let expected = application
            .store()
            .read(|connection| {
                connection
                    .prepare(
                        "SELECT membership.root_item_id
                         FROM smart_folder_generation generation
                         JOIN smart_folder_membership membership
                           ON membership.generation_id = generation.generation_id
                         WHERE generation.smart_folder_id = ?1
                           AND generation.state = 'active'
                         ORDER BY membership.root_item_id",
                    )?
                    .query_map([smart_folder_id], |row| row.get::<_, u32>(0))?
                    .collect::<rusqlite::Result<RoaringBitmap>>()
            })
            .unwrap();
        assert_eq!(
            application
                .projections()
                .selection_snapshot()
                .smart_folder_bitmap(smart_folder_id),
            expected
        );
        let canonical = application
            .store()
            .read(|connection| load_bitmap(connection, BitmapDomain::SmartFolder, smart_folder_id))
            .unwrap();
        assert_eq!(canonical, expected);
        state.0
    }

    #[test]
    fn navigation_reads_ordered_folder_and_smart_folder_rows() {
        let (_directory, application) = fixture();
        application
            .create_folder(&crate::folders_v2::CreateFolderInput {
                name: "Folder".to_string(),
                parent_id: None,
                folder_key: None,
            })
            .unwrap();
        let (smart_id, _) = application
            .create_smart_folder_v2(&input("Smart", None, 1))
            .unwrap();

        let snapshot = navigation(&application).unwrap();
        assert_eq!(snapshot.folders[0].name, "Folder");
        assert_eq!(snapshot.smart_folders[0].smart_folder_id, smart_id);
        assert_eq!(snapshot.smart_folders[0].name, "Smart");
        assert_eq!(snapshot.revision, 3);
    }

    #[test]
    fn moving_below_a_descendant_is_rejected() {
        let (_directory, application) = fixture();
        let (parent, _) = application
            .create_smart_folder_v2(&input("Parent", None, 1))
            .unwrap();
        let (child, _) = application
            .create_smart_folder_v2(&input("Child", Some(parent), 2))
            .unwrap();

        assert!(application
            .move_smart_folder_v2(parent, Some(child))
            .is_err());
    }

    #[test]
    fn deleting_a_parent_reports_and_deletes_the_whole_hierarchy() {
        let (_directory, application) = fixture();
        let (parent, _) = application
            .create_smart_folder_v2(&input("Parent", None, 1))
            .unwrap();
        let (child, _) = application
            .create_smart_folder_v2(&input("Child", Some(parent), 2))
            .unwrap();
        let receipt = application.delete_smart_folder_v2(parent).unwrap();

        assert_eq!(receipt.deleted_smart_folder_ids, vec![parent, child]);
        assert_eq!(receipt.fallback_smart_folder_id, None);
        assert!(navigation(&application).unwrap().smart_folders.is_empty());
        assert!(application
            .projections()
            .selection_snapshot()
            .smart_folder_bitmap(parent)
            .is_empty());

        application.undo().unwrap();
        let restored = navigation(&application).unwrap().smart_folders;
        assert_eq!(restored.len(), 2);
        assert!(restored
            .iter()
            .any(|folder| folder.smart_folder_id == parent));
        assert!(restored
            .iter()
            .any(|folder| folder.smart_folder_id == child));
        let parent_state = generation_state(&application, parent);
        assert_exact_active(&application, parent, parent_state.1 as u64, 0);
    }

    #[test]
    fn create_update_and_move_publish_exact_active_generations() {
        let (_directory, application) = fixture();
        seed_media(&application);
        let (high, high_create) = application
            .create_smart_folder_v2(&input("High", None, 4))
            .unwrap();
        let (any, any_create) = application
            .create_smart_folder_v2(&input("Any", None, 0))
            .unwrap();
        let (child, child_create) = application
            .create_smart_folder_v2(&input("Child", Some(high), 0))
            .unwrap();

        assert_exact_active(&application, high, high_create.receipt.revision, 1);
        assert_exact_active(&application, any, any_create.receipt.revision, 2);
        assert_exact_active(&application, child, child_create.receipt.revision, 1);

        let updated = application
            .update_smart_folder_v2(high, &input("High", None, 5))
            .unwrap();
        assert_eq!(updated.smart_folder_ids, vec![high, child]);
        assert_exact_active(&application, high, updated.receipt.revision, 1);
        assert_exact_active(&application, child, updated.receipt.revision, 1);

        let moved = application.move_smart_folder_v2(child, Some(any)).unwrap();
        assert_exact_active(&application, child, moved.receipt.revision, 2);
    }

    #[test]
    fn metadata_only_edits_and_same_parent_moves_keep_the_active_generation() {
        let (_directory, application) = fixture();
        let (smart_folder_id, created) = application
            .create_smart_folder_v2(&input("Original", None, 1))
            .unwrap();
        let original =
            assert_exact_active(&application, smart_folder_id, created.receipt.revision, 0);

        let mut metadata_edit = input("Renamed", None, 1);
        metadata_edit.icon = Some("star".to_string());
        metadata_edit.notes = Some("presentation only".to_string());
        application
            .update_smart_folder_v2(smart_folder_id, &metadata_edit)
            .unwrap();
        assert_eq!(generation_state(&application, smart_folder_id).0, original);
        assert_eq!(generation_state(&application, smart_folder_id).4, 0);

        application
            .move_smart_folder_v2(smart_folder_id, None)
            .unwrap();
        assert_eq!(generation_state(&application, smart_folder_id).0, original);
        assert_eq!(generation_state(&application, smart_folder_id).4, 0);
    }

    #[test]
    fn descendant_walk_terminates_on_a_corrupt_parent_cycle() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();
        let empty_predicate =
            serde_json::to_string(&SmartFolderPredicate { groups: Vec::new() }).unwrap();
        connection
            .execute(
                "INSERT INTO smart_folder (
                     smart_folder_id, smart_folder_key, name, predicate_json,
                     created_at, updated_at
                 ) VALUES
                     (10, 'ten', 'Ten', ?1, 'now', 'now'),
                     (11, 'eleven', 'Eleven', ?1, 'now', 'now')",
                [&empty_predicate],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE smart_folder SET parent_id = 11 WHERE smart_folder_id = 10",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE smart_folder SET parent_id = 10 WHERE smart_folder_id = 11",
                [],
            )
            .unwrap();

        let transaction = connection.transaction().unwrap();
        assert_eq!(descendant_ids(&transaction, 10).unwrap(), vec![10, 11]);
        transaction.rollback().unwrap();
    }
}
