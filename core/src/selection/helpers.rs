//! Selection query helpers — shared bitmap/hash resolution for selection operations.
//!
//! Scope resolution is delegated to `crate::scope::resolver::resolve_scope`.

use std::collections::{HashMap, HashSet};

use roaring::RoaringBitmap;

use crate::scope::resolver::{resolve_scope, ScopeFilter};
use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::files::batch_get_by_hashes;
use crate::sqlite::SqliteDatabase;
use crate::types::{tag_display_key, SelectionMode, SelectionQuerySpec, SelectionTagCount};

/// Collect all hashes matching a selection query (bounded snapshot).
pub async fn collect_selection_hashes(
    db: &SqliteDatabase,
    selection: &SelectionQuerySpec,
) -> Result<Vec<String>, String> {
    match &selection.mode {
        SelectionMode::ExplicitHashes => {
            let excluded: HashSet<String> = selection
                .excluded_hashes
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect();
            let hashes = selection.hashes.clone().unwrap_or_default();
            if excluded.is_empty() {
                Ok(hashes)
            } else {
                Ok(hashes
                    .into_iter()
                    .filter(|h| !excluded.contains(h))
                    .collect())
            }
        }
        SelectionMode::AllResults => {
            // Reuse bitmap resolution (handles exclusions internally).
            let (_base_bm, filtered_bm) = selection_bitmap_for_all_results(db, selection).await?;
            let file_ids: Vec<i64> = filtered_bm.iter().map(|id| id as i64).collect();
            let resolved = db.resolve_ids_batch(&file_ids).await?;
            Ok(resolved.into_iter().map(|(_, h)| h).collect())
        }
    }
}

pub async fn summarize_hashes_bulk(
    db: &SqliteDatabase,
    hashes: &[String],
) -> Result<
    (
        i64,
        Option<i64>,
        Option<HashMap<String, i64>>,
        Vec<SelectionTagCount>,
        Vec<SelectionTagCount>,
        Vec<String>,
    ),
    String,
> {
    use crate::tags::db::get_entities_tags as sql_get_entities_tags;

    let hash_vec = hashes.to_vec();
    let (files, file_tags_map) = db
        .with_conn(move |conn| {
            let files = batch_get_by_hashes(conn, &hash_vec)?;
            let file_ids: Vec<i64> = files.iter().map(|f| f.file_id).collect();
            let tags = sql_get_entities_tags(conn, &file_ids)?;
            Ok((files, tags))
        })
        .await?;

    let total_count = files.len() as i64;
    let mut total_size = 0_i64;
    let mut mimes: HashMap<String, i64> = HashMap::new();
    let mut tag_freq: HashMap<String, i64> = HashMap::new();
    let mut shared: Option<HashMap<String, i64>> = None;

    for file in &files {
        total_size = total_size.saturating_add(file.size);
        *mimes.entry(file.mime.clone()).or_insert(0) += 1;

        let tags = file_tags_map
            .get(&file.file_id)
            .cloned()
            .unwrap_or_default();
        let mut per_file: HashMap<String, i64> = HashMap::new();
        for t in tags {
            let key = tag_display_key(
                t.display_ns.as_deref().unwrap_or(&t.namespace),
                t.display_st.as_deref().unwrap_or(&t.subtag),
            );
            *tag_freq.entry(key.clone()).or_insert(0) += 1;
            *per_file.entry(key).or_insert(0) += 1;
        }
        shared = match shared.take() {
            None => Some(per_file),
            Some(prev) => {
                let mut next = HashMap::new();
                for (k, prev_count) in prev {
                    if let Some(c) = per_file.get(&k) {
                        next.insert(k, prev_count.min(*c));
                    }
                }
                Some(next)
            }
        };
    }

    let mut top = tag_freq.into_iter().collect::<Vec<_>>();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_tags = top
        .into_iter()
        .take(30)
        .map(|(tag, count)| SelectionTagCount { tag, count })
        .collect::<Vec<_>>();

    let mut shared_tags = Vec::new();
    if let Some(shared_map) = shared {
        let mut items = shared_map.into_iter().collect::<Vec<_>>();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        shared_tags = items
            .into_iter()
            .take(30)
            .map(|(tag, count)| SelectionTagCount { tag, count })
            .collect();
    }

    let sample_hashes = hashes.iter().take(10).cloned().collect::<Vec<_>>();

    Ok((
        total_count,
        Some(total_size),
        Some(mimes),
        shared_tags,
        top_tags,
        sample_hashes,
    ))
}

