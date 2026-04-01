use super::SqliteDatabase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityExpansionMode {
    EntityOnly,
    DescendantsOnly,
    EntityAndDescendants,
}

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

    /// Batch resolve top-level entity hashes → (hash, entity_id) pairs.
    ///
    /// This does not expand collection members. Callers must opt into
    /// descendant expansion explicitly through `expand_entity_ids` or
    /// `resolve_entity_hashes_with_expansion`.
    pub async fn resolve_entity_hashes_batch(
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
                .with_read_conn_labeled("hash_index/resolve_entity_hashes_batch", move |conn| {
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
                        let rows = stmt
                            .query_map(rusqlite::params_from_iter(misses.iter()), |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                            })?;
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
                        let rows = stmt
                            .query_map(rusqlite::params_from_iter(still_missing.iter()), |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                            })?;
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

        // Dedup by entity_id
        let mut seen = std::collections::HashSet::new();
        results.retain(|(_, id)| seen.insert(*id));

        Ok(results)
    }

    pub async fn resolve_entity_hashes_for_ids(
        &self,
        entity_ids: &[i64],
    ) -> Result<Vec<(String, i64)>, String> {
        if entity_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids = entity_ids.to_vec();
        self.with_read_conn(move |conn| {
            let mut resolved = Vec::new();
            for &entity_id in &ids {
                if let Ok(hash) = conn.query_row(
                    "SELECT hash FROM file WHERE file_id = ?1",
                    [entity_id],
                    |row| row.get::<_, String>(0),
                ) {
                    resolved.push((hash, entity_id));
                    continue;
                }
                if let Ok(hash) = conn.query_row(
                    "SELECT hash FROM media_entity WHERE entity_id = ?1",
                    [entity_id],
                    |row| row.get::<_, String>(0),
                ) {
                    resolved.push((hash, entity_id));
                }
            }
            Ok(resolved)
        })
        .await
    }

    /// Expand a list of entity_ids according to the explicit expansion mode.
    pub async fn expand_entity_ids(
        &self,
        entity_ids: Vec<i64>,
        expansion: EntityExpansionMode,
    ) -> Result<Vec<i64>, String> {
        if entity_ids.is_empty() {
            return Ok(entity_ids);
        }
        if expansion == EntityExpansionMode::EntityOnly {
            let mut ids = entity_ids;
            ids.sort_unstable();
            ids.dedup();
            return Ok(ids);
        }

        self.with_read_conn(move |conn| {
            let mut expanded = Vec::new();
            for &eid in &entity_ids {
                let is_collection: bool = conn
                    .query_row(
                        "SELECT kind = 'collection' FROM media_entity WHERE entity_id = ?1",
                        [eid],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);
                if !is_collection || expansion == EntityExpansionMode::EntityAndDescendants {
                    expanded.push(eid);
                }
                if is_collection {
                    let member_fids =
                        crate::folders::collections_db::get_collection_member_file_ids(conn, eid)?;
                    expanded.extend(member_fids);
                }
            }
            expanded.sort_unstable();
            expanded.dedup();
            Ok(expanded)
        })
        .await
    }

    pub async fn resolve_entity_hashes_with_expansion(
        &self,
        hashes: &[String],
        expansion: EntityExpansionMode,
    ) -> Result<Vec<(String, i64)>, String> {
        let base = self.resolve_entity_hashes_batch(hashes).await?;
        if expansion == EntityExpansionMode::EntityOnly {
            return Ok(base);
        }
        let ids = self
            .expand_entity_ids(base.iter().map(|(_, id)| *id).collect(), expansion)
            .await?;
        self.resolve_entity_hashes_for_ids(&ids).await
    }
}
