//! Deferred work + analysis results — split from db/mod.rs, same `impl LibraryDatabase`.

use crate::blob_store::BlobStore;

use super::*;

impl LibraryDatabase {
    // ── Deferred work ────────────────────────────────────────────

    pub fn get_deferred_work_summary(
        &self,
    ) -> Result<crate::background_work::DeferredWorkSummary, String> {
        self.with_read(|conn| {
            let pending: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item WHERE status = 'pending'",
                [],
                |r| r.get(0),
            )?;
            let running: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item WHERE status = 'running'",
                [],
                |r| r.get(0),
            )?;
            let failed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item WHERE status = 'pending' AND attempt_count > 0",
                [],
                |r| r.get(0),
            )?;
            let dominant_pending: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item
                 WHERE work_type = 'dominant_colors' AND status = 'pending' AND attempt_count = 0",
                [],
                |r| r.get(0),
            )?;
            let dominant_running: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item
                 WHERE work_type = 'dominant_colors' AND status = 'running'",
                [],
                |r| r.get(0),
            )?;
            let dominant_failed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item
                 WHERE work_type = 'dominant_colors' AND status = 'pending' AND attempt_count > 0",
                [],
                |r| r.get(0),
            )?;
            Ok(crate::background_work::DeferredWorkSummary {
                pending_count: pending,
                running_count: running,
                failed_count: failed,
                dominant_colors_pending_count: dominant_pending,
                dominant_colors_running_count: dominant_running,
                dominant_colors_failed_count: dominant_failed,
            })
        })
    }

    pub fn retry_deferred_work(&self, entity_hash: &str) -> Result<(), String> {
        let h = entity_hash.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending', attempt_count = 0, available_at = ?1, last_error = NULL
                 WHERE entity_hash = ?2",
                rusqlite::params![now, h],
            )?;
            Ok(())
        })
    }

    pub fn enqueue_deferred_jobs(
        &self,
        entity_hash: &str,
        work_types: &[crate::background_work::DeferredWorkType],
    ) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let work_types: Vec<String> = work_types
            .iter()
            .map(|work_type| work_type.as_db_str().to_string())
            .collect();
        self.with_write(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let mut stmt = conn.prepare(
                "INSERT INTO deferred_work_item
                     (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                     (?1, ?2, 'pending', 0, ?3, ?3)
                 ON CONFLICT(entity_hash, work_type) DO UPDATE SET
                     status = 'pending',
                     attempt_count = 0,
                     available_at = excluded.available_at,
                     last_error = NULL,
                     queued_at = excluded.queued_at,
                     started_at = NULL,
                     finished_at = NULL,
                     last_error_at = NULL",
            )?;
            for work_type in &work_types {
                stmt.execute(rusqlite::params![hash, work_type, now])?;
            }
            Ok(())
        })
    }

    /// Request physical blob cleanup and immediately make one lease-aware
    /// attempt. The durable row is created before touching the filesystem, so
    /// a busy importer or an actual delete failure remains retryable.
    pub fn enqueue_blob_delete_and_attempt(
        &self,
        blob_store: &BlobStore,
        file_hash: &str,
    ) -> Result<types::BlobCleanupResult, String> {
        self.enqueue_deferred_jobs(
            file_hash,
            &[crate::background_work::DeferredWorkType::BlobDelete],
        )?;
        self.cleanup_blob_delete_if_unreferenced(blob_store, file_hash)
    }

    pub fn ensure_deferred_jobs_present(
        &self,
        entity_hash: &str,
        work_types: &[crate::background_work::DeferredWorkType],
    ) -> Result<(), String> {
        self.ensure_deferred_jobs_present_batch(vec![(
            entity_hash.to_string(),
            work_types.to_vec(),
        )])
    }

    pub fn ensure_deferred_jobs_present_batch(
        &self,
        items: Vec<(String, Vec<crate::background_work::DeferredWorkType>)>,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let mut stmt = conn.prepare(
                "INSERT INTO deferred_work_item
                     (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                     (?1, ?2, 'pending', 0, ?3, ?3)
                 ON CONFLICT(entity_hash, work_type) DO NOTHING",
            )?;
            for (entity_hash, work_types) in &items {
                for work_type in work_types {
                    stmt.execute(rusqlite::params![entity_hash, work_type.as_db_str(), now])?;
                }
            }
            Ok(())
        })
    }

    pub fn enqueue_deferred_jobs_batch(
        &self,
        items: Vec<(String, Vec<crate::background_work::DeferredWorkType>)>,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let mut stmt = conn.prepare(
                "INSERT INTO deferred_work_item
                     (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                     (?1, ?2, 'pending', 0, ?3, ?3)
                 ON CONFLICT(entity_hash, work_type) DO UPDATE SET
                     status = 'pending',
                     attempt_count = 0,
                     available_at = excluded.available_at,
                     last_error = NULL,
                     queued_at = excluded.queued_at,
                     started_at = NULL,
                     finished_at = NULL,
                     last_error_at = NULL",
            )?;
            for (entity_hash, work_types) in &items {
                for work_type in work_types {
                    stmt.execute(rusqlite::params![entity_hash, work_type.as_db_str(), now])?;
                }
            }
            Ok(())
        })
    }

    pub fn list_deferred_work_items(
        &self,
        filter: crate::background_work::DeferredWorkFilter,
    ) -> Result<Vec<crate::background_work::DeferredWorkItemInfo>, String> {
        self.with_read(move |conn| {
            let mut sql = String::from(
                "SELECT entity_hash, work_type, status, attempt_count, available_at, queued_at, started_at, finished_at, last_error, last_error_at
                 FROM deferred_work_item",
            );
            let mut conditions = Vec::<String>::new();
            let mut params: Vec<String> = Vec::new();

            if let Some(entity_hash) = &filter.entity_hash {
                conditions.push("entity_hash = ?".to_string());
                params.push(entity_hash.clone());
            }
            if let Some(work_type) = filter.work_type {
                conditions.push("work_type = ?".to_string());
                params.push(work_type.as_db_str().to_string());
            }
            if let Some(status) = filter.status {
                conditions.push("status = ?".to_string());
                params.push(status.as_db_str().to_string());
            }
            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&conditions.join(" AND "));
            }
            sql.push_str(" ORDER BY available_at ASC, work_id ASC");
            let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
            sql.push_str(&format!(" LIMIT {limit}"));

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                    let work_type_raw: String = row.get(1)?;
                    let status_raw: String = row.get(2)?;
                    Ok(crate::background_work::DeferredWorkItemInfo {
                        entity_hash: row.get(0)?,
                        work_type: crate::background_work::DeferredWorkType::from_db_str(&work_type_raw)
                            .ok_or(rusqlite::Error::InvalidQuery)?,
                        status: crate::background_work::DeferredWorkStatus::from_db_str(&status_raw)
                            .ok_or(rusqlite::Error::InvalidQuery)?,
                        attempt_count: row.get(3)?,
                        available_at: row.get(4)?,
                        queued_at: row.get(5)?,
                        started_at: row.get(6)?,
                        finished_at: row.get(7)?,
                        last_error: row.get(8)?,
                        last_error_at: row.get(9)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn reset_running_deferred_work_items(&self) -> Result<usize, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending',
                     started_at = NULL,
                     finished_at = NULL,
                     queued_at = ?1
                 WHERE status = 'running'",
                [&now],
            )
        })
    }

    pub fn claim_next_deferred_work_items(
        &self,
    ) -> Result<Vec<types::ClaimedDeferredWorkItem>, String> {
        self.with_write(|conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let next_hash: Option<String> = conn
                .query_row(
                    "SELECT entity_hash
                     FROM deferred_work_item
                     WHERE status = 'pending' AND available_at <= ?1
                     ORDER BY work_id ASC
                     LIMIT 1",
                    [&now],
                    |row| row.get(0),
                )
                .optional()?;

            let Some(next_hash) = next_hash else {
                return Ok(Vec::new());
            };

            let mut stmt = conn.prepare(
                "SELECT work_id, entity_hash, work_type, attempt_count
                 FROM deferred_work_item
                 WHERE entity_hash = ?1 AND status = 'pending' AND available_at <= ?2
                 ORDER BY work_id ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![next_hash, now], |row| {
                    Ok(types::ClaimedDeferredWorkItem {
                        work_id: row.get(0)?,
                        entity_hash: row.get(1)?,
                        work_type: row.get(2)?,
                        attempt_count: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(stmt);

            for row in &rows {
                conn.execute(
                    "UPDATE deferred_work_item
                     SET status = 'running',
                         started_at = ?2
                     WHERE work_id = ?1",
                    rusqlite::params![row.work_id, now],
                )?;
            }
            Ok(rows)
        })
    }

    pub fn complete_deferred_work_item(&self, work_id: i64) -> Result<(), String> {
        self.with_write(move |conn| {
            conn.execute(
                "DELETE FROM deferred_work_item WHERE work_id = ?1",
                [work_id],
            )?;
            Ok(())
        })
    }

    /// Settle every claimed job independently. If deleting a completed row
    /// fails, return that job to the retry queue before moving on.
    pub fn complete_deferred_work_batch(
        &self,
        jobs: &[types::ClaimedDeferredWorkItem],
    ) -> Result<(), String> {
        let mut errors = Vec::new();
        for job in jobs {
            if let Err(error) = self.complete_deferred_work_item(job.work_id) {
                let retry_error = format!("Completion cleanup failed: {error}");
                match self.retry_deferred_work_item(
                    job.work_id,
                    job.attempt_count.saturating_add(1),
                    &retry_error,
                ) {
                    Ok(()) => errors.push(format!(
                        "work item {} completion failed and was requeued: {error}",
                        job.work_id
                    )),
                    Err(retry_failure) => errors.push(format!(
                        "work item {} completion failed: {error}; requeue failed: {retry_failure}",
                        job.work_id
                    )),
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Delete a blob only while holding the database write transaction.
    ///
    /// Reimports serialize on the same writer boundary. A live media_file
    /// reference cancels only the obsolete blob-delete job; derivative work is
    /// deliberately left untouched.
    pub fn cleanup_blob_delete_if_unreferenced(
        &self,
        blob_store: &BlobStore,
        file_hash: &str,
    ) -> Result<types::BlobCleanupResult, String> {
        let file_hash = file_hash.to_string();
        let Some(_blob_lease) = blob_store.try_acquire_hash_lease(&file_hash) else {
            return Err(format!("blob hash {file_hash} is being imported"));
        };
        self.with_write(|conn| {
            let referenced: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM media_file WHERE file_hash = ?1 LIMIT 1",
                    [&file_hash],
                    |row| row.get(0),
                )
                .optional()?;
            if referenced.is_some() {
                conn.execute(
                    "DELETE FROM deferred_work_item
                     WHERE entity_hash = ?1 AND work_type = 'blob_delete'",
                    [&file_hash],
                )?;
                return Ok(types::BlobCleanupResult::CancelledReferenced);
            }

            blob_store
                .delete(&file_hash)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            conn.execute(
                "DELETE FROM deferred_work_item
                 WHERE entity_hash = ?1",
                [&file_hash],
            )?;
            Ok(types::BlobCleanupResult::Deleted)
        })
    }

    pub fn complete_blob_delete_for_hash(&self, file_hash: &str) -> Result<(), String> {
        let file_hash = file_hash.to_string();
        self.with_write(move |conn| {
            conn.execute(
                "DELETE FROM deferred_work_item
                 WHERE entity_hash = ?1 AND work_type = 'blob_delete'",
                [file_hash],
            )?;
            Ok(())
        })
    }

    pub fn retry_deferred_work_item(
        &self,
        work_id: i64,
        next_attempt: i64,
        error: &str,
    ) -> Result<(), String> {
        let error = error.to_string();
        let available_at = {
            let exp = (next_attempt.saturating_sub(1)).clamp(0, 10) as u32;
            let delay_secs = (30_i64.saturating_mul(1_i64 << exp)).min(60 * 60);
            (chrono::Utc::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339()
        };
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending',
                     attempt_count = ?2,
                     available_at = ?3,
                     last_error = ?4,
                     queued_at = ?5,
                     started_at = NULL,
                     last_error_at = ?5
                 WHERE work_id = ?1",
                rusqlite::params![work_id, next_attempt, available_at, error, now],
            )?;
            Ok(())
        })
    }

    /// Requeue every claimed job even when one retry write fails.
    pub fn retry_deferred_work_batch(
        &self,
        jobs: &[types::ClaimedDeferredWorkItem],
        error: &str,
    ) -> Result<(), String> {
        let mut errors = Vec::new();
        for job in jobs {
            if let Err(retry_failure) = self.retry_deferred_work_item(
                job.work_id,
                job.attempt_count.saturating_add(1),
                error,
            ) {
                errors.push(format!(
                    "work item {} requeue failed: {retry_failure}",
                    job.work_id
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn retry_blob_delete_for_hash(&self, file_hash: &str, error: &str) -> Result<(), String> {
        let file_hash = file_hash.to_string();
        let error = error.to_string();
        self.with_write(move |conn| {
            let Some((work_id, attempt_count)): Option<(i64, i64)> = conn
                .query_row(
                    "SELECT work_id, attempt_count
                     FROM deferred_work_item
                     WHERE entity_hash = ?1 AND work_type = 'blob_delete'",
                    [&file_hash],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
            else {
                return Ok(());
            };
            let next_attempt = attempt_count.saturating_add(1);
            let exp = (next_attempt.saturating_sub(1)).clamp(0, 10) as u32;
            let delay_secs = (30_i64.saturating_mul(1_i64 << exp)).min(60 * 60);
            let available_at =
                (chrono::Utc::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending',
                     attempt_count = ?2,
                     available_at = ?3,
                     last_error = ?4,
                     queued_at = ?5,
                     started_at = NULL,
                     last_error_at = ?5
                 WHERE work_id = ?1",
                rusqlite::params![work_id, next_attempt, available_at, error, now],
            )?;
            Ok(())
        })
    }

    pub fn set_phash_for_entity_hash(&self, entity_hash: &str, phash: &str) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let value = phash.to_string();
        self.with_write(move |conn| {
            let file_id: i64 = conn.query_row(
                "SELECT mf.file_id
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE me.entity_hash = ?1",
                [&hash],
                |row| row.get(0),
            )?;
            write::files::update_file_analysis(conn, file_id, Some(&value), None, None)
        })
    }

    pub fn replace_file_phash(&self, file_id: i64, phash: Option<&str>) -> Result<(), String> {
        let value = phash.map(str::to_string);
        self.with_write(move |conn| {
            write::files::replace_file_phash(conn, file_id, value.as_deref())
        })
    }

    pub fn set_file_colors_for_entity_hash(
        &self,
        entity_hash: &str,
        colors: &[(String, f32, f32, f32)],
        dominant_color_hex: Option<&str>,
        dominant_palette_blob: Option<&[u8]>,
        color_analysis_version: i64,
    ) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let colors = colors.to_vec();
        let dominant = dominant_color_hex.map(str::to_string);
        let palette_blob = dominant_palette_blob.map(|blob| blob.to_vec());
        self.with_write(move |conn| {
            let file_id: i64 = conn.query_row(
                "SELECT mf.file_id
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE me.entity_hash = ?1",
                [&hash],
                |row| row.get(0),
            )?;
            write::files::replace_file_color_analysis(
                conn,
                file_id,
                &colors,
                dominant.as_deref(),
                palette_blob.as_deref(),
                color_analysis_version,
            )
        })
    }

    pub fn replace_file_colors(
        &self,
        file_id: i64,
        colors: &[(String, f32, f32, f32)],
        dominant_color_hex: Option<&str>,
        dominant_palette_blob: Option<&[u8]>,
        color_analysis_version: i64,
    ) -> Result<(), String> {
        let colors = colors.to_vec();
        let dominant = dominant_color_hex.map(str::to_string);
        let palette_blob = dominant_palette_blob.map(|blob| blob.to_vec());
        self.with_write(move |conn| {
            write::files::replace_file_color_analysis(
                conn,
                file_id,
                &colors,
                dominant.as_deref(),
                palette_blob.as_deref(),
                color_analysis_version,
            )
        })
    }

    pub fn get_file_colors_for_entity_hash(
        &self,
        entity_hash: &str,
    ) -> Result<Vec<(String, f64, f64, f64)>, String> {
        let hash = entity_hash.to_string();
        self.with_read(|conn| {
            let row = conn
                .query_row(
                    "SELECT mf.file_id, mf.dominant_palette_blob
                     FROM media_entity me
                     JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                     JOIN media_file mf ON mf.file_id = sme.file_id
                     WHERE me.entity_hash = ?1",
                    [&hash],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
                )
                .optional()?;
            let Some((file_id, dominant_palette_blob)) = row else {
                return Ok(Vec::new());
            };

            if let Some(blob) = dominant_palette_blob.as_deref() {
                match crate::media_processing::colors::deserialize_dominant_palette_blob(blob) {
                    Ok(colors) => {
                        return Ok(colors
                            .into_iter()
                            .map(|color| (color.hex, color.l, color.a, color.b))
                            .collect());
                    }
                    Err(error) => {
                        tracing::warn!(
                            entity_hash = %hash,
                            file_id,
                            error = %error,
                            "Failed to decode dominant_palette_blob, falling back to file_color"
                        );
                    }
                }
            }

            let mut stmt = conn.prepare_cached(
                "SELECT hex, l, a, b FROM file_color WHERE file_id = ?1 ORDER BY rowid",
            )?;
            let colors = stmt
                .query_map([file_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, f64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(colors)
        })
    }

    pub fn enqueue_stale_color_analysis_jobs(&self, target_version: i64) -> Result<usize, String> {
        let candidates = self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT me.entity_hash, mf.mime_type, mf.frame_count
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE mf.color_analysis_version < ?1
                    OR (mf.color_analysis_version >= ?1 AND mf.dominant_palette_blob IS NULL)",
            )?;
            let rows = stmt
                .query_map([target_version], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;

        let items = candidates
            .into_iter()
            .filter_map(|(entity_hash, mime_type, frame_count)| {
                let caps = crate::media_capabilities::capabilities_for_stored_media(
                    &mime_type,
                    frame_count,
                );
                caps.can_dominant_colors.then_some((
                    entity_hash,
                    vec![crate::background_work::DeferredWorkType::DominantColors],
                ))
            })
            .collect::<Vec<_>>();

        let count = items.len();
        if count > 0 {
            self.ensure_deferred_jobs_present_batch(items)?;
        }
        Ok(count)
    }

    /// Get the entity_hash of a collection's primary member (first by ordinal).
    pub fn get_primary_member_hash(&self, collection_hash: &str) -> Result<Option<String>, String> {
        let h = collection_hash.to_string();
        self.with_read(|conn| {
            use rusqlite::OptionalExtension;
            conn.query_row(
                "SELECT pm.entity_hash FROM media_entity me
                 JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
                 WHERE me.entity_hash = ?1 AND me.entity_kind = 'collection'",
                [&h],
                |row| row.get(0),
            )
            .optional()
        })
    }
}
