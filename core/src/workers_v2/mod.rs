//! One durable worker path for derivative and cleanup work.
//!
//! SQLite owns the queue. The executor only performs the side effect; every
//! queue state transition is a small Store transaction.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::store::Store;

pub const DEFAULT_BATCH_SIZE: usize = 8;
const BASE_BACKOFF_SECONDS: i64 = 1;
const MAX_BACKOFF_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    Thumbnail,
    DominantColors,
    PerceptualHash,
    BlobDelete,
    AiTag,
}

impl WorkKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::DominantColors => "dominant_colors",
            Self::PerceptualHash => "perceptual_hash",
            Self::BlobDelete => "blob_delete",
            Self::AiTag => "ai_tag",
        }
    }

    fn from_str(value: &str) -> rusqlite::Result<Self> {
        match value {
            "thumbnail" => Ok(Self::Thumbnail),
            "dominant_colors" => Ok(Self::DominantColors),
            "perceptual_hash" => Ok(Self::PerceptualHash),
            "blob_delete" => Ok(Self::BlobDelete),
            "ai_tag" => Ok(Self::AiTag),
            _ => Err(rusqlite::Error::InvalidColumnType(
                0,
                "work_type".to_string(),
                rusqlite::types::Type::Text,
            )),
        }
    }

    /// Higher values are claimed first. Keep this persisted rather than
    /// deriving queue order in every claim so the ready index can satisfy the
    /// scheduler without a work-type sort.
    const fn priority(self) -> i64 {
        match self {
            Self::Thumbnail => 500,
            Self::DominantColors => 400,
            Self::PerceptualHash => 300,
            Self::AiTag => 200,
            Self::BlobDelete => 100,
        }
    }
}

