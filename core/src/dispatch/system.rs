//! System-level handlers: settings, stats, lifecycle, OS integration,
//! sidebar, view prefs, zoom, and legacy/stub commands.

use std::time::Instant;

use crate::state::AppState;
use crate::types::*;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_settings" => {
            let result = state.settings.get();
            Some(to_json(&result))
        }
        "save_settings" => {
            let value: crate::settings::AppSettings = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return Some(Err(e.to_string())),
            };
            state.settings.update(value);
            Some(ok_null())
        }

        "get_library_info" => {
            let path_str = state.library_root.to_string_lossy().to_string();
            let name = state
                .library_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Library".to_string());
            let display_name = name.strip_suffix(".library").unwrap_or(&name).to_string();
            let file_count = state.db.count_files(None).await.unwrap_or(0);
            Some(to_json(&serde_json::json!({
                "path": path_str,
                "name": display_name,
                "file_count": file_count,
            })))
        }

        "get_perf_snapshot" => {
            let mut perf = match serde_json::to_value(crate::perf::get_snapshot()) {
                Ok(v) => v,
                Err(e) => return Some(Err(format!("Failed to serialize perf snapshot: {e}"))),
            };
            if let serde_json::Value::Object(ref mut map) = perf {
                if let Ok(ptr_val) =
                    serde_json::to_value(crate::ptr_sync::get_ptr_sync_perf_breakdown())
                {
                    map.insert("ptr_sync".to_string(), ptr_val);
                }
            }
            Some(to_json(&perf))
        }
        "check_perf_slo" => {
            let result = crate::perf::check_default_slo();
            Some(to_json(&result))
        }

        "open_external_url" => {
            let url: String = match de(args, "url") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            Some(
                open::that(&url)
                    .map_err(|e| format!("Failed to open URL: {}", e))
                    .and_then(|_| ok_null()),
            )
        }

        "get_sidebar_tree" => {
            let started = Instant::now();
            let result =
                crate::sidebar_controller::SidebarController::get_sidebar_tree(&state.db).await;
            crate::perf::record_sidebar_tree(started.elapsed().as_secs_f64() * 1000.0);
            Some(result.and_then(|v| to_json(&v)))
        }
        "reorder_sidebar_nodes" => {
            let moves: Vec<(String, i64)> = match de(args, "moves") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::sidebar_controller::SidebarController::reorder_sidebar_nodes(
                &state.db, moves,
            )
            .await;
            if let Err(e) = result {
                return Some(Err(e));
            }
            crate::events::emit_mutation(
                "reorder_sidebar_nodes",
                crate::events::MutationImpact::sidebar(crate::events::Domain::Sidebar),
            );
            Some(ok_null())
        }

        "get_view_prefs" => {
            let scope_key: String = de_opt::<String>(args, "scope_key").unwrap_or_default();
            let result = crate::view_prefs_controller::ViewPrefsController::get_view_prefs(
                &state.db, scope_key,
            )
            .await;
            Some(result.and_then(|v| to_json(&v)))
        }
        "set_view_prefs" => {
            let scope_key: String = de_opt::<String>(args, "scope_key").unwrap_or_default();
            let patch: ViewPrefsPatch = match de(args, "patch") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::view_prefs_controller::ViewPrefsController::set_view_prefs(
                &state.db, scope_key, patch,
            )
            .await;
            match result {
                Ok(ref v) => {
                    crate::events::emit_mutation(
                        "set_view_prefs",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::ViewPrefs])
                            .view_prefs(),
                    );
                    Some(to_json(v))
                }
                Err(e) => Some(Err(e)),
            }
        }

        "set_zoom_factor" => {
            let factor: f64 = match de(args, "factor") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let mut s = state.settings.get();
            s.zoom_factor = Some(factor);
            state.settings.update(s);
            crate::events::emit(
                crate::events::event_names::ZOOM_FACTOR_CHANGED,
                &crate::events::ZoomFactorChangedEvent { factor },
            );
            Some(ok_null())
        }
        "get_zoom_factor" => {
            let factor = state.settings.get().zoom_factor.unwrap_or(1.0);
            Some(to_json(&factor))
        }

        "enable_modern_window_style" => {
            tracing::debug!(command = command, "Legacy no-op command acknowledged");
            Some(ok_null())
        }

        _ => None,
    }
}
