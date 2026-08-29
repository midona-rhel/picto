use roaring::RoaringBitmap;
use rusqlite::{params, Connection, Transaction};

use crate::predicate::TextField;
use crate::{Result, RootId};

fn field_column(field: TextField) -> Option<&'static str> {
    match field {
        TextField::Global => None,
        TextField::Name => Some("name"),
        TextField::Notes => Some("notes"),
        TextField::SourceUrl => Some("urls"),
    }
}

fn substring_match(
    connection: &Connection,
    column: Option<&str>,
    value: &str,
) -> Result<RoaringBitmap> {
    // LIKE selects trigram candidates. instr() removes false positives when
    // the literal value contains LIKE wildcard characters.
    let pattern = format!("%{value}%");
    let sql = match column {
        Some(column) => format!(
            "SELECT rowid FROM root_fts
             WHERE {column} LIKE ?1 AND instr({column}, ?2) > 0"
        ),
        None => ["name", "notes", "urls", "source_text"]
            .into_iter()
            .map(|column| {
                format!(
                    "SELECT rowid FROM root_fts
                     WHERE {column} LIKE ?1 AND instr({column}, ?2) > 0"
                )
            })
            .collect::<Vec<_>>()
            .join(" UNION "),
    };
    let mut statement = connection.prepare_cached(&sql)?;
    let rows = statement.query_map([&pattern, value], |row| row.get::<_, u32>(0))?;
    Ok(rows.collect::<std::result::Result<RoaringBitmap, _>>()?)
}

/// Matches the complete trimmed query as a literal substring. Text is lowered
/// before indexing and querying so the trigram path has consistent Unicode
/// case behavior independent of SQLite's ASCII-only NOCASE rules.
pub fn search(connection: &Connection, field: TextField, query: &str) -> Result<RoaringBitmap> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(RoaringBitmap::new());
    }
    substring_match(connection, field_column(field), &query)
}

pub fn remove_root(transaction: &Transaction<'_>, root_id: u32) -> Result<()> {
    transaction.execute("DELETE FROM root_fts WHERE rowid = ?1", [root_id])?;
    Ok(())
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
        remove_root(transaction, *root_id)?;
        if let Ok((id, name, notes, urls, source_text)) = content {
            transaction.execute(
                "INSERT INTO root_fts(rowid, name, notes, urls, source_text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    name.to_lowercase(),
                    notes.to_lowercase(),
                    urls.to_lowercase(),
                    source_text.to_lowercase(),
                ],
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
