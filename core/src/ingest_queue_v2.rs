//! Durable entrypoint for every media import.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::app::{resources, Application, ItemId, Lifecycle, MutationReceipt};
use crate::ingest_v2::{PreparedMediaInput, DELETED_SOURCE_ITEM_ERROR};

const DEFAULT_BATCH_SIZE: usize = 64;
const INVALIDATION_CADENCE: StdDuration = StdDuration::from_millis(16);
const MAX_INVALIDATION_ITEM_IDS: usize = 256;
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

#[derive(Debug, Clone)]
struct ExistingJob {
    ingest_job_id: i64,
    source_path: String,
    status: String,
    source_state: Option<String>,
    media_item_id: Option<i64>,
}

struct StagedSource {
    path: PathBuf,
    hash: String,
    size_bytes: i64,
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
        compact_succeeded_payloads(application)?;
        discard_abandoned_gallery_sources(application)?;
        reset_running(application)?;
        recover_settled_provisional_collections(application)?;
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

/// Successful jobs retain their durable identity, but their retry input is no
/// longer useful. A fresh enqueue supplies fresh input if a terminal job ever
/// needs to be reactivated.
pub(crate) fn compact_succeeded_payloads(application: &Application) -> Result<usize, String> {
    let (compacted, _, _) = application.store().transaction_if_changed(|transaction| {
        let compacted = transaction.execute(
            "UPDATE ingest_job SET payload_json = '{}'
             WHERE status = 'succeeded' AND payload_json <> '{}'",
            [],
        )?;
        Ok((compacted, compacted != 0))
    })?;
    Ok(compacted)
}

/// Removes transient gallery work whose owning job was deleted before the
/// media materialized. Materialized history and active run items are untouched.
pub(crate) fn discard_abandoned_gallery_sources(
    application: &Application,
) -> Result<usize, String> {
    let (discarded, _, _) = application.store().transaction_if_changed(|transaction| {
        let discarded = transaction.execute(
            "DELETE FROM ingest_job
             WHERE source_item_id IN (
                 SELECT si.source_item_id
                 FROM source_item si
                 JOIN source_post sp ON sp.source_post_id = si.source_post_id
                 WHERE si.media_item_id IS NULL
                   AND sp.site_id = 'ehentai'
                   AND NOT EXISTS (
                       SELECT 1 FROM subscription_run_source_item rsi
                       WHERE rsi.source_item_id = si.source_item_id
                   )
             )",
            [],
        )?;
        let source_items = transaction.execute(
            "DELETE FROM source_item
             WHERE media_item_id IS NULL
               AND source_post_id IN (
                   SELECT source_post_id FROM source_post WHERE site_id = 'ehentai'
               )
               AND NOT EXISTS (
                   SELECT 1 FROM subscription_run_source_item rsi
                   WHERE rsi.source_item_id = source_item.source_item_id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM ingest_job ij
                   WHERE ij.source_item_id = source_item.source_item_id
               )",
            [],
        )?;
        let source_posts = transaction.execute(
            "DELETE FROM source_post
             WHERE root_item_id IS NULL
               AND site_id = 'ehentai'
               AND NOT EXISTS (
                   SELECT 1 FROM source_item si
                   WHERE si.source_post_id = source_post.source_post_id
               )",
            [],
        )?;
        let changed = discarded + source_items + source_posts;
        Ok((discarded, changed != 0))
    })?;
    if discarded != 0 {
        tracing::warn!(discarded, "Discarded abandoned gallery ingest jobs");
    }
    Ok(discarded)
}

/// Makes successfully ingested partial source posts visible after a restart.
///
/// Grouped downloads stay provisional while their post is actively being
/// ingested. If a run ends before any item carries the final `post_complete`
/// marker, all completed media would otherwise remain permanently hidden even
/// though their jobs committed successfully. Pending/running work blocks this
/// recovery so an active post is never exposed midway through ingestion.
pub(crate) fn recover_settled_provisional_collections(
    application: &Application,
) -> Result<usize, String> {
    let candidates = application.store().read(|connection| {
        let mut statement = connection.prepare(
            "SELECT cm.collection_id, si.source_post_id,
                        COALESCE(MAX(ij.lifecycle), 'inbox')
                 FROM collection_member cm
                 JOIN source_item si ON si.media_item_id = cm.media_item_id
                 JOIN source_post sp ON sp.source_post_id = si.source_post_id
                 LEFT JOIN library_root lr ON lr.item_id = cm.collection_id
                 LEFT JOIN ingest_job ij ON ij.source_item_id = si.source_item_id
                 WHERE lr.item_id IS NULL AND sp.root_item_id IS NULL
                   AND NOT EXISTS (
                       SELECT 1
                       FROM source_item pending_si
                       JOIN ingest_job pending_job
                         ON pending_job.source_item_id = pending_si.source_item_id
                       WHERE pending_si.source_post_id = si.source_post_id
                         AND pending_job.status IN ('pending', 'running')
                   )
                 GROUP BY cm.collection_id, si.source_post_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let mut recovered = 0;
    for (collection_id, source_post_id, lifecycle) in candidates {
        let lifecycle = match lifecycle.as_str() {
            "inbox" => Lifecycle::Inbox,
            "active" => Lifecycle::Active,
            "trash" => Lifecycle::Trash,
            value => return Err(format!("Invalid provisional lifecycle: {value}")),
        };
        recovered += usize::from(application.recover_provisional_source_root(
            collection_id,
            source_post_id,
            lifecycle,
        )?);
    }
    if recovered != 0 {
        tracing::info!(recovered, "Recovered settled provisional collections");
    }
    Ok(recovered)
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
        let missing_subscription_media = existing.source_state.as_deref() != Some("deleted")
            && existing.source_state.is_some()
            && existing.media_item_id.is_none();
        if existing.status == "failed"
            || (existing.status == "succeeded" && missing_subscription_media)
        {
            return reactivate_terminal_job(application, spec, &existing);
        }
        remove_owned_source(spec);
        return Ok(EnqueueResult {
            ingest_job_id: existing.ingest_job_id,
            inserted: false,
            revision: application.store().revision()?,
        });
    }

    let staged_source = stage_source(application, spec)?;
    let mut staged = spec.clone();
    staged.source_path = staged_source.path.display().to_string();
    staged.delete_after_ingest = false;
    staged.input.file_hash = staged_source.hash;
    staged.input.size_bytes = staged_source.size_bytes;
    let result = enqueue_at(application, &staged, &Utc::now().to_rfc3339());
    match result {
        Ok(result) => {
            remove_owned_source(spec);
            Ok(result)
        }
        Err(error) => Err(error),
    }
}

fn reactivate_terminal_job(
    application: &Application,
    spec: &IngestJobSpec,
    existing: &ExistingJob,
) -> Result<EnqueueResult, String> {
    let staged_source = stage_source(application, spec)?;
    let mut staged_input = spec.input.clone();
    staged_input.file_hash = staged_source.hash;
    staged_input.size_bytes = staged_source.size_bytes;
    let payload = serde_json::to_string(&staged_input).map_err(|error| error.to_string())?;
    let now = Utc::now().to_rfc3339();
    let result = application.store().transaction_if_changed(|transaction| {
        let changed = transaction.execute(
            "UPDATE ingest_job
             SET source_path = ?1, payload_json = ?2, lifecycle = ?3,
                delete_after_ingest = 0, status = 'pending', attempt_count = 0,
                 available_at = ?4, last_error = NULL, updated_at = ?4
             WHERE ingest_job_id = ?5 AND status = ?6",
            params![
                staged_source.path.display().to_string(),
                payload,
                spec.input.lifecycle.as_str(),
                now,
                existing.ingest_job_id,
                existing.status,
            ],
        )?;
        if changed == 1 {
            transaction.execute(
                "UPDATE source_item
                 SET state = 'downloaded', last_error = NULL, updated_at = ?1
                 WHERE source_item_id = (
                     SELECT source_item_id FROM ingest_job WHERE ingest_job_id = ?2
                 ) AND media_item_id IS NULL AND state <> 'deleted'",
                params![now, existing.ingest_job_id],
            )?;
        }
        Ok((changed == 1, changed == 1))
    });
    match result {
        Ok((true, revision, _)) => {
            remove_ingest_staging_path(application, &existing.source_path);
            remove_owned_source(spec);
            Ok(EnqueueResult {
                ingest_job_id: existing.ingest_job_id,
                inserted: true,
                revision,
            })
        }
        Ok((false, _, _)) => {
            remove_owned_source(spec);
            let current = existing_job(application, &spec.job_key)?
                .ok_or_else(|| "Ingest job disappeared while retrying it".to_string())?;
            Ok(EnqueueResult {
                ingest_job_id: current.ingest_job_id,
                inserted: false,
                revision: application.store().revision()?,
            })
        }
        Err(error) => Err(error),
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
        return Err(DELETED_SOURCE_ITEM_ERROR.to_string());
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
                DELETED_SOURCE_ITEM_ERROR.to_string(),
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

fn existing_job(application: &Application, job_key: &str) -> Result<Option<ExistingJob>, String> {
    application.store().read(|connection| {
        connection
            .query_row(
                "SELECT ij.ingest_job_id, ij.source_path, ij.status,
                        si.state, si.media_item_id
                 FROM ingest_job ij
                 LEFT JOIN source_item si ON si.source_item_id = ij.source_item_id
                 WHERE ij.job_key = ?1",
                [job_key],
                |row| {
                    Ok(ExistingJob {
                        ingest_job_id: row.get(0)?,
                        source_path: row.get(1)?,
                        status: row.get(2)?,
                        source_state: row.get(3)?,
                        media_item_id: row.get(4)?,
                    })
                },
            )
            .optional()
    })
}

fn stage_source(application: &Application, spec: &IngestJobSpec) -> Result<StagedSource, String> {
    if tokio::runtime::Handle::try_current()
        .ok()
        .is_some_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        return tokio::task::block_in_place(|| stage_source_blocking(application, spec));
    }
    stage_source_blocking(application, spec)
}

fn stage_source_blocking(
    application: &Application,
    spec: &IngestJobSpec,
) -> Result<StagedSource, String> {
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
    let staged = (|| {
        let mut input = fs::File::open(source)?;
        let mut output = fs::File::create(&partial_path)?;
        let mut hasher = Sha256::new();
        let mut size_bytes = 0_i64;
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            size_bytes = size_bytes
                .checked_add(read as i64)
                .ok_or_else(|| std::io::Error::other("ingest source is too large"))?;
        }
        output.sync_all()?;
        Ok::<_, std::io::Error>((hex::encode(hasher.finalize()), size_bytes))
    })()
    .map_err(|error| format!("Failed to stage ingest source: {error}"));
    let (hash, size_bytes) = match staged {
        Ok(staged) => staged,
        Err(error) => {
            let _ = fs::remove_file(&partial_path);
            return Err(error);
        }
    };
    if !spec.input.file_hash.is_empty() && spec.input.file_hash != hash {
        let _ = fs::remove_file(&partial_path);
        return Err(format!(
            "Ingest source hash mismatch: expected {}, found {hash}",
            spec.input.file_hash
        ));
    }
    fs::rename(&partial_path, &final_path)
        .map_err(|error| format!("Failed to publish staged ingest source: {error}"))?;
    #[cfg(unix)]
    if let Ok(directory) = fs::File::open(&staging) {
        let _ = directory.sync_all();
    }
    let blob_extension = crate::blob_store::mime_to_extension(&spec.input.mime_type);
    let blob_path = application
        .blobs()
        .promote_verified_original(&hash, &final_path, Some(blob_extension))
        .map_err(|error| format!("Failed to promote verified ingest source: {error}"))?;
    Ok(StagedSource {
        path: blob_path,
        hash,
        size_bytes,
    })
}

fn remove_owned_source(spec: &IngestJobSpec) {
    if spec.delete_after_ingest {
        let _ = fs::remove_file(&spec.source_path);
    }
}

fn remove_ingest_staging_path(application: &Application, path: &str) {
    let path = Path::new(path);
    let staging = application.store().library_root().join("ingest-staging");
    if path.starts_with(staging) {
        let _ = fs::remove_file(path);
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
            "SELECT ij.ingest_job_id, ij.source_path, ij.delete_after_ingest,
                    ij.payload_json, ij.attempt_count
             FROM ingest_job ij
             WHERE ij.status = 'pending' AND ij.available_at <= ?1
             ORDER BY ij.available_at, ij.ingest_job_id
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

        let job_ids = rows.iter().map(|row| row.0).collect::<Vec<_>>();
        if !job_ids.is_empty() {
            let encoded = serde_json::to_string(&job_ids)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            transaction.execute(
                "UPDATE ingest_job SET status = 'running', updated_at = ?1
                 WHERE status = 'pending'
                   AND ingest_job_id IN (
                       SELECT CAST(value AS INTEGER) FROM json_each(?2)
                   )",
                params![now, encoded],
            )?;
        }
        let jobs = rows
            .into_iter()
            .map(
                |(ingest_job_id, source_path, delete_after_ingest, payload, attempt_count)| {
                    let input: PreparedMediaInput =
                        serde_json::from_str(&payload).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?;
                    Ok(IngestJob {
                        ingest_job_id,
                        source_path,
                        delete_after_ingest,
                        input,
                        status: IngestJobStatus::Running,
                        attempt_count,
                    })
                },
            )
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let changed = !jobs.is_empty();
        Ok((jobs, changed))
    })?;
    Ok(jobs)
}

pub fn run_batch(application: &Application, limit: usize) -> Result<IngestRunReport, String> {
    let _execution = application.lock_ingest_execution()?;
    let jobs = claim(application, limit)?;
    run_claimed_batch(application, jobs)
}

pub(crate) fn drain_query(
    _application: &Application,
    _run_query_id: i64,
    _limit: usize,
) -> Result<IngestRunReport, String> {
    // Source runners only enqueue durable work. The maintenance loop is the
    // sole ingest consumer, preventing competing publishers and out-of-order
    // source settlement.
    Ok(IngestRunReport::default())
}

fn run_claimed_batch(
    application: &Application,
    jobs: Vec<IngestJob>,
) -> Result<IngestRunReport, String> {
    let mut report = IngestRunReport {
        claimed: jobs.len(),
        ..IngestRunReport::default()
    };
    let mut resources_changed = BTreeSet::new();
    let mut item_ids = BTreeSet::new();
    let mut report_item_ids = BTreeSet::new();
    let mut last_invalidation = Instant::now();
    let preparations = prepare_jobs_bounded(application, jobs);

    let mut ready = Vec::new();
    for (job, preparation) in preparations {
        match preparation {
            Ok(thumbnail_prepared) => ready.push((job, thumbnail_prepared)),
            Err(error) => {
                if retry_or_fail(application, &job, &error)? {
                    report.failed += 1;
                } else {
                    report.retried += 1;
                }
            }
        }
    }

    // File preparation is outside SQLite writer ownership. The caller holds
    // the queue execution lease so another driver cannot claim later source
    // items and publish them out of order while this batch is being prepared.
    let mut succeeded = Vec::new();
    let mut deferred = Vec::new();
    let mut processed = 0_usize;
    let commit_started = Instant::now();

    for (job, thumbnail_prepared) in ready {
        if processed != 0 && commit_started.elapsed() >= INVALIDATION_CADENCE {
            deferred.push(job);
            continue;
        }
        processed += 1;
        match application.ingest_prepared(&job.input) {
            Ok(result) => {
                report.ingested += 1;
                succeeded.push((job, thumbnail_prepared));
                if let Some(receipt) = result.receipt {
                    resources_changed.extend(receipt.resources);
                    let visible_ids = receipt
                        .item_ids
                        .into_iter()
                        .map(|item_id| item_id.0)
                        .collect::<Vec<_>>();
                    item_ids.extend(visible_ids.iter().copied());
                    report_item_ids.extend(visible_ids);
                }
                if last_invalidation.elapsed() >= INVALIDATION_CADENCE {
                    publish_ingest_changes(application, &mut resources_changed, &mut item_ids)?;
                    last_invalidation = Instant::now();
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

    if !deferred.is_empty() {
        report.claimed = report.claimed.saturating_sub(deferred.len());
        release_claimed(application, &deferred)?;
    }

    settle_succeeded(application, &succeeded)?;
    for (job, _) in &succeeded {
        cleanup_staged_source(job);
    }

    report.item_ids = report_item_ids.into_iter().map(ItemId).collect();
    if report.claimed != 0 {
        resources_changed.insert(resources::TASKS.to_string());
        publish_ingest_changes(application, &mut resources_changed, &mut item_ids)?;
    }
    Ok(report)
}

fn prepare_jobs_bounded(
    application: &Application,
    jobs: Vec<IngestJob>,
) -> Vec<(IngestJob, Result<bool, String>)> {
    let mut representatives = BTreeMap::new();
    for job in &jobs {
        representatives
            .entry((job.input.file_hash.clone(), job.input.mime_type.clone()))
            .or_insert_with(|| job.clone());
    }
    let representatives = representatives.into_iter().collect::<Vec<_>>();
    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(4)
        .min(representatives.len().max(1));
    let next = AtomicUsize::new(0);
    let prepared = Mutex::new(Vec::with_capacity(representatives.len()));
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((key, job)) = representatives.get(index) else {
                    return;
                };
                let result = prepare_job_media(application, job);
                prepared
                    .lock()
                    .expect("preparation lock poisoned")
                    .push((key.clone(), result));
            });
        }
    });
    let prepared = prepared
        .into_inner()
        .expect("preparation lock poisoned")
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    jobs.into_iter()
        .map(|job| {
            let key = (job.input.file_hash.clone(), job.input.mime_type.clone());
            let result = prepared
                .get(&key)
                .cloned()
                .unwrap_or_else(|| Err("Ingest preparation produced no result".to_string()));
            (job, result)
        })
        .collect()
}

fn release_claimed(application: &Application, jobs: &[IngestJob]) -> Result<(), String> {
    let job_ids = jobs.iter().map(|job| job.ingest_job_id).collect::<Vec<_>>();
    let encoded = serde_json::to_string(&job_ids)
        .map_err(|error| format!("Could not encode deferred ingest IDs: {error}"))?;
    application.store().transaction_if_changed(|transaction| {
        let changed = transaction.execute(
            "UPDATE ingest_job SET status = 'pending'
             WHERE status = 'running'
               AND ingest_job_id IN (
                   SELECT CAST(value AS INTEGER) FROM json_each(?1)
               )",
            [encoded],
        )? != 0;
        Ok(((), changed))
    })?;
    Ok(())
}

fn publish_ingest_changes(
    application: &Application,
    resources_changed: &mut BTreeSet<String>,
    item_ids: &mut BTreeSet<i64>,
) -> Result<(), String> {
    if resources_changed.is_empty() && item_ids.is_empty() {
        return Ok(());
    }
    let bounded_item_ids = if item_ids.len() <= MAX_INVALIDATION_ITEM_IDS {
        item_ids.iter().copied().map(ItemId).collect()
    } else {
        Vec::new()
    };
    application.publish(&MutationReceipt {
        revision: application.store().revision()?,
        resources: std::mem::take(resources_changed).into_iter().collect(),
        item_ids: bounded_item_ids,
    });
    item_ids.clear();
    Ok(())
}

pub fn has_ready_or_running(application: &Application) -> Result<bool, String> {
    let now = Utc::now().to_rfc3339();
    application.store().read(|connection| {
        connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM ingest_job
                 WHERE status = 'running'
                    OR (status = 'pending' AND available_at <= ?1)
             )",
            [now],
            |row| row.get(0),
        )
    })
}

