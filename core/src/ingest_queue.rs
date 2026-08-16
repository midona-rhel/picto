//! Durable ingest queue — committed source files wait here until the background
//! ingest worker imports them into the library.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::ingest::{
    apply_compiler_plan, build_ingest_change_impact, ingest_single_path, IngestBatchSummary,
    IngestSourceKind, SingleIngestDisposition, SingleIngestOutcome, SingleIngestRequest,
};
use crate::subscriptions::runtime_service::link_subscription_entity;
use crate::subscriptions::source_adapter::ParsedMetadata;
use crate::tags::logging::{preview_tag_strings, summarize_tag_strings};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestQueueItemPayload {
    pub request: SingleIngestRequest,
    pub subscription_metadata: Option<ParsedMetadata>,
    pub target_folder_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct IngestQueueEntry {
    pub queue_id: i64,
    pub source_kind: String,
    pub subscription_id: Option<i64>,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub cleanup_root: Option<String>,
    pub post_id: Option<String>,
    pub category: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct IngestQueueItem {
    pub item_id: i64,
    pub queue_id: i64,
    pub source_path: String,
    pub page_num: Option<i64>,
    pub payload_json: String,
    pub delete_after_ingest: bool,
    pub status: String,
    pub result_kind: Option<IngestQueueItemResultKind>,
    pub resolved_entity_hash: Option<String>,
    pub resolved_file_hash: Option<String>,
}

#[derive(Debug)]
pub struct StagedIngestSources {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug)]
struct IngestQueueItemCompletion {
    item_id: i64,
    result_kind: IngestQueueItemResultKind,
    resolved_entity_hash: String,
    resolved_file_hash: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IngestQueueCounts {
    pub queued: usize,
    pub ingesting: usize,
    pub ingested: usize,
    pub reused: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SubscriptionIngestCheckpoint {
    pub query_run_id: i64,
    pub files_downloaded: i64,
    pub posts_processed: i64,
    pub metadata_validated: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestQueueItemResultKind {
    Imported,
    Reused,
    Failed,
}

impl IngestQueueItemResultKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Reused => "reused",
            Self::Failed => "failed",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "imported" => Some(Self::Imported),
            "reused" => Some(Self::Reused),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

fn collect_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_files_recursive(&path));
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

/// Copy producer-owned files into durable library storage before queueing them.
/// Once this returns, the producer may delete its source directory without
/// invalidating the ingest queue.
pub async fn stage_ingest_sources(
    library_root: &Path,
    source_paths: &[PathBuf],
) -> Result<StagedIngestSources, String> {
    if source_paths.is_empty() {
        return Err("Cannot stage an empty ingest batch".to_string());
    }

    let root = library_root
        .join("ingest-queue")
        .join(format!("{:016x}", rand::random::<u64>()));
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|error| format!("Failed to create ingest staging directory: {error}"))?;

    let mut staged_paths = Vec::with_capacity(source_paths.len());
    for (index, source_path) in source_paths.iter().enumerate() {
        let mut staged_path = root.join(format!("{index:04}"));
        if let Some(extension) = source_path.extension() {
            staged_path.set_extension(extension);
        }
        let partial_path = staged_path.with_extension(format!(
            "{}.part",
            staged_path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("media")
        ));

        let stage_result = async {
            tokio::fs::copy(source_path, &partial_path)
                .await
                .map_err(|error| {
                    format!(
                        "Failed to stage ingest source {}: {error}",
                        source_path.display()
                    )
                })?;
            tokio::fs::rename(&partial_path, &staged_path)
                .await
                .map_err(|error| {
                    format!(
                        "Failed to commit staged ingest source {}: {error}",
                        source_path.display()
                    )
                })
        }
        .await;

        if let Err(error) = stage_result {
            let _ = tokio::fs::remove_dir_all(&root).await;
            return Err(error);
        }
        staged_paths.push(staged_path);
    }

    Ok(StagedIngestSources {
        root,
        paths: staged_paths,
    })
}

fn collect_import_paths(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let mut directories = Vec::<PathBuf>::new();
    let mut files = Vec::<PathBuf>::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|err| format!("Failed to read {}: {err}", directory.display()))?;
        let mut child_paths = Vec::<PathBuf>::new();
        for entry in entries {
            let entry = entry
                .map_err(|err| format!("Failed to read entry in {}: {err}", directory.display()))?;
            child_paths.push(entry.path());
        }
        child_paths.sort();
        for path in child_paths {
            if path.is_dir() {
                directories.push(path.clone());
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    directories.sort();
    files.sort();
    Ok((directories, files))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn queue_payload_post_id(payload: &IngestQueueItemPayload) -> &str {
    payload
        .subscription_metadata
        .as_ref()
        .and_then(|metadata| metadata.post_id.as_deref())
        .unwrap_or("?")
}

fn queue_payload_category(payload: &IngestQueueItemPayload) -> &str {
    payload
        .subscription_metadata
        .as_ref()
        .and_then(|metadata| metadata.category.as_deref())
        .unwrap_or("?")
}

fn log_ingest_queue_payload(
    stage: &str,
    queue_id: i64,
    item_id: i64,
    payload: &IngestQueueItemPayload,
) {
    let summary = summarize_tag_strings(&payload.request.tag_strings);
    info!(
        stage,
        queue_id,
        item_id,
        post_id = queue_payload_post_id(payload),
        category = queue_payload_category(payload),
        request_tag_count = summary.total,
        request_namespaced_tag_count = summary.namespaced_count(),
        request_creator_tag_count = summary.creator,
        request_character_tag_count = summary.character,
        request_series_tag_count = summary.series,
        request_general_tag_count = summary.general,
        request_meta_tag_count = summary.meta,
        request_other_namespaced_tag_count = summary.other_namespaced,
        request_tag_preview = ?preview_tag_strings(&payload.request.tag_strings, 5),
        "ingest queue payload"
    );
}

fn create_ingest_queue_entry(
    conn: &Connection,
    source_kind: &str,
    subscription_id: Option<i64>,
    query_id: Option<i64>,
    query_run_id: Option<i64>,
    cleanup_root: Option<&str>,
    post_id: Option<&str>,
    category: Option<&str>,
) -> rusqlite::Result<i64> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO ingest_queue (
             source_kind, subscription_id, query_id, query_run_id,
             cleanup_root, post_id, category, status, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?8)",
        params![
            source_kind,
            subscription_id,
            query_id,
            query_run_id,
            cleanup_root,
            post_id,
            category,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn add_ingest_queue_item(
    conn: &Connection,
    queue_id: i64,
    source_path: &str,
    page_num: Option<i64>,
    payload_json: &str,
    delete_after_ingest: bool,
) -> rusqlite::Result<i64> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO ingest_queue_item (
             queue_id, source_path, page_num, payload_json, delete_after_ingest,
             status, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?6)",
        params![
            queue_id,
            source_path,
            // Column is NOT NULL DEFAULT 0 (cursor ordering); an explicit NULL
            // bind bypasses the DEFAULT, so pageless items must bind 0.
            page_num.unwrap_or(0),
            payload_json,
            if delete_after_ingest { 1 } else { 0 },
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn lease_next_ingest_queue(conn: &Connection) -> rusqlite::Result<Option<IngestQueueEntry>> {
    let leased: Option<IngestQueueEntry> = conn
        .query_row(
            "SELECT queue_id, source_kind, subscription_id, query_id, query_run_id,
                    cleanup_root, post_id, category, status
             FROM ingest_queue
             WHERE status = 'pending'
             ORDER BY created_at ASC, queue_id ASC
             LIMIT 1",
            [],
            |row| {
                Ok(IngestQueueEntry {
                    queue_id: row.get(0)?,
                    source_kind: row.get(1)?,
                    subscription_id: row.get(2)?,
                    query_id: row.get(3)?,
                    query_run_id: row.get(4)?,
                    cleanup_root: row.get(5)?,
                    post_id: row.get(6)?,
                    category: row.get(7)?,
                    status: row.get(8)?,
                })
            },
        )
        .optional()?;
    if let Some(ref queue) = leased {
        conn.execute(
            "UPDATE ingest_queue SET status = 'running', updated_at = ?1 WHERE queue_id = ?2",
            params![now_rfc3339(), queue.queue_id],
        )?;
    }
    Ok(leased)
}

fn list_ingest_queue_items(
    conn: &Connection,
    queue_id: i64,
) -> rusqlite::Result<Vec<IngestQueueItem>> {
    let mut stmt = conn.prepare(
        "SELECT item_id, queue_id, source_path, page_num, payload_json, delete_after_ingest, status,
                result_kind, resolved_entity_hash, resolved_file_hash
         FROM ingest_queue_item
         WHERE queue_id = ?1
         ORDER BY page_num ASC, item_id ASC",
    )?;
    let rows = stmt.query_map([queue_id], |row| {
        let result_kind_raw: Option<String> = row.get(7)?;
        Ok(IngestQueueItem {
            item_id: row.get(0)?,
            queue_id: row.get(1)?,
            source_path: row.get(2)?,
            page_num: row.get(3)?,
            payload_json: row.get(4)?,
            delete_after_ingest: row.get::<_, i64>(5)? != 0,
            status: row.get(6)?,
            result_kind: result_kind_raw
                .as_deref()
                .and_then(IngestQueueItemResultKind::from_str),
            resolved_entity_hash: row.get(8)?,
            resolved_file_hash: row.get(9)?,
        })
    })?;
    rows.collect()
}

fn mark_ingest_queue_item_status(
    conn: &Connection,
    item_id: i64,
    status: &str,
    result_kind: Option<IngestQueueItemResultKind>,
    resolved_entity_hash: Option<&str>,
    resolved_file_hash: Option<&str>,
    last_error: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ingest_queue_item
         SET status = ?1,
             result_kind = ?2,
             resolved_entity_hash = ?3,
             resolved_file_hash = ?4,
             last_error = ?5,
             updated_at = ?6
         WHERE item_id = ?7",
        params![
            status,
            result_kind.map(IngestQueueItemResultKind::as_str),
            resolved_entity_hash,
            resolved_file_hash,
            last_error,
            now_rfc3339(),
            item_id
        ],
    )?;
    Ok(())
}

fn mark_ingest_queue_status(
    conn: &Connection,
    queue_id: i64,
    status: &str,
    last_error: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ingest_queue
         SET status = ?1, last_error = ?2, updated_at = ?3
         WHERE queue_id = ?4",
        params![status, last_error, now_rfc3339(), queue_id],
    )?;
    Ok(())
}

fn complete_ingest_queue(
    conn: &Connection,
    queue_id: i64,
    completions: &[IngestQueueItemCompletion],
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT item_id FROM ingest_queue_item
         WHERE queue_id = ?1 AND status != 'complete'
         ORDER BY item_id",
    )?;
    let expected = stmt
        .query_map([queue_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    let actual = completions
        .iter()
        .map(|completion| completion.item_id)
        .collect::<HashSet<_>>();
    if expected.is_empty() || actual != expected || actual.len() != completions.len() {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "completion set does not match ingest queue {queue_id}"
        )));
    }
    for completion in completions {
        mark_ingest_queue_item_status(
            conn,
            completion.item_id,
            "complete",
            Some(completion.result_kind),
            Some(&completion.resolved_entity_hash),
            Some(&completion.resolved_file_hash),
            None,
        )?;
    }
    mark_ingest_queue_status(conn, queue_id, "complete", None)
}

fn count_ingest_queue_by_subscription(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<IngestQueueCounts> {
    conn.query_row(
        "SELECT
             COALESCE(SUM(CASE WHEN i.status = 'pending' THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN i.status = 'running' THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN i.status = 'complete' AND i.result_kind = 'imported' THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN i.status = 'complete' AND i.result_kind = 'reused' THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN i.status = 'failed' THEN 1 ELSE 0 END), 0)
         FROM ingest_queue_item i
         JOIN ingest_queue q ON q.queue_id = i.queue_id
         WHERE q.subscription_id = ?1",
        [subscription_id],
        |row| {
            Ok(IngestQueueCounts {
                queued: row.get::<_, i64>(0)? as usize,
                ingesting: row.get::<_, i64>(1)? as usize,
                ingested: row.get::<_, i64>(2)? as usize,
                reused: row.get::<_, i64>(3)? as usize,
                failed: row.get::<_, i64>(4)? as usize,
            })
        },
    )
}

fn requeue_running_ingest(conn: &Connection) -> rusqlite::Result<()> {
    let now = now_rfc3339();
    conn.execute(
        "UPDATE ingest_queue_item
         SET status = 'pending', updated_at = ?1
         WHERE status = 'running'",
        [&now],
    )?;
    conn.execute(
        "UPDATE ingest_queue
         SET status = 'pending', updated_at = ?1
         WHERE status = 'running'",
        [&now],
    )?;
    Ok(())
}

fn delete_completed_ingest_queues(conn: &Connection) -> rusqlite::Result<usize> {
    Ok(conn.execute(
        "DELETE FROM ingest_queue
         WHERE status = 'complete'
           AND (
               query_run_id IS NULL
               OR NOT EXISTS (
                   SELECT 1
                   FROM subscription_query_run qr
                   JOIN subscription_run r ON r.run_id = qr.run_id
                   WHERE qr.query_run_id = ingest_queue.query_run_id
                     AND r.status = 'running'
               )
           )",
        [],
    )?)
}

fn count_retained_sources_under_root(
    conn: &Connection,
    cleanup_root: &str,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM ingest_queue_item i
         JOIN ingest_queue q ON q.queue_id = i.queue_id
         WHERE q.cleanup_root = ?1
           AND i.delete_after_ingest = 1
           AND i.status != 'complete'",
        [cleanup_root],
        |row| row.get(0),
    )
}

fn list_exact_hash_failed_queue_candidates(
    conn: &Connection,
) -> rusqlite::Result<Vec<(i64, i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT q.queue_id, i.item_id, i.source_path
         FROM ingest_queue q
         JOIN ingest_queue_item i ON i.queue_id = q.queue_id
         WHERE q.status = 'failed'
           AND i.status = 'failed'
           AND COALESCE(i.last_error, q.last_error, '') LIKE '%UNIQUE constraint failed: media_file.file_hash%'
         ORDER BY q.queue_id ASC",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

fn reset_ingest_queue_item_for_retry(
    conn: &Connection,
    queue_id: i64,
    item_id: i64,
) -> rusqlite::Result<()> {
    let now = now_rfc3339();
    conn.execute(
        "UPDATE ingest_queue
         SET status = 'pending', last_error = NULL, updated_at = ?1
         WHERE queue_id = ?2",
        params![now, queue_id],
    )?;
    conn.execute(
        "UPDATE ingest_queue_item
         SET status = 'pending',
             result_kind = NULL,
             resolved_entity_hash = NULL,
             resolved_file_hash = NULL,
             last_error = NULL,
             updated_at = ?1
         WHERE item_id = ?2",
        params![now_rfc3339(), item_id],
    )?;
    Ok(())
}

impl LibraryDatabase {
    pub(crate) async fn enqueue_ingest_queue(
        &self,
        source_kind: &str,
        subscription_id: Option<i64>,
        query_id: Option<i64>,
        query_run_id: Option<i64>,
        cleanup_root: Option<&Path>,
        post_id: Option<&str>,
        category: Option<&str>,
        items: Vec<(PathBuf, Option<i64>, IngestQueueItemPayload, bool)>,
        subscription_checkpoint: Option<SubscriptionIngestCheckpoint>,
    ) -> Result<i64, String> {
        let source_kind = source_kind.to_string();
        let cleanup_root = cleanup_root.map(|path| path.display().to_string());
        let post_id = post_id.map(ToOwned::to_owned);
        let category = category.map(ToOwned::to_owned);
        self.with_write(move |conn| {
            let queue_id = create_ingest_queue_entry(
                &conn,
                &source_kind,
                subscription_id,
                query_id,
                query_run_id,
                cleanup_root.as_deref(),
                post_id.as_deref(),
                category.as_deref(),
            )?;
            for (source_path, page_num, payload, delete_after_ingest) in items {
                let payload_json = serde_json::to_string(&payload)
                    .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
                let item_id = add_ingest_queue_item(
                    &conn,
                    queue_id,
                    &source_path.display().to_string(),
                    page_num,
                    &payload_json,
                    delete_after_ingest,
                )?;
                log_ingest_queue_payload("enqueue", queue_id, item_id, &payload);
            }
            if let Some(checkpoint) = subscription_checkpoint {
                crate::subscriptions::runtime_db::checkpoint_subscription_query_progress(
                    &conn,
                    checkpoint.query_run_id,
                    checkpoint.files_downloaded,
                    checkpoint.posts_processed,
                    checkpoint.metadata_validated,
                )?;
            }
            Ok(queue_id)
        })
    }

    pub async fn lease_next_ingest_queue(&self) -> Result<Option<IngestQueueEntry>, String> {
        self.with_write(lease_next_ingest_queue)
    }

    pub async fn list_ingest_queue_items(
        &self,
        queue_id: i64,
    ) -> Result<Vec<IngestQueueItem>, String> {
        self.with_read(move |conn| list_ingest_queue_items(conn, queue_id))
    }

    pub async fn mark_ingest_queue_item_running(&self, item_id: i64) -> Result<(), String> {
        self.with_write(move |conn| {
            mark_ingest_queue_item_status(conn, item_id, "running", None, None, None, None)
        })
    }

    pub async fn mark_ingest_queue_item_complete(
        &self,
        item_id: i64,
        result_kind: IngestQueueItemResultKind,
        resolved_entity_hash: Option<String>,
        resolved_file_hash: Option<String>,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            mark_ingest_queue_item_status(
                conn,
                item_id,
                "complete",
                Some(result_kind),
                resolved_entity_hash.as_deref(),
                resolved_file_hash.as_deref(),
                None,
            )
        })
    }

    pub async fn mark_ingest_queue_item_failed(
        &self,
        item_id: i64,
        last_error: &str,
    ) -> Result<(), String> {
        let last_error = last_error.to_string();
        self.with_write(move |conn| {
            mark_ingest_queue_item_status(
                conn,
                item_id,
                "failed",
                Some(IngestQueueItemResultKind::Failed),
                None,
                None,
                Some(&last_error),
            )
        })
    }

    async fn complete_ingest_queue(
        &self,
        queue_id: i64,
        completions: Vec<IngestQueueItemCompletion>,
    ) -> Result<(), String> {
        self.with_write(move |conn| complete_ingest_queue(conn, queue_id, &completions))
    }

    pub async fn mark_ingest_queue_failed(
        &self,
        queue_id: i64,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), String> {
        let status = status.to_string();
        let last_error = last_error.map(ToOwned::to_owned);
        self.with_write(move |conn| {
            mark_ingest_queue_status(conn, queue_id, &status, last_error.as_deref())
        })
    }

    pub async fn cleanup_ingest_queue(&self) -> Result<(), String> {
        self.with_write(move |conn| {
            requeue_running_ingest(conn)?;
            delete_completed_ingest_queues(conn)?;
            Ok(())
        })
    }

    pub async fn count_subscription_ingest_queue(
        &self,
        subscription_id: i64,
    ) -> Result<IngestQueueCounts, String> {
        self.with_read(move |conn| count_ingest_queue_by_subscription(conn, subscription_id))
    }

    pub async fn list_exact_hash_failed_queue_candidates(
        &self,
    ) -> Result<Vec<(i64, i64, String)>, String> {
        self.with_read(list_exact_hash_failed_queue_candidates)
    }

    pub async fn reset_ingest_queue_item_for_retry(
        &self,
        queue_id: i64,
        item_id: i64,
    ) -> Result<(), String> {
        self.with_write(move |conn| reset_ingest_queue_item_for_retry(conn, queue_id, item_id))
    }

    pub async fn has_retained_ingest_sources_for_root(
        &self,
        cleanup_root: &Path,
    ) -> Result<bool, String> {
        let cleanup_root = cleanup_root.display().to_string();
        self.with_read(move |conn| Ok(count_retained_sources_under_root(conn, &cleanup_root)? > 0))
    }
}

pub async fn enqueue_single_ingest_request(
    db: &LibraryDatabase,
    source_kind: IngestSourceKind,
    query_id: Option<i64>,
    query_run_id: Option<i64>,
    cleanup_root: Option<&Path>,
    source_path: &Path,
    request: SingleIngestRequest,
    subscription_metadata: Option<ParsedMetadata>,
    target_folder_id: Option<i64>,
    delete_after_ingest: bool,
) -> Result<i64, String> {
    let post_id = subscription_metadata
        .as_ref()
        .and_then(|m| m.post_id.clone());
    let category = subscription_metadata
        .as_ref()
        .and_then(|m| m.category.clone());
    let page_num = subscription_metadata
        .as_ref()
        .and_then(|m| m.page_num.map(i64::from));
    db.enqueue_ingest_queue(
        match source_kind {
            IngestSourceKind::Manual => "manual",
            IngestSourceKind::WatchFolder => "watch_folder",
            IngestSourceKind::Subscription => "subscription",
            IngestSourceKind::Migration => "migration",
        },
        request.subscription_id,
        query_id,
        query_run_id,
        cleanup_root,
        post_id.as_deref(),
        category.as_deref(),
        vec![(
            source_path.to_path_buf(),
            page_num,
            IngestQueueItemPayload {
                request,
                subscription_metadata,
                target_folder_id,
            },
            delete_after_ingest,
        )],
        None,
    )
    .await
}

pub async fn enqueue_manual_files(
    db: &LibraryDatabase,
    paths: Vec<String>,
    tag_strings: Option<Vec<String>>,
    source_urls: Option<Vec<String>>,
    initial_status: i64,
    target_folder_id: Option<i64>,
    library_root: Option<&Path>,
) -> Result<(), String> {
    let mut file_paths: Vec<PathBuf> = paths
        .into_iter()
        .flat_map(|p| {
            let path = PathBuf::from(&p);
            let path = path.canonicalize().unwrap_or(path);
            if path.is_dir() {
                collect_files_recursive(&path)
            } else {
                vec![path]
            }
        })
        .filter(|p| {
            p.is_file()
                && crate::media_processing::has_supported_extension(p)
                && !library_root.is_some_and(|root| p.starts_with(root))
        })
        .collect();
    file_paths.sort();
    file_paths.dedup();
    if file_paths.is_empty() {
        return Err("No supported media files to add".to_string());
    }

    for path in &file_paths {
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Manual,
            path: path.clone(),
            tag_strings: tag_strings.clone().unwrap_or_default(),
            source_urls: source_urls.clone().unwrap_or_default(),
            name: None,
            notes: None,
            created_at: None,
            initial_status,
            skip_thumbnail: false,
            tag_provenance_mask: crate::db::types::TAG_PROVENANCE_MANUAL,
            subscription_id: None,
        };
        enqueue_single_ingest_request(
            db,
            IngestSourceKind::Manual,
            None,
            None,
            None,
            path,
            request,
            None,
            target_folder_id,
            false,
        )
        .await?;
    }

    Ok(())
}

