//! Folder CRUD + manual ordering with gap-based ranking.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::bitmaps::BitmapKey;
use super::compilers::CompilerEvent;
use super::SqliteDatabase;

/// Gap between position_rank values for folder file ordering.
const RANK_GAP: i64 = 1 << 20; // ~1M

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub folder_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

pub struct NewFolder {
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

pub fn create_folder(conn: &Connection, f: &NewFolder) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let max_order: Option<i64> = conn
        .query_row(
            "SELECT MAX(sort_order) FROM folder WHERE parent_id IS ?1",
            [f.parent_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let sort_order = max_order.unwrap_or(0) + 1;

    conn.execute(
        "INSERT INTO folder (name, parent_id, icon, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![f.name, f.parent_id, f.icon, f.color, sort_order, now, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<Option<Folder>> {
    conn.query_row(
        "SELECT folder_id, name, parent_id, icon, color, sort_order, created_at, updated_at
         FROM folder WHERE folder_id = ?1",
        [folder_id],
        |row| {
            Ok(Folder {
                folder_id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                sort_order: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .optional()
}

pub fn list_folders(conn: &Connection) -> rusqlite::Result<Vec<Folder>> {
    let mut stmt = conn.prepare_cached(
        "SELECT folder_id, name, parent_id, icon, color, sort_order, created_at, updated_at
         FROM folder ORDER BY sort_order",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Folder {
            folder_id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            icon: row.get(3)?,
            color: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn update_folder(
    conn: &Connection,
    folder_id: i64,
    name: &str,
    icon: Option<&str>,
    color: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE folder SET name = ?1, icon = ?2, color = ?3, updated_at = ?4
         WHERE folder_id = ?5",
        params![name, icon, color, now, folder_id],
    )?;
    Ok(())
}

pub fn delete_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM folder WHERE folder_id = ?1", [folder_id])?;
    Ok(())
}

pub fn update_folder_parent(
    conn: &Connection,
    folder_id: i64,
    new_parent_id: Option<i64>,
) -> rusqlite::Result<()> {
    // Cycle detection: walk parent chain from new_parent_id, ensure folder_id is not in it
    if let Some(pid) = new_parent_id {
        let mut current = Some(pid);
        while let Some(cid) = current {
            if cid == folder_id {
                return Err(rusqlite::Error::InvalidParameterName(
                    "Cycle detected: folder cannot be its own ancestor".to_string(),
                ));
            }
            current = conn
                .query_row(
                    "SELECT parent_id FROM folder WHERE folder_id = ?1",
                    [cid],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .optional()?
                .flatten();
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE folder SET parent_id = ?1, updated_at = ?2 WHERE folder_id = ?3",
        params![new_parent_id, now, folder_id],
    )?;
    Ok(())
}

/// Add an entity to a folder at the end.
pub fn add_entity_to_folder(
    conn: &Connection,
    folder_id: i64,
    entity_id: i64,
) -> rusqlite::Result<()> {
    let max_rank: Option<i64> = conn
        .query_row(
            "SELECT MAX(position_rank) FROM folder_entity WHERE folder_id = ?1",
            [folder_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let rank = max_rank.unwrap_or(0) + RANK_GAP;

    conn.execute(
        "INSERT OR IGNORE INTO folder_entity (folder_id, entity_id, position_rank) VALUES (?1, ?2, ?3)",
        params![folder_id, entity_id, rank],
    )?;
    Ok(())
}

pub fn add_entities_to_folder_batch(
    conn: &Connection,
    folder_id: i64,
    entity_ids: &[i64],
) -> rusqlite::Result<usize> {
    if entity_ids.is_empty() {
        return Ok(0);
    }
    let max_rank: Option<i64> = conn
        .query_row(
            "SELECT MAX(position_rank) FROM folder_entity WHERE folder_id = ?1",
            [folder_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let mut rank = max_rank.unwrap_or(0);
    let mut inserted = 0usize;
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO folder_entity (folder_id, entity_id, position_rank) VALUES (?1, ?2, ?3)",
    )?;
    for &eid in entity_ids {
        rank += RANK_GAP;
        let changed = stmt.execute(params![folder_id, eid, rank])?;
        if changed > 0 {
            inserted += 1;
        }
    }
    Ok(inserted)
}

/// Remove an entity from a folder.
pub fn remove_entity_from_folder(
    conn: &Connection,
    folder_id: i64,
    entity_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM folder_entity WHERE folder_id = ?1 AND entity_id = ?2",
        params![folder_id, entity_id],
    )?;
    Ok(())
}

/// Reorder an entity within a folder (move between prev and next).
pub fn reorder_entity(
    conn: &Connection,
    folder_id: i64,
    entity_id: i64,
    prev_rank: Option<i64>,
    next_rank: Option<i64>,
) -> rusqlite::Result<()> {
    let new_rank = match (prev_rank, next_rank) {
        (Some(p), Some(n)) => {
            if n - p <= 1 {
                // Gap exhausted — redistribute
                redistribute_ranks(conn, folder_id)?;
                // Re-fetch surrounding ranks and compute midpoint
                let midpoint = (p + n) / 2;
                midpoint.max(p + 1)
            } else {
                (p + n) / 2
            }
        }
        (Some(p), None) => p + RANK_GAP,
        (None, Some(n)) => n / 2,
        (None, None) => RANK_GAP,
    };

    conn.execute(
        "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
        params![new_rank, folder_id, entity_id],
    )?;
    Ok(())
}

/// A folder membership entry (for "which folders does this entity belong to?").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMembership {
    pub folder_id: i64,
    pub folder_name: String,
}

/// Get all folders that an entity belongs to.
pub fn get_entity_folder_memberships(
    conn: &Connection,
    entity_id: i64,
) -> rusqlite::Result<Vec<FolderMembership>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.folder_id, f.name
         FROM folder_entity fe
         JOIN folder f ON f.folder_id = fe.folder_id
         WHERE fe.entity_id = ?1
         ORDER BY f.name",
    )?;
    let rows = stmt.query_map([entity_id], |row| {
        Ok(FolderMembership {
            folder_id: row.get(0)?,
            folder_name: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Count entities in a folder.
pub fn count_folder_entities(conn: &Connection, folder_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM folder_entity WHERE folder_id = ?1",
        [folder_id],
        |row| row.get(0),
    )
}

/// Get the hash of the first file in a folder (by position_rank) for cover preview.
pub fn get_folder_cover_hash(
    conn: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT f.hash FROM folder_entity fe
         JOIN file f ON f.file_id = fe.entity_id
         WHERE fe.folder_id = ?1 AND f.status != 2
         ORDER BY fe.position_rank
         LIMIT 1",
        [folder_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Get ordered entity_ids in a folder.
pub fn get_folder_entity_ids(conn: &Connection, folder_id: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT entity_id FROM folder_entity WHERE folder_id = ?1 ORDER BY position_rank",
    )?;
    let rows = stmt.query_map([folder_id], |row| row.get(0))?;
    rows.collect()
}

fn get_entity_rank_in_folder(
    conn: &Connection,
    folder_id: i64,
    entity_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT position_rank FROM folder_entity WHERE folder_id = ?1 AND entity_id = ?2",
        params![folder_id, entity_id],
        |row| row.get(0),
    )
    .optional()
}

/// Redistribute ranks evenly across a folder (gap reset).
fn redistribute_ranks(conn: &Connection, folder_id: i64) -> rusqlite::Result<()> {
    let entity_ids = get_folder_entity_ids(conn, folder_id)?;
    let mut stmt = conn.prepare_cached(
        "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
    )?;
    for (i, eid) in entity_ids.iter().enumerate() {
        let rank = (i as i64 + 1) * RANK_GAP;
        stmt.execute(params![rank, folder_id, eid])?;
    }
    Ok(())
}

/// Sort all (or a subset of) folder items by a file column and reassign position_rank.
///
/// `sort_by` must be one of: "name", "imported_at", "size", "rating", "mime".
/// `direction` must be "asc" or "desc".
/// If `entity_ids` is Some, only those items are rearranged (in-place among their current rank
/// slots, preserving the relative position of other items). Otherwise, all items are sorted.
pub fn sort_folder_items(
    conn: &Connection,
    folder_id: i64,
    sort_by: &str,
    direction: &str,
    entity_ids: Option<&[i64]>,
) -> rusqlite::Result<()> {
    let sort_col = match sort_by {
        "name" => "f.name COLLATE NOCASE",
        "imported_at" => "f.imported_at",
        "size" => "f.size",
        "rating" => "f.rating",
        "mime" => "f.mime",
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Invalid sort column: {}",
                sort_by
            )));
        }
    };
    let dir = if direction == "asc" { "ASC" } else { "DESC" };

    if let Some(subset_ids) = entity_ids {
        if subset_ids.is_empty() {
            return Ok(());
        }
        // Partial sort: rearrange only the given subset within their current rank slots.
        // 1. Collect current ranks for subset items (in position_rank order).
        let placeholders: String = subset_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let ranks_sql = format!(
            "SELECT entity_id, position_rank FROM folder_entity \
             WHERE folder_id = ?1 AND entity_id IN ({}) ORDER BY position_rank ASC",
            placeholders
        );
        let mut ranks_stmt = conn.prepare(&ranks_sql)?;
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(folder_id)];
        for &eid in subset_ids {
            param_values.push(Box::new(eid));
        }
        let refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let rank_rows: Vec<(i64, i64)> = ranks_stmt
            .query_map(refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let ranks: Vec<i64> = rank_rows.iter().map(|r| r.1).collect();

        // 2. Query subset sorted by the requested column.
        let sorted_sql = format!(
            "SELECT fe.entity_id FROM folder_entity fe \
             JOIN file f ON f.file_id = fe.entity_id \
             WHERE fe.folder_id = ?1 AND fe.entity_id IN ({}) \
             ORDER BY {} {}",
            placeholders, sort_col, dir
        );
        let mut sorted_stmt = conn.prepare(&sorted_sql)?;
        let sorted_ids: Vec<i64> = sorted_stmt
            .query_map(refs.as_slice(), |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        // 3. Assign sorted entity_ids to the same rank slots.
        let mut update_stmt = conn.prepare_cached(
            "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
        )?;
        for (i, eid) in sorted_ids.iter().enumerate() {
            if i < ranks.len() {
                update_stmt.execute(params![ranks[i], folder_id, eid])?;
            }
        }
    } else {
        // Full sort: reorder all items by the sort column.
        let sql = format!(
            "SELECT fe.entity_id FROM folder_entity fe \
             JOIN file f ON f.file_id = fe.entity_id \
             WHERE fe.folder_id = ?1 \
             ORDER BY {} {}",
            sort_col, dir
        );
        let mut stmt = conn.prepare(&sql)?;
        let sorted_ids: Vec<i64> = stmt
            .query_map([folder_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut update_stmt = conn.prepare_cached(
            "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
        )?;
        for (i, eid) in sorted_ids.iter().enumerate() {
            let rank = (i as i64 + 1) * RANK_GAP;
            update_stmt.execute(params![rank, folder_id, eid])?;
        }
    }
    Ok(())
}

/// Reverse the position_rank order of items in a folder.
/// If `entity_ids` is Some, only reverse those items among themselves (preserving other items).
/// Otherwise, reverse all items.
pub fn reverse_folder_items(
    conn: &Connection,
    folder_id: i64,
    entity_ids: Option<&[i64]>,
) -> rusqlite::Result<()> {
    if let Some(subset_ids) = entity_ids {
        if subset_ids.len() < 2 {
            return Ok(());
        }
        // Partial reverse: collect the subset's current ranks (ascending),
        // then assign them in reverse to the reversed entity_id order.
        let placeholders: String = subset_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT entity_id, position_rank FROM folder_entity \
             WHERE folder_id = ?1 AND entity_id IN ({}) ORDER BY position_rank ASC",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(folder_id)];
        for &eid in subset_ids {
            param_values.push(Box::new(eid));
        }
        let refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let rows: Vec<(i64, i64)> = stmt
            .query_map(refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let ranks: Vec<i64> = rows.iter().map(|r| r.1).collect();
        let entity_ids_ordered: Vec<i64> = rows.iter().map(|r| r.0).collect();

        // Assign in reverse: first entity gets last rank, etc.
        let mut update_stmt = conn.prepare_cached(
            "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
        )?;
        let n = entity_ids_ordered.len();
        for i in 0..n {
            update_stmt.execute(params![ranks[n - 1 - i], folder_id, entity_ids_ordered[i]])?;
        }
    } else {
        // Full reverse: get all items in current order, assign ranks in reverse.
        let current_ids = get_folder_entity_ids(conn, folder_id)?;
        if current_ids.len() < 2 {
            return Ok(());
        }
        let reversed: Vec<i64> = current_ids.into_iter().rev().collect();
        let mut stmt = conn.prepare_cached(
            "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
        )?;
        for (i, eid) in reversed.iter().enumerate() {
            let rank = (i as i64 + 1) * RANK_GAP;
            stmt.execute(params![rank, folder_id, eid])?;
        }
    }
    Ok(())
}

pub fn move_folder(
    conn: &Connection,
    folder_id: i64,
    new_parent_id: Option<i64>,
    sibling_order: &[(i64, i64)],
) -> rusqlite::Result<()> {
    if let Some(pid) = new_parent_id {
        let mut current = Some(pid);
        while let Some(cid) = current {
            if cid == folder_id {
                return Err(rusqlite::Error::InvalidParameterName(
                    "Cycle detected: folder cannot be its own ancestor".to_string(),
                ));
            }
            current = conn
                .query_row(
                    "SELECT parent_id FROM folder WHERE folder_id = ?1",
                    [cid],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .optional()?
                .flatten();
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE folder SET parent_id = ?1, updated_at = ?2 WHERE folder_id = ?3",
        params![new_parent_id, now, folder_id],
    )?;
    let mut stmt = conn.prepare_cached("UPDATE folder SET sort_order = ?1 WHERE folder_id = ?2")?;
    for &(fid, new_sort_order) in sibling_order {
        stmt.execute(params![new_sort_order, fid])?;
    }
    Ok(())
}

/// Batch-update sort_order on the canonical folder table.
pub fn reorder_folders(conn: &Connection, moves: &[(i64, i64)]) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached("UPDATE folder SET sort_order = ?1 WHERE folder_id = ?2")?;
    for &(folder_id, new_sort_order) in moves {
        stmt.execute(params![new_sort_order, folder_id])?;
    }
    Ok(())
}

impl SqliteDatabase {
    pub async fn create_folder(&self, f: NewFolder) -> Result<Folder, String> {
        let folder_id = self.with_conn(move |conn| create_folder(conn, &f)).await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        let fid = folder_id;
        self.with_read_conn(move |conn| get_folder(conn, fid))
            .await?
            .ok_or_else(|| "Folder not found after creation".to_string())
    }

    pub async fn list_folders(&self) -> Result<Vec<Folder>, String> {
        self.with_read_conn(list_folders).await
    }

    pub async fn delete_folder(&self, folder_id: i64) -> Result<(), String> {
        self.bitmaps.remove_key(&BitmapKey::Folder(folder_id));
        self.with_conn(move |conn| delete_folder(conn, folder_id))
            .await
    }

    pub async fn add_entity_to_folder(&self, folder_id: i64, hash: &str) -> Result<(), String> {
        let entity_id = self.resolve_hash(hash).await?;
        self.with_conn(move |conn| add_entity_to_folder(conn, folder_id, entity_id))
            .await?;
        self.bitmaps
            .insert(&BitmapKey::Folder(folder_id), entity_id as u32);
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn add_entities_to_folder_batch(
        &self,
        folder_id: i64,
        hashes: &[String],
    ) -> Result<usize, String> {
        if hashes.is_empty() {
            return Ok(0);
        }
        let resolved = self.resolve_hashes_batch(hashes).await?;
        let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
        let eids = entity_ids.clone();
        let inserted = self
            .with_conn(move |conn| add_entities_to_folder_batch(conn, folder_id, &eids))
            .await?;
        for &eid in &entity_ids {
            self.bitmaps
                .insert(&BitmapKey::Folder(folder_id), eid as u32);
        }
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(inserted)
    }

    pub async fn remove_entity_from_folder(
        &self,
        folder_id: i64,
        hash: &str,
    ) -> Result<(), String> {
        let entity_id = self.resolve_hash(hash).await?;
        self.with_conn(move |conn| remove_entity_from_folder(conn, folder_id, entity_id))
            .await?;
        self.bitmaps
            .remove(&BitmapKey::Folder(folder_id), entity_id as u32);
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn remove_entities_from_folder_batch(
        &self,
        folder_id: i64,
        hashes: &[String],
    ) -> Result<usize, String> {
        if hashes.is_empty() {
            return Ok(0);
        }
        let resolved = self.resolve_hashes_batch(hashes).await?;
        let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
        let eids = entity_ids.clone();
        let removed = self
            .with_conn(move |conn| {
                let mut count = 0usize;
                for chunk in eids.chunks(500) {
                    let placeholders: String = (0..chunk.len())
                        .map(|i| format!("?{}", i + 2))
                        .collect::<Vec<_>>()
                        .join(",");
                    let sql = format!(
                        "DELETE FROM folder_entity WHERE folder_id = ?1 AND entity_id IN ({placeholders})"
                    );
                    let mut param_values: Vec<rusqlite::types::Value> =
                        Vec::with_capacity(chunk.len() + 1);
                    param_values.push(rusqlite::types::Value::Integer(folder_id));
                    for &eid in chunk {
                        param_values.push(rusqlite::types::Value::Integer(eid));
                    }
                    let changed = conn.execute(
                        &sql,
                        rusqlite::params_from_iter(param_values.iter()),
                    )?;
                    count += changed;
                }
                Ok(count)
            })
            .await?;
        for &eid in &entity_ids {
            self.bitmaps
                .remove(&BitmapKey::Folder(folder_id), eid as u32);
        }
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(removed)
    }

    pub async fn get_entity_folder_memberships(
        &self,
        hash: &str,
    ) -> Result<Vec<FolderMembership>, String> {
        let entity_id = self.resolve_hash(hash).await?;
        self.with_read_conn(move |conn| get_entity_folder_memberships(conn, entity_id))
            .await
    }

    pub async fn get_entity_folder_memberships_by_entity_id(
        &self,
        entity_id: i64,
    ) -> Result<Vec<FolderMembership>, String> {
        self.with_read_conn(move |conn| get_entity_folder_memberships(conn, entity_id))
            .await
    }

    pub async fn get_folder_cover_hash(&self, folder_id: i64) -> Result<Option<String>, String> {
        self.with_read_conn(move |conn| get_folder_cover_hash(conn, folder_id))
            .await
    }

    pub async fn get_folder_entity_hashes(&self, folder_id: i64) -> Result<Vec<String>, String> {
        let entity_ids = self
            .with_read_conn(move |conn| get_folder_entity_ids(conn, folder_id))
            .await?;
        let resolved = self.resolve_ids_batch(&entity_ids).await?;
        Ok(resolved.into_iter().map(|(_, h)| h).collect())
    }

    pub async fn update_folder_parent(
        &self,
        folder_id: i64,
        new_parent_id: Option<i64>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| update_folder_parent(conn, folder_id, new_parent_id))
            .await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn update_folder(
        &self,
        folder_id: i64,
        name: String,
        icon: Option<String>,
        color: Option<String>,
    ) -> Result<(), String> {
        let n = name;
        let i = icon;
        let c = color;
        self.with_conn(move |conn| update_folder(conn, folder_id, &n, i.as_deref(), c.as_deref()))
            .await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn reorder_folders(&self, moves: Vec<(i64, i64)>) -> Result<(), String> {
        self.with_conn(move |conn| reorder_folders(conn, &moves))
            .await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id: 0 });
        Ok(())
    }

    pub async fn move_folder(
        &self,
        folder_id: i64,
        new_parent_id: Option<i64>,
        sibling_order: Vec<(i64, i64)>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| move_folder(conn, folder_id, new_parent_id, &sibling_order))
            .await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn reorder_entity_in_folder(
        &self,
        folder_id: i64,
        hash: &str,
        prev_hash: Option<&str>,
        next_hash: Option<&str>,
    ) -> Result<(), String> {
        let target_entity_id = self.resolve_hash(hash).await?;
        let prev_entity_id = match prev_hash {
            Some(h) => Some(self.resolve_hash(h).await?),
            None => None,
        };
        let next_entity_id = match next_hash {
            Some(h) => Some(self.resolve_hash(h).await?),
            None => None,
        };

        self.with_conn(move |conn| {
            let prev_rank = match prev_entity_id {
                Some(eid) => get_entity_rank_in_folder(conn, folder_id, eid)?,
                None => None,
            };
            let next_rank = match next_entity_id {
                Some(eid) => get_entity_rank_in_folder(conn, folder_id, eid)?,
                None => None,
            };
            reorder_entity(conn, folder_id, target_entity_id, prev_rank, next_rank)
        })
        .await?;

        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn reorder_folder_items_batch(
        &self,
        folder_id: i64,
        moves: Vec<crate::types::FolderReorderMove>,
    ) -> Result<(), String> {
        if moves.is_empty() {
            return Ok(());
        }
        let mut all_hashes: Vec<String> = Vec::new();
        for mv in &moves {
            all_hashes.push(mv.hash.clone());
            if let Some(ref h) = mv.after_hash {
                all_hashes.push(h.clone());
            }
            if let Some(ref h) = mv.before_hash {
                all_hashes.push(h.clone());
            }
        }
        let resolved = self.resolve_hashes_batch(&all_hashes).await?;
        let hash_to_id: std::collections::HashMap<String, i64> = resolved.into_iter().collect();

        struct ResolvedMove {
            entity_id: i64,
            after_id: Option<i64>,
            before_id: Option<i64>,
        }
        let mut resolved_moves = Vec::with_capacity(moves.len());
        for mv in &moves {
            let entity_id = *hash_to_id
                .get(&mv.hash)
                .ok_or_else(|| format!("Hash not found: {}", mv.hash))?;
            let after_id = match &mv.after_hash {
                Some(h) => Some(
                    *hash_to_id
                        .get(h)
                        .ok_or_else(|| format!("Hash not found: {}", h))?,
                ),
                None => None,
            };
            let before_id = match &mv.before_hash {
                Some(h) => Some(
                    *hash_to_id
                        .get(h)
                        .ok_or_else(|| format!("Hash not found: {}", h))?,
                ),
                None => None,
            };
            resolved_moves.push(ResolvedMove {
                entity_id,
                after_id,
                before_id,
            });
        }

        self.with_conn(move |conn| {
            for rm in &resolved_moves {
                let prev_rank = match rm.after_id {
                    Some(eid) => get_entity_rank_in_folder(conn, folder_id, eid)?,
                    None => None,
                };
                let next_rank = match rm.before_id {
                    Some(eid) => get_entity_rank_in_folder(conn, folder_id, eid)?,
                    None => None,
                };
                reorder_entity(conn, folder_id, rm.entity_id, prev_rank, next_rank)?;
            }
            Ok(())
        })
        .await?;

        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn sort_folder_items(
        &self,
        folder_id: i64,
        sort_by: String,
        direction: String,
        hashes: Option<Vec<String>>,
    ) -> Result<(), String> {
        let entity_ids = match hashes {
            Some(hs) => {
                let mut ids = Vec::with_capacity(hs.len());
                for h in &hs {
                    ids.push(self.resolve_hash(h).await?);
                }
                Some(ids)
            }
            None => None,
        };
        let sb = sort_by;
        let dir = direction;
        self.with_conn(move |conn| {
            sort_folder_items(conn, folder_id, &sb, &dir, entity_ids.as_deref())
        })
        .await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }

    pub async fn reverse_folder_items(
        &self,
        folder_id: i64,
        hashes: Option<Vec<String>>,
    ) -> Result<(), String> {
        let entity_ids = match hashes {
            Some(hs) => {
                let mut ids = Vec::with_capacity(hs.len());
                for h in &hs {
                    ids.push(self.resolve_hash(h).await?);
                }
                Some(ids)
            }
            None => None,
        };
        self.with_conn(move |conn| reverse_folder_items(conn, folder_id, entity_ids.as_deref()))
            .await?;
        self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id });
        Ok(())
    }
}
