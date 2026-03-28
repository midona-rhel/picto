//! Smart folder CRUD + predicate → bitmap compilation.

use std::collections::HashSet;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::sqlite::bitmaps::{BitmapKey, BitmapStore};
use crate::sqlite::ReadModelEvent;
use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFolder {
    pub smart_folder_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub predicate_json: String,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub display_order: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Predicate system for smart folders — groups-based format from the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SmartFolderPredicate {
    pub groups: Vec<SmartRuleGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SmartRuleGroup {
    pub match_mode: MatchMode,
    #[serde(default)]
    pub negate: bool,
    pub rules: Vec<PredicateRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    All,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct PredicateRule {
    pub field: String,
    pub op: String,
    #[serde(default)]
    #[ts(type = "any")]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    #[ts(type = "any")]
    pub value2: Option<serde_json::Value>,
    #[serde(default)]
    pub values: Option<Vec<String>>,
}

pub fn create_smart_folder(
    conn: &Connection,
    name: &str,
    parent_id: Option<i64>,
    predicate_json: &str,
    icon: Option<&str>,
    color: Option<&str>,
    notes: Option<&str>,
    sort_field: Option<&str>,
    sort_order: Option<&str>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO smart_folder (name, parent_id, predicate_json, icon, color, notes, sort_field, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            name,
            parent_id,
            predicate_json,
            icon,
            color,
            notes,
            sort_field,
            sort_order,
            now,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<Option<SmartFolder>> {
    conn.query_row(
        "SELECT smart_folder_id, name, parent_id, icon, color, notes, predicate_json, sort_field, sort_order, display_order, created_at, updated_at
         FROM smart_folder WHERE smart_folder_id = ?1",
        [smart_folder_id],
        |row| {
            Ok(SmartFolder {
                smart_folder_id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                notes: row.get(5)?,
                predicate_json: row.get(6)?,
                sort_field: row.get(7)?,
                sort_order: row.get(8)?,
                display_order: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        },
    )
    .optional()
}

pub fn list_smart_folders(conn: &Connection) -> rusqlite::Result<Vec<SmartFolder>> {
    let mut stmt = conn.prepare_cached(
        "SELECT smart_folder_id, name, parent_id, icon, color, notes, predicate_json, sort_field, sort_order, display_order, created_at, updated_at
         FROM smart_folder
         ORDER BY COALESCE(parent_id, 0), COALESCE(display_order, smart_folder_id), smart_folder_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SmartFolder {
            smart_folder_id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            icon: row.get(3)?,
            color: row.get(4)?,
            notes: row.get(5)?,
            predicate_json: row.get(6)?,
            sort_field: row.get(7)?,
            sort_order: row.get(8)?,
            display_order: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    })?;
    rows.collect()
}

pub fn update_smart_folder(conn: &Connection, sf: &SmartFolder) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE smart_folder SET name = ?1, parent_id = ?2, predicate_json = ?3, icon = ?4, color = ?5,
         notes = ?6, sort_field = ?7, sort_order = ?8, updated_at = ?9
         WHERE smart_folder_id = ?10",
        params![
            sf.name,
            sf.parent_id,
            sf.predicate_json,
            sf.icon,
            sf.color,
            sf.notes,
            sf.sort_field,
            sf.sort_order,
            now,
            sf.smart_folder_id
        ],
    )?;
    Ok(())
}

pub fn move_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
    new_parent_id: Option<i64>,
    sibling_order: &[(i64, i64)],
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE smart_folder SET parent_id = ?1 WHERE smart_folder_id = ?2",
        params![new_parent_id, smart_folder_id],
    )?;
    if sibling_order.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "UPDATE smart_folder SET display_order = ?1, parent_id = ?2 WHERE smart_folder_id = ?3",
    )?;
    for &(id, order) in sibling_order {
        stmt.execute(params![order, new_parent_id, id])?;
    }
    Ok(())
}

pub fn collect_descendant_smart_folder_ids(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut descendants = Vec::new();
    let mut stack = vec![smart_folder_id];
    while let Some(current) = stack.pop() {
        let mut stmt = conn.prepare_cached(
            "SELECT smart_folder_id FROM smart_folder WHERE parent_id = ?1 ORDER BY smart_folder_id",
        )?;
        let child_ids: Vec<i64> = stmt
            .query_map([current], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for child_id in child_ids {
            descendants.push(child_id);
            stack.push(child_id);
        }
    }
    Ok(descendants)
}

pub fn delete_smart_folder(conn: &Connection, smart_folder_id: i64) -> rusqlite::Result<Vec<i64>> {
    let parent_id: Option<i64> = conn.query_row(
        "SELECT parent_id FROM smart_folder WHERE smart_folder_id = ?1",
        [smart_folder_id],
        |row| row.get(0),
    )?;
    let child_ids: Vec<i64> = conn
        .prepare_cached(
            "SELECT smart_folder_id FROM smart_folder WHERE parent_id = ?1 ORDER BY COALESCE(display_order, smart_folder_id)",
        )?
        .query_map([smart_folder_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    conn.execute(
        "UPDATE smart_folder SET parent_id = ?1 WHERE parent_id = ?2",
        params![parent_id, smart_folder_id],
    )?;
    conn.execute(
        "DELETE FROM smart_folder WHERE smart_folder_id = ?1",
        [smart_folder_id],
    )?;
    Ok(child_ids)
}

pub fn has_local_rules(pred: &SmartFolderPredicate) -> bool {
    pred.groups.iter().any(|group| !group.rules.is_empty())
}

fn parse_predicate_json(predicate_json: &str) -> rusqlite::Result<SmartFolderPredicate> {
    serde_json::from_str(predicate_json)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

pub fn get_smart_folder_chain(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<Vec<SmartFolder>> {
    let mut chain = Vec::new();
    let mut current_id = Some(smart_folder_id);
    let mut visited = HashSet::new();

    while let Some(id) = current_id {
        if !visited.insert(id) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let folder = get_smart_folder(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        current_id = folder.parent_id;
        chain.push(folder);
    }

    chain.reverse();
    Ok(chain)
}

pub fn build_effective_predicate_for_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<SmartFolderPredicate> {
    let chain = get_smart_folder_chain(conn, smart_folder_id)?;
    let mut groups = Vec::new();
    for folder in chain {
        let pred = parse_predicate_json(&folder.predicate_json)?;
        if has_local_rules(&pred) {
            groups.extend(pred.groups);
        }
    }
    Ok(SmartFolderPredicate { groups })
}

/// Batch-update display_order within a specific parent scope.
pub fn reorder_smart_folders(
    conn: &Connection,
    parent_id: Option<i64>,
    moves: &[(i64, i64)],
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached(
        "UPDATE smart_folder SET display_order = ?1, parent_id = ?2 WHERE smart_folder_id = ?3",
    )?;
    for &(smart_folder_id, new_display_order) in moves {
        stmt.execute(params![new_display_order, parent_id, smart_folder_id])?;
    }
    Ok(())
}

/// Compile a predicate into a RoaringBitmap using bitmap store + SQL fallback.
///
/// Groups are ANDed together at the top level. This means a multi-group predicate
/// is an intersection — the file must satisfy ALL groups. This models "AND" across
/// groups while each group can internally use either AND or OR for its rules.
///
/// Within each group, rules are combined according to the group's `match_mode`
/// (all = AND, any = OR). If `group.negate` is set, the group bitmap is inverted
/// against active (status=1) — `active - group_result`.
pub fn compile_predicate(
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

    Ok(final_result.unwrap_or_default())
}

/// Compile a single rule group into a bitmap.
fn compile_group(
    conn: &Connection,
    group: &SmartRuleGroup,
    bitmaps: &BitmapStore,
    active: &RoaringBitmap,
) -> rusqlite::Result<RoaringBitmap> {
    let mut include_bitmaps: Vec<RoaringBitmap> = Vec::new();
    let mut exclude_bitmaps: Vec<RoaringBitmap> = Vec::new();

    // Pre-resolve all tag references in this group with one batch query.
    let all_tag_pairs: Vec<(String, String)> = group
        .rules
        .iter()
        .filter(|r| r.field == "tags")
        .flat_map(|r| {
            r.values
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter_map(|s| crate::tags::normalize::parse_tag(s))
        })
        .collect();
    let tag_id_map = batch_find_tag_ids(conn, &all_tag_pairs)?;

    for rule in &group.rules {
        let field = rule.field.as_str();
        let op = rule.op.as_str();

        match field {
            "tags" => {
                let tag_values = rule.values.as_deref().unwrap_or(&[]);
                match op {
                    "include" | "include_all" => {
                        for tag_str in tag_values {
                            if let Some(key) = crate::tags::normalize::parse_tag(tag_str) {
                                if let Some(&tag_id) = tag_id_map.get(&key) {
                                    include_bitmaps
                                        .push(bitmaps.get(&BitmapKey::EffectiveTag(tag_id)));
                                } else {
                                    include_bitmaps.push(RoaringBitmap::new());
                                }
                            } else {
                                include_bitmaps.push(RoaringBitmap::new());
                            }
                        }
                    }
                    "include_any" => {
                        let mut any_bm = RoaringBitmap::new();
                        for tag_str in tag_values {
                            if let Some(key) = crate::tags::normalize::parse_tag(tag_str) {
                                if let Some(&tag_id) = tag_id_map.get(&key) {
                                    any_bm |= &bitmaps.get(&BitmapKey::EffectiveTag(tag_id));
                                }
                            }
                        }
                        include_bitmaps.push(any_bm);
                    }
                    "do_not_include" => {
                        for tag_str in tag_values {
                            if let Some(key) = crate::tags::normalize::parse_tag(tag_str) {
                                if let Some(&tag_id) = tag_id_map.get(&key) {
                                    exclude_bitmaps
                                        .push(bitmaps.get(&BitmapKey::EffectiveTag(tag_id)));
                                }
                            }
                        }
                    }
                    _ => {
                        tracing::warn!("Unknown tags op: {op}");
                    }
                }
            }

            "rating" => {
                compile_numeric_rule(conn, "rating", rule, &mut include_bitmaps)?;
            }
            "file_size" => {
                compile_numeric_rule(conn, "size", rule, &mut include_bitmaps)?;
            }
            "width" => {
                compile_numeric_rule(conn, "width", rule, &mut include_bitmaps)?;
            }
            "height" => {
                compile_numeric_rule(conn, "height", rule, &mut include_bitmaps)?;
            }
            "duration" => {
                compile_numeric_rule(conn, "duration", rule, &mut include_bitmaps)?;
            }
            "aspect_ratio" => {
                let expr = "CAST(width AS REAL) / CAST(height AS REAL)";
                compile_numeric_rule_expr(conn, expr, rule, &mut include_bitmaps)?;
            }

            "file_type" => match op {
                "is" => {
                    if let Some(v) = rule.value.as_ref().and_then(|v| v.as_str()) {
                        include_bitmaps.push(sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file WHERE mime = ?1",
                            params![v],
                        )?);
                    }
                }
                "is_not" => {
                    if let Some(v) = rule.value.as_ref().and_then(|v| v.as_str()) {
                        exclude_bitmaps.push(sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file WHERE mime = ?1",
                            params![v],
                        )?);
                    }
                }
                _ => {
                    tracing::warn!("Unknown file_type op: {op}");
                }
            },

            "date_imported" => match op {
                "after" => {
                    if let Some(v) = rule.value.as_ref().and_then(|v| v.as_str()) {
                        include_bitmaps.push(sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file WHERE imported_at > ?1",
                            params![v],
                        )?);
                    }
                }
                "before" => {
                    if let Some(v) = rule.value.as_ref().and_then(|v| v.as_str()) {
                        include_bitmaps.push(sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file WHERE imported_at < ?1",
                            params![v],
                        )?);
                    }
                }
                "is" => {
                    if let Some(v) = rule.value.as_ref().and_then(|v| v.as_str()) {
                        let pattern = format!("{}%", v);
                        include_bitmaps.push(sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file WHERE imported_at LIKE ?1",
                            params![pattern],
                        )?);
                    }
                }
                _ => {
                    tracing::warn!("Unknown date_imported op: {op}");
                }
            },

            "has_audio" => {
                if op == "is" {
                    let bool_val = match rule.value.as_ref() {
                        Some(serde_json::Value::Bool(b)) => Some(if *b { 1i64 } else { 0i64 }),
                        Some(serde_json::Value::Number(n)) => n.as_i64(),
                        _ => None,
                    };
                    if let Some(v) = bool_val {
                        include_bitmaps.push(sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file WHERE has_audio = ?1",
                            params![v],
                        )?);
                    }
                } else {
                    tracing::warn!("Unknown has_audio op: {op}");
                }
            }

            "notes" => {
                compile_text_rule(
                    conn,
                    "notes",
                    rule,
                    &mut include_bitmaps,
                    &mut exclude_bitmaps,
                )?;
            }

            "name" => {
                compile_text_rule(
                    conn,
                    "name",
                    rule,
                    &mut include_bitmaps,
                    &mut exclude_bitmaps,
                )?;
            }

            "source_url" => {
                compile_text_rule(
                    conn,
                    "source_url",
                    rule,
                    &mut include_bitmaps,
                    &mut exclude_bitmaps,
                )?;
            }

            "color" => {
                if op == "contains" {
                    let color_values = rule.values.as_deref().unwrap_or(&[]);
                    let mut color_bm = RoaringBitmap::new();
                    for hex in color_values {
                        color_bm |= &sql_to_bitmap(
                            conn,
                            "SELECT file_id FROM file_color WHERE hex_color = ?1",
                            params![hex],
                        )?;
                    }
                    include_bitmaps.push(color_bm);
                } else {
                    tracing::warn!("Unknown color op: {op}");
                }
            }

            "shape" => {
                if op == "is" {
                    if let Some(shape) = rule.value.as_ref().and_then(|v| v.as_str()) {
                        let sql = match shape {
                            "landscape" => Some("SELECT file_id FROM file WHERE width > height"),
                            "portrait" => Some("SELECT file_id FROM file WHERE height > width"),
                            "square" => Some("SELECT file_id FROM file WHERE width = height"),
                            _ => {
                                tracing::warn!("Unknown shape value: {shape}");
                                None
                            }
                        };
                        if let Some(sql) = sql {
                            include_bitmaps.push(sql_to_bitmap(conn, sql, [])?);
                        }
                    }
                } else {
                    tracing::warn!("Unknown shape op: {op}");
                }
            }

            _ => {
                tracing::warn!("Unknown smart folder rule field: {field}");
            }
        }
    }

    let combined = match group.match_mode {
        MatchMode::All => {
            if include_bitmaps.is_empty() {
                active.clone()
            } else {
                let mut result = include_bitmaps[0].clone();
                for bm in &include_bitmaps[1..] {
                    result &= bm;
                }
                result
            }
        }
        MatchMode::Any => {
            if include_bitmaps.is_empty() {
                active.clone()
            } else {
                let mut result = RoaringBitmap::new();
                for bm in &include_bitmaps {
                    result |= bm;
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

/// Compile a numeric comparison rule against a simple column name.
fn compile_numeric_rule(
    conn: &Connection,
    column: &str,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let op = rule.op.as_str();
    if op == "between" {
        let v1 = rule.value.as_ref().and_then(|v| v.as_f64());
        let v2 = rule.value2.as_ref().and_then(|v| v.as_f64());
        if let (Some(lo), Some(hi)) = (v1, v2) {
            let sql = format!(
                "SELECT file_id FROM file WHERE {} BETWEEN ?1 AND ?2",
                column
            );
            include_bitmaps.push(sql_to_bitmap(conn, &sql, params![lo, hi])?);
        }
        return Ok(());
    }

    if let Some(sql_tpl) = numeric_comparison_sql(column, op) {
        if let Some(v) = rule.value.as_ref().and_then(|v| v.as_f64()) {
            include_bitmaps.push(sql_to_bitmap(conn, &sql_tpl, params![v])?);
        }
    } else {
        tracing::warn!("Unknown numeric op for {column}: {op}");
    }
    Ok(())
}

/// Compile a numeric comparison rule against an arbitrary SQL expression.
fn compile_numeric_rule_expr(
    conn: &Connection,
    expr: &str,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let op = rule.op.as_str();
    if op == "between" {
        let v1 = rule.value.as_ref().and_then(|v| v.as_f64());
        let v2 = rule.value2.as_ref().and_then(|v| v.as_f64());
        if let (Some(lo), Some(hi)) = (v1, v2) {
            let sql = format!("SELECT file_id FROM file WHERE {} BETWEEN ?1 AND ?2", expr);
            include_bitmaps.push(sql_to_bitmap(conn, &sql, params![lo, hi])?);
        }
        return Ok(());
    }

    let cmp = match op {
        "eq" => Some("="),
        "neq" => Some("!="),
        "gt" => Some(">"),
        "gte" => Some(">="),
        "lt" => Some("<"),
        "lte" => Some("<="),
        _ => None,
    };
    if let Some(cmp) = cmp {
        if let Some(v) = rule.value.as_ref().and_then(|v| v.as_f64()) {
            let sql = format!("SELECT file_id FROM file WHERE {} {} ?1", expr, cmp);
            include_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
        }
    } else {
        tracing::warn!("Unknown numeric op for expression: {op}");
    }
    Ok(())
}

/// Compile a text comparison rule. Negative ops (is_not, does_not_contain) go
/// into `exclude_bitmaps`; everything else into `include_bitmaps`.
fn compile_text_rule(
    conn: &Connection,
    column: &str,
    rule: &PredicateRule,
    include_bitmaps: &mut Vec<RoaringBitmap>,
    exclude_bitmaps: &mut Vec<RoaringBitmap>,
) -> rusqlite::Result<()> {
    let op = rule.op.as_str();
    let val_str = rule
        .value
        .as_ref()
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match op {
        "is" => {
            if let Some(v) = val_str {
                let sql = format!("SELECT file_id FROM file WHERE {} = ?1", column);
                include_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
            }
        }
        "is_not" => {
            if let Some(v) = val_str {
                let sql = format!("SELECT file_id FROM file WHERE {} = ?1", column);
                exclude_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
            }
        }
        "contains" => {
            if let Some(v) = val_str {
                let sql = format!(
                    "SELECT file_id FROM file WHERE {} LIKE '%' || ?1 || '%'",
                    column
                );
                include_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
            }
        }
        "does_not_contain" => {
            if let Some(v) = val_str {
                let sql = format!(
                    "SELECT file_id FROM file WHERE {} LIKE '%' || ?1 || '%'",
                    column
                );
                exclude_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
            }
        }
        "starts_with" => {
            if let Some(v) = val_str {
                let sql = format!("SELECT file_id FROM file WHERE {} LIKE ?1 || '%'", column);
                include_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
            }
        }
        "ends_with" => {
            if let Some(v) = val_str {
                let sql = format!("SELECT file_id FROM file WHERE {} LIKE '%' || ?1", column);
                include_bitmaps.push(sql_to_bitmap(conn, &sql, params![v])?);
            }
        }
        "is_empty" => {
            let sql = format!(
                "SELECT file_id FROM file WHERE {} IS NULL OR {} = ''",
                column, column
            );
            include_bitmaps.push(sql_to_bitmap(conn, &sql, [])?);
        }
        "is_not_empty" => {
            let sql = format!(
                "SELECT file_id FROM file WHERE {} IS NOT NULL AND {} != ''",
                column, column
            );
            include_bitmaps.push(sql_to_bitmap(conn, &sql, [])?);
        }
        _ => {
            tracing::warn!("Unknown text op for {column}: {op}");
        }
    }
    Ok(())
}

/// Helper: return a SELECT for simple numeric comparisons on a column.
fn numeric_comparison_sql(column: &str, op: &str) -> Option<String> {
    match op {
        "eq" => Some(format!("SELECT file_id FROM file WHERE {} = ?1", column)),
        "neq" => Some(format!("SELECT file_id FROM file WHERE {} != ?1", column)),
        "gt" => Some(format!("SELECT file_id FROM file WHERE {} > ?1", column)),
        "gte" => Some(format!("SELECT file_id FROM file WHERE {} >= ?1", column)),
        "lt" => Some(format!("SELECT file_id FROM file WHERE {} < ?1", column)),
        "lte" => Some(format!("SELECT file_id FROM file WHERE {} <= ?1", column)),
        _ => None,
    }
}

/// Batch lookup tag_ids for multiple (namespace, subtag) pairs in one query.
fn batch_find_tag_ids(
    conn: &Connection,
    tags: &[(String, String)],
) -> rusqlite::Result<std::collections::HashMap<(String, String), i64>> {
    use std::collections::HashMap;
    if tags.is_empty() {
        return Ok(HashMap::new());
    }

    // Build VALUES list: (?,?), (?,?), ...
    let placeholders = std::iter::repeat_n("(?,?)", tags.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT tag_id, namespace, subtag FROM tag
         WHERE (namespace, subtag) IN (VALUES {})",
        placeholders
    );
    let mut flat_params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(tags.len() * 2);
    for (ns, st) in tags {
        flat_params.push(ns);
        flat_params.push(st);
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
        let (id, ns, st) = row?;
        map.insert((ns, st), id);
    }
    Ok(map)
}

fn sql_to_bitmap(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<RoaringBitmap> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| row.get::<_, i64>(0))?;
    let mut bm = RoaringBitmap::new();
    for row in rows {
        bm.insert(row? as u32);
    }
    Ok(bm)
}

impl SqliteDatabase {
    pub async fn create_smart_folder(
        &self,
        name: String,
        parent_id: Option<i64>,
        predicate_json: String,
        icon: Option<String>,
        color: Option<String>,
        notes: Option<String>,
        sort_field: Option<String>,
        sort_order: Option<String>,
    ) -> Result<SmartFolder, String> {
        let sf_id = self
            .with_conn(move |conn| {
                create_smart_folder(
                    conn,
                    &name,
                    parent_id,
                    &predicate_json,
                    icon.as_deref(),
                    color.as_deref(),
                    notes.as_deref(),
                    sort_field.as_deref(),
                    sort_order.as_deref(),
                )
            })
            .await?;

        self.emit_read_model_event(ReadModelEvent::SmartFolderChanged {
            smart_folder_id: sf_id,
        });

        self.with_read_conn(move |conn| get_smart_folder(conn, sf_id))
            .await?
            .ok_or_else(|| "Smart folder not found after creation".to_string())
    }

    pub async fn list_smart_folders(&self) -> Result<Vec<SmartFolder>, String> {
        self.with_read_conn(list_smart_folders).await
    }

    pub async fn update_smart_folder(&self, sf: SmartFolder) -> Result<(), String> {
        let sf_id = sf.smart_folder_id;
        self.with_conn(move |conn| update_smart_folder(conn, &sf))
            .await?;
        self.emit_read_model_event(ReadModelEvent::SmartFolderChanged {
            smart_folder_id: sf_id,
        });
        Ok(())
    }

    pub async fn reorder_smart_folders(
        &self,
        parent_id: Option<i64>,
        moves: Vec<(i64, i64)>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| reorder_smart_folders(conn, parent_id, &moves))
            .await?;
        self.emit_read_model_event(ReadModelEvent::SmartFolderChanged { smart_folder_id: 0 });
        Ok(())
    }

    pub async fn move_smart_folder(
        &self,
        smart_folder_id: i64,
        new_parent_id: Option<i64>,
        sibling_order: Vec<(i64, i64)>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            move_smart_folder(conn, smart_folder_id, new_parent_id, &sibling_order)
        })
        .await?;
        self.emit_read_model_event(ReadModelEvent::SmartFolderChanged { smart_folder_id: 0 });
        Ok(())
    }

    pub async fn delete_smart_folder(&self, smart_folder_id: i64) -> Result<Vec<i64>, String> {
        self.bitmaps
            .remove_key(&BitmapKey::SmartFolder(smart_folder_id));
        let descendants = self
            .with_read_conn(move |conn| collect_descendant_smart_folder_ids(conn, smart_folder_id))
            .await?;
        for descendant_id in &descendants {
            self.bitmaps
                .remove_key(&BitmapKey::SmartFolder(*descendant_id));
        }
        self.with_conn(move |conn| delete_smart_folder(conn, smart_folder_id))
            .await
    }

    /// Count files matching a smart folder.
    pub async fn count_smart_folder(&self, smart_folder_id: i64) -> Result<i64, String> {
        let count = self.bitmaps.len(&BitmapKey::SmartFolder(smart_folder_id));
        if count > 0 {
            return Ok(count as i64);
        }

        let bitmaps = self.bitmaps.clone();
        self.with_read_conn(move |conn| {
            get_smart_folder(conn, smart_folder_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            let pred = build_effective_predicate_for_smart_folder(conn, smart_folder_id)?;
            let result = compile_predicate(conn, &pred, &bitmaps)?;
            Ok(result.len() as i64)
        })
        .await
    }
}
