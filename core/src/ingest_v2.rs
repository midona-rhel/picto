//! One durable-media materialization path for manual and source imports.

use std::collections::{BTreeMap, BTreeSet};

use rand::RngCore;
use roaring::RoaringBitmap;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, Lifecycle, MutationReceipt};
use crate::projection_v2::{
    timestamp_ms, FolderProjectionChange, GroupOrderProjectionChange, ItemProjectionChange,
    MediaClassificationProjectionChange, MembershipProjectionChange, RootProjectionChange,
    RootSummaryProjectionChange, StructureProjectionDelta,
};

pub(crate) const DELETED_SOURCE_ITEM_ERROR: &str =
    "This source item was deliberately deleted and cannot be resurrected";

pub(crate) fn is_deleted_source_item_error(error: &str) -> bool {
    error.contains(DELETED_SOURCE_ITEM_ERROR)
}

const STAGED_ROOT_ORGANIZATION_KEY: &str = "_picto_root_organization";
const STAGED_SOURCE_METADATA_RAW_KEY: &str = "_picto_source_metadata_raw";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SourcePostInput {
    pub site_id: String,
    pub post_key: String,
    pub item_key: String,
    pub position: i64,
    #[serde(default)]
    pub post_complete: bool,
    /// Force a source container such as a ZIP to remain a collection even
    /// when it contains only one accepted item.
    #[serde(default)]
    pub force_collection: bool,
    /// Group accepted siblings from this source post into one collection.
    #[serde(default)]
    pub group_post: bool,
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
    pub lifecycle: Lifecycle,
    pub captured_at: Option<String>,
    pub source: Option<SourcePostInput>,
    #[serde(default)]
    pub target_folder_id: Option<i64>,
    #[serde(default)]
    pub target_folder_ids: Vec<i64>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StagedRootOrganization {
    #[serde(default)]
    cover_order: Option<(i64, String)>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    rating: Option<i64>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    source_urls: BTreeSet<String>,
    #[serde(default)]
    tags: Vec<StagedRootTag>,
    #[serde(default)]
    folder_ids: BTreeSet<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StagedRootTag {
    namespace: String,
    subtag: String,
}

#[derive(Default)]
struct RootOrganizationChanges {
    tag_ids: Vec<i64>,
    folder_ids: Vec<i64>,
    metadata_changed: bool,
}

#[derive(Default)]
struct IngestProjectionDelta {
    structure: StructureProjectionDelta,
    root_tags_added: Vec<(i64, i64)>,
    root_tags_removed: Vec<(i64, i64)>,
    summary_root_ids: BTreeSet<i64>,
    summaries: Vec<RootSummaryProjectionChange>,
}

impl IngestProjectionDelta {
    fn add_organization(&mut self, root_item_id: i64, changes: RootOrganizationChanges) {
        self.summary_root_ids.insert(root_item_id);
        self.root_tags_added.extend(
            changes
                .tag_ids
                .into_iter()
                .map(|tag_id| (root_item_id, tag_id)),
        );
        self.structure
            .folders
            .extend(
                changes
                    .folder_ids
                    .into_iter()
                    .map(|folder_id| FolderProjectionChange {
                        folder_id,
                        item_id: root_item_id,
                        present: true,
                    }),
            );
    }

    fn prepare_summaries(&mut self, transaction: &Transaction<'_>) -> rusqlite::Result<()> {
        for group in &self.structure.group_orders {
            crate::operations_v2::upsert_group_root_summary(
                transaction,
                group.collection_id,
                &group.media_ids,
            )?;
        }
        self.summary_root_ids.extend(
            self.structure
                .roots
                .iter()
                .filter(|change| change.lifecycle.is_some())
                .map(|change| change.item_id),
        );
        self.summary_root_ids.extend(
            self.structure
                .memberships
                .iter()
                .map(|change| change.collection_id),
        );

        self.summaries.clear();
        for item_id in &self.summary_root_ids {
            let summary = transaction
                .query_row(
                    "SELECT summary.total_size_bytes, summary.media_count,
                            summary.sort_rating, file.duration_ms,
                            file.pixel_width, file.pixel_height,
                            summary.imported_at, summary.updated_at
                     FROM root_summary summary
                     LEFT JOIN media_asset cover
                       ON cover.item_id = summary.cover_media_item_id
                     LEFT JOIN media_file file ON file.file_id = cover.file_id
                     WHERE summary.root_item_id = ?1",
                    [item_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, Option<i64>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    },
                )
                .optional()?;
            let Some((
                total_size_bytes,
                media_count,
                rating,
                duration_ms,
                pixel_width,
                pixel_height,
                imported_at,
                modified_at,
            )) = summary
            else {
                continue;
            };
            self.summaries.push(RootSummaryProjectionChange {
                item_id: *item_id,
                total_size_bytes: u64::try_from(total_size_bytes)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, total_size_bytes))?,
                media_count: u64::try_from(media_count)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, media_count))?,
                rating: rating
                    .map(|rating| {
                        u8::try_from(rating)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, rating))
                    })
                    .transpose()?,
                display_duration_ms: optional_nonnegative_u64(duration_ms, 3)?,
                display_width: optional_nonnegative_u64(pixel_width, 4)?,
                display_height: optional_nonnegative_u64(pixel_height, 5)?,
                imported_at_ms: imported_at.as_deref().and_then(timestamp_ms),
                modified_at_ms: modified_at.as_deref().and_then(timestamp_ms),
            });
        }
        Ok(())
    }
}

