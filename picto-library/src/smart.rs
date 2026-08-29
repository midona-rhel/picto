use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use rayon::prelude::*;
use roaring::RoaringBitmap;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::model::SmartFolderId;
use crate::predicate::{self, FilterExpr, ViewQuerySpec};
use crate::projection::{ProjectionSnapshot, SharedBitmap};
use crate::{LibraryError, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartFolderRecord {
    pub smart_folder_id: SmartFolderId,
    pub name: String,
    pub parent_id: Option<SmartFolderId>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub view: ViewQuerySpec,
    pub display_order: i64,
    pub count: u64,
}

pub const MAX_SMART_FOLDER_DEPTH: usize = 8;
pub const MAX_SMART_FOLDER_LOCAL_RULES: usize = 10;
pub const MAX_SMART_FOLDERS: usize = 1_000;

#[derive(Clone)]
struct SavedDefinition {
    parent_id: Option<SmartFolderId>,
    view: ViewQuerySpec,
}

static SMART_FOLDER_POOL: OnceLock<std::result::Result<rayon::ThreadPool, String>> =
    OnceLock::new();

pub(crate) fn load(connection: &Connection, snapshot: &mut ProjectionSnapshot) -> Result<()> {
    refresh_all(connection, snapshot)
}

pub(crate) fn settle_affected(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
) -> Result<()> {
    settle_affected_for(
        connection,
        snapshot,
        affected,
        predicate::DependencyChange::All,
    )
}

pub(crate) fn settle_affected_for(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
    change: predicate::DependencyChange,
) -> Result<()> {
    if affected.is_empty() || snapshot.smart_queries.is_empty() {
        return Ok(());
    }
    let definitions = load_definitions(connection)?;
    let changed_local_ids = definitions
        .iter()
        .filter_map(|(id, definition)| {
            predicate::depends_on(&definition.view.filter, change).then_some(*id)
        })
        .collect::<Vec<_>>();
    let changed_local_set = changed_local_ids.iter().copied().collect::<HashSet<_>>();
    let active_affected = SharedBitmap::from(snapshot.active() & affected);
    let local_replacements = evaluate_local_results(
        connection,
        snapshot,
        &definitions,
        changed_local_ids.iter().copied(),
        &active_affected,
    )?;
    let mut local_results = (*snapshot.smart_local_results).clone();
    for (smart_folder_id, matches) in local_replacements {
        let result = local_results.entry(smart_folder_id).or_default();
        replace_affected(result, affected, &matches);
    }

    let mut children = HashMap::<u32, Vec<u32>>::new();
    let mut pending = Vec::new();
    for (id, definition) in &definitions {
        if let Some(parent_id) = definition.parent_id {
            children.entry(parent_id.0).or_default().push(*id);
        } else {
            pending.push((*id, active_affected.clone(), false));
        }
    }

    let mut replacements = Vec::new();
    while let Some((smart_folder_id, universe, parent_changed)) = pending.pop() {
        let local_changed = changed_local_set.contains(&smart_folder_id);
        let changed = parent_changed || local_changed;
        let matches: SharedBitmap = if changed {
            let local = &local_results[&smart_folder_id];
            if universe.is_subset(local) {
                universe.clone()
            } else if local.is_subset(&universe) {
                local.clone()
            } else {
                SharedBitmap::from(&universe & &**local)
            }
        } else {
            SharedBitmap::from(
                snapshot
                    .smart_results
                    .get(&smart_folder_id)
                    .map_or_else(RoaringBitmap::new, |result| result & affected),
            )
        };
        if let Some(child_ids) = children.get(&smart_folder_id) {
            pending.extend(
                child_ids
                    .iter()
                    .map(|child_id| (*child_id, matches.clone(), changed)),
            );
        }
        if changed {
            replacements.push((smart_folder_id, matches));
        }
    }

    let results = Arc::make_mut(&mut snapshot.smart_results);
    for (smart_folder_id, matches) in replacements {
        let result = results.entry(smart_folder_id).or_default();
        replace_affected(result, affected, &matches);
    }
    snapshot.smart_local_results = Arc::new(local_results);
    Ok(())
}

fn replace_affected(
    current: &mut SharedBitmap,
    affected: &RoaringBitmap,
    replacement: &SharedBitmap,
) {
    if current.is_subset(affected) {
        *current = replacement.clone();
        return;
    }
    let mut combined = current.to_bitmap();
    combined -= affected;
    combined |= replacement;
    *current = combined.into();
}

pub(crate) fn remove(snapshot: &mut ProjectionSnapshot, smart_folder_id: SmartFolderId) {
    Arc::make_mut(&mut snapshot.smart_queries).remove(&smart_folder_id.0);
    Arc::make_mut(&mut snapshot.smart_effective_queries).remove(&smart_folder_id.0);
    Arc::make_mut(&mut snapshot.smart_local_results).remove(&smart_folder_id.0);
    Arc::make_mut(&mut snapshot.smart_results).remove(&smart_folder_id.0);
}

pub(crate) fn rewrite_tag_references(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
    source: crate::TagId,
    destination: Option<crate::TagId>,
) -> Result<Vec<crate::history::SavedQueryChange>> {
    let queries = snapshot.smart_queries.clone();
    let mut changes = Vec::new();
    for (smart_folder_id, before) in queries.iter() {
        let after = predicate::rewrite_tag_reference(before, source, destination);
        if after == *before {
            continue;
        }
        let smart_folder_id = SmartFolderId(*smart_folder_id);
        connection.execute(
            "UPDATE smart_folder_definition SET view_query_json = ?2
             WHERE smart_folder_id = ?1",
            rusqlite::params![smart_folder_id.0, serde_json::to_string(&after)?],
        )?;
        changes.push(crate::history::SavedQueryChange {
            smart_folder_id,
            before: before.clone(),
            after,
        });
    }
    if !changes.is_empty() {
        refresh_all(connection, snapshot)?;
    }
    Ok(changes)
}

pub(crate) fn validate_view(view: &ViewQuerySpec) -> Result<()> {
    let count = predicate::clause_count(&view.filter);
    if count > MAX_SMART_FOLDER_LOCAL_RULES {
        return Err(LibraryError::InvalidInput(format!(
            "a smart folder may contain at most {MAX_SMART_FOLDER_LOCAL_RULES} rules; received {count}"
        )));
    }
    Ok(())
}

pub(crate) fn validate_capacity(connection: &Connection) -> Result<()> {
    let count =
        connection.query_row("SELECT COUNT(*) FROM smart_folder_definition", [], |row| {
            row.get::<_, i64>(0)
        })?;
    if count >= MAX_SMART_FOLDERS as i64 {
        return Err(LibraryError::InvalidInput(format!(
            "a library may contain at most {MAX_SMART_FOLDERS} smart folders"
        )));
    }
    Ok(())
}

pub(crate) fn refresh_all(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
) -> Result<()> {
    let definitions = load_definitions(connection)?;
    let local_queries = definitions
        .iter()
        .map(|(id, definition)| (*id, definition.view.clone()))
        .collect::<HashMap<_, _>>();
    let effective_queries = compile_effective_queries(&definitions)?;
    let roots = definitions
        .iter()
        .filter_map(|(id, definition)| definition.parent_id.is_none().then_some(*id))
        .collect::<Vec<_>>();
    let active = SharedBitmap::from(snapshot.active().clone());
    let local_results = evaluate_local_results(
        connection,
        snapshot,
        &definitions,
        definitions.keys().copied(),
        &active,
    )?;
    let results = compose_trees(snapshot, &definitions, roots, &local_results)?;
    snapshot.smart_queries = Arc::new(local_queries);
    snapshot.smart_effective_queries = Arc::new(effective_queries);
    snapshot.smart_local_results = Arc::new(local_results);
    snapshot.smart_results = Arc::new(results);
    Ok(())
}

pub(crate) fn refresh_subtree(
    connection: &Connection,
    snapshot: &mut ProjectionSnapshot,
    smart_folder_id: SmartFolderId,
) -> Result<()> {
    let definitions = load_definitions(connection)?;
    if !definitions.contains_key(&smart_folder_id.0) {
        return Err(LibraryError::NotFound(format!(
            "smart folder {}",
            smart_folder_id.0
        )));
    }
    let local_queries = definitions
        .iter()
        .map(|(id, definition)| (*id, definition.view.clone()))
        .collect::<HashMap<_, _>>();
    let effective_queries = compile_effective_queries(&definitions)?;
    let active = SharedBitmap::from(snapshot.active().clone());
    let replacement_local = evaluate_local_results(
        connection,
        snapshot,
        &definitions,
        [smart_folder_id.0],
        &active,
    )?;
    let mut local_results = (*snapshot.smart_local_results).clone();
    local_results.retain(|id, _| definitions.contains_key(id));
    local_results.extend(replacement_local);
    let replacements = compose_trees(
        snapshot,
        &definitions,
        vec![smart_folder_id.0],
        &local_results,
    )?;
    let mut results = (*snapshot.smart_results).clone();
    results.retain(|id, _| definitions.contains_key(id));
    results.extend(replacements);
    snapshot.smart_queries = Arc::new(local_queries);
    snapshot.smart_effective_queries = Arc::new(effective_queries);
    snapshot.smart_local_results = Arc::new(local_results);
    snapshot.smart_results = Arc::new(results);
    Ok(())
}

fn evaluate_local_results(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    definitions: &HashMap<u32, SavedDefinition>,
    ids: impl IntoIterator<Item = u32>,
    base: &SharedBitmap,
) -> Result<HashMap<u32, SharedBitmap>> {
    let mut groups = HashMap::<String, Vec<u32>>::new();
    for id in ids {
        groups
            .entry(serde_json::to_string(&definitions[&id].view.filter)?)
            .or_default()
            .push(id);
    }
    let (structured, text): (Vec<_>, Vec<_>) = groups
        .into_values()
        .partition(|ids| !predicate::contains_text(&definitions[&ids[0]].view.filter));

    let evaluate_structured = |ids: &Vec<u32>| -> Result<(Vec<u32>, SharedBitmap)> {
        let id = ids[0];
        let mut no_text = |_, _: &str| {
            Err(LibraryError::InvalidState(
                "structured smart-folder evaluation reached FTS".into(),
            ))
        };
        let result =
            predicate::evaluate(&definitions[&id].view.filter, base, snapshot, &mut no_text)?;
        Ok((ids.clone(), result.into()))
    };
    let structured_results = if structured.len() >= 8 && base.len() >= 4_096 {
        smart_folder_pool()?.install(|| {
            structured
                .par_iter()
                .map(evaluate_structured)
                .collect::<Vec<_>>()
        })
    } else {
        structured.iter().map(evaluate_structured).collect()
    };

    let mut results = HashMap::new();
    for result in structured_results {
        let (ids, result) = result?;
        results.extend(ids.into_iter().map(|id| (id, result.clone())));
    }
    for ids in text {
        let id = ids[0];
        let result: SharedBitmap =
            evaluate(connection, snapshot, &definitions[&id].view, base)?.into();
        results.extend(ids.into_iter().map(|id| (id, result.clone())));
    }
    Ok(results)
}

fn smart_folder_pool() -> Result<&'static rayon::ThreadPool> {
    let pool = SMART_FOLDER_POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|value| value.get().saturating_sub(1).max(1))
            .unwrap_or(1);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("picto-smart-{index}"))
            .build()
            .map_err(|error| error.to_string())
    });
    pool.as_ref().map_err(|error| {
        LibraryError::InvalidState(format!("failed to start smart-folder workers: {error}"))
    })
}

