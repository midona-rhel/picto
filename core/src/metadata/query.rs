//! File metadata orchestration — resolves full file details including tags,
//! dominant colors, and display-resolved tag info.

use crate::sqlite::SqliteDatabase;
use crate::tags::db::FileTagInfo;
use crate::tags::normalize;
use crate::types::{
    tag_display_key, DominantColorDto, EntityAllMetadata, EntityDetails, ResolvedTagInfo, TagInfo,
};

pub fn file_tag_to_resolved_info(t: FileTagInfo) -> ResolvedTagInfo {
    let raw_tag = normalize::combine_tag(&t.namespace, &t.subtag);
    let disp_ns = t.display_ns.as_deref().unwrap_or(&t.namespace);
    let disp_st = t.display_st.as_deref().unwrap_or(&t.subtag);
    let display_tag = tag_display_key(disp_ns, disp_st);
    let read_only = t.source != "local";
    ResolvedTagInfo {
        raw_tag,
        display_tag,
        namespace: t.display_ns.unwrap_or(t.namespace),
        subtag: t.display_st.unwrap_or(t.subtag),
        source: t.source,
        read_only,
    }
}

/// Query implied (parent) tags for a file by entity_id.
fn get_implied_tags(conn: &rusqlite::Connection, entity_id: i64) -> rusqlite::Result<Vec<TagInfo>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag,
                COALESCE(td.display_ns, t.namespace),
                COALESCE(td.display_st, t.subtag)
         FROM entity_tag_implied eti
         JOIN tag t ON t.tag_id = eti.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE eti.entity_id = ?1",
    )?;
    let rows = stmt.query_map([entity_id], |row| {
        let tag_id: i64 = row.get(0)?;
        let ns: String = row.get(1)?;
        let st: String = row.get(2)?;
        let disp_ns: String = row.get(3)?;
        let disp_st: String = row.get(4)?;
        Ok(TagInfo {
            tag_id,
            namespace: ns,
            subtag: st,
            display: tag_display_key(&disp_ns, &disp_st),
            file_count: 0,
            read_only: true,
        })
    })?;
    rows.collect()
}

pub struct MetadataQuery;

impl MetadataQuery {
    pub async fn get_entity_all_metadata(
        db: &SqliteDatabase,
        hash: String,
    ) -> Result<EntityAllMetadata, String> {
        let entity = match db.get_entity_details_by_hash(&hash).await? {
            Some(e) => e,
            None => {
                // Collection hashes don't have file rows — not an error.
                tracing::debug!(hash = %hash, "get_entity_all_metadata: no file entity (may be a collection)");
                return Err(format!("Entity not found: {}", hash));
            }
        };
        let local_tags = db.get_entity_tags(&hash).await.unwrap_or_default();

        let tags: Vec<ResolvedTagInfo> = local_tags
            .into_iter()
            .map(file_tag_to_resolved_info)
            .collect();

        let file_id = match db.resolve_hash(&hash).await {
            Ok(id) => id,
            Err(_) => {
                tracing::debug!(hash = %hash, "get_entity_all_metadata: hash not resolvable");
                return Err(format!("Entity not found: {}", hash));
            }
        };

        let fid_for_colors = file_id;
        let colors = db
            .with_read_conn(move |conn| crate::sqlite::files::get_file_colors(conn, fid_for_colors))
            .await?;

        let parent_tags: Vec<TagInfo> = db
            .with_read_conn(move |conn| get_implied_tags(conn, file_id))
            .await?;

        // Fetch entity timestamps from media_entity (not on FileRecord)
        let fid_for_ts = file_id;
        let (created_at, updated_at): (Option<String>, Option<String>) = db
            .with_read_conn(move |conn| {
                use rusqlite::OptionalExtension;
                conn.query_row(
                    "SELECT created_at, updated_at FROM media_entity WHERE entity_id = ?1",
                    [fid_for_ts],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map(|opt| opt.unwrap_or((None, None)))
            })
            .await?;

        let mut entity_info: EntityDetails = entity;
        entity_info.dominant_colors = Some(
            colors
                .into_iter()
                .map(|(hex, l, a, b)| DominantColorDto { hex, l, a, b })
                .collect(),
        );
        entity_info.created_at = created_at;
        entity_info.updated_at = updated_at;

        Ok(EntityAllMetadata {
            entity: entity_info,
            tags,
            parent_tags,
        })
    }
}
