use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::query::duplicates::DuplicateSingleRef;
use crate::db::types::{DuplicateResolutionResult, DuplicateResolveStatus};

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

pub fn reconcile_detected_duplicate_pairs(
    conn: &Connection,
    candidate_pairs: &[(i64, i64, u32)],
) -> rusqlite::Result<Vec<(i64, i64, u32)>> {
    let mut current = BTreeMap::<(i64, i64), u32>::new();
    for (file_id_a, file_id_b, distance) in candidate_pairs {
        let (a, b) = if file_id_a < file_id_b {
            (*file_id_a, *file_id_b)
        } else {
            (*file_id_b, *file_id_a)
        };
        current
            .entry((a, b))
            .and_modify(|existing| *existing = (*existing).min(*distance))
            .or_insert(*distance);
    }

    let stale_pairs: Vec<(i64, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT file_id_a, file_id_b
             FROM duplicate
             WHERE status = 'detected'",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (file_id_a, file_id_b) in stale_pairs {
        if !current.contains_key(&(file_id_a, file_id_b)) {
            conn.execute(
                "DELETE FROM duplicate
                 WHERE file_id_a = ?1 AND file_id_b = ?2 AND status = 'detected'",
                params![file_id_a, file_id_b],
            )?;
        }
    }

    let mut newly_detected = Vec::new();
    for ((file_id_a, file_id_b), distance) in current {
        let existing_status: Option<String> = conn
            .query_row(
                "SELECT status FROM duplicate WHERE file_id_a = ?1 AND file_id_b = ?2",
                params![file_id_a, file_id_b],
                |row| row.get(0),
            )
            .optional()?;
        match existing_status.as_deref() {
            Some("detected") => {
                conn.execute(
                    "UPDATE duplicate SET distance = ?3
                     WHERE file_id_a = ?1 AND file_id_b = ?2 AND status = 'detected'",
                    params![file_id_a, file_id_b, distance as i64],
                )?;
            }
            Some(_) => {}
            None => {
                conn.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance)
                     VALUES (?1, ?2, ?3)",
                    params![file_id_a, file_id_b, distance as i64],
                )?;
                newly_detected.push((file_id_a, file_id_b, distance));
            }
        }
    }
    Ok(newly_detected)
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
    distance: Option<u32>,
) -> rusqlite::Result<Option<(DuplicateSingleRef, DuplicateSingleRef)>> {
    match action {
        "keep_left" => Ok(Some((left.clone(), right.clone()))),
        "keep_right" => Ok(Some((right.clone(), left.clone()))),
        "smart_merge" => {
            let decision = crate::duplicates::quality::smart_merge_quality_decision(
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
                distance,
                left.file_hash == right.file_hash,
            );
            Ok(match decision {
                crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                    Some((left.clone(), right.clone()))
                }
                crate::duplicates::quality::ImageQualityDecision::RightBetter => {
                    Some((right.clone(), left.clone()))
                }
                crate::duplicates::quality::ImageQualityDecision::Ambiguous => None,
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
) -> rusqlite::Result<DuplicateResolutionResult> {
    let active_or_inbox_entities: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM media_entity
         WHERE entity_id IN (?1, ?2)
           AND status IN (0, 1)",
        params![left.entity_id, right.entity_id],
        |row| row.get(0),
    )?;
    if active_or_inbox_entities != 2 {
        return Err(rusqlite::Error::InvalidParameterName(
            "duplicate entities must be active or inbox".to_string(),
        ));
    }

    let pair: Option<(String, i64)> = conn
        .query_row(
            "SELECT status, distance FROM duplicate
             WHERE (file_id_a = ?1 AND file_id_b = ?2)
                OR (file_id_a = ?2 AND file_id_b = ?1)",
            params![left.file_id, right.file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if pair.as_ref().map(|(status, _)| status.as_str()) != Some("detected") {
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
            loser_file_hash: None,
            blob_cleanup_pending: false,
            cleanup_error: None,
            action: action.to_string(),
            affected_folder_ids: Vec::new(),
            tags_merged: 0,
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
            loser_file_hash: None,
            blob_cleanup_pending: false,
            cleanup_error: None,
            action: action.to_string(),
            affected_folder_ids: Vec::new(),
            tags_merged: 0,
        });
    }

    let distance = pair.and_then(|(_, distance)| u32::try_from(distance).ok());
    let Some((winner, loser)) = choose_winner(action, &left, &right, distance)? else {
        return Ok(DuplicateResolutionResult {
            status: DuplicateResolveStatus::QualityAmbiguous,
            winner_hash: None,
            loser_hash: None,
            loser_file_hash: None,
            blob_cleanup_pending: false,
            cleanup_error: None,
            action: action.to_string(),
            affected_folder_ids: Vec::new(),
            tags_merged: 0,
        });
    };

    let tags_merged: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM entity_tag et
             WHERE et.entity_id = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM entity_tag existing
                   WHERE existing.entity_id = ?2
                     AND existing.tag_id = et.tag_id
                     AND existing.source = et.source
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

    let merged_name = winner.name.clone().or(loser.name.clone());
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
    let merged_date_added = winner.date_added.min(loser.date_added);
    let merged_status = merged_status(winner.status, loser.status);
    conn.execute(
        "UPDATE media_entity
         SET status = ?1,
             name = ?2,
             notes = ?3,
             source_urls_json = ?4,
             rating = ?5,
             date_created = ?6,
             date_added = ?7,
             date_modified = ?8
         WHERE entity_id = ?9",
        params![
            merged_status,
            merged_name.as_deref(),
            merged_notes.as_deref(),
            merged_urls.as_deref(),
            merged_rating,
            merged_created_at,
            merged_date_added,
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

    conn.execute(
        "INSERT INTO entity_tag_implied (entity_id, tag_id)
         SELECT ?1, tag_id FROM entity_tag_implied WHERE entity_id = ?2
         ON CONFLICT(entity_id, tag_id) DO NOTHING",
        params![winner.entity_id, loser.entity_id],
    )?;
    conn.execute(
        "INSERT INTO media_view (entity_id, viewed_at)
         SELECT ?1, viewed_at FROM media_view WHERE entity_id = ?2
         ON CONFLICT(entity_id) DO UPDATE SET viewed_at = MAX(media_view.viewed_at, excluded.viewed_at)",
        params![winner.entity_id, loser.entity_id],
    )?;
    conn.execute(
        "UPDATE subscription_post_member SET entity_id = ?1 WHERE entity_id = ?2",
        params![winner.entity_id, loser.entity_id],
    )?;

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
        "DELETE FROM media_entity WHERE entity_id = ?1",
        [loser.entity_id],
    )?;

    conn.execute(
        "DELETE FROM file_color_rtree
         WHERE id IN (SELECT rowid FROM file_color WHERE file_id = ?1)",
        [loser.file_id],
    )?;
    let file_deleted = conn.execute(
        "DELETE FROM media_file
         WHERE file_id = ?1
           AND NOT EXISTS (SELECT 1 FROM media_entity WHERE file_id = ?1)",
        [loser.file_id],
    )? > 0;
    if file_deleted {
        conn.execute(
            "DELETE FROM deferred_work_item
             WHERE entity_hash = ?1 AND work_type != 'blob_delete'",
            [&loser.file_hash],
        )?;
        conn.execute(
            "INSERT INTO deferred_work_item
                 (entity_hash, work_type, status, attempt_count, available_at, queued_at)
             VALUES (?1, 'blob_delete', 'pending', 0, ?2, ?2)
             ON CONFLICT(entity_hash, work_type) DO NOTHING",
            params![loser.file_hash, chrono::Utc::now().to_rfc3339()],
        )?;
    }

    Ok(DuplicateResolutionResult {
        status: DuplicateResolveStatus::Resolved,
        winner_hash: Some(winner.entity_hash),
        loser_hash: Some(loser.entity_hash),
        loser_file_hash: Some(loser.file_hash),
        blob_cleanup_pending: file_deleted,
        cleanup_error: None,
        action: action.to_string(),
        affected_folder_ids,
        tags_merged,
    })
}