fn prepare_job_media(application: &Application, job: &IngestJob) -> Result<bool, String> {
    let extension = crate::blob_store::mime_to_extension(&job.input.mime_type);
    let source = Path::new(&job.source_path);
    let stored = application
        .blobs()
        .original_path_with_ext(&job.input.file_hash, Some(extension))
        .map_err(|error| format!("Original blob path failed: {error}"))?;
    if source == stored {
        let actual_size = fs::metadata(source)
            .map_err(|error| format!("Stored ingest source is missing: {error}"))?
            .len();
        if actual_size != job.input.size_bytes.max(0) as u64 {
            return Err(format!(
                "Staged ingest hash mismatch: expected {} bytes for {}, found {actual_size}",
                job.input.size_bytes, job.input.file_hash
            ));
        }
    } else {
        application
            .blobs()
            .write_original_from_path(&job.input.file_hash, source, Some(extension))
            .map_err(|error| match error {
                crate::blob_store::BlobError::HashMismatch { expected, actual } => {
                    format!("Staged ingest hash mismatch: expected {expected}, found {actual}")
                }
                error => format!("Failed to persist original blob: {error}"),
            })?;
    }
    use crate::media_capabilities::ThumbnailBackend;

    let capabilities = crate::media_capabilities::capabilities_for_stored_media(
        &job.input.mime_type,
        job.input.frame_count,
    );
    if capabilities.thumbnail_backend != Some(ThumbnailBackend::Inline) {
        return Ok(false);
    }

    if application
        .blobs()
        .find_thumbnail_path(&job.input.file_hash)
        .map_err(|error| format!("Thumbnail lookup failed: {error}"))?
        .is_none()
    {
        let extension = crate::blob_store::mime_to_extension(&job.input.mime_type);
        let original = application
            .blobs()
            .original_path_with_ext(&job.input.file_hash, Some(extension))
            .map_err(|error| format!("Original lookup failed: {error}"))?;
        let mut source = crate::media_processing::PreparedMediaSource::from_stored_metadata(
            original,
            &job.input.mime_type,
            job.input.duration_ms,
            job.input.frame_count,
        );
        let (bytes, thumbnail_extension) = source
            .render_inline_thumbnail_bytes(crate::media_processing::DEFAULT_THUMBNAIL_DIMENSIONS)
            .map_err(|error| format!("Initial thumbnail generation failed: {error}"))?;
        application
            .blobs()
            .write_thumbnail(&job.input.file_hash, &bytes, &thumbnail_extension)
            .map_err(|error| format!("Initial thumbnail write failed: {error}"))?;
    }
    Ok(true)
}

