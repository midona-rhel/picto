//! Gallery-dl archive reset helpers for subscriptions.
//!
//! Reset behavior is domain policy, not controller glue. Keeping it isolated
//! makes reset semantics testable without dragging run orchestration with it.

use std::path::{Path, PathBuf};

fn subscription_archive_root(library_root: &Path, subscription_id: i64) -> PathBuf {
    library_root
        .join("source-runners/gallery-dl")
        .join(format!("subscription-{subscription_id}"))
}

pub fn query_archive_path(library_root: &Path, subscription_id: i64, query_id: i64) -> PathBuf {
    subscription_archive_root(library_root, subscription_id)
        .join(format!("query-{query_id}"))
        .join("archive.sqlite3")
}

pub fn query_temp_root(library_root: &Path, subscription_id: i64, query_id: i64) -> PathBuf {
    subscription_archive_root(library_root, subscription_id)
        .join(format!("query-{query_id}"))
        .join("runs")
}

/// Remove archive entries for specific posts of one query, so an interrupted
/// post (some files downloaded, some not) is re-fetched WHOLE on the next run.
/// Without this, gallery-dl would archive-skip the already-downloaded files and
/// the post could never complete.
pub async fn clear_post_archive_entries_at_root(
    library_root: &Path,
    subscription_id: i64,
    query_id: i64,
    post_ids: &[String],
) -> Result<(), String> {
    if post_ids.is_empty() {
        return Ok(());
    }
    let patterns: Vec<String> = post_ids
        .iter()
        .map(|post_id| format!("%{}%", escape_like(post_id)))
        .collect();
    clear_archive_patterns_at_path(
        query_archive_path(library_root, subscription_id, query_id),
        patterns,
    )
    .await
    .map(|_| ())
}

/// Forget every gallery-dl archive entry owned by one subscription while
/// preserving every other subscription's physically separate state.
pub fn clear_subscription_archive_entries_at_root(
    library_root: &Path,
    subscription_id: i64,
) -> Result<usize, String> {
    let archive_root = subscription_archive_root(library_root, subscription_id);
    remove_state_directory(&archive_root, "subscription")
}

pub fn clear_query_archive_at_root(
    library_root: &Path,
    subscription_id: i64,
    query_id: i64,
) -> Result<usize, String> {
    let archive_path = query_archive_path(library_root, subscription_id, query_id);
    let query_root = archive_path
        .parent()
        .expect("query archive always has a parent directory");
    remove_state_directory(query_root, "query")
}

fn remove_state_directory(path: &Path, owner: &str) -> Result<usize, String> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(1),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(format!("Failed to clear gallery-dl {owner} state: {error}")),
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

async fn clear_archive_patterns_at_path(
    archive_path: PathBuf,
    patterns: Vec<String>,
) -> Result<usize, String> {
    if !archive_path.exists() {
        return Ok(0);
    }
    tokio::task::spawn_blocking(move || -> Result<usize, String> {
        let conn = rusqlite::Connection::open(&archive_path)
            .map_err(|e| format!("Failed to open gallery-dl archive: {e}"))?;
        conn.execute_batch("PRAGMA busy_timeout = 5000;")
            .map_err(|e| format!("Failed to configure gallery-dl archive connection: {e}"))?;
        let mut stmt = match conn.prepare("DELETE FROM archive WHERE entry LIKE ?1 ESCAPE '\\'") {
            Ok(stmt) => stmt,
            Err(e) if e.to_string().contains("no such table: archive") => return Ok(0),
            Err(e) => return Err(format!("Failed to prepare archive delete: {e}")),
        };
        let mut deleted = 0;
        for pattern in patterns {
            deleted += stmt
                .execute([&pattern])
                .map_err(|e| format!("Failed to delete archive entries: {e}"))?;
        }
        Ok(deleted)
    })
    .await
    .map_err(|e| format!("Archive cleanup task failed: {e}"))?
}
