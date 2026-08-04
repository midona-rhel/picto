//! Recently viewed persistence.

use super::*;

impl LibraryDatabase {
    /// Record the top-level logical entity and return its hash plus visible recent count.
    pub fn record_media_view(&self, entity_hash: &str) -> Result<(String, i64), String> {
        let entity_hash = entity_hash.to_string();
        let viewed_at = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            let top_level: Option<(i64, String)> = conn
                .query_row(
                    "SELECT top.entity_id, top.entity_hash
                     FROM media_entity selected
                     JOIN media_entity top
                       ON top.entity_id = COALESCE(selected.parent_collection_entity_id, selected.entity_id)
                     WHERE selected.entity_hash = ?1",
                    [&entity_hash],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (entity_id, top_level_hash) = top_level.ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(format!(
                    "Unknown media entity hash: {entity_hash}"
                ))
            })?;

            conn.execute(
                "INSERT INTO media_view (entity_id, viewed_at) VALUES (?1, ?2)
                 ON CONFLICT(entity_id) DO UPDATE SET viewed_at = excluded.viewed_at",
                rusqlite::params![entity_id, viewed_at],
            )?;
            let count = query::grid::recently_viewed_count(conn)?;
            Ok((top_level_hash, count))
        })
    }
}
