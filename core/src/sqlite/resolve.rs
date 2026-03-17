use super::{files, SqliteDatabase};

impl SqliteDatabase {
    /// Resolve a hex hash to file_id, checking cache first, then DB.
    pub async fn resolve_hash(&self, hash: &str) -> Result<i64, String> {
        if let Some(id) = self.hash_index.get_id(hash) {
            return Ok(id);
        }
        let hash_owned = hash.to_string();
        let id = self
            .with_read_conn_labeled("hash_index/resolve_hash", move |conn| {
                conn.query_row(
                    "SELECT file_id FROM file WHERE hash = ?1",
                    [&hash_owned],
                    |row| row.get::<_, i64>(0),
                )
            })
            .await?;
        self.hash_index.insert(hash.to_string(), id);
        Ok(id)
    }

    /// Resolve a file_id to hex hash, checking cache first, then DB.
    pub async fn resolve_id(&self, file_id: i64) -> Result<String, String> {
        if let Some(hash) = self.hash_index.get_hash(file_id) {
            return Ok(hash);
        }
        let hash = self
            .with_read_conn_labeled("hash_index/resolve_id", move |conn| {
                conn.query_row(
                    "SELECT hash FROM file WHERE file_id = ?1",
                    [file_id],
                    |row| row.get::<_, String>(0),
                )
            })
            .await?;
        self.hash_index.insert(hash.clone(), file_id);
        Ok(hash)
    }

    /// Batch resolve file_ids → hashes. Checks cache first, then DB for misses.
    /// Returns results in arbitrary order; missing IDs are silently skipped.
    pub async fn resolve_ids_batch(&self, file_ids: &[i64]) -> Result<Vec<(i64, String)>, String> {
        let mut results = Vec::with_capacity(file_ids.len());
        let mut misses = Vec::new();

        for &fid in file_ids {
            if let Some(hash) = self.hash_index.get_hash(fid) {
                results.push((fid, hash));
            } else {
                misses.push(fid);
            }
        }

        if !misses.is_empty() {
            let hash_index = self.hash_index.clone();
            let db_results = self
                .with_read_conn_labeled("hash_index/resolve_ids_batch", move |conn| {
                    let placeholders = std::iter::repeat_n("?", misses.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "SELECT file_id, hash FROM file WHERE file_id IN ({})",
                        placeholders
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt
                        .query_map(rusqlite::params_from_iter(misses.iter()), |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                        })?;
                    let mut batch = Vec::new();
                    for row in rows {
                        let (fid, hash) = row?;
                        hash_index.insert(hash.clone(), fid);
                        batch.push((fid, hash));
                    }
                    Ok(batch)
                })
                .await?;
            results.extend(db_results);
        }

        Ok(results)
    }

    /// Batch resolve hashes → file_ids. Checks cache first, then DB for misses.
    /// Returns results in arbitrary order; missing hashes are silently skipped.
    pub async fn resolve_hashes_batch(
        &self,
        hashes: &[String],
    ) -> Result<Vec<(String, i64)>, String> {
        let mut results = Vec::with_capacity(hashes.len());
        let mut misses = Vec::new();

        for hash in hashes {
            if let Some(id) = self.hash_index.get_id(hash) {
                results.push((hash.clone(), id));
            } else {
                misses.push(hash.clone());
            }
        }

        if !misses.is_empty() {
            let hash_index = self.hash_index.clone();
            let db_results = self
                .with_read_conn_labeled("hash_index/resolve_hashes_batch", move |conn| {
                    let placeholders = std::iter::repeat_n("?", misses.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "SELECT hash, file_id FROM file WHERE hash IN ({})",
                        placeholders
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt
                        .query_map(rusqlite::params_from_iter(misses.iter()), |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                        })?;
                    let mut batch = Vec::new();
                    for row in rows {
                        let (hash, fid) = row?;
                        hash_index.insert(hash.clone(), fid);
                        batch.push((hash, fid));
                    }
                    Ok(batch)
                })
                .await?;
            results.extend(db_results);
        }

        Ok(results)
    }

    /// Expand hashes to include collection member hashes. If any hash belongs to
    /// a collection cover file, the member files' hashes are appended.
    pub async fn expand_hashes_for_collections(&self, hashes: &[String]) -> Result<Vec<String>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let resolved = self.resolve_hashes_batch(hashes).await?;
        let mut all_hashes: Vec<String> = hashes.to_vec();
        for (_, fid) in &resolved {
            let fid = *fid;
            let members = self
                .with_read_conn(move |conn| {
                    if let Some(cid) = files::find_collection_for_cover_file(conn, fid)? {
                        files::get_collection_member_files(conn, cid)
                    } else {
                        Ok(vec![])
                    }
                })
                .await?;
            for (_, member_hash) in members {
                all_hashes.push(member_hash);
            }
        }
        all_hashes.sort_unstable();
        all_hashes.dedup();
        Ok(all_hashes)
    }

    /// Expand a list of file_ids (== entity_ids for singles) to include collection
    /// member file_ids. Any file_id that is a collection cover gets its member
    /// file_ids appended. Non-collection file_ids pass through unchanged.
    pub async fn expand_collection_members(&self, file_ids: Vec<i64>) -> Result<Vec<i64>, String> {
        if file_ids.is_empty() {
            return Ok(file_ids);
        }
        self.with_read_conn(move |conn| {
            let mut expanded = file_ids.clone();
            for &fid in &file_ids {
                if let Some(cid) = files::find_collection_for_cover_file(conn, fid)? {
                    let members = files::get_collection_member_files(conn, cid)?;
                    for (member_fid, _) in members {
                        expanded.push(member_fid);
                    }
                }
            }
            expanded.sort_unstable();
            expanded.dedup();
            Ok(expanded)
        })
        .await
    }
}