fn compose_trees(
    snapshot: &ProjectionSnapshot,
    definitions: &HashMap<u32, SavedDefinition>,
    roots: Vec<u32>,
    local_results: &HashMap<u32, SharedBitmap>,
) -> Result<HashMap<u32, SharedBitmap>> {
    let mut children = HashMap::<u32, Vec<u32>>::new();
    for (id, definition) in definitions {
        if let Some(parent_id) = definition.parent_id {
            children.entry(parent_id.0).or_default().push(*id);
        }
    }
    let active = SharedBitmap::from(snapshot.active().clone());
    let mut pending = Vec::with_capacity(roots.len());
    for root in roots {
        let definition = &definitions[&root];
        let universe = match definition.parent_id {
            Some(parent_id) => snapshot
                .smart_results
                .get(&parent_id.0)
                .cloned()
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "smart folder {root} has no published parent result for {}",
                        parent_id.0
                    ))
                })?,
            None => active.clone(),
        };
        pending.push((root, universe));
    }

    let mut results = HashMap::new();
    while let Some((id, universe)) = pending.pop() {
        let local = local_results.get(&id).ok_or_else(|| {
            LibraryError::InvalidState(format!("smart folder {id} has no local result"))
        })?;
        let result = if universe.is_subset(local) {
            universe.clone()
        } else if local.is_subset(&universe) {
            local.clone()
        } else {
            SharedBitmap::from(&universe & &**local)
        };
        if let Some(child_ids) = children.get(&id) {
            pending.extend(child_ids.iter().map(|child_id| (*child_id, result.clone())));
        }
        results.insert(id, result);
    }
    Ok(results)
}

