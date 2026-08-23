//! Durable execution for replacement-backend media work.
//!
//! The queue owns claim, retry, and completion state. This runtime only
//! performs the side effect and batches the resulting invalidation receipt.

use std::collections::BTreeSet;

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::media_processing_v2::{self, BlobSource};
use crate::store::Store;
use crate::workers_v2::{self, WorkItem, WorkKind, WorkSpec, Worker, DEFAULT_BATCH_SIZE};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrainBatchResult {
    pub claimed: usize,
    pub succeeded: usize,
    pub retried: usize,
    pub receipt: Option<MutationReceipt>,
}

/// Durable executor for one bounded batch of replacement-backend work.
pub struct BackgroundRuntime<'a> {
    application: &'a Application,
    queue: Worker<'a>,
}

impl<'a> BackgroundRuntime<'a> {
    /// Recover work interrupted by a previous process before accepting claims.
    pub fn start(application: &'a Application) -> Result<Self, String> {
        Ok(Self {
            application,
            queue: Worker::start(application.store())?,
        })
    }

    pub fn enqueue(&self, spec: WorkSpec) -> Result<workers_v2::EnqueueResult, String> {
        self.queue.enqueue(spec)
    }

    /// Claim and execute at most `limit` items. The queue applies its own
    /// hard batch bound, so callers cannot accidentally drain unbounded work.
    pub async fn drain_batch(&self, limit: usize) -> Result<DrainBatchResult, String> {
        drain_claimed_batch(self.application, self.application.blobs(), limit).await
    }
}

pub async fn drain_batch(
    application: &Application,
    limit: usize,
) -> Result<DrainBatchResult, String> {
    drain_claimed_batch(application, application.blobs(), limit).await
}

async fn drain_claimed_batch<B: BlobSource>(
    application: &Application,
    blobs: &B,
    limit: usize,
) -> Result<DrainBatchResult, String> {
    let store = application.store();
    let items = workers_v2::claim(store, limit.min(DEFAULT_BATCH_SIZE))?;
    let mut result = DrainBatchResult {
        claimed: items.len(),
        ..DrainBatchResult::default()
    };
    let mut affected_item_ids = BTreeSet::new();
    let mut affected_resources = BTreeSet::from([resources::TASKS.to_string()]);

    for item in items {
        match execute_item(application, blobs, &item).await {
            Ok(operation_receipt) => {
                if item.kind != WorkKind::AiTag {
                    let (item_ids, file_hash) = affected_targets(store, &item)?;
                    affected_item_ids.extend(item_ids);
                    if let Some(file_hash) = file_hash {
                        affected_resources.insert(file_resource(&file_hash));
                    }
                    if item.kind != WorkKind::BlobDelete {
                        affected_resources.insert(resources::LIBRARY.to_string());
                    }
                }
                if let Some(receipt) = operation_receipt {
                    affected_resources.extend(receipt.resources);
                    affected_item_ids.extend(receipt.item_ids.into_iter().map(|id| id.0));
                }
                if !workers_v2::complete(store, item.work_id)? {
                    return Err(format!(
                        "Work item {} succeeded but could not be completed",
                        item.work_id
                    ));
                }
                result.succeeded += 1;
            }
            Err(error) => {
                if workers_v2::fail(store, item.work_id, &error)? {
                    result.retried += 1;
                } else {
                    return Err(format!(
                        "Work item {} failed but could not be requeued: {error}",
                        item.work_id
                    ));
                }
            }
        }
    }

    if result.claimed != 0 {
        result.receipt = Some(MutationReceipt {
            revision: store.revision()?,
            resources: affected_resources.into_iter().collect(),
            item_ids: affected_item_ids.into_iter().map(ItemId).collect(),
        });
    }
    Ok(result)
}

async fn execute_item<B: BlobSource>(
    application: &Application,
    blobs: &B,
    item: &WorkItem,
) -> Result<Option<MutationReceipt>, String> {
    match item.kind {
        WorkKind::BlobDelete => media_processing_v2::execute_blob_delete(blobs, item).map(|_| None),
        WorkKind::Thumbnail | WorkKind::DominantColors | WorkKind::PerceptualHash => {
            media_processing_v2::execute_work(application.store(), blobs, item)
                .await
                .map(|_| None)
        }
        WorkKind::AiTag => {
            let media_item_id = item
                .media_item_id
                .ok_or_else(|| "AI tagging work is missing its media item ID".to_string())?;
            crate::ai_runtime_v2::execute_ai_tagging(application, ItemId(media_item_id))
                .await
                .map(|result| result.receipt)
        }
    }
}

