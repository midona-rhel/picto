//! Durable entrypoint for every media import.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension};

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::ingest_v2::{IngestMediaResult, PreparedMediaInput};

const DEFAULT_BATCH_SIZE: usize = 8;
const MAX_ATTEMPTS: i64 = 8;
const MAX_BACKOFF_SECONDS: i64 = 300;

#[derive(Debug, Clone)]
pub struct IngestJobSpec {
    pub job_key: String,
    pub source_kind: String,
    pub source_path: String,
    pub delete_after_ingest: bool,
    pub input: PreparedMediaInput,
}

impl IngestJobSpec {
    pub fn subscription(
        source_path: impl Into<String>,
        delete_after_ingest: bool,
        input: PreparedMediaInput,
    ) -> Result<Self, String> {
        let source = input
            .source
            .as_ref()
            .ok_or_else(|| "A subscription ingest needs source identity".to_string())?;
        Ok(Self {
            job_key: format!(
                "subscription:{}:{}:{}",
                source.site_id, source.post_key, source.item_key
            ),
            source_kind: "subscription".to_string(),
            source_path: source_path.into(),
            delete_after_ingest,
            input,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestJobStatus {
    Pending,
    Running,
}

#[derive(Debug, Clone)]
pub struct IngestJob {
    pub ingest_job_id: i64,
    pub source_path: String,
    pub delete_after_ingest: bool,
    pub input: PreparedMediaInput,
    pub status: IngestJobStatus,
    pub attempt_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnqueueResult {
    pub ingest_job_id: i64,
    pub inserted: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestRunReport {
    pub claimed: usize,
    pub ingested: usize,
    pub retried: usize,
    pub failed: usize,
    pub item_ids: Vec<ItemId>,
}

pub struct IngestQueue<'a> {
    application: &'a Application,
}

impl<'a> IngestQueue<'a> {
    pub fn start(application: &'a Application) -> Result<Self, String> {
        reset_running(application)?;
        cleanup_orphaned_staging(application)?;
        Ok(Self { application })
    }

    pub fn enqueue(&self, spec: &IngestJobSpec) -> Result<EnqueueResult, String> {
        enqueue(self.application, spec)
    }

    pub fn run_batch(&self, limit: usize) -> Result<IngestRunReport, String> {
        run_batch(self.application, limit)
    }
}

fn cleanup_orphaned_staging(application: &Application) -> Result<(), String> {
    let staging = application.store().library_root().join("ingest-staging");
    let entries = match fs::read_dir(&staging) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Failed to inspect ingest staging: {error}")),
    };
    let referenced = application.store().read(|connection| {
        let mut statement = connection.prepare("SELECT source_path FROM ingest_job")?;
        let paths = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(paths)
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && !referenced.contains(path.to_string_lossy().as_ref()) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

pub fn enqueue(application: &Application, spec: &IngestJobSpec) -> Result<EnqueueResult, String> {
    validate_spec(spec)?;
    reject_deleted_source_item(application, &spec.input)?;
    if let Some(existing) = existing_job(application, &spec.job_key)? {
        remove_owned_source(spec);
        return Ok(existing);
    }

    let staged_path = stage_source(application, spec)?;
    let mut staged = spec.clone();
    staged.source_path = staged_path.display().to_string();
    staged.delete_after_ingest = true;
    let result = enqueue_at(application, &staged, &Utc::now().to_rfc3339());
    match result {
        Ok(result) => {
            if !result.inserted {
                let _ = fs::remove_file(&staged_path);
            }
            remove_owned_source(spec);
            Ok(result)
        }
        Err(error) => {
            let _ = fs::remove_file(staged_path);
            Err(error)
        }
    }
}

fn reject_deleted_source_item(
    application: &Application,
    input: &PreparedMediaInput,
) -> Result<(), String> {
    let Some(source) = &input.source else {
        return Ok(());
    };
    let deleted = application.store().read(|connection| {
        connection
            .query_row(
                "SELECT si.state
                 FROM source_item si
                 JOIN source_post sp ON sp.source_post_id = si.source_post_id
                 WHERE sp.site_id = ?1 AND sp.post_key = ?2 AND si.item_key = ?3",
                params![source.site_id, source.post_key, source.item_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|state| state.as_deref() == Some("deleted"))
    })?;
    if deleted {
        return Err("This source item was deliberately deleted and cannot be resurrected".into());
    }
    Ok(())
}

fn enqueue_at(
    application: &Application,
    spec: &IngestJobSpec,
    now: &str,
) -> Result<EnqueueResult, String> {
    validate_spec(spec)?;
    let payload = serde_json::to_string(&spec.input).map_err(|error| error.to_string())?;
    let (result, revision, _) = application.store().transaction_if_changed(|transaction| {
        let source_item = if let Some(source) = &spec.input.source {
            transaction
                .query_row(
                    "SELECT si.source_item_id, si.state
                     FROM source_item si
                     JOIN source_post sp ON sp.source_post_id = si.source_post_id
                     WHERE sp.site_id = ?1 AND sp.post_key = ?2 AND si.item_key = ?3",
                    params![source.site_id, source.post_key, source.item_key],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
        } else {
            None
        };
        if source_item
            .as_ref()
            .is_some_and(|(_, state)| state == "deleted")
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "This source item was deliberately deleted and cannot be resurrected".to_string(),
            ));
        }
        let source_item_id = source_item.map(|(source_item_id, _)| source_item_id);
        let existing = transaction
            .query_row(
                "SELECT ingest_job_id FROM ingest_job
                 WHERE job_key = ?1 OR (?2 IS NOT NULL AND source_item_id = ?2)",
                params![spec.job_key, source_item_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(ingest_job_id) = existing {
            return Ok((
                EnqueueResult {
                    ingest_job_id,
                    inserted: false,
                    revision: 0,
                },
                false,
            ));
        }
        if let Some(source_item_id) = source_item_id {
            transaction.execute(
                "UPDATE source_item
                 SET state = 'downloaded', last_error = NULL, updated_at = ?1
                 WHERE source_item_id = ?2 AND state <> 'deleted'",
                params![now, source_item_id],
            )?;
        }
        transaction.execute(
            "INSERT INTO ingest_job (
                 job_key, source_kind, source_path, source_item_id, payload_json,
                 lifecycle, delete_after_ingest, status, attempt_count,
                 available_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', 0, ?8, ?8, ?8)",
            params![
                spec.job_key,
                spec.source_kind,
                spec.source_path,
                source_item_id,
                payload,
                spec.input.lifecycle.as_str(),
                spec.delete_after_ingest as i64,
                now,
            ],
        )?;
        Ok((
            EnqueueResult {
                ingest_job_id: transaction.last_insert_rowid(),
                inserted: true,
                revision: 0,
            },
            true,
        ))
    })?;
    Ok(EnqueueResult { revision, ..result })
}

fn validate_spec(spec: &IngestJobSpec) -> Result<(), String> {
    if spec.job_key.trim().is_empty() {
        return Err("An ingest job key is required".to_string());
    }
    if spec.source_kind.trim().is_empty() || spec.source_path.trim().is_empty() {
        return Err("An ingest source kind and path are required".to_string());
    }
    Ok(())
}

fn existing_job(application: &Application, job_key: &str) -> Result<Option<EnqueueResult>, String> {
    application.store().read(|connection| {
        connection
            .query_row(
                "SELECT ingest_job_id FROM ingest_job WHERE job_key = ?1",
                [job_key],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map(|job| {
                job.map(|ingest_job_id| EnqueueResult {
                    ingest_job_id,
                    inserted: false,
                    revision: 0,
                })
            })
    })
}

fn stage_source(application: &Application, spec: &IngestJobSpec) -> Result<PathBuf, String> {
    let source = Path::new(&spec.source_path);
    if !source.is_file() {
        return Err(format!("Ingest source is missing: {}", source.display()));
    }
    let staging = application.store().library_root().join("ingest-staging");
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create ingest staging: {error}"))?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::blob_store::mime_to_extension(&spec.input.mime_type));
    let nonce = rand::random::<u64>();
    let final_path = staging.join(format!("{nonce:016x}.{extension}"));
    let partial_path = staging.join(format!("{nonce:016x}.partial"));
    fs::copy(source, &partial_path)
        .map_err(|error| format!("Failed to stage ingest source: {error}"))?;
    fs::File::open(&partial_path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Failed to sync staged ingest source: {error}"))?;
    fs::rename(&partial_path, &final_path)
        .map_err(|error| format!("Failed to publish staged ingest source: {error}"))?;
    Ok(final_path)
}

fn remove_owned_source(spec: &IngestJobSpec) {
    if spec.delete_after_ingest {
        let _ = fs::remove_file(&spec.source_path);
    }
}

pub fn reset_running(application: &Application) -> Result<usize, String> {
    reset_running_at(application, &Utc::now().to_rfc3339())
}

fn reset_running_at(application: &Application, now: &str) -> Result<usize, String> {
    let (count, _, _) = application.store().transaction_if_changed(|transaction| {
        let count = transaction.execute(
            "UPDATE ingest_job
             SET status = 'pending', available_at = ?1, updated_at = ?1
             WHERE status = 'running'",
            [now],
        )?;
        Ok((count, count != 0))
    })?;
    Ok(count)
}

pub fn claim(application: &Application, limit: usize) -> Result<Vec<IngestJob>, String> {
    claim_at(
        application,
        limit.min(DEFAULT_BATCH_SIZE),
        &Utc::now().to_rfc3339(),
    )
}

fn claim_at(application: &Application, limit: usize, now: &str) -> Result<Vec<IngestJob>, String> {
    let (jobs, _, _) = application.store().transaction_if_changed(|transaction| {
        let mut statement = transaction.prepare(
            "SELECT ingest_job_id, source_path, delete_after_ingest,
                    payload_json, attempt_count
             FROM ingest_job
             WHERE status = 'pending' AND available_at <= ?1
             ORDER BY available_at, ingest_job_id
             LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![now, limit as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let mut jobs = Vec::with_capacity(rows.len());
        for (ingest_job_id, source_path, delete_after_ingest, payload, attempt_count) in rows {
            let input: PreparedMediaInput = serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let changed = transaction.execute(
                "UPDATE ingest_job SET status = 'running', updated_at = ?1
                 WHERE ingest_job_id = ?2 AND status = 'pending'",
                params![now, ingest_job_id],
            )?;
            if changed == 1 {
                jobs.push(IngestJob {
                    ingest_job_id,
                    source_path,
                    delete_after_ingest,
                    input,
                    status: IngestJobStatus::Running,
                    attempt_count,
                });
            }
        }
        let changed = !jobs.is_empty();
        Ok((jobs, changed))
    })?;
    Ok(jobs)
}

pub fn run_batch(application: &Application, limit: usize) -> Result<IngestRunReport, String> {
    let jobs = claim(application, limit)?;
    let mut report = IngestRunReport {
        claimed: jobs.len(),
        ..IngestRunReport::default()
    };
    let mut resources_changed = BTreeSet::from([resources::TASKS.to_string()]);
    let mut item_ids = BTreeSet::new();

    for job in jobs {
        match process_job(application, &job) {
            Ok(result) => {
                mark_succeeded(application, job.ingest_job_id)?;
                report.ingested += 1;
                item_ids.insert(result.root_item_id.0);
                if let Some(receipt) = result.receipt {
                    resources_changed.extend(receipt.resources);
                    item_ids.extend(receipt.item_ids.into_iter().map(|item_id| item_id.0));
                }
            }
            Err(error) => {
                if retry_or_fail(application, &job, &error)? {
                    report.failed += 1;
                } else {
                    report.retried += 1;
                }
            }
        }
    }

    report.item_ids = item_ids.iter().copied().map(ItemId).collect();
    if report.claimed != 0 {
        application.publish(&MutationReceipt {
            revision: application.store().revision()?,
            resources: resources_changed.into_iter().collect(),
            item_ids: report.item_ids.clone(),
        });
    }
    Ok(report)
}

fn process_job(application: &Application, job: &IngestJob) -> Result<IngestMediaResult, String> {
    let bytes = fs::read(&job.source_path)
        .map_err(|error| format!("Failed to read staged ingest source: {error}"))?;
    let actual_hash = hex::encode(crate::media_processing::get_hash_from_bytes(&bytes));
    if actual_hash != job.input.file_hash {
        return Err(format!(
            "Staged ingest hash mismatch: expected {}, found {actual_hash}",
            job.input.file_hash
        ));
    }
    let extension = crate::blob_store::mime_to_extension(&job.input.mime_type);
    application
        .blobs()
        .write_original(&job.input.file_hash, &bytes, Some(extension))
        .map_err(|error| format!("Failed to persist original blob: {error}"))?;
    let result = application.ingest_prepared(&job.input)?;
    if job.delete_after_ingest {
        match fs::remove_file(&job.source_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Imported media but could not remove source: {error}"
                ))
            }
        }
    }
    Ok(result)
}

fn mark_succeeded(application: &Application, ingest_job_id: i64) -> Result<(), String> {
    application.store().transaction(|transaction| {
        transaction.execute(
            "UPDATE ingest_job
             SET status = 'succeeded', last_error = NULL, updated_at = ?1
             WHERE ingest_job_id = ?2 AND status = 'running'",
            params![Utc::now().to_rfc3339(), ingest_job_id],
        )?;
        Ok(())
    })?;
    Ok(())
}

fn retry_or_fail(application: &Application, job: &IngestJob, error: &str) -> Result<bool, String> {
    let attempts = job.attempt_count + 1;
    let terminal = attempts >= MAX_ATTEMPTS;
    let delay = (1_i64 << attempts.min(8) as u32).min(MAX_BACKOFF_SECONDS);
    let available_at = (Utc::now() + Duration::seconds(delay)).to_rfc3339();
    application.store().transaction(|transaction| {
        transaction.execute(
            "UPDATE ingest_job
             SET status = ?1, attempt_count = ?2, available_at = ?3,
                 last_error = ?4, updated_at = ?5
             WHERE ingest_job_id = ?6 AND status = 'running'",
            params![
                if terminal { "failed" } else { "pending" },
                attempts,
                available_at,
                error,
                Utc::now().to_rfc3339(),
                job.ingest_job_id,
            ],
        )?;
        Ok(())
    })?;
    Ok(terminal)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use rusqlite::params;

    use super::{claim_at, enqueue_at, reset_running_at, IngestJobSpec, IngestQueue};
    use crate::app::{Application, Lifecycle};
    use crate::ingest_v2::{PreparedMediaInput, SourcePostInput};
    use crate::store::Store;

    const MEDIA_BYTES: &[u8] = b"picto-ingest-test";

    fn application() -> (tempfile::TempDir, Application) {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        (directory, app)
    }

    fn input() -> PreparedMediaInput {
        PreparedMediaInput {
            file_hash: hex::encode(crate::media_processing::get_hash_from_bytes(MEDIA_BYTES)),
            mime_type: "image/png".to_string(),
            size_bytes: MEDIA_BYTES.len() as i64,
            pixel_width: Some(10),
            pixel_height: Some(10),
            duration_ms: None,
            frame_count: Some(1),
            has_audio: false,
            name: Some("item".to_string()),
            notes: None,
            rating: None,
            source_urls: Vec::new(),
            tags: Vec::new(),
            provenance_mask: 1,
            lifecycle: Lifecycle::Inbox,
            captured_at: None,
            source: Some(SourcePostInput {
                site_id: "example".to_string(),
                post_key: "post".to_string(),
                item_key: "item".to_string(),
                position: 0,
                canonical_post_url: None,
                canonical_media_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
            }),
        }
    }

    fn spec(path: &str) -> IngestJobSpec {
        IngestJobSpec {
            job_key: "source:example:post:item".to_string(),
            source_kind: "subscription".to_string(),
            source_path: path.to_string(),
            delete_after_ingest: false,
            input: input(),
        }
    }

    #[test]
    fn enqueue_is_idempotent_and_restart_recovers_running_jobs() {
        let (_directory, app) = application();
        let first = enqueue_at(&app, &spec("/tmp/item"), "2026-01-01T00:00:00Z").unwrap();
        let repeated = enqueue_at(&app, &spec("/tmp/item"), "2026-01-01T00:00:00Z").unwrap();
        assert!(first.inserted);
        assert!(!repeated.inserted);
        assert_eq!(first.ingest_job_id, repeated.ingest_job_id);

        let jobs = claim_at(&app, 8, "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(reset_running_at(&app, "2026-01-01T00:01:00Z").unwrap(), 1);
        assert_eq!(claim_at(&app, 8, "2026-01-01T00:01:00Z").unwrap().len(), 1);
    }

    #[test]
    fn pending_subscription_item_flows_through_the_durable_queue() {
        let (directory, app) = application();
        let source = directory.path().join("source.png");
        fs::write(&source, MEDIA_BYTES).unwrap();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO source_post (
                         site_id, post_key, created_at, updated_at
                     ) VALUES ('example', 'post', 'now', 'now')",
                    [],
                )?;
                let post_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, state, created_at, updated_at
                     ) VALUES (?1, 'item', 0, 'downloaded', 'now', 'now')",
                    [post_id],
                )?;
                Ok(())
            })
            .unwrap();

        let queue = IngestQueue::start(&app).unwrap();
        queue.enqueue(&spec(source.to_str().unwrap())).unwrap();
        let downloaded_state: String = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT state FROM source_item", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(downloaded_state, "downloaded");
        let report = queue.run_batch(8).unwrap();
        let job_state: (String, Option<String>) = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT status, last_error FROM ingest_job", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(report.ingested, 1, "report={report:?}, job={job_state:?}");
        assert_eq!(report.item_ids.len(), 1);
        app.store()
            .read(|connection| {
                let (state, media_item_id): (String, Option<i64>) = connection.query_row(
                    "SELECT state, media_item_id FROM source_item WHERE item_key = 'item'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(state, "ingested");
                assert!(media_item_id.is_some());
                let status: String =
                    connection.query_row("SELECT status FROM ingest_job", [], |row| row.get(0))?;
                assert_eq!(status, "succeeded");
                Ok(())
            })
            .unwrap();
        let extension = crate::blob_store::mime_to_extension(&input().mime_type);
        assert!(app
            .blobs()
            .find_original(&input().file_hash, Some(extension))
            .unwrap()
            .is_some());
        assert!(fs::read_dir(directory.path().join("ingest-staging"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn a_failed_job_returns_to_pending_with_backoff() {
        let (directory, app) = application();
        let source = directory.path().join("invalid.bin");
        fs::write(&source, MEDIA_BYTES).unwrap();
        let mut invalid = spec(source.to_str().unwrap());
        invalid.input.mime_type = "application/octet-stream".to_string();
        enqueue_at(&app, &invalid, "2026-01-01T00:00:00Z").unwrap();
        let report = IngestQueue::start(&app).unwrap().run_batch(1).unwrap();
        assert_eq!(report.retried, 1);
        app.store()
            .read(|connection| {
                let (status, attempts, error): (String, i64, Option<String>) = connection
                    .query_row(
                        "SELECT status, attempt_count, last_error FROM ingest_job",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                assert_eq!(status, "pending");
                assert_eq!(attempts, 1);
                assert!(error.unwrap().contains("Unsupported media type"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn deleted_source_item_remains_a_tombstone() {
        let (_directory, app) = application();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('example', 'post', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, state, created_at, updated_at
                     ) VALUES (?1, 'item', 0, 'deleted', 'now', 'now')",
                    params![transaction.last_insert_rowid()],
                )?;
                Ok(())
            })
            .unwrap();
        let queue = IngestQueue::start(&app).unwrap();
        let error = queue.enqueue(&spec("/tmp/item")).unwrap_err();
        assert!(error.contains("cannot be resurrected"));
        let state: String = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT state FROM source_item", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(state, "deleted");
        let jobs: i64 = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM ingest_job", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(jobs, 0);
    }
}
