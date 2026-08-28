use roaring::RoaringBitmap;
use rusqlite::{params, Connection, Transaction};

use crate::predicate::TextField;
use crate::{Result, RootId};

pub fn search(connection: &Connection, field: TextField, query: &str) -> Result<RoaringBitmap> {
    let column = match field {
        TextField::Global => "root_fts",
        TextField::Notes => "notes",
        TextField::SourceUrl => "urls",
    };
    let expression = if column == "root_fts" {
        query.to_owned()
    } else {
        format!("{column}:({query})")
    };
    let mut statement = connection
        .prepare_cached("SELECT CAST(root_id AS INTEGER) FROM root_fts WHERE root_fts MATCH ?1")?;
    let rows = statement.query_map([expression], |row| row.get::<_, u32>(0))?;
    Ok(rows.collect::<std::result::Result<RoaringBitmap, _>>()?)
}

pub fn mark_dirty(
    transaction: &Transaction<'_>,
    roots: &RoaringBitmap,
    category: u8,
    queued_at_ms: i64,
) -> Result<()> {
    let mut statement = transaction.prepare_cached(
        "INSERT INTO fts_dirty(root_id, category, queued_at_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(root_id, category) DO UPDATE SET queued_at_ms = excluded.queued_at_ms",
    )?;
    for root_id in roots {
        statement.execute(params![root_id, category, queued_at_ms])?;
    }
    Ok(())
}

pub fn settle_batch(transaction: &Transaction<'_>, limit: usize) -> Result<RoaringBitmap> {
    let roots = {
        let mut statement = transaction.prepare(
            "WITH ranked AS (
                 SELECT root_id, category, queued_at_ms,
                        row_number() OVER (
                            PARTITION BY category ORDER BY queued_at_ms, root_id
                        ) AS category_rank
                 FROM fts_dirty
             )
             SELECT root_id FROM ranked
             ORDER BY category_rank, queued_at_ms, category, root_id
             LIMIT ?1",
        )?;
        let values = statement
            .query_map([limit as i64], |row| row.get::<_, u32>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        values
    };
    let unique = roots.into_iter().collect::<std::collections::BTreeSet<_>>();
    for root_id in &unique {
        let content = transaction.query_row(
            "SELECT root.root_id, root.name, COALESCE(root.notes, ''), root.source_urls_json,
                    COALESCE(group_concat(source.source_text, ' '), '')
             FROM library_root root
             LEFT JOIN media_item media ON media.media_id = root.cover_media_id
             LEFT JOIN source_provenance source ON source.media_id = media.media_id
             WHERE root.root_id = ?1
             GROUP BY root.root_id",
            [root_id],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        );
        transaction.execute("DELETE FROM root_fts WHERE root_id = ?1", [root_id])?;
        if let Ok((id, name, notes, urls, source_text)) = content {
            transaction.execute(
                "INSERT INTO root_fts(root_id, name, notes, urls, source_text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, notes, urls, source_text],
            )?;
        }
        transaction.execute("DELETE FROM fts_dirty WHERE root_id = ?1", [root_id])?;
    }
    Ok(unique.into_iter().collect())
}

pub fn dirty_age_ms(connection: &Connection, now_ms: i64) -> Result<Option<u64>> {
    let queued = connection.query_row("SELECT MIN(queued_at_ms) FROM fts_dirty", [], |row| {
        row.get::<_, Option<i64>>(0)
    })?;
    Ok(queued.map(|value| now_ms.saturating_sub(value) as u64))
}

pub fn mark_one(transaction: &Transaction<'_>, root_id: RootId, now_ms: i64) -> Result<()> {
    mark_dirty(transaction, &[root_id.0].into_iter().collect(), 1, now_ms)
}
