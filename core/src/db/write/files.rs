//! Media file write operations — insert and analysis updates.

use rusqlite::{params, Connection};

/// Insert a new media file record. Returns the assigned file_id.
pub fn insert_file(
    conn: &Connection,
    file_hash: &str,
    mime_type: &str,
    size_bytes: i64,
    pixel_width: Option<i64>,
    pixel_height: Option<i64>,
    duration_ms: Option<i64>,
    frame_count: Option<i64>,
    has_audio: bool,
    date_added: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO media_file (file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio, date_added)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio as i64, date_added],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update file analysis fields (perceptual hash, dominant color).
pub fn update_file_analysis(
    conn: &Connection,
    file_id: i64,
    perceptual_hash: Option<&str>,
    dominant_color_hex: Option<&str>,
    dominant_palette_blob: Option<&[u8]>,
) -> rusqlite::Result<()> {
    if let Some(ph) = perceptual_hash {
        conn.execute(
            "UPDATE media_file SET perceptual_hash = ?1 WHERE file_id = ?2",
            params![ph, file_id],
        )?;
    }
    if let Some(hex) = dominant_color_hex {
        conn.execute(
            "UPDATE media_file SET dominant_color_hex = ?1 WHERE file_id = ?2",
            params![hex, file_id],
        )?;
    }
    if let Some(blob) = dominant_palette_blob {
        conn.execute(
            "UPDATE media_file SET dominant_palette_blob = ?1 WHERE file_id = ?2",
            params![blob, file_id],
        )?;
    }
    Ok(())
}

pub fn replace_file_phash(
    conn: &Connection,
    file_id: i64,
    perceptual_hash: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE media_file SET perceptual_hash = ?1 WHERE file_id = ?2",
        params![perceptual_hash, file_id],
    )?;
    Ok(())
}

pub fn replace_file_dominant_color(
    conn: &Connection,
    file_id: i64,
    dominant_color_hex: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE media_file SET dominant_color_hex = ?1 WHERE file_id = ?2",
        params![dominant_color_hex, file_id],
    )?;
    Ok(())
}

pub fn replace_file_color_analysis(
    conn: &Connection,
    file_id: i64,
    colors: &[(String, f32, f32, f32)],
    dominant_color_hex: Option<&str>,
    dominant_palette_blob: Option<&[u8]>,
    color_analysis_version: i64,
) -> rusqlite::Result<()> {
    save_file_colors(conn, file_id, colors)?;
    conn.execute(
        "UPDATE media_file
         SET dominant_color_hex = ?1,
             dominant_palette_blob = ?2,
             color_analysis_version = ?3
         WHERE file_id = ?4",
        params![
            dominant_color_hex,
            dominant_palette_blob,
            color_analysis_version,
            file_id
        ],
    )?;
    Ok(())
}

pub fn save_file_colors(
    conn: &Connection,
    file_id: i64,
    colors: &[(String, f32, f32, f32)],
) -> rusqlite::Result<()> {
    {
        let mut rid_stmt =
            conn.prepare_cached("SELECT rowid FROM file_color WHERE file_id = ?1")?;
        let existing_rowids: Vec<i64> = rid_stmt
            .query_map([file_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if !existing_rowids.is_empty() {
            let mut rtree_del =
                conn.prepare_cached("DELETE FROM file_color_rtree WHERE id = ?1")?;
            for rid in &existing_rowids {
                rtree_del.execute([rid])?;
            }
        }
    }

    conn.execute("DELETE FROM file_color WHERE file_id = ?1", [file_id])?;

    let mut color_stmt = conn.prepare_cached(
        "INSERT INTO file_color (file_id, hex, l, a, b) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut rtree_delete_stmt =
        conn.prepare_cached("DELETE FROM file_color_rtree WHERE id = ?1")?;
    let mut rtree_stmt = conn.prepare_cached(
        "INSERT INTO file_color_rtree (id, min_l, max_l, min_a, max_a, min_b, max_b)
         VALUES (?1, ?2, ?2, ?3, ?3, ?4, ?4)",
    )?;
    for (hex, l, a, b) in colors {
        color_stmt.execute(params![file_id, hex, l, a, b])?;
        let rowid = conn.last_insert_rowid();
        rtree_delete_stmt.execute(params![rowid])?;
        rtree_stmt.execute(params![rowid, l, a, b])?;
    }
    Ok(())
}

pub fn delete_file(conn: &Connection, file_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM media_file WHERE file_id = ?1", [file_id])?;
    Ok(())
}