pub async fn enqueue_folder_import(
    db: &LibraryDatabase,
    path: String,
    parent_folder_id: Option<i64>,
    tag_strings: Option<Vec<String>>,
    source_urls: Option<Vec<String>>,
    initial_status: i64,
) -> Result<(), String> {
    let root_path = {
        let path = PathBuf::from(path);
        path.canonicalize().unwrap_or(path)
    };
    if !root_path.is_dir() {
        return Err(format!("Folder not found: {}", root_path.display()));
    }

    let (directories, mut file_paths) = collect_import_paths(&root_path)?;
    file_paths.retain(|path| crate::media_processing::has_supported_extension(path));
    file_paths.sort();
    file_paths.dedup();
    if file_paths.is_empty() {
        return Err("No supported media files to add".to_string());
    }
    let mut folder_cache = std::collections::HashMap::<PathBuf, i64>::new();
    let root_name = root_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Imported Folder")
        .to_string();
    let root_folder_id = db.create_folder(&root_name, parent_folder_id, None, None)?;
    folder_cache.insert(PathBuf::new(), root_folder_id);

    for directory in directories {
        let relative = match directory.strip_prefix(&root_path) {
            Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
            _ => continue,
        };
        let parent_relative = relative.parent().map(Path::to_path_buf).unwrap_or_default();
        let Some(parent_id) = folder_cache.get(&parent_relative).copied() else {
            continue;
        };
        let name = directory
            .file_name()
            .and_then(|entry| entry.to_str())
            .filter(|entry| !entry.is_empty())
            .unwrap_or("Imported Folder")
            .to_string();
        let folder_id = db.create_folder(&name, Some(parent_id), None, None)?;
        folder_cache.insert(relative, folder_id);
    }

    for file_path in &file_paths {
        let relative_parent = file_path
            .strip_prefix(&root_path)
            .ok()
            .and_then(|relative| relative.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let target_folder_id = folder_cache.get(&relative_parent).copied();
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Manual,
            path: file_path.clone(),
            tag_strings: tag_strings.clone().unwrap_or_default(),
            source_urls: source_urls.clone().unwrap_or_default(),
            name: None,
            notes: None,
            created_at: None,
            initial_status,
            skip_thumbnail: false,
            tag_provenance_mask: crate::db::types::TAG_PROVENANCE_MANUAL,
            subscription_id: None,
        };
        enqueue_single_ingest_request(
            db,
            IngestSourceKind::Manual,
            None,
            None,
            None,
            file_path,
            request,
            None,
            target_folder_id,
            false,
        )
        .await?;
    }

    Ok(())
}

