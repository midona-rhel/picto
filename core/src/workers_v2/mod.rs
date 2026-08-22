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
}

pub(crate) fn enqueue_blob_delete_in(
    transaction: &Transaction<'_>,
    file_hash: &str,
    now: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO work_item (
             file_hash, work_type, status, attempt_count,
             available_at, created_at, updated_at
         ) VALUES (?1, 'blob_delete', 'pending', 0, ?2, ?2, ?2)
         ON CONFLICT(file_hash, work_type) WHERE file_hash IS NOT NULL DO NOTHING",
        params![file_hash, now],
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunReport {
    pub claimed: usize,
    pub succeeded: usize,
    pub retried: usize,
}

pub trait WorkExecutor {
    fn execute(&mut self, item: &WorkItem) -> Result<(), String>;
}

impl<F> WorkExecutor for F
where
    F: FnMut(&WorkItem) -> Result<(), String>,
{
    fn execute(&mut self, item: &WorkItem) -> Result<(), String> {
        self(item)
    }
}

pub struct Worker<'a> {
    store: &'a Store,
}

impl<'a> Worker<'a> {
    /// Reset interrupted work before the worker starts accepting claims.
    pub fn start(store: &'a Store) -> Result<Self, String> {
        reset_running(store)?;
        Ok(Self { store })
    }

    pub fn enqueue(&self, spec: WorkSpec) -> Result<EnqueueResult, String> {
        enqueue(self.store, spec)
    }

    pub fn run_batch<E: WorkExecutor>(
        &self,
        limit: usize,
        executor: &mut E,
    ) -> Result<RunReport, String> {
        run_batch(self.store, limit, executor)
    }
}

pub fn enqueue(store: &Store, spec: WorkSpec) -> Result<EnqueueResult, String> {
    enqueue_at(store, spec, &Utc::now().to_rfc3339())
}

pub fn enqueue_at(store: &Store, spec: WorkSpec, now: &str) -> Result<EnqueueResult, String> {
    if spec.media_item_id.is_none() && spec.file_id.is_none() && spec.file_hash.is_none() {
        return Err("work item needs a media item or file target".to_string());
    }

    let (result, _, _) = store.transaction_if_changed(|transaction| {
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
                 media_item_id, file_id, file_hash, work_type, status, attempt_count,
                 available_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?5, ?5)",
            params![
                spec.media_item_id,
                spec.file_id,
                spec.file_hash,
                spec.kind.as_str(),
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

pub fn reset_running_at(store: &Store, now: &str) -> Result<usize, String> {
    let (count, _, _) = store.transaction_if_changed(|transaction| {
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
    let (items, _, _) = store.transaction_if_changed(|transaction| {
        let mut statement = transaction.prepare(
            "SELECT work_id, media_item_id, file_id, file_hash, work_type, attempt_count,
                    available_at, last_error
             FROM work_item
             WHERE status = 'pending' AND available_at <= ?1
             ORDER BY available_at, work_id
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

        let mut items = Vec::with_capacity(rows.len());
        for (
            work_id,
            media_item_id,
            file_id,
            file_hash,
            kind,
            attempt_count,
            available_at,
            last_error,
        ) in rows
        {
            let changed = transaction.execute(
                "UPDATE work_item
                 SET status = 'running', updated_at = ?1
                 WHERE work_id = ?2 AND status = 'pending'",
                params![now, work_id],
            )?;
            if changed == 1 {
                items.push(WorkItem {
                    work_id,
                    media_item_id,
                    file_id,
                    file_hash,
                    kind,
                    status: WorkStatus::Running,
                    attempt_count,
                    available_at,
                    last_error,
                });
            }
        }
        let changed = !items.is_empty();
        Ok((items, changed))
    })?;
    Ok(items)
}

pub fn complete(store: &Store, work_id: i64) -> Result<bool, String> {
    let (changed, _, _) = store.transaction_if_changed(|transaction| {
        let count = transaction.execute(
            "DELETE FROM work_item WHERE work_id = ?1 AND status = 'running'",
            [work_id],
        )?;
        Ok((count == 1, count == 1))
    })?;
    Ok(changed)
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
    let (changed, _, _) = store.transaction_if_changed(|transaction| {
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

pub fn run_batch<E: WorkExecutor>(
    store: &Store,
    limit: usize,
    executor: &mut E,
) -> Result<RunReport, String> {
    let items = claim(store, limit)?;
    let mut report = RunReport {
        claimed: items.len(),
        ..RunReport::default()
    };
    for item in items {
        match executor.execute(&item) {
            Ok(()) => {
                complete(store, item.work_id)?;
                report.succeeded += 1;
            }
            Err(error) => {
                fail(store, item.work_id, &error)?;
                report.retried += 1;
            }
        }
    }
    Ok(report)
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
