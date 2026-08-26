//! Recently viewed persistence for library roots.

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::store::history::HistoryDescriptor;

impl Application {
    /// Record a view for a visible root item.
    pub fn record_recent_view(&self, root_item_id: ItemId) -> Result<MutationReceipt, String> {
        self.record_recent_view_at(root_item_id, &Utc::now().to_rfc3339())
    }

    /// Clear the persisted Recently Viewed history without changing library items.
    pub fn clear_recent_views(&self) -> Result<MutationReceipt, String> {
        let (item_ids, revision, _, _) = self.undoable_transaction_if_changed(
            HistoryDescriptor::new(
                "items.clear_recent_views",
                "Clear recently viewed",
                vec![
                    resources::RECENTLY_VIEWED.to_string(),
                    resources::SIDEBAR.to_string(),
                ],
                Vec::new(),
            ),
            |transaction| {
                let item_ids = transaction
                    .prepare("SELECT item_id FROM media_view ORDER BY item_id")?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let changed = transaction.execute("DELETE FROM media_view", [])? > 0;
                Ok((item_ids, (), changed))
            },
            |_, ()| Ok(()),
        )?;

        Ok(MutationReceipt {
            revision,
            resources: vec![
                resources::RECENTLY_VIEWED.to_string(),
                resources::SIDEBAR.to_string(),
            ],
            item_ids: item_ids.into_iter().map(ItemId).collect(),
        })
    }

    fn record_recent_view_at(
        &self,
        root_item_id: ItemId,
        viewed_at: &str,
    ) -> Result<MutationReceipt, String> {
        let (_, revision, _) = self.transaction_if_changed(
            |transaction| {
                require_root(transaction, root_item_id.0)?;
                let changed = transaction.execute(
                    "INSERT INTO media_view (item_id, viewed_at) VALUES (?1, ?2)
                     ON CONFLICT(item_id) DO UPDATE SET viewed_at = excluded.viewed_at
                     WHERE media_view.viewed_at IS NOT excluded.viewed_at",
                    params![root_item_id.0, viewed_at],
                )? > 0;
                Ok(((), (), changed))
            },
            |_, ()| Ok(()),
        )?;

        Ok(MutationReceipt {
            revision,
            resources: vec![
                resources::RECENTLY_VIEWED.to_string(),
                resources::SIDEBAR.to_string(),
            ],
            item_ids: vec![root_item_id],
        })
    }
}

fn require_root(transaction: &Transaction<'_>, item_id: i64) -> rusqlite::Result<()> {
    transaction
        .query_row(
            "SELECT 1 FROM library_root WHERE item_id = ?1",
            [item_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| invalid(format!("Item {item_id} is not a library root")))?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::Application;
    use crate::app::{resources, ItemId};
    use crate::store::Store;

    fn fixture() -> (tempfile::TempDir, Application) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        store
            .transaction(|transaction| {
                insert_media(transaction, 1, "standalone");
                insert_media(transaction, 11, "member");
                transaction.execute("DELETE FROM library_root WHERE item_id = 11", [])?;
                transaction.execute(
                    "INSERT INTO library_item
                        (item_id, item_key, kind, label, created_at, updated_at)
                     VALUES (10, 'collection', 'collection', 'Album', '2026-01-01', '2026-01-01')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (10, 'active')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (10, 11, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        (directory, Application::new(store))
    }

    fn insert_media(transaction: &rusqlite::Transaction<'_>, item_id: i64, item_key: &str) {
        transaction
            .execute(
                "INSERT INTO library_item
                    (item_id, item_key, kind, created_at, updated_at)
                 VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
                rusqlite::params![item_id, item_key],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                [item_id],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO media_file
                    (file_id, file_hash, mime_type, size_bytes, created_at)
                 VALUES (?1, ?2, 'image/jpeg', 1, '2026-01-01')",
                rusqlite::params![item_id, format!("hash-{item_id}")],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                 VALUES (?1, ?1, '2026-01-01', '2026-01-01')",
                [item_id],
            )
            .unwrap();
    }

    #[test]
    fn records_and_upserts_a_root_view_with_a_compact_receipt() {
        let (_directory, application) = fixture();

        let first = application
            .record_recent_view_at(ItemId(1), "2026-08-23T10:00:00Z")
            .unwrap();
        assert_eq!(first.revision, 2);
        assert_eq!(
            first.resources,
            vec![
                resources::RECENTLY_VIEWED.to_string(),
                resources::SIDEBAR.to_string()
            ]
        );
        assert_eq!(first.item_ids, vec![ItemId(1)]);

        let second = application
            .record_recent_view_at(ItemId(1), "2026-08-23T11:00:00Z")
            .unwrap();
        assert_eq!(second.revision, 3);
        let viewed_at: String = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT viewed_at FROM media_view WHERE item_id = 1",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(viewed_at, "2026-08-23T11:00:00Z");
    }

    #[test]
    fn unchanged_view_does_not_advance_revision() {
        let (_directory, application) = fixture();
        let timestamp = "2026-08-23T10:00:00Z";

        let first = application
            .record_recent_view_at(ItemId(1), timestamp)
            .unwrap();
        let second = application
            .record_recent_view_at(ItemId(1), timestamp)
            .unwrap();

        assert_eq!(first.revision, 2);
        assert_eq!(second.revision, first.revision);
        assert_eq!(application.store().revision().unwrap(), first.revision);
    }

    #[test]
    fn rejects_ids_that_are_not_roots_without_writes() {
        let (_directory, application) = fixture();

        let missing = application.record_recent_view(ItemId(99));
        assert!(missing.is_err());
        let member = application.record_recent_view(ItemId(11));
        assert!(member.is_err());

        let (view_count, revision) = application
            .store()
            .read(|connection| {
                Ok::<_, rusqlite::Error>((
                    connection.query_row("SELECT COUNT(*) FROM media_view", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    crate::store::schema::revision(connection)?,
                ))
            })
            .unwrap();
        assert_eq!(view_count, 0);
        assert_eq!(revision, 1);
    }

    #[test]
    fn accepts_a_collection_root() {
        let (_directory, application) = fixture();

        application
            .record_recent_view_at(ItemId(10), "2026-08-23T10:00:00Z")
            .unwrap();

        let stored: i64 = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT item_id FROM media_view WHERE item_id = 10",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(stored, 10);
    }

    #[test]
    fn clears_recent_history_through_one_canonical_mutation() {
        let (_directory, application) = fixture();
        application
            .record_recent_view_at(ItemId(1), "2026-08-23T10:00:00Z")
            .unwrap();
        application
            .record_recent_view_at(ItemId(10), "2026-08-23T11:00:00Z")
            .unwrap();

        let receipt = application.clear_recent_views().unwrap();
        assert_eq!(receipt.item_ids, vec![ItemId(1), ItemId(10)]);
        assert_eq!(
            receipt.resources,
            vec![resources::RECENTLY_VIEWED, resources::SIDEBAR]
        );
        let count: i64 = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM media_view", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(count, 0);

        application.undo().unwrap();
        let restored: i64 = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM media_view", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(restored, 2);

        let unchanged = application.clear_recent_views().unwrap();
        assert!(unchanged.revision > receipt.revision);
        assert_eq!(unchanged.item_ids, vec![ItemId(1), ItemId(10)]);

        let no_op = application.clear_recent_views().unwrap();
        assert_eq!(no_op.revision, unchanged.revision);
        assert!(no_op.item_ids.is_empty());
    }
}
