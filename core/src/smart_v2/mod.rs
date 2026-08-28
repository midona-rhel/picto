//! Smart-folder predicates for the replacement schema.
//!
//! A predicate is evaluated against media rows and projected to library roots.
//! This keeps collection membership out of the predicate language: each rule
//! can match a different member, while the resulting root sets are combined.

use roaring::RoaringBitmap;
use rusqlite::{types::Value, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SmartFolderPredicate {
    #[serde(default)]
    pub groups: Vec<SmartRuleGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SmartRuleGroup {
    #[serde(default)]
    pub match_mode: MatchMode,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub rules: Vec<PredicateRule>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    All,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PredicateRule {
    pub field: String,
    pub op: String,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub value2: Option<serde_json::Value>,
    #[serde(default)]
    pub values: Option<Vec<String>>,
}

/// Evaluate one smart folder to the active root item IDs it currently matches.
/// Materialized folders read their exact active generation; unmaterialized
/// folders evaluate their effective predicate against canonical bitmap state.
pub fn evaluate(connection: &Connection, smart_folder_id: i64) -> rusqlite::Result<Vec<i64>> {
    let has_active_generation: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM smart_folder_generation
             WHERE smart_folder_id = ?1 AND state = 'active'
         )",
        [smart_folder_id],
        |row| row.get(0),
    )?;
    if has_active_generation {
        return connection
            .prepare(
                "SELECT membership.root_item_id
                 FROM smart_folder_generation generation
                 JOIN smart_folder_membership membership
                   ON membership.generation_id = generation.generation_id
                 WHERE generation.smart_folder_id = ?1
                   AND generation.state = 'active'
                 ORDER BY membership.root_item_id",
            )?
            .query_map([smart_folder_id], |row| row.get(0))?
            .collect();
    }
    let predicate = effective_predicate(connection, smart_folder_id)?;
    Ok(evaluate_canonical_predicate(connection, &predicate)?
        .iter()
        .map(i64::from)
        .collect())
}

/// Evaluate a predicate against the canonical active set and bitmap state.
pub(crate) fn evaluate_canonical_predicate(
    connection: &Connection,
    predicate: &SmartFolderPredicate,
) -> rusqlite::Result<RoaringBitmap> {
    let active = canonical_active_roots(connection)?;
    evaluate_impacted_with_tag_bitmaps(connection, predicate, &active, |tag_id| {
        canonical_tag_bitmap(connection, tag_id)
    })
}

/// The current active root set. `root_summary` is maintained inside every
/// write transaction; fall back to `library_root` for databases whose read
/// models have not been seeded yet.
fn canonical_active_roots(connection: &Connection) -> rusqlite::Result<RoaringBitmap> {
    let from_summary = connection
        .prepare_cached("SELECT root_item_id FROM root_summary WHERE lifecycle = 'active'")?
        .query_map([], |row| {
            let root_id: i64 = row.get(0)?;
            u32::try_from(root_id).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, root_id))
        })?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    if !from_summary.is_empty() {
        return Ok(from_summary);
    }
    connection
        .prepare_cached("SELECT item_id FROM library_root WHERE lifecycle = 'active'")?
        .query_map([], |row| {
            let root_id: i64 = row.get(0)?;
            u32::try_from(root_id).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, root_id))
        })?
        .collect()
}

/// Canonical direct-tag membership as durably persisted in the same database.
/// Checksum or decoding failures propagate: a corrupt component must fail the
/// evaluation loudly instead of masquerading as an empty tag.
fn canonical_tag_bitmap(connection: &Connection, tag_id: i64) -> rusqlite::Result<RoaringBitmap> {
    crate::canonical_bitmap::load_bitmap(
        connection,
        crate::canonical_bitmap::BitmapDomain::Tag,
        tag_id,
    )
}

/// Answer one non-tag rule with SQL over immutable facts, scoped to the
/// staged active set. Group ownership comes from the canonical group-member
/// staging table, never from relationship rows.
fn single_rule_roots(
    connection: &Connection,
    rule: &PredicateRule,
) -> rusqlite::Result<RoaringBitmap> {
    if rule.field == "tags" {
        return Err(invalid("tag rules are evaluated from canonical bitmaps"));
    }
    let mut arguments = Vec::new();
    let sql = if is_root_owned_rule(&rule.field) {
        let condition = root_rule_condition(rule, &mut arguments)?;
        format!(
            "WITH active_roots(root_id) AS (
                 SELECT root_item_id FROM picto_smart_impacted_root
             )
             SELECT ar.root_id
             FROM active_roots ar
             JOIN library_item li ON li.item_id = ar.root_id
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = ar.root_id
             LEFT JOIN media_asset cover_asset
               ON cover_asset.item_id = COALESCE(
                    li.cover_media_item_id,
                    CASE WHEN li.kind = 'media' THEN ar.root_id END
                  )
             WHERE {condition}"
        )
    } else {
        let condition = rule_condition(rule, &mut arguments)?;
        format!(
            "WITH active_roots(root_id) AS (
                 SELECT root_item_id FROM picto_smart_impacted_root
             ),
             root_media(root_id, media_id) AS (
                 SELECT ar.root_id, ar.root_id
                 FROM active_roots ar
                 JOIN library_item li ON li.item_id = ar.root_id AND li.kind = 'media'
                 JOIN media_asset own ON own.item_id = ar.root_id
                 UNION ALL
                 SELECT gm.root_id, gm.media_id FROM picto_smart_group_member gm
             )
             SELECT DISTINCT rm.root_id
             FROM root_media rm
             JOIN media_asset ma ON ma.item_id = rm.media_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE {condition}"
        )
    };
    connection
        .prepare(&sql)?
        .query_map(rusqlite::params_from_iter(arguments), |row| {
            let root_id: i64 = row.get(0)?;
            u32::try_from(root_id).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, root_id))
        })?
        .collect()
}

