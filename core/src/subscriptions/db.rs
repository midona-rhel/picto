//! Subscription + query + file + credential-domain CRUD.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub subscription_id: i64,
    pub name: String,
    pub paused: bool,
    pub group_id: Option<i64>,
    pub initial_post_limit: i64,
    pub periodic_post_limit: i64,
    pub auto_collections: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQuery {
    pub query_id: i64,
    pub subscription_id: i64,
    pub site_id: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub paused: bool,
    pub last_check_time: Option<String>,
    pub files_found: i64,
    pub posts_found: i64,
    pub completed_initial_run: bool,
    pub resume_cursor: Option<String>,
    pub resume_strategy: Option<String>,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_message: Option<String>,
}

fn map_subscription_row(row: &rusqlite::Row) -> rusqlite::Result<Subscription> {
    Ok(Subscription {
        subscription_id: row.get(0)?,
        name: row.get(1)?,
        paused: row.get::<_, i64>(2)? != 0,
        group_id: row.get(3)?,
        initial_post_limit: row.get(4)?,
        periodic_post_limit: row.get(5)?,
        auto_collections: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

const SUB_COLS: &str = "subscription_id, name, paused, group_id, initial_post_limit, periodic_post_limit, auto_collections, created_at";

fn map_query_row(row: &rusqlite::Row) -> rusqlite::Result<SubscriptionQuery> {
    Ok(SubscriptionQuery {
        query_id: row.get(0)?,
        subscription_id: row.get(1)?,
        site_id: row.get(2)?,
        query_text: row.get(3)?,
        display_name: row.get(4)?,
        notes: row.get(5)?,
        paused: row.get::<_, i64>(6)? != 0,
        last_check_time: row.get(7)?,
        files_found: row.get(8)?,
        posts_found: row.get(9)?,
        completed_initial_run: row.get::<_, i64>(10)? != 0,
        resume_cursor: row.get(11)?,
        resume_strategy: row.get(12)?,
        last_success_at: row.get(13)?,
        last_failure_at: row.get(14)?,
        last_failure_kind: row.get(15)?,
        last_failure_message: row.get(16)?,
    })
}

const QUERY_COLS: &str = "query_id, subscription_id, site_id, query_text, display_name, notes, paused, last_check_time, files_found, posts_found, completed_initial_run, resume_cursor, resume_strategy, last_success_at, last_failure_at, last_failure_kind, last_failure_message";

pub fn create_subscription(
    conn: &Connection,
    name: &str,
    group_id: Option<i64>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription (name, group_id, created_at)
         VALUES (?1, ?2, ?3)",
        params![name, group_id, now],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn get_subscription(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Option<Subscription>> {
    conn.query_row(
        &format!("SELECT {SUB_COLS} FROM subscription WHERE subscription_id = ?1"),
        [subscription_id],
        map_subscription_row,
    )
    .optional()
}

pub fn list_subscriptions(conn: &Connection) -> rusqlite::Result<Vec<Subscription>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {SUB_COLS} FROM subscription ORDER BY name"
    ))?;
    let rows = stmt.query_map([], map_subscription_row)?;
    rows.collect()
}

pub fn delete_subscription(conn: &Connection, subscription_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM subscription WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    Ok(())
}

pub fn set_subscription_paused(
    conn: &Connection,
    subscription_id: i64,
    paused: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription SET paused = ?1 WHERE subscription_id = ?2",
        params![paused as i64, subscription_id],
    )?;
    Ok(())
}

pub fn set_subscription_auto_collections(
    conn: &Connection,
    subscription_id: i64,
    auto_collections: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription SET auto_collections = ?1 WHERE subscription_id = ?2",
        params![auto_collections as i64, subscription_id],
    )?;
    Ok(())
}

pub fn add_subscription_query(
    conn: &Connection,
    subscription_id: i64,
    site_id: &str,
    query_text: &str,
    display_name: Option<&str>,
    notes: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO subscription_query (subscription_id, site_id, query_text, display_name, notes) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![subscription_id, site_id, query_text, display_name, notes],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_subscription_queries(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQuery>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {QUERY_COLS} FROM subscription_query WHERE subscription_id = ?1"
    ))?;
    let rows = stmt.query_map([subscription_id], map_query_row)?;
    rows.collect()
}

pub fn get_subscription_query(
    conn: &Connection,
    query_id: i64,
) -> rusqlite::Result<Option<SubscriptionQuery>> {
    conn.query_row(
        &format!("SELECT {QUERY_COLS} FROM subscription_query WHERE query_id = ?1"),
        [query_id],
        map_query_row,
    )
    .optional()
}

pub fn delete_subscription_query(conn: &Connection, query_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM subscription_query WHERE query_id = ?1",
        [query_id],
    )?;
    Ok(())
}

pub fn update_subscription_query(
    conn: &Connection,
    query_id: i64,
    site_id: &str,
    query_text: &str,
    display_name: Option<&str>,
    notes: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET site_id = ?1, query_text = ?2, display_name = ?3, notes = ?4 WHERE query_id = ?5",
        params![site_id, query_text, display_name, notes, query_id],
    )?;
    Ok(())
}

pub fn set_query_paused(conn: &Connection, query_id: i64, paused: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET paused = ?1 WHERE query_id = ?2",
        params![paused as i64, query_id],
    )?;
    Ok(())
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

