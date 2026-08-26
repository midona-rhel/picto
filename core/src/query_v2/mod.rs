//! Canonical read path for library item scopes.
//!
//! The query starts from `library_root` and projects collection members into
//! their owning collection. Every scope, filter, count, and page therefore
//! uses the same root set.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{
    FileHash, FilterMatchMode, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemTarget,
    Lifecycle,
};
use crate::store::Store;

const DEFAULT_PAGE_LIMIT: i64 = 100;
const MAX_PAGE_LIMIT: i64 = 500;
const COLOR_FILTER_DELTA_E: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemPageRequest {
    #[ts(type = "number")]
    pub offset: i64,
    #[ts(type = "number")]
    pub limit: i64,
}

impl Default for ItemPageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

impl ItemPageRequest {
    pub fn new(offset: i64, limit: i64) -> Self {
        Self { offset, limit }
    }

    fn normalized(self) -> Self {
        Self {
            offset: self.offset.max(0),
            limit: self.limit.clamp(1, MAX_PAGE_LIMIT),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemSummary {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub lifecycle: Lifecycle,
    pub name: Option<String>,
    pub display_file_hash: FileHash,
    pub display_mime_type: String,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub frame_count: Option<i64>,
    pub dominant_color_hex: Option<String>,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
    #[ts(type = "number")]
    pub media_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemPage {
    pub items: Vec<ItemSummary>,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number | null")]
    pub visible_item_count: Option<i64>,
    #[ts(type = "number | null")]
    pub visible_media_count: Option<i64>,
    #[ts(type = "number | null")]
    pub total_size_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MediaDetails {
    pub media_item_id: ItemId,
    pub file_hash: FileHash,
    pub mime_type: String,
    pub dominant_color_hex: Option<String>,
    pub dominant_colors: Vec<String>,
    #[ts(type = "number")]
    pub size_bytes: i64,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub name: Option<String>,
    pub notes: Option<String>,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
    pub source_urls: Vec<String>,
    pub captured_at: Option<String>,
    pub imported_at: String,
    #[ts(type = "number")]
    pub position: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemDetails {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub lifecycle: Lifecycle,
    pub label: Option<String>,
    pub cover_media_item_id: Option<ItemId>,
    #[ts(type = "number[]")]
    pub folder_ids: Vec<i64>,
    pub media: Vec<MediaDetails>,
    pub aggregate_tags: Vec<String>,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionTagCount {
    pub tag: String,
    #[ts(type = "number")]
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionFolderInfo {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionCollectionCandidate {
    pub collection_id: ItemId,
    pub label: Option<String>,
    #[ts(type = "number")]
    pub member_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionRatingStats {
    #[ts(type = "number | null")]
    pub min: Option<i64>,
    #[ts(type = "number | null")]
    pub max: Option<i64>,
    #[ts(type = "number | null")]
    pub shared: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionSummaryStats {
    #[ts(type = "number | null")]
    pub total_size_bytes: Option<i64>,
    #[ts(type = "Record<string, number>")]
    pub mime_counts: BTreeMap<String, i64>,
    pub rating_stats: SelectionRatingStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionSummary {
    #[ts(type = "number")]
    pub total_count: i64,
    #[ts(type = "number")]
    pub selected_count: i64,
    pub sample_hashes: Vec<FileHash>,
    pub shared_tags: Vec<SelectionTagCount>,
    pub top_tags: Vec<SelectionTagCount>,
    pub shared_folders: Vec<SelectionFolderInfo>,
    pub selected_collection_candidates: Vec<SelectionCollectionCandidate>,
    pub shared_notes: Option<String>,
    #[ts(type = "number")]
    pub notes_present_count: i64,
    pub shared_source_urls: Option<Vec<String>>,
    #[ts(type = "number")]
    pub source_urls_present_count: i64,
    pub stats: SelectionSummaryStats,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ScopeCount {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SidebarCounts {
    #[ts(type = "number")]
    pub all: i64,
    #[ts(type = "number")]
    pub inbox: i64,
    #[ts(type = "number")]
    pub trash: i64,
    #[ts(type = "number")]
    pub recently_viewed: i64,
    #[ts(type = "number")]
    pub untagged: i64,
    #[ts(type = "number")]
    pub uncategorized: i64,
    #[ts(type = "number")]
    pub duplicates: i64,
    pub folders: Vec<ScopeCount>,
    pub smart_folders: Vec<ScopeCount>,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct LibraryStatistics {
    #[ts(type = "number")]
    pub active_items: i64,
    #[ts(type = "number")]
    pub inbox_items: i64,
    #[ts(type = "number")]
    pub trash_items: i64,
    #[ts(type = "number")]
    pub standalone_items: i64,
    #[ts(type = "number")]
    pub collections: i64,
    #[ts(type = "number")]
    pub media_assets: i64,
    #[ts(type = "number")]
    pub image_assets: i64,
    #[ts(type = "number")]
    pub video_assets: i64,
    #[ts(type = "number")]
    pub audio_assets: i64,
    #[ts(type = "number")]
    pub other_assets: i64,
    #[ts(type = "number")]
    pub physical_files: i64,
    #[ts(type = "number")]
    pub original_bytes: i64,
    #[ts(type = "number")]
    pub tags: i64,
    #[ts(type = "number")]
    pub folders: i64,
    #[ts(type = "number")]
    pub smart_folders: i64,
    #[ts(type = "number")]
    pub subscriptions: i64,
    #[ts(type = "number")]
    pub revision: u64,
}

/// Resolve one canonical page from the replacement store.
pub fn query(
    store: &Store,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> Result<ItemPage, String> {
    let page = page.normalized();
    store.read_snapshot(|connection| resolve_connection(connection, item_query, page))
}

pub fn details(store: &Store, item_id: ItemId) -> Result<ItemDetails, String> {
    store.read_snapshot(|connection| details_connection(connection, item_id.0))
}

pub fn selection_summary(store: &Store, target: &ItemTarget) -> Result<SelectionSummary, String> {
    store.read(|connection| {
        let selection = target_selection_sql(connection, target)?;
        let parameters = selection.parameters();
        let mut mime_counts = BTreeMap::new();
        let mut tag_media_counts = BTreeMap::<String, i64>::new();
        let mut folder_root_counts = BTreeMap::<i64, (String, i64)>::new();
        let mut selected_collection_candidates = Vec::new();
        let stats_sql = format!(
            "{}
             SELECT
                 (SELECT COUNT(*) FROM selected_roots),
                 COUNT(sm.media_item_id),
                 COALESCE(SUM(mf.size_bytes), 0),
                 MIN(ma.rating),
                 MAX(ma.rating),
                 COUNT(ma.rating),
                 COUNT(NULLIF(TRIM(ma.notes), '')),
                 MIN(COALESCE(NULLIF(TRIM(ma.notes), ''), '')),
                 MAX(COALESCE(NULLIF(TRIM(ma.notes), ''), '')),
                 COALESCE(SUM(
                     CASE WHEN json_array_length(COALESCE(ma.source_urls_json, '[]')) > 0
                          THEN 1 ELSE 0 END
                 ), 0),
                 MIN(COALESCE(ma.source_urls_json, '[]')),
                 MAX(COALESCE(ma.source_urls_json, '[]'))
             FROM selected_media sm
             JOIN media_asset ma ON ma.item_id = sm.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id",
            selection.with_clause
        );
        let (
            selected_count,
            selected_media_count,
            total_size_bytes,
            min_rating,
            max_rating,
            rated_count,
            notes_present_count,
            min_notes,
            max_notes,
            source_urls_present_count,
            min_source_urls_json,
            max_source_urls_json,
        ): (
            i64,
            i64,
            i64,
            Option<i64>,
            Option<i64>,
            i64,
            i64,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
            Option<String>,
        ) = connection.query_row(&stats_sql, parameters.as_slice(), |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
            ))
        })?;
        let rating_stats = SelectionRatingStats {
            min: min_rating,
            max: max_rating,
            shared: (selected_media_count > 0
                && rated_count == selected_media_count
                && min_rating == max_rating)
                .then_some(min_rating)
                .flatten(),
        };
        let shared_notes = (selected_media_count > 0 && min_notes == max_notes)
            .then_some(min_notes)
            .flatten()
            .filter(|notes| !notes.is_empty());
        let shared_source_urls = (selected_media_count > 0
            && min_source_urls_json == max_source_urls_json)
            .then_some(min_source_urls_json)
            .flatten()
            .map(|json| serde_json::from_str::<Vec<String>>(&json))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;

        let mime_sql = format!(
            "{}
             SELECT mf.mime_type, COUNT(*)
             FROM selected_media sm
             JOIN media_asset ma ON ma.item_id = sm.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             GROUP BY mf.mime_type",
            selection.with_clause
        );
        for row in connection
            .prepare(&mime_sql)?
            .query_map(parameters.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
        {
            let (mime_type, count) = row?;
            mime_counts.insert(mime_type, count);
        }

        let tags_sql = format!(
            "{}
             SELECT
                 CASE WHEN t.namespace IN ('', 'general') THEN t.subtag
                      ELSE t.namespace || ':' || t.subtag END,
                 COUNT(DISTINCT sm.media_item_id)
             FROM selected_media sm
             JOIN media_tag mt ON mt.media_item_id = sm.media_item_id
             JOIN tag t ON t.tag_id = mt.tag_id
             GROUP BY t.namespace, t.subtag",
            selection.with_clause
        );
        for row in connection
            .prepare(&tags_sql)?
            .query_map(parameters.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
        {
            let (tag, count) = row?;
            tag_media_counts.insert(tag, count);
        }

        let folders_sql = format!(
            "{}
             SELECT f.folder_id, f.name, COUNT(*)
             FROM selected_roots sr
             JOIN folder_item fi ON fi.item_id = sr.item_id
             JOIN folder f ON f.folder_id = fi.folder_id
             GROUP BY f.folder_id, f.name",
            selection.with_clause
        );
        for row in connection
            .prepare(&folders_sql)?
            .query_map(parameters.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
        {
            let (folder_id, name, count) = row?;
            folder_root_counts.insert(folder_id, (name, count));
        }

        let candidates_sql = format!(
            "{}
             SELECT sr.item_id, li.label, COUNT(cm.media_item_id)
             FROM selected_roots sr
             JOIN library_item li ON li.item_id = sr.item_id
             LEFT JOIN collection_member cm ON cm.collection_id = sr.item_id
             WHERE li.kind = 'collection'
             GROUP BY sr.item_id, li.label
             ORDER BY sr.item_id",
            selection.with_clause
        );
        for row in connection
            .prepare(&candidates_sql)?
            .query_map(parameters.as_slice(), |row| {
                Ok(SelectionCollectionCandidate {
                    collection_id: ItemId(row.get(0)?),
                    label: row.get(1)?,
                    member_count: row.get(2)?,
                })
            })?
        {
            selected_collection_candidates.push(row?);
        }

        let sample_item_ids = match target {
            ItemTarget::Explicit { item_ids } => item_ids
                .iter()
                .rev()
                .take(5)
                .rev()
                .map(|item_id| item_id.0)
                .collect(),
            ItemTarget::Query { .. } => {
                let samples_sql = format!(
                    "{}
                     SELECT recent.item_id
                     FROM (
                         SELECT sr.item_id, MAX(ma.imported_at) AS imported_at
                         FROM selected_roots sr
                         LEFT JOIN selected_media sm ON sm.root_item_id = sr.item_id
                         LEFT JOIN media_asset ma ON ma.item_id = sm.media_item_id
                         GROUP BY sr.item_id
                         ORDER BY imported_at DESC, sr.item_id DESC
                         LIMIT 5
                     ) recent
                     ORDER BY recent.imported_at ASC, recent.item_id ASC",
                    selection.with_clause
                );
                connection
                    .prepare(&samples_sql)?
                    .query_map(parameters.as_slice(), |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        let mut sample_hashes = Vec::new();
        for item_id in sample_item_ids {
            if let Some(hash) = selection_display_hash(connection, item_id)? {
                sample_hashes.push(FileHash(hash));
            }
        }

        let shared_tags = tag_media_counts
            .iter()
            .filter(|(_, count)| selected_media_count > 0 && **count == selected_media_count)
            .map(|(tag, count)| SelectionTagCount {
                tag: tag.clone(),
                count: *count,
            })
            .collect::<Vec<_>>();
        let mut top_tags = tag_media_counts
            .iter()
            .map(|(tag, count)| SelectionTagCount {
                tag: tag.clone(),
                count: *count,
            })
            .collect::<Vec<_>>();
        top_tags.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.tag.cmp(&right.tag))
        });
        top_tags.truncate(20);

        let shared_folders = folder_root_counts
            .iter()
            .filter(|(_, (_, count))| *count == selected_count)
            .map(|(folder_id, (name, _))| SelectionFolderInfo {
                folder_id: *folder_id,
                name: name.clone(),
            })
            .collect::<Vec<_>>();

        Ok(SelectionSummary {
            total_count: selected_count,
            selected_count,
            sample_hashes,
            shared_tags,
            top_tags,
            shared_folders,
            selected_collection_candidates,
            shared_notes,
            notes_present_count,
            shared_source_urls,
            source_urls_present_count,
            stats: SelectionSummaryStats {
                total_size_bytes: Some(total_size_bytes),
                mime_counts,
                rating_stats,
            },
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

struct TargetSelectionSql {
    with_clause: String,
    arguments: Vec<Box<dyn ToSql>>,
}

impl TargetSelectionSql {
    fn parameters(&self) -> Vec<&dyn ToSql> {
        self.arguments.iter().map(|value| value.as_ref()).collect()
    }
}

fn target_selection_sql(
    connection: &Connection,
    target: &ItemTarget,
) -> rusqlite::Result<TargetSelectionSql> {
    match target {
        ItemTarget::Explicit { item_ids } => {
            let unique_ids = item_ids
                .iter()
                .map(|item_id| item_id.0)
                .collect::<std::collections::HashSet<_>>();
            if unique_ids.is_empty() || unique_ids.len() != item_ids.len() {
                return Err(invalid_target(
                    "An explicit target must contain unique library root IDs",
                ));
            }
            let encoded = serde_json::to_string(
                &item_ids.iter().map(|item_id| item_id.0).collect::<Vec<_>>(),
            )
            .map_err(|error| invalid_target(format!("Could not encode item target: {error}")))?;
            let valid_count: i64 = connection.query_row(
                "SELECT COUNT(*)
                 FROM json_each(?1) target
                 JOIN library_root lr ON lr.item_id = CAST(target.value AS INTEGER)",
                [&encoded],
                |row| row.get(0),
            )?;
            if valid_count != item_ids.len() as i64 {
                return Err(invalid_target("A targeted item is not a library root"));
            }
            Ok(TargetSelectionSql {
                with_clause: "WITH
                    selected_roots(item_id) AS (
                        SELECT lr.item_id
                        FROM json_each(?1) target
                        JOIN library_root lr ON lr.item_id = CAST(target.value AS INTEGER)
                    ),
                    selected_media(root_item_id, media_item_id) AS (
                        SELECT sr.item_id, sr.item_id
                        FROM selected_roots sr
                        JOIN media_asset ma ON ma.item_id = sr.item_id
                        UNION ALL
                        SELECT sr.item_id, cm.media_item_id
                        FROM selected_roots sr
                        JOIN collection_member cm ON cm.collection_id = sr.item_id
                    )"
                .to_string(),
                arguments: vec![Box::new(encoded)],
            })
        }
        ItemTarget::Query {
            query,
            excluded_item_ids,
        } => {
            let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(match &query.scope {
                ItemScope::Folder { folder_id } => *folder_id,
                _ => -1,
            })];
            let mut predicates = vec![scope_predicate(connection, &query.scope, &mut arguments)?];
            apply_filters(&query.filters, &mut predicates, &mut arguments);
            if !excluded_item_ids.is_empty() {
                let encoded = serde_json::to_string(
                    &excluded_item_ids
                        .iter()
                        .map(|item_id| item_id.0)
                        .collect::<Vec<_>>(),
                )
                .map_err(|error| {
                    invalid_target(format!("Could not encode excluded item IDs: {error}"))
                })?;
                let index = push_argument(&mut arguments, encoded);
                predicates.push(format!(
                    "NOT EXISTS (
                        SELECT 1 FROM json_each(?{index}) excluded
                        WHERE CAST(excluded.value AS INTEGER) = ri.item_id
                    )"
                ));
            }
            Ok(TargetSelectionSql {
                with_clause: format!(
                    "WITH
                     root_items AS NOT MATERIALIZED (
                         SELECT lr.item_id, lr.lifecycle, li.kind, li.label,
                                li.created_at, li.updated_at,
                                fi.position_rank AS folder_position, mv.viewed_at
                         FROM library_root lr
                         JOIN library_item li ON li.item_id = lr.item_id
                         LEFT JOIN folder_item fi
                           ON fi.item_id = lr.item_id AND fi.folder_id = ?1
                         LEFT JOIN media_view mv ON mv.item_id = lr.item_id
                         WHERE li.kind = 'collection'
                            OR NOT EXISTS (
                                SELECT 1 FROM collection_member member_root
                                WHERE member_root.media_item_id = lr.item_id
                            )
                     ),
                     root_media AS NOT MATERIALIZED (
                         SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
                         FROM root_items ri WHERE ri.kind = 'media'
                         UNION ALL
                         SELECT ri.item_id, cm.media_item_id
                         FROM root_items ri
                         JOIN collection_member cm ON cm.collection_id = ri.item_id
                         WHERE ri.kind = 'collection'
                     ),
                     selected_roots(item_id) AS (
                         SELECT ri.item_id FROM root_items ri WHERE {}
                     ),
                     selected_media(root_item_id, media_item_id) AS (
                         SELECT rm.root_item_id, rm.media_item_id
                         FROM root_media rm
                         JOIN selected_roots sr ON sr.item_id = rm.root_item_id
                     )",
                    predicates.join(" AND ")
                ),
                arguments,
            })
        }
    }
}

fn selection_display_hash(
    connection: &Connection,
    item_id: i64,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT mf.file_hash
             FROM library_item li
             LEFT JOIN media_asset direct ON direct.item_id = li.item_id
             LEFT JOIN media_asset first_member ON first_member.item_id = (
                 SELECT cm.media_item_id FROM collection_member cm
                 WHERE cm.collection_id = li.item_id
                 ORDER BY cm.position_rank, cm.media_item_id LIMIT 1
             )
             LEFT JOIN media_file mf ON mf.file_id = COALESCE(
                 direct.file_id, first_member.file_id
             )
             WHERE li.item_id = ?1",
            [item_id],
            |row| row.get(0),
        )
        .optional()
}

pub fn sidebar_counts(store: &Store) -> Result<SidebarCounts, String> {
    store.read(|connection| {
        let mut result = connection.query_row(
            "WITH visible_roots(item_id, lifecycle) AS (
                 SELECT lr.item_id, lr.lifecycle FROM library_root lr
                 WHERE NOT EXISTS (
                     SELECT 1 FROM collection_member cm WHERE cm.media_item_id = lr.item_id
                 )
             ),
             root_media(root_item_id, media_item_id) AS (
                 SELECT vr.item_id, vr.item_id
                 FROM visible_roots vr JOIN media_asset ma ON ma.item_id = vr.item_id
                 UNION ALL
                 SELECT cm.collection_id, cm.media_item_id FROM collection_member cm
             )
             SELECT
                 COUNT(*) FILTER (WHERE vr.lifecycle = 'active'),
                 COUNT(*) FILTER (WHERE vr.lifecycle = 'inbox'),
                 COUNT(*) FILTER (WHERE vr.lifecycle = 'trash'),
                 COUNT(*) FILTER (
                     WHERE vr.lifecycle = 'active' AND mv.item_id IS NOT NULL
                 ),
                 COUNT(*) FILTER (
                     WHERE vr.lifecycle = 'active' AND NOT EXISTS (
                         SELECT 1 FROM root_media rm
                         JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                         WHERE rm.root_item_id = vr.item_id
                     )
                 ),
                 COUNT(*) FILTER (
                     WHERE vr.lifecycle = 'active' AND NOT EXISTS (
                         SELECT 1 FROM folder_item fi WHERE fi.item_id = vr.item_id
                     )
                 )
             FROM visible_roots vr
             LEFT JOIN media_view mv ON mv.item_id = vr.item_id",
            [],
            |row| {
                Ok(SidebarCounts {
                    all: row.get(0)?,
                    inbox: row.get(1)?,
                    trash: row.get(2)?,
                    recently_viewed: row.get(3)?,
                    untagged: row.get(4)?,
                    uncategorized: row.get(5)?,
                    ..SidebarCounts::default()
                })
            },
        )?;
        result.duplicates = crate::duplicates_v2::count_candidates(connection)?;
        result.folders = connection
            .prepare(
                "SELECT f.folder_id, COUNT(lr.item_id)
                 FROM folder f
                 LEFT JOIN folder_item fi ON fi.folder_id = f.folder_id
                 LEFT JOIN library_root lr ON lr.item_id = fi.item_id
                   AND lr.lifecycle = 'active'
                   AND NOT EXISTS (
                       SELECT 1 FROM collection_member cm WHERE cm.media_item_id = lr.item_id
                   )
                 GROUP BY f.folder_id ORDER BY f.folder_id",
            )?
            .query_map([], |row| {
                Ok(ScopeCount {
                    id: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let smart_ids = connection
            .prepare("SELECT smart_folder_id FROM smart_folder ORDER BY smart_folder_id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for smart_folder_id in smart_ids {
            result.smart_folders.push(ScopeCount {
                id: smart_folder_id,
                count: crate::smart_v2::count_smart_folder(connection, smart_folder_id)?,
            });
        }
        result.revision = crate::store::schema::revision(connection)?;
        Ok(result)
    })
}

pub fn library_statistics(store: &Store) -> Result<LibraryStatistics, String> {
    store.read(|connection| {
        connection.query_row(
            "WITH visible_roots AS (
                 SELECT lr.item_id, lr.lifecycle, li.kind
                 FROM library_root lr
                 JOIN library_item li ON li.item_id = lr.item_id
                 WHERE NOT EXISTS (
                     SELECT 1 FROM collection_member cm WHERE cm.media_item_id = lr.item_id
                 )
             )
             SELECT
                 COUNT(*) FILTER (WHERE lifecycle = 'active'),
                 COUNT(*) FILTER (WHERE lifecycle = 'inbox'),
                 COUNT(*) FILTER (WHERE lifecycle = 'trash'),
                 COUNT(*) FILTER (WHERE kind = 'media'),
                 COUNT(*) FILTER (WHERE kind = 'collection'),
                 (SELECT COUNT(*) FROM media_asset),
                 (SELECT COUNT(*) FROM media_asset ma JOIN media_file mf ON mf.file_id = ma.file_id WHERE mf.mime_type LIKE 'image/%'),
                 (SELECT COUNT(*) FROM media_asset ma JOIN media_file mf ON mf.file_id = ma.file_id WHERE mf.mime_type LIKE 'video/%'),
                 (SELECT COUNT(*) FROM media_asset ma JOIN media_file mf ON mf.file_id = ma.file_id WHERE mf.mime_type LIKE 'audio/%'),
                 (SELECT COUNT(*) FROM media_asset ma JOIN media_file mf ON mf.file_id = ma.file_id WHERE mf.mime_type NOT LIKE 'image/%' AND mf.mime_type NOT LIKE 'video/%' AND mf.mime_type NOT LIKE 'audio/%'),
                 (SELECT COUNT(*) FROM media_file),
                 (SELECT COALESCE(SUM(size_bytes), 0) FROM media_file),
                 (SELECT COUNT(*) FROM tag),
                 (SELECT COUNT(*) FROM folder),
                 (SELECT COUNT(*) FROM smart_folder),
                 (SELECT COUNT(*) FROM subscription),
                 (SELECT revision FROM library_meta WHERE singleton = 1)
             FROM visible_roots",
            [],
            |row| {
                Ok(LibraryStatistics {
                    active_items: row.get(0)?,
                    inbox_items: row.get(1)?,
                    trash_items: row.get(2)?,
                    standalone_items: row.get(3)?,
                    collections: row.get(4)?,
                    media_assets: row.get(5)?,
                    image_assets: row.get(6)?,
                    video_assets: row.get(7)?,
                    audio_assets: row.get(8)?,
                    other_assets: row.get(9)?,
                    physical_files: row.get(10)?,
                    original_bytes: row.get(11)?,
                    tags: row.get(12)?,
                    folders: row.get(13)?,
                    smart_folders: row.get(14)?,
                    subscriptions: row.get(15)?,
                    revision: row.get(16)?,
                })
            },
        )
    })
}

fn details_connection(connection: &Connection, item_id: i64) -> rusqlite::Result<ItemDetails> {
    let (kind, lifecycle, label, cover_media_item_id): (
        String,
        String,
        Option<String>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT li.kind, lr.lifecycle, li.label,
                    CASE WHEN li.kind = 'collection' THEN (
                        SELECT cm.media_item_id FROM collection_member cm
                        WHERE cm.collection_id = li.item_id
                        ORDER BY cm.position_rank, cm.media_item_id LIMIT 1
                    ) END
             FROM library_root lr JOIN library_item li ON li.item_id = lr.item_id
             WHERE lr.item_id = ?1",
            [item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| invalid_target(format!("Item {item_id} is not a library root")))?;
    let kind = parse_kind(&kind)?;
    let lifecycle = parse_lifecycle(&lifecycle)?;
    let folder_ids = connection
        .prepare("SELECT folder_id FROM folder_item WHERE item_id = ?1 ORDER BY folder_id")?
        .query_map([item_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut media = connection
        .prepare(
            "WITH root_media(media_item_id, position) AS (
                 SELECT ?1, 0 WHERE ?2 = 'media'
                 UNION ALL
                 SELECT media_item_id, position_rank FROM collection_member
                 WHERE collection_id = ?1 AND ?2 = 'collection'
             )
             SELECT ma.item_id, mf.file_hash, mf.mime_type, mf.dominant_color_hex,
                    mf.dominant_palette_blob, mf.size_bytes, mf.pixel_width, mf.pixel_height,
                    mf.duration_ms, mf.frame_count, mf.has_audio, ma.name, ma.notes, ma.rating,
                    ma.source_urls_json, ma.captured_at, ma.imported_at, rm.position
             FROM root_media rm
             JOIN media_asset ma ON ma.item_id = rm.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             ORDER BY rm.position, ma.item_id",
        )?
        .query_map(params![item_id, kind_string(kind)], |row| {
            let mime_type: String = row.get(2)?;
            let frame_count: Option<i64> = row.get(9)?;
            let supports_colors =
                crate::media_capabilities::capabilities_for_stored_media(&mime_type, frame_count)
                    .can_dominant_colors;
            let dominant_color_hex: Option<String> =
                supports_colors.then(|| row.get(3)).transpose()?.flatten();
            let palette_blob: Option<Vec<u8>> =
                supports_colors.then(|| row.get(4)).transpose()?.flatten();
            let mut dominant_colors: Vec<String> = palette_blob
                .as_deref()
                .and_then(|blob| {
                    crate::media_processing::colors::deserialize_dominant_palette_blob(blob).ok()
                })
                .map(|palette| palette.into_iter().map(|color| color.hex).collect())
                .unwrap_or_default();
            if dominant_colors.is_empty() {
                dominant_colors.extend(dominant_color_hex.iter().cloned());
            }
            Ok(MediaDetails {
                media_item_id: ItemId(row.get(0)?),
                file_hash: FileHash(row.get(1)?),
                mime_type,
                dominant_color_hex,
                dominant_colors,
                size_bytes: row.get(5)?,
                pixel_width: row.get(6)?,
                pixel_height: row.get(7)?,
                duration_ms: row.get(8)?,
                frame_count,
                has_audio: row.get(10)?,
                name: row.get(11)?,
                notes: row.get(12)?,
                rating: row.get(13)?,
                source_urls: {
                    let source_urls_json: Option<String> = row.get(14)?;
                    source_urls_json
                        .as_deref()
                        .and_then(|json| serde_json::from_str(json).ok())
                        .unwrap_or_default()
                },
                captured_at: row.get(15)?,
                imported_at: row.get(16)?,
                position: row.get(17)?,
                tags: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let media_indexes = media
        .iter()
        .enumerate()
        .map(|(index, media)| (media.media_item_id.0, index))
        .collect::<BTreeMap<_, _>>();
    let mut aggregate_tags = BTreeSet::new();
    if !media.is_empty() {
        let placeholders = std::iter::repeat_n("?", media.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT mt.media_item_id, t.namespace, t.subtag
             FROM media_tag mt JOIN tag t ON t.tag_id = mt.tag_id
             WHERE mt.media_item_id IN ({placeholders})
             ORDER BY mt.media_item_id, t.namespace, t.subtag"
        );
        let ids = media.iter().map(|item| item.media_item_id.0);
        let rows = connection
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(ids), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (media_item_id, namespace, subtag) in rows {
            let name = if namespace == "general" {
                subtag
            } else {
                format!("{namespace}:{subtag}")
            };
            aggregate_tags.insert(name.clone());
            if let Some(index) = media_indexes.get(&media_item_id) {
                media[*index].tags.push(name);
            }
        }
    }
    Ok(ItemDetails {
        item_id: ItemId(item_id),
        kind,
        lifecycle,
        label,
        cover_media_item_id: cover_media_item_id.map(ItemId),
        folder_ids,
        media,
        aggregate_tags: aggregate_tags.into_iter().collect(),
        revision: crate::store::schema::revision(connection)?,
    })
}

fn parse_kind(value: &str) -> rusqlite::Result<ItemKind> {
    match value {
        "media" => Ok(ItemKind::Media),
        "collection" => Ok(ItemKind::Collection),
        _ => Err(invalid_target(format!("Unknown item kind: {value}"))),
    }
}

fn parse_lifecycle(value: &str) -> rusqlite::Result<Lifecycle> {
    match value {
        "inbox" => Ok(Lifecycle::Inbox),
        "active" => Ok(Lifecycle::Active),
        "trash" => Ok(Lifecycle::Trash),
        _ => Err(invalid_target(format!("Unknown lifecycle: {value}"))),
    }
}

fn kind_string(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Media => "media",
        ItemKind::Collection => "collection",
    }
}

/// Resolve a mutation target inside the caller's transaction. Query targets
/// use the same scope and filter compiler as grid pages and counts.
pub(crate) fn resolve_target_ids(
    connection: &Connection,
    target: &ItemTarget,
) -> rusqlite::Result<Vec<i64>> {
    match target {
        ItemTarget::Explicit { item_ids } => {
            let unique_ids = item_ids
                .iter()
                .map(|item_id| item_id.0)
                .collect::<std::collections::HashSet<_>>();
            if unique_ids.is_empty() || unique_ids.len() != item_ids.len() {
                return Err(invalid_target(
                    "An explicit target must contain unique library root IDs",
                ));
            }
            let encoded = serde_json::to_string(
                &item_ids.iter().map(|item_id| item_id.0).collect::<Vec<_>>(),
            )
            .map_err(|error| invalid_target(format!("Could not encode item target: {error}")))?;
            let mut statement = connection.prepare(
                "SELECT lr.item_id
                 FROM json_each(?1) target
                 JOIN library_root lr ON lr.item_id = CAST(target.value AS INTEGER)
                 ORDER BY CAST(target.key AS INTEGER)",
            )?;
            let resolved = statement
                .query_map([encoded], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if resolved.len() != unique_ids.len() {
                return Err(invalid_target("A targeted item is not a library root"));
            }
            Ok(resolved)
        }
        ItemTarget::Query {
            query,
            excluded_item_ids,
        } => {
            let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(match &query.scope {
                ItemScope::Folder { folder_id } => *folder_id,
                _ => -1,
            })];
            let mut predicates = vec![scope_predicate(connection, &query.scope, &mut arguments)?];
            apply_filters(&query.filters, &mut predicates, &mut arguments);
            if !excluded_item_ids.is_empty() {
                let excluded = excluded_item_ids
                    .iter()
                    .map(|item_id| {
                        let index = push_argument(&mut arguments, item_id.0);
                        format!("?{index}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                predicates.push(format!("ri.item_id NOT IN ({excluded})"));
            }
            let sql = format!(
                "WITH
                 root_items AS NOT MATERIALIZED (
                     SELECT lr.item_id, lr.lifecycle, li.kind, li.label,
                            li.created_at, li.updated_at,
                            fi.position_rank AS folder_position, mv.viewed_at
                     FROM library_root lr
                     JOIN library_item li ON li.item_id = lr.item_id
                     LEFT JOIN folder_item fi
                       ON fi.item_id = lr.item_id AND fi.folder_id = ?1
                     LEFT JOIN media_view mv ON mv.item_id = lr.item_id
                     WHERE li.kind = 'collection'
                        OR NOT EXISTS (
                            SELECT 1 FROM collection_member member_root
                            WHERE member_root.media_item_id = lr.item_id
                        )
                 ),
                 root_media AS NOT MATERIALIZED (
                     SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
                     FROM root_items ri WHERE ri.kind = 'media'
                     UNION ALL
                     SELECT ri.item_id, cm.media_item_id
                     FROM root_items ri
                     JOIN collection_member cm ON cm.collection_id = ri.item_id
                     WHERE ri.kind = 'collection'
                 )
                 SELECT ri.item_id FROM root_items ri
                 WHERE {}
                 ORDER BY ri.item_id",
                predicates.join(" AND ")
            );
            let references: Vec<&dyn ToSql> =
                arguments.iter().map(|value| value.as_ref()).collect();
            let mut statement = connection.prepare(&sql)?;
            let resolved = statement
                .query_map(references.as_slice(), |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(resolved)
        }
    }
}

fn invalid_target(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn resolve_connection(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> rusqlite::Result<ItemPage> {
    let mut arguments: Vec<Box<dyn ToSql>> = Vec::new();

    // Parameter 1 is always present so the root CTE can expose folder order
    // without requiring a separate SQL shape for folder queries.
    arguments.push(Box::new(match &item_query.scope {
        ItemScope::Folder { folder_id } => *folder_id,
        _ => -1,
    }));

    let mut predicates = vec![scope_predicate(
        connection,
        &item_query.scope,
        &mut arguments,
    )?];
    apply_filters(&item_query.filters, &mut predicates, &mut arguments);
    let where_clause = predicates.join(" AND ");

    let order_by = order_expression(item_query, &mut arguments);
    let limit_index = arguments.len() + 1;
    arguments.push(Box::new(page.limit));
    let offset_index = arguments.len() + 1;
    arguments.push(Box::new(page.offset));

    let metrics_cte = if page.offset == 0 {
        "metrics AS (
             SELECT
                 (SELECT revision FROM library_meta WHERE singleton = 1) AS revision,
                 COUNT(*) AS visible_item_count,
                 COALESCE(SUM(collection_member_count), 0) AS visible_media_count,
                 COALESCE(SUM(total_size_bytes), 0) AS total_size_bytes
             FROM filtered_roots
         )"
    } else {
        "metrics AS (
             SELECT revision, NULL AS visible_item_count,
                    NULL AS visible_media_count, NULL AS total_size_bytes
             FROM library_meta WHERE singleton = 1
         )"
    };
    let sql = format!(
        "WITH
         root_items AS NOT MATERIALIZED (
             SELECT
                 lr.item_id,
                 lr.lifecycle,
                 li.kind,
                 li.label,
                 li.created_at,
                 li.updated_at,
                 fi.position_rank AS folder_position,
                 mv.viewed_at
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             LEFT JOIN folder_item fi
               ON fi.item_id = lr.item_id AND fi.folder_id = ?1
             LEFT JOIN media_view mv ON mv.item_id = lr.item_id
             WHERE li.kind = 'collection'
                OR NOT EXISTS (
                    SELECT 1
                    FROM collection_member member_root
                    WHERE member_root.media_item_id = lr.item_id
                )
         ),
         root_media AS NOT MATERIALIZED (
             SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
             FROM root_items ri
             WHERE ri.kind = 'media'
             UNION ALL
             SELECT ri.item_id, cm.media_item_id
             FROM root_items ri
             JOIN collection_member cm ON cm.collection_id = ri.item_id
             WHERE ri.kind = 'collection'
         ),
         candidate_roots AS MATERIALIZED (
             SELECT ri.*
             FROM root_items ri
             WHERE {where_clause}
         ),
         root_stats AS (
             SELECT
                 candidate.item_id AS root_item_id,
                 1 AS collection_member_count,
                 mf.size_bytes AS total_size_bytes,
                 ma.imported_at,
                 ma.captured_at,
                 ma.rating AS sort_rating,
                 ma.item_id AS first_media_item_id
             FROM candidate_roots candidate
             JOIN media_asset ma ON ma.item_id = candidate.item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE candidate.kind = 'media'

             UNION ALL

             SELECT
                 candidate.item_id AS root_item_id,
                 COUNT(*) AS collection_member_count,
                 COALESCE(SUM(mf.size_bytes), 0) AS total_size_bytes,
                 MAX(ma.imported_at) AS imported_at,
                 MAX(ma.captured_at) AS captured_at,
                 MAX(ma.rating) AS sort_rating,
                 MIN(cm.media_item_id) AS first_media_item_id
             FROM candidate_roots candidate
             JOIN collection_member cm ON cm.collection_id = candidate.item_id
             JOIN media_asset ma ON ma.item_id = cm.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE candidate.kind = 'collection'
             GROUP BY candidate.item_id
         ),
         filtered_roots AS (
             SELECT
                 ri.item_id,
                 ri.lifecycle,
                 ri.kind,
                 ri.label,
                 ri.created_at,
                 ri.updated_at,
                 ri.folder_position,
                 ri.viewed_at,
                 rs.collection_member_count,
                 rs.total_size_bytes,
                 COALESCE(rs.imported_at, ri.created_at) AS imported_at,
                 rs.captured_at,
                 COALESCE(ri.label, first_asset.name) AS sort_name,
                 rs.sort_rating,
                 CASE
                     WHEN ri.kind = 'collection' THEN (
                         SELECT cm.media_item_id
                         FROM collection_member cm
                         WHERE cm.collection_id = ri.item_id
                         ORDER BY cm.position_rank ASC, cm.media_item_id ASC
                         LIMIT 1
                     )
                     ELSE ri.item_id
                 END AS resolved_cover_media_item_id
             FROM candidate_roots ri
             JOIN root_stats rs ON rs.root_item_id = ri.item_id
             JOIN media_asset first_asset ON first_asset.item_id = rs.first_media_item_id
         ),
         {metrics_cte},
         paged AS (
             SELECT
                 fr.*,
                 display_asset.name AS display_name,
                 display_file.mime_type AS display_mime_type,
                 display_file.file_hash AS display_file_hash,
                 display_file.pixel_width,
                 display_file.pixel_height,
                 display_file.duration_ms,
                 display_file.frame_count,
                 display_file.has_audio,
                 display_file.dominant_color_hex
             FROM filtered_roots fr
             JOIN media_asset display_asset
               ON display_asset.item_id = fr.resolved_cover_media_item_id
             JOIN media_file display_file ON display_file.file_id = display_asset.file_id
             ORDER BY {order_by}, fr.item_id ASC
             LIMIT ?{limit_index} OFFSET ?{offset_index}
         )
         SELECT
             metrics.revision,
             metrics.visible_item_count,
             metrics.visible_media_count,
             metrics.total_size_bytes,
             paged.item_id,
             paged.kind,
             paged.lifecycle,
             COALESCE(paged.label, paged.display_name),
             paged.display_file_hash,
             paged.display_mime_type,
             paged.pixel_width,
             paged.pixel_height,
             paged.duration_ms,
             paged.frame_count,
             paged.dominant_color_hex,
             paged.sort_rating,
             paged.collection_member_count
         FROM metrics
         LEFT JOIN paged ON TRUE"
    );

    let references: Vec<&dyn ToSql> = arguments.iter().map(|value| value.as_ref()).collect();
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = statement.query(references.as_slice())?;

    let mut page_result: Option<ItemPage> = None;
    while let Some(row) = rows.next()? {
        let revision: u64 = row.get(0)?;
        let visible_item_count: Option<i64> = row.get(1)?;
        let visible_media_count: Option<i64> = row.get(2)?;
        let total_size_bytes: Option<i64> = row.get(3)?;
        let item_id: Option<i64> = row.get(4)?;
        let items = if let Some(item_id) = item_id {
            vec![read_summary(row, item_id)?]
        } else {
            Vec::new()
        };

        if let Some(result) = &mut page_result {
            result.items.extend(items);
        } else {
            page_result = Some(ItemPage {
                items,
                revision,
                visible_item_count,
                visible_media_count,
                total_size_bytes,
            });
        }
    }

    Ok(page_result.unwrap_or(ItemPage {
        items: Vec::new(),
        revision: 0,
        visible_item_count: None,
        visible_media_count: None,
        total_size_bytes: None,
    }))
}

fn read_summary(row: &rusqlite::Row<'_>, item_id: i64) -> rusqlite::Result<ItemSummary> {
    let kind = match row.get::<_, String>(5)?.as_str() {
        "media" => ItemKind::Media,
        "collection" => ItemKind::Collection,
        value => {
            return Err(rusqlite::Error::InvalidColumnType(
                5,
                value.into(),
                rusqlite::types::Type::Text,
            ))
        }
    };
    let lifecycle = match row.get::<_, String>(6)?.as_str() {
        "inbox" => Lifecycle::Inbox,
        "active" => Lifecycle::Active,
        "trash" => Lifecycle::Trash,
        value => {
            return Err(rusqlite::Error::InvalidColumnType(
                6,
                value.into(),
                rusqlite::types::Type::Text,
            ))
        }
    };

    let display_mime_type: String = row.get(9)?;
    let frame_count: Option<i64> = row.get(13)?;
    let supports_colors =
        crate::media_capabilities::capabilities_for_stored_media(&display_mime_type, frame_count)
            .can_dominant_colors;

    Ok(ItemSummary {
        item_id: ItemId(item_id),
        kind,
        lifecycle,
        name: row.get(7)?,
        display_file_hash: FileHash(row.get(8)?),
        display_mime_type,
        pixel_width: row.get(10)?,
        pixel_height: row.get(11)?,
        duration_ms: row.get(12)?,
        frame_count,
        dominant_color_hex: supports_colors.then(|| row.get(14)).transpose()?.flatten(),
        rating: row.get(15)?,
        media_count: row.get(16)?,
    })
}

fn scope_predicate(
    connection: &Connection,
    scope: &ItemScope,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<String> {
    let predicate = match scope {
        ItemScope::All => "ri.lifecycle = 'active'".to_string(),
        ItemScope::Inbox => "ri.lifecycle = 'inbox'".to_string(),
        ItemScope::Trash => "ri.lifecycle = 'trash'".to_string(),
        ItemScope::RecentlyViewed => {
            "ri.lifecycle = 'active' AND ri.viewed_at IS NOT NULL".to_string()
        }
        ItemScope::Untagged => "ri.lifecycle = 'active' AND NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::Uncategorized => "ri.lifecycle = 'active' AND NOT EXISTS (
                 SELECT 1 FROM folder_item categorized_folder
                 WHERE categorized_folder.item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::Folder { .. } => "ri.lifecycle = 'active' AND EXISTS (
                 SELECT 1 FROM folder_item scoped_folder
                 WHERE scoped_folder.folder_id = ?1
                   AND scoped_folder.item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::SmartFolder { smart_folder_id } => {
            let (sql, values) = crate::smart_v2::compile_smart_folder_sql(
                connection,
                *smart_folder_id,
                arguments.len(),
            )?;
            arguments.extend(
                values
                    .into_iter()
                    .map(|value| Box::new(value) as Box<dyn ToSql>),
            );
            format!("ri.lifecycle = 'active' AND ri.item_id IN ({sql})")
        }
    };
    Ok(predicate)
}

fn apply_filters(
    filters: &ItemFilters,
    predicates: &mut Vec<String>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(text) = filters.text.as_deref().filter(|text| !text.is_empty()) {
        if let Some(query) = fts_match_query(text) {
            let index = push_argument(arguments, query);
            predicates.push(format!(
                "ri.item_id IN (
                    SELECT COALESCE(cm.collection_id, item_search_fts.item_id)
                    FROM item_search_fts
                    LEFT JOIN collection_member cm
                      ON cm.media_item_id = item_search_fts.item_id
                    WHERE item_search_fts MATCH ?{index}

                    UNION

                    SELECT COALESCE(cm.collection_id, media_search_fts.media_item_id)
                    FROM media_search_fts
                    LEFT JOIN collection_member cm
                      ON cm.media_item_id = media_search_fts.media_item_id
                    WHERE media_search_fts MATCH ?{index}

                    UNION

                    SELECT COALESCE(cm.collection_id, mt.media_item_id)
                    FROM tag_search_fts
                    JOIN media_tag mt ON mt.tag_id = tag_search_fts.tag_id
                    LEFT JOIN collection_member cm ON cm.media_item_id = mt.media_item_id
                    WHERE tag_search_fts MATCH ?{index}

                    UNION

                    SELECT fi.item_id
                    FROM folder_search_fts
                    JOIN folder_item fi ON fi.folder_id = folder_search_fts.folder_id
                    WHERE folder_search_fts MATCH ?{index}
                )"
            ));
        } else {
            predicates.push("0".to_string());
        }
    }

    if let Some(color_hex) = filters
        .color_hex
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let hex_index = push_argument(arguments, color_hex.to_ascii_lowercase());
        if let Some((l, a, b)) = crate::media_processing::colors::lab_components_from_hex(color_hex)
        {
            let l_index = push_argument(arguments, l);
            let a_index = push_argument(arguments, a);
            let b_index = push_argument(arguments, b);
            let threshold_index =
                push_argument(arguments, COLOR_FILTER_DELTA_E * COLOR_FILTER_DELTA_E);
            predicates.push(format!(
                "EXISTS (
                     SELECT 1 FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     JOIN file_color fc ON fc.file_id = ma.file_id
                     WHERE rm.root_item_id = ri.item_id
                       AND (lower(fc.hex) = ?{hex_index}
                            OR ((fc.l - ?{l_index}) * (fc.l - ?{l_index})
                              + (fc.a - ?{a_index}) * (fc.a - ?{a_index})
                              + (fc.b - ?{b_index}) * (fc.b - ?{b_index})) <= ?{threshold_index})
                 )"
            ));
        } else {
            predicates.push(format!(
                "EXISTS (
                     SELECT 1 FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     JOIN file_color fc ON fc.file_id = ma.file_id
                     WHERE rm.root_item_id = ri.item_id AND lower(fc.hex) = ?{hex_index}
                 )"
            ));
        }
    }

    apply_text_range(
        "COALESCE((SELECT MAX(ma.imported_at) FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id WHERE rm.root_item_id = ri.item_id), ri.created_at)",
        filters.imported_after.as_deref(),
        filters.imported_before.as_deref(),
        predicates,
        arguments,
    );
    apply_text_range(
        "MAX(ri.updated_at, COALESCE((SELECT MAX(ma.updated_at) FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id WHERE rm.root_item_id = ri.item_id), ri.updated_at))",
        filters.modified_after.as_deref(),
        filters.modified_before.as_deref(),
        predicates,
        arguments,
    );
    apply_i64_range(
        &display_file_metric("duration_ms"),
        filters.min_duration_ms,
        filters.max_duration_ms,
        predicates,
        arguments,
    );
    apply_i64_range(
        "COALESCE((SELECT SUM(mf.size_bytes) FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id JOIN media_file mf ON mf.file_id = ma.file_id WHERE rm.root_item_id = ri.item_id), 0)",
        filters.min_size_bytes,
        filters.max_size_bytes,
        predicates,
        arguments,
    );
    apply_i64_range(
        &display_file_metric("pixel_width"),
        filters.min_width,
        filters.max_width,
        predicates,
        arguments,
    );
    apply_i64_range(
        &display_file_metric("pixel_height"),
        filters.min_height,
        filters.max_height,
        predicates,
        arguments,
    );
    apply_presence_filter(
        "EXISTS (SELECT 1 FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id WHERE rm.root_item_id = ri.item_id AND NULLIF(TRIM(ma.notes), '') IS NOT NULL)",
        filters.notes_present,
        predicates,
    );
    if let Some(keyword) = nonempty_filter_text(filters.notes_contains.as_deref()) {
        let index = push_argument(arguments, format!("%{keyword}%"));
        predicates.push(format!(
            "EXISTS (SELECT 1 FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id WHERE rm.root_item_id = ri.item_id AND ma.notes LIKE ?{index})"
        ));
    }
    apply_presence_filter(
        "EXISTS (SELECT 1 FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id WHERE rm.root_item_id = ri.item_id AND json_array_length(COALESCE(ma.source_urls_json, '[]')) > 0)",
        filters.source_url_present,
        predicates,
    );
    if let Some(keyword) = nonempty_filter_text(filters.source_url_contains.as_deref()) {
        let index = push_argument(arguments, format!("%{keyword}%"));
        predicates.push(format!(
            "EXISTS (SELECT 1 FROM root_media rm JOIN media_asset ma ON ma.item_id = rm.media_item_id WHERE rm.root_item_id = ri.item_id AND ma.source_urls_json LIKE ?{index})"
        ));
    }

    if !filters.include_folder_ids.is_empty() {
        let matches = filters
            .include_folder_ids
            .iter()
            .map(|folder_id| {
                let index = push_argument(arguments, *folder_id);
                format!("fi.folder_id = ?{index}")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        match filters.folder_match_mode {
            FilterMatchMode::Any => predicates.push(format!(
                "EXISTS (SELECT 1 FROM folder_item fi WHERE fi.item_id = ri.item_id AND ({matches}))"
            )),
            FilterMatchMode::All | FilterMatchMode::Exact => {
                predicates.push(format!(
                    "(SELECT COUNT(DISTINCT fi.folder_id) FROM folder_item fi WHERE fi.item_id = ri.item_id AND ({matches})) = {}",
                    filters.include_folder_ids.len()
                ));
                if filters.folder_match_mode == FilterMatchMode::Exact {
                    predicates.push(format!(
                        "(SELECT COUNT(DISTINCT fi.folder_id) FROM folder_item fi WHERE fi.item_id = ri.item_id) = {}",
                        filters.include_folder_ids.len()
                    ));
                }
            }
        }
    }

    if !filters.exclude_folder_ids.is_empty() {
        let matches = filters
            .exclude_folder_ids
            .iter()
            .map(|folder_id| {
                let index = push_argument(arguments, *folder_id);
                format!("fi.folder_id = ?{index}")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        predicates.push(format!(
            "NOT EXISTS (SELECT 1 FROM folder_item fi WHERE fi.item_id = ri.item_id AND ({matches}))"
        ));
    }

    if !filters.ratings.is_empty() {
        let selected = filters
            .ratings
            .iter()
            .map(|rating| {
                let index = push_argument(arguments, *rating);
                format!("?{index}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id AND COALESCE(ma.rating, 0) IN ({selected})
             )"
        ));
    }

    if !filters.include_mime_types.is_empty() {
        let matches = filters
            .include_mime_types
            .iter()
            .map(|mime_type| {
                let index = push_argument(arguments, mime_type.to_ascii_lowercase());
                format!("lower(mf.mime_type) = ?{index}")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE rm.root_item_id = ri.item_id AND ({matches})
             )"
        ));
    }

    if !filters.exclude_mime_types.is_empty() {
        let matches = filters
            .exclude_mime_types
            .iter()
            .map(|mime_type| {
                let index = push_argument(arguments, mime_type.to_ascii_lowercase());
                format!("lower(mf.mime_type) = ?{index}")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        predicates.push(format!(
            "NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE rm.root_item_id = ri.item_id AND ({matches})
             )"
        ));
    }

    if !filters.include_tags.is_empty() {
        let effective_matches = filters
            .include_tags
            .iter()
            .map(|tag| {
                let (namespace, subtag) = split_tag(tag);
                let namespace_index = push_argument(arguments, namespace);
                let subtag_index = push_argument(arguments, subtag);
                crate::tags_v2::effective_tag_exists_sql(
                    "rm.media_item_id",
                    namespace_index,
                    subtag_index,
                )
            })
            .collect::<Vec<_>>();

        match filters.tag_match_mode {
            FilterMatchMode::Any => predicates.push(format!(
                "EXISTS (
                     SELECT 1 FROM root_media rm
                     WHERE rm.root_item_id = ri.item_id AND ({})
                 )",
                effective_matches.join(" OR ")
            )),
            FilterMatchMode::All | FilterMatchMode::Exact => {
                predicates.extend(effective_matches.into_iter().map(|effective_match| {
                    format!(
                        "EXISTS (
                             SELECT 1 FROM root_media rm
                             WHERE rm.root_item_id = ri.item_id AND {effective_match}
                         )"
                    )
                }));
                if filters.tag_match_mode == FilterMatchMode::Exact {
                    predicates.push(format!(
                        "(SELECT COUNT(DISTINCT mt.tag_id)
                           FROM root_media rm
                           JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                          WHERE rm.root_item_id = ri.item_id) = {}",
                        filters.include_tags.len()
                    ));
                }
            }
        }
    }

    for tag in &filters.exclude_tags {
        let (namespace, subtag) = split_tag(tag);
        let namespace_index = push_argument(arguments, namespace);
        let subtag_index = push_argument(arguments, subtag);
        let effective_match = crate::tags_v2::effective_tag_exists_sql(
            "rm.media_item_id",
            namespace_index,
            subtag_index,
        );
        predicates.push(format!(
            "NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 WHERE rm.root_item_id = ri.item_id
                   AND {effective_match}
             )"
        ));
    }
}

fn fts_match_query(text: &str) -> Option<String> {
    let terms = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\"*"))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

fn display_file_metric(column: &str) -> String {
    format!(
        "(SELECT mf.{column}
          FROM media_asset ma
          JOIN media_file mf ON mf.file_id = ma.file_id
          WHERE ma.item_id = CASE
              WHEN ri.kind = 'collection' THEN (
                  SELECT cm.media_item_id FROM collection_member cm
                  WHERE cm.collection_id = ri.item_id
                  ORDER BY cm.position_rank, cm.media_item_id LIMIT 1
              )
              ELSE ri.item_id
          END)"
    )
}

fn apply_i64_range(
    expression: &str,
    minimum: Option<i64>,
    maximum: Option<i64>,
    predicates: &mut Vec<String>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(minimum) = minimum {
        let index = push_argument(arguments, minimum);
        predicates.push(format!("{expression} >= ?{index}"));
    }
    if let Some(maximum) = maximum {
        let index = push_argument(arguments, maximum);
        predicates.push(format!("{expression} <= ?{index}"));
    }
}

fn apply_text_range(
    expression: &str,
    after: Option<&str>,
    before: Option<&str>,
    predicates: &mut Vec<String>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(after) = nonempty_filter_text(after) {
        let index = push_argument(arguments, after.to_string());
        predicates.push(format!("{expression} >= ?{index}"));
    }
    if let Some(before) = nonempty_filter_text(before) {
        let index = push_argument(arguments, before.to_string());
        predicates.push(format!("{expression} < ?{index}"));
    }
}

fn apply_presence_filter(
    exists_predicate: &str,
    present: Option<bool>,
    predicates: &mut Vec<String>,
) {
    match present {
        Some(true) => predicates.push(exists_predicate.to_string()),
        Some(false) => predicates.push(format!("NOT ({exists_predicate})")),
        None => {}
    }
}

fn nonempty_filter_text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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

fn push_argument<T: ToSql + 'static>(arguments: &mut Vec<Box<dyn ToSql>>, value: T) -> usize {
    arguments.push(Box::new(value));
    arguments.len()
}

fn order_expression(item_query: &ItemQuery, arguments: &mut Vec<Box<dyn ToSql>>) -> String {
    use crate::app::{ItemSortField, SortDirection};

    if matches!(item_query.scope, ItemScope::RecentlyViewed)
        && matches!(item_query.sort.field, ItemSortField::ImportedAt)
    {
        return "fr.viewed_at DESC".to_string();
    }

    let expression = match item_query.sort.field {
        ItemSortField::ImportedAt => "fr.imported_at",
        ItemSortField::CapturedAt => "fr.captured_at",
        ItemSortField::Name => "fr.sort_name",
        ItemSortField::Rating => "COALESCE(fr.sort_rating, -1)",
        ItemSortField::Size => "fr.total_size_bytes",
        ItemSortField::Random => {
            let seed = stable_seed(item_query.sort.random_seed.as_deref().unwrap_or_default());
            let index = push_argument(arguments, seed);
            return format!(
                "((fr.item_id * 1103515245 + ?{index}) & 2147483647) {}",
                match item_query.sort.direction {
                    SortDirection::Ascending => "ASC",
                    SortDirection::Descending => "DESC",
                }
            );
        }
        ItemSortField::FolderOrder => "COALESCE(fr.folder_position, 9223372036854775807)",
    };

    let direction = match item_query.sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    format!("{expression} {direction}")
}

fn stable_seed(value: &str) -> i64 {
    value.bytes().fold(2_166_136_261_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(16_777_619)
    }) as i64
}

#[cfg(test)]
mod tests {
    use super::{
        details, library_statistics, query, resolve_target_ids, selection_summary, sidebar_counts,
        ItemPageRequest, SelectionCollectionCandidate,
    };
    use crate::app::{
        FilterMatchMode, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemSort, ItemTarget,
    };
    use crate::store::Store;

    fn query_for(scope: ItemScope) -> ItemQuery {
        ItemQuery {
            scope,
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        }
    }

    fn seed_store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|tx| {
                insert_media(tx, 1, "standalone", "active", "one.jpg", "image/jpeg", 10, Some(2));
                insert_media(tx, 2, "inbox", "inbox", "inbox.jpg", "image/jpeg", 20, Some(1));
                insert_media(tx, 3, "trash", "trash", "trash.mp4", "video/mp4", 30, Some(4));
                insert_collection(tx, 10, "collection", "active", "Album");
                insert_media(tx, 11, "member-a", "active", "member-a.jpg", "image/jpeg", 40, Some(3));
                insert_media(tx, 12, "member-b", "active", "member-b.mp4", "video/mp4", 50, Some(5));
                tx.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (10, 11, 0), (10, 12, 1)",
                    [],
                )?;
                // Reads must follow member order even if an old cached cover is stale.
                tx.execute(
                    "UPDATE library_item SET cover_media_item_id = 12 WHERE item_id = 10",
                    [],
                )?;
                tx.execute("INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at) VALUES (7, 'folder', 'Folder', 'now', 'now')", [])?;
                tx.execute("UPDATE folder SET notes = 'portfolio bucket' WHERE folder_id = 7", [])?;
                tx.execute("INSERT INTO folder_item (folder_id, item_id, position_rank) VALUES (7, 10, 0), (7, 1, 1)", [])?;
                tx.execute("INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'member-tag')", [])?;
                tx.execute("INSERT INTO media_tag (media_item_id, tag_id) VALUES (11, 1)", [])?;
                tx.execute(
                    "INSERT INTO source_post (
                         source_post_id, site_id, post_key, canonical_url, creator_name,
                         title, description, root_item_id, created_at, updated_at
                     ) VALUES (
                         1, 'example', 'post', 'https://example.test/post', 'Source Creator',
                         'Source Title', 'Source Description', 10, 'now', 'now'
                     )",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO source_item (
                         source_item_id, source_post_id, item_key, position, media_url,
                         canonical_url, media_item_id, state, created_at, updated_at
                     ) VALUES (
                         1, 1, 'item', 0, 'https://cdn.example.test/media',
                         'https://example.test/item', 11, 'ingested', 'now', 'now'
                     )",
                    [],
                )?;
                tx.execute(
                    "UPDATE media_asset
                     SET notes = 'member annotation',
                         source_urls_json = '[\"https://example.test/member-source\"]'
                     WHERE item_id = 11",
                    [],
                )?;
                tx.execute(
                    "UPDATE media_file
                     SET dominant_color_hex = '#123456',
                         dominant_palette_blob = CAST(
                             '[{\"hex\":\"#123456\",\"l\":12.0,\"a\":1.0,\"b\":2.0},{\"hex\":\"#abcdef\",\"l\":80.0,\"a\":3.0,\"b\":4.0}]'
                             AS BLOB
                         )
                     WHERE file_id = 11",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO file_color (file_id, hex, l, a, b)
                     VALUES (11, '#123456', 12.0, 1.0, 2.0),
                            (11, '#abcdef', 80.0, 3.0, 4.0)",
                    [],
                )?;
                tx.execute(
                    "UPDATE media_file SET pixel_width = 1600, pixel_height = 900 WHERE file_id = 1",
                    [],
                )?;
                tx.execute(
                    "UPDATE media_file SET pixel_width = 800, pixel_height = 800 WHERE file_id = 11",
                    [],
                )?;
                tx.execute("INSERT INTO media_view (item_id, viewed_at) VALUES (1, '2026-02-01')", [])?;
                Ok(())
            })
            .unwrap();
        (directory, store)
    }

    fn insert_collection(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        label: &str,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
             VALUES (?1, ?2, 'collection', ?3, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key, label],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
    }

    fn insert_media(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        name: &str,
        mime_type: &str,
        size_bytes: i64,
        rating: Option<i64>,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
             VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, '2026-01-01')",
            rusqlite::params![item_id, format!("hash-{item_id}"), mime_type, size_bytes],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_asset (item_id, file_id, name, rating, imported_at, updated_at)
             VALUES (?1, ?1, ?2, ?3, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, name, rating],
        )
        .unwrap();
    }

    #[test]
    fn all_is_active_and_collapses_collection_members() {
        let (_directory, store) = seed_store();
        let page = query(
            &store,
            &query_for(ItemScope::All),
            ItemPageRequest::default(),
        )
        .unwrap();

        assert_eq!(page.items.len(), 2);
        let collection = page
            .items
            .iter()
            .find(|item| item.item_id == ItemId(10))
            .unwrap();
        assert_eq!(collection.kind, ItemKind::Collection);
        assert_eq!(collection.display_file_hash.0, "hash-11");
        assert_eq!(collection.media_count, 2);
        assert_eq!(page.visible_item_count, Some(2));
        assert_eq!(page.visible_media_count, Some(3));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(11)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(12)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(2)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(3)));
    }

    #[test]
    fn inbox_and_trash_are_separate_root_scopes() {
        let (_directory, store) = seed_store();
        let inbox = query(
            &store,
            &query_for(ItemScope::Inbox),
            ItemPageRequest::default(),
        )
        .unwrap();
        let trash = query(
            &store,
            &query_for(ItemScope::Trash),
            ItemPageRequest::default(),
        )
        .unwrap();

        assert_eq!(
            inbox
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(2)]
        );
        assert_eq!(
            trash
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(3)]
        );
        assert_eq!(inbox.visible_item_count, Some(1));
        assert_eq!(trash.visible_media_count, Some(1));
    }

    #[test]
    fn folder_scope_is_active_and_root_only() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::Folder { folder_id: 7 });
        item_query.sort.field = crate::app::ItemSortField::FolderOrder;
        item_query.sort.direction = crate::app::SortDirection::Ascending;
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10), ItemId(1)]
        );
        assert_eq!(page.visible_item_count, Some(2));
        assert_eq!(page.visible_media_count, Some(3));
    }

    #[test]
    fn media_filters_project_member_matches_to_collection_root() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.include_tags = vec!["member-tag".to_string()];
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
        assert_eq!(page.visible_item_count, Some(1));
        assert_eq!(page.visible_media_count, Some(2));
    }

    #[test]
    fn applicable_filter_contract_uses_display_and_aggregate_values() {
        let (_directory, store) = seed_store();
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE media_asset
                     SET imported_at = CASE item_id
                         WHEN 1 THEN '2026-01-10T00:00:00Z'
                         WHEN 11 THEN '2026-02-10T00:00:00Z'
                         ELSE imported_at END,
                         updated_at = CASE item_id
                         WHEN 1 THEN '2026-04-10T00:00:00Z'
                         WHEN 11 THEN '2026-02-10T00:00:00Z'
                         ELSE updated_at END
                     WHERE item_id IN (1, 11)",
                    [],
                )?;
                tx.execute(
                    "UPDATE media_file
                     SET duration_ms = CASE file_id WHEN 1 THEN 5000 WHEN 11 THEN 10000 END
                     WHERE file_id IN (1, 11)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let matching_ids = |filters: ItemFilters| {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters = filters;
            query(&store, &item_query, ItemPageRequest::default())
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>()
        };

        let mut filters = ItemFilters::default();
        filters.imported_after = Some("2026-02-01T00:00:00Z".into());
        filters.imported_before = Some("2026-03-01T00:00:00Z".into());
        assert_eq!(matching_ids(filters), vec![ItemId(10)]);

        let mut filters = ItemFilters::default();
        filters.modified_after = Some("2026-04-01T00:00:00Z".into());
        assert_eq!(matching_ids(filters), vec![ItemId(1)]);

        let mut filters = ItemFilters::default();
        filters.min_duration_ms = Some(9000);
        assert_eq!(matching_ids(filters), vec![ItemId(10)]);

        let mut filters = ItemFilters::default();
        filters.max_duration_ms = Some(6000);
        assert_eq!(matching_ids(filters), vec![ItemId(1)]);

        let mut filters = ItemFilters::default();
        filters.min_size_bytes = Some(80);
        assert_eq!(matching_ids(filters), vec![ItemId(10)]);

        let mut filters = ItemFilters::default();
        filters.max_size_bytes = Some(20);
        assert_eq!(matching_ids(filters), vec![ItemId(1)]);

        let mut filters = ItemFilters::default();
        filters.min_width = Some(1000);
        filters.min_height = Some(850);
        assert_eq!(matching_ids(filters), vec![ItemId(1)]);

        let mut filters = ItemFilters::default();
        filters.max_width = Some(1000);
        filters.max_height = Some(850);
        assert_eq!(matching_ids(filters), vec![ItemId(10)]);

        let mut filters = ItemFilters::default();
        filters.notes_present = Some(true);
        filters.notes_contains = Some("annotation".into());
        assert_eq!(matching_ids(filters), vec![ItemId(10)]);

        let mut filters = ItemFilters::default();
        filters.notes_present = Some(false);
        assert_eq!(matching_ids(filters), vec![ItemId(1)]);

        let mut filters = ItemFilters::default();
        filters.source_url_present = Some(true);
        filters.source_url_contains = Some("member-source".into());
        assert_eq!(matching_ids(filters), vec![ItemId(10)]);

        let mut filters = ItemFilters::default();
        filters.source_url_present = Some(false);
        assert_eq!(matching_ids(filters), vec![ItemId(1)]);

        let mut query_target = query_for(ItemScope::All);
        query_target.filters.min_size_bytes = Some(80);
        let target = ItemTarget::Query {
            query: query_target,
            excluded_item_ids: Vec::new(),
        };
        assert_eq!(
            store
                .read(|connection| resolve_target_ids(connection, &target))
                .unwrap(),
            vec![10]
        );
    }

    #[test]
    fn folder_filter_matches_root_membership_and_supports_exclusion() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.include_folder_ids = vec![7];
        let included = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            included
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1), ItemId(10)]
        );

        item_query.filters.include_folder_ids.clear();
        item_query.filters.exclude_folder_ids = vec![7];
        let excluded = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert!(excluded.items.is_empty());
    }

    #[test]
    fn rating_filter_is_exact_multi_value_and_includes_unrated() {
        let (_directory, store) = seed_store();
        store
            .transaction(|tx| {
                insert_media(
                    tx,
                    4,
                    "unrated",
                    "active",
                    "unrated.png",
                    "image/png",
                    12,
                    None,
                );
                Ok(())
            })
            .unwrap();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.ratings = vec![2];
        let rated = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            rated
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );

        item_query.filters.ratings = vec![3, 5];
        let collection = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            collection
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );

        item_query.filters.ratings = vec![0];
        let unrated = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            unrated
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(4)]
        );
    }

    #[test]
    fn type_filter_supports_multiple_includes_and_right_click_exclusions() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.include_mime_types = vec!["image/jpeg".to_string()];
        let images = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            images
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1), ItemId(10)]
        );

        item_query.filters.exclude_mime_types = vec!["video/mp4".to_string()];
        let without_video_collections =
            query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            without_video_collections
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );
    }

    #[test]
    fn text_search_covers_all_user_facing_fields_and_item_types() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE library_item SET label = 'hidden member label' WHERE item_id = 11",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let mut item_query = query_for(ItemScope::All);
        for (text, expected) in [
            ("one.jpg", vec![ItemId(1)]),
            ("Album", vec![ItemId(10)]),
            ("member annotation", vec![ItemId(10)]),
            ("member-source", vec![ItemId(10)]),
            ("Source Creator", vec![ItemId(10)]),
            ("Source Title", vec![ItemId(10)]),
            ("Source Description", vec![ItemId(10)]),
            ("cdn.example", vec![ItemId(10)]),
            ("video/mp4", vec![ItemId(10)]),
            ("member-tag", vec![ItemId(10)]),
            ("hidden member label", vec![ItemId(10)]),
            ("Folder", vec![ItemId(1), ItemId(10)]),
            ("portfolio bucket", vec![ItemId(1), ItemId(10)]),
            ("collection", vec![ItemId(10)]),
            ("standalone", vec![ItemId(1)]),
            ("%", vec![]),
        ] {
            item_query.filters.text = Some(text.to_string());
            let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();
            assert_eq!(
                page.items
                    .iter()
                    .map(|item| item.item_id)
                    .collect::<Vec<_>>(),
                expected,
                "search text {text:?}"
            );
        }
    }

    #[test]
    fn text_search_indexes_follow_metadata_tag_folder_and_source_renames() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET name = 'renamed asset' WHERE item_id = 1",
                    [],
                )?;
                transaction.execute(
                    "UPDATE source_post SET creator_name = 'Renamed Person'
                     WHERE source_post_id = 1",
                    [],
                )?;
                transaction
                    .execute("UPDATE tag SET subtag = 'renamed-tag' WHERE tag_id = 1", [])?;
                transaction.execute(
                    "UPDATE folder SET name = 'Renamed Folder' WHERE folder_id = 7",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        for (text, expected) in [
            ("renamed asset", vec![ItemId(1)]),
            ("Renamed Person", vec![ItemId(10)]),
            ("renamed-tag", vec![ItemId(10)]),
            ("Renamed Folder", vec![ItemId(1), ItemId(10)]),
        ] {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters.text = Some(text.to_string());
            let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();
            assert_eq!(
                page.items
                    .iter()
                    .map(|item| item.item_id)
                    .collect::<Vec<_>>(),
                expected,
                "search text {text:?}"
            );
        }

        for stale in ["one.jpg", "Source Creator", "member-tag"] {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters.text = Some(stale.to_string());
            let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();
            assert!(page.items.is_empty(), "stale search text {stale:?}");
        }
    }

    #[test]
    fn color_filter_matches_any_persisted_palette_color() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.color_hex = Some("#ABCDEF".to_string());
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
    }

    #[test]
    fn color_filter_matches_perceptually_near_palette_colors() {
        let (_directory, store) = seed_store();
        let (l, a, b) =
            crate::media_processing::colors::lab_components_from_hex("#36a852").unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO file_color (file_id, hex, l, a, b)
                     VALUES (11, '#36a852', ?1, ?2, ?3)",
                    rusqlite::params![l, a, b],
                )?;
                Ok(())
            })
            .unwrap();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.color_hex = Some("#2f9f4b".to_string());

        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
    }

    #[test]
    fn revision_and_counts_survive_empty_pages() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.text = Some("does-not-exist".to_string());
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert!(page.items.is_empty());
        assert_eq!(page.visible_item_count, Some(0));
        assert_eq!(page.visible_media_count, Some(0));
        assert_eq!(page.revision, 1);

        let append = query(
            &store,
            &query_for(ItemScope::All),
            ItemPageRequest::new(1, 1),
        )
        .unwrap();
        assert_eq!(append.items.len(), 1);
        assert_eq!(append.visible_item_count, None);
        assert_eq!(append.visible_media_count, None);
        assert_eq!(append.total_size_bytes, None);
        assert_eq!(append.revision, 1);
    }

    #[test]
    fn active_only_derived_scopes_use_the_same_root_set() {
        let (_directory, store) = seed_store();

        let recent = query(
            &store,
            &query_for(ItemScope::RecentlyViewed),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(recent.items[0].item_id, ItemId(1));

        let untagged = query(
            &store,
            &query_for(ItemScope::Untagged),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(
            untagged
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );

        let uncategorized = query(
            &store,
            &query_for(ItemScope::Uncategorized),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert!(uncategorized.items.is_empty());
    }

    #[test]
    fn excluded_member_tag_excludes_the_collection_root() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.exclude_tags = vec!["member-tag".to_string()];
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );
    }

    #[test]
    fn tag_filters_use_aliases_and_transitive_implications() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO tag (tag_id, namespace, subtag) VALUES
                         (2, 'general', 'alias-tag'),
                         (3, 'general', 'parent-tag'),
                         (4, 'general', 'grandparent-tag')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO tag_alias (from_tag_id, to_tag_id) VALUES (1, 2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO tag_implication (child_tag_id, parent_tag_id) VALUES
                         (1, 3), (3, 4)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        for tag in ["alias-tag", "parent-tag", "grandparent-tag"] {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters.include_tags = vec![tag.to_string()];
            let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();
            assert_eq!(page.items.len(), 1, "effective tag {tag}");
            assert_eq!(page.items[0].item_id, ItemId(10));
        }
    }

    #[test]
    fn tag_filter_modes_match_any_all_and_exact_root_tags() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO tag (tag_id, namespace, subtag) VALUES
                     (5, 'general', 'red'), (6, 'general', 'blue')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO media_tag (media_item_id, tag_id) VALUES
                     (1, 5), (11, 5), (12, 6)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let ids_for = |mode| {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters.include_tags = vec!["red".into(), "blue".into()];
            item_query.filters.tag_match_mode = mode;
            query(&store, &item_query, ItemPageRequest::default())
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids_for(FilterMatchMode::Any), vec![ItemId(1), ItemId(10)]);
        assert_eq!(ids_for(FilterMatchMode::All), vec![ItemId(10)]);

        let mut exact = query_for(ItemScope::All);
        exact.filters.include_tags = vec!["red".into()];
        exact.filters.tag_match_mode = FilterMatchMode::Exact;
        let exact_ids = query(&store, &exact, ItemPageRequest::default())
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.item_id)
            .collect::<Vec<_>>();
        assert_eq!(exact_ids, vec![ItemId(1)]);
    }

    #[test]
    fn folder_filter_modes_match_any_all_and_exact_membership() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                 VALUES (8, 'second-folder', 'Second', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO folder_item (folder_id, item_id, position_rank) VALUES (8, 10, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let ids_for = |mode| {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters.include_folder_ids = vec![7, 8];
            item_query.filters.folder_match_mode = mode;
            query(&store, &item_query, ItemPageRequest::default())
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids_for(FilterMatchMode::Any), vec![ItemId(1), ItemId(10)]);
        assert_eq!(ids_for(FilterMatchMode::All), vec![ItemId(10)]);

        let mut exact = query_for(ItemScope::All);
        exact.filters.include_folder_ids = vec![7];
        exact.filters.folder_match_mode = FilterMatchMode::Exact;
        let exact_ids = query(&store, &exact, ItemPageRequest::default())
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.item_id)
            .collect::<Vec<_>>();
        assert_eq!(exact_ids, vec![ItemId(1)]);
    }

    #[test]
    fn seeded_random_order_is_stable() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.sort.field = crate::app::ItemSortField::Random;
        item_query.sort.random_seed = Some("stable".to_string());

        let first = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        let second = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(first.items, second.items);
    }

    #[test]
    fn smart_folder_scope_uses_the_same_page_and_count_query() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (9, 'smart:9', 'Tagged', ?1, 'now', 'now')",
                    [serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [{
                                "field": "tags",
                                "op": "include",
                                "values": ["general:member-tag"]
                            }]
                        }]
                    })
                    .to_string()],
                )?;
                Ok(())
            })
            .unwrap();

        let page = query(
            &store,
            &query_for(ItemScope::SmartFolder { smart_folder_id: 9 }),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(page.visible_item_count, Some(1));
        assert_eq!(page.visible_media_count, Some(2));
        assert_eq!(page.items[0].item_id, ItemId(10));
    }

    #[test]
    fn query_mutation_target_matches_grid_and_applies_exclusions() {
        let (_directory, store) = seed_store();
        let target = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: vec![ItemId(1)],
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![10]);
    }

    #[test]
    fn explicit_target_preserves_order_without_sql_parameter_expansion() {
        let (_directory, store) = seed_store();
        let target = ItemTarget::Explicit {
            item_ids: vec![ItemId(10), ItemId(1), ItemId(3), ItemId(2)],
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![10, 1, 3, 2]);

        let duplicate = ItemTarget::Explicit {
            item_ids: vec![ItemId(1), ItemId(1)],
        };
        assert!(store
            .read(|connection| resolve_target_ids(connection, &duplicate))
            .is_err());
    }

    #[test]
    fn collection_details_are_ordered_and_aggregate_member_tags() {
        let (_directory, store) = seed_store();
        let details = details(&store, ItemId(10)).unwrap();

        assert_eq!(details.kind, ItemKind::Collection);
        assert_eq!(details.folder_ids, vec![7]);
        assert_eq!(
            details
                .media
                .iter()
                .map(|media| media.media_item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(11), ItemId(12)]
        );
        assert_eq!(details.aggregate_tags, vec!["member-tag"]);
        assert_eq!(
            details.media[0].dominant_color_hex.as_deref(),
            Some("#123456")
        );
        assert_eq!(details.media[0].dominant_colors, vec!["#123456", "#abcdef"]);
        assert_eq!(details.media[0].tags, vec!["member-tag"]);
        assert!(details.media[1].tags.is_empty());
    }

    #[test]
    fn selection_summary_uses_the_same_root_and_media_counts_as_grid() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_tag (media_item_id, tag_id) VALUES (1, 1)",
                    [],
                )?;
                transaction.execute(
                    "UPDATE media_asset
                     SET notes = CASE WHEN item_id = 12 THEN 'different' ELSE 'shared note' END,
                         source_urls_json = CASE WHEN item_id = 12
                             THEN '[\"https://example.com/two\"]'
                             ELSE '[\"https://example.com/one\"]' END
                     WHERE item_id IN (1, 11, 12)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let target = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: Vec::new(),
        };
        let summary = selection_summary(&store, &target).unwrap();

        assert_eq!(summary.selected_count, 2);
        assert_eq!(summary.stats.mime_counts["image/jpeg"], 2);
        assert_eq!(summary.stats.mime_counts["video/mp4"], 1);
        assert_eq!(summary.stats.total_size_bytes, Some(100));
        assert!(summary.shared_tags.is_empty());
        assert_eq!(summary.top_tags[0].tag, "member-tag");
        assert_eq!(summary.top_tags[0].count, 2);
        assert_eq!(summary.stats.rating_stats.min, Some(2));
        assert_eq!(summary.stats.rating_stats.max, Some(5));
        assert_eq!(summary.stats.rating_stats.shared, None);
        assert_eq!(summary.notes_present_count, 3);
        assert_eq!(summary.shared_notes, None);
        assert_eq!(summary.source_urls_present_count, 3);
        assert_eq!(summary.shared_source_urls, None);
        assert_eq!(
            summary.selected_collection_candidates,
            vec![SelectionCollectionCandidate {
                collection_id: ItemId(10),
                label: Some("Album".to_string()),
                member_count: 2,
            }]
        );
        assert_eq!(summary.shared_folders.len(), 1);
        assert_eq!(summary.revision, 2);

        let excluded = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: vec![ItemId(1)],
        };
        let excluded_summary = selection_summary(&store, &excluded).unwrap();
        assert_eq!(excluded_summary.selected_count, 1);
        assert_eq!(excluded_summary.stats.total_size_bytes, Some(90));
        assert_eq!(excluded_summary.stats.mime_counts["image/jpeg"], 1);
        assert_eq!(excluded_summary.stats.mime_counts["video/mp4"], 1);

        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_tag (media_item_id, tag_id) VALUES (12, 1)",
                    [],
                )?;
                transaction.execute(
                    "UPDATE media_asset
                     SET notes = 'shared note',
                         source_urls_json = '[\"https://example.com/one\"]'
                     WHERE item_id IN (1, 11, 12)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let summary = selection_summary(&store, &target).unwrap();
        assert_eq!(summary.shared_tags[0].tag, "member-tag");
        assert_eq!(summary.shared_tags[0].count, 3);
        assert_eq!(summary.shared_notes.as_deref(), Some("shared note"));
        assert_eq!(
            summary.shared_source_urls,
            Some(vec!["https://example.com/one".to_string()])
        );
    }

    #[test]
    fn selection_summary_preserves_recent_explicit_selection_order_for_stacking() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET imported_at = CASE item_id
                         WHEN 3 THEN '2025-12-01T00:00:00Z'
                         WHEN 2 THEN '2026-01-01T00:00:00Z'
                         WHEN 11 THEN '2026-02-01T00:00:00Z'
                         WHEN 12 THEN '2026-02-02T00:00:00Z'
                         WHEN 1 THEN '2026-03-01T00:00:00Z'
                         ELSE imported_at END",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let summary = selection_summary(
            &store,
            &ItemTarget::Explicit {
                item_ids: vec![ItemId(1), ItemId(2), ItemId(3), ItemId(10)],
            },
        )
        .unwrap();

        assert_eq!(
            summary.sample_hashes,
            vec![
                crate::app::FileHash("hash-1".to_string()),
                crate::app::FileHash("hash-2".to_string()),
                crate::app::FileHash("hash-3".to_string()),
                crate::app::FileHash("hash-11".to_string()),
            ]
        );
    }

    #[test]
    fn sidebar_counts_match_canonical_active_root_scopes() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (9, 'smart:9', 'Tagged', ?1, 'now', 'now')",
                    [serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [{
                                "field": "tags",
                                "op": "include",
                                "values": ["general:member-tag"]
                            }]
                        }]
                    })
                    .to_string()],
                )?;
                Ok(())
            })
            .unwrap();

        let counts = sidebar_counts(&store).unwrap();
        assert_eq!(counts.all, 2);
        assert_eq!(counts.inbox, 1);
        assert_eq!(counts.trash, 1);
        assert_eq!(counts.recently_viewed, 1);
        assert_eq!(counts.untagged, 1);
        assert_eq!(counts.uncategorized, 0);
        assert_eq!(counts.folders[0].count, 2);
        assert_eq!(counts.smart_folders[0].count, 1);
        assert_eq!(counts.revision, 2);
    }

    #[test]
    fn library_statistics_separate_roots_assets_and_physical_storage() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (9, 'smart:9', 'Everything', '{\"groups\":[]}', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO subscription (
                         subscription_id, subscription_key, name, created_at
                     ) VALUES (1, 'subscription:1', 'Example', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let statistics = library_statistics(&store).unwrap();
        assert_eq!(statistics.active_items, 2);
        assert_eq!(statistics.inbox_items, 1);
        assert_eq!(statistics.trash_items, 1);
        assert_eq!(statistics.standalone_items, 3);
        assert_eq!(statistics.collections, 1);
        assert_eq!(statistics.media_assets, 5);
        assert_eq!(statistics.image_assets, 3);
        assert_eq!(statistics.video_assets, 2);
        assert_eq!(statistics.audio_assets, 0);
        assert_eq!(statistics.other_assets, 0);
        assert_eq!(statistics.physical_files, 5);
        assert_eq!(statistics.original_bytes, 150);
        assert_eq!(statistics.tags, 1);
        assert_eq!(statistics.folders, 1);
        assert_eq!(statistics.smart_folders, 1);
        assert_eq!(statistics.subscriptions, 1);
        assert_eq!(statistics.revision, 2);
    }
}
