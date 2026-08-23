//! Handler functions for folder operations.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::{Domain, SidebarNodePatch};
use crate::state::AppState;

fn folder_meta_json_canonical(folder: &crate::db::query::folders::FolderRow) -> String {
    serde_json::json!({
        "folder_id": folder.folder_id,
        "notes": folder.notes,
        "auto_tags": folder.auto_tags,
        "watch_path": folder.watch_path,
        "watch_enabled": folder.watch_enabled,
        "watch_subfolders": folder.watch_subfolders,
        "watch_import_status_mode": folder.watch_import_status_mode,
    })
    .to_string()
}

const FOLDER_RANK_GAP: i64 = 1 << 20;

fn get_folder_member_ids(
    conn: &rusqlite::Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT entity_id
         FROM folder_member
         WHERE folder_id = ?1
         ORDER BY COALESCE(position_rank, 0), entity_id",
    )?;
    let rows = stmt.query_map([folder_id], |row| row.get(0))?;
    rows.collect()
}

fn get_folder_member_rank(
    conn: &rusqlite::Connection,
    folder_id: i64,
    entity_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT position_rank
         FROM folder_member
         WHERE folder_id = ?1 AND entity_id = ?2",
        params![folder_id, entity_id],
        |row| row.get(0),
    )
    .optional()
}

fn get_next_folder_member_rank(
    conn: &rusqlite::Connection,
    folder_id: i64,
    anchor_rank: i64,
    anchor_entity_id: i64,
    exclude_entity_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT position_rank
         FROM folder_member
         WHERE folder_id = ?1
           AND entity_id != ?4
           AND entity_id != ?5
           AND (COALESCE(position_rank, 0) > ?2 OR (COALESCE(position_rank, 0) = ?2 AND entity_id > ?3))
         ORDER BY COALESCE(position_rank, 0) ASC, entity_id ASC
         LIMIT 1",
        params![folder_id, anchor_rank, anchor_entity_id, exclude_entity_id, anchor_entity_id],
        |row| row.get(0),
    )
    .optional()
}

fn get_prev_folder_member_rank(
    conn: &rusqlite::Connection,
    folder_id: i64,
    anchor_rank: i64,
    anchor_entity_id: i64,
    exclude_entity_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT position_rank
         FROM folder_member
         WHERE folder_id = ?1
           AND entity_id != ?4
           AND entity_id != ?5
           AND (COALESCE(position_rank, 0) < ?2 OR (COALESCE(position_rank, 0) = ?2 AND entity_id < ?3))
         ORDER BY COALESCE(position_rank, 0) DESC, entity_id DESC
         LIMIT 1",
        params![folder_id, anchor_rank, anchor_entity_id, exclude_entity_id, anchor_entity_id],
        |row| row.get(0),
    )
    .optional()
}

fn redistribute_folder_member_ranks(
    conn: &rusqlite::Connection,
    folder_id: i64,
) -> rusqlite::Result<()> {
    let entity_ids = get_folder_member_ids(conn, folder_id)?;
    let mut stmt = conn.prepare_cached(
        "UPDATE folder_member
         SET position_rank = ?1
         WHERE folder_id = ?2 AND entity_id = ?3",
    )?;
    for (index, entity_id) in entity_ids.iter().enumerate() {
        stmt.execute(params![
            (index as i64 + 1) * FOLDER_RANK_GAP,
            folder_id,
            entity_id
        ])?;
    }
    Ok(())
}

fn reorder_folder_member(
    conn: &rusqlite::Connection,
    folder_id: i64,
    entity_id: i64,
    prev_rank: Option<i64>,
    next_rank: Option<i64>,
    after_entity_id: Option<i64>,
    before_entity_id: Option<i64>,
) -> rusqlite::Result<()> {
    let (prev_rank, next_rank) = match (prev_rank, next_rank) {
        (Some(previous), Some(next)) if next - previous <= 1 => {
            redistribute_folder_member_ranks(conn, folder_id)?;
            let refreshed_previous = after_entity_id
                .and_then(|id| get_folder_member_rank(conn, folder_id, id).ok().flatten());
            let refreshed_next = before_entity_id
                .and_then(|id| get_folder_member_rank(conn, folder_id, id).ok().flatten());
            (
                refreshed_previous.or(Some(previous)),
                refreshed_next.or(Some(next)),
            )
        }
        other => other,
    };
    let new_rank = match (prev_rank, next_rank) {
        (Some(previous), Some(next)) => (previous + next) / 2,
        (Some(previous), None) => previous + FOLDER_RANK_GAP,
        (None, Some(next)) => next / 2,
        (None, None) => FOLDER_RANK_GAP,
    };
    conn.execute(
        "INSERT OR IGNORE INTO folder_member (folder_id, entity_id, position_rank)
         VALUES (?1, ?2, ?3)",
        params![folder_id, entity_id, new_rank],
    )?;
    conn.execute(
        "UPDATE folder_member
         SET position_rank = ?1
         WHERE folder_id = ?2 AND entity_id = ?3",
        params![new_rank, folder_id, entity_id],
    )?;
    Ok(())
}

