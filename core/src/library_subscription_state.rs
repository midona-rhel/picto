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
use picto_sources::{SkipReason, SourcePostOutcome};

const RESOURCES: [&str; 2] = ["subscriptions", "tasks"];
const MAX_ATTEMPTS: i64 = 3;
const RETRY_BASE_SECONDS: i64 = 60;

fn resources() -> Vec<String> {
    RESOURCES.iter().map(|value| (*value).to_owned()).collect()
}

pub fn recover(application: &LibraryApplication, now: &str) -> Result<RecoveryCounts, String> {
    match std::fs::remove_dir_all(application.root().join("source-runners/native")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("native source recovery cleanup failed: {error}")),
    }
    let result = application
        .library()
        .auxiliary_write_if_changed(
            WorkPriority::CorrectnessRecovery,
            resources(),
            [],
            |transaction, _| {
                transaction.execute(
                    "UPDATE source_post_attempt
                     SET state = 'added', terminal_reason = NULL, settled_at = ?1
                     WHERE state NOT IN ('added', 'skipped', 'failed', 'cancelled')
                       AND EXISTS (
                           SELECT 1 FROM source_attempt_root result
                           WHERE result.attempt_id = source_post_attempt.attempt_id
                             AND result.root_id IS NOT NULL
                       )",
                    [now],
                )?;
                transaction.execute(
                    "UPDATE source_post_attempt
                     SET state = 'skipped', terminal_reason = 'exact_duplicate', settled_at = ?1
                     WHERE state NOT IN ('added', 'skipped', 'failed', 'cancelled')
                       AND EXISTS (
                           SELECT 1
                           FROM source_post post
                           JOIN library_root root ON root.root_id = post.root_item_id
                           WHERE post.source_post_id = source_post_attempt.source_post_id
                             AND NOT EXISTS (
                                 SELECT 1 FROM source_attempt_root result
                                 WHERE result.attempt_id = source_post_attempt.attempt_id
                                   AND result.root_id IS NOT NULL
                             )
                       )",
                    [now],
                )?;
                transaction.execute(
                    "UPDATE source_file_attempt
                     SET state = CASE
                             WHEN EXISTS (
                                 SELECT 1 FROM source_post_attempt post_attempt
                                 WHERE post_attempt.attempt_id = source_file_attempt.attempt_id
                                   AND post_attempt.state = 'added'
                             ) THEN 'retained'
                             ELSE 'duplicate'
                         END,
                         staged_path = NULL
                     WHERE state = 'staged'
                       AND EXISTS (
                           SELECT 1 FROM source_post_attempt post_attempt
                           WHERE post_attempt.attempt_id = source_file_attempt.attempt_id
                             AND post_attempt.state IN ('added', 'skipped')
                       )",
                    [],
                )?;
                // No canonical commit exists for these attempts. Replay the
                // same provider boundary from its last committed cursor.
                transaction.execute(
                    "DELETE FROM source_post_attempt
                     WHERE state NOT IN ('added', 'skipped', 'failed', 'cancelled')",
                    [],
                )?;
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
                     WHERE status = 'running'",
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
                           -- Gallery downloads are globally single-flight. E-Hentai and
                           -- ExHentai use different domain keys but share one gallery pipeline.
                           AND (
                               q.site_id <> 'ehentai'
                               OR NOT EXISTS (
                                   SELECT 1 FROM subscription_run_query gallery_rq
                                   JOIN subscription_query gallery_q
                                     ON gallery_q.query_id = gallery_rq.query_id
                                   WHERE gallery_rq.status = 'running'
                                     AND gallery_q.site_id = 'ehentai'
                               )
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
                        "SELECT COUNT(*) FROM source_post_attempt
                         WHERE run_query_id = ?1 AND state = 'added'",
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
                        "SELECT COUNT(*) FROM source_post_attempt
                         WHERE run_query_id = ?1
                           AND state NOT IN ('added', 'skipped', 'failed', 'cancelled')",
                        [candidate.run_query_id],
                        |row| row.get::<_, i64>(0),
                    )?
                    .try_into()
                    .unwrap_or(u32::MAX);
                if in_flight_posts != 0 {
                    return Ok(None);
                }
                candidate.run_post_limit = Some(remaining);
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

/// Resolve the current post from canonical state after its ingest work has
/// settled. Source runners use this result to advance their cursor and added
/// budget; they never infer success from downloads alone.
pub fn settled_post_outcome(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    post_key: &str,
) -> Result<SourcePostOutcome, String> {
    application
        .library()
        .auxiliary_write(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let row = transaction
                    .query_row(
                        "SELECT attempt.attempt_id, post.source_post_id, post.root_item_id,
                            EXISTS(
                                SELECT 1 FROM source_attempt_root result
                                WHERE result.attempt_id = attempt.attempt_id
                                  AND result.root_id IS NOT NULL
                            ),
                            COUNT(item.source_item_id),
                            COALESCE(SUM(item.state = 'pending'), 0),
                            COALESCE(SUM(item.state = 'downloaded'), 0),
                            COALESCE(SUM(item.state = 'ingested'), 0),
                            COALESCE(SUM(item.state = 'failed'), 0),
                            COALESCE(SUM(item.state = 'deleted'), 0)
                     FROM subscription_run_query run_query
                     JOIN subscription_query definition USING(query_id)
                     JOIN source_post post
                       ON post.site_id = definition.site_id AND post.post_key = ?2
                     JOIN source_post_attempt attempt
                       ON attempt.run_query_id = run_query.run_query_id
                      AND attempt.source_post_id = post.source_post_id
                     LEFT JOIN library_root root ON root.root_id = post.root_item_id
                     LEFT JOIN source_item item USING(source_post_id)
                     WHERE run_query.run_query_id = ?1
                     GROUP BY post.source_post_id",
                        params![query.run_query_id, post_key],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, Option<u32>>(2)?,
                                row.get::<_, Option<bool>>(3)?.unwrap_or(false),
                                row.get::<_, i64>(4)?,
                                row.get::<_, i64>(5)?,
                                row.get::<_, i64>(6)?,
                                row.get::<_, i64>(7)?,
                                row.get::<_, i64>(8)?,
                                row.get::<_, i64>(9)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((
                    attempt_id,
                    source_post_id,
                    root_id,
                    created_this_attempt,
                    total,
                    pending,
                    downloaded,
                    ingested,
                    failed,
                    deleted,
                )) = row
                else {
                    return Err(LibraryError::InvalidState(
                        "completed source post was not recorded".into(),
                    ));
                };
                if pending != 0 || downloaded != 0 {
                    return Err(LibraryError::InvalidState(
                        "completed source post still has unsettled media".into(),
                    ));
                }
                let outcome = if root_id.is_some() {
                    if created_this_attempt {
                        let roots = {
                            let mut statement = transaction.prepare(
                                "SELECT DISTINCT root.root_id, item.stable_key
                             FROM library_root root
                             JOIN library_item item ON item.local_id = root.root_id
                             WHERE root.root_id = ?1
                                OR root.root_id IN (
                                    SELECT media_item_id FROM source_item
                                    WHERE source_post_id = ?2 AND media_item_id IS NOT NULL
                                )
                             ORDER BY root.root_id",
                            )?;
                            let roots = statement
                                .query_map(params![root_id, source_post_id], |row| {
                                    Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
                                })?
                                .collect::<rusqlite::Result<Vec<_>>>()?;
                            roots
                        };
                        if roots.is_empty() {
                            return Err(LibraryError::InvalidState(
                                "added source post has no canonical roots".into(),
                            ));
                        }
                        for (root_id, stable_key) in &roots {
                            transaction.execute(
                            "INSERT INTO source_attempt_root(attempt_id, root_id, root_stable_key)
                             VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                            params![attempt_id, root_id, stable_key],
                        )?;
                        }
                        SourcePostOutcome::Added {
                            root_ids: roots.into_iter().map(|(root_id, _)| root_id).collect(),
                        }
                    } else {
                        SourcePostOutcome::Skipped {
                            reason: SkipReason::ExactDuplicate,
                        }
                    }
                } else if ingested != 0 {
                    return Err(LibraryError::InvalidState(
                        "ingested source media has no canonical root".into(),
                    ));
                } else {
                    let reason = if total == 0 {
                        SkipReason::NoUsableMedia
                    } else if failed == total {
                        SkipReason::SourceUnavailable
                    } else if deleted == total {
                        SkipReason::Other("deleted".into())
                    } else {
                        SkipReason::NoUsableMedia
                    };
                    SourcePostOutcome::Skipped { reason }
                };
                let (state, terminal_reason) = match &outcome {
                    SourcePostOutcome::Added { .. } => ("added", None),
                    SourcePostOutcome::Skipped { reason } => {
                        let reason = match reason {
                            SkipReason::NoUsableMedia => "no_usable_media".to_string(),
                            SkipReason::ExactDuplicate | SkipReason::AlreadyImported => {
                                "exact_duplicate".to_string()
                            }
                            SkipReason::UnsupportedMedia => "unsupported_media".to_string(),
                            SkipReason::SourceUnavailable => "source_unavailable".to_string(),
                            SkipReason::Other(reason) => reason.clone(),
                        };
                        ("skipped", Some(reason))
                    }
                    SourcePostOutcome::Failed { reason, .. } => ("failed", Some(reason.clone())),
                };
                transaction.execute(
                    "UPDATE source_file_attempt
                 SET state = CASE
                         WHEN state = 'staged' AND ?2 = 'added' THEN 'retained'
                         WHEN state = 'staged' THEN 'duplicate'
                         ELSE state
                     END,
                     staged_path = NULL
                 WHERE attempt_id = ?1",
                    params![attempt_id, state],
                )?;
                transaction.execute(
                    "UPDATE source_post_attempt
                 SET state = ?1, terminal_reason = ?2, settled_at = ?3
                 WHERE attempt_id = ?4
                   AND state NOT IN ('added', 'skipped', 'failed', 'cancelled')",
                    params![
                        state,
                        terminal_reason,
                        chrono::Utc::now().to_rfc3339(),
                        attempt_id
                    ],
                )?;
                Ok(outcome)
            },
        )
        .map(|(outcome, _)| outcome)
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

pub fn source_attempt_id(
    application: &LibraryApplication,
    run_query_id: i64,
    post_key: &str,
) -> Result<i64, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            connection
                .query_row(
                    "SELECT attempt.attempt_id
                     FROM source_post_attempt attempt
                     JOIN source_post post USING(source_post_id)
                     WHERE attempt.run_query_id = ?1 AND post.post_key = ?2",
                    params![run_query_id, post_key],
                    |row| row.get(0),
                )
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())
}

pub fn mark_source_item_staged(
    application: &LibraryApplication,
    run_query_id: i64,
    source_item_id: i64,
    content_hash: &str,
    staged_path: &str,
    bytes_staged: u64,
    now: &str,
) -> Result<bool, String> {
    let bytes_staged = i64::try_from(bytes_staged)
        .map_err(|_| "downloaded media size exceeds SQLite integer range".to_string())?;
    application
        .library()
        .auxiliary_write(
            WorkPriority::CanonicalIngest,
            resources(),
            [],
            |transaction, _| {
                let attempt_id = transaction.query_row(
                    "SELECT attempt.attempt_id
                     FROM source_post_attempt attempt
                     JOIN source_item item ON item.source_post_id = attempt.source_post_id
                     WHERE attempt.run_query_id = ?1 AND item.source_item_id = ?2",
                    params![run_query_id, source_item_id],
                    |row| row.get::<_, i64>(0),
                )?;
                let duplicate = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM media_file WHERE content_hash = ?1)
                         OR EXISTS(
                             SELECT 1 FROM source_file_attempt sibling
                             WHERE sibling.attempt_id = ?2
                               AND sibling.source_item_id != ?3
                               AND sibling.content_hash = ?1
                               AND sibling.state IN ('staged', 'retained', 'duplicate')
                         )",
                    params![content_hash, attempt_id, source_item_id],
                    |row| row.get::<_, bool>(0),
                )?;
                transaction.execute(
                    "UPDATE source_file_attempt
                     SET content_hash = ?1, state = ?2, staged_path = ?3,
                         bytes_staged = ?4, error = NULL
                     WHERE attempt_id = ?5 AND source_item_id = ?6",
                    params![
                        content_hash,
                        if duplicate { "duplicate" } else { "staged" },
                        staged_path,
                        bytes_staged,
                        attempt_id,
                        source_item_id,
                    ],
                )?;
                transaction.execute(
                    "UPDATE source_post_attempt SET state = 'downloading'
                     WHERE attempt_id = ?1 AND state = 'discovered'",
                    [attempt_id],
                )?;
                transaction.execute(
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
                Ok(duplicate)
            },
        )
        .map(|(duplicate, _)| duplicate)
        .map_err(|error| error.to_string())
}

