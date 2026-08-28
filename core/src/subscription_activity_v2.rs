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
use crate::store::Store;

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
    pub counts: ActivityCounts,
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
    pub open_only: bool,
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

/// Returns the newest persisted runs, bounded to a small page.
pub fn list_runs(
    store: &Store,
    subscription_id: i64,
    limit: usize,
) -> Result<SubscriptionRunList, String> {
    let limit = bounded_limit(limit);
    store.read(|connection| list_runs_from_connection(connection, subscription_id, limit))
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

/// Returns one run with every persisted query and source-item activity row.
pub fn run_activity(
    store: &Store,
    run_id: i64,
    source_item_limit: usize,
) -> Result<Option<SubscriptionRunActivity>, String> {
    let source_item_limit = bounded_limit(source_item_limit);
    store.read(|connection| run_activity_from_connection(connection, run_id, source_item_limit))
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

/// Returns persisted progress for the active run, if the subscription is running.
pub fn current_progress(
    store: &Store,
    subscription_id: i64,
) -> Result<Option<CurrentSubscriptionProgress>, String> {
    store.read_snapshot(|connection| current_progress_from_connection(connection, subscription_id))
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
                    END
             FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
             ORDER BY run_id DESC LIMIT 1",
            [subscription_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((run_id, status)) = active_run else {
        return Ok(None);
    };
    let summary = connection.query_row(RUN_SUMMARY_BY_ID_SQL, [run_id], run_summary_from_row)?;
    Ok(Some(CurrentSubscriptionProgress {
        subscription_id,
        run_id,
        status,
        counts: summary.counts,
    }))
}

/// Returns open or historical issues using a stable `(last_seen_at, issue_id)` cursor.
pub fn list_issues(store: &Store, request: &IssuePageRequest) -> Result<IssuePage, String> {
    let limit = bounded_limit(request.limit);
    store.read(|connection| list_issues_from_connection(connection, request, limit))
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
               AND (?3 = 0 OR status = 'open')",
        params![
            request.subscription_id,
            request.query_id,
            request.open_only as i64,
        ],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT issue_id, issue_key, subscription_id, query_id, issue_kind,
                    message, detail, status, first_seen_at, last_seen_at, resolved_at
             FROM subscription_issue
             WHERE subscription_id = ?1
               AND (?2 IS NULL OR query_id = ?2)
               AND (?3 = 0 OR status = 'open')
               AND (
                   ?4 IS NULL
                   OR last_seen_at < ?4
                   OR (last_seen_at = ?4 AND issue_id < ?5)
               )
             ORDER BY last_seen_at DESC, issue_id DESC
             LIMIT ?6",
    )?;
    let mut rows = statement.query(params![
        request.subscription_id,
        request.query_id,
        request.open_only as i64,
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
        "SELECT (SELECT COUNT(DISTINCT ssp.source_post_id)
                 FROM subscription_run_query current
                 JOIN subscription_source_post ssp
                   ON ssp.query_id = current.query_id
                  AND ssp.last_seen_run_id = current.run_id
                 WHERE current.run_query_id = ?1),
                COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                    THEN si.source_post_id END),
                COUNT(DISTINCT rsi.source_item_id),
                COUNT(DISTINCT CASE WHEN si.state IN ('downloaded', 'ingested')
                                    THEN rsi.source_item_id END),
                COUNT(DISTINCT CASE WHEN ij.status IN ('pending', 'running')
                                    THEN rsi.source_item_id END),
                COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                    THEN rsi.source_item_id END),
                COUNT(DISTINCT CASE WHEN si.state = 'failed' OR ij.status = 'failed'
                                    THEN rsi.source_item_id END),
                COUNT(DISTINCT CASE WHEN si.state = 'deleted'
                                    THEN rsi.source_item_id END)
         FROM subscription_run_source_item rsi
         JOIN source_item si ON si.source_item_id = rsi.source_item_id
         LEFT JOIN ingest_job ij ON ij.source_item_id = si.source_item_id
         WHERE rsi.run_query_id = ?1",
        [run_query_id],
        |row| {
            Ok(ActivityCounts {
                posts_traversed: row.get(0)?,
                posts_added: row.get(1)?,
                fetched: row.get(2)?,
                downloaded: row.get(3)?,
                queued: row.get(4)?,
                ingested: row.get(5)?,
                failed: row.get(6)?,
                deleted: row.get(7)?,
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
            fetched: row.get(12)?,
            downloaded: row.get(13)?,
            queued: row.get(14)?,
            ingested: row.get(15)?,
            failed: row.get(16)?,
            deleted: row.get(17)?,
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
post_counts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT ssp.source_post_id) AS posts_traversed
    FROM target_runs tr
    JOIN subscription_run_query srq ON srq.run_id = tr.run_id
    LEFT JOIN subscription_source_post ssp
      ON ssp.query_id = srq.query_id AND ssp.last_seen_run_id = srq.run_id
    GROUP BY srq.run_id
),
run_counts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                               THEN si.source_post_id END) AS posts_added,
           COUNT(DISTINCT rsi.source_item_id) AS fetched,
           COUNT(DISTINCT CASE WHEN si.state IN ('downloaded', 'ingested')
                               THEN rsi.source_item_id END) AS downloaded,
           COUNT(DISTINCT CASE WHEN ij.status IN ('pending', 'running')
                               THEN rsi.source_item_id END) AS queued,
           COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                               THEN rsi.source_item_id END) AS ingested,
           COUNT(DISTINCT CASE WHEN si.state = 'failed' OR ij.status = 'failed'
                               THEN rsi.source_item_id END) AS failed,
           COUNT(DISTINCT CASE WHEN si.state = 'deleted'
                               THEN rsi.source_item_id END) AS deleted
    FROM target_runs tr
    JOIN subscription_run_query srq ON srq.run_id = tr.run_id
    LEFT JOIN subscription_run_source_item rsi ON rsi.run_query_id = srq.run_query_id
    LEFT JOIN source_item si ON si.source_item_id = rsi.source_item_id
    LEFT JOIN ingest_job ij ON ij.source_item_id = si.source_item_id
    GROUP BY srq.run_id
)
SELECT sr.run_id, sr.subscription_id, sr.requested_by, sr.status,
       sr.started_at, sr.finished_at, sr.failure_kind, sr.error_message,
       sr.created_at,
       (SELECT COUNT(*) FROM subscription_run_query WHERE run_id = sr.run_id),
       COALESCE(pc.posts_traversed, 0), COALESCE(rc.posts_added, 0),
       COALESCE(rc.fetched, 0), COALESCE(rc.downloaded, 0),
       COALESCE(rc.queued, 0), COALESCE(rc.ingested, 0),
       COALESCE(rc.failed, 0), COALESCE(rc.deleted, 0)
