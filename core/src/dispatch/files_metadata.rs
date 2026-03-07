//! File metadata handlers: tags, parents, rating, name, notes, view count,
//! source URLs, and storage stats.

use std::collections::HashMap;

use crate::state::AppState;
use crate::types::*;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_file_all_metadata" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::metadata_controller::MetadataController::get_file_all_metadata(
                &state.db,
                &state.ptr_db,
                hash,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_file_tags_display" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::metadata_controller::MetadataController::get_file_tags_display(
                &state.db,
                &state.ptr_db,
                hash,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_file_parents" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::metadata_controller::MetadataController::get_file_parents(&state.db, hash)
                    .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "update_rating" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let rating: Option<i64> = de_opt(args, "rating");
            let hash_clone = hash.clone();
            let result = crate::metadata_controller::MetadataController::update_rating(
                &state.db, hash, rating,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "update_rating",
                        crate::events::MutationImpact::file_metadata(hash_clone),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_file_name" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let name: Option<String> = de_opt(args, "name");
            let hash_clone = hash.clone();
            let result = crate::metadata_controller::MetadataController::set_file_name(
                &state.db, hash, name,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "set_file_name",
                        crate::events::MutationImpact::file_metadata(hash_clone),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "get_file_notes" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::metadata_controller::MetadataController::get_file_notes(&state.db, hash)
                    .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "set_file_notes" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let notes: HashMap<String, String> = match de(args, "notes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash_clone = hash.clone();
            let result = crate::metadata_controller::MetadataController::set_file_notes(
                &state.db, hash, notes,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "set_file_notes",
                        crate::events::MutationImpact::file_metadata(hash_clone),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "increment_view_count" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash_clone = hash.clone();
            let result = crate::metadata_controller::MetadataController::increment_view_count(
                &state.db, hash,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "increment_view_count",
                        crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Files,
                                crate::events::Domain::Sidebar,
                            ])
                            .metadata_hashes(vec![hash_clone.clone()])
                            .file_hashes(vec![hash_clone])
                            .sidebar_tree()
                            .grid_scopes(vec!["system:recently_viewed".to_string()]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_source_urls" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let urls: Vec<String> = match de(args, "urls") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let urls_json = if urls.is_empty() {
                None
            } else {
                match serde_json::to_string(&urls) {
                    Ok(s) => Some(s),
                    Err(e) => return Some(Err(e.to_string())),
                }
            };
            let result = state.db.set_source_urls(&hash, urls_json.as_deref()).await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "set_source_urls",
                        crate::events::MutationImpact::file_metadata(hash),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        "get_storage_stats" => {
            let result = state.db.count_files(None).await;
            Some(result.and_then(|file_count| to_json(&StorageStats { file_count })))
        }
        "get_image_storage_stats" => {
            let result = state.db.aggregate_file_stats().await;
            Some(result.and_then(|stats| to_json(&stats)))
        }

        _ => None,
    }
}