pub async fn selection_bitmap_for_all_results(
    db: &SqliteDatabase,
    selection: &SelectionQuerySpec,
) -> Result<(RoaringBitmap, RoaringBitmap), String> {
    let scope_filter = ScopeFilter::from(selection);
    let base = resolve_scope(db, &scope_filter).await?;

    let mut filtered = base.clone();
    if let Some(excluded_hashes) = &selection.excluded_hashes {
        if !excluded_hashes.is_empty() {
            let hashes = excluded_hashes.clone();
            let excluded_files = db
                .with_conn(move |conn| batch_get_by_hashes(conn, &hashes))
                .await?;
            for f in excluded_files {
                filtered.remove(f.file_id as u32);
            }
        }
    }
    Ok((base, filtered))
}

pub async fn summarize_tags_from_bitmap(
    db: &SqliteDatabase,
    selected_bitmap: &RoaringBitmap,
) -> Result<(Vec<SelectionTagCount>, Vec<SelectionTagCount>), String> {
    let selected_count = selected_bitmap.len() as i64;
    if selected_count <= 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let all_counts = db.get_all_tags_with_counts().await?;
    let mut top: Vec<SelectionTagCount> = Vec::new();
    let mut shared: Vec<SelectionTagCount> = Vec::new();

    for t in all_counts {
        let mut bm = db.bitmaps.get(&BitmapKey::EffectiveTag(t.tag_id));
        if bm.is_empty() {
            continue;
        }
        bm &= selected_bitmap;
        let count = bm.len() as i64;
        if count <= 0 {
            continue;
        }
        let tag = tag_display_key(&t.namespace, &t.subtag);
        if count == selected_count {
            shared.push(SelectionTagCount {
                tag: tag.clone(),
                count,
            });
        }
        top.push(SelectionTagCount { tag, count });
    }

    top.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
    top.truncate(30);
    shared.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
    shared.truncate(30);

    Ok((shared, top))
}

/// Rating stats for a selection.
pub struct RatingStats {
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// If every file in the selection has the same rating, this is that value.
    pub shared: Option<i64>,
}

/// Compute total_size_bytes, mime_counts, and rating stats from a bitmap of file IDs.
pub async fn summarize_stats_from_bitmap(
    db: &SqliteDatabase,
    bitmap: &RoaringBitmap,
) -> Result<(i64, HashMap<String, i64>, RatingStats), String> {
    if bitmap.is_empty() {
        return Ok((
            0,
            HashMap::new(),
            RatingStats {
                min: None,
                max: None,
                shared: None,
            },
        ));
    }

    let file_ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
    db.with_read_conn(move |conn| {
        let placeholders = std::iter::repeat_n("?", file_ids.len())
            .collect::<Vec<_>>()
            .join(", ");

        // Size + mime aggregation
        let sql = format!(
            "SELECT COALESCE(SUM(size), 0), COUNT(*), mime FROM file WHERE file_id IN ({}) GROUP BY mime",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(file_ids.iter()),
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)),
        )?;

        let mut total_size: i64 = 0;
        let mut mime_counts: HashMap<String, i64> = HashMap::new();
        for row in rows {
            let (size_sum, count, mime) = row?;
            total_size += size_sum;
            mime_counts.insert(mime, count);
        }

        // Rating aggregation — single query
        let rating_sql = format!(
            "SELECT MIN(COALESCE(rating, 0)), MAX(COALESCE(rating, 0)), COUNT(DISTINCT COALESCE(rating, 0)) FROM file WHERE file_id IN ({})",
            placeholders
        );
        let mut rating_stmt = conn.prepare(&rating_sql)?;
        let (r_min, r_max, r_distinct): (i64, i64, i64) = rating_stmt.query_row(
            rusqlite::params_from_iter(file_ids.iter()),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let shared = if r_distinct == 1 { Some(r_min) } else { None };

        Ok((total_size, mime_counts, RatingStats { min: Some(r_min), max: Some(r_max), shared }))
    })
    .await
}
