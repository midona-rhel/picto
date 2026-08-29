//! Canonical subscription persistence over the schema-1 library coordinator.
//!
//! Source acquisition remains in `picto_core`, but all durable state changes
//! use the `picto_library` writer scheduler. No legacy Store is opened.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration};
use picto_library::database::WorkPriority;
use picto_library::{LibraryError, MutationReceipt};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::library_application::LibraryApplication;
use crate::subscriptions::{
    ClaimedQueryRun, CreatedRun, DomainSchedule, NormalizedPost, RecoveryCounts,
};

const RESOURCES: [&str; 2] = ["subscriptions", "tasks"];
const MAX_ATTEMPTS: i64 = 3;
const RETRY_BASE_SECONDS: i64 = 60;

fn resources() -> Vec<String> {
    RESOURCES.iter().map(|value| (*value).to_owned()).collect()
}

pub fn recover(application: &LibraryApplication, now: &str) -> Result<RecoveryCounts, String> {
    let result = application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CorrectnessRecovery,
            resources(),
            [],
            |transaction, _| {
                let query_runs = transaction.execute(
                    "UPDATE subscription_run_query
                     SET status = 'pending', available_at = ?1, started_at = NULL,
                         finished_at = NULL,
                         attempt_count = MAX(attempt_count - 1, 0),
                         failure_kind = NULL, error_message = NULL
                     WHERE status = 'running'",
                    [now],
                )?;
                let runs = transaction.execute(
                    "UPDATE subscription_run
                     SET status = 'pending', started_at = NULL, finished_at = NULL,
                         failure_kind = NULL, error_message = NULL
                     WHERE status = 'running'
                       AND EXISTS (
                           SELECT 1 FROM subscription_run_query query_run
                           WHERE query_run.run_id = subscription_run.run_id
                             AND query_run.status = 'running'
                       )",
                    [],
                )?;
                let counts = RecoveryCounts { runs, query_runs };
                Ok((runs != 0 || query_runs != 0).then_some(counts))
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(result.map(|(counts, _)| counts).unwrap_or_default())
}

pub fn schedule_due_runs(
    application: &LibraryApplication,
    now: &str,
) -> Result<Vec<CreatedRun>, String> {
    let result = application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let due = transaction
                    .prepare(
                        "SELECT subscription_id, schedule
                         FROM subscription
                         WHERE paused = 0 AND next_run_at IS NOT NULL AND next_run_at <= ?1
                         ORDER BY next_run_at, subscription_id",
                    )?
                    .query_map([now], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                if due.is_empty() {
                    return Ok(None);
                }
                let mut created = Vec::new();
                for (subscription_id, schedule) in due {
                    let run = crate::subscriptions::create_run_in(
                        transaction,
                        subscription_id,
                        "scheduled",
                        now,
                    )?;
                    let next = crate::subscriptions::next_schedule_at(&schedule, now)
                        .map_err(sql_error)?;
                    transaction.execute(
                        "UPDATE subscription SET next_run_at = ?1 WHERE subscription_id = ?2",
                        params![next, subscription_id],
                    )?;
                    if run.created {
                        created.push(run);
                    }
                }
                Ok(Some(created))
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(result.map(|(runs, _)| runs).unwrap_or_default())
}

pub fn claim_next_query(
    application: &LibraryApplication,
    schedule: &mut DomainSchedule,
    now: &str,
) -> Result<Option<ClaimedQueryRun>, String> {
    let now_ms = DateTime::parse_from_rfc3339(now)
        .map_err(|error| format!("invalid timestamp {now}: {error}"))?
        .timestamp_millis();
    let result = application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let candidates = transaction
                    .prepare(
                        "SELECT qr.run_query_id, qr.run_id, qr.query_id, r.subscription_id,
                                q.site_id, q.domain_key, q.query_kind, q.query_text, q.group_posts,
                                r.requested_by, s.initial_post_limit, s.periodic_post_limit,
                                q.initial_run_complete, COALESCE(qr.resume_cursor, q.resume_cursor),
                                qr.attempt_count
                         FROM subscription_run_query qr
                         JOIN subscription_run r ON r.run_id = qr.run_id
                         JOIN subscription_query q ON q.query_id = qr.query_id
                         JOIN subscription s ON s.subscription_id = r.subscription_id
                         WHERE qr.status = 'pending' AND qr.available_at <= ?1
                           AND r.status IN ('pending', 'running')
                           AND s.paused = 0
                           AND (q.paused = 0 OR r.requested_by = 'manual-query')
                           AND NOT EXISTS (
                               SELECT 1 FROM subscription_run_query active_rq
                               JOIN subscription_query active_q ON active_q.query_id = active_rq.query_id
                               WHERE active_rq.status = 'running' AND active_q.domain_key = q.domain_key
                           )
                           -- One exclusive execution per subscription: queries of
                           -- one subscription run serially, never side by side.
                           AND NOT EXISTS (
                               SELECT 1 FROM subscription_run_query sibling_rq
                               JOIN subscription_run sibling_run ON sibling_run.run_id = sibling_rq.run_id
                               WHERE sibling_rq.status = 'running'
                                 AND sibling_run.subscription_id = r.subscription_id
                           )
                         ORDER BY qr.available_at, qr.run_query_id",
                    )?
                    .query_map([now], |row| {
                        Ok(ClaimedQueryRun {
                            run_query_id: row.get(0)?,
                            run_id: row.get(1)?,
                            query_id: row.get(2)?,
                            subscription_id: row.get(3)?,
                            site_id: row.get(4)?,
                            domain_key: row.get(5)?,
                            query_kind: row.get(6)?,
                            query_text: row.get(7)?,
                            group_posts: row.get(8)?,
                            requested_by: row.get(9)?,
                            initial_post_limit: row.get(10)?,
                            periodic_post_limit: row.get(11)?,
                            run_post_limit: None,
                            initial_run_complete: row.get(12)?,
                            resume_cursor: row.get(13)?,
                            attempt_count: row.get(14)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let Some(mut candidate) = candidates
                    .into_iter()
                    .find(|candidate| schedule.allows(&candidate.domain_key, now_ms))
                else {
                    return Ok(None);
                };
                let accepted_posts: u32 = transaction
                    .query_row(
                        "SELECT COUNT(DISTINCT post.root_item_id)
                         FROM subscription_run_source_item linked
                         JOIN source_item item USING(source_item_id)
                         JOIN source_post post USING(source_post_id)
                         JOIN subscription_run_query run_query USING(run_query_id)
                         JOIN subscription_run run USING(run_id)
                         JOIN library_root root ON root.root_id = post.root_item_id
                         WHERE linked.run_query_id = ?1
                           AND item.state = 'ingested'
                           AND root.imported_at_ms >= unixepoch(run.created_at) * 1000",
                        [candidate.run_query_id],
                        |row| row.get::<_, i64>(0),
                    )?
                    .try_into()
                    .unwrap_or(u32::MAX);
                let configured_limit = candidate.source_post_batch_size();
                let remaining = configured_limit.saturating_sub(accepted_posts);
                if remaining == 0 {
                    transaction.execute(
                        "UPDATE subscription_run_query
                         SET status = 'succeeded', finished_at = ?1,
                             failure_kind = NULL, error_message = NULL
                         WHERE run_query_id = ?2 AND status = 'pending'",
                        params![now, candidate.run_query_id],
                    )?;
                    settle_run(transaction, candidate.run_id, now)?;
                    return Ok(Some(None));
                }
                // Downloads settle ahead of canonical ingestion. A post that is
                // downloaded but not yet ingested is invisible to the accepted
                // count above, so an unreserved claim would hand out a window
                // that overruns the added-post budget once ingestion catches
                // up. Reserve those in-flight posts; when they fill the whole
                // remaining budget, wait for ingestion instead of claiming or
                // prematurely settling (a skip releases its reservation on the
                // next tick).
                let in_flight_posts: u32 = transaction
                    .query_row(
                        "SELECT COUNT(DISTINCT item.source_post_id)
                         FROM subscription_run_source_item linked
                         JOIN source_item item USING(source_item_id)
                         JOIN source_post post ON post.source_post_id = item.source_post_id
                         WHERE linked.run_query_id = ?1
                           AND item.state IN ('pending', 'downloaded')
                           AND post.root_item_id IS NULL",
                        [candidate.run_query_id],
                        |row| row.get::<_, i64>(0),
                    )?
                    .try_into()
                    .unwrap_or(u32::MAX);
                let claimable = remaining.saturating_sub(in_flight_posts);
                if claimable == 0 {
                    return Ok(None);
                }
                candidate.run_post_limit = Some(claimable);
                if transaction.execute(
                    "UPDATE subscription_run_query
                     SET status = 'running', started_at = ?1, attempt_count = attempt_count + 1,
                         failure_kind = NULL, error_message = NULL
                     WHERE run_query_id = ?2 AND status = 'pending' AND available_at <= ?1",
                    params![now, candidate.run_query_id],
                )? != 1 {
                    return Ok(None);
                }
                transaction.execute(
                    "UPDATE subscription_run
                     SET status = 'running', started_at = COALESCE(started_at, ?1)
                     WHERE run_id = ?2 AND status = 'pending'",
                    params![now, candidate.run_id],
                )?;
                Ok(Some(Some(ClaimedQueryRun {
                    attempt_count: candidate.attempt_count + 1,
                    ..candidate
                })))
            },
        )
        .map_err(|error| error.to_string())?;
    let claim = result.and_then(|(claim, _)| claim);
    if let Some(claim) = &claim {
        schedule.mark_started(claim.domain_key.clone(), now_ms);
    }
    Ok(claim)
}

pub fn query_is_running(
    application: &LibraryApplication,
    run_query_id: i64,
) -> Result<bool, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            connection
                .query_row(
                    "SELECT status = 'running' FROM subscription_run_query WHERE run_query_id = ?1",
                    [run_query_id],
                    |row| row.get(0),
                )
                .optional()
                .map(|value| value.unwrap_or(false))
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())
}

pub fn query_ingest_settlement(
    application: &LibraryApplication,
    run_query_id: i64,
) -> Result<Result<bool, String>, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            let failed = connection
                .query_row(
                    "SELECT job.last_error
                     FROM subscription_run_source_item linked
                     JOIN ingest_job job USING(source_item_id)
                     WHERE linked.run_query_id = ?1 AND job.status = 'failed'
                     ORDER BY job.ingest_job_id LIMIT 1",
                    [run_query_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?;
            if let Some(message) = failed {
                return Ok(Err(
                    message.unwrap_or_else(|| "Canonical ingest failed".into())
                ));
            }
            let pending = connection.query_row(
                "SELECT EXISTS(
                     SELECT 1
                     FROM subscription_run_source_item linked
                     JOIN source_item item USING(source_item_id)
                     WHERE linked.run_query_id = ?1 AND item.state = 'downloaded'
                 )",
                [run_query_id],
                |row| row.get::<_, bool>(0),
            )?;
            Ok(Ok(!pending))
        })
        .map_err(|error| error.to_string())
}

