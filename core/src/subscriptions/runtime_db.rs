use rusqlite::{params, Connection, OptionalExtension};

use super::types::{
    SubscriptionDownloadAttemptRecord, SubscriptionDownloadAttemptUpsert, SubscriptionIssueRecord,
    SubscriptionPostMemberRecord, SubscriptionPostMemberUpsert, SubscriptionQueryJob,
    SubscriptionQueryRunRecord, SubscriptionRunRecord,
};
use crate::subscriptions::gallery_dl_runner::FailureKind;

fn map_subscription_run_row(row: &rusqlite::Row) -> rusqlite::Result<SubscriptionRunRecord> {
    Ok(SubscriptionRunRecord {
        run_id: row.get(0)?,
        subscription_id: row.get(1)?,
        started_at: row.get(2)?,
        finished_at: row.get(3)?,
        status: row.get(4)?,
        failure_kind: row.get(5)?,
        error_message: row.get(6)?,
        files_downloaded: row.get(7)?,
        files_skipped: row.get(8)?,
        metadata_validated: row.get(9)?,
        metadata_invalid: row.get(10)?,
    })
}

fn map_subscription_query_run_row(
    row: &rusqlite::Row,
) -> rusqlite::Result<SubscriptionQueryRunRecord> {
    Ok(SubscriptionQueryRunRecord {
        query_run_id: row.get(0)?,
        run_id: row.get(1)?,
        subscription_id: row.get(2)?,
        query_id: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        status: row.get(6)?,
        failure_kind: row.get(7)?,
        error_message: row.get(8)?,
        posts_processed: row.get(9)?,
        files_downloaded: row.get(10)?,
        files_skipped: row.get(11)?,
    })
}

fn map_subscription_query_job_row(row: &rusqlite::Row) -> rusqlite::Result<SubscriptionQueryJob> {
    Ok(SubscriptionQueryJob {
        job_id: row.get(0)?,
        run_id: row.get(1)?,
        subscription_id: row.get(2)?,
        query_id: row.get(3)?,
        site_id: row.get(4)?,
        status: row.get(5)?,
        job_kind: row.get(6)?,
        requested_by: row.get(7)?,
        post_id: row.get(8)?,
        attempt_count: row.get(9)?,
        available_at: row.get(10)?,
        queued_at: row.get(11)?,
        started_at: row.get(12)?,
        finished_at: row.get(13)?,
        failure_kind: row.get(14)?,
        error_message: row.get(15)?,
    })
}

fn map_subscription_issue_row(row: &rusqlite::Row) -> rusqlite::Result<SubscriptionIssueRecord> {
    Ok(SubscriptionIssueRecord {
        issue_id: row.get(0)?,
        issue_key: row.get(1)?,
        subscription_id: row.get(2)?,
        query_id: row.get(3)?,
        issue_kind: row.get(4)?,
        status: row.get(5)?,
        message: row.get(6)?,
        detail: row.get(7)?,
        first_seen_at: row.get(8)?,
        last_seen_at: row.get(9)?,
        resolved_at: row.get(10)?,
        recovery_action: row.get(11)?,
        next_retry_at: row.get(12)?,
    })
}

fn map_subscription_download_attempt_row(
    row: &rusqlite::Row,
) -> rusqlite::Result<SubscriptionDownloadAttemptRecord> {
    Ok(SubscriptionDownloadAttemptRecord {
        attempt_id: row.get(0)?,
        subscription_id: row.get(1)?,
        query_id: row.get(2)?,
        query_run_id: row.get(3)?,
        item_key: row.get(4)?,
        site_category: row.get(5)?,
        post_id: row.get(6)?,
        page_num: row.get(7)?,
        canonical_post_url: row.get(8)?,
        media_url: row.get(9)?,
        retry_url: row.get(10)?,
        retry_count: row.get(11)?,
        status: row.get(12)?,
        failure_kind: row.get(13)?,
        last_error: row.get(14)?,
        next_retry_at: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
        resolved_at: row.get(18)?,
    })
}

