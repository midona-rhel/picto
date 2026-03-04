//! File CRUD operations.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::bitmaps::BitmapKey;
use super::compilers::CompilerEvent;
use super::SqliteDatabase;

/// Default visibility clause: active (1) only, excludes inbox (0) and trash (2).
pub const DEFAULT_VISIBILITY_CLAUSE: &str = "status = 1";

/// Slim DTO for grid display — no filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadataSlim {
    #[serde(skip)]
    pub file_id: i64,
    #[serde(default)]
    pub entity_id: i64,
    #[serde(default)]
    pub is_collection: bool,
    #[serde(default)]
    pub collection_item_count: Option<i64>,
    pub hash: String,
    pub name: Option<String>,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub size: i64,
    pub status: u8,
    pub rating: Option<i64>,
    pub blurhash: Option<String>,
    pub imported_at: String,
    pub dominant_color_hex: Option<String>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub view_count: i64,
    /// Only populated when sorting by folder position_rank.
    #[serde(skip)]
    pub position_rank: Option<i64>,
}

/// Full file record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: i64,
    pub hash: String,
    pub name: Option<String>,
    pub size: i64,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub blurhash: Option<String>,
    pub status: i64,
    pub rating: Option<i64>,
    pub view_count: i64,
    pub phash: Option<String>,
    pub imported_at: String,
    pub notes: Option<String>,
    pub source_urls_json: Option<String>,
    pub dominant_color_hex: Option<String>,
}

/// Input for inserting a new file.
pub struct NewFile {
    pub hash: String,
    pub name: Option<String>,
    pub size: i64,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub blurhash: Option<String>,
    pub status: i64,
    pub imported_at: String,
    pub notes: Option<String>,
    pub source_urls_json: Option<String>,
    pub dominant_color_hex: Option<String>,
    pub dominant_palette_blob: Option<Vec<u8>>,
}

pub fn insert_file(conn: &Connection, f: &NewFile) -> rusqlite::Result<i64> {
    // Keep the invariant `single.entity_id == file_id` intact by ensuring file_id
    // never collides with an existing collection entity_id.
    let mut file_id: i64 = conn.query_row(
        "SELECT COALESCE(MAX(file_id), 0) + 1 FROM file",
        [],
        |row| row.get(0),
    )?;
    loop {
        let collides_with_collection: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM media_entity
                WHERE entity_id = ?1 AND kind = 'collection'
            )",
            [file_id],
            |row| row.get(0),
        )?;
        if !collides_with_collection {
            break;
        }
        file_id += 1;
    }

    conn.execute(
        "INSERT INTO file (file_id, hash, name, size, mime, width, height, duration_ms, num_frames,
         has_audio, blurhash, status, imported_at, notes, source_urls_json,
         dominant_color_hex, dominant_palette_blob)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            file_id,
            f.hash,
            f.name,
            f.size,
            f.mime,
            f.width,
            f.height,
            f.duration_ms,
            f.num_frames,
            f.has_audio as i64,
            f.blurhash,
            f.status,
            f.imported_at,
            f.notes,
            f.source_urls_json,
            f.dominant_color_hex,
            f.dominant_palette_blob,
        ],
    )?;

    let _ = conn.execute(
        "INSERT INTO file_fts(rowid, name, notes, source_urls) VALUES (?1, ?2, ?3, ?4)",
        params![file_id, f.name, f.notes, f.source_urls_json],
    );

    // Keep single-file media entity tables in sync for entity-aware reads.
    conn.execute(
        "INSERT OR IGNORE INTO media_entity
            (entity_id, kind, name, description, status, rating, created_at, updated_at)
         VALUES (?1, 'single', ?2, '', ?3, NULL, ?4, ?4)",
        params![file_id, f.name, f.status, f.imported_at],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (?1, ?2)",
        params![file_id, file_id],
    )?;

    Ok(file_id)
}

pub fn rebuild_file_fts(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO file_fts(file_fts) VALUES('rebuild')", [])?;
    Ok(())
}

pub fn get_file_by_hash(conn: &Connection, hash: &str) -> rusqlite::Result<Option<FileRecord>> {
    conn.query_row(
        "SELECT file_id, hash, name, size, mime, width, height, duration_ms, num_frames,
                has_audio, blurhash, status, rating, view_count, phash, imported_at,
                notes, source_urls_json, dominant_color_hex
         FROM file WHERE hash = ?1",
        [hash],
        |row| {
            Ok(FileRecord {
                file_id: row.get(0)?,
                hash: row.get(1)?,
                name: row.get(2)?,
                size: row.get(3)?,
                mime: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
                duration_ms: row.get(7)?,
                num_frames: row.get(8)?,
                has_audio: row.get::<_, i64>(9)? != 0,
                blurhash: row.get(10)?,
                status: row.get(11)?,
                rating: row.get(12)?,
                view_count: row.get(13)?,
                phash: row.get(14)?,
                imported_at: row.get(15)?,
                notes: row.get(16)?,
                source_urls_json: row.get(17)?,
                dominant_color_hex: row.get(18)?,
            })
        },
    )
    .optional()
}

pub fn get_file_by_id(conn: &Connection, file_id: i64) -> rusqlite::Result<Option<FileRecord>> {
    conn.query_row(
        "SELECT file_id, hash, name, size, mime, width, height, duration_ms, num_frames,
                has_audio, blurhash, status, rating, view_count, phash, imported_at,
                notes, source_urls_json, dominant_color_hex
         FROM file WHERE file_id = ?1",
        [file_id],
        |row| {
            Ok(FileRecord {
                file_id: row.get(0)?,
                hash: row.get(1)?,
                name: row.get(2)?,
                size: row.get(3)?,
                mime: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
                duration_ms: row.get(7)?,
                num_frames: row.get(8)?,
                has_audio: row.get::<_, i64>(9)? != 0,
                blurhash: row.get(10)?,
                status: row.get(11)?,
                rating: row.get(12)?,
                view_count: row.get(13)?,
                phash: row.get(14)?,
                imported_at: row.get(15)?,
                notes: row.get(16)?,
                source_urls_json: row.get(17)?,
                dominant_color_hex: row.get(18)?,
            })
        },
    )
    .optional()
}

