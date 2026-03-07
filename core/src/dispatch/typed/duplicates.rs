//! Typed command implementations for duplicate-detection operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetDuplicatesInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct ScanDuplicatesInput {
    #[ts(type = "number | null")]
    #[serde(default)]
    pub threshold: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
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
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct ResolveDuplicatePairInput {
    pub action: String,
    pub hash_a: String,
    pub hash_b: String,
    #[serde(default)]
    pub preferred_hash: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
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
    #[serde(default, rename = "duplicateAutoMergeSubscriptionsOnly")]
    pub duplicate_auto_merge_subscriptions_only: Option<bool>,
    #[serde(default, rename = "duplicateAutoMergeEnabled")]
    pub duplicate_auto_merge_enabled: Option<bool>,
}

// ─── Command structs ───────────────────────────────────────────────────────

struct GetDuplicates;
struct ScanDuplicates;
struct GetDuplicatePairs;
struct ResolveDuplicatePair;
struct GetDuplicateCount;
struct GetDuplicateSettings;
struct UpdateDuplicateSettings;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for GetDuplicates {
    const NAME: &'static str = "get_duplicates";
    type Input = GetDuplicatesInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::duplicates::controller::DuplicateController::get_duplicates(&state.db, input.hash)
                .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for ScanDuplicates {
    const NAME: &'static str = "scan_duplicates";
    type Input = ScanDuplicatesInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
        let result = crate::duplicates::controller::DuplicateController::scan_duplicates(
            &state.db,
            effective_threshold,
            review_threshold,
        )
        .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetDuplicatePairs {
    const NAME: &'static str = "get_duplicate_pairs";
    type Input = GetDuplicatePairsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let max_distance = match input.status.as_deref() {
            None | Some("detected") => {
                let s = state.settings.get();
                Some(crate::settings::store::similarity_pct_to_distance(
                    s.duplicate_review_similarity_pct,
                ) as f64)
            }
            _ => None,
        };
        let result = crate::duplicates::controller::DuplicateController::get_duplicate_pairs(
            &state.db,
            input.cursor,
            input.limit,
            input.status,
            max_distance,
        )
        .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for ResolveDuplicatePair {
    const NAME: &'static str = "resolve_duplicate_pair";
    type Input = ResolveDuplicatePairInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::duplicates::controller::DuplicateController::resolve_duplicate_pair(
            &state.db,
            &input.action,
            input.hash_a,
            input.hash_b,
            input.preferred_hash,
        )
        .await?;
        crate::events::emit_mutation(
            "resolve_duplicate_pair",
            crate::events::MutationImpact::domain_only(crate::events::Domain::Files),
        );
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetDuplicateCount {
    const NAME: &'static str = "get_duplicate_count";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let count =
            crate::duplicates::controller::DuplicateController::get_duplicate_count(&state.db)
                .await?;
        Ok(serde_json::json!({ "count": count }))
    }
}

impl TypedCommand for GetDuplicateSettings {
    const NAME: &'static str = "get_duplicate_settings";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let s = state.settings.get();
        Ok(serde_json::json!({
            "duplicateDetectSimilarityPct": s.duplicate_detect_similarity_pct,
            "duplicateReviewSimilarityPct": s.duplicate_review_similarity_pct,
            "duplicateAutoMergeSimilarityPct": s.duplicate_auto_merge_similarity_pct,
            "duplicateAutoMergeSubscriptionsOnly": s.duplicate_auto_merge_subscriptions_only,
            "duplicateAutoMergeEnabled": s.duplicate_auto_merge_enabled,
        }))
    }
}

impl TypedCommand for UpdateDuplicateSettings {
    const NAME: &'static str = "update_duplicate_settings";
    type Input = UpdateDuplicateSettingsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
        if let Some(v) = input.duplicate_auto_merge_subscriptions_only {
            s.duplicate_auto_merge_subscriptions_only = v;
        }
        if let Some(v) = input.duplicate_auto_merge_enabled {
            s.duplicate_auto_merge_enabled = v;
        }
        state.settings.update(s);
        Ok(serde_json::json!({ "ok": true }))
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        GetDuplicates::NAME => Some(run_typed::<GetDuplicates>(state, args).await),
        ScanDuplicates::NAME => Some(run_typed::<ScanDuplicates>(state, args).await),
        GetDuplicatePairs::NAME => Some(run_typed::<GetDuplicatePairs>(state, args).await),
        ResolveDuplicatePair::NAME => Some(run_typed::<ResolveDuplicatePair>(state, args).await),
        GetDuplicateCount::NAME => Some(run_typed::<GetDuplicateCount>(state, args).await),
        GetDuplicateSettings::NAME => Some(run_typed::<GetDuplicateSettings>(state, args).await),
        UpdateDuplicateSettings::NAME => {
            Some(run_typed::<UpdateDuplicateSettings>(state, args).await)
        }
        _ => None,
    }
}
