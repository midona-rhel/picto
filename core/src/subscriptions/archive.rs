//! Gallery-dl archive reset helpers for subscriptions.
//!
//! Reset behavior is domain policy, not controller glue. Keeping it isolated
//! makes reset semantics testable without dragging run orchestration with it.

use std::path::Path;

pub fn subscription_query_archive_prefix(subscription_id: i64, query_id: i64) -> String {
    format!("picto_s{subscription_id}_q{query_id}_")
}

/// Remove archive entries for specific posts of one query, so an interrupted
/// post (some files downloaded, some not) is re-fetched WHOLE on the next run.
/// Without this, gallery-dl would archive-skip the already-downloaded files and
/// the post could never complete.
pub async fn clear_post_archive_entries_at_root(
    library_root: &Path,
    archive_prefix: &str,
    post_ids: &[String],
) -> Result<(), String> {
    if post_ids.is_empty() {
        return Ok(());
    }
    let archive_path = library_root.join("gdl-archive.sqlite3");
    if !archive_path.exists() {
        return Ok(());
    }
    let escape = |value: &str| {
        value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    };
    let patterns: Vec<String> = post_ids
        .iter()
        .map(|post_id| format!("{}%{}%", escape(archive_prefix), escape(post_id)))
        .collect();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = rusqlite::Connection::open(&archive_path)
            .map_err(|e| format!("Failed to open gallery-dl archive: {e}"))?;
        conn.execute_batch("PRAGMA busy_timeout = 5000;")
            .map_err(|e| format!("Failed to configure gallery-dl archive connection: {e}"))?;
        let mut stmt = match conn.prepare("DELETE FROM archive WHERE entry LIKE ?1 ESCAPE '\\'") {
            Ok(stmt) => stmt,
            Err(e) if e.to_string().contains("no such table: archive") => return Ok(()),
            Err(e) => return Err(format!("Failed to prepare archive delete: {e}")),
        };
        for pattern in patterns {
            stmt.execute([&pattern])
                .map_err(|e| format!("Failed to delete archive entries: {e}"))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Archive cleanup task failed: {e}"))?
}
