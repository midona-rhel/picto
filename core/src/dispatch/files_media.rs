//! File media handlers — migrated to typed dispatch.
//!
//! All commands have been moved to `dispatch::typed::files_media`.
//! This module is retained only for the legacy handler signature (returns None).

use crate::state::AppState;

pub async fn handle(
    _state: &AppState,
    _command: &str,
    _args: &serde_json::Value,
) -> Option<Result<String, String>> {
    None
}
