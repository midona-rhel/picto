//! Tag and status bitmap compilation.
//! Rebuilds Status, Tag, ImpliedTag, EffectiveTag, Tagged, and
//! CollectionMember bitmaps from authoritative tables.

use std::collections::HashMap;

use roaring::RoaringBitmap;
use rusqlite::Connection;

use super::bitmaps::{BitmapKey, BitmapStore};

/// Rebuild all status bitmaps from authoritative tables.
/// Excludes collection members from status bitmaps (they are not
/// top-level entities visible in system scopes).
pub fn compile_status_bitmaps(conn: &Connection, bitmaps: &BitmapStore) {
    for status in 0..=2i64 {
        let mut bitmap = RoaringBitmap::new();
        if let Ok(mut stmt) = conn.prepare_cached(
            "SELECT entity_id FROM media_entity
             WHERE status = ?1
               AND (entity_kind = 'collection' OR parent_collection_entity_id IS NULL)",
        ) {
            if let Ok(rows) = stmt.query_map([status], |row| row.get::<_, i64>(0)) {
                for row in rows.flatten() {
                    bitmap.insert(row as u32);
                }
            }
        }
        bitmaps.set(BitmapKey::Status(status), bitmap);
    }

    // CollectionMember bitmap
    let mut members = RoaringBitmap::new();
    if let Ok(mut stmt) = conn.prepare_cached(
        "SELECT entity_id FROM media_entity WHERE parent_collection_entity_id IS NOT NULL",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) {
            for row in rows.flatten() {
                members.insert(row as u32);
            }
        }
    }
    bitmaps.set(BitmapKey::CollectionMember, members);
}

/// Rebuild a single tag bitmap.
pub fn compile_tag_bitmap(conn: &Connection, bitmaps: &BitmapStore, tag_id: i64) {
    let mut bitmap = RoaringBitmap::new();
    if let Ok(mut stmt) = conn.prepare_cached("SELECT entity_id FROM entity_tag WHERE tag_id = ?1")
    {
        if let Ok(rows) = stmt.query_map([tag_id], |row| row.get::<_, i64>(0)) {
            for row in rows.flatten() {
                bitmap.insert(row as u32);
            }
        }
    }
    bitmaps.set(BitmapKey::Tag(tag_id), bitmap);
}

/// Rebuild all tag bitmaps in one pass.
pub fn compile_all_tag_bitmaps(conn: &Connection, bitmaps: &BitmapStore) {
    // Get all tag_ids with entries
    let tag_ids: Vec<i64> = conn
        .prepare("SELECT DISTINCT tag_id FROM entity_tag")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for tag_id in tag_ids {
        compile_tag_bitmap(conn, bitmaps, tag_id);
    }
}

/// Rebuild the Tagged bitmap (union of all EffectiveTag bitmaps).
pub fn compile_tagged_bitmap(conn: &Connection, bitmaps: &BitmapStore) {
    let mut tagged = RoaringBitmap::new();
    if let Ok(mut stmt) = conn.prepare_cached(
        "SELECT DISTINCT entity_id FROM entity_tag
         UNION
         SELECT DISTINCT entity_id FROM entity_tag_implied",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) {
            for row in rows.flatten() {
                tagged.insert(row as u32);
            }
        }
    }
    bitmaps.set(BitmapKey::Tagged, tagged);
}

