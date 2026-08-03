//! Smart folder predicate → bitmap compilation.
//! Smart folder membership is derived, not authoritative.

use std::collections::HashMap;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection};

use super::bitmaps::{BitmapKey, BitmapStore};
use crate::smart_folders::types::{MatchMode, PredicateRule, SmartFolderPredicate, SmartRuleGroup};

const ENTITY_BASE_SQL: &str = "SELECT me.entity_id
    FROM media_entity me
    LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
    LEFT JOIN media_file mf ON mf.file_id = sme.file_id
    LEFT JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
    LEFT JOIN single_media_entity psme ON psme.entity_id = pm.entity_id
    LEFT JOIN media_file pmf ON pmf.file_id = psme.file_id
    WHERE me.parent_collection_entity_id IS NULL";

pub fn compile_smart_folder(conn: &Connection, bitmaps: &BitmapStore, smart_folder_id: i64) {
    let result = build_effective_predicate(conn, smart_folder_id)
        .and_then(|predicate| compile_predicate(conn, &predicate, bitmaps))
        .unwrap_or_else(|error| {
            tracing::warn!(
                smart_folder_id,
                error = %error,
                "Failed to compile smart folder predicate"
            );
            RoaringBitmap::new()
        });

    bitmaps.set(BitmapKey::SmartFolder(smart_folder_id), result);
}

pub fn compile_all_smart_folders(conn: &Connection, bitmaps: &BitmapStore) {
    let sf_ids: Vec<i64> = conn
        .prepare("SELECT smart_folder_id FROM smart_folder")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for sf_id in sf_ids {
        compile_smart_folder(conn, bitmaps, sf_id);
    }
}

pub(crate) fn compile_predicate(
    conn: &Connection,
    pred: &SmartFolderPredicate,
    bitmaps: &BitmapStore,
) -> rusqlite::Result<RoaringBitmap> {
    if pred.groups.is_empty() {
        return Ok(RoaringBitmap::new());
    }

    let active = bitmaps.get(&BitmapKey::Status(1));
    let mut final_result: Option<RoaringBitmap> = None;

    for group in &pred.groups {
        let group_bm = compile_group(conn, group, bitmaps, &active)?;
        final_result = Some(match final_result {
            Some(prev) => prev & &group_bm,
            None => group_bm,
        });
    }

    Ok(final_result.unwrap_or_default() & active)
}

