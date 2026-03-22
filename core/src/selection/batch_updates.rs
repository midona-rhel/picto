//! Selection batch updates — bulk operations on file selections.
//!
//! Supports both `ExplicitHashes` (user-picked files) and `AllResults`
//! (current grid scope) selection modes.

use std::collections::{HashMap, HashSet};

use crate::selection::helpers::selection_bitmap_for_all_results;
use crate::sqlite::EntityExpansionMode;
use crate::sqlite::SqliteDatabase;
use crate::types::{SelectionMode, SelectionQuerySpec};

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
            // Use lightweight hash→file_id resolver instead of loading full records.
            let resolved = db.resolve_entity_hashes_batch(&filtered).await?;
            let file_ids: Vec<i64> = resolved.into_iter().map(|(_, id)| id).collect();
            Ok(file_ids)
        }
        SelectionMode::AllResults => {
            let (_base_bm, filtered_bm) = selection_bitmap_for_all_results(db, selection).await?;
            let file_ids: Vec<i64> = filtered_bm.iter().map(|id| id as i64).collect();
            Ok(file_ids)
        }
    }
}

pub async fn add_tags_selection(
    db: &SqliteDatabase,
    selection: SelectionQuerySpec,
    tag_strings: Vec<String>,
) -> Result<usize, String> {
    if tag_strings.is_empty() {
        return Ok(0);
    }

    let file_ids = collect_file_ids(db, &selection).await?;
    if file_ids.is_empty() {
        return Ok(0);
    }
    // Expand to include collection member entities
    let expanded = db
        .expand_entity_ids(file_ids, EntityExpansionMode::EntityAndDescendants)
        .await?;
    let affected = expanded.len();
    db.add_tags_batch_by_entity_ids(expanded, tag_strings, "local".to_string())
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

    let file_ids = collect_file_ids(db, &selection).await?;
    if file_ids.is_empty() {
        return Ok(0);
    }
    // Expand to include collection member entities
    let expanded = db
        .expand_entity_ids(file_ids, EntityExpansionMode::EntityAndDescendants)
        .await?;
    let affected = expanded.len();
    db.remove_tags_batch_by_entity_ids(expanded, tag_strings)
        .await?;
    Ok(affected)
}

pub async fn update_rating_selection(
    db: &SqliteDatabase,
    selection: SelectionQuerySpec,
    rating: Option<i64>,
) -> Result<usize, String> {
    let file_ids = collect_file_ids(db, &selection).await?;
    if file_ids.is_empty() {
        return Ok(0);
    }
    // Expand to include collection member files
    let expanded = db
        .expand_entity_ids(file_ids, EntityExpansionMode::EntityAndDescendants)
        .await?;
    let affected = expanded.len();
    for file_id in expanded {
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
    let file_ids = {
        let base = collect_file_ids(db, &selection).await?;
        db.expand_entity_ids(base, EntityExpansionMode::EntityAndDescendants)
            .await?
    };
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
    let file_ids = {
        let base = collect_file_ids(db, &selection).await?;
        db.expand_entity_ids(base, EntityExpansionMode::EntityAndDescendants)
            .await?
    };
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
