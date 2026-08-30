//! Read-only subscription activity from the replacement SQLite schema.
//!
//! This module deliberately reports only facts persisted by the replacement
//! schema. Source-item state tells us whether an item was fetched, downloaded,
//! ingested, failed, or deleted. `ingest_job` tells us about the durable ingest
//! attempt. There is no persisted download-attempt table, so this module does
//! not invent download attempt counts or statuses.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::library_application::LibraryApplication;

const MAX_PAGE_SIZE: usize = 100;

/// Counts are derived from source-item and ingest-job rows at read time.
///
/// These are independent facts, not a forced partition. For example, an item
/// can be counted as downloaded and failed when its bytes arrived but its
/// durable ingest job later failed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ActivityCounts {
    #[ts(type = "number")]
    pub posts_traversed: i64,
    #[ts(type = "number")]
    pub posts_added: i64,
    #[ts(type = "number")]
    pub posts_skipped: i64,
    #[ts(type = "number")]
    pub fetched: i64,
    #[ts(type = "number")]
    pub downloaded: i64,
    #[ts(type = "number")]
    pub queued: i64,
    #[ts(type = "number")]
    pub ingested: i64,
    #[ts(type = "number")]
    pub failed: i64,
    #[ts(type = "number")]
    pub deleted: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionRunSummary {
    #[ts(type = "number")]
    pub run_id: i64,
    #[ts(type = "number")]
    pub subscription_id: i64,
    pub requested_by: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    #[ts(type = "number")]
    pub query_count: i64,
    pub counts: ActivityCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionRunList {
    #[ts(type = "number")]
    pub subscription_id: i64,
    pub runs: Vec<SubscriptionRunSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct IngestAttempt {
    #[ts(type = "number")]
    pub ingest_job_id: i64,
    pub status: String,
    #[ts(type = "number")]
    pub attempt_count: i64,
    pub available_at: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SourceItemActivity {
    #[ts(type = "number")]
    pub source_item_id: i64,
    #[ts(type = "number")]
    pub source_post_id: i64,
    pub site_id: String,
    pub post_key: String,
    pub item_key: String,
    #[ts(type = "number")]
    pub position: i64,
    #[ts(type = "number | null")]
    pub media_item_id: Option<i64>,
    pub state: String,
    pub last_error: Option<String>,
    pub ingest: Option<IngestAttempt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionQueryActivity {
    #[ts(type = "number")]
    pub run_query_id: i64,
    #[ts(type = "number")]
    pub run_id: i64,
    #[ts(type = "number")]
    pub query_id: i64,
    pub site_id: String,
    pub query_text: String,
    pub status: String,
    #[ts(type = "number")]
    pub attempt_count: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub counts: ActivityCounts,
    pub source_items: Vec<SourceItemActivity>,
    pub source_items_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionRunActivity {
    pub summary: SubscriptionRunSummary,
    pub queries: Vec<SubscriptionQueryActivity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CurrentSubscriptionProgress {
    #[ts(type = "number")]
    pub subscription_id: i64,
    #[ts(type = "number")]
    pub run_id: i64,
    pub status: String,
    pub requested_by: String,
    pub counts: ActivityCounts,
    #[ts(type = "number | null")]
    pub gallery_total_items: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionIssue {
    #[ts(type = "number")]
    pub issue_id: i64,
    pub issue_key: String,
    #[ts(type = "number")]
    pub subscription_id: i64,
    #[ts(type = "number | null")]
    pub query_id: Option<i64>,
    pub issue_kind: String,
    pub message: String,
    pub detail: Option<String>,
    pub status: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub resolved_at: Option<String>,
    pub source_item_key: Option<String>,
    pub source_post_key: Option<String>,
    pub source_post_title: Option<String>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct IssueCursor {
    pub last_seen_at: String,
    #[ts(type = "number")]
    pub issue_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct IssuePageRequest {
    #[ts(type = "number")]
    pub subscription_id: i64,
    #[ts(type = "number | null")]
    pub query_id: Option<i64>,
    pub unresolved_only: bool,
    pub cursor: Option<IssueCursor>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct IssuePage {
    #[ts(type = "number")]
    pub subscription_id: i64,
    pub issues: Vec<SubscriptionIssue>,
    pub next_cursor: Option<IssueCursor>,
    #[ts(type = "number")]
    pub total_count: i64,
}

pub fn list_runs_library(
    application: &LibraryApplication,
    subscription_id: i64,
    limit: usize,
) -> Result<SubscriptionRunList, String> {
    let limit = bounded_limit(limit);
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                list_runs_from_connection(connection, subscription_id, limit).map_err(Into::into)
            },
        )
        .map_err(|error| error.to_string())
}

fn list_runs_from_connection(
    connection: &Connection,
    subscription_id: i64,
    limit: usize,
) -> rusqlite::Result<SubscriptionRunList> {
    let mut statement = connection.prepare(RUN_SUMMARY_SQL)?;
    let runs = statement
        .query_map(params![subscription_id, limit as i64], run_summary_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(SubscriptionRunList {
        subscription_id,
        runs,
    })
}

pub fn run_activity_library(
    application: &LibraryApplication,
    run_id: i64,
    source_item_limit: usize,
) -> Result<Option<SubscriptionRunActivity>, String> {
    let source_item_limit = bounded_limit(source_item_limit);
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                run_activity_from_connection(connection, run_id, source_item_limit)
                    .map_err(Into::into)
            },
        )
        .map_err(|error| error.to_string())
}

fn run_activity_from_connection(
    connection: &Connection,
    run_id: i64,
    source_item_limit: usize,
) -> rusqlite::Result<Option<SubscriptionRunActivity>> {
    let Some(summary) = connection
        .query_row(RUN_SUMMARY_BY_ID_SQL, [run_id], run_summary_from_row)
        .optional()?
    else {
        return Ok(None);
    };
    let mut query_statement = connection.prepare(
        "SELECT run_query_id FROM subscription_run_query
         WHERE run_id = ?1 ORDER BY run_query_id",
    )?;
    let query_ids = query_statement
        .query_map([run_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let queries = query_ids
        .into_iter()
        .map(|run_query_id| {
            query_activity_from_connection(connection, run_query_id, source_item_limit)
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(SubscriptionRunActivity { summary, queries }))
}

pub fn current_progress_library(
    application: &LibraryApplication,
    subscription_id: i64,
) -> Result<Option<CurrentSubscriptionProgress>, String> {
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                current_progress_from_connection(connection, subscription_id).map_err(Into::into)
            },
        )
        .map_err(|error| error.to_string())
}

fn current_progress_from_connection(
    connection: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Option<CurrentSubscriptionProgress>> {
    let active_run = connection
        .query_row(
            "SELECT run_id,
                    CASE
                        WHEN status = 'pending'
                         AND failure_kind IN ('paused', 'inbox_full')
                        THEN failure_kind
                        ELSE status
                    END,
                    requested_by
             FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
             ORDER BY run_id DESC LIMIT 1",
            [subscription_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((run_id, status, requested_by)) = active_run else {
        return Ok(None);
    };
    let summary = connection.query_row(RUN_SUMMARY_BY_ID_SQL, [run_id], run_summary_from_row)?;
    let gallery_total_items = connection.query_row(
        "SELECT MAX(CAST(COALESCE(
                    json_extract(post.metadata_json, '$.filecount'),
                    json_extract(post.metadata_json, '$.count')
                ) AS INTEGER))
         FROM subscription_run_query query_run
         JOIN subscription_query query ON query.query_id = query_run.query_id
         JOIN subscription_source_post linked
           ON linked.query_id = query.query_id AND linked.last_seen_run_id = query_run.run_id
         JOIN source_post post ON post.source_post_id = linked.source_post_id
         WHERE query_run.run_id = ?1 AND query.site_id = 'ehentai'",
        [run_id],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    // A gallery adapter can initially report the current item count as its
    // total. Do not publish that provisional 1/1, 2/2 value; a real total is
    // useful once it is ahead of download progress.
    let gallery_total_items = gallery_total_items
        .filter(|total| summary.counts.downloaded == 0 || *total > summary.counts.downloaded);
    Ok(Some(CurrentSubscriptionProgress {
        subscription_id,
        run_id,
        status,
        requested_by,
        counts: summary.counts,
        gallery_total_items,
    }))
}

pub fn list_issues_library(
    application: &LibraryApplication,
    request: &IssuePageRequest,
) -> Result<IssuePage, String> {
    let limit = bounded_limit(request.limit);
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                list_issues_from_connection(connection, request, limit).map_err(Into::into)
            },
        )
        .map_err(|error| error.to_string())
}

fn list_issues_from_connection(
    connection: &Connection,
    request: &IssuePageRequest,
    limit: usize,
) -> rusqlite::Result<IssuePage> {
    let fetch_limit = (limit + 1) as i64;
    let cursor_time = request
        .cursor
        .as_ref()
        .map(|cursor| cursor.last_seen_at.as_str());
    let cursor_id = request.cursor.as_ref().map(|cursor| cursor.issue_id);
    let total_count = connection.query_row(
        "SELECT COUNT(*) FROM subscription_issue
             WHERE subscription_id = ?1
               AND (?2 IS NULL OR query_id = ?2)
               AND (?3 = 0 OR status != 'resolved')",
        params![
            request.subscription_id,
            request.query_id,
            request.unresolved_only as i64,
        ],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT issue.issue_id, issue.issue_key, issue.subscription_id, issue.query_id,
                    issue.issue_kind, issue.message, issue.detail, issue.status,
                    issue.first_seen_at, issue.last_seen_at, issue.resolved_at,
                    item.item_key, post.post_key, post.title,
                    COALESCE(post.canonical_url, item.canonical_url), item.media_url
             FROM subscription_issue issue
             LEFT JOIN source_item item ON item.source_item_id = CASE
                 WHEN issue.issue_kind = 'download_item'
                 THEN CAST(substr(issue.issue_key, 13, length(issue.issue_key) - 21) AS INTEGER)
             END
             LEFT JOIN source_post post ON post.source_post_id = item.source_post_id
             WHERE issue.subscription_id = ?1
               AND (?2 IS NULL OR issue.query_id = ?2)
               AND (?3 = 0 OR issue.status != 'resolved')
               AND (
                   ?4 IS NULL
                   OR issue.last_seen_at < ?4
                   OR (issue.last_seen_at = ?4 AND issue.issue_id < ?5)
               )
             ORDER BY issue.last_seen_at DESC, issue.issue_id DESC
             LIMIT ?6",
    )?;
    let mut rows = statement.query(params![
        request.subscription_id,
        request.query_id,
        request.unresolved_only as i64,
        cursor_time,
        cursor_id,
        fetch_limit,
    ])?;
    let mut issues = Vec::with_capacity(limit);
    while let Some(row) = rows.next()? {
        issues.push(SubscriptionIssue {
            issue_id: row.get(0)?,
            issue_key: row.get(1)?,
            subscription_id: row.get(2)?,
            query_id: row.get(3)?,
            issue_kind: row.get(4)?,
            message: row.get(5)?,
            detail: row.get(6)?,
            status: row.get(7)?,
            first_seen_at: row.get(8)?,
            last_seen_at: row.get(9)?,
            resolved_at: row.get(10)?,
            source_item_key: row.get(11)?,
            source_post_key: row.get(12)?,
            source_post_title: row.get(13)?,
            canonical_post_url: row.get(14)?,
            media_url: row.get(15)?,
        });
    }
    let has_more = issues.len() > limit;
    if has_more {
        issues.truncate(limit);
    }
    let next_cursor = if has_more {
        issues.last().map(|issue| IssueCursor {
            last_seen_at: issue.last_seen_at.clone(),
            issue_id: issue.issue_id,
        })
    } else {
        None
    };
    Ok(IssuePage {
        subscription_id: request.subscription_id,
        issues,
        next_cursor,
        total_count,
    })
}

fn bounded_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_PAGE_SIZE)
}

fn query_activity_from_connection(
    connection: &Connection,
    run_query_id: i64,
    source_item_limit: usize,
) -> rusqlite::Result<SubscriptionQueryActivity> {
    let (
        run_id,
        query_id,
        site_id,
        query_text,
        status,
        attempt_count,
        started_at,
        finished_at,
        failure_kind,
        error_message,
    ) = connection.query_row(
        "SELECT qr.run_id, qr.query_id, q.site_id, q.query_text, qr.status,
                qr.attempt_count, qr.started_at, qr.finished_at,
                qr.failure_kind, qr.error_message
         FROM subscription_run_query qr
         JOIN subscription_query q ON q.query_id = qr.query_id
         WHERE qr.run_query_id = ?1",
        [run_query_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ))
        },
    )?;

    let mut statement = connection.prepare(
        "SELECT si.source_item_id, si.source_post_id, sp.site_id, sp.post_key,
                si.item_key, si.position, si.media_item_id, si.state, si.last_error,
                ij.ingest_job_id, ij.status, ij.attempt_count,
                ij.available_at, ij.last_error
         FROM subscription_run_source_item rsi
         JOIN source_item si ON si.source_item_id = rsi.source_item_id
         JOIN source_post sp ON sp.source_post_id = si.source_post_id
         LEFT JOIN ingest_job ij ON ij.source_item_id = si.source_item_id
         WHERE rsi.run_query_id = ?1
         ORDER BY si.position, si.source_item_id
         LIMIT ?2",
    )?;
    let counts = activity_counts_for_query(connection, run_query_id)?;
    let mut source_items = Vec::new();
    let mut rows = statement.query(params![run_query_id, source_item_limit as i64])?;
    while let Some(row) = rows.next()? {
        let state: String = row.get(7)?;
        let ingest_status: Option<String> = row.get(10)?;
        source_items.push(SourceItemActivity {
            source_item_id: row.get(0)?,
            source_post_id: row.get(1)?,
            site_id: row.get(2)?,
            post_key: row.get(3)?,
            item_key: row.get(4)?,
            position: row.get(5)?,
            media_item_id: row.get(6)?,
            state,
            last_error: row.get(8)?,
            ingest: row
                .get::<_, Option<i64>>(9)?
                .map(|ingest_job_id| IngestAttempt {
                    ingest_job_id,
                    status: ingest_status.expect("ingest status exists with ingest job id"),
                    attempt_count: row.get(11).expect("ingest attempt count exists"),
                    available_at: row.get(12).expect("ingest available time exists"),
                    last_error: row.get(13).expect("ingest error exists"),
                }),
        });
    }

    Ok(SubscriptionQueryActivity {
        run_query_id,
        run_id,
        query_id,
        site_id,
        query_text,
        status,
        attempt_count,
        started_at,
        finished_at,
        failure_kind,
        error_message,
        source_items_truncated: counts.fetched > source_items.len() as i64,
        counts,
        source_items,
    })
}

fn activity_counts_for_query(
    connection: &Connection,
    run_query_id: i64,
) -> rusqlite::Result<ActivityCounts> {
    connection.query_row(
        "SELECT COUNT(DISTINCT attempt.attempt_id),
                COUNT(DISTINCT CASE WHEN attempt.state = 'added' THEN attempt.attempt_id END),
                COUNT(DISTINCT CASE WHEN attempt.state = 'skipped' THEN attempt.attempt_id END),
                COUNT(DISTINCT file.file_attempt_id),
                COUNT(DISTINCT CASE WHEN file.state IN ('staged', 'retained') THEN file.file_attempt_id END),
                COUNT(DISTINCT CASE WHEN job.status IN ('pending', 'running')
                                    THEN file.file_attempt_id END),
                COUNT(DISTINCT CASE WHEN file.state = 'retained' THEN file.file_attempt_id END),
                COUNT(DISTINCT CASE WHEN file.state = 'failed' THEN file.file_attempt_id END),
                0
         FROM source_post_attempt attempt
         LEFT JOIN source_file_attempt file USING(attempt_id)
         LEFT JOIN ingest_job job ON job.source_item_id = file.source_item_id
         WHERE attempt.run_query_id = ?1",
        [run_query_id],
        |row| {
            Ok(ActivityCounts {
                posts_traversed: row.get(0)?,
                posts_added: row.get(1)?,
                posts_skipped: row.get(2)?,
                fetched: row.get(3)?,
                downloaded: row.get(4)?,
                queued: row.get(5)?,
                ingested: row.get(6)?,
                failed: row.get(7)?,
                deleted: row.get(8)?,
            })
        },
    )
}

fn run_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubscriptionRunSummary> {
    Ok(SubscriptionRunSummary {
        run_id: row.get(0)?,
        subscription_id: row.get(1)?,
        requested_by: row.get(2)?,
        status: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        failure_kind: row.get(6)?,
        error_message: row.get(7)?,
        created_at: row.get(8)?,
        query_count: row.get(9)?,
        counts: ActivityCounts {
            posts_traversed: row.get(10)?,
            posts_added: row.get(11)?,
            posts_skipped: row.get(12)?,
            fetched: row.get(13)?,
            downloaded: row.get(14)?,
            queued: row.get(15)?,
            ingested: row.get(16)?,
            failed: row.get(17)?,
            deleted: row.get(18)?,
        },
    })
}

const RUN_SUMMARY_SQL: &str = r#"
WITH target_runs AS (
    SELECT *
    FROM subscription_run
    WHERE subscription_id = ?1
    ORDER BY created_at DESC, run_id DESC
    LIMIT ?2
),
counts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT attempt.attempt_id) AS posts_traversed,
           COUNT(DISTINCT CASE WHEN attempt.state = 'added' THEN attempt.attempt_id END) AS posts_added,
           COUNT(DISTINCT CASE WHEN attempt.state = 'skipped' THEN attempt.attempt_id END) AS posts_skipped,
           COUNT(DISTINCT file.file_attempt_id) AS fetched,
           COUNT(DISTINCT CASE WHEN file.state IN ('staged', 'retained') THEN file.file_attempt_id END) AS downloaded,
           COUNT(DISTINCT CASE WHEN job.status IN ('pending', 'running') THEN file.file_attempt_id END) AS queued,
           COUNT(DISTINCT CASE WHEN file.state = 'retained' THEN file.file_attempt_id END) AS ingested,
           COUNT(DISTINCT CASE WHEN file.state = 'failed' THEN file.file_attempt_id END) AS failed,
           0 AS deleted
    FROM target_runs tr
    JOIN subscription_run_query srq ON srq.run_id = tr.run_id
    LEFT JOIN source_post_attempt attempt ON attempt.run_query_id = srq.run_query_id
    LEFT JOIN source_file_attempt file USING(attempt_id)
    LEFT JOIN ingest_job job ON job.source_item_id = file.source_item_id
    GROUP BY srq.run_id
)
SELECT sr.run_id, sr.subscription_id, sr.requested_by, sr.status,
       sr.started_at, sr.finished_at, sr.failure_kind, sr.error_message,
       sr.created_at,
       (SELECT COUNT(*) FROM subscription_run_query WHERE run_id = sr.run_id),
       COALESCE(c.posts_traversed, 0), COALESCE(c.posts_added, 0),
       COALESCE(c.posts_skipped, 0),
       COALESCE(c.fetched, 0), COALESCE(c.downloaded, 0),
       COALESCE(c.queued, 0), COALESCE(c.ingested, 0),
       COALESCE(c.failed, 0), COALESCE(c.deleted, 0)
FROM target_runs sr
LEFT JOIN counts c ON c.run_id = sr.run_id
ORDER BY sr.created_at DESC, sr.run_id DESC
"#;

const RUN_SUMMARY_BY_ID_SQL: &str = r#"
WITH counts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT attempt.attempt_id) AS posts_traversed,
           COUNT(DISTINCT CASE WHEN attempt.state = 'added' THEN attempt.attempt_id END) AS posts_added,
           COUNT(DISTINCT CASE WHEN attempt.state = 'skipped' THEN attempt.attempt_id END) AS posts_skipped,
           COUNT(DISTINCT file.file_attempt_id) AS fetched,
           COUNT(DISTINCT CASE WHEN file.state IN ('staged', 'retained') THEN file.file_attempt_id END) AS downloaded,
           COUNT(DISTINCT CASE WHEN job.status IN ('pending', 'running') THEN file.file_attempt_id END) AS queued,
           COUNT(DISTINCT CASE WHEN file.state = 'retained' THEN file.file_attempt_id END) AS ingested,
           COUNT(DISTINCT CASE WHEN file.state = 'failed' THEN file.file_attempt_id END) AS failed,
           0 AS deleted
    FROM subscription_run_query srq
    LEFT JOIN source_post_attempt attempt ON attempt.run_query_id = srq.run_query_id
    LEFT JOIN source_file_attempt file USING(attempt_id)
    LEFT JOIN ingest_job job ON job.source_item_id = file.source_item_id
    WHERE srq.run_id = ?1
    GROUP BY srq.run_id
)
SELECT sr.run_id, sr.subscription_id, sr.requested_by, sr.status,
       sr.started_at, sr.finished_at, sr.failure_kind, sr.error_message,
       sr.created_at,
       (SELECT COUNT(*) FROM subscription_run_query WHERE run_id = sr.run_id),
       COALESCE(c.posts_traversed, 0), COALESCE(c.posts_added, 0),
       COALESCE(c.posts_skipped, 0),
       COALESCE(c.fetched, 0), COALESCE(c.downloaded, 0),
       COALESCE(c.queued, 0), COALESCE(c.ingested, 0),
       COALESCE(c.failed, 0), COALESCE(c.deleted, 0)