fn build_effective_predicate(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<SmartFolderPredicate> {
    let mut groups = Vec::new();
    let mut current_id = Some(smart_folder_id);
    let mut visited = std::collections::HashSet::new();

    while let Some(id) = current_id {
        if !visited.insert(id) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let folder = crate::db::query::folders::get_smart_folder(conn, id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        current_id = folder.parent_id;
        let pred: SmartFolderPredicate = serde_json::from_str(&folder.predicate_json)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        if pred.groups.iter().any(|group| !group.rules.is_empty()) {
            groups.extend(pred.groups.into_iter().rev());
        }
    }

    groups.reverse();
    Ok(SmartFolderPredicate { groups })
}

fn compile_group(
    conn: &Connection,
    group: &SmartRuleGroup,
    bitmaps: &BitmapStore,
    active: &RoaringBitmap,
) -> rusqlite::Result<RoaringBitmap> {
    let all_tag_pairs: Vec<(String, String)> = group
        .rules
        .iter()
        .filter(|rule| rule.field == "tags")
        .flat_map(|rule| {
            rule.values
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter_map(|tag| crate::tags::normalize::parse_tag(tag))
        })
        .collect();
    let tag_id_map = batch_find_tag_ids(conn, &all_tag_pairs)?;

    let mut include_bitmaps = Vec::new();
    let mut exclude_bitmaps = Vec::new();

    for rule in &group.rules {
        match rule.field.as_str() {
            "tags" => compile_tag_rule(rule, bitmaps, &tag_id_map, &mut include_bitmaps, &mut exclude_bitmaps),
            "rating" => compile_numeric_rule(conn, "me.rating", rule, &mut include_bitmaps)?,
            "file_size" => compile_numeric_rule(
                conn,
                "COALESCE(mf.size_bytes, me.total_size_bytes, 0)",
                rule,
                &mut include_bitmaps,
            )?,
            "width" => compile_numeric_rule(
                conn,
                "COALESCE(mf.pixel_width, pmf.pixel_width)",
                rule,
                &mut include_bitmaps,
            )?,
            "height" => compile_numeric_rule(
                conn,
                "COALESCE(mf.pixel_height, pmf.pixel_height)",
                rule,
                &mut include_bitmaps,
            )?,
            "duration" => compile_numeric_rule(
                conn,
                "COALESCE(mf.duration_ms, pmf.duration_ms)",
                rule,
                &mut include_bitmaps,
            )?,
            "aspect_ratio" => compile_numeric_rule(
                conn,
                "CAST(COALESCE(mf.pixel_width, pmf.pixel_width) AS REAL) / NULLIF(COALESCE(mf.pixel_height, pmf.pixel_height), 0)",
                rule,
                &mut include_bitmaps,
            )?,
            "file_type" => compile_file_type_rule(conn, rule, &mut include_bitmaps, &mut exclude_bitmaps)?,
            "date_imported" | "date_added" => {
                compile_text_comparable_rule(conn, "me.date_added", rule, &mut include_bitmaps)?
            }
            "date_created" => {
                compile_text_comparable_rule(conn, "me.date_created", rule, &mut include_bitmaps)?
            }
            "date_modified" => {
                compile_text_comparable_rule(conn, "me.date_modified", rule, &mut include_bitmaps)?
            }
            "has_audio" => compile_has_audio_rule(conn, rule, &mut include_bitmaps)?,
            "notes" => compile_text_rule(conn, "me.notes", rule, &mut include_bitmaps, &mut exclude_bitmaps)?,
            "name" => compile_text_rule(conn, "me.name", rule, &mut include_bitmaps, &mut exclude_bitmaps)?,
            "source_url" => compile_text_rule(
                conn,
                "me.source_urls_json",
                rule,
                &mut include_bitmaps,
                &mut exclude_bitmaps,
            )?,
            "color" => compile_color_rule(conn, rule, &mut include_bitmaps)?,
            "shape" => compile_shape_rule(conn, rule, &mut include_bitmaps)?,
            field => tracing::warn!(field, "Unknown smart folder rule field"),
        }
    }

    let combined = match group.match_mode {
        MatchMode::All => {
            if include_bitmaps.is_empty() {
                active.clone()
            } else {
                let mut result = include_bitmaps[0].clone();
                for bitmap in &include_bitmaps[1..] {
                    result &= bitmap;
                }
                result
            }
        }
        MatchMode::Any => {
            if include_bitmaps.is_empty() {
                active.clone()
            } else {
                let mut result = RoaringBitmap::new();
                for bitmap in &include_bitmaps {
                    result |= bitmap;
                }
                result
            }
        }
    };

    let mut result = combined & active;
    for exclude in &exclude_bitmaps {
        result -= exclude;
    }

    if group.negate {
        result = active - &result;
    }

    Ok(result)
}

fn compile_tag_rule(
    rule: &PredicateRule,
    bitmaps: &BitmapStore,
    tag_id_map: &HashMap<(String, String), i64>,
    include_bitmaps: &mut Vec<RoaringBitmap>,
    exclude_bitmaps: &mut Vec<RoaringBitmap>,
) {
    let tag_values = rule.values.as_deref().unwrap_or(&[]);
    match rule.op.as_str() {
        "include" | "include_all" => {
            for tag_str in tag_values {
                if let Some(key) = crate::tags::normalize::parse_tag(tag_str) {
                    include_bitmaps.push(
                        tag_id_map
                            .get(&key)
                            .map(|tag_id| bitmaps.get(&BitmapKey::EffectiveTag(*tag_id)))
                            .unwrap_or_default(),
                    );
                } else {
                    include_bitmaps.push(RoaringBitmap::new());
                }
            }
        }
        "include_any" => {
            let mut any_bm = RoaringBitmap::new();
            for tag_str in tag_values {
                if let Some(key) = crate::tags::normalize::parse_tag(tag_str) {
                    if let Some(tag_id) = tag_id_map.get(&key) {
                        any_bm |= &bitmaps.get(&BitmapKey::EffectiveTag(*tag_id));
                    }
                }
            }
            include_bitmaps.push(any_bm);
        }
        "do_not_include" => {
            for tag_str in tag_values {
                if let Some(key) = crate::tags::normalize::parse_tag(tag_str) {
                    if let Some(tag_id) = tag_id_map.get(&key) {
                        exclude_bitmaps.push(bitmaps.get(&BitmapKey::EffectiveTag(*tag_id)));
                    }
                }
            }
        }
        op => tracing::warn!(op, "Unknown smart folder tags op"),
    }
}

fn compile_numeric_rule(
    conn: &Connection,
    expr: &str,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let op = rule.op.as_str();
    if op == "between" {
        let low = rule.value.as_ref().and_then(|value| value.as_f64());
        let high = rule.value2.as_ref().and_then(|value| value.as_f64());
        if let (Some(low), Some(high)) = (low, high) {
            include_bitmaps.push(entity_sql_to_bitmap(
                conn,
                &format!("{ENTITY_BASE_SQL} AND {expr} BETWEEN ?1 AND ?2"),
                params![low, high],
            )?);
        }
        return Ok(());
    }

    let comparator = match op {
        "eq" => Some("="),
        "neq" => Some("!="),
        "gt" => Some(">"),
        "gte" => Some(">="),
        "lt" => Some("<"),
        "lte" => Some("<="),
        _ => None,
    };
    if let (Some(comparator), Some(value)) = (
        comparator,
        rule.value.as_ref().and_then(|value| value.as_f64()),
    ) {
        include_bitmaps.push(entity_sql_to_bitmap(
            conn,
            &format!("{ENTITY_BASE_SQL} AND {expr} {comparator} ?1"),
            params![value],
        )?);
    }
    Ok(())
}

fn compile_text_comparable_rule(
    conn: &Connection,
    expr: &str,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let comparator = match rule.op.as_str() {
        "eq" | "is" => Some("="),
        "neq" | "is_not" => Some("!="),
        "gt" => Some(">"),
        "gte" => Some(">="),
        "lt" => Some("<"),
        "lte" => Some("<="),
        _ => None,
    };

    if let (Some(comparator), Some(value)) = (
        comparator,
        rule.value.as_ref().and_then(|value| value.as_str()),
    ) {
        include_bitmaps.push(entity_sql_to_bitmap(
            conn,
            &format!("{ENTITY_BASE_SQL} AND {expr} {comparator} ?1"),
            params![value],
        )?);
    }
    Ok(())
}

fn compile_text_rule(
    conn: &Connection,
    column: &str,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
    exclude_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let value = rule.value.as_ref().and_then(|raw| raw.as_str());
    match rule.op.as_str() {
        "is" => {
            if let Some(value) = value {
                include_bitmaps.push(entity_sql_to_bitmap(
                    conn,
                    &format!("{ENTITY_BASE_SQL} AND {column} = ?1"),
                    params![value],
                )?);
            }
        }
        "is_not" => {
            if let Some(value) = value {
                exclude_bitmaps.push(entity_sql_to_bitmap(
                    conn,
                    &format!("{ENTITY_BASE_SQL} AND {column} = ?1"),
                    params![value],
                )?);
            }
        }
        "contains" => {
            if let Some(value) = value {
                include_bitmaps.push(entity_sql_to_bitmap(
                    conn,
                    &format!("{ENTITY_BASE_SQL} AND {column} LIKE '%' || ?1 || '%'"),
                    params![value],
                )?);
            }
        }
        "does_not_contain" => {
            if let Some(value) = value {
                exclude_bitmaps.push(entity_sql_to_bitmap(
                    conn,
                    &format!("{ENTITY_BASE_SQL} AND {column} LIKE '%' || ?1 || '%'"),
                    params![value],
                )?);
            }
        }
        "starts_with" => {
            if let Some(value) = value {
                include_bitmaps.push(entity_sql_to_bitmap(
                    conn,
                    &format!("{ENTITY_BASE_SQL} AND {column} LIKE ?1 || '%'"),
                    params![value],
                )?);
            }
        }
        "ends_with" => {
            if let Some(value) = value {
                include_bitmaps.push(entity_sql_to_bitmap(
                    conn,
                    &format!("{ENTITY_BASE_SQL} AND {column} LIKE '%' || ?1"),
                    params![value],
                )?);
            }
        }
        "is_empty" => {
            include_bitmaps.push(entity_sql_to_bitmap(
                conn,
                &format!("{ENTITY_BASE_SQL} AND ({column} IS NULL OR {column} = '')"),
                [],
            )?);
        }
        "is_not_empty" => {
            include_bitmaps.push(entity_sql_to_bitmap(
                conn,
                &format!("{ENTITY_BASE_SQL} AND ({column} IS NOT NULL AND {column} != '')"),
                [],
            )?);
        }
        op => tracing::warn!(op, column, "Unknown smart folder text op"),
    }
    Ok(())
}

fn compile_file_type_rule(
    conn: &Connection,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
    exclude_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let Some(value) = rule.value.as_ref().and_then(|raw| raw.as_str()) else {
        return Ok(());
    };

    let (clause, arg) = match value {
        "image" => (
            "COALESCE(mf.mime_type, pmf.mime_type, '') LIKE ?1",
            "image/%".to_string(),
        ),
        "video" => (
            "COALESCE(mf.mime_type, pmf.mime_type, '') LIKE ?1",
            "video/%".to_string(),
        ),
        "audio" => (
            "COALESCE(mf.mime_type, pmf.mime_type, '') LIKE ?1",
            "audio/%".to_string(),
        ),
        other => (
            "COALESCE(mf.mime_type, pmf.mime_type, '') = ?1",
            other.to_string(),
        ),
    };
    let bitmap = entity_sql_to_bitmap(
        conn,
        &format!("{ENTITY_BASE_SQL} AND {clause}"),
        params![arg],
    )?;

    match rule.op.as_str() {
        "is" => include_bitmaps.push(bitmap),
        "is_not" => exclude_bitmaps.push(bitmap),
        op => tracing::warn!(op, "Unknown smart folder file_type op"),
    }
    Ok(())
}

fn compile_has_audio_rule(
    conn: &Connection,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    if rule.op != "is" {
        tracing::warn!(op = rule.op.as_str(), "Unknown smart folder has_audio op");
        return Ok(());
    }
    let value = match rule.value.as_ref() {
        Some(serde_json::Value::Bool(value)) => Some(if *value { 1_i64 } else { 0_i64 }),
        Some(serde_json::Value::Number(value)) => value.as_i64(),
        _ => None,
    };
    if let Some(value) = value {
        include_bitmaps.push(entity_sql_to_bitmap(
            conn,
            &format!("{ENTITY_BASE_SQL} AND COALESCE(mf.has_audio, pmf.has_audio, 0) = ?1"),
            params![value],
        )?);
    }
    Ok(())
}

fn compile_color_rule(
    conn: &Connection,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    if rule.op != "contains" {
        tracing::warn!(op = rule.op.as_str(), "Unknown smart folder color op");
        return Ok(());
    }
    let color_values = rule.values.as_deref().unwrap_or(&[]);
    if color_values.is_empty() {
        include_bitmaps.push(RoaringBitmap::new());
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", color_values.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "{ENTITY_BASE_SQL} AND EXISTS (
            SELECT 1
            FROM file_color fc
            WHERE fc.file_id IN (COALESCE(mf.file_id, -1), COALESCE(pmf.file_id, -1))
              AND fc.hex IN ({placeholders})
        )"
    );
    let params: Vec<&dyn rusqlite::types::ToSql> = color_values
        .iter()
        .map(|value| value as &dyn rusqlite::types::ToSql)
        .collect();
    include_bitmaps.push(entity_sql_to_bitmap(conn, &sql, params.as_slice())?);
    Ok(())
}

fn compile_shape_rule(
    conn: &Connection,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    if rule.op != "is" {
        tracing::warn!(op = rule.op.as_str(), "Unknown smart folder shape op");
        return Ok(());
    }
    let Some(shape) = rule.value.as_ref().and_then(|raw| raw.as_str()) else {
        return Ok(());
    };
    let dims_w = "COALESCE(mf.pixel_width, pmf.pixel_width)";
    let dims_h = "COALESCE(mf.pixel_height, pmf.pixel_height)";
    let clause = match shape {
        "landscape" => format!("{dims_w} > {dims_h}"),
        "portrait" => format!("{dims_h} > {dims_w}"),
        "square" => format!("{dims_w} = {dims_h}"),
        other => {
            tracing::warn!(shape = other, "Unknown smart folder shape value");
            return Ok(());
        }
    };
    include_bitmaps.push(entity_sql_to_bitmap(
        conn,
        &format!("{ENTITY_BASE_SQL} AND {clause}"),
        [],
    )?);
    Ok(())
}

fn batch_find_tag_ids(
    conn: &Connection,
    tags: &[(String, String)],
) -> rusqlite::Result<HashMap<(String, String), i64>> {
    if tags.is_empty() {
        return Ok(HashMap::new());
    }

    let clauses = (0..tags.len())
        .map(|index| {
            let base = index * 2 + 1;
            format!("(namespace = ?{base} AND subtag = ?{})", base + 1)
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!("SELECT tag_id, namespace, subtag FROM tag WHERE {clauses}");
    let mut flat_params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(tags.len() * 2);
    for (namespace, subtag) in tags {
        flat_params.push(namespace);
        flat_params.push(subtag);
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(flat_params.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut map = HashMap::with_capacity(tags.len());
    for row in rows {
        let (tag_id, namespace, subtag) = row?;
        map.insert((namespace, subtag), tag_id);
    }
    Ok(map)
}

fn entity_sql_to_bitmap(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<RoaringBitmap> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| row.get::<_, i64>(0))?;
    let mut bitmap = RoaringBitmap::new();
    for row in rows {
        bitmap.insert(row? as u32);
    }
    Ok(bitmap)
}

#[cfg(test)]
mod tests {
    use super::compile_predicate;
    use crate::db::core::schema::LIBRARY_DDL;
    use crate::db::projection::bitmaps::{BitmapKey, BitmapStore};
    use crate::smart_folders::types::{
        MatchMode, PredicateRule, SmartFolderPredicate, SmartRuleGroup,
    };
    use roaring::RoaringBitmap;
    use rusqlite::params;

    fn seeded_conn() -> (rusqlite::Connection, BitmapStore) {
        let conn = rusqlite::Connection::open_in_memory().expect("open db");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        let bitmaps = BitmapStore::new();

        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, notes, rating, source_urls_json,
                date_created, date_added, date_modified
            ) VALUES
                (1, 'e1', 'single', 1, 'Landscape', 'alpha', 4, '[\"https://a\"]', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'e2', 'single', 1, 'Portrait', 'beta', 2, '[\"https://b\"]', '2026-04-02', '2026-04-02', '2026-04-02'),
                (3, 'e3', 'single', 2, 'Trash', 'gamma', 5, '[\"https://c\"]', '2026-04-03', '2026-04-03', '2026-04-03')",
            [],
        )
        .expect("insert entities");
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms,
                has_audio, dominant_color_hex, date_added
            ) VALUES
                (1, 'f1', 'image/png', 100, 1000, 500, 0, 0, '#111111', '2026-04-01'),
                (2, 'f2', 'image/jpeg', 200, 400, 800, 0, 1, '#222222', '2026-04-02'),
                (3, 'f3', 'video/mp4', 300, 1920, 1080, 6000, 1, '#333333', '2026-04-03')",
            [],
        )
        .expect("insert files");
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2), (3, 3)",
            [],
        )
        .expect("link files");
        conn.execute("INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'landscape'), (2, 'general', 'portrait')", [])
            .expect("insert tags");
        conn.execute(
            "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source) VALUES
                (1, 1, 1, 'local'),
                (2, 2, 1, 'local')",
            [],
        )
        .expect("insert entity tags");
        conn.execute(
            "INSERT INTO file_color (file_id, hex, l, a, b) VALUES
                (1, '#ff0000', 50.0, 60.0, 70.0),
                (2, '#00ff00', 40.0, 50.0, 60.0)",
            [],
        )
        .expect("insert colors");
        conn.execute(
            "INSERT INTO smart_folder (
                smart_folder_id, name, parent_id, predicate_json, date_added, date_modified
            ) VALUES
                (10, 'Parent', NULL, ?1, '2026-04-01', '2026-04-01'),
                (11, 'Child', 10, ?2, '2026-04-01', '2026-04-01')",
            params![
                serde_json::json!({
                    "groups": [{
                        "match_mode": "all",
                        "negate": false,
                        "rules": [{ "field": "rating", "op": "gte", "value": 4 }]
                    }]
                })
                .to_string(),
                serde_json::json!({
                    "groups": [{
                        "match_mode": "all",
                        "negate": false,
                        "rules": [{ "field": "color", "op": "contains", "values": ["#ff0000"] }]
                    }]
                })
                .to_string(),
            ],
        )
        .expect("insert smart folders");

        bitmaps.set(
            BitmapKey::Status(1),
            RoaringBitmap::from_iter([1_u32, 2_u32]),
        );
        bitmaps.set(BitmapKey::Status(2), RoaringBitmap::from_iter([3_u32]));
        bitmaps.set(
            BitmapKey::EffectiveTag(1),
            RoaringBitmap::from_iter([1_u32]),
        );
        bitmaps.set(
            BitmapKey::EffectiveTag(2),
            RoaringBitmap::from_iter([2_u32]),
        );

        (conn, bitmaps)
    }

    #[test]
    fn compile_predicate_uses_canonical_rule_model() {
        let (conn, bitmaps) = seeded_conn();
        let pred: SmartFolderPredicate = serde_json::from_value(serde_json::json!({
            "groups": [{
                "match_mode": "all",
                "negate": false,
                "rules": [
                    { "field": "rating", "op": "gte", "value": 4 },
                    { "field": "tags", "op": "include_all", "values": ["landscape"] }
                ]
            }]
        }))
        .expect("parse predicate");

        let bitmap = compile_predicate(&conn, &pred, &bitmaps).expect("compile predicate");
        assert_eq!(bitmap, RoaringBitmap::from_iter([1_u32]));
    }

    #[test]
    fn compile_predicate_supports_non_tag_and_negated_rules() {
        let (conn, bitmaps) = seeded_conn();
        let pred = SmartFolderPredicate {
            groups: vec![
                SmartRuleGroup {
                    match_mode: MatchMode::All,
                    negate: false,
                    rules: vec![PredicateRule {
                        field: "date_added".into(),
                        op: "gte".into(),
                        value: Some(serde_json::json!("2026-04-01")),
                        value2: None,
                        values: None,
                    }],
                },
                SmartRuleGroup {
                    match_mode: MatchMode::All,
                    negate: true,
                    rules: vec![PredicateRule {
                        field: "has_audio".into(),
                        op: "is".into(),
                        value: Some(serde_json::json!(true)),
                        value2: None,
                        values: None,
                    }],
                },
            ],
        };

        let bitmap = compile_predicate(&conn, &pred, &bitmaps).expect("compile predicate");
        assert_eq!(bitmap, RoaringBitmap::from_iter([1_u32]));
    }
}