pub async fn enqueue_watch_path(
    db: &LibraryDatabase,
    folder_id: i64,
    root_path: &Path,
    watch_subfolders: bool,
    watch_import_status_mode: &str,
    path: &Path,
) -> Result<(), String> {
    let relative_parent = path
        .strip_prefix(root_path)
        .ok()
        .and_then(|relative| relative.parent())
        .unwrap_or_else(|| Path::new(""));
    if !watch_subfolders && !relative_parent.as_os_str().is_empty() {
        return Ok(());
    }

    let target_folder_id = if relative_parent.as_os_str().is_empty() {
        folder_id
    } else {
        let mut current_folder_id = folder_id;
        for component in relative_parent.components() {
            let std::path::Component::Normal(name) = component else {
                continue;
            };
            let Some(name) = name.to_str() else {
                continue;
            };
            let child_id = match db.find_child_folder_id(current_folder_id, name)? {
                Some(folder_id) => folder_id,
                None => db.create_folder(name, Some(current_folder_id), None, None)?,
            };
            current_folder_id = child_id;
        }
        current_folder_id
    };

    let initial_status = match watch_import_status_mode {
        "inbox" => 0,
        "active" => 1,
        _ => 0,
    };
    let request = SingleIngestRequest {
        source_kind: IngestSourceKind::WatchFolder,
        path: path.to_path_buf(),
        tag_strings: Vec::new(),
        source_urls: Vec::new(),
        name: None,
        notes: None,
        created_at: None,
        initial_status,
        skip_thumbnail: false,
        tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
        subscription_id: None,
    };
    enqueue_single_ingest_request(
        db,
        IngestSourceKind::WatchFolder,
        None,
        None,
        None,
        path,
        request,
        None,
        Some(target_folder_id),
        false,
    )
    .await?;
    Ok(())
}

