//! Recently viewed persistence.

use super::*;

impl LibraryDatabase {
    /// Record the media entity and return its hash plus visible recent count.
    pub fn record_media_view(&self, entity_hash: &str) -> Result<(String, i64), String> {
        let entity_hash = entity_hash.to_string();
        let viewed_at = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            let entity: Option<(i64, String)> = conn
                .query_row(
                    "SELECT entity_id, entity_hash
                     FROM media_entity
                     WHERE entity_hash = ?1",
                    [&entity_hash],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (entity_id, resolved_hash) = entity.ok_or_else(|| {
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
            Ok((resolved_hash, count))
        })
    }
}