pub fn reset_query_progress(conn: &Connection, query_id: i64) -> rusqlite::Result<()> {
    conn.execute(
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
    Ok(())
}

pub fn reset_subscription_query_state(
    conn: &Connection,
    query_id: i64,
) -> rusqlite::Result<(usize, usize, usize, usize, usize)> {
    let tx = conn.unchecked_transaction()?;

    let query_reset = tx.execute(
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

    let query_runs_deleted = tx.execute(
        "DELETE FROM subscription_query_run WHERE query_id = ?1",
        [query_id],
    )?;

    let issues_deleted = tx.execute(
        "DELETE FROM subscription_issue WHERE query_id = ?1",
        [query_id],
    )?;

    let attempts_deleted = tx.execute(
        "DELETE FROM subscription_download_attempt WHERE query_id = ?1",
        [query_id],
    )?;

    let queues_deleted = tx.execute(
        "DELETE FROM download_queue WHERE query_id = ?1",
        [query_id],
    )?;

    tx.commit()?;
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
    let tx = conn.unchecked_transaction()?;

    let queries_reset = tx.execute(
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

    let entities_deleted = tx.execute(
        "DELETE FROM subscription_entity WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    let post_maps_deleted = tx.execute(
        "DELETE FROM subscription_post_collection WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    tx.commit()?;
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

pub fn get_subscription_entity_ids(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn
        .prepare_cached("SELECT entity_id FROM subscription_entity WHERE subscription_id = ?1")?;
    let rows = stmt.query_map([subscription_id], |row| row.get(0))?;
    rows.collect()
}

pub fn rename_subscription(
    conn: &Connection,
    subscription_id: i64,
    name: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription SET name = ?1 WHERE subscription_id = ?2",
        params![name, subscription_id],
    )?;
    Ok(())
}

pub fn get_subscription_entity_count(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM subscription_entity WHERE subscription_id = ?1",
        [subscription_id],
        |row| row.get(0),
    )
}

/// All subscriptions with aggregated file counts in a single query.
pub fn list_subscriptions_with_file_counts(
    conn: &Connection,
) -> rusqlite::Result<Vec<(Subscription, i64)>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT s.{}, COALESCE(fc.cnt, 0)
             FROM subscription s
             LEFT JOIN (
                 SELECT subscription_id, COUNT(*) AS cnt
                 FROM subscription_entity GROUP BY subscription_id
             ) fc ON fc.subscription_id = s.subscription_id
             ORDER BY s.name",
        SUB_COLS.replace(", ", ", s.")
    ))?;
    let rows = stmt.query_map([], |row| {
        Ok((map_subscription_row(row)?, row.get::<_, i64>(8)?))
    })?;
    rows.collect()
}

/// All subscription queries in a single query (no filter — caller groups by subscription_id).
pub fn list_all_subscription_queries(
    conn: &Connection,
) -> rusqlite::Result<Vec<SubscriptionQuery>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {QUERY_COLS} FROM subscription_query ORDER BY subscription_id, query_id"
    ))?;
    let rows = stmt.query_map([], map_query_row)?;
    rows.collect()
}

/// Subscriptions for a given group with aggregated file counts — single query.
pub fn list_subscriptions_for_group_with_file_counts(
    conn: &Connection,
    group_id: i64,
) -> rusqlite::Result<Vec<(Subscription, i64)>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT s.{}, COALESCE(fc.cnt, 0)
             FROM subscription s
             LEFT JOIN (
                 SELECT subscription_id, COUNT(*) AS cnt
                 FROM subscription_entity GROUP BY subscription_id
             ) fc ON fc.subscription_id = s.subscription_id
             WHERE s.group_id = ?1
             ORDER BY s.name",
        SUB_COLS.replace(", ", ", s.")
    ))?;
    let rows = stmt.query_map([group_id], |row| {
        Ok((map_subscription_row(row)?, row.get::<_, i64>(8)?))
    })?;
    rows.collect()
}

