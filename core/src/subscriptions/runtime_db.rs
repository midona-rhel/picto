use rusqlite::{params, Connection, OptionalExtension};

use super::types::{
    SubscriptionDownloadAttemptRecord, SubscriptionDownloadAttemptUpsert, SubscriptionIssueRecord,
    SubscriptionPostMemberUpsert, SubscriptionQueryJob, SubscriptionQueryRunCompletion,
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
        metadata_validated: row.get(12)?,
        metadata_invalid: row.get(13)?,
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

#[derive(Debug, Default)]
pub struct SubscriptionReconcileReport {
    pub source_complete_jobs_finalized: usize,
    pub jobs_requeued: usize,
    pub orphan_runs_finalized: usize,
    pub query_runs_finalized: usize,
    pub health_rows_repaired: usize,
    pub query_kinds_repaired: usize,
}

/// Restore durable subscription work after an app quit/crash mid-run.
///
/// Safe only at library open, before the site-runner worker starts. A leased
/// job returns to the queue and keeps its original full-run identity.
/// Execution history records the interruption, but queued jobs and runs remain
/// active so the normal worker can finish them after startup.
pub fn reconcile_stale_subscription_runtime(
    conn: &Connection,
) -> rusqlite::Result<SubscriptionReconcileReport> {
    let now = chrono::Utc::now().to_rfc3339();
    let stale = FailureKind::Stale.as_str();
    let interrupted = "Interrupted — the app was closed while this run was active";
    let mut report = SubscriptionReconcileReport::default();

    let source_complete_jobs: Vec<(i64, String, Option<String>, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT job.job_id, substr(query_run.status, 10),
                    query_run.failure_kind, query_run.error_message
             FROM subscription_query_job job
             JOIN subscription_query_run query_run
               ON query_run.subscription_id = job.subscription_id
              AND query_run.query_id = job.query_id
              AND query_run.run_id IS job.run_id
              AND query_run.started_at >= job.started_at
             WHERE job.status = 'running'
               AND query_run.status LIKE 'settling_%'
               AND query_run.query_run_id = (
                   SELECT MAX(candidate.query_run_id)
                   FROM subscription_query_run candidate
                   WHERE candidate.subscription_id = job.subscription_id
                     AND candidate.query_id = job.query_id
                     AND candidate.run_id IS job.run_id
                     AND candidate.started_at >= job.started_at
               )",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (job_id, status, failure_kind, error_message) in source_complete_jobs {
        report.source_complete_jobs_finalized += conn.execute(
            "UPDATE subscription_query_job
             SET status = ?1, finished_at = ?2,
                 failure_kind = COALESCE(failure_kind, ?3),
                 error_message = COALESCE(error_message, ?4)
             WHERE job_id = ?5 AND status = 'running'",
            params![status, now, failure_kind, error_message, job_id],
        )?;
    }
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
           )
           AND NOT EXISTS (
               SELECT 1
               FROM ingest_queue q
               JOIN subscription_query_run qr ON qr.query_run_id = q.query_run_id
               WHERE qr.run_id = subscription_run.run_id
                 AND q.status IN ('pending', 'running')
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
    let is_running: bool = conn
        .query_row(
            "SELECT status = 'running' FROM subscription_run WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(false);
    if !is_running {
        return Ok(None);
    }
    let active_jobs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM subscription_query_job
         WHERE run_id = ?1 AND status IN ('queued', 'running')",
        [run_id],
        |row| row.get(0),
    )?;
    if active_jobs > 0 {
        return Ok(None);
    }

    let active_query_runs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM subscription_query_run
         WHERE run_id = ?1 AND finished_at IS NULL",
        [run_id],
        |row| row.get(0),
    )?;
    if active_query_runs > 0 {
        return Ok(None);
    }

    let active_ingest: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM ingest_queue q
         JOIN subscription_query_run qr ON qr.query_run_id = q.query_run_id
         WHERE qr.run_id = ?1 AND q.status IN ('pending', 'running')",
        [run_id],
        |row| row.get(0),
    )?;
    if active_ingest > 0 {
        return Ok(None);
    }

    let job_failure: Option<(String, Option<String>, Option<String>)> = conn
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
    let ingest_failure: Option<String> = conn
        .query_row(
            "SELECT COALESCE(q.last_error, 'Subscription ingest failed')
             FROM ingest_queue q
             JOIN subscription_query_run qr ON qr.query_run_id = q.query_run_id
             WHERE qr.run_id = ?1 AND q.status = 'failed'
             ORDER BY q.queue_id DESC
             LIMIT 1",
            [run_id],
            |row| row.get(0),
        )
        .optional()?;
    let (status, failure_kind, error_message) = match job_failure {
        Some((job_status, failure_kind, error_message)) => {
            let status = if job_status == "failed" {
                "failed"
            } else {
                "cancelled"
            };
            (status, failure_kind, error_message)
        }
        None if ingest_failure.is_some() => (
            "failed",
            Some(FailureKind::IngestQueueFailure.as_str().to_string()),
            ingest_failure,
        ),
        None => ("succeeded", None, None),
    };
    let (files_downloaded, source_skipped): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(files_downloaded), 0),
                COALESCE(SUM(files_skipped), 0)
         FROM subscription_query_run
         WHERE run_id = ?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let reused: i64 = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN i.result_kind = 'reused' THEN 1 ELSE 0 END), 0)
         FROM ingest_queue q
         JOIN subscription_query_run qr ON qr.query_run_id = q.query_run_id
         JOIN ingest_queue_item i ON i.queue_id = q.queue_id
         WHERE qr.run_id = ?1",
        [run_id],
        |row| row.get(0),
    )?;
    let files_skipped = source_skipped + reused;
    let (metadata_validated, metadata_invalid): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(metadata_validated), 0),
                COALESCE(SUM(metadata_invalid), 0)
         FROM subscription_query_run
         WHERE run_id = ?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn.execute(
        "UPDATE subscription_run
         SET finished_at = ?1,
             status = ?2,
             failure_kind = ?3,
             error_message = ?4,
             files_downloaded = ?5,
             files_skipped = ?6,
             metadata_validated = ?7,
             metadata_invalid = ?8
         WHERE run_id = ?9 AND status = 'running'",
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
    if changed == 0 {
        return Ok(None);
    }
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

pub fn list_running_subscription_run_ids(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT run_id FROM subscription_run WHERE status = 'running' ORDER BY run_id",
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
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

pub(crate) fn checkpoint_subscription_query_progress(
    conn: &Connection,
    query_run_id: i64,
    files_downloaded: i64,
    posts_processed: i64,
    metadata_validated: i64,
) -> rusqlite::Result<()> {
    let query_id: i64 = conn.query_row(
        "SELECT query_id
         FROM subscription_query_run
         WHERE query_run_id = ?1 AND status = 'running'",
        [query_run_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE subscription_query_run
         SET files_downloaded = files_downloaded + ?1,
             posts_processed = posts_processed + ?2,
             metadata_validated = metadata_validated + ?3
         WHERE query_run_id = ?4 AND status = 'running'",
        params![
            files_downloaded,
            posts_processed,
            metadata_validated,
            query_run_id
        ],
    )?;
    conn.execute(
        "UPDATE subscription_query
         SET files_found = files_found + ?1,
             posts_found = posts_found + ?2
         WHERE query_id = ?3",
        params![files_downloaded, posts_processed, query_id],
    )?;
    Ok(())
}

pub fn record_subscription_query_source_completion(
    conn: &Connection,
    query_run_id: i64,
    completion: &SubscriptionQueryRunCompletion,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query_run
         SET status = ?1,
             failure_kind = ?2,
             error_message = ?3,
             posts_processed = MAX(posts_processed, ?4),
             files_downloaded = MAX(files_downloaded, ?5),
             files_skipped = MAX(files_skipped, ?6),
             metadata_validated = MAX(metadata_validated, ?7),
             metadata_invalid = MAX(metadata_invalid, ?8)
         WHERE query_run_id = ?9 AND status = 'running'",
        params![
            format!("settling_{}", completion.status),
            completion.failure_kind,
            completion.error_message,
            completion.posts_processed,
            completion.files_downloaded,
            completion.files_skipped,
            completion.metadata_validated,
            completion.metadata_invalid,
            query_run_id
        ],
    )?;
    Ok(())
}

pub fn finalize_subscription_query_run_if_terminal(
    conn: &Connection,
    query_run_id: i64,
) -> rusqlite::Result<Option<SubscriptionQueryRunRecord>> {
    let state: Option<(
        String,
        Option<String>,
        Option<String>,
        bool,
        bool,
        Option<String>,
    )> = conn
        .query_row(
            "SELECT query_run.status, query_run.failure_kind, query_run.error_message,
                    EXISTS (
                        SELECT 1 FROM subscription_query_job job
                        WHERE job.subscription_id = query_run.subscription_id
                          AND job.query_id = query_run.query_id
                          AND job.run_id IS query_run.run_id
                          AND job.status = 'running'
                          AND job.started_at IS NOT NULL
                          AND job.started_at <= query_run.started_at
                    ),
                    EXISTS (
                        SELECT 1 FROM ingest_queue queue
                        WHERE queue.query_run_id = query_run.query_run_id
                          AND queue.status IN ('pending', 'running')
                    ),
                    (
                        SELECT COALESCE(queue.last_error, 'Subscription ingest failed')
                        FROM ingest_queue queue
                        WHERE queue.query_run_id = query_run.query_run_id
                          AND queue.status = 'failed'
                        ORDER BY queue.queue_id DESC LIMIT 1
                    )
             FROM subscription_query_run query_run
             WHERE query_run.query_run_id = ?1
               AND query_run.status LIKE 'settling_%'",
            [query_run_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((
        settling_status,
        source_failure_kind,
        source_error_message,
        active_source_job,
        active_ingest,
        ingest_failure,
    )) = state
    else {
        return Ok(None);
    };
    if active_source_job || active_ingest {
        return Ok(None);
    }
    let source_status = settling_status
        .strip_prefix("settling_")
        .unwrap_or("failed")
        .to_string();
    let (status, failure_kind, error_message) = if source_status == "succeeded" {
        if let Some(error) = ingest_failure {
            (
                "failed".to_string(),
                Some(FailureKind::IngestQueueFailure.as_str().to_string()),
                Some(error),
            )
        } else {
            (source_status, source_failure_kind, source_error_message)
        }
    } else {
        (source_status, source_failure_kind, source_error_message)
    };
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn.execute(
        "UPDATE subscription_query_run
         SET finished_at = ?1, status = ?2, failure_kind = ?3, error_message = ?4
         WHERE query_run_id = ?5 AND status = ?6",
        params![
            now,
            status,
            failure_kind,
            error_message,
            query_run_id,
            settling_status
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    conn.query_row(
        "SELECT query_run_id, run_id, subscription_id, query_id, started_at, finished_at,
                status, failure_kind, error_message, posts_processed,
                files_downloaded, files_skipped, metadata_validated, metadata_invalid
         FROM subscription_query_run WHERE query_run_id = ?1",
        [query_run_id],
        map_subscription_query_run_row,
    )
    .optional()
}

pub fn list_subscription_query_runs(
    conn: &Connection,
    query_id: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryRunRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT query_run_id, run_id, subscription_id, query_id, started_at, finished_at,
                CASE WHEN status LIKE 'settling_%' THEN 'running' ELSE status END,
                failure_kind, error_message, posts_processed,
                files_downloaded, files_skipped, metadata_validated, metadata_invalid
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

pub fn add_query_progress(
    conn: &Connection,
    query_id: i64,
    last_check_time: Option<&str>,
    files_found_delta: i64,
    posts_found_delta: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query
         SET last_check_time = COALESCE(?1, last_check_time),
             files_found = files_found + ?2,
             posts_found = posts_found + ?3
         WHERE query_id = ?4",
        params![
            last_check_time,
            files_found_delta,
            posts_found_delta,
            query_id
        ],
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
) -> rusqlite::Result<(usize, usize)> {
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

    Ok((queries_reset, entities_deleted))
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

pub fn list_subscription_issues_page(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    cursor: Option<i64>,
    limit: i64,
) -> rusqlite::Result<crate::subscriptions::types::SubscriptionIssuePage> {
    let total_count = conn.query_row(
        "SELECT COUNT(*)
         FROM subscription_issue
         WHERE subscription_id = ?1
           AND (?2 IS NULL OR query_id = ?2)
           AND status <> 'resolved'",
        params![subscription_id, query_id],
        |row| row.get(0),
    )?;
    let mut stmt = conn.prepare_cached(
        "SELECT issue_id, issue_key, subscription_id, query_id, issue_kind, status,
                message, detail, first_seen_at, last_seen_at, resolved_at,
                recovery_action, next_retry_at
         FROM subscription_issue
         WHERE subscription_id = ?1
           AND (?2 IS NULL OR query_id = ?2)
           AND status <> 'resolved'
           AND (?3 IS NULL OR issue_id < ?3)
         ORDER BY issue_id DESC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, cursor, limit + 1],
        map_subscription_issue_row,
    )?;
    let mut items: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
    let has_more = items.len() as i64 > limit;
    if has_more {
        items.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| items.last().map(|issue| issue.issue_id))
        .flatten();
    Ok(crate::subscriptions::types::SubscriptionIssuePage {
        items,
        next_cursor,
        total_count,
    })
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
    let changed = conn.execute(
        "UPDATE subscription_download_attempt
         SET status = 'resolved',
             resolved_at = ?1,
             updated_at = ?1,
             next_retry_at = NULL
         WHERE subscription_id = ?2
           AND query_id IS ?3
           AND item_key = ?4
           AND status <> 'resolved'
           AND resolved_at IS NULL",
        params![now, subscription_id, query_id, item_key],
    )?;
    if changed > 0 {
        let unresolved: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM subscription_download_attempt
             WHERE subscription_id = ?1
               AND query_id IS ?2
               AND status NOT IN ('resolved', 'succeeded')
               AND resolved_at IS NULL",
            params![subscription_id, query_id],
            |row| row.get(0),
        )?;
        if unresolved == 0 {
            resolve_subscription_issues(
                conn,
                subscription_id,
                query_id,
                FailureKind::DownloadFailure,
            )?;
        }
    }
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

pub fn find_unresolved_subscription_post_attempts(
    conn: &Connection,
    subscription_id: i64,
    query_id: i64,
    post_id: &str,
) -> rusqlite::Result<Vec<SubscriptionDownloadAttemptRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT attempt_id, subscription_id, query_id, query_run_id, item_key, site_category,
                post_id, page_num, canonical_post_url, media_url, retry_url, retry_count,
                status, failure_kind, last_error, next_retry_at, created_at, updated_at, resolved_at
         FROM subscription_download_attempt
         WHERE subscription_id = ?1
           AND query_id = ?2
           AND post_id = ?3
           AND status <> 'resolved'
         ORDER BY updated_at DESC, attempt_id DESC",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, post_id],
        map_subscription_download_attempt_row,
    )?;
    rows.collect()
}

pub fn list_subscription_retry_targets(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<crate::subscriptions::types::SubscriptionRetryTarget>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT attempt.query_id, query.site_id, attempt.post_id
         FROM subscription_download_attempt attempt
         JOIN subscription_query query ON query.query_id = attempt.query_id
         WHERE attempt.subscription_id = ?1
           AND attempt.query_id IS NOT NULL
           AND attempt.post_id IS NOT NULL
           AND attempt.status NOT IN ('resolved', 'succeeded')
           AND attempt.resolved_at IS NULL
           AND COALESCE(attempt.retry_url, attempt.canonical_post_url) IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM subscription_issue issue
               WHERE issue.subscription_id = attempt.subscription_id
                 AND issue.query_id = attempt.query_id
                 AND issue.status = 'open'
                 AND issue.recovery_action = 'retry_now'
           )
         ORDER BY attempt.query_id ASC, attempt.post_id ASC",
    )?;
    let rows = stmt.query_map([subscription_id], |row| {
        Ok(crate::subscriptions::types::SubscriptionRetryTarget {
            query_id: row.get(0)?,
            site_id: row.get(1)?,
            post_id: row.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn list_subscription_download_attempts_page(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    cursor: Option<i64>,
    limit: i64,
) -> rusqlite::Result<crate::subscriptions::types::SubscriptionDownloadAttemptPage> {
    let failed_post_count = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT attempt.query_id, attempt.post_id
             FROM subscription_download_attempt attempt
             WHERE attempt.subscription_id = ?1
               AND (?2 IS NULL OR attempt.query_id = ?2)
               AND attempt.post_id IS NOT NULL
               AND attempt.status NOT IN ('resolved', 'succeeded')
               AND attempt.resolved_at IS NULL
             GROUP BY attempt.query_id, attempt.post_id
         )",
        params![subscription_id, query_id],
        |row| row.get(0),
    )?;
    let retryable_post_count = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT attempt.query_id, attempt.post_id
             FROM subscription_download_attempt attempt
             WHERE attempt.subscription_id = ?1
               AND (?2 IS NULL OR attempt.query_id = ?2)
               AND attempt.query_id IS NOT NULL
               AND attempt.post_id IS NOT NULL
               AND attempt.status NOT IN ('resolved', 'succeeded')
               AND attempt.resolved_at IS NULL
               AND COALESCE(attempt.retry_url, attempt.canonical_post_url) IS NOT NULL
               AND EXISTS (
                   SELECT 1 FROM subscription_issue issue
                   WHERE issue.subscription_id = attempt.subscription_id
                     AND issue.query_id = attempt.query_id
                     AND issue.status = 'open'
                     AND issue.recovery_action = 'retry_now'
               )
             GROUP BY attempt.query_id, attempt.post_id
         )",
        params![subscription_id, query_id],
        |row| row.get(0),
    )?;
    let mut stmt = conn.prepare_cached(
        "SELECT attempt_id, subscription_id, query_id, query_run_id, item_key, site_category,
                post_id, page_num, canonical_post_url, media_url, retry_url, retry_count,
                status, failure_kind, last_error, next_retry_at, created_at, updated_at, resolved_at
         FROM subscription_download_attempt
         WHERE subscription_id = ?1
           AND (?2 IS NULL OR query_id = ?2)
           AND status NOT IN ('resolved', 'succeeded')
           AND resolved_at IS NULL
           AND (?3 IS NULL OR attempt_id < ?3)
         ORDER BY attempt_id DESC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![subscription_id, query_id, cursor, limit + 1],
        map_subscription_download_attempt_row,
    )?;
    let mut items: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
    let has_more = items.len() as i64 > limit;
    if has_more {
        items.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| items.last().map(|attempt| attempt.attempt_id))
        .flatten();
    Ok(
        crate::subscriptions::types::SubscriptionDownloadAttemptPage {
            items,
            next_cursor,
            failed_post_count,
            retryable_post_count,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        add_query_progress, find_unresolved_subscription_post_attempts,
        list_subscription_download_attempts_page, list_subscription_issues_page,
        list_subscription_retry_targets, resolve_subscription_download_attempt,
    };
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
            "INSERT INTO subscription (subscription_id, name, uuid, date_added)
             VALUES (1, 'Test subscription', 'subscription-retry-test', '2026-01-01')",
            [],
        )
        .expect("insert subscription");
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, uuid, site_id, query_text)
             VALUES (1, 1, 'query-retry-test', 'danbooru', 'tag:test')",
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
            "source-alias",
            "danbooru_v2",
            "post-1",
            "pending",
            "2026-04-01",
            "https://retry.example/source-alias",
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
                   AND post_id = ?3
                   AND status <> 'resolved'
                 ORDER BY updated_at DESC, attempt_id DESC",
                params![1, 1, "post-1"],
                |row| row.get(3),
            )
            .expect("explain retry lookup");
        assert!(
            query_plan.contains("idx_subscription_download_attempt_retry"),
            "retry lookup must use its index: {query_plan}"
        );

        let matches = find_unresolved_subscription_post_attempts(&conn, 1, 1, "post-1")
            .expect("find retry attempts");

        assert_eq!(
            matches
                .iter()
                .map(|attempt| attempt.item_key.as_str())
                .collect::<Vec<_>>(),
            [
                "source-alias",
                "new-match-higher-id",
                "new-match",
                "old-match"
            ]
        );
        assert_eq!(
            matches
                .iter()
                .find_map(|attempt| attempt.retry_url.as_deref()),
            Some("https://retry.example/source-alias")
        );
    }

    #[test]
    fn bulk_retry_targets_are_distinct_uncapped_and_use_the_query_site() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, uuid, date_added)
             VALUES (1, 'Test subscription', 'subscription-bulk-retry-test', '2026-01-01')",
            [],
        )
        .expect("insert subscription");
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, uuid, site_id, query_text)
             VALUES (1, 1, 'query-bulk-retry-test', 'pixivuser', '12345')",
            [],
        )
        .expect("insert query");
        conn.execute(
            "INSERT INTO subscription_issue (
                 issue_key, subscription_id, query_id, issue_kind, status, message,
                 first_seen_at, last_seen_at, recovery_action
             ) VALUES ('query:1:download_failure', 1, 1, 'download_failure', 'open',
                       'Download failed', '2026-01-01', '2026-01-01', 'retry_now')",
            [],
        )
        .expect("insert retry issue");

        insert_attempt(
            &conn,
            1,
            "pixiv:42:0",
            "pixiv",
            "42",
            "pending",
            "2026-01-01",
            "https://www.pixiv.net/artworks/42",
        );
        insert_attempt(
            &conn,
            4,
            "pixivuser:42:2",
            "pixivuser",
            "42",
            "pending",
            "2026-01-01",
            "https://www.pixiv.net/artworks/42",
        );
        insert_attempt(
            &conn,
            2,
            "pixiv:42:1",
            "pixiv",
            "42",
            "pending",
            "2026-01-01",
            "https://www.pixiv.net/artworks/42",
        );
        insert_attempt(
            &conn,
            3,
            "resolved",
            "pixiv",
            "43",
            "resolved",
            "2026-01-01",
            "https://www.pixiv.net/artworks/43",
        );

        assert_eq!(
            list_subscription_retry_targets(&conn, 1).expect("list targets"),
            [crate::subscriptions::types::SubscriptionRetryTarget {
                query_id: 1,
                site_id: "pixivuser".to_string(),
                post_id: "42".to_string(),
            }]
        );

        let attempts = list_subscription_download_attempts_page(&conn, 1, None, None, 1)
            .expect("first attempt page");
        assert_eq!(attempts.items.len(), 1);
        assert!(attempts.next_cursor.is_some());
        assert_eq!(attempts.failed_post_count, 1);
        assert_eq!(attempts.retryable_post_count, 1);
        let next_attempts =
            list_subscription_download_attempts_page(&conn, 1, None, attempts.next_cursor, 10)
                .expect("second attempt page");
        assert_eq!(next_attempts.items.len(), 2);
        assert!(next_attempts.next_cursor.is_none());

        let issues = list_subscription_issues_page(&conn, 1, None, None, 1).expect("issue page");
        assert_eq!(issues.total_count, 1);
        assert_eq!(issues.items.len(), 1);
        assert!(issues.next_cursor.is_none());

        resolve_subscription_download_attempt(&conn, 1, Some(1), "pixiv:42:0")
            .expect("resolve first member");
        let issue_status: String = conn
            .query_row(
                "SELECT status FROM subscription_issue WHERE issue_id = ?1",
                [issues.items[0].issue_id],
                |row| row.get(0),
            )
            .expect("read issue status");
        assert_eq!(issue_status, "open");

        resolve_subscription_download_attempt(&conn, 1, Some(1), "pixiv:42:1")
            .expect("resolve second member");
        resolve_subscription_download_attempt(&conn, 1, Some(1), "pixivuser:42:2")
            .expect("resolve final member");
        let issue_status: String = conn
            .query_row(
                "SELECT status FROM subscription_issue WHERE issue_id = ?1",
                [issues.items[0].issue_id],
                |row| row.get(0),
            )
            .expect("read resolved issue status");
        assert_eq!(issue_status, "resolved");
    }

    #[test]
    fn query_progress_keeps_accepted_work_from_interrupted_segments() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, uuid, date_added)
             VALUES (1, 'Test subscription', 'subscription-progress-test', '2026-01-01')",
            [],
        )
        .expect("insert subscription");
        conn.execute(
            "INSERT INTO subscription_query (
                 query_id, subscription_id, uuid, site_id, query_text,
                 last_check_time, files_found, posts_found
             ) VALUES (1, 1, 'query-progress-test', 'gelbooru', 'test', 'previous-check', 95, 95)",
            [],
        )
        .expect("insert query");

        add_query_progress(&conn, 1, None, 5, 5).expect("add interrupted progress");
        let interrupted = conn
            .query_row(
                "SELECT last_check_time, files_found, posts_found
                 FROM subscription_query WHERE query_id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("read interrupted progress");
        assert_eq!(interrupted, (Some("previous-check".to_string()), 100, 100));

        add_query_progress(&conn, 1, Some("successful-check"), 0, 0)
            .expect("record successful check");
        let last_check: String = conn
            .query_row(
                "SELECT last_check_time FROM subscription_query WHERE query_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("read successful check");
        assert_eq!(last_check, "successful-check");
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
             canonical_post_url, media_url, entity_id, status, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT(subscription_id, site_id, post_id, item_key)
         DO UPDATE SET page_num = excluded.page_num,
                       canonical_post_url = excluded.canonical_post_url,
                       media_url = excluded.media_url,
                       entity_id = excluded.entity_id,
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
            input.entity_id,
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