pub fn file_exists(conn: &Connection, hash: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM file WHERE hash = ?1",
        [hash],
        |row| row.get(0),
    )
}

pub fn count_files(conn: &Connection, status: Option<i64>) -> rusqlite::Result<i64> {
    match status {
        Some(s) => conn.query_row("SELECT COUNT(*) FROM file WHERE status = ?1", [s], |row| {
            row.get(0)
        }),
        None => conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM file WHERE {}",
                DEFAULT_VISIBILITY_CLAUSE
            ),
            [],
            |row| row.get(0),
        ),
    }
}

pub fn update_status(conn: &Connection, file_id: i64, status: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET status = ?1 WHERE file_id = ?2",
        params![status, file_id],
    )?;
    conn.execute(
        "UPDATE media_entity
         SET status = ?1, updated_at = CURRENT_TIMESTAMP
         WHERE entity_id IN (
             SELECT entity_id FROM entity_file WHERE file_id = ?2
         )",
        params![status, file_id],
    )?;
    Ok(())
}

pub fn update_rating(conn: &Connection, file_id: i64, rating: Option<i64>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET rating = ?1 WHERE file_id = ?2",
        params![rating, file_id],
    )?;
    Ok(())
}

pub fn update_name(conn: &Connection, file_id: i64, name: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET name = ?1 WHERE file_id = ?2",
        params![name, file_id],
    )?;
    Ok(())
}

pub fn set_notes(conn: &Connection, file_id: i64, notes: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET notes = ?1 WHERE file_id = ?2",
        params![notes, file_id],
    )?;
    Ok(())
}

pub fn set_source_urls(
    conn: &Connection,
    file_id: i64,
    urls_json: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET source_urls_json = ?1 WHERE file_id = ?2",
        params![urls_json, file_id],
    )?;
    Ok(())
}

pub fn set_phash(conn: &Connection, file_id: i64, phash: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET phash = ?1 WHERE file_id = ?2",
        params![phash, file_id],
    )?;
    Ok(())
}

pub fn set_blurhash(
    conn: &Connection,
    file_id: i64,
    blurhash: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET blurhash = ?1 WHERE file_id = ?2",
        params![blurhash, file_id],
    )?;
    Ok(())
}

pub fn increment_view_count(conn: &Connection, file_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE file SET view_count = view_count + 1, last_viewed_at = datetime('now') WHERE file_id = ?1",
        [file_id],
    )?;
    Ok(())
}

/// Delete ALL files and related data (bulk wipe).
pub fn wipe_all_files(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM entity_tag_implied", [])?;
    conn.execute("DELETE FROM entity_metadata_projection", [])?;
    conn.execute("DELETE FROM file_color_rtree", [])?;
    conn.execute("DELETE FROM file_color", [])?;
    conn.execute("DELETE FROM duplicate", [])?;
    conn.execute("DELETE FROM folder_entity", [])?;
    conn.execute("DELETE FROM subscription_entity", [])?;
    conn.execute("DELETE FROM entity_tag_raw", [])?;
    conn.execute("DELETE FROM file", [])?;
    conn.execute("DELETE FROM collection_member", [])?;
    conn.execute("DELETE FROM collection_tag", [])?;
    conn.execute("DELETE FROM entity_file", [])?;
    conn.execute("DELETE FROM media_entity", [])?;
    Ok(())
}

