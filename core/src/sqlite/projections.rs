//! Shared metadata projection builder.
//!
//! Domain-owned metadata reads live under `crate::metadata`.

use super::files::FileMetadataSlim;
use crate::tags::db::FileTagInfo;
use rusqlite::{params, Connection};

/// Batch build projections for multiple files using a single JOIN query.
pub fn build_projections_batch(
    conn: &Connection,
    file_ids: &[i64],
    epoch: i64,
) -> rusqlite::Result<()> {
    if file_ids.is_empty() {
        return Ok(());
    }

    // Process in chunks to avoid exceeding SQLite parameter limits
    const CHUNK_SIZE: usize = 500;
    for chunk in file_ids.chunks(CHUNK_SIZE) {
        build_projections_batch_chunk(conn, chunk, epoch)?;
    }
    Ok(())
}

fn build_projections_batch_chunk(
    conn: &Connection,
    file_ids: &[i64],
    epoch: i64,
) -> rusqlite::Result<()> {
    if file_ids.is_empty() {
        return Ok(());
    }

    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let ph_str = placeholders.join(",");

    let sql = format!(
        "SELECT f.file_id, f.hash, f.name, f.mime, f.width, f.height, f.size, f.status,
                f.rating, f.imported_at, f.dominant_color_hex,
                f.duration_ms, f.num_frames, f.has_audio, f.view_count,
                t.tag_id, t.namespace, t.subtag, td.display_ns, td.display_st, etr.source
         FROM file f
         LEFT JOIN entity_tag_raw etr ON etr.entity_id = f.file_id
         LEFT JOIN tag t ON t.tag_id = etr.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE f.file_id IN ({ph_str})
         ORDER BY f.file_id, t.namespace, t.subtag"
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = file_ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params.as_slice())?;

    let mut current_file_id: Option<i64> = None;
    let mut current_slim: Option<FileMetadataSlim> = None;
    let mut current_tags: Vec<FileTagInfo> = Vec::new();

    let mut upsert_stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO entity_metadata_projection (entity_id, epoch, resolved_json, parents_json)
         VALUES (?1, ?2, ?3, ?4)",
    )?;

    let flush = |upsert: &mut rusqlite::Statement,
                 file_id: i64,
                 slim: FileMetadataSlim,
                 tags: Vec<FileTagInfo>|
     -> rusqlite::Result<()> {
        let resolved = crate::metadata::db::ResolvedMetadata { file: slim, tags };
        let resolved_json = serde_json::to_string(&resolved).unwrap_or_default();
        upsert.execute(params![file_id, epoch, resolved_json, "[]"])?;
        Ok(())
    };

    while let Some(row) = rows.next()? {
        let file_id: i64 = row.get(0)?;

        if current_file_id != Some(file_id) {
            // Flush previous file
            if let (Some(prev_fid), Some(slim)) = (current_file_id, current_slim.take()) {
                let tags = std::mem::take(&mut current_tags);
                flush(&mut upsert_stmt, prev_fid, slim, tags)?;
            }

            current_file_id = Some(file_id);
            current_slim = Some(FileMetadataSlim {
                file_id,
                entity_id: file_id,
                kind: "single".to_string(),
                member_count: None,
                hash: row.get(1)?,
                thumbnail_hash: row.get(1)?,
                name: row.get(2)?,
                mime: row.get(3)?,
                width: row.get(4)?,
                height: row.get(5)?,
                size: row.get(6)?,
                status: row.get::<_, i64>(7)? as u8,
                rating: row.get(8)?,
                imported_at: row.get(9)?,
                dominant_color_hex: row.get(10)?,
                duration_ms: row.get(11)?,
                num_frames: row.get(12)?,
                has_audio: row.get::<_, i64>(13)? != 0,
                view_count: row.get(14)?,
                position_rank: None,
                date_created: None,
                date_modified: None,
            });
        }

        // Collect tag if present (LEFT JOIN may produce NULL tag_id)
        let tag_id: Option<i64> = row.get(15)?;
        if let Some(tid) = tag_id {
            current_tags.push(FileTagInfo {
                tag_id: tid,
                namespace: row.get(16)?,
                subtag: row.get(17)?,
                display_ns: row.get(18)?,
                display_st: row.get(19)?,
                source: row.get(20)?,
            });
        }
    }

    if let (Some(prev_fid), Some(slim)) = (current_file_id, current_slim.take()) {
        flush(&mut upsert_stmt, prev_fid, slim, current_tags)?;
    }

    Ok(())
}
