use std::collections::HashMap;
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::model::SmartFolderId;
use crate::predicate::{self, ViewQuerySpec};
use crate::projection::ProjectionSnapshot;
use crate::{LibraryError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct SmartFolderRecord {
    pub smart_folder_id: SmartFolderId,
    pub name: String,
    pub parent_id: Option<SmartFolderId>,
    pub view: ViewQuerySpec,
    pub display_order: i64,
    pub count: u64,
}

pub(crate) fn load(connection: &Connection, snapshot: &mut ProjectionSnapshot) -> Result<()> {
    let mut queries = HashMap::new();
    let mut results = HashMap::new();
    let mut statement = connection
        .prepare("SELECT smart_folder_id, view_query_json FROM smart_folder_definition")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (smart_folder_id, json) = row?;
        let view: ViewQuerySpec = serde_json::from_str(&json).map_err(|error| {
            LibraryError::InvalidState(format!(
                "smart folder {smart_folder_id} contains an invalid query: {error}"
            ))
        })?;
        let result = evaluate(connection, snapshot, &view, snapshot.active())?;
        queries.insert(smart_folder_id, view);
        results.insert(smart_folder_id, result);
    }
    snapshot.smart_queries = Arc::new(queries);
    snapshot.smart_results = Arc::new(results);
    Ok(())
}

pub(crate) fn settle_affected(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
) -> Result<()> {
    if affected.is_empty() || snapshot.smart_queries.is_empty() {
        return Ok(());
    }
    let universe = snapshot.active() & affected;
    let queries = snapshot.smart_queries.clone();
    let mut replacements = Vec::with_capacity(queries.len());
    for (smart_folder_id, view) in queries.iter() {
        let matches = evaluate(connection, snapshot, view, &universe)?;
        replacements.push((*smart_folder_id, matches));
    }
    let results = Arc::make_mut(&mut snapshot.smart_results);
    for (smart_folder_id, matches) in replacements {
        let result = results.entry(smart_folder_id).or_default();
        *result -= affected;
        *result |= matches;
    }
    Ok(())
}

pub(crate) fn replace_query(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
    smart_folder_id: SmartFolderId,
    view: ViewQuerySpec,
) -> Result<()> {
    let result = evaluate(connection, snapshot, &view, snapshot.active())?;
    Arc::make_mut(&mut snapshot.smart_queries).insert(smart_folder_id.0, view);
    Arc::make_mut(&mut snapshot.smart_results).insert(smart_folder_id.0, result);
    Ok(())
}

pub(crate) fn remove(snapshot: &mut ProjectionSnapshot, smart_folder_id: SmartFolderId) {
    Arc::make_mut(&mut snapshot.smart_queries).remove(&smart_folder_id.0);
    Arc::make_mut(&mut snapshot.smart_results).remove(&smart_folder_id.0);
}

pub fn list(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
) -> Result<Vec<SmartFolderRecord>> {
    let mut statement = connection.prepare(
        "SELECT smart_folder_id, name, parent_id, view_query_json, display_order
         FROM smart_folder_definition ORDER BY display_order, smart_folder_id",
    )?;
    let rows = statement.query_map([], |row| {
        let smart_folder_id = SmartFolderId(row.get(0)?);
        let json = row.get::<_, String>(3)?;
        Ok((
            smart_folder_id,
            row.get::<_, String>(1)?,
            row.get::<_, Option<u32>>(2)?.map(SmartFolderId),
            json,
            row.get::<_, i64>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (smart_folder_id, name, parent_id, json, display_order) = row?;
        let view = serde_json::from_str(&json)?;
        let count = snapshot
            .smart_results
            .get(&smart_folder_id.0)
            .map_or(0, RoaringBitmap::len);
        Ok(SmartFolderRecord {
            smart_folder_id,
            name,
            parent_id,
            view,
            display_order,
            count,
        })
    })
    .collect()
}

fn evaluate(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    view: &ViewQuerySpec,
    universe: &RoaringBitmap,
) -> Result<RoaringBitmap> {
    let mut text = |field, query: &str| crate::fts::search(connection, field, query);
    predicate::evaluate(&view.filter, universe, snapshot, &mut text)
}
