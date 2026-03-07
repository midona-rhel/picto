//! Duplicate detection orchestration — perceptual hash scanning, pair
//! resolution (keep/delete/merge), and smart auto-merge with tag consolidation.
//!
//! Delegates to `sqlite::duplicates` for pair storage and `duplicates.rs` for
//! phash computation.

use std::collections::HashMap;

use rusqlite::OptionalExtension;

use crate::sqlite::compilers::CompilerEvent;
use crate::sqlite::SqliteDatabase;
use crate::types::{
    DuplicateInfo, DuplicatePairDto, DuplicatePairResponse, DuplicatePairsResponse,
    ScanDuplicatesResponse, SmartMergeResult,
};

pub struct DuplicateController;

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

impl DuplicateController {
    pub async fn get_duplicates(
        db: &SqliteDatabase,
        hash: String,
    ) -> Result<Vec<DuplicateInfo>, String> {
        let file_id = db.resolve_hash(&hash).await?;
        let pairs = db
            .with_read_conn(move |conn| {
                crate::duplicates::db::get_duplicates_for_file(conn, file_id)
            })
            .await?;

        let other_ids: Vec<i64> = pairs
            .iter()
            .map(|p| {
                if p.file_id_a == file_id {
                    p.file_id_b
                } else {
                    p.file_id_a
                }
            })
            .collect();
        let resolved = db.resolve_ids_batch(&other_ids).await?;
        let id_to_hash: HashMap<i64, String> = resolved.into_iter().collect();

        let result = pairs
            .iter()
            .filter_map(|pair| {
                let other_id = if pair.file_id_a == file_id {
                    pair.file_id_b
                } else {
                    pair.file_id_a
                };
                let other_hash = id_to_hash.get(&other_id)?.clone();
                Some(DuplicateInfo {
                    other_hash,
                    distance: pair.distance,
                    status: pair.status.clone(),
                })
            })
            .collect();
        Ok(result)
    }

    pub async fn get_all_detected_duplicates(
        db: &SqliteDatabase,
    ) -> Result<Vec<DuplicatePairResponse>, String> {
        let pairs = db
            .with_read_conn(crate::duplicates::db::get_all_detected_duplicates)
            .await?;

        let all_ids: Vec<i64> = pairs
            .iter()
            .flat_map(|p| [p.file_id_a, p.file_id_b])
            .collect();
        let resolved = db.resolve_ids_batch(&all_ids).await?;
        let id_to_hash: HashMap<i64, String> = resolved.into_iter().collect();

        let result = pairs
            .iter()
            .filter_map(|pair| {
                let hash_a = id_to_hash.get(&pair.file_id_a)?.clone();
                let hash_b = id_to_hash.get(&pair.file_id_b)?.clone();
                Some(DuplicatePairResponse {
                    hash_a,
                    hash_b,
                    distance: pair.distance,
                })
            })
            .collect();
        Ok(result)
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
        action: &str,
        hash_a: String,
        hash_b: String,
        _preferred_hash: Option<String>,
    ) -> Result<serde_json::Value, String> {
        match action {
            "smart_merge" => {
                let result = Self::smart_merge(db, &hash_a, &hash_b).await?;
                Ok(serde_json::to_value(&result).unwrap_or_default())
            }
            "keep_left" => {
                Self::keep_one(db, &hash_a, &hash_b, &hash_a).await?;
                Ok(serde_json::json!({ "kept": hash_a, "trashed": hash_b }))
            }
            "keep_right" => {
                Self::keep_one(db, &hash_a, &hash_b, &hash_b).await?;
                Ok(serde_json::json!({ "kept": hash_b, "trashed": hash_a }))
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
                db.emit_compiler_event(CompilerEvent::DuplicateChanged);
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
                db.emit_compiler_event(CompilerEvent::DuplicateChanged);
                Ok(serde_json::json!({ "status": "dismissed_keep_both" }))
            }
            _ => Err(format!(
                "Invalid action: {}. Must be smart_merge, keep_left, keep_right, not_duplicate, or keep_both.",
                action
            )),
        }
    }

    /// Smart merge: pick winner by deterministic scoring, merge metadata, trash loser.
    async fn smart_merge(
        db: &SqliteDatabase,
        hash_a: &str,
        hash_b: &str,
    ) -> Result<SmartMergeResult, String> {
        Self::smart_merge_with_source(db, hash_a, hash_b, "manual").await
    }

    /// Smart merge with a custom decision_source (e.g. "manual", "subscription_auto").
    async fn smart_merge_with_source(
        db: &SqliteDatabase,
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

        let total_views = winner_file.view_count + loser_file.view_count;
        if loser_file.view_count > 0 {
            let w_id = db.resolve_hash(&winner_hash).await?;
            db.with_conn(move |conn| {
                conn.execute(
                    "UPDATE file SET view_count = ?1 WHERE file_id = ?2",
                    rusqlite::params![total_views, w_id],
                )?;
                Ok(())
            })
            .await?;
        }

        let winner_fid = db.resolve_hash(&winner_hash).await?;
        let loser_fid = db.resolve_hash(&loser_hash).await?;
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

        if loser_in_collection {
            let w_fid = winner_fid;
            let l_fid = loser_fid;
            db.with_conn(move |conn| {
                crate::folders::collections_db::repoint_entity_to_file(conn, l_fid, w_fid)?;
                conn.execute("UPDATE file SET status = 2 WHERE file_id = ?1", [l_fid])?;
                Ok(())
            })
            .await?;
        } else {
            db.update_file_status(&loser_hash, 2).await?;
        }

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

        db.emit_compiler_event(CompilerEvent::FileTagsChanged {
            file_id: db.resolve_hash(&winner_hash).await?,
        });
        db.emit_compiler_event(CompilerEvent::DuplicateChanged);

        Ok(SmartMergeResult {
            winner_hash,
            loser_hash,
            tags_merged,
        })
    }

