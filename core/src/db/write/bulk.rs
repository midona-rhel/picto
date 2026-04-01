//! Bulk write helpers for query_results targets.
//!
//! Materializes an EntityViewQuery into a temp table of entity_ids,
//! then write operations JOIN against it. This avoids loading millions
//! of ids into Rust memory for "select all" operations.
//!
//! Uses the same query builder as grid reads so filters, tags, smart
//! folders, and search all work identically.

use std::sync::Arc;

use rusqlite::{Connection, ToSql};

use crate::db::projection::bitmaps::BitmapStore;
use crate::db::types::{EntityViewQuery, ExpansionMode, QueryPage};

/// Populate `_bulk_target` temp table with entity_ids matching the query,
/// minus any excluded hashes. Returns the count of ids in the table.
///
/// Reuses the grid query builder with no pagination to get the full
/// matching set. This ensures scope + filters + tags + search semantics
/// are identical to what the user sees in the grid.
pub fn populate_bulk_target(
    conn: &Connection,
    query: &EntityViewQuery,
    exclusions: &[String],
    bitmaps: &Arc<BitmapStore>,
) -> rusqlite::Result<i64> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS _bulk_target;
         CREATE TEMP TABLE _bulk_target (entity_id INTEGER PRIMARY KEY);",
    )?;

    // Build a full query with no pagination limit to get all matching entity_ids.
    let mut unbounded = query.clone();
    unbounded.page = QueryPage {
        limit: i64::MAX,
        cursor: None,
    };

    // Pre-resolve SmartFolder bitmap to entity_ids, same as
    // LibraryDatabase::query_entity_view does for normal reads.
    let preresolved = if matches!(
        query.base_scope.kind,
        crate::db::types::ScopeKind::SmartFolder
    ) {
        let sf_id = query.base_scope.id.unwrap_or(0);
        let bitmap = bitmaps.get(&crate::db::projection::bitmaps::BitmapKey::SmartFolder(
            sf_id,
        ));
        Some(bitmap.iter().map(|id| id as i64).collect::<Vec<_>>())
    } else {
        None
    };

    let result =
        crate::db::query::grid::query_entity_view(conn, &unbounded, preresolved.as_deref())?;

    // Insert all matching entity hashes → entity_ids into the temp table.
    // The grid query already returned the right set.
    if !result.items.is_empty() {
        let mut insert_stmt = conn.prepare(
            "INSERT OR IGNORE INTO _bulk_target (entity_id)
             SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
        )?;
        for item in &result.items {
            insert_stmt.execute([&item.entity_hash])?;
        }
    }

    // Remove exclusions
    if !exclusions.is_empty() {
        let placeholders: Vec<String> = (1..=exclusions.len()).map(|i| format!("?{i}")).collect();
        let del_sql = format!(
            "DELETE FROM _bulk_target WHERE entity_id IN (
                SELECT entity_id FROM media_entity WHERE entity_hash IN ({})
            )",
            placeholders.join(",")
        );
        let excl_refs: Vec<&dyn ToSql> = exclusions.iter().map(|h| h as &dyn ToSql).collect();
        conn.execute(&del_sql, excl_refs.as_slice())?;
    }

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM _bulk_target", [], |r| r.get(0))?;
    Ok(count)
}

/// Expand _bulk_target in-place to include collection member entity_ids.
pub fn expand_bulk_target(conn: &Connection, mode: ExpansionMode) -> rusqlite::Result<()> {
    match mode {
        ExpansionMode::EntityOnly => {}
        ExpansionMode::DescendantsOnly => {
            conn.execute_batch(
                "CREATE TEMP TABLE _bulk_expanded AS
                 SELECT me.entity_id FROM media_entity me
                 WHERE me.parent_collection_entity_id IN (SELECT entity_id FROM _bulk_target);
                 DELETE FROM _bulk_target;
                 INSERT INTO _bulk_target SELECT entity_id FROM _bulk_expanded;
                 DROP TABLE _bulk_expanded;",
            )?;
        }
        ExpansionMode::EntityAndDescendants => {
            conn.execute(
                "INSERT OR IGNORE INTO _bulk_target (entity_id)
                 SELECT me.entity_id FROM media_entity me
                 WHERE me.parent_collection_entity_id IN (SELECT entity_id FROM _bulk_target)",
                [],
            )?;
        }
    }
    Ok(())
}

/// Collect all entity_hashes from _bulk_target.
pub fn collect_bulk_hashes(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT me.entity_hash FROM media_entity me
         JOIN _bulk_target bt ON bt.entity_id = me.entity_id",
    )?;
    let hashes = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(hashes)
}

/// Collect all entity_ids from _bulk_target.
pub fn collect_bulk_ids(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT entity_id FROM _bulk_target")?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(ids)
}