/// Complete pending definition or tag-graph rebuilds inside the canonical
/// write transaction. Ordinary root changes use `refresh_impacted_roots`;
/// broad changes build a complete shadow generation before activation.
pub(crate) fn refresh_materialized(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    refresh_canonical_materialized(transaction)
}

/// Build replacement generations for definitions whose meaning changed.
/// Existing active generations remain readable until every replacement has
/// been materialized and the transaction commits.
pub(crate) fn rebuild_generations(
    transaction: &Transaction<'_>,
    smart_folder_ids: &[i64],
    active_roots: &RoaringBitmap,
    mut tag_bitmap: impl FnMut(i64) -> rusqlite::Result<RoaringBitmap>,
) -> rusqlite::Result<()> {
    if smart_folder_ids.is_empty() {
        return Ok(());
    }
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_smart_rebuild_target (
             smart_folder_id INTEGER PRIMARY KEY
         );
         DELETE FROM picto_smart_rebuild_target;",
    )?;
    {
        let mut insert = transaction.prepare_cached(
            "INSERT INTO picto_smart_rebuild_target(smart_folder_id) VALUES (?1)
             ON CONFLICT(smart_folder_id) DO NOTHING",
        )?;
        for smart_folder_id in smart_folder_ids {
            insert.execute([smart_folder_id])?;
        }
    }
    transaction.execute(
        "DELETE FROM smart_folder_generation
         WHERE state = 'building'
           AND smart_folder_id IN (
               SELECT smart_folder_id FROM picto_smart_rebuild_target
           )",
        [],
    )?;
    transaction.execute(
        "INSERT INTO smart_folder_generation (
             smart_folder_id, database_revision, state, created_at
         )
         SELECT folder.smart_folder_id,
                (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
                'building', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         FROM smart_folder folder
         JOIN picto_smart_rebuild_target target
           ON target.smart_folder_id = folder.smart_folder_id",
        [],
    )?;
    settle_building_generations(transaction, true, Some((active_roots, &mut tag_bitmap)))?;
    transaction.execute("DELETE FROM picto_smart_rebuild_target", [])?;
    Ok(())
}

