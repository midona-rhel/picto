//! Duplicate detection orchestration — perceptual hash scanning, pair
//! resolution (keep/delete/merge), and smart auto-merge with tag consolidation.
//!
//! Delegates to `sqlite::duplicates` for pair storage and `duplicates.rs` for
//! phash computation.

use std::collections::HashMap;

use rusqlite::OptionalExtension;

use crate::blob_store::BlobStore;
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::sqlite::ReadModelEvent;
use crate::sqlite::SqliteDatabase;
use crate::types::{
    DuplicatePairDto, DuplicatePairsResponse, ScanDuplicatesResponse, SmartMergeResult,
};

pub struct DuplicateOrchestrator;

/// Format priority for smart merge winner selection (higher = preferred).
fn format_priority(mime: &str) -> u32 {
    match mime {
        "image/png" => 5,
        "image/tiff" => 4,
        "image/webp" => 3,
        "image/jpeg" | "image/jpg" => 2,
        "image/gif" => 2,
        _ if mime.starts_with("video/") => 1,
        _ => 0,
    }
}

impl DuplicateOrchestrator {
    fn repoint_entity_relationships(
        conn: &rusqlite::Connection,
        winner_id: i64,
        loser_id: i64,
    ) -> rusqlite::Result<Vec<i64>> {
        conn.execute(
            "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id)
             SELECT subscription_id, ?1
             FROM subscription_entity
             WHERE entity_id = ?2",
            rusqlite::params![winner_id, loser_id],
        )?;
        conn.execute(
            "DELETE FROM subscription_entity WHERE entity_id = ?1",
            [loser_id],
        )?;

        let mut folder_stmt = conn.prepare_cached(
            "SELECT folder_id FROM folder_entity WHERE entity_id = ?1 ORDER BY folder_id",
        )?;
        let affected_folder_ids = folder_stmt
            .query_map([loser_id], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        conn.execute(
            "INSERT OR IGNORE INTO folder_entity (folder_id, entity_id, position_rank)
             SELECT fe.folder_id, ?1, fe.position_rank
             FROM folder_entity fe
             WHERE fe.entity_id = ?2
               AND NOT EXISTS (
                   SELECT 1
                   FROM folder_entity existing
                   WHERE existing.folder_id = fe.folder_id
                     AND existing.entity_id = ?1
               )",
            rusqlite::params![winner_id, loser_id],
        )?;
        conn.execute("DELETE FROM folder_entity WHERE entity_id = ?1", [loser_id])?;

        Ok(affected_folder_ids)
    }

    /// Get paginated duplicate pairs.
    pub async fn get_duplicate_pairs(
        db: &SqliteDatabase,
        cursor: Option<String>,
        limit: usize,
        status: Option<String>,
        max_distance: Option<f64>,
    ) -> Result<DuplicatePairsResponse, String> {
        let status_filter = status.unwrap_or_else(|| "detected".into());
        let cursor_clone = cursor.clone();
        let limit_val = limit.min(200).max(1);
        let max_distance_filter = max_distance;

        let (pairs, next_cursor, total) = db
            .with_read_conn(move |conn| {
                crate::duplicates::db::get_duplicate_pairs_paginated(
                    conn,
                    cursor_clone.as_deref(),
                    limit_val,
                    &status_filter,
                    max_distance_filter,
                )
            })
            .await?;

        let all_ids: Vec<i64> = pairs
            .iter()
            .flat_map(|p| [p.file_id_a, p.file_id_b])
            .collect();
        let resolved = db.resolve_ids_batch(&all_ids).await?;
        let id_to_hash: HashMap<i64, String> = resolved.into_iter().collect();

        let items: Vec<DuplicatePairDto> = pairs
            .iter()
            .filter_map(|pair| {
                let hash_a = id_to_hash.get(&pair.file_id_a)?.clone();
                let hash_b = id_to_hash.get(&pair.file_id_b)?.clone();
                let similarity_pct = ((1.0 - pair.distance / 64.0) * 100.0).round();
                Some(DuplicatePairDto {
                    hash_a,
                    hash_b,
                    distance: pair.distance,
                    similarity_pct,
                    status: pair.status.clone(),
                })
            })
            .collect();

        let has_more = next_cursor.is_some();
        Ok(DuplicatePairsResponse {
            items,
            next_cursor,
            has_more,
            total,
        })
    }

    /// Count detected duplicate pairs (for sidebar).
    pub async fn get_duplicate_count(db: &SqliteDatabase) -> Result<i64, String> {
        db.with_read_conn(|conn| crate::duplicates::db::count_by_status(conn, "detected"))
            .await
    }

    /// Resolve a duplicate pair with an action.
    pub async fn resolve_duplicate_pair(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        action: &str,
        hash_a: String,
        hash_b: String,
    ) -> Result<serde_json::Value, String> {
        match action {
            "smart_merge" => {
                let result = Self::smart_merge(db, blob_store, &hash_a, &hash_b).await?;
                Ok(serde_json::to_value(&result).unwrap_or_default())
            }
            "keep_left" => {
                Self::keep_one(db, blob_store, &hash_a, &hash_b, &hash_a).await?;
                Ok(serde_json::json!({ "kept": hash_a, "deleted": hash_b }))
            }
            "keep_right" => {
                Self::keep_one(db, blob_store, &hash_a, &hash_b, &hash_b).await?;
                Ok(serde_json::json!({ "kept": hash_b, "deleted": hash_a }))
            }
            "not_duplicate" => {
                let id_a = db.resolve_hash(&hash_a).await?;
                let id_b = db.resolve_hash(&hash_b).await?;
                db.with_conn(move |conn| {
                    crate::duplicates::db::resolve_pair_with_decision(
                        conn,
                        id_a,
                        id_b,
                        "ignored_false_positive",
                        "manual",
                        "User marked as not duplicate",
                        None,
                        None,
                    )
                })
                .await?;
                db.emit_read_model_event(ReadModelEvent::DuplicateChanged);
                Ok(serde_json::json!({ "status": "ignored_false_positive" }))
            }
            "keep_both" => {
                let id_a = db.resolve_hash(&hash_a).await?;
                let id_b = db.resolve_hash(&hash_b).await?;
                db.with_conn(move |conn| {
                    crate::duplicates::db::resolve_pair_with_decision(
                        conn,
                        id_a,
                        id_b,
                        "dismissed_keep_both",
                        "manual",
                        "User chose to keep both",
                        None,
                        None,
                    )
                })
                .await?;
                db.emit_read_model_event(ReadModelEvent::DuplicateChanged);
                Ok(serde_json::json!({ "status": "dismissed_keep_both" }))
            }
            _ => Err(format!(
                "Invalid action: {}. Must be smart_merge, keep_left, keep_right, not_duplicate, or keep_both.",
                action
            )),
        }
    }

    /// Smart merge: pick winner by deterministic scoring, merge metadata, delete loser.
    async fn smart_merge(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        hash_a: &str,
        hash_b: &str,
    ) -> Result<SmartMergeResult, String> {
        Self::smart_merge_with_source(db, blob_store, hash_a, hash_b, "manual").await
    }

    /// Smart merge with a custom decision_source (e.g. "manual", "subscription_auto").
    async fn smart_merge_with_source(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        hash_a: &str,
        hash_b: &str,
        decision_source: &str,
    ) -> Result<SmartMergeResult, String> {
        let id_a = db.resolve_hash(hash_a).await?;
        let id_b = db.resolve_hash(hash_b).await?;

        let (file_a, file_b) = db
            .with_read_conn(move |conn| {
                let a = crate::sqlite::files::get_file_by_id(conn, id_a)?
                    .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
                let b = crate::sqlite::files::get_file_by_id(conn, id_b)?
                    .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
                Ok((a, b))
            })
            .await?;

        let tags_a = db.get_entity_tags(hash_a).await?;
        let tags_b = db.get_entity_tags(hash_b).await?;

        let pixels_a = file_a
            .width
            .unwrap_or(0)
            .saturating_mul(file_a.height.unwrap_or(0));
        let pixels_b = file_b
            .width
            .unwrap_or(0)
            .saturating_mul(file_b.height.unwrap_or(0));

        let fmt_a = format_priority(&file_a.mime);
        let fmt_b = format_priority(&file_b.mime);

        // Quality proxy: bytes per pixel (higher = more detail retained)
        let bpp_a = if pixels_a > 0 {
            file_a.size as f64 / pixels_a as f64
        } else {
            0.0
        };
        let bpp_b = if pixels_b > 0 {
            file_b.size as f64 / pixels_b as f64
        } else {
            0.0
        };

        // Metadata richness: count of non-null metadata fields
        let richness_a = tags_a.len()
            + file_a.notes.as_ref().map_or(0, |_| 1)
            + file_a.source_urls_json.as_ref().map_or(0, |_| 1)
            + file_a.rating.map_or(0, |_| 1);
        let richness_b = tags_b.len()
            + file_b.notes.as_ref().map_or(0, |_| 1)
            + file_b.source_urls_json.as_ref().map_or(0, |_| 1)
            + file_b.rating.map_or(0, |_| 1);

        let a_wins = (
            pixels_a,
            fmt_a,
            (bpp_a * 1000.0) as i64,
            richness_a,
            -(file_a.file_id),
        );
        let b_wins = (
            pixels_b,
            fmt_b,
            (bpp_b * 1000.0) as i64,
            richness_b,
            -(file_b.file_id),
        );

        let (winner_hash, loser_hash, winner_file, loser_file, winner_tags, loser_tags) =
            if a_wins >= b_wins {
                (
                    hash_a.to_string(),
                    hash_b.to_string(),
                    &file_a,
                    &file_b,
                    &tags_a,
                    &tags_b,
                )
            } else {
                (
                    hash_b.to_string(),
                    hash_a.to_string(),
                    &file_b,
                    &file_a,
                    &tags_b,
                    &tags_a,
                )
            };

        // Merge metadata onto winner: tags, source URLs, notes, rating, view count
        let winner_tag_set: std::collections::HashSet<(String, String)> = winner_tags
            .iter()
            .map(|t| (t.namespace.clone(), t.subtag.clone()))
            .collect();
        let new_tags: Vec<String> = loser_tags
            .iter()
            .filter(|t| !winner_tag_set.contains(&(t.namespace.clone(), t.subtag.clone())))
            .map(|t| {
                if t.namespace.is_empty() {
                    t.subtag.clone()
                } else {
                    format!("{}:{}", t.namespace, t.subtag)
                }
            })
            .collect();
        let tags_merged = new_tags.len();
        if !new_tags.is_empty() {
            db.add_tags_by_strings(&winner_hash, &new_tags).await?;
        }

        let merged_urls = merge_source_urls(
            winner_file.source_urls_json.as_deref(),
            loser_file.source_urls_json.as_deref(),
        );
        if let Some(ref urls_json) = merged_urls {
            db.set_source_urls(&winner_hash, Some(urls_json)).await?;
        }

        let merged_notes = merge_notes(winner_file.notes.as_deref(), loser_file.notes.as_deref());
        if let Some(ref notes_json) = merged_notes {
            db.set_notes(&winner_hash, Some(notes_json)).await?;
        }

        if let Some(loser_rating) = loser_file.rating {
            let winner_rating = winner_file.rating.unwrap_or(0);
            if loser_rating > winner_rating {
                db.update_rating(&winner_hash, Some(loser_rating)).await?;
            }
        }

        // Consolidate timestamps: preserve earliest dates, mark as modified
        let winner_fid = db.resolve_hash(&winner_hash).await?;
        let loser_fid = db.resolve_hash(&loser_hash).await?;
        consolidate_merge_timestamps(
            db,
            &winner_hash,
            winner_fid,
            loser_fid,
            &winner_file.imported_at,
            &loser_file.imported_at,
        )
        .await?;
        let loser_in_collection: bool = db
            .with_read_conn(move |conn| {
                let parent: Option<i64> = conn
                    .query_row(
                        "SELECT parent_collection_id FROM media_entity WHERE entity_id = ?1",
                        [loser_fid],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                Ok(parent.is_some())
            })
            .await?;

        // Record the decision before deleting the loser (deletion cascades the duplicate row)
        let winner_id = db.resolve_hash(&winner_hash).await?;
        let loser_id = db.resolve_hash(&loser_hash).await?;
        let source_owned = decision_source.to_string();
        db.with_conn(move |conn| {
            crate::duplicates::db::resolve_pair_with_decision(
                conn,
                winner_id,
                loser_id,
                "confirmed_merged",
                &source_owned,
                "Smart merge",
                Some(winner_id),
                Some(loser_id),
            )
        })
        .await?;

        let affected_folder_ids = if loser_in_collection {
            let w_fid = winner_fid;
            let l_fid = loser_fid;
            db.with_conn(move |conn| {
                crate::folders::collections_db::repoint_entity_to_file(conn, l_fid, w_fid)?;
                let folder_ids = Self::repoint_entity_relationships(conn, w_fid, l_fid)?;
                crate::sqlite::files::delete_file(conn, l_fid)?;
                Ok(folder_ids)
            })
            .await?
        } else {
            let w_fid = winner_fid;
            let l_fid = loser_fid;
            let folder_ids = db
                .with_conn(move |conn| Self::repoint_entity_relationships(conn, w_fid, l_fid))
                .await?;
            db.delete_file_by_hash(&loser_hash).await?;
            folder_ids
        };
        blob_store.delete(&loser_hash).map_err(|e| e.to_string())?;

        db.emit_read_model_event(ReadModelEvent::FileTagsChanged { file_id: winner_id });
        db.emit_read_model_event(ReadModelEvent::FileDeleted { file_id: loser_id });
        for &folder_id in &affected_folder_ids {
            db.emit_read_model_event(ReadModelEvent::FolderChanged { folder_id });
        }
        db.emit_read_model_event(ReadModelEvent::DuplicateChanged);

        let mut impact = ChangeImpact::file_lifecycle(db)
            .entity_hashes(vec![winner_hash.clone(), loser_hash.clone()]);
        if !affected_folder_ids.is_empty() {
            impact = impact.folder_membership_changed(affected_folder_ids.clone());
        }
        if tags_merged > 0 {
            impact = impact.tags_changed().all_smart_folder_scopes_changed();
        }
        crate::events::emit_state_changed("resolve_duplicate_pair", impact);

        Ok(SmartMergeResult {
            winner_hash,
            loser_hash,
            tags_merged,
        })
    }

    /// Keep one file, delete the other.
    async fn keep_one(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        hash_a: &str,
        hash_b: &str,
        keep_hash: &str,
    ) -> Result<(), String> {
        let delete_hash = if keep_hash == hash_a { hash_b } else { hash_a };
        let reason = if keep_hash == hash_a {
            "Keep left"
        } else {
            "Keep right"
        };

        let winner_id = db.resolve_hash(keep_hash).await?;
        let loser_id = db.resolve_hash(delete_hash).await?;

        // If loser is a collection member, repoint to winner's file instead of deleting entity
        let l_id = loser_id;
        let loser_in_collection: bool = db
            .with_read_conn(move |conn| {
                let parent: Option<i64> = conn
                    .query_row(
                        "SELECT parent_collection_id FROM media_entity WHERE entity_id = ?1",
                        [l_id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                Ok(parent.is_some())
            })
            .await?;

        // Record the decision before deleting (deletion cascades the duplicate row)
        let reason_owned = reason.to_string();
        db.with_conn(move |conn| {
            crate::duplicates::db::resolve_pair_with_decision(
                conn,
                winner_id,
                loser_id,
                "confirmed_merged",
                "manual",
                &reason_owned,
                Some(winner_id),
                Some(loser_id),
            )
        })
        .await?;

        // Consolidate timestamps before deletion
        {
            let w_id = winner_id;
            let l_id = loser_id;
            let (w_imported, l_imported) = db
                .with_read_conn(move |conn| {
                    let wi: String = conn.query_row(
                        "SELECT imported_at FROM file WHERE file_id = ?1",
                        [w_id],
                        |row| row.get(0),
                    )?;
                    let li: String = conn.query_row(
                        "SELECT imported_at FROM file WHERE file_id = ?1",
                        [l_id],
                        |row| row.get(0),
                    )?;
                    Ok((wi, li))
                })
                .await?;
            consolidate_merge_timestamps(
                db,
                keep_hash,
                winner_id,
                loser_id,
                &w_imported,
                &l_imported,
            )
            .await?;
        }

        let affected_folder_ids = if loser_in_collection {
            let w_fid = winner_id;
            let l_fid = loser_id;
            db.with_conn(move |conn| {
                crate::folders::collections_db::repoint_entity_to_file(conn, l_fid, w_fid)?;
                let folder_ids = Self::repoint_entity_relationships(conn, w_fid, l_fid)?;
                crate::sqlite::files::delete_file(conn, l_fid)?;
                Ok(folder_ids)
            })
            .await?
        } else {
            let w_fid = winner_id;
            let l_fid = loser_id;
            let folder_ids = db
                .with_conn(move |conn| Self::repoint_entity_relationships(conn, w_fid, l_fid))
                .await?;
            db.delete_file_by_hash(delete_hash).await?;
            folder_ids
        };
        blob_store.delete(delete_hash).map_err(|e| e.to_string())?;

        db.emit_read_model_event(ReadModelEvent::FileDeleted { file_id: loser_id });
        for &folder_id in &affected_folder_ids {
            db.emit_read_model_event(ReadModelEvent::FolderChanged { folder_id });
        }
        db.emit_read_model_event(ReadModelEvent::DuplicateChanged);

        let mut impact = ChangeImpact::file_lifecycle(db)
            .entity_hashes(vec![keep_hash.to_string(), delete_hash.to_string()]);
        if !affected_folder_ids.is_empty() {
            impact = impact.folder_membership_changed(affected_folder_ids);
        }
        crate::events::emit_state_changed("resolve_duplicate_pair", impact);
        Ok(())
    }

    /// Find all images sorted by visual similarity (phash distance) to a source image.
    /// No threshold cutoff — returns every image with a phash, sorted closest-first.
    /// If the source image has no phash, computes it on-the-fly from the blob store.
    pub async fn find_similar(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        source_hash: &str,
    ) -> Result<crate::types::FindSimilarResponse, String> {
        use img_hash::ImageHash;

        let source_hash_owned = source_hash.to_string();

        // Check if source has a phash; compute one if missing.
        let source_phash_b64: Option<String> = db
            .with_read_conn({
                let h = source_hash_owned.clone();
                move |conn| {
                    conn.query_row("SELECT phash FROM file WHERE hash = ?1", [&h], |row| {
                        row.get::<_, Option<String>>(0)
                    })
                    .optional()
                    .map(|o| o.flatten())
                }
            })
            .await?;

        let source_phash_b64 = match source_phash_b64 {
            Some(b64) => b64,
            None => {
                // Compute phash from the original file
                let file = db
                    .get_file_by_hash(source_hash)
                    .await?
                    .ok_or_else(|| format!("File not found: {source_hash}"))?;
                let ext = crate::blob_store::mime_to_extension(&file.mime).to_string();
                let h = source_hash.to_string();
                let bs_path = blob_store
                    .find_original(&h, Some(&ext))
                    .map_err(|e| format!("{e}"))?
                    .ok_or_else(|| format!("Original not found for {h}"))?;
                let bytes = std::fs::read(&bs_path.0).map_err(|e| format!("Read error: {e}"))?;
                let b64 = crate::duplicates::phash::compute_phash_base64(&bytes)
                    .map_err(|e| format!("Phash compute failed: {e}"))?;
                // Store for future use
                let _ = db.set_phash(&h, &b64).await;
                b64
            }
        };

        let files_with_phash: Vec<(String, String)> = db
            .with_read_conn(|conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT f.hash, f.phash FROM file f
                     WHERE f.phash IS NOT NULL AND f.status IN (0, 1)",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.collect()
            })
            .await?;

        let mut results: Vec<(String, u32)> = tokio::task::spawn_blocking(move || {
            let Ok(source) = ImageHash::<Vec<u8>>::from_base64(&source_phash_b64) else {
                return Vec::new();
            };

            let mut distances: Vec<(String, u32)> = files_with_phash
                .iter()
                .filter_map(|(hash, phash_b64)| {
                    if hash == &source_hash_owned {
                        return None;
                    }
                    let ph = ImageHash::<Vec<u8>>::from_base64(phash_b64).ok()?;
                    Some((hash.clone(), source.dist(&ph)))
                })
                .collect();

            distances.sort_by_key(|(_, d)| *d);
            distances
        })
        .await
        .map_err(|e| format!("find_similar task failed: {e}"))?;

        let items: Vec<crate::types::SimilarItem> = results
            .drain(..)
            .map(|(hash, distance)| crate::types::SimilarItem { hash, distance })
            .collect();

        Ok(crate::types::FindSimilarResponse {
            source_hash: source_hash.to_string(),
            items,
        })
    }

    /// Check a newly imported file for near-duplicates and auto-merge if within threshold.
    ///
    /// Called from the subscription import pipeline after a file is imported and its
    /// phash stored. Builds a BK-tree over all existing phashes, queries for matches
    /// within `distance_threshold`, inserts duplicate pairs, and auto-merges the closest.
    pub async fn check_and_auto_merge(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        imported_hash: &str,
        distance_threshold: u32,
        require_matching_dimensions: bool,
    ) -> Result<Option<SmartMergeResult>, String> {
        use crate::duplicates::phash::BkTree;
        use img_hash::ImageHash;

        let imported_hash_owned = imported_hash.to_string();

        let files_with_phash: Vec<(i64, String, String)> = db
            .with_read_conn(|conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT f.file_id, f.hash, f.phash FROM file f
                     WHERE f.phash IS NOT NULL AND f.status IN (0, 1)",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?;
                rows.collect()
            })
            .await?;

        if files_with_phash.len() < 2 {
            return Ok(None);
        }

        let threshold = distance_threshold;
        let matches: Vec<(String, u32)> = tokio::task::spawn_blocking(move || {
            let mut tree = BkTree::new();
            let mut new_file_phash: Option<ImageHash<Vec<u8>>> = None;

            for (_, file_hash, phash_b64) in &files_with_phash {
                if let Ok(h) = ImageHash::<Vec<u8>>::from_base64(phash_b64) {
                    if file_hash == &imported_hash_owned {
                        new_file_phash = Some(h.clone());
                    }
                    tree.insert(file_hash.clone(), h);
                }
            }

            let Some(query_phash) = new_file_phash else {
                return Vec::new();
            };

            tree.find_within(&query_phash, threshold)
                .into_iter()
                .filter(|(h, _)| h != &imported_hash_owned) // exclude self
                .collect()
        })
        .await
        .map_err(|e| format!("BK-tree task error: {}", e))?;

        if matches.is_empty() {
            return Ok(None);
        }

        let imported_hash_for_pairs = imported_hash.to_string();
        let matches_for_pairs = matches.clone();
        db.with_conn(move |conn| {
            let new_fid: i64 = conn.query_row(
                "SELECT file_id FROM file WHERE hash = ?1",
                [&imported_hash_for_pairs],
                |row| row.get(0),
            )?;
            for (match_hash, dist) in &matches_for_pairs {
                let match_fid: i64 = conn.query_row(
                    "SELECT file_id FROM file WHERE hash = ?1",
                    [match_hash],
                    |row| row.get(0),
                )?;
                let (a, b) = if new_fid < match_fid {
                    (new_fid, match_fid)
                } else {
                    (match_fid, new_fid)
                };
                // Insert if not already present (ON CONFLICT IGNORE)
                crate::duplicates::db::insert_duplicate(conn, a, b, *dist as f64)?;
            }
            Ok(())
        })
        .await?;

        db.emit_read_model_event(ReadModelEvent::DuplicateChanged);

        let (closest_hash, closest_dist) = matches.iter().min_by_key(|(_, d)| *d).unwrap();

        if *closest_dist != 0 {
            tracing::info!(
                imported = %imported_hash,
                closest = %closest_hash,
                distance = closest_dist,
                total_matches = matches.len(),
                "Auto-merge skipped: only exact (distance=0) matches are merged"
            );
            return Ok(None);
        }

        if require_matching_dimensions {
            let imported_id = db.resolve_hash(imported_hash).await?;
            let closest_id = db.resolve_hash(closest_hash).await?;
            let ((imported_width, imported_height), (closest_width, closest_height)) = db
                .with_read_conn(move |conn| {
                    let imported = crate::sqlite::files::get_file_by_id(conn, imported_id)?
                        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
                    let closest = crate::sqlite::files::get_file_by_id(conn, closest_id)?
                        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
                    Ok((
                        (imported.width, imported.height),
                        (closest.width, closest.height),
                    ))
                })
                .await?;

            if imported_width != closest_width || imported_height != closest_height {
                tracing::info!(
                    imported = %imported_hash,
                    closest = %closest_hash,
                    imported_width = ?imported_width,
                    imported_height = ?imported_height,
                    closest_width = ?closest_width,
                    closest_height = ?closest_height,
                    "Auto-merge skipped: matching dimensions required"
                );
                return Ok(None);
            }
        }

        tracing::info!(
            imported = %imported_hash,
            closest = %closest_hash,
            distance = closest_dist,
            total_matches = matches.len(),
            "Auto-merging duplicate from subscription import"
        );

        let result = Self::smart_merge_with_source(
            db,
            blob_store,
            imported_hash,
            closest_hash,
            "subscription_auto",
        )
        .await?;

        crate::events::emit(
            crate::events::event_names::DUPLICATE_AUTO_MERGE_FINISHED,
            &crate::events::DuplicateAutoMergeFinishedEvent {
                winner_hash: result.winner_hash.clone(),
                loser_hash: result.loser_hash.clone(),
                distance: *closest_dist,
                tags_merged: result.tags_merged,
            },
        );

        Ok(Some(result))
    }

    /// Scan all files with phashes, build a BK-tree, and insert new duplicate pairs.
    pub async fn scan_duplicates(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        threshold: Option<u32>,
        review_threshold: Option<u32>,
    ) -> Result<ScanDuplicatesResponse, String> {
        use crate::duplicates::phash::{BkTree, DEFAULT_DISTANCE_THRESHOLD};
        use img_hash::ImageHash;

        let distance_threshold = threshold.unwrap_or(DEFAULT_DISTANCE_THRESHOLD);
        let review_distance_threshold = review_threshold.unwrap_or(distance_threshold);
        let review_distance = review_distance_threshold as f64;
        let total_files = db.count_files(None).await? as usize;
        let reviewable_before = db
            .with_read_conn(move |conn| {
                crate::duplicates::db::count_by_status_with_max_distance(
                    conn,
                    "detected",
                    review_distance,
                )
            })
            .await? as usize;

        // Epoch caching: only query files imported after the last scan
        let (last_scan_at, last_scan_threshold) = db
            .with_read_conn(|conn| crate::duplicates::db::get_last_duplicate_scan(conn))
            .await?;
        // Force full scan if threshold changed
        let effective_last_scan_at = if last_scan_threshold == Some(distance_threshold) {
            last_scan_at
        } else {
            None
        };

        let mut files_with_phash: Vec<(i64, String, String, String)> = db
            .with_read_conn(|conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT f.file_id, f.hash, f.phash, f.imported_at FROM file f
                     WHERE f.phash IS NOT NULL AND f.status IN (0, 1)",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?;
                rows.collect()
            })
            .await?;

        // Upgrade stale phashes: old 8x8 hashes decode to 8 bytes, new 16x16 to 32 bytes.
        // Recompute from thumbnail if the decoded size is wrong.
        {
            const EXPECTED_HASH_BYTES: usize = 32; // 16x16 = 256 bits = 32 bytes
            let stale_count = files_with_phash
                .iter()
                .filter(|(_, _, p, _)| {
                    ImageHash::<Vec<u8>>::from_base64(p)
                        .map(|h| h.as_bytes().len() != EXPECTED_HASH_BYTES)
                        .unwrap_or(true)
                })
                .count();
            if stale_count > 0 {
                tracing::info!(
                    stale_count,
                    total = files_with_phash.len(),
                    "upgrading stale phashes to 16x16"
                );
                // Collect stale entries to recompute
                let stale_indices: Vec<usize> = files_with_phash
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, _, p, _))| {
                        ImageHash::<Vec<u8>>::from_base64(p)
                            .map(|h| h.as_bytes().len() != EXPECTED_HASH_BYTES)
                            .unwrap_or(true)
                    })
                    .map(|(i, _)| i)
                    .collect();

                let bs = blob_store;
                for idx in stale_indices {
                    let (file_id, ref hex_hash, ref mut phash_b64, _) = files_with_phash[idx];
                    let thumb_data = bs
                        .find_thumbnail_path(hex_hash)
                        .ok()
                        .flatten()
                        .and_then(|p| std::fs::read(&p).ok());
                    if let Some(data) = thumb_data {
                        if let Ok(new_b64) = crate::duplicates::phash::compute_phash_base64(&data) {
                            let fid = file_id;
                            let stored = new_b64.clone();
                            let _ = db
                                .with_conn(move |conn| {
                                    crate::sqlite::files::set_phash(conn, fid, &stored)
                                })
                                .await;
                            *phash_b64 = new_b64;
                        }
                    }
                }
            }
        }

        let phash_count = files_with_phash.len();
        tracing::info!(
            threshold = distance_threshold,
            total_files,
            "duplicate scan starting"
        );

        if phash_count < 2 {
            return Ok(ScanDuplicatesResponse {
                candidates_found: 0,
                pairs_inserted: 0,
                reviewable_detected_total: reviewable_before,
                reviewable_detected_new: 0,
                total_files,
                files_with_phash: phash_count,
                files_scanned: 0,
                closest_distance: None,
            });
        }

        let cutoff = effective_last_scan_at.clone();
        let (pairs, files_scanned): (Vec<(i64, i64, u32)>, usize) =
            tokio::task::spawn_blocking(move || {
                let mut tree = BkTree::new();
                let mut parsed: Vec<(i64, String, ImageHash<Vec<u8>>, String)> =
                    Vec::with_capacity(phash_count);

                for (file_id, file_hash, phash_b64, imported_at) in &files_with_phash {
                    if let Ok(h) = ImageHash::<Vec<u8>>::from_base64(phash_b64) {
                        parsed.push((*file_id, file_hash.clone(), h, imported_at.clone()));
                    }
                }

                let mut found_pairs: Vec<(i64, i64, u32)> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut scanned = 0usize;

                for (i, (file_id, _file_hash, phash, imported_at)) in parsed.iter().enumerate() {
                    if i > 0 {
                        let is_new = cutoff
                            .as_ref()
                            .map_or(true, |c| imported_at.as_str() > c.as_str());
                        if is_new {
                            scanned += 1;
                            let matches = tree.find_within(phash, distance_threshold);
                            for (match_hash, dist) in matches {
                                if let Some((match_fid, _, _, _)) =
                                    parsed.iter().find(|(_, h, _, _)| h == &match_hash)
                                {
                                    let (a, b) = if *file_id < *match_fid {
                                        (*file_id, *match_fid)
                                    } else {
                                        (*match_fid, *file_id)
                                    };
                                    if seen.insert((a, b)) {
                                        found_pairs.push((a, b, dist));
                                    }
                                }
                            }
                        }
                    }
                    tree.insert(_file_hash.clone(), phash.clone());
                }

                (found_pairs, scanned)
            })
            .await
            .map_err(|e| format!("Scan task error: {}", e))?;

        let candidates_found = pairs.len();
        let closest_distance = pairs.iter().map(|(_, _, d)| *d).min();
        let mut pairs_inserted = 0usize;

        if !pairs.is_empty() {
            pairs_inserted = db
                .with_conn(move |conn| {
                    let mut inserted = 0usize;
                    for (a, b, dist) in pairs {
                        if crate::duplicates::db::insert_duplicate_counted(conn, a, b, dist as f64)?
                        {
                            inserted += 1;
                        }
                    }
                    Ok(inserted)
                })
                .await?;
        }

        // Safety net: reset confirmed_merged pairs where the loser is still active
        let pairs_reset = db
            .with_conn(|conn| crate::duplicates::db::reset_stale_merged_pairs(conn))
            .await?;
        if pairs_reset > 0 {
            tracing::warn!(
                pairs_reset,
                "reset stale confirmed_merged pairs where loser was still active"
            );
        }

        if pairs_inserted > 0 || pairs_reset > 0 {
            db.emit_read_model_event(ReadModelEvent::DuplicateChanged);
        }

        // Auto-merge exact duplicates (distance=0) detected during this scan
        let exact_pairs: Vec<(i64, i64)> = db
            .with_read_conn(|conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT d.file_id_a, d.file_id_b FROM duplicate d
                     WHERE d.status = 'detected' AND d.distance = 0.0
                       AND EXISTS (SELECT 1 FROM file WHERE file_id = d.file_id_a AND status IN (0, 1))
                       AND EXISTS (SELECT 1 FROM file WHERE file_id = d.file_id_b AND status IN (0, 1))",
                )?;
                let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
                rows.collect()
            })
            .await?;
        let mut auto_merged = 0usize;
        for (fid_a, fid_b) in &exact_pairs {
            let hash_a = match db.resolve_id(*fid_a).await {
                Ok(h) => h,
                Err(_) => continue,
            };
            let hash_b = match db.resolve_id(*fid_b).await {
                Ok(h) => h,
                Err(_) => continue,
            };
            match Self::smart_merge_with_source(db, blob_store, &hash_a, &hash_b, "scan_auto").await
            {
                Ok(result) => {
                    tracing::info!(
                        winner = %result.winner_hash,
                        loser = %result.loser_hash,
                        tags_merged = result.tags_merged,
                        "auto-merged exact duplicate during scan"
                    );
                    auto_merged += 1;
                }
                Err(e) => {
                    tracing::warn!(hash_a = %hash_a, hash_b = %hash_b, error = %e, "scan auto-merge failed");
                }
            }
        }
        if auto_merged > 0 {
            db.emit_read_model_event(ReadModelEvent::DuplicateChanged);
        }

        // Count reviewable after both inserts and resets
        let review_distance = review_distance_threshold as f64;
        let reviewable_detected_total = db
            .with_read_conn(move |conn| {
                crate::duplicates::db::count_by_status_with_max_distance(
                    conn,
                    "detected",
                    review_distance,
                )
            })
            .await? as usize;
        let reviewable_detected_new = reviewable_detected_total.saturating_sub(reviewable_before);

        // Record scan epoch
        db.with_conn(move |conn| {
            crate::duplicates::db::set_last_duplicate_scan(conn, distance_threshold)
        })
        .await?;

        tracing::info!(
            candidates_found,
            pairs_inserted,
            pairs_reset,
            files_scanned,
            files_with_phash = phash_count,
            "duplicate scan complete"
        );

        Ok(ScanDuplicatesResponse {
            candidates_found,
            pairs_inserted,
            reviewable_detected_total,
            reviewable_detected_new,
            total_files,
            files_with_phash: phash_count,
            files_scanned,
            closest_distance,
        })
    }
}

