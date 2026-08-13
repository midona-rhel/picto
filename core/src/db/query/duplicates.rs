use rusqlite::{params, Connection};

use crate::db::types::{DuplicatePairPage, DuplicatePairRecord};

#[derive(Debug, Clone)]
pub struct DuplicateScanSource {
    pub file_id: i64,
    pub entity_hash: String,
    pub perceptual_hash: String,
    pub mime_type: String,
    pub frame_count: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PerceptualHashSource {
    pub file_id: i64,
    pub mime_type: String,
    pub frame_count: Option<i64>,
    pub perceptual_hash: String,
}

#[derive(Debug, Clone)]
pub struct DuplicateSingleRef {
    pub entity_id: i64,
    pub file_id: i64,
    pub entity_hash: String,
    pub file_hash: String,
    pub status: i64,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub frame_count: Option<i64>,
    pub notes: Option<String>,
    pub source_urls_json: Option<String>,
    pub rating: Option<i64>,
    pub date_created: String,
    pub parent_collection_entity_id: Option<i64>,
    pub collection_ordinal: Option<i64>,
}

pub fn list_perceptual_hash_sources(
    conn: &Connection,
) -> rusqlite::Result<Vec<PerceptualHashSource>> {
    let mut stmt = conn.prepare(
        "SELECT mf.file_id, mf.mime_type, mf.frame_count, mf.perceptual_hash
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE mf.perceptual_hash IS NOT NULL
           AND me.status IN (0, 1)",
    )?;
    let rows = stmt.query_map([], map_perceptual_hash_source)?;
    rows.collect()
}

/// Return a superset of pHash matches using the durable eight-way partition
/// index. Callers must verify the full Hamming distance before acting.
pub fn list_indexed_perceptual_hash_sources(
    conn: &Connection,
    partitions: &[i64; 8],
) -> rusqlite::Result<Vec<PerceptualHashSource>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             mf.file_id,
             mf.mime_type,
             mf.frame_count,
             mf.perceptual_hash
         FROM media_file_phash_index idx
         JOIN media_file mf ON mf.file_id = idx.file_id
         JOIN single_media_entity sme ON sme.file_id = mf.file_id
         JOIN media_entity me ON me.entity_id = sme.entity_id
         WHERE me.status IN (0, 1)
           AND (
                idx.partition_0 = ?1 OR idx.partition_1 = ?2
             OR idx.partition_2 = ?3 OR idx.partition_3 = ?4
             OR idx.partition_4 = ?5 OR idx.partition_5 = ?6
             OR idx.partition_6 = ?7 OR idx.partition_7 = ?8
           )",
    )?;
    let rows = stmt.query_map(
        params![
            partitions[0],
            partitions[1],
            partitions[2],
            partitions[3],
            partitions[4],
            partitions[5],
            partitions[6],
            partitions[7],
        ],
        map_perceptual_hash_source,
    )?;
    rows.collect()
}

fn map_perceptual_hash_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<PerceptualHashSource> {
    Ok(PerceptualHashSource {
        file_id: row.get(0)?,
        mime_type: row.get(1)?,
        frame_count: row.get(2)?,
        perceptual_hash: row.get(3)?,
    })
}

pub fn list_duplicate_scan_sources(
    conn: &Connection,
) -> rusqlite::Result<Vec<DuplicateScanSource>> {
    let mut stmt = conn.prepare(
        "SELECT mf.file_id, me.entity_hash, mf.perceptual_hash, mf.mime_type, mf.frame_count
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE mf.perceptual_hash IS NOT NULL
           AND me.status IN (0, 1)",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DuplicateScanSource {
            file_id: row.get(0)?,
            entity_hash: row.get(1)?,
            perceptual_hash: row.get(2)?,
            mime_type: row.get(3)?,
            frame_count: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn count_duplicate_pairs(conn: &Connection, status: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM duplicate d
         JOIN single_media_entity sme_a ON sme_a.file_id = d.file_id_a
         JOIN media_entity me_a ON me_a.entity_id = sme_a.entity_id
         JOIN single_media_entity sme_b ON sme_b.file_id = d.file_id_b
         JOIN media_entity me_b ON me_b.entity_id = sme_b.entity_id
         WHERE d.status = ?1
           AND me_a.status IN (0, 1)
           AND me_b.status IN (0, 1)",
        [status],
        |row| row.get(0),
    )
}

pub fn count_duplicate_pairs_with_max_distance(
    conn: &Connection,
    status: &str,
    max_distance: i64,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM duplicate d
         JOIN single_media_entity sme_a ON sme_a.file_id = d.file_id_a
         JOIN media_entity me_a ON me_a.entity_id = sme_a.entity_id
         JOIN single_media_entity sme_b ON sme_b.file_id = d.file_id_b
         JOIN media_entity me_b ON me_b.entity_id = sme_b.entity_id
         WHERE d.status = ?1
           AND d.distance <= ?2
           AND me_a.status IN (0, 1)
           AND me_b.status IN (0, 1)",
        params![status, max_distance],
        |row| row.get(0),
    )
}

