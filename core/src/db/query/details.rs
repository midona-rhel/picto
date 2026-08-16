//! Entity detail queries for the inspector/detail panel.

use rusqlite::{Connection, OptionalExtension};

use crate::db::types::{mask_from_db_bits, EntityDetails, FolderInfo, TagInfo};

/// Get full details for one media entity by its stable hash.
pub fn get_entity_details(
    conn: &Connection,
    entity_hash: &str,
) -> rusqlite::Result<Option<EntityDetails>> {
    let row = conn
        .query_row(
            "SELECT
                me.entity_id,
                me.entity_hash,
                me.name,
                me.status,
                me.rating,
                me.notes,
                me.source_urls_json,
                me.date_created,
                me.date_added,
                me.date_modified,
                mf.mime_type,
                mf.size_bytes,
                mf.pixel_width,
                mf.pixel_height,
                mf.duration_ms,
                mf.frame_count,
                COALESCE(mf.has_audio, 0),
                mf.dominant_color_hex,
                mf.perceptual_hash,
                mf.file_hash
             FROM media_entity me
             JOIN media_file mf ON mf.file_id = me.file_id
             WHERE me.entity_hash = ?1",
            [entity_hash],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, Option<i64>>(15)?,
                    row.get::<_, i64>(16)?,
                    row.get::<_, Option<String>>(17)?,
                    row.get::<_, Option<String>>(18)?,
                    row.get::<_, String>(19)?,
                ))
            },
        )
        .optional()?;

    let Some((
        entity_id,
        entity_hash,
        name,
        status,
        rating,
        notes,
        source_urls_json,
        date_created,
        date_added,
        date_modified,
        mime_type,
        size_bytes,
        pixel_width,
        pixel_height,
        duration_ms,
        frame_count,
        has_audio,
        dominant_color_hex,
        perceptual_hash,
        _file_hash,
    )) = row
    else {
        return Ok(None);
    };

    let source_urls =
        source_urls_json.and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok());
    let mut tag_stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, et.provenance_mask, et.source
         FROM entity_tag et
         JOIN tag t ON t.tag_id = et.tag_id
         WHERE et.entity_id = ?1
         ORDER BY t.namespace, t.subtag",
    )?;
    let tags: Vec<TagInfo> = tag_stmt
        .query_map([entity_id], |row| {
            Ok(TagInfo {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
                provenance_mask: mask_from_db_bits(row.get::<_, Option<i64>>(3)?.unwrap_or(0)),
                source: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut folder_stmt = conn.prepare(
        "SELECT f.folder_id, f.name
         FROM folder_member fm
         JOIN folder f ON f.folder_id = fm.folder_id
         WHERE fm.entity_id = ?1
         ORDER BY f.name",
    )?;
    let folders: Vec<FolderInfo> = folder_stmt
        .query_map([entity_id], |row| {
            Ok(FolderInfo {
                folder_id: row.get(0)?,
                name: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(EntityDetails {
        entity_hash,
        name,
        mime_type,
        size_bytes,
        pixel_width,
        pixel_height,
        duration_ms,
        frame_count,
        has_audio: has_audio != 0,
        status,
        rating,
        notes,
        source_urls,
        date_created,
        date_added,
        date_modified,
        dominant_color_hex,
        dominant_colors: None,
        perceptual_hash,
        tags,
        folders,
    }))
}
