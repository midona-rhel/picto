//! Duplicate-detection command handlers.

use crate::state::AppState;

use super::common::{de, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_duplicates" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::duplicate_controller::DuplicateController::get_duplicates(&state.db, hash)
                    .await;
            Some(match result {
                Ok(r) => to_json(&r),
                Err(e) => Err(e),
            })
        }
        "get_all_detected_duplicates" => {
            let result =
                crate::duplicate_controller::DuplicateController::get_all_detected_duplicates(
                    &state.db,
                )
                .await;
            Some(match result {
                Ok(r) => to_json(&r),
                Err(e) => Err(e),
            })
        }
        "scan_duplicates" => {
            let threshold: Option<u32> = de::<u32>(args, "threshold").ok();
            let effective_threshold = threshold.or_else(|| {
                let s = state.settings.get();
                Some(crate::settings::similarity_pct_to_distance(
                    s.duplicate_detect_similarity_pct,
                ))
            });
            let review_threshold = {
                let s = state.settings.get();
                Some(crate::settings::similarity_pct_to_distance(
                    s.duplicate_review_similarity_pct,
                ))
            };
            let result = crate::duplicate_controller::DuplicateController::scan_duplicates(
                &state.db,
                effective_threshold,
                review_threshold,
            )
            .await;
            Some(match result {
                Ok(r) => to_json(&r),
                Err(e) => Err(e),
            })
        }
        "get_duplicate_pairs" => {
            let cursor: Option<String> = de::<String>(args, "cursor").ok();
            let limit: usize = de::<usize>(args, "limit").unwrap_or(50);
            let status: Option<String> = de::<String>(args, "status").ok();
            let max_distance = match status.as_deref() {
                None | Some("detected") => {
                    let s = state.settings.get();
                    Some(crate::settings::similarity_pct_to_distance(
                        s.duplicate_review_similarity_pct,
                    ) as f64)
                }
                _ => None,
            };
            let result = crate::duplicate_controller::DuplicateController::get_duplicate_pairs(
                &state.db,
                cursor,
                limit,
                status,
                max_distance,
            )
            .await;
            Some(match result {
                Ok(r) => to_json(&r),
                Err(e) => Err(e),
            })
        }
        "resolve_duplicate_pair" => {
            let action: String = match de(args, "action") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash_a: String = match de(args, "hash_a") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash_b: String = match de(args, "hash_b") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let preferred_hash: Option<String> = de::<String>(args, "preferred_hash").ok();
            let result = crate::duplicate_controller::DuplicateController::resolve_duplicate_pair(
                &state.db,
                &action,
                hash_a,
                hash_b,
                preferred_hash,
            )
            .await;
            match result {
                Ok(r) => {
                    crate::events::emit_state_changed(
                        "resolve_duplicate_pair",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Files]),
                    );
                    Some(to_json(&r))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "get_duplicate_count" => {
            let count =
                crate::duplicate_controller::DuplicateController::get_duplicate_count(&state.db)
                    .await;
            Some(match count {
                Ok(c) => to_json(&serde_json::json!({ "count": c })),
                Err(e) => Err(e),
            })
        }
        "get_duplicate_settings" => {
            let s = state.settings.get();
            Some(to_json(&serde_json::json!({
                "duplicateDetectSimilarityPct": s.duplicate_detect_similarity_pct,
                "duplicateReviewSimilarityPct": s.duplicate_review_similarity_pct,
                "duplicateAutoMergeSimilarityPct": s.duplicate_auto_merge_similarity_pct,
                "duplicateAutoMergeSubscriptionsOnly": s.duplicate_auto_merge_subscriptions_only,
                "duplicateAutoMergeEnabled": s.duplicate_auto_merge_enabled,
            })))
        }
        "update_duplicate_settings" => {
            let mut s = state.settings.get();
            if let Ok(v) = de::<u32>(args, "duplicateDetectSimilarityPct") {
                s.duplicate_detect_similarity_pct = v.clamp(95, 100);
            }
            if let Ok(v) = de::<u32>(args, "duplicateReviewSimilarityPct") {
                s.duplicate_review_similarity_pct = v.clamp(95, 100);
            }
            if let Ok(v) = de::<u32>(args, "duplicateAutoMergeSimilarityPct") {
                s.duplicate_auto_merge_similarity_pct = v.clamp(95, 100);
            }
            if let Ok(v) = de::<bool>(args, "duplicateAutoMergeSubscriptionsOnly") {
                s.duplicate_auto_merge_subscriptions_only = v;
            }
            if let Ok(v) = de::<bool>(args, "duplicateAutoMergeEnabled") {
                s.duplicate_auto_merge_enabled = v;
            }
            state.settings.update(s);
            Some(to_json(&serde_json::json!({ "ok": true })))
        }
        _ => None,
    }
}