pub fn set_inbox_wait_state(
    application: &LibraryApplication,
    inbox_full: bool,
    limit: u64,
) -> Result<Option<MutationReceipt>, String> {
    let message = format!("Stopped because Inbox reached its limit of {limit} items.");
    application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let changed = if inbox_full {
                    transaction.execute(
                        "UPDATE subscription_run_query
                         SET failure_kind = 'inbox_full', error_message = ?1
                         WHERE status = 'pending' AND COALESCE(failure_kind, '') != 'inbox_full'",
                        [&message],
                    )? + transaction.execute(
                        "UPDATE subscription_run
                         SET failure_kind = 'inbox_full', error_message = ?1
                         WHERE status = 'pending' AND COALESCE(failure_kind, '') != 'inbox_full'",
                        [&message],
                    )?
                } else {
                    transaction.execute(
                        "UPDATE subscription_run_query SET failure_kind = NULL, error_message = NULL
                         WHERE status = 'pending' AND failure_kind = 'inbox_full'",
                        [],
                    )? + transaction.execute(
                        "UPDATE subscription_run SET failure_kind = NULL, error_message = NULL
                         WHERE status = 'pending' AND failure_kind = 'inbox_full'",
                        [],
                    )?
                };
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|value| value.map(|(_, receipt)| receipt))
        .map_err(|error| error.to_string())
}