fn optional_nonnegative_u64(value: Option<i64>, index: usize) -> rusqlite::Result<Option<u64>> {
    value
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

fn settle_ingest_projection(
    projections: &crate::projection_v2::ProjectionStore,
    delta: IngestProjectionDelta,
) -> Result<(), String> {
    let mut removed_by_tag = BTreeMap::<i64, RoaringBitmap>::new();
    for (root_id, tag_id) in delta.root_tags_removed {
        let root_id = u32::try_from(root_id)
            .map_err(|_| format!("Item ID {root_id} exceeds projection capacity"))?;
        removed_by_tag.entry(tag_id).or_default().insert(root_id);
    }
    for (tag_id, root_ids) in removed_by_tag {
        projections.apply_root_tag_bitmap(tag_id, &root_ids, false)?;
    }

    projections.apply_structure_delta(delta.structure)?;
    projections.apply_root_summary_changes(&delta.summaries, &RoaringBitmap::new())?;

    let mut added_by_tag = BTreeMap::<i64, RoaringBitmap>::new();
    for (root_id, tag_id) in delta.root_tags_added {
        let root_id = u32::try_from(root_id)
            .map_err(|_| format!("Item ID {root_id} exceeds projection capacity"))?;
        added_by_tag.entry(tag_id).or_default().insert(root_id);
    }
    for (tag_id, root_ids) in added_by_tag {
        projections.apply_root_tag_bitmap(tag_id, &root_ids, true)?;
    }
    Ok(())
}

struct RootSettlement {
    root_item_id: i64,
    promoted: bool,
    visible: bool,
    replaced_root_item_id: Option<i64>,
    removed_root_tag_ids: Vec<i64>,
    group_order: Option<Vec<i64>>,
}

impl Application {
    pub(crate) fn ingest_prepared(
        &self,
        input: &PreparedMediaInput,
    ) -> Result<IngestMediaResult, String> {
        validate_input(input)?;
        let enqueue_ai = should_enqueue_ai(self, input)?;
        let now = chrono::Utc::now().to_rfc3339();
        let ((media_item_id, root_item_id, reused, promoted, visible), revision, changed) = self
            .transaction_if_changed_maintenance(
                |transaction| {
                    if let Some(source) = &input.source {
                        if let Some(existing) =
                            existing_source_item(transaction, self.projections(), source)?
                        {
                            match existing {
                                ExistingSourceItem::Present {
                                    media_item_id,
                                    root_item_id,
                                    root_visible,
                                } => {
                                    let mut delta = IngestProjectionDelta::default();
                                    let changed = if root_visible {
                                        let organization =
                                            StagedRootOrganization::from_input(input, true);
                                        let preserve_cover =
                                            root_kind(transaction, root_item_id)? == "collection";
                                        let changes = merge_root_organization(
                                            transaction,
                                            self.projections(),
                                            root_item_id,
                                            &organization,
                                            preserve_cover,
                                            &now,
                                        )?;
                                        let changed = changes.metadata_changed
                                            || !changes.tag_ids.is_empty()
                                            || !changes.folder_ids.is_empty();
                                        delta.add_organization(root_item_id, changes);
                                        changed
                                    } else {
                                        stage_source_root_organization(
                                            transaction,
                                            source,
                                            input,
                                            &now,
                                        )?
                                    };
                                    if changed {
                                        delta.prepare_summaries(transaction)?;
                                    }
                                    return Ok((
                                        (media_item_id, root_item_id, true, false, root_visible),
                                        delta,
                                        changed,
                                    ));
                                }
                                ExistingSourceItem::Deleted => {
                                    return Err(invalid(DELETED_SOURCE_ITEM_ERROR));
                                }
                                ExistingSourceItem::Pending => {}
                            }
                        }
                    } else if let Some((media_item_id, root_item_id)) =
                        existing_manual_item(transaction, self.projections(), &input.file_hash)?
                    {
                        let mut delta = IngestProjectionDelta::default();
                        let organization = StagedRootOrganization::from_input(input, false);
                        let preserve_cover = root_kind(transaction, root_item_id)? == "collection";
                        let changes = merge_root_organization(
                            transaction,
                            self.projections(),
                            root_item_id,
                            &organization,
                            preserve_cover,
                            &now,
                        )?;
                        let changed = changes.metadata_changed
                            || !changes.tag_ids.is_empty()
                            || !changes.folder_ids.is_empty();
                        delta.add_organization(root_item_id, changes);
                        if changed {
                            delta.prepare_summaries(transaction)?;
                        }
                        return Ok((
                            (media_item_id, root_item_id, true, false, true),
                            delta,
                            changed,
                        ));
                    }

                    let file_id = upsert_file(transaction, input, &now)?;
                    let media_item_id = insert_media_asset(transaction, file_id, input, &now)?;
                    let settlement = if let Some(source) = &input.source {
                        attach_source_item(transaction, source, media_item_id, &now)?;
                        if source.group_post || source.force_collection {
                            stage_source_root_organization(transaction, source, input, &now)?;
                        }
                        let settled = settle_source_post_root(
                            transaction,
                            self.projections(),
                            source,
                            media_item_id,
                            input.lifecycle,
                            &now,
                        )?;
                        let (source_post_id, source_item_id): (i64, i64) = transaction.query_row(
                            "SELECT sp.source_post_id, si.source_item_id
                             FROM source_post sp
                             JOIN source_item si ON si.source_post_id = sp.source_post_id
                             WHERE sp.site_id = ?1 AND sp.post_key = ?2 AND si.item_key = ?3",
                            params![source.site_id, source.post_key, source.item_key],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )?;
                        let mut post_fields = vec!["exists", "root_item"];
                        if source.canonical_post_url.is_some() {
                            post_fields.push("canonical_url");
                        }
                        if source.creator_name.is_some() {
                            post_fields.push("creator_name");
                        }
                        if source.title.is_some() {
                            post_fields.push("title");
                        }
                        if source.description.is_some() {
                            post_fields.push("description");
                        }
                        if source.captured_at.is_some() {
                            post_fields.push("captured_at");
                        }
                        if source.metadata_json.is_some() {
                            post_fields.push("metadata_json");
                        }
                        crate::cloud::capture::record_source_post_upsert(
                            transaction,
                            source_post_id,
                            &post_fields,
                        )?;
                        crate::cloud::capture::record_source_item_upsert(
                            transaction,
                            source_item_id,
                            &[
                                "exists",
                                "position",
                                "media_url",
                                "canonical_url",
                                "media_item",
                            ],
                        )?;
                        settled
                    } else {
                        insert_root(transaction, media_item_id, input.lifecycle)?;
                        RootSettlement {
                            root_item_id: media_item_id,
                            promoted: false,
                            visible: true,
                            replaced_root_item_id: None,
                            removed_root_tag_ids: Vec::new(),
                            group_order: None,
                        }
                    };
                    let root_item_id = settlement.root_item_id;
                    let promoted = settlement.promoted;
                    let root_visible = settlement.visible;
                    if root_visible {
                        enqueue_root_thumbnail(transaction, root_item_id, &now)?;
                    }
                    enqueue_derivatives(
                        transaction,
                        media_item_id,
                        file_id,
                        input,
                        enqueue_ai,
                        &now,
                    )?;
                    let mut delta = IngestProjectionDelta::default();
                    delta.structure.items.push(ItemProjectionChange {
                        item_id: media_item_id,
                        kind: crate::app::ItemKind::Media,
                        present: true,
                    });
                    delta.structure.media_classifications.push(
                        MediaClassificationProjectionChange {
                            media_id: media_item_id,
                            is_image: input.mime_type.starts_with("image/"),
                            mime_type: input.mime_type.clone(),
                        },
                    );
                    if media_item_id == root_item_id {
                        delta.structure.roots.push(RootProjectionChange {
                            item_id: media_item_id,
                            lifecycle: Some(input.lifecycle),
                        });
                    } else if promoted {
                        delta.structure.items.push(ItemProjectionChange {
                            item_id: root_item_id,
                            kind: crate::app::ItemKind::Collection,
                            present: true,
                        });
                        delta.structure.roots.push(RootProjectionChange {
                            item_id: root_item_id,
                            lifecycle: Some(input.lifecycle),
                        });
                        let members = settlement.group_order.as_ref().ok_or_else(|| {
                            invalid("Promoted source group is missing canonical member order")
                        })?;
                        let folder_ids = settlement
                            .replaced_root_item_id
                            .map(|root_id| self.projections().folder_ids_for_root(root_id))
                            .unwrap_or_default();
                        for folder_id in folder_ids {
                            delta.structure.folders.push(FolderProjectionChange {
                                folder_id,
                                item_id: root_item_id,
                                present: true,
                            });
                            if let Some(replaced_root_item_id) = settlement.replaced_root_item_id {
                                delta.structure.folders.push(FolderProjectionChange {
                                    folder_id,
                                    item_id: replaced_root_item_id,
                                    present: false,
                                });
                            }
                        }
                        for member_id in members {
                            delta.structure.roots.push(RootProjectionChange {
                                item_id: *member_id,
                                lifecycle: None,
                            });
                            delta
                                .structure
                                .memberships
                                .push(MembershipProjectionChange {
                                    collection_id: root_item_id,
                                    media_id: *member_id,
                                    present: true,
                                });
                        }
                    } else {
                        delta
                            .structure
                            .memberships
                            .push(MembershipProjectionChange {
                                collection_id: root_item_id,
                                media_id: media_item_id,
                                present: true,
                            });
                    }
                    if let Some(media_ids) = settlement.group_order.clone() {
                        delta
                            .structure
                            .group_orders
                            .push(GroupOrderProjectionChange {
                                collection_id: root_item_id,
                                media_ids,
                            });
                    }
                    if root_visible {
                        let organization = if input
                            .source
                            .as_ref()
                            .is_some_and(|source| source.group_post || source.force_collection)
                        {
                            take_staged_source_root_organization(
                                transaction,
                                input.source.as_ref().expect("source checked above"),
                            )?
                            .unwrap_or_else(|| StagedRootOrganization::from_input(input, true))
                        } else {
                            StagedRootOrganization::from_input(input, input.source.is_some())
                        };
                        let preserve_cover = root_kind(transaction, root_item_id)? == "collection";
                        let changes = merge_root_organization(
                            transaction,
                            self.projections(),
                            root_item_id,
                            &organization,
                            preserve_cover,
                            &now,
                        )?;
                        delta.add_organization(root_item_id, changes);
                        if promoted {
                            delta.root_tags_added.extend(
                                settlement
                                    .removed_root_tag_ids
                                    .iter()
                                    .copied()
                                    .map(|tag_id| (root_item_id, tag_id)),
                            );
                        }
                    }
                    if let Some(replaced_root_item_id) = settlement.replaced_root_item_id {
                        delta.root_tags_removed.extend(
                            settlement
                                .removed_root_tag_ids
                                .into_iter()
                                .map(|tag_id| (replaced_root_item_id, tag_id)),
                        );
                    }
                    delta.prepare_summaries(transaction)?;
                    Ok((
                        (media_item_id, root_item_id, false, promoted, root_visible),
                        delta,
                        true,
                    ))
                },
                settle_ingest_projection,
            )?;

        // Provisional source groups intentionally have no library root. Their
        // canonical members may commit progressively, but no visible-library
        // invalidation is published until the coherent group root exists.
        let receipt = (changed && visible).then(|| MutationReceipt {
            revision,
            resources: {
                let mut changed_resources = vec![
                    resources::LIBRARY.to_string(),
                    resources::SIDEBAR.to_string(),
                    resources::DUPLICATES.to_string(),
                    resources::TASKS.to_string(),
                ];
                if input.target_folder_id.is_some() || !input.target_folder_ids.is_empty() {
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

    pub(crate) fn recover_provisional_source_root(
        &self,
        collection_id: i64,
        source_post_id: i64,
        lifecycle: Lifecycle,
    ) -> Result<bool, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let (_, _, changed) = self.transaction_if_changed_maintenance(
            |transaction| {
                let inserted = transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)
                     ON CONFLICT(item_id) DO NOTHING",
                    params![collection_id, lifecycle.as_str()],
                )? != 0;
                if !inserted {
                    return Ok(((), IngestProjectionDelta::default(), false));
                }

                let metadata = transaction.query_row(
                    "SELECT metadata_json FROM source_post WHERE source_post_id = ?1",
                    [source_post_id],
                    |row| row.get::<_, Option<String>>(0),
                )?;
                let organization = take_staged_root_organization_by_id(
                    transaction,
                    source_post_id,
                    metadata.as_deref(),
                )?;
                let organization = match organization {
                    Some(organization) => organization,
                    None => staged_root_organization_from_jobs(transaction, source_post_id)?,
                };
                let changes = merge_root_organization(
                    transaction,
                    self.projections(),
                    collection_id,
                    &organization,
                    false,
                    &now,
                )?;
                transaction.execute(
                    "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                     WHERE source_post_id = ?3 AND root_item_id IS NULL",
                    params![collection_id, now, source_post_id],
                )?;
                enqueue_root_thumbnail(transaction, collection_id, &now)?;

                let mut delta = IngestProjectionDelta::default();
                delta.structure.items.push(ItemProjectionChange {
                    item_id: collection_id,
                    kind: crate::app::ItemKind::Collection,
                    present: true,
                });
                delta.structure.roots.push(RootProjectionChange {
                    item_id: collection_id,
                    lifecycle: Some(lifecycle),
                });
                let members = source_media_order(transaction, source_post_id)?;
                set_group_cover(transaction, collection_id, &members)?;
                for media_id in &members {
                    delta
                        .structure
                        .memberships
                        .push(MembershipProjectionChange {
                            collection_id,
                            media_id: *media_id,
                            present: true,
                        });
                }
                delta
                    .structure
                    .group_orders
                    .push(GroupOrderProjectionChange {
                        collection_id,
                        media_ids: members,
                    });
                delta.add_organization(collection_id, changes);
                delta.prepare_summaries(transaction)?;
                Ok(((), delta, true))
            },
            settle_ingest_projection,
        )?;
        Ok(changed)
    }
}

fn staged_root_organization_from_jobs(
    transaction: &Transaction<'_>,
    source_post_id: i64,
) -> rusqlite::Result<StagedRootOrganization> {
    let mut statement = transaction.prepare(
        "SELECT ingest_job.payload_json
         FROM source_item
         JOIN ingest_job USING (source_item_id)
         WHERE source_item.source_post_id = ?1
           AND ingest_job.payload_json <> '{}'
         ORDER BY source_item.position, source_item.source_item_id",
    )?;
    let payloads = statement
        .query_map([source_post_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut organization = StagedRootOrganization::default();
    for payload in payloads {
        if let Ok(input) = serde_json::from_str::<PreparedMediaInput>(&payload) {
            organization.merge(StagedRootOrganization::from_input(&input, true));
        }
    }
    Ok(organization)
}

enum ExistingSourceItem {
    Present {
        media_item_id: i64,
        root_item_id: i64,
        root_visible: bool,
    },
    Pending,
    Deleted,
}

fn existing_manual_item(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    file_hash: &str,
) -> rusqlite::Result<Option<(i64, i64)>> {
    let media_item_id = transaction
        .query_row(
            "SELECT ma.item_id
             FROM media_asset ma
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE mf.file_hash = ?1
             ORDER BY ma.item_id
             LIMIT 1",
            [file_hash],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(media_item_id.and_then(|media_item_id| {
        projections
            .root_for_media(media_item_id)
            .map(|root_item_id| (media_item_id, root_item_id))
    }))
}

impl StagedRootOrganization {
    fn from_input(input: &PreparedMediaInput, external_tags: bool) -> Self {
        let mut organization = Self::default();
        let source_order = input
            .source
            .as_ref()
            .map(|source| (source.position, source.item_key.clone()))
            .unwrap_or_else(|| (0, String::new()));
        organization.cover_order = Some(source_order);
        organization.name = trimmed(input.name.as_deref())
            .map(str::to_string)
            .or_else(|| {
                input
                    .source
                    .as_ref()
                    .and_then(|source| trimmed(source.title.as_deref()))
                    .map(str::to_string)
            });
        organization.rating = input.rating;
        organization.notes = trimmed(input.notes.as_deref()).map(str::to_string);
        organization.source_urls.extend(
            input
                .source_urls
                .iter()
                .filter_map(|url| trimmed(Some(url.as_str())).map(str::to_string)),
        );
        if let Some(source) = &input.source {
            organization.source_urls.extend(
                [
                    source.canonical_post_url.as_deref(),
                    source.canonical_media_url.as_deref(),
                ]
                .into_iter()
                .filter_map(|url| trimmed(url).map(str::to_string)),
            );
        }
        organization.folder_ids.extend(
            input
                .target_folder_id
                .into_iter()
                .chain(input.target_folder_ids.iter().copied()),
        );
        organization.tags = parsed_root_tags(input, external_tags);
        organization
    }

    fn merge(&mut self, incoming: Self) {
        self.source_urls.extend(incoming.source_urls);
        self.folder_ids.extend(incoming.folder_ids);

        let mut tags = self
            .tags
            .drain(..)
            .map(|tag| (tag.namespace, tag.subtag))
            .collect::<BTreeSet<_>>();
        for tag in incoming.tags {
            tags.insert((tag.namespace, tag.subtag));
        }
        self.tags = tags
            .into_iter()
            .map(|(namespace, subtag)| StagedRootTag { namespace, subtag })
            .collect();

        if incoming.cover_order < self.cover_order || self.cover_order.is_none() {
            self.cover_order = incoming.cover_order;
            self.name = incoming.name;
            self.rating = incoming.rating;
            self.notes = incoming.notes;
        }
    }
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parsed_root_tags(input: &PreparedMediaInput, external: bool) -> Vec<StagedRootTag> {
    input
        .tags
        .iter()
        .filter_map(|tag| {
            let parsed = if external {
                crate::tag_name_v2::parse_external(tag)
            } else {
                crate::tag_name_v2::parse_local(tag)
            };
            parsed.ok()
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|(namespace, subtag)| StagedRootTag { namespace, subtag })
        .collect()
}

fn merge_root_organization(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    root_item_id: i64,
    organization: &StagedRootOrganization,
    preserve_cover_fields: bool,
    now: &str,
) -> rusqlite::Result<RootOrganizationChanges> {
    let root_bitmap_id = u32::try_from(root_item_id).map_err(|_| {
        rusqlite::Error::InvalidParameterName(format!(
            "root item {root_item_id} is outside the bitmap ID domain"
        ))
    })?;
    let existing = transaction
        .query_row(
            "SELECT name, rating, notes, source_urls_json
             FROM root_metadata WHERE root_item_id = ?1",
            [root_item_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;

    let mut source_urls = existing
        .as_ref()
        .and_then(|(_, _, _, json)| serde_json::from_str::<BTreeSet<String>>(json).ok())
        .unwrap_or_default();
    source_urls.extend(organization.source_urls.iter().cloned());
    let source_urls_json = serde_json::to_string(&source_urls)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;

    let (name, rating, notes) = match existing.as_ref() {
        None => (
            organization.name.clone(),
            organization.rating,
            organization.notes.clone(),
        ),
        Some((name, rating, notes, _)) if preserve_cover_fields => {
            (name.clone(), *rating, notes.clone())
        }
        Some((name, rating, notes, _)) => (
            name.clone().or_else(|| organization.name.clone()),
            rating.or(organization.rating),
            merge_notes(notes.as_deref(), organization.notes.as_deref()),
        ),
    };

    let metadata_changed = transaction.execute(
        "INSERT INTO root_metadata (
             root_item_id, name, rating, notes, source_urls_json, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(root_item_id) DO UPDATE SET
             name = excluded.name,
             rating = excluded.rating,
             notes = excluded.notes,
             source_urls_json = excluded.source_urls_json,
             updated_at = excluded.updated_at
         WHERE root_metadata.name IS NOT excluded.name
            OR root_metadata.rating IS NOT excluded.rating
            OR root_metadata.notes IS NOT excluded.notes
            OR root_metadata.source_urls_json <> excluded.source_urls_json",
        params![root_item_id, name, rating, notes, source_urls_json, now],
    )? != 0;

    let encoded_tags = serde_json::to_string(&organization.tags)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "WITH input(namespace, subtag) AS (
             SELECT json_extract(value, '$.namespace'), json_extract(value, '$.subtag')
             FROM json_each(?1)
         )
         INSERT INTO tag (namespace, subtag)
         SELECT namespace, subtag FROM input WHERE 1
         ON CONFLICT(namespace, subtag) DO NOTHING",
        [&encoded_tags],
    )?;
    let tag_ids = transaction
        .prepare(
            "WITH input(namespace, subtag) AS (
                 SELECT json_extract(value, '$.namespace'),
                        json_extract(value, '$.subtag')
                 FROM json_each(?1)
             )
             SELECT tag.tag_id
             FROM input JOIN tag USING (namespace, subtag)
             ORDER BY tag.tag_id",
        )?
        .query_map([encoded_tags], |row| row.get::<_, i64>(0))?
        .filter_map(|tag_id| match tag_id {
            Ok(tag_id)
                if !projections
                    .direct_tag_bitmap(tag_id)
                    .contains(root_bitmap_id) =>
            {
                Some(Ok(tag_id))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let encoded_folders = serde_json::to_string(&organization.folder_ids)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let folder_ids = {
        let current = projections.folder_ids_for_root(root_item_id);
        let mut statement = transaction.prepare(
            "SELECT folder.folder_id
             FROM json_each(?1) input
             JOIN folder ON folder.folder_id = CAST(input.value AS INTEGER)
             ORDER BY folder.folder_id",
        )?;
        let missing = statement
            .query_map([encoded_folders], |row| row.get::<_, i64>(0))?
            .filter_map(|folder_id| match folder_id {
                Ok(folder_id) if !current.contains(&folder_id) => Some(Ok(folder_id)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        missing
    };

    Ok(RootOrganizationChanges {
        tag_ids,
        folder_ids,
        metadata_changed,
    })
}

fn merge_notes(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
    let existing = trimmed(existing);
    let incoming = trimmed(incoming);
    match (existing, incoming) {
        (None, None) => None,
        (Some(existing), None) => Some(existing.to_string()),
        (None, Some(incoming)) => Some(incoming.to_string()),
        (Some(existing), Some(incoming))
            if existing.split("\n\n").any(|part| part.trim() == incoming) =>
        {
            Some(existing.to_string())
        }
        (Some(existing), Some(incoming)) => Some(format!("{existing}\n\n{incoming}")),
    }
}

fn validate_input(input: &PreparedMediaInput) -> Result<(), String> {
    if input.file_hash.trim().is_empty() {
        return Err("A physical file hash is required".to_string());
    }
    if input.size_bytes < 0 {
        return Err("Media size cannot be negative".to_string());
    }
    if !crate::media_processing::formats::is_supported_mime(&input.mime_type)
        || input.mime_type == "application/zip"
    {
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
    projections: &crate::projection_v2::ProjectionStore,
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
    row.map(|(state, media_item_id, persisted_root_id)| {
        if state == "deleted" {
            Ok(ExistingSourceItem::Deleted)
        } else if media_item_id.is_none() {
            Ok(ExistingSourceItem::Pending)
        } else {
            let media_item_id = media_item_id.unwrap();
            let root_item_id = persisted_root_id
                .or_else(|| projections.root_for_media(media_item_id))
                .unwrap_or(media_item_id);
            let root_visible = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_root WHERE item_id = ?1)",
                [root_item_id],
                |row| row.get(0),
            )?;
            Ok(ExistingSourceItem::Present {
                media_item_id,
                root_item_id,
                root_visible,
            })
        }
    })
    .transpose()
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
    transaction.execute(
        "INSERT INTO media_asset (
             item_id, file_id, name, captured_at, imported_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![item_id, file_id, input.name, input.captured_at, now,],
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
    let previous_metadata = transaction
        .query_row(
            "SELECT metadata_json FROM source_post WHERE site_id = ?1 AND post_key = ?2",
            params![source.site_id, source.post_key],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let metadata_json = preserve_staged_root_organization(
        previous_metadata.as_deref(),
        source.metadata_json.as_deref(),
    )?;
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
            metadata_json,
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

fn preserve_staged_root_organization(
    previous: Option<&str>,
    incoming: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    let Some(incoming) = incoming else {
        return Ok(previous.map(str::to_string));
    };
    let Some(staged) = parse_metadata_object(previous).remove(STAGED_ROOT_ORGANIZATION_KEY) else {
        return Ok(Some(incoming.to_string()));
    };
    let mut incoming_value = metadata_staging_object(Some(incoming));
    incoming_value.insert(
        STAGED_SOURCE_METADATA_RAW_KEY.to_string(),
        serde_json::Value::String(incoming.to_string()),
    );
    incoming_value.insert(STAGED_ROOT_ORGANIZATION_KEY.to_string(), staged);
    encode_metadata_staging_object(incoming_value)
}

fn parse_metadata_object(metadata: Option<&str>) -> serde_json::Map<String, serde_json::Value> {
    metadata_staging_object(metadata)
}

fn metadata_staging_object(metadata: Option<&str>) -> serde_json::Map<String, serde_json::Value> {
    let Some(metadata) = metadata else {
        return serde_json::Map::new();
    };
    serde_json::from_str::<serde_json::Value>(metadata)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_else(|| {
            serde_json::Map::from_iter([(
                STAGED_SOURCE_METADATA_RAW_KEY.to_string(),
                serde_json::Value::String(metadata.to_string()),
            )])
        })
}

fn encode_metadata_staging_object(
    mut metadata: serde_json::Map<String, serde_json::Value>,
) -> rusqlite::Result<Option<String>> {
    if !metadata.contains_key(STAGED_ROOT_ORGANIZATION_KEY) {
        if let Some(serde_json::Value::String(raw)) =
            metadata.remove(STAGED_SOURCE_METADATA_RAW_KEY)
        {
            return Ok(Some(raw));
        }
    }
    if metadata.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&metadata)
        .map(Some)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn stage_source_root_organization(
    transaction: &Transaction<'_>,
    source: &SourcePostInput,
    input: &PreparedMediaInput,
    now: &str,
) -> rusqlite::Result<bool> {
    let (source_post_id, metadata_json): (i64, Option<String>) = transaction.query_row(
        "SELECT source_post_id, metadata_json FROM source_post
         WHERE site_id = ?1 AND post_key = ?2",
        params![source.site_id, source.post_key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut metadata = parse_metadata_object(metadata_json.as_deref());
    if !metadata.contains_key(STAGED_ROOT_ORGANIZATION_KEY) {
        if let Some(metadata_json) = metadata_json {
            metadata.insert(
                STAGED_SOURCE_METADATA_RAW_KEY.to_string(),
                serde_json::Value::String(metadata_json),
            );
        }
    }
    let mut organization = metadata
        .get(STAGED_ROOT_ORGANIZATION_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<StagedRootOrganization>(value).ok())
        .unwrap_or_default();
    organization.merge(StagedRootOrganization::from_input(input, true));
    metadata.insert(
        STAGED_ROOT_ORGANIZATION_KEY.to_string(),
        serde_json::to_value(organization)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
    );
    let encoded = encode_metadata_staging_object(metadata)?;
    Ok(transaction.execute(
        "UPDATE source_post SET metadata_json = ?1, updated_at = ?2
         WHERE source_post_id = ?3 AND metadata_json IS NOT ?1",
        params![encoded, now, source_post_id],
    )? != 0)
}

fn take_staged_source_root_organization(
    transaction: &Transaction<'_>,
    source: &SourcePostInput,
) -> rusqlite::Result<Option<StagedRootOrganization>> {
    let (source_post_id, metadata): (i64, Option<String>) = transaction.query_row(
        "SELECT source_post_id, metadata_json FROM source_post
         WHERE site_id = ?1 AND post_key = ?2",
        params![source.site_id, source.post_key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    take_staged_root_organization_by_id(transaction, source_post_id, metadata.as_deref())
}

fn take_staged_root_organization_by_id(
    transaction: &Transaction<'_>,
    source_post_id: i64,
    metadata: Option<&str>,
) -> rusqlite::Result<Option<StagedRootOrganization>> {
    let mut metadata = parse_metadata_object(metadata);
    let organization = metadata
        .remove(STAGED_ROOT_ORGANIZATION_KEY)
        .and_then(|value| serde_json::from_value(value).ok());
    if organization.is_some() {
        let encoded = encode_metadata_staging_object(metadata)?;
        transaction.execute(
            "UPDATE source_post SET metadata_json = ?1 WHERE source_post_id = ?2",
            params![encoded, source_post_id],
        )?;
    }
    Ok(organization)
}

fn settle_source_post_root(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    source: &SourcePostInput,
    new_media_item_id: i64,
    lifecycle: Lifecycle,
    now: &str,
) -> rusqlite::Result<RootSettlement> {
    let (source_post_id, current_root): (i64, Option<i64>) = transaction.query_row(
        "SELECT source_post_id, root_item_id FROM source_post
         WHERE site_id = ?1 AND post_key = ?2",
        params![source.site_id, source.post_key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    if !source.group_post && !source.force_collection {
        insert_root(transaction, new_media_item_id, lifecycle)?;
        return Ok(RootSettlement {
            root_item_id: new_media_item_id,
            promoted: false,
            visible: true,
            replaced_root_item_id: None,
            removed_root_tag_ids: Vec::new(),
            group_order: None,
        });
    }

    let provisional_collection =
        provisional_source_collection(transaction, projections, source_post_id)?;
    if current_root.is_none()
        && (source.force_collection || provisional_collection.is_some() || !source.post_complete)
    {
        let collection_id = if let Some(collection_id) = provisional_collection {
            collection_id
        } else {
            transaction.execute(
                "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                 VALUES (?1, 'collection', ?2, ?2)",
                params![new_key("collection"), now],
            )?;
            transaction.last_insert_rowid()
        };
        let group_order = source_media_order(transaction, source_post_id)?;
        set_group_cover(transaction, collection_id, &group_order)?;
        if source.post_complete {
            insert_root(transaction, collection_id, lifecycle)?;
            transaction.execute(
                "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                 WHERE source_post_id = ?3",
                params![collection_id, now, source_post_id],
            )?;
            return Ok(RootSettlement {
                root_item_id: collection_id,
                promoted: true,
                visible: true,
                replaced_root_item_id: None,
                removed_root_tag_ids: Vec::new(),
                group_order: Some(group_order),
            });
        }
        return Ok(RootSettlement {
            root_item_id: collection_id,
            promoted: false,
            visible: false,
            replaced_root_item_id: None,
            removed_root_tag_ids: Vec::new(),
            group_order: Some(group_order),
        });
    }

    let media_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM source_item
         WHERE source_post_id = ?1 AND state = 'ingested' AND media_item_id IS NOT NULL",
        [source_post_id],
        |row| row.get(0),
    )?;

    match (current_root, media_count) {
        (None, 1) => {
            insert_root(transaction, new_media_item_id, lifecycle)?;
            transaction.execute(
                "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                 WHERE source_post_id = ?3",
                params![new_media_item_id, now, source_post_id],
            )?;
            Ok(RootSettlement {
                root_item_id: new_media_item_id,
                promoted: false,
                visible: true,
                replaced_root_item_id: None,
                removed_root_tag_ids: Vec::new(),
                group_order: None,
            })
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
            let removed_root_tag_ids = projections.tag_ids_for_root(root_id);
            transaction.execute(
                "INSERT INTO root_metadata (
                     root_item_id, name, rating, notes, source_urls_json, updated_at
                 )
                 SELECT ?1, name, rating, notes, source_urls_json, ?2
                 FROM root_metadata WHERE root_item_id = ?3
                 ON CONFLICT(root_item_id) DO NOTHING",
                params![collection_id, now, root_id],
            )?;
            let group_order = source_media_order(transaction, source_post_id)?;
            transaction.execute(
                "DELETE FROM library_root
                 WHERE item_id IN (
                     SELECT media_item_id FROM source_item
                     WHERE source_post_id = ?1
                       AND state = 'ingested'
                       AND media_item_id IS NOT NULL
                     ORDER BY position, source_item_id
                     LIMIT 2
                 )",
                [source_post_id],
            )?;
            set_group_cover(transaction, collection_id, &group_order)?;
            transaction.execute(
                "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                 WHERE source_post_id = ?3",
                params![collection_id, now, source_post_id],
            )?;
            Ok(RootSettlement {
                root_item_id: collection_id,
                promoted: true,
                visible: true,
                replaced_root_item_id: Some(root_id),
                removed_root_tag_ids,
                group_order: Some(group_order),
            })
        }
        (Some(root_id), _) if root_kind(transaction, root_id)? == "collection" => {
            let group_order = source_media_order(transaction, source_post_id)?;
            set_group_cover(transaction, root_id, &group_order)?;
            Ok(RootSettlement {
                root_item_id: root_id,
                promoted: false,
                visible: true,
                replaced_root_item_id: None,
                removed_root_tag_ids: Vec::new(),
                group_order: Some(group_order),
            })
        }
        (Some(root_id), _) => Ok(RootSettlement {
            root_item_id: root_id,
            promoted: false,
            visible: true,
            replaced_root_item_id: None,
            removed_root_tag_ids: Vec::new(),
            group_order: None,
        }),
        (None, _) => Err(invalid("Source post has media without a visible root")),
    }
}

fn source_media_order(
    transaction: &Transaction<'_>,
    source_post_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    transaction
        .prepare_cached(
            "SELECT media_item_id
             FROM source_item
             WHERE source_post_id = ?1
               AND state = 'ingested'
               AND media_item_id IS NOT NULL
             ORDER BY position, source_item_id",
        )?
        .query_map([source_post_id], |row| row.get(0))?
        .collect()
}

fn set_group_cover(
    transaction: &Transaction<'_>,
    collection_id: i64,
    group_order: &[i64],
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE library_item
         SET cover_media_item_id = ?2
         WHERE item_id = ?1 AND cover_media_item_id IS NOT ?2",
        params![collection_id, group_order.first()],
    )?;
    Ok(())
}

fn provisional_source_collection(
    transaction: &Transaction<'_>,
    projections: &crate::projection_v2::ProjectionStore,
    source_post_id: i64,
) -> rusqlite::Result<Option<i64>> {
    let media_item_id = transaction
        .query_row(
            "SELECT media_item_id
             FROM source_item
             WHERE source_post_id = ?1
               AND state = 'ingested'
               AND media_item_id IS NOT NULL
             ORDER BY position, source_item_id
             LIMIT 1",
            [source_post_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(collection_id) = media_item_id.and_then(|id| projections.root_for_media(id)) else {
        return Ok(None);
    };
    transaction
        .query_row(
            "SELECT item.item_id
             FROM library_item item
             LEFT JOIN library_root root ON root.item_id = item.item_id
             WHERE item.item_id = ?1 AND item.kind = 'collection' AND root.item_id IS NULL",
            [collection_id],
            |row| row.get(0),
        )
        .optional()
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

fn enqueue_derivatives(
    transaction: &Transaction<'_>,
    media_item_id: i64,
    file_id: i64,
    input: &PreparedMediaInput,
    enqueue_ai: bool,
    now: &str,
) -> rusqlite::Result<()> {
    let capabilities = crate::media_capabilities::capabilities_for_stored_media(
        &input.mime_type,
        input.frame_count,
    );
    // Queue the user-visible derivative first. The worker claims in work_id
    // order, so this order is the ingestion priority contract rather than an
    // incidental alphabetical sort.
    let mut work = Vec::with_capacity(3);
    if capabilities.can_dominant_colors {
        work.push("dominant_colors");
    }
    if capabilities.can_perceptual_hash {
        // Color analysis and pHash both consume the same reduced thumbnail.
        // The dominant-colors worker writes both when available, avoiding a
        // second queue claim and thumbnail decode for normal image ingest.
        if !capabilities.can_dominant_colors {
            work.push("perceptual_hash");
        }
        if enqueue_ai {
            work.push("ai_tag");
        }
    }
    if !work.is_empty() {
        let work_json = serde_json::to_string(&work)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "INSERT INTO work_item (
                 media_item_id, file_id, work_type, priority, status, attempt_count,
                 available_at, created_at, updated_at
             )
             SELECT ?1, ?2, value,
                    CASE value
                        WHEN 'thumbnail' THEN 500
                        WHEN 'dominant_colors' THEN 400
                        WHEN 'perceptual_hash' THEN 300
                        WHEN 'ai_tag' THEN 200
                        WHEN 'blob_delete' THEN 100
                        ELSE 0
                    END,
                    'pending', 0, ?3, ?3, ?3
             FROM json_each(?4)
             WHERE 1
             ON CONFLICT DO NOTHING",
            params![media_item_id, file_id, now, work_json],
        )?;
    }
    Ok(())
}

fn enqueue_root_thumbnail(
    transaction: &Transaction<'_>,
    root_item_id: i64,
    now: &str,
) -> rusqlite::Result<()> {
    let target = transaction
        .query_row(
            "SELECT
                 CASE WHEN li.kind = 'collection' THEN li.cover_media_item_id ELSE lr.item_id END,
                 ma.file_id,
                 mf.mime_type,
                 mf.frame_count
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             JOIN media_asset ma ON ma.item_id = CASE
                 WHEN li.kind = 'collection' THEN li.cover_media_item_id ELSE lr.item_id END
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE lr.item_id = ?1",
            [root_item_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((media_item_id, file_id, mime_type, frame_count)) = target else {
        return Ok(());
    };
    if !crate::media_capabilities::capabilities_for_stored_media(&mime_type, frame_count)
        .can_thumbnail()
    {
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO work_item (
             media_item_id, file_id, work_type, priority, status, attempt_count,
             available_at, created_at, updated_at
         ) VALUES (?1, ?2, 'thumbnail', 500, 'pending', 0, ?3, ?3, ?3)
         ON CONFLICT DO NOTHING",
        params![media_item_id, file_id, now],
    )?;
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
        "aiTaggerOppaiOracleEnabled",
        "aiTaggerDanbooruTagQueryEnabled",
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
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use roaring::RoaringBitmap;

    use super::{
        merge_root_organization, PreparedMediaInput, SourcePostInput, StagedRootOrganization,
    };
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
            lifecycle: Lifecycle::Inbox,
            captured_at: None,
            source: Some(SourcePostInput {
                site_id: "example".to_string(),
                post_key: post.to_string(),
                item_key: item.to_string(),
                position,
                post_complete: true,
                force_collection: false,
                group_post: true,
                canonical_post_url: None,
                canonical_media_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
            }),
            target_folder_id: None,
            target_folder_ids: Vec::new(),
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
        let aggregate = app
            .projections()
            .numeric_aggregates(&RoaringBitmap::from_iter([second.root_item_id.0 as u32]));
        assert_eq!(aggregate.total_size_bytes.sum, 20);
        assert_eq!(aggregate.media_count.sum, 2);
        let second_thumbnail_jobs = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM work_item
                     WHERE media_item_id = ?1 AND work_type = 'thumbnail'",
                    [second.media_item_id.0],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(second_thumbnail_jobs, 0);
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
    fn ungrouped_source_post_creates_one_root_per_media_item() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let mut first_input = input("first", "post", "a", 0);
        first_input.source.as_mut().unwrap().group_post = false;
        first_input.source.as_mut().unwrap().post_complete = false;
        let first = app.ingest_prepared(&first_input).unwrap();

        let mut second_input = input("second", "post", "b", 1);
        second_input.source.as_mut().unwrap().group_post = false;
        let second = app.ingest_prepared(&second_input).unwrap();

        assert_eq!(first.media_item_id, first.root_item_id);
        assert_eq!(second.media_item_id, second.root_item_id);
        assert!(!first.promoted_to_collection);
        assert!(!second.promoted_to_collection);
        app.store()
            .read(|connection| {
                let roots: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))?;
                let collection_members: i64 =
                    connection.query_row("SELECT COUNT(*) FROM collection_member", [], |row| {
                        row.get(0)
                    })?;
                let post_root: Option<i64> = connection.query_row(
                    "SELECT root_item_id FROM source_post
                     WHERE site_id = 'example' AND post_key = 'post'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(roots, 2);
                assert_eq!(collection_members, 0);
                assert_eq!(post_root, None);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn source_collection_is_hidden_until_the_source_post_is_complete() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let mut first_input = input("first", "post", "a", 0);
        first_input.source.as_mut().unwrap().post_complete = false;
        first_input.name = Some("Cover name".to_string());
        first_input.notes = Some("Cover notes".to_string());
        first_input.rating = Some(5);
        first_input.source_urls = vec!["https://example.test/cover".to_string()];
        first_input.tags = vec!["creator:cover".to_string()];
        first_input.source.as_mut().unwrap().metadata_json =
            Some("[1, {\"immutable\": true}]".to_string());
        let first = app.ingest_prepared(&first_input).unwrap();

        assert_ne!(first.media_item_id, first.root_item_id);
        assert!(!first.promoted_to_collection);
        assert!(first.receipt.is_none());
        assert!(app.projections().inbox_bitmap().is_empty());
        let roots_after_first: i64 = app
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(roots_after_first, 0);
        app.store()
            .read(|connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM root_metadata", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    0
                );
                Ok(())
            })
            .unwrap();

        let mut second_input = input("second", "post", "b", 1);
        second_input.source.as_mut().unwrap().post_complete = true;
        second_input.notes = Some("Non-cover notes".to_string());
        second_input.rating = Some(1);
        second_input.source_urls = vec!["https://example.test/second".to_string()];
        second_input.tags = vec!["character:second".to_string()];
        let second = app.ingest_prepared(&second_input).unwrap();

        assert_eq!(second.root_item_id, first.root_item_id);
        assert!(second.promoted_to_collection);
        assert!(second.receipt.is_some());
        assert_eq!(app.projections().inbox_bitmap().len(), 1);
        assert!(app
            .projections()
            .inbox_bitmap()
            .contains(second.root_item_id.0 as u32));
        app.store()
            .read(|connection| {
                let (name, rating, notes, source_urls): (String, i64, String, String) = connection
                    .query_row(
                        "SELECT name, rating, notes, source_urls_json
                         FROM root_metadata WHERE root_item_id = ?1",
                        [second.root_item_id.0],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )?;
                assert_eq!(name, "Cover name");
                assert_eq!(rating, 5);
                assert_eq!(notes, "Cover notes");
                assert_eq!(
                    serde_json::from_str::<BTreeSet<String>>(&source_urls).unwrap(),
                    BTreeSet::from([
                        "https://example.test/cover".to_string(),
                        "https://example.test/second".to_string(),
                    ])
                );
                let metadata: String = connection.query_row(
                    "SELECT metadata_json FROM source_post
                     WHERE site_id = 'example' AND post_key = 'post'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(metadata, "[1, {\"immutable\": true}]");
                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections()
                .tag_ids_for_root(second.root_item_id.0)
                .len(),
            2
        );
    }

    #[test]
    fn repeated_source_item_is_idempotent_and_deleted_source_does_not_resurrect() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let mut original = input("hash", "post", "item", 0);
        original.name = Some("Accepted name".to_string());
        original.notes = Some("Original note".to_string());
        original.rating = Some(4);
        original.source_urls = vec!["https://example.test/first".to_string()];
        let first = app.ingest_prepared(&original).unwrap();

        let mut update = original.clone();
        update.name = Some("Incoming generated name".to_string());
        update.notes = Some("Additional note".to_string());
        update.rating = Some(1);
        update.source_urls = vec![
            "https://example.test/first".to_string(),
            "https://example.test/second".to_string(),
        ];
        update.tags.push("creator:leonardo".to_string());
        let repeated = app.ingest_prepared(&update).unwrap();
        assert!(repeated.reused_existing_item);
        assert!(repeated.receipt.is_some());
        assert_eq!(first.media_item_id, repeated.media_item_id);

        let repeated_again = app.ingest_prepared(&update).unwrap();
        assert!(repeated_again.receipt.is_none());
        let creator_tag_id = app
            .store()
            .read(|connection| {
                let (name, notes, rating, sources): (String, String, i64, String) = connection
                    .query_row(
                        "SELECT name, notes, rating, source_urls_json
                         FROM root_metadata WHERE root_item_id = ?1",
                        [first.root_item_id.0],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )?;
                assert_eq!(name, "Accepted name");
                assert_eq!(notes, "Original note\n\nAdditional note");
                assert_eq!(rating, 4);
                assert_eq!(
                    serde_json::from_str::<Vec<String>>(&sources).unwrap(),
                    vec![
                        "https://example.test/first".to_string(),
                        "https://example.test/second".to_string()
                    ]
                );
                connection.query_row(
                    "SELECT tag_id FROM tag
                     WHERE namespace = 'creator' AND subtag = 'leonardo'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert!(app
            .projections()
            .direct_tag_bitmap(creator_tag_id)
            .contains(first.root_item_id.0 as u32));

        app.delete_items(&ItemTarget::Explicit {
            item_ids: vec![first.root_item_id],
        })
        .unwrap();
        let error = app.ingest_prepared(&original).unwrap_err();
        assert!(error.contains("cannot be resurrected"));
    }

    #[test]
    fn tag_ingest_deduplicates_inputs_and_reports_only_changed_assignments() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let mut media = input("tag-hash", "tag-post", "tag-item", 0);
        media.tags = vec![
            "general:first".to_string(),
            "general:first".to_string(),
            "general:second".to_string(),
        ];
        let result = app.ingest_prepared(&media).unwrap();

        let tag_ids: Vec<i64> = app
            .store()
            .read(|connection| {
                connection
                    .prepare(
                        "SELECT tag_id FROM tag
                         WHERE namespace = 'general' AND subtag IN ('first', 'second')
                         ORDER BY subtag",
                    )?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect()
            })
            .unwrap();
        assert_eq!(tag_ids.len(), 2);
        for tag_id in &tag_ids {
            assert!(app
                .projections()
                .direct_tag_bitmap(*tag_id)
                .contains(result.root_item_id.0 as u32));
        }

        app.store()
            .transaction(|transaction| {
                let unchanged = merge_root_organization(
                    transaction,
                    app.projections(),
                    result.root_item_id.0,
                    &StagedRootOrganization::from_input(&media, true),
                    false,
                    "now",
                )?;
                assert!(unchanged.tag_ids.is_empty());

                let changed = merge_root_organization(
                    transaction,
                    app.projections(),
                    result.root_item_id.0,
                    &StagedRootOrganization::from_input(&media, true),
                    false,
                    "later",
                )?;
                assert!(changed.tag_ids.is_empty());
                Ok(())
            })
            .unwrap();

        assert!(app
            .projections()
            .direct_tag_bitmap(tag_ids[0])
            .contains(result.root_item_id.0 as u32));
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
                let legacy_folders: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE item_id = ?1",
                    [first.root_item_id.0],
                    |row| row.get(0),
                )?;
                assert_eq!(media, 1);
                assert_eq!(legacy_folders, 0);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            app.projections().folder_ids_for_root(first.root_item_id.0),
            vec![first_folder.0, second_folder.0]
        );
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

        assert_eq!(
            app.projections().folder_ids_for_root(second.root_item_id.0),
            vec![folder.0]
        );
        assert!(app
            .projections()
            .folder_ids_for_root(first.media_item_id.0)
            .is_empty());
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
                    connection.prepare("SELECT work_type FROM work_item ORDER BY work_id")?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(work_types, ["thumbnail", "dominant_colors", "ai_tag"]);
    }

    #[test]
    fn audio_ingest_enqueues_thumbnail_without_palette() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let mut audio = input("audio-hash", "audio-post", "track", 0);
        audio.mime_type = "audio/mpeg".to_string();
        audio.pixel_width = None;
        audio.pixel_height = None;
        audio.frame_count = None;
        audio.duration_ms = Some(12_000);
        audio.has_audio = true;

        app.ingest_prepared(&audio).unwrap();

        let work_types: Vec<String> = app
            .store()
            .read(|connection| {
                let mut statement =
                    connection.prepare("SELECT work_type FROM work_item ORDER BY work_id")?;
                let rows = statement
                    .query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(work_types, ["thumbnail"]);
    }

    #[test]
    fn forced_source_container_with_one_item_is_still_a_collection() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let mut archived = input("archive-hash", "zip:one", "only", 0);
        archived.source.as_mut().unwrap().force_collection = true;

        let result = app.ingest_prepared(&archived).unwrap();

        assert_ne!(result.media_item_id, result.root_item_id);
        let (kind, legacy_members): (String, i64) = app
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT li.kind,
                            (SELECT COUNT(*) FROM collection_member cm
                             WHERE cm.collection_id = li.item_id)
                     FROM library_item li WHERE li.item_id = ?1",
                    [result.root_item_id.0],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(kind, "collection");
        assert_eq!(legacy_members, 0);
        assert_eq!(
            app.projections().group_order(result.root_item_id.0),
            Some(vec![result.media_item_id.0])
        );
    }
}