pub fn delete_file(conn: &Connection, file_id: i64) -> rusqlite::Result<()> {
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

    let entity_id: Option<i64> = conn
        .query_row(
            "SELECT entity_id FROM entity_file WHERE file_id = ?1",
            [file_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(entity_id) = entity_id {
        conn.execute("DELETE FROM media_entity WHERE entity_id = ?1", [entity_id])?;
    }
    conn.execute("DELETE FROM file WHERE file_id = ?1", [file_id])?;
    Ok(())
}

/// Optional filter parameters for grid queries.
#[derive(Debug, Default, Clone)]
pub struct GridFilters {
    pub rating_min: Option<i64>,
    pub mime_prefixes: Option<Vec<String>>,
    pub search_text: Option<String>,
}

/// Append filter WHERE clauses to a SQL query.
/// `col_prefix` is "f." for JOINed queries or "" for direct queries.
fn append_filter_clauses(
    sql: &mut String,
    param_values: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    filters: &GridFilters,
    col_prefix: &str,
    conn: &Connection,
) {
    if let Some(rating_min) = filters.rating_min {
        sql.push_str(&format!(
            " AND {}rating >= ?{}",
            col_prefix,
            param_values.len() + 1
        ));
        param_values.push(Box::new(rating_min));
    }

    if let Some(prefixes) = &filters.mime_prefixes {
        if !prefixes.is_empty() {
            let conditions: Vec<String> = prefixes
                .iter()
                .enumerate()
                .map(|(i, _)| format!("{}mime LIKE ?{}", col_prefix, param_values.len() + 1 + i))
                .collect();
            sql.push_str(&format!(" AND ({})", conditions.join(" OR ")));
            for prefix in prefixes {
                // "image/" → "image/%", "image/gif" → "image/gif" (exact prefix match)
                let like_pattern = if prefix.ends_with('/') {
                    format!("{}%", prefix)
                } else {
                    prefix.clone()
                };
                param_values.push(Box::new(like_pattern));
            }
        }
    }

    if let Some(text) = &filters.search_text {
        if !text.is_empty() {
            // FTS5 search: get matching file_ids from file_fts
            let fts_query = format!("\"{}\"", text.replace('"', "\"\""));
            let fts_ids: Vec<i64> = conn
                .prepare("SELECT rowid FROM file_fts WHERE file_fts MATCH ?1")
                .and_then(|mut stmt| stmt.query_map([&fts_query], |row| row.get(0))?.collect())
                .unwrap_or_default();

            if fts_ids.is_empty() {
                // No FTS matches — add impossible condition
                sql.push_str(" AND 0=1");
            } else {
                // Also match name LIKE for simple substring fallback
                let placeholders: Vec<String> = fts_ids
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", param_values.len() + 1 + i))
                    .collect();
                sql.push_str(&format!(
                    " AND ({}file_id IN ({}) OR {}name LIKE ?{})",
                    col_prefix,
                    placeholders.join(","),
                    col_prefix,
                    param_values.len() + 1 + fts_ids.len()
                ));
                for id in fts_ids {
                    param_values.push(Box::new(id));
                }
                param_values.push(Box::new(format!("%{}%", text)));
            }
        }
    }
}

/// Map a sort field name to the SQL expression for entity-aware queries.
/// Returns a COALESCE expression that works for both singles (f.* populated) and collections (f.* NULL).
fn entity_sort_expr(sort_field: &str) -> &'static str {
    match sort_field {
        "imported_at" => {
            "CASE WHEN me.kind = 'collection'
                  THEN COALESCE(me.created_at, '')
                  ELSE COALESCE(f.imported_at, me.created_at, '')
             END"
        }
        "size" => {
            "CASE WHEN me.kind = 'collection'
                  THEN COALESCE(me.cached_total_size_bytes, 0)
                  ELSE COALESCE(f.size, me.cached_total_size_bytes, 0)
             END"
        }
        "rating" => {
            "CASE WHEN me.kind = 'collection'
                  THEN COALESCE(me.rating, 0)
                  ELSE COALESCE(f.rating, me.rating, 0)
             END"
        }
        "view_count" => {
            "CASE WHEN me.kind = 'collection'
                  THEN 0
                  ELSE COALESCE(f.view_count, 0)
             END"
        }
        "name" => {
            "CASE WHEN me.kind = 'collection'
                  THEN COALESCE(me.name, '')
                  ELSE COALESCE(f.name, me.name, '')
             END"
        }
        "mime" => {
            "CASE WHEN me.kind = 'collection'
                  THEN COALESCE(cover_f.mime, 'application/x-collection')
                  ELSE COALESCE(f.mime, 'application/x-collection')
             END"
        }
        _ => {
            "CASE WHEN me.kind = 'collection'
                  THEN COALESCE(me.created_at, '')
                  ELSE COALESCE(f.imported_at, me.created_at, '')
             END"
        }
    }
}

/// Shared SELECT column list for entity-aware grid queries.
/// Returns both singles and collections with unified column shape.
const ENTITY_SLIM_SELECT: &str =
    "me.entity_id,
     (me.kind = 'collection') AS is_collection,
     CASE WHEN me.kind = 'collection' THEN me.cached_item_count ELSE NULL END AS collection_item_count,
     CASE
         WHEN me.kind = 'collection' THEN COALESCE(cover_f.hash, '')
         ELSE COALESCE(f.hash, cover_f.hash, '')
     END AS hash,
     CASE
         WHEN me.kind = 'collection' THEN me.name
         ELSE COALESCE(f.name, me.name)
     END AS name,
     CASE
         WHEN me.kind = 'collection' THEN COALESCE(cover_f.mime, 'application/x-collection')
         ELSE COALESCE(f.mime, cover_f.mime, 'application/x-collection')
     END AS mime,
     CASE
         WHEN me.kind = 'collection' THEN cover_f.width
         ELSE COALESCE(f.width, cover_f.width)
     END AS width,
     CASE
         WHEN me.kind = 'collection' THEN cover_f.height
         ELSE COALESCE(f.height, cover_f.height)
     END AS height,
     CASE
         WHEN me.kind = 'collection' THEN COALESCE(me.cached_total_size_bytes, 0)
         ELSE COALESCE(f.size, me.cached_total_size_bytes, 0)
     END AS size,
     me.status,
     CASE
         WHEN me.kind = 'collection' THEN me.rating
         ELSE COALESCE(f.rating, me.rating)
     END AS rating,
     CASE
         WHEN me.kind = 'collection' THEN cover_f.blurhash
         ELSE COALESCE(f.blurhash, cover_f.blurhash)
     END AS blurhash,
     CASE
         WHEN me.kind = 'collection' THEN COALESCE(me.created_at, '')
         ELSE COALESCE(f.imported_at, me.created_at, '')
     END AS imported_at,
     CASE
         WHEN me.kind = 'collection' THEN cover_f.dominant_color_hex
         ELSE COALESCE(f.dominant_color_hex, cover_f.dominant_color_hex)
     END AS dominant_color_hex,
     CASE WHEN me.kind = 'collection' THEN NULL ELSE f.duration_ms END AS duration_ms,
     CASE WHEN me.kind = 'collection' THEN NULL ELSE f.num_frames END AS num_frames,
     CASE WHEN me.kind = 'collection' THEN 0 ELSE COALESCE(f.has_audio, 0) END AS has_audio,
     CASE WHEN me.kind = 'collection' THEN 0 ELSE COALESCE(f.view_count, 0) END AS view_count,
     CASE WHEN me.kind = 'collection' THEN 0 ELSE COALESCE(f.file_id, 0) END AS file_id";

/// Shared FROM/JOIN clause for entity-aware grid queries.
const ENTITY_SLIM_FROM: &str = " FROM media_entity me
      LEFT JOIN entity_file ef ON ef.entity_id = me.entity_id
      LEFT JOIN file f ON f.file_id = ef.file_id
      LEFT JOIN file cover_f ON cover_f.file_id = me.cover_file_id";

/// In normal scopes, treat collection members as replaced by their collection tile.
/// Members stay in DB (so collection/detail/split can still work) but do not render as standalone rows.
const NON_MEMBER_SINGLE_CLAUSE: &str = " AND (me.kind = 'collection'
           OR me.parent_collection_id IS NULL)";