FROM target_runs sr
LEFT JOIN post_counts pc ON pc.run_id = sr.run_id
LEFT JOIN run_counts rc ON rc.run_id = sr.run_id
ORDER BY sr.created_at DESC, sr.run_id DESC
"#;

const RUN_SUMMARY_BY_ID_SQL: &str = r#"
WITH post_counts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT ssp.source_post_id) AS posts_traversed
    FROM subscription_run_query srq
    LEFT JOIN subscription_source_post ssp
      ON ssp.query_id = srq.query_id AND ssp.last_seen_run_id = srq.run_id
    WHERE srq.run_id = ?1
    GROUP BY srq.run_id
),
run_counts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                               THEN si.source_post_id END) AS posts_added,
           COUNT(DISTINCT rsi.source_item_id) AS fetched,
           COUNT(DISTINCT CASE WHEN si.state IN ('downloaded', 'ingested')
                               THEN rsi.source_item_id END) AS downloaded,
           COUNT(DISTINCT CASE WHEN ij.status IN ('pending', 'running')
                               THEN rsi.source_item_id END) AS queued,
           COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                               THEN rsi.source_item_id END) AS ingested,
           COUNT(DISTINCT CASE WHEN si.state = 'failed' OR ij.status = 'failed'
                               THEN rsi.source_item_id END) AS failed,
           COUNT(DISTINCT CASE WHEN si.state = 'deleted'
                               THEN rsi.source_item_id END) AS deleted
    FROM subscription_run_query srq
    LEFT JOIN subscription_run_source_item rsi ON rsi.run_query_id = srq.run_query_id
    LEFT JOIN source_item si ON si.source_item_id = rsi.source_item_id
    LEFT JOIN ingest_job ij ON ij.source_item_id = si.source_item_id
    WHERE srq.run_id = ?1
    GROUP BY srq.run_id
)
SELECT sr.run_id, sr.subscription_id, sr.requested_by, sr.status,
       sr.started_at, sr.finished_at, sr.failure_kind, sr.error_message,
       sr.created_at,
       (SELECT COUNT(*) FROM subscription_run_query WHERE run_id = sr.run_id),
       COALESCE(pc.posts_traversed, 0), COALESCE(rc.posts_added, 0),
       COALESCE(rc.fetched, 0), COALESCE(rc.downloaded, 0),
       COALESCE(rc.queued, 0), COALESCE(rc.ingested, 0),
       COALESCE(rc.failed, 0), COALESCE(rc.deleted, 0)
