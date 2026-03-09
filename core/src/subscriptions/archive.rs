//! Gallery-dl archive reset helpers for subscriptions.
//!
//! Reset behavior is domain policy, not controller glue. Keeping it isolated
//! makes reset semantics testable without dragging run orchestration with it.

use crate::sqlite::SqliteDatabase;

pub async fn clear_subscription_archive_entries(
    db: &SqliteDatabase,
    archive_prefixes: &[String],
) -> Result<(), String> {
    if archive_prefixes.is_empty() {
        return Ok(());
    }

    let archive_path = db
        .db_dir()
        .parent()
        .map(|r| r.join("gdl-archive.sqlite3"))
        .unwrap_or_else(|| std::path::PathBuf::from("gdl-archive.sqlite3"));
    if !archive_path.exists() {
        return Ok(());
    }

    let prefixes = archive_prefixes.to_vec();
    let (deleted_rows, remaining_rows) =
        tokio::task::spawn_blocking(move || -> Result<(usize, usize), String> {
            let conn = rusqlite::Connection::open(&archive_path)
                .map_err(|e| format!("Failed to open gallery-dl archive: {e}"))?;
            conn.execute_batch("PRAGMA busy_timeout = 5000;")
                .map_err(|e| format!("Failed to configure gallery-dl archive connection: {e}"))?;
            let mut deleted = 0usize;
            let mut stmt = match conn.prepare("DELETE FROM archive WHERE entry LIKE ?1 ESCAPE '\\'") {
                Ok(stmt) => stmt,
                Err(e) => {
                    if e.to_string().contains("no such table: archive") {
                        return Ok((0, 0));
                    }
                    return Err(format!("Failed to prepare gallery-dl archive delete: {e}"));
                }
            };
            let mut count_stmt =
                match conn.prepare("SELECT COUNT(*) FROM archive WHERE entry LIKE ?1 ESCAPE '\\'") {
                    Ok(stmt) => stmt,
                    Err(e) => {
                        if e.to_string().contains("no such table: archive") {
                            return Ok((0, 0));
                        }
                        return Err(format!("Failed to prepare gallery-dl archive count: {e}"));
                    }
                };

            let mut remaining = 0usize;
            for prefix in prefixes {
                let escaped_prefix = prefix
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                let pattern = format!("{escaped_prefix}%");
                let removed = stmt
                    .execute([pattern.clone()])
                    .map_err(|e| format!("Failed to clear gallery-dl archive entries: {e}"))?;
                deleted += removed;

                let still: i64 = count_stmt
                    .query_row([pattern], |row| row.get(0))
                    .map_err(|e| {
                        format!("Failed to count remaining gallery-dl archive entries: {e}")
                    })?;
                if still > 0 {
                    remaining += still as usize;
                }
            }
            Ok((deleted, remaining))
        })
        .await
        .map_err(|e| format!("Gallery-dl archive reset task failed: {e}"))??;

    tracing::info!(
        deleted_rows,
        remaining_rows,
        prefixes = archive_prefixes.len(),
        "Subscription reset: cleared gallery-dl archive rows"
    );

    if remaining_rows > 0 {
        tracing::warn!(
            remaining_rows,
            "Subscription reset: some archive rows still match reset prefixes"
        );
    }

    Ok(())
}
