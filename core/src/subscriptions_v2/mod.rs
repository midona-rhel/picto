//! Durable subscription state for the replacement backend.
//!
//! This module owns persistence only. An adapter claims a query run, records
//! normalized posts, and advances source-item state; it does not execute a
//! source-specific downloader here.

use std::collections::BTreeMap;

use chrono::DateTime;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::store::Store;

const DOMAIN_INTERVAL_MS: i64 = 1_000;

#[derive(Debug, Clone)]
pub struct SubscriptionInput {
    pub subscription_key: String,
    pub name: String,
    pub schedule: String,
    pub paused: bool,
    pub initial_post_limit: Option<i64>,
    pub periodic_post_limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct QueryInput {
    pub query_key: String,
    pub site_id: String,
    pub domain_key: String,
    pub query_kind: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NormalizedPost {
    pub site_id: String,
    pub post_key: String,
    pub canonical_url: Option<String>,
    pub creator_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub captured_at: Option<String>,
    pub metadata_json: Option<String>,
    pub items: Vec<NormalizedItem>,
}

#[derive(Debug, Clone)]
pub struct NormalizedItem {
    pub item_key: String,
    pub position: i64,
    pub media_url: Option<String>,
    pub canonical_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl RunState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceItemState {
    Pending,
    Downloaded,
    Ingested,
    Failed,
    Deleted,
}

impl SourceItemState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloaded => "downloaded",
            Self::Ingested => "ingested",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunRecord {
    pub run_id: i64,
    pub state: RunState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryRunRecord {
    pub run_query_id: i64,
    pub run_id: i64,
    pub query_id: i64,
    pub state: RunState,
    pub attempt_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedRun {
    pub run_id: i64,
    pub created: bool,
    pub state: RunState,
}

#[derive(Debug, Clone)]
pub struct ClaimedQueryRun {
    pub run_query_id: i64,
    pub run_id: i64,
    pub query_id: i64,
    pub subscription_id: i64,
    pub site_id: String,
    pub domain_key: String,
    pub query_kind: String,
    pub query_text: String,
    pub initial_post_limit: Option<i64>,
    pub periodic_post_limit: Option<i64>,
    pub initial_run_complete: bool,
    pub resume_cursor: Option<String>,
    pub attempt_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryRunTransition {
    pub query_state: RunState,
    pub run_state: Option<RunState>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceItemCounters {
    pub total: i64,
    pub pending: i64,
    pub downloaded: i64,
    pub ingested: i64,
    pub failed: i64,
    pub deleted: i64,
}

impl SourceItemCounters {
    pub fn completed(self) -> i64 {
        self.downloaded + self.ingested + self.failed + self.deleted
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryCounts {
    pub runs: usize,
    pub query_runs: usize,
}

/// Process-local domain throttle. The durable queue remains in SQLite; only
/// the next allowed time is intentionally lost when the process exits.
#[derive(Debug, Default)]
pub struct DomainSchedule {
    next_allowed_at_ms: BTreeMap<String, i64>,
}

impl DomainSchedule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_allowed_at_ms(&self, domain_key: &str) -> Option<i64> {
        self.next_allowed_at_ms.get(domain_key).copied()
    }

    fn allows(&self, domain_key: &str, now_ms: i64) -> bool {
        self.next_allowed_at_ms(domain_key)
            .is_none_or(|next| next <= now_ms)
    }

    fn mark_started(&mut self, domain_key: String, now_ms: i64) {
        self.next_allowed_at_ms
            .insert(domain_key, now_ms + DOMAIN_INTERVAL_MS);
    }
}

pub fn create_subscription(
    store: &Store,
    input: &SubscriptionInput,
    now: &str,
) -> Result<i64, String> {
    require_text("subscription key", &input.subscription_key)?;
    require_text("subscription name", &input.name)?;
    require_text("schedule", &input.schedule)?;
    let (id, _) = store.transaction(|tx| {
        tx.execute(
            "INSERT INTO subscription (
                 subscription_key, name, schedule, paused,
                 initial_post_limit, periodic_post_limit, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                input.subscription_key,
                input.name,
                input.schedule,
                input.paused as i64,
                input.initial_post_limit,
                input.periodic_post_limit,
                now,
            ],
        )?;
        Ok(tx.last_insert_rowid())
    })?;
    Ok(id)
}

pub fn create_query(
    store: &Store,
    subscription_id: i64,
    input: &QueryInput,
) -> Result<i64, String> {
    require_text("query key", &input.query_key)?;
    require_text("site id", &input.site_id)?;
    require_text("domain key", &input.domain_key)?;
    require_text("query kind", &input.query_kind)?;
    let (id, _) = store.transaction(|tx| {
        tx.execute(
            "INSERT INTO subscription_query (
                 query_key, subscription_id, site_id, domain_key, query_kind, query_text,
                 display_name, notes
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                input.query_key,
                subscription_id,
                input.site_id,
                input.domain_key,
                input.query_kind,
                input.query_text,
                input.display_name,
                input.notes,
            ],
        )?;
        Ok(tx.last_insert_rowid())
    })?;
    Ok(id)
}

pub fn create_run(
    store: &Store,
    subscription_id: i64,
    requested_by: &str,
    now: &str,
) -> Result<CreatedRun, String> {
    require_text("requested by", requested_by)?;
    let (run, _) = store.transaction(|tx| {
        if let Some((run_id, state)) = tx
            .query_row(
                "SELECT run_id, status FROM subscription_run
                 WHERE subscription_id = ?1 AND status IN ('pending', 'running')
                 ORDER BY run_id LIMIT 1",
                [subscription_id],
                |row| Ok((row.get::<_, i64>(0)?, parse_run_state(row.get(1)?))),
            )
            .optional()?
        {
            return Ok(CreatedRun {
                run_id,
                created: false,
                state: state?,
            });
        }

        tx.execute(
            "INSERT INTO subscription_run (
                 subscription_id, requested_by, status, created_at
             ) VALUES (?1, ?2, 'pending', ?3)",
            params![subscription_id, requested_by, now],
        )?;
        let run_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO subscription_run_query (
                 run_id, query_id, status, available_at
             ) SELECT ?1, query_id, 'pending', ?2
               FROM subscription_query
               WHERE subscription_id = ?3 AND paused = 0",
            params![run_id, now, subscription_id],
        )?;
        let state = settle_run(tx, run_id, now)?.unwrap_or(RunState::Pending);
        Ok(CreatedRun {
            run_id,
            created: true,
            state,
        })
    })?;
    Ok(run)
}

pub fn recover_startup(store: &Store, now: &str) -> Result<RecoveryCounts, String> {
    let (counts, _) = store.transaction(|tx| {
        let query_runs = tx.execute(
            "UPDATE subscription_run_query
             SET status = 'pending', available_at = ?1, started_at = NULL,
                 finished_at = NULL
             WHERE status = 'running'",
            [now],
        )?;
        let runs = tx.execute(
            "UPDATE subscription_run
             SET status = 'pending', started_at = NULL, finished_at = NULL
             WHERE status = 'running'",
            [],
        )?;
        Ok(RecoveryCounts { runs, query_runs })
    })?;
    Ok(counts)
}

pub fn claim_next_query_run(
    store: &Store,
    schedule: &mut DomainSchedule,
    now: &str,
) -> Result<Option<ClaimedQueryRun>, String> {
    let now_ms = parse_timestamp_ms(now)?;
    let (claim, _) = store.transaction(|tx| {
        let mut statement = tx.prepare(
            "SELECT qr.run_query_id, qr.run_id, qr.query_id, r.subscription_id,
                    q.site_id, q.domain_key, q.query_kind, q.query_text,
                    s.initial_post_limit, s.periodic_post_limit,
                    q.initial_run_complete, COALESCE(qr.resume_cursor, q.resume_cursor),
                    qr.attempt_count
             FROM subscription_run_query qr
             JOIN subscription_run r ON r.run_id = qr.run_id
             JOIN subscription_query q ON q.query_id = qr.query_id
             JOIN subscription s ON s.subscription_id = r.subscription_id
             WHERE qr.status = 'pending' AND qr.available_at <= ?1
               AND r.status IN ('pending', 'running')
               AND q.paused = 0 AND s.paused = 0
             ORDER BY qr.available_at, qr.run_query_id",
        )?;
        let candidates = statement
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
                    initial_post_limit: row.get(8)?,
                    periodic_post_limit: row.get(9)?,
                    initial_run_complete: row.get(10)?,
                    resume_cursor: row.get(11)?,
                    attempt_count: row.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let Some(candidate) = candidates
            .into_iter()
            .find(|candidate| schedule.allows(&candidate.domain_key, now_ms))
        else {
            return Ok(None);
        };

        let changed = tx.execute(
            "UPDATE subscription_run_query
             SET status = 'running', started_at = ?1, attempt_count = attempt_count + 1
             WHERE run_query_id = ?2 AND status = 'pending' AND available_at <= ?1",
            params![now, candidate.run_query_id],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        tx.execute(
            "UPDATE subscription_run
             SET status = 'running', started_at = COALESCE(started_at, ?1)
             WHERE run_id = ?2 AND status = 'pending'",
            params![now, candidate.run_id],
        )?;
        Ok(Some(ClaimedQueryRun {
            attempt_count: candidate.attempt_count + 1,
            ..candidate
        }))
    })?;
    if let Some(claim) = &claim {
        schedule.mark_started(claim.domain_key.clone(), now_ms);
    }
    Ok(claim)
}

pub fn record_post(
    store: &Store,
    run_query_id: i64,
    post: &NormalizedPost,
    now: &str,
) -> Result<usize, String> {
    require_text("post site id", &post.site_id)?;
    require_text("post key", &post.post_key)?;
    for item in &post.items {
        require_text("item key", &item.item_key)?;
        if item.position < 0 {
            return Err("item position cannot be negative".to_string());
        }
    }
    if post
        .items
        .iter()
        .map(|item| item.item_key.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != post.items.len()
    {
        return Err("post contains duplicate item keys".to_string());
    }
    let (count, _) = store.transaction(|tx| {
        let (run_id, query_id, subscription_id, site_id, status): (i64, i64, i64, String, String) =
            tx.query_row(
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
        if status != "running" {
            return Err(sql_error("query run must be running to record posts"));
        }
        if site_id != post.site_id {
            return Err(sql_error("post site does not match query site"));
        }

        tx.execute(
            "INSERT INTO source_post (
                 site_id, post_key, canonical_url, creator_name, title,
                 description, captured_at, metadata_json, created_at, updated_at
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
        let source_post_id: i64 = tx.query_row(
            "SELECT source_post_id FROM source_post WHERE site_id = ?1 AND post_key = ?2",
            params![post.site_id, post.post_key],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO subscription_source_post (
                 subscription_id, query_id, source_post_id, last_seen_run_id
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(subscription_id, query_id, source_post_id)
             DO UPDATE SET last_seen_run_id = excluded.last_seen_run_id",
            params![subscription_id, query_id, source_post_id, run_id],
        )?;
        for item in &post.items {
            tx.execute(
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
            let source_item_id: i64 = tx.query_row(
                "SELECT source_item_id FROM source_item
                 WHERE source_post_id = ?1 AND item_key = ?2",
                params![source_post_id, item.item_key],
                |row| row.get(0),
            )?;
            tx.execute(
                "INSERT INTO subscription_run_source_item (run_query_id, source_item_id)
                 VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                params![run_query_id, source_item_id],
            )?;
        }
        Ok(post.items.len())
    })?;
    Ok(count)
}

pub fn update_source_item_state(
    store: &Store,
    source_item_id: i64,
    state: SourceItemState,
    error: Option<&str>,
    now: &str,
) -> Result<(), String> {
    store.transaction(|tx| {
        let changed = tx.execute(
            "UPDATE source_item SET state = ?1, last_error = ?2, updated_at = ?3
             WHERE source_item_id = ?4",
            params![state.as_str(), error, now, source_item_id],
        )?;
        if changed != 1 {
            return Err(sql_error("source item does not exist"));
        }
        Ok(())
    })?;
    Ok(())
}

pub fn source_item_counters(
    store: &Store,
    run_query_id: i64,
) -> Result<SourceItemCounters, String> {
    store.read(|connection| {
        connection.query_row(
            "SELECT
                 COUNT(*),
                 COALESCE(SUM(CASE WHEN si.state = 'pending' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN si.state = 'downloaded' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN si.state = 'ingested' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN si.state = 'failed' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN si.state = 'deleted' THEN 1 ELSE 0 END), 0)
             FROM subscription_run_source_item rsi
             JOIN source_item si ON si.source_item_id = rsi.source_item_id
             WHERE rsi.run_query_id = ?1",
            [run_query_id],
            |row| {
                Ok(SourceItemCounters {
                    total: row.get(0)?,
                    pending: row.get(1)?,
                    downloaded: row.get(2)?,
                    ingested: row.get(3)?,
                    failed: row.get(4)?,
                    deleted: row.get(5)?,
                })
            },
        )
    })
}

pub fn complete_query_run(
    store: &Store,
    run_query_id: i64,
    now: &str,
) -> Result<QueryRunTransition, String> {
    complete_query_run_with_cursor(store, run_query_id, None, now)
}

pub fn complete_query_run_with_cursor(
    store: &Store,
    run_query_id: i64,
    resume_cursor: Option<&str>,
    now: &str,
) -> Result<QueryRunTransition, String> {
    let (transition, _) = store.transaction(|tx| {
        let query_id: i64 = tx.query_row(
            "SELECT query_id FROM subscription_run_query
             WHERE run_query_id = ?1 AND status = 'running'",
            [run_query_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE subscription_run_query
             SET status = 'succeeded', finished_at = ?1,
                 resume_cursor = COALESCE(?2, resume_cursor),
                 failure_kind = NULL, error_message = NULL
             WHERE run_query_id = ?3 AND status = 'running'",
            params![now, resume_cursor, run_query_id],
        )?;
        tx.execute(
            "UPDATE subscription_query
             SET last_success_at = ?1, initial_run_complete = 1,
                 resume_cursor = COALESCE(?2, resume_cursor)
             WHERE query_id = ?3",
            params![now, resume_cursor, query_id],
        )?;
        let run_state = settle_run_for_query(tx, run_query_id, now)?;
        Ok(QueryRunTransition {
            query_state: RunState::Succeeded,
            run_state,
        })
    })?;
    Ok(transition)
}

pub fn fail_query_run(
    store: &Store,
    run_query_id: i64,
    failure_kind: &str,
    message: &str,
    retry_at: Option<&str>,
    now: &str,
) -> Result<QueryRunTransition, String> {
    let (transition, _) = store.transaction(|tx| {
        let query_id: i64 = tx.query_row(
            "SELECT query_id FROM subscription_run_query
             WHERE run_query_id = ?1 AND status = 'running'",
            [run_query_id],
            |row| row.get(0),
        )?;
        let (status, finished): (&str, Option<&str>) = if retry_at.is_some() {
            ("pending", None)
        } else {
            ("failed", Some(now))
        };
        tx.execute(
            "UPDATE subscription_run_query
             SET status = ?1, available_at = COALESCE(?2, available_at),
                 finished_at = ?3, failure_kind = ?4, error_message = ?5
             WHERE run_query_id = ?6 AND status = 'running'",
            params![
                status,
                retry_at,
                finished,
                failure_kind,
                message,
                run_query_id
            ],
        )?;
        tx.execute(
            "UPDATE subscription_query
             SET last_failure_at = ?1, last_failure_kind = ?2,
                 last_failure_message = ?3
             WHERE query_id = ?4",
            params![now, failure_kind, message, query_id],
        )?;
        let query_state = if retry_at.is_some() {
            RunState::Pending
        } else {
            RunState::Failed
        };
        let run_state = if retry_at.is_some() {
            None
        } else {
            settle_run_for_query(tx, run_query_id, now)?
        };
        Ok(QueryRunTransition {
            query_state,
            run_state,
        })
    })?;
    Ok(transition)
}

pub fn retry_query_run(store: &Store, run_query_id: i64, available_at: &str) -> Result<(), String> {
    store.transaction(|tx| {
        let changed = tx.execute(
            "UPDATE subscription_run_query
             SET status = 'pending', available_at = ?1, finished_at = NULL
             WHERE run_query_id = ?2 AND status = 'failed'",
            params![available_at, run_query_id],
        )?;
        if changed != 1 {
            return Err(sql_error("only a failed query run can be retried"));
        }
        tx.execute(
            "UPDATE subscription_run SET status = 'pending', finished_at = NULL,
                 failure_kind = NULL, error_message = NULL
             WHERE run_id = (SELECT run_id FROM subscription_run_query WHERE run_query_id = ?1)
               AND status = 'failed'",
            [run_query_id],
        )?;
        Ok(())
    })?;
    Ok(())
}

pub fn cancel_query_run(
    store: &Store,
    run_query_id: i64,
    now: &str,
) -> Result<QueryRunTransition, String> {
    let (transition, _) = store.transaction(|tx| {
        let changed = tx.execute(
            "UPDATE subscription_run_query
             SET status = 'cancelled', finished_at = ?1
             WHERE run_query_id = ?2 AND status IN ('pending', 'running')",
            params![now, run_query_id],
        )?;
        if changed != 1 {
            return Err(sql_error("only an active query run can be cancelled"));
        }
        let run_state = settle_run_for_query(tx, run_query_id, now)?;
        Ok(QueryRunTransition {
            query_state: RunState::Cancelled,
            run_state,
        })
    })?;
    Ok(transition)
}

pub fn cancel_run(store: &Store, run_id: i64, now: &str) -> Result<(), String> {
    store.transaction(|tx| {
        let changed = tx.execute(
            "UPDATE subscription_run_query
             SET status = 'cancelled', finished_at = ?1
             WHERE run_id = ?2 AND status IN ('pending', 'running')",
            params![now, run_id],
        )?;
        let run_changed = tx.execute(
            "UPDATE subscription_run SET status = 'cancelled', finished_at = ?1
             WHERE run_id = ?2 AND status IN ('pending', 'running')",
            params![now, run_id],
        )?;
        if changed == 0 && run_changed == 0 {
            return Err(sql_error("run is not active"));
        }
        Ok(())
    })?;
    Ok(())
}

pub fn get_run(store: &Store, run_id: i64) -> Result<Option<RunRecord>, String> {
    store.read(|connection| {
        connection
            .query_row(
                "SELECT run_id, status FROM subscription_run WHERE run_id = ?1",
                [run_id],
                |row| {
                    Ok(RunRecord {
                        run_id: row.get(0)?,
                        state: parse_run_state(row.get(1)?)?,
                    })
                },
            )
            .optional()
    })
}

pub fn get_query_run(store: &Store, run_query_id: i64) -> Result<Option<QueryRunRecord>, String> {
    store.read(|connection| {
        connection
            .query_row(
                "SELECT run_query_id, run_id, query_id, status, attempt_count
                 FROM subscription_run_query WHERE run_query_id = ?1",
                [run_query_id],
                |row| {
                    Ok(QueryRunRecord {
                        run_query_id: row.get(0)?,
                        run_id: row.get(1)?,
                        query_id: row.get(2)?,
                        state: parse_run_state(row.get(3)?)?,
                        attempt_count: row.get(4)?,
                    })
                },
            )
            .optional()
    })
}

fn settle_run_for_query(
    tx: &Transaction<'_>,
    run_query_id: i64,
    now: &str,
) -> rusqlite::Result<Option<RunState>> {
    let run_id: i64 = tx.query_row(
        "SELECT run_id FROM subscription_run_query WHERE run_query_id = ?1",
        [run_query_id],
        |row| row.get(0),
    )?;
    settle_run(tx, run_id, now)
}

fn settle_run(tx: &Transaction<'_>, run_id: i64, now: &str) -> rusqlite::Result<Option<RunState>> {
    let (total, pending, running, failed, cancelled): (i64, i64, i64, i64, i64) = tx.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END), 0)
         FROM subscription_run_query WHERE run_id = ?1",
        [run_id],
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
    if pending > 0 || running > 0 {
        return Ok(None);
    }
    let state = if failed > 0 {
        RunState::Failed
    } else if cancelled > 0 {
        RunState::Cancelled
    } else {
        RunState::Succeeded
    };
    let changed = tx.execute(
        "UPDATE subscription_run SET status = ?1, finished_at = ?2
         WHERE run_id = ?3 AND status IN ('pending', 'running')",
        params![state.as_str(), now, run_id],
    )?;
    if changed == 0 && total > 0 {
        return Ok(Some(parse_run_state(tx.query_row(
            "SELECT status FROM subscription_run WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )?)?));
    }
    Ok(Some(state))
}

fn parse_run_state(value: String) -> rusqlite::Result<RunState> {
    match value.as_str() {
        "pending" => Ok(RunState::Pending),
        "running" => Ok(RunState::Running),
        "succeeded" => Ok(RunState::Succeeded),
        "failed" => Ok(RunState::Failed),
        "cancelled" => Ok(RunState::Cancelled),
        _ => Err(sql_error("invalid persisted run state")),
    }
}

fn require_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} is required"))
    } else {
        Ok(())
    }
}

fn parse_timestamp_ms(value: &str) -> Result<i64, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.timestamp_millis())
        .map_err(|error| format!("invalid timestamp {value}: {error}"))
}

fn sql_error(message: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        (directory, store)
    }

    fn subscription(store: &Store) -> i64 {
        create_subscription(
            store,
            &SubscriptionInput {
                subscription_key: "sub".into(),
                name: "Subscription".into(),
                schedule: "manual".into(),
                paused: false,
                initial_post_limit: None,
                periodic_post_limit: None,
            },
            "2026-01-01T00:00:00Z",
        )
        .unwrap()
    }

    fn query(store: &Store, subscription_id: i64, key: &str, site: &str) -> i64 {
        create_query(
            store,
            subscription_id,
            &QueryInput {
                query_key: key.into(),
                site_id: site.into(),
                domain_key: site.into(),
                query_kind: "search".into(),
                query_text: key.into(),
                display_name: None,
                notes: None,
            },
        )
        .unwrap()
    }

    fn post(site: &str, key: &str, items: usize) -> NormalizedPost {
        NormalizedPost {
            site_id: site.into(),
            post_key: key.into(),
            canonical_url: None,
            creator_name: None,
            title: None,
            description: None,
            captured_at: None,
            metadata_json: None,
            items: (0..items)
                .map(|position| NormalizedItem {
                    item_key: format!("item-{position}"),
                    position: position as i64,
                    media_url: Some(format!("https://{site}/{position}.jpg")),
                    canonical_url: None,
                })
                .collect(),
        }
    }

    #[test]
    fn running_rows_recover_after_reopening_store() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let sub = subscription(&store);
        query(&store, sub, "q", "example.test");
        let run = create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let mut schedule = DomainSchedule::new();
        let claim = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            get_query_run(&store, claim.run_query_id)
                .unwrap()
                .unwrap()
                .state,
            RunState::Running
        );
        drop(store);

        let reopened = Store::open(directory.path()).unwrap();
        assert_eq!(
            recover_startup(&reopened, "2026-01-01T00:00:02Z").unwrap(),
            RecoveryCounts {
                runs: 1,
                query_runs: 1
            }
        );
        assert_eq!(
            get_run(&reopened, run.run_id).unwrap().unwrap().state,
            RunState::Pending
        );
        assert_eq!(
            get_query_run(&reopened, claim.run_query_id)
                .unwrap()
                .unwrap()
                .state,
            RunState::Pending
        );
    }

    #[test]
    fn active_run_creation_is_idempotent() {
        let (_directory, store) = store();
        let sub = subscription(&store);
        query(&store, sub, "q", "example.test");
        let first = create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let second = create_run(&store, sub, "manual", "2026-01-01T00:00:01Z").unwrap();
        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.run_id, second.run_id);
        let count: i64 = store
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM subscription_run", [], |row| {
                    row.get(0)
                })
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn different_adapters_on_one_domain_share_the_same_throttle() {
        let (_directory, store) = store();
        let sub = subscription(&store);
        for (key, site) in [("search", "pixiv"), ("user", "pixivuser")] {
            create_query(
                &store,
                sub,
                &QueryInput {
                    query_key: key.into(),
                    site_id: site.into(),
                    domain_key: "pixiv.net".into(),
                    query_kind: key.into(),
                    query_text: "123".into(),
                    display_name: None,
                    notes: None,
                },
            )
            .unwrap();
        }
        create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let mut schedule = DomainSchedule::new();
        let first = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_eq!(first.domain_key, "pixiv.net");
        assert!(
            claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
                .unwrap()
                .is_none()
        );
        let second = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:02Z")
            .unwrap()
            .unwrap();
        assert_ne!(first.site_id, second.site_id);
        assert_eq!(second.domain_key, "pixiv.net");
    }

    #[test]
    fn failure_can_be_retried_and_attempt_is_persisted() {
        let (_directory, store) = store();
        let sub = subscription(&store);
        query(&store, sub, "q", "example.test");
        let run = create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let mut schedule = DomainSchedule::new();
        let claim = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        let transition = fail_query_run(
            &store,
            claim.run_query_id,
            "network",
            "temporary",
            Some("2026-01-01T00:00:03Z"),
            "2026-01-01T00:00:02Z",
        )
        .unwrap();
        assert_eq!(transition.query_state, RunState::Pending);
        assert_eq!(
            get_run(&store, run.run_id).unwrap().unwrap().state,
            RunState::Running
        );

        let mut later = DomainSchedule::new();
        assert!(
            claim_next_query_run(&store, &mut later, "2026-01-01T00:00:02Z")
                .unwrap()
                .is_none()
        );
        let retry = claim_next_query_run(&store, &mut later, "2026-01-01T00:00:03Z")
            .unwrap()
            .unwrap();
        assert_eq!(retry.attempt_count, 2);
    }

    #[test]
    fn terminal_failure_and_cancel_settle_the_parent_run() {
        let (_directory, store) = store();
        let sub = subscription(&store);
        query(&store, sub, "q", "example.test");
        let run = create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let mut schedule = DomainSchedule::new();
        let claim = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        let failed = fail_query_run(
            &store,
            claim.run_query_id,
            "auth",
            "login required",
            None,
            "2026-01-01T00:00:02Z",
        )
        .unwrap();
        assert_eq!(failed.query_state, RunState::Failed);
        assert_eq!(failed.run_state, Some(RunState::Failed));
        assert_eq!(
            get_run(&store, run.run_id).unwrap().unwrap().state,
            RunState::Failed
        );

        retry_query_run(&store, claim.run_query_id, "2026-01-01T00:00:03Z").unwrap();
        let mut retry_schedule = DomainSchedule::new();
        let retry = claim_next_query_run(&store, &mut retry_schedule, "2026-01-01T00:00:03Z")
            .unwrap()
            .unwrap();
        let cancelled =
            cancel_query_run(&store, retry.run_query_id, "2026-01-01T00:00:04Z").unwrap();
        assert_eq!(cancelled.query_state, RunState::Cancelled);
        assert_eq!(cancelled.run_state, Some(RunState::Cancelled));
        assert_eq!(
            get_run(&store, run.run_id).unwrap().unwrap().state,
            RunState::Cancelled
        );
    }

    #[test]
    fn counters_follow_source_item_state_not_claim_count() {
        let (_directory, store) = store();
        let sub = subscription(&store);
        query(&store, sub, "q", "example.test");
        let run = create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let mut schedule = DomainSchedule::new();
        let claim = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            record_post(
                &store,
                claim.run_query_id,
                &post("example.test", "post", 4),
                "2026-01-01T00:00:02Z"
            )
            .unwrap(),
            4
        );
        let ids: Vec<i64> = store
            .read(|connection| {
                let mut statement = connection
                    .prepare("SELECT source_item_id FROM source_item ORDER BY source_item_id")?;
                let ids = statement.query_map([], |row| row.get(0))?.collect();
                ids
            })
            .unwrap();
        update_source_item_state(
            &store,
            ids[0],
            SourceItemState::Downloaded,
            None,
            "2026-01-01T00:00:03Z",
        )
        .unwrap();
        update_source_item_state(
            &store,
            ids[1],
            SourceItemState::Ingested,
            None,
            "2026-01-01T00:00:03Z",
        )
        .unwrap();
        update_source_item_state(
            &store,
            ids[2],
            SourceItemState::Failed,
            Some("bad media"),
            "2026-01-01T00:00:03Z",
        )
        .unwrap();
        update_source_item_state(
            &store,
            ids[3],
            SourceItemState::Deleted,
            None,
            "2026-01-01T00:00:03Z",
        )
        .unwrap();
        assert_eq!(
            source_item_counters(&store, claim.run_query_id).unwrap(),
            SourceItemCounters {
                total: 4,
                pending: 0,
                downloaded: 1,
                ingested: 1,
                failed: 1,
                deleted: 1,
            }
        );
        assert_eq!(
            source_item_counters(&store, claim.run_query_id)
                .unwrap()
                .completed(),
            4
        );
        complete_query_run_with_cursor(
            &store,
            claim.run_query_id,
            Some("next-page"),
            "2026-01-01T00:00:04Z",
        )
        .unwrap();
        assert_eq!(
            get_run(&store, run.run_id).unwrap().unwrap().state,
            RunState::Succeeded
        );
        let query_state = store
            .read(|connection| {
                connection.query_row(
                    "SELECT initial_run_complete, resume_cursor
                     FROM subscription_query WHERE query_id = ?1",
                    [claim.query_id],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, String>(1)?)),
                )
            })
            .unwrap();
        assert_eq!(query_state, (true, "next-page".to_string()));
    }

    #[test]
    fn domain_schedule_delays_same_site_but_not_other_sites() {
        let (_directory, store) = store();
        let sub = subscription(&store);
        query(&store, sub, "a", "same.test");
        query(&store, sub, "b", "other.test");
        create_run(&store, sub, "manual", "2026-01-01T00:00:00Z").unwrap();
        let mut schedule = DomainSchedule::new();
        let first = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        let second = claim_next_query_run(&store, &mut schedule, "2026-01-01T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_ne!(first.site_id, second.site_id);
        assert_eq!(
            schedule.next_allowed_at_ms(&first.site_id),
            Some(parse_timestamp_ms("2026-01-01T00:00:01Z").unwrap() + DOMAIN_INTERVAL_MS)
        );
    }
}
