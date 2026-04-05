//! Handler functions for folder and collection operations.

use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use ts_rs::TS;

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

fn get_folder_member_ids(conn: &rusqlite::Connection, folder_id: i64) -> rusqlite::Result<Vec<i64>> {
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
        stmt.execute(params![(index as i64 + 1) * FOLDER_RANK_GAP, folder_id, entity_id])?;
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
            (refreshed_previous.or(Some(previous)), refreshed_next.or(Some(next)))
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
                 LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 LEFT JOIN media_file mf ON mf.file_id = sme.file_id
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
             LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
             LEFT JOIN media_file mf ON mf.file_id = sme.file_id
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
            update_stmt.execute(params![(index as i64 + 1) * FOLDER_RANK_GAP, folder_id, entity_id])?;
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
                update_stmt.execute(params![ranks[len - 1 - index], folder_id, ordered_entity_ids[index]])?;
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
            update_stmt.execute(params![(index as i64 + 1) * FOLDER_RANK_GAP, folder_id, entity_id])?;
        }
        let _ = len;
        Ok(())
    })
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFolderFilesInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFolderCoverHashInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileFoldersInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetEntityFoldersInput {
    #[ts(type = "number")]
    pub entity_id: i64,
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
pub struct UpdateFolderParentInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddFilesToFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "import('../../src/shared/types/canonical').EntityTarget")]
    pub target: crate::db::types::EntityTarget,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveFilesFromFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "import('../../src/shared/types/canonical').EntityTarget")]
    pub target: crate::db::types::EntityTarget,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderFoldersInput {
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
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

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetCollectionSummaryInput {
    #[ts(type = "number")]
    pub id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateCollectionInput {
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddCollectionTagsInput {
    #[ts(type = "number")]
    pub id: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveCollectionTagsInput {
    #[ts(type = "number")]
    pub id: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn list_folders(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<Vec<crate::db::query::folders::FolderRow>, String> {
    state.engine.list_folders()
}

// ── Legacy-only read handlers ────────────────────────────────────────────
// The following handlers are NOT called by the rebuilt frontend (src/).
// They exist only for the legacy frontend (legacy/frontend/) and will be
// removed when the legacy frontend is deleted.

pub async fn get_folder_files(
    state: &AppState,
    input: GetFolderFilesInput,
) -> Result<Vec<String>, String> {
    state.engine.get_folder_entity_hashes(input.folder_id)
}

pub async fn get_folder_cover_hash(
    state: &AppState,
    input: GetFolderCoverHashInput,
) -> Result<Option<String>, String> {
    state.engine.get_folder_cover_hash(input.folder_id)
}

pub async fn get_file_folders(
    state: &AppState,
    input: GetFileFoldersInput,
) -> Result<Vec<crate::folders::db::FolderMembership>, String> {
    state.engine.get_entity_folder_memberships(&input.hash)
}

pub async fn get_entity_folders(
    state: &AppState,
    input: GetEntityFoldersInput,
) -> Result<Vec<crate::folders::db::FolderMembership>, String> {
    state.engine.get_entity_folder_memberships_by_entity_id(input.entity_id)
}

pub async fn move_folder(state: &AppState, input: MoveFolderInput) -> Result<(), String> {
    let sibling_order = input.sibling_order;
    state
        .engine
        .move_folder(input.folder_id, input.new_parent_id)?;
    if !sibling_order.is_empty() {
        state.engine.reorder_folders(&sibling_order)?;
    }
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "move_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
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
    let upsert = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{folder_id}"),
        removed: None,
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
    };
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "create_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
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
    let sidebar_patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: None,
        name: input.name,
        icon: input.icon.map(Some),
        color: input.color.map(Some),
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: Some(Some(folder_meta_json_canonical(&folder))),
    };
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "update_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
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
        let patch = crate::runtime_contract::state_change::SidebarNodePatch {
            node_id: format!("folder:{}", input.folder_id),
            removed: None,
            upsert: None,
            kind: None,
            parent_id: None,
            name: None,
            icon: None,
            color: None,
            sort_order: None,
            count: None,
            selectable: None,
            freshness: None,
            meta_json: Some(
                state
                    .engine
                    .get_folder(input.folder_id)
                    .ok()
                    .flatten()
                    .map(|f| folder_meta_json_canonical(&f)),
            ),
        };
        crate::events::emit_state_changed(
            "set_folder_watch_config",
            crate::runtime_contract::change_builder::ChangeImpact::new()
                .add_domains(&[
                    crate::runtime_contract::state_change::Domain::Folders,
                    crate::runtime_contract::state_change::Domain::Sidebar,
                ])
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
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: None,
        name: None,
        icon: None,
        color: None,
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: Some(Some(folder_meta_json_canonical(&folder))),
    };
    crate::events::emit_state_changed(
        "clear_folder_watch_config",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(patch),
    );
    Ok(())
}

pub async fn delete_folder(state: &AppState, input: DeleteFolderInput) -> Result<(), String> {
    state.engine.delete_folder(input.folder_id)?;
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: Some(true),
        upsert: None,
        kind: None,
        parent_id: None,
        name: None,
        icon: None,
        color: None,
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: None,
    };
    crate::events::emit_state_changed(
        "delete_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
                crate::runtime_contract::state_change::Domain::Selection,
            ])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(patch),
    );
    Ok(())
}

pub async fn update_folder_parent(
    state: &AppState,
    input: UpdateFolderParentInput,
) -> Result<(), String> {
    state
        .engine
        .move_folder(input.folder_id, input.new_parent_id)?;
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: Some(
            input
                .new_parent_id
                .map(|pid| format!("folder:{pid}"))
                .or(Some("section:folders".into())),
        ),
        name: None,
        icon: None,
        color: None,
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: None,
    };
    crate::events::emit_state_changed(
        "update_folder_parent",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![input.folder_id])
            .folder_parent_changes(vec![(input.folder_id, input.new_parent_id)])
            .sidebar_node_patch(patch),
    );
    Ok(())
}