fn map_subscription_post_member_row(
    row: &rusqlite::Row,
) -> rusqlite::Result<SubscriptionPostMemberRecord> {
    Ok(SubscriptionPostMemberRecord {
        subscription_id: row.get(0)?,
        site_id: row.get(1)?,
        post_id: row.get(2)?,
        item_key: row.get(3)?,
        page_num: row.get(4)?,
        canonical_post_url: row.get(5)?,
        media_url: row.get(6)?,
        entity_hash: row.get(7)?,
        status: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[derive(Debug, Default)]
pub struct SubscriptionReconcileReport {
    pub jobs_requeued: usize,
    pub orphan_runs_finalized: usize,
    pub query_runs_finalized: usize,
    pub health_rows_repaired: usize,
    pub query_kinds_repaired: usize,
}

/// Restore durable subscription work after an app quit/crash mid-run.
///
/// Safe only at library open, before the site-runner worker starts: any
/// A leased job returns to the queue and keeps its original full-run identity.
/// Execution history records the interruption, but queued jobs and runs remain
/// active so the normal worker can finish them after startup.
pub fn reconcile_stale_subscription_runtime(
    conn: &Connection,
) -> rusqlite::Result<SubscriptionReconcileReport> {
    let now = chrono::Utc::now().to_rfc3339();
    let stale = FailureKind::Stale.as_str();
    let interrupted = "Interrupted — the app was closed while this run was active";
    let mut report = SubscriptionReconcileReport::default();

    report.jobs_requeued = conn.execute(
        "UPDATE subscription_query_job
         SET status = 'queued', started_at = NULL, finished_at = NULL,
             failure_kind = NULL, error_message = NULL, available_at = ?1
         WHERE status = 'running'",
        [&now],
    )?;
    report.orphan_runs_finalized = conn.execute(
        "UPDATE subscription_run
         SET status = 'cancelled', finished_at = ?1,
             failure_kind = COALESCE(failure_kind, ?2),
             error_message = COALESCE(error_message, ?3)
         WHERE status = 'running'
           AND NOT EXISTS (
               SELECT 1 FROM subscription_query_job j
               WHERE j.run_id = subscription_run.run_id
                 AND j.status IN ('queued', 'running')
           )",
        params![now, stale, interrupted],
    )?;
    report.query_runs_finalized = conn.execute(
        "UPDATE subscription_query_run
         SET status = 'cancelled', finished_at = ?1,
             failure_kind = COALESCE(failure_kind, ?2),
             error_message = COALESCE(error_message, ?3)
         WHERE status = 'running'",
        params![now, stale, interrupted],
    )?;

    // Auth-bad health for a site with no stored credential is impossible by
    // definition — earlier builds wrote these from misclassified download
    // failures.
    report.health_rows_repaired = conn.execute(
        "DELETE FROM credential_health
         WHERE health_status IN ('unauthorized', 'expired', 'error')
           AND site_category NOT IN (SELECT site_category FROM credential_domain)",
        [],
    )?;
    // 'error' rows were written for content failures (e.g. user-not-found);
    // reset so the next successful run can mark them valid again.
    report.health_rows_repaired += conn.execute(
        "UPDATE credential_health
         SET health_status = 'unknown', last_error = NULL
         WHERE health_status = 'error'",
        [],
    )?;

    Ok(report)
}

pub fn create_subscription_run(conn: &Connection, subscription_id: i64) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_run (subscription_id, started_at, status)
         VALUES (?1, ?2, 'running')",
        params![subscription_id, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_subscription_run(
    conn: &Connection,
    run_id: i64,
    status: &str,
    failure_kind: Option<&str>,
    error_message: Option<&str>,
    files_downloaded: i64,
    files_skipped: i64,
    metadata_validated: i64,
    metadata_invalid: i64,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_run
         SET finished_at = ?1,
             status = ?2,
             failure_kind = ?3,
             error_message = ?4,
             files_downloaded = ?5,
             files_skipped = ?6,
             metadata_validated = ?7,
             metadata_invalid = ?8
         WHERE run_id = ?9",
        params![
            now,
            status,
            failure_kind,
            error_message,
            files_downloaded,
            files_skipped,
            metadata_validated,
            metadata_invalid,
            run_id
        ],
    )?;
    Ok(())
}

pub fn finalize_subscription_run_status(
    conn: &Connection,
    run_id: i64,
    status: &str,
    failure_kind: Option<&str>,
    error_message: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_run
         SET finished_at = ?1,
             status = ?2,
             failure_kind = ?3,
             error_message = ?4
         WHERE run_id = ?5",
        params![now, status, failure_kind, error_message, run_id],
    )?;
    Ok(())
}

/// Settle one full subscription run once all jobs belonging to that run are
/// terminal. This never inspects or mutates another run for the subscription.
pub fn finalize_subscription_run_if_terminal(
    conn: &Connection,
    run_id: i64,
) -> rusqlite::Result<Option<SubscriptionRunRecord>> {
    let active_jobs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM subscription_query_job
         WHERE run_id = ?1 AND status IN ('queued', 'running')",
        [run_id],
        |row| row.get(0),
    )?;
    if active_jobs > 0 {
        return Ok(None);
    }

    let failure: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT status, failure_kind, error_message
             FROM subscription_query_job
             WHERE run_id = ?1 AND status IN ('failed', 'cancelled')
             ORDER BY CASE status WHEN 'failed' THEN 0 ELSE 1 END, job_id DESC
             LIMIT 1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let (status, failure_kind, error_message) = match failure {
        Some((job_status, failure_kind, error_message)) => {
            let status = if job_status == "failed" {
                "failed"
            } else {
                "cancelled"
            };
            (status, failure_kind, error_message)
        }
        None => ("succeeded", None, None),
    };
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_run
         SET finished_at = ?1, status = ?2, failure_kind = ?3, error_message = ?4
         WHERE run_id = ?5 AND status = 'running'",
        params![now, status, failure_kind, error_message, run_id],
    )?;
    conn.query_row(
        "SELECT run_id, subscription_id, started_at, finished_at, status,
                failure_kind, error_message, files_downloaded, files_skipped,
                metadata_validated, metadata_invalid
         FROM subscription_run WHERE run_id = ?1",
        [run_id],
        map_subscription_run_row,
    )
    .optional()
}

