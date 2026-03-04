//! Per-scope view settings (sort, layout, tile size, display options).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewPref {
    pub scope: String,
    pub sort_field: Option<String>,
    pub sort_dir: Option<String>,
    pub layout: Option<String>,
    pub tile_size: Option<i64>,
    pub show_name: Option<bool>,
    pub show_resolution: Option<bool>,
    pub show_extension: Option<bool>,
    pub show_label: Option<bool>,
    pub thumbnail_fit: Option<String>,
}

// ─── Standalone functions ───

pub fn get_view_pref(conn: &Connection, scope: &str) -> rusqlite::Result<Option<ViewPref>> {
    conn.query_row(
        "SELECT scope, sort_field, sort_dir, layout, tile_size,
                show_name, show_resolution, show_extension, show_label, thumbnail_fit
         FROM view_pref WHERE scope = ?1",
        [scope],
        |row| {
            Ok(ViewPref {
                scope: row.get(0)?,
                sort_field: row.get(1)?,
                sort_dir: row.get(2)?,
                layout: row.get(3)?,
                tile_size: row.get(4)?,
                show_name: row.get(5)?,
                show_resolution: row.get(6)?,
                show_extension: row.get(7)?,
                show_label: row.get(8)?,
                thumbnail_fit: row.get(9)?,
            })
        },
    )
    .optional()
}

pub fn set_view_pref(conn: &Connection, pref: &ViewPref) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO view_pref
         (scope, sort_field, sort_dir, layout, tile_size,
          show_name, show_resolution, show_extension, show_label, thumbnail_fit)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            pref.scope,
            pref.sort_field,
            pref.sort_dir,
            pref.layout,
            pref.tile_size,
            pref.show_name,
            pref.show_resolution,
            pref.show_extension,
            pref.show_label,
            pref.thumbnail_fit,
        ],
    )?;
    Ok(())
}

pub fn delete_view_pref(conn: &Connection, scope: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM view_pref WHERE scope = ?1", [scope])?;
    Ok(())
}

/// Get view pref for scope with fallback to system:all.
pub fn get_view_pref_with_fallback(
    conn: &Connection,
    scope: &str,
) -> rusqlite::Result<Option<ViewPref>> {
    if let Some(pref) = get_view_pref(conn, scope)? {
        return Ok(Some(pref));
    }
    if scope != "system:all" {
        return get_view_pref(conn, "system:all");
    }
    Ok(None)
}

// ─── High-level SqliteDatabase methods ───

impl SqliteDatabase {
    pub async fn get_view_pref(&self, scope: &str) -> Result<Option<ViewPref>, String> {
        let s = scope.to_string();
        self.with_read_conn(move |conn| get_view_pref_with_fallback(conn, &s))
            .await
    }

    pub async fn set_view_pref(&self, pref: ViewPref) -> Result<(), String> {
        self.with_conn(move |conn| set_view_pref(conn, &pref)).await
    }

    pub async fn delete_view_pref(&self, scope: &str) -> Result<(), String> {
        let s = scope.to_string();
        self.with_conn(move |conn| delete_view_pref(conn, &s)).await
    }
}
