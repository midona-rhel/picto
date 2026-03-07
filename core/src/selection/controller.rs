//! Selection orchestration — batch operations on file selections and
//! selection summary computation (tag counts, shared tags, stats).
//!
//! Supports both `ExplicitHashes` (user-picked files) and `AllResults`
//! (current grid scope) selection modes.

use std::collections::{HashMap, HashSet};

use chrono::Utc;

use crate::selection::helpers::{
    selection_bitmap_for_all_results, summarize_hashes_bulk, summarize_stats_from_bitmap,
    summarize_tags_from_bitmap,
};
use crate::sqlite::SqliteDatabase;
use crate::types::{
    SelectionMode, SelectionQuerySpec, SelectionSummary, SelectionSummaryStats, SelectionTagCount,
};

pub struct SelectionController;

impl SelectionController {
    pub async fn add_tags_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        tag_strings: Vec<String>,
    ) -> Result<usize, String> {
        if tag_strings.is_empty() {
            return Ok(0);
        }

        let file_ids = Self::collect_file_ids(db, &selection).await?;
        if file_ids.is_empty() {
            return Ok(0);
        }
        let affected = file_ids.len();
        db.add_tags_batch_by_entity_ids(file_ids, tag_strings, "local".to_string())
            .await?;
        Ok(affected)
    }

    pub async fn remove_tags_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        tag_strings: Vec<String>,
    ) -> Result<usize, String> {
        if tag_strings.is_empty() {
            return Ok(0);
        }

        let file_ids = Self::collect_file_ids(db, &selection).await?;
        if file_ids.is_empty() {
            return Ok(0);
        }
        let affected = file_ids.len();
        db.remove_tags_batch_by_entity_ids(file_ids, tag_strings)
            .await?;
        Ok(affected)
    }

    async fn collect_file_ids(
        db: &SqliteDatabase,
        selection: &SelectionQuerySpec,
    ) -> Result<Vec<i64>, String> {
        let excluded: HashSet<String> = selection
            .excluded_hashes
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();

        match &selection.mode {
            SelectionMode::ExplicitHashes => {
                let hashes = selection.hashes.clone().unwrap_or_default();
                let filtered: Vec<String> = hashes
                    .into_iter()
                    .filter(|h| !excluded.contains(h))
                    .collect();
                // PBI-011: Use lightweight hash→file_id resolver instead of loading full records.
                let resolved = db.resolve_hashes_batch(&filtered).await?;
                let file_ids: Vec<i64> = resolved.into_iter().map(|(_, id)| id).collect();
                Ok(file_ids)
            }
            SelectionMode::AllResults => {
                let (_base_bm, filtered_bm) =
                    selection_bitmap_for_all_results(db, selection).await?;
                let file_ids: Vec<i64> = filtered_bm.iter().map(|id| id as i64).collect();
                Ok(file_ids)
            }
        }
    }

    pub async fn update_rating_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        rating: Option<i64>,
    ) -> Result<usize, String> {
        let file_ids = Self::collect_file_ids(db, &selection).await?;
        if file_ids.is_empty() {
            return Ok(0);
        }
        let affected = file_ids.len();
        for file_id in file_ids {
            db.with_conn(move |conn| crate::sqlite::files::update_rating(conn, file_id, rating))
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(affected)
    }

    pub async fn set_notes_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        notes: HashMap<String, String>,
    ) -> Result<usize, String> {
        let file_ids = Self::collect_file_ids(db, &selection).await?;
        if file_ids.is_empty() {
            return Ok(0);
        }
        let affected = file_ids.len();
        let notes_json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        for file_id in file_ids {
            let json = notes_json.clone();
            db.with_conn(move |conn| {
                conn.execute(
                    "UPDATE file SET notes = ?1 WHERE file_id = ?2",
                    rusqlite::params![json, file_id],
                )
            })
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(affected)
    }

    pub async fn set_source_urls_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        urls: Vec<String>,
    ) -> Result<usize, String> {
        let file_ids = Self::collect_file_ids(db, &selection).await?;
        if file_ids.is_empty() {
            return Ok(0);
        }
        let affected = file_ids.len();
        let urls_json = serde_json::to_string(&urls).map_err(|e| e.to_string())?;
        for file_id in file_ids {
            let json = urls_json.clone();
            db.with_conn(move |conn| {
                conn.execute(
                    "UPDATE file SET source_urls = ?1 WHERE file_id = ?2",
                    rusqlite::params![json, file_id],
                )
            })
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(affected)
    }

    pub async fn get_selection_summary(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
    ) -> Result<SelectionSummary, String> {
        let excluded: HashSet<String> = selection
            .excluded_hashes
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();

        let mut shared_tags: Vec<SelectionTagCount> = Vec::new();
        let mut top_tags: Vec<SelectionTagCount> = Vec::new();
        let mut total_size_bytes: Option<i64> = None;
        let mut mime_counts: Option<HashMap<String, i64>> = None;
        let mut rating_stats_val: Option<serde_json::Value> = None;
        let mut pending = matches!(selection.mode, SelectionMode::AllResults);
        let mut used_fast_cache = false;

        let (total_count, mut sample_hashes) = match &selection.mode {
            SelectionMode::ExplicitHashes => {
                let hashes = selection.hashes.clone().unwrap_or_default();
                let filtered: Vec<String> = hashes
                    .into_iter()
                    .filter(|h| !excluded.contains(h))
                    .collect();
                let (count, total_size, mimes, shared, top, sample) =
                    summarize_hashes_bulk(db, &filtered).await?;
                total_size_bytes = total_size;
                mime_counts = mimes;
                shared_tags = shared;
                top_tags = top;
                pending = false;
                (count, sample)
            }
            SelectionMode::AllResults => {
                if selection.smart_folder_predicate.is_some()
                    || selection
                        .search_tags
                        .as_ref()
                        .map(|t| !t.is_empty())
                        .unwrap_or(false)
                    || selection
                        .search_excluded_tags
                        .as_ref()
                        .map(|t| !t.is_empty())
                        .unwrap_or(false)
                    || selection.status.is_some()
                    || selection
                        .folder_ids
                        .as_ref()
                        .map(|v| !v.is_empty())
                        .unwrap_or(false)
                    || selection
                        .excluded_folder_ids
                        .as_ref()
                        .map(|v| !v.is_empty())
                        .unwrap_or(false)
                {
                    let (base_bm, filtered_bm) =
                        selection_bitmap_for_all_results(db, &selection).await?;
                    let total = base_bm.len() as i64;

                    let sample_ids: Vec<i64> =
                        filtered_bm.iter().take(10).map(|id| id as i64).collect();
                    let resolved = db.resolve_ids_batch(&sample_ids).await?;
                    let sample: Vec<String> = resolved.into_iter().map(|(_, h)| h).collect();

                    let (shared, top) = summarize_tags_from_bitmap(db, &filtered_bm).await?;
                    shared_tags = shared;
                    top_tags = top;

                    let (size, mimes, rstats) =
                        summarize_stats_from_bitmap(db, &filtered_bm).await?;
                    total_size_bytes = Some(size);
                    mime_counts = Some(mimes);
                    rating_stats_val = Some(serde_json::json!({
                        "min": rstats.min,
                        "max": rstats.max,
                        "shared": rstats.shared,
                    }));

                    pending = false;
                    (total, sample)
                } else {
                    // Fast path for default "all active" view.
                    // Use Status(1) count to match selection_bitmap_for_all_results
                    // default case — excludes inbox, trash, and collection members.
                    let total = db.count_files(Some(1)).await?;

                    // Entity-aware total_size (excludes collection members)
                    if let Ok(size) = db
                        .with_read_conn(|conn| {
                            conn.query_row(
                                "SELECT COALESCE(SUM(f.size), 0)
                                 FROM media_entity me
                                 JOIN entity_file ef ON ef.entity_id = me.entity_id
                                 JOIN file f ON f.file_id = ef.file_id
                                 WHERE me.status = 1
                                   AND (me.kind = 'collection' OR me.parent_collection_id IS NULL)",
                                [],
                                |row| row.get::<_, i64>(0),
                            )
                        })
                        .await
                    {
                        total_size_bytes = Some(size);
                    }

                    if excluded.is_empty() && total > 0 {
                        if let Ok(all_counts) = db.get_all_tags_with_counts().await {
                            top_tags = all_counts
                                .iter()
                                .take(30)
                                .map(|t| SelectionTagCount {
                                    tag: if t.namespace.is_empty() {
                                        t.subtag.clone()
                                    } else {
                                        format!("{}:{}", t.namespace, t.subtag)
                                    },
                                    count: t.file_count,
                                })
                                .collect();
                            shared_tags = all_counts
                                .iter()
                                .filter(|t| t.file_count == total)
                                .take(30)
                                .map(|t| SelectionTagCount {
                                    tag: if t.namespace.is_empty() {
                                        t.subtag.clone()
                                    } else {
                                        format!("{}:{}", t.namespace, t.subtag)
                                    },
                                    count: t.file_count,
                                })
                                .collect();
                            used_fast_cache = true;
                        }
                    }
                    // Entity-aware rating aggregate (excludes collection members)
                    if let Ok((r_min, r_max, r_distinct)) = db
                        .with_read_conn(|conn| {
                            conn.query_row(
                                "SELECT MIN(COALESCE(me.rating,0)), MAX(COALESCE(me.rating,0)), COUNT(DISTINCT COALESCE(me.rating,0))
                                 FROM media_entity me
                                 WHERE me.status = 1
                                   AND (me.kind = 'collection' OR me.parent_collection_id IS NULL)",
                                [],
                                |row| Ok((row.get::<_,i64>(0)?, row.get::<_,i64>(1)?, row.get::<_,i64>(2)?)),
                            )
                        })
                        .await
                    {
                        let shared_r = if r_distinct == 1 { Some(r_min) } else { None };
                        rating_stats_val = Some(serde_json::json!({
                            "min": r_min,
                            "max": r_max,
                            "shared": shared_r,
                        }));
                    }

                    // Entity-aware sample hashes (excludes collection members)
                    let sample: Vec<String> = db
                        .with_read_conn(move |conn| {
                            let mut stmt = conn.prepare(
                                "SELECT f.hash
                                 FROM media_entity me
                                 JOIN entity_file ef ON ef.entity_id = me.entity_id
                                 JOIN file f ON f.file_id = ef.file_id
                                 WHERE me.status = 1
                                   AND (me.kind = 'collection' OR me.parent_collection_id IS NULL)
                                 ORDER BY me.updated_at DESC LIMIT 10",
                            )?;
                            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                            rows.collect()
                        })
                        .await?;
                    pending = !used_fast_cache;
                    (total, sample)
                }
            }
        };

        let selected_count = match &selection.mode {
            SelectionMode::AllResults => (total_count - excluded.len() as i64).max(0),
            SelectionMode::ExplicitHashes => total_count,
        };

        sample_hashes.truncate(10);

        Ok(SelectionSummary {
            total_count,
            selected_count,
            sample_hashes,
            shared_tags,
            top_tags,
            stats: SelectionSummaryStats {
                total_size_bytes,
                mime_counts,
                rating_stats: rating_stats_val,
            },
            pending,
            generated_at: Utc::now().to_rfc3339(),
        })
    }
}