fn affected_targets(store: &Store, item: &WorkItem) -> Result<(Vec<i64>, Option<String>), String> {
    if item.kind == WorkKind::BlobDelete {
        return Ok((Vec::new(), item.file_hash.clone()));
    }

    let file_id = item.file_id;
    let media_item_id = item.media_item_id;
    store.read(|connection| {
        let mut item_ids = BTreeSet::new();
        let mut file_hash = None;
        if let Some(media_item_id) = media_item_id {
            item_ids.insert(media_item_id);
        }

        if let Some(file_id) = file_id {
            file_hash = connection
                .query_row(
                    "SELECT file_hash FROM media_file WHERE file_id = ?1",
                    [file_id],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            let mut statement = connection
                .prepare("SELECT item_id FROM media_asset WHERE file_id = ?1 ORDER BY item_id")?;
            let rows = statement.query_map([file_id], |row| row.get::<_, i64>(0))?;
            for row in rows {
                item_ids.insert(row?);
            }
        }

        Ok((item_ids.into_iter().collect(), file_hash))
    })
}

fn file_resource(file_hash: &str) -> String {
    format!("file:{file_hash}")
}

#[cfg(test)]
mod tests {
    use super::{drain_claimed_batch, DrainBatchResult};
    use crate::app::{resources, Application, ItemId};
    use crate::media_processing_v2::BlobSource;
    use crate::store::Store;
    use crate::workers_v2::{self, WorkKind, WorkSpec};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T00:00:00Z";

    #[derive(Clone, Default)]
    struct FakeBlobSource {
        deleted: Arc<Mutex<Vec<String>>>,
        fail_delete: bool,
    }

    impl BlobSource for FakeBlobSource {
        fn original_path(&self, _file_hash: &str, _mime_type: &str) -> Result<PathBuf, String> {
            Ok(PathBuf::from("/unused/original"))
        }

        fn thumbnail_exists(&self, _file_hash: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn write_thumbnail(
            &self,
            _file_hash: &str,
            _bytes: &[u8],
            _extension: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn delete(&self, file_hash: &str) -> Result<(), String> {
            if self.fail_delete {
                return Err("fake delete failure".to_string());
            }
            self.deleted.lock().unwrap().push(file_hash.to_string());
            Ok(())
        }
    }

    fn fixture() -> (TempDir, Application, FakeBlobSource) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                     (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (7, 'hash-7', 'image/png', 1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                     VALUES (9, 'item-9', 'media', ?1, ?1),
                            (10, 'item-10', 'media', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (9, 7, ?1, ?1), (10, 7, ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (9, 'active'), (10, 'active')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let application = Application::new(Arc::new(store));
        (directory, application, FakeBlobSource::default())
    }

    fn enqueue(store: &Store, spec: WorkSpec) -> i64 {
        workers_v2::enqueue_at(store, spec, NOW).unwrap().work_id
    }

    #[tokio::test]
    async fn derivative_batch_is_bounded_and_receipt_is_exact() {
        let (_directory, application, blobs) = fixture();
        enqueue(application.store(), WorkSpec::file(7, WorkKind::Thumbnail));
        enqueue(application.store(), WorkSpec::blob("orphan-hash"));

        let result = drain_claimed_batch(&application, &blobs, 1).await.unwrap();

        assert_eq!(
            result,
            DrainBatchResult {
                claimed: 1,
                succeeded: 1,
                retried: 0,
                receipt: Some(crate::app::MutationReceipt {
                    revision: 5,
                    resources: vec![
                        "file:hash-7".to_string(),
                        resources::LIBRARY.to_string(),
                        resources::TASKS.to_string(),
                    ],
                    item_ids: vec![ItemId(9), ItemId(10)],
                }),
            }
        );
        assert_eq!(
            application
                .store()
                .read(|connection| {
                    connection.query_row(
                        "SELECT status FROM work_item WHERE work_id = 2",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                })
                .unwrap(),
            "pending"
        );
    }

    #[tokio::test]
    async fn blob_delete_and_unconfigured_ai_complete() {
        let (_directory, application, blobs) = fixture();
        enqueue(application.store(), WorkSpec::blob("orphan-hash"));
        let ai_id = enqueue(
            application.store(),
            WorkSpec::media_only(9, WorkKind::AiTag),
        );
        let deleted = blobs.deleted.clone();

        let result = drain_claimed_batch(&application, &blobs, 2).await.unwrap();

        assert_eq!(result.claimed, 2);
        assert_eq!(result.succeeded, 2);
        assert_eq!(result.retried, 0);
        assert_eq!(
            result.receipt.unwrap().resources,
            vec!["file:orphan-hash".to_string(), resources::TASKS.to_string()]
        );
        assert_eq!(deleted.lock().unwrap().as_slice(), &["orphan-hash"]);

        let ai = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM work_item WHERE work_id = ?1",
                    [ai_id],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(ai, 0);
    }

    #[tokio::test]
    async fn blob_failure_is_retried_and_empty_batch_has_no_receipt() {
        let (_directory, application, _) = fixture();
        enqueue(application.store(), WorkSpec::blob("orphan-hash"));
        let failing = FakeBlobSource {
            fail_delete: true,
            ..FakeBlobSource::default()
        };

        let result = drain_claimed_batch(&application, &failing, 1)
            .await
            .unwrap();
        assert_eq!(result.claimed, 1);
        assert_eq!(result.succeeded, 0);
        assert_eq!(result.retried, 1);
        assert_eq!(result.receipt.unwrap().resources, vec![resources::TASKS]);

        let empty = drain_claimed_batch(&application, &failing, 1)
            .await
            .unwrap();
        assert_eq!(empty, DrainBatchResult::default());
    }
}
