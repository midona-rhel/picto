use std::path::Path;

use tracing::warn;

/// After import, call this to remove the temp download directory.
pub async fn cleanup_temp_dir(temp_dir: &Path) {
    if let Err(e) = tokio::fs::remove_dir_all(temp_dir).await {
        warn!(path = %temp_dir.display(), error = %e, "Failed to clean up temp dir");
    }
}
