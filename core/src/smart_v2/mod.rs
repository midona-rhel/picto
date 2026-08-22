//! Smart-folder predicates for the replacement schema.
//!
//! A predicate is evaluated against media rows and projected to library roots.
//! This keeps collection membership out of the predicate language: each rule
//! can match a different member, while the resulting root sets are combined.

use std::collections::HashSet;

use rusqlite::{types::Value, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmartFolderPredicate {
    #[serde(default)]
    pub groups: Vec<SmartRuleGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmartRuleGroup {
    #[serde(default)]
    pub match_mode: MatchMode,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub rules: Vec<PredicateRule>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    All,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PredicateRule {
    pub field: String,
    pub op: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub value2: Option<serde_json::Value>,
    #[serde(default)]
    pub values: Option<Vec<String>>,
}

/// Compile one smart folder to the active root item IDs it currently matches.
pub fn compile_smart_folder(
    connection: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let predicate = effective_predicate(connection, smart_folder_id)?;
    compile_predicate(connection, &predicate)
}

pub(crate) fn compile_smart_folder_sql(
    connection: &Connection,
    smart_folder_id: i64,
    parameter_offset: usize,
) -> rusqlite::Result<(String, Vec<Value>)> {
    let predicate = effective_predicate(connection, smart_folder_id)?;
    predicate_sql(&predicate, parameter_offset)
}

/// Evaluate a predicate against the replacement schema and return root IDs.
pub fn compile_predicate(
    connection: &Connection,
    predicate: &SmartFolderPredicate,
) -> rusqlite::Result<Vec<i64>> {
    let (sql, arguments) = predicate_sql(predicate, 0)?;
    let mut statement = connection.prepare(&sql)?;
    let roots = statement
        .query_map(rusqlite::params_from_iter(arguments), |row| row.get(0))?
        .collect();
    roots
}

fn predicate_sql(
    predicate: &SmartFolderPredicate,
    parameter_offset: usize,
) -> rusqlite::Result<(String, Vec<Value>)> {
    if predicate.groups.is_empty() {
        return Ok((
            "SELECT item_id AS root_id FROM library_root WHERE 0".to_string(),
            Vec::new(),
        ));
    }

    let mut sql = String::from(
        "WITH root_media AS (
             SELECT lr.item_id AS root_id, lr.item_id AS media_id
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             JOIN media_asset ma ON ma.item_id = lr.item_id
             WHERE lr.lifecycle = 'active'
               AND li.kind = 'media'
               AND NOT EXISTS (
                   SELECT 1 FROM collection_member cm
                   WHERE cm.media_item_id = lr.item_id
               )
             UNION
             SELECT lr.item_id AS root_id, cm.media_item_id AS media_id
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             JOIN collection_member cm ON cm.collection_id = lr.item_id
             WHERE lr.lifecycle = 'active'
               AND li.kind = 'collection'
         )",
    );
    let mut arguments = vec![Value::Null; parameter_offset];
    let mut group_names = Vec::with_capacity(predicate.groups.len());
    let mut rule_index = 0;

    for (group_index, group) in predicate.groups.iter().enumerate() {
        let mut rule_names = Vec::with_capacity(group.rules.len());
        for rule in &group.rules {
            let name = format!("r{rule_index}");
            rule_index += 1;
            let condition = rule_condition(rule, &mut arguments)?;
            sql.push_str(&format!(
                ", {name} AS ( SELECT DISTINCT rm.root_id
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_id
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE {condition} )"
            ));
            rule_names.push(name);
        }

        let raw_name = format!("g{group_index}_raw");
        let group_name = format!("g{group_index}");
        if rule_names.is_empty() {
            sql.push_str(&format!(", {raw_name} AS (SELECT root_id FROM root_media)"));
        } else {
            let joiner = match group.match_mode {
                MatchMode::All => " INTERSECT ",
                MatchMode::Any => " UNION ",
            };
            let set_expression = rule_names
                .iter()
                .map(|name| format!("SELECT root_id FROM {name}"))
                .collect::<Vec<_>>()
                .join(joiner);
            sql.push_str(&format!(", {raw_name} AS ({set_expression})"));
        }
        if group.negate {
            sql.push_str(&format!(
                ", {group_name} AS (
                    SELECT root_id FROM root_media
                    EXCEPT
                    SELECT root_id FROM {raw_name}
                )"
            ));
        } else {
            sql.push_str(&format!(
                ", {group_name} AS (SELECT root_id FROM {raw_name})"
            ));
        }
        group_names.push(group_name);
    }

    let groups = group_names
        .iter()
        .map(|name| format!("SELECT root_id FROM {name}"))
        .collect::<Vec<_>>()
        .join(" INTERSECT ");
    sql.push_str(" SELECT root_id FROM ");
    sql.push_str(&format!("({groups}) ORDER BY root_id"));

    Ok((sql, arguments.split_off(parameter_offset)))
}

