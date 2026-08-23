//! DB-backed query-result targets. Rich grid rows never cross into Rust.

use crate::db::types::{
    mask_to_db_bits, EntityChange, EntityViewQuery, FolderMembershipChange, MediaEntityPatch,
    StatusChange, TagChange,
};
use rusqlite::{params, Connection, OptionalExtension, ToSql};

pub fn populate_bulk_target(
    conn: &Connection,
    query: &EntityViewQuery,
    exclusions: &[String],
) -> rusqlite::Result<i64> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS _bulk_target;
         DROP TABLE IF EXISTS _bulk_exclusion;
         CREATE TEMP TABLE _bulk_target (entity_id INTEGER PRIMARY KEY);
         CREATE TEMP TABLE _bulk_exclusion (entity_hash TEXT PRIMARY KEY);",
    )?;
    if !exclusions.is_empty() {
        let mut insert =
            conn.prepare("INSERT OR IGNORE INTO _bulk_exclusion(entity_hash) VALUES (?1)")?;
        for hash in exclusions {
            insert.execute([hash])?;
        }
    }

    let filter = crate::db::query::grid::build_entity_filter(query);
    let sql = format!(
        "INSERT OR IGNORE INTO _bulk_target(entity_id)
         SELECT me.entity_id FROM media_entity me
         JOIN media_file mf ON mf.file_id = me.file_id
         LEFT JOIN media_view mv ON mv.entity_id = me.entity_id
         WHERE {} AND NOT EXISTS (
             SELECT 1 FROM _bulk_exclusion excluded WHERE excluded.entity_hash = me.entity_hash
         )",
        filter.where_clause,
    );
    let params: Vec<&dyn ToSql> = filter
        .params
        .iter()
        .map(|value| value as &dyn ToSql)
        .collect();
    conn.execute(&sql, params.as_slice())?;
    conn.query_row("SELECT COUNT(*) FROM _bulk_target", [], |row| row.get(0))
}

fn target_entities(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    conn.prepare(
        "SELECT me.entity_id, me.entity_hash
         FROM _bulk_target bt
         JOIN media_entity me ON me.entity_id = bt.entity_id
         ORDER BY me.entity_id",
    )?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect()
}

pub fn collect_target_hashes(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    target_entities(conn).map(|entities| entities.into_iter().map(|(_, hash)| hash).collect())
}

pub fn collect_bulk_ids(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    conn.prepare("SELECT entity_id FROM _bulk_target")?
        .query_map([], |row| row.get(0))?
        .collect()
}

fn entity_change(entities: &[(i64, String)]) -> EntityChange {
    EntityChange {
        entity_ids: entities.iter().map(|(id, _)| *id).collect(),
        entity_hashes: entities.iter().map(|(_, hash)| hash.clone()).collect(),
        freed_file_hashes: Vec::new(),
    }
}

pub fn patch_target(
    conn: &Connection,
    patch: &MediaEntityPatch,
    now: &str,
) -> rusqlite::Result<EntityChange> {
    let entities = target_entities(conn)?;
    let source_urls = patch
        .source_urls
        .as_ref()
        .map(|urls| serde_json::to_string(urls).unwrap_or_default());
    conn.execute(
        "UPDATE media_entity SET
             name = CASE WHEN ?1 THEN ?2 ELSE name END,
             rating = CASE WHEN ?3 THEN ?4 ELSE rating END,
             notes = CASE WHEN ?5 THEN ?6 ELSE notes END,
             source_urls_json = CASE WHEN ?7 THEN ?8 ELSE source_urls_json END,
             date_modified = CASE WHEN ?1 OR ?3 OR ?5 OR ?7 THEN ?9 ELSE date_modified END
         WHERE entity_id IN (SELECT entity_id FROM _bulk_target)",
        params![
            patch.name.is_some(),
            patch.name,
            patch.rating.is_some(),
            patch.rating,
            patch.notes.is_some(),
            patch.notes.as_ref().and_then(|value| value.as_deref()),
            patch.source_urls.is_some(),
            source_urls,
            now,
        ],
    )?;
    Ok(entity_change(&entities))
}

pub fn set_target_status(
    conn: &Connection,
    status: i64,
    now: &str,
) -> rusqlite::Result<StatusChange> {
    let entities = target_entities(conn)?;
    conn.execute(
        "UPDATE media_entity SET status = ?1, date_modified = ?2
         WHERE entity_id IN (SELECT entity_id FROM _bulk_target)",
        params![status, now],
    )?;
    Ok(StatusChange {
        entity_ids: entities.iter().map(|(id, _)| *id).collect(),
        entity_hashes: entities.into_iter().map(|(_, hash)| hash).collect(),
        new_status: status,
    })
}

pub fn add_tags_to_target(
    conn: &Connection,
    tags: &[String],
    provenance_mask: u64,
) -> rusqlite::Result<TagChange> {
    let entities = target_entities(conn)?;
    let mut change = TagChange {
        entity_ids: entities.iter().map(|(id, _)| *id).collect(),
        ..TagChange::default()
    };
    for tag in tags {
        let tag_id = super::tags::ensure_tag(conn, tag)?;
        conn.execute(
            "INSERT INTO entity_tag(entity_id, tag_id, provenance_mask, source)
             SELECT entity_id, ?1, ?2, 'local' FROM _bulk_target WHERE 1
             ON CONFLICT(entity_id, tag_id, source)
             DO UPDATE SET provenance_mask = entity_tag.provenance_mask | excluded.provenance_mask",
            params![tag_id, mask_to_db_bits(provenance_mask)],
        )?;
        change.tag_ids.push(tag_id);
        change.tags_added.push(tag.clone());
    }
    Ok(change)
}

pub fn remove_tags_from_target(conn: &Connection, tags: &[String]) -> rusqlite::Result<TagChange> {
    let entities = target_entities(conn)?;
    let mut change = TagChange {
        entity_ids: entities.iter().map(|(id, _)| *id).collect(),
        ..TagChange::default()
    };
    for tag in tags {
        let Some((namespace, subtag)) = crate::tags::normalize::parse_tag(tag) else {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Invalid tag: {tag}"
            )));
        };
        let tag_id = conn
            .query_row(
                "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                params![namespace, subtag],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(tag_id) = tag_id else { continue };
        conn.execute(
            "DELETE FROM entity_tag
             WHERE tag_id = ?1 AND entity_id IN (SELECT entity_id FROM _bulk_target)",
            [tag_id],
        )?;
        change.tag_ids.push(tag_id);
        change.tags_removed.push(tag.clone());
    }
    Ok(change)
}

pub fn add_folder_members_to_target(
    conn: &Connection,
    folder_id: i64,
) -> rusqlite::Result<FolderMembershipChange> {
    let entities = target_entities(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO folder_member(folder_id, entity_id)
         SELECT ?1, entity_id FROM _bulk_target",
        [folder_id],
    )?;
    Ok(FolderMembershipChange {
        folder_id,
        entity_ids: entities.into_iter().map(|(id, _)| id).collect(),
    })
}

pub fn remove_folder_members_from_target(
    conn: &Connection,
    folder_id: i64,
) -> rusqlite::Result<FolderMembershipChange> {
    let entities = target_entities(conn)?;
    conn.execute(
        "DELETE FROM folder_member
         WHERE folder_id = ?1 AND entity_id IN (SELECT entity_id FROM _bulk_target)",
        [folder_id],
    )?;
    Ok(FolderMembershipChange {
        folder_id,
        entity_ids: entities.into_iter().map(|(id, _)| id).collect(),
    })
}