fn settle_succeeded(
    application: &Application,
    succeeded: &[(IngestJob, bool)],
) -> Result<(), String> {
    if succeeded.is_empty() {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    application.store().transaction_if_changed(|transaction| {
        let job_ids = succeeded
            .iter()
            .map(|(job, _)| job.ingest_job_id)
            .collect::<Vec<_>>();
        let encoded_job_ids = serde_json::to_string(&job_ids)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let changed = transaction.execute(
            "UPDATE ingest_job
             SET status = 'succeeded', payload_json = '{}', last_error = NULL, updated_at = ?1
             WHERE status = 'running'
               AND ingest_job_id IN (
                   SELECT CAST(value AS INTEGER) FROM json_each(?2)
               )",
            params![now, encoded_job_ids],
        )? != 0;

        let thumbnail_hashes = succeeded
            .iter()
            .filter(|(_, prepared)| *prepared)
            .map(|(job, _)| job.input.file_hash.as_str())
            .collect::<Vec<_>>();
        if !thumbnail_hashes.is_empty() {
            let encoded_hashes = serde_json::to_string(&thumbnail_hashes)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            transaction.execute(
                "DELETE FROM work_item
                 WHERE work_type = 'thumbnail'
                   AND file_id IN (
                       SELECT file_id FROM media_file
                       WHERE file_hash IN (SELECT value FROM json_each(?1))
                   )",
                [encoded_hashes],
            )?;
        }
        Ok(((), changed))
    })?;
    Ok(())
}

fn cleanup_staged_source(job: &IngestJob) {
    if job.delete_after_ingest {
        if let Err(error) = fs::remove_file(&job.source_path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %job.source_path,
                    error = %error,
                    "Could not remove completed ingest staging file"
                );
            }
        }
    }
}

