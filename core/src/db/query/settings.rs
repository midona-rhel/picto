//! Canonical view preference reads.

use rusqlite::{params, Connection, OptionalExtension};

use crate::settings::types::ViewPref;

pub fn get_view_pref(conn: &Connection, scope: &str) -> rusqlite::Result<Option<ViewPref>> {
    conn.query_row(
        "SELECT scope, sort_field, sort_dir, layout, tile_size,
                show_name, show_resolution, show_extension, show_label, thumbnail_fit
         FROM view_pref
         WHERE scope = ?1",
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

pub fn get_view_pref_with_fallback(
    conn: &Connection,
    scope: &str,
) -> rusqlite::Result<Option<ViewPref>> {
    if let Some(pref) = get_view_pref(conn, scope)? {
        return Ok(Some(pref));
    }
    if scope != "system:active" {
        return get_view_pref(conn, "system:active");
    }
    Ok(None)
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
