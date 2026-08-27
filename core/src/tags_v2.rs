//! Tag-manager reads and mutations for the replacement backend.

use std::collections::{BTreeMap, BTreeSet};

use roaring::RoaringBitmap;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::projection_v2::{ProjectionStore, TagGraphProjectionDelta, TagIdentityProjectionChange};
use crate::store::history::{
    HistoryDescriptor, SemanticHistoryPayload, SemanticHistoryRecord, SemanticMembershipDelta,
    SemanticRootTagState, SemanticTagGraphDelta, SemanticTagIdentityState, SemanticTagRootSet,
};

const MAX_LIMIT: i64 = 500;
const MAX_RECEIPT_ITEM_IDS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TagSummary {
    #[ts(type = "number")]
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    #[ts(type = "number")]
    pub media_count: i64,
    #[ts(type = "number")]
    pub root_count: i64,
}

impl TagSummary {
    pub fn name(&self) -> String {
        if self.namespace == "general" {
            self.subtag.clone()
        } else {
            format!("{}:{}", self.namespace, self.subtag)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TagPage {
    pub tags: Vec<TagSummary>,
    pub next_cursor: Option<String>,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn list(
    application: &Application,
    namespace: Option<&str>,
    search: Option<&str>,
    cursor: Option<&str>,
    limit: i64,
) -> Result<TagPage, String> {
    let limit = limit.clamp(1, MAX_LIMIT);
    let cursor = cursor
        .filter(|value| !value.is_empty())
        .map(str::parse::<i64>)
        .transpose()
        .map_err(|_| "Invalid tag cursor".to_string())?
        .unwrap_or(0);
    let search = search
        .map(normalize_search)
        .filter(|value| !value.is_empty())
        .map(|value| format!("%{value}%"));
    application.store().read(|connection| {
        let mut statement = connection.prepare(
            "SELECT t.tag_id, t.namespace, t.subtag,
                    COALESCE(summary.visible_root_count, 0) AS media_count,
                    COALESCE(summary.visible_root_count, 0) AS root_count
             FROM tag t
             LEFT JOIN tag_summary summary ON summary.tag_id = t.tag_id
             WHERE t.tag_id > ?1
               AND (?2 IS NULL OR t.namespace = ?2)
               AND (?3 IS NULL OR LOWER(t.subtag) LIKE ?3)
             ORDER BY t.tag_id
             LIMIT ?4",
        )?;
        let mut tags = statement
            .query_map(params![cursor, namespace, search, limit + 1], |row| {
                Ok(TagSummary {
                    tag_id: row.get(0)?,
                    namespace: row.get(1)?,
                    subtag: row.get(2)?,
                    media_count: row.get(3)?,
                    root_count: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_cursor =
            (tags.len() as i64 > limit).then(|| tags[limit as usize - 1].tag_id.to_string());
        tags.truncate(limit as usize);
        Ok(TagPage {
            tags,
            next_cursor,
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

pub fn namespace_counts(application: &Application) -> Result<Vec<(String, i64)>, String> {
    application.store().read(|connection| {
        connection
            .prepare(
                "SELECT namespace, COUNT(*) FROM tag
                 GROUP BY namespace ORDER BY namespace",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
    })
}

pub fn unused_count(application: &Application) -> Result<i64, String> {
    application.store().read(|connection| {
        connection.query_row(
            "SELECT COUNT(*) FROM tag t
             LEFT JOIN tag_summary summary ON summary.tag_id = t.tag_id
             WHERE COALESCE(summary.assignment_count, 0) = 0",
            [],
            |row| row.get(0),
        )
    })
}

impl Application {
    pub fn rename_or_merge_tag(
        &self,
        tag_id: i64,
        new_name: &str,
    ) -> Result<MutationReceipt, String> {
        let (namespace, subtag) = parse_tag(new_name)?;
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed(
            tag_history("tags.rename_or_merge", "Rename tag"),
            |transaction| {
                require_tag(transaction, tag_id)?;
                let target = transaction
                    .query_row(
                        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                        params![namespace, subtag],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?;
                let scope = [Some(tag_id), target]
                    .into_iter()
                    .flatten()
                    .collect::<BTreeSet<_>>();
                let before = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                match target {
                    Some(target_id) if target_id != tag_id => {
                        let smart_roots = graph_affected_roots(transaction, [tag_id, target_id])?;
                        move_tag_assignments(transaction, tag_id, target_id)?;
                        transaction.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;
                        let projection = TagGraphProjectionDelta {
                            identities: vec![TagIdentityProjectionChange {
                                source_tag_id: tag_id,
                                target_tag_id: Some(target_id),
                                remove_tag: true,
                            }],
                        };
                        refresh_tag_summaries(
                            transaction,
                            affected_tag_ids(&projection).into_iter(),
                        )?;
                        let after = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                        let record = semantic_tag_record(&before, &after, &smart_roots)?;
                        let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                            unreachable!()
                        };
                        Ok((
                            smart_roots.into_iter().collect::<Vec<_>>(),
                            redo.clone(),
                            Some(record),
                            true,
                        ))
                    }
                    Some(_) => Ok((Vec::new(), SemanticTagGraphDelta::default(), None, false)),
                    None => {
                        let smart_roots = graph_affected_roots(transaction, [tag_id])?;
                        let (old_namespace, old_subtag) = before
                            .identities
                            .get(&tag_id)
                            .expect("required tag identity is present before rename");
                        let old_name = canonical_tag_name(old_namespace, old_subtag);
                        let new_name = canonical_tag_name(&namespace, &subtag);
                        stage_tag_rename_smart_targets(transaction, &old_name, &new_name)?;
                        transaction.execute(
                            "UPDATE tag SET namespace = ?1, subtag = ?2 WHERE tag_id = ?3",
                            params![namespace, subtag, tag_id],
                        )?;
                        settle_tag_rename_smart_targets(
                            transaction,
                            &smart_roots,
                            tag_id,
                            &old_name,
                            &new_name,
                        )?;
                        refresh_tag_summaries(transaction, [tag_id])?;
                        let after = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                        let record = semantic_tag_record(&before, &after, &smart_roots)?;
                        let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                            unreachable!()
                        };
                        Ok((
                            smart_roots.into_iter().collect(),
                            redo.clone(),
                            Some(record),
                            true,
                        ))
                    }
                }
            },
            apply_semantic_tag_projection,
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn rename_tag_group(
        &self,
        namespace: &str,
        new_namespace: &str,
    ) -> Result<MutationReceipt, String> {
        let namespace = normalize_group(namespace)?;
        let new_namespace = normalize_group(new_namespace)?;
        if namespace == "general" {
            return Err("The General group cannot be renamed".to_string());
        }
        if namespace == new_namespace {
            return Ok(tag_receipt(self.store().revision()?));
        }
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed(
            tag_history("tags.group.rename", "Rename tag group"),
            |transaction| {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tag WHERE namespace = ?1)",
                    [&namespace],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(invalid(format!("Tag group {namespace} does not exist")));
                }
                let scope = namespace_move_scope(transaction, &namespace, &new_namespace)?;
                let before = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                let mut smart_roots = graph_affected_roots_for_namespace(transaction, &namespace)?;
                let (roots, projection) = move_namespace(transaction, &namespace, &new_namespace)?;
                smart_roots.extend(roots);
                smart_roots.extend(graph_affected_roots(
                    transaction,
                    affected_tag_ids(&projection),
                )?);
                refresh_tag_summaries(transaction, affected_tag_ids(&projection).into_iter())?;
                let after = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                let record = semantic_tag_record(&before, &after, &smart_roots)?;
                let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                    unreachable!()
                };
                Ok((
                    smart_roots.into_iter().collect::<Vec<_>>(),
                    redo.clone(),
                    Some(record),
                    true,
                ))
            },
            apply_semantic_tag_projection,
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn delete_tag_group(&self, namespace: &str) -> Result<MutationReceipt, String> {
        let namespace = normalize_group(namespace)?;
        if namespace == "general" {
            return Err("The General group cannot be deleted".to_string());
        }
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed(
            tag_history("tags.group.delete", "Delete tag group"),
            |transaction| {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tag WHERE namespace = ?1)",
                    [&namespace],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(invalid(format!("Tag group {namespace} does not exist")));
                }
                let scope = namespace_move_scope(transaction, &namespace, "general")?;
                let before = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                let mut smart_roots = graph_affected_roots_for_namespace(transaction, &namespace)?;
                let (roots, projection) = move_namespace(transaction, &namespace, "general")?;
                smart_roots.extend(roots);
                smart_roots.extend(graph_affected_roots(
                    transaction,
                    affected_tag_ids(&projection),
                )?);
                refresh_tag_summaries(transaction, affected_tag_ids(&projection).into_iter())?;
                let after = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                let record = semantic_tag_record(&before, &after, &smart_roots)?;
                let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                    unreachable!()
                };
                Ok((
                    smart_roots.into_iter().collect::<Vec<_>>(),
                    redo.clone(),
                    Some(record),
                    true,
                ))
            },
            apply_semantic_tag_projection,
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<MutationReceipt, String> {
        let (item_ids, revision, _) = self.semantic_undoable_transaction(
            tag_history("tags.delete", "Delete tag"),
            |transaction| {
                require_tag(transaction, tag_id)?;
                let scope = BTreeSet::from([tag_id]);
                let before = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                let smart_roots = graph_affected_roots(transaction, [tag_id])?;
                transaction.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;
                let projection = TagGraphProjectionDelta {
                    identities: vec![TagIdentityProjectionChange {
                        source_tag_id: tag_id,
                        target_tag_id: None,
                        remove_tag: true,
                    }],
                };
                refresh_tag_summaries(transaction, affected_tag_ids(&projection).into_iter())?;
                let after = semantic_tag_snapshot(transaction, scope.iter().copied())?;
                let record = semantic_tag_record(&before, &after, &smart_roots)?;
                let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                    unreachable!()
                };
                Ok((
                    smart_roots.into_iter().collect::<Vec<_>>(),
                    redo.clone(),
                    record,
                ))
            },
            apply_semantic_tag_projection,
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn delete_unused_tags(&self) -> Result<MutationReceipt, String> {
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            tag_history("tags.delete_unused", "Delete unused tags"),
            |transaction| {
                let deleted = transaction.execute(
                    "DELETE FROM tag
                     WHERE COALESCE((
                               SELECT summary.assignment_count
                               FROM tag_summary summary
                               WHERE summary.tag_id = tag.tag_id
                           ), 0) = 0
                    ",
                    [],
                )?;
                Ok(((), (), deleted != 0))
            },
            |_, ()| Ok(()),
        )?;
        Ok(tag_receipt(revision))
    }
}

fn tag_history(command: &str, label: &str) -> HistoryDescriptor {
    HistoryDescriptor::new(
        command,
        label,
        vec![
            resources::LIBRARY.to_string(),
            resources::SIDEBAR.to_string(),
            resources::SMART_FOLDERS.to_string(),
            resources::TAGS.to_string(),
        ],
        Vec::new(),
    )
}

/// Return roots directly assigned any seed tag.
fn graph_affected_roots(
    connection: &rusqlite::Connection,
    tag_ids: impl IntoIterator<Item = i64>,
) -> rusqlite::Result<BTreeSet<i64>> {
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_tag_graph_seed (
             tag_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_tag_graph_seed;",
    )?;
    {
        let mut insert = connection.prepare_cached(
            "INSERT INTO picto_tag_graph_seed(tag_id) VALUES (?1)
             ON CONFLICT DO NOTHING",
        )?;
        for tag_id in tag_ids {
            insert.execute([tag_id])?;
        }
    }
    query_graph_affected_roots(connection)
}

fn graph_affected_roots_for_namespace(
    connection: &rusqlite::Connection,
    namespace: &str,
) -> rusqlite::Result<BTreeSet<i64>> {
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_tag_graph_seed (
             tag_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_tag_graph_seed;",
    )?;
    connection.execute(
        "INSERT INTO picto_tag_graph_seed(tag_id)
         SELECT tag_id FROM tag WHERE namespace = ?1",
        [namespace],
    )?;
    query_graph_affected_roots(connection)
}

fn query_graph_affected_roots(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<BTreeSet<i64>> {
    connection
        .prepare(
            "SELECT DISTINCT relation.root_item_id
             FROM picto_tag_graph_seed affected
             JOIN root_tag relation ON relation.tag_id = affected.tag_id
             ORDER BY relation.root_item_id",
        )?
        .query_map([], |row| row.get(0))?
        .collect()
}

fn canonical_tag_name(namespace: &str, subtag: &str) -> String {
    if namespace == "general" {
        subtag.to_string()
    } else {
        format!("{namespace}:{subtag}")
    }
}

/// Snapshot only the smart folders reached by either rename identity. The
/// schema trigger may create replacement generations for these folders; the
/// snapshot lets incremental settlement remove only builds caused here.
fn stage_tag_rename_smart_targets(
    transaction: &rusqlite::Transaction<'_>,
    old_name: &str,
    new_name: &str,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_tag_rename_smart_target (
             smart_folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_tag_rename_existing_build (
             generation_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS picto_tag_rename_dependency_remap (
             smart_folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_tag_rename_smart_target;
         DELETE FROM picto_tag_rename_existing_build;
         DELETE FROM picto_tag_rename_dependency_remap;",
    )?;
    transaction.execute(
        "INSERT INTO picto_tag_rename_smart_target(smart_folder_id)
         SELECT DISTINCT smart_folder_id
         FROM smart_folder_dependency
         WHERE dependency_kind = 'tag'
           AND dependency_key IN (?1, ?2)",
        params![old_name, new_name],
    )?;
    transaction.execute(
        "INSERT INTO picto_tag_rename_existing_build(generation_id)
         SELECT generation.generation_id
         FROM smart_folder_generation generation
         JOIN picto_tag_rename_smart_target target
           ON target.smart_folder_id = generation.smart_folder_id
         WHERE generation.state = 'building'",
        [],
    )?;
    Ok(())
}

/// Incrementally re-evaluate affected roots for both identities. Temporarily
/// mapping old-only dependencies to the new identity lets the canonical smart
/// refresh select the exact old and new folders while evaluating their
/// unchanged predicates against the post-rename tag graph.
fn settle_tag_rename_smart_targets(
    transaction: &rusqlite::Transaction<'_>,
    affected_roots: &BTreeSet<i64>,
    tag_id: i64,
    old_name: &str,
    new_name: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO picto_tag_rename_dependency_remap(smart_folder_id)
         SELECT target.smart_folder_id
         FROM picto_tag_rename_smart_target target
         WHERE EXISTS (
                   SELECT 1 FROM smart_folder_dependency dependency
                   WHERE dependency.smart_folder_id = target.smart_folder_id
                     AND dependency.dependency_kind = 'tag'
                     AND dependency.dependency_key = ?1
               )
           AND NOT EXISTS (
                   SELECT 1 FROM smart_folder_dependency dependency
                   WHERE dependency.smart_folder_id = target.smart_folder_id
                     AND dependency.dependency_kind = 'tag'
                     AND dependency.dependency_key = ?2
               )",
        params![old_name, new_name],
    )?;
    transaction.execute(
        "UPDATE smart_folder_dependency
         SET dependency_key = ?2
         WHERE dependency_kind = 'tag'
           AND dependency_key = ?1
           AND smart_folder_id IN (
               SELECT smart_folder_id FROM picto_tag_rename_dependency_remap
           )",
        params![old_name, new_name],
    )?;

    let roots = affected_roots
        .iter()
        .map(|root_id| {
            u32::try_from(*root_id)
                .map_err(|_| invalid(format!("Root item {root_id} exceeds projection capacity")))
        })
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    crate::smart_v2::refresh_impacted_roots(transaction, &roots, &["tags"], &[tag_id])?;

    transaction.execute(
        "UPDATE smart_folder_dependency
         SET dependency_key = ?1
         WHERE dependency_kind = 'tag'
           AND dependency_key = ?2
           AND smart_folder_id IN (
               SELECT smart_folder_id FROM picto_tag_rename_dependency_remap
           )",
        params![old_name, new_name],
    )?;
    transaction.execute(
        "DELETE FROM smart_folder_generation
         WHERE state = 'building'
           AND smart_folder_id IN (
               SELECT smart_folder_id FROM picto_tag_rename_smart_target
           )
           AND generation_id NOT IN (
               SELECT generation_id FROM picto_tag_rename_existing_build
           )",
        [],
    )?;
    transaction.execute_batch(
        "DELETE FROM picto_tag_rename_smart_target;
         DELETE FROM picto_tag_rename_existing_build;
         DELETE FROM picto_tag_rename_dependency_remap;",
    )?;
    Ok(())
}

#[derive(Default)]
struct SemanticTagSnapshot {
    identities: BTreeMap<i64, (String, String)>,
    root_tags: BTreeSet<(i64, i64)>,
}

fn semantic_tag_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    tag_ids: impl IntoIterator<Item = i64>,
) -> rusqlite::Result<SemanticTagSnapshot> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_tag_history_scope (
             tag_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_tag_history_scope;",
    )?;
    {
        let mut insert = transaction.prepare_cached(
            "INSERT INTO picto_tag_history_scope(tag_id) VALUES (?1)
             ON CONFLICT DO NOTHING",
        )?;
        for tag_id in tag_ids {
            insert.execute([tag_id])?;
        }
    }
    let identities = transaction
        .prepare(
            "SELECT tag.tag_id, tag.namespace, tag.subtag
             FROM tag JOIN picto_tag_history_scope scope USING(tag_id)",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                (row.get::<_, String>(1)?, row.get::<_, String>(2)?),
            ))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let root_tags = transaction
        .prepare(
            "SELECT relation.root_item_id, relation.tag_id
             FROM root_tag relation
             JOIN picto_tag_history_scope scope ON scope.tag_id = relation.tag_id",
        )?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    Ok(SemanticTagSnapshot {
        identities,
        root_tags,
    })
}

fn semantic_tag_record(
    before: &SemanticTagSnapshot,
    after: &SemanticTagSnapshot,
    affected_roots: &BTreeSet<i64>,
) -> rusqlite::Result<SemanticHistoryRecord> {
    Ok(SemanticHistoryRecord::new(
        SemanticHistoryPayload::TagGraph(semantic_tag_direction(after, before, affected_roots)?),
        SemanticHistoryPayload::TagGraph(semantic_tag_direction(before, after, affected_roots)?),
    ))
}

fn semantic_tag_direction(
    current: &SemanticTagSnapshot,
    desired: &SemanticTagSnapshot,
    affected_roots: &BTreeSet<i64>,
) -> rusqlite::Result<SemanticTagGraphDelta> {
    let identity_ids = current
        .identities
        .keys()
        .chain(desired.identities.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let identities = identity_ids
        .iter()
        .filter(|tag_id| current.identities.get(tag_id) != desired.identities.get(tag_id))
        .map(|tag_id| {
            let desired_identity = desired.identities.get(tag_id);
            let fallback = current.identities.get(tag_id);
            let (namespace, subtag) = desired_identity.or(fallback).unwrap();
            SemanticTagIdentityState {
                tag_id: *tag_id,
                namespace: namespace.clone(),
                subtag: subtag.clone(),
                present: desired_identity.is_some(),
            }
        })
        .collect::<Vec<_>>();

    let root_keys = current
        .root_tags
        .iter()
        .chain(desired.root_tags.iter())
        .copied()
        .filter(|key| current.root_tags.contains(key) != desired.root_tags.contains(key))
        .collect::<BTreeSet<_>>();
    let mut clear_by_tag = BTreeMap::<i64, RoaringBitmap>::new();
    let mut desired_groups = BTreeMap::<i64, RoaringBitmap>::new();
    let mut projection_by_tag = BTreeMap::<i64, (RoaringBitmap, RoaringBitmap)>::new();
    for (root_id, tag_id) in root_keys {
        let root_id_u32 = u32::try_from(root_id)
            .map_err(|_| invalid(format!("Root item {root_id} exceeds projection capacity")))?;
        clear_by_tag.entry(tag_id).or_default().insert(root_id_u32);
        match (
            current.root_tags.contains(&(root_id, tag_id)),
            desired.root_tags.contains(&(root_id, tag_id)),
        ) {
            (false, true) => {
                projection_by_tag
                    .entry(tag_id)
                    .or_default()
                    .0
                    .insert(root_id_u32);
            }
            (true, false) => {
                projection_by_tag
                    .entry(tag_id)
                    .or_default()
                    .1
                    .insert(root_id_u32);
            }
            _ => {}
        }
        if desired.root_tags.contains(&(root_id, tag_id)) {
            desired_groups
                .entry(tag_id)
                .or_default()
                .insert(root_id_u32);
        }
    }

    let mut affected_tag_ids = identity_ids;
    affected_tag_ids.extend(clear_by_tag.keys().copied());
    let affected_roots = affected_roots
        .iter()
        .map(|root_id| {
            u32::try_from(*root_id)
                .map_err(|_| invalid(format!("Root item {root_id} exceeds projection capacity")))
        })
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    Ok(SemanticTagGraphDelta {
        identities,
        clear_root_tags: clear_by_tag
            .into_iter()
            .map(|(tag_id, roots)| SemanticTagRootSet { tag_id, roots })
            .collect(),
        root_tags: desired_groups
            .into_iter()
            .map(|(tag_id, roots)| SemanticRootTagState { tag_id, roots })
            .collect(),
        projection_tags: projection_by_tag
            .into_iter()
            .map(|(relation_id, (add, remove))| SemanticMembershipDelta {
                relation_id,
                add,
                remove,
            })
            .collect(),
        removed_tag_ids: current
            .identities
            .keys()
            .filter(|tag_id| !desired.identities.contains_key(tag_id))
            .copied()
            .collect(),
        affected_roots,
        affected_tag_ids: affected_tag_ids.into_iter().collect(),
    })
}

fn apply_semantic_tag_projection(
    projections: &ProjectionStore,
    delta: SemanticTagGraphDelta,
) -> Result<(), String> {
    projections.apply_tag_graph_delta(TagGraphProjectionDelta {
        identities: delta
            .removed_tag_ids
            .iter()
            .map(|tag_id| TagIdentityProjectionChange {
                source_tag_id: *tag_id,
                target_tag_id: None,
                remove_tag: true,
            })
            .collect(),
    })?;
    for change in &delta.projection_tags {
        projections.apply_root_tag_bitmap(change.relation_id, &change.remove, false)?;
        projections.apply_root_tag_bitmap(change.relation_id, &change.add, true)?;
    }
    Ok(())
}

fn move_namespace(
    transaction: &rusqlite::Transaction<'_>,
    source_namespace: &str,
    target_namespace: &str,
) -> rusqlite::Result<(Vec<i64>, TagGraphProjectionDelta)> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_tag_merge (
             source_tag_id INTEGER PRIMARY KEY,
             target_tag_id INTEGER NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM picto_tag_merge;",
    )?;
    transaction.execute(
        "INSERT INTO picto_tag_merge (source_tag_id, target_tag_id)
         SELECT source.tag_id, target.tag_id
         FROM tag source
         JOIN tag target
           ON target.namespace = ?1
          AND target.subtag = source.subtag
         WHERE source.namespace = ?2",
        params![target_namespace, source_namespace],
    )?;
    let roots = transaction
        .prepare(
            "SELECT DISTINCT relation.root_item_id
             FROM root_tag relation
             JOIN picto_tag_merge mapping
               ON mapping.source_tag_id = relation.tag_id
             ORDER BY relation.root_item_id",
        )?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let identities = transaction
        .prepare(
            "SELECT source_tag_id, target_tag_id
             FROM picto_tag_merge ORDER BY source_tag_id",
        )?
        .query_map([], |row| {
            Ok(TagIdentityProjectionChange {
                source_tag_id: row.get(0)?,
                target_tag_id: Some(row.get(1)?),
                remove_tag: true,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    transaction.execute(
        "UPDATE tag
         SET namespace = ?1
         WHERE namespace = ?2
           AND NOT EXISTS (
               SELECT 1 FROM tag target
               WHERE target.namespace = ?1
                 AND target.subtag = tag.subtag
           )",
        params![target_namespace, source_namespace],
    )?;
    transaction.execute(
        "INSERT INTO root_tag(root_item_id, tag_id)
         SELECT relation.root_item_id, mapping.target_tag_id
         FROM root_tag relation
         JOIN picto_tag_merge mapping ON mapping.source_tag_id = relation.tag_id
         WHERE TRUE
         ON CONFLICT(root_item_id, tag_id) DO NOTHING",
        [],
    )?;
    transaction.execute(
        "DELETE FROM root_tag
         WHERE tag_id IN (SELECT source_tag_id FROM picto_tag_merge)",
        [],
    )?;
    transaction.execute(
        "DELETE FROM tag
         WHERE tag_id IN (SELECT source_tag_id FROM picto_tag_merge)",
        [],
    )?;
    Ok((roots, TagGraphProjectionDelta { identities }))
}

fn namespace_move_scope(
    transaction: &rusqlite::Transaction<'_>,
    source_namespace: &str,
    target_namespace: &str,
) -> rusqlite::Result<BTreeSet<i64>> {
    transaction
        .prepare(
            "SELECT source.tag_id
             FROM tag source
             WHERE source.namespace = ?1
             UNION
             SELECT target.tag_id
             FROM tag source
             JOIN tag target
               ON target.namespace = ?2
              AND target.subtag = source.subtag
             WHERE source.namespace = ?1",
        )?
        .query_map(params![source_namespace, target_namespace], |row| {
            row.get::<_, i64>(0)
        })?
        .collect()
}

fn move_tag_assignments(
    transaction: &rusqlite::Transaction<'_>,
    source_id: i64,
    target_id: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO root_tag(root_item_id, tag_id)
         SELECT root_item_id, ?1
         FROM root_tag WHERE tag_id = ?2
         ON CONFLICT(root_item_id, tag_id) DO NOTHING",
        params![target_id, source_id],
    )?;
    transaction.execute("DELETE FROM root_tag WHERE tag_id = ?1", [source_id])?;
    Ok(())
}

fn affected_tag_ids(delta: &TagGraphProjectionDelta) -> BTreeSet<i64> {
    let mut tag_ids = BTreeSet::new();
    for identity in &delta.identities {
        tag_ids.insert(identity.source_tag_id);
        tag_ids.extend(identity.target_tag_id);
    }
    tag_ids
}

/// Refresh only tag-manager rows whose assignments changed.
fn refresh_tag_summaries(
    transaction: &rusqlite::Transaction<'_>,
    tag_ids: impl IntoIterator<Item = i64>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_dirty_tag_summary (
             tag_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_dirty_tag_summary;",
    )?;
    {
        let mut insert = transaction.prepare_cached(
            "INSERT INTO picto_dirty_tag_summary (tag_id) VALUES (?1)
             ON CONFLICT DO NOTHING",
        )?;
        for tag_id in tag_ids {
            insert.execute([tag_id])?;
        }
    }
    transaction.execute(
        "DELETE FROM tag_summary
         WHERE tag_id IN (SELECT tag_id FROM picto_dirty_tag_summary)",
        [],
    )?;
    transaction.execute(
        "INSERT INTO tag_summary (
             tag_id, visible_root_count, assignment_count
         )
         SELECT tag.tag_id,
                COUNT(DISTINCT CASE WHEN root.lifecycle = 'active'
                                    THEN relation.root_item_id END),
                COUNT(relation.root_item_id)
         FROM picto_dirty_tag_summary dirty
         JOIN tag ON tag.tag_id = dirty.tag_id
         LEFT JOIN root_tag relation ON relation.tag_id = tag.tag_id
         LEFT JOIN library_root root ON root.item_id = relation.root_item_id
         GROUP BY tag.tag_id",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
fn rebuild_tag_summaries(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let tag_ids = transaction
        .prepare("SELECT tag_id FROM tag")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    refresh_tag_summaries(transaction, tag_ids)
}

fn require_tag(connection: &rusqlite::Connection, tag_id: i64) -> rusqlite::Result<()> {
    connection
        .query_row("SELECT 1 FROM tag WHERE tag_id = ?1", [tag_id], |_| Ok(()))
        .optional()?
        .ok_or_else(|| invalid(format!("Tag {tag_id} does not exist")))
}

fn parse_tag(value: &str) -> Result<(String, String), String> {
    crate::tag_name_v2::parse_local(value)
}

fn normalize_group(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase().replace(' ', "_");
    if value.is_empty() {
        return Err("Tag group name cannot be empty".to_string());
    }
    if value.contains(':') {
        return Err("Tag group name cannot contain ':'".to_string());
    }
    Ok(value)
}

fn normalize_search(value: &str) -> String {
    value
        .trim()
        .split_once(':')
        .map(|(_, subtag)| subtag)
        .unwrap_or(value.trim())
        .to_lowercase()
        .replace(' ', "_")
}

/// Match a visible root against one directly assigned tag.
pub(crate) fn effective_tag_exists_sql(
    root_id_expression: &str,
    namespace_parameter: usize,
    subtag_parameter: usize,
) -> String {
    format!(
        "EXISTS (
             SELECT 1
             FROM root_tag assigned
             JOIN tag ON tag.tag_id = assigned.tag_id
             WHERE assigned.root_item_id = {root_id_expression}
               AND tag.namespace = ?{namespace_parameter}
               AND tag.subtag = ?{subtag_parameter}
         )"
    )
}

pub(crate) fn effective_query_tag_ids(
    connection: &rusqlite::Connection,
    namespace: &str,
    subtag: &str,
) -> rusqlite::Result<Vec<i64>> {
    connection
        .prepare_cached(
            "SELECT tag_id FROM tag
             WHERE namespace = ?1 AND subtag = ?2
             ORDER BY tag_id",
        )?
        .query_map(rusqlite::params![namespace, subtag], |row| {
            row.get::<_, i64>(0)
        })?
        .collect()
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn tag_receipt(revision: u64) -> MutationReceipt {
    tag_receipt_with_items(revision, &[])
}

fn tag_receipt_with_items(revision: u64, item_ids: &[i64]) -> MutationReceipt {
    let mut resources_changed = BTreeSet::from([
        resources::LIBRARY.to_string(),
        resources::SIDEBAR.to_string(),
        resources::SMART_FOLDERS.to_string(),
        resources::TAGS.to_string(),
    ]);
    let bounded_item_ids = (item_ids.len() <= MAX_RECEIPT_ITEM_IDS).then_some(item_ids);
    if let Some(item_ids) = bounded_item_ids {
        resources_changed.extend(item_ids.iter().copied().map(resources::item));
    }
    MutationReceipt {
        revision,
        resources: resources_changed.into_iter().collect(),
        item_ids: bounded_item_ids
            .unwrap_or_default()
            .iter()
            .copied()
            .map(ItemId)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::app::{ItemId, Lifecycle};
    use crate::ingest_v2::PreparedMediaInput;
    use crate::navigation_v2::CreateSmartFolderInput;
    use crate::smart_v2::{MatchMode, PredicateRule, SmartFolderPredicate, SmartRuleGroup};
    use crate::store::Store;

    fn fixture() -> (tempfile::TempDir, Application, ItemId) {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let media = application
            .ingest_prepared(&PreparedMediaInput {
                file_hash: "hash-a".into(),
                mime_type: "image/png".into(),
                size_bytes: 10,
                pixel_width: Some(1),
                pixel_height: Some(1),
                duration_ms: None,
                frame_count: Some(1),
                has_audio: false,
                name: Some("Media".into()),
                notes: None,
                rating: None,
                source_urls: Vec::new(),
                tags: vec!["general:one_girl".into(), "character:melon".into()],
                lifecycle: Lifecycle::Active,
                captured_at: None,
                source: None,
                target_folder_id: None,
                target_folder_ids: Vec::new(),
            })
            .unwrap()
            .root_item_id;
        application
            .store()
            .transaction(|transaction| rebuild_tag_summaries(transaction))
            .unwrap();
        (directory, application, media)
    }

    fn assert_tag_projection_matches_sql(application: &Application, extra_tag_ids: &[i64]) {
        let mut tag_ids = application
            .store()
            .read(|connection| {
                connection
                    .prepare("SELECT tag_id FROM tag ORDER BY tag_id")?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        tag_ids.extend_from_slice(extra_tag_ids);
        tag_ids.sort_unstable();
        tag_ids.dedup();
        for tag_id in tag_ids {
            let direct = application
                .store()
                .read(|connection| {
                    connection
                        .prepare(
                            "SELECT root_item_id FROM root_tag
                             WHERE tag_id = ?1 ORDER BY root_item_id",
                        )?
                        .query_map([tag_id], |row| row.get::<_, u32>(0))?
                        .collect::<rusqlite::Result<RoaringBitmap>>()
                })
                .unwrap();
            assert_eq!(
                application.projections().direct_tag_bitmap(tag_id),
                direct,
                "direct projection differs for tag {tag_id}"
            );
        }
    }

    fn tag_smart_folder(application: &Application, tag: &str) -> i64 {
        let smart_folder_id = application
            .create_smart_folder_v2(&CreateSmartFolderInput {
                name: format!("Tagged {tag}"),
                parent_id: None,
                predicate: SmartFolderPredicate {
                    groups: vec![SmartRuleGroup {
                        match_mode: MatchMode::All,
                        negate: false,
                        rules: vec![PredicateRule {
                            field: "tags".into(),
                            op: "include_any".into(),
                            value: None,
                            value2: None,
                            values: Some(vec![tag.into()]),
                        }],
                    }],
                },
                icon: None,
                color: None,
                notes: None,
                sort_field: None,
                sort_order: None,
            })
            .unwrap()
            .0;
        application
            .store()
            .transaction(crate::smart_v2::refresh_materialized)
            .unwrap();
        smart_folder_id
    }

    fn smart_contains(application: &Application, smart_folder_id: i64, root_id: i64) -> bool {
        application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1
                         FROM smart_folder_generation generation
                         JOIN smart_folder_membership membership
                           ON membership.generation_id = generation.generation_id
                         WHERE generation.smart_folder_id = ?1
                           AND generation.state = 'active'
                           AND membership.root_item_id = ?2
                     )",
                    params![smart_folder_id, root_id],
                    |row| row.get(0),
                )
            })
            .unwrap()
    }

    #[test]
    fn search_ignores_namespace_and_normalizes_spaces() {
        let (_directory, application, _) = fixture();
        let page = list(&application, None, Some("species:one girl"), None, 20).unwrap();
        assert_eq!(page.tags.len(), 1);
        assert_eq!(page.tags[0].name(), "one_girl");
        assert_eq!(page.tags[0].media_count, 1);
    }

    #[test]
    fn list_does_not_count_tags_from_media_without_a_visible_root() {
        let (_directory, application, media) = fixture();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute("DELETE FROM library_root WHERE item_id = ?1", [media.0])?;
                rebuild_tag_summaries(transaction)?;
                Ok(())
            })
            .unwrap();

        let page = list(&application, None, Some("one girl"), None, 20).unwrap();
        assert_eq!(page.tags.len(), 1);
        assert_eq!(page.tags[0].media_count, 0);
        assert_eq!(page.tags[0].root_count, 0);
    }

    #[test]
    fn list_counts_only_active_roots_but_retains_inbox_assignments() {
        let (_directory, application, _) = fixture();
        let (tag_id, _) = application
            .store()
            .transaction(|transaction| {
                let tag_id = transaction.query_row(
                    "SELECT tag_id FROM tag WHERE subtag = 'one_girl'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (99, 'inbox-root', 'media', '2026-01-01', '2026-01-01')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (99, 'inbox')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO root_tag(root_item_id, tag_id) VALUES (99, ?1)",
                    [tag_id],
                )?;
                rebuild_tag_summaries(transaction)?;
                Ok(tag_id)
            })
            .unwrap();

        let page = list(&application, None, Some("one girl"), None, 20).unwrap();
        assert_eq!(page.tags.len(), 1);
        assert_eq!(page.tags[0].media_count, 1);
        assert_eq!(page.tags[0].root_count, 1);
        application
            .store()
            .read(|connection| {
                let counts = connection.query_row(
                    "SELECT visible_root_count, assignment_count
                     FROM tag_summary WHERE tag_id = ?1",
                    [tag_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )?;
                assert_eq!(counts, (1, 2));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn tag_rename_settles_old_and_new_smart_names() {
        let (_directory, application, media) = fixture();
        let tag_id = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'character' AND subtag = 'melon'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        let old_folder = tag_smart_folder(&application, "character:melon");
        let new_folder = tag_smart_folder(&application, "character:slime");
        assert!(smart_contains(&application, old_folder, media.0));
        assert!(!smart_contains(&application, new_folder, media.0));

        application
            .rename_or_merge_tag(tag_id, "character:slime")
            .unwrap();

        assert!(!smart_contains(&application, old_folder, media.0));
        assert!(smart_contains(&application, new_folder, media.0));
    }

    #[test]
    fn tag_rename_undo_and_redo_restore_identity_without_rebuild() {
        let (_directory, application, media) = fixture();
        let tag_id = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag
                     WHERE namespace = 'character' AND subtag = 'melon'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();

        application
            .rename_or_merge_tag(tag_id, "character:slime")
            .unwrap();
        application.undo().unwrap();
        let name = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT namespace || ':' || subtag FROM tag WHERE tag_id = ?1",
                    [tag_id],
                    |row| row.get::<_, String>(0),
                )
            })
            .unwrap();
        assert_eq!(name, "character:melon");
        application.redo().unwrap();
        let name = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT namespace || ':' || subtag FROM tag WHERE tag_id = ?1",
                    [tag_id],
                    |row| row.get::<_, String>(0),
                )
            })
            .unwrap();
        assert_eq!(name, "character:slime");
        assert!(application
            .projections()
            .direct_tag_bitmap(tag_id)
            .contains(media.0 as u32));
        assert_tag_projection_matches_sql(&application, &[]);
    }

    #[test]
    fn merge_keeps_one_plain_root_tag_membership() {
        let (_directory, application, media) = fixture();
        let (from, to) = application
            .store()
            .transaction(|transaction| {
                let ids = transaction.query_row(
                    "SELECT
                         (SELECT tag_id FROM tag WHERE subtag = 'melon'),
                         (SELECT tag_id FROM tag WHERE subtag = 'one_girl')",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )?;
                Ok(ids)
            })
            .unwrap()
            .0;

        application.rename_or_merge_tag(from, "one_girl").unwrap();

        application
            .store()
            .read(|connection| {
                let merged = connection.query_row(
                    "SELECT COUNT(*) FROM root_tag
                     WHERE root_item_id = ?1 AND tag_id = ?2",
                    params![media.0, to],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(merged, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn unused_cleanup_deletes_only_unreferenced_tags() {
        let (_directory, application, _) = fixture();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES ('general', 'orphan')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(unused_count(&application).unwrap(), 1);
        application.delete_unused_tags().unwrap();
        application
            .store()
            .read(|connection| {
                let orphan: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM tag WHERE subtag = 'orphan'",
                    [],
                    |row| row.get(0),
                )?;
                let assigned: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM tag WHERE subtag = 'one_girl'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!((orphan, assigned), (0, 1));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn group_rename_is_atomic_and_merges_name_collisions() {
        let (_directory, application, media) = fixture();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES ('creator', 'melon')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let source_id = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'character' AND subtag = 'melon'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        application
            .rename_tag_group("character", "creator")
            .unwrap();

        application
            .store()
            .read(|connection| {
                let old_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM tag WHERE namespace = 'character'",
                    [],
                    |row| row.get(0),
                )?;
                let target_id: i64 = connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'creator' AND subtag = 'melon'",
                    [],
                    |row| row.get(0),
                )?;
                let assignment_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM root_tag WHERE root_item_id = ?1 AND tag_id = ?2",
                    params![media.0, target_id],
                    |row| row.get(0),
                )?;
                assert_eq!(old_count, 0);
                assert_eq!(assignment_count, 1);
                assert!(!application
                    .projections()
                    .direct_tag_bitmap(source_id)
                    .contains(media.0 as u32));
                assert!(application
                    .projections()
                    .direct_tag_bitmap(target_id)
                    .contains(media.0 as u32));
                Ok(())
            })
            .unwrap();
        assert_tag_projection_matches_sql(&application, &[source_id]);

        application.undo().unwrap();
        let restored = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM tag
                         WHERE tag_id = ?1 AND namespace = 'character'
                     ) AND EXISTS(
                         SELECT 1 FROM root_tag
                         WHERE root_item_id = ?2 AND tag_id = ?1
                     )",
                    params![source_id, media.0],
                    |row| row.get::<_, bool>(0),
                )
            })
            .unwrap();
        assert!(restored);
        assert_tag_projection_matches_sql(&application, &[]);
    }

    #[test]
    fn deleting_a_group_moves_tags_to_general_and_merges_collisions() {
        let (_directory, application, media) = fixture();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES ('general', 'melon')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let source_id = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'character' AND subtag = 'melon'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        application.delete_tag_group("character").unwrap();

        application
            .store()
            .read(|connection| {
                let old_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM tag WHERE namespace = 'character'",
                    [],
                    |row| row.get(0),
                )?;
                let target_id: i64 = connection.query_row(
                    "SELECT tag_id FROM tag WHERE namespace = 'general' AND subtag = 'melon'",
                    [],
                    |row| row.get(0),
                )?;
                let assignment_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM root_tag WHERE root_item_id = ?1 AND tag_id = ?2",
                    params![media.0, target_id],
                    |row| row.get(0),
                )?;
                assert_eq!(old_count, 0);
                assert_eq!(assignment_count, 1);
                assert!(!application
                    .projections()
                    .direct_tag_bitmap(source_id)
                    .contains(media.0 as u32));
                assert!(application
                    .projections()
                    .direct_tag_bitmap(target_id)
                    .contains(media.0 as u32));
                Ok(())
            })
            .unwrap();
        assert_tag_projection_matches_sql(&application, &[source_id]);

        application.undo().unwrap();
        let restored = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM tag
                         WHERE tag_id = ?1 AND namespace = 'character'
                     ) AND EXISTS(
                         SELECT 1 FROM root_tag
                         WHERE root_item_id = ?2 AND tag_id = ?1
                     )",
                    params![source_id, media.0],
                    |row| row.get::<_, bool>(0),
                )
            })
            .unwrap();
        assert!(restored);
        assert_tag_projection_matches_sql(&application, &[]);
    }
}
