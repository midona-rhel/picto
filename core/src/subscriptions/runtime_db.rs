use rusqlite::{params, Connection, OptionalExtension};

use super::types::{
    SubscriptionDownloadAttemptRecord, SubscriptionDownloadAttemptUpsert, SubscriptionIssueRecord,
    SubscriptionPostMemberRecord, SubscriptionPostMemberUpsert, SubscriptionQueryJob,
    SubscriptionQueryRunRecord, SubscriptionRunRecord,
};

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
        queued_at: row.get(9)?,
        started_at: row.get(10)?,
        finished_at: row.get(11)?,
        failure_kind: row.get(12)?,
        error_message: row.get(13)?,
    })
}

fn map_subscription_issue_row(row: &rusqlite::Row) -> rusqlite::Result<SubscriptionIssueRecord> {
    Ok(SubscriptionIssueRecord {
        issue_id: row.get(0)?,
        subscription_id: row.get(1)?,
        query_id: row.get(2)?,
        issue_kind: row.get(3)?,
        status: row.get(4)?,
        message: row.get(5)?,
        detail: row.get(6)?,
        first_seen_at: row.get(7)?,
        last_seen_at: row.get(8)?,
        resolved_at: row.get(9)?,
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
) -> rusqlite::Result<i64> {
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
        return Ok(job_id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_query_job (
             run_id, subscription_id, query_id, site_id, status, job_kind, requested_by, post_id, queued_at
         ) VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, ?7, ?8)",
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
    Ok(conn.last_insert_rowid())
}

pub fn list_queued_subscription_query_jobs(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryJob>> {
    let mut stmt = conn.prepare_cached(
        "SELECT job_id, run_id, subscription_id, query_id, site_id, status, job_kind,
                requested_by, post_id, queued_at, started_at, finished_at, failure_kind, error_message
         FROM subscription_query_job
         WHERE status = 'queued'
         ORDER BY queued_at ASC, job_id ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], map_subscription_query_job_row)?;
    rows.collect()
}

pub fn list_subscription_query_jobs_for_run(
    conn: &Connection,
    run_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryJob>> {
    let mut stmt = conn.prepare_cached(
        "SELECT job_id, run_id, subscription_id, query_id, site_id, status, job_kind,
                requested_by, post_id, queued_at, started_at, finished_at, failure_kind, error_message
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
                requested_by, post_id, queued_at, started_at, finished_at, failure_kind, error_message
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

    let queues_deleted = conn.execute("DELETE FROM ingest_queue WHERE query_id = ?1", [query_id])?;

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
    issue_kind: &str,
    message: &str,
    detail: Option<&str>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_issue (
             subscription_id, query_id, issue_kind, status, message, detail, first_seen_at, last_seen_at
         ) VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?6, ?6)
         ON CONFLICT(subscription_id, query_id, issue_kind, message)
         DO UPDATE SET status = 'open',
                       detail = excluded.detail,
                       last_seen_at = excluded.last_seen_at,
                       resolved_at = NULL",
        params![subscription_id, query_id, issue_kind, message, detail, now],
    )?;
    conn.query_row(
        "SELECT issue_id FROM subscription_issue
         WHERE subscription_id = ?1
           AND query_id IS ?2
           AND issue_kind = ?3
           AND message = ?4",
        params![subscription_id, query_id, issue_kind, message],
        |row| row.get(0),
    )
}

pub fn resolve_subscription_issues(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    issue_kind: &str,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
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
        "SELECT issue_id, subscription_id, query_id, issue_kind, status,
                message, detail, first_seen_at, last_seen_at, resolved_at
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
             subscription_id, site_id, post_id, collection_entity_id, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(subscription_id, site_id, post_id)
         DO UPDATE SET collection_entity_id = excluded.collection_entity_id,
                       updated_at = excluded.updated_at",
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
