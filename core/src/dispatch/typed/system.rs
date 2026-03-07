//! Typed command implementations for system-level operations:
//! settings, stats, lifecycle, OS integration, sidebar, view prefs, zoom.

use std::time::Instant;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::*;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct OpenExternalUrlInput {
    pub url: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReorderSidebarNodesInput {
    #[ts(type = "[string, number][]")]
    pub moves: Vec<(String, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetViewPrefsInput {
    #[serde(default)]
    pub scope_key: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetViewPrefsInput {
    #[serde(default)]
    pub scope_key: Option<String>,
    pub patch: ViewPrefsPatch,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetZoomFactorInput {
    pub factor: f64,
}

// ─── Command structs ───────────────────────────────────────────────────────

struct GetSettings;
struct SaveSettings;
struct GetLibraryInfo;
struct GetPerfSnapshot;
struct CheckPerfSlo;
struct OpenExternalUrl;
struct GetSidebarTree;
struct ReorderSidebarNodes;
struct GetViewPrefs;
struct SetViewPrefs;
struct SetZoomFactor;
struct GetZoomFactor;
struct EnableModernWindowStyle;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for GetSettings {
    const NAME: &'static str = "get_settings";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = state.settings.get();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for SaveSettings {
    const NAME: &'static str = "save_settings";
    type Input = serde_json::Value;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let value: crate::settings::AppSettings =
            serde_json::from_value(input).map_err(|e| e.to_string())?;
        state.settings.update(value);
        Ok(())
    }
}

impl TypedCommand for GetLibraryInfo {
    const NAME: &'static str = "get_library_info";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for GetPerfSnapshot {
    const NAME: &'static str = "get_perf_snapshot";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let mut perf = serde_json::to_value(crate::perf::get_snapshot())
            .map_err(|e| format!("Failed to serialize perf snapshot: {e}"))?;
        if let serde_json::Value::Object(ref mut map) = perf {
            if let Ok(ptr_val) =
                serde_json::to_value(crate::ptr_sync::get_ptr_sync_perf_breakdown())
            {
                map.insert("ptr_sync".to_string(), ptr_val);
            }
        }
        Ok(perf)
    }
}

impl TypedCommand for CheckPerfSlo {
    const NAME: &'static str = "check_perf_slo";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::perf::check_default_slo();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for OpenExternalUrl {
    const NAME: &'static str = "open_external_url";
    type Input = OpenExternalUrlInput;
    type Output = ();

    async fn execute(_state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        open::that(&input.url).map_err(|e| format!("Failed to open URL: {}", e))?;
        Ok(())
    }
}

impl TypedCommand for GetSidebarTree {
    const NAME: &'static str = "get_sidebar_tree";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let started = Instant::now();
        let result =
            crate::sidebar_controller::SidebarController::get_sidebar_tree(&state.db).await?;
        crate::perf::record_sidebar_tree(started.elapsed().as_secs_f64() * 1000.0);
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for ReorderSidebarNodes {
    const NAME: &'static str = "reorder_sidebar_nodes";
    type Input = ReorderSidebarNodesInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::sidebar_controller::SidebarController::reorder_sidebar_nodes(
            &state.db,
            input.moves,
        )
        .await?;
        crate::events::emit_mutation(
            "reorder_sidebar_nodes",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Sidebar),
        );
        Ok(())
    }
}

impl TypedCommand for GetViewPrefs {
    const NAME: &'static str = "get_view_prefs";
    type Input = GetViewPrefsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let scope_key = input.scope_key.unwrap_or_default();
        let result = crate::view_prefs_controller::ViewPrefsController::get_view_prefs(
            &state.db, scope_key,
        )
        .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for SetViewPrefs {
    const NAME: &'static str = "set_view_prefs";
    type Input = SetViewPrefsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let scope_key = input.scope_key.unwrap_or_default();
        let result = crate::view_prefs_controller::ViewPrefsController::set_view_prefs(
            &state.db,
            scope_key,
            input.patch,
        )
        .await?;
        crate::events::emit_mutation(
            "set_view_prefs",
            crate::events::MutationImpact::new()
                .domains(&[crate::events::Domain::ViewPrefs])
                .view_prefs(),
        );
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for SetZoomFactor {
    const NAME: &'static str = "set_zoom_factor";
    type Input = SetZoomFactorInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for GetZoomFactor {
    const NAME: &'static str = "get_zoom_factor";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let factor = state.settings.get().zoom_factor.unwrap_or(1.0);
        Ok(serde_json::to_value(&factor).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for EnableModernWindowStyle {
    const NAME: &'static str = "enable_modern_window_style";
    type Input = serde_json::Value;
    type Output = ();

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        tracing::debug!(command = "enable_modern_window_style", "Legacy no-op command acknowledged");
        Ok(())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        GetSettings::NAME => Some(run_typed::<GetSettings>(state, args).await),
        SaveSettings::NAME => Some(run_typed::<SaveSettings>(state, args).await),
        GetLibraryInfo::NAME => Some(run_typed::<GetLibraryInfo>(state, args).await),
        GetPerfSnapshot::NAME => Some(run_typed::<GetPerfSnapshot>(state, args).await),
        CheckPerfSlo::NAME => Some(run_typed::<CheckPerfSlo>(state, args).await),
        OpenExternalUrl::NAME => Some(run_typed::<OpenExternalUrl>(state, args).await),
        GetSidebarTree::NAME => Some(run_typed::<GetSidebarTree>(state, args).await),
        ReorderSidebarNodes::NAME => Some(run_typed::<ReorderSidebarNodes>(state, args).await),
        GetViewPrefs::NAME => Some(run_typed::<GetViewPrefs>(state, args).await),
        SetViewPrefs::NAME => Some(run_typed::<SetViewPrefs>(state, args).await),
        SetZoomFactor::NAME => Some(run_typed::<SetZoomFactor>(state, args).await),
        GetZoomFactor::NAME => Some(run_typed::<GetZoomFactor>(state, args).await),
        EnableModernWindowStyle::NAME => {
            Some(run_typed::<EnableModernWindowStyle>(state, args).await)
        }
        _ => None,
    }
}
