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