fn load_definitions(connection: &Connection) -> Result<HashMap<u32, SavedDefinition>> {
    let mut definitions = HashMap::new();
    let mut statement = connection.prepare(
        "SELECT smart_folder_id, parent_id, view_query_json FROM smart_folder_definition",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, Option<u32>>(1)?.map(SmartFolderId),
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (smart_folder_id, parent_id, json) = row?;
        let view: ViewQuerySpec = serde_json::from_str(&json).map_err(|error| {
            LibraryError::InvalidState(format!(
                "smart folder {smart_folder_id} contains an invalid query: {error}"
            ))
        })?;
        definitions.insert(smart_folder_id, SavedDefinition { parent_id, view });
    }
    Ok(definitions)
}

fn compile_effective_queries(
    definitions: &HashMap<u32, SavedDefinition>,
) -> Result<HashMap<u32, ViewQuerySpec>> {
    let mut effective = HashMap::with_capacity(definitions.len());
    let mut depths = HashMap::with_capacity(definitions.len());
    let mut resolving = HashSet::new();
    let mut ids = definitions.keys().copied().collect::<Vec<_>>();
    ids.sort_unstable();
    for id in ids {
        resolve_effective(id, definitions, &mut resolving, &mut effective, &mut depths)?;
    }
    Ok(effective)
}