fn merge_source_urls(winner_json: Option<&str>, loser_json: Option<&str>) -> Option<String> {
    let parse = |json: Option<&str>| -> Vec<String> {
        json.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default()
    };

    let mut winner_urls = parse(winner_json);
    let loser_urls = parse(loser_json);

    if loser_urls.is_empty() {
        return None; // nothing to merge
    }

    let existing: std::collections::HashSet<String> = winner_urls.iter().cloned().collect();
    for url in loser_urls {
        if !existing.contains(&url) {
            winner_urls.push(url);
        }
    }

    Some(serde_json::to_string(&winner_urls).unwrap_or_else(|_| "[]".into()))
}

fn merge_notes(winner_json: Option<&str>, loser_json: Option<&str>) -> Option<String> {
    let parse = |json: Option<&str>| -> serde_json::Map<String, serde_json::Value> {
        json.and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    };

    let mut winner_notes = parse(winner_json);
    let loser_notes = parse(loser_json);

    if loser_notes.is_empty() {
        return None; // nothing to merge
    }

    for (key, value) in loser_notes {
        winner_notes.entry(key).or_insert(value);
    }

    Some(serde_json::to_string(&winner_notes).unwrap_or_else(|_| "{}".into()))
}

/// Return the earlier of two ISO-8601 timestamp strings.
/// Lexicographic comparison is safe for RFC 3339 / ISO 8601 strings.
fn min_timestamp<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a <= b {
        a
    } else {
        b
    }
}

