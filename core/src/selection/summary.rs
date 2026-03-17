//! Selection summary query — computes tag counts, shared tags, stats
//! for the current selection.

use std::collections::HashSet;

use chrono::Utc;

use crate::selection::helpers::{
    sample_hashes_from_entity_bitmap, selection_bitmap_for_all_results, summarize_entity_stats_from_bitmap,
    summarize_hashes_bulk, summarize_tags_from_bitmap,
};
use crate::sqlite::SqliteDatabase;
use crate::types::{
    SelectionMode, SelectionQuerySpec, SelectionSummary, SelectionSummaryStats,
};

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

    let (total_count, mut sample_hashes, shared_tags, top_tags, total_size_bytes, mime_counts, rating_stats_val, pending) = match &selection.mode {
        SelectionMode::ExplicitHashes => {
            let hashes = selection.hashes.clone().unwrap_or_default();
            let filtered: Vec<String> = hashes
                .into_iter()
                .filter(|h| !excluded.contains(h))
                .collect();
            let (count, total_size, mimes, shared, top, sample) =
                summarize_hashes_bulk(db, &filtered).await?;
            (count, sample, shared, top, total_size, mimes, None, false)
        }
        SelectionMode::AllResults => {
            let (base_bm, filtered_bm) = selection_bitmap_for_all_results(db, &selection).await?;
            let total = base_bm.len() as i64;

            let sample = sample_hashes_from_entity_bitmap(db, &filtered_bm, 10).await?;

            let (shared, top) = summarize_tags_from_bitmap(db, &filtered_bm).await?;

            let (size, mimes, rstats) = summarize_entity_stats_from_bitmap(db, &filtered_bm).await?;
            (
                total,
                sample,
                shared,
                top,
                Some(size),
                Some(mimes),
                Some(serde_json::json!({
                    "min": rstats.min,
                    "max": rstats.max,
                    "shared": rstats.shared,
                })),
                false,
            )
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