/// Alias named for callers that treat compilation as evaluation.
pub fn evaluate(connection: &Connection, smart_folder_id: i64) -> rusqlite::Result<Vec<i64>> {
    compile_smart_folder(connection, smart_folder_id)
}

fn effective_predicate(
    connection: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<SmartFolderPredicate> {
    let mut chain = Vec::new();
    let mut current_id = Some(smart_folder_id);
    let mut visited = HashSet::new();

    while let Some(id) = current_id {
        if !visited.insert(id) {
            return Err(invalid("Smart-folder parent cycle"));
        }
        let row = connection.query_row(
            "SELECT parent_id, predicate_json FROM smart_folder WHERE smart_folder_id = ?1",
            [id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?)),
        )?;
        let (parent_id, predicate_json) = row;
        let predicate: SmartFolderPredicate = serde_json::from_str(&predicate_json)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        chain.push(predicate);
        current_id = parent_id;
    }

    let mut groups = Vec::new();
    for predicate in chain.into_iter().rev() {
        groups.extend(predicate.groups);
    }
    Ok(SmartFolderPredicate { groups })
}

fn rule_condition(rule: &PredicateRule, arguments: &mut Vec<Value>) -> rusqlite::Result<String> {
    match rule.field.as_str() {
        "tags" => tag_condition(rule, arguments),
        "rating" => numeric_condition("ma.rating", rule, arguments),
        "file_size" => numeric_condition("mf.size_bytes", rule, arguments),
        "width" => numeric_condition("mf.pixel_width", rule, arguments),
        "height" => numeric_condition("mf.pixel_height", rule, arguments),
        "duration" => numeric_condition("mf.duration_ms", rule, arguments),
        "aspect_ratio" => numeric_condition(
            "CAST(mf.pixel_width AS REAL) / NULLIF(mf.pixel_height, 0)",
            rule,
            arguments,
        ),
        "file_type" => file_type_condition(rule, arguments),
        "imported" | "imported_at" | "imported_date" | "date_imported" | "date_added" => {
            comparable_condition("ma.imported_at", rule, arguments)
        }
        "captured" | "captured_at" | "captured_date" | "date_captured" => {
            comparable_condition("ma.captured_at", rule, arguments)
        }
        "has_audio" => has_audio_condition(rule, arguments),
        "notes" => text_condition("ma.notes", rule, arguments),
        "name" => text_condition("ma.name", rule, arguments),
        "source_url" | "source_urls" => text_condition("ma.source_urls_json", rule, arguments),
        "color" => color_condition(rule, arguments),
        field => Err(invalid(format!("Unknown smart-folder field: {field}"))),
    }
}

fn numeric_condition(
    column: &str,
    rule: &PredicateRule,
    arguments: &mut Vec<Value>,
) -> rusqlite::Result<String> {
    if rule.op == "between" {
        let low = number_value(rule.value.as_ref())?;
        let high = number_value(rule.value2.as_ref())?;
        arguments.push(low);
        arguments.push(high);
        return Ok(format!(
            "{column} BETWEEN ?{} AND ?{}",
            arguments.len() - 1,
            arguments.len()
        ));
    }
    let operator = comparison_operator(&rule.op)?;
    arguments.push(number_value(rule.value.as_ref())?);
    Ok(format!("{column} {operator} ?{}", arguments.len()))
}

fn comparable_condition(
    column: &str,
    rule: &PredicateRule,
    arguments: &mut Vec<Value>,
) -> rusqlite::Result<String> {
    let operator = comparison_operator(&rule.op)?;
    arguments.push(text_value(rule.value.as_ref())?);
    Ok(format!("{column} {operator} ?{}", arguments.len()))
}

