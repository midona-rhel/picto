use std::path::Path;

use tracing::{debug, warn};

use super::{ParsedMetadata, parse_metadata};

/// Parse the sidecar JSON for a single file path (streaming import).
/// Gallery-dl writes `{filename}.{ext}.json` — e.g., `image.jpg` → `image.jpg.json`.
pub(super) fn parse_sidecar_for_file(file_path: &Path) -> ParsedMetadata {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let sidecar_path = file_path.with_extension(format!("{ext}.json"));

    if sidecar_path.is_file() {
        match std::fs::read_to_string(&sidecar_path) {
            Ok(json_str) => match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(json) => parse_metadata(&json),
                Err(e) => {
                    warn!(path = %sidecar_path.display(), error = %e, "Sidecar parse error");
                    ParsedMetadata::default()
                }
            },
            Err(e) => {
                warn!(path = %sidecar_path.display(), error = %e, "Sidecar read error");
                ParsedMetadata::default()
            }
        }
    } else {
        debug!(path = %file_path.display(), "No sidecar found");
        ParsedMetadata::default()
    }
}

/// After import, call this to remove the temp download directory.
pub async fn cleanup_temp_dir(temp_dir: &Path) {
    if let Err(e) = tokio::fs::remove_dir_all(temp_dir).await {
        warn!(path = %temp_dir.display(), error = %e, "Failed to clean up temp dir");
    }
}
