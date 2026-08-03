//! Handler functions for duplicate-detection operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ScanDuplicatesInput {
    #[ts(type = "number | null")]
    #[serde(default)]
    pub threshold: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetDuplicatePairsInput {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_duplicate_pairs_limit")]
    #[ts(type = "number")]
    pub limit: usize,
    #[serde(default)]
    pub status: Option<String>,
}

fn default_duplicate_pairs_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResolveDuplicatePairInput {
    pub action: String,
    pub hash_a: String,
    pub hash_b: String,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub preferred_collection_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct FindSimilarInput {
    pub hash: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn find_similar(
    state: &AppState,
    input: FindSimilarInput,
) -> Result<serde_json::Value, String> {
    let result = state.engine.find_similar(&input.hash)?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn scan_duplicates(
    state: &AppState,
    input: ScanDuplicatesInput,
) -> Result<serde_json::Value, String> {
    let effective_threshold = input.threshold.or_else(|| {
        let s = state.settings.get();
        Some(crate::settings::store::similarity_pct_to_distance(
            s.duplicate_detect_similarity_pct,
        ))
    });
    let review_threshold = {
        let s = state.settings.get();
        Some(crate::settings::store::similarity_pct_to_distance(
            s.duplicate_review_similarity_pct,
        ))
    };
    let result = state
        .engine
        .scan_duplicates(effective_threshold, review_threshold)?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn get_duplicate_pairs(
    state: &AppState,
    input: GetDuplicatePairsInput,
) -> Result<serde_json::Value, String> {
    let max_distance = match input.status.as_deref() {
        None | Some("detected") => {
            let s = state.settings.get();
            Some(crate::settings::store::similarity_pct_to_distance(
                s.duplicate_review_similarity_pct,
            ) as f64)
        }
        _ => None,
    };
    let result =
        state
            .engine
            .get_duplicate_pairs(input.cursor, input.limit, input.status, max_distance)?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn resolve_duplicate_pair(
    state: &AppState,
    input: ResolveDuplicatePairInput,
) -> Result<serde_json::Value, String> {
    let result = state.engine.resolve_duplicate_pair(
        &input.action,
        &input.hash_a,
        &input.hash_b,
        input.preferred_collection_id,
    )?;
    if matches!(
        result.status,
        crate::db::types::DuplicateResolveStatus::Resolved
    ) {
        if let Some(loser_hash) = result.loser_hash.as_deref() {
            let _ = state.blob_store.delete(loser_hash);
        }
    }
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}
