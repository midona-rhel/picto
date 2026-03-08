//! Handler functions for system-level operations:
//! settings, stats, lifecycle, OS integration, sidebar, view prefs, zoom.

use std::time::Instant;

use serde::Deserialize;
use ts_rs::TS;

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

pub async fn get_settings(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = state.settings.get();
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn save_settings(state: &AppState, input: serde_json::Value) -> Result<(), String> {
    let value: crate::settings::store::AppSettings =
        serde_json::from_value(input).map_err(|e| e.to_string())?;
    state.settings.update(value);
    Ok(())
}

pub async fn get_library_info(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path_str = state.library_root.to_string_lossy().to_string();
    let name = state
        .library_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Library".to_string());
    let display_name = name.strip_suffix(".library").unwrap_or(&name).to_string();
    let file_count = state.db.count_files(None).await.unwrap_or(0);
    Ok(serde_json::json!({
        "path": path_str,
        "name": display_name,
        "file_count": file_count,
    }))
}

pub async fn get_perf_snapshot(_state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let perf = serde_json::to_value(crate::perf::get_snapshot())
        .map_err(|e| format!("Failed to serialize perf snapshot: {e}"))?;
    Ok(perf)
}

pub async fn check_perf_slo(_state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = crate::perf::check_default_slo();
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn open_external_url(_state: &AppState, input: OpenExternalUrlInput) -> Result<(), String> {
    open::that(&input.url).map_err(|e| format!("Failed to open URL: {}", e))?;
    Ok(())
}

pub async fn get_sidebar_tree(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let nodes = state.db.get_sidebar_tree().await?;
    let tree_epoch = state.db.manifest.published_epoch();
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

pub async fn reorder_sidebar_nodes(state: &AppState, input: ReorderSidebarNodesInput) -> Result<(), String> {
    state.db.reorder_sidebar_nodes(input.moves).await?;
    crate::events::emit_mutation(
        "reorder_sidebar_nodes",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Sidebar),
    );
    Ok(())
}

pub async fn get_view_prefs(state: &AppState, input: GetViewPrefsInput) -> Result<serde_json::Value, String> {
    let scope_key = input.scope_key.unwrap_or_default();
    let result = crate::settings::controller::ViewPrefsController::get_view_prefs(
        &state.db, scope_key,
    ).await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn set_view_prefs(state: &AppState, input: SetViewPrefsInput) -> Result<serde_json::Value, String> {
    let scope_key = input.scope_key.unwrap_or_default();
    let result = crate::settings::controller::ViewPrefsController::set_view_prefs(
        &state.db, scope_key, input.patch,
    ).await?;
    crate::events::emit_mutation(
        "set_view_prefs",
        crate::events::MutationImpact::view_prefs_change(),
    );
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn set_zoom_factor(state: &AppState, input: SetZoomFactorInput) -> Result<(), String> {
    let mut s = state.settings.get();
    s.zoom_factor = Some(input.factor);
    state.settings.update(s);
    crate::events::emit(
        crate::events::event_names::ZOOM_FACTOR_CHANGED,
        &crate::events::ZoomFactorChangedEvent {
            factor: input.factor,
        },
    );
    Ok(())
}

pub async fn get_zoom_factor(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let factor = state.settings.get().zoom_factor.unwrap_or(1.0);
    Ok(serde_json::to_value(&factor).map_err(|e| e.to_string())?)
}