/// Map a database row from an entity-aware query to FileMetadataSlim.
/// Column order must match ENTITY_SLIM_SELECT.
fn row_to_entity_slim(row: &rusqlite::Row) -> rusqlite::Result<FileMetadataSlim> {
    Ok(FileMetadataSlim {
        entity_id: row.get(0)?,
        is_collection: row.get::<_, i64>(1)? != 0,
        collection_item_count: row.get(2)?,
        hash: row.get(3)?,
        name: row.get(4)?,
        mime: row.get(5)?,
        width: row.get(6)?,
        height: row.get(7)?,
        size: row.get(8)?,
        status: row.get::<_, i64>(9)? as u8,
        rating: row.get(10)?,
        blurhash: row.get(11)?,
        imported_at: row.get(12)?,
        dominant_color_hex: row.get(13)?,
        duration_ms: row.get(14)?,
        num_frames: row.get(15)?,
        has_audio: row.get::<_, i64>(16)? != 0,
        view_count: row.get(17)?,
        file_id: row.get(18)?,
        position_rank: None,
    })
}

/// List entities (singles + collections) with keyset pagination.
/// `cursor` is the last seen value of the sort column for keyset pagination.
pub fn list_files_slim(
    conn: &Connection,
    limit: i64,
    status: Option<i64>,
    sort_field: &str,
    sort_dir: &str,
    cursor: Option<&str>,
    filters: Option<&GridFilters>,
) -> rusqlite::Result<Vec<FileMetadataSlim>> {
    let sort_expr = entity_sort_expr(sort_field);
    let dir = if sort_dir == "asc" { "ASC" } else { "DESC" };
    let op = if sort_dir == "asc" { ">" } else { "<" };

    let mut sql = format!(
        "SELECT {}{} WHERE 1=1",
        ENTITY_SLIM_SELECT, ENTITY_SLIM_FROM
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(s) = status {
        sql.push_str(&format!(" AND me.status = ?{}", param_values.len() + 1));
        param_values.push(Box::new(s));
    } else {
        // Default: active only (status=1)
        sql.push_str(" AND me.status = 1");
    }
    sql.push_str(NON_MEMBER_SINGLE_CLAUSE);

    // Apply filters (use f. prefix — file-level filters naturally exclude collections)
    if let Some(f) = filters {
        append_filter_clauses(&mut sql, &mut param_values, f, "f.", conn);
    }

    if let Some(c) = cursor {
        // Composite cursor: "sort_value\0entity_id" for stable keyset pagination
        let parts: Vec<&str> = c.splitn(2, '\0').collect();
        if parts.len() == 2 {
            let cursor_entity_id: i64 = parts[1].parse().unwrap_or(0);
            let p1 = param_values.len() + 1;
            let p2 = param_values.len() + 2;
            sql.push_str(&format!(
                " AND ({sort_expr}, me.entity_id) {op} (?{p1}, ?{p2})",
            ));
            param_values.push(Box::new(parts[0].to_string()));
            param_values.push(Box::new(cursor_entity_id));
        } else {
            // Legacy single-value cursor fallback
            sql.push_str(&format!(
                " AND {} {} ?{}",
                sort_expr,
                op,
                param_values.len() + 1
            ));
            param_values.push(Box::new(c.to_string()));
        }
    }

    sql.push_str(&format!(
        " ORDER BY {} {}, me.entity_id {} LIMIT ?{}",
        sort_expr,
        dir,
        dir,
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_entity_slim)?;

    rows.collect()
}

/// Batch get file metadata by file_ids — single query.
pub fn batch_get_slim(
    conn: &Connection,
    file_ids: &[i64],
) -> rusqlite::Result<Vec<FileMetadataSlim>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT hash, name, mime, width, height, size, status, rating, blurhash,
                imported_at, dominant_color_hex, duration_ms, num_frames, has_audio, view_count
         FROM file WHERE file_id IN ({})",
        placeholders.join(",")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = file_ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |row| {
        Ok(FileMetadataSlim {
            file_id: 0,
            entity_id: 0,
            is_collection: false,
            collection_item_count: None,
            hash: row.get(0)?,
            name: row.get(1)?,
            mime: row.get(2)?,
            width: row.get(3)?,
            height: row.get(4)?,
            size: row.get(5)?,
            status: row.get::<_, i64>(6)? as u8,
            rating: row.get(7)?,
            blurhash: row.get(8)?,
            imported_at: row.get(9)?,
            dominant_color_hex: row.get(10)?,
            duration_ms: row.get(11)?,
            num_frames: row.get(12)?,
            has_audio: row.get::<_, i64>(13)? != 0,
            view_count: row.get(14)?,
            position_rank: None,
        })
    })?;

    rows.collect()
}

/// Batch get full file records by hashes.
pub fn batch_get_by_hashes(
    conn: &Connection,
    hashes: &[String],
) -> rusqlite::Result<Vec<FileRecord>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (1..=hashes.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT file_id, hash, name, size, mime, width, height, duration_ms, num_frames,
                has_audio, blurhash, status, rating, view_count, phash, imported_at,
                notes, source_urls_json, dominant_color_hex
         FROM file WHERE hash IN ({})",
        placeholders.join(",")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = hashes
        .iter()
        .map(|h| h as &dyn rusqlite::types::ToSql)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |row| {
        Ok(FileRecord {
            file_id: row.get(0)?,
            hash: row.get(1)?,
            name: row.get(2)?,
            size: row.get(3)?,
            mime: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            duration_ms: row.get(7)?,
            num_frames: row.get(8)?,
            has_audio: row.get::<_, i64>(9)? != 0,
            blurhash: row.get(10)?,
            status: row.get(11)?,
            rating: row.get(12)?,
            view_count: row.get(13)?,
            phash: row.get(14)?,
            imported_at: row.get(15)?,
            notes: row.get(16)?,
            source_urls_json: row.get(17)?,
            dominant_color_hex: row.get(18)?,
        })
    })?;

    rows.collect()
}

