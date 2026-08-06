use rusqlite::{params, Connection, OptionalExtension};

use crate::db::query::duplicates::DuplicateSingleRef;
use crate::db::types::{
    DuplicateCollectionConflict, DuplicateResolutionResult, DuplicateResolveStatus,
};

pub fn upsert_duplicate_pair_for_review(
    conn: &Connection,
    file_id_a: i64,
    file_id_b: i64,
    distance: u32,
) -> rusqlite::Result<()> {
    let (file_id_a, file_id_b) = if file_id_a < file_id_b {
        (file_id_a, file_id_b)
    } else {
        (file_id_b, file_id_a)
    };
    conn.execute(
        "INSERT INTO duplicate (
             file_id_a, file_id_b, distance, status, decision_at, decision_source, decision_reason, winner_file_id, loser_file_id
         ) VALUES (?1, ?2, ?3, 'detected', NULL, NULL, NULL, NULL, NULL)
         ON CONFLICT(file_id_a, file_id_b) DO UPDATE SET
             distance = excluded.distance,
             status = 'detected',
             decision_at = NULL,
             decision_source = NULL,
             decision_reason = NULL,
             winner_file_id = NULL,
             loser_file_id = NULL",
        params![file_id_a, file_id_b, distance as i64],
    )?;
    Ok(())
}

pub fn insert_duplicate_pairs_for_scan(
    conn: &Connection,
    candidate_pairs: &[(i64, i64, u32)],
) -> rusqlite::Result<usize> {
    let mut inserted = 0usize;
    for (file_id_a, file_id_b, distance) in candidate_pairs {
        let (a, b) = if file_id_a < file_id_b {
            (*file_id_a, *file_id_b)
        } else {
            (*file_id_b, *file_id_a)
        };
        inserted += conn.execute(
            "INSERT OR IGNORE INTO duplicate (file_id_a, file_id_b, distance)
             VALUES (?1, ?2, ?3)",
            params![a, b, *distance as i64],
        )? as usize;
    }
    Ok(inserted)
}

fn merged_status(left: i64, right: i64) -> i64 {
    if left == 1 || right == 1 {
        1
    } else if left == 0 || right == 0 {
        0
    } else {
        left
    }
}

fn merge_notes(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
    let existing = existing.unwrap_or("").trim();
    let incoming = incoming.unwrap_or("").trim();
    match (existing.is_empty(), incoming.is_empty()) {
        (true, true) => None,
        (true, false) => Some(incoming.to_string()),
        (false, true) => Some(existing.to_string()),
        (false, false) if existing.contains(incoming) => Some(existing.to_string()),
        (false, false) => Some(format!("{existing}\n\n{incoming}")),
    }
}

fn merge_source_urls(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
    let mut merged = Vec::<String>::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for raw in [existing, incoming].into_iter().flatten() {
        let urls: Vec<String> = serde_json::from_str(raw).unwrap_or_default();
        for url in urls {
            if !url.trim().is_empty() && seen.insert(url.clone()) {
                merged.push(url);
            }
        }
    }
    if merged.is_empty() {
        None
    } else {
        serde_json::to_string(&merged).ok()
    }
}

fn choose_winner(
    action: &str,
    left: &DuplicateSingleRef,
    right: &DuplicateSingleRef,
) -> rusqlite::Result<(DuplicateSingleRef, DuplicateSingleRef)> {
    match action {
        "keep_left" => Ok((left.clone(), right.clone())),
        "keep_right" => Ok((right.clone(), left.clone())),
        "smart_merge" => {
            let decision = crate::duplicates::quality::compare_static_image_quality(
                &crate::duplicates::quality::ComparableImageCandidate {
                    mime_type: &left.mime_type,
                    size_bytes: left.size_bytes,
                    pixel_width: left.pixel_width,
                    pixel_height: left.pixel_height,
                    frame_count: left.frame_count,
                },
                &crate::duplicates::quality::ComparableImageCandidate {
                    mime_type: &right.mime_type,
                    size_bytes: right.size_bytes,
                    pixel_width: right.pixel_width,
                    pixel_height: right.pixel_height,
                    frame_count: right.frame_count,
                },
            );
            Ok(match decision {
                crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                    (left.clone(), right.clone())
                }
                crate::duplicates::quality::ImageQualityDecision::RightBetter => {
                    (right.clone(), left.clone())
                }
                crate::duplicates::quality::ImageQualityDecision::Ambiguous => {
                    if left.entity_hash <= right.entity_hash {
                        (left.clone(), right.clone())
                    } else {
                        (right.clone(), left.clone())
                    }
                }
            })
        }
        other => Err(rusqlite::Error::InvalidParameterName(format!(
            "Invalid duplicate action: {other}"
        ))),
    }
}

