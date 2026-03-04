//! Selection domain handlers.

use crate::state::AppState;
use crate::types::*;

use super::common::{de, de_opt, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "add_tags_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::selection_controller::SelectionController::add_tags_selection(
                &state.db,
                selection,
                tag_strings,
            )
            .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "add_tags_selection",
                            crate::events::MutationImpact::new()
                                .domains(&[
                                    crate::events::Domain::Tags,
                                    crate::events::Domain::Files,
                                    crate::events::Domain::Selection,
                                ])
                                .selection_summary()
                                .grid_all(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_tags_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::selection_controller::SelectionController::remove_tags_selection(
                &state.db,
                selection,
                tag_strings,
            )
            .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "remove_tags_selection",
                            crate::events::MutationImpact::new()
                                .domains(&[
                                    crate::events::Domain::Tags,
                                    crate::events::Domain::Files,
                                    crate::events::Domain::Selection,
                                ])
                                .selection_summary()
                                .grid_all(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "get_selection_summary" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let started = std::time::Instant::now();
            let result = crate::selection_controller::SelectionController::get_selection_summary(
                &state.db, selection,
            )
            .await;
            crate::perf::record_selection_summary(started.elapsed().as_secs_f64() * 1000.0);
            Some(result.and_then(|r| to_json(&r)))
        }
        "update_rating_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let rating: Option<i64> = de_opt(args, "rating");
            let result = crate::selection_controller::SelectionController::update_rating_selection(
                &state.db, selection, rating,
            )
            .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "update_rating_selection",
                            crate::events::MutationImpact::new()
                                .domains(&[crate::events::Domain::Files])
                                .selection_summary()
                                .grid_all(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_notes_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let notes: std::collections::HashMap<String, String> = match de(args, "notes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::selection_controller::SelectionController::set_notes_selection(
                &state.db, selection, notes,
            )
            .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "set_notes_selection",
                            crate::events::MutationImpact::new()
                                .domains(&[crate::events::Domain::Files])
                                .selection_summary(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_source_urls_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let urls: Vec<String> = match de(args, "urls") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::selection_controller::SelectionController::set_source_urls_selection(
                    &state.db, selection, urls,
                )
                .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "set_source_urls_selection",
                            crate::events::MutationImpact::new()
                                .domains(&[crate::events::Domain::Files])
                                .selection_summary(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        _ => None,
    }
}