pub fn accumulate_subscription_run_counters(
    conn: &Connection,
    run_id: i64,
    files_downloaded_delta: i64,
    files_skipped_delta: i64,
    metadata_validated_delta: i64,
    metadata_invalid_delta: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_run
         SET files_downloaded = files_downloaded + ?1,
             files_skipped = files_skipped + ?2,
             metadata_validated = metadata_validated + ?3,
             metadata_invalid = metadata_invalid + ?4
         WHERE run_id = ?5",
        params![
            files_downloaded_delta,
            files_skipped_delta,
            metadata_validated_delta,
            metadata_invalid_delta,
            run_id
        ],
    )?;
    Ok(())
}

pub fn list_subscription_runs(
    conn: &Connection,
    subscription_id: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionRunRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT run_id, subscription_id, started_at, finished_at, status,
                failure_kind, error_message, files_downloaded, files_skipped,
                metadata_validated, metadata_invalid
         FROM subscription_run
         WHERE subscription_id = ?1
         ORDER BY run_id DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![subscription_id, limit], map_subscription_run_row)?;
    rows.collect()
}

pub fn create_subscription_query_run(
    conn: &Connection,
    run_id: Option<i64>,
    subscription_id: i64,
    query_id: i64,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_query_run (run_id, subscription_id, query_id, started_at, status)
         VALUES (?1, ?2, ?3, ?4, 'running')",
        params![run_id, subscription_id, query_id, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_subscription_query_run(
    conn: &Connection,
    query_run_id: i64,
    status: &str,
    failure_kind: Option<&str>,
    error_message: Option<&str>,
    posts_processed: i64,
    files_downloaded: i64,
    files_skipped: i64,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_query_run
         SET finished_at = ?1,
             status = ?2,
             failure_kind = ?3,
             error_message = ?4,
             posts_processed = ?5,
             files_downloaded = ?6,
             files_skipped = ?7
         WHERE query_run_id = ?8",
        params![
            now,
            status,
            failure_kind,
            error_message,
            posts_processed,
            files_downloaded,
            files_skipped,
            query_run_id
        ],
    )?;
    Ok(())
}

pub fn list_subscription_query_runs(
    conn: &Connection,
    query_id: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryRunRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT query_run_id, run_id, subscription_id, query_id, started_at, finished_at,
                status, failure_kind, error_message, posts_processed,
                files_downloaded, files_skipped
         FROM subscription_query_run
         WHERE query_id = ?1
         ORDER BY query_run_id DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query_id, limit], map_subscription_query_run_row)?;
    rows.collect()
}

pub fn enqueue_subscription_query_job(
    conn: &Connection,
    run_id: Option<i64>,
    subscription_id: i64,
    query_id: i64,
    site_id: &str,
    job_kind: &str,
    requested_by: &str,
    post_id: Option<&str>,
) -> rusqlite::Result<(i64, bool)> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT job_id
             FROM subscription_query_job
             WHERE subscription_id = ?1
               AND query_id = ?2
               AND job_kind = ?3
               AND status IN ('queued', 'running')
               AND ((post_id IS NULL AND ?4 IS NULL) OR post_id = ?4)
             ORDER BY job_id DESC
             LIMIT 1",
            params![subscription_id, query_id, job_kind, post_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(job_id) = existing {
        // Deduplicated against an in-flight job — the caller must know no new
        // work was queued, or it will create run rows nothing ever finalizes.
        return Ok((job_id, false));
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_query_job (
             run_id, subscription_id, query_id, site_id, status, job_kind, requested_by,
             post_id, available_at, queued_at
         ) VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, ?7, ?8, ?8)",
        params![
            run_id,
            subscription_id,
            query_id,
            site_id,
            job_kind,
            requested_by,
            post_id,
            now
        ],
    )?;
    Ok((conn.last_insert_rowid(), true))
}