pub fn get_duplicate_pairs_paginated(
    conn: &Connection,
    cursor: Option<&str>,
    limit: usize,
    status: &str,
    max_distance: Option<f64>,
) -> rusqlite::Result<DuplicatePairPage> {
    let total: i64 = if let Some(max_distance) = max_distance {
        count_duplicate_pairs_with_max_distance(conn, status, max_distance as i64)?
    } else {
        count_duplicate_pairs(conn, status)?
    };

    let mut params_vec: Vec<rusqlite::types::Value> = vec![status.to_string().into()];
    let cursor_clause = if let Some(cursor) = cursor {
        let parts: Vec<&str> = cursor.split(',').collect();
        if parts.len() == 3 {
            let distance = parts[0].parse::<i64>().unwrap_or(0);
            let file_id_a = parts[1].parse::<i64>().unwrap_or(0);
            let file_id_b = parts[2].parse::<i64>().unwrap_or(0);
            params_vec.push(distance.into());
            params_vec.push(file_id_a.into());
            params_vec.push(file_id_b.into());
            " AND (d.distance > ?2 OR (d.distance = ?2 AND d.file_id_a > ?3) OR (d.distance = ?2 AND d.file_id_a = ?3 AND d.file_id_b > ?4))"
        } else {
            ""
        }
    } else {
        ""
    };

    let distance_clause = if let Some(max_distance) = max_distance {
        if cursor_clause.is_empty() {
            params_vec.push((max_distance as i64).into());
            " AND d.distance <= ?2"
        } else {
            params_vec.push((max_distance as i64).into());
            " AND d.distance <= ?5"
        }
    } else {
        ""
    };

    params_vec.push((limit as i64).into());
    let limit_param = format!("?{}", params_vec.len());
    let sql = format!(
        "SELECT
             me_a.entity_hash,
             me_b.entity_hash,
             d.distance,
             d.status,
             d.file_id_a,
             d.file_id_b
         FROM duplicate d
         JOIN single_media_entity sme_a ON sme_a.file_id = d.file_id_a
         JOIN media_entity me_a ON me_a.entity_id = sme_a.entity_id
         JOIN single_media_entity sme_b ON sme_b.file_id = d.file_id_b
         JOIN media_entity me_b ON me_b.entity_id = sme_b.entity_id
         WHERE d.status = ?1
           AND me_a.status IN (0, 1)
           AND me_b.status IN (0, 1){cursor_clause}{distance_clause}
         ORDER BY d.distance ASC, d.file_id_a ASC, d.file_id_b ASC
         LIMIT {limit_param}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
        let distance: i64 = row.get(2)?;
        Ok((
            DuplicatePairRecord {
                hash_a: row.get(0)?,
                hash_b: row.get(1)?,
                distance: distance as f64,
                similarity_pct: ((1.0 - distance as f64 / 256.0) * 100.0).round(),
                status: row.get(3)?,
            },
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            distance,
        ))
    })?;
    let rows: Vec<_> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let next_cursor = if rows.len() == limit {
        rows.last().map(|(_, file_id_a, file_id_b, distance)| {
            format!("{distance},{file_id_a},{file_id_b}")
        })
    } else {
        None
    };
    Ok(DuplicatePairPage {
        items: rows.into_iter().map(|(row, _, _, _)| row).collect(),
        next_cursor: next_cursor.clone(),
        has_more: next_cursor.is_some(),
        total,
    })
}

pub fn get_duplicate_single_ref_by_hash(
    conn: &Connection,
    entity_hash: &str,
) -> rusqlite::Result<DuplicateSingleRef> {
    conn.query_row(
        "SELECT
             me.entity_id,
             sme.file_id,
             me.entity_hash,
             mf.file_hash,
             me.status,
             mf.mime_type,
             mf.size_bytes,
             mf.pixel_width,
             mf.pixel_height,
             mf.frame_count,
             me.notes,
             me.source_urls_json,
             me.rating,
             me.date_created,
             me.parent_collection_entity_id,
             me.collection_ordinal
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE me.entity_hash = ?1",
        [entity_hash],
        |row| {
            Ok(DuplicateSingleRef {
                entity_id: row.get(0)?,
                file_id: row.get(1)?,
                entity_hash: row.get(2)?,
                file_hash: row.get(3)?,
                status: row.get(4)?,
                mime_type: row.get(5)?,
                size_bytes: row.get(6)?,
                pixel_width: row.get(7)?,
                pixel_height: row.get(8)?,
                frame_count: row.get(9)?,
                notes: row.get(10)?,
                source_urls_json: row.get(11)?,
                rating: row.get(12)?,
                date_created: row.get(13)?,
                parent_collection_entity_id: row.get(14)?,
                collection_ordinal: row.get(15)?,
            })
        },
    )
}
