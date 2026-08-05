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
    apply_compiler_plan, build_ingest_change_impact, ingest_single_path, materialize_collection,
    CollectionIngestMember, IngestBatchSummary, IngestSourceKind, SingleIngestDisposition,
    SingleIngestOutcome, SingleIngestRequest,
};
use crate::subscriptions::runtime_service::SubscriptionRuntimeService;
use crate::subscriptions::source_adapter::ParsedMetadata;
use crate::tags::logging::{preview_tag_strings, summarize_tag_strings};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestQueueKind {
    Single,
    Collection,
}

impl IngestQueueKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Collection => "collection",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "single" => Some(Self::Single),
            "collection" => Some(Self::Collection),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestQueueItemPayload {
    pub request: SingleIngestRequest,
    pub subscription_metadata: Option<ParsedMetadata>,
    pub target_folder_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct IngestQueueEntry {
    pub queue_id: i64,
    pub queue_kind: IngestQueueKind,
    pub source_kind: String,
    pub subscription_id: Option<i64>,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub cleanup_root: Option<String>,
    pub post_id: Option<String>,
    pub category: Option<String>,
    pub preferred_name: Option<String>,
    pub expected_count: Option<i64>,
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

/// Collections are image aggregates. Extensions only narrow the manual picker
/// candidates; the bytes decide whether a collection import is valid.
async fn preflight_collection_sources(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let mime = crate::media_processing::get_mime(path)
            .await
            .map_err(|error| {
                format!(
                    "Failed to inspect collection source {}: {error}",
                    path.display()
                )
            })?;
        if !crate::media_processing::is_image(mime) {
            return Err(format!(
                "Collections can contain images only: {} is {}",
                path.display(),
                mime.mime_string()
            ));
        }
    }
    Ok(())
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
    queue_kind: IngestQueueKind,
    source_kind: &str,
    subscription_id: Option<i64>,
    query_id: Option<i64>,
    query_run_id: Option<i64>,
    cleanup_root: Option<&str>,
    post_id: Option<&str>,
    category: Option<&str>,
    preferred_name: Option<&str>,
    expected_count: Option<i64>,
) -> rusqlite::Result<i64> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO ingest_queue (
             queue_kind, source_kind, subscription_id, query_id, query_run_id,
             cleanup_root, post_id, category, preferred_name, expected_count,
             status, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'pending', ?11, ?11)",
        params![
            queue_kind.as_str(),
            source_kind,
            subscription_id,
            query_id,
            query_run_id,
            cleanup_root,
            post_id,
            category,
            preferred_name,
            expected_count,
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
            "SELECT queue_id, queue_kind, source_kind, subscription_id, query_id, query_run_id,
                    cleanup_root, post_id, category, preferred_name, expected_count, status
             FROM ingest_queue
             WHERE status = 'pending'
             ORDER BY created_at ASC, queue_id ASC
             LIMIT 1",
            [],
            |row| {
                let queue_kind_raw: String = row.get(1)?;
                Ok(IngestQueueEntry {
                    queue_id: row.get(0)?,
                    queue_kind: IngestQueueKind::from_str(&queue_kind_raw).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("invalid ingest queue kind: {queue_kind_raw}"),
                            )),
                        )
                    })?,
                    source_kind: row.get(2)?,
                    subscription_id: row.get(3)?,
                    query_id: row.get(4)?,
                    query_run_id: row.get(5)?,
                    cleanup_root: row.get(6)?,
                    post_id: row.get(7)?,
                    category: row.get(8)?,
                    preferred_name: row.get(9)?,
                    expected_count: row.get(10)?,
                    status: row.get(11)?,
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
    let mut stmt =
        conn.prepare("SELECT item_id FROM ingest_queue_item WHERE queue_id = ?1 ORDER BY item_id")?;
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

fn subscription_run_id_for_ingest_queue(
    conn: &Connection,
    queue_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT qr.run_id
         FROM ingest_queue q
         JOIN subscription_query_run qr ON qr.query_run_id = q.query_run_id
         WHERE q.queue_id = ?1",
        [queue_id],
        |row| row.get::<_, Option<i64>>(0),
    )
    .optional()
    .map(Option::flatten)
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

