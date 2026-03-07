//! File metadata orchestration — resolves full file details including tags,
//! dominant colors, PTR overlay data, and display-resolved tag info.
//!
//! Assembles `FileAllMetadata` by joining data from the library DB, PTR
//! overlay, and tag sibling/parent resolution.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::sqlite::tags::FileTagInfo;
use crate::sqlite::SqliteDatabase;
use crate::sqlite_ptr::PtrSqliteDatabase;
use crate::tags;
use crate::types::{
    tag_display_key, DominantColorDto, FileAllMetadata, FileInfo, ResolvedTagInfo, TagInfo,
};

fn file_tag_to_resolved_info(t: FileTagInfo) -> ResolvedTagInfo {
    let raw_tag = tags::combine_tag(&t.namespace, &t.subtag);
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

pub struct MetadataController;

impl MetadataController {
    pub async fn get_file(db: &SqliteDatabase, hash: String) -> Result<Option<FileInfo>, String> {
        let file = db.get_file_by_hash(&hash).await?;
        Ok(file.map(FileInfo::from))
    }

    pub async fn get_file_all_metadata(
        db: &SqliteDatabase,
        ptr_db: &PtrSqliteDatabase,
        hash: String,
    ) -> Result<FileAllMetadata, String> {
        let file = db
            .get_file_by_hash(&hash)
            .await?
            .ok_or_else(|| format!("File not found: {}", hash))?;
        let local_tags = db.get_entity_tags(&hash).await?;
        // Use overlay (pre-compiled) path — no live graph joins
        let ptr_tags = ptr_db
            .get_overlay(&hash)
            .await
            .unwrap_or(None)
            .unwrap_or_default();

        let mut seen = HashSet::new();
        let mut tags: Vec<ResolvedTagInfo> = local_tags
            .into_iter()
            .map(|t| {
                let info = file_tag_to_resolved_info(t);
                seen.insert(info.display_tag.clone());
                info
            })
            .collect();

        for pt in ptr_tags {
            let display = tag_display_key(&pt.display_ns, &pt.display_st);
            if !seen.contains(&display) {
                seen.insert(display.clone());
                tags.push(ResolvedTagInfo {
                    raw_tag: tags::combine_tag(&pt.raw_ns, &pt.raw_st),
                    display_tag: display,
                    namespace: pt.display_ns,
                    subtag: pt.display_st,
                    source: "ptr".to_string(),
                    read_only: true,
                });
            }
        }

        let file_id = db.resolve_hash(&hash).await?;

        // Query dominant colors from file_color table
        let fid_for_colors = file_id;
        let colors = db
            .with_read_conn(move |conn| crate::sqlite::files::get_file_colors(conn, fid_for_colors))
            .await?;

        let parent_tags: Vec<TagInfo> = db
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT t.tag_id, t.namespace, t.subtag,
                            COALESCE(td.display_ns, t.namespace),
                            COALESCE(td.display_st, t.subtag)
                     FROM entity_tag_implied eti
                     JOIN tag t ON t.tag_id = eti.tag_id
                     LEFT JOIN tag_display td ON td.tag_id = t.tag_id
                     WHERE eti.entity_id = ?1",
                )?;
                let rows = stmt.query_map([file_id], |row| {
                    let tag_id: i64 = row.get(0)?;
                    let ns: String = row.get(1)?;
                    let st: String = row.get(2)?;
                    let disp_ns: String = row.get(3)?;
                    let disp_st: String = row.get(4)?;
                    Ok(TagInfo {
                        tag_id,
                        namespace: ns,
                        subtag: st.clone(),
                        display: tag_display_key(&disp_ns, &disp_st),
                        file_count: 0,
                        read_only: true,
                    })
                })?;
                rows.collect()
            })
            .await?;

        let mut file_info = FileInfo::from(file);
        file_info.dominant_colors = Some(
            colors
                .into_iter()
                .map(|(hex, l, a, b)| DominantColorDto { hex, l, a, b })
                .collect(),
        );

        Ok(FileAllMetadata {
            file: file_info,
            tags,
            parent_tags,
        })
    }

    pub async fn get_file_tags_display(
        db: &SqliteDatabase,
        ptr_db: &PtrSqliteDatabase,
        hash: String,
    ) -> Result<Vec<ResolvedTagInfo>, String> {
        let (local_result, ptr_result) =
            tokio::join!(db.get_entity_tags(&hash), ptr_db.get_overlay(&hash),);
        let local_tags = local_result?;
        let ptr_tags = ptr_result.unwrap_or(None).unwrap_or_default();

        let mut seen = HashSet::new();
        let mut result: Vec<ResolvedTagInfo> = local_tags
            .into_iter()
            .map(|t| {
                let info = file_tag_to_resolved_info(t);
                seen.insert(info.display_tag.clone());
                info
            })
            .collect();

        for pt in ptr_tags {
            let display = tag_display_key(&pt.display_ns, &pt.display_st);
            if !seen.contains(&display) {
                seen.insert(display.clone());
                result.push(ResolvedTagInfo {
                    raw_tag: tags::combine_tag(&pt.raw_ns, &pt.raw_st),
                    display_tag: display,
                    namespace: pt.display_ns,
                    subtag: pt.display_st,
                    source: "ptr".to_string(),
                    read_only: true,
                });
            }
        }

        Ok(result)
    }

    pub async fn get_file_parents(
        db: &SqliteDatabase,
        hash: String,
    ) -> Result<Vec<TagInfo>, String> {
        let file_id = db.resolve_hash(&hash).await?;
        db.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT t.tag_id, t.namespace, t.subtag,
                        COALESCE(td.display_ns, t.namespace),
                        COALESCE(td.display_st, t.subtag)
                 FROM entity_tag_implied eti
                 JOIN tag t ON t.tag_id = eti.tag_id
                 LEFT JOIN tag_display td ON td.tag_id = t.tag_id
                 WHERE eti.entity_id = ?1",
            )?;
            let rows = stmt.query_map([file_id], |row| {
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
        })
        .await
    }

    pub async fn update_rating(
        db: &SqliteDatabase,
        hash: String,
        rating: Option<i64>,
    ) -> Result<(), String> {
        db.update_rating(&hash, rating).await
    }

    pub async fn set_file_name(
        db: &SqliteDatabase,
        hash: String,
        name: Option<String>,
    ) -> Result<(), String> {
        db.set_file_name(&hash, name.as_deref()).await
    }

    pub async fn get_file_notes(
        db: &SqliteDatabase,
        hash: String,
    ) -> Result<Option<HashMap<String, String>>, String> {
        let file = db.get_file_by_hash(&hash).await?;
        match file {
            Some(f) => Ok(f
                .notes
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())),
            None => Ok(None),
        }
    }

    pub async fn set_file_notes(
        db: &SqliteDatabase,
        hash: String,
        notes: HashMap<String, String>,
    ) -> Result<(), String> {
        let json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        db.set_notes(&hash, Some(&json)).await
    }

    pub async fn increment_view_count(db: &SqliteDatabase, hash: String) -> Result<(), String> {
        db.increment_view_count(&hash).await
    }
}