/// Finalize every still-'running' run row for a subscription. Used by stop and
/// cleanup paths where no executor is alive to do it.
pub fn finalize_open_runs_for_subscription(
    conn: &Connection,
    subscription_id: i64,
    status: &str,
    failure_kind: Option<&str>,
    error_message: Option<&str>,
) -> rusqlite::Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_run
         SET status = ?1, finished_at = ?2,
             failure_kind = COALESCE(failure_kind, ?3),
             error_message = COALESCE(error_message, ?4)
         WHERE subscription_id = ?5 AND status = 'running'",
        params![status, now, failure_kind, error_message, subscription_id],
    )
}

pub fn list_queued_subscription_query_jobs(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryJob>> {
    let mut stmt = conn.prepare_cached(
        "SELECT job_id, run_id, subscription_id, query_id, site_id, status, job_kind,
                requested_by, post_id, attempt_count, available_at, queued_at, started_at,
                finished_at, failure_kind, error_message
         FROM subscription_query_job
         WHERE status = 'queued' AND available_at <= ?1
         ORDER BY available_at ASC, job_id ASC
         LIMIT ?2",
    )?;
    let now = chrono::Utc::now().to_rfc3339();
    let rows = stmt.query_map(params![now, limit], map_subscription_query_job_row)?;
    rows.collect()
}

pub fn list_subscription_query_jobs_for_run(
    conn: &Connection,
    run_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryJob>> {
    let mut stmt = conn.prepare_cached(
        "SELECT job_id, run_id, subscription_id, query_id, site_id, status, job_kind,
                requested_by, post_id, attempt_count, available_at, queued_at, started_at,
                finished_at, failure_kind, error_message
         FROM subscription_query_job
         WHERE run_id = ?1
         ORDER BY job_id ASC",
    )?;
    let rows = stmt.query_map([run_id], map_subscription_query_job_row)?;
    rows.collect()
}

pub fn lease_subscription_query_job(
    conn: &Connection,
    job_id: i64,
) -> rusqlite::Result<Option<SubscriptionQueryJob>> {
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn.execute(
        "UPDATE subscription_query_job
         SET status = 'running', started_at = COALESCE(started_at, ?1), failure_kind = NULL, error_message = NULL
         WHERE job_id = ?2 AND status = 'queued'",
        params![now, job_id],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    conn.query_row(
        "SELECT job_id, run_id, subscription_id, query_id, site_id, status, job_kind,
                requested_by, post_id, attempt_count, available_at, queued_at, started_at,
                finished_at, failure_kind, error_message
         FROM subscription_query_job
         WHERE job_id = ?1",
        [job_id],
        map_subscription_query_job_row,
    )
    .optional()
}

pub fn finish_subscription_query_job(
    conn: &Connection,
    job_id: i64,
    status: &str,
    failure_kind: Option<&str>,
    error_message: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_query_job
         SET status = ?1,
             finished_at = ?2,
             failure_kind = ?3,
             error_message = ?4
         WHERE job_id = ?5",
        params![status, now, failure_kind, error_message, job_id],
    )?;
    Ok(())
}

pub fn reschedule_subscription_query_job(
    conn: &Connection,
    job_id: i64,
    available_at: &str,
    failure_kind: &str,
    error_message: Option<&str>,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "UPDATE subscription_query_job
         SET status = 'queued', attempt_count = attempt_count + 1,
             available_at = ?1, started_at = NULL, finished_at = NULL,
             failure_kind = ?2, error_message = ?3
         WHERE job_id = ?4 AND status = 'running'",
        params![available_at, failure_kind, error_message, job_id],
    )?;
    Ok(changed == 1)
}

pub fn requeue_interrupted_subscription_query_job(
    conn: &Connection,
    job_id: i64,
) -> rusqlite::Result<bool> {
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn.execute(
        "UPDATE subscription_query_job
         SET status = 'queued', available_at = ?1, started_at = NULL,
             finished_at = NULL, failure_kind = NULL, error_message = NULL
         WHERE job_id = ?2 AND status = 'running'",
        params![now, job_id],
    )?;
    Ok(changed == 1)
}

pub fn set_subscription_issue_next_retry(
    conn: &Connection,
    subscription_id: i64,
    query_id: i64,
    failure_kind: FailureKind,
    next_retry_at: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_issue
         SET next_retry_at = ?1
         WHERE subscription_id = ?2 AND query_id = ?3
           AND issue_kind = ?4 AND status = 'open'",
        params![
            next_retry_at,
            subscription_id,
            query_id,
            failure_kind.issue_kind()
        ],
    )?;
    Ok(())
}