pub(crate) fn existing_watch_job_keys(
    application: &Application,
) -> Result<std::collections::HashSet<String>, String> {
    application.store().read(|connection| {
        let mut statement = connection.prepare(
            "SELECT job_key FROM ingest_job
             WHERE source_kind = 'watch' AND status <> 'failed'",
        )?;
        let keys = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(keys)
    })
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

    use super::{
        claim_at, discard_abandoned_gallery_sources, enqueue_at,
        recover_settled_provisional_collections, reset_running_at, IngestJobSpec, IngestQueue,
    };
    use crate::app::{Application, Lifecycle};
    use crate::ingest_v2::{PreparedMediaInput, SourcePostInput};
    use crate::store::Store;

    const MEDIA_BYTES: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

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
            lifecycle: Lifecycle::Inbox,
            captured_at: None,
            source: Some(SourcePostInput {
                site_id: "example".to_string(),
                post_key: "post".to_string(),
                item_key: "item".to_string(),
                position: 0,
                post_complete: true,
                force_collection: false,
                group_post: true,
                canonical_post_url: None,
                canonical_media_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
            }),
            target_folder_id: None,
            target_folder_ids: Vec::new(),
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
    fn abandoned_unmaterialized_gallery_jobs_are_removed() {
        let (directory, app) = application();
        let source_path = directory.path().join("item.png");
        fs::write(&source_path, MEDIA_BYTES).unwrap();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO source_post (
                         site_id, post_key, created_at, updated_at
                     ) VALUES ('ehentai', 'post', ?1, ?1)",
                    ["2026-01-01T00:00:00Z"],
                )?;
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, state, created_at, updated_at
                     ) VALUES (?1, 'item', 0, 'downloaded', ?2, ?2)",
                    params![transaction.last_insert_rowid(), "2026-01-01T00:00:00Z"],
                )?;
                Ok(())
            })
            .unwrap();
        let mut job = spec(source_path.to_str().unwrap());
        job.input.source.as_mut().unwrap().site_id = "ehentai".to_string();
        enqueue_at(&app, &job, "2026-01-01T00:00:00Z").unwrap();

        assert_eq!(discard_abandoned_gallery_sources(&app).unwrap(), 1);
        app.store()
            .read(|connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM ingest_job", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    0
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM source_item", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    0
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM source_post", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    0
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn restart_promotes_a_settled_provisional_collection() {
        let (_directory, app) = application();
        let mut first = input();
        let source = first.source.as_mut().unwrap();
        source.post_complete = false;
        source.force_collection = true;
        first.tags = vec!["creator:example".to_string()];
        let result = app.ingest_prepared(&first).unwrap();

        app.store()
            .read(|connection| {
                let roots: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE item_id = ?1",
                    [result.root_item_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(roots, 0);
                Ok(())
            })
            .unwrap();

        assert_eq!(recover_settled_provisional_collections(&app).unwrap(), 1);
        app.store()
            .read(|connection| {
                let recovered: (String, i64) = connection.query_row(
                    "SELECT lr.lifecycle, sp.root_item_id
                     FROM library_root lr
                     JOIN source_post sp ON sp.root_item_id = lr.item_id
                     WHERE lr.item_id = ?1",
                    [result.root_item_id.0],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(recovered, ("inbox".to_string(), result.root_item_id.0));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn enqueue_reactivates_a_terminal_failed_job_with_fresh_staging() {
        let (directory, app) = application();
        let source = directory.path().join("retry.png");
        fs::write(&source, MEDIA_BYTES).unwrap();
        let queue = IngestQueue::start(&app).unwrap();
        let first = queue.enqueue(&spec(source.to_str().unwrap())).unwrap();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE ingest_job
                     SET status = 'failed', attempt_count = 8, last_error = 'broken'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let retried = queue.enqueue(&spec(source.to_str().unwrap())).unwrap();
        assert!(retried.inserted);
        assert_eq!(retried.ingest_job_id, first.ingest_job_id);
        app.store()
            .read(|connection| {
                let state: (String, i64, Option<String>) = connection.query_row(
                    "SELECT status, attempt_count, last_error FROM ingest_job",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(state, ("pending".to_string(), 0, None));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            fs::read_dir(directory.path().join("ingest-staging"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn enqueue_streams_the_canonical_hash_and_size_into_the_job() {
        let (directory, app) = application();
        let source = directory.path().join("streamed.png");
        fs::write(&source, MEDIA_BYTES).unwrap();
        let mut spec = spec(source.to_str().unwrap());
        spec.input.file_hash.clear();
        spec.input.size_bytes = 0;

        IngestQueue::start(&app).unwrap().enqueue(&spec).unwrap();

        app.store()
            .read(|connection| {
                let (source_path, payload, delete_after_ingest): (String, String, i64) = connection
                    .query_row(
                        "SELECT source_path, payload_json, delete_after_ingest FROM ingest_job",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                let staged: PreparedMediaInput = serde_json::from_str(&payload).unwrap();
                assert_eq!(
                    staged.file_hash,
                    hex::encode(crate::media_processing::get_hash_from_bytes(MEDIA_BYTES))
                );
                assert_eq!(staged.size_bytes, MEDIA_BYTES.len() as i64);
                assert!(std::path::Path::new(&source_path)
                    .starts_with(directory.path().join("blobs/f")));
                assert_eq!(delete_after_ingest, 0);
                assert_eq!(fs::read(source_path).unwrap(), MEDIA_BYTES);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn worker_rejects_a_staged_source_changed_after_enqueue() {
        let (directory, app) = application();
        let source = directory.path().join("changed.png");
        fs::write(&source, MEDIA_BYTES).unwrap();
        let queue = IngestQueue::start(&app).unwrap();
        queue.enqueue(&spec(source.to_str().unwrap())).unwrap();
        let staged_path: String = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT source_path FROM ingest_job", [], |row| row.get(0))
            })
            .unwrap();
        fs::write(staged_path, b"changed after staging").unwrap();

        let report = queue.run_batch(1).unwrap();

        assert_eq!(report.retried, 1);
        let (status, error): (String, Option<String>) = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT status, last_error FROM ingest_job", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(status, "pending");
        assert!(error
            .as_deref()
            .is_some_and(|value| value.contains("Staged ingest hash mismatch")));
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
        assert!(app
            .blobs()
            .find_thumbnail_path(&input().file_hash)
            .unwrap()
            .is_some());
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
                let thumbnail_jobs: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM work_item WHERE work_type = 'thumbnail'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(thumbnail_jobs, 0);
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
    fn enqueue_reactivates_succeeded_job_when_source_media_is_missing() {
        let (directory, app) = application();
        let source = directory.path().join("deleted-media.png");
        fs::write(&source, MEDIA_BYTES).unwrap();
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('example', 'post', 'now', 'now')",
                    [],
                )?;
                let source_post_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, state, created_at, updated_at
                     ) VALUES (?1, 'item', 0, 'downloaded', 'now', 'now')",
                    [source_post_id],
                )?;
                Ok(())
            })
            .unwrap();

        let queue = IngestQueue::start(&app).unwrap();
        let first = queue.enqueue(&spec(source.to_str().unwrap())).unwrap();
        assert_eq!(queue.run_batch(1).unwrap().ingested, 1);
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE source_item
                     SET state = 'pending', media_item_id = NULL, updated_at = 'now'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let retried = queue.enqueue(&spec(source.to_str().unwrap())).unwrap();
        assert!(retried.inserted);
        assert_eq!(retried.ingest_job_id, first.ingest_job_id);
        app.store()
            .read(|connection| {
                let job: (String, i64, Option<String>) = connection.query_row(
                    "SELECT status, attempt_count, last_error FROM ingest_job",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(job, ("pending".to_string(), 0, None));
                let source_item: (String, Option<i64>) = connection.query_row(
                    "SELECT state, media_item_id FROM source_item",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(source_item, ("downloaded".to_string(), None));
                Ok(())
            })
            .unwrap();

        assert_eq!(queue.run_batch(1).unwrap().ingested, 1);
        app.store()
            .read(|connection| {
                let source_item: (String, Option<i64>) = connection.query_row(
                    "SELECT state, media_item_id FROM source_item",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(source_item.0, "ingested");
                assert!(source_item.1.is_some());
                Ok(())
            })
            .unwrap();
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
