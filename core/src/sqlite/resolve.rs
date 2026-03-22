use super::SqliteDatabase;

impl SqliteDatabase {
    /// Pre-warm the hash index cache with the most recently imported active files.
    /// Called once after library open so the first grid page has zero cache misses.
    pub async fn warm_hash_index(&self) -> Result<usize, String> {
        let hash_index = self.hash_index.clone();
        let count = self
            .with_read_conn_labeled("hash_index/warm", move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT hash, file_id FROM file WHERE status = 1
                     ORDER BY imported_at DESC LIMIT 50000",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?;
                let mut pairs = Vec::new();
                for row in rows {
                    pairs.push(row?);
                }
                let count = pairs.len();
                hash_index.insert_batch(pairs);
                Ok(count)
            })
            .await?;
        Ok(count)
    }

    /// Resolve a hex hash to entity_id, checking cache first, then DB.
    /// Checks the file table first, then falls back to media_entity.hash
    /// (for collection entities which have their own hash identity).
    pub async fn resolve_hash(&self, hash: &str) -> Result<i64, String> {
        if let Some(id) = self.hash_index.get_id(hash) {
            return Ok(id);
        }
        let hash_owned = hash.to_string();
        let id = self
            .with_read_conn_labeled("hash_index/resolve_hash", move |conn| {
                // Try file table first (covers all single entities)
                if let Ok(fid) = conn.query_row(
                    "SELECT file_id FROM file WHERE hash = ?1",
                    [&hash_owned],
                    |row| row.get::<_, i64>(0),
                ) {
                    return Ok(fid);
                }
                // Fall back to media_entity.hash (covers collection entities)
                conn.query_row(
                    "SELECT entity_id FROM media_entity WHERE hash = ?1",
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

    /// Batch resolve hashes → (hash, entity_id) pairs.
    ///
    /// Checks file table first, then falls back to media_entity.hash for
    /// collection entities. If a resolved entity is a collection, the result
    /// set automatically includes ALL member entity_ids — so every operation
    /// that goes through this function transparently applies to collections
    /// and their children.
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
                    let mut batch = Vec::new();
                    let mut still_missing = Vec::new();

                    // 1. Try file table
                    if !misses.is_empty() {
                        let placeholders = std::iter::repeat_n("?", misses.len())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let sql = format!(
                            "SELECT hash, file_id FROM file WHERE hash IN ({})",
                            placeholders
                        );
                        let mut stmt = conn.prepare(&sql)?;
                        let rows = stmt.query_map(
                            rusqlite::params_from_iter(misses.iter()),
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                        )?;
                        let mut found = std::collections::HashSet::new();
                        for row in rows {
                            let (hash, fid) = row?;
                            hash_index.insert(hash.clone(), fid);
                            found.insert(hash.clone());
                            batch.push((hash, fid));
                        }
                        for h in &misses {
                            if !found.contains(h) {
                                still_missing.push(h.clone());
                            }
                        }
                    }

                    // 2. Fallback: media_entity.hash (collection entities)
                    if !still_missing.is_empty() {
                        let placeholders = std::iter::repeat_n("?", still_missing.len())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let sql = format!(
                            "SELECT hash, entity_id FROM media_entity WHERE hash IN ({})",
                            placeholders
                        );
                        let mut stmt = conn.prepare(&sql)?;
                        let rows = stmt.query_map(
                            rusqlite::params_from_iter(still_missing.iter()),
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                        )?;
                        for row in rows {
                            let (hash, eid) = row?;
                            hash_index.insert(hash.clone(), eid);
                            batch.push((hash, eid));
                        }
                    }

                    Ok(batch)
                })
                .await?;
            results.extend(db_results);
        }

        // Collection expansion: for any resolved entity_id that is a collection,
        // include all member entity_ids so actions propagate to children.
        let entity_ids: Vec<i64> = results.iter().map(|(_, id)| *id).collect();
        if !entity_ids.is_empty() {
            let extra = self
                .with_read_conn(move |conn| {
                    let mut expanded: Vec<(String, i64)> = Vec::new();
                    for &eid in &entity_ids {
                        let is_collection: bool = conn
                            .query_row(
                                "SELECT kind = 'collection' FROM media_entity WHERE entity_id = ?1",
                                [eid],
                                |row| row.get(0),
                            )
                            .unwrap_or(false);
                        if is_collection {
                            let member_fids =
                                crate::folders::collections_db::get_collection_member_file_ids(
                                    conn, eid,
                                )?;
                            for member_fid in member_fids {
                                if let Ok(hash) = conn.query_row(
                                    "SELECT hash FROM file WHERE file_id = ?1",
                                    [member_fid],
                                    |row| row.get::<_, String>(0),
                                ) {
                                    expanded.push((hash, member_fid));
                                }
                            }
                        }
                    }
                    Ok(expanded)
                })
                .await?;
            results.extend(extra);
        }

        // Dedup by entity_id
        let mut seen = std::collections::HashSet::new();
        results.retain(|(_, id)| seen.insert(*id));

        Ok(results)
    }

    /// Expand hashes to include collection member hashes. If any hash belongs to
    /// a collection cover file, the member files' hashes are appended.
    pub async fn expand_hashes_for_collections(
        &self,
        hashes: &[String],
    ) -> Result<Vec<String>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let resolved = self.resolve_hashes_batch(hashes).await?;
        let mut all_hashes: Vec<String> = hashes.to_vec();
        for (_, fid) in &resolved {
            let fid = *fid;
            let members = self
                .with_read_conn(move |conn| {
                    crate::folders::collections_db::get_cover_collection_member_files(conn, fid)
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
                // Check if this file_id is a cover file for a collection
                let members =
                    crate::folders::collections_db::get_cover_collection_member_files(conn, fid)?;
                if !members.is_empty() {
                    for (member_fid, _) in members {
                        expanded.push(member_fid);
                    }
                } else {
                    // Also check if this ID is a collection entity directly
                    // (status bitmaps include collection entity_ids)
                    let direct_members =
                        crate::folders::collections_db::get_collection_member_file_ids(conn, fid)?;
                    for member_fid in direct_members {
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

    /// Given a list of hashes (some may be collection cover hashes), expand any
    /// collection hashes to include the collection's member file hashes AND keep
    /// the original hash (so the collection entity itself is also included).
    pub async fn expand_collection_hashes_to_members(
        &self,
        hashes: &[String],
    ) -> Result<Vec<String>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let hashes = hashes.to_vec();
        self.with_read_conn(move |conn| {
            let mut result: Vec<String> = Vec::new();
            for hash in &hashes {
                let file_id: Option<i64> = conn
                    .query_row(
                        "SELECT file_id FROM file WHERE hash = ?1",
                        [hash],
                        |row| row.get(0),
                    )
                    .ok();
                let Some(fid) = file_id else {
                    result.push(hash.clone());
                    continue;
                };
                let collection_id =
                    crate::folders::collections_db::find_collection_for_cover_file(conn, fid)?;
                if let Some(cid) = collection_id {
                    result.push(hash.clone());
                    let member_hashes =
                        crate::folders::collections_db::list_collection_member_hashes(conn, cid)?;
                    result.extend(member_hashes);
                } else {
                    result.push(hash.clone());
                }
            }
            result.sort_unstable();
            result.dedup();
            Ok(result)
        })
        .await
    }

    /// Given cover file hashes, find the collection entity_ids they belong to.
    pub async fn find_collection_entity_ids_for_cover_hashes(
        &self,
        hashes: &[String],
    ) -> Result<Vec<i64>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let hashes = hashes.to_vec();
        self.with_read_conn(move |conn| {
            let mut ids = Vec::new();
            for hash in &hashes {
                let file_id: Option<i64> = conn
                    .query_row(
                        "SELECT file_id FROM file WHERE hash = ?1",
                        [hash],
                        |row| row.get(0),
                    )
                    .ok();
                if let Some(fid) = file_id {
                    if let Ok(Some(cid)) =
                        crate::folders::collections_db::find_collection_for_cover_file(conn, fid)
                    {
                        ids.push(cid);
                    }
                }
            }
            ids.sort_unstable();
            ids.dedup();
            Ok(ids)
        })
        .await
    }
}
