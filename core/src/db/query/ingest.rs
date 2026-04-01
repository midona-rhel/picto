use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct ExistingImportTarget {
    pub file_id: i64,
    pub file_hash: String,
    pub entity_id: i64,
    pub entity_hash: String,
    pub name: Option<String>,
    pub status: i64,
    pub notes: Option<String>,
    pub source_urls_json: Option<String>,
    pub date_created: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub frame_count: Option<i64>,
    pub perceptual_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DerivativeTarget {
    pub file_id: i64,
    pub entity_hash: String,
    pub file_hash: String,
    pub mime_type: String,
    pub duration_ms: Option<i64>,
    pub frame_count: Option<i64>,
    pub dominant_color_hex: Option<String>,
    pub perceptual_hash: Option<String>,
}

pub fn get_existing_import_target_by_file_hash(
    conn: &Connection,
    file_hash: &str,
) -> rusqlite::Result<Option<ExistingImportTarget>> {
    get_existing_import_target_by_clause(conn, "mf.file_hash = ?1", [file_hash])
}

pub fn get_existing_import_target_by_entity_hash(
    conn: &Connection,
    entity_hash: &str,
) -> rusqlite::Result<Option<ExistingImportTarget>> {
    get_existing_import_target_by_clause(conn, "me.entity_hash = ?1", [entity_hash])
}

fn get_existing_import_target_by_clause<P>(
    conn: &Connection,
    clause: &str,
    params: P,
) -> rusqlite::Result<Option<ExistingImportTarget>>
where
    P: rusqlite::Params,
{
    conn.query_row(
        &format!(
            "SELECT
             mf.file_id,
             mf.file_hash,
             me.entity_id,
             me.entity_hash,
             me.name,
             me.status,
             me.notes,
             me.source_urls_json,
             me.date_created,
             mf.mime_type,
             mf.size_bytes,
             mf.pixel_width,
             mf.pixel_height,
             mf.frame_count,
             mf.perceptual_hash
         FROM media_file mf
         JOIN single_media_entity sme ON sme.file_id = mf.file_id
         JOIN media_entity me ON me.entity_id = sme.entity_id
         WHERE {clause}"
        ),
        params,
        |row| {
            Ok(ExistingImportTarget {
                file_id: row.get(0)?,
                file_hash: row.get(1)?,
                entity_id: row.get(2)?,
                entity_hash: row.get(3)?,
                name: row.get(4)?,
                status: row.get(5)?,
                notes: row.get(6)?,
                source_urls_json: row.get(7)?,
                date_created: row.get(8)?,
                mime_type: row.get(9)?,
                size_bytes: row.get(10)?,
                pixel_width: row.get(11)?,
                pixel_height: row.get(12)?,
                frame_count: row.get(13)?,
                perceptual_hash: row.get(14)?,
            })
        },
    )
    .optional()
}

pub fn get_derivative_target_by_entity_hash(
    conn: &Connection,
    entity_hash: &str,
) -> rusqlite::Result<Option<DerivativeTarget>> {
    conn.query_row(
        "SELECT
             mf.file_id,
             me.entity_hash,
             mf.file_hash,
             mf.mime_type,
             mf.duration_ms,
             mf.frame_count,
             mf.dominant_color_hex,
             mf.perceptual_hash
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE me.entity_hash = ?1",
        [entity_hash],
        |row| {
            Ok(DerivativeTarget {
                file_id: row.get(0)?,
                entity_hash: row.get(1)?,
                file_hash: row.get(2)?,
                mime_type: row.get(3)?,
                duration_ms: row.get(4)?,
                frame_count: row.get(5)?,
                dominant_color_hex: row.get(6)?,
                perceptual_hash: row.get(7)?,
            })
        },
    )
    .optional()
}

pub fn find_child_folder_id(
    conn: &Connection,
    parent_id: i64,
    name: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT folder_id
         FROM folder
         WHERE parent_id = ?1 AND name = ?2",
        params![parent_id, name],
        |row| row.get(0),
    )
    .optional()
}