/// List files with keyset pagination, restricted to a pre-filtered set of file_ids from bitmaps.
pub fn list_files_slim_by_ids(
    conn: &Connection,
    file_ids: &[i64],
    limit: i64,
    sort_field: &str,
    sort_dir: &str,
    cursor: Option<&str>,
    filters: Option<&GridFilters>,
    random_seed: Option<i64>,
) -> rusqlite::Result<Vec<FileMetadataSlim>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Determine sort expression — seeded random or standard
    let (sort_expr_owned, is_random) = if sort_field == "random" {
        if let Some(seed) = random_seed {
            (
                format!("((me.entity_id * 2654435761 + {}) % 2147483647)", seed),
                true,
            )
        } else {
            (entity_sort_expr(sort_field).to_string(), false)
        }
    } else {
        (entity_sort_expr(sort_field).to_string(), false)
    };
    let sort_expr = &sort_expr_owned;
    let dir = if is_random {
        "ASC"
    } else if sort_dir == "asc" {
        "ASC"
    } else {
        "DESC"
    };
    let op = if is_random {
        ">"
    } else if sort_dir == "asc" {
        ">"
    } else {
        "<"
    };

    // Build temp table for filtered entity_ids for efficient JOIN
    conn.execute(
        "CREATE TEMP TABLE IF NOT EXISTS _grid_filter (file_id INTEGER PRIMARY KEY)",
        [],
    )?;
    conn.execute("DELETE FROM _grid_filter", [])?;

    // Batch INSERT — chunks of 500 values per statement to reduce overhead
    for chunk in file_ids.chunks(500) {
        let placeholders: String = chunk
            .iter()
            .enumerate()
            .map(|(i, _)| format!("(?{})", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("INSERT INTO _grid_filter (file_id) VALUES {}", placeholders);
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        conn.execute(&sql, params.as_slice())?;
    }

    let mut sql = format!(
        "SELECT {}
         FROM media_entity me
         INNER JOIN _grid_filter gf ON gf.file_id = me.entity_id
         LEFT JOIN entity_file ef ON ef.entity_id = me.entity_id
         LEFT JOIN file f ON f.file_id = ef.file_id
         LEFT JOIN file cover_f ON cover_f.file_id = me.cover_file_id
         WHERE 1=1",
        ENTITY_SLIM_SELECT
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    sql.push_str(NON_MEMBER_SINGLE_CLAUSE);

    // Apply filters (f. prefix — file-level filters naturally exclude collections)
    if let Some(f) = filters {
        append_filter_clauses(&mut sql, &mut param_values, f, "f.", conn);
    }

    if let Some(c) = cursor {
        // Composite cursor: "sort_value\0entity_id" for stable keyset pagination
        let parts: Vec<&str> = c.splitn(2, '\0').collect();
        if parts.len() == 2 {
            let cursor_entity_id: i64 = parts[1].parse().unwrap_or(0);
            let p1 = param_values.len() + 1;
            let p2 = param_values.len() + 2;
            sql.push_str(&format!(
                " AND ({sort_expr}, me.entity_id) {op} (?{p1}, ?{p2})",
            ));
            // Random sort expression produces integer values — bind cursor as i64
            // to match SQLite type affinity (TEXT vs INTEGER would always fail).
            if is_random {
                let cursor_sort_val: i64 = parts[0].parse().unwrap_or(0);
                param_values.push(Box::new(cursor_sort_val));
            } else {
                param_values.push(Box::new(parts[0].to_string()));
            }
            param_values.push(Box::new(cursor_entity_id));
        } else {
            // Legacy single-value cursor fallback
            sql.push_str(&format!(
                " AND {} {} ?{}",
                sort_expr,
                op,
                param_values.len() + 1
            ));
            param_values.push(Box::new(c.to_string()));
        }
    }

    sql.push_str(&format!(
        " ORDER BY {} {}, me.entity_id {} LIMIT ?{}",
        sort_expr,
        dir,
        dir,
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_entity_slim)?;

    rows.collect()
}

/// List entities ordered by folder position_rank (for manual/custom sort within a folder).
pub fn list_files_slim_by_folder_rank(
    conn: &Connection,
    file_ids: &[i64],
    folder_id: i64,
    limit: i64,
    sort_dir: &str,
    cursor: Option<&str>,
    filters: Option<&GridFilters>,
) -> rusqlite::Result<Vec<FileMetadataSlim>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    let dir = if sort_dir == "asc" { "ASC" } else { "DESC" };
    let op = if sort_dir == "asc" { ">" } else { "<" };

    // Build temp table for filtered file_ids
    conn.execute(
        "CREATE TEMP TABLE IF NOT EXISTS _grid_filter (file_id INTEGER PRIMARY KEY)",
        [],
    )?;
    conn.execute("DELETE FROM _grid_filter", [])?;

    for chunk in file_ids.chunks(500) {
        let placeholders: String = chunk
            .iter()
            .enumerate()
            .map(|(i, _)| format!("(?{})", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("INSERT INTO _grid_filter (file_id) VALUES {}", placeholders);
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        conn.execute(&sql, params.as_slice())?;
    }

    let mut sql = format!(
        "SELECT {}, fe.position_rank
         FROM media_entity me
         INNER JOIN _grid_filter gf ON gf.file_id = me.entity_id
         INNER JOIN folder_entity fe ON fe.entity_id = me.entity_id AND fe.folder_id = ?1
         LEFT JOIN entity_file ef ON ef.entity_id = me.entity_id
         LEFT JOIN file f ON f.file_id = ef.file_id
         LEFT JOIN file cover_f ON cover_f.file_id = me.cover_file_id
         WHERE 1=1",
        ENTITY_SLIM_SELECT
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(folder_id)];
    sql.push_str(NON_MEMBER_SINGLE_CLAUSE);

    // Apply filters (f. prefix — file-level filters naturally exclude collections)
    if let Some(f) = filters {
        append_filter_clauses(&mut sql, &mut param_values, f, "f.", conn);
    }

    if let Some(c) = cursor {
        let parts: Vec<&str> = c.splitn(2, '\0').collect();
        if parts.len() == 2 {
            let cursor_rank: i64 = parts[0].parse().unwrap_or(0);
            let cursor_entity_id: i64 = parts[1].parse().unwrap_or(0);
            let p1 = param_values.len() + 1;
            let p2 = param_values.len() + 2;
            sql.push_str(&format!(
                " AND (fe.position_rank, me.entity_id) {op} (?{p1}, ?{p2})",
            ));
            param_values.push(Box::new(cursor_rank));
            param_values.push(Box::new(cursor_entity_id));
        }
    }

    sql.push_str(&format!(
        " ORDER BY fe.position_rank {}, me.entity_id {} LIMIT ?{}",
        dir,
        dir,
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        let mut item = row_to_entity_slim(row)?;
        item.position_rank = row.get(19)?; // position_rank after 19 entity_slim columns
        Ok(item)
    })?;

    rows.collect()
}

/// List files ordered by media_entity.collection_ordinal (manual/custom collection order).
pub fn list_files_slim_by_collection_rank(
    conn: &Connection,
    file_ids: &[i64],
    collection_id: i64,
    limit: i64,
    cursor: Option<&str>,
    filters: Option<&GridFilters>,
) -> rusqlite::Result<Vec<FileMetadataSlim>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Build temp table for filtered file_ids
    conn.execute(
        "CREATE TEMP TABLE IF NOT EXISTS _grid_filter (file_id INTEGER PRIMARY KEY)",
        [],
    )?;
    conn.execute("DELETE FROM _grid_filter", [])?;

    for chunk in file_ids.chunks(500) {
        let placeholders: String = chunk
            .iter()
            .enumerate()
            .map(|(i, _)| format!("(?{})", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("INSERT INTO _grid_filter (file_id) VALUES {}", placeholders);
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        conn.execute(&sql, params.as_slice())?;
    }

    let mut sql = String::from(
        "SELECT f.hash, f.name, f.mime, f.width, f.height, f.size, f.status, f.rating, f.blurhash,
                f.imported_at, f.dominant_color_hex, f.duration_ms, f.num_frames, f.has_audio, f.view_count,
                f.file_id, ef.entity_id, COALESCE(me.collection_ordinal, 0)
         FROM file f
         INNER JOIN _grid_filter gf ON gf.file_id = f.file_id
         INNER JOIN entity_file ef ON ef.file_id = f.file_id
         INNER JOIN media_entity me
            ON me.entity_id = ef.entity_id
           AND me.kind = 'single'
           AND me.parent_collection_id = ?1
         WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(collection_id)];

    if let Some(f) = filters {
        append_filter_clauses(&mut sql, &mut param_values, f, "f.", conn);
    }

    if let Some(c) = cursor {
        let parts: Vec<&str> = c.splitn(2, '\0').collect();
        if parts.len() == 2 {
            let cursor_rank: i64 = parts[0].parse().unwrap_or(0);
            let cursor_file_id: i64 = parts[1].parse().unwrap_or(0);
            let p1 = param_values.len() + 1;
            let p2 = param_values.len() + 2;
            sql.push_str(&format!(
                " AND (COALESCE(me.collection_ordinal, 0), f.file_id) > (?{p1}, ?{p2})",
            ));
            param_values.push(Box::new(cursor_rank));
            param_values.push(Box::new(cursor_file_id));
        }
    }

    sql.push_str(&format!(
        " ORDER BY COALESCE(me.collection_ordinal, 0) ASC, f.file_id ASC LIMIT ?{}",
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(FileMetadataSlim {
            file_id: row.get(15)?,
            entity_id: row.get(16)?,
            is_collection: false,
            collection_item_count: None,
            hash: row.get(0)?,
            name: row.get(1)?,
            mime: row.get(2)?,
            width: row.get(3)?,
            height: row.get(4)?,
            size: row.get(5)?,
            status: row.get::<_, i64>(6)? as u8,
            rating: row.get(7)?,
            blurhash: row.get(8)?,
            imported_at: row.get(9)?,
            dominant_color_hex: row.get(10)?,
            duration_ms: row.get(11)?,
            num_frames: row.get(12)?,
            has_audio: row.get::<_, i64>(13)? != 0,
            view_count: row.get(14)?,
            position_rank: row.get(17)?,
        })
    })?;

    rows.collect()
}

/// Get dominant colors for a file (used by inspector detail view).
pub fn get_file_colors(
    conn: &Connection,
    file_id: i64,
) -> rusqlite::Result<Vec<(String, f64, f64, f64)>> {
    let mut stmt = conn
        .prepare_cached("SELECT hex, l, a, b FROM file_color WHERE file_id = ?1 ORDER BY rowid")?;
    let rows = stmt.query_map([file_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, f64>(3)?,
        ))
    })?;
    rows.collect()
}

/// Batch get dominant colors for multiple files, keyed by file_id.
pub fn get_files_colors_batch(
    conn: &Connection,
    file_ids: &[i64],
) -> rusqlite::Result<std::collections::HashMap<i64, Vec<(String, f64, f64, f64)>>> {
    use std::collections::HashMap;
    if file_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT file_id, hex, l, a, b FROM file_color WHERE file_id IN ({}) ORDER BY file_id, rowid",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = file_ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = stmt.query_map(params.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, f64>(4)?,
        ))
    })?;
    let mut result: HashMap<i64, Vec<(String, f64, f64, f64)>> = HashMap::new();
    for row in rows {
        let (fid, hex, l, a, b) = row?;
        result.entry(fid).or_default().push((hex, l, a, b));
    }
    Ok(result)
}