pub fn cancel_pending_subscription_jobs_for_subscription(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_query_job
         SET status = 'cancelled',
             finished_at = ?1,
             error_message = COALESCE(error_message, 'Cancelled before execution')
         WHERE subscription_id = ?2
           AND status = 'queued'",
        params![now, subscription_id],
    )
}

pub fn count_active_subscription_query_jobs(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM subscription_query_job
         WHERE subscription_id = ?1
           AND status IN ('queued', 'running')",
        [subscription_id],
        |row| row.get(0),
    )
}

pub fn update_query_progress(
    conn: &Connection,
    query_id: i64,
    last_check_time: &str,
    files_found: i64,
    posts_found: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET last_check_time = ?1, files_found = ?2, posts_found = ?3
         WHERE query_id = ?4",
        params![last_check_time, files_found, posts_found, query_id],
    )?;
    Ok(())
}

pub fn reset_subscription_query_state(
    conn: &Connection,
    query_id: i64,
) -> rusqlite::Result<(usize, usize, usize, usize, usize)> {
    let query_reset = conn.execute(
        "UPDATE subscription_query
         SET files_found = 0,
             posts_found = 0,
             last_check_time = NULL,
             completed_initial_run = 0,
             resume_cursor = NULL,
             resume_strategy = NULL,
             last_success_at = NULL,
             last_failure_at = NULL,
             last_failure_kind = NULL,
             last_failure_message = NULL
         WHERE query_id = ?1",
        [query_id],
    )?;

    conn.execute(
        "DELETE FROM subscription_query_job WHERE query_id = ?1",
        [query_id],
    )?;
    let query_runs_deleted = conn.execute(
        "DELETE FROM subscription_query_run WHERE query_id = ?1",
        [query_id],
    )?;

    let issues_deleted = conn.execute(
        "DELETE FROM subscription_issue WHERE query_id = ?1",
        [query_id],
    )?;

    let attempts_deleted = conn.execute(
        "DELETE FROM subscription_download_attempt WHERE query_id = ?1",
        [query_id],
    )?;

    let queues_deleted =
        conn.execute("DELETE FROM ingest_queue WHERE query_id = ?1", [query_id])?;

    Ok((
        query_reset,
        query_runs_deleted,
        issues_deleted,
        attempts_deleted,
        queues_deleted,
    ))
}

pub fn reset_subscription_state(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<(usize, usize, usize)> {
    let queries_reset = conn.execute(
        "UPDATE subscription_query
         SET files_found = 0,
             posts_found = 0,
             last_check_time = NULL,
             completed_initial_run = 0,
             resume_cursor = NULL,
             resume_strategy = NULL,
             last_success_at = NULL,
             last_failure_at = NULL,
             last_failure_kind = NULL,
             last_failure_message = NULL
         WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    conn.execute(
        "DELETE FROM ingest_queue WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_query_job WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_query_run WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_run WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_issue WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_download_attempt WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_post_member WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    let entities_deleted = conn.execute(
        "DELETE FROM subscription_entity WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    let post_maps_deleted = conn.execute(
        "DELETE FROM subscription_post_collection WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    Ok((queries_reset, entities_deleted, post_maps_deleted))
}

pub fn set_query_resume_state(
    conn: &Connection,
    query_id: i64,
    resume_cursor: Option<&str>,
    resume_strategy: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query
         SET resume_cursor = ?1, resume_strategy = ?2
         WHERE query_id = ?3",
        params![resume_cursor, resume_strategy, query_id],
    )?;
    Ok(())
}

pub fn set_query_completed_initial_run(
    conn: &Connection,
    query_id: i64,
    completed: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET completed_initial_run = ?1 WHERE query_id = ?2",
        params![completed as i64, query_id],
    )?;
    Ok(())
}

pub fn set_query_terminal_state(
    conn: &Connection,
    query_id: i64,
    last_success_at: Option<&str>,
    last_failure_at: Option<&str>,
    last_failure_kind: Option<&str>,
    last_failure_message: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query
         SET last_success_at = ?1,
             last_failure_at = ?2,
             last_failure_kind = ?3,
             last_failure_message = ?4
         WHERE query_id = ?5",
        params![
            last_success_at,
            last_failure_at,
            last_failure_kind,
            last_failure_message,
            query_id
        ],
    )?;
    Ok(())
}