fn reorder_folder_items_canonical(
    db: &crate::db::LibraryDatabase,
    folder_id: i64,
    moves: Vec<crate::types::FolderReorderMove>,
) -> Result<(), String> {
    if moves.is_empty() {
        return Ok(());
    }
    let mut all_hashes = Vec::<String>::new();
    for movement in &moves {
        all_hashes.push(movement.hash.clone());
        if let Some(hash) = movement.after_hash.as_ref() {
            all_hashes.push(hash.clone());
        }
        if let Some(hash) = movement.before_hash.as_ref() {
            all_hashes.push(hash.clone());
        }
    }
    let resolved_ids = db.resolve_entity_hashes(&all_hashes)?;
    let hash_to_id: std::collections::HashMap<String, i64> =
        all_hashes.into_iter().zip(resolved_ids).collect();

    struct ResolvedMove {
        entity_id: i64,
        after_entity_id: Option<i64>,
        before_entity_id: Option<i64>,
    }

    let mut resolved_moves = Vec::<ResolvedMove>::with_capacity(moves.len());
    for movement in moves {
        let entity_id = *hash_to_id
            .get(&movement.hash)
            .ok_or_else(|| format!("Hash not found: {}", movement.hash))?;
        let after_entity_id = movement
            .after_hash
            .as_ref()
            .map(|hash| {
                hash_to_id
                    .get(hash)
                    .copied()
                    .ok_or_else(|| format!("Hash not found: {hash}"))
            })
            .transpose()?;
        let before_entity_id = movement
            .before_hash
            .as_ref()
            .map(|hash| {
                hash_to_id
                    .get(hash)
                    .copied()
                    .ok_or_else(|| format!("Hash not found: {hash}"))
            })
            .transpose()?;
        resolved_moves.push(ResolvedMove {
            entity_id,
            after_entity_id,
            before_entity_id,
        });
    }

    db.with_write(move |conn| {
        for movement in &resolved_moves {
            let previous_rank = match movement.after_entity_id {
                Some(entity_id) => get_folder_member_rank(conn, folder_id, entity_id)?,
                None => None,
            };
            let next_rank = match movement.before_entity_id {
                Some(entity_id) => get_folder_member_rank(conn, folder_id, entity_id)?,
                None => None,
            };
            let (previous_rank, next_rank) = match (previous_rank, next_rank) {
                (Some(previous), None) => (
                    Some(previous),
                    get_next_folder_member_rank(
                        conn,
                        folder_id,
                        previous,
                        movement.after_entity_id.unwrap(),
                        movement.entity_id,
                    )?,
                ),
                (None, Some(next)) => (
                    get_prev_folder_member_rank(
                        conn,
                        folder_id,
                        next,
                        movement.before_entity_id.unwrap(),
                        movement.entity_id,
                    )?,
                    Some(next),
                ),
                other => other,
            };
            reorder_folder_member(
                conn,
                folder_id,
                movement.entity_id,
                previous_rank,
                next_rank,
                movement.after_entity_id,
                movement.before_entity_id,
            )?;
        }
        Ok(())
    })
}

