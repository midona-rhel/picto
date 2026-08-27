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
    SemanticTagGraphDelta, SemanticTagIdentityState,
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
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
            tag_history("tags.rename_or_merge", "Rename tag"),
            ProjectionStore::selection_snapshot,
            |transaction, projection| {
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
                let before =
                    semantic_tag_snapshot(transaction, &projection, scope.iter().copied())?;
                match target {
                    Some(target_id) if target_id != tag_id => {
                        transaction.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;
                        let identity_changes = vec![TagIdentityProjectionChange {
                            source_tag_id: tag_id,
                            target_tag_id: Some(target_id),
                            remove_tag: true,
                        }];
                        let after = transformed_tag_snapshot(
                            transaction,
                            &projection,
                            &scope,
                            &identity_changes,
                        )?;
                        let record = semantic_tag_record(&before, &after)?;
                        let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                            unreachable!()
                        };
                        stage_tag_dependency_keys(transaction, &redo.dependency_keys)?;
                        Ok((
                            redo.affected_roots.iter().map(i64::from).collect(),
                            redo.clone(),
                            Some(record),
                            true,
                        ))
                    }
                    Some(_) => Ok((Vec::new(), SemanticTagGraphDelta::default(), None, false)),
                    None => {
                        transaction.execute(
                            "UPDATE tag SET namespace = ?1, subtag = ?2 WHERE tag_id = ?3",
                            params![namespace, subtag, tag_id],
                        )?;
                        let identity_changes = vec![TagIdentityProjectionChange {
                            source_tag_id: tag_id,
                            target_tag_id: Some(tag_id),
                            remove_tag: false,
                        }];
                        let after = transformed_tag_snapshot(
                            transaction,
                            &projection,
                            &scope,
                            &identity_changes,
                        )?;
                        let record = semantic_tag_record(&before, &after)?;
                        let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                            unreachable!()
                        };
                        stage_tag_dependency_keys(transaction, &redo.dependency_keys)?;
                        Ok((
                            redo.affected_roots.iter().map(i64::from).collect(),
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
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
            tag_history("tags.group.rename", "Rename tag group"),
            ProjectionStore::selection_snapshot,
            |transaction, projection| {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tag WHERE namespace = ?1)",
                    [&namespace],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(invalid(format!("Tag group {namespace} does not exist")));
                }
                let scope = namespace_move_scope(transaction, &namespace, &new_namespace)?;
                let before =
                    semantic_tag_snapshot(transaction, &projection, scope.iter().copied())?;
                let identity_changes = move_namespace(transaction, &namespace, &new_namespace)?;
                let after =
                    transformed_tag_snapshot(transaction, &projection, &scope, &identity_changes)?;
                let record = semantic_tag_record(&before, &after)?;
                let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                    unreachable!()
                };
                stage_tag_dependency_keys(transaction, &redo.dependency_keys)?;
                Ok((
                    redo.affected_roots
                        .iter()
                        .map(i64::from)
                        .collect::<Vec<_>>(),
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
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
            tag_history("tags.group.delete", "Delete tag group"),
            ProjectionStore::selection_snapshot,
            |transaction, projection| {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tag WHERE namespace = ?1)",
                    [&namespace],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(invalid(format!("Tag group {namespace} does not exist")));
                }
                let scope = namespace_move_scope(transaction, &namespace, "general")?;
                let before =
                    semantic_tag_snapshot(transaction, &projection, scope.iter().copied())?;
                let identity_changes = move_namespace(transaction, &namespace, "general")?;
                let after =
                    transformed_tag_snapshot(transaction, &projection, &scope, &identity_changes)?;
                let record = semantic_tag_record(&before, &after)?;
                let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                    unreachable!()
                };
                stage_tag_dependency_keys(transaction, &redo.dependency_keys)?;
                Ok((
                    redo.affected_roots
                        .iter()
                        .map(i64::from)
                        .collect::<Vec<_>>(),
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
        let (item_ids, revision, _, _) = self.semantic_undoable_transaction_if_changed_captured(
            tag_history("tags.delete", "Delete tag"),
            ProjectionStore::selection_snapshot,
            |transaction, projection| {
                require_tag(transaction, tag_id)?;
                let scope = BTreeSet::from([tag_id]);
                let before = semantic_tag_snapshot(transaction, &projection, [tag_id])?;
                transaction.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;
                let identity_changes = vec![TagIdentityProjectionChange {
                    source_tag_id: tag_id,
                    target_tag_id: None,
                    remove_tag: true,
                }];
                let after =
                    transformed_tag_snapshot(transaction, &projection, &scope, &identity_changes)?;
                let record = semantic_tag_record(&before, &after)?;
                let SemanticHistoryPayload::TagGraph(redo) = &record.redo else {
                    unreachable!()
                };
                stage_tag_dependency_keys(transaction, &redo.dependency_keys)?;
                Ok((
                    redo.affected_roots
                        .iter()
                        .map(i64::from)
                        .collect::<Vec<_>>(),
                    redo.clone(),
                    Some(record),
                    true,
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

fn canonical_tag_name(namespace: &str, subtag: &str) -> String {
    if namespace == "general" {
        subtag.to_string()
    } else {
        format!("{namespace}:{subtag}")
    }
}

#[derive(Clone, Default)]
struct SemanticTagSnapshot {
    identities: BTreeMap<i64, (String, String)>,
    root_tags: BTreeMap<i64, RoaringBitmap>,
}

fn semantic_tag_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    tag_ids: impl IntoIterator<Item = i64>,
) -> rusqlite::Result<SemanticTagSnapshot> {
    let tag_ids = tag_ids.into_iter().collect::<BTreeSet<_>>();
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
        for tag_id in &tag_ids {
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
    let root_tags = tag_ids
        .into_iter()
        .map(|tag_id| (tag_id, projection.tag_bitmap(tag_id)))
        .collect();
    Ok(SemanticTagSnapshot {
        identities,
        root_tags,
    })
}

fn transformed_tag_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    scope: &BTreeSet<i64>,
    identities: &[TagIdentityProjectionChange],
) -> rusqlite::Result<SemanticTagSnapshot> {
    let mut snapshot = semantic_tag_snapshot(transaction, projection, scope.iter().copied())?;
    for identity in identities {
        if identity.target_tag_id == Some(identity.source_tag_id) {
            continue;
        }
        let source = snapshot
            .root_tags
            .remove(&identity.source_tag_id)
            .unwrap_or_default();
        if let Some(target_tag_id) = identity.target_tag_id {
            *snapshot.root_tags.entry(target_tag_id).or_default() |= source;
        }
    }
    Ok(snapshot)
}

fn semantic_tag_record(
    before: &SemanticTagSnapshot,
    after: &SemanticTagSnapshot,
) -> rusqlite::Result<SemanticHistoryRecord> {
    Ok(SemanticHistoryRecord::new(
        SemanticHistoryPayload::TagGraph(semantic_tag_direction(after, before)?),
        SemanticHistoryPayload::TagGraph(semantic_tag_direction(before, after)?),
    ))
}

fn semantic_tag_direction(
    current: &SemanticTagSnapshot,
    desired: &SemanticTagSnapshot,
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

    let membership_ids = current
        .root_tags
        .keys()
        .chain(desired.root_tags.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut projection_tags = Vec::new();
    let mut affected_roots = RoaringBitmap::new();
    for tag_id in &membership_ids {
        let current_roots = current.root_tags.get(tag_id).cloned().unwrap_or_default();
        let desired_roots = desired.root_tags.get(tag_id).cloned().unwrap_or_default();
        let identity_changed = current.identities.get(tag_id) != desired.identities.get(tag_id);
        let mut add = &desired_roots - &current_roots;
        let mut remove = &current_roots - &desired_roots;
        if identity_changed && add.is_empty() && remove.is_empty() && !current_roots.is_empty() {
            add = current_roots.clone();
            remove = current_roots.clone();
        }
        if !add.is_empty() || !remove.is_empty() {
            affected_roots |= &current_roots;
            affected_roots |= &desired_roots;
            projection_tags.push(SemanticMembershipDelta {
                relation_id: *tag_id,
                add,
                remove,
            });
        }
    }
    let dependency_keys = identity_ids
        .iter()
        .flat_map(|tag_id| {
            [
                current.identities.get(tag_id),
                desired.identities.get(tag_id),
            ]
            .into_iter()
            .flatten()
            .map(|(namespace, subtag)| canonical_tag_name(namespace, subtag))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut affected_tag_ids = identity_ids;
    affected_tag_ids.extend(membership_ids);
    Ok(SemanticTagGraphDelta {
        identities,
        projection_tags,
        removed_tag_ids: current
            .identities
            .keys()
            .filter(|tag_id| !desired.identities.contains_key(tag_id))
            .copied()
            .collect(),
        affected_roots,
        affected_tag_ids: affected_tag_ids.into_iter().collect(),
        dependency_keys,
    })
}

fn stage_tag_dependency_keys(
    transaction: &rusqlite::Transaction<'_>,
    dependency_keys: &[String],
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_changed_tag_dependency_key (
             dependency_key TEXT PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_changed_tag_dependency_key;",
    )?;
    if dependency_keys.is_empty() {
        return Ok(());
    }
    let encoded = serde_json::to_string(dependency_keys)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO picto_changed_tag_dependency_key(dependency_key)
         SELECT CAST(value AS TEXT) FROM json_each(?1)
         WHERE TRUE
         ON CONFLICT DO NOTHING",
        [encoded],
    )?;
    Ok(())
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
) -> rusqlite::Result<Vec<TagIdentityProjectionChange>> {
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
        "DELETE FROM tag
         WHERE tag_id IN (SELECT source_tag_id FROM picto_tag_merge)",
        [],
    )?;
    Ok(identities)
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
        (directory, application, media)
    }

    fn assert_tag_projection_matches_canonical(application: &Application, extra_tag_ids: &[i64]) {
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
            let canonical = application
                .store()
                .read(|connection| {
                    crate::canonical_bitmap::load_bitmap(
                        connection,
                        crate::canonical_bitmap::BitmapDomain::Tag,
                        tag_id,
                    )
                })
                .unwrap();
            assert_eq!(
                application.projections().direct_tag_bitmap(tag_id),
                canonical,
                "published projection differs from canonical tag bitmap {tag_id}"
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
            .delete_items(&crate::app::ItemTarget::Explicit {
                item_ids: vec![media],
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
        let tag_id = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag WHERE subtag = 'one_girl'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        application
            .ingest_prepared(&PreparedMediaInput {
                file_hash: "hash-inbox".into(),
                mime_type: "image/png".into(),
                size_bytes: 10,
                pixel_width: Some(1),
                pixel_height: Some(1),
                duration_ms: None,
                frame_count: Some(1),
                has_audio: false,
                name: Some("Inbox media".into()),
                notes: None,
                rating: None,
                source_urls: Vec::new(),
                tags: vec!["general:one_girl".into()],
                lifecycle: Lifecycle::Inbox,
                captured_at: None,
                source: None,
                target_folder_id: None,
                target_folder_ids: Vec::new(),
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
        assert_tag_projection_matches_canonical(&application, &[]);
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

        assert!(application
            .projections()
            .direct_tag_bitmap(to)
            .contains(media.0 as u32));
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
                assert_eq!(old_count, 0);
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
        assert_tag_projection_matches_canonical(&application, &[source_id]);

        application.undo().unwrap();
        let restored = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM tag
                         WHERE tag_id = ?1 AND namespace = 'character'
                     )",
                    [source_id],
                    |row| row.get::<_, bool>(0),
                )
            })
            .unwrap();
        assert!(restored);
        assert!(application
            .projections()
            .direct_tag_bitmap(source_id)
            .contains(media.0 as u32));
        assert_tag_projection_matches_canonical(&application, &[]);
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
                assert_eq!(old_count, 0);
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
        assert_tag_projection_matches_canonical(&application, &[source_id]);

        application.undo().unwrap();
        let restored = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM tag
                         WHERE tag_id = ?1 AND namespace = 'character'
                     )",
                    [source_id],
                    |row| row.get::<_, bool>(0),
                )
            })
            .unwrap();
        assert!(restored);
        assert!(application
            .projections()
            .direct_tag_bitmap(source_id)
            .contains(media.0 as u32));
        assert_tag_projection_matches_canonical(&application, &[]);
    }
}