pub fn upsert_subscription_issue(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    failure_kind: FailureKind,
    message: &str,
    detail: Option<&str>,
) -> rusqlite::Result<Option<i64>> {
    if !failure_kind.creates_issue() {
        return Ok(None);
    }
    let now = chrono::Utc::now().to_rfc3339();
    let issue_kind = failure_kind.issue_kind();
    let recovery_action = failure_kind.recovery_action().as_str();
    let issue_key = query_id
        .map(|query_id| format!("query:{query_id}:{issue_kind}"))
        .unwrap_or_else(|| format!("subscription:{subscription_id}:{issue_kind}"));
    conn.execute(
        "INSERT INTO subscription_issue (
             issue_key, subscription_id, query_id, issue_kind, status, message, detail,
             first_seen_at, last_seen_at, recovery_action, next_retry_at
         ) VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6, ?7, ?7, ?8, NULL)
         ON CONFLICT(issue_key)
         DO UPDATE SET status = 'open',
                       message = excluded.message,
                       detail = excluded.detail,
                       last_seen_at = excluded.last_seen_at,
                       recovery_action = excluded.recovery_action,
                       next_retry_at = NULL,
                       resolved_at = NULL",
        params![
            issue_key,
            subscription_id,
            query_id,
            issue_kind,
            message,
            detail,
            now,
            recovery_action,
        ],
    )?;
    conn.query_row(
        "SELECT issue_id FROM subscription_issue
         WHERE issue_key = ?1",
        [issue_key],
        |row| row.get(0),
    )
    .map(Some)
}

pub fn resolve_subscription_issues(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    failure_kind: FailureKind,
) -> rusqlite::Result<()> {
    if !failure_kind.creates_issue() {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let issue_kind = failure_kind.issue_kind();
    conn.execute(
        "UPDATE subscription_issue
         SET status = 'resolved',
             resolved_at = ?1
         WHERE subscription_id = ?2
           AND query_id IS ?3
           AND issue_kind = ?4
           AND status != 'resolved'",
        params![now, subscription_id, query_id, issue_kind],
    )?;
    Ok(())
}

pub fn list_subscription_issues(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionIssueRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT issue_id, issue_key, subscription_id, query_id, issue_kind, status,
                message, detail, first_seen_at, last_seen_at, resolved_at,
                recovery_action, next_retry_at
         FROM subscription_issue
         WHERE subscription_id = ?1
           AND (?2 IS NULL OR query_id = ?2)
         ORDER BY last_seen_at DESC, issue_id DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, limit],
        map_subscription_issue_row,
    )?;
    rows.collect()
}

pub fn upsert_subscription_download_attempt(
    conn: &Connection,
    input: SubscriptionDownloadAttemptUpsert<'_>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_download_attempt (
             subscription_id, query_id, query_run_id, item_key, site_category, post_id, page_num,
             canonical_post_url, media_url, retry_url, retry_count, status, failure_kind,
             last_error, next_retry_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 'pending', ?11, ?12, ?13, ?14, ?14)
         ON CONFLICT(subscription_id, query_id, item_key)
         DO UPDATE SET query_run_id = excluded.query_run_id,
                       site_category = excluded.site_category,
                       post_id = excluded.post_id,
                       page_num = excluded.page_num,
                       canonical_post_url = excluded.canonical_post_url,
                       media_url = excluded.media_url,
                       retry_url = excluded.retry_url,
                       retry_count = subscription_download_attempt.retry_count + 1,
                       status = 'pending',
                       failure_kind = excluded.failure_kind,
                       last_error = excluded.last_error,
                       next_retry_at = excluded.next_retry_at,
                       updated_at = excluded.updated_at,
                       resolved_at = NULL",
        params![
            input.subscription_id,
            input.query_id,
            input.query_run_id,
            input.item_key,
            input.site_category,
            input.post_id,
            input.page_num,
            input.canonical_post_url,
            input.media_url,
            input.retry_url,
            input.failure_kind,
            input.last_error,
            input.next_retry_at,
            now
        ],
    )?;
    conn.query_row(
        "SELECT attempt_id FROM subscription_download_attempt
         WHERE subscription_id = ?1 AND query_id IS ?2 AND item_key = ?3",
        params![input.subscription_id, input.query_id, input.item_key],
        |row| row.get(0),
    )
}