fn text_condition(
    column: &str,
    rule: &PredicateRule,
    arguments: &mut Vec<Value>,
) -> rusqlite::Result<String> {
    match rule.op.as_str() {
        "is" | "eq" => {
            arguments.push(text_value(rule.value.as_ref())?);
            Ok(format!("{column} = ?{}", arguments.len()))
        }
        "is_not" | "neq" => {
            arguments.push(text_value(rule.value.as_ref())?);
            Ok(format!("{column} != ?{}", arguments.len()))
        }
        "contains" => {
            arguments.push(text_value(rule.value.as_ref())?);
            Ok(format!("{column} LIKE '%' || ?{} || '%'", arguments.len()))
        }
        "does_not_contain" => {
            arguments.push(text_value(rule.value.as_ref())?);
            Ok(format!(
                "{column} NOT LIKE '%' || ?{} || '%'",
                arguments.len()
            ))
        }
        "starts_with" => {
            arguments.push(text_value(rule.value.as_ref())?);
            Ok(format!("{column} LIKE ?{} || '%'", arguments.len()))
        }
        "ends_with" => {
            arguments.push(text_value(rule.value.as_ref())?);
            Ok(format!("{column} LIKE '%' || ?{}", arguments.len()))
        }
        "is_empty" => Ok(format!("{column} IS NULL OR {column} = ''")),
        "is_not_empty" => Ok(format!("{column} IS NOT NULL AND {column} != ''")),
        op => Err(invalid(format!("Unknown text operator: {op}"))),
    }
}

fn tag_condition(rule: &PredicateRule, arguments: &mut Vec<Value>) -> rusqlite::Result<String> {
    let values = rule.values.as_deref().unwrap_or_default();
    if values.is_empty() {
        return Ok(if rule.op == "do_not_include" || rule.op == "exclude" {
            "1".to_string()
        } else {
            "0".to_string()
        });
    }

    let exists = values
        .iter()
        .map(|tag| {
            let (namespace, subtag) = split_tag(tag);
            arguments.push(Value::Text(namespace));
            let namespace_index = arguments.len();
            arguments.push(Value::Text(subtag));
            let subtag_index = arguments.len();
            crate::tags_v2::effective_tag_exists_sql("ma.item_id", namespace_index, subtag_index)
        })
        .collect::<Vec<_>>();

    match rule.op.as_str() {
        "include" | "include_all" => Ok(exists.join(" AND ")),
        "include_any" => Ok(format!("({})", exists.join(" OR "))),
        "do_not_include" | "exclude" => Ok(format!("NOT ({})", exists.join(" OR "))),
        op => Err(invalid(format!("Unknown tag operator: {op}"))),
    }
}

fn file_type_condition(
    rule: &PredicateRule,
    arguments: &mut Vec<Value>,
) -> rusqlite::Result<String> {
    let value = rule
        .value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            rule.values
                .as_deref()
                .and_then(|values| values.first().map(String::as_str))
        })
        .ok_or_else(|| invalid("file_type requires a value"))?;
    let pattern = match value {
        "image" => "image/%".to_string(),
        "video" => "video/%".to_string(),
        other => other.to_string(),
    };
    arguments.push(Value::Text(pattern));
    let operator = match rule.op.as_str() {
        "is" | "eq" => "LIKE",
        "is_not" | "neq" => "NOT LIKE",
        op => return Err(invalid(format!("Unknown file_type operator: {op}"))),
    };
    Ok(format!("mf.mime_type {operator} ?{}", arguments.len()))
}

fn has_audio_condition(
    rule: &PredicateRule,
    arguments: &mut Vec<Value>,
) -> rusqlite::Result<String> {
    if !matches!(rule.op.as_str(), "is" | "eq") {
        return Err(invalid(format!("Unknown has_audio operator: {}", rule.op)));
    }
    let value = match rule.value.as_ref() {
        Some(serde_json::Value::Bool(value)) => i64::from(*value),
        Some(serde_json::Value::Number(value)) => value
            .as_i64()
            .ok_or_else(|| invalid("has_audio must be boolean or integer"))?,
        _ => return Err(invalid("has_audio requires a boolean or integer value")),
    };
    arguments.push(Value::Integer(value));
    Ok(format!("mf.has_audio = ?{}", arguments.len()))
}

