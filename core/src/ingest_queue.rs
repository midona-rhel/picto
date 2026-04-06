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
    apply_compiler_plan, build_ingest_change_impact, ingest_single_path,
    materialize_subscription_collection, IngestBatchSummary, IngestSourceKind,
    SingleIngestDisposition, SingleIngestOutcome, SingleIngestRequest,
    SubscriptionCollectionMember,
};
use crate::subscriptions::gallery_dl_runner::ParsedMetadata;
use crate::subscriptions::runtime_service::SubscriptionRuntimeService;
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
            page_num,
            payload_json,
            if delete_after_ingest { 1 } else { 0 },
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn lease_next_ingest_queue(conn: &Connection) -> rusqlite::Result<Option<IngestQueueEntry>> {
    let tx = conn.unchecked_transaction()?;
    let leased: Option<IngestQueueEntry> = tx
        .query_row(
            "SELECT queue_id, queue_kind, source_kind, subscription_id, query_id, query_run_id,
                    cleanup_root, post_id, category, preferred_name, expected_count, status
             FROM ingest_queue
             WHERE status IN ('pending', 'stale')
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
        tx.execute(
            "UPDATE ingest_queue SET status = 'running', updated_at = ?1 WHERE queue_id = ?2",
            params![now_rfc3339(), queue.queue_id],
        )?;
    }
    tx.commit()?;
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

fn mark_running_ingest_stale(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ingest_queue
         SET status = 'stale', updated_at = ?1
         WHERE status = 'running'",
        params![now_rfc3339()],
    )?;
    Ok(())
}

fn delete_completed_ingest_queues(conn: &Connection) -> rusqlite::Result<usize> {
    Ok(conn.execute("DELETE FROM ingest_queue WHERE status = 'complete'", [])?)
}

fn delete_stale_ingest_queues_older_than(conn: &Connection, days: i64) -> rusqlite::Result<usize> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    Ok(conn.execute(
        "DELETE FROM ingest_queue WHERE status = 'stale' AND updated_at < ?1",
        [cutoff],
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
         SET status = 'stale', last_error = NULL, updated_at = ?1
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

fn mark_all_pending_ingest_stale_for_subscription(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ingest_queue
         SET status = 'stale', updated_at = ?1
         WHERE subscription_id = ?2 AND status IN ('pending', 'running')",
        params![now_rfc3339(), subscription_id],
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
            let tx = conn.unchecked_transaction()?;
            let queue_id = create_ingest_queue_entry(
                &tx,
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
                    &tx,
                    queue_id,
                    &source_path.display().to_string(),
                    page_num,
                    &payload_json,
                    delete_after_ingest,
                )?;
                log_ingest_queue_payload("enqueue", queue_id, item_id, &payload);
            }
            tx.commit()?;
            Ok(queue_id)
        })
    }

    pub async fn mark_all_pending_ingest_stale_for_subscription(
        &self,
        subscription_id: i64,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            mark_all_pending_ingest_stale_for_subscription(conn, subscription_id)
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

    pub async fn mark_ingest_queue_complete(&self, queue_id: i64) -> Result<(), String> {
        self.with_write(move |conn| mark_ingest_queue_status(conn, queue_id, "complete", None))
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
            mark_running_ingest_stale(conn)?;
            delete_completed_ingest_queues(conn)?;
            delete_stale_ingest_queues_older_than(conn, 7)?;
            Ok(())
        })
    }

    pub async fn count_subscription_ingest_queue(
        &self,
        subscription_id: i64,
    ) -> Result<IngestQueueCounts, String> {
        self.with_read(move |conn| count_ingest_queue_by_subscription(conn, subscription_id))
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
    library_root: Option<&Path>,
) -> Result<crate::types::ImportBatchResult, String> {
    let file_paths: Vec<PathBuf> = paths
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
            None,
            false,
        )
        .await?;
    }

    Ok(crate::types::ImportBatchResult {
        imported: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    })
}