/// All subscription queries belonging to a group — single query.
pub fn list_subscription_queries_for_group(
    conn: &Connection,
    group_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQuery>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT sq.{}
             FROM subscription_query sq
             INNER JOIN subscription s ON s.subscription_id = sq.subscription_id
             WHERE s.group_id = ?1
             ORDER BY sq.subscription_id, sq.query_id",
        QUERY_COLS.replace(", ", ", sq.")
    ))?;
    let rows = stmt.query_map([group_id], map_query_row)?;
    rows.collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionRunRecord {
    pub run_id: i64,
    pub subscription_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub files_downloaded: i64,
    pub files_skipped: i64,
    pub metadata_validated: i64,
    pub metadata_invalid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQueryRunRecord {
    pub query_run_id: i64,
    pub run_id: Option<i64>,
    pub subscription_id: i64,
    pub query_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub posts_processed: i64,
    pub files_downloaded: i64,
    pub files_skipped: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionIssueRecord {
    pub issue_id: i64,
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub issue_kind: String,
    pub status: String,
    pub message: String,
    pub detail: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDownloadAttemptRecord {
    pub attempt_id: i64,
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub item_key: String,
    pub site_category: Option<String>,
    pub post_id: Option<String>,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub retry_url: Option<String>,
    pub retry_count: i64,
    pub status: String,
    pub failure_kind: Option<String>,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPostMemberRecord {
    pub subscription_id: i64,
    pub site_id: String,
    pub post_id: String,
    pub item_key: String,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub entity_hash: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

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

fn map_subscription_issue_row(
    row: &rusqlite::Row,
) -> rusqlite::Result<SubscriptionIssueRecord> {
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

pub struct SubscriptionDownloadAttemptUpsert<'a> {
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub item_key: &'a str,
    pub site_category: Option<&'a str>,
    pub post_id: Option<&'a str>,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<&'a str>,
    pub media_url: Option<&'a str>,
    pub retry_url: Option<&'a str>,
    pub failure_kind: Option<&'a str>,
    pub last_error: Option<&'a str>,
    pub next_retry_at: Option<&'a str>,
}

pub struct SubscriptionPostMemberUpsert<'a> {
    pub subscription_id: i64,
    pub site_id: &'a str,
    pub post_id: &'a str,
    pub item_key: &'a str,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<&'a str>,
    pub media_url: Option<&'a str>,
    pub entity_hash: Option<&'a str>,
    pub status: &'a str,
}

#[derive(Debug, Clone)]
pub struct OwnedSubscriptionPostMemberUpsert {
    pub subscription_id: i64,
    pub site_id: String,
    pub post_id: String,
    pub item_key: String,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub entity_hash: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct OwnedSubscriptionDownloadAttemptUpsert {
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub item_key: String,
    pub site_category: Option<String>,
    pub post_id: Option<String>,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub retry_url: Option<String>,
    pub failure_kind: Option<String>,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialDomain {
    pub site_category: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialHealth {
    pub site_category: String,
    pub health_status: String,
    pub last_checked_at: String,
    pub last_error: Option<String>,
}

pub fn upsert_credential_domain(
    conn: &Connection,
    site_category: &str,
    credential_type: &str,
    display_name: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO credential_domain (site_category, credential_type, display_name, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(site_category) DO UPDATE SET credential_type = ?2, display_name = ?3",
        params![site_category, credential_type, display_name, now],
    )?;
    Ok(())
}

pub fn list_credential_domains(conn: &Connection) -> rusqlite::Result<Vec<CredentialDomain>> {
    let mut stmt = conn.prepare_cached(
        "SELECT site_category, credential_type, display_name, created_at
         FROM credential_domain ORDER BY site_category",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CredentialDomain {
            site_category: row.get(0)?,
            credential_type: row.get(1)?,
            display_name: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_credential_domain(conn: &Connection, site_category: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM credential_domain WHERE site_category = ?1",
        [site_category],
    )?;
    Ok(())
}

pub fn upsert_credential_health(
    conn: &Connection,
    site_category: &str,
    health_status: &str,
    last_error: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(site_category)
         DO UPDATE SET health_status = excluded.health_status,
                       last_checked_at = excluded.last_checked_at,
                       last_error = excluded.last_error",
        params![site_category, health_status, now, last_error],
    )?;
    Ok(())
}

pub fn list_credential_health(conn: &Connection) -> rusqlite::Result<Vec<CredentialHealth>> {
    let mut stmt = conn.prepare_cached(
        "SELECT site_category, health_status, last_checked_at, last_error
         FROM credential_health
         ORDER BY site_category",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CredentialHealth {
            site_category: row.get(0)?,
            health_status: row.get(1)?,
            last_checked_at: row.get(2)?,
            last_error: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_credential_health(conn: &Connection, site_category: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM credential_health WHERE site_category = ?1",
        [site_category],
    )?;
    Ok(())
}

impl SqliteDatabase {
    pub async fn create_subscription(
        &self,
        name: &str,
        group_id: Option<i64>,
    ) -> Result<Subscription, String> {
        let n = name.to_string();
        let sub_id = self
            .with_conn(move |conn| create_subscription(conn, &n, group_id))
            .await?;
        let sid = sub_id;
        self.with_read_conn(move |conn| get_subscription(conn, sid))
            .await?
            .ok_or_else(|| "Subscription not found after creation".to_string())
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>, String> {
        self.with_read_conn(list_subscriptions).await
    }

    pub async fn delete_subscription(&self, subscription_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_subscription(conn, subscription_id))
            .await
    }

    pub async fn set_subscription_paused(
        &self,
        subscription_id: i64,
        paused: bool,
    ) -> Result<(), String> {
        self.with_conn(move |conn| set_subscription_paused(conn, subscription_id, paused))
            .await
    }

    pub async fn set_subscription_auto_collections(
        &self,
        subscription_id: i64,
        auto_collections: bool,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            set_subscription_auto_collections(conn, subscription_id, auto_collections)
        })
        .await
    }

    pub async fn add_subscription_query(
        &self,
        subscription_id: i64,
        site_id: &str,
        query_text: &str,
        display_name: Option<&str>,
        notes: Option<&str>,
    ) -> Result<SubscriptionQuery, String> {
        let site = site_id.to_string();
        let site_for_db = site.clone();
        let qt = query_text.to_string();
        let dn = display_name.map(|s| s.to_string());
        let notes_for_db = notes.map(|s| s.to_string());
        let qid = self
            .with_conn(move |conn| {
                add_subscription_query(
                    conn,
                    subscription_id,
                    &site_for_db,
                    &qt,
                    dn.as_deref(),
                    notes_for_db.as_deref(),
                )
            })
            .await?;
        Ok(SubscriptionQuery {
            query_id: qid,
            subscription_id,
            site_id: site,
            query_text: query_text.to_string(),
            display_name: display_name.map(|s| s.to_string()),
            notes: notes.map(|s| s.to_string()),
            paused: false,
            last_check_time: None,
            files_found: 0,
            posts_found: 0,
            completed_initial_run: false,
            resume_cursor: None,
            resume_strategy: None,
            last_success_at: None,
            last_failure_at: None,
            last_failure_kind: None,
            last_failure_message: None,
        })
    }

    pub async fn get_subscription_queries(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<SubscriptionQuery>, String> {
        self.with_read_conn(move |conn| get_subscription_queries(conn, subscription_id))
            .await
    }

    pub async fn delete_subscription_query(&self, query_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_subscription_query(conn, query_id))
            .await
    }

    pub async fn update_subscription_query(
        &self,
        query_id: i64,
        site_id: String,
        query_text: String,
        display_name: Option<String>,
        notes: Option<String>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            update_subscription_query(
                conn,
                query_id,
                &site_id,
                &query_text,
                display_name.as_deref(),
                notes.as_deref(),
            )
        })
        .await
    }

    pub async fn set_query_paused(&self, query_id: i64, paused: bool) -> Result<(), String> {
        self.with_conn(move |conn| set_query_paused(conn, query_id, paused))
            .await
    }

    pub async fn update_query_progress(
        &self,
        query_id: i64,
        last_check_time: &str,
        files_found: i64,
        posts_found: i64,
    ) -> Result<(), String> {
        let lct = last_check_time.to_string();
        self.with_conn(move |conn| {
            update_query_progress(conn, query_id, &lct, files_found, posts_found)
        })
        .await
    }

    pub async fn get_subscription_query(
        &self,
        query_id: i64,
    ) -> Result<Option<SubscriptionQuery>, String> {
        self.with_read_conn(move |conn| get_subscription_query(conn, query_id))
            .await
    }

    pub async fn reset_query_progress(&self, query_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| reset_query_progress(conn, query_id))
            .await
    }

    pub async fn reset_subscription_query_state(
        &self,
        query_id: i64,
    ) -> Result<(usize, usize, usize, usize, usize), String> {
        self.with_conn(move |conn| reset_subscription_query_state(conn, query_id))
            .await
    }

    pub async fn reset_subscription_state(
        &self,
        subscription_id: i64,
    ) -> Result<(usize, usize, usize), String> {
        self.with_conn(move |conn| reset_subscription_state(conn, subscription_id))
            .await
    }

    pub async fn set_query_completed_initial_run(
        &self,
        query_id: i64,
        completed: bool,
    ) -> Result<(), String> {
        self.with_conn(move |conn| set_query_completed_initial_run(conn, query_id, completed))
            .await
    }

    pub async fn set_query_resume_state(
        &self,
        query_id: i64,
        resume_cursor: Option<String>,
        resume_strategy: Option<String>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            set_query_resume_state(
                conn,
                query_id,
                resume_cursor.as_deref(),
                resume_strategy.as_deref(),
            )
        })
        .await
    }

    pub async fn set_query_terminal_state(
        &self,
        query_id: i64,
        last_success_at: Option<String>,
        last_failure_at: Option<String>,
        last_failure_kind: Option<String>,
        last_failure_message: Option<String>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            set_query_terminal_state(
                conn,
                query_id,
                last_success_at.as_deref(),
                last_failure_at.as_deref(),
                last_failure_kind.as_deref(),
                last_failure_message.as_deref(),
            )
        })
        .await
    }

    pub async fn add_subscription_entity(
        &self,
        subscription_id: i64,
        hash: &str,
    ) -> Result<bool, String> {
        let entity_id = self.resolve_hash(hash).await?;
        self.with_conn(move |conn| add_subscription_entity(conn, subscription_id, entity_id))
            .await
    }

    pub async fn upsert_subscription_post_collection(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
        collection_entity_id: i64,
    ) -> Result<(), String> {
        let site = site_id.to_string();
        let post = post_id.to_string();
        self.with_conn(move |conn| {
            upsert_subscription_post_collection(
                conn,
                subscription_id,
                &site,
                &post,
                collection_entity_id,
            )
        })
        .await
    }

    pub async fn get_subscription_post_collection(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
    ) -> Result<Option<i64>, String> {
        let site = site_id.to_string();
        let post = post_id.to_string();
        self.with_read_conn(move |conn| {
            get_subscription_post_collection(conn, subscription_id, &site, &post)
        })
        .await
    }

    pub async fn rename_subscription(
        &self,
        subscription_id: i64,
        name: &str,
    ) -> Result<(), String> {
        let n = name.to_string();
        self.with_conn(move |conn| rename_subscription(conn, subscription_id, &n))
            .await
    }

    pub async fn get_subscription_entity_count(&self, subscription_id: i64) -> Result<i64, String> {
        self.with_read_conn(move |conn| get_subscription_entity_count(conn, subscription_id))
            .await
    }

    pub async fn list_subscriptions_with_file_counts(
        &self,
    ) -> Result<Vec<(Subscription, i64)>, String> {
        self.with_read_conn(list_subscriptions_with_file_counts)
            .await
    }

    pub async fn list_all_subscription_queries(&self) -> Result<Vec<SubscriptionQuery>, String> {
        self.with_read_conn(list_all_subscription_queries).await
    }

    pub async fn list_subscriptions_for_group_with_file_counts(
        &self,
        group_id: i64,
    ) -> Result<Vec<(Subscription, i64)>, String> {
        self.with_read_conn(move |conn| {
            list_subscriptions_for_group_with_file_counts(conn, group_id)
        })
        .await
    }

    pub async fn list_subscription_queries_for_group(
        &self,
        group_id: i64,
    ) -> Result<Vec<SubscriptionQuery>, String> {
        self.with_read_conn(move |conn| list_subscription_queries_for_group(conn, group_id))
            .await
    }

    pub async fn create_subscription_run(&self, subscription_id: i64) -> Result<i64, String> {
        self.with_conn(move |conn| create_subscription_run(conn, subscription_id))
            .await
    }

    pub async fn finish_subscription_run(
        &self,
        run_id: i64,
        status: &str,
        failure_kind: Option<String>,
        error_message: Option<String>,
        files_downloaded: i64,
        files_skipped: i64,
        metadata_validated: i64,
        metadata_invalid: i64,
    ) -> Result<(), String> {
        let status = status.to_string();
        self.with_conn(move |conn| {
            finish_subscription_run(
                conn,
                run_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
                files_downloaded,
                files_skipped,
                metadata_validated,
                metadata_invalid,
            )
        })
        .await
    }

    pub async fn list_subscription_runs(
        &self,
        subscription_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionRunRecord>, String> {
        self.with_read_conn(move |conn| list_subscription_runs(conn, subscription_id, limit))
            .await
    }

    pub async fn create_subscription_query_run(
        &self,
        run_id: Option<i64>,
        subscription_id: i64,
        query_id: i64,
    ) -> Result<i64, String> {
        self.with_conn(move |conn| create_subscription_query_run(conn, run_id, subscription_id, query_id))
            .await
    }

    pub async fn finish_subscription_query_run(
        &self,
        query_run_id: i64,
        status: &str,
        failure_kind: Option<String>,
        error_message: Option<String>,
        posts_processed: i64,
        files_downloaded: i64,
        files_skipped: i64,
    ) -> Result<(), String> {
        let status = status.to_string();
        self.with_conn(move |conn| {
            finish_subscription_query_run(
                conn,
                query_run_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
                posts_processed,
                files_downloaded,
                files_skipped,
            )
        })
        .await
    }

    pub async fn list_subscription_query_runs(
        &self,
        query_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionQueryRunRecord>, String> {
        self.with_read_conn(move |conn| list_subscription_query_runs(conn, query_id, limit))
            .await
    }

    pub async fn upsert_subscription_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        issue_kind: &str,
        message: &str,
        detail: Option<&str>,
    ) -> Result<i64, String> {
        let issue_kind = issue_kind.to_string();
        let message = message.to_string();
        let detail = detail.map(|s| s.to_string());
        self.with_conn(move |conn| {
            upsert_subscription_issue(
                conn,
                subscription_id,
                query_id,
                &issue_kind,
                &message,
                detail.as_deref(),
            )
        })
        .await
    }

    pub async fn resolve_subscription_issues(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        issue_kind: &str,
    ) -> Result<(), String> {
        let issue_kind = issue_kind.to_string();
        self.with_conn(move |conn| {
            resolve_subscription_issues(conn, subscription_id, query_id, &issue_kind)
        })
        .await
    }

    pub async fn list_subscription_issues(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SubscriptionIssueRecord>, String> {
        self.with_read_conn(move |conn| {
            list_subscription_issues(conn, subscription_id, query_id, limit)
        })
        .await
    }

    pub async fn upsert_subscription_download_attempt(
        &self,
        input: OwnedSubscriptionDownloadAttemptUpsert,
    ) -> Result<i64, String> {
        self.with_conn(move |conn| {
            upsert_subscription_download_attempt(
                conn,
                SubscriptionDownloadAttemptUpsert {
                    subscription_id: input.subscription_id,
                    query_id: input.query_id,
                    query_run_id: input.query_run_id,
                    item_key: &input.item_key,
                    site_category: input.site_category.as_deref(),
                    post_id: input.post_id.as_deref(),
                    page_num: input.page_num,
                    canonical_post_url: input.canonical_post_url.as_deref(),
                    media_url: input.media_url.as_deref(),
                    retry_url: input.retry_url.as_deref(),
                    failure_kind: input.failure_kind.as_deref(),
                    last_error: input.last_error.as_deref(),
                    next_retry_at: input.next_retry_at.as_deref(),
                },
            )
        })
            .await
    }

    pub async fn resolve_subscription_download_attempt(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        item_key: &str,
    ) -> Result<(), String> {
        let item_key = item_key.to_string();
        self.with_conn(move |conn| {
            resolve_subscription_download_attempt(conn, subscription_id, query_id, &item_key)
        })
        .await
    }

    pub async fn mark_subscription_download_attempt_retrying(
        &self,
        attempt_id: i64,
    ) -> Result<(), String> {
        self.with_conn(move |conn| mark_subscription_download_attempt_retrying(conn, attempt_id))
            .await
    }

    pub async fn list_retryable_subscription_download_attempts(
        &self,
        subscription_id: i64,
        query_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionDownloadAttemptRecord>, String> {
        self.with_read_conn(move |conn| {
            list_retryable_subscription_download_attempts(conn, subscription_id, query_id, limit)
        })
        .await
    }

    pub async fn list_subscription_download_attempts(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SubscriptionDownloadAttemptRecord>, String> {
        self.with_read_conn(move |conn| {
            list_subscription_download_attempts(conn, subscription_id, query_id, limit)
        })
        .await
    }

    pub async fn upsert_subscription_post_member(
        &self,
        input: OwnedSubscriptionPostMemberUpsert,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            upsert_subscription_post_member(
                conn,
                SubscriptionPostMemberUpsert {
                    subscription_id: input.subscription_id,
                    site_id: &input.site_id,
                    post_id: &input.post_id,
                    item_key: &input.item_key,
                    page_num: input.page_num,
                    canonical_post_url: input.canonical_post_url.as_deref(),
                    media_url: input.media_url.as_deref(),
                    entity_hash: input.entity_hash.as_deref(),
                    status: &input.status,
                },
            )
        })
        .await
    }

    pub async fn list_subscription_post_members(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
    ) -> Result<Vec<SubscriptionPostMemberRecord>, String> {
        let site_id = site_id.to_string();
        let post_id = post_id.to_string();
        self.with_read_conn(move |conn| {
            list_subscription_post_members(conn, subscription_id, &site_id, &post_id)
        })
        .await
    }

    pub async fn upsert_credential_domain(
        &self,
        site_category: &str,
        credential_type: &str,
        display_name: Option<&str>,
    ) -> Result<(), String> {
        let sc = site_category.to_string();
        let ct = credential_type.to_string();
        let dn = display_name.map(|s| s.to_string());
        self.with_conn(move |conn| upsert_credential_domain(conn, &sc, &ct, dn.as_deref()))
            .await
    }

    pub async fn list_credential_domains(&self) -> Result<Vec<CredentialDomain>, String> {
        self.with_read_conn(list_credential_domains).await
    }

    pub async fn delete_credential_domain(&self, site_category: &str) -> Result<(), String> {
        let sc = site_category.to_string();
        self.with_conn(move |conn| delete_credential_domain(conn, &sc))
            .await
    }

    pub async fn upsert_credential_health(
        &self,
        site_category: &str,
        health_status: &str,
        last_error: Option<&str>,
    ) -> Result<(), String> {
        let sc = site_category.to_string();
        let hs = health_status.to_string();
        let le = last_error.map(|s| s.to_string());
        self.with_conn(move |conn| upsert_credential_health(conn, &sc, &hs, le.as_deref()))
            .await
    }

    pub async fn list_credential_health(&self) -> Result<Vec<CredentialHealth>, String> {
        self.with_read_conn(list_credential_health).await
    }

    pub async fn delete_credential_health(&self, site_category: &str) -> Result<(), String> {
        let sc = site_category.to_string();
        self.with_conn(move |conn| delete_credential_health(conn, &sc))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_subscription_query, create_subscription, get_subscription_query,
        list_subscriptions_for_group_with_file_counts, list_subscriptions_with_file_counts,
        reset_subscription_query_state, set_query_completed_initial_run,
        set_query_resume_state, set_query_terminal_state, update_query_progress,
    };
    use rusqlite::{params, Connection};

    #[test]
    fn create_subscription_inserts_without_site_id() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE subscription (
                 subscription_id         INTEGER PRIMARY KEY,
                 name                    TEXT NOT NULL,
                 paused                  INTEGER NOT NULL DEFAULT 0,
                 group_id                INTEGER,
                 initial_post_limit      INTEGER NOT NULL DEFAULT 100,
                 periodic_post_limit     INTEGER NOT NULL DEFAULT 50,
                 auto_collections        INTEGER NOT NULL DEFAULT 1,
                 created_at              TEXT NOT NULL
             );",
        )
        .expect("create subscription table");

        let sub_id = create_subscription(&conn, "Test", None).expect("insert subscription");

        let name: String = conn
            .query_row(
                "SELECT name FROM subscription WHERE subscription_id = ?1",
                [sub_id],
                |row| row.get(0),
            )
            .expect("read inserted subscription");

        assert_eq!(name, "Test");
    }

    #[test]
    fn list_subscriptions_with_file_counts_reads_new_subscription_shape() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE subscription (
                 subscription_id         INTEGER PRIMARY KEY,
                 name                    TEXT NOT NULL,
                 paused                  INTEGER NOT NULL DEFAULT 0,
                 group_id                INTEGER,
                 initial_post_limit      INTEGER NOT NULL DEFAULT 100,
                 periodic_post_limit     INTEGER NOT NULL DEFAULT 50,
                 auto_collections        INTEGER NOT NULL DEFAULT 1,
                 created_at              TEXT NOT NULL
             );
             CREATE TABLE subscription_entity (
                 subscription_id INTEGER NOT NULL,
                 entity_id       INTEGER NOT NULL,
                 PRIMARY KEY (subscription_id, entity_id)
             );",
        )
        .expect("create subscription tables");

        let first_id = create_subscription(&conn, "Alpha", None).expect("insert alpha");
        let second_id = create_subscription(&conn, "Beta", Some(7)).expect("insert beta");
        conn.execute(
            "INSERT INTO subscription_entity (subscription_id, entity_id) VALUES (?1, ?2), (?1, ?3)",
            params![second_id, 100_i64, 101_i64],
        )
        .expect("insert subscription entities");

        let subs = list_subscriptions_with_file_counts(&conn).expect("list subscriptions");

        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].0.subscription_id, first_id);
        assert_eq!(subs[0].0.name, "Alpha");
        assert_eq!(subs[0].1, 0);
        assert_eq!(subs[1].0.subscription_id, second_id);
        assert_eq!(subs[1].0.group_id, Some(7));
        assert_eq!(subs[1].1, 2);
    }

    #[test]
    fn list_subscriptions_for_group_with_file_counts_reads_new_subscription_shape() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE subscription (
                 subscription_id         INTEGER PRIMARY KEY,
                 name                    TEXT NOT NULL,
                 paused                  INTEGER NOT NULL DEFAULT 0,
                 group_id                INTEGER,
                 initial_post_limit      INTEGER NOT NULL DEFAULT 100,
                 periodic_post_limit     INTEGER NOT NULL DEFAULT 50,
                 auto_collections        INTEGER NOT NULL DEFAULT 1,
                 created_at              TEXT NOT NULL
             );
             CREATE TABLE subscription_entity (
                 subscription_id INTEGER NOT NULL,
                 entity_id       INTEGER NOT NULL,
                 PRIMARY KEY (subscription_id, entity_id)
             );",
        )
        .expect("create subscription tables");

        let group_id = 11_i64;
        let in_group = create_subscription(&conn, "Grouped", Some(group_id)).expect("insert grouped");
        let _other = create_subscription(&conn, "Other", Some(99)).expect("insert other");
        conn.execute(
            "INSERT INTO subscription_entity (subscription_id, entity_id) VALUES (?1, ?2)",
            params![in_group, 500_i64],
        )
        .expect("insert grouped entity");

        let subs = list_subscriptions_for_group_with_file_counts(&conn, group_id)
            .expect("list group subscriptions");

        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].0.subscription_id, in_group);
        assert_eq!(subs[0].0.name, "Grouped");
        assert_eq!(subs[0].0.group_id, Some(group_id));
        assert_eq!(subs[0].1, 1);
    }

    #[test]
    fn subscription_query_notes_and_terminal_state_round_trip() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE subscription (
                 subscription_id         INTEGER PRIMARY KEY,
                 name                    TEXT NOT NULL,
                 paused                  INTEGER NOT NULL DEFAULT 0,
                 group_id                INTEGER,
                 initial_post_limit      INTEGER NOT NULL DEFAULT 100,
                 periodic_post_limit     INTEGER NOT NULL DEFAULT 50,
                 auto_collections        INTEGER NOT NULL DEFAULT 1,
                 created_at              TEXT NOT NULL
             );
             CREATE TABLE subscription_query (
                 query_id              INTEGER PRIMARY KEY,
                 subscription_id       INTEGER NOT NULL,
                 site_id               TEXT NOT NULL,
                 query_text            TEXT NOT NULL,
                 display_name          TEXT,
                 notes                 TEXT,
                 paused                INTEGER NOT NULL DEFAULT 0,
                 last_check_time       TEXT,
                 files_found           INTEGER NOT NULL DEFAULT 0,
                 posts_found           INTEGER NOT NULL DEFAULT 0,
                 completed_initial_run INTEGER NOT NULL DEFAULT 0,
                 resume_cursor         TEXT,
                 resume_strategy       TEXT,
                 last_success_at       TEXT,
                 last_failure_at       TEXT,
                 last_failure_kind     TEXT,
                 last_failure_message  TEXT
             );",
        )
        .expect("create query tables");

        let sub_id = create_subscription(&conn, "Queries", None).expect("insert subscription");
        let query_id = add_subscription_query(
            &conn,
            sub_id,
            "gelbooru",
            "princess_peach dress",
            Some("Peach"),
            Some("Safe baseline query"),
        )
        .expect("insert query");
        set_query_terminal_state(
            &conn,
            query_id,
            Some("2026-03-30T10:00:00Z"),
            Some("2026-03-30T11:00:00Z"),
            Some("download_error"),
            Some("failed to fetch one member"),
        )
        .expect("set terminal state");

        let query = get_subscription_query(&conn, query_id)
            .expect("read query")
            .expect("query exists");
        assert_eq!(query.notes.as_deref(), Some("Safe baseline query"));
        assert_eq!(query.last_success_at.as_deref(), Some("2026-03-30T10:00:00Z"));
        assert_eq!(query.last_failure_kind.as_deref(), Some("download_error"));
        assert_eq!(
            query.last_failure_message.as_deref(),
            Some("failed to fetch one member")
        );
    }

    #[test]
    fn reset_subscription_query_state_clears_query_owned_runtime_rows() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE subscription (
                 subscription_id         INTEGER PRIMARY KEY,
                 name                    TEXT NOT NULL,
                 paused                  INTEGER NOT NULL DEFAULT 0,
                 group_id                INTEGER,
                 initial_post_limit      INTEGER NOT NULL DEFAULT 100,
                 periodic_post_limit     INTEGER NOT NULL DEFAULT 50,
                 auto_collections        INTEGER NOT NULL DEFAULT 1,
                 created_at              TEXT NOT NULL
             );
             CREATE TABLE subscription_query (
                 query_id              INTEGER PRIMARY KEY,
                 subscription_id       INTEGER NOT NULL,
                 site_id               TEXT NOT NULL,
                 query_text            TEXT NOT NULL,
                 display_name          TEXT,
                 notes                 TEXT,
                 paused                INTEGER NOT NULL DEFAULT 0,
                 last_check_time       TEXT,
                 files_found           INTEGER NOT NULL DEFAULT 0,
                 posts_found           INTEGER NOT NULL DEFAULT 0,
                 completed_initial_run INTEGER NOT NULL DEFAULT 0,
                 resume_cursor         TEXT,
                 resume_strategy       TEXT,
                 last_success_at       TEXT,
                 last_failure_at       TEXT,
                 last_failure_kind     TEXT,
                 last_failure_message  TEXT
             );
             CREATE TABLE subscription_query_run (
                 query_run_id         INTEGER PRIMARY KEY,
                 run_id               INTEGER,
                 subscription_id      INTEGER NOT NULL,
                 query_id             INTEGER NOT NULL,
                 started_at           TEXT NOT NULL,
                 finished_at          TEXT,
                 status               TEXT NOT NULL DEFAULT 'running',
                 failure_kind         TEXT,
                 error_message        TEXT,
                 posts_processed      INTEGER NOT NULL DEFAULT 0,
                 files_downloaded     INTEGER NOT NULL DEFAULT 0,
                 files_skipped        INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE subscription_issue (
                 issue_id             INTEGER PRIMARY KEY,
                 subscription_id      INTEGER NOT NULL,
                 query_id             INTEGER,
                 issue_kind           TEXT NOT NULL,
                 status               TEXT NOT NULL DEFAULT 'open',
                 message              TEXT NOT NULL,
                 detail               TEXT,
                 first_seen_at        TEXT NOT NULL,
                 last_seen_at         TEXT NOT NULL,
                 resolved_at          TEXT
             );
             CREATE TABLE subscription_download_attempt (
                 attempt_id           INTEGER PRIMARY KEY,
                 subscription_id      INTEGER NOT NULL,
                 query_id             INTEGER,
                 query_run_id         INTEGER,
                 item_key             TEXT NOT NULL,
                 site_category        TEXT,
                 post_id              TEXT,
                 page_num             INTEGER,
                 canonical_post_url   TEXT,
                 media_url            TEXT,
                 retry_url            TEXT,
                 retry_count          INTEGER NOT NULL DEFAULT 0,
                 status               TEXT NOT NULL DEFAULT 'pending',
                 failure_kind         TEXT,
                 last_error           TEXT,
                 next_retry_at        TEXT,
                 created_at           TEXT NOT NULL,
                 updated_at           TEXT NOT NULL,
                 resolved_at          TEXT
             );
             CREATE TABLE download_queue (
                 queue_id        INTEGER PRIMARY KEY,
                 subscription_id INTEGER NOT NULL,
                 query_id        INTEGER,
                 post_id         TEXT NOT NULL,
                 category        TEXT NOT NULL,
                 preferred_name  TEXT,
                 expected_count  INTEGER,
                 status          TEXT NOT NULL DEFAULT 'pending',
                 created_at      TEXT NOT NULL,
                 updated_at      TEXT NOT NULL
             );",
        )
        .expect("create tables");

        let sub_id = create_subscription(&conn, "Reset", None).expect("subscription");
        let query_id = add_subscription_query(
            &conn,
            sub_id,
            "gelbooru",
            "peach",
            Some("Peach"),
            Some("notes"),
        )
        .expect("query");
        update_query_progress(&conn, query_id, "2026-03-30T10:00:00Z", 12, 4)
            .expect("progress");
        set_query_resume_state(&conn, query_id, Some("123"), Some("tag_id_lt"))
            .expect("resume");
        set_query_completed_initial_run(&conn, query_id, true).expect("completed");
        set_query_terminal_state(
            &conn,
            query_id,
            Some("2026-03-30T10:00:00Z"),
            Some("2026-03-30T11:00:00Z"),
            Some("network"),
            Some("timed out"),
        )
        .expect("terminal");
        conn.execute(
            "INSERT INTO subscription_query_run (subscription_id, query_id, started_at)
             VALUES (?1, ?2, '2026-03-30T10:00:00Z')",
            params![sub_id, query_id],
        )
        .expect("query run");
        conn.execute(
            "INSERT INTO subscription_issue (
                 subscription_id, query_id, issue_kind, message, first_seen_at, last_seen_at
             ) VALUES (?1, ?2, 'network', 'oops', '2026-03-30T10:00:00Z', '2026-03-30T10:00:00Z')",
            params![sub_id, query_id],
        )
        .expect("issue");
        conn.execute(
            "INSERT INTO subscription_download_attempt (
                 subscription_id, query_id, item_key, created_at, updated_at
             ) VALUES (?1, ?2, 'gelbooru:1:0', '2026-03-30T10:00:00Z', '2026-03-30T10:00:00Z')",
            params![sub_id, query_id],
        )
        .expect("attempt");
        conn.execute(
            "INSERT INTO download_queue (
                 subscription_id, query_id, post_id, category, created_at, updated_at
             ) VALUES (?1, ?2, '1', 'gelbooru', '2026-03-30T10:00:00Z', '2026-03-30T10:00:00Z')",
            params![sub_id, query_id],
        )
        .expect("queue");

        let counts = reset_subscription_query_state(&conn, query_id).expect("reset query");
        assert_eq!(counts.0, 1);
        assert_eq!(counts.1, 1);
        assert_eq!(counts.2, 1);
        assert_eq!(counts.3, 1);
        assert_eq!(counts.4, 1);

        let query = get_subscription_query(&conn, query_id)
            .expect("read query")
            .expect("query exists");
        assert_eq!(query.files_found, 0);
        assert_eq!(query.posts_found, 0);
        assert!(!query.completed_initial_run);
        assert!(query.last_check_time.is_none());
        assert!(query.resume_cursor.is_none());
        assert!(query.resume_strategy.is_none());
        assert!(query.last_success_at.is_none());
        assert!(query.last_failure_at.is_none());
        assert!(query.last_failure_kind.is_none());
        assert!(query.last_failure_message.is_none());

        let remaining_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM subscription_query_run WHERE query_id = ?1",
                [query_id],
                |row| row.get(0),
            )
            .expect("count runs");
        let remaining_issues: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM subscription_issue WHERE query_id = ?1",
                [query_id],
                |row| row.get(0),
            )
            .expect("count issues");
        let remaining_attempts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM subscription_download_attempt WHERE query_id = ?1",
                [query_id],
                |row| row.get(0),
            )
            .expect("count attempts");
        let remaining_queues: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM download_queue WHERE query_id = ?1",
                [query_id],
                |row| row.get(0),
            )
            .expect("count queues");
        assert_eq!(remaining_runs, 0);
        assert_eq!(remaining_issues, 0);
        assert_eq!(remaining_attempts, 0);
        assert_eq!(remaining_queues, 0);
    }
}