pub fn mark_source_item_failed(
    application: &LibraryApplication,
    run_query_id: i64,
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
                transaction.execute(
                    "UPDATE source_file_attempt
                     SET state = 'failed', error = ?1
                     WHERE source_item_id = ?2 AND attempt_id = (
                         SELECT attempt_id FROM source_post_attempt
                         WHERE run_query_id = ?3
                           AND source_post_id = (
                               SELECT source_post_id FROM source_item WHERE source_item_id = ?2
                           )
                     )",
                    params![error, source_item_id, run_query_id],
                )?;
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
        "INSERT INTO source_post_attempt (
             run_query_id, source_post_id, state, started_at
         ) VALUES (?1, ?2, 'discovered', ?3)
         ON CONFLICT(run_query_id, source_post_id) DO NOTHING",
        params![run_query_id, source_post_id, now],
    )?;
    let attempt_id = transaction.query_row(
        "SELECT attempt_id FROM source_post_attempt
         WHERE run_query_id = ?1 AND source_post_id = ?2",
        params![run_query_id, source_post_id],
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
                 state = CASE
                     WHEN source_item.state = 'ingested'
                      AND source_item.media_item_id IS NULL
                      AND (SELECT root_item_id FROM source_post
                           WHERE source_post_id = source_item.source_post_id) IS NULL
                      AND EXISTS (
                          SELECT 1 FROM deletion_tombstone tombstone
                          WHERE tombstone.stable_key =
                              'source:' || ?7 || ':' || ?8 || ':' || source_item.item_key
                      )
                     THEN 'deleted'
                     WHEN source_item.state = 'ingested'
                      AND source_item.media_item_id IS NULL
                      AND (SELECT root_item_id FROM source_post
                           WHERE source_post_id = source_item.source_post_id) IS NULL
                     THEN 'pending'
                     ELSE source_item.state
                 END,
                 last_error = CASE
                     WHEN source_item.state = 'ingested'
                      AND source_item.media_item_id IS NULL
                      AND (SELECT root_item_id FROM source_post
                           WHERE source_post_id = source_item.source_post_id) IS NULL
                     THEN NULL
                     ELSE source_item.last_error
                 END,
                 updated_at = excluded.updated_at",
            params![
                source_post_id,
                item.item_key,
                item.position,
                item.media_url,
                item.canonical_url,
                now,
                post.site_id,
                post.post_key,
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
        transaction.execute(
            "INSERT INTO source_file_attempt (
                 attempt_id, source_item_id, state
             ) VALUES (?1, ?2, 'discovered')
             ON CONFLICT(attempt_id, source_item_id) DO NOTHING",
            params![attempt_id, source_item_id],
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
    complete_query_with_policy(application, query, resume_cursor, true, now)
}

pub fn complete_query_terminal(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    resume_cursor: Option<&str>,
    now: &str,
) -> Result<MutationReceipt, String> {
    complete_query_with_policy(application, query, resume_cursor, false, now)
}

fn complete_query_with_policy(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    resume_cursor: Option<&str>,
    continue_if_budget_remaining: bool,
    now: &str,
) -> Result<MutationReceipt, String> {
    write_transition(application, |transaction| {
        let added_posts: u32 = transaction
            .query_row(
                "SELECT COUNT(*) FROM source_post_attempt
                 WHERE run_query_id = ?1 AND state = 'added'",
                [query.run_query_id],
                |row| row.get::<_, i64>(0),
            )?
            .try_into()
            .unwrap_or(u32::MAX);
        let continue_run = continue_if_budget_remaining
            && resume_cursor != Some("")
            && added_posts < query.configured_post_limit();
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
        if retry_at.is_none() {
            transaction.execute(
                "UPDATE source_post_attempt
                 SET state = 'failed', terminal_reason = ?1, settled_at = ?2
                 WHERE run_query_id = ?3
                   AND state NOT IN ('added', 'skipped', 'failed', 'cancelled')",
                params![kind, now, query.run_query_id],
            )?;
        }
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
    let unsettled = transaction.query_row(
        "SELECT COUNT(*)
         FROM subscription_run_query query_run
         JOIN source_post_attempt attempt USING(run_query_id)
         WHERE query_run.run_id = ?1
           AND attempt.state NOT IN ('added', 'skipped', 'failed', 'cancelled')",
        [run_id],
        |row| row.get::<_, i64>(0),
    )?;
    if unsettled != 0 && failed == 0 && cancelled == 0 {
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
    let candidate = candidate.and_local_timezone(chrono::Local).earliest()? + Duration::minutes(2);
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
        mark_source_item_staged(
            &application,
            first.run_query_id,
            ids["media-1"],
            "run-created-hash",
            "/tmp/run-created",
            1,
            "2026-08-29T00:00:03Z",
        )
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
                        "INSERT INTO source_attempt_root(attempt_id, root_id, root_stable_key)
                         SELECT attempt_id, 5000, 'run-created-root'
                         FROM source_post_attempt WHERE run_query_id = ?1",
                        [first.run_query_id],
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
        assert!(matches!(
            settled_post_outcome(&application, &first, "post-1").unwrap(),
            SourcePostOutcome::Added { .. }
        ));
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
    fn root_created_by_an_earlier_post_in_the_same_run_is_not_counted_as_added() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Exact duplicate".into(),
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
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00.100Z")
            .unwrap();
        let query = claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:00.200Z",
        )
        .unwrap()
        .unwrap();
        let ids = record_post(
            &application,
            query.run_query_id,
            &NormalizedPost {
                site_id: query.site_id.clone(),
                post_key: "duplicate".into(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![NormalizedItem {
                    item_key: "duplicate:media".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:00.500Z",
        )
        .unwrap();
        mark_source_item_staged(
            &application,
            query.run_query_id,
            ids["duplicate:media"],
            "existing-hash",
            "/tmp/existing",
            1,
            "2026-08-29T00:00:00.600Z",
        )
        .unwrap();
        // This root was created after the run started but before this post was
        // traversed, as happens when two posts in one run reuse the same media.
        let imported_at_ms = DateTime::parse_from_rfc3339("2026-08-29T00:00:00.300Z")
            .unwrap()
            .timestamp_millis();
        application
            .library()
            .auxiliary_write(
                WorkPriority::CanonicalIngest,
                ["subscriptions".to_owned()],
                [],
                |transaction, _| {
                    transaction.execute(
                        "INSERT INTO library_item(local_id, stable_key, item_kind)
                         VALUES (7000, 'existing-root', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, content_hash, file_path, mime, size_bytes)
                         VALUES (7001, 'existing-hash', '/tmp/existing', 'image/png', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_item(media_id, media_name, file_id)
                         VALUES (7000, 'existing', 7001)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root
                             (root_id, name, cover_media_id, imported_at_ms, modified_at_ms,
                              media_count, total_size_bytes)
                         VALUES (7000, 'existing', 7000, ?1, ?1, 1, 1)",
                        [imported_at_ms],
                    )?;
                    transaction.execute(
                        "UPDATE source_item SET state = 'ingested', media_item_id = 7000
                         WHERE source_item_id = ?1",
                        [ids["duplicate:media"]],
                    )?;
                    transaction.execute(
                        "UPDATE source_post SET root_item_id = 7000
                         WHERE source_post_id = (
                             SELECT source_post_id FROM source_item WHERE source_item_id = ?1
                         )",
                        [ids["duplicate:media"]],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(
            settled_post_outcome(&application, &query, "duplicate").unwrap(),
            SourcePostOutcome::Skipped {
                reason: SkipReason::ExactDuplicate,
            }
        );
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
        assert!(
            claim_next_query(&application, &mut sibling_schedule, "2026-08-29T00:00:02Z")
                .unwrap()
                .is_none()
        );

        complete_query(&application, &first, None, "2026-08-29T00:00:03Z").unwrap();
        let second = claim_next_query(&application, &mut sibling_schedule, "2026-08-29T00:00:04Z")
            .unwrap()
            .expect("the sibling becomes claimable once the first settles");
        assert_ne!(second.query_id, first.query_id);
    }

    #[test]
    fn gallery_queries_are_single_flight_across_subscriptions_and_hosts() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let create_gallery = |name: &str, query_text: &str| {
            application
                .create_subscription_definition_library(
                    &NewSubscription {
                        name: name.into(),
                        schedule: "manual".into(),
                        initial_post_limit: Some(1),
                        periodic_post_limit: Some(1),
                        queries: vec![NewSubscriptionQuery {
                            site_id: "ehentai".into(),
                            query_text: query_text.into(),
                            display_name: Some("Gallery import".into()),
                            notes: None,
                            group_posts: true,
                        }],
                    },
                    "2026-08-29T00:00:00Z",
                )
                .unwrap()
                .0
        };
        let ehentai = create_gallery("E-Hentai", "https://e-hentai.org/g/12345/67890abcde/");
        let exhentai = create_gallery("ExHentai", "https://exhentai.org/g/54321/abcdef0123/");
        application
            .request_subscription_run_library(ehentai, "2026-08-29T00:00:00Z")
            .unwrap();
        assert!(application
            .request_subscription_run_library(exhentai, "2026-08-29T00:00:00Z")
            .unwrap_err()
            .contains("gallery download is already running"));

        let mut schedule = DomainSchedule::new();
        let first = claim_next_query(&application, &mut schedule, "2026-08-29T00:00:01Z")
            .unwrap()
            .expect("the first gallery is claimable");
        assert_eq!(first.site_id, "ehentai");
        assert!(
            claim_next_query(
                &application,
                &mut DomainSchedule::new(),
                "2026-08-29T00:00:02Z",
            )
            .unwrap()
            .is_none(),
            "a gallery on the other host must wait for the active gallery"
        );

        complete_query_terminal(&application, &first, None, "2026-08-29T00:00:03Z").unwrap();
        application
            .request_subscription_run_library(exhentai, "2026-08-29T00:00:03Z")
            .unwrap();
        let second = claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:04Z",
        )
        .unwrap()
        .expect("the second gallery becomes claimable after settlement");
        assert_ne!(second.subscription_id, first.subscription_id);
    }

    #[test]
    fn downloaded_but_not_ingested_post_blocks_a_second_worker_claim() {
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
                    item_key: "post-1:media".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        mark_source_item_staged(
            &application,
            first.run_query_id,
            ids["post-1:media"],
            "post-1-hash",
            "/tmp/post-1",
            1,
            "2026-08-29T00:00:03Z",
        )
        .unwrap();
        complete_query(&application, &first, None, "2026-08-29T00:00:04Z").unwrap();

        // An open post attempt is exclusively owned by the current worker. A
        // retry cannot claim the query and begin another post beside it.
        let mut waiting_schedule = DomainSchedule::new();
        assert!(
            claim_next_query(&application, &mut waiting_schedule, "2026-08-29T00:00:05Z")
                .unwrap()
                .is_none()
        );
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
        assert_eq!(
            status, "pending",
            "reserved budget must not settle the query"
        );

        mark_source_item_failed(
            &application,
            first.run_query_id,
            first.subscription_id,
            first.query_id,
            ids["post-1:media"],
            "gone",
            "2026-08-29T00:00:05Z",
        )
        .unwrap();
        assert!(matches!(
            settled_post_outcome(&application, &first, "post-1").unwrap(),
            SourcePostOutcome::Skipped { .. }
        ));
        let mut retry_schedule = DomainSchedule::new();
        let second = claim_next_query(&application, &mut retry_schedule, "2026-08-29T00:00:06Z")
            .unwrap()
            .expect("a released reservation makes budget claimable again");
        assert_eq!(
            second.source_post_batch_size(),
            2,
            "a skipped post does not consume the added-post budget"
        );
    }

    #[test]
    fn recovery_replays_an_incomplete_post_from_the_committed_cursor() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Interrupted download".into(),
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
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let query = claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:01Z",
        )
        .unwrap()
        .unwrap();
        let ids = record_post(
            &application,
            query.run_query_id,
            &NormalizedPost {
                site_id: query.site_id.clone(),
                post_key: "interrupted".into(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![NormalizedItem {
                    item_key: "interrupted:media".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        mark_source_item_staged(
            &application,
            query.run_query_id,
            ids["interrupted:media"],
            "interrupted-hash",
            "/tmp/interrupted",
            1,
            "2026-08-29T00:00:03Z",
        )
        .unwrap();

        assert_eq!(
            recover(&application, "2026-08-29T00:01:00Z").unwrap(),
            RecoveryCounts {
                runs: 1,
                query_runs: 1,
            }
        );
        let (attempts, run_status, query_status): (i64, String, String) = application
            .library()
            .auxiliary_read(WorkPriority::VisibleRead, |connection| {
                Ok((
                    connection.query_row(
                        "SELECT COUNT(*) FROM source_post_attempt WHERE run_query_id = ?1",
                        [query.run_query_id],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT status FROM subscription_run WHERE run_id = ?1",
                        [query.run_id],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT status FROM subscription_run_query WHERE run_query_id = ?1",
                        [query.run_query_id],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(
            attempts, 0,
            "the current post is redownloaded after restart"
        );
        assert_eq!(run_status, "pending");
        assert_eq!(query_status, "pending");
        assert!(claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:01:01Z",
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn recovery_settles_a_canonical_commit_before_replaying_the_query() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Committed ingest".into(),
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
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let query = claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:01Z",
        )
        .unwrap()
        .unwrap();
        let ids = record_post(
            &application,
            query.run_query_id,
            &NormalizedPost {
                site_id: query.site_id.clone(),
                post_key: "committed".into(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![NormalizedItem {
                    item_key: "committed:media".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        mark_source_item_staged(
            &application,
            query.run_query_id,
            ids["committed:media"],
            "committed-hash",
            "/tmp/committed",
            1,
            "2026-08-29T00:00:03Z",
        )
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
                         VALUES (6000, 'recovered-root', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, content_hash, file_path, mime, size_bytes)
                         VALUES (6001, 'committed-hash', '/tmp/committed', 'image/png', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_item(media_id, media_name, file_id)
                         VALUES (6000, 'recovered', 6001)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root
                             (root_id, name, cover_media_id, imported_at_ms, modified_at_ms,
                              media_count, total_size_bytes)
                         VALUES (6000, 'recovered', 6000, 1787976003000, 1787976003000, 1, 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO source_attempt_root(attempt_id, root_id, root_stable_key)
                         SELECT attempt_id, 6000, 'recovered-root'
                         FROM source_post_attempt WHERE run_query_id = ?1",
                        [query.run_query_id],
                    )?;
                    transaction.execute(
                        "UPDATE source_item SET state = 'ingested', media_item_id = 6000
                         WHERE source_item_id = ?1",
                        [ids["committed:media"]],
                    )?;
                    transaction.execute(
                        "UPDATE source_post SET root_item_id = 6000
                         WHERE source_post_id = (
                             SELECT source_post_id FROM source_item WHERE source_item_id = ?1
                         )",
                        [ids["committed:media"]],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        recover(&application, "2026-08-29T00:01:00Z").unwrap();
        let (state, roots, file_state): (String, i64, String) = application
            .library()
            .auxiliary_read(WorkPriority::VisibleRead, |connection| {
                Ok((
                    connection.query_row(
                        "SELECT state FROM source_post_attempt WHERE run_query_id = ?1",
                        [query.run_query_id],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT COUNT(*) FROM source_attempt_root result
                         JOIN source_post_attempt attempt USING(attempt_id)
                         WHERE attempt.run_query_id = ?1 AND result.root_id = 6000",
                        [query.run_query_id],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT file.state FROM source_file_attempt file
                         JOIN source_post_attempt attempt USING(attempt_id)
                         WHERE attempt.run_query_id = ?1",
                        [query.run_query_id],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(state, "added");
        assert_eq!(roots, 1);
        assert_eq!(file_state, "retained");
    }

    #[test]
    fn rediscovery_requeues_an_orphaned_ingested_source_item() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Gallery recovery".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![NewSubscriptionQuery {
                site_id: "ehentai".into(),
                query_text: "https://e-hentai.org/g/2428940/abcdef1234/".into(),
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
        let query = claim_next_query(
            &application,
            &mut DomainSchedule::new(),
            "2026-08-29T00:00:01Z",
        )
        .unwrap()
        .unwrap();
        let post = NormalizedPost {
            site_id: "ehentai".into(),
            post_key: "gallery-1".into(),
            canonical_url: None,
            creator_name: None,
            title: None,
            description: None,
            captured_at: None,
            metadata_json: None,
            items: vec![NormalizedItem {
                item_key: "gallery-1:page-1".into(),
                position: 1,
                media_url: None,
                canonical_url: None,
            }],
        };
        let ids = record_post(
            &application,
            query.run_query_id,
            &post,
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        application
            .library()
            .auxiliary_write(
                WorkPriority::CanonicalIngest,
                ["subscriptions".to_owned()],
                [],
                |transaction, _| {
                    transaction.execute(
                        "UPDATE source_item SET state = 'ingested' WHERE source_item_id = ?1",
                        [ids["gallery-1:page-1"]],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        record_post(
            &application,
            query.run_query_id,
            &post,
            "2026-08-29T00:00:03Z",
        )
        .unwrap();
        let state = application
            .library()
            .auxiliary_read(WorkPriority::VisibleRead, |connection| {
                connection
                    .query_row(
                        "SELECT state FROM source_item WHERE source_item_id = ?1",
                        [ids["gallery-1:page-1"]],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(state, "pending");
    }
}
