//! One durable-media materialization path for manual and source imports.

use std::collections::BTreeSet;

use rand::RngCore;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, Lifecycle, MutationReceipt};
use crate::projection_v2::{
    FolderProjectionChange, ItemProjectionChange, MembershipProjectionChange, RootProjectionChange,
    StructureProjectionDelta, TagProjectionChange,
};

pub(crate) const DELETED_SOURCE_ITEM_ERROR: &str =
    "This source item was deliberately deleted and cannot be resurrected";

pub(crate) fn is_deleted_source_item_error(error: &str) -> bool {
    error.contains(DELETED_SOURCE_ITEM_ERROR)
}

const RANK_GAP: i64 = 1024;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SourcePostInput {
    pub site_id: String,
    pub post_key: String,
    pub item_key: String,
    pub position: i64,
    pub canonical_post_url: Option<String>,
    pub canonical_media_url: Option<String>,
    pub creator_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub captured_at: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PreparedMediaInput {
    pub file_hash: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub name: Option<String>,
    pub notes: Option<String>,
    pub rating: Option<i64>,
    pub source_urls: Vec<String>,
    pub tags: Vec<String>,
    pub provenance_mask: i64,
    pub lifecycle: Lifecycle,
    pub captured_at: Option<String>,
    pub source: Option<SourcePostInput>,
    #[serde(default)]
    pub target_folder_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct IngestMediaResult {
    pub media_item_id: ItemId,
    pub root_item_id: ItemId,
    pub reused_existing_item: bool,
    pub promoted_to_collection: bool,
    pub receipt: Option<MutationReceipt>,
}

impl Application {
    pub(crate) fn ingest_prepared(
        &self,
        input: &PreparedMediaInput,
    ) -> Result<IngestMediaResult, String> {
        validate_input(input)?;
        let enqueue_ai = should_enqueue_ai(self, input)?;
        let now = chrono::Utc::now().to_rfc3339();
        let ((media_item_id, root_item_id, reused, promoted), revision, changed) = self
            .transaction_if_changed(
                |transaction| {
                if let Some(source) = &input.source {
                    if let Some(existing) = existing_source_item(transaction, source)? {
                        match existing {
                            ExistingSourceItem::Present {
                                media_item_id,
                                root_item_id,
                            } => {
                                return Ok((
                                    (media_item_id, root_item_id, true, false),
                                    StructureProjectionDelta::default(),
                                    false,
                                ));
                            }
                            ExistingSourceItem::Deleted => {
                                return Err(invalid(DELETED_SOURCE_ITEM_ERROR));
                            }
                            ExistingSourceItem::Pending => {}
                        }
                    }
                } else if let Some((media_item_id, root_item_id)) =
                    existing_manual_item(transaction, &input.file_hash)?
                {
                    let mut delta = StructureProjectionDelta::default();
                    let tag_ids = insert_tags(
                        transaction,
                        media_item_id,
                        &input.tags,
                        input.provenance_mask,
                        false,
                    )?;
                    delta.tags.extend(tag_ids.into_iter().map(|tag_id| TagProjectionChange {
                        media_id: media_item_id,
                        tag_id,
                        present: true,
                    }));
                    let folder_changed = attach_target_folder(
                        transaction,
                        input.target_folder_id,
                        root_item_id,
                        &mut delta,
                    )?;
                    let changed = folder_changed || !delta.tags.is_empty();
                    return Ok(((media_item_id, root_item_id, true, false), delta, changed));
                }

                let file_id = upsert_file(transaction, input, &now)?;
                let media_item_id = insert_media_asset(transaction, file_id, input, &now)?;
                let tag_ids = insert_tags(
                    transaction,
                    media_item_id,
                    &input.tags,
                    input.provenance_mask,
                    input.source.is_some(),
                )?;
                enqueue_derivatives(
                    transaction,
                    media_item_id,
                    file_id,
                    input,
                    enqueue_ai,
                    &now,
                )?;

                let (root_item_id, promoted) = if let Some(source) = &input.source {
                    attach_source_item(transaction, source, media_item_id, &now)?;
                    settle_source_post_root(
                        transaction,
                        source,
                        media_item_id,
                        input.lifecycle,
                        &now,
                    )?
                } else {
                    insert_root(transaction, media_item_id, input.lifecycle)?;
                    (media_item_id, false)
                };
                let mut delta = StructureProjectionDelta::default();
                delta.items.push(ItemProjectionChange {
                    item_id: media_item_id,
                    kind: crate::app::ItemKind::Media,
                    present: true,
                });
                if media_item_id == root_item_id {
                    delta.roots.push(RootProjectionChange {
                        item_id: media_item_id,
                        lifecycle: Some(input.lifecycle),
                    });
                } else if promoted {
                    delta.items.push(ItemProjectionChange {
                        item_id: root_item_id,
                        kind: crate::app::ItemKind::Collection,
                        present: true,
                    });
                    delta.roots.push(RootProjectionChange {
                        item_id: root_item_id,
                        lifecycle: Some(input.lifecycle),
                    });
                    let mut statement = transaction.prepare(
                        "SELECT media_item_id FROM collection_member
                         WHERE collection_id = ?1",
                    )?;
                    let members = statement
                        .query_map([root_item_id], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    drop(statement);
                    let mut folder_statement = transaction.prepare(
                        "SELECT folder_id FROM folder_item WHERE item_id = ?1",
                    )?;
                    let folder_ids = folder_statement
                        .query_map([root_item_id], |row| row.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    drop(folder_statement);
                    for folder_id in folder_ids {
                        delta.folders.push(FolderProjectionChange {
                            folder_id,
                            item_id: root_item_id,
                            present: true,
                        });
                        for member_id in &members {
                            delta.folders.push(FolderProjectionChange {
                                folder_id,
                                item_id: *member_id,
                                present: false,
                            });
                        }
                    }
                    for member_id in members {
                        delta.roots.push(RootProjectionChange {
                            item_id: member_id,
                            lifecycle: None,
                        });
                        delta.memberships.push(MembershipProjectionChange {
                            collection_id: root_item_id,
                            media_id: member_id,
                            present: true,
                        });
                    }
                } else {
                    delta.memberships.push(MembershipProjectionChange {
                        collection_id: root_item_id,
                        media_id: media_item_id,
                        present: true,
                    });
                }
                attach_target_folder(
                    transaction,
                    input.target_folder_id,
                    root_item_id,
                    &mut delta,
                )?;
                delta.tags.extend(tag_ids.into_iter().map(|tag_id| TagProjectionChange {
                    media_id: media_item_id,
                    tag_id,
                    present: true,
                }));
                Ok(((media_item_id, root_item_id, false, promoted), delta, true))
            },
            |projections, delta| projections.apply_structure_delta(delta),
        )?;

        let receipt = changed.then(|| MutationReceipt {
            revision,
            resources: {
                let mut changed_resources = vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::DUPLICATES.to_string(),
                    resources::TASKS.to_string(),
                ];
                if input.target_folder_id.is_some() {
                    changed_resources.push(resources::FOLDERS.to_string());
                }
                changed_resources
            },
            item_ids: if media_item_id == root_item_id {
                vec![ItemId(root_item_id)]
            } else {
                vec![ItemId(root_item_id), ItemId(media_item_id)]
            },
        });

        Ok(IngestMediaResult {
            media_item_id: ItemId(media_item_id),
            root_item_id: ItemId(root_item_id),
            reused_existing_item: reused,
            promoted_to_collection: promoted,
            receipt,
        })
    }
}

enum ExistingSourceItem {
    Present {
        media_item_id: i64,
        root_item_id: i64,
    },
    Pending,
    Deleted,
}

fn existing_manual_item(
    transaction: &Transaction<'_>,
    file_hash: &str,
) -> rusqlite::Result<Option<(i64, i64)>> {
    transaction
        .query_row(
            "SELECT ma.item_id, COALESCE(cm.collection_id, lr.item_id)
             FROM media_asset ma
             JOIN media_file mf ON mf.file_id = ma.file_id
             LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
             LEFT JOIN library_root lr ON lr.item_id = ma.item_id
             WHERE mf.file_hash = ?1
               AND COALESCE(cm.collection_id, lr.item_id) IS NOT NULL
             ORDER BY cm.collection_id IS NOT NULL, ma.item_id
             LIMIT 1",
            [file_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
}

fn attach_target_folder(
    transaction: &Transaction<'_>,
    folder_id: Option<i64>,
    root_item_id: i64,
    delta: &mut StructureProjectionDelta,
) -> rusqlite::Result<bool> {
    let Some(folder_id) = folder_id else {
        return Ok(false);
    };
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM folder WHERE folder_id = ?1)",
        [folder_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(invalid(format!("Folder {folder_id} does not exist")));
    }
    let changed = transaction.execute(
        "INSERT INTO folder_item (folder_id, item_id) VALUES (?1, ?2)
         ON CONFLICT(folder_id, item_id) DO NOTHING",
        params![folder_id, root_item_id],
    )? != 0;
    if changed {
        delta.folders.push(FolderProjectionChange {
            folder_id,
            item_id: root_item_id,
            present: true,
        });
    }
    Ok(changed)
}

fn validate_input(input: &PreparedMediaInput) -> Result<(), String> {
    if input.file_hash.trim().is_empty() {
        return Err("A physical file hash is required".to_string());
    }
    if input.size_bytes < 0 {
        return Err("Media size cannot be negative".to_string());
    }
    if !input.mime_type.starts_with("image/") && !input.mime_type.starts_with("video/") {
        return Err(format!("Unsupported media type: {}", input.mime_type));
    }
    if let Some(source) = &input.source {
        if source.site_id.trim().is_empty()
            || source.post_key.trim().is_empty()
            || source.item_key.trim().is_empty()
        {
            return Err("Source site, post, and item identity are required".to_string());
        }
    }
    Ok(())
}

fn existing_source_item(
    transaction: &Transaction<'_>,
    source: &SourcePostInput,
) -> rusqlite::Result<Option<ExistingSourceItem>> {
    let row = transaction
        .query_row(
            "SELECT si.state, si.media_item_id, sp.root_item_id
             FROM source_item si
             JOIN source_post sp ON sp.source_post_id = si.source_post_id
             WHERE sp.site_id = ?1 AND sp.post_key = ?2 AND si.item_key = ?3",
            params![source.site_id, source.post_key, source.item_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map(|(state, media_item_id, root_item_id)| {
        if state == "deleted" {
            ExistingSourceItem::Deleted
        } else if media_item_id.is_none() {
            ExistingSourceItem::Pending
        } else {
            ExistingSourceItem::Present {
                media_item_id: media_item_id.unwrap(),
                root_item_id: root_item_id.unwrap_or_else(|| media_item_id.unwrap()),
            }
        }
    }))
}

fn upsert_file(
    transaction: &Transaction<'_>,
    input: &PreparedMediaInput,
    now: &str,
) -> rusqlite::Result<i64> {
    transaction.execute(
        "DELETE FROM work_item
         WHERE file_hash = ?1 AND work_type = 'blob_delete' AND status = 'pending'",
        [&input.file_hash],
    )?;
    transaction.execute(
        "INSERT INTO media_file (
             file_hash, mime_type, size_bytes, pixel_width, pixel_height,
             duration_ms, frame_count, has_audio, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(file_hash) DO NOTHING",
        params![
            input.file_hash,
            input.mime_type,
            input.size_bytes,
            input.pixel_width,
            input.pixel_height,
            input.duration_ms,
            input.frame_count,
            input.has_audio as i64,
            now,
        ],
    )?;
    transaction.query_row(
        "SELECT file_id FROM media_file WHERE file_hash = ?1",
        [&input.file_hash],
        |row| row.get(0),
    )
}

fn insert_media_asset(
    transaction: &Transaction<'_>,
    file_id: i64,
    input: &PreparedMediaInput,
    now: &str,
) -> rusqlite::Result<i64> {
    transaction.execute(
        "INSERT INTO library_item (item_key, kind, created_at, updated_at)
         VALUES (?1, 'media', ?2, ?2)",
        params![new_key("media"), now],
    )?;
    let item_id = transaction.last_insert_rowid();
    let source_urls_json = if input.source_urls.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(&input.source_urls)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        )
    };
    transaction.execute(
        "INSERT INTO media_asset (
             item_id, file_id, name, notes, rating, source_urls_json,
             captured_at, imported_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            item_id,
            file_id,
            input.name,
            input.notes,
            input.rating,
            source_urls_json,
            input.captured_at,
            now,
        ],
    )?;
    Ok(item_id)
}

fn insert_root(
    transaction: &Transaction<'_>,
    item_id: i64,
    lifecycle: Lifecycle,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
        params![item_id, lifecycle.as_str()],
    )?;
    Ok(())
}

fn attach_source_item(
    transaction: &Transaction<'_>,
    source: &SourcePostInput,
    media_item_id: i64,
    now: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO source_post (
             site_id, post_key, canonical_url, creator_name, title, description,
             captured_at, metadata_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(site_id, post_key) DO UPDATE SET
             canonical_url = COALESCE(excluded.canonical_url, source_post.canonical_url),
             creator_name = COALESCE(excluded.creator_name, source_post.creator_name),
             title = COALESCE(excluded.title, source_post.title),
             description = COALESCE(excluded.description, source_post.description),
             captured_at = COALESCE(excluded.captured_at, source_post.captured_at),
             metadata_json = COALESCE(excluded.metadata_json, source_post.metadata_json),
             updated_at = excluded.updated_at",
        params![
            source.site_id,
            source.post_key,
            source.canonical_post_url,
            source.creator_name,
            source.title,
            source.description,
            source.captured_at,
            source.metadata_json,
            now,
        ],
    )?;
    let source_post_id: i64 = transaction.query_row(
        "SELECT source_post_id FROM source_post WHERE site_id = ?1 AND post_key = ?2",
        params![source.site_id, source.post_key],
        |row| row.get(0),
    )?;
    let changed = transaction.execute(
        "INSERT INTO source_item (
             source_post_id, item_key, position, media_url, canonical_url,
             media_item_id, state, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ingested', ?7, ?7)
         ON CONFLICT(source_post_id, item_key) DO UPDATE SET
             position = excluded.position,
             media_url = COALESCE(excluded.media_url, source_item.media_url),
             canonical_url = COALESCE(excluded.canonical_url, source_item.canonical_url),
             media_item_id = excluded.media_item_id,
             state = 'ingested',
             last_error = NULL,
             updated_at = excluded.updated_at
         WHERE source_item.state <> 'deleted'",
        params![
            source_post_id,
            source.item_key,
            source.position,
            source.canonical_media_url,
            source.canonical_media_url,
            media_item_id,
            now,
        ],
    )?;
    if changed != 1 {
        return Err(invalid(DELETED_SOURCE_ITEM_ERROR));
    }
    Ok(())
}

fn settle_source_post_root(
    transaction: &Transaction<'_>,
    source: &SourcePostInput,
    new_media_item_id: i64,
    lifecycle: Lifecycle,
    now: &str,
) -> rusqlite::Result<(i64, bool)> {
    let (source_post_id, current_root): (i64, Option<i64>) = transaction.query_row(
        "SELECT source_post_id, root_item_id FROM source_post
         WHERE site_id = ?1 AND post_key = ?2",
        params![source.site_id, source.post_key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut stmt = transaction.prepare(
        "SELECT media_item_id FROM source_item
         WHERE source_post_id = ?1 AND state = 'ingested' AND media_item_id IS NOT NULL
         ORDER BY position, source_item_id",
    )?;
    let media_ids = stmt
        .query_map([source_post_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    match (current_root, media_ids.len()) {
        (None, 1) => {
            insert_root(transaction, new_media_item_id, lifecycle)?;
            transaction.execute(
                "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                 WHERE source_post_id = ?3",
                params![new_media_item_id, now, source_post_id],
            )?;
            Ok((new_media_item_id, false))
        }
        (Some(root_id), 2) if root_kind(transaction, root_id)? == "media" => {
            let existing_lifecycle: String = transaction.query_row(
                "SELECT lifecycle FROM library_root WHERE item_id = ?1",
                [root_id],
                |row| row.get(0),
            )?;
            if existing_lifecycle != lifecycle.as_str() {
                return Err(invalid("Source post items cannot cross lifecycle scopes"));
            }
            transaction.execute(
                "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                 VALUES (?1, 'collection', ?2, ?2)",
                params![new_key("collection"), now],
            )?;
            let collection_id = transaction.last_insert_rowid();
            insert_root(transaction, collection_id, lifecycle)?;
            transaction.execute(
                "INSERT INTO folder_item (folder_id, item_id, position_rank)
                 SELECT folder_id, ?1, position_rank FROM folder_item WHERE item_id = ?2",
                params![collection_id, root_id],
            )?;
            for (index, media_id) in media_ids.iter().enumerate() {
                transaction.execute("DELETE FROM library_root WHERE item_id = ?1", [media_id])?;
                transaction.execute(
                    "INSERT INTO collection_member
                         (collection_id, media_item_id, position_rank)
                     VALUES (?1, ?2, ?3)",
                    params![collection_id, media_id, (index as i64 + 1) * RANK_GAP],
                )?;
            }
            transaction.execute(
                "UPDATE library_item SET cover_media_item_id = ?1 WHERE item_id = ?2",
                params![media_ids[0], collection_id],
            )?;
            transaction.execute(
                "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                 WHERE source_post_id = ?3",
                params![collection_id, now, source_post_id],
            )?;
            Ok((collection_id, true))
        }
        (Some(root_id), _) if root_kind(transaction, root_id)? == "collection" => {
            transaction.execute(
                "INSERT INTO collection_member
                     (collection_id, media_item_id, position_rank)
                 VALUES (?1, ?2, ?3)",
                params![root_id, new_media_item_id, (source.position + 1) * RANK_GAP],
            )?;
            Ok((root_id, false))
        }
        (Some(root_id), _) => Ok((root_id, false)),
        (None, _) => Err(invalid("Source post has media without a visible root")),
    }
}

fn root_kind(transaction: &Transaction<'_>, root_id: i64) -> rusqlite::Result<String> {
    transaction.query_row(
        "SELECT li.kind FROM library_root lr
         JOIN library_item li ON li.item_id = lr.item_id
         WHERE lr.item_id = ?1",
        [root_id],
        |row| row.get(0),
    )
}

fn insert_tags(
    transaction: &Transaction<'_>,
    media_item_id: i64,
    tags: &[String],
    provenance_mask: i64,
    external: bool,
) -> rusqlite::Result<Vec<i64>> {
    let mut tag_ids = BTreeSet::new();
    for tag in tags {
        let parsed = if external {
            crate::tag_name_v2::parse_external(tag)
        } else {
            crate::tag_name_v2::parse_local(tag)
        };
        let Ok((namespace, subtag)) = parsed else {
            continue;
        };
        transaction.execute(
            "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)
             ON CONFLICT(namespace, subtag) DO NOTHING",
            params![namespace, subtag],
        )?;
        let tag_id: i64 = transaction.query_row(
            "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
            params![namespace, subtag],
            |row| row.get(0),
        )?;
        let changed = transaction.execute(
            "INSERT INTO media_tag (media_item_id, tag_id, source, provenance_mask)
             VALUES (?1, ?2, ?4, ?3)
             ON CONFLICT(media_item_id, tag_id, source) DO UPDATE SET
                 provenance_mask = media_tag.provenance_mask | excluded.provenance_mask
             WHERE media_tag.provenance_mask <> (media_tag.provenance_mask | excluded.provenance_mask)",
            params![
                media_item_id,
                tag_id,
                provenance_mask,
                if external { "remote" } else { "local" }
            ],
        )?;
        if changed != 0 {
            tag_ids.insert(tag_id);
        }
    }
    Ok(tag_ids.into_iter().collect())
}

fn enqueue_derivatives(
    transaction: &Transaction<'_>,
    media_item_id: i64,
    file_id: i64,
    input: &PreparedMediaInput,
    enqueue_ai: bool,
    now: &str,
) -> rusqlite::Result<()> {
    let mut work = BTreeSet::from(["thumbnail", "dominant_colors"]);
    if input.mime_type.starts_with("image/") {
        work.insert("perceptual_hash");
        if enqueue_ai {
            work.insert("ai_tag");
        }
    }
    for work_type in work {
        transaction.execute(
            "INSERT INTO work_item (
                 media_item_id, file_id, work_type, status, attempt_count,
                 available_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'pending', 0, ?4, ?4, ?4)
             ON CONFLICT DO NOTHING",
            params![media_item_id, file_id, work_type, now],
        )?;
    }
    Ok(())
}

fn should_enqueue_ai(
    application: &Application,
    input: &PreparedMediaInput,
) -> Result<bool, String> {
    if !input.mime_type.starts_with("image/") {
        return Ok(false);
    }
    let settings = crate::settings_v2::application_settings(application)?.value;
    let auto = settings
        .get("aiTaggerAutoOnImport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let has_model = [
        "aiTaggerWd14Enabled",
        "aiTaggerE621Enabled",
        "aiTaggerEva02Enabled",
    ]
    .iter()
    .any(|key| {
        settings
            .get(*key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    });
    Ok(auto && has_model)
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn new_key(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}:{}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{PreparedMediaInput, SourcePostInput};
    use crate::app::{Application, ItemTarget, Lifecycle};
    use crate::store::Store;

    fn input(hash: &str, post: &str, item: &str, position: i64) -> PreparedMediaInput {
        PreparedMediaInput {
            file_hash: hash.to_string(),
            mime_type: "image/png".to_string(),
            size_bytes: 10,
            pixel_width: Some(10),
            pixel_height: Some(10),
            duration_ms: None,
            frame_count: Some(1),
            has_audio: false,
            name: Some(item.to_string()),
            notes: None,
            rating: None,
            source_urls: Vec::new(),
            tags: vec!["general:test".to_string()],
            provenance_mask: 1,
            lifecycle: Lifecycle::Inbox,
            captured_at: None,
            source: Some(SourcePostInput {
                site_id: "example".to_string(),
                post_key: post.to_string(),
                item_key: item.to_string(),
                position,
                canonical_post_url: None,
                canonical_media_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
            }),
            target_folder_id: None,
        }
    }

    #[test]
    fn promotes_second_source_item_and_reuses_physical_bytes_across_posts() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));

        let first = app
            .ingest_prepared(&input("same", "post-a", "a", 0))
            .unwrap();
        assert_eq!(first.media_item_id, first.root_item_id);
        let second = app
            .ingest_prepared(&input("other", "post-a", "b", 1))
            .unwrap();
        assert!(second.promoted_to_collection);
        assert_ne!(second.media_item_id, second.root_item_id);
        assert_eq!(
            app.projections().root_for_media(first.media_item_id.0),
            Some(second.root_item_id.0)
        );
        assert_eq!(
            app.projections().root_for_media(second.media_item_id.0),
            Some(second.root_item_id.0)
        );
        assert!(app
            .projections()
            .inbox_bitmap()
            .contains(second.root_item_id.0 as u32));
        assert!(!app
            .projections()
            .inbox_bitmap()
            .contains(first.media_item_id.0 as u32));
        let other_post = app
            .ingest_prepared(&input("same", "post-b", "a", 0))
            .unwrap();
        assert_ne!(other_post.media_item_id, first.media_item_id);

        app.store()
            .read(|connection| {
                let files: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?;
                let media: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM media_asset", [], |row| row.get(0))?;
                let roots: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))?;
                assert_eq!(files, 2);
                assert_eq!(media, 3);
                assert_eq!(roots, 2);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn repeated_source_item_is_idempotent_and_deleted_source_does_not_resurrect() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let input = input("hash", "post", "item", 0);
        let first = app.ingest_prepared(&input).unwrap();
        let repeated = app.ingest_prepared(&input).unwrap();
        assert!(repeated.reused_existing_item);
        assert!(repeated.receipt.is_none());
        assert_eq!(first.media_item_id, repeated.media_item_id);

        app.delete_items(&ItemTarget::Explicit {
            item_ids: vec![first.root_item_id],
        })
        .unwrap();
        let error = app.ingest_prepared(&input).unwrap_err();
        assert!(error.contains("cannot be resurrected"));
    }

    #[test]
    fn ingest_cancels_pending_cleanup_for_the_same_physical_file() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO work_item (
                         file_hash, work_type, status, attempt_count,
                         available_at, created_at, updated_at
                     ) VALUES ('hash', 'blob_delete', 'pending', 0, ?1, ?1, ?1)",
                    ["2026-01-01T00:00:00Z"],
                )?;
                Ok(())
            })
            .unwrap();

        app.ingest_prepared(&input("hash", "post", "item", 0))
            .unwrap();

        let cleanup_jobs = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM work_item
                     WHERE file_hash = 'hash' AND work_type = 'blob_delete'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(cleanup_jobs, 0);
    }

    #[test]
    fn repeated_manual_bytes_reuse_item_preserve_lifecycle_and_add_folders() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let first_folder = app
            .create_folder(&crate::folders_v2::CreateFolderInput {
                name: "First".to_string(),
                parent_id: None,
                folder_key: None,
            })
            .unwrap()
            .0;
        let second_folder = app
            .create_folder(&crate::folders_v2::CreateFolderInput {
                name: "Second".to_string(),
                parent_id: None,
                folder_key: None,
            })
            .unwrap()
            .0;
        let mut manual = input("manual-hash", "unused", "manual", 0);
        manual.source = None;
        manual.target_folder_id = Some(first_folder.0);
        let first = app.ingest_prepared(&manual).unwrap();

        manual.lifecycle = Lifecycle::Active;
        manual.target_folder_id = Some(second_folder.0);
        let repeated = app.ingest_prepared(&manual).unwrap();
        assert!(repeated.reused_existing_item);
        assert_eq!(first.media_item_id, repeated.media_item_id);
        assert!(app
            .projections()
            .inbox_bitmap()
            .contains(first.root_item_id.0 as u32));
        app.store()
            .read(|connection| {
                let media: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM media_asset", [], |row| row.get(0))?;
                let folders: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                    [first.root_item_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(media, 1);
                assert_eq!(folders, 2);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn source_collection_promotion_inherits_existing_folder_membership() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let folder = app
            .create_folder(&crate::folders_v2::CreateFolderInput {
                name: "Source".to_string(),
                parent_id: None,
                folder_key: None,
            })
            .unwrap()
            .0;
        let mut first_input = input("first", "post", "first", 0);
        first_input.target_folder_id = Some(folder.0);
        let first = app.ingest_prepared(&first_input).unwrap();
        let mut second_input = input("second", "post", "second", 1);
        second_input.target_folder_id = Some(folder.0);
        let second = app.ingest_prepared(&second_input).unwrap();
        assert!(second.promoted_to_collection);

        app.store()
            .read(|connection| {
                let collection_memberships: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id = ?1 AND item_id = ?2",
                    rusqlite::params![folder.0, second.root_item_id.0],
                    |row| row.get(0),
                )?;
                let old_memberships: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                    [first.media_item_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(collection_memberships, 1);
                assert_eq!(old_memberships, 0);
                Ok(())
            })
            .unwrap();
        app.set_lifecycle(
            &ItemTarget::Explicit {
                item_ids: vec![second.root_item_id],
            },
            Lifecycle::Active,
        )
        .unwrap();
        assert!(app
            .projections()
            .folder_bitmap(folder.0)
            .contains(second.root_item_id.0 as u32));
    }

    #[test]
    fn automatic_ai_work_is_only_enqueued_when_enabled_with_a_model() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.patch_application_settings(&serde_json::json!({
            "aiTaggerAutoOnImport": true,
            "aiTaggerWd14Enabled": true
        }))
        .unwrap();
        app.ingest_prepared(&input("ai-hash", "post", "item", 0))
            .unwrap();
        let work_types = app
            .store()
            .read(|connection| {
                let mut statement =
                    connection.prepare("SELECT work_type FROM work_item ORDER BY work_type")?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            work_types,
            ["ai_tag", "dominant_colors", "perceptual_hash", "thumbnail"]
        );
    }
}