pub async fn enqueue_folder_import(
    db: &LibraryDatabase,
    path: String,
    preserve_structure: bool,
    parent_folder_id: Option<i64>,
    initial_status: i64,
) -> Result<crate::types::ImportBatchResult, String> {
    let root_path = {
        let path = PathBuf::from(path);
        path.canonicalize().unwrap_or(path)
    };
    if !root_path.is_dir() {
        return Err(format!("Folder not found: {}", root_path.display()));
    }

    let (directories, file_paths) = collect_import_paths(&root_path)?;
    let mut folder_cache = std::collections::HashMap::<PathBuf, i64>::new();
    if preserve_structure {
        let root_name = root_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Imported Folder")
            .to_string();
        let root_folder_id =
            db.create_folder(&root_name, parent_folder_id, None, None)?;
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
    }

    for file_path in &file_paths {
        let target_folder_id = if preserve_structure {
            let relative_parent = file_path
                .strip_prefix(&root_path)
                .ok()
                .and_then(|relative| relative.parent())
                .map(Path::to_path_buf)
                .unwrap_or_default();
            folder_cache.get(&relative_parent).copied()
        } else {
            parent_folder_id
        };
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Manual,
            path: file_path.clone(),
            tag_strings: Vec::new(),
            source_urls: Vec::new(),
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

    Ok(crate::types::ImportBatchResult {
        imported: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    })
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
        .filter(|member| member.status == "imported")
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
) -> Result<(), String> {
    let mut payload: IngestQueueItemPayload =
        serde_json::from_str(&item.payload_json).map_err(|err| err.to_string())?;
    log_ingest_queue_payload("execute_single", queue.queue_id, item.item_id, &payload);
    payload.request.path = PathBuf::from(&item.source_path);
    db.mark_ingest_queue_item_running(item.item_id).await?;

    // If the source file no longer exists, this is a stale queue entry —
    // the file was already processed or cleaned up. Mark as complete no-op
    // instead of failing (which would pin the temp root forever).
    if !payload.request.path.exists() {
        info!(
            queue_id = queue.queue_id,
            source = %item.source_path,
            "Source file missing, treating as already-processed no-op"
        );
        db.mark_ingest_queue_item_complete(
            item.item_id,
            IngestQueueItemResultKind::Reused,
            None,
            None,
        )
        .await?;
        return Ok(());
    }

    let outcome: SingleIngestOutcome =
        ingest_single_path(db, blob_store, &payload.request).await?;

    let mut summary = IngestBatchSummary::default();
    summary.flags.merge(&outcome.flags);
    if let Some(folder_id) = payload.target_folder_id {
        let ids = db.resolve_entity_hashes(&[outcome.entity_hash.clone()])?;
        if !ids.is_empty() {
            db.add_folder_members(
                folder_id,
                &ids,
                crate::db::types::ExpansionMode::EntityOnly,
            )?;
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
                    .resolve_subscription_download_attempt(subscription_id, queue.query_id, &item_key)
                    .await;
            }
        }
        persist_subscription_post_member(
            db,
            subscription_id,
            metadata,
            Some(outcome.entity_hash.as_str()),
            "imported",
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
            build_ingest_change_impact(&summary, extra_grid_scopes),
        );
    }

    let result_kind = match outcome.disposition {
        SingleIngestDisposition::Imported => IngestQueueItemResultKind::Imported,
        SingleIngestDisposition::Reused => IngestQueueItemResultKind::Reused,
    };
    db.mark_ingest_queue_item_complete(
        item.item_id,
        result_kind,
        Some(outcome.entity_hash.clone()),
        Some(outcome.file_hash.clone()),
    )
    .await?;
    crate::background_work::enqueue_missing_derivative_jobs(
        db,
        blob_store,
        std::slice::from_ref(&outcome.entity_hash),
    )
    .await;
    if item.delete_after_ingest {
        delete_source_file_if_owned(&item.source_path).await;
    }
    Ok(())
}