fn persist_subscription_post_member(
    canonical_db: &LibraryDatabase,
    subscription_id: i64,
    metadata: &ParsedMetadata,
    entity_hash: Option<&str>,
    status: &str,
) -> Result<(), String> {
    let Some(post_id) = metadata
        .post_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let site_id = metadata
        .category
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let Some(item_key) = metadata.item_key.clone() else {
        return Ok(());
    };
    let site_id = site_id.to_string();
    let post_id = post_id.to_string();
    let canonical_post_url = metadata.canonical_post_url.clone();
    let media_url = metadata.media_url.clone();
    let entity_id = entity_hash
        .map(|hash| canonical_db.resolve_entity_hashes(&[hash.to_string()]))
        .transpose()?
        .and_then(|ids| ids.into_iter().next());
    let status = status.to_string();
    let page_num = metadata.page_num.map(i64::from);
    canonical_db.with_write(move |conn| {
        crate::subscriptions::runtime_db::upsert_subscription_post_member(
            conn,
            crate::subscriptions::types::SubscriptionPostMemberUpsert {
                subscription_id,
                site_id: &site_id,
                post_id: &post_id,
                item_key: &item_key,
                page_num,
                canonical_post_url: canonical_post_url.as_deref(),
                media_url: media_url.as_deref(),
                entity_id,
                status: &status,
            },
        )
    })
}

async fn delete_source_file_if_owned(path: &str) {
    if let Err(error) = tokio::fs::remove_file(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(path, error = %error, "Failed to delete queued ingest source file");
        }
    }
}

async fn maybe_cleanup_root(db: &LibraryDatabase, cleanup_root: Option<&str>) {
    let Some(cleanup_root) = cleanup_root else {
        return;
    };
    let path = PathBuf::from(cleanup_root);
    match db.has_retained_ingest_sources_for_root(&path).await {
        Ok(true) => {}
        Ok(false) => {
            if let Err(error) = tokio::fs::remove_dir_all(&path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(path = %path.display(), error = %error, "Failed to clean up ingest root");
                }
            }
        }
        Err(error) => {
            warn!(path = %path.display(), error = %error, "Failed to inspect ingest root ownership")
        }
    }
}

