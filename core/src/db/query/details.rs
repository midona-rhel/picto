//! Entity detail queries — returns EntityDetails for inspector/detail panel.
//! Fully independent from EntityGridItem.

use rusqlite::{Connection, OptionalExtension};

use crate::db::types::{mask_from_db_bits, EntityDetails, EntityKind, FolderInfo, TagInfo};

use super::collections;

/// Get full details for an entity by hash.
pub fn get_entity_details(
    conn: &Connection,
    entity_hash: &str,
) -> rusqlite::Result<Option<EntityDetails>> {
    let row = conn
        .query_row(
            "SELECT
                me.entity_id,
                me.entity_hash,
                me.entity_kind,
                me.name,
                me.status,
                me.rating,
                me.notes,
                me.source_urls_json,
                me.date_created,
                me.date_added,
                me.date_modified,
                me.member_count,
                me.total_size_bytes,
                COALESCE(mf.mime_type, 'application/x-collection') AS mime_type,
                COALESCE(mf.size_bytes, me.total_size_bytes, 0) AS size_bytes,
                COALESCE(mf.pixel_width, pmf.pixel_width) AS pixel_width,
                COALESCE(mf.pixel_height, pmf.pixel_height) AS pixel_height,
                COALESCE(mf.duration_ms, pmf.duration_ms) AS duration_ms,
                COALESCE(mf.frame_count, pmf.frame_count) AS frame_count,
                COALESCE(mf.has_audio, pmf.has_audio, 0) AS has_audio,
                COALESCE(mf.dominant_color_hex, pmf.dominant_color_hex) AS dominant_color_hex,
                mf.perceptual_hash,
                COALESCE(mf.file_hash, pmf.file_hash, me.entity_hash) AS thumbnail_hash
             FROM media_entity me
             LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
             LEFT JOIN media_file mf ON mf.file_id = sme.file_id
             LEFT JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
             LEFT JOIN single_media_entity psme ON psme.entity_id = pm.entity_id
             LEFT JOIN media_file pmf ON pmf.file_id = psme.file_id
             WHERE me.entity_hash = ?1",
            [entity_hash],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,             // entity_id
                    row.get::<_, String>(1)?,          // entity_hash
                    row.get::<_, String>(2)?,          // entity_kind
                    row.get::<_, Option<String>>(3)?,  // name
                    row.get::<_, i64>(4)?,             // status
                    row.get::<_, Option<i64>>(5)?,     // rating
                    row.get::<_, Option<String>>(6)?,  // notes
                    row.get::<_, Option<String>>(7)?,  // source_urls_json
                    row.get::<_, String>(8)?,          // date_created
                    row.get::<_, String>(9)?,          // date_added
                    row.get::<_, String>(10)?,         // date_modified
                    row.get::<_, Option<i64>>(11)?,    // member_count
                    row.get::<_, Option<i64>>(12)?,    // total_size_bytes
                    row.get::<_, String>(13)?,         // mime_type
                    row.get::<_, i64>(14)?,            // size_bytes
                    row.get::<_, Option<i64>>(15)?,    // pixel_width
                    row.get::<_, Option<i64>>(16)?,    // pixel_height
                    row.get::<_, Option<i64>>(17)?,    // duration_ms
                    row.get::<_, Option<i64>>(18)?,    // frame_count
                    row.get::<_, i64>(19)?,            // has_audio
                    row.get::<_, Option<String>>(20)?, // dominant_color_hex
                    row.get::<_, Option<String>>(21)?, // perceptual_hash
                    row.get::<_, String>(22)?,         // thumbnail_hash
                ))
            },
        )
        .optional()?;

    let Some((
        entity_id,
        entity_hash,
        entity_kind_str,
        name,
        status,
        rating,
        notes,
        source_urls_json,
        date_created,
        date_added,
        date_modified,
        member_count,
        total_size_bytes,
        mime_type,
        size_bytes,
        pixel_width,
        pixel_height,
        duration_ms,
        frame_count,
        has_audio,
        dominant_color_hex,
        perceptual_hash,
        thumbnail_hash,
    )) = row
    else {
        return Ok(None);
    };

    let entity_kind = EntityKind::from_str(&entity_kind_str).unwrap_or(EntityKind::Single);
    let (rating, notes, source_urls, tags) = if entity_kind == EntityKind::Collection {
        let content = collections::get_collection_content_metadata(conn, entity_id)?;
        (
            content.rating,
            content.notes,
            (!content.source_urls.is_empty()).then_some(content.source_urls),
            content.tags,
        )
    } else {
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
        (rating, notes, source_urls, tags)
    };

    // Fetch folders
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
        thumbnail_hash,
        entity_kind,
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
        member_count,
        total_size_bytes,
    }))
}