/// Re-evaluate only the supplied roots against definitions affected by the
/// changed fields or tags. Callers never need to know generation identities or
/// membership-table details.
pub(crate) fn refresh_impacted_roots(
    transaction: &Transaction<'_>,
    roots: &RoaringBitmap,
    changed_fields: &[&str],
    changed_tag_ids: &[i64],
) -> rusqlite::Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let operation_started = std::time::Instant::now();
    let mut stage_started = operation_started;
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_smart_impacted_root (
             root_item_id INTEGER PRIMARY KEY
         );
         CREATE TEMP TABLE IF NOT EXISTS picto_smart_target_folder (
             smart_folder_id INTEGER PRIMARY KEY
         );
         CREATE TEMP TABLE IF NOT EXISTS picto_smart_changed_tag (
             tag_id INTEGER PRIMARY KEY
         );
         CREATE TEMP TABLE IF NOT EXISTS picto_smart_projection_delta (
             smart_folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_smart_impacted_root;
         DELETE FROM picto_smart_target_folder;
         DELETE FROM picto_smart_changed_tag;",
    )?;
    let encoded_roots = bitmap_json(roots);
    trace_smart_stage("stage_roots", stage_started);
    stage_started = std::time::Instant::now();

    if changed_fields.contains(&"lifecycle") {
        transaction.execute(
            "INSERT INTO picto_smart_target_folder(smart_folder_id)
             SELECT smart_folder_id FROM smart_folder
             WHERE TRUE
             ON CONFLICT(smart_folder_id) DO NOTHING",
            [],
        )?;
    } else {
        let mut target = transaction.prepare_cached(
            "INSERT INTO picto_smart_target_folder(smart_folder_id)
             SELECT smart_folder_id
             FROM smart_folder_dependency
             WHERE dependency_kind = ?1 AND dependency_key = ?2
             ON CONFLICT(smart_folder_id) DO NOTHING",
        )?;
        for field in changed_fields {
            if *field == "tags" {
                continue;
            }
            let canonical = canonical_dependency_field(field);
            let kind = if is_root_owned_rule(canonical) {
                "root_field"
            } else {
                "media_field"
            };
            target.execute(rusqlite::params![kind, canonical])?;
        }
    }

    if !changed_tag_ids.is_empty() {
        let mut insert = transaction.prepare_cached(
            "INSERT INTO picto_smart_changed_tag(tag_id) VALUES (?1)
             ON CONFLICT(tag_id) DO NOTHING",
        )?;
        for tag_id in changed_tag_ids {
            insert.execute([tag_id])?;
        }
        transaction.execute(
            "WITH affected(tag_id) AS (
                 SELECT tag_id FROM picto_smart_changed_tag
             )
             INSERT INTO picto_smart_target_folder(smart_folder_id)
             SELECT DISTINCT dependency.smart_folder_id
             FROM affected
             JOIN tag ON tag.tag_id = affected.tag_id
             JOIN smart_folder_dependency dependency
               ON dependency.dependency_kind = 'tag'
              AND dependency.dependency_key = CASE
                    WHEN tag.namespace = 'general' THEN tag.subtag
                    ELSE tag.namespace || ':' || tag.subtag
                  END
             ON CONFLICT(smart_folder_id) DO NOTHING",
            [],
        )?;
    }

    let targets = transaction
        .prepare(
            "SELECT target.smart_folder_id, generation.generation_id
             FROM picto_smart_target_folder target
             JOIN smart_folder_generation generation
               ON generation.smart_folder_id = target.smart_folder_id
              AND generation.state = 'active'
             ORDER BY target.smart_folder_id",
        )?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    stage_projection_changes(
        transaction,
        &targets
            .iter()
            .map(|(smart_folder_id, _)| *smart_folder_id)
            .collect::<Vec<_>>(),
    )?;
    trace_smart_stage("resolve_targets", stage_started);
    if targets.is_empty() {
        transaction.execute_batch(
            "DELETE FROM picto_smart_impacted_root;
             DELETE FROM picto_smart_changed_tag;",
        )?;
        return Ok(());
    }
    let impacted_active = transaction
        .prepare(
            // CROSS JOIN pins the join order: drive from the impacted ids and
            // probe root_summary by primary key. Left reorderable, the planner
            // scans the active index and re-parses the JSON id array once per
            // active row — O(active x impacted), minutes at 100k.
            "SELECT summary.root_item_id
             FROM json_each(?1) impacted
             CROSS JOIN root_summary summary
               ON summary.root_item_id = CAST(impacted.value AS INTEGER)
             WHERE summary.lifecycle = 'active'",
        )?
        .query_map([&encoded_roots], |row| {
            let root_id: i64 = row.get(0)?;
            u32::try_from(root_id).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, root_id))
        })?
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    trace_smart_stage("impacted_active", stage_started);
    stage_started = std::time::Instant::now();
    let next_revision: i64 = transaction.query_row(
        "SELECT revision + 1 FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    transaction.execute(
        "UPDATE projection_write_control
         SET suppress_smart_dirty = 1
         WHERE singleton = 1",
        [],
    )?;
    let targets = targets
        .into_iter()
        .map(|(smart_folder_id, generation_id)| {
            effective_predicate(transaction, smart_folder_id)
                .map(|predicate| (smart_folder_id, generation_id, predicate))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    trace_smart_stage("effective_predicates", stage_started);
    if !impacted_active.is_empty() {
        stage_started = std::time::Instant::now();
        let needs_media = targets
            .iter()
            .any(|(_, _, predicate)| predicate_needs_media(predicate));
        stage_active_context(transaction, &impacted_active, needs_media)?;
        trace_smart_stage("stage_active_context", stage_started);
    }
    for (_smart_folder_id, generation_id, predicate) in targets {
        stage_started = std::time::Instant::now();
        transaction.execute(
            "DELETE FROM smart_folder_membership
             WHERE generation_id = ?1
               AND root_item_id IN (
                   SELECT CAST(value AS INTEGER) FROM json_each(?2)
               )",
            rusqlite::params![generation_id, &encoded_roots],
        )?;
        trace_smart_stage("delete_membership", stage_started);
        if !impacted_active.is_empty() {
            stage_started = std::time::Instant::now();
            let matches = evaluate_staged(transaction, &predicate, &impacted_active, |tag_id| {
                canonical_tag_bitmap(transaction, tag_id)
            })?;
            let mut insert = transaction.prepare_cached(
                "INSERT INTO smart_folder_membership (generation_id, root_item_id)
                 VALUES (?1, ?2)",
            )?;
            for root_id in matches {
                insert.execute(rusqlite::params![generation_id, root_id])?;
            }
            trace_smart_stage("insert_matches", stage_started);
        }
        stage_started = std::time::Instant::now();
        transaction.execute(
            "UPDATE smart_folder_generation
             SET database_revision = ?2,
                 member_count = (
                     SELECT COUNT(*) FROM smart_folder_membership membership
                     WHERE membership.generation_id = ?1
                 )
             WHERE generation_id = ?1 AND state = 'active'",
            rusqlite::params![generation_id, next_revision],
        )?;
        trace_smart_stage("update_count", stage_started);
    }

    transaction.execute(
        "UPDATE projection_write_control
         SET suppress_smart_dirty = 0
         WHERE singleton = 1",
        [],
    )?;

    transaction.execute_batch(
        "DELETE FROM picto_smart_impacted_root;
         DELETE FROM picto_smart_target_folder;
         DELETE FROM picto_smart_changed_tag;",
    )?;
    trace_smart_stage("total", operation_started);
    Ok(())
}

fn trace_smart_stage(stage: &str, started: std::time::Instant) {
    if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some() {
        let elapsed = started.elapsed();
        if elapsed >= std::time::Duration::from_millis(5) || stage == "total" {
            eprintln!(
                "smart_settlement_stage stage={stage} elapsed_ms={:.3}",
                elapsed.as_secs_f64() * 1_000.0
            );
        }
    }
}

fn bitmap_json(bitmap: &RoaringBitmap) -> String {
    let mut json = String::with_capacity(bitmap.len() as usize * 8 + 2);
    json.push('[');
    for (index, value) in bitmap.iter().enumerate() {
        if index != 0 {
            json.push(',');
        }
        json.push_str(&value.to_string());
    }
    json.push(']');
    json
}

fn refresh_canonical_materialized(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT OR IGNORE INTO smart_folder_generation (
             smart_folder_id, database_revision, state, created_at
         )
         SELECT folder.smart_folder_id,
                (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
                'building',
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         FROM smart_folder folder
         WHERE NOT EXISTS (
                   SELECT 1 FROM smart_folder_generation generation
                   WHERE generation.smart_folder_id = folder.smart_folder_id
                     AND generation.state IN ('active', 'building')
               )",
        [],
    )?;
    settle_building_generations(transaction, false, None)
}

fn settle_building_generations(
    transaction: &Transaction<'_>,
    targeted: bool,
    bitmap_context: Option<(
        &RoaringBitmap,
        &mut dyn FnMut(i64) -> rusqlite::Result<RoaringBitmap>,
    )>,
) -> rusqlite::Result<()> {
    let target_filter = if targeted {
        "AND smart_folder_id IN (
             SELECT smart_folder_id FROM picto_smart_rebuild_target
         )"
    } else {
        ""
    };
    let builds = transaction
        .prepare(&format!(
            "SELECT smart_folder_id, generation_id
             FROM smart_folder_generation
             WHERE state = 'building' {target_filter}
             ORDER BY smart_folder_id, generation_id"
        ))?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if builds.is_empty() {
        return Ok(());
    }

    let smart_folder_ids = builds.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    stage_projection_changes(transaction, &smart_folder_ids)?;
    rebuild_dependencies(transaction, &smart_folder_ids)?;
    let next_revision: i64 = transaction.query_row(
        "SELECT revision + 1 FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    transaction.execute(
        "UPDATE projection_write_control
         SET suppress_smart_dirty = 1
         WHERE singleton = 1",
        [],
    )?;
    let builds = builds
        .into_iter()
        .map(|(smart_folder_id, generation_id)| {
            effective_predicate(transaction, smart_folder_id)
                .map(|predicate| (smart_folder_id, generation_id, predicate))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let (context_active, mut context_tags) = match bitmap_context {
        Some((active_roots, tag_bitmap)) => (Some(active_roots), Some(tag_bitmap)),
        None => (None, None),
    };
    let canonical_active = match context_active {
        Some(_) => None,
        None => Some(canonical_active_roots(transaction)?),
    };
    let active_roots = context_active.unwrap_or_else(|| {
        canonical_active
            .as_ref()
            .expect("canonical active set built when no context is supplied")
    });
    let needs_media = builds
        .iter()
        .any(|(_, _, predicate)| predicate_needs_media(predicate));
    stage_active_context(transaction, active_roots, needs_media)?;
    for (smart_folder_id, generation_id, predicate) in builds {
        transaction.execute(
            "DELETE FROM smart_folder_membership WHERE generation_id = ?1",
            [generation_id],
        )?;
        let matches = match context_tags.as_mut() {
            Some(tag_bitmap) => evaluate_staged(transaction, &predicate, active_roots, |tag_id| {
                (*tag_bitmap)(tag_id)
            })?,
            None => evaluate_staged(transaction, &predicate, active_roots, |tag_id| {
                canonical_tag_bitmap(transaction, tag_id)
            })?,
        };
        let mut insert = transaction.prepare_cached(
            "INSERT INTO smart_folder_membership (generation_id, root_item_id)
             VALUES (?1, ?2)",
        )?;
        for root_id in matches {
            insert.execute(rusqlite::params![generation_id, root_id])?;
        }
        let member_count: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM smart_folder_membership WHERE generation_id = ?1",
            [generation_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE smart_folder_generation
             SET state = 'retired'
             WHERE smart_folder_id = ?1 AND state = 'active'",
            [smart_folder_id],
        )?;
        transaction.execute(
            "UPDATE smart_folder_generation
             SET state = 'active', member_count = ?2,
                 database_revision = ?3,
                 activated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE generation_id = ?1 AND state = 'building'",
            rusqlite::params![generation_id, member_count, next_revision],
        )?;
        transaction.execute(
            "DELETE FROM smart_folder_generation
             WHERE smart_folder_id = ?1 AND state = 'retired'",
            [smart_folder_id],
        )?;
    }
    transaction.execute(
        "UPDATE projection_write_control
         SET suppress_smart_dirty = 0
         WHERE singleton = 1",
        [],
    )?;
    Ok(())
}

pub(crate) fn stage_projection_changes(
    transaction: &Transaction<'_>,
    smart_folder_ids: &[i64],
) -> rusqlite::Result<()> {
    if smart_folder_ids.is_empty() {
        return Ok(());
    }
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_smart_projection_delta (
             smart_folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;",
    )?;
    let mut insert = transaction.prepare_cached(
        "INSERT INTO picto_smart_projection_delta(smart_folder_id) VALUES (?1)
         ON CONFLICT(smart_folder_id) DO NOTHING",
    )?;
    for smart_folder_id in smart_folder_ids {
        insert.execute([smart_folder_id])?;
    }
    Ok(())
}

/// Return whether changing one direct tag can affect a smart-folder predicate.
pub(crate) fn tag_affects_any_smart_folder(
    connection: &Connection,
    changed_tag_id: i64,
) -> rusqlite::Result<bool> {
    connection.query_row(
        "WITH affected_query_tag(tag_id) AS (SELECT ?1)
         SELECT EXISTS(
             SELECT 1
             FROM affected_query_tag affected
             JOIN tag ON tag.tag_id = affected.tag_id
             JOIN smart_folder_dependency dependency
               ON dependency.dependency_kind = 'tag'
              AND dependency.dependency_key = CASE
                    WHEN tag.namespace = 'general' THEN tag.subtag
                    ELSE tag.namespace || ':' || tag.subtag
                  END
         )",
        [changed_tag_id],
        |row| row.get(0),
    )
}

fn rebuild_dependencies(
    transaction: &Transaction<'_>,
    smart_folder_ids: &[i64],
) -> rusqlite::Result<()> {
    let mut insert = transaction.prepare_cached(
        "INSERT INTO smart_folder_dependency (
             smart_folder_id, dependency_kind, dependency_key
         ) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
    )?;
    for smart_folder_id in smart_folder_ids {
        transaction.execute(
            "DELETE FROM smart_folder_dependency WHERE smart_folder_id = ?1",
            [smart_folder_id],
        )?;
        let predicate = effective_predicate(transaction, *smart_folder_id)?;
        let mut dependencies = BTreeSet::new();
        for rule in predicate.groups.iter().flat_map(|group| &group.rules) {
            if rule.field == "tags" {
                for tag in rule.values.as_deref().unwrap_or_default() {
                    let (namespace, subtag) = split_tag(tag);
                    let key = if namespace == "general" {
                        subtag
                    } else {
                        format!("{namespace}:{subtag}")
                    };
                    dependencies.insert(("tag", key));
                }
            } else {
                let kind = if is_root_owned_rule(&rule.field) {
                    "root_field"
                } else {
                    "media_field"
                };
                dependencies.insert((kind, canonical_dependency_field(&rule.field).to_string()));
            }
        }
        for (kind, key) in dependencies {
            insert.execute(rusqlite::params![smart_folder_id, kind, key])?;
        }
    }
    Ok(())
}

fn canonical_dependency_field(field: &str) -> &str {
    match field {
        "source_url" | "source_urls_json" => "source_urls",
        "imported" | "imported_at" | "imported_date" | "date_imported" => "date_added",
        "captured" | "captured_at" | "captured_date" => "date_captured",
        field => field,
    }
}

pub(crate) fn effective_predicate(
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

/// Re-evaluate a bounded active-root set while resolving tag ownership from
/// the candidate bitmap projection. Non-tag rules continue to use their
/// indexed SQL predicates; tag ownership is never expanded into persistent
/// relationship rows.
pub(crate) fn evaluate_impacted_with_tag_bitmaps(
    connection: &Connection,
    predicate: &SmartFolderPredicate,
    active_roots: &RoaringBitmap,
    tag_bitmap: impl FnMut(i64) -> rusqlite::Result<RoaringBitmap>,
) -> rusqlite::Result<RoaringBitmap> {
    if predicate.groups.is_empty() || active_roots.is_empty() {
        return Ok(RoaringBitmap::new());
    }
    stage_active_context(connection, active_roots, predicate_needs_media(predicate))?;
    evaluate_staged(connection, predicate, active_roots, tag_bitmap)
}

fn predicate_needs_media(predicate: &SmartFolderPredicate) -> bool {
    predicate
        .groups
        .iter()
        .flat_map(|group| &group.rules)
        .any(|rule| rule.field != "tags" && !is_root_owned_rule(&rule.field))
}

/// Stage the active root set — and, when media rules will run, the canonical
/// group members owned by those roots — into connection-local temp tables.
fn stage_active_context(
    connection: &Connection,
    active_roots: &RoaringBitmap,
    include_group_members: bool,
) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_smart_impacted_root (
             root_item_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_smart_group_member (
             root_id INTEGER NOT NULL,
             media_id INTEGER NOT NULL,
             PRIMARY KEY (root_id, media_id)
         ) WITHOUT ROWID;
         DELETE FROM picto_smart_impacted_root;
         DELETE FROM picto_smart_group_member;",
    )?;
    {
        let mut insert = connection
            .prepare_cached("INSERT INTO picto_smart_impacted_root(root_item_id) VALUES (?1)")?;
        for root_id in active_roots {
            insert.execute([root_id])?;
        }
    }
    if include_group_members {
        let mut insert = connection.prepare_cached(
            "INSERT INTO picto_smart_group_member(root_id, media_id) VALUES (?1, ?2)",
        )?;
        for (group_id, members) in crate::canonical_bitmap::load_domain(
            connection,
            crate::canonical_bitmap::BitmapDomain::GroupMember,
        )? {
            let Ok(group_root) = u32::try_from(group_id) else {
                continue;
            };
            if !active_roots.contains(group_root) {
                continue;
            }
            for media_id in members.iter().map(i64::from) {
                insert.execute(rusqlite::params![group_id, media_id])?;
            }
        }
    }
    Ok(())
}

/// Evaluate against an already-staged active context.
fn evaluate_staged(
    connection: &Connection,
    predicate: &SmartFolderPredicate,
    active_roots: &RoaringBitmap,
    mut tag_bitmap: impl FnMut(i64) -> rusqlite::Result<RoaringBitmap>,
) -> rusqlite::Result<RoaringBitmap> {
    if predicate.groups.is_empty() || active_roots.is_empty() {
        return Ok(RoaringBitmap::new());
    }
    let mut result = active_roots.clone();
    for group in &predicate.groups {
        let mut group_result = match group.match_mode {
            MatchMode::All => active_roots.clone(),
            MatchMode::Any => RoaringBitmap::new(),
        };
        for rule in &group.rules {
            let matches = if rule.field == "tags" {
                tag_rule_bitmap(connection, rule, active_roots, &mut tag_bitmap)?
            } else {
                single_rule_roots(connection, rule)?
            };
            match group.match_mode {
                MatchMode::All => group_result &= matches,
                MatchMode::Any => group_result |= matches,
            }
        }
        if group.negate {
            group_result = active_roots - &group_result;
        }
        result &= group_result;
    }
    Ok(result)
}

fn tag_rule_bitmap(
    connection: &Connection,
    rule: &PredicateRule,
    active_roots: &RoaringBitmap,
    tag_bitmap: &mut impl FnMut(i64) -> rusqlite::Result<RoaringBitmap>,
) -> rusqlite::Result<RoaringBitmap> {
    let values = rule.values.as_deref().unwrap_or_default();
    if values.is_empty() {
        return Ok(
            if matches!(rule.op.as_str(), "do_not_include" | "exclude") {
                active_roots.clone()
            } else {
                RoaringBitmap::new()
            },
        );
    }
    let mut sets = Vec::with_capacity(values.len());
    for value in values {
        let (namespace, subtag) = split_tag(value);
        let tag_id = connection
            .query_row(
                "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                rusqlite::params![namespace, subtag],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let members = match tag_id {
            Some(tag_id) => tag_bitmap(tag_id)?,
            None => RoaringBitmap::new(),
        };
        sets.push(members & active_roots);
    }
    let combined = match rule.op.as_str() {
        "include" | "include_all" => sets
            .into_iter()
            .reduce(|left, right| left & right)
            .unwrap_or_default(),
        "include_any" => sets
            .into_iter()
            .reduce(|left, right| left | right)
            .unwrap_or_default(),
        "do_not_include" | "exclude" => {
            let excluded = sets
                .into_iter()
                .reduce(|left, right| left | right)
                .unwrap_or_default();
            active_roots - &excluded
        }
        op => return Err(invalid(format!("Unknown tag operator: {op}"))),
    };
    Ok(combined)
}

fn rule_condition(rule: &PredicateRule, arguments: &mut Vec<Value>) -> rusqlite::Result<String> {
    match rule.field.as_str() {
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
        "color" => color_condition(rule, arguments),
        field => Err(invalid(format!("Unknown smart-folder field: {field}"))),
    }
}

fn is_root_owned_rule(field: &str) -> bool {
    matches!(
        field,
        "tags" | "rating" | "notes" | "name" | "source_url" | "source_urls"
    )
}

fn root_rule_condition(
    rule: &PredicateRule,
    arguments: &mut Vec<Value>,
) -> rusqlite::Result<String> {
    match rule.field.as_str() {
        "rating" => numeric_condition("metadata.rating", rule, arguments),
        "notes" => text_condition("metadata.notes", rule, arguments),
        "name" => text_condition("COALESCE(metadata.name, cover_asset.name)", rule, arguments),
        "source_url" | "source_urls" => {
            text_condition("metadata.source_urls_json", rule, arguments)
        }
        field => Err(invalid(format!(
            "Unknown root-owned smart-folder field: {field}"
        ))),
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
    use crate::store::schema::create_canonical_v1;
    use rusqlite::params;

    fn connection() -> Connection {
        canonical_connection()
    }

    fn canonical_connection() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();
        connection
    }

    fn canonical_media(connection: &Connection, item_id: i64, name: &str, rating: i64) {
        connection
            .execute_batch(&format!(
                "INSERT INTO media_file (
                     file_id, file_hash, mime_type, size_bytes, created_at
                 ) VALUES ({item_id}, 'hash-{item_id}', 'image/png', 10, 'now');
                 INSERT INTO library_item (
                     item_id, item_key, kind, created_at, updated_at
                 ) VALUES ({item_id}, 'item-{item_id}', 'media', 'now', 'now');
                 INSERT INTO library_root(item_id, lifecycle)
                 VALUES ({item_id}, 'active');
                 INSERT INTO media_asset (
                     item_id, file_id, name, imported_at, updated_at
                 ) VALUES ({item_id}, {item_id}, '{name}', 'now', 'now');
                 INSERT INTO root_metadata (
                     root_item_id, name, rating, notes, source_urls_json, updated_at
                 ) VALUES ({item_id}, '{name}', {rating}, NULL, '[]', 'now');"
            ))
            .unwrap();
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
                "INSERT INTO media_asset (item_id, file_id, name, imported_at, updated_at)
                 VALUES (?1, ?1, ?2, '2026-01-01', '2026-01-01')",
                params![item_id, name],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO root_metadata (
                     root_item_id, name, rating, notes, source_urls_json, updated_at
                 ) VALUES (?1, ?2, ?3, NULL, '[]', '2026-01-01')",
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

    fn evaluate_predicate(connection: &Connection, predicate: &SmartFolderPredicate) -> Vec<i64> {
        evaluate_canonical_predicate(connection, predicate)
            .unwrap()
            .iter()
            .map(i64::from)
            .collect()
    }

    fn tag_rule(op: &str, values: &[&str]) -> PredicateRule {
        PredicateRule {
            field: "tags".to_string(),
            op: op.to_string(),
            value: None,
            value2: None,
            values: Some(values.iter().map(|value| (*value).to_string()).collect()),
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

        assert_eq!(evaluate(&connection, 11).unwrap(), vec![1]);
        assert_eq!(evaluate(&connection, 11).unwrap().len() as i64, 1);
    }

    #[test]
    fn collection_members_can_satisfy_distinct_and_rules() {
        let mut connection = connection();
        media(&connection, 1, "one", "active", "one", 5, 10);
        media(&connection, 2, "two", "active", "two", 2, 200);
        connection
            .execute(
                "UPDATE media_file SET pixel_width = 500 WHERE file_id = 1",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                 VALUES (10, 'collection', 'collection', '2026-01-01', '2026-01-01')",
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
                "INSERT INTO root_metadata (
                     root_item_id, name, source_urls_json, updated_at
                 ) VALUES (10, 'Collection', '[]', '2026-01-01')",
                [],
            )
            .unwrap();
        {
            let transaction = connection.transaction().unwrap();
            crate::canonical_bitmap::replace_bitmap(
                &transaction,
                crate::canonical_bitmap::BitmapDomain::GroupMember,
                10,
                1,
                &RoaringBitmap::from_iter([1_u32, 2]),
            )
            .unwrap();
            transaction.commit().unwrap();
        }

        let predicate = SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules: vec![
                    rule("width", "gte", serde_json::json!(400)),
                    rule("file_size", "gte", serde_json::json!(100)),
                ],
            }],
        };

        assert_eq!(evaluate_predicate(&connection, &predicate), vec![10]);
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

        assert_eq!(evaluate_predicate(&connection, &predicate), vec![1]);
    }

    #[test]
    fn metadata_rules_read_root_metadata_instead_of_media_assets() {
        let connection = connection();
        media(&connection, 1, "root-owned", "active", "media-owned", 1, 10);
        connection
            .execute(
                "UPDATE root_metadata
                 SET name = 'root-owned', rating = 5
                 WHERE root_item_id = 1",
                [],
            )
            .unwrap();
        let predicate = SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules: vec![
                    rule("name", "is", serde_json::json!("root-owned")),
                    rule("rating", "gte", serde_json::json!(5)),
                ],
            }],
        };

        assert_eq!(evaluate_predicate(&connection, &predicate), vec![1]);
    }

    #[test]
    fn field_refresh_keeps_tag_rules_exact_for_mixed_predicates() {
        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "one", 5);
        canonical_media(&connection, 2, "two", 2);
        connection
            .execute(
                "INSERT INTO tag(tag_id, namespace, subtag) VALUES (10, 'general', 'blue')",
                [],
            )
            .unwrap();
        {
            let transaction = connection.transaction().unwrap();
            crate::canonical_bitmap::replace_bitmap(
                &transaction,
                crate::canonical_bitmap::BitmapDomain::Tag,
                10,
                1,
                &RoaringBitmap::from_iter([1_u32, 2]),
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        folder(
            &connection,
            10,
            None,
            &predicate(vec![
                tag_rule("include", &["blue"]),
                rule("rating", "gte", serde_json::json!(4)),
            ]),
        );
        {
            let transaction = connection.transaction().unwrap();
            refresh_materialized(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(evaluate(&connection, 10).unwrap(), vec![1]);

        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "UPDATE root_metadata SET rating = 5 WHERE root_item_id = 2",
                [],
            )
            .unwrap();
        refresh_impacted_roots(
            &transaction,
            &RoaringBitmap::from_iter([2]),
            &["rating"],
            &[],
        )
        .unwrap();
        assert_eq!(evaluate(&transaction, 10).unwrap(), vec![1, 2]);
        transaction.commit().unwrap();
    }

    #[test]
    fn impacted_tag_rules_use_candidate_bitmaps_without_relationship_rows() {
        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "one", 5);
        canonical_media(&connection, 2, "two", 2);
        connection
            .execute(
                "INSERT INTO tag(tag_id, namespace, subtag)
                 VALUES (10, 'general', 'blue'),
                        (11, 'general', 'round')",
                [],
            )
            .unwrap();
        let predicate = SmartFolderPredicate {
            groups: vec![SmartRuleGroup {
                match_mode: MatchMode::All,
                negate: false,
                rules: vec![
                    tag_rule("include_all", &["blue", "round"]),
                    rule("rating", "gte", serde_json::json!(4)),
                ],
            }],
        };
        let transaction = connection.transaction().unwrap();
        let matches = evaluate_impacted_with_tag_bitmaps(
            &transaction,
            &predicate,
            &RoaringBitmap::from_iter([1, 2]),
            |tag_id| {
                Ok(match tag_id {
                    10 => RoaringBitmap::from_iter([1, 2]),
                    11 => RoaringBitmap::from_iter([1]),
                    _ => RoaringBitmap::new(),
                })
            },
        )
        .unwrap();

        assert_eq!(matches, RoaringBitmap::from_iter([1]));
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

        assert_eq!(evaluate_predicate(&connection, &predicate), vec![1]);
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

        assert!(evaluate(&connection, 10).is_err());
    }

    #[test]
    fn canonical_impacted_root_refresh_updates_active_generation_exactly() {
        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "one", 5);
        canonical_media(&connection, 2, "two", 2);
        folder(
            &connection,
            10,
            None,
            &predicate(vec![rule("rating", "gte", serde_json::json!(4))]),
        );
        {
            let transaction = connection.transaction().unwrap();
            refresh_materialized(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        let generation_id: i64 = connection
            .query_row(
                "SELECT generation_id FROM smart_folder_generation
                 WHERE smart_folder_id = 10 AND state = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(evaluate(&connection, 10).unwrap(), vec![1]);
        assert_eq!(evaluate(&connection, 10).unwrap().len() as i64, 1);

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "UPDATE root_metadata SET rating = 5 WHERE root_item_id = 2",
                    [],
                )
                .unwrap();
            let roots = RoaringBitmap::from_iter([2]);
            refresh_impacted_roots(&transaction, &roots, &["rating"], &[]).unwrap();
            assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 2);
            assert_eq!(
                transaction
                    .query_row(
                        "SELECT member_count FROM smart_folder_generation
                         WHERE generation_id = ?1 AND state = 'active'",
                        [generation_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                2
            );
            transaction.commit().unwrap();
        }

        assert_eq!(evaluate(&connection, 10).unwrap(), vec![1, 2]);
        assert_eq!(evaluate(&connection, 10).unwrap().len() as i64, 2);
        assert_eq!(
            connection
                .query_row(
                    "SELECT generation_id FROM smart_folder_generation
                     WHERE smart_folder_id = 10 AND state = 'active'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            generation_id
        );

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute("DELETE FROM library_root WHERE item_id = 2", [])
                .unwrap();
            let roots = RoaringBitmap::from_iter([2]);
            refresh_impacted_roots(&transaction, &roots, &["lifecycle"], &[]).unwrap();
            assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 1);
            transaction.commit().unwrap();
        }
        assert_eq!(evaluate(&connection, 10).unwrap(), vec![1]);
        assert_eq!(evaluate(&connection, 10).unwrap().len() as i64, 1);
    }

    #[test]
    fn canonical_name_rule_uses_cover_media_when_root_name_is_missing() {
        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "media fallback", 0);
        connection
            .execute(
                "UPDATE root_metadata SET name = NULL WHERE root_item_id = 1",
                [],
            )
            .unwrap();
        folder(
            &connection,
            10,
            None,
            &predicate(vec![rule(
                "name",
                "is",
                serde_json::json!("media fallback"),
            )]),
        );

        let transaction = connection.transaction().unwrap();
        refresh_materialized(&transaction).unwrap();
        assert_eq!(evaluate(&transaction, 10).unwrap(), vec![1]);
        assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 1);
        transaction.commit().unwrap();
    }

    #[test]
    fn canonical_definition_rebuild_activates_complete_shadow_generation() {
        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "one", 5);
        canonical_media(&connection, 2, "two", 2);
        folder(
            &connection,
            10,
            None,
            &predicate(vec![rule("rating", "gte", serde_json::json!(4))]),
        );
        {
            let transaction = connection.transaction().unwrap();
            refresh_materialized(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        let first_generation: i64 = connection
            .query_row(
                "SELECT generation_id FROM smart_folder_generation
                 WHERE smart_folder_id = 10 AND state = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "UPDATE smart_folder SET predicate_json = ?1
                     WHERE smart_folder_id = 10",
                    [predicate(vec![rule("rating", "gte", serde_json::json!(2))])],
                )
                .unwrap();
            assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 1);
            assert_eq!(
                transaction
                    .query_row(
                        "SELECT COUNT(*) FROM smart_folder_generation
                         WHERE smart_folder_id = 10 AND state = 'building'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
            refresh_materialized(&transaction).unwrap();
            assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 2);
            transaction.commit().unwrap();
        }

        let active: (i64, i64) = connection
            .query_row(
                "SELECT generation_id, member_count
                 FROM smart_folder_generation
                 WHERE smart_folder_id = 10 AND state = 'active'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_ne!(active.0, first_generation);
        assert_eq!(active.1, 2);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM smart_folder_generation
                     WHERE smart_folder_id = 10 AND state != 'active'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn targeted_rebuild_leaves_unrelated_pending_generation_unsettled() {
        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "one", 5);
        canonical_media(&connection, 2, "two", 2);
        folder(
            &connection,
            10,
            None,
            &predicate(vec![rule("rating", "gte", serde_json::json!(4))]),
        );
        folder(
            &connection,
            11,
            None,
            &predicate(vec![rule("name", "is", serde_json::json!("two"))]),
        );
        {
            let transaction = connection.transaction().unwrap();
            refresh_materialized(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        let unrelated_active: i64 = connection
            .query_row(
                "SELECT generation_id FROM smart_folder_generation
                 WHERE smart_folder_id = 11 AND state = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "UPDATE smart_folder SET predicate_json = ?1
                 WHERE smart_folder_id = 10",
                [predicate(vec![rule("rating", "gte", serde_json::json!(2))])],
            )
            .unwrap();
        transaction
            .execute(
                "UPDATE smart_folder SET predicate_json = ?1
                 WHERE smart_folder_id = 11",
                [predicate(vec![rule(
                    "name",
                    "is",
                    serde_json::json!("one"),
                )])],
            )
            .unwrap();

        rebuild_generations(
            &transaction,
            &[10],
            &RoaringBitmap::from_iter([1, 2]),
            |_| Ok(RoaringBitmap::new()),
        )
        .unwrap();

        assert_eq!(evaluate(&transaction, 10).unwrap(), vec![1, 2]);
        assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 2);
        assert_eq!(evaluate(&transaction, 11).unwrap(), vec![2]);
        assert_eq!(
            transaction
                .query_row(
                    "SELECT generation_id FROM smart_folder_generation
                     WHERE smart_folder_id = 11 AND state = 'active'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            unrelated_active
        );
        assert_eq!(
            transaction
                .query_row(
                    "SELECT COUNT(*) FROM smart_folder_generation
                     WHERE smart_folder_id = 11 AND state = 'building'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        transaction.commit().unwrap();
    }

    #[test]
    fn impacted_root_refresh_work_is_bounded_to_dependent_active_folders() {
        const UNRELATED_FOLDERS: i64 = 64;

        let mut connection = canonical_connection();
        canonical_media(&connection, 1, "one", 5);
        canonical_media(&connection, 2, "two", 2);
        folder(
            &connection,
            10,
            None,
            &predicate(vec![rule("rating", "gte", serde_json::json!(4))]),
        );
        for offset in 0..UNRELATED_FOLDERS {
            folder(
                &connection,
                100 + offset,
                None,
                &predicate(vec![rule("name", "is", serde_json::json!("two"))]),
            );
        }
        {
            let transaction = connection.transaction().unwrap();
            refresh_materialized(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        let rating_generation: i64 = connection
            .query_row(
                "SELECT generation_id FROM smart_folder_generation
                 WHERE smart_folder_id = 10 AND state = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "UPDATE smart_folder
                 SET predicate_json = ?1
                 WHERE smart_folder_id >= 100",
                [predicate(vec![rule(
                    "name",
                    "is",
                    serde_json::json!("one"),
                )])],
            )
            .unwrap();
        assert_eq!(
            transaction
                .query_row(
                    "SELECT COUNT(*) FROM smart_folder_generation WHERE state = 'building'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            UNRELATED_FOLDERS
        );
        transaction
            .execute(
                "UPDATE root_metadata SET rating = 5 WHERE root_item_id = 2",
                [],
            )
            .unwrap();
        refresh_impacted_roots(
            &transaction,
            &RoaringBitmap::from_iter([2]),
            &["rating"],
            &[],
        )
        .unwrap();

        assert_eq!(evaluate(&transaction, 10).unwrap().len() as i64, 2);
        assert_eq!(
            transaction
                .query_row(
                    "SELECT generation_id FROM smart_folder_generation
                     WHERE smart_folder_id = 10 AND state = 'active'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            rating_generation
        );
        assert_eq!(
            transaction
                .query_row(
                    "SELECT COUNT(*) FROM smart_folder_generation WHERE state = 'building'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            UNRELATED_FOLDERS
        );
        assert_eq!(evaluate(&transaction, 100).unwrap(), vec![2]);
        transaction.commit().unwrap();
    }
}