    /// Keep one file, trash the other.
    async fn keep_one(
        db: &SqliteDatabase,
        hash_a: &str,
        hash_b: &str,
        keep_hash: &str,
    ) -> Result<(), String> {
        let trash_hash = if keep_hash == hash_a { hash_b } else { hash_a };
        let reason = if keep_hash == hash_a {
            "Keep left"
        } else {
            "Keep right"
        };

        let winner_id = db.resolve_hash(keep_hash).await?;
        let loser_id = db.resolve_hash(trash_hash).await?;

        // If loser is a collection member, repoint to winner's file instead of trashing entity
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

        if loser_in_collection {
            let w_fid = winner_id;
            let l_fid = loser_id;
            db.with_conn(move |conn| {
                crate::folders::collections_db::repoint_entity_to_file(conn, l_fid, w_fid)?;
                conn.execute("UPDATE file SET status = 2 WHERE file_id = ?1", [l_fid])?;
                Ok(())
            })
            .await?;
        } else {
            db.update_file_status(trash_hash, 2).await?;
        }
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

        db.emit_compiler_event(CompilerEvent::DuplicateChanged);
        Ok(())
    }

    /// Check a newly imported file for near-duplicates and auto-merge if within threshold.
    ///
    /// Called from the subscription import pipeline after a file is imported and its
    /// phash stored. Builds a BK-tree over all existing phashes, queries for matches
    /// within `distance_threshold`, inserts duplicate pairs, and auto-merges the closest.
    pub async fn check_and_auto_merge(
        db: &SqliteDatabase,
        imported_hash: &str,
        distance_threshold: u32,
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
        db.with_conn({
            let hash_ref = imported_hash_for_pairs.clone();
            move |conn| {
                let new_fid: i64 = conn.query_row(
                    "SELECT file_id FROM file WHERE hash = ?1",
                    [&hash_ref],
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
                    crate::duplicates::db::insert_duplicate(conn, a, b, dist.clone() as f64)?;
                }
                Ok(())
            }
        })
        .await?;

        db.emit_compiler_event(CompilerEvent::DuplicateChanged);

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

        tracing::info!(
            imported = %imported_hash,
            closest = %closest_hash,
            distance = closest_dist,
            total_matches = matches.len(),
            "Auto-merging duplicate from subscription import"
        );

        let result =
            Self::smart_merge_with_source(db, imported_hash, closest_hash, "subscription_auto")
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

        let phash_count = files_with_phash.len();
        if phash_count < 2 {
            return Ok(ScanDuplicatesResponse {
                candidates_found: 0,
                pairs_inserted: 0,
                reviewable_detected_total: reviewable_before,
                reviewable_detected_new: 0,
                total_files,
                files_with_phash: phash_count,
                closest_distance: None,
            });
        }

        let pairs: Vec<(i64, i64, u32)> = tokio::task::spawn_blocking(move || {
            let mut tree = BkTree::new();
            let mut parsed: Vec<(i64, String, ImageHash<Vec<u8>>)> =
                Vec::with_capacity(phash_count);

            for (file_id, file_hash, phash_b64) in &files_with_phash {
                if let Ok(h) = ImageHash::<Vec<u8>>::from_base64(phash_b64) {
                    parsed.push((*file_id, file_hash.clone(), h));
                }
            }

            let mut found_pairs: Vec<(i64, i64, u32)> = Vec::new();
            let mut seen = std::collections::HashSet::new();

            for (i, (file_id, _file_hash, phash)) in parsed.iter().enumerate() {
                if i > 0 {
                    let matches = tree.find_within(phash, distance_threshold);
                    for (match_hash, dist) in matches {
                        if let Some((match_fid, _, _)) =
                            parsed.iter().find(|(_, h, _)| h == &match_hash)
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
                tree.insert(_file_hash.clone(), phash.clone());
            }

            found_pairs
        })
        .await
        .map_err(|e| format!("Scan task error: {}", e))?;

        let candidates_found = pairs.len();
        let closest_distance = pairs.iter().map(|(_, _, d)| *d).min();
        let mut pairs_inserted = 0usize;

        if !pairs.is_empty() {
            let pairs_clone = pairs.clone();
            pairs_inserted = db
                .with_conn(move |conn| {
                    let mut inserted = 0usize;
                    for (a, b, dist) in pairs_clone {
                        if crate::duplicates::db::insert_duplicate_counted(
                            conn,
                            a,
                            b,
                            dist as f64,
                        )? {
                            inserted += 1;
                        }
                    }
                    Ok(inserted)
                })
                .await?;
        }

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

        if pairs_inserted > 0 {
            db.emit_compiler_event(CompilerEvent::DuplicateChanged);
        }

        Ok(ScanDuplicatesResponse {
            candidates_found,
            pairs_inserted,
            reviewable_detected_total,
            reviewable_detected_new,
            total_files,
            files_with_phash: phash_count,
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
