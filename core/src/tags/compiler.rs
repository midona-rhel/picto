use roaring::RoaringBitmap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::SqliteDatabase;

pub(crate) async fn compile_status_bitmaps(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        for status in 0..=2i64 {
            let mut bitmap = RoaringBitmap::new();
            let mut stmt = conn.prepare_cached(
                "SELECT me.entity_id
                 FROM media_entity me
                 WHERE me.status = ?1
                   AND (
                       me.kind = 'collection'
                       OR me.parent_collection_id IS NULL
                   )",
            )?;
            let rows = stmt.query_map([status], |row| row.get::<_, i64>(0))?;
            for row in rows {
                bitmap.insert(row? as u32);
            }
            bitmaps.set(BitmapKey::Status(status), bitmap);
        }

        Ok(())
    })
    .await?;
    Ok(())
}

pub(crate) async fn compile_tag_bitmap(
    db: &Arc<SqliteDatabase>,
    tag_id: i64,
) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let mut bitmap = RoaringBitmap::new();
        let mut stmt = conn.prepare_cached(
            "SELECT etr.entity_id FROM entity_tag_raw etr
                 JOIN media_entity me ON me.entity_id = etr.entity_id
                 WHERE etr.tag_id = ?1
                   AND (me.kind = 'collection' OR me.parent_collection_id IS NULL)",
        )?;
        let rows = stmt.query_map([tag_id], |row| row.get::<_, i64>(0))?;
        for row in rows {
            bitmap.insert(row? as u32);
        }
        bitmaps.set(BitmapKey::Tag(tag_id), bitmap);
        Ok(())
    })
    .await?;
    Ok(())
}

pub(crate) async fn compile_all_tag_bitmaps(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT etr.tag_id, etr.entity_id FROM entity_tag_raw etr
                 JOIN media_entity me ON me.entity_id = etr.entity_id
                 WHERE me.kind = 'collection' OR me.parent_collection_id IS NULL
                 ORDER BY etr.tag_id",
        )?;
        let mut current_tag: Option<i64> = None;
        let mut current_bitmap = RoaringBitmap::new();

        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;

        for row in rows {
            let (tag_id, entity_id) = row?;
            if current_tag != Some(tag_id) {
                if let Some(previous_tag) = current_tag {
                    bitmaps.set(
                        BitmapKey::Tag(previous_tag),
                        std::mem::take(&mut current_bitmap),
                    );
                }
                current_tag = Some(tag_id);
            }
            current_bitmap.insert(entity_id as u32);
        }

        if let Some(last_tag) = current_tag {
            bitmaps.set(BitmapKey::Tag(last_tag), current_bitmap);
        }

        Ok(())
    })
    .await?;
    Ok(())
}

pub(crate) async fn compile_tag_graph(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_conn(move |conn| {
        conn.execute("DELETE FROM tag_ancestor", [])?;
        conn.execute_batch(
            "INSERT OR IGNORE INTO tag_ancestor (tag_id, ancestor_id, depth)
             WITH RECURSIVE ancestors(tag_id, ancestor_id, depth) AS (
                 SELECT child_tag_id, parent_tag_id, 1
                 FROM tag_implication
                 UNION ALL
                 SELECT a.tag_id, tp.parent_tag_id, a.depth + 1
                 FROM ancestors a
                 JOIN tag_implication tp ON tp.child_tag_id = a.ancestor_id
                 WHERE a.depth < 50
             )
             SELECT tag_id, ancestor_id, depth FROM ancestors",
        )?;

        conn.execute("DELETE FROM tag_display", [])?;
        conn.execute_batch(
            "INSERT OR REPLACE INTO tag_display (tag_id, display_ns, display_st)
             SELECT t.tag_id,
                    COALESCE(st.display_ns, t.namespace),
                    COALESCE(st.display_st, t.subtag)
             FROM tag t
             LEFT JOIN (
                 SELECT ts.from_tag_id,
                        t2.namespace AS display_ns,
                        t2.subtag AS display_st
                 FROM tag_alias ts
                 JOIN tag t2 ON t2.tag_id = ts.to_tag_id
             ) st ON st.from_tag_id = t.tag_id",
        )?;

        conn.execute("DELETE FROM entity_tag_implied", [])?;
        conn.execute_batch(
            "INSERT OR IGNORE INTO entity_tag_implied (entity_id, tag_id)
             SELECT etr.entity_id, ta.ancestor_id
             FROM entity_tag_raw etr
             JOIN tag_ancestor ta ON ta.tag_id = etr.tag_id",
        )?;

        let mut stmt = conn
            .prepare_cached("SELECT tag_id, entity_id FROM entity_tag_implied ORDER BY tag_id")?;
        let mut current_tag: Option<i64> = None;
        let mut current_bitmap = RoaringBitmap::new();

        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;

        for row in rows {
            let (tag_id, entity_id) = row?;
            if current_tag != Some(tag_id) {
                if let Some(previous_tag) = current_tag {
                    bitmaps.set(
                        BitmapKey::ImpliedTag(previous_tag),
                        std::mem::take(&mut current_bitmap),
                    );
                }
                current_tag = Some(tag_id);
            }
            current_bitmap.insert(entity_id as u32);
        }

        if let Some(last_tag) = current_tag {
            bitmaps.set(BitmapKey::ImpliedTag(last_tag), current_bitmap);
        }

        Ok(())
    })
    .await?;
    Ok(())
}

pub(crate) async fn compile_effective_tags(
    db: &Arc<SqliteDatabase>,
    dirty_tag_ids: &HashSet<i64>,
    rebuild_all: bool,
) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();

    if rebuild_all {
        let tag_ids: Vec<i64> = db
            .with_read_conn(|conn| {
                let mut stmt = conn.prepare_cached("SELECT tag_id FROM tag")?;
                let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
                rows.collect()
            })
            .await?;

        for tag_id in tag_ids {
            let direct = bitmaps.get(&BitmapKey::Tag(tag_id));
            let implied = bitmaps.get(&BitmapKey::ImpliedTag(tag_id));
            bitmaps.set(BitmapKey::EffectiveTag(tag_id), &direct | &implied);
        }
    } else {
        for &tag_id in dirty_tag_ids {
            let direct = bitmaps.get(&BitmapKey::Tag(tag_id));
            let implied = bitmaps.get(&BitmapKey::ImpliedTag(tag_id));
            bitmaps.set(BitmapKey::EffectiveTag(tag_id), &direct | &implied);
        }
    }

    Ok(())
}

pub(crate) async fn compile_tagged_bitmap(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let mut tagged = RoaringBitmap::new();

        let mut stmt = conn.prepare_cached("SELECT DISTINCT entity_id FROM entity_tag_raw")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        for row in rows {
            tagged.insert(row? as u32);
        }

        let mut stmt = conn.prepare_cached("SELECT DISTINCT entity_id FROM entity_tag_implied")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        for row in rows {
            tagged.insert(row? as u32);
        }

        tagged &= &bitmaps.get(&BitmapKey::Status(1));
        bitmaps.set(BitmapKey::Tagged, tagged);
        Ok(())
    })
    .await?;
    Ok(())
}