pub(crate) fn enqueue_blob_delete_in(
    transaction: &Transaction<'_>,
    file_hash: &str,
    now: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO work_item (
             file_hash, work_type, priority, status, attempt_count,
             available_at, created_at, updated_at
         ) VALUES (?1, 'blob_delete', ?2, 'pending', 0, ?3, ?3, ?3)
         ON CONFLICT(file_hash, work_type) WHERE file_hash IS NOT NULL DO NOTHING",
        params![file_hash, WorkKind::BlobDelete.priority(), now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkStatus {
    Pending,
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkSpec {
    pub media_item_id: Option<i64>,
    pub file_id: Option<i64>,
    pub file_hash: Option<String>,
    pub kind: WorkKind,
}

impl WorkSpec {
    pub fn media(media_item_id: i64, file_id: i64, kind: WorkKind) -> Self {
        Self {
            media_item_id: Some(media_item_id),
            file_id: Some(file_id),
            file_hash: None,
            kind,
        }
    }

    pub fn file(file_id: i64, kind: WorkKind) -> Self {
        Self {
            media_item_id: None,
            file_id: Some(file_id),
            file_hash: None,
            kind,
        }
    }

    pub fn media_only(media_item_id: i64, kind: WorkKind) -> Self {
        Self {
            media_item_id: Some(media_item_id),
            file_id: None,
            file_hash: None,
            kind,
        }
    }

    pub fn blob(file_hash: impl Into<String>) -> Self {
        Self {
            media_item_id: None,
            file_id: None,
            file_hash: Some(file_hash.into()),
            kind: WorkKind::BlobDelete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub work_id: i64,
    pub media_item_id: Option<i64>,
    pub file_id: Option<i64>,
    pub file_hash: Option<String>,
    pub kind: WorkKind,
    pub status: WorkStatus,
    pub attempt_count: i64,
    pub available_at: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnqueueResult {
    pub work_id: i64,
    pub inserted: bool,
}

pub fn enqueue(store: &Store, spec: WorkSpec) -> Result<EnqueueResult, String> {
    enqueue_at(store, spec, &Utc::now().to_rfc3339())
}

pub fn enqueue_at(store: &Store, spec: WorkSpec, now: &str) -> Result<EnqueueResult, String> {
    if spec.media_item_id.is_none() && spec.file_id.is_none() && spec.file_hash.is_none() {
        return Err("work item needs a media item or file target".to_string());
    }

    let (result, _, _) = store.transaction_if_changed_background(|transaction| {
        let existing = find_work_id(transaction, &spec)?;
        if let Some(work_id) = existing {
            return Ok((
                EnqueueResult {
                    work_id,
                    inserted: false,
                },
                false,
            ));
        }

        transaction.execute(
            "INSERT INTO work_item (
                 media_item_id, file_id, file_hash, work_type, priority, status, attempt_count,
                 available_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?6, ?6, ?6)",
            params![
                spec.media_item_id,
                spec.file_id,
                spec.file_hash,
                spec.kind.as_str(),
                spec.kind.priority(),
                now
            ],
        )?;
        Ok((
            EnqueueResult {
                work_id: transaction.last_insert_rowid(),
                inserted: true,
            },
            true,
        ))
    })?;
    Ok(result)
}

pub fn reset_running(store: &Store) -> Result<usize, String> {
    reset_running_at(store, &Utc::now().to_rfc3339())
}

/// Remove thumbnail jobs that can never be claimed under the collection-cover
/// policy. Missing member thumbnails are queued on demand when a viewport or
/// collection editor actually requests them.
pub fn prune_deferred_thumbnail_work(store: &Store) -> Result<usize, String> {
    let (count, _, _) = store.transaction_if_changed_background(|transaction| {
        let count = transaction.execute(
            "DELETE FROM work_item
             WHERE status = 'pending'
               AND work_type = 'thumbnail'
               AND media_item_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM library_root
                   WHERE library_root.item_id = work_item.media_item_id
               )
               AND NOT EXISTS (
                   SELECT 1
                   FROM library_root collection_root
                   JOIN library_item collection
                     ON collection.item_id = collection_root.item_id
                   WHERE collection.kind = 'collection'
                     AND collection.cover_media_item_id = work_item.media_item_id
               )",
            [],
        )?;
        Ok((count, count != 0))
    })?;
    Ok(count)
}

pub fn reset_running_at(store: &Store, now: &str) -> Result<usize, String> {
    let (count, _, _) = store.transaction_if_changed_background(|transaction| {
        let count = transaction.execute(
            "UPDATE work_item
             SET status = 'pending', available_at = ?1, updated_at = ?1
             WHERE status = 'running'",
            [now],
        )?;
        Ok((count, count != 0))
    })?;
    Ok(count)
}

pub fn claim(store: &Store, limit: usize) -> Result<Vec<WorkItem>, String> {
    claim_at(store, limit, &Utc::now().to_rfc3339())
}

pub fn claim_at(store: &Store, limit: usize, now: &str) -> Result<Vec<WorkItem>, String> {
    let limit = limit.min(DEFAULT_BATCH_SIZE);
    let (items, _, _) = store.transaction_if_changed_background(|transaction| {
        let mut statement = transaction.prepare(
            "SELECT work_id, media_item_id, file_id, file_hash, work_type, attempt_count,
                    available_at, last_error
             FROM work_item
             WHERE status = 'pending' AND available_at <= ?1
               AND (
                   work_type <> 'thumbnail'
                   OR media_item_id IS NULL
                   OR EXISTS (
                       SELECT 1 FROM library_root
                       WHERE library_root.item_id = work_item.media_item_id
                   )
                   OR EXISTS (
                       SELECT 1
                       FROM library_root collection_root
                       JOIN library_item collection
                         ON collection.item_id = collection_root.item_id
                       WHERE collection.kind = 'collection'
                         AND work_item.media_item_id = (
                             SELECT member.media_item_id
                             FROM collection_member member
                             WHERE member.collection_id = collection.item_id
                             ORDER BY member.position_rank, member.media_item_id
                             LIMIT 1
                         )
                   )
               )
             ORDER BY priority DESC, available_at,
                      work_id
             LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![now, limit as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    WorkKind::from_str(&row.get::<_, String>(4)?)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let work_ids = rows.iter().map(|row| row.0).collect::<Vec<_>>();
        if !work_ids.is_empty() {
            let encoded = serde_json::to_string(&work_ids)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            transaction.execute(
                "UPDATE work_item
                 SET status = 'running', updated_at = ?1
                 WHERE status = 'pending'
                   AND work_id IN (SELECT CAST(value AS INTEGER) FROM json_each(?2))",
                params![now, encoded],
            )?;
        }
        let items = rows
            .into_iter()
            .map(
                |(
                    work_id,
                    media_item_id,
                    file_id,
                    file_hash,
                    kind,
                    attempt_count,
                    available_at,
                    last_error,
                )| WorkItem {
                    work_id,
                    media_item_id,
                    file_id,
                    file_hash,
                    kind,
                    status: WorkStatus::Running,
                    attempt_count,
                    available_at,
                    last_error,
                },
            )
            .collect::<Vec<_>>();
        let changed = !items.is_empty();
        Ok((items, changed))
    })?;
    Ok(items)
}

pub fn complete(store: &Store, work_id: i64) -> Result<bool, String> {
    let (changed, _, _) = store.transaction_if_changed_background(|transaction| {
        let count = transaction.execute(
            "DELETE FROM work_item WHERE work_id = ?1 AND status = 'running'",
            [work_id],
        )?;
        Ok((count == 1, count == 1))
    })?;
    Ok(changed)
}

pub fn complete_many(store: &Store, work_ids: &[i64]) -> Result<usize, String> {
    if work_ids.is_empty() {
        return Ok(0);
    }
    let (completed, _, _) = store.transaction_if_changed_background(|transaction| {
        let encoded = serde_json::to_string(work_ids)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let completed = transaction.execute(
            "DELETE FROM work_item
             WHERE status = 'running'
               AND work_id IN (SELECT CAST(value AS INTEGER) FROM json_each(?1))",
            [encoded],
        )?;
        Ok((completed, completed != 0))
    })?;
    Ok(completed)
}

pub fn fail(store: &Store, work_id: i64, error: &str) -> Result<bool, String> {
    fail_at(store, work_id, &Utc::now(), error)
}

pub fn fail_at(
    store: &Store,
    work_id: i64,
    now: &DateTime<Utc>,
    error: &str,
) -> Result<bool, String> {
    let now_text = now.to_rfc3339();
    let (changed, _, _) = store.transaction_if_changed_background(|transaction| {
        let attempt_count: Option<i64> = transaction
            .query_row(
                "SELECT attempt_count FROM work_item
                 WHERE work_id = ?1 AND status = 'running'",
                [work_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(attempt_count) = attempt_count else {
            return Ok((false, false));
        };
        let next_attempt = attempt_count.saturating_add(1);
        let available_at = (*now + retry_delay(next_attempt)).to_rfc3339();
        transaction.execute(
            "UPDATE work_item
             SET status = 'pending', attempt_count = ?1, available_at = ?2,
                 last_error = ?3, updated_at = ?4
             WHERE work_id = ?5 AND status = 'running'",
            params![next_attempt, available_at, error, now_text, work_id],
        )?;
        Ok((true, true))
    })?;
    Ok(changed)
}

fn find_work_id(transaction: &Transaction<'_>, spec: &WorkSpec) -> rusqlite::Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT work_id FROM work_item
             WHERE media_item_id IS ?1 AND file_id IS ?2 AND file_hash IS ?3
               AND work_type = ?4",
            params![
                spec.media_item_id,
                spec.file_id,
                spec.file_hash,
                spec.kind.as_str()
            ],
            |row| row.get(0),
        )
        .optional()
}

fn retry_delay(attempt_count: i64) -> Duration {
    let exponent = attempt_count.saturating_sub(1).clamp(0, 8) as u32;
    let seconds = BASE_BACKOFF_SECONDS
        .saturating_mul(1_i64 << exponent)
        .min(MAX_BACKOFF_SECONDS);
    Duration::seconds(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use rusqlite::params;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T00:00:00Z";

    fn store() -> (TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file (
                         file_id, file_hash, mime_type, size_bytes, created_at
                     ) VALUES (7, 'hash-7', 'image/png', 1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (9, 'item-9', 'media', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (
                         item_id, file_id, imported_at, updated_at
                     ) VALUES (9, 7, ?1, ?1)",
                    [NOW],
                )?;
                Ok(())
            })
            .unwrap();
        (directory, store)
    }

    fn count(store: &Store) -> i64 {
        store
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM work_item", [], |row| row.get(0))
            })
            .unwrap()
    }

    #[test]
    fn enqueue_is_idempotent() {
        let (_directory, store) = store();
        let spec = WorkSpec::media(9, 7, WorkKind::Thumbnail);
        let first = enqueue_at(&store, spec.clone(), NOW).unwrap();
        let second = enqueue_at(&store, spec, "2026-01-02T00:00:00Z").unwrap();

        assert!(first.inserted);
        assert_eq!(first.work_id, second.work_id);
        assert!(!second.inserted);
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn deferred_collection_member_thumbnail_jobs_are_pruned() {
        let (_directory, store) = store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (10, 'collection-10', 'collection', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (10, 'inbox')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (11, 'item-11', 'media', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (11, 7, ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (10, 9, 1000), (10, 11, 2000)",
                    [],
                )?;
                transaction.execute(
                    "UPDATE library_item SET cover_media_item_id = 9 WHERE item_id = 10",
                    [],
                )?;
                for media_item_id in [9_i64, 11] {
                    transaction.execute(
                        "INSERT INTO work_item (
                             media_item_id, file_id, work_type, status, attempt_count,
                             available_at, created_at, updated_at
                         ) VALUES (?1, 7, 'thumbnail', 'pending', 0, ?2, ?2, ?2)",
                        params![media_item_id, NOW],
                    )?;
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(prune_deferred_thumbnail_work(&store).unwrap(), 1);
        let remaining = store
            .read(|connection| {
                connection.query_row(
                    "SELECT media_item_id FROM work_item WHERE work_type = 'thumbnail'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(remaining, 9);
    }

    #[test]
    fn startup_recovers_running_items() {
        let (_directory, store) = store();
        let work_id = enqueue_at(&store, WorkSpec::media(9, 7, WorkKind::DominantColors), NOW)
            .unwrap()
            .work_id;
        assert_eq!(claim_at(&store, 1, NOW).unwrap().len(), 1);

        assert_eq!(reset_running_at(&store, "2026-01-01T00:01:00Z").unwrap(), 1);
        let row = store
            .read(|connection| {
                connection.query_row(
                    "SELECT status, available_at FROM work_item WHERE work_id = ?1",
                    [work_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
            })
            .unwrap();
        assert_eq!(
            row,
            ("pending".to_string(), "2026-01-01T00:01:00Z".to_string())
        );
    }

    #[test]
    fn claim_is_bounded_and_marks_items_running() {
        let (_directory, store) = store();
        for kind in [
            WorkKind::Thumbnail,
            WorkKind::DominantColors,
            WorkKind::PerceptualHash,
        ] {
            enqueue_at(&store, WorkSpec::media(9, 7, kind), NOW).unwrap();
        }

        let claimed = claim_at(&store, 2, NOW).unwrap();
        assert_eq!(claimed.len(), 2);
        assert!(claimed
            .iter()
            .all(|item| item.status == WorkStatus::Running));
        assert_eq!(count_status(&store, "running"), 2);
        assert_eq!(count_status(&store, "pending"), 1);
    }

    #[test]
    fn user_visible_derivatives_outrank_older_background_analysis() {
        let (_directory, store) = store();
        enqueue_at(
            &store,
            WorkSpec::file(7, WorkKind::PerceptualHash),
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        enqueue_at(
            &store,
            WorkSpec::file(7, WorkKind::DominantColors),
            "2026-01-01T00:01:00Z",
        )
        .unwrap();
        enqueue_at(
            &store,
            WorkSpec::file(7, WorkKind::Thumbnail),
            "2026-01-01T00:02:00Z",
        )
        .unwrap();

        let thumbnail = claim_at(&store, 1, "2026-01-01T00:03:00Z").unwrap();
        assert_eq!(thumbnail[0].kind, WorkKind::Thumbnail);
        assert!(complete(&store, thumbnail[0].work_id).unwrap());

        let colors = claim_at(&store, 1, "2026-01-01T00:03:00Z").unwrap();
        assert_eq!(colors[0].kind, WorkKind::DominantColors);
        assert!(complete(&store, colors[0].work_id).unwrap());

        let phash = claim_at(&store, 1, "2026-01-01T00:03:00Z").unwrap();
        assert_eq!(phash[0].kind, WorkKind::PerceptualHash);
    }

    #[test]
    fn enqueue_persists_numeric_priority_used_by_the_ready_index() {
        let (_directory, store) = store();
        let colors = enqueue_at(&store, WorkSpec::file(7, WorkKind::DominantColors), NOW).unwrap();
        let phash = enqueue_at(&store, WorkSpec::file(7, WorkKind::PerceptualHash), NOW).unwrap();
        let priorities = store
            .read(|connection| {
                let mut statement = connection.prepare(
                    "SELECT work_id, priority FROM work_item
                     WHERE work_id IN (?1, ?2) ORDER BY priority DESC",
                )?;
                let rows = statement
                    .query_map(params![colors.work_id, phash.work_id], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            priorities,
            vec![(colors.work_id, 400), (phash.work_id, 300)]
        );
    }

    #[test]
    fn collection_members_only_generate_the_visible_cover_thumbnail_in_background() {
        let (_directory, store) = store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file (
                         file_id, file_hash, mime_type, size_bytes, created_at
                     ) VALUES (8, 'hash-8', 'image/png', 1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (10, 'item-10', 'media', ?1, ?1),
                              (20, 'collection-20', 'collection', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (10, 8, ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (20, 9, 1), (20, 10, 2)",
                    [],
                )?;
                transaction.execute(
                    "UPDATE library_item SET cover_media_item_id = 9 WHERE item_id = 20",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (20, 'active')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        enqueue_at(&store, WorkSpec::media(9, 7, WorkKind::Thumbnail), NOW).unwrap();
        enqueue_at(&store, WorkSpec::media(10, 8, WorkKind::Thumbnail), NOW).unwrap();
        enqueue_at(
            &store,
            WorkSpec::media(10, 8, WorkKind::DominantColors),
            NOW,
        )
        .unwrap();

        let claimed = claim_at(&store, 8, NOW).unwrap();
        assert!(claimed
            .iter()
            .any(|item| { item.media_item_id == Some(9) && item.kind == WorkKind::Thumbnail }));
        assert!(claimed.iter().any(|item| {
            item.media_item_id == Some(10) && item.kind == WorkKind::DominantColors
        }));
        assert!(!claimed
            .iter()
            .any(|item| { item.media_item_id == Some(10) && item.kind == WorkKind::Thumbnail }));
        assert_eq!(count_status(&store, "pending"), 1);
    }

    #[test]
    fn success_deletes_claimed_item() {
        let (_directory, store) = store();
        let work_id = enqueue_at(&store, WorkSpec::file(7, WorkKind::BlobDelete), NOW)
            .unwrap()
            .work_id;
        claim_at(&store, 1, NOW).unwrap();

        assert!(complete(&store, work_id).unwrap());
        assert_eq!(count(&store), 0);
    }

    #[test]
    fn failure_requeues_with_bounded_exponential_backoff() {
        let (_directory, store) = store();
        let work_id = enqueue_at(&store, WorkSpec::media_only(9, WorkKind::AiTag), NOW)
            .unwrap()
            .work_id;
        claim_at(&store, 1, NOW).unwrap();

        let first_time = DateTime::parse_from_rfc3339(NOW)
            .unwrap()
            .with_timezone(&Utc);
        assert!(fail_at(&store, work_id, &first_time, "first failure").unwrap());
        let first_available = work_item(&store, work_id);
        assert_eq!(first_available.attempt_count, 1);
        assert_eq!(first_available.available_at, "2026-01-01T00:00:01+00:00");
        assert_eq!(first_available.last_error.as_deref(), Some("first failure"));

        claim_at(&store, 1, "2026-01-01T00:00:01+00:00").unwrap();
        let second_time = DateTime::parse_from_rfc3339("2026-01-01T00:00:01+00:00")
            .unwrap()
            .with_timezone(&Utc);
        fail_at(&store, work_id, &second_time, "second failure").unwrap();
        let second_available = work_item(&store, work_id);
        assert_eq!(second_available.attempt_count, 2);
        assert_eq!(second_available.available_at, "2026-01-01T00:00:03+00:00");
    }

    fn count_status(store: &Store, status: &str) -> i64 {
        store
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM work_item WHERE status = ?1",
                    [status],
                    |row| row.get(0),
                )
            })
            .unwrap()
    }

    fn work_item(store: &Store, work_id: i64) -> WorkItem {
        store
            .read(|connection| {
                connection.query_row(
                    "SELECT work_id, media_item_id, file_id, file_hash, work_type, status,
                            attempt_count, available_at, last_error
                     FROM work_item WHERE work_id = ?1",
                    params![work_id],
                    |row| {
                        Ok(WorkItem {
                            work_id: row.get(0)?,
                            media_item_id: row.get(1)?,
                            file_id: row.get(2)?,
                            file_hash: row.get(3)?,
                            kind: WorkKind::from_str(&row.get::<_, String>(4)?)?,
                            status: match row.get::<_, String>(5)?.as_str() {
                                "pending" => WorkStatus::Pending,
                                "running" => WorkStatus::Running,
                                _ => unreachable!(),
                            },
                            attempt_count: row.get(6)?,
                            available_at: row.get(7)?,
                            last_error: row.get(8)?,
                        })
                    },
                )
            })
            .unwrap()
    }
}