async fn process_single_queue(
    db: &Arc<LibraryDatabase>,
    blob_store: &Arc<BlobStore>,
    queue: &IngestQueueEntry,
    item: &IngestQueueItem,
) -> Result<IngestQueueItemCompletion, String> {
    let mut payload: IngestQueueItemPayload =
        serde_json::from_str(&item.payload_json).map_err(|err| err.to_string())?;
    log_ingest_queue_payload("execute_single", queue.queue_id, item.item_id, &payload);
    payload.request.path = PathBuf::from(&item.source_path);
    db.mark_ingest_queue_item_running(item.item_id).await?;

    // Missing durable queue input is data loss, not a successful duplicate.
    if !payload.request.path.exists() {
        return Err(format!(
            "Queued ingest source is missing: {}",
            item.source_path
        ));
    }

    let outcome: SingleIngestOutcome = ingest_single_path(db, blob_store, &payload.request).await?;

    if let Some(subscription_id) = queue.subscription_id {
        link_subscription_entity(db, subscription_id, &outcome.entity_hash).await?;
    }

    let mut summary = IngestBatchSummary::default();
    summary.flags.merge(&outcome.flags);
    if let Some(folder_id) = payload.target_folder_id {
        let ids = db.resolve_entity_hashes(&[outcome.entity_hash.clone()])?;
        if !ids.is_empty() {
            db.add_folder_members(folder_id, &ids)?;
            summary.folder_ids.push(folder_id);
        }
    }
    if outcome.disposition.is_imported() {
        summary.imported_hashes.push(outcome.entity_hash.clone());
    } else {
        summary.skipped_hashes.push(outcome.entity_hash.clone());
    }

    if let (Some(subscription_id), Some(metadata)) = (
        queue.subscription_id,
        payload.subscription_metadata.as_ref(),
    ) {
        if let Some(item_key) = metadata.item_key.clone() {
            db.with_write(move |conn| {
                crate::subscriptions::runtime_db::resolve_subscription_download_attempt(
                    conn,
                    subscription_id,
                    queue.query_id,
                    &item_key,
                )
            })?;
        }
        persist_subscription_post_member(
            db,
            subscription_id,
            metadata,
            Some(outcome.entity_hash.as_str()),
            outcome.disposition.result_kind(),
        )?;
    }

    let should_emit = outcome.disposition.is_imported()
        || payload.target_folder_id.is_some()
        || summary.flags.status_changed
        || summary.flags.tags_changed
        || summary.flags.metadata_changed;
    if should_emit {
        apply_compiler_plan(db, &summary.flags, &summary.folder_ids);
        let extra_grid_scopes = if queue.source_kind == "subscription" {
            vec!["system:inbox".into()]
        } else {
            vec!["system:active".into(), "system:inbox".into()]
        };
        crate::events::emit_state_changed(
            "ingest_queue_single_commit",
            crate::ingest::attach_current_sidebar_counts(
                db,
                build_ingest_change_impact(&summary, extra_grid_scopes),
                summary.flags.duplicates_changed,
            ),
        );
    }

    let result_kind = match outcome.disposition {
        SingleIngestDisposition::Imported => IngestQueueItemResultKind::Imported,
        SingleIngestDisposition::Reused => IngestQueueItemResultKind::Reused,
    };
    crate::background_work::enqueue_missing_derivative_jobs(
        db,
        blob_store,
        std::slice::from_ref(&outcome.entity_hash),
    )
    .await;
    Ok(IngestQueueItemCompletion {
        item_id: item.item_id,
        result_kind,
        resolved_entity_hash: outcome.entity_hash,
        resolved_file_hash: outcome.file_hash,
    })
}

async fn repair_exact_hash_failed_queues(db: &Arc<LibraryDatabase>) -> Result<(), String> {
    let candidates = db.list_exact_hash_failed_queue_candidates().await?;
    for (queue_id, item_id, source_path) in candidates {
        let path = PathBuf::from(&source_path);
        if !path.exists() {
            continue;
        }
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                warn!(queue_id, path = %path.display(), error = %error, "Failed to read duplicate queue source during repair");
                continue;
            }
        };
        let file_hash = hex::encode(crate::media_processing::get_hash_from_bytes(&bytes));
        if db
            .get_existing_import_target_by_file_hash_write(&file_hash)?
            .is_some()
        {
            db.reset_ingest_queue_item_for_retry(queue_id, item_id)
                .await?;
            info!(
                queue_id,
                item_id, "Reset duplicate-failed ingest queue item for retry"
            );
        }
    }
    Ok(())
}

async fn process_queue_entry(
    db: &Arc<LibraryDatabase>,
    blob_store: &Arc<BlobStore>,
    queue: IngestQueueEntry,
) -> Result<(), String> {
    let items = db.list_ingest_queue_items(queue.queue_id).await?;
    if items.is_empty() {
        db.mark_ingest_queue_failed(queue.queue_id, "failed", Some("queue has no items"))
            .await?;
        return Ok(());
    }

    let result = async {
        let mut completions = Vec::new();
        for item in &items {
            if item.status == "complete" {
                continue;
            }
            completions.push(process_single_queue(db, blob_store, &queue, item).await?);
        }
        if completions.is_empty() {
            db.mark_ingest_queue_failed(queue.queue_id, "complete", None)
                .await?;
        } else {
            db.complete_ingest_queue(queue.queue_id, completions)
                .await?;
        }
        Ok::<(), String>(())
    }
    .await;

    match result {
        Ok(()) => {
            for item in &items {
                if item.delete_after_ingest {
                    delete_source_file_if_owned(&item.source_path).await;
                }
            }
            maybe_cleanup_root(db, queue.cleanup_root.as_deref()).await;
            Ok(())
        }
        Err(error) => {
            for item in &items {
                let _ = db.mark_ingest_queue_item_failed(item.item_id, &error).await;
            }
            db.mark_ingest_queue_failed(queue.queue_id, "failed", Some(&error))
                .await?;
            maybe_cleanup_root(db, queue.cleanup_root.as_deref()).await;
            Err(error)
        }
    }
}

async fn release_failed_subscription_archive(library_root: &Path, queue: &IngestQueueEntry) {
    let (Some(subscription_id), Some(query_id), Some(post_id)) = (
        queue.subscription_id,
        queue.query_id,
        queue.post_id.as_ref(),
    ) else {
        return;
    };
    let prefix =
        crate::subscriptions::archive::subscription_query_archive_prefix(subscription_id, query_id);
    if let Err(error) = crate::subscriptions::archive::clear_post_archive_entries_at_root(
        library_root,
        &prefix,
        std::slice::from_ref(post_id),
    )
    .await
    {
        warn!(queue_id = queue.queue_id, %error, "Failed to release subscription archive entry after ingest failure");
    }
}

