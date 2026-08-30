pub mod pixiv_oauth;
pub mod sites;
pub mod source_adapter;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Months, Utc};
use rusqlite::{params, OptionalExtension, Transaction};

const DOMAIN_INTERVAL_MS: i64 = 1_000;
pub const DEFAULT_SOURCE_POST_BATCH_SIZE: u32 = 100;

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
    pub group_posts: bool,
    pub requested_by: String,
    pub initial_post_limit: Option<i64>,
    pub periodic_post_limit: Option<i64>,
    pub run_post_limit: Option<u32>,
    pub initial_run_complete: bool,
    pub resume_cursor: Option<String>,
    pub attempt_count: i64,
}

impl ClaimedQueryRun {
    pub fn configured_post_limit(&self) -> u32 {
        let configured = if self.initial_run_complete {
            self.periodic_post_limit.or(self.initial_post_limit)
        } else {
            self.initial_post_limit.or(self.periodic_post_limit)
        };
        configured
            .filter(|value| *value > 0)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(DEFAULT_SOURCE_POST_BATCH_SIZE)
    }

    /// Number of successfully added posts still available to this query run.
    pub fn source_post_batch_size(&self) -> u32 {
        let configured = self.configured_post_limit();
        self.run_post_limit
            .map_or(configured, |remaining| configured.min(remaining))
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

    pub(crate) fn allows(&self, domain_key: &str, now_ms: i64) -> bool {
        self.next_allowed_at_ms(domain_key)
            .is_none_or(|next| next <= now_ms)
    }

    pub(crate) fn mark_started(&mut self, domain_key: String, now_ms: i64) {
        self.next_allowed_at_ms
            .insert(domain_key, now_ms + DOMAIN_INTERVAL_MS);
    }

    pub(crate) fn mark_finished(&mut self, domain_key: String, now_ms: i64) {
        self.next_allowed_at_ms
            .insert(domain_key, now_ms + DOMAIN_INTERVAL_MS);
    }
}

pub(crate) fn create_run_in(
    tx: &Transaction<'_>,
    subscription_id: i64,
    requested_by: &str,
    now: &str,
) -> rusqlite::Result<CreatedRun> {
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
}

pub(crate) fn create_query_run_in(
    tx: &Transaction<'_>,
    query_id: i64,
    requested_by: &str,
    now: &str,
) -> rusqlite::Result<CreatedRun> {
    let subscription_id = tx
        .query_row(
            "SELECT subscription_id FROM subscription_query WHERE query_id = ?1",
            [query_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| sql_error("subscription query does not exist"))?;

    if let Some((run_id, status)) = tx
        .query_row(
            "SELECT run_id, status FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
             ORDER BY run_id LIMIT 1",
            [subscription_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        let already_scheduled = tx.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM subscription_run_query WHERE run_id = ?1 AND query_id = ?2
             )",
            params![run_id, query_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !already_scheduled {
            return Err(sql_error("subscription already has an active run"));
        }
        return Ok(CreatedRun {
            run_id,
            created: false,
            state: parse_run_state(status)?,
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
         ) VALUES (?1, ?2, 'pending', ?3)",
        params![run_id, query_id, now],
    )?;
    Ok(CreatedRun {
        run_id,
        created: true,
        state: RunState::Pending,
    })
}

pub(crate) fn next_schedule_at(schedule: &str, now: &str) -> Result<Option<String>, String> {
    let now = DateTime::parse_from_rfc3339(now)
        .map_err(|error| format!("Invalid schedule timestamp: {error}"))?
        .with_timezone(&Utc);
    let next = match schedule {
        "manual" => return Ok(None),
        "daily" => now + Duration::days(1),
        "weekly" => now + Duration::weeks(1),
        "monthly" => now
            .checked_add_months(Months::new(1))
            .ok_or_else(|| "Monthly schedule overflowed".to_string())?,
        _ => {
            return Err(format!(
                "Invalid schedule: {schedule}. Must be one of: manual, daily, weekly, monthly"
            ))
        }
    };
    Ok(Some(next.to_rfc3339()))
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
        if pending > 0 && running == 0 {
            tx.execute(
                "UPDATE subscription_run SET status = 'pending'
                 WHERE run_id = ?1 AND status = 'running'",
                [run_id],
            )?;
        }
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

pub(crate) fn parse_run_state(value: String) -> rusqlite::Result<RunState> {
    match value.as_str() {
        "pending" => Ok(RunState::Pending),
        "running" => Ok(RunState::Running),
        "succeeded" => Ok(RunState::Succeeded),
        "failed" => Ok(RunState::Failed),
        "cancelled" => Ok(RunState::Cancelled),
        _ => Err(sql_error("invalid persisted run state")),
    }
}

fn sql_error(message: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_query_run_contains_only_the_requested_held_query() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE subscription_query (
                     query_id INTEGER PRIMARY KEY,
                     subscription_id INTEGER NOT NULL,
                     paused INTEGER NOT NULL
                 );
                 CREATE TABLE subscription_run (
                     run_id INTEGER PRIMARY KEY,
                     subscription_id INTEGER NOT NULL,
                     requested_by TEXT NOT NULL,
                     status TEXT NOT NULL,
                     created_at TEXT NOT NULL
                 );
                 CREATE TABLE subscription_run_query (
                     run_query_id INTEGER PRIMARY KEY,
                     run_id INTEGER NOT NULL,
                     query_id INTEGER NOT NULL,
                     status TEXT NOT NULL,
                     available_at TEXT NOT NULL,
                     UNIQUE(run_id, query_id)
                 );
                 INSERT INTO subscription_query(query_id, subscription_id, paused)
                 VALUES (11, 7, 1), (12, 7, 0);",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();

        let run = create_query_run_in(&transaction, 11, "manual-query", "now").unwrap();
        assert!(run.created);
        assert_eq!(run.state, RunState::Pending);
        assert_eq!(
            transaction
                .query_row(
                    "SELECT requested_by FROM subscription_run WHERE run_id = ?1",
                    [run.run_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "manual-query"
        );
        assert_eq!(
            transaction
                .query_row(
                    "SELECT GROUP_CONCAT(query_id) FROM subscription_run_query WHERE run_id = ?1",
                    [run.run_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "11"
        );

        let same = create_query_run_in(&transaction, 11, "manual-query", "later").unwrap();
        assert!(!same.created);
        assert!(create_query_run_in(&transaction, 12, "manual-query", "later").is_err());
    }
}
