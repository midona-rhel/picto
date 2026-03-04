//! Grid and file query domain handlers.

use std::time::Instant;

use crate::state::AppState;

use super::{de, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_grid_page_slim" => {
            let query: crate::types::GridPageSlimQuery = match de(args, "query") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let started = Instant::now();
            let result =
                crate::grid_controller::GridController::get_grid_page_slim(&state.db, query).await;
            crate::perf::record_grid_page_slim(started.elapsed().as_secs_f64() * 1000.0);
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_file" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::metadata_controller::MetadataController::get_file(&state.db, hash).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_files_metadata_batch" => {
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::grid_controller::GridController::get_files_metadata_batch(
                &state.db,
                &state.ptr_db,
                hashes,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_file_count" => {
            let result = state.db.count_files(None).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        _ => None,
    }
}