pub async fn start_worker_loop(
    db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    library_root: PathBuf,
    running_subscriptions: crate::types::RunningSubscriptions,
    cancel: CancellationToken,
) {
    if let Err(error) = repair_exact_hash_failed_queues(&db).await {
        warn!(error = %error, "Failed to repair exact-hash ingest failures");
    }

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Ingest queue worker cancelled");
                return;
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(400)) => {}
        }

        let next = match db.lease_next_ingest_queue().await {
            Ok(next) => next,
            Err(error) => {
                warn!(error = %error, "Failed to lease ingest queue entry");
                continue;
            }
        };
        let Some(queue) = next else {
            continue;
        };

        let result = process_queue_entry(&db, &blob_store, queue.clone()).await;
        if result.is_err() {
            release_failed_subscription_archive(&library_root, &queue).await;
        }
        if let Some(query_run_id) = queue.query_run_id {
            let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
                db.as_ref(),
                &library_root,
            );
            if let Err(error) = crate::subscriptions::settlement::settle_query_run(
                &runtime,
                &running_subscriptions,
                query_run_id,
            )
            .await
            {
                warn!(query_run_id, error = %error, "Failed to settle subscription query run after ingest");
            }
        }
        if let Err(error) = result {
            warn!(queue_id = queue.queue_id, error = %error, "Ingest queue entry failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::fs;
    use std::io::Cursor;
    use tempfile::TempDir;

    fn setup_queue_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE ingest_queue (
                 queue_id INTEGER PRIMARY KEY,
                 source_kind TEXT NOT NULL,
                 subscription_id INTEGER,
                 query_id INTEGER,
                 query_run_id INTEGER,
                 cleanup_root TEXT,
                 post_id TEXT,
                 category TEXT,
                 status TEXT NOT NULL DEFAULT 'pending',
                 last_error TEXT,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE ingest_queue_item (
                 item_id INTEGER PRIMARY KEY,
                 queue_id INTEGER NOT NULL,
                 source_path TEXT NOT NULL,
                 page_num INTEGER,
                 payload_json TEXT NOT NULL,
                 delete_after_ingest INTEGER NOT NULL DEFAULT 0,
                 status TEXT NOT NULL DEFAULT 'pending',
                 result_kind TEXT,
                 resolved_entity_hash TEXT,
                 resolved_file_hash TEXT,
                 last_error TEXT,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );",
        )
        .unwrap();
    }

    fn write_image(path: &Path, color: u8) {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            8,
            8,
            Rgba([color, color, color, 255]),
        ));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[tokio::test]
    async fn staged_ingest_sources_survive_producer_cleanup() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let producer_root = temp.path().join("gallery-download");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&producer_root).unwrap();
        let source = producer_root.join("download.png");
        write_image(&source, 64);
        let expected = fs::read(&source).unwrap();

        let staged = stage_ingest_sources(&library_root, &[source])
            .await
            .unwrap();
        let staged_path = staged.paths[0].clone();
        fs::remove_dir_all(&producer_root).unwrap();

        assert!(staged.root.starts_with(library_root.join("ingest-queue")));
        assert_eq!(fs::read(&staged_path).unwrap(), expected);

        let db = Arc::new(LibraryDatabase::open(&library_root).unwrap());
        let blob_store = Arc::new(BlobStore::open(&library_root).unwrap());
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Subscription,
            path: staged_path.clone(),
            tag_strings: Vec::new(),
            source_urls: Vec::new(),
            name: None,
            notes: None,
            created_at: None,
            initial_status: 0,
            skip_thumbnail: false,
            tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
            subscription_id: None,
        };
        db.enqueue_ingest_queue(
            "subscription",
            None,
            None,
            None,
            Some(&staged.root),
            None,
            None,
            vec![(
                staged_path,
                None,
                IngestQueueItemPayload {
                    request,
                    subscription_metadata: None,
                    target_folder_id: None,
                },
                true,
            )],
            None,
        )
        .await
        .unwrap();

        let queue = db.lease_next_ingest_queue().await.unwrap().unwrap();
        process_queue_entry(&db, &blob_store, queue).await.unwrap();

        let entity_count = db
            .with_read(|conn| {
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .unwrap();
        assert_eq!(entity_count, 1);
        assert!(!staged.root.exists());
    }

    #[tokio::test]
    async fn subscription_enqueue_checkpoints_progress_atomically() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        fs::create_dir_all(&library_root).unwrap();
        let db = LibraryDatabase::open(&library_root).unwrap();
        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            &db,
            &library_root,
        );
        let subscription = runtime
            .create_subscription("Checkpoint".to_string(), None, None)
            .await
            .unwrap();
        let subscription_id = subscription.id.parse::<i64>().unwrap();
        let query = runtime
            .add_subscription_query(
                subscription.id,
                "gelbooru".to_string(),
                Some("search".to_string()),
                "1girl".to_string(),
                None,
            )
            .await
            .unwrap();
        let query_id = query.id.parse::<i64>().unwrap();
        let query_run_id = runtime
            .create_subscription_query_run(None, subscription_id, query_id)
            .await
            .unwrap();
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Subscription,
            path: library_root.join("source.png"),
            tag_strings: Vec::new(),
            source_urls: Vec::new(),
            name: None,
            notes: None,
            created_at: None,
            initial_status: 0,
            skip_thumbnail: false,
            tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
            subscription_id: Some(subscription_id),
        };
        let item = |path: &str| {
            (
                library_root.join(path),
                None,
                IngestQueueItemPayload {
                    request: request.clone(),
                    subscription_metadata: None,
                    target_folder_id: None,
                },
                true,
            )
        };

        db.enqueue_ingest_queue(
            "subscription",
            Some(subscription_id),
            Some(query_id),
            Some(query_run_id),
            None,
            Some("post-1"),
            Some("gelbooru"),
            vec![item("first.png"), item("second.png")],
            Some(SubscriptionIngestCheckpoint {
                query_run_id,
                files_downloaded: 2,
                posts_processed: 1,
                metadata_validated: 2,
            }),
        )
        .await
        .unwrap();

        let (run_counts, query_counts) = db
            .with_read(move |conn| {
                let run_counts = conn.query_row(
                    "SELECT files_downloaded, posts_processed, metadata_validated
                     FROM subscription_query_run WHERE query_run_id = ?1",
                    [query_run_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )?;
                let query_counts = conn.query_row(
                    "SELECT files_found, posts_found FROM subscription_query WHERE query_id = ?1",
                    [query_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )?;
                Ok((run_counts, query_counts))
            })
            .unwrap();
        assert_eq!(run_counts, (2, 1, 2));
        assert_eq!(query_counts, (2, 1));

        let error = db
            .enqueue_ingest_queue(
                "subscription",
                Some(subscription_id),
                Some(query_id),
                Some(query_run_id),
                None,
                Some("post-2"),
                Some("gelbooru"),
                vec![item("rollback.png")],
                Some(SubscriptionIngestCheckpoint {
                    query_run_id: i64::MAX,
                    files_downloaded: 1,
                    posts_processed: 1,
                    metadata_validated: 1,
                }),
            )
            .await
            .unwrap_err();
        assert!(error.contains("Query returned no rows"));
        assert_eq!(
            db.with_read(|conn| {
                conn.query_row("SELECT COUNT(*) FROM ingest_queue", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .unwrap(),
            1
        );

        runtime
            .record_subscription_query_source_completion(
                query_run_id,
                crate::subscriptions::types::SubscriptionQueryRunCompletion {
                    status: "failed".to_string(),
                    failure_kind: Some("runtime".to_string()),
                    error_message: Some("executor stopped".to_string()),
                    posts_processed: 0,
                    files_downloaded: 0,
                    files_skipped: 0,
                    metadata_validated: 0,
                    metadata_invalid: 0,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            db.with_read(move |conn| {
                conn.query_row(
                    "SELECT files_downloaded, posts_processed, metadata_validated
                     FROM subscription_query_run WHERE query_run_id = ?1",
                    [query_run_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
            })
            .unwrap(),
            (2, 1, 2)
        );
    }

    #[tokio::test]
    async fn subscription_single_ingest_links_the_imported_entity() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        let source = source_root.join("download.png");
        write_image(&source, 64);

        let db = Arc::new(LibraryDatabase::open(&library_root).unwrap());
        let blob_store = Arc::new(BlobStore::open(&library_root).unwrap());
        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            &db,
            &library_root,
        );
        let subscription = runtime
            .create_subscription("Artist".to_string(), None, None)
            .await
            .unwrap();
        let subscription_id = subscription.id.parse::<i64>().unwrap();
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Subscription,
            path: source.clone(),
            tag_strings: Vec::new(),
            source_urls: Vec::new(),
            name: None,
            notes: None,
            created_at: None,
            initial_status: 0,
            skip_thumbnail: false,
            tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
            subscription_id: Some(subscription_id),
        };
        db.enqueue_ingest_queue(
            "subscription",
            Some(subscription_id),
            None,
            None,
            None,
            Some("post-1"),
            Some("site"),
            vec![(
                source,
                None,
                IngestQueueItemPayload {
                    request,
                    subscription_metadata: None,
                    target_folder_id: None,
                },
                false,
            )],
            None,
        )
        .await
        .unwrap();

        let queue = db.lease_next_ingest_queue().await.unwrap().unwrap();
        process_queue_entry(&db, &blob_store, queue).await.unwrap();

        db.with_read(move |conn| {
            let linked: i64 = conn.query_row(
                "SELECT COUNT(*) FROM subscription_entity WHERE subscription_id = ?1",
                [subscription_id],
                |row| row.get(0),
            )?;
            assert_eq!(linked, 1);
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn multi_file_post_stays_ordered_but_each_media_entity_is_independent() {
        use crate::db::types::{EntityTarget, EntityTargetKind};
        use crate::engine::folders::MembershipOperation;
        use crate::engine::tags::TagOperation;
        use crate::engine::ApplicationEngine;

        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();

        let db = Arc::new(LibraryDatabase::open(&library_root).unwrap());
        let blob_store = Arc::new(BlobStore::open(&library_root).unwrap());
        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            &db,
            &library_root,
        );
        let subscription = runtime
            .create_subscription("Three files".to_string(), None, None)
            .await
            .unwrap();
        let subscription_id = subscription.id.parse::<i64>().unwrap();
        let shared_post_url = "https://example.test/posts/42";
        let shared_description = "Shared post description";

        let mut queue_items = Vec::new();
        for page_num in 1..=3 {
            let source = source_root.join(format!("page-{page_num}.png"));
            write_image(&source, 32 * page_num as u8);
            let media_url = format!("https://cdn.example.test/42/{page_num}.png");
            queue_items.push((
                source.clone(),
                Some(page_num),
                IngestQueueItemPayload {
                    request: SingleIngestRequest {
                        source_kind: IngestSourceKind::Subscription,
                        path: source,
                        tag_strings: vec!["general:shared".to_string()],
                        source_urls: vec![shared_post_url.to_string(), media_url.clone()],
                        name: Some(format!("Page {page_num}")),
                        notes: Some(shared_description.to_string()),
                        created_at: Some("2026-08-16T00:00:00Z".to_string()),
                        initial_status: 0,
                        skip_thumbnail: false,
                        tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
                        subscription_id: Some(subscription_id),
                    },
                    subscription_metadata: Some(ParsedMetadata {
                        tags: vec![("general".to_string(), "shared".to_string())],
                        description: Some(shared_description.to_string()),
                        source_url: Some(shared_post_url.to_string()),
                        source_urls: vec![shared_post_url.to_string(), media_url.clone()],
                        media_url: Some(media_url),
                        post_id: Some("42".to_string()),
                        created_at: Some("2026-08-16T00:00:00Z".to_string()),
                        category: Some("fixture".to_string()),
                        page_num: Some(page_num as u32),
                        page_count: Some(3),
                        canonical_post_url: Some(shared_post_url.to_string()),
                        item_key: Some(format!("fixture:42:{page_num}")),
                        ..Default::default()
                    }),
                    target_folder_id: None,
                },
                false,
            ));
        }

        db.enqueue_ingest_queue(
            "subscription",
            Some(subscription_id),
            None,
            None,
            None,
            Some("42"),
            Some("fixture"),
            queue_items,
            None,
        )
        .await
        .unwrap();
        let queue = db.lease_next_ingest_queue().await.unwrap().unwrap();
        process_queue_entry(&db, &blob_store, queue).await.unwrap();

        let members: Vec<(i64, i64, String, String, String)> = db
            .with_read(move |conn| {
                conn.prepare(
                    "SELECT spm.page_num, me.entity_id, me.entity_hash, me.notes,
                            me.source_urls_json
                       FROM subscription_post_member spm
                       JOIN media_entity me ON me.entity_id = spm.entity_id
                      WHERE spm.subscription_id = ?1 AND spm.post_id = '42'
                      ORDER BY spm.page_num",
                )?
                .query_map([subscription_id], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(
            members.iter().map(|row| row.0).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            members
                .iter()
                .map(|row| row.2.as_str())
                .collect::<HashSet<_>>()
                .len(),
            3
        );
        assert!(members.iter().all(|row| row.3 == shared_description));
        assert!(members.iter().all(|row| row.4.contains(shared_post_url)));
        db.with_read(|conn| {
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
                    .get::<_, i64>(0))?,
                3
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row
                    .get::<_, i64>(0))?,
                3
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM folder", [], |row| row
                    .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM folder_member", [], |row| row
                    .get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .unwrap();
        let counts = db.get_scope_counts().unwrap();
        assert_eq!((counts.active, counts.inbox, counts.trash), (0, 3, 0));

        let engine = ApplicationEngine::new(db.clone());
        let all_hashes = members.iter().map(|row| row.2.clone()).collect::<Vec<_>>();
        let target = |hashes: Vec<String>| EntityTarget {
            kind: EntityTargetKind::EntityHashes,
            entity_hashes: Some(hashes),
            query: None,
            excluded_entity_hashes: None,
        };
        engine
            .set_entity_status(target(all_hashes.clone()), 1)
            .unwrap();
        let folder_id = engine.create_folder("Ordered", None, None, None).unwrap();
        engine
            .update_folder_membership(
                target(all_hashes.clone()),
                folder_id,
                MembershipOperation::Add,
            )
            .unwrap();
        let reordered = vec![(members[2].1, 0), (members[0].1, 1), (members[1].1, 2)];
        engine.reorder_folder_items(folder_id, &reordered).unwrap();
        engine
            .apply_entity_tags(
                target(vec![members[1].2.clone()]),
                TagOperation::Add,
                &["general:middle-only".to_string()],
                None,
            )
            .unwrap();

        let deleted_blob = blob_store
            .original_path_with_ext(&members[1].2, Some("png"))
            .unwrap();
        assert!(deleted_blob.exists());
        let deleted = engine
            .delete_entities(target(vec![members[1].2.clone()]))
            .unwrap();
        assert_eq!(deleted.entity_ids, vec![members[1].1]);
        assert_eq!(deleted.freed_file_hashes, vec![members[1].2.clone()]);
        db.enqueue_blob_delete_and_attempt(&blob_store, &members[1].2)
            .unwrap();
        assert!(!deleted_blob.exists());

        db.with_read(|conn| {
            let remaining: Vec<i64> = conn
                .prepare("SELECT entity_id FROM media_entity ORDER BY entity_id")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(remaining, vec![members[0].1, members[2].1]);
            let provenance: Vec<(i64, Option<i64>, String)> = conn
                .prepare(
                    "SELECT page_num, entity_id, status
                       FROM subscription_post_member
                      WHERE subscription_id = ?1 AND post_id = '42'
                      ORDER BY page_num",
                )?
                .query_map([subscription_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(provenance[0].1, Some(members[0].1));
            assert_eq!(provenance[1], (2, None, "deleted".to_string()));
            assert_eq!(provenance[2].1, Some(members[2].1));
            let folder_order: Vec<i64> = conn
                .prepare(
                    "SELECT entity_id FROM folder_member
                      WHERE folder_id = ?1 ORDER BY position_rank, entity_id",
                )?
                .query_map([folder_id], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(folder_order, vec![members[2].1, members[0].1]);
            let local_tag_count: i64 = conn.query_row(
                "SELECT COUNT(*)
                   FROM entity_tag et JOIN tag t ON t.tag_id = et.tag_id
                  WHERE t.subtag = 'middle-only'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(local_tag_count, 0);
            Ok(())
        })
        .unwrap();
        let counts = db.get_scope_counts().unwrap();
        assert_eq!((counts.active, counts.inbox, counts.trash), (2, 0, 0));
    }

    #[tokio::test]
    async fn failed_subscription_ingest_releases_only_its_post_archive_entry() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("gdl-archive.sqlite3");
        let conn = Connection::open(&archive_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE archive (entry TEXT PRIMARY KEY);
             INSERT INTO archive VALUES ('picto_s7_q9_site_post-42_file');
             INSERT INTO archive VALUES ('picto_s7_q9_site_post-43_file');",
        )
        .unwrap();
        drop(conn);
        let queue = IngestQueueEntry {
            queue_id: 1,
            source_kind: "subscription".to_string(),
            subscription_id: Some(7),
            query_id: Some(9),
            query_run_id: Some(11),
            cleanup_root: None,
            post_id: Some("post-42".to_string()),
            category: Some("site".to_string()),
            status: "failed".to_string(),
        };

        release_failed_subscription_archive(temp.path(), &queue).await;

        let conn = Connection::open(&archive_path).unwrap();
        let entries = conn
            .prepare("SELECT entry FROM archive ORDER BY entry")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(entries, vec!["picto_s7_q9_site_post-43_file"]);
    }

    #[test]
    fn count_ingest_queue_by_subscription_reports_imported_reused_and_failed_items() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_id, source_kind, subscription_id, status, created_at, updated_at
             ) VALUES (1, 'subscription', 7, 'running', 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO ingest_queue_item (
                 item_id, queue_id, source_path, payload_json, delete_after_ingest,
                 status, result_kind, created_at, updated_at
             ) VALUES
                 (1, 1, '/tmp/a', '{}', 1, 'pending', NULL, 'now', 'now'),
                 (2, 1, '/tmp/b', '{}', 1, 'running', NULL, 'now', 'now'),
                 (3, 1, '/tmp/c', '{}', 1, 'complete', 'imported', 'now', 'now'),
                 (4, 1, '/tmp/d', '{}', 1, 'complete', 'reused', 'now', 'now'),
                 (5, 1, '/tmp/e', '{}', 1, 'failed', 'failed', 'now', 'now');",
        )
        .unwrap();

        let counts = count_ingest_queue_by_subscription(&conn, 7).unwrap();
        assert_eq!(counts.queued, 1);
        assert_eq!(counts.ingesting, 1);
        assert_eq!(counts.ingested, 1);
        assert_eq!(counts.reused, 1);
        assert_eq!(counts.failed, 1);
    }

    #[test]
    fn startup_requeues_running_ingest_without_discarding_it() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_id, source_kind, subscription_id, status, created_at, updated_at
             ) VALUES (1, 'subscription', 7, 'running', 'old', 'old')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ingest_queue_item (
                 item_id, queue_id, source_path, payload_json, delete_after_ingest,
                 status, created_at, updated_at
             ) VALUES (1, 1, '/tmp/a', '{}', 1, 'running', 'old', 'old')",
            [],
        )
        .unwrap();

        requeue_running_ingest(&conn).unwrap();

        let queue_status: String = conn
            .query_row(
                "SELECT status FROM ingest_queue WHERE queue_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let item_status: String = conn
            .query_row(
                "SELECT status FROM ingest_queue_item WHERE item_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queue_status, "pending");
        assert_eq!(item_status, "pending");
    }

    #[test]
    fn queue_completion_requires_and_commits_every_item() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_id, source_kind, subscription_id, status, created_at, updated_at
             ) VALUES (1, 'subscription', 7, 'running', 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO ingest_queue_item (
                 item_id, queue_id, source_path, payload_json, delete_after_ingest,
                 status, created_at, updated_at
             ) VALUES
                 (1, 1, '/tmp/a', '{}', 1, 'running', 'now', 'now'),
                 (2, 1, '/tmp/b', '{}', 1, 'running', 'now', 'now');",
        )
        .unwrap();
        let completion = |item_id| IngestQueueItemCompletion {
            item_id,
            result_kind: IngestQueueItemResultKind::Imported,
            resolved_entity_hash: format!("entity-{item_id}"),
            resolved_file_hash: format!("file-{item_id}"),
        };

        assert!(complete_ingest_queue(&conn, 1, &[completion(1)]).is_err());
        let unchanged: (String, i64) = conn
            .query_row(
                "SELECT q.status,
                        (SELECT COUNT(*) FROM ingest_queue_item WHERE queue_id = q.queue_id AND status = 'running')
                 FROM ingest_queue q WHERE q.queue_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(unchanged, ("running".to_string(), 2));

        complete_ingest_queue(&conn, 1, &[completion(1), completion(2)]).unwrap();
        let completed: (String, i64) = conn
            .query_row(
                "SELECT q.status,
                        (SELECT COUNT(*) FROM ingest_queue_item WHERE queue_id = q.queue_id AND status = 'complete')
                 FROM ingest_queue q WHERE q.queue_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(completed, ("complete".to_string(), 2));
    }

    #[tokio::test]
    async fn manual_file_queue_adds_imported_entity_to_target_folder() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        let source = source_root.join("manual.png");
        write_image(&source, 96);

        let db = Arc::new(LibraryDatabase::open(&library_root).unwrap());
        let blob_store = Arc::new(BlobStore::open(&library_root).unwrap());
        let folder_id = db.create_folder("Imported", None, None, None).unwrap();
        enqueue_manual_files(
            db.as_ref(),
            vec![source.display().to_string()],
            None,
            None,
            1,
            Some(folder_id),
            Some(&library_root),
        )
        .await
        .unwrap();

        let queue = db.lease_next_ingest_queue().await.unwrap().unwrap();
        process_queue_entry(&db, &blob_store, queue).await.unwrap();

        db.with_read(|conn| {
            let member_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM folder_member WHERE folder_id = ?1",
                params![folder_id],
                |row| row.get(0),
            )?;
            assert_eq!(member_count, 1);
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn manual_file_enqueue_rejects_zero_supported_media() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        fs::create_dir_all(&library_root).unwrap();
        let db = LibraryDatabase::open(&library_root).unwrap();

        let error = enqueue_manual_files(
            &db,
            vec![temp.path().join("missing.png").display().to_string()],
            None,
            None,
            1,
            None,
            Some(&library_root),
        )
        .await
        .unwrap_err();

        assert_eq!(error, "No supported media files to add");
    }

    #[tokio::test]
    async fn structured_folder_enqueue_rejects_zero_supported_media_before_creating_folders() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        fs::write(source_root.join("ignored.txt"), "not media").unwrap();
        let db = LibraryDatabase::open(&library_root).unwrap();

        let error =
            enqueue_folder_import(&db, source_root.display().to_string(), None, None, None, 1)
                .await
                .unwrap_err();

        assert_eq!(error, "No supported media files to add");
        db.with_read(|conn| {
            let folder_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM folder", [], |row| row.get(0))?;
            assert_eq!(folder_count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn reset_ingest_queue_item_for_retry_clears_failure_fields() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_id, source_kind, subscription_id, status, last_error, created_at, updated_at
             ) VALUES (9, 'subscription', 1, 'failed', 'boom', 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ingest_queue_item (
                 item_id, queue_id, source_path, payload_json, delete_after_ingest,
                 status, result_kind, resolved_entity_hash, resolved_file_hash, last_error, created_at, updated_at
             ) VALUES (
                 4, 9, '/tmp/a', '{}', 1,
                 'failed', 'failed', 'eh', 'fh', 'boom', 'now', 'now'
             )",
            [],
        )
        .unwrap();

        reset_ingest_queue_item_for_retry(&conn, 9, 4).unwrap();

        let (queue_status, queue_error): (String, Option<String>) = conn
            .query_row(
                "SELECT status, last_error FROM ingest_queue WHERE queue_id = 9",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(queue_status, "pending");
        assert_eq!(queue_error, None);

        let row: (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT status, result_kind, resolved_entity_hash, resolved_file_hash, last_error
                 FROM ingest_queue_item
                 WHERE item_id = 4",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(row.0, "pending");
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
        assert_eq!(row.3, None);
        assert_eq!(row.4, None);
    }

    #[test]
    fn failed_queue_candidates_only_include_exact_hash_failures() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute_batch(
            "INSERT INTO ingest_queue (
                 queue_id, source_kind, subscription_id, status, last_error, created_at, updated_at
             ) VALUES
                 (1, 'subscription', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (2, 'subscription', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (3, 'subscription', 1, 'failed', 'other', 'now', 'now');
             INSERT INTO ingest_queue_item (
                 item_id, queue_id, source_path, payload_json, delete_after_ingest,
                 status, last_error, created_at, updated_at
             ) VALUES
                 (11, 1, '/tmp/a', '{}', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (12, 2, '/tmp/b', '{}', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (13, 3, '/tmp/c', '{}', 1, 'failed', 'other', 'now', 'now');",
        )
        .unwrap();

        let candidates = list_exact_hash_failed_queue_candidates(&conn).unwrap();
        assert_eq!(
            candidates,
            vec![(1, 11, "/tmp/a".to_string()), (2, 12, "/tmp/b".to_string()),]
        );
    }
}