fn color_condition(rule: &PredicateRule, arguments: &mut Vec<Value>) -> rusqlite::Result<String> {
    let (negative, contains) = match rule.op.as_str() {
        "contains" => (false, true),
        "does_not_contain" => (true, true),
        "is" | "eq" => (false, false),
        "is_not" | "neq" => (true, false),
        op => return Err(invalid(format!("Unknown color operator: {op}"))),
    };
    let values = rule.values.as_deref().unwrap_or_default();
    if values.is_empty() {
        let value = text_value(rule.value.as_ref())?;
        arguments.push(value);
        let comparison = if contains {
            format!("fc.hex LIKE '%' || ?{} || '%'", arguments.len())
        } else {
            format!("fc.hex = ?{}", arguments.len())
        };
        return Ok(format!(
            "{} (SELECT 1 FROM file_color fc WHERE fc.file_id = mf.file_id AND {comparison})",
            if negative { "NOT EXISTS" } else { "EXISTS" },
        ));
    }

    let placeholders = values
        .iter()
        .map(|value| {
            arguments.push(Value::Text(value.clone()));
            format!("?{}", arguments.len())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let comparisons = if contains {
        placeholders
            .split(", ")
            .map(|placeholder| format!("fc.hex LIKE '%' || {placeholder} || '%'"))
            .collect::<Vec<_>>()
            .join(" OR ")
    } else {
        format!("fc.hex IN ({placeholders})")
    };
    Ok(format!(
        "{} (SELECT 1 FROM file_color fc WHERE fc.file_id = mf.file_id AND ({comparisons}))",
        if negative { "NOT EXISTS" } else { "EXISTS" }
    ))
}

fn comparison_operator(op: &str) -> rusqlite::Result<&'static str> {
    match op {
        "eq" | "is" => Ok("="),
        "neq" | "is_not" => Ok("!="),
        "gt" => Ok(">"),
        "gte" => Ok(">="),
        "lt" => Ok("<"),
        "lte" => Ok("<="),
        op => Err(invalid(format!("Unknown comparison operator: {op}"))),
    }
}

fn number_value(value: Option<&serde_json::Value>) -> rusqlite::Result<Value> {
    let value = value.ok_or_else(|| invalid("Numeric predicate requires a value"))?;
    value
        .as_f64()
        .map(Value::Real)
        .ok_or_else(|| invalid("Numeric predicate value must be a number"))
}

fn text_value(value: Option<&serde_json::Value>) -> rusqlite::Result<Value> {
    let value = value.ok_or_else(|| invalid("Text predicate requires a value"))?;
    value
        .as_str()
        .map(|value| Value::Text(value.to_string()))
        .ok_or_else(|| invalid("Text predicate value must be a string"))
}

fn split_tag(value: &str) -> (String, String) {
    value
        .split_once(':')
        .map(|(namespace, subtag)| {
            (
                namespace.trim().to_lowercase(),
                subtag.trim().to_lowercase(),
            )
        })
        .unwrap_or_else(|| ("general".to_string(), value.trim().to_lowercase()))
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::schema::LIBRARY_DDL;
    use rusqlite::params;

    fn connection() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(LIBRARY_DDL).unwrap();
        connection
    }

    fn media(
        connection: &Connection,
        item_id: i64,
        key: &str,
        lifecycle: &str,
        name: &str,
        rating: i64,
        size: i64,
    ) {
        connection
            .execute(
                "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                 VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
                params![item_id, key],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
                params![item_id, lifecycle],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, pixel_width,
                 pixel_height, created_at) VALUES (?1, ?2, 'image/png', ?3, 100, 100, '2026-01-01')",
                params![item_id, format!("hash-{item_id}"), size],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO media_asset (item_id, file_id, name, rating, imported_at, updated_at)
                 VALUES (?1, ?1, ?2, ?3, '2026-01-01', '2026-01-01')",
                params![item_id, name, rating],
            )
            .unwrap();
    }

    fn folder(connection: &Connection, id: i64, parent_id: Option<i64>, predicate: &str) {
        connection
            .execute(
                "INSERT INTO smart_folder
                 (smart_folder_id, smart_folder_key, name, parent_id, predicate_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, '2026-01-01', '2026-01-01')",
                params![id, format!("folder-{id}"), format!("Folder {id}"), parent_id, predicate],
            )
            .unwrap();
    }

    fn predicate(rules: Vec<PredicateRule>) -> String {
        serde_json::to_string(&SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules,
            }],
        })
        .unwrap()
    }

    fn rule(field: &str, op: &str, value: serde_json::Value) -> PredicateRule {
        PredicateRule {
            field: field.to_string(),
            op: op.to_string(),
            value: Some(value),
            value2: None,
            values: None,
        }
    }

    #[test]
    fn parent_and_child_predicates_are_composed() {
        let connection = connection();
        media(&connection, 1, "one", "active", "one", 5, 10);
        media(&connection, 2, "two", "active", "two", 2, 10);
        folder(
            &connection,
            10,
            None,
            &predicate(vec![rule("rating", "gte", serde_json::json!(4))]),
        );
        folder(
            &connection,
            11,
            Some(10),
            &predicate(vec![rule("name", "is", serde_json::json!("one"))]),
        );

        assert_eq!(compile_smart_folder(&connection, 11).unwrap(), vec![1]);
    }

    #[test]
    fn collection_members_can_satisfy_distinct_and_rules() {
        let connection = connection();
        media(&connection, 1, "one", "active", "one", 5, 10);
        media(&connection, 2, "two", "active", "two", 2, 200);
        connection
            .execute(
                "INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
                 VALUES (10, 'collection', 'collection', 'Collection', '2026-01-01', '2026-01-01')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO library_root (item_id, lifecycle) VALUES (10, 'active')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                 VALUES (10, 1, 0), (10, 2, 1)",
                [],
            )
            .unwrap();

        let predicate = SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules: vec![
                    rule("rating", "gte", serde_json::json!(4)),
                    rule("file_size", "gte", serde_json::json!(100)),
                ],
            }],
        };

        assert_eq!(
            compile_predicate(&connection, &predicate).unwrap(),
            vec![10]
        );
    }

    #[test]
    fn only_active_roots_are_returned() {
        let connection = connection();
        media(&connection, 1, "active", "active", "match", 5, 10);
        media(&connection, 2, "inbox", "inbox", "match", 5, 10);
        media(&connection, 3, "trash", "trash", "match", 5, 10);
        let predicate = SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules: vec![rule("rating", "gte", serde_json::json!(4))],
            }],
        };

        assert_eq!(compile_predicate(&connection, &predicate).unwrap(), vec![1]);
    }

    #[test]
    fn color_rules_use_file_color_rows() {
        let connection = connection();
        media(&connection, 1, "red", "active", "red", 1, 10);
        connection
            .execute(
                "INSERT INTO file_color (file_id, hex, l, a, b)
                 VALUES (1, '#ff0000', 50, 60, 70)",
                [],
            )
            .unwrap();
        let predicate = SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules: vec![PredicateRule {
                    field: "color".to_string(),
                    op: "contains".to_string(),
                    value: None,
                    value2: None,
                    values: Some(vec!["#ff0000".to_string()]),
                }],
            }],
        };

        assert_eq!(compile_predicate(&connection, &predicate).unwrap(), vec![1]);
    }

    #[test]
    fn tag_rules_use_aliases_and_transitive_implications() {
        let connection = connection();
        media(&connection, 1, "one", "active", "one", 5, 10);
        connection
            .execute(
                "INSERT INTO tag (tag_id, namespace, subtag) VALUES
                     (1, 'general', 'direct'),
                     (2, 'general', 'alias'),
                     (3, 'general', 'parent'),
                     (4, 'general', 'grandparent')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO media_tag (media_item_id, tag_id) VALUES (1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tag_alias (from_tag_id, to_tag_id) VALUES (1, 2)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tag_implication (child_tag_id, parent_tag_id) VALUES
                     (1, 3), (3, 4)",
                [],
            )
            .unwrap();

        for tag in ["alias", "parent", "grandparent"] {
            let predicate = SmartFolderPredicate {
                groups: vec![SmartRuleGroup {
                    match_mode: MatchMode::All,
                    negate: false,
                    rules: vec![PredicateRule {
                        field: "tags".to_string(),
                        op: "include".to_string(),
                        value: None,
                        value2: None,
                        values: Some(vec![tag.to_string()]),
                    }],
                }],
            };
            assert_eq!(compile_predicate(&connection, &predicate).unwrap(), vec![1]);
        }
    }

    #[test]
    fn parent_cycles_are_rejected() {
        let connection = connection();
        let empty = predicate(Vec::new());
        folder(&connection, 10, None, &empty);
        folder(&connection, 11, None, &empty);
        connection
            .execute(
                "UPDATE smart_folder SET parent_id = 11 WHERE smart_folder_id = 10",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE smart_folder SET parent_id = 10 WHERE smart_folder_id = 11",
                [],
            )
            .unwrap();

        assert!(compile_smart_folder(&connection, 10).is_err());
    }
}
