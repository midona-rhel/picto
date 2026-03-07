//! Duplicate pair CRUD (phash-based).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicatePair {
    pub file_id_a: i64,
    pub file_id_b: i64,
    pub distance: f64,
    pub status: String,
}

pub fn insert_duplicate(
    conn: &Connection,
    file_id_a: i64,
    file_id_b: i64,
    distance: f64,
) -> rusqlite::Result<()> {
    let _ = insert_duplicate_counted(conn, file_id_a, file_id_b, distance)?;
    Ok(())
}

pub fn insert_duplicate_counted(
    conn: &Connection,
    file_id_a: i64,
    file_id_b: i64,
    distance: f64,
) -> rusqlite::Result<bool> {
    // Enforce a < b ordering
    let (a, b) = if file_id_a < file_id_b {
        (file_id_a, file_id_b)
    } else {
        (file_id_b, file_id_a)
    };
    let changed = conn.execute(
        "INSERT OR IGNORE INTO duplicate (file_id_a, file_id_b, distance) VALUES (?1, ?2, ?3)",
        params![a, b, distance],
    )?;
    Ok(changed > 0)
}

pub fn get_duplicates_for_file(
    conn: &Connection,
    file_id: i64,
) -> rusqlite::Result<Vec<DuplicatePair>> {
    let mut stmt = conn.prepare_cached(
        "SELECT file_id_a, file_id_b, distance, status FROM duplicate
         WHERE (file_id_a = ?1 OR file_id_b = ?1) AND status = 'detected'",
    )?;
    let rows = stmt.query_map([file_id], |row| {
        Ok(DuplicatePair {
            file_id_a: row.get(0)?,
            file_id_b: row.get(1)?,
            distance: row.get(2)?,
            status: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn get_all_detected_duplicates(conn: &Connection) -> rusqlite::Result<Vec<DuplicatePair>> {
    let mut stmt = conn.prepare_cached(
        "SELECT file_id_a, file_id_b, distance, status FROM duplicate
         WHERE status = 'detected' ORDER BY distance ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DuplicatePair {
            file_id_a: row.get(0)?,
            file_id_b: row.get(1)?,
            distance: row.get(2)?,
            status: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Keyset-paginated query for duplicate pairs.
/// Cursor format: "distance,file_id_a,file_id_b" (or None for first page).
pub fn get_duplicate_pairs_paginated(
    conn: &Connection,
    cursor: Option<&str>,
    limit: usize,
    status_filter: &str,
    max_distance: Option<f64>,
) -> rusqlite::Result<(Vec<DuplicatePair>, Option<String>, i64)> {
    let total: i64 = if let Some(max_dist) = max_distance {
        conn.query_row(
            "SELECT COUNT(*) FROM duplicate WHERE status = ?1 AND distance <= ?2",
            params![status_filter, max_dist],
            |row| row.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM duplicate WHERE status = ?1",
            [status_filter],
            |row| row.get(0),
        )?
    };

    let pairs = if let Some(cursor_str) = cursor {
        // Parse cursor: "distance,file_id_a,file_id_b"
        let parts: Vec<&str> = cursor_str.split(',').collect();
        if parts.len() != 3 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Invalid cursor format".into(),
            ));
        }
        let c_dist: f64 = parts[0].parse().unwrap_or(0.0);
        let c_a: i64 = parts[1].parse().unwrap_or(0);
        let c_b: i64 = parts[2].parse().unwrap_or(0);

        if let Some(max_dist) = max_distance {
            let mut stmt = conn.prepare_cached(
                "SELECT file_id_a, file_id_b, distance, status FROM duplicate
                 WHERE status = ?1
                   AND distance <= ?2
                   AND (distance > ?3
                        OR (distance = ?3 AND file_id_a > ?4)
                        OR (distance = ?3 AND file_id_a = ?4 AND file_id_b > ?5))
                 ORDER BY distance ASC, file_id_a ASC, file_id_b ASC
                 LIMIT ?6",
            )?;
            let rows = stmt.query_map(
                params![status_filter, max_dist, c_dist, c_a, c_b, limit as i64],
                |row| {
                    Ok(DuplicatePair {
                        file_id_a: row.get(0)?,
                        file_id_b: row.get(1)?,
                        distance: row.get(2)?,
                        status: row.get(3)?,
                    })
                },
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT file_id_a, file_id_b, distance, status FROM duplicate
                 WHERE status = ?1
                   AND (distance > ?2
                        OR (distance = ?2 AND file_id_a > ?3)
                        OR (distance = ?2 AND file_id_a = ?3 AND file_id_b > ?4))
                 ORDER BY distance ASC, file_id_a ASC, file_id_b ASC
                 LIMIT ?5",
            )?;
            let rows = stmt.query_map(
                params![status_filter, c_dist, c_a, c_b, limit as i64],
                |row| {
                    Ok(DuplicatePair {
                        file_id_a: row.get(0)?,
                        file_id_b: row.get(1)?,
                        distance: row.get(2)?,
                        status: row.get(3)?,
                    })
                },
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        }
    } else {
        if let Some(max_dist) = max_distance {
            let mut stmt = conn.prepare_cached(
                "SELECT file_id_a, file_id_b, distance, status FROM duplicate
                 WHERE status = ?1 AND distance <= ?2
                 ORDER BY distance ASC, file_id_a ASC, file_id_b ASC
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![status_filter, max_dist, limit as i64], |row| {
                Ok(DuplicatePair {
                    file_id_a: row.get(0)?,
                    file_id_b: row.get(1)?,
                    distance: row.get(2)?,
                    status: row.get(3)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT file_id_a, file_id_b, distance, status FROM duplicate
                 WHERE status = ?1
                 ORDER BY distance ASC, file_id_a ASC, file_id_b ASC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![status_filter, limit as i64], |row| {
                Ok(DuplicatePair {
                    file_id_a: row.get(0)?,
                    file_id_b: row.get(1)?,
                    distance: row.get(2)?,
                    status: row.get(3)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        }
    };

    let next_cursor = if pairs.len() == limit {
        pairs
            .last()
            .map(|p| format!("{},{},{}", p.distance, p.file_id_a, p.file_id_b))
    } else {
        None
    };

    Ok((pairs, next_cursor, total))
}

pub fn count_by_status(conn: &Connection, status: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM duplicate WHERE status = ?1",
        [status],
        |row| row.get(0),
    )
}

pub fn count_by_status_with_max_distance(
    conn: &Connection,
    status: &str,
    max_distance: f64,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM duplicate WHERE status = ?1 AND distance <= ?2",
        params![status, max_distance],
        |row| row.get(0),
    )
}

/// Resolve a pair with full decision metadata (V7 columns).
pub fn resolve_pair_with_decision(
    conn: &Connection,
    file_id_a: i64,
    file_id_b: i64,
    status: &str,
    decision_source: &str,
    decision_reason: &str,
    winner_file_id: Option<i64>,
    loser_file_id: Option<i64>,
) -> rusqlite::Result<()> {
    let (a, b) = if file_id_a < file_id_b {
        (file_id_a, file_id_b)
    } else {
        (file_id_b, file_id_a)
    };
    conn.execute(
        "UPDATE duplicate SET status = ?1, decision_at = datetime('now'),
                decision_source = ?2, decision_reason = ?3,
                winner_file_id = ?4, loser_file_id = ?5
         WHERE file_id_a = ?6 AND file_id_b = ?7",
        params![
            status,
            decision_source,
            decision_reason,
            winner_file_id,
            loser_file_id,
            a,
            b
        ],
    )?;
    Ok(())
}

impl SqliteDatabase {
    pub async fn insert_duplicate(
        &self,
        hash_a: &str,
        hash_b: &str,
        distance: f64,
    ) -> Result<(), String> {
        let id_a = self.resolve_hash(hash_a).await?;
        let id_b = self.resolve_hash(hash_b).await?;
        self.with_conn(move |conn| insert_duplicate(conn, id_a, id_b, distance))
            .await
    }

    pub async fn get_duplicates_for_hash(&self, hash: &str) -> Result<Vec<(String, f64)>, String> {
        let file_id = self.resolve_hash(hash).await?;
        let pairs = self
            .with_read_conn(move |conn| get_duplicates_for_file(conn, file_id))
            .await?;

        let other_ids: Vec<i64> = pairs
            .iter()
            .map(|p| {
                if p.file_id_a == file_id {
                    p.file_id_b
                } else {
                    p.file_id_a
                }
            })
            .collect();
        let resolved = self.resolve_ids_batch(&other_ids).await?;
        let id_to_hash: std::collections::HashMap<i64, String> = resolved.into_iter().collect();

        let result = pairs
            .iter()
            .filter_map(|pair| {
                let other_id = if pair.file_id_a == file_id {
                    pair.file_id_b
                } else {
                    pair.file_id_a
                };
                let h = id_to_hash.get(&other_id)?.clone();
                Some((h, pair.distance))
            })
            .collect();
        Ok(result)
    }
}