pub fn resolve_duplicate_pair(
    conn: &Connection,
    action: &str,
    left: DuplicateSingleRef,
    right: DuplicateSingleRef,
    preferred_collection_id: Option<i64>,
) -> rusqlite::Result<DuplicateResolutionResult> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM duplicate
             WHERE (file_id_a = ?1 AND file_id_b = ?2)
                OR (file_id_a = ?2 AND file_id_b = ?1)",
            params![left.file_id, right.file_id],
            |row| row.get(0),
        )
        .optional()?;
    if status.as_deref() != Some("detected") {
        return Err(rusqlite::Error::InvalidParameterName(
            "duplicate pair is not awaiting review".to_string(),
        ));
    }

    if action == "not_duplicate" {
        conn.execute(
            "UPDATE duplicate
             SET status = 'ignored_false_positive',
                 decision_at = datetime('now'),
                 decision_source = 'manual',
                 decision_reason = 'User marked as not duplicate'
             WHERE (file_id_a = ?1 AND file_id_b = ?2) OR (file_id_a = ?2 AND file_id_b = ?1)",
            params![left.file_id, right.file_id],
        )?;
        return Ok(DuplicateResolutionResult {
            status: DuplicateResolveStatus::Resolved,
            winner_hash: None,
            loser_hash: None,
            action: action.to_string(),
            affected_folder_ids: Vec::new(),
            affected_collection_ids: Vec::new(),
            tags_merged: 0,
            conflict: None,
        });
    }

    if action == "keep_both" {
        conn.execute(
            "UPDATE duplicate
             SET status = 'dismissed_keep_both',
                 decision_at = datetime('now'),
                 decision_source = 'manual',
                 decision_reason = 'User chose to keep both'
             WHERE (file_id_a = ?1 AND file_id_b = ?2) OR (file_id_a = ?2 AND file_id_b = ?1)",
            params![left.file_id, right.file_id],
        )?;
        return Ok(DuplicateResolutionResult {
            status: DuplicateResolveStatus::Resolved,
            winner_hash: None,
            loser_hash: None,
            action: action.to_string(),
            affected_folder_ids: Vec::new(),
            affected_collection_ids: Vec::new(),
            tags_merged: 0,
            conflict: None,
        });
    }

    let (winner, loser) = choose_winner(action, &left, &right)?;

    if winner.parent_collection_entity_id.is_some()
        && loser.parent_collection_entity_id.is_some()
        && winner.parent_collection_entity_id != loser.parent_collection_entity_id
        && preferred_collection_id.is_none()
    {
        return Ok(DuplicateResolutionResult {
            status: DuplicateResolveStatus::Conflict,
            winner_hash: Some(winner.entity_hash.clone()),
            loser_hash: Some(loser.entity_hash.clone()),
            action: action.to_string(),
            affected_folder_ids: Vec::new(),
            affected_collection_ids: Vec::new(),
            tags_merged: 0,
            conflict: Some(DuplicateCollectionConflict {
                winner_hash: winner.entity_hash.clone(),
                loser_hash: loser.entity_hash.clone(),
                winner_collection_id: winner.parent_collection_entity_id,
                loser_collection_id: loser.parent_collection_entity_id,
            }),
        });
    }

    if let Some(chosen_collection_id) = preferred_collection_id {
        let valid = [
            winner.parent_collection_entity_id,
            loser.parent_collection_entity_id,
        ]
        .into_iter()
        .flatten()
        .any(|value| value == chosen_collection_id);
        if !valid {
            return Err(rusqlite::Error::InvalidParameterName(
                "preferred_collection_id must match one of the duplicate owners".into(),
            ));
        }
    }

    let final_collection_id = preferred_collection_id
        .or(winner.parent_collection_entity_id)
        .or(loser.parent_collection_entity_id);

    let tags_merged: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM entity_tag et
             WHERE et.entity_id = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM entity_tag existing
                   WHERE existing.entity_id = ?2
                     AND existing.tag_id = et.tag_id
               )",
            params![loser.entity_id, winner.entity_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize;

    conn.execute(
        "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
         SELECT ?1, et.tag_id, et.provenance_mask, et.source
         FROM entity_tag et
         WHERE et.entity_id = ?2
         ON CONFLICT(entity_id, tag_id, source)
         DO UPDATE SET provenance_mask = entity_tag.provenance_mask | excluded.provenance_mask",
        params![winner.entity_id, loser.entity_id],
    )?;

    let merged_notes = merge_notes(winner.notes.as_deref(), loser.notes.as_deref());
    let merged_urls = merge_source_urls(
        winner.source_urls_json.as_deref(),
        loser.source_urls_json.as_deref(),
    );
    let merged_rating = match (winner.rating, loser.rating) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    };
    let merged_created_at = winner.date_created.min(loser.date_created);
    let merged_status = merged_status(winner.status, loser.status);
    conn.execute(
        "UPDATE media_entity
         SET status = ?1,
             notes = ?2,
             source_urls_json = ?3,
             rating = ?4,
             date_created = ?5,
             date_modified = ?6
         WHERE entity_id = ?7",
        params![
            merged_status,
            merged_notes.as_deref(),
            merged_urls.as_deref(),
            merged_rating,
            merged_created_at,
            chrono::Utc::now().to_rfc3339(),
            winner.entity_id
        ],
    )?;

    let affected_folder_ids = {
        let mut stmt = conn.prepare(
            "SELECT folder_id FROM folder_member WHERE entity_id = ?1 ORDER BY folder_id",
        )?;
        let ids = stmt
            .query_map([loser.entity_id], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        conn.execute(
            "INSERT OR IGNORE INTO folder_member (folder_id, entity_id, position_rank)
             SELECT folder_id, ?1, position_rank
             FROM folder_member
             WHERE entity_id = ?2",
            params![winner.entity_id, loser.entity_id],
        )?;
        conn.execute(
            "DELETE FROM folder_member WHERE entity_id = ?1",
            [loser.entity_id],
        )?;
        ids
    };

    conn.execute(
        "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id)
         SELECT subscription_id, ?1
         FROM subscription_entity
         WHERE entity_id = ?2",
        params![winner.entity_id, loser.entity_id],
    )?;
    conn.execute(
        "DELETE FROM subscription_entity WHERE entity_id = ?1",
        [loser.entity_id],
    )?;

    let mut affected_collection_ids = Vec::<i64>::new();
    for collection_id in [
        winner.parent_collection_entity_id,
        loser.parent_collection_entity_id,
        final_collection_id,
    ]
    .into_iter()
    .flatten()
    {
        if !affected_collection_ids.contains(&collection_id) {
            affected_collection_ids.push(collection_id);
        }
    }

    match final_collection_id {
        Some(collection_id) => {
            let ordinal = if loser.parent_collection_entity_id == Some(collection_id) {
                loser.collection_ordinal.unwrap_or(1)
            } else if winner.parent_collection_entity_id == Some(collection_id) {
                winner.collection_ordinal.unwrap_or(1)
            } else {
                conn.query_row(
                    "SELECT COALESCE(MAX(collection_ordinal), 0) + 1
                     FROM media_entity
                     WHERE parent_collection_entity_id = ?1",
                    [collection_id],
                    |row| row.get::<_, i64>(0),
                )?
            };
            conn.execute(
                "UPDATE media_entity
                 SET parent_collection_entity_id = ?1,
                     collection_ordinal = ?2
                 WHERE entity_id = ?3",
                params![collection_id, ordinal, winner.entity_id],
            )?;
        }
        None => {
            conn.execute(
                "UPDATE media_entity
                 SET parent_collection_entity_id = NULL,
                     collection_ordinal = NULL
                 WHERE entity_id = ?1",
                [winner.entity_id],
            )?;
        }
    }

    conn.execute(
        "UPDATE duplicate
         SET status = 'resolved',
             decision_at = datetime('now'),
             decision_source = 'manual',
             decision_reason = ?3,
             winner_file_id = ?4,
             loser_file_id = ?5
         WHERE (file_id_a = ?1 AND file_id_b = ?2) OR (file_id_a = ?2 AND file_id_b = ?1)",
        params![
            left.file_id,
            right.file_id,
            format!("resolved via {action}"),
            winner.file_id,
            loser.file_id
        ],
    )?;
    conn.execute(
        "DELETE FROM duplicate WHERE file_id_a = ?1 OR file_id_b = ?1",
        [loser.file_id],
    )?;
    conn.execute(
        "DELETE FROM single_media_entity WHERE entity_id = ?1",
        [loser.entity_id],
    )?;
    conn.execute(
        "DELETE FROM entity_tag WHERE entity_id = ?1",
        [loser.entity_id],
    )?;
    conn.execute(
        "DELETE FROM media_entity WHERE entity_id = ?1",
        [loser.entity_id],
    )?;
    conn.execute("DELETE FROM media_file WHERE file_id = ?1", [loser.file_id])?;

    for collection_id in &affected_collection_ids {
        crate::db::write::collections::sync_aggregates(&conn, *collection_id)?;
    }

    Ok(DuplicateResolutionResult {
        status: DuplicateResolveStatus::Resolved,
        winner_hash: Some(winner.entity_hash),
        loser_hash: Some(loser.entity_hash),
        action: action.to_string(),
        affected_folder_ids,
        affected_collection_ids,
        tags_merged,
        conflict: None,
    })
}
