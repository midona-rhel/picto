//! Smart folder predicate → bitmap compilation.
//! Smart folder membership is derived, not authoritative.

use roaring::RoaringBitmap;
use rusqlite::Connection;

use super::bitmaps::{BitmapKey, BitmapStore};

/// Compile a single smart folder's bitmap from its predicate.
/// The predicate is a JSON structure with groups of rules.
/// Each group is ANDed; rules within a group are ORed.
pub fn compile_smart_folder(conn: &Connection, bitmaps: &BitmapStore, smart_folder_id: i64) {
    let predicate_json: Option<String> = conn
        .query_row(
            "SELECT predicate_json FROM smart_folder WHERE smart_folder_id = ?1",
            [smart_folder_id],
            |row| row.get(0),
        )
        .ok();

    let Some(json) = predicate_json else {
        bitmaps.set(
            BitmapKey::SmartFolder(smart_folder_id),
            RoaringBitmap::new(),
        );
        return;
    };

    let result = evaluate_predicate(bitmaps, &json);
    // Intersect with active status (smart folders only show active entities)
    let active = bitmaps.get(&BitmapKey::Status(1));
    bitmaps.set(BitmapKey::SmartFolder(smart_folder_id), &result & &active);
}

/// Compile all smart folders.
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

/// Evaluate a predicate JSON against the bitmap store.
/// Predicate format: { "groups": [ { "match": "all"|"any", "rules": [...] } ] }
/// Groups are ANDed together.
pub(crate) fn evaluate_predicate(bitmaps: &BitmapStore, json: &str) -> RoaringBitmap {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return RoaringBitmap::new(),
    };

    let groups = match parsed.get("groups").and_then(|g| g.as_array()) {
        Some(g) => g,
        None => return RoaringBitmap::new(),
    };

    let mut result: Option<RoaringBitmap> = None;

    for group in groups {
        let rules = match group.get("rules").and_then(|r| r.as_array()) {
            Some(r) => r,
            None => continue,
        };
        if rules.is_empty() {
            continue;
        }

        let match_mode = group.get("match").and_then(|m| m.as_str()).unwrap_or("all");

        let group_result = evaluate_group(bitmaps, rules, match_mode);

        result = Some(match result {
            Some(prev) => &prev & &group_result,
            None => group_result,
        });
    }

    result.unwrap_or_default()
}

fn evaluate_group(
    bitmaps: &BitmapStore,
    rules: &[serde_json::Value],
    match_mode: &str,
) -> RoaringBitmap {
    let mut result: Option<RoaringBitmap> = None;

    for rule in rules {
        let rule_type = rule.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let operator = rule
            .get("operator")
            .and_then(|o| o.as_str())
            .unwrap_or("include_all");

        let bitmap = match rule_type {
            "tag" => {
                let tag_id = rule.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
                bitmaps.get(&BitmapKey::EffectiveTag(tag_id))
            }
            _ => RoaringBitmap::new(),
        };

        let effective = match operator {
            "do_not_include" => {
                // This will be used as a subtraction in the combining step
                // For now, return the bitmap and handle in combine
                bitmap
            }
            _ => bitmap,
        };

        result = Some(match result {
            Some(prev) => {
                if match_mode == "any" {
                    &prev | &effective
                } else {
                    &prev & &effective
                }
            }
            None => effective,
        });
    }

    result.unwrap_or_default()
}