fn list_duplicate_failed_single_queue_candidates(
    conn: &Connection,
) -> rusqlite::Result<Vec<(i64, i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT q.queue_id, i.item_id, i.source_path
         FROM ingest_queue q
         JOIN ingest_queue_item i ON i.queue_id = q.queue_id
         WHERE q.queue_kind = 'single'
           AND q.status = 'failed'
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
    pub async fn enqueue_ingest_queue(
        &self,
        queue_kind: IngestQueueKind,
        source_kind: &str,
        subscription_id: Option<i64>,
        query_id: Option<i64>,
        query_run_id: Option<i64>,
        cleanup_root: Option<&Path>,
        post_id: Option<&str>,
        category: Option<&str>,
        preferred_name: Option<&str>,
        expected_count: Option<i64>,
        items: Vec<(PathBuf, Option<i64>, IngestQueueItemPayload, bool)>,
    ) -> Result<i64, String> {
        let source_kind = source_kind.to_string();
        let cleanup_root = cleanup_root.map(|path| path.display().to_string());
        let post_id = post_id.map(ToOwned::to_owned);
        let category = category.map(ToOwned::to_owned);
        let preferred_name = preferred_name.map(ToOwned::to_owned);
        self.with_write(move |conn| {
            let queue_id = create_ingest_queue_entry(
                &conn,
                queue_kind,
                &source_kind,
                subscription_id,
                query_id,
                query_run_id,
                cleanup_root.as_deref(),
                post_id.as_deref(),
                category.as_deref(),
                preferred_name.as_deref(),
                expected_count,
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

    pub async fn subscription_run_id_for_ingest_queue(
        &self,
        queue_id: i64,
    ) -> Result<Option<i64>, String> {
        self.with_read(move |conn| subscription_run_id_for_ingest_queue(conn, queue_id))
    }

    pub async fn list_duplicate_failed_single_queue_candidates(
        &self,
    ) -> Result<Vec<(i64, i64, String)>, String> {
        self.with_read(list_duplicate_failed_single_queue_candidates)
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
    let name = request.name.clone();
    let post_id = subscription_metadata
        .as_ref()
        .and_then(|m| m.post_id.clone());
    let category = subscription_metadata
        .as_ref()
        .and_then(|m| m.category.clone());
    let expected_count = subscription_metadata
        .as_ref()
        .and_then(|m| m.page_count.map(i64::from));
    let page_num = subscription_metadata
        .as_ref()
        .and_then(|m| m.page_num.map(i64::from));
    db.enqueue_ingest_queue(
        IngestQueueKind::Single,
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
        name.as_deref(),
        expected_count,
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

pub async fn enqueue_manual_collection(
    db: &LibraryDatabase,
    paths: Vec<String>,
    name: String,
    tag_strings: Option<Vec<String>>,
    source_urls: Option<Vec<String>>,
    initial_status: i64,
    target_folder_id: Option<i64>,
    library_root: Option<&Path>,
) -> Result<i64, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Collection name cannot be blank".to_string());
    }

    let mut file_paths: Vec<PathBuf> = paths
        .into_iter()
        .flat_map(|raw_path| {
            let path = PathBuf::from(raw_path);
            let path = path.canonicalize().unwrap_or(path);
            if path.is_dir() {
                collect_files_recursive(&path)
            } else {
                vec![path]
            }
        })
        .filter(|path| {
            path.is_file()
                && crate::media_processing::has_supported_extension(path)
                && !library_root.is_some_and(|root| path.starts_with(root))
        })
        .collect();
    file_paths.sort();
    file_paths.dedup();
    if file_paths.is_empty() {
        return Err("No supported media files to add to collection".to_string());
    }
    preflight_collection_sources(&file_paths).await?;

    let tag_strings = tag_strings.unwrap_or_default();
    let source_urls = source_urls.unwrap_or_default();
    let items = file_paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let request = SingleIngestRequest {
                source_kind: IngestSourceKind::Manual,
                path: path.clone(),
                tag_strings: tag_strings.clone(),
                source_urls: source_urls.clone(),
                name: None,
                notes: None,
                created_at: None,
                initial_status,
                skip_thumbnail: false,
                tag_provenance_mask: crate::db::types::TAG_PROVENANCE_MANUAL,
                subscription_id: None,
            };
            (
                path,
                Some(index as i64),
                IngestQueueItemPayload {
                    request,
                    subscription_metadata: None,
                    target_folder_id,
                },
                false,
            )
        })
        .collect();

    db.enqueue_ingest_queue(
        IngestQueueKind::Collection,
        "manual",
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&name),
        None,
        items,
    )
    .await
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

fn subscription_item_key(metadata: &ParsedMetadata) -> Option<String> {
    metadata.item_key.clone().or_else(|| {
        let category = metadata.category.as_deref()?;
        let target = metadata
            .post_id
            .as_deref()
            .or(metadata.canonical_post_url.as_deref())
            .or(metadata.media_url.as_deref())?;
        Some(format!(
            "{category}:{target}:{}",
            metadata.page_num.unwrap_or(0)
        ))
    })
}

async fn persist_subscription_post_member(
    canonical_db: &LibraryDatabase,
    subscription_id: i64,
    metadata: &ParsedMetadata,
    entity_hash: Option<&str>,
    status: &str,
) {
    let Ok(state) = crate::state::get_state() else {
        return;
    };
    let runtime = SubscriptionRuntimeService::new(canonical_db, &state.library_root);
    let Some(post_id) = metadata
        .post_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let site_id = metadata
        .category
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let item_key = subscription_item_key(metadata)
        .unwrap_or_else(|| format!("{site_id}:{post_id}:{}", metadata.page_num.unwrap_or(0)));
    let _ = runtime
        .upsert_subscription_post_member(
            crate::subscriptions::types::OwnedSubscriptionPostMemberUpsert {
                subscription_id,
                site_id: site_id.to_string(),
                post_id: post_id.to_string(),
                item_key,
                page_num: metadata.page_num.map(i64::from),
                canonical_post_url: metadata.canonical_post_url.clone(),
                media_url: metadata.media_url.clone(),
                entity_hash: entity_hash.map(ToOwned::to_owned),
                status: status.to_string(),
            },
        )
        .await;
}

async fn reconcile_subscription_collection_order(
    canonical_db: &LibraryDatabase,
    subscription_id: i64,
    site_id: &str,
    post_id: &str,
    collection_id: i64,
) {
    let Ok(state) = crate::state::get_state() else {
        return;
    };
    let runtime = SubscriptionRuntimeService::new(canonical_db, &state.library_root);
    let Ok(members) = runtime
        .list_subscription_post_members(subscription_id, site_id, post_id)
        .await
    else {
        return;
    };
    let ordered_hashes: Vec<String> = members
        .into_iter()
        .filter(|member| matches!(member.status.as_str(), "imported" | "reused"))
        .filter_map(|member| member.entity_hash)
        .collect();
    if ordered_hashes.is_empty() {
        return;
    }
    let _ = canonical_db.reorder_collection_members_by_hashes(collection_id, &ordered_hashes);
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

fn unique_entity_hashes<'a>(entity_hashes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for entity_hash in entity_hashes {
        if seen.insert(entity_hash) {
            unique.push(entity_hash.to_string());
        }
    }
    unique
}

async fn process_single_queue(
    db: &Arc<LibraryDatabase>,
    blob_store: &Arc<BlobStore>,
    queue: &IngestQueueEntry,
    item: &IngestQueueItem,
) -> Result<Vec<IngestQueueItemCompletion>, String> {
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

    let mut summary = IngestBatchSummary::default();
    summary.flags.merge(&outcome.flags);
    if let Some(folder_id) = payload.target_folder_id {
        let ids = db.resolve_entity_hashes(&[outcome.entity_hash.clone()])?;
        if !ids.is_empty() {
            db.add_folder_members(folder_id, &ids, crate::db::types::ExpansionMode::EntityOnly)?;
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
        if let Some(item_key) = subscription_item_key(metadata) {
            let runtime = crate::state::get_state()
                .ok()
                .map(|state| SubscriptionRuntimeService::new(db, &state.library_root));
            if let Some(runtime) = runtime {
                let _ = runtime
                    .resolve_subscription_download_attempt(
                        subscription_id,
                        queue.query_id,
                        &item_key,
                    )
                    .await;
            }
        }
        persist_subscription_post_member(
            db,
            subscription_id,
            metadata,
            Some(outcome.entity_hash.as_str()),
            outcome.disposition.result_kind(),
        )
        .await;
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
    if outcome.disposition.is_imported() {
        if let Ok(state) = crate::state::get_state() {
            crate::dispatch::typed::ai_tagger::auto_tag_imported(
                state.as_ref(),
                std::slice::from_ref(&outcome.entity_hash),
            )
            .await;
        }
    }
    Ok(vec![IngestQueueItemCompletion {
        item_id: item.item_id,
        result_kind,
        resolved_entity_hash: outcome.entity_hash,
        resolved_file_hash: outcome.file_hash,
    }])
}

#[derive(Debug)]
struct SubscriptionCollectionContext<'a> {
    subscription_id: i64,
    category: &'a str,
    post_id: &'a str,
}

fn subscription_collection_context(
    queue: &IngestQueueEntry,
) -> Result<Option<SubscriptionCollectionContext<'_>>, String> {
    match (
        queue.subscription_id,
        queue.category.as_deref(),
        queue.post_id.as_deref(),
    ) {
        (Some(subscription_id), Some(category), Some(post_id))
            if !category.trim().is_empty() && !post_id.trim().is_empty() =>
        {
            Ok(Some(SubscriptionCollectionContext {
                subscription_id,
                category,
                post_id,
            }))
        }
        (None, None, None) => Ok(None),
        _ => Err("collection queue has partial subscription context".to_string()),
    }
}

async fn persist_subscription_collection_association(
    db: &LibraryDatabase,
    context: &SubscriptionCollectionContext<'_>,
    collection_id: i64,
) {
    let Ok(state) = crate::state::get_state() else {
        return;
    };
    let runtime = SubscriptionRuntimeService::new(db, &state.library_root);
    if let Err(error) = runtime
        .upsert_subscription_post_collection(
            context.subscription_id,
            context.category,
            context.post_id,
            collection_id,
        )
        .await
    {
        warn!(
            subscription_id = context.subscription_id,
            category = context.category,
            post_id = context.post_id,
            collection_id,
            %error,
            "Failed to persist subscription collection association"
        );
    }
}

async fn process_collection_queue(
    db: &Arc<LibraryDatabase>,
    blob_store: &Arc<BlobStore>,
    queue: &IngestQueueEntry,
    items: &[IngestQueueItem],
) -> Result<Vec<IngestQueueItemCompletion>, String> {
    let subscription = subscription_collection_context(queue)?;
    let preferred_name = queue
        .preferred_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "collection queue missing preferred_name".to_string())?;

    let mut target_folder_id = None;
    let mut prepared = Vec::new();
    let mut missing_count = 0usize;
    for item in items {
        let payload: IngestQueueItemPayload =
            serde_json::from_str(&item.payload_json).map_err(|err| err.to_string())?;
        if let Some(previous_target) = target_folder_id {
            if previous_target != payload.target_folder_id {
                return Err("collection queue has conflicting target folders".to_string());
            }
        } else {
            target_folder_id = Some(payload.target_folder_id);
        }
        log_ingest_queue_payload("execute_collection", queue.queue_id, item.item_id, &payload);
        let path = PathBuf::from(&item.source_path);
        if !path.exists() {
            missing_count += 1;
            prepared.push((item.clone(), payload, None));
            continue;
        }
        prepared.push((item.clone(), payload, Some(path)));
    }

    if missing_count > 0 {
        return Err(format!(
            "Collection ingest is missing {missing_count} queued source file{}",
            if missing_count == 1 { "" } else { "s" },
        ));
    }

    let present_paths: Vec<_> = prepared
        .iter()
        .filter_map(|(_, _, path)| path.clone())
        .collect();
    preflight_collection_sources(&present_paths).await?;

    let mut processable = Vec::new();
    for (item, payload, path) in prepared {
        let path = path.expect("missing collection sources were rejected above");
        db.mark_ingest_queue_item_running(item.item_id).await?;
        let mut request = payload.request;
        request.path = path;
        processable.push((
            item,
            CollectionIngestMember {
                request,
                metadata: payload.subscription_metadata,
            },
        ));
    }
    processable.sort_by_key(|(_, member)| {
        member
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.page_num)
            .unwrap_or(u32::MAX)
    });
    for (index, (_, member)) in processable.iter_mut().enumerate() {
        member.request.skip_thumbnail = index > 0;
    }
    let (processable_items, members): (Vec<_>, Vec<_>) = processable.into_iter().unzip();

    let (existing_collection_id, prior_member_hashes) = if let Some(context) = &subscription {
        let runtime = crate::state::get_state()
            .ok()
            .map(|state| SubscriptionRuntimeService::new(db, &state.library_root));
        let existing_collection_id = match &runtime {
            Some(runtime) => runtime
                .get_subscription_post_collection(
                    context.subscription_id,
                    context.category,
                    context.post_id,
                )
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let prior_member_hashes = match runtime {
            Some(runtime) => runtime
                .list_subscription_post_members(
                    context.subscription_id,
                    context.category,
                    context.post_id,
                )
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|member| matches!(member.status.as_str(), "imported" | "reused"))
                .filter_map(|member| member.entity_hash)
                .collect(),
            None => Vec::new(),
        };
        (existing_collection_id, prior_member_hashes)
    } else {
        (None, Vec::new())
    };

    let result = materialize_collection(
        db,
        blob_store,
        preferred_name,
        &members,
        existing_collection_id,
        &prior_member_hashes,
    )
    .await?;
    let collection_id = result.collection_id.or(existing_collection_id);
    let target_folder_id = target_folder_id.flatten();
    let mut folder_ids = Vec::new();
    if let (Some(folder_id), Some(collection_id)) = (target_folder_id, collection_id) {
        db.add_folder_members(
            folder_id,
            &[collection_id],
            crate::db::types::ExpansionMode::EntityOnly,
        )?;
        folder_ids.push(folder_id);
    }

    if let Some(context) = &subscription {
        for member in &result.resolved_members {
            let Some(mut metadata) = member.metadata.clone() else {
                continue;
            };
            metadata.post_id = Some(context.post_id.to_string());
            metadata.category = Some(context.category.to_string());
            if let Some(item_key) = subscription_item_key(&metadata) {
                if let Ok(state) = crate::state::get_state() {
                    let runtime = SubscriptionRuntimeService::new(db, &state.library_root);
                    let _ = runtime
                        .resolve_subscription_download_attempt(
                            context.subscription_id,
                            queue.query_id,
                            &item_key,
                        )
                        .await;
                }
            }
            persist_subscription_post_member(
                db,
                context.subscription_id,
                &metadata,
                Some(member.entity_hash.as_str()),
                member.disposition.result_kind(),
            )
            .await;
        }
        if let Some(collection_id) = collection_id {
            persist_subscription_collection_association(db, context, collection_id).await;
            reconcile_subscription_collection_order(
                db,
                context.subscription_id,
                context.category,
                context.post_id,
                collection_id,
            )
            .await;
        }
    }

    apply_compiler_plan(db, &result.flags, &folder_ids);
    let mut summary = IngestBatchSummary::default();
    summary.flags.merge(&result.flags);
    summary.folder_ids = folder_ids;
    if let Some(collection_hash) = result.collection_hash.clone() {
        summary.imported_hashes.push(collection_hash);
    } else {
        summary
            .imported_hashes
            .extend(result.imported_hashes.clone());
    }
    let extra_grid_scopes = if subscription.is_some() {
        vec!["system:inbox".into()]
    } else {
        vec!["system:active".into(), "system:inbox".into()]
    };
    let mut impact = build_ingest_change_impact(&summary, extra_grid_scopes);
    if let Some(collection_id) = collection_id {
        let collection_folder_ids = db
            .get_collection_folder_ids(collection_id)
            .unwrap_or_default();
        impact = impact.merge(
            crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
                collection_id,
                &collection_folder_ids,
            ),
        );
    }
    crate::events::emit_state_changed(
        "ingest_queue_collection_commit",
        crate::ingest::attach_current_sidebar_counts(db, impact),
    );

    let completions = processable_items
        .into_iter()
        .zip(result.resolved_members.iter())
        .map(|(item, member)| IngestQueueItemCompletion {
            item_id: item.item_id,
            result_kind: match member.disposition {
                SingleIngestDisposition::Imported => IngestQueueItemResultKind::Imported,
                SingleIngestDisposition::Reused => IngestQueueItemResultKind::Reused,
            },
            resolved_entity_hash: member.entity_hash.clone(),
            resolved_file_hash: member.file_hash.clone(),
        })
        .collect::<Vec<_>>();
    let derivative_entity_hashes = unique_entity_hashes(
        result
            .resolved_members
            .iter()
            .map(|member| member.entity_hash.as_str()),
    );
    crate::background_work::enqueue_missing_derivative_jobs(
        db,
        blob_store,
        &derivative_entity_hashes,
    )
    .await;
    let imported_entity_hashes = unique_entity_hashes(
        result
            .resolved_members
            .iter()
            .filter(|member| member.disposition.is_imported())
            .map(|member| member.entity_hash.as_str()),
    );
    if !imported_entity_hashes.is_empty() {
        if let Ok(state) = crate::state::get_state() {
            crate::dispatch::typed::ai_tagger::auto_tag_imported(
                state.as_ref(),
                &imported_entity_hashes,
            )
            .await;
        }
    }
    Ok(completions)
}