pub fn resolve_subscription_download_attempt(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    item_key: &str,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_download_attempt
         SET status = 'resolved',
             resolved_at = ?1,
             updated_at = ?1,
             next_retry_at = NULL
         WHERE subscription_id = ?2
           AND query_id IS ?3
           AND item_key = ?4",
        params![now, subscription_id, query_id, item_key],
    )?;
    Ok(())
}

pub fn mark_subscription_download_attempt_retrying(
    conn: &Connection,
    attempt_id: i64,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE subscription_download_attempt
         SET status = 'retrying',
             updated_at = ?1
         WHERE attempt_id = ?2",
        params![now, attempt_id],
    )?;
    Ok(())
}

pub fn list_retryable_subscription_download_attempts(
    conn: &Connection,
    subscription_id: i64,
    query_id: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionDownloadAttemptRecord>> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare_cached(
        "SELECT attempt_id, subscription_id, query_id, query_run_id, item_key, site_category,
                post_id, page_num, canonical_post_url, media_url, retry_url, retry_count,
                status, failure_kind, last_error, next_retry_at, created_at, updated_at, resolved_at
         FROM subscription_download_attempt
         WHERE subscription_id = ?1
           AND query_id = ?2
           AND status IN ('pending', 'retrying')
           AND (next_retry_at IS NULL OR next_retry_at <= ?3)
         ORDER BY attempt_id ASC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, now, limit],
        map_subscription_download_attempt_row,
    )?;
    rows.collect()
}

pub fn find_unresolved_subscription_download_attempts(
    conn: &Connection,
    subscription_id: i64,
    query_id: i64,
    site_category: &str,
    post_id: &str,
) -> rusqlite::Result<Vec<SubscriptionDownloadAttemptRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT attempt_id, subscription_id, query_id, query_run_id, item_key, site_category,
                post_id, page_num, canonical_post_url, media_url, retry_url, retry_count,
                status, failure_kind, last_error, next_retry_at, created_at, updated_at, resolved_at
         FROM subscription_download_attempt
         WHERE subscription_id = ?1
           AND query_id = ?2
           AND site_category = ?3
           AND post_id = ?4
           AND status <> 'resolved'
         ORDER BY updated_at DESC, attempt_id DESC",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, site_category, post_id],
        map_subscription_download_attempt_row,
    )?;
    rows.collect()
}

pub fn list_subscription_download_attempts(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionDownloadAttemptRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT attempt_id, subscription_id, query_id, query_run_id, item_key, site_category,
                post_id, page_num, canonical_post_url, media_url, retry_url, retry_count,
                status, failure_kind, last_error, next_retry_at, created_at, updated_at, resolved_at
         FROM subscription_download_attempt
         WHERE subscription_id = ?1
           AND (?2 IS NULL OR query_id = ?2)
         ORDER BY updated_at DESC, attempt_id DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, limit],
        map_subscription_download_attempt_row,
    )?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::find_unresolved_subscription_download_attempts;
    use crate::db::core::schema::LIBRARY_DDL;
    use rusqlite::{params, Connection};

    fn insert_attempt(
        conn: &Connection,
        attempt_id: i64,
        item_key: &str,
        site_category: &str,
        post_id: &str,
        status: &str,
        updated_at: &str,
        retry_url: &str,
    ) {
        conn.execute(
            "INSERT INTO subscription_download_attempt (
                 attempt_id, subscription_id, query_id, item_key, site_category, post_id,
                 status, retry_url, created_at, updated_at
             ) VALUES (?1, 1, 1, ?2, ?3, ?4, ?5, ?6, '2026-01-01', ?7)",
            params![
                attempt_id,
                item_key,
                site_category,
                post_id,
                status,
                retry_url,
                updated_at
            ],
        )
        .expect("insert download attempt");
    }

    #[test]
    fn retry_lookup_returns_all_matching_attempts_newest_first() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, site_id, date_added)
             VALUES (1, 'Test subscription', 'danbooru', '2026-01-01')",
            [],
        )
        .expect("insert subscription");
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, site_id, query_text)
             VALUES (1, 1, 'danbooru', 'tag:test')",
            [],
        )
        .expect("insert query");

        insert_attempt(
            &conn,
            1,
            "old-match",
            "danbooru",
            "post-1",
            "pending",
            "2026-01-01",
            "https://retry.example/old",
        );
        insert_attempt(
            &conn,
            2,
            "new-match",
            "danbooru",
            "post-1",
            "retrying",
            "2026-03-01",
            "https://retry.example/new",
        );
        insert_attempt(
            &conn,
            5,
            "new-match-higher-id",
            "danbooru",
            "post-1",
            "pending",
            "2026-03-01",
            "https://retry.example/new-higher-id",
        );
        insert_attempt(
            &conn,
            3,
            "resolved",
            "danbooru",
            "post-1",
            "resolved",
            "2026-04-01",
            "https://retry.example/resolved",
        );
        insert_attempt(
            &conn,
            4,
            "wrong-site",
            "gelbooru",
            "post-1",
            "pending",
            "2026-04-01",
            "https://retry.example/wrong-site",
        );
        for attempt_id in 10..515 {
            insert_attempt(
                &conn,
                attempt_id,
                &format!("newer-{attempt_id}"),
                "danbooru",
                &format!("post-{attempt_id}"),
                "pending",
                "2026-02-01",
                &format!("https://retry.example/noise-{attempt_id}"),
            );
        }

        let query_plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN
                 SELECT attempt_id
                 FROM subscription_download_attempt
                 WHERE subscription_id = ?1
                   AND query_id = ?2
                   AND site_category = ?3
                   AND post_id = ?4
                   AND status <> 'resolved'
                 ORDER BY updated_at DESC, attempt_id DESC",
                params![1, 1, "danbooru", "post-1"],
                |row| row.get(3),
            )
            .expect("explain retry lookup");
        assert!(
            query_plan.contains("idx_subscription_download_attempt_retry"),
            "retry lookup must use its index: {query_plan}"
        );

        let matches =
            find_unresolved_subscription_download_attempts(&conn, 1, 1, "danbooru", "post-1")
                .expect("find retry attempts");

        assert_eq!(
            matches
                .iter()
                .map(|attempt| attempt.item_key.as_str())
                .collect::<Vec<_>>(),
            ["new-match-higher-id", "new-match", "old-match"]
        );
        assert_eq!(
            matches
                .iter()
                .find_map(|attempt| attempt.retry_url.as_deref()),
            Some("https://retry.example/new-higher-id")
        );
    }
}

