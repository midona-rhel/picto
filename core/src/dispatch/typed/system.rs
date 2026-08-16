//! Handler functions for system-level operations:
//! settings, stats, lifecycle, OS integration, sidebar, view prefs, zoom.

use std::time::Instant;

use serde::Deserialize;
use ts_rs::TS;

use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::Domain;
use crate::state::AppState;
use crate::types::*;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct OpenExternalUrlInput {
    pub url: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderSidebarNodesInput {
    #[ts(type = "[string, number][]")]
    pub moves: Vec<(String, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct PinSidebarItemInput {
    pub node_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UnpinSidebarItemInput {
    pub node_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderPinnedItemsInput {
    #[ts(type = "[string, number][]")]
    pub moves: Vec<(String, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetViewPrefsInput {
    #[serde(default)]
    pub scope_key: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetViewPrefsInput {
    #[serde(default)]
    pub scope_key: Option<String>,
    pub patch: ViewPrefsPatch,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetZoomFactorInput {
    pub factor: f64,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_settings(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = state.settings.get();
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn save_settings(state: &AppState, input: serde_json::Value) -> Result<(), String> {
    let value: crate::settings::store::AppSettings =
        serde_json::from_value(input).map_err(|e| e.to_string())?;
    state.settings.update(value);
    crate::events::emit_state_changed("save_settings", ChangeImpact::new().view_prefs_changed());
    Ok(())
}

pub async fn open_external_url(
    _state: &AppState,
    input: OpenExternalUrlInput,
) -> Result<(), String> {
    open::that(&input.url).map_err(|e| format!("Failed to open URL: {}", e))?;
    Ok(())
}

pub async fn get_sidebar_tree(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let nodes = state.engine.get_sidebar_tree()?;
    let tree_epoch = state.engine.get_sidebar_tree_epoch()?;
    let result = crate::types::SidebarTreeResponse {
        nodes: nodes
            .into_iter()
            .map(|n| {
                let meta: Option<serde_json::Value> = n
                    .meta_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                crate::types::SidebarNodeDto {
                    id: n.node_id,
                    kind: n.kind,
                    parent_id: n.parent_id,
                    name: n.name,
                    icon: n.icon,
                    color: n.color,
                    sort_order: n.sort_order,
                    count: n.count,
                    freshness: n.freshness,
                    selectable: n.selectable,
                    expanded_by_default: n.expanded_by_default,
                    meta,
                }
            })
            .collect(),
        tree_epoch,
        generated_at: chrono::Utc::now().to_rfc3339(),
    };
    crate::perf::record_sidebar_tree(started.elapsed().as_secs_f64() * 1000.0);
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn reorder_sidebar_nodes(
    state: &AppState,
    input: ReorderSidebarNodesInput,
) -> Result<(), String> {
    let mut folder_ids = Vec::new();
    let mut smart_folder_ids = Vec::new();
    for (id, _) in &input.moves {
        if let Some(fid) = id.strip_prefix("folder:") {
            if let Ok(n) = fid.parse::<i64>() {
                folder_ids.push(n);
            }
        } else if let Some(sfid) = id.strip_prefix("smart:") {
            if let Ok(n) = sfid.parse::<i64>() {
                smart_folder_ids.push(n);
            }
        }
    }
    state.engine.reorder_sidebar_nodes(&input.moves)?;
    let mut impact = ChangeImpact::new().add_domain(Domain::Sidebar);
    if !folder_ids.is_empty() {
        impact = impact.add_domain(Domain::Folders).folder_ids(folder_ids);
    }
    if !smart_folder_ids.is_empty() {
        impact = impact
            .add_domain(Domain::SmartFolders)
            .smart_folder_ids(smart_folder_ids);
    }
    crate::events::emit_state_changed("reorder_sidebar_nodes", impact);
    Ok(())
}

pub async fn get_view_prefs(
    state: &AppState,
    input: GetViewPrefsInput,
) -> Result<serde_json::Value, String> {
    let scope_key = input.scope_key.unwrap_or_default();
    let result = state.engine.get_view_prefs(&scope_key)?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn set_view_prefs(
    state: &AppState,
    input: SetViewPrefsInput,
) -> Result<serde_json::Value, String> {
    let scope_key = input.scope_key.unwrap_or_default();
    let result = state.engine.set_view_prefs(&scope_key, input.patch)?;
    crate::events::emit_state_changed("set_view_prefs", ChangeImpact::view_prefs_change());
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn set_zoom_factor(state: &AppState, input: SetZoomFactorInput) -> Result<(), String> {
    let mut s = state.settings.get();
    s.zoom_factor = Some(input.factor);
    state.settings.update(s);
    Ok(())
}

// ── Sidebar Pinning ──────────────────────────────────────────────────

pub async fn pin_sidebar_item(state: &AppState, input: PinSidebarItemInput) -> Result<(), String> {
    state.engine.pin_sidebar_item(&input.node_id)?;
    crate::events::emit_state_changed(
        "pin_sidebar_item",
        ChangeImpact::new().add_domain(Domain::Sidebar),
    );
    Ok(())
}

pub async fn unpin_sidebar_item(
    state: &AppState,
    input: UnpinSidebarItemInput,
) -> Result<(), String> {
    state.engine.unpin_sidebar_item(&input.node_id)?;
    crate::events::emit_state_changed(
        "unpin_sidebar_item",
        ChangeImpact::new().add_domain(Domain::Sidebar),
    );
    Ok(())
}

pub async fn reorder_pinned_items(
    state: &AppState,
    input: ReorderPinnedItemsInput,
) -> Result<(), String> {
    state.engine.reorder_pinned_items(&input.moves)?;
    crate::events::emit_state_changed(
        "reorder_pinned_items",
        ChangeImpact::new().add_domain(Domain::Sidebar),
    );
    Ok(())
}