async fn repair_duplicate_failed_single_queues(db: &Arc<LibraryDatabase>) -> Result<(), String> {
    let candidates = db.list_duplicate_failed_single_queue_candidates().await?;
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

    let result = match queue.queue_kind {
        IngestQueueKind::Single => process_single_queue(db, blob_store, &queue, &items[0]).await,
        IngestQueueKind::Collection => {
            process_collection_queue(db, blob_store, &queue, &items).await
        }
    };

    match result {
        Ok(completions) => {
            db.complete_ingest_queue(queue.queue_id, completions)
                .await?;
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

pub async fn start_worker_loop(
    db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    library_root: PathBuf,
    running_subscriptions: crate::types::RunningSubscriptions,
    cancel: CancellationToken,
) {
    if let Err(error) = repair_duplicate_failed_single_queues(&db).await {
        warn!(error = %error, "Failed to repair duplicate-failed ingest queue rows");
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
        if let Some(run_id) = db
            .subscription_run_id_for_ingest_queue(queue.queue_id)
            .await
            .ok()
            .flatten()
        {
            let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
                db.as_ref(),
                &library_root,
            );
            if let Err(error) = crate::subscriptions::settlement::settle_run(
                &runtime,
                &running_subscriptions,
                run_id,
            )
            .await
            {
                warn!(run_id, error = %error, "Failed to settle subscription run after ingest");
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
                 queue_kind TEXT NOT NULL,
                 source_kind TEXT NOT NULL,
                 subscription_id INTEGER,
                 query_id INTEGER,
                 query_run_id INTEGER,
                 cleanup_root TEXT,
                 post_id TEXT,
                 category TEXT,
                 preferred_name TEXT,
                 expected_count INTEGER,
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

    fn write_video_fixture(path: &Path) {
        // Enough of an ISO base media header for the MIME detector to identify MP4.
        fs::write(path, b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom\0\0\0\0").unwrap();
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
            IngestQueueKind::Single,
            "subscription",
            None,
            None,
            None,
            Some(&staged.root),
            None,
            None,
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

    #[test]
    fn count_ingest_queue_by_subscription_reports_imported_reused_and_failed_items() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_id, queue_kind, source_kind, subscription_id, status, created_at, updated_at
             ) VALUES (1, 'single', 'subscription', 7, 'running', 'now', 'now')",
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
                 queue_id, queue_kind, source_kind, subscription_id, status, created_at, updated_at
             ) VALUES (1, 'single', 'subscription', 7, 'running', 'old', 'old')",
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
                 queue_id, queue_kind, source_kind, subscription_id, status, created_at, updated_at
             ) VALUES (1, 'collection', 'subscription', 7, 'running', 'now', 'now')",
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

    #[test]
    fn collection_queue_rejects_partial_subscription_context() {
        let queue = IngestQueueEntry {
            queue_id: 1,
            queue_kind: IngestQueueKind::Collection,
            source_kind: "subscription".to_string(),
            subscription_id: Some(7),
            query_id: None,
            query_run_id: None,
            cleanup_root: None,
            post_id: None,
            category: Some("site".to_string()),
            preferred_name: Some("post".to_string()),
            expected_count: None,
            status: "pending".to_string(),
        };

        assert_eq!(
            subscription_collection_context(&queue).unwrap_err(),
            "collection queue has partial subscription context"
        );
    }

    #[tokio::test]
    async fn manual_collection_enqueue_rejects_blank_names_and_empty_media() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        fs::create_dir_all(&library_root).unwrap();
        let db = LibraryDatabase::open(&library_root).unwrap();

        let blank_name = enqueue_manual_collection(
            &db,
            Vec::new(),
            "  ".to_string(),
            None,
            None,
            1,
            None,
            Some(&library_root),
        )
        .await
        .unwrap_err();
        assert_eq!(blank_name, "Collection name cannot be blank");

        let empty_media = enqueue_manual_collection(
            &db,
            Vec::new(),
            "Valid name".to_string(),
            None,
            None,
            1,
            None,
            Some(&library_root),
        )
        .await
        .unwrap_err();
        assert_eq!(empty_media, "No supported media files to add to collection");
    }

    #[tokio::test]
    async fn manual_collection_enqueue_rejects_non_image_without_creating_queue_rows() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        let image = source_root.join("image.png");
        let video = source_root.join("video.mp4");
        write_image(&image, 96);
        write_video_fixture(&video);
        let db = LibraryDatabase::open(&library_root).unwrap();

        let error = enqueue_manual_collection(
            &db,
            vec![image.display().to_string(), video.display().to_string()],
            "Mixed collection".to_string(),
            None,
            None,
            1,
            None,
            Some(&library_root),
        )
        .await
        .unwrap_err();

        assert!(error.contains("Collections can contain images only"));
        db.with_read(|conn| {
            let queue_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM ingest_queue", [], |row| row.get(0))?;
            assert_eq!(queue_count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn collection_execution_preflight_keeps_items_pending_for_non_image_sources() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        let image = source_root.join("image.png");
        let video = source_root.join("video.mp4");
        write_image(&image, 96);
        write_video_fixture(&video);

        let db = Arc::new(LibraryDatabase::open(&library_root).unwrap());
        let blob_store = Arc::new(BlobStore::open(&library_root).unwrap());
        let payload = |path: &Path| IngestQueueItemPayload {
            request: SingleIngestRequest {
                source_kind: IngestSourceKind::Manual,
                path: path.to_path_buf(),
                tag_strings: Vec::new(),
                source_urls: Vec::new(),
                name: None,
                notes: None,
                created_at: None,
                initial_status: 1,
                skip_thumbnail: false,
                tag_provenance_mask: crate::db::types::TAG_PROVENANCE_MANUAL,
                subscription_id: None,
            },
            subscription_metadata: None,
            target_folder_id: None,
        };
        db.enqueue_ingest_queue(
            IngestQueueKind::Collection,
            "manual",
            None,
            None,
            None,
            None,
            None,
            None,
            Some("Mixed collection"),
            None,
            vec![
                (image.clone(), Some(0), payload(&image), false),
                (video.clone(), Some(1), payload(&video), false),
            ],
        )
        .await
        .unwrap();

        let queue = db.lease_next_ingest_queue().await.unwrap().unwrap();
        let items = db.list_ingest_queue_items(queue.queue_id).await.unwrap();
        let error = process_collection_queue(&db, &blob_store, &queue, &items)
            .await
            .unwrap_err();
        assert!(error.contains("Collections can contain images only"));
        assert!(blob_store.list_originals().is_empty());

        db.with_read(|conn| {
            let pending_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ingest_queue_item WHERE queue_id = ?1 AND status = 'pending'",
                params![queue.queue_id],
                |row| row.get(0),
            )?;
            let entity_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))?;
            assert_eq!(pending_count, 2);
            assert_eq!(entity_count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn manual_collection_queue_adds_collection_to_folder_not_members() {
        let temp = TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&library_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        let first = source_root.join("first.png");
        let second = source_root.join("second.png");
        write_image(&first, 32);
        write_image(&second, 224);

        let db = Arc::new(LibraryDatabase::open(&library_root).unwrap());
        let blob_store = Arc::new(BlobStore::open(&library_root).unwrap());
        let folder_id = db.create_folder("Imported", None, None, None).unwrap();
        enqueue_manual_collection(
            db.as_ref(),
            vec![first.display().to_string(), second.display().to_string()],
            "Manual collection".to_string(),
            Some(vec!["general:manual".to_string()]),
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
            let (collection_id, member_count): (i64, i64) = conn.query_row(
                "SELECT entity_id, member_count FROM media_entity
                 WHERE entity_kind = 'collection' AND name = 'Manual collection'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(member_count, 2);
            let collection_folder_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM folder_member WHERE folder_id = ?1 AND entity_id = ?2",
                params![folder_id, collection_id],
                |row| row.get(0),
            )?;
            let child_folder_count: i64 = conn.query_row(
                "SELECT COUNT(*)
                 FROM folder_member fm
                 JOIN media_entity child ON child.entity_id = fm.entity_id
                 WHERE fm.folder_id = ?1 AND child.parent_collection_entity_id = ?2",
                params![folder_id, collection_id],
                |row| row.get(0),
            )?;
            assert_eq!(collection_folder_count, 1);
            assert_eq!(child_folder_count, 0);
            Ok(())
        })
        .unwrap();
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
                 queue_id, queue_kind, source_kind, subscription_id, status, last_error, created_at, updated_at
             ) VALUES (9, 'single', 'subscription', 1, 'failed', 'boom', 'now', 'now')",
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
    fn duplicate_failed_single_queue_candidates_only_include_exact_hash_failures() {
        let conn = Connection::open_in_memory().unwrap();
        setup_queue_schema(&conn);
        conn.execute_batch(
            "INSERT INTO ingest_queue (
                 queue_id, queue_kind, source_kind, subscription_id, status, last_error, created_at, updated_at
             ) VALUES
                 (1, 'single', 'subscription', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (2, 'collection', 'subscription', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (3, 'single', 'subscription', 1, 'failed', 'other', 'now', 'now');
             INSERT INTO ingest_queue_item (
                 item_id, queue_id, source_path, payload_json, delete_after_ingest,
                 status, last_error, created_at, updated_at
             ) VALUES
                 (11, 1, '/tmp/a', '{}', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (12, 2, '/tmp/b', '{}', 1, 'failed', 'UNIQUE constraint failed: media_file.file_hash', 'now', 'now'),
                 (13, 3, '/tmp/c', '{}', 1, 'failed', 'other', 'now', 'now');",
        )
        .unwrap();

        let candidates = list_duplicate_failed_single_queue_candidates(&conn).unwrap();
        assert_eq!(candidates, vec![(1, 11, "/tmp/a".to_string())]);
    }

    #[test]
    fn unique_entity_hashes_preserves_order_while_deduping() {
        let unique = unique_entity_hashes(["a", "b", "a", "c", "b"]);
        assert_eq!(
            unique,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }
}
