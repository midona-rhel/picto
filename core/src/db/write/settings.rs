//! Settings and view preference writes.

use rusqlite::{params, Connection};

pub fn set_kv(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO kv_settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_kv(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT value FROM kv_settings WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
}

pub fn set_view_pref(
    conn: &Connection,
    scope: &str,
    sort_field: Option<&str>,
    sort_dir: Option<&str>,
    layout: Option<&str>,
    tile_size: Option<i64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO view_pref (scope, sort_field, sort_dir, layout, tile_size) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![scope, sort_field, sort_dir, layout, tile_size],
    )?;
    Ok(())
}