fn sort_folder_items_canonical(
    db: &crate::db::LibraryDatabase,
    folder_id: i64,
    sort_by: &str,
    direction: &str,
    hashes: Option<Vec<String>>,
) -> Result<(), String> {
    let sort_column = match sort_by {
        "name" => "me.name COLLATE NOCASE",
        "imported_at" => "me.date_added",
        "size" => "COALESCE(me.total_size_bytes, mf.size_bytes, 0)",
        "rating" => "me.rating",
        "mime" => "mf.mime_type",
        other => return Err(format!("Invalid sort column: {other}")),
    };
    let direction_sql = if direction == "asc" { "ASC" } else { "DESC" };
    let entity_ids = hashes
        .as_ref()
        .map(|hashes| db.resolve_entity_hashes(hashes))
        .transpose()?;

    db.with_write(move |conn| {
        if let Some(subset_ids) = entity_ids.as_deref() {
            if subset_ids.is_empty() {
                return Ok(());
            }
            let placeholders = subset_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let ranks_sql = format!(
                "SELECT entity_id, COALESCE(position_rank, 0)
                 FROM folder_member
                 WHERE folder_id = ?1 AND entity_id IN ({placeholders})
                 ORDER BY COALESCE(position_rank, 0) ASC, entity_id ASC"
            );
            let mut rank_stmt = conn.prepare(&ranks_sql)?;
            let mut rank_values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(folder_id)];
            for entity_id in subset_ids {
                rank_values.push(Box::new(*entity_id));
            }
            let rank_refs: Vec<&dyn rusqlite::ToSql> =
                rank_values.iter().map(|value| value.as_ref()).collect();
            let rank_rows: Vec<(i64, i64)> = rank_stmt
                .query_map(rank_refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let ranks: Vec<i64> = rank_rows.iter().map(|row| row.1).collect();

            let sorted_sql = format!(
                "SELECT fm.entity_id
                 FROM folder_member fm
                 JOIN media_entity me ON me.entity_id = fm.entity_id
                 JOIN media_file mf ON mf.file_id = me.file_id
                 WHERE fm.folder_id = ?1 AND fm.entity_id IN ({placeholders})
                 ORDER BY {sort_column} {direction_sql}"
            );
            let mut sorted_stmt = conn.prepare(&sorted_sql)?;
            let sorted_ids: Vec<i64> = sorted_stmt
                .query_map(rank_refs.as_slice(), |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;

            let mut update_stmt = conn.prepare_cached(
                "UPDATE folder_member
                 SET position_rank = ?1
                 WHERE folder_id = ?2 AND entity_id = ?3",
            )?;
            for (index, entity_id) in sorted_ids.iter().enumerate() {
                if index < ranks.len() {
                    update_stmt.execute(params![ranks[index], folder_id, entity_id])?;
                }
            }
            return Ok(());
        }

        let sorted_sql = format!(
            "SELECT fm.entity_id
             FROM folder_member fm
             JOIN media_entity me ON me.entity_id = fm.entity_id
             JOIN media_file mf ON mf.file_id = me.file_id
             WHERE fm.folder_id = ?1
             ORDER BY {sort_column} {direction_sql}"
        );
        let mut sorted_stmt = conn.prepare(&sorted_sql)?;
        let sorted_ids: Vec<i64> = sorted_stmt
            .query_map([folder_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut update_stmt = conn.prepare_cached(
            "UPDATE folder_member
             SET position_rank = ?1
             WHERE folder_id = ?2 AND entity_id = ?3",
        )?;
        for (index, entity_id) in sorted_ids.iter().enumerate() {
            update_stmt.execute(params![
                (index as i64 + 1) * FOLDER_RANK_GAP,
                folder_id,
                entity_id
            ])?;
        }
        Ok(())
    })
}

fn reverse_folder_items_canonical(
    db: &crate::db::LibraryDatabase,
    folder_id: i64,
    hashes: Option<Vec<String>>,
) -> Result<(), String> {
    let entity_ids = hashes
        .as_ref()
        .map(|hashes| db.resolve_entity_hashes(hashes))
        .transpose()?;
    db.with_write(move |conn| {
        if let Some(subset_ids) = entity_ids.as_deref() {
            if subset_ids.len() < 2 {
                return Ok(());
            }
            let placeholders = subset_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT entity_id, COALESCE(position_rank, 0)
                 FROM folder_member
                 WHERE folder_id = ?1 AND entity_id IN ({placeholders})
                 ORDER BY COALESCE(position_rank, 0) ASC, entity_id ASC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(folder_id)];
            for entity_id in subset_ids {
                values.push(Box::new(*entity_id));
            }
            let refs: Vec<&dyn rusqlite::ToSql> =
                values.iter().map(|value| value.as_ref()).collect();
            let rows: Vec<(i64, i64)> = stmt
                .query_map(refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let ranks: Vec<i64> = rows.iter().map(|row| row.1).collect();
            let ordered_entity_ids: Vec<i64> = rows.iter().map(|row| row.0).collect();
            let mut update_stmt = conn.prepare_cached(
                "UPDATE folder_member
                 SET position_rank = ?1
                 WHERE folder_id = ?2 AND entity_id = ?3",
            )?;
            let len = ordered_entity_ids.len();
            for index in 0..len {
                update_stmt.execute(params![
                    ranks[len - 1 - index],
                    folder_id,
                    ordered_entity_ids[index]
                ])?;
            }
            return Ok(());
        }

        let ordered_entity_ids = get_folder_member_ids(conn, folder_id)?;
        let len = ordered_entity_ids.len();
        let mut update_stmt = conn.prepare_cached(
            "UPDATE folder_member
             SET position_rank = ?1
             WHERE folder_id = ?2 AND entity_id = ?3",
        )?;
        for (index, entity_id) in ordered_entity_ids.iter().rev().enumerate() {
            update_stmt.execute(params![
                (index as i64 + 1) * FOLDER_RANK_GAP,
                folder_id,
                entity_id
            ])?;
        }
        let _ = len;
        Ok(())
    })
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFolderCoverHashesInput {
    #[ts(type = "number")]
    pub folder_ids: Vec<i64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct FolderCoverHashDto {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub entity_hash: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct MoveFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub sibling_order: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateFolderInput {
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub auto_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetFolderWatchConfigInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub watch_path: String,
    #[serde(default = "default_true")]
    pub watch_enabled: bool,
    #[serde(default)]
    pub watch_subfolders: bool,
    pub watch_import_status_mode: String,
    #[serde(default)]
    pub import_existing_now: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ClearFolderWatchConfigInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveFilesFromFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "import('../../src/shared/types/canonical').EntityTarget")]
    pub target: crate::db::types::EntityTarget,
}

/// Unified folder item reorder. Exactly one mode: `moves` (drag-drop),
/// `sort_by`+`direction` (sort), or `reverse: true` (reverse).
#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderFolderItemsInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[serde(default)]
    pub moves: Option<Vec<crate::types::FolderReorderMove>>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub hashes: Option<Vec<String>>,
    #[serde(default)]
    pub reverse: Option<bool>,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_folder_cover_hashes(
    state: &AppState,
    input: GetFolderCoverHashesInput,
) -> Result<Vec<FolderCoverHashDto>, String> {
    Ok(state
        .engine
        .get_folder_cover_hashes(&input.folder_ids)?
        .into_iter()
        .map(|(folder_id, entity_hash)| FolderCoverHashDto {
            folder_id,
            entity_hash,
        })
        .collect())
}

pub async fn move_folder(state: &AppState, input: MoveFolderInput) -> Result<(), String> {
    let sibling_order = input.sibling_order;
    state
        .engine
        .move_folder(input.folder_id, input.new_parent_id)?;
    if !sibling_order.is_empty() {
        state.engine.reorder_folders(&sibling_order)?;
    }
    state.engine.rebuild_sidebar();
    crate::events::emit_state_changed(
        "move_folder",
        ChangeImpact::new()
            .add_domains(&[Domain::Folders, Domain::Sidebar])
            .folder_ids(vec![input.folder_id])
            .folder_parent_changes(vec![(input.folder_id, input.new_parent_id)])
            .folder_order_changes(sibling_order),
    );
    Ok(())
}

pub async fn create_folder(
    state: &AppState,
    input: CreateFolderInput,
) -> Result<crate::db::query::folders::FolderRow, String> {
    let folder_id = state.engine.create_folder(
        &input.name,
        input.parent_id,
        input.icon.as_deref(),
        input.color.as_deref(),
    )?;
    let folder = state
        .engine
        .get_folder(folder_id)?
        .ok_or_else(|| format!("Folder {folder_id} not found after create"))?;
    let meta = folder_meta_json_canonical(&folder);
    let upsert = SidebarNodePatch {
        node_id: format!("folder:{folder_id}"),
        upsert: Some(true),
        kind: Some("folder".into()),
        parent_id: Some(
            folder
                .parent_id
                .map(|pid| format!("folder:{pid}"))
                .or(Some("section:folders".into())),
        ),
        name: Some(folder.name.clone()),
        icon: Some(folder.icon.clone()),
        color: Some(folder.color.clone()),
        sort_order: Some(folder.sort_order),
        count: Some(Some(0)),
        selectable: Some(true),
        freshness: Some("exact".into()),
        meta_json: Some(Some(meta)),
        ..Default::default()
    };
    state.engine.rebuild_sidebar();
    crate::events::emit_state_changed(
        "create_folder",
        ChangeImpact::new()
            .add_domains(&[Domain::Folders, Domain::Sidebar])
            .folder_ids(vec![folder_id])
            .sidebar_node_patch(upsert),
    );
    Ok(folder)
}

pub async fn update_folder(state: &AppState, input: UpdateFolderInput) -> Result<(), String> {
    let patch = crate::db::types::FolderPatch {
        name: input.name.clone(),
        icon: input.icon.clone(),
        color: input.color.clone(),
        notes: input.notes,
        auto_tags: input
            .auto_tags
            .map(|tags| serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into())),
        ..Default::default()
    };
    state.engine.update_folder(input.folder_id, &patch)?;
    let folder = state
        .engine
        .get_folder(input.folder_id)?
        .ok_or_else(|| format!("Folder {} not found after update", input.folder_id))?;
    let sidebar_patch = SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        name: input.name,
        icon: input.icon.map(Some),
        color: input.color.map(Some),
        meta_json: Some(Some(folder_meta_json_canonical(&folder))),
        ..Default::default()
    };
    state.engine.rebuild_sidebar();
    crate::events::emit_state_changed(
        "update_folder",
        ChangeImpact::new()
            .add_domains(&[Domain::Folders, Domain::Sidebar])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(sidebar_patch),
    );
    Ok(())
}

pub async fn set_folder_watch_config(
    state: &AppState,
    input: SetFolderWatchConfigInput,
) -> Result<(), String> {
    if !matches!(
        input.watch_import_status_mode.as_str(),
        "inherit" | "inbox" | "active"
    ) {
        return Err("watch_import_status_mode must be inherit, inbox, or active".into());
    }
    let canonical_path = std::fs::canonicalize(&input.watch_path)
        .map_err(|err| format!("Failed to resolve watch path: {err}"))?;
    if !canonical_path.is_dir() {
        return Err(format!(
            "Watch path is not a directory: {}",
            canonical_path.display()
        ));
    }
    // Prevent watching paths inside the library directory (would cause circular imports)
    if canonical_path.starts_with(&state.library_root) {
        return Err("Cannot watch a folder inside the current library directory.".into());
    }
    let canonical_path = canonical_path.to_string_lossy().to_string();

    let watch_patch = crate::db::types::FolderPatch {
        watch_path: Some(canonical_path.clone()),
        watch_enabled: Some(input.watch_enabled),
        watch_subfolders: Some(input.watch_subfolders),
        watch_import_status_mode: Some(input.watch_import_status_mode.clone()),
        ..Default::default()
    };
    state.engine.update_folder(input.folder_id, &watch_patch)?;

    if input.import_existing_now {
        // import_existing_for_folder_watch emits its own final combined delta
        // that includes both the imported files and folder membership changes.
        // No separate watch-config emission needed — one combined action.
        crate::folders::watch::import_existing_for_folder_watch(
            state.engine.db(),
            &state.blob_store,
            input.folder_id,
            &canonical_path,
            input.watch_subfolders,
            &input.watch_import_status_mode,
        )
        .await?;
    } else {
        let patch = SidebarNodePatch {
            node_id: format!("folder:{}", input.folder_id),
            meta_json: Some(
                state
                    .engine
                    .get_folder(input.folder_id)
                    .ok()
                    .flatten()
                    .map(|f| folder_meta_json_canonical(&f)),
            ),
            ..Default::default()
        };
        crate::events::emit_state_changed(
            "set_folder_watch_config",
            ChangeImpact::new()
                .add_domains(&[Domain::Folders, Domain::Sidebar])
                .folder_ids(vec![input.folder_id])
                .sidebar_node_patch(patch),
        );
    }

    let _ = state
        .folder_watch_commands
        .send(crate::folders::watch::FolderWatchCommand::Reload);

    Ok(())
}

pub async fn clear_folder_watch_config(
    state: &AppState,
    input: ClearFolderWatchConfigInput,
) -> Result<(), String> {
    let clear_patch = crate::db::types::FolderPatch {
        watch_path: Some(String::new()),
        watch_enabled: Some(false),
        watch_subfolders: Some(false),
        watch_import_status_mode: Some("inherit".into()),
        ..Default::default()
    };
    state.engine.update_folder(input.folder_id, &clear_patch)?;
    let _ = state
        .folder_watch_commands
        .send(crate::folders::watch::FolderWatchCommand::Reload);
    let folder = state
        .engine
        .get_folder(input.folder_id)?
        .ok_or_else(|| format!("Folder {} not found", input.folder_id))?;
    let patch = SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        meta_json: Some(Some(folder_meta_json_canonical(&folder))),
        ..Default::default()
    };
    crate::events::emit_state_changed(
        "clear_folder_watch_config",
        ChangeImpact::new()
            .add_domains(&[Domain::Folders, Domain::Sidebar])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(patch),
    );
    Ok(())
}

fn folder_deletion_impact(folder_ids: &[i64]) -> ChangeImpact {
    let patches = folder_ids
        .iter()
        .map(|folder_id| SidebarNodePatch {
            node_id: format!("folder:{folder_id}"),
            removed: Some(true),
            ..Default::default()
        })
        .collect();
    let mut scopes: Vec<String> = folder_ids
        .iter()
        .map(|folder_id| format!("folder:{folder_id}"))
        .collect();
    scopes.push("system:uncategorized".into());

    ChangeImpact::new()
        .domains(&[Domain::Folders, Domain::Sidebar, Domain::Selection])
        .folder_ids(folder_ids.to_vec())
        .folder_membership_changed(folder_ids.to_vec())
        .extra_grid_scopes(scopes)
        .sidebar_node_patches(patches)
}

fn reload_folder_watches(
    commands: &tokio::sync::mpsc::UnboundedSender<crate::folders::watch::FolderWatchCommand>,
) {
    let _ = commands.send(crate::folders::watch::FolderWatchCommand::Reload);
}

pub async fn delete_folder(state: &AppState, input: DeleteFolderInput) -> Result<(), String> {
    let deleted = state.engine.delete_folder(input.folder_id)?;
    if deleted.is_empty() {
        return Ok(());
    }
    let folder_ids = deleted.folder_ids();
    reload_folder_watches(&state.folder_watch_commands);
    state.engine.rebuild_sidebar();
    crate::events::emit_state_changed(
        "delete_folder",
        crate::ingest::attach_current_sidebar_counts(
            state.engine.db(),
            folder_deletion_impact(&folder_ids),
            false,
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{folder_deletion_impact, reload_folder_watches};

    #[test]
    fn folder_deletion_impact_reports_every_removed_folder() {
        let impact = folder_deletion_impact(&[30, 20, 10]);

        assert_eq!(impact.folder_ids, Some(vec![30, 20, 10]));
        assert_eq!(impact.folder_membership_changed, Some(vec![30, 20, 10]));
        assert_eq!(
            impact.extra_grid_scopes,
            Some(vec![
                "folder:30".into(),
                "folder:20".into(),
                "folder:10".into(),
                "system:uncategorized".into(),
            ])
        );
        let patches = impact.sidebar_node_patches.expect("removal patches");
        assert_eq!(patches.len(), 3);
        assert_eq!(patches[0].node_id, "folder:30");
        assert_eq!(patches[1].node_id, "folder:20");
        assert_eq!(patches[2].node_id, "folder:10");
        assert!(patches.iter().all(|patch| patch.removed == Some(true)));
    }

    #[test]
    fn folder_deletion_requests_a_watch_reload() {
        let (commands, mut received) = crate::folders::watch::channel();

        reload_folder_watches(&commands);

        assert!(matches!(
            received.try_recv(),
            Ok(crate::folders::watch::FolderWatchCommand::Reload)
        ));
    }
}

pub async fn remove_files_from_folder(
    state: &AppState,
    input: RemoveFilesFromFolderInput,
) -> Result<usize, String> {
    let count = state.engine.update_folder_membership(
        input.target,
        input.folder_id,
        crate::engine::folders::MembershipOperation::Remove,
    )?;
    Ok(count.entity_ids.len())
}

pub async fn reorder_folder_items(
    state: &AppState,
    input: ReorderFolderItemsInput,
) -> Result<(), String> {
    if let Some(moves) = input.moves {
        reorder_folder_items_canonical(state.engine.db(), input.folder_id, moves)?;
    } else if let Some(sort_by) = input.sort_by {
        let direction = input.direction.unwrap_or_else(|| "asc".to_string());
        sort_folder_items_canonical(
            state.engine.db(),
            input.folder_id,
            &sort_by,
            &direction,
            input.hashes,
        )?;
    } else if input.reverse == Some(true) {
        reverse_folder_items_canonical(state.engine.db(), input.folder_id, input.hashes)?;
    } else {
        return Err("No reorder operation specified".to_string());
    }
    crate::events::emit_state_changed(
        "reorder_folder_items",
        ChangeImpact::folder_item_reorder(input.folder_id),
    );
    Ok(())
}
