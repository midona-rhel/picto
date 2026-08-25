//! Tag-manager reads and mutations for the replacement backend.

use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, MutationReceipt};
use crate::store::history::HistoryDescriptor;

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
    pub aliases_to: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TagRelation {
    #[ts(type = "number")]
    pub tag_id: i64,
    pub name: String,
    pub direction: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TagRelations {
    pub aliases: Vec<TagRelation>,
    pub parents: Vec<TagRelation>,
    pub children: Vec<TagRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AliasDelta {
    from_tag_id: i64,
    removed_to_tag_id: Option<i64>,
    added_to_tag_id: Option<i64>,
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
                    COUNT(DISTINCT mt.media_item_id) AS media_count,
                    COUNT(DISTINCT COALESCE(cm.collection_id, mt.media_item_id)) AS root_count,
                    CASE WHEN alias_to.tag_id IS NULL THEN NULL
                         WHEN alias_to.namespace = 'general' THEN alias_to.subtag
                         ELSE alias_to.namespace || ':' || alias_to.subtag END AS aliases_to
             FROM tag t
             LEFT JOIN media_tag mt ON mt.tag_id = t.tag_id
             LEFT JOIN collection_member cm ON cm.media_item_id = mt.media_item_id
             LEFT JOIN tag_alias ta ON ta.from_tag_id = t.tag_id AND ta.source = 'local'
             LEFT JOIN tag alias_to ON alias_to.tag_id = ta.to_tag_id
             WHERE t.tag_id > ?1
               AND (?2 IS NULL OR t.namespace = ?2)
               AND (?3 IS NULL OR LOWER(t.subtag) LIKE ?3)
             GROUP BY t.tag_id, t.namespace, t.subtag,
                      alias_to.tag_id, alias_to.namespace, alias_to.subtag
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
                    aliases_to: row.get(5)?,
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
             WHERE NOT EXISTS (SELECT 1 FROM media_tag mt WHERE mt.tag_id = t.tag_id)
               AND NOT EXISTS (SELECT 1 FROM tag_alias a WHERE a.from_tag_id = t.tag_id OR a.to_tag_id = t.tag_id)
               AND NOT EXISTS (SELECT 1 FROM tag_implication i WHERE i.child_tag_id = t.tag_id OR i.parent_tag_id = t.tag_id)",
            [],
            |row| row.get(0),
        )
    })
}

pub fn relations(application: &Application, tag_id: i64) -> Result<TagRelations, String> {
    application.store().read(|connection| {
        require_tag(connection, tag_id)?;
        Ok(TagRelations {
            aliases: relation_rows(
                connection,
                "SELECT t.tag_id, t.namespace, t.subtag,
                        CASE WHEN a.from_tag_id = ?1 THEN 'outgoing' ELSE 'incoming' END
                 FROM tag_alias a
                 JOIN tag t ON t.tag_id = CASE
                     WHEN a.from_tag_id = ?1 THEN a.to_tag_id ELSE a.from_tag_id END
                 WHERE (a.from_tag_id = ?1 OR a.to_tag_id = ?1)
                 ORDER BY t.namespace, t.subtag",
                tag_id,
            )?,
            parents: relation_rows(
                connection,
                "SELECT t.tag_id, t.namespace, t.subtag, 'parent'
                 FROM tag_implication i JOIN tag t ON t.tag_id = i.parent_tag_id
                 WHERE i.child_tag_id = ?1 ORDER BY t.namespace, t.subtag",
                tag_id,
            )?,
            children: relation_rows(
                connection,
                "SELECT t.tag_id, t.namespace, t.subtag, 'child'
                 FROM tag_implication i JOIN tag t ON t.tag_id = i.child_tag_id
                 WHERE i.parent_tag_id = ?1 ORDER BY t.namespace, t.subtag",
                tag_id,
            )?,
        })
    })
}