pub fn upsert_subscription_post_member(
    conn: &Connection,
    input: SubscriptionPostMemberUpsert<'_>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_post_member (
             subscription_id, site_id, post_id, item_key, page_num,
             canonical_post_url, media_url, entity_hash, status, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT(subscription_id, site_id, post_id, item_key)
         DO UPDATE SET page_num = excluded.page_num,
                       canonical_post_url = excluded.canonical_post_url,
                       media_url = excluded.media_url,
                       entity_hash = excluded.entity_hash,
                       status = excluded.status,
                       updated_at = excluded.updated_at",
        params![
            input.subscription_id,
            input.site_id,
            input.post_id,
            input.item_key,
            input.page_num,
            input.canonical_post_url,
            input.media_url,
            input.entity_hash,
            input.status,
            now,
        ],
    )?;
    Ok(())
}

pub fn add_subscription_entity(
    conn: &Connection,
    subscription_id: i64,
    entity_id: i64,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id) VALUES (?1, ?2)",
        params![subscription_id, entity_id],
    )?;
    Ok(changed > 0)
}

pub fn upsert_subscription_post_collection(
    conn: &Connection,
    subscription_id: i64,
    site_id: &str,
    post_id: &str,
    collection_entity_id: i64,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_post_collection (
             subscription_id, site_id, post_id, collection_entity_id, date_added, date_modified
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(subscription_id, site_id, post_id)
         DO UPDATE SET collection_entity_id = excluded.collection_entity_id,
                       date_modified = excluded.date_modified",
        params![subscription_id, site_id, post_id, collection_entity_id, now],
    )?;
    Ok(())
}

pub fn get_subscription_post_collection(
    conn: &Connection,
    subscription_id: i64,
    site_id: &str,
    post_id: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT collection_entity_id
         FROM subscription_post_collection
         WHERE subscription_id = ?1 AND site_id = ?2 AND post_id = ?3",
        params![subscription_id, site_id, post_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn list_subscription_post_members(
    conn: &Connection,
    subscription_id: i64,
    site_id: &str,
    post_id: &str,
) -> rusqlite::Result<Vec<SubscriptionPostMemberRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT subscription_id, site_id, post_id, item_key, page_num,
                canonical_post_url, media_url, entity_hash, status, created_at, updated_at
         FROM subscription_post_member
         WHERE subscription_id = ?1
           AND site_id = ?2
           AND post_id = ?3
         ORDER BY COALESCE(page_num, 9223372036854775807) ASC, item_key ASC",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, site_id, post_id],
        map_subscription_post_member_row,
    )?;
    rows.collect()
}
