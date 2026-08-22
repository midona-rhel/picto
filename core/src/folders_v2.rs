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

use crate::app::{resources, Application, MutationReceipt};

const RANK_GAP: i64 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
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

        let (folder_id, revision) = self.transaction_rebuilding(|transaction| {
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
            Ok(transaction.last_insert_rowid())
        })?;

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
        let ((), revision) = self.transaction_rebuilding(|transaction| {
            require_folder(transaction, folder_id.0)?;
            transaction.execute(
                "UPDATE folder SET name = ?1, updated_at = ?2 WHERE folder_id = ?3",
                params![name, now, folder_id.0],
            )?;
            Ok(())
        })?;

        Ok(folder_receipt(revision, vec![folder_id], Vec::new(), None))
    }

    pub fn move_folder(
        &self,
        folder_id: FolderId,
        parent_id: Option<FolderId>,
    ) -> Result<FolderMutationReceipt, String> {
        let parent_id = parent_id.map(|id| id.0);
        let now = Utc::now().to_rfc3339();
        let ((), revision) = self.transaction_rebuilding(|transaction| {
            require_folder(transaction, folder_id.0)?;
            if let Some(parent_id) = parent_id {
                require_folder(transaction, parent_id)?;
                if parent_id == folder_id.0 || is_descendant(transaction, folder_id.0, parent_id)? {
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
            Ok(())
        })?;

        Ok(folder_receipt(revision, vec![folder_id], Vec::new(), None))
    }

    pub fn reorder_folder_children(
        &self,
        input: &ReorderFolderChildrenInput,
    ) -> Result<FolderMutationReceipt, String> {
        let folder_ids = unique_folder_ids(&input.folder_ids)?;
        let parent_id = input.parent_id.map(|id| id.0);
        let now = Utc::now().to_rfc3339();

        let ((), revision) = self.transaction_rebuilding(|transaction| {
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
            for (index, folder_id) in folder_ids.iter().enumerate() {
                transaction.execute(
                    "UPDATE folder SET sort_rank = ?1, updated_at = ?2 WHERE folder_id = ?3",
                    params![(index as i64 + 1) * RANK_GAP, now, folder_id.0],
                )?;
            }
            Ok(())
        })?;

        Ok(folder_receipt(revision, folder_ids, Vec::new(), None))
    }

    pub fn delete_folder(&self, folder_id: FolderId) -> Result<FolderMutationReceipt, String> {
        let ((deleted_folder_ids, fallback_folder_id), revision) =
            self.transaction_rebuilding(|transaction| {
                require_folder(transaction, folder_id.0)?;
                let fallback_folder_id = transaction.query_row(
                    "SELECT parent_id FROM folder WHERE folder_id = ?1",
                    [folder_id.0],
                    |row| row.get::<_, Option<i64>>(0),
                )?;
                let deleted_folder_ids = descendant_folder_ids(transaction, folder_id.0)?;
                transaction.execute("DELETE FROM folder WHERE folder_id = ?1", [folder_id.0])?;
                Ok((deleted_folder_ids, fallback_folder_id))
            })?;

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
    resources.extend(
        folder_ids
            .iter()
            .map(|folder_id| format!("folder:{}", folder_id.0)),
    );
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

fn require_folder(transaction: &Transaction<'_>, folder_id: i64) -> rusqlite::Result<()> {
    transaction
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

fn descendant_folder_ids(
    transaction: &Transaction<'_>,
    folder_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut statement = transaction.prepare(
        "WITH RECURSIVE descendants(folder_id, depth) AS (
             SELECT folder_id, 0 FROM folder WHERE folder_id = ?1
             UNION ALL
             SELECT child.folder_id, parent.depth + 1
             FROM folder child
             JOIN descendants parent ON child.parent_id = parent.folder_id
         )
         SELECT folder_id FROM descendants ORDER BY depth, folder_id",
    )?;
    let folder_ids = statement
        .query_map([folder_id], |row| row.get(0))?
        .collect();
    folder_ids
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

    use super::{CreateFolderInput, FolderId, ReorderFolderChildrenInput};
    use crate::app::Application;
    use crate::store::Store;

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
}
