//! Handler functions for smart folder operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

fn smart_folder_meta_json_canonical(row: &crate::db::query::folders::SmartFolderRow) -> String {
    serde_json::json!({
        "smart_folder_id": row.smart_folder_id,
        "parent_id": row.parent_id,
        "notes": row.notes,
        "predicate": serde_json::from_str::<serde_json::Value>(&row.predicate_json)
            .unwrap_or_else(|_| serde_json::json!({ "groups": [] })),
        "sort_field": row.sort_field,
        "sort_order": row.sort_order,
    })
    .to_string()
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderSmartFoldersInput {
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct MoveSmartFolderInput {
    pub smart_folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub sibling_order: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateSmartFolderInput {
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub folder: Option<crate::smart_folders::types::SmartFolder>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub predicate_json: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateSmartFolderInput {
    pub id: String,
    #[ts(type = "Record<string, unknown>")]
    pub folder: crate::smart_folders::types::SmartFolder,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteSmartFolderInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CountSmartFolderInput {
    pub predicate: crate::smart_folders::types::SmartFolderPredicate,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn list_smart_folders(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = state.engine.list_smart_folders()?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn create_smart_folder(
    state: &AppState,
    input: CreateSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let default_predicate = || serde_json::json!({ "groups": [] }).to_string();
    let folder = input.folder.unwrap_or_else(|| crate::smart_folders::types::SmartFolder {
        smart_folder_id: 0,
        name: input.name.unwrap_or_else(|| "New Smart Folder".to_string()),
        parent_id: input.parent_id,
        icon: input.icon,
        color: input.color,
        notes: input.notes,
        predicate_json: input.predicate_json.unwrap_or_else(default_predicate),
        sort_field: None,
        sort_order: None,
        display_order: None,
        created_at: None,
        updated_at: None,
    });
    let sf = &folder;
    let sf_id = state.engine.create_smart_folder(
        &sf.name,
        sf.parent_id,
        &sf.predicate_json,
        sf.icon.as_deref(),
        sf.color.as_deref(),
        sf.notes.as_deref(),
    )?;
    let row = state
        .engine
        .get_smart_folder(sf_id)?
        .ok_or_else(|| format!("Smart folder {sf_id} not found after create"))?;
    let meta = smart_folder_meta_json_canonical(&row);
    let upsert = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("smart:{sf_id}"),
        removed: None,
        upsert: Some(true),
        kind: Some("smart_folder".into()),
        parent_id: Some(
            row.parent_id
                .map(|pid| format!("smart:{pid}"))
                .or(Some("section:smart_folders".into())),
        ),
        name: Some(row.name.clone()),
        icon: Some(row.icon.clone()),
        color: Some(row.color.clone()),
        sort_order: Some(row.display_order),
        count: Some(Some(0)),
        selectable: Some(true),
        freshness: Some("stale".into()),
        meta_json: Some(Some(meta)),
    };
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            dirty_smart_folder_ids: vec![sf_id],
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "create_smart_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::SmartFolders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .smart_folder_ids(vec![sf_id])
            .sidebar_node_patch(upsert),
    );
    Ok(serde_json::to_value(&row).map_err(|e| e.to_string())?)
}

pub async fn update_smart_folder(
    state: &AppState,
    input: UpdateSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let sf_id: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid smart folder id: {}", input.id))?;
    if input.folder.parent_id == Some(sf_id) {
        return Err("A smart folder cannot be its own parent".to_string());
    }
    if let Some(parent_id) = input.folder.parent_id {
        let descendants = state.engine.collect_descendant_smart_folder_ids(sf_id)?;
        if descendants.iter().any(|&id| id == parent_id) {
            return Err("A smart folder cannot be moved under one of its descendants".to_string());
        }
    }
    // Read old predicate to detect changes
    let old_predicate = state
        .engine
        .get_smart_folder(sf_id)?
        .map(|r| r.predicate_json.clone())
        .unwrap_or_default();

    let sf = &input.folder;
    state.engine.update_smart_folder(
        sf_id,
        Some(&sf.name),
        Some(&sf.predicate_json),
        sf.icon.as_deref(),
        sf.color.as_deref(),
        sf.notes.as_deref(),
        sf.sort_field.as_deref(),
        sf.sort_order.as_deref(),
    )?;
    let row = state
        .engine
        .get_smart_folder(sf_id)?
        .ok_or_else(|| format!("Smart folder {sf_id} not found after update"))?;
    let predicate_changed = row.predicate_json != old_predicate;

    let meta = smart_folder_meta_json_canonical(&row);
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("smart:{sf_id}"),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: None,
        name: Some(row.name.clone()),
        icon: Some(row.icon.clone()),
        color: Some(row.color.clone()),
        sort_order: Some(row.display_order),
        count: None,
        selectable: None,
        freshness: None,
        meta_json: Some(Some(meta)),
    };
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            dirty_smart_folder_ids: if predicate_changed {
                vec![sf_id]
            } else {
                vec![]
            },
            ..Default::default()
        });
    // Read the updated bitmap count after compiler ran
    let sf_count = state.engine.smart_folder_bitmap_len(sf_id);
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .add_domains(&[
            crate::runtime_contract::state_change::Domain::SmartFolders,
            crate::runtime_contract::state_change::Domain::Sidebar,
        ])
        .smart_folder_ids(vec![sf_id])
        .smart_folder_counts(vec![(sf_id, sf_count)])
        .sidebar_node_patch(patch);
    if predicate_changed {
        impact = impact.extra_grid_scopes(vec![format!("smart:{sf_id}")]);
    }
    crate::events::emit_state_changed("update_smart_folder", impact);
    Ok(serde_json::to_value(&row).map_err(|e| e.to_string())?)
}

pub async fn move_smart_folder(
    state: &AppState,
    input: MoveSmartFolderInput,
) -> Result<(), String> {
    if input.new_parent_id == Some(input.smart_folder_id) {
        return Err("A smart folder cannot be its own parent".to_string());
    }
    if let Some(new_parent_id) = input.new_parent_id {
        let descendants = state
            .engine
            .collect_descendant_smart_folder_ids(input.smart_folder_id)?;
        if descendants.iter().any(|&id| id == new_parent_id) {
            return Err("A smart folder cannot be moved under one of its descendants".to_string());
        }
    }
    let sibling_order = input.sibling_order;
    state
        .engine
        .move_smart_folder(input.smart_folder_id, input.new_parent_id)?;
    if !sibling_order.is_empty() {
        state.engine.reorder_smart_folders(&sibling_order)?;
    }
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "move_smart_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::SmartFolders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .smart_folder_ids(vec![input.smart_folder_id])
            .smart_folder_parent_changes(vec![(input.smart_folder_id, input.new_parent_id)])
            .smart_folder_order_changes(sibling_order),
    );
    Ok(())
}

pub async fn delete_smart_folder(
    state: &AppState,
    input: DeleteSmartFolderInput,
) -> Result<(), String> {
    let sf_id: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid smart folder id: {}", input.id))?;
    let (promoted_ids, deleted_parent_id) = state.engine.delete_smart_folder(sf_id)?;

    let mut patches = vec![crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("smart:{sf_id}"),
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
    }];
    let new_parent = deleted_parent_id
        .map(|pid| format!("smart:{pid}"))
        .unwrap_or_else(|| "section:smart_folders".into());
    for child_id in &promoted_ids {
        patches.push(crate::runtime_contract::state_change::SidebarNodePatch {
            node_id: format!("smart:{child_id}"),
            removed: None,
            upsert: None,
            kind: None,
            parent_id: Some(Some(new_parent.clone())),
            name: None,
            icon: None,
            color: None,
            sort_order: None,
            count: None,
            selectable: None,
            freshness: None,
            meta_json: None,
        });
    }

    let mut all_sf_ids = vec![sf_id];
    all_sf_ids.extend(&promoted_ids);

    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "delete_smart_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::SmartFolders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .smart_folder_ids(all_sf_ids)
            .extra_grid_scopes(vec![format!("smart:{sf_id}")])
            .sidebar_node_patches(patches),
    );
    Ok(())
}

// Compatibility handler: the rebuilt frontend does not call this directly,
// but it now evaluates the predicate against the canonical bitmap store.
pub async fn count_smart_folder(
    state: &AppState,
    input: CountSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let count = state.engine.count_smart_folder_predicate(&input.predicate)?;
    Ok(serde_json::to_value(&count).map_err(|e| e.to_string())?)
}

pub async fn reorder_smart_folders(
    state: &AppState,
    input: ReorderSmartFoldersInput,
) -> Result<(), String> {
    let sfids: Vec<i64> = input.moves.iter().map(|(id, _)| *id).collect();
    let order_changes = input.moves.clone();
    state.engine.reorder_smart_folders(&input.moves)?;
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "reorder_smart_folders",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::SmartFolders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .smart_folder_ids(sfids)
            .smart_folder_order_changes(order_changes),
    );
    Ok(())
}
