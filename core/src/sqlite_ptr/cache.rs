//! Negative miss cache — hashes confirmed to have no PTR tags.
//! Avoids repeated lookups for hashes with no PTR data.

use rusqlite::{params, Connection};

use super::PtrSqliteDatabase;

// ─── Standalone functions ───

/// Check if a hash is in the negative cache.
pub fn is_negative_cached(conn: &Connection, hash: &str) -> rusqlite::Result<bool> {
    let hash_blob = super::hash_to_blob(hash);
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM ptr_negative_cache WHERE hash = ?1",
        [&hash_blob],
        |row| row.get(0),
    )
}

/// Add a hash to the negative cache.
pub fn add_negative(conn: &Connection, hash: &str, epoch: i64) -> rusqlite::Result<()> {
    let hash_blob = super::hash_to_blob(hash);
    conn.execute(
        "INSERT OR REPLACE INTO ptr_negative_cache (hash, epoch) VALUES (?1, ?2)",
        params![hash_blob, epoch],
    )?;
    Ok(())
}

/// Add multiple hashes to the negative cache in one transaction.
pub fn batch_add_negative(
    conn: &mut Connection,
    hashes: &[String],
    epoch: i64,
) -> rusqlite::Result<()> {
    if hashes.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO ptr_negative_cache (hash, epoch) VALUES (?1, ?2)",
        )?;
        for hash in hashes {
            let hash_blob = super::hash_to_blob(hash);
            stmt.execute(params![hash_blob, epoch])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Remove a hash from the negative cache (e.g., after new PTR data arrives).
pub fn remove_negative(conn: &Connection, hash: &str) -> rusqlite::Result<()> {
    let hash_blob = super::hash_to_blob(hash);
    conn.execute(
        "DELETE FROM ptr_negative_cache WHERE hash = ?1",
        [&hash_blob],
    )?;
    Ok(())
}

/// Clear the entire negative cache.
pub fn clear_negative_cache(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM ptr_negative_cache", [])?;
    Ok(())
}

/// Batch check which hashes are in the negative cache for the given epoch.
/// Only returns hashes whose cached epoch matches (stale entries ignored).
pub fn batch_check_negative(
    conn: &Connection,
    hashes: &[String],
    epoch: i64,
) -> rusqlite::Result<Vec<String>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }

    let hash_blobs: Vec<Vec<u8>> = hashes.iter().map(|h| super::hash_to_blob(h)).collect();

    let placeholders = std::iter::repeat_n("?", hashes.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT hash
         FROM ptr_negative_cache
         WHERE epoch = ?1 AND hash IN ({})",
        placeholders
    );

    let mut stmt = conn.prepare(&sql)?;
    // First param is the epoch, then the hash blobs
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
        Vec::with_capacity(1 + hash_blobs.len());
    param_values.push(Box::new(epoch));
    for blob in &hash_blobs {
        param_values.push(Box::new(blob.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(super::blob_to_hash(&row.get::<_, Vec<u8>>(0)?))
    })?;
    rows.collect()
}

// ─── High-level methods ───

impl PtrSqliteDatabase {
    pub async fn is_negative_cached(&self, hash: &str) -> Result<bool, String> {
        if self.negative_cache_mem().read().await.contains(hash) {
            return Ok(true);
        }
        let h = hash.to_string();
        let found = self
            .with_read_conn(move |conn| is_negative_cached(conn, &h))
            .await?;
        if found {
            self.negative_cache_mem()
                .write()
                .await
                .insert(hash.to_string());
        }
        Ok(found)
    }

    pub async fn add_negative_cache(&self, hash: &str, epoch: i64) -> Result<(), String> {
        let h = hash.to_string();
        self.with_conn(move |conn| add_negative(conn, &h, epoch))
            .await?;
        let mem = self.negative_cache_mem();
        mem.write().await.insert(hash.to_string());
        Ok(())
    }

    pub async fn clear_negative_cache(&self) -> Result<(), String> {
        self.with_conn(clear_negative_cache).await?;
        self.negative_cache_mem().write().await.clear();
        Ok(())
    }

    pub async fn batch_check_negative(&self, hashes: Vec<String>) -> Result<Vec<String>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }

        let mem = self.negative_cache_mem();
        let read_guard = mem.read().await;
        let mut mem_hits = Vec::new();
        let mut remaining = Vec::new();
        for h in hashes {
            if read_guard.contains(&h) {
                mem_hits.push(h);
            } else {
                remaining.push(h);
            }
        }
        drop(read_guard);

        if remaining.is_empty() {
            return Ok(mem_hits);
        }

        let epoch = self.overlay_epoch();
        let db_hits = self
            .with_read_conn(move |conn| batch_check_negative(conn, &remaining, epoch))
            .await?;

        if !db_hits.is_empty() {
            let mut write_guard = mem.write().await;
            for h in &db_hits {
                write_guard.insert(h.clone());
            }
        }

        mem_hits.extend(db_hits);
        Ok(mem_hits)
    }

    pub async fn batch_add_negative_cache(&self, hashes: Vec<String>) -> Result<(), String> {
        if hashes.is_empty() {
            return Ok(());
        }
        let epoch = self.overlay_epoch();
        let db_hashes = hashes.clone();
        self.with_conn_mut(move |conn| batch_add_negative(conn, &db_hashes, epoch))
            .await?;
        let mem = self.negative_cache_mem();
        let mut write_guard = mem.write().await;
        for h in hashes {
            write_guard.insert(h);
        }
        Ok(())
    }

    /// Add hashes to the in-memory negative cache only (no DB write).
    /// Use this on the read path to avoid writer lock contention during sync.
    pub async fn add_negative_cache_mem_only(&self, hashes: Vec<String>) {
        if hashes.is_empty() {
            return;
        }
        let mem = self.negative_cache_mem();
        let mut write_guard = mem.write().await;
        for h in hashes {
            write_guard.insert(h);
        }
    }
}
