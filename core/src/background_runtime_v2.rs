//! Durable execution for replacement-backend media work.
//!
//! The queue owns claim, retry, and completion state. This runtime only
//! performs the side effect and batches the resulting invalidation receipt.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::media_processing_v2::{self, BlobSource, DerivativeOutcome};
use crate::store::Store;
use crate::workers_v2::{self, WorkItem, WorkKind, DEFAULT_BATCH_SIZE};

type WorkExecution = Result<(Option<MutationReceipt>, Option<DerivativeOutcome>), String>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrainBatchResult {
    pub claimed: usize,
    pub succeeded: usize,
    pub retried: usize,
    pub receipt: Option<MutationReceipt>,
    pub thumbnail_file_hashes: Vec<String>,
    pub dominant_color_changes: Vec<DominantColorChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DominantColorChange {
    pub file_hash: String,
    pub dominant_color_hex: Option<String>,
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
    let mut affected_files: BTreeMap<i64, (Vec<i64>, Option<String>)> = BTreeMap::new();
    let mut thumbnail_file_hashes = BTreeSet::new();
    let mut dominant_color_changes = BTreeMap::new();
    let mut duplicate_analysis_touched = false;
    let mut completed_work_ids = Vec::new();

    let mut groups: Vec<Vec<WorkItem>> = Vec::new();
    let mut group_indexes: HashMap<(u8, i64), usize> = HashMap::new();
    for item in items {
        let key = if item.kind == WorkKind::AiTag {
            (0_u8, 0_i64)
        } else if is_derivative(item.kind) {
            (1_u8, item.file_id.unwrap_or(item.work_id))
        } else {
            (2_u8, item.work_id)
        };
        if let Some(index) = group_indexes.get(&key).copied() {
            groups[index].push(item);
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(vec![item]);
        }
    }

    for group in groups {
        let executions: Vec<WorkExecution> = if group.iter().all(|item| is_derivative(item.kind)) {
            media_processing_v2::execute_work_group(store, blobs, &group)
                .await
                .into_iter()
                .map(|result| result.map(|outcome| (None, Some(outcome))))
                .collect::<Vec<_>>()
        } else if group.iter().all(|item| item.kind == WorkKind::AiTag) {
            let media_item_ids = group
                .iter()
                .map(|item| {
                    item.media_item_id
                        .map(ItemId)
                        .ok_or_else(|| "AI tagging work is missing its media item ID".to_string())
                })
                .collect::<Result<Vec<_>, String>>();
            match media_item_ids {
                Ok(media_item_ids) => {
                    match crate::ai_runtime_v2::execute_ai_tagging_batch(
                        application,
                        &media_item_ids,
                    )
                    .await
                    {
                        Ok(results) => results
                            .into_iter()
                            .map(|result| Ok((result.receipt, None)))
                            .collect(),
                        Err(error) => (0..group.len()).map(|_| Err(error.clone())).collect(),
                    }
                }
                Err(error) => (0..group.len()).map(|_| Err(error.clone())).collect(),
            }
        } else {
            vec![execute_non_derivative(application, blobs, &group[0])
                .await
                .map(|receipt| (receipt, None))]
        };
        for (item, execution) in group.into_iter().zip(executions) {
            match execution {
                Ok((operation_receipt, derivative_outcome)) => {
                    if item.kind == WorkKind::BlobDelete {
                        if let Some(file_hash) = &item.file_hash {
                            affected_resources.insert(format!("file:{file_hash}"));
                        }
                    }
                    let publishes_derivative =
                        derivative_outcome.is_some_and(|outcome| match item.kind {
                            WorkKind::Thumbnail => outcome.thumbnail_written,
                            WorkKind::DominantColors => outcome.dominant_colors_written,
                            _ => false,
                        });
                    if publishes_derivative {
                        let (item_ids, file_hash) = if let Some(file_id) = item.file_id {
                            if let Some(affected) = affected_files.get(&file_id) {
                                affected.clone()
                            } else {
                                let affected = affected_targets(store, &item)?;
                                affected_files.insert(file_id, affected.clone());
                                affected
                            }
                        } else {
                            affected_targets(store, &item)?
                        };
                        affected_item_ids.extend(item_ids);
                        if let Some(file_hash) = file_hash {
                            match item.kind {
                                WorkKind::Thumbnail => {
                                    thumbnail_file_hashes.insert(file_hash);
                                }
                                WorkKind::DominantColors => {
                                    let dominant_color_hex =
                                        dominant_color_for_file(store, item.file_id)?;
                                    dominant_color_changes.insert(
                                        file_hash.clone(),
                                        DominantColorChange {
                                            file_hash,
                                            dominant_color_hex,
                                        },
                                    );
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    if let Some(receipt) = operation_receipt {
                        affected_resources.extend(receipt.resources);
                        affected_item_ids.extend(receipt.item_ids.into_iter().map(|id| id.0));
                    }
                    duplicate_analysis_touched |= item.kind == WorkKind::PerceptualHash
                        || derivative_outcome
                            .is_some_and(|outcome| outcome.perceptual_hash_written);
                    completed_work_ids.push(item.work_id);
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
    }

    if !completed_work_ids.is_empty() {
        let completed = workers_v2::complete_many(store, &completed_work_ids)?;
        if completed != completed_work_ids.len() {
            return Err(format!(
                "{} work items succeeded but only {completed} could be completed",
                completed_work_ids.len()
            ));
        }
    }

    if duplicate_analysis_touched {
        let more_hash_work: bool = store.read(|connection| {
            connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM work_item
                    WHERE work_type = 'perceptual_hash'
                      AND status IN ('pending', 'running')
                )",
                [],
                |row| row.get(0),
            )
        })?;
        if !more_hash_work {
            let scan = crate::duplicates_v2::scan(application, 10)?;
            affected_resources.extend(scan.receipt.resources);
            affected_item_ids.extend(scan.affected_item_ids.into_iter().map(|id| id.0));
        }
    }

    if result.claimed != 0 {
        result.receipt = Some(MutationReceipt {
            revision: store.revision()?,
            resources: affected_resources.into_iter().collect(),
            item_ids: affected_item_ids.into_iter().map(ItemId).collect(),
        });
    }
    result.thumbnail_file_hashes = thumbnail_file_hashes.into_iter().collect();
    result.dominant_color_changes = dominant_color_changes.into_values().collect();
    Ok(result)
}

fn is_derivative(kind: WorkKind) -> bool {
    matches!(
        kind,
        WorkKind::Thumbnail | WorkKind::DominantColors | WorkKind::PerceptualHash
    )
}

async fn execute_non_derivative<B: BlobSource>(
    application: &Application,
    blobs: &B,
    item: &WorkItem,
) -> Result<Option<MutationReceipt>, String> {
    match item.kind {
        WorkKind::BlobDelete => {
            media_processing_v2::execute_blob_delete(application.store(), blobs, item).map(|_| None)
        }
        WorkKind::Thumbnail | WorkKind::DominantColors | WorkKind::PerceptualHash => {
            Err("derivative work must execute through its physical-file group".to_string())
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

fn dominant_color_for_file(store: &Store, file_id: Option<i64>) -> Result<Option<String>, String> {
    let Some(file_id) = file_id else {
        return Ok(None);
    };
    store.read(|connection| {
        connection.query_row(
            "SELECT dominant_color_hex FROM media_file WHERE file_id = ?1",
            [file_id],
            |row| row.get(0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{drain_claimed_batch, DrainBatchResult};
    use crate::app::{resources, Application, ItemId};
    use crate::media_processing_v2::BlobSource;
    use crate::store::Store;
    use crate::workers_v2::{self, WorkKind, WorkSpec};
    use image::{DynamicImage, RgbImage};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    #[derive(Clone)]
    struct CountingBlobSource {
        original: PathBuf,
        original_lookups: Arc<AtomicUsize>,
        thumbnail_writes: Arc<AtomicUsize>,
    }

    impl BlobSource for CountingBlobSource {
        fn original_path(&self, _file_hash: &str, _mime_type: &str) -> Result<PathBuf, String> {
            self.original_lookups.fetch_add(1, Ordering::Relaxed);
            Ok(self.original.clone())
        }

        fn thumbnail_exists(&self, _file_hash: &str) -> Result<bool, String> {
            Ok(false)
        }

        fn write_thumbnail(
            &self,
            _file_hash: &str,
            _bytes: &[u8],
            _extension: &str,
        ) -> Result<(), String> {
            self.thumbnail_writes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn delete(&self, _file_hash: &str) -> Result<(), String> {
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
                     VALUES (7, '7777777777777777777777777777777777777777777777777777777777777777', 'image/png', 1, ?1)",
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
    async fn completed_thumbnail_does_not_publish_a_false_change() {
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
                    revision: 6,
                    resources: vec![resources::TASKS.to_string()],
                    item_ids: vec![],
                }),
                thumbnail_file_hashes: vec![],
                dominant_color_changes: vec![],
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
    async fn derivative_rows_for_one_file_share_media_preparation() {
        let (directory, application, _) = fixture();
        let original = directory.path().join("source.png");
        let image =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(128, 128, image::Rgb([24, 96, 180])));
        image.save(&original).unwrap();
        let thumbnail = std::fs::read(&original).unwrap();
        application
            .blobs()
            .write_thumbnail(
                "7777777777777777777777777777777777777777777777777777777777777777",
                &thumbnail,
                "png",
            )
            .unwrap();
        application
            .blobs()
            .write_thumbnail(
                "8888888888888888888888888888888888888888888888888888888888888888",
                &thumbnail,
                "png",
            )
            .unwrap();
        let existing_hash =
            crate::media_processing::compute_phash_base64_from_image(&image).unwrap();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, perceptual_hash, created_at)
                     VALUES (8, '8888888888888888888888888888888888888888888888888888888888888888', 'image/png', 1, ?1, ?2)",
                    rusqlite::params![existing_hash, NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (11, 'item-11', 'media', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (11, 8, ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (11, 'active')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let original_lookups = Arc::new(AtomicUsize::new(0));
        let thumbnail_writes = Arc::new(AtomicUsize::new(0));
        let blobs = CountingBlobSource {
            original,
            original_lookups: Arc::clone(&original_lookups),
            thumbnail_writes: Arc::clone(&thumbnail_writes),
        };
        enqueue(application.store(), WorkSpec::file(7, WorkKind::Thumbnail));
        enqueue(
            application.store(),
            WorkSpec::file(7, WorkKind::DominantColors),
        );
        enqueue(
            application.store(),
            WorkSpec::file(7, WorkKind::PerceptualHash),
        );

        let result = drain_claimed_batch(&application, &blobs, 3).await.unwrap();

        assert_eq!(result.claimed, 3);
        assert_eq!(result.succeeded, 3);
        assert_eq!(result.retried, 0);
        assert_eq!(
            result.thumbnail_file_hashes,
            ["7777777777777777777777777777777777777777777777777777777777777777"]
        );
        assert_eq!(result.dominant_color_changes.len(), 1);
        assert_eq!(
            result.dominant_color_changes[0].file_hash,
            "7777777777777777777777777777777777777777777777777777777777777777"
        );
        assert!(result.dominant_color_changes[0]
            .dominant_color_hex
            .is_some());
        let receipt = result.receipt.as_ref().unwrap();
        assert_eq!(
            receipt.resources,
            [
                resources::DUPLICATES.to_string(),
                resources::SIDEBAR.to_string(),
                resources::TASKS.to_string()
            ]
        );
        assert_eq!(receipt.item_ids, [ItemId(9), ItemId(10), ItemId(11)]);
        assert_eq!(original_lookups.load(Ordering::Relaxed), 1);
        assert_eq!(thumbnail_writes.load(Ordering::Relaxed), 1);
        application
            .store()
            .read(|connection| {
                let derived: (i64, i64) = connection.query_row(
                    "SELECT dominant_palette_blob IS NOT NULL, perceptual_hash IS NOT NULL
                     FROM media_file WHERE file_id = 7",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(derived, (1, 1));
                let queued: i64 =
                    connection.query_row("SELECT COUNT(*) FROM work_item", [], |row| row.get(0))?;
                assert_eq!(queued, 0);
                let duplicates: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(duplicates, 1);
                Ok(())
            })
            .unwrap();
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
    async fn stale_blob_delete_cannot_remove_a_referenced_file() {
        let (_directory, application, blobs) = fixture();
        enqueue(
            application.store(),
            WorkSpec::blob("7777777777777777777777777777777777777777777777777777777777777777"),
        );
        let deleted = blobs.deleted.clone();

        let result = drain_claimed_batch(&application, &blobs, 1).await.unwrap();

        assert_eq!(result.claimed, 1);
        assert_eq!(result.succeeded, 1);
        assert!(deleted.lock().unwrap().is_empty());
        assert_eq!(
            application
                .store()
                .read(|connection| {
                    connection.query_row("SELECT COUNT(*) FROM work_item", [], |row| {
                        row.get::<_, i64>(0)
                    })
                })
                .unwrap(),
            0
        );
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