/// Save file colors for color search.
pub fn save_file_colors(
    conn: &Connection,
    file_id: i64,
    colors: &[(String, f32, f32, f32)], // (hex, l, a, b)
) -> rusqlite::Result<()> {
    // Clean up R*Tree entries for existing colors
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
        "INSERT INTO file_color_rtree (id, l_min, l_max, a_min, a_max, b_min, b_max)
         VALUES (?1, ?2, ?2, ?3, ?3, ?4, ?4)",
    )?;
    for (hex, l, a, b) in colors {
        color_stmt.execute(params![file_id, hex, l, a, b])?;
        let rowid = conn.last_insert_rowid();
        // Defensively clear stale R*Tree rows that can survive legacy cleanup paths.
        rtree_delete_stmt.execute(params![rowid])?;
        rtree_stmt.execute(params![rowid, l, a, b])?;
    }
    Ok(())
}

/// Search files by Lab color range using R*Tree spatial index.
pub fn search_by_color(
    conn: &Connection,
    l_range: (f32, f32),
    a_range: (f32, f32),
    b_range: (f32, f32),
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT fc.file_id
         FROM file_color_rtree rt
         JOIN file_color fc ON fc.rowid = rt.id
         WHERE rt.l_max >= ?1 AND rt.l_min <= ?2
           AND rt.a_max >= ?3 AND rt.a_min <= ?4
           AND rt.b_max >= ?5 AND rt.b_min <= ?6",
    )?;
    let rows = stmt.query_map(
        params![l_range.0, l_range.1, a_range.0, a_range.1, b_range.0, b_range.1],
        |row| row.get(0),
    )?;
    rows.collect()
}