async fn process_collection_queue(
    db: &Arc<LibraryDatabase>,
    blob_store: &Arc<BlobStore>,
    queue: &IngestQueueEntry,
    items: &[IngestQueueItem],
) -> Result<(), String> {
    let subscription_id = queue
        .subscription_id
        .ok_or_else(|| "subscription collection queue missing subscription_id".to_string())?;
    let category = queue
        .category
        .as_deref()
        .ok_or_else(|| "subscription collection queue missing category".to_string())?;
    let post_id = queue
        .post_id
        .as_deref()
        .ok_or_else(|| "subscription collection queue missing post_id".to_string())?;
    let preferred_name = queue
        .preferred_name
        .as_deref()
        .ok_or_else(|| "subscription collection queue missing preferred_name".to_string())?;

    let mut members = Vec::new();
    let mut processable_items = Vec::new();
    let mut missing_count = 0usize;
    for item in items {
        db.mark_ingest_queue_item_running(item.item_id).await?;
        let path = PathBuf::from(&item.source_path);
        if !path.exists() {
            missing_count += 1;
            db.mark_ingest_queue_item_complete(
                item.item_id,
                IngestQueueItemResultKind::Reused,
                None,
                None,
            )
            .await?;
            continue;
        }
        let payload: IngestQueueItemPayload =
            serde_json::from_str(&item.payload_json).map_err(|err| err.to_string())?;
        log_ingest_queue_payload("execute_collection", queue.queue_id, item.item_id, &payload);
        let metadata = payload
            .subscription_metadata
            .clone()
            .ok_or_else(|| "subscription collection item missing metadata".to_string())?;
        members.push(SubscriptionCollectionMember {
            path,
            metadata,
            skip_thumbnail: false,
        });
        processable_items.push(item);
    }
    if missing_count > 0 {
        info!(
            queue_id = queue.queue_id,
            missing_count,
            remaining = members.len(),
            "Collection queue: some source files missing, skipping them"
        );
    }
    if members.is_empty() {
        // All source files are gone — entire queue is stale, mark items as done
        info!(
            queue_id = queue.queue_id,
            "All collection source files missing, treating as already-processed no-op"
        );
        return Ok(());
    }
    members.sort_by_key(|member| member.metadata.page_num.unwrap_or(u32::MAX));
    for (index, member) in members.iter_mut().enumerate() {
        member.skip_thumbnail = index > 0;
    }

    let existing_collection_id = if let Ok(state) = crate::state::get_state() {
        let runtime = SubscriptionRuntimeService::new(db, &state.library_root);
        runtime
            .get_subscription_post_collection(subscription_id, category, post_id)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let expected_count = queue.expected_count.unwrap_or(0);
    let force_collection =
        existing_collection_id.is_some() || expected_count > 1 || members.len() > 1;

    let result = materialize_subscription_collection(
        db,
        blob_store,
        subscription_id,
        category,
        post_id,
        preferred_name,
        &members,
        existing_collection_id,
        force_collection,
    )
    .await?;

    apply_compiler_plan(db, &result.flags, &[]);
    for member in &result.resolved_members {
        if let Some(item_key) = member.item_key.as_deref() {
            if let Ok(state) = crate::state::get_state() {
                let runtime = SubscriptionRuntimeService::new(db, &state.library_root);
                let _ = runtime
                    .resolve_subscription_download_attempt(subscription_id, queue.query_id, item_key)
                    .await;
            }
        }
        persist_subscription_post_member(
            db,
            subscription_id,
            &ParsedMetadata {
                item_key: member.item_key.clone(),
                page_num: member.page_num,
                canonical_post_url: member.canonical_post_url.clone(),
                media_url: member.media_url.clone(),
                post_id: Some(post_id.to_string()),
                category: Some(category.to_string()),
                ..Default::default()
            },
            Some(member.entity_hash.as_str()),
            "imported",
        )
        .await;
    }
    if let Some(collection_id) = result.collection_id.or(existing_collection_id) {
        reconcile_subscription_collection_order(
            db,
            subscription_id,
            category,
            post_id,
            collection_id,
        )
        .await;
    }

    let mut impact = if let Some(collection_hash) = result.collection_hash {
        let mut summary = IngestBatchSummary::default();
        summary.flags.merge(&result.flags);
        summary.imported_hashes.push(collection_hash);
        build_ingest_change_impact(&summary, vec!["system:inbox".into()])
    } else {
        let mut summary = IngestBatchSummary::default();
        summary.flags.merge(&result.flags);
        summary
            .imported_hashes
            .extend(result.imported_hashes.clone());
        build_ingest_change_impact(&summary, vec!["system:inbox".into()])
    };
    if let Some(collection_id) = result.collection_id.or(existing_collection_id) {
        let folder_ids = db
            .get_collection_folder_ids(collection_id)
            .unwrap_or_default();
        impact = impact.merge(
            crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
                collection_id,
                &folder_ids,
            ),
        );
    }
    crate::events::emit_state_changed("ingest_queue_collection_commit", impact);

    for (item, member) in processable_items
        .into_iter()
        .zip(result.resolved_members.iter())
    {
        let result_kind = match member.disposition {
            SingleIngestDisposition::Imported => IngestQueueItemResultKind::Imported,
            SingleIngestDisposition::Reused => IngestQueueItemResultKind::Reused,
        };
        db.mark_ingest_queue_item_complete(
            item.item_id,
            result_kind,
            Some(member.entity_hash.clone()),
            Some(member.file_hash.clone()),
        )
        .await?;
        if item.delete_after_ingest {
            delete_source_file_if_owned(&item.source_path).await;
        }
    }
    let derivative_entity_hashes =
        unique_entity_hashes(result.resolved_members.iter().map(|member| member.entity_hash.as_str()));
    crate::background_work::enqueue_missing_derivative_jobs(
        db,
        blob_store,
        &derivative_entity_hashes,
    )
    .await;
    Ok(())
}

async fn repair_duplicate_failed_single_queues(
    db: &Arc<LibraryDatabase>,
) -> Result<(), String> {
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
        IngestQueueKind::Single => {
            process_single_queue(db, blob_store, &queue, &items[0]).await
        }
        IngestQueueKind::Collection => {
            process_collection_queue(db, blob_store, &queue, &items).await
        }
    };

    match result {
        Ok(()) => {
            db.mark_ingest_queue_complete(queue.queue_id).await?;
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

        if let Err(error) = process_queue_entry(&db, &blob_store, queue.clone()).await {
            warn!(queue_id = queue.queue_id, error = %error, "Ingest queue entry failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(queue_status, "stale");
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
        assert_eq!(unique, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }
}