FROM subscription_run sr
LEFT JOIN post_counts pc ON pc.run_id = sr.run_id
LEFT JOIN run_counts rc ON rc.run_id = sr.run_id
WHERE sr.run_id = ?1
"#;

#[cfg(test)]
mod tests {
    use super::{current_progress, list_issues, list_runs, run_activity, IssuePageRequest};
    use crate::store::Store;
    use rusqlite::params;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        (directory, store)
    }

    fn subscription_run(store: &Store, status: &str, created_at: &str) -> (i64, i64) {
        store
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO subscription (
                         subscription_key, name, schedule, created_at
                     ) VALUES (?1, 'Subscription', 'manual', ?2)",
                    params![format!("subscription-{created_at}"), created_at],
                )?;
                let subscription_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO subscription_query (
                         query_key, subscription_id, site_id, domain_key,
                         query_kind, query_text
                     ) VALUES (?1, ?2, 'example', 'example', 'tag', 'artist')",
                    params![format!("query-{created_at}"), subscription_id],
                )?;
                let query_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO subscription_run (
                         subscription_id, requested_by, status, created_at
                     ) VALUES (?1, 'test', ?2, ?3)",
                    params![subscription_id, status, created_at],
                )?;
                let run_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO subscription_run_query (
                         run_id, query_id, status, available_at
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![run_id, query_id, status, created_at],
                )?;
                Ok((subscription_id, run_id))
            })
            .unwrap()
            .0
    }

    #[test]
    fn runs_are_newest_first_and_bounded() {
        let (_directory, store) = fixture();
        let (subscription_id, _old_run) =
            subscription_run(&store, "succeeded", "2026-01-01T00:00:00Z");
        let (_same_subscription, _new_run) = store
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO subscription_run (
                         subscription_id, requested_by, status, created_at
                     ) VALUES (?1, 'test', 'failed', '2026-01-02T00:00:00Z')",
                    [subscription_id],
                )?;
                Ok(())
            })
            .unwrap();

        let page = list_runs(&store, subscription_id, 1).unwrap();
        assert_eq!(page.runs.len(), 1);
        assert_eq!(page.runs[0].status, "failed");
    }

    #[test]
    fn progress_and_activity_use_persisted_source_and_ingest_rows() {
        let (_directory, store) = fixture();
        let (subscription_id, run_id) = subscription_run(&store, "running", "2026-01-01T00:00:00Z");
        let run_query_id: i64 = store
            .read(|connection| {
                connection.query_row(
                    "SELECT run_query_id FROM subscription_run_query WHERE run_id = ?1",
                    [run_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        store
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('example', 'post', 'now', 'now')",
                    [],
                )?;
                let post_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO subscription_source_post (
                         subscription_id, query_id, source_post_id, last_seen_run_id
                     ) VALUES (?1, (
                         SELECT query_id FROM subscription_run_query WHERE run_query_id = ?2
                     ), ?3, ?4)",
                    params![subscription_id, run_query_id, post_id, run_id],
                )?;
                tx.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('example', 'post-without-media', 'now', 'now')",
                    [],
                )?;
                let empty_post_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO subscription_source_post (
                         subscription_id, query_id, source_post_id, last_seen_run_id
                     ) VALUES (?1, (
                         SELECT query_id FROM subscription_run_query WHERE run_query_id = ?2
                     ), ?3, ?4)",
                    params![subscription_id, run_query_id, empty_post_id, run_id],
                )?;
                for (item_key, state) in [
                    ("downloaded", "downloaded"),
                    ("ingested", "ingested"),
                    ("failed", "failed"),
                    ("deleted", "deleted"),
                    ("pending", "pending"),
                ] {
                    tx.execute(
                        "INSERT INTO source_item (
                             source_post_id, item_key, position, state,
                             created_at, updated_at
                         ) VALUES (?1, ?2, ?3, ?4, 'now', 'now')",
                        params![post_id, item_key, item_key.len() as i64, state],
                    )?;
                    let source_item_id = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO subscription_run_source_item
                             (run_query_id, source_item_id) VALUES (?1, ?2)",
                        params![run_query_id, source_item_id],
                    )?;
                    if item_key == "pending" {
                        tx.execute(
                            "INSERT INTO ingest_job (
                                 job_key, source_kind, source_path, source_item_id,
                                 payload_json, lifecycle, status, available_at,
                                 created_at, updated_at
                             ) VALUES (?1, 'subscription', '/tmp/item', ?2, '{}',
                                       'inbox', 'pending', 'now', 'now', 'now')",
                            params![format!("job-{item_key}"), source_item_id],
                        )?;
                    }
                }
                Ok(())
            })
            .unwrap();

        let progress = current_progress(&store, subscription_id).unwrap().unwrap();
        assert_eq!(progress.counts.posts_traversed, 2);
        assert_eq!(progress.counts.posts_added, 1);
        assert_eq!(progress.counts.fetched, 5);
        assert_eq!(progress.counts.downloaded, 2);
        assert_eq!(progress.counts.queued, 1);
        assert_eq!(progress.counts.ingested, 1);
        assert_eq!(progress.counts.failed, 1);
        assert_eq!(progress.counts.deleted, 1);

        let activity = run_activity(&store, run_id, 100).unwrap().unwrap();
        assert_eq!(activity.queries[0].source_items.len(), 5);
        assert!(!activity.queries[0].source_items_truncated);
        assert_eq!(activity.queries[0].counts, progress.counts);

        let bounded = run_activity(&store, run_id, 2).unwrap().unwrap();
        assert_eq!(bounded.queries[0].source_items.len(), 2);
        assert!(bounded.queries[0].source_items_truncated);
        assert_eq!(bounded.queries[0].counts, progress.counts);
    }

    #[test]
    fn issue_cursor_is_stable_and_query_filter_is_scoped() {
        let (_directory, store) = fixture();
        let (subscription_id, _run_id) =
            subscription_run(&store, "succeeded", "2026-01-01T00:00:00Z");
        let query_id: i64 = store
            .read(|connection| {
                connection.query_row(
                    "SELECT query_id FROM subscription_query WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        store
            .transaction(|tx| {
                for (key, timestamp, status) in [
                    ("old", "2026-01-01T00:00:00Z", "open"),
                    ("new", "2026-01-03T00:00:00Z", "open"),
                    ("closed", "2026-01-02T00:00:00Z", "resolved"),
                ] {
                    tx.execute(
                        "INSERT INTO subscription_issue (
                             issue_key, subscription_id, query_id, issue_kind,
                             message, status, first_seen_at, last_seen_at
                         ) VALUES (?1, ?2, ?3, 'network', ?4, ?5, ?6, ?6)",
                        params![key, subscription_id, query_id, key, status, timestamp],
                    )?;
                }
                Ok(())
            })
            .unwrap();

        let first = list_issues(
            &store,
            &IssuePageRequest {
                subscription_id,
                query_id: Some(query_id),
                open_only: true,
                cursor: None,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(first.issues[0].issue_key, "new");
        let second = list_issues(
            &store,
            &IssuePageRequest {
                subscription_id,
                query_id: Some(query_id),
                open_only: true,
                cursor: first.next_cursor,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(second.issues[0].issue_key, "old");
        assert!(second.next_cursor.is_none());
    }
}