pub async fn add_files_to_folder(
    state: &AppState,
    input: AddFilesToFolderInput,
) -> Result<usize, String> {
    let count = state
        .engine
        .update_folder_membership(
            input.target,
            input.folder_id,
            crate::engine::folders::MembershipOperation::Add,
        )?;
    Ok(count.entity_ids.len())
}

pub async fn remove_files_from_folder(
    state: &AppState,
    input: RemoveFilesFromFolderInput,
) -> Result<usize, String> {
    let count = state
        .engine
        .update_folder_membership(
            input.target,
            input.folder_id,
            crate::engine::folders::MembershipOperation::Remove,
        )?;
    Ok(count.entity_ids.len())
}

pub async fn reorder_folders(state: &AppState, input: ReorderFoldersInput) -> Result<(), String> {
    let fids: Vec<i64> = input.moves.iter().map(|(id, _)| *id).collect();
    let order_changes = input.moves.clone();
    state.engine.reorder_folders(&input.moves)?;
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "reorder_folders",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(fids)
            .folder_order_changes(order_changes),
    );
    Ok(())
}

pub async fn reorder_folder_items(
    state: &AppState,
    input: ReorderFolderItemsInput,
) -> Result<(), String> {
    if let Some(moves) = input.moves {
        reorder_folder_items_canonical(state.engine.db(), input.folder_id, moves)?;
    } else if let Some(sort_by) = input.sort_by {
        let direction = input.direction.unwrap_or_else(|| "asc".to_string());
        sort_folder_items_canonical(state.engine.db(), input.folder_id, &sort_by, &direction, input.hashes)?;
    } else if input.reverse == Some(true) {
        reverse_folder_items_canonical(state.engine.db(), input.folder_id, input.hashes)?;
    } else {
        return Err("No reorder operation specified".to_string());
    }
    crate::events::emit_state_changed(
        "reorder_folder_items",
        crate::runtime_contract::change_builder::ChangeImpact::folder_item_reorder(input.folder_id),
    );
    Ok(())
}

// New engine: reorder folder items by entity_id + position_rank.
#[derive(Debug, Deserialize)]
pub struct ReorderFolderMembersInput {
    pub folder_id: i64,
    pub moves: Vec<(i64, i64)>, // (entity_id, position_rank)
}

pub async fn reorder_folder_members(
    state: &AppState,
    input: ReorderFolderMembersInput,
) -> Result<(), String> {
    state.engine.reorder_folder_items(input.folder_id, &input.moves)?;
    crate::events::emit_state_changed(
        "reorder_folder_members",
        crate::runtime_contract::change_builder::ChangeImpact::folder_item_reorder(input.folder_id),
    );
    Ok(())
}

pub async fn get_collections(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<Vec<crate::db::types::CollectionRecord>, String> {
    state.engine.get_collections()
}

pub async fn get_collection_summary(
    state: &AppState,
    input: GetCollectionSummaryInput,
) -> Result<crate::db::types::CollectionSummary, String> {
    state.engine.get_collection_summary(input.id)
}

pub async fn create_collection(
    state: &AppState,
    input: CreateCollectionInput,
) -> Result<i64, String> {
    state.engine.create_collection(&input.name)
}

pub async fn update_collection(
    state: &AppState,
    input: UpdateCollectionInput,
) -> Result<(), String> {
    if input.tags.is_some() {
        return Err(
            "Collection tag editing no longer lives in update_collection; use the canonical tag commands instead"
                .to_string(),
        );
    }
    let Some(name) = input.name.as_deref() else {
        return Ok(());
    };
    state.engine.update_collection(input.id, name)
}

// add_collection_tags / remove_collection_tags removed —
// collection tags use the generic add_tags/remove_tags path via entity_tag_raw.

pub async fn reorder_collection_members(
    state: &AppState,
    input: ReorderCollectionMembersInput,
) -> Result<(), String> {
    state
        .engine
        .reorder_collection_members_by_hashes(input.id, &input.hashes)
}

pub async fn add_collection_members(
    state: &AppState,
    input: AddCollectionMembersInput,
) -> Result<usize, String> {
    Ok(state
        .engine
        .add_collection_members_by_hashes(input.id, &input.hashes)?
        .added
        .len())
}

pub async fn remove_collection_members(
    state: &AppState,
    input: RemoveCollectionMembersInput,
) -> Result<usize, String> {
    Ok(state
        .engine
        .remove_collection_members_by_hashes(input.id, &input.hashes)?
        .removed
        .len())
}

pub async fn delete_collection(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<(), String> {
    state.engine.delete_collection(input.id)
}

pub async fn list_collection_member_hashes(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<Vec<String>, String> {
    state.engine.list_collection_member_hashes(input.id)
}
