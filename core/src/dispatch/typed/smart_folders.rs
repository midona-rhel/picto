//! Handler functions for smart folder operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::{Domain, SidebarNodePatch};
use crate::state::AppState;

fn smart_folder_meta_json_canonical(row: &crate::db::query::folders::SmartFolderRow) -> String {
    serde_json::json!({
        "smart_folder_id": row.smart_folder_id,
        "parent_id": row.parent_id,
        "notes": row.notes,
        "predicate": serde_json::from_str::<serde_json::Value>(&row.predicate_json)
            .unwrap_or_else(|_| serde_json::json!({ "groups": [] })),
    })
    .to_string()
}

// ─── Input structs ─────────────────────────────────────────────────────────

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

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn create_smart_folder(
    state: &AppState,
    input: CreateSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let default_predicate = || serde_json::json!({ "groups": [] }).to_string();
    let folder = input
        .folder
        .unwrap_or_else(|| crate::smart_folders::types::SmartFolder {
            smart_folder_id: 0,
            name: input.name.unwrap_or_else(|| "New Smart Folder".to_string()),
            parent_id: input.parent_id,
            icon: input.icon,
            color: input.color,
            notes: input.notes,
            predicate_json: input.predicate_json.unwrap_or_else(default_predicate),
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
    let counts = state.engine.settle_smart_folders(&[sf_id]);
    let count = counts.first().map(|(_, count)| *count).unwrap_or(0);
    let meta = smart_folder_meta_json_canonical(&row);
    let upsert = SidebarNodePatch {
        node_id: format!("smart:{sf_id}"),
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
        count: Some(Some(count)),
        selectable: Some(true),
        freshness: Some("exact".into()),
        meta_json: Some(Some(meta)),
        ..Default::default()
    };
    crate::events::emit_state_changed(
        "create_smart_folder",
        ChangeImpact::new()
            .add_domains(&[Domain::SmartFolders, Domain::Sidebar])
            .smart_folder_ids(vec![sf_id])
            .smart_folder_counts(counts)
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
    )?;
    let row = state
        .engine
        .get_smart_folder(sf_id)?
        .ok_or_else(|| format!("Smart folder {sf_id} not found after update"))?;
    let predicate_changed = row.predicate_json != old_predicate;

    let meta = smart_folder_meta_json_canonical(&row);
    let patch = SidebarNodePatch {
        node_id: format!("smart:{sf_id}"),
        name: Some(row.name.clone()),
        icon: Some(row.icon.clone()),
        color: Some(row.color.clone()),
        sort_order: Some(row.display_order),
        meta_json: Some(Some(meta)),
        ..Default::default()
    };
    let affected_ids = if predicate_changed {
        state.engine.smart_folder_subtree_ids(sf_id)?
    } else {
        Vec::new()
    };
    let counts = if predicate_changed {
        state.engine.settle_smart_folders(&affected_ids)
    } else {
        state.engine.rebuild_sidebar();
        Vec::new()
    };
    let mut impact = ChangeImpact::new()
        .add_domains(&[Domain::SmartFolders, Domain::Sidebar])
        .sidebar_node_patch(patch);
    if predicate_changed {
        impact = impact
            .smart_folder_ids(affected_ids.clone())
            .smart_folder_counts(counts);
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
    let affected_ids = state
        .engine
        .smart_folder_subtree_ids(input.smart_folder_id)?;
    let counts = state.engine.settle_smart_folders(&affected_ids);
    crate::events::emit_state_changed(
        "move_smart_folder",
        ChangeImpact::new()
            .add_domains(&[Domain::SmartFolders, Domain::Sidebar])
            .smart_folder_ids(affected_ids.clone())
            .smart_folder_counts(counts)
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

    let mut patches = vec![SidebarNodePatch {
        node_id: format!("smart:{sf_id}"),
        removed: Some(true),
        ..Default::default()
    }];
    let new_parent = deleted_parent_id
        .map(|pid| format!("smart:{pid}"))
        .unwrap_or_else(|| "section:smart_folders".into());
    for child_id in &promoted_ids {
        patches.push(SidebarNodePatch {
            node_id: format!("smart:{child_id}"),
            parent_id: Some(Some(new_parent.clone())),
            ..Default::default()
        });
    }

    let mut surviving_ids = Vec::new();
    for child_id in &promoted_ids {
        surviving_ids.extend(state.engine.smart_folder_subtree_ids(*child_id)?);
    }
    let mut affected_ids = vec![sf_id];
    affected_ids.extend(&surviving_ids);
    let counts = state.engine.settle_smart_folders(&affected_ids);
    crate::events::emit_state_changed(
        "delete_smart_folder",
        ChangeImpact::new()
            .add_domains(&[Domain::SmartFolders, Domain::Sidebar])
            .smart_folder_ids(affected_ids)
            .smart_folder_counts(counts)
            .sidebar_node_patches(patches),
    );
    Ok(())
}