/// Get aggregate file statistics — single query.
pub fn aggregate_stats(conn: &Connection) -> rusqlite::Result<FileStats> {
    let mut inbox: i64 = 0;
    let mut active: i64 = 0;
    let mut trash: i64 = 0;
    let mut total_size: i64 = 0;

    let mut stmt = conn.prepare_cached(
        "SELECT status, COUNT(*), COALESCE(SUM(size), 0) FROM file GROUP BY status",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    for row in rows {
        let (status, count, size_sum) = row?;
        match status {
            0 => {
                inbox = count;
                total_size += size_sum;
            }
            1 => {
                active = count;
                total_size += size_sum;
            }
            2 => {
                trash = count;
            }
            _ => {}
        }
    }

    Ok(FileStats {
        total: inbox + active,
        inbox,
        active,
        trash,
        total_size,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStats {
    pub total: i64,
    pub inbox: i64,
    pub active: i64,
    pub trash: i64,
    pub total_size: i64,
}

impl SqliteDatabase {
    pub async fn wipe_all_files(&self) -> Result<(), String> {
        self.with_conn(|conn| wipe_all_files(conn)).await?;
        self.hash_index.clear();
        self.bitmaps.clear();
        self.emit_compiler_event(CompilerEvent::RebuildAll);
        Ok(())
    }

    pub async fn insert_file(&self, f: NewFile) -> Result<i64, String> {
        let hash = f.hash.clone();
        let status = f.status;
        let file_id = self.with_conn(move |conn| insert_file(conn, &f)).await?;
        self.hash_index.insert(hash, file_id);
        self.bitmaps
            .insert(&BitmapKey::Status(status), file_id as u32);
        self.emit_compiler_event(CompilerEvent::FileInserted { file_id });
        Ok(file_id)
    }

    pub async fn get_file_by_hash(&self, hash: &str) -> Result<Option<FileRecord>, String> {
        let h = hash.to_string();
        self.with_read_conn(move |conn| get_file_by_hash(conn, &h))
            .await
    }

    pub async fn file_exists(&self, hash: &str) -> Result<bool, String> {
        let h = hash.to_string();
        self.with_read_conn(move |conn| file_exists(conn, &h)).await
    }

    pub async fn count_files(&self, status: Option<i64>) -> Result<i64, String> {
        // Use bitmap for O(1) count when possible
        match status {
            Some(s) => Ok(self.bitmaps.len(&BitmapKey::Status(s)) as i64),
            None => {
                // V2 default visibility excludes trash.
                let total = self.bitmaps.len(&BitmapKey::Status(0))
                    + self.bitmaps.len(&BitmapKey::Status(1));
                if total > 0 {
                    Ok(total as i64)
                } else {
                    // Fallback to SQL if bitmaps not yet populated
                    self.with_read_conn(move |conn| count_files(conn, None))
                        .await
                }
            }
        }
    }

    pub async fn update_file_status(&self, hash: &str, status: i64) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        let fid = file_id;
        self.with_conn(move |conn| update_status(conn, fid, status))
            .await?;

        // Update status bitmaps
        let fid_u32 = file_id as u32;
        for s in 0..=2i64 {
            self.bitmaps.remove(&BitmapKey::Status(s), fid_u32);
        }
        self.bitmaps.insert(&BitmapKey::Status(status), fid_u32);

        self.emit_compiler_event(CompilerEvent::FileStatusChanged { file_id });
        Ok(())
    }

    /// Batch update status for many files at once (single transaction + bulk bitmap swap).
    pub async fn update_file_status_batch(
        &self,
        file_ids: &roaring::RoaringBitmap,
        status: i64,
    ) -> Result<usize, String> {
        let ids: Vec<i64> = file_ids.iter().map(|id| id as i64).collect();
        let count = ids.len();
        if count == 0 {
            return Ok(0);
        }

        let s = status;
        self.with_conn_mut(move |conn| {
            let tx = conn.transaction()?;
            for chunk in ids.chunks(999) {
                let placeholders: String = std::iter::repeat("?")
                    .take(chunk.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!("UPDATE file SET status = ? WHERE file_id IN ({placeholders})");
                let mut flat_params: Vec<Box<dyn rusqlite::types::ToSql>> =
                    Vec::with_capacity(1 + chunk.len());
                flat_params.push(Box::new(s));
                for &fid in chunk {
                    flat_params.push(Box::new(fid));
                }
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    flat_params.iter().map(|p| p.as_ref()).collect();
                tx.execute(&sql, param_refs.as_slice())?;

                let entity_sql = format!(
                    "UPDATE media_entity
                     SET status = ?, updated_at = CURRENT_TIMESTAMP
                     WHERE entity_id IN (
                         SELECT entity_id FROM entity_file WHERE file_id IN ({placeholders})
                     )"
                );
                tx.execute(&entity_sql, param_refs.as_slice())?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;

        // Bulk bitmap update
        for fid in file_ids.iter() {
            for s in 0..=2i64 {
                self.bitmaps.remove(&BitmapKey::Status(s), fid);
            }
            self.bitmaps.insert(&BitmapKey::Status(status), fid);
        }

        // One compiler event for the whole batch
        self.emit_compiler_event(CompilerEvent::StatusBatchChanged);
        Ok(count)
    }

    pub async fn delete_file_by_hash(&self, hash: &str) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;

        // Query folder memberships BEFORE deletion (CASCADE will remove folder_entity rows)
        let fid = file_id;
        let folder_ids = self
            .with_read_conn(move |conn| super::folders::get_entity_folder_memberships(conn, fid))
            .await?;

        let fid = file_id;
        self.with_conn(move |conn| delete_file(conn, fid)).await?;

        // Clean up caches
        let fid_u32 = file_id as u32;
        for s in 0..=2i64 {
            self.bitmaps.remove(&BitmapKey::Status(s), fid_u32);
        }
        // Remove from folder bitmaps (counts were stale without this)
        for membership in &folder_ids {
            self.bitmaps
                .remove(&BitmapKey::Folder(membership.folder_id), fid_u32);
        }
        self.hash_index.remove_by_hash(hash);
        self.emit_compiler_event(CompilerEvent::FileDeleted { file_id });
        Ok(())
    }

    pub async fn list_files_slim(
        &self,
        limit: i64,
        status: Option<i64>,
        sort_field: String,
        sort_dir: String,
        cursor: Option<String>,
        filters: Option<GridFilters>,
    ) -> Result<Vec<FileMetadataSlim>, String> {
        self.with_read_conn(move |conn| {
            list_files_slim(
                conn,
                limit,
                status,
                &sort_field,
                &sort_dir,
                cursor.as_deref(),
                filters.as_ref(),
            )
        })
        .await
    }

    pub async fn batch_get_metadata_slim(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<FileMetadataSlim>, String> {
        self.with_read_conn(move |conn| batch_get_by_hashes(conn, &hashes))
            .await
            .map(|records| {
                records
                    .into_iter()
                    .map(|r| FileMetadataSlim {
                        file_id: r.file_id,
                        entity_id: r.file_id,
                        is_collection: false,
                        collection_item_count: None,
                        hash: r.hash,
                        name: r.name,
                        mime: r.mime,
                        width: r.width,
                        height: r.height,
                        size: r.size,
                        status: r.status as u8,
                        rating: r.rating,
                        blurhash: r.blurhash,
                        imported_at: r.imported_at,
                        dominant_color_hex: r.dominant_color_hex,
                        duration_ms: r.duration_ms,
                        num_frames: r.num_frames,
                        has_audio: r.has_audio,
                        view_count: r.view_count,
                        position_rank: None,
                    })
                    .collect()
            })
    }

    pub async fn update_rating(&self, hash: &str, rating: Option<i64>) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        self.with_conn(move |conn| update_rating(conn, file_id, rating))
            .await
    }

    pub async fn set_file_name(&self, hash: &str, name: Option<&str>) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        let n = name.map(|s| s.to_string());
        self.with_conn(move |conn| update_name(conn, file_id, n.as_deref()))
            .await
    }

    pub async fn set_notes(&self, hash: &str, notes: Option<&str>) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        let n = notes.map(|s| s.to_string());
        self.with_conn(move |conn| set_notes(conn, file_id, n.as_deref()))
            .await
    }

    pub async fn set_source_urls(&self, hash: &str, urls_json: Option<&str>) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        let u = urls_json.map(|s| s.to_string());
        self.with_conn(move |conn| set_source_urls(conn, file_id, u.as_deref()))
            .await
    }

    pub async fn increment_view_count(&self, hash: &str) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        self.with_conn(move |conn| increment_view_count(conn, file_id))
            .await?;
        self.emit_compiler_event(CompilerEvent::ViewCountChanged);
        Ok(())
    }

    pub async fn set_phash(&self, hash: &str, phash: &str) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        let p = phash.to_string();
        self.with_conn(move |conn| set_phash(conn, file_id, &p))
            .await
    }

    pub async fn set_blurhash(&self, hash: &str, blurhash: Option<&str>) -> Result<(), String> {
        let file_id = self.resolve_hash(hash).await?;
        let b = blurhash.map(|s| s.to_string());
        self.with_conn(move |conn| set_blurhash(conn, file_id, b.as_deref()))
            .await
    }

    pub async fn aggregate_file_stats(&self) -> Result<FileStats, String> {
        self.with_read_conn(aggregate_stats).await
    }
}

#[cfg(test)]
mod tests {
    use super::{insert_file, save_file_colors, NewFile};
    use rusqlite::Connection;

    #[test]
    fn save_file_colors_handles_stale_rtree_rowid_collision() {
        let conn = Connection::open_in_memory().unwrap();
        crate::sqlite::schema::apply_pragmas(&conn).unwrap();
        crate::sqlite::schema::init_schema(&conn).unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let file_id = insert_file(
            &conn,
            &NewFile {
                hash: "hash_color_collision".to_string(),
                name: Some("collision".to_string()),
                size: 1024,
                mime: "image/png".to_string(),
                width: Some(128),
                height: Some(128),
                duration_ms: None,
                num_frames: None,
                has_audio: false,
                blurhash: None,
                status: 0,
                imported_at: now,
                notes: None,
                source_urls_json: None,
                dominant_color_hex: None,
                dominant_palette_blob: None,
            },
        )
        .unwrap();

        // Simulate a stale RTree row left behind by legacy cleanup paths.
        conn.execute(
            "INSERT INTO file_color_rtree (id, l_min, l_max, a_min, a_max, b_min, b_max)
             VALUES (1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)",
            [],
        )
        .unwrap();

        save_file_colors(
            &conn,
            file_id,
            &[("#ffffff".to_string(), 100.0, 0.0, 0.0)],
        )
        .unwrap();

        let rtree_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_color_rtree", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rtree_rows, 1);
    }
}