/// Consolidate timestamps during a merge: preserve the earliest dates on the
/// winner and mark it as modified. Must be called before deleting the loser.
async fn consolidate_merge_timestamps(
    db: &SqliteDatabase,
    winner_hash: &str,
    winner_id: i64,
    loser_id: i64,
    winner_imported_at: &str,
    loser_imported_at: &str,
) -> Result<(), String> {
    // 1. imported_at → keep the earliest import date
    let min_imported = min_timestamp(winner_imported_at, loser_imported_at);
    if min_imported != winner_imported_at {
        db.set_date_added(winner_hash, min_imported).await?;
    }

    // 2. created_at → keep the earliest content date (lives on media_entity)
    let w_id = winner_id;
    let l_id = loser_id;
    let (w_created, l_created): (Option<String>, Option<String>) = db
        .with_read_conn(move |conn| {
            let wc: Option<String> = conn
                .query_row(
                    "SELECT created_at FROM media_entity WHERE entity_id = ?1",
                    [w_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            let lc: Option<String> = conn
                .query_row(
                    "SELECT created_at FROM media_entity WHERE entity_id = ?1",
                    [l_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            Ok((wc, lc))
        })
        .await?;

    let mut updated_via_created_at = false;
    match (&w_created, &l_created) {
        (Some(wc), Some(lc)) => {
            let min_created = min_timestamp(wc, lc);
            if min_created != wc.as_str() {
                // set_date_created also sets updated_at = CURRENT_TIMESTAMP
                db.set_date_created(winner_hash, min_created).await?;
                updated_via_created_at = true;
            }
        }
        (None, Some(lc)) => {
            db.set_date_created(winner_hash, lc).await?;
            updated_via_created_at = true;
        }
        _ => {}
    }

    // 3. Touch updated_at if not already done by set_date_created
    if !updated_via_created_at {
        db.touch_date_modified(winner_hash).await?;
    }

    Ok(())
}
