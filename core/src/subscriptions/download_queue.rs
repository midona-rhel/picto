//! Persistent download queue — stages collection members on disk so interrupted
//! imports survive app restarts.

use rusqlite::{params, Connection, OptionalExtension};

use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub queue_id: i64,
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub post_id: String,
    pub category: String,
    pub preferred_name: Option<String>,
    pub expected_count: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct QueueItem {
    pub item_id: i64,
    pub queue_id: i64,
    pub blob_hash: Option<String>,
    pub page_num: Option<i64>,
    pub metadata: Option<String>,
    pub status: String,
}

// ── Synchronous DB helpers ──────────────────────────────────────────────

pub fn create_queue_entry(
    conn: &Connection,
    subscription_id: i64,
    query_id: Option<i64>,
    post_id: &str,
    category: &str,
    preferred_name: Option<&str>,
    expected_count: Option<i64>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO download_queue (subscription_id, query_id, post_id, category, preferred_name, expected_count, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?7)",
        params![subscription_id, query_id, post_id, category, preferred_name, expected_count, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_queue_entry(
    conn: &Connection,
    subscription_id: i64,
    category: &str,
    post_id: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT queue_id FROM download_queue
         WHERE subscription_id = ?1 AND category = ?2 AND post_id = ?3 AND status = 'pending'",
        params![subscription_id, category, post_id],
        |row| row.get(0),
    ).optional()
}

pub fn add_queue_item(
    conn: &Connection,
    queue_id: i64,
    page_num: Option<i64>,
    metadata_json: Option<&str>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO download_queue_item (queue_id, page_num, metadata, status, created_at)
         VALUES (?1, ?2, ?3, 'pending', ?4)",
        params![queue_id, page_num, metadata_json, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn mark_item_prepared(
    conn: &Connection,
    item_id: i64,
    blob_hash: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE download_queue_item SET blob_hash = ?1, status = 'prepared' WHERE item_id = ?2",
        params![blob_hash, item_id],
    )?;
    Ok(())
}

pub fn mark_queue_complete(conn: &Connection, queue_id: i64) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE download_queue SET status = 'complete', updated_at = ?1 WHERE queue_id = ?2",
        params![now, queue_id],
    )?;
    Ok(())
}

pub fn mark_queue_stale(conn: &Connection, queue_id: i64) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE download_queue SET status = 'stale', updated_at = ?1 WHERE queue_id = ?2",
        params![now, queue_id],
    )?;
    Ok(())
}

pub fn list_pending_queues(conn: &Connection) -> rusqlite::Result<Vec<QueueEntry>> {
    let mut stmt = conn.prepare(
        "SELECT queue_id, subscription_id, query_id, post_id, category, preferred_name, expected_count, status
         FROM download_queue WHERE status IN ('pending', 'stale')
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(QueueEntry {
            queue_id: row.get(0)?,
            subscription_id: row.get(1)?,
            query_id: row.get(2)?,
            post_id: row.get(3)?,
            category: row.get(4)?,
            preferred_name: row.get(5)?,
            expected_count: row.get(6)?,
            status: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn list_queue_items(conn: &Connection, queue_id: i64) -> rusqlite::Result<Vec<QueueItem>> {
    let mut stmt = conn.prepare(
        "SELECT item_id, queue_id, blob_hash, page_num, metadata, status
         FROM download_queue_item WHERE queue_id = ?1
         ORDER BY page_num ASC, item_id ASC",
    )?;
    let rows = stmt.query_map([queue_id], |row| {
        Ok(QueueItem {
            item_id: row.get(0)?,
            queue_id: row.get(1)?,
            blob_hash: row.get(2)?,
            page_num: row.get(3)?,
            metadata: row.get(4)?,
            status: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn delete_completed_queues(conn: &Connection) -> rusqlite::Result<usize> {
    Ok(conn.execute("DELETE FROM download_queue WHERE status = 'complete'", [])?)
}

pub fn delete_stale_queues_older_than(conn: &Connection, days: i64) -> rusqlite::Result<usize> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    let cutoff_str = cutoff.to_rfc3339();
    Ok(conn.execute(
        "DELETE FROM download_queue WHERE status = 'stale' AND updated_at < ?1",
        [cutoff_str],
    )?)
}

pub fn mark_all_pending_stale(conn: &Connection, subscription_id: i64) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE download_queue SET status = 'stale', updated_at = ?1
         WHERE subscription_id = ?2 AND status = 'pending'",
        params![now, subscription_id],
    )?;
    Ok(())
}

// ── Async wrappers ──────────────────────────────────────────────────────

impl SqliteDatabase {
    pub async fn create_or_get_queue_entry(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        post_id: &str,
        category: &str,
        preferred_name: Option<&str>,
        expected_count: Option<i64>,
    ) -> Result<i64, String> {
        let pid = post_id.to_string();
        let cat = category.to_string();
        let pname = preferred_name.map(|s| s.to_string());
        self.with_conn(move |conn| {
            if let Some(qid) = find_queue_entry(conn, subscription_id, &cat, &pid)? {
                Ok(qid)
            } else {
                create_queue_entry(conn, subscription_id, query_id, &pid, &cat, pname.as_deref(), expected_count)
            }
        })
        .await
    }

    pub async fn add_queue_item(
        &self,
        queue_id: i64,
        page_num: Option<i64>,
        metadata_json: Option<&str>,
    ) -> Result<i64, String> {
        let meta = metadata_json.map(|s| s.to_string());
        self.with_conn(move |conn| add_queue_item(conn, queue_id, page_num, meta.as_deref()))
            .await
    }

    pub async fn mark_queue_complete(&self, queue_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| mark_queue_complete(conn, queue_id))
            .await
    }

    pub async fn mark_all_pending_stale_for_subscription(&self, subscription_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| mark_all_pending_stale(conn, subscription_id))
            .await
    }

    pub async fn cleanup_download_queue(&self) -> Result<(), String> {
        self.with_conn(move |conn| {
            delete_completed_queues(conn)?;
            delete_stale_queues_older_than(conn, 7)?;
            Ok(())
        })
        .await
    }

    pub async fn list_pending_download_queues(&self) -> Result<Vec<QueueEntry>, String> {
        self.with_read_conn(list_pending_queues).await
    }
}
