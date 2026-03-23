//! Tag search and FTS queries.

use rusqlite::{params, Connection};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TagSearchResult {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub file_count: i64,
}

/// Search tags by prefix.
pub fn search_tags(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<TagSearchResult>> {
    let pattern = format!("{query}%");
    let mut stmt = conn.prepare(
        "SELECT tag_id, namespace, subtag, file_count
         FROM tag
         WHERE subtag LIKE ?1 OR namespace LIKE ?1
         ORDER BY file_count DESC
         LIMIT ?2",
    )?;
    let results = stmt
        .query_map(params![pattern, limit], |row| {
            Ok(TagSearchResult {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
                file_count: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(results)
}