pub fn record_post(
    application: &LibraryApplication,
    run_query_id: i64,
    post: &NormalizedPost,
    now: &str,
) -> Result<BTreeMap<String, i64>, String> {
    validate_post(post)?;
    application
        .library()
        .auxiliary_write(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| record_post_in(transaction, run_query_id, post, now),
        )
        .map(|(items, _)| items)
        .map_err(|error| error.to_string())
}

pub fn source_item_id(
    application: &LibraryApplication,
    site_id: &str,
    post_key: &str,
    item_key: &str,
) -> Result<i64, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            connection
                .query_row(
                    "SELECT item.source_item_id
                 FROM source_item item
                 JOIN source_post post ON post.source_post_id = item.source_post_id
                 WHERE post.site_id = ?1 AND post.post_key = ?2 AND item.item_key = ?3",
                    params![site_id, post_key, item_key],
                    |row| row.get(0),
                )
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())
}

pub fn mark_source_items_downloaded(
    application: &LibraryApplication,
    source_item_ids: &[i64],
    now: &str,
) -> Result<(), String> {
    if source_item_ids.is_empty() {
        return Ok(());
    }
    application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let mut changed = 0;
                for source_item_id in source_item_ids {
                    changed += transaction.execute(
                        "UPDATE source_item SET state = 'downloaded', last_error = NULL, updated_at = ?1
                         WHERE source_item_id = ?2 AND state = 'pending'",
                        params![now, source_item_id],
                    )?;
                    transaction.execute(
                        "UPDATE subscription_issue
                         SET status = 'resolved', last_seen_at = ?1, resolved_at = ?1
                         WHERE issue_key = ?2 AND status IN ('open', 'acknowledged')",
                        params![now, format!("source_item:{source_item_id}:download")],
                    )?;
                }
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn mark_source_item_failed(
    application: &LibraryApplication,
    subscription_id: i64,
    query_id: i64,
    source_item_id: i64,
    error: &str,
    now: &str,
) -> Result<(), String> {
    application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE source_item
                     SET state = 'failed', last_error = ?1, updated_at = ?2
                     WHERE source_item_id = ?3
                       AND media_item_id IS NULL
                       AND state != 'deleted'
                       AND (state != 'failed' OR last_error IS NOT ?1)",
                    params![error, now, source_item_id],
                )?;
                if changed == 0 {
                    return Ok(None);
                }
                transaction.execute(
                    "INSERT INTO subscription_issue (
                         issue_key, subscription_id, query_id, issue_kind, message,
                         status, first_seen_at, last_seen_at
                     ) VALUES (?1, ?2, ?3, 'download_item', ?4, 'open', ?5, ?5)
                     ON CONFLICT(issue_key) DO UPDATE SET
                         message = excluded.message, status = 'open',
                         last_seen_at = excluded.last_seen_at, resolved_at = NULL",
                    params![
                        format!("source_item:{source_item_id}:download"),
                        subscription_id,
                        query_id,
                        error,
                        now,
                    ],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn record_post_in(
    transaction: &Transaction<'_>,
    run_query_id: i64,
    post: &NormalizedPost,
    now: &str,
) -> picto_library::Result<BTreeMap<String, i64>> {
    let (run_id, query_id, subscription_id, site_id, status): (i64, i64, i64, String, String) =
        transaction.query_row(
            "SELECT qr.run_id, qr.query_id, r.subscription_id, q.site_id, qr.status
             FROM subscription_run_query qr
             JOIN subscription_run r ON r.run_id = qr.run_id
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
                ))
            },
        )?;
    if status != "running" || site_id != post.site_id {
        return Err(LibraryError::InvalidState(
            "source post does not belong to the running query".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO source_post (
             site_id, post_key, canonical_url, creator_name, title, description,
             captured_at, metadata_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(site_id, post_key) DO UPDATE SET
             canonical_url = COALESCE(excluded.canonical_url, source_post.canonical_url),
             creator_name = COALESCE(excluded.creator_name, source_post.creator_name),
             title = COALESCE(excluded.title, source_post.title),
             description = COALESCE(excluded.description, source_post.description),
             captured_at = COALESCE(excluded.captured_at, source_post.captured_at),
             metadata_json = COALESCE(excluded.metadata_json, source_post.metadata_json),
             updated_at = excluded.updated_at",
        params![
            post.site_id,
            post.post_key,
            post.canonical_url,
            post.creator_name,
            post.title,
            post.description,
            post.captured_at,
            post.metadata_json,
            now,
        ],
    )?;
    let source_post_id = transaction.query_row(
        "SELECT source_post_id FROM source_post WHERE site_id = ?1 AND post_key = ?2",
        params![post.site_id, post.post_key],
        |row| row.get::<_, i64>(0),
    )?;
    transaction.execute(
        "INSERT INTO subscription_source_post
             (subscription_id, query_id, source_post_id, last_seen_run_id)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(subscription_id, query_id, source_post_id)
         DO UPDATE SET last_seen_run_id = excluded.last_seen_run_id",
        params![subscription_id, query_id, source_post_id, run_id],
    )?;
    let mut ids = BTreeMap::new();
    for item in &post.items {
        transaction.execute(
            "INSERT INTO source_item (
                 source_post_id, item_key, position, media_url, canonical_url,
                 state, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?6)
             ON CONFLICT(source_post_id, item_key) DO UPDATE SET
                 position = excluded.position,
                 media_url = COALESCE(excluded.media_url, source_item.media_url),
                 canonical_url = COALESCE(excluded.canonical_url, source_item.canonical_url),
                 updated_at = excluded.updated_at",
            params![
                source_post_id,
                item.item_key,
                item.position,
                item.media_url,
                item.canonical_url,
                now,
            ],
        )?;
        let source_item_id = transaction.query_row(
            "SELECT source_item_id FROM source_item WHERE source_post_id = ?1 AND item_key = ?2",
            params![source_post_id, item.item_key],
            |row| row.get::<_, i64>(0),
        )?;
        transaction.execute(
            "INSERT INTO subscription_run_source_item(run_query_id, source_item_id)
             VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            params![run_query_id, source_item_id],
        )?;
        ids.insert(item.item_key.clone(), source_item_id);
    }
    Ok(ids)
}

pub fn complete_query(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    resume_cursor: Option<&str>,
    now: &str,
) -> Result<MutationReceipt, String> {
    write_transition(application, |transaction| {
        let added_posts: u32 = transaction
            .query_row(
                "SELECT COUNT(DISTINCT post.root_item_id)
                 FROM subscription_run_source_item linked
                 JOIN source_item item USING(source_item_id)
                 JOIN source_post post USING(source_post_id)
                 JOIN subscription_run_query run_query USING(run_query_id)
                 JOIN subscription_run run USING(run_id)
                 JOIN library_root root ON root.root_id = post.root_item_id
                 WHERE linked.run_query_id = ?1
                   AND item.state = 'ingested'
                   AND root.imported_at_ms >= unixepoch(run.created_at) * 1000",
                [query.run_query_id],
                |row| row.get::<_, i64>(0),
            )?
            .try_into()
            .unwrap_or(u32::MAX);
        let continue_run = resume_cursor != Some("") && added_posts < query.configured_post_limit();
        if continue_run {
            transaction.execute(
                "UPDATE subscription_run_query
                 SET status = 'pending', available_at = ?1, started_at = NULL,
                     finished_at = NULL, resume_cursor = COALESCE(?2, resume_cursor),
                     failure_kind = NULL, error_message = NULL
                 WHERE run_query_id = ?3 AND status = 'running'",
                params![now, resume_cursor, query.run_query_id],
            )?;
            transaction.execute(
                "UPDATE subscription_query
                 SET resume_cursor = COALESCE(?1, resume_cursor),
                     last_failure_at = NULL, last_failure_kind = NULL,
                     last_failure_message = NULL
                 WHERE query_id = ?2",
                params![resume_cursor, query.query_id],
            )?;
            transaction.execute(
                "UPDATE subscription_run SET status = 'pending'
                 WHERE run_id = ?1 AND status = 'running'",
                [query.run_id],
            )?;
            return Ok(());
        }
        transaction.execute(
            "UPDATE subscription_run_query
             SET status = 'succeeded', finished_at = ?1,
                 resume_cursor = COALESCE(?2, resume_cursor),
                 failure_kind = NULL, error_message = NULL
             WHERE run_query_id = ?3 AND status = 'running'",
            params![now, resume_cursor, query.run_query_id],
        )?;
        transaction.execute(
            "UPDATE subscription_query
             SET last_success_at = ?1, initial_run_complete = 1,
                 resume_cursor = COALESCE(?2, resume_cursor),
                 last_failure_at = NULL, last_failure_kind = NULL,
                 last_failure_message = NULL
             WHERE query_id = ?3",
            params![now, resume_cursor, query.query_id],
        )?;
        transaction.execute(
            "UPDATE subscription_issue SET status = 'resolved', last_seen_at = ?1, resolved_at = ?1
             WHERE query_id = ?2
               AND status IN ('open', 'acknowledged')
               AND issue_key LIKE 'query:%'",
            params![now, query.query_id],
        )?;
        settle_run(transaction, query.run_id, now)?;
        Ok(())
    })
}

pub fn interrupt_query(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    now: &str,
) -> Result<MutationReceipt, String> {
    write_transition(application, |transaction| {
        transaction.execute(
            "UPDATE subscription_run_query
             SET status = 'pending', available_at = ?1, started_at = NULL, finished_at = NULL,
                 attempt_count = MAX(attempt_count - 1, 0), failure_kind = NULL, error_message = NULL
             WHERE run_query_id = ?2 AND status = 'running'",
            params![now, query.run_query_id],
        )?;
        transaction.execute(
            "UPDATE subscription_run
             SET status = 'pending', started_at = NULL, finished_at = NULL,
                 failure_kind = NULL, error_message = NULL
             WHERE run_id = ?1 AND status = 'running'",
            [query.run_id],
        )?;
        Ok(())
    })
}

pub fn fail_query(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    kind: &str,
    message: &str,
    retryable: bool,
    now: &str,
) -> Result<MutationReceipt, String> {
    // A site that states when its limit resets gets parked until then
    // instead of burning generic backoff retries against a closed window.
    let stated_reset = (kind == "rate_limited")
        .then(|| rate_limit_reset_at(message, now))
        .flatten();
    let retry_at = if retryable {
        match stated_reset {
            Some(reset) => Some(reset),
            None => (query.attempt_count < MAX_ATTEMPTS)
                .then(|| next_retry_at(now, query.attempt_count))
                .transpose()?,
        }
    } else {
        None
    };
    write_transition(application, |transaction| {
        let (status, finished): (&str, Option<&str>) = if retry_at.is_some() {
            ("pending", None)
        } else {
            ("failed", Some(now))
        };
        transaction.execute(
            "UPDATE subscription_run_query
             SET status = ?1, available_at = COALESCE(?2, available_at), finished_at = ?3,
                 failure_kind = ?4, error_message = ?5
             WHERE run_query_id = ?6 AND status = 'running'",
            params![
                status,
                retry_at,
                finished,
                kind,
                message,
                query.run_query_id
            ],
        )?;
        transaction.execute(
            "UPDATE subscription_query
             SET last_failure_at = ?1, last_failure_kind = ?2, last_failure_message = ?3
             WHERE query_id = ?4",
            params![now, kind, message, query.query_id],
        )?;
        let issue_key = format!("query:{}:{kind}", query.query_id);
        transaction.execute(
            "INSERT INTO subscription_issue (
                 issue_key, subscription_id, query_id, issue_kind, message,
                 status, first_seen_at, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?6)
             ON CONFLICT(issue_key) DO UPDATE SET
                 message = excluded.message, status = 'open',
                 last_seen_at = excluded.last_seen_at, resolved_at = NULL",
            params![
                issue_key,
                query.subscription_id,
                query.query_id,
                kind,
                message,
                now
            ],
        )?;
        if retry_at.is_some() {
            transaction.execute(
                "UPDATE subscription_run SET status = 'pending', finished_at = NULL
                 WHERE run_id = ?1 AND status = 'running'",
                [query.run_id],
            )?;
        } else {
            settle_run(transaction, query.run_id, now)?;
        }
        Ok(())
    })
}

