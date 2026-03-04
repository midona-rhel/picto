//! File domain routing layer — delegates to lifecycle, metadata, media, and
//! review submodules.

use crate::state::AppState;

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    if let r @ Some(_) = super::files_lifecycle::handle(state, command, args).await {
        return r;
    }
    if let r @ Some(_) = super::files_metadata::handle(state, command, args).await {
        return r;
    }
    if let r @ Some(_) = super::files_media::handle(state, command, args).await {
        return r;
    }
    if let r @ Some(_) = super::files_review::handle(state, command, args).await {
        return r;
    }
    None
}
