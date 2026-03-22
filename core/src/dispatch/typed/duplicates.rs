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
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateDuplicateSettingsInput {
    #[serde(default, rename = "duplicateDetectSimilarityPct")]
    #[ts(type = "number | null")]
    pub duplicate_detect_similarity_pct: Option<u32>,
    #[serde(default, rename = "duplicateReviewSimilarityPct")]
    #[ts(type = "number | null")]
    pub duplicate_review_similarity_pct: Option<u32>,
    #[serde(default, rename = "duplicateAutoMergeSimilarityPct")]
    #[ts(type = "number | null")]
    pub duplicate_auto_merge_similarity_pct: Option<u32>,
    #[serde(default, rename = "duplicateAutoMergeRequireMatchingDimensions")]
    pub duplicate_auto_merge_require_matching_dimensions: Option<bool>,
    #[serde(default, rename = "duplicateAutoMergeSubscriptionsOnly")]
    pub duplicate_auto_merge_subscriptions_only: Option<bool>,
    #[serde(default, rename = "duplicateAutoMergeEnabled")]
    pub duplicate_auto_merge_enabled: Option<bool>,
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
    let result = crate::duplicates::orchestrator::DuplicateOrchestrator::find_similar(
        &state.db,
        &state.blob_store,
        &input.hash,
    )
    .await?;
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
    let result = crate::duplicates::orchestrator::DuplicateOrchestrator::scan_duplicates(
        &state.db,
        &state.blob_store,
        effective_threshold,
        review_threshold,
    )
    .await?;
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
    let result = crate::duplicates::orchestrator::DuplicateOrchestrator::get_duplicate_pairs(
        &state.db,
        input.cursor,
        input.limit,
        input.status,
        max_distance,
    )
    .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn resolve_duplicate_pair(
    state: &AppState,
    input: ResolveDuplicatePairInput,
) -> Result<serde_json::Value, String> {
    let result = crate::duplicates::orchestrator::DuplicateOrchestrator::resolve_duplicate_pair(
        &state.db,
        &state.blob_store,
        &input.action,
        input.hash_a,
        input.hash_b,
    )
    .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn get_duplicate_count(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let count =
        crate::duplicates::orchestrator::DuplicateOrchestrator::get_duplicate_count(&state.db)
            .await?;
    Ok(serde_json::json!({ "count": count }))
}

pub async fn get_duplicate_settings(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let s = state.settings.get();
    Ok(serde_json::json!({
        "duplicateDetectSimilarityPct": s.duplicate_detect_similarity_pct,
        "duplicateReviewSimilarityPct": s.duplicate_review_similarity_pct,
        "duplicateAutoMergeSimilarityPct": s.duplicate_auto_merge_similarity_pct,
        "duplicateAutoMergeRequireMatchingDimensions": s.duplicate_auto_merge_require_matching_dimensions,
        "duplicateAutoMergeSubscriptionsOnly": s.duplicate_auto_merge_subscriptions_only,
        "duplicateAutoMergeEnabled": s.duplicate_auto_merge_enabled,
    }))
}

pub async fn update_duplicate_settings(
    state: &AppState,
    input: UpdateDuplicateSettingsInput,
) -> Result<serde_json::Value, String> {
    let mut s = state.settings.get();
    if let Some(v) = input.duplicate_detect_similarity_pct {
        s.duplicate_detect_similarity_pct = v.clamp(95, 100);
    }
    if let Some(v) = input.duplicate_review_similarity_pct {
        s.duplicate_review_similarity_pct = v.clamp(95, 100);
    }
    if let Some(v) = input.duplicate_auto_merge_similarity_pct {
        s.duplicate_auto_merge_similarity_pct = v.clamp(95, 100);
    }
    if let Some(v) = input.duplicate_auto_merge_require_matching_dimensions {
        s.duplicate_auto_merge_require_matching_dimensions = v;
    }
    if let Some(v) = input.duplicate_auto_merge_subscriptions_only {
        s.duplicate_auto_merge_subscriptions_only = v;
    }
    if let Some(v) = input.duplicate_auto_merge_enabled {
        s.duplicate_auto_merge_enabled = v;
    }
    state.settings.update(s);
    Ok(serde_json::json!({ "ok": true }))
}
