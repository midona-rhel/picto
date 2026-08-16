//! Handler functions for media lifecycle operations:
//! import, status changes, deletion, and FTS rebuild.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddMediaInput {
    pub paths: Vec<String>,
    #[serde(default)]
    pub tag_strings: Option<Vec<String>>,
    #[serde(default)]
    pub source_urls: Option<Vec<String>>,
    #[serde(default = "default_initial_status")]
    #[ts(type = "number")]
    pub initial_status: i64,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub parent_folder_id: Option<i64>,
    #[serde(default)]
    pub preserve_structure: bool,
}

fn default_initial_status() -> i64 {
    1
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn add_media(state: &AppState, input: AddMediaInput) -> Result<(), String> {
    if input.paths.is_empty() {
        return Err("At least one media path is required".to_string());
    }
    if input.preserve_structure && input.paths.len() != 1 {
        return Err("Preserving folder structure requires exactly one folder path".to_string());
    }

    // Reject paths inside the library directory to prevent circular imports
    let library_root = &state.library_root;
    for p in &input.paths {
        if let Ok(canonical) = std::fs::canonicalize(p) {
            if canonical.starts_with(library_root) {
                return Err(format!(
                    "Cannot import files from inside the library directory: {}",
                    canonical.display()
                ));
            }
        }
    }

    state
        .engine
        .add_media(
            input.paths,
            input.tag_strings,
            input.source_urls,
            input.initial_status,
            input.parent_folder_id,
            input.preserve_structure,
            Some(&state.library_root),
        )
        .await
}