/// Rebuild implied tag bitmaps from the tag_implication graph.
pub fn compile_implied_tags(conn: &Connection, bitmaps: &BitmapStore) {
    // Rebuild tag_ancestor closure table
    let _ = conn.execute_batch(
        "DELETE FROM tag_ancestor;
         INSERT INTO tag_ancestor (tag_id, ancestor_id, depth)
         WITH RECURSIVE tc(tag_id, ancestor_id, depth) AS (
             SELECT child_tag_id, parent_tag_id, 1 FROM tag_implication
             UNION ALL
             SELECT tc.tag_id, ti.parent_tag_id, tc.depth + 1
             FROM tc
             JOIN tag_implication ti ON ti.child_tag_id = tc.ancestor_id
             WHERE tc.depth < 20
         )
         SELECT tag_id, ancestor_id, MIN(depth) FROM tc GROUP BY tag_id, ancestor_id;",
    );

    // Rebuild entity_tag_implied
    let _ = conn.execute_batch(
        "DELETE FROM entity_tag_implied;
         INSERT OR IGNORE INTO entity_tag_implied (entity_id, tag_id)
         SELECT et.entity_id, ta.ancestor_id
         FROM entity_tag et
         JOIN tag_ancestor ta ON ta.tag_id = et.tag_id
         UNION
         SELECT et.entity_id, alias.to_tag_id
         FROM entity_tag et
         JOIN tag_alias alias ON alias.from_tag_id = et.tag_id
         UNION
         SELECT et.entity_id, ta.ancestor_id
         FROM entity_tag et
         JOIN tag_alias alias ON alias.from_tag_id = et.tag_id
         JOIN tag_ancestor ta ON ta.tag_id = alias.to_tag_id;",
    );

    // Build ImpliedTag bitmaps
    let implied_tag_ids: Vec<i64> = conn
        .prepare("SELECT DISTINCT tag_id FROM entity_tag_implied")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for tag_id in &implied_tag_ids {
        let mut bitmap = RoaringBitmap::new();
        if let Ok(mut stmt) =
            conn.prepare_cached("SELECT entity_id FROM entity_tag_implied WHERE tag_id = ?1")
        {
            if let Ok(rows) = stmt.query_map([tag_id], |row| row.get::<_, i64>(0)) {
                for row in rows.flatten() {
                    bitmap.insert(row as u32);
                }
            }
        }
        bitmaps.set(BitmapKey::ImpliedTag(*tag_id), bitmap);
    }
}

/// Rebuild effective tag membership for entities and their visible collection aggregates.
pub fn compile_effective_tag_bitmaps(conn: &Connection, bitmaps: &BitmapStore) {
    let sql = "WITH membership(entity_id, tag_id) AS (
                   SELECT entity_id, tag_id FROM entity_tag
                   UNION
                   SELECT entity_id, tag_id FROM entity_tag_implied
               ), equivalent(requested_tag_id, entity_id) AS (
                   SELECT tag_id, entity_id FROM membership
                   UNION
                   SELECT alias.from_tag_id, membership.entity_id
                   FROM membership
                   JOIN tag_alias alias ON alias.to_tag_id = membership.tag_id
                   UNION
                   SELECT alias.to_tag_id, membership.entity_id
                   FROM membership
                   JOIN tag_alias alias ON alias.from_tag_id = membership.tag_id
                   UNION
                   SELECT requested_alias.from_tag_id, membership.entity_id
                   FROM membership
                   JOIN tag_alias member_alias ON member_alias.from_tag_id = membership.tag_id
                   JOIN tag_alias requested_alias
                     ON requested_alias.to_tag_id = member_alias.to_tag_id
               )
               SELECT equivalent.requested_tag_id,
                      equivalent.entity_id,
                      me.parent_collection_entity_id
               FROM equivalent
               JOIN media_entity me ON me.entity_id = equivalent.entity_id";

    let result = (|| -> rusqlite::Result<HashMap<i64, RoaringBitmap>> {
        let mut effective = HashMap::new();
        let mut tag_stmt = conn.prepare("SELECT tag_id FROM tag")?;
        let tag_ids = tag_stmt.query_map([], |row| row.get::<_, i64>(0))?;
        for tag_id in tag_ids {
            effective.insert(tag_id?, RoaringBitmap::new());
        }

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (tag_id, entity_id, parent_id) = row?;
            let bitmap = effective.entry(tag_id).or_default();
            bitmap.insert(entity_id as u32);
            if let Some(parent_id) = parent_id {
                bitmap.insert(parent_id as u32);
            }
        }
        Ok(effective)
    })();

    let effective = match result {
        Ok(effective) => effective,
        Err(error) => {
            tracing::warn!(%error, "Failed to rebuild effective tag bitmaps");
            return;
        }
    };

    for (tag_id, bitmap) in effective {
        bitmaps.set(BitmapKey::EffectiveTag(tag_id), bitmap);
    }
}