fn resolve_effective(
    id: u32,
    definitions: &HashMap<u32, SavedDefinition>,
    resolving: &mut HashSet<u32>,
    effective: &mut HashMap<u32, ViewQuerySpec>,
    depths: &mut HashMap<u32, usize>,
) -> Result<ViewQuerySpec> {
    if let Some(view) = effective.get(&id) {
        return Ok(view.clone());
    }
    if !resolving.insert(id) {
        return Err(LibraryError::InvalidState(
            "smart folder parentage contains a cycle".into(),
        ));
    }
    let definition = definitions.get(&id).cloned().ok_or_else(|| {
        LibraryError::InvalidState(format!("smart folder {id} is missing its definition"))
    })?;
    validate_view(&definition.view).map_err(|error| match error {
        LibraryError::InvalidInput(message) => LibraryError::InvalidState(message),
        error => error,
    })?;
    let (depth, filter) = if let Some(parent_id) = definition.parent_id {
        if !definitions.contains_key(&parent_id.0) {
            return Err(LibraryError::InvalidState(format!(
                "smart folder {id} references missing parent {}",
                parent_id.0
            )));
        }
        let parent = resolve_effective(parent_id.0, definitions, resolving, effective, depths)?;
        let depth = depths[&parent_id.0] + 1;
        (
            depth,
            FilterExpr::All(vec![parent.filter, definition.view.filter.clone()]),
        )
    } else {
        (1, definition.view.filter.clone())
    };
    if depth > MAX_SMART_FOLDER_DEPTH {
        return Err(LibraryError::InvalidState(format!(
            "smart folder {id} exceeds the maximum depth of {MAX_SMART_FOLDER_DEPTH}"
        )));
    }
    resolving.remove(&id);
    depths.insert(id, depth);
    let view = ViewQuerySpec {
        filter,
        sort: definition.view.sort,
    };
    effective.insert(id, view.clone());
    Ok(view)
}

pub fn list(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
) -> Result<Vec<SmartFolderRecord>> {
    let mut statement = connection.prepare(
        "SELECT smart_folder_id, name, parent_id, icon, color, notes,
                view_query_json, display_order
         FROM smart_folder_definition ORDER BY display_order, smart_folder_id",
    )?;
    let rows = statement.query_map([], |row| {
        let smart_folder_id = SmartFolderId(row.get(0)?);
        let json = row.get::<_, String>(6)?;
        Ok((
            smart_folder_id,
            row.get::<_, String>(1)?,
            row.get::<_, Option<u32>>(2)?.map(SmartFolderId),
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            json,
            row.get::<_, i64>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (smart_folder_id, name, parent_id, icon, color, notes, json, display_order) = row?;
        let view = serde_json::from_str(&json)?;
        let count = snapshot
            .smart_results
            .get(&smart_folder_id.0)
            .map_or(0, |values| values.len());
        Ok(SmartFolderRecord {
            smart_folder_id,
            name,
            parent_id,
            icon,
            color,
            notes,
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