FROM subscription_run sr
LEFT JOIN counts c ON c.run_id = sr.run_id
WHERE sr.run_id = ?1
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_application::LibraryApplication;
    use crate::subscription_catalog::{NewSubscription, NewSubscriptionQuery};
    use crate::subscriptions::{DomainSchedule, NormalizedItem, NormalizedPost};

    #[test]
    fn active_gallery_reports_downloaded_images_against_gallery_total() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Gallery".into(),
            schedule: "manual".into(),
            initial_post_limit: None,
            periodic_post_limit: None,
            queries: vec![NewSubscriptionQuery {
                site_id: "ehentai".into(),
                query_text: "https://e-hentai.org/g/123/0123456789/".into(),
                display_name: Some("Gallery import".into()),
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let query = crate::library_subscription_state::claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:01Z",
        )
        .unwrap()
        .unwrap();
        let ids = crate::library_subscription_state::record_post(
            &application,
            query.run_query_id,
            &NormalizedPost {
                site_id: "ehentai".into(),
                post_key: "gallery-123".into(),
                canonical_url: Some("https://e-hentai.org/g/123/0123456789/".into()),
                creator_name: None,
                title: Some("Gallery".into()),
                description: None,
                captured_at: None,
                metadata_json: Some(r#"{"filecount":"37"}"#.into()),
                items: vec![NormalizedItem {
                    item_key: "page-1".into(),
                    position: 1,
                    media_url: Some("https://example.invalid/1.jpg".into()),
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        crate::library_subscription_state::mark_source_item_staged(
            &application,
            query.run_query_id,
            ids["page-1"],
            "gallery-page-1-hash",
            "/tmp/gallery-page-1",
            1,
            "2026-08-29T00:00:03Z",
        )
        .unwrap();
        let progress = current_progress_library(&application, subscription_id)
            .unwrap()
            .unwrap();
        assert_eq!(progress.gallery_total_items, Some(37));
        assert_eq!(progress.counts.downloaded, 1);
        assert_eq!(progress.counts.posts_added, 0);
    }

    #[test]
    fn revisiting_an_old_ingested_item_does_not_report_current_progress() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Existing source".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![NewSubscriptionQuery {
                site_id: "e621".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .library()
            .auxiliary_write(
                picto_library::database::WorkPriority::CanonicalIngest,
                ["tests".to_owned()],
                [],
                |transaction, _| {
                    transaction.execute(
                        "INSERT INTO library_item(local_id, stable_key, item_kind)
                         VALUES (5000, 'old-root', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, content_hash, file_path, mime, size_bytes)
                         VALUES (5001, 'old-hash', '/tmp/old', 'image/png', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_item(media_id, media_name, file_id)
                         VALUES (5000, 'old', 5001)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root
                             (root_id, name, cover_media_id, imported_at_ms, modified_at_ms,
                              media_count, total_size_bytes)
                         VALUES (5000, 'old', 5000, 1700000000000, 1700000000000, 1, 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO source_post
                             (site_id, post_key, root_item_id, created_at, updated_at)
                         VALUES ('e621', 'old-post', 5000,
                                 '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')",
                        [],
                    )?;
                    let source_post_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO source_item
                             (source_post_id, item_key, position, media_item_id, state,
                              created_at, updated_at)
                         VALUES (?1, 'old-media', 0, 5000, 'ingested',
                                 '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')",
                        [source_post_id],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:01Z")
            .unwrap();
        let query = crate::library_subscription_state::claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:02Z",
        )
        .unwrap()
        .unwrap();
        crate::library_subscription_state::record_post(
            &application,
            query.run_query_id,
            &NormalizedPost {
                site_id: "e621".into(),
                post_key: "old-post".into(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![NormalizedItem {
                    item_key: "old-media".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:03Z",
        )
        .unwrap();
        assert!(matches!(
            crate::library_subscription_state::settled_post_outcome(
                &application,
                &query,
                "old-post",
            )
            .unwrap(),
            picto_sources::SourcePostOutcome::Skipped { .. }
        ));

        let progress = current_progress_library(&application, subscription_id)
            .unwrap()
            .unwrap();
        assert_eq!(progress.counts.posts_traversed, 1);
        assert_eq!(progress.counts.posts_added, 0);
        assert_eq!(progress.counts.posts_skipped, 1);
        assert_eq!(progress.counts.downloaded, 0);
        assert_eq!(progress.counts.ingested, 0);
    }
}