impl Application {
    pub fn set_tag_alias(
        &self,
        from_tag_id: i64,
        to_tag_id: Option<i64>,
    ) -> Result<MutationReceipt, String> {
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            tag_history("tags.set_alias", "Change tag alias").rebuilding_projections(),
            |transaction| {
                require_tag(transaction, from_tag_id)?;
                if let Some(to_tag_id) = to_tag_id {
                    require_tag(transaction, to_tag_id)?;
                    if from_tag_id == to_tag_id {
                        return Err(invalid("A tag cannot alias itself"));
                    }
                }
                let previous = transaction
                    .query_row(
                        "SELECT to_tag_id FROM tag_alias
                         WHERE from_tag_id = ?1 AND source = 'local'",
                        [from_tag_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?;
                if previous == to_tag_id {
                    return Ok((
                        (),
                        AliasDelta {
                            from_tag_id,
                            removed_to_tag_id: None,
                            added_to_tag_id: None,
                        },
                        false,
                    ));
                }
                transaction.execute(
                    "DELETE FROM tag_alias WHERE from_tag_id = ?1 AND source = 'local'",
                    [from_tag_id],
                )?;
                if let Some(to_tag_id) = to_tag_id {
                    transaction.execute(
                        "INSERT INTO tag_alias (from_tag_id, to_tag_id, source)
                         VALUES (?1, ?2, 'local')",
                        params![from_tag_id, to_tag_id],
                    )?;
                }
                Ok((
                    (),
                    AliasDelta {
                        from_tag_id,
                        removed_to_tag_id: previous,
                        added_to_tag_id: to_tag_id,
                    },
                    true,
                ))
            },
            |projections, delta| {
                if let Some(to_tag_id) = delta.removed_to_tag_id {
                    projections.apply_tag_alias_delta(delta.from_tag_id, to_tag_id, false)?;
                }
                if let Some(to_tag_id) = delta.added_to_tag_id {
                    projections.apply_tag_alias_delta(delta.from_tag_id, to_tag_id, true)?;
                }
                Ok(())
            },
        )?;
        Ok(tag_receipt(revision))
    }

    pub fn set_tag_implication(
        &self,
        child_tag_id: i64,
        parent_tag_id: i64,
        present: bool,
    ) -> Result<MutationReceipt, String> {
        if child_tag_id == parent_tag_id {
            return Err("A tag cannot imply itself".to_string());
        }
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            tag_history("tags.set_implication", "Change tag implication").rebuilding_projections(),
            |transaction| {
                require_tag(transaction, child_tag_id)?;
                require_tag(transaction, parent_tag_id)?;
                let changed = if present {
                    transaction.execute(
                        "INSERT INTO tag_implication (child_tag_id, parent_tag_id, source)
                         VALUES (?1, ?2, 'local') ON CONFLICT DO NOTHING",
                        params![child_tag_id, parent_tag_id],
                    )?
                } else {
                    transaction.execute(
                        "DELETE FROM tag_implication
                         WHERE child_tag_id = ?1 AND parent_tag_id = ?2 AND source = 'local'",
                        params![child_tag_id, parent_tag_id],
                    )?
                };
                Ok(((), (), changed != 0))
            },
            |projections, ()| {
                projections.apply_tag_implication_delta(child_tag_id, parent_tag_id, present)
            },
        )?;
        Ok(tag_receipt(revision))
    }