pub fn mark_credential_success(
    application: &LibraryApplication,
    site_id: &str,
    now: &str,
) -> Result<(), String> {
    credential_health(application, site_id, "healthy", now, None)
}

pub fn mark_credential_failure(
    application: &LibraryApplication,
    site_id: &str,
    now: &str,
    message: &str,
) -> Result<(), String> {
    credential_health(application, site_id, "invalid", now, Some(message))
}

pub fn settle_ingest_runs(application: &LibraryApplication, now: &str) -> Result<(), String> {
    application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let failed_queries = transaction.execute(
                    "UPDATE subscription_run_query
                     SET status = 'failed', failure_kind = 'ingest',
                         error_message = COALESCE((
                             SELECT job.last_error
                             FROM subscription_run_source_item linked
                             JOIN ingest_job job USING(source_item_id)
                             WHERE linked.run_query_id = subscription_run_query.run_query_id
                               AND job.status = 'failed'
                             ORDER BY job.ingest_job_id LIMIT 1
                         ), 'Canonical ingest failed')
                     WHERE status = 'succeeded'
                       AND EXISTS (
                           SELECT 1
                           FROM subscription_run_source_item linked
                           JOIN ingest_job job USING(source_item_id)
                           WHERE linked.run_query_id = subscription_run_query.run_query_id
                             AND job.status = 'failed'
                       )",
                    [],
                )?;
                let run_ids = {
                    let mut statement = transaction.prepare(
                        "SELECT run_id FROM subscription_run
                         WHERE status = 'running' ORDER BY run_id",
                    )?;
                    let run_ids = statement
                        .query_map([], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    run_ids
                };
                let mut settled = 0;
                for run_id in run_ids {
                    settled += settle_run(transaction, run_id, now)? as usize;
                }
                Ok((failed_queries != 0 || settled != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn acknowledge_subscription_issues(
    application: &LibraryApplication,
    subscription_id: i64,
) -> Result<Option<MutationReceipt>, String> {
    application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::ForegroundMutation,
            ["subscriptions".to_owned()],
            [],
            |transaction, _| {
                let exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM subscription WHERE subscription_id = ?1)",
                    [subscription_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if !exists {
                    return Err(LibraryError::NotFound(format!(
                        "subscription {subscription_id}"
                    )));
                }
                let changed = transaction.execute(
                    "UPDATE subscription_issue
                     SET status = 'acknowledged'
                     WHERE subscription_id = ?1 AND status = 'open'",
                    [subscription_id],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|published| published.map(|(_, receipt)| receipt))
        .map_err(|error| error.to_string())
}

fn credential_health(
    application: &LibraryApplication,
    site_id: &str,
    status: &str,
    now: &str,
    error: Option<&str>,
) -> Result<(), String> {
    application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::Maintenance,
            ["auth".to_owned()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE credential_health SET status = ?1, checked_at = ?2, last_error = ?3
                     WHERE site_id = ?4
                       AND (status != ?1 OR checked_at IS NOT ?2 OR last_error IS NOT ?3)",
                    params![status, now, error, site_id],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn write_transition(
    application: &LibraryApplication,
    operation: impl FnOnce(&Transaction<'_>) -> picto_library::Result<()>,
) -> Result<MutationReceipt, String> {
    application
        .library()
        .auxiliary_write(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| operation(transaction),
        )
        .map(|(_, receipt)| receipt)
        .map_err(|error| error.to_string())
}

fn settle_run(
    transaction: &Transaction<'_>,
    run_id: i64,
    now: &str,
) -> picto_library::Result<bool> {
    let (pending, running, failed, cancelled): (i64, i64, i64, i64) = transaction.query_row(
        "SELECT
             COALESCE(SUM(status = 'pending'), 0),
             COALESCE(SUM(status = 'running'), 0),
             COALESCE(SUM(status = 'failed'), 0),
             COALESCE(SUM(status = 'cancelled'), 0)
         FROM subscription_run_query WHERE run_id = ?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if pending != 0 || running != 0 {
        return Ok(false);
    }
    let downloaded = transaction.query_row(
        "SELECT COUNT(*)
         FROM subscription_run_query query_run
         JOIN subscription_run_source_item linked USING(run_query_id)
         JOIN source_item item USING(source_item_id)
         WHERE query_run.run_id = ?1 AND item.state = 'downloaded'",
        [run_id],
        |row| row.get::<_, i64>(0),
    )?;
    if downloaded != 0 && failed == 0 && cancelled == 0 {
        return Ok(false);
    }
    let status = if failed != 0 {
        "failed"
    } else if cancelled != 0 {
        "cancelled"
    } else {
        "succeeded"
    };
    let changed = transaction.execute(
        "UPDATE subscription_run SET status = ?1, finished_at = ?2
         WHERE run_id = ?3 AND status IN ('pending', 'running')",
        params![status, now, run_id],
    )?;
    Ok(changed != 0)
}

fn validate_post(post: &NormalizedPost) -> Result<(), String> {
    if post.site_id.trim().is_empty() || post.post_key.trim().is_empty() {
        return Err("source post identity is required".into());
    }
    let unique = post
        .items
        .iter()
        .map(|item| item.item_key.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if unique.len() != post.items.len()
        || post
            .items
            .iter()
            .any(|item| item.item_key.trim().is_empty() || item.position < 0)
    {
        return Err("source post contains invalid or duplicate items".into());
    }
    Ok(())
}

/// Parse a provider-stated local reset time ("Rate limit will reset at
/// 20:04:22") into the next matching instant, with a safety margin.
fn rate_limit_reset_at(message: &str, now: &str) -> Option<String> {
    static RESET: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let captures = RESET
        .get_or_init(|| {
            regex::Regex::new(r"reset at (\d{1,2}):(\d{2}):(\d{2})").expect("valid reset regex")
        })
        .captures(message)?;
    let time = chrono::NaiveTime::from_hms_opt(
        captures[1].parse().ok()?,
        captures[2].parse().ok()?,
        captures[3].parse().ok()?,
    )?;
    let now_local = DateTime::parse_from_rfc3339(now)
        .ok()?
        .with_timezone(&chrono::Local);
    let mut candidate = now_local.date_naive().and_time(time);
    if candidate <= now_local.naive_local() {
        candidate += Duration::days(1);
    }
    let candidate = candidate
        .and_local_timezone(chrono::Local)
        .earliest()?
        + Duration::minutes(2);
    Some(candidate.with_timezone(&chrono::Utc).to_rfc3339())
}

fn next_retry_at(now: &str, attempt_count: i64) -> Result<String, String> {
    let timestamp = DateTime::parse_from_rfc3339(now)
        .map_err(|error| format!("invalid retry timestamp {now}: {error}"))?;
    let exponent = attempt_count.saturating_sub(1).clamp(0, 3) as u32;
    Ok((timestamp + Duration::seconds(RETRY_BASE_SECONDS * 2_i64.pow(exponent))).to_rfc3339())
}

fn sql_error(error: String) -> LibraryError {
    LibraryError::InvalidInput(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscription_catalog::{NewSubscription, NewSubscriptionQuery};
    use crate::subscriptions::{NormalizedItem, NormalizedPost};

    #[test]
    fn posts_per_run_is_independent_for_each_subscription_query() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Two sources".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![
                NewSubscriptionQuery {
                    site_id: "twitter".into(),
                    query_text: "example".into(),
                    display_name: None,
                    notes: None,
                    group_posts: true,
                },
                NewSubscriptionQuery {
                    site_id: "e621".into(),
                    query_text: "example".into(),
                    display_name: None,
                    notes: None,
                    group_posts: true,
                },
            ],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let mut schedule = DomainSchedule::new();
        let first = claim_next_query(&application, &mut schedule, "2026-08-29T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_eq!(first.source_post_batch_size(), 1);

        let ids = record_post(
            &application,
            first.run_query_id,
            &NormalizedPost {
                site_id: first.site_id.clone(),
                post_key: "post-1".into(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![NormalizedItem {
                    item_key: "media-1".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        mark_source_items_downloaded(&application, &[ids["media-1"]], "2026-08-29T00:00:03Z")
            .unwrap();
        application
            .library()
            .auxiliary_write(
                WorkPriority::CanonicalIngest,
                ["subscriptions".to_owned()],
                [],
                |transaction, _| {
                    transaction.execute(
                        "INSERT INTO library_item(local_id, stable_key, item_kind)
                         VALUES (5000, 'run-created-root', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, content_hash, file_path, mime, size_bytes)
                         VALUES (5001, 'run-created-hash', '/tmp/run-created', 'image/png', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_item(media_id, media_name, file_id)
                         VALUES (5000, 'run-created', 5001)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root
                             (root_id, name, cover_media_id, imported_at_ms, modified_at_ms,
                              media_count, total_size_bytes)
                         VALUES (5000, 'run-created', 5000, 1787976003000, 1787976003000, 1, 1)",
                        [],
                    )?;
                    transaction.execute(
                        "UPDATE source_item SET state = 'ingested' WHERE source_item_id = ?1",
                        [ids["media-1"]],
                    )?;
                    transaction.execute(
                        "UPDATE source_post SET root_item_id = 5000
                         WHERE source_post_id = (
                             SELECT source_post_id FROM source_item WHERE source_item_id = ?1
                         )",
                        [ids["media-1"]],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        complete_query(&application, &first, None, "2026-08-29T00:00:04Z").unwrap();

        let mut next_schedule = DomainSchedule::new();
        let second = claim_next_query(&application, &mut next_schedule, "2026-08-29T00:00:05Z")
            .unwrap()
            .expect("the second query retains its own one-post budget");
        assert_ne!(second.query_id, first.query_id);
        let statuses = application
            .library()
            .auxiliary_read(WorkPriority::VisibleRead, |connection| {
                let mut statement = connection
                    .prepare("SELECT status FROM subscription_run_query ORDER BY run_query_id")?;
                let statuses = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(statuses)
            })
            .unwrap();
        assert_eq!(statuses, ["succeeded", "running"]);
    }

    #[test]
    fn one_subscription_executes_its_queries_serially() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Serial".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![
                NewSubscriptionQuery {
                    site_id: "twitter".into(),
                    query_text: "example".into(),
                    display_name: None,
                    notes: None,
                    group_posts: true,
                },
                NewSubscriptionQuery {
                    site_id: "e621".into(),
                    query_text: "example".into(),
                    display_name: None,
                    notes: None,
                    group_posts: true,
                },
            ],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let mut schedule = DomainSchedule::new();
        let first = claim_next_query(&application, &mut schedule, "2026-08-29T00:00:01Z")
            .unwrap()
            .unwrap();

        // While the first query is running, its sibling on a different domain
        // must not be claimable — one exclusive execution per subscription.
        let mut sibling_schedule = DomainSchedule::new();
        assert!(claim_next_query(&application, &mut sibling_schedule, "2026-08-29T00:00:02Z")
            .unwrap()
            .is_none());

        complete_query(&application, &first, None, "2026-08-29T00:00:03Z").unwrap();
        let second = claim_next_query(&application, &mut sibling_schedule, "2026-08-29T00:00:04Z")
            .unwrap()
            .expect("the sibling becomes claimable once the first settles");
        assert_ne!(second.query_id, first.query_id);
    }

    #[test]
    fn downloaded_but_not_ingested_posts_reserve_the_added_post_budget() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Reservation".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(2),
            periodic_post_limit: Some(2),
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
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let mut schedule = DomainSchedule::new();
        let first = claim_next_query(&application, &mut schedule, "2026-08-29T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_eq!(first.source_post_batch_size(), 2);

        // The window downloads two posts, but neither has been canonically
        // ingested yet: no roots exist, items sit in `downloaded`.
        for post in ["post-1", "post-2"] {
            let ids = record_post(
                &application,
                first.run_query_id,
                &NormalizedPost {
                    site_id: first.site_id.clone(),
                    post_key: post.into(),
                    canonical_url: None,
                    creator_name: None,
                    title: None,
                    description: None,
                    captured_at: None,
                    metadata_json: None,
                    items: vec![NormalizedItem {
                        item_key: format!("{post}:media"),
                        position: 0,
                        media_url: None,
                        canonical_url: None,
                    }],
                },
                "2026-08-29T00:00:02Z",
            )
            .unwrap();
            mark_source_items_downloaded(
                &application,
                &[ids[&format!("{post}:media")]],
                "2026-08-29T00:00:03Z",
            )
            .unwrap();
        }
        complete_query(&application, &first, None, "2026-08-29T00:00:04Z").unwrap();

        // Both in-flight posts cover the whole budget: nothing is claimable
        // and the query must NOT settle as succeeded while ingestion is
        // pending — the claim waits.
        let mut waiting_schedule = DomainSchedule::new();
        assert!(claim_next_query(&application, &mut waiting_schedule, "2026-08-29T00:00:05Z")
            .unwrap()
            .is_none());
        let status: String = application
            .library()
            .auxiliary_read(WorkPriority::VisibleRead, |connection| {
                connection
                    .query_row(
                        "SELECT status FROM subscription_run_query WHERE run_query_id = ?1",
                        [first.run_query_id],
                        |row| row.get(0),
                    )
                    .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(status, "pending", "reserved budget must not settle the query");

        // One post skips (its item resolves without a root): the reservation
        // is released and exactly one slot becomes claimable again.
        application
            .library()
            .auxiliary_write(
                WorkPriority::CanonicalIngest,
                ["subscriptions".to_owned()],
                [],
                |transaction, _| {
                    transaction.execute(
                        "UPDATE source_item SET state = 'ingested'
                         WHERE item_key = 'post-1:media'",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        let mut retry_schedule = DomainSchedule::new();
        let second = claim_next_query(&application, &mut retry_schedule, "2026-08-29T00:00:06Z")
            .unwrap()
            .expect("a released reservation makes budget claimable again");
        assert_eq!(
            second.source_post_batch_size(),
            1,
            "the still-in-flight post keeps its slot reserved"
        );
    }
}
