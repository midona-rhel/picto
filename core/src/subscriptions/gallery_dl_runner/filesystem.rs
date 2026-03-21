use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use super::{DownloadedItem, ParsedMetadata, parse_metadata};

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

/// Walk the temp directory and pair each media file with its `.json` sidecar.
pub(super) async fn scan_output_dir(dir: &Path) -> Result<Vec<DownloadedItem>, String> {
    let mut items = Vec::new();
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .map_err(|e| format!("Read dir error: {e}"))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            // Skip JSON sidecars and config files — we'll find them when processing media files
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "json" {
                continue;
            }

            // Look for matching sidecar: {filename}.json
            let sidecar_path = path.with_extension(format!(
                "{}.json",
                path.extension().and_then(|e| e.to_str()).unwrap_or("")
            ));

            let metadata = if sidecar_path.is_file() {
                match tokio::fs::read_to_string(&sidecar_path).await {
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
                debug!(path = %path.display(), "No sidecar found");
                ParsedMetadata::default()
            };

            items.push(DownloadedItem {
                file_path: path,
                metadata,
            });
        }
    }

    // Sort by post_id descending (newest first) so import order matches
    // the source site's default ordering. Filesystem read_dir order is arbitrary.
    items.sort_by(|a, b| {
        let id_a = a.metadata.post_id.as_deref().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        let id_b = b.metadata.post_id.as_deref().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        id_b.cmp(&id_a)
    });

    Ok(items)
}