    pub fn rename_or_merge_tag(
        &self,
        tag_id: i64,
        new_name: &str,
    ) -> Result<MutationReceipt, String> {
        let (namespace, subtag) = parse_tag(new_name)?;
        let (item_ids, revision, _, _) = self.undoable_transaction_if_changed_rebuilding(
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
                match target {
                    Some(target_id) if target_id != tag_id => {
                        let roots = roots_for_tag(transaction, tag_id)?;
                        transaction.execute(
                            "INSERT INTO media_tag (media_item_id, tag_id, source, provenance_mask)
                             SELECT media_item_id, ?1, source, provenance_mask FROM media_tag
                             WHERE tag_id = ?2
                             ON CONFLICT(media_item_id, tag_id, source) DO UPDATE SET
                                 provenance_mask = media_tag.provenance_mask | excluded.provenance_mask",
                            params![target_id, tag_id],
                        )?;
                        transaction.execute("DELETE FROM media_tag WHERE tag_id = ?1", [tag_id])?;
                        transaction.execute(
                            "INSERT INTO tag_alias (from_tag_id, to_tag_id, source)
                             VALUES (?1, ?2, 'local')
                             ON CONFLICT(from_tag_id, source) DO UPDATE SET to_tag_id = excluded.to_tag_id",
                            params![tag_id, target_id],
                        )?;
                        Ok((roots, true))
                    }
                    Some(_) => Ok((Vec::new(), false)),
                    None => {
                        transaction.execute(
                            "UPDATE tag SET namespace = ?1, subtag = ?2 WHERE tag_id = ?3",
                            params![namespace, subtag, tag_id],
                        )?;
                        Ok((Vec::new(), true))
                    }
                }
            },
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
        let (item_ids, revision, _, _) = self.undoable_transaction_if_changed_rebuilding(
            tag_history("tags.group.rename", "Rename tag group"),
            |transaction| {
                let tags = tags_in_namespace(transaction, &namespace)?;
                if tags.is_empty() {
                    return Err(invalid(format!("Tag group {namespace} does not exist")));
                }
                let mut roots = BTreeSet::new();
                for (tag_id, subtag) in tags {
                    let target_id = transaction
                        .query_row(
                            "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                            params![new_namespace, subtag],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?;
                    if let Some(target_id) = target_id {
                        roots.extend(merge_tag_rows(transaction, tag_id, target_id)?);
                    } else {
                        transaction.execute(
                            "UPDATE tag SET namespace = ?1 WHERE tag_id = ?2",
                            params![new_namespace, tag_id],
                        )?;
                    }
                }
                Ok((roots.into_iter().collect::<Vec<_>>(), true))
            },
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn delete_tag_group(&self, namespace: &str) -> Result<MutationReceipt, String> {
        let namespace = normalize_group(namespace)?;
        if namespace == "general" {
            return Err("The General group cannot be deleted".to_string());
        }
        let (item_ids, revision, _, _) = self.undoable_transaction_if_changed_rebuilding(
            tag_history("tags.group.delete", "Delete tag group"),
            |transaction| {
                let tags = tags_in_namespace(transaction, &namespace)?;
                if tags.is_empty() {
                    return Err(invalid(format!("Tag group {namespace} does not exist")));
                }
                let mut roots = BTreeSet::new();
                for (tag_id, subtag) in tags {
                    let target_id = transaction
                        .query_row(
                            "SELECT tag_id FROM tag WHERE namespace = 'general' AND subtag = ?1",
                            [&subtag],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?;
                    if let Some(target_id) = target_id {
                        roots.extend(merge_tag_rows(transaction, tag_id, target_id)?);
                    } else {
                        transaction.execute(
                            "UPDATE tag SET namespace = 'general' WHERE tag_id = ?1",
                            [tag_id],
                        )?;
                    }
                }
                Ok((roots.into_iter().collect::<Vec<_>>(), true))
            },
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<MutationReceipt, String> {
        let (item_ids, revision, _) = self.undoable_transaction_rebuilding(
            tag_history("tags.delete", "Delete tag"),
            |transaction| {
                require_tag(transaction, tag_id)?;
                let roots = roots_for_tag(transaction, tag_id)?;
                transaction.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;
                Ok(roots)
            },
        )?;
        Ok(tag_receipt_with_items(revision, &item_ids))
    }

    pub fn delete_unused_tags(&self) -> Result<MutationReceipt, String> {
        let (_, revision, _, _) = self.undoable_transaction_if_changed(
            tag_history("tags.delete_unused", "Delete unused tags"),
            |transaction| {
                let deleted = transaction.execute(
                    "DELETE FROM tag
                     WHERE NOT EXISTS (SELECT 1 FROM media_tag mt WHERE mt.tag_id = tag.tag_id)
                       AND NOT EXISTS (SELECT 1 FROM tag_alias a WHERE a.from_tag_id = tag.tag_id OR a.to_tag_id = tag.tag_id)
                       AND NOT EXISTS (SELECT 1 FROM tag_implication i WHERE i.child_tag_id = tag.tag_id OR i.parent_tag_id = tag.tag_id)",
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

fn relation_rows(
    connection: &rusqlite::Connection,
    sql: &str,
    tag_id: i64,
) -> rusqlite::Result<Vec<TagRelation>> {
    connection
        .prepare(sql)?
        .query_map([tag_id], |row| {
            let namespace: String = row.get(1)?;
            let subtag: String = row.get(2)?;
            Ok(TagRelation {
                tag_id: row.get(0)?,
                name: if namespace == "general" {
                    subtag
                } else {
                    format!("{namespace}:{subtag}")
                },
                direction: row.get(3)?,
            })
        })?
        .collect()
}

fn roots_for_tag(connection: &rusqlite::Connection, tag_id: i64) -> rusqlite::Result<Vec<i64>> {
    connection
        .prepare(
            "SELECT DISTINCT COALESCE(cm.collection_id, mt.media_item_id)
             FROM media_tag mt
             LEFT JOIN collection_member cm ON cm.media_item_id = mt.media_item_id
             WHERE mt.tag_id = ?1 ORDER BY 1",
        )?
        .query_map([tag_id], |row| row.get(0))?
        .collect()
}

fn tags_in_namespace(
    connection: &rusqlite::Connection,
    namespace: &str,
) -> rusqlite::Result<Vec<(i64, String)>> {
    connection
        .prepare("SELECT tag_id, subtag FROM tag WHERE namespace = ?1 ORDER BY tag_id")?
        .query_map([namespace], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

fn merge_tag_rows(
    transaction: &rusqlite::Transaction<'_>,
    source_id: i64,
    target_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let roots = roots_for_tag(transaction, source_id)?;
    transaction.execute(
        "INSERT INTO media_tag (media_item_id, tag_id, source, provenance_mask)
         SELECT media_item_id, ?1, source, provenance_mask FROM media_tag WHERE tag_id = ?2
         ON CONFLICT(media_item_id, tag_id, source) DO UPDATE SET
           provenance_mask = media_tag.provenance_mask | excluded.provenance_mask",
        params![target_id, source_id],
    )?;
    transaction.execute("DELETE FROM media_tag WHERE tag_id = ?1", [source_id])?;

    transaction.execute(
        "UPDATE tag_alias SET to_tag_id = ?1 WHERE to_tag_id = ?2 AND from_tag_id != ?1",
        params![target_id, source_id],
    )?;
    transaction.execute(
        "DELETE FROM tag_alias WHERE from_tag_id = ?1 OR (from_tag_id = ?2 AND to_tag_id = ?2)",
        params![source_id, target_id],
    )?;

    transaction.execute(
        "INSERT OR IGNORE INTO tag_implication (child_tag_id, parent_tag_id, source)
         SELECT CASE WHEN child_tag_id = ?1 THEN ?2 ELSE child_tag_id END,
                CASE WHEN parent_tag_id = ?1 THEN ?2 ELSE parent_tag_id END,
                source
         FROM tag_implication
         WHERE child_tag_id = ?1 OR parent_tag_id = ?1",
        params![source_id, target_id],
    )?;
    transaction.execute(
        "DELETE FROM tag_implication WHERE child_tag_id = ?1 OR parent_tag_id = ?1",
        [source_id],
    )?;
    transaction.execute(
        "DELETE FROM tag_implication WHERE child_tag_id = parent_tag_id",
        [],
    )?;
    transaction.execute("DELETE FROM tag WHERE tag_id = ?1", [source_id])?;
    Ok(roots)
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

/// Match a media row against a requested effective tag. The recursive set is
/// rooted at one requested tag, so aliases and implication children are
/// resolved without expanding every media/tag assignment in the library.
pub(crate) fn effective_tag_exists_sql(
    media_id_expression: &str,
    namespace_parameter: usize,
    subtag_parameter: usize,
) -> String {
    format!(
        "EXISTS (
             WITH RECURSIVE matching_tags(tag_id) AS (
                 SELECT tag_id FROM tag
                 WHERE namespace = ?{namespace_parameter}
                   AND subtag = ?{subtag_parameter}
                 UNION
                 SELECT CASE
                            WHEN alias.from_tag_id = matching.tag_id THEN alias.to_tag_id
                            ELSE alias.from_tag_id
                        END
                 FROM matching_tags matching
                 JOIN tag_alias alias
                   ON alias.from_tag_id = matching.tag_id
                   OR alias.to_tag_id = matching.tag_id
                 UNION
                 SELECT implication.child_tag_id
                 FROM matching_tags matching
                 JOIN tag_implication implication
                   ON implication.parent_tag_id = matching.tag_id
             )
             SELECT 1 FROM media_tag assigned
             JOIN matching_tags matching ON matching.tag_id = assigned.tag_id
             WHERE assigned.media_item_id = {media_id_expression}
         )"
    )
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
    resources_changed.extend(item_ids.iter().copied().map(resources::item));
    MutationReceipt {
        revision,
        resources: resources_changed.into_iter().collect(),
        item_ids: if item_ids.len() <= MAX_RECEIPT_ITEM_IDS {
            item_ids.iter().copied().map(ItemId).collect()
        } else {
            Vec::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::app::{ItemId, Lifecycle};
    use crate::ingest_v2::PreparedMediaInput;
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
                provenance_mask: 1,
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

    #[test]
    fn search_ignores_namespace_and_normalizes_spaces() {
        let (_directory, application, _) = fixture();
        let page = list(&application, None, Some("species:one girl"), None, 20).unwrap();
        assert_eq!(page.tags.len(), 1);
        assert_eq!(page.tags[0].name(), "one_girl");
        assert_eq!(page.tags[0].media_count, 1);
    }

    #[test]
    fn implication_updates_effective_root_projection() {
        let (_directory, application, media) = fixture();
        let ids = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT
                         (SELECT tag_id FROM tag WHERE subtag = 'melon'),
                         (SELECT tag_id FROM tag WHERE subtag = 'one_girl')",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
            })
            .unwrap();
        application.set_tag_implication(ids.0, ids.1, true).unwrap();
        assert!(application
            .projections()
            .effective_tag_bitmap(ids.1)
            .contains(media.0 as u32));
    }

    #[test]
    fn alias_relations_report_direction_from_requested_tag() {
        let (_directory, application, _) = fixture();
        let (from, to) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT
                         (SELECT tag_id FROM tag WHERE subtag = 'melon'),
                         (SELECT tag_id FROM tag WHERE subtag = 'one_girl')",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
            })
            .unwrap();
        application.set_tag_alias(from, Some(to)).unwrap();

        let outgoing = relations(&application, from).unwrap();
        assert_eq!(outgoing.aliases[0].tag_id, to);
        assert_eq!(outgoing.aliases[0].direction, "outgoing");

        let incoming = relations(&application, to).unwrap();
        assert_eq!(incoming.aliases[0].tag_id, from);
        assert_eq!(incoming.aliases[0].direction, "incoming");
    }

    #[test]
    fn merge_moves_assignments_and_keeps_old_name_as_alias() {
        let (_directory, application, media) = fixture();
        let (from, to) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT
                         (SELECT tag_id FROM tag WHERE subtag = 'melon'),
                         (SELECT tag_id FROM tag WHERE subtag = 'one_girl')",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
            })
            .unwrap();
        application.rename_or_merge_tag(from, "one_girl").unwrap();
        application
            .store()
            .read(|connection| {
                let direct: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_tag WHERE media_item_id = ?1 AND tag_id = ?2",
                    params![media.0, to],
                    |row| row.get(0),
                )?;
                let alias: i64 = connection.query_row(
                    "SELECT to_tag_id FROM tag_alias WHERE from_tag_id = ?1",
                    [from],
                    |row| row.get(0),
                )?;
                assert_eq!(direct, 1);
                assert_eq!(alias, to);
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
                transaction.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES ('general', 'relation_only')",
                    [],
                )?;
                let relation_only: i64 = transaction.query_row(
                    "SELECT tag_id FROM tag WHERE subtag = 'relation_only'",
                    [],
                    |row| row.get(0),
                )?;
                let assigned: i64 = transaction.query_row(
                    "SELECT tag_id FROM tag WHERE subtag = 'one_girl'",
                    [],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO tag_alias (from_tag_id, to_tag_id, source) VALUES (?1, ?2, 'local')",
                    params![relation_only, assigned],
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
                let relation_only: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM tag WHERE subtag = 'relation_only'",
                    [],
                    |row| row.get(0),
                )?;
                let assigned: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM tag WHERE subtag = 'one_girl'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!((orphan, relation_only, assigned), (0, 1, 1));
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

        application.rename_tag_group("character", "creator").unwrap();

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
                    "SELECT COUNT(*) FROM media_tag WHERE media_item_id = ?1 AND tag_id = ?2",
                    params![media.0, target_id],
                    |row| row.get(0),
                )?;
                assert_eq!(old_count, 0);
                assert_eq!(assignment_count, 1);
                Ok(())
            })
            .unwrap();
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
                    "SELECT COUNT(*) FROM media_tag WHERE media_item_id = ?1 AND tag_id = ?2",
                    params![media.0, target_id],
                    |row| row.get(0),
                )?;
                assert_eq!(old_count, 0);
                assert_eq!(assignment_count, 1);
                Ok(())
            })
            .unwrap();
    }
}
