//! Canonical read path for library item scopes.
//!
//! The query starts from `library_root` and projects collection members into
//! their owning collection. Every scope, filter, count, and page therefore
//! uses the same root set.

use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::{Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{
    FileHash, FilterMatchMode, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemTarget,
    Lifecycle,
};
use crate::store::Store;
use crate::{app::Application, projection_v2::ProjectionSelectionSnapshot};

const DEFAULT_PAGE_LIMIT: i64 = 100;
const MAX_PAGE_LIMIT: i64 = 500;
const MAX_CURSOR_LENGTH: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemPageRequest {
    pub cursor: Option<String>,
    #[ts(type = "number")]
    pub limit: i64,
}

impl Default for ItemPageRequest {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

impl ItemPageRequest {
    pub fn new(cursor: Option<String>, limit: i64) -> Self {
        Self { cursor, limit }
    }

    fn normalized(self) -> Self {
        Self {
            cursor: self.cursor,
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
    pub next_cursor: Option<String>,
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
    #[ts(type = "number")]
    pub media_count: i64,
    pub all_media_are_images: bool,
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
    pub has_notes: bool,
    pub shared_source_urls: Option<Vec<String>>,
    pub has_source_urls: bool,
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

/// Resolve a grid page against the immutable projection captured with the
/// same SQLite revision. Structured filters use the shared bitmap compiler;
/// SQL-only predicates retain the indexed SQL path.
pub fn query_for_application(
    application: &Application,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> Result<ItemPage, String> {
    let page = page.normalized();
    application.store().read_snapshot_captured(
        || application.projections().selection_snapshot(),
        |connection, revision, projection| {
            if let Some(roots) =
                crate::predicate_v2::compile_item_query(connection, &projection, item_query)
                    .map_err(|error| error.to_string())?
            {
                if matches!(
                    item_query.sort.field,
                    crate::app::ItemSortField::FolderOrder
                ) {
                    if let ItemScope::Folder { folder_id } = item_query.scope {
                        return resolve_projected_folder_order_page(
                            connection,
                            item_query,
                            page,
                            revision,
                            &projection,
                            &roots,
                            folder_id,
                        )
                        .map_err(|error| error.to_string());
                    }
                }
                if matches!(item_query.sort.field, crate::app::ItemSortField::ImportedAt) {
                    return resolve_projected_imported_page(
                        connection,
                        item_query,
                        page,
                        revision,
                        &projection,
                        &roots,
                    )
                    .map_err(|error| error.to_string());
                }
                if matches!(
                    item_query.sort.field,
                    crate::app::ItemSortField::CapturedAt
                        | crate::app::ItemSortField::Name
                        | crate::app::ItemSortField::Rating
                        | crate::app::ItemSortField::Size
                        | crate::app::ItemSortField::Random
                ) {
                    return resolve_projected_sorted_page(
                        connection,
                        item_query,
                        page,
                        revision,
                        &projection,
                        &roots,
                    )
                    .map_err(|error| error.to_string());
                }
            }
            resolve_connection(connection, item_query, page).map_err(|error| error.to_string())
        },
    )
}

pub fn details(application: &Application, item_id: ItemId) -> Result<ItemDetails, String> {
    application.store().read_snapshot_captured(
        || application.projections().selection_snapshot(),
        |connection, revision, projection| {
            details_connection(connection, item_id.0, &projection, revision)
                .map_err(|error| error.to_string())
        },
    )
}

pub fn selection_summary(store: &Store, target: &ItemTarget) -> Result<SelectionSummary, String> {
    store.read_snapshot(|connection| selection_summary_connection(connection, target, None, None))
}

pub fn selection_summary_for_application(
    application: &Application,
    target: &ItemTarget,
) -> Result<SelectionSummary, String> {
    application.store().read_snapshot_captured(
        || application.projections().selection_snapshot(),
        |connection, _revision, projection| {
            let preselected = match target {
                ItemTarget::Query {
                    query,
                    excluded_item_ids,
                } => crate::predicate_v2::compile_item_query(connection, &projection, query)
                    .map_err(|error| error.to_string())?
                    .map(|mut roots| {
                        for item_id in excluded_item_ids {
                            if let Ok(item_id) = u32::try_from(item_id.0) {
                                roots.remove(item_id);
                            }
                        }
                        roots
                    }),
                ItemTarget::Explicit { .. } | ItemTarget::Range { .. } => None,
            };
            selection_summary_connection(
                connection,
                target,
                Some(&projection),
                preselected.as_ref(),
            )
            .map_err(|error| error.to_string())
        },
    )
}

fn selection_summary_connection(
    connection: &Connection,
    target: &ItemTarget,
    projection: Option<&ProjectionSelectionSnapshot>,
    preselected: Option<&roaring::RoaringBitmap>,
) -> rusqlite::Result<SelectionSummary> {
    let operation_started = std::time::Instant::now();
    let mut stage_started = operation_started;
    let selection = MaterializedSelection::new(connection, target, preselected)?;
    trace_selection_stage("materialize", stage_started);
    stage_started = std::time::Instant::now();
    let selected_roots = projection.map(|_| selection.bitmap()).transpose()?;
    let (
        selected_count,
        selected_active_count,
        total_size_bytes,
        media_count,
        min_rating,
        max_rating,
        rated_count,
    ) = if let (Some(projection), Some(roots)) = (projection, selected_roots.as_ref()) {
        let aggregate = projection.numeric_aggregates(roots);
        (
            i64_from_u64(aggregate.selected_root_count)?,
            i64_from_u64(aggregate.active_root_count)?,
            i64_from_u128(aggregate.total_size_bytes.sum)?,
            i64_from_u128(aggregate.media_count.sum)?,
            aggregate.rating_min.map(i64::from),
            aggregate.rating_max.map(i64::from),
            i64_from_u64(aggregate.rating.count)?,
        )
    } else {
        connection
            .prepare_cached(
                "SELECT COUNT(*),
                            COALESCE(SUM(summary.lifecycle = 'active'), 0),
                            COALESCE(SUM(summary.total_size_bytes), 0),
                            COALESCE(SUM(summary.media_count), 0),
                            MIN(metadata.rating),
                            MAX(metadata.rating),
                            COUNT(metadata.rating)
                     FROM picto_selected_root selected
                     JOIN root_summary summary ON summary.root_item_id = selected.item_id
                     LEFT JOIN root_metadata metadata
                       ON metadata.root_item_id = selected.item_id",
            )?
            .query_row([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
    };
    trace_selection_stage("scalar_aggregates", stage_started);
    stage_started = std::time::Instant::now();

    let (has_notes, shared_notes, has_source_urls, shared_source_urls) =
        selection_shared_metadata(connection, selected_count)?;
    trace_selection_stage("shared_metadata", stage_started);
    stage_started = std::time::Instant::now();
    let rating_stats = SelectionRatingStats {
        min: min_rating,
        max: max_rating,
        shared: (selected_count > 0 && rated_count == selected_count && min_rating == max_rating)
            .then_some(min_rating)
            .flatten(),
    };
    let all_media_are_images =
        if let (Some(projection), Some(roots)) = (projection, selected_roots.as_ref()) {
            projection.all_media_are_images(roots)
        } else {
            selection_all_media_are_images(connection, media_count)?
        };
    trace_selection_stage("media_compatibility", stage_started);
    stage_started = std::time::Instant::now();
    let active_count: i64 = connection.query_row(
        "SELECT root_count FROM lifecycle_summary WHERE lifecycle = 'active'",
        [],
        |row| row.get(0),
    )?;
    let full_active_library =
        selected_active_count == selected_count && selected_count == active_count;
    let (shared_tags, top_tags) =
        if !full_active_library && projection.is_some() && selected_roots.is_some() {
            selection_tag_counts_projected(
                connection,
                projection.unwrap(),
                selected_roots.as_ref().unwrap(),
                selected_count,
            )?
        } else {
            selection_tag_counts(connection, selected_count, selected_active_count)?
        };
    trace_selection_stage("tag_counts", stage_started);
    stage_started = std::time::Instant::now();
    let shared_folders =
        if let (Some(projection), Some(roots)) = (projection, selected_roots.as_ref()) {
            selection_shared_folders_projected(connection, projection, roots)?
        } else {
            selection_shared_folders(connection, selected_count)?
        };
    trace_selection_stage("shared_folders", stage_started);
    stage_started = std::time::Instant::now();
    let selected_collection_candidates = selection_collection_candidates(connection)?;
    trace_selection_stage("collection_candidates", stage_started);
    stage_started = std::time::Instant::now();

    let sample_item_ids = match target {
        ItemTarget::Explicit { item_ids } => item_ids
            .iter()
            .rev()
            .take(6)
            .rev()
            .map(|item_id| item_id.0)
            .collect(),
        ItemTarget::Query { .. } | ItemTarget::Range { .. } => selection.recent_ids()?,
    };
    let sample_hashes = selection_display_hashes(connection, &sample_item_ids)?
        .into_iter()
        .map(FileHash)
        .collect();
    trace_selection_stage("sample_hashes", stage_started);
    trace_selection_stage("total", operation_started);
    Ok(SelectionSummary {
        total_count: selected_count,
        selected_count,
        sample_hashes,
        shared_tags,
        top_tags,
        shared_folders,
        selected_collection_candidates,
        shared_notes,
        has_notes,
        shared_source_urls,
        has_source_urls,
        stats: SelectionSummaryStats {
            total_size_bytes: Some(total_size_bytes),
            media_count,
            all_media_are_images,
            rating_stats,
        },
        revision: crate::store::schema::revision(connection)?,
    })
}

fn i64_from_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn i64_from_u128(value: u128) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn selection_shared_metadata(
    connection: &Connection,
    selected_count: i64,
) -> rusqlite::Result<(bool, Option<String>, bool, Option<Vec<String>>)> {
    if selected_count == 0 {
        return Ok((false, None, false, None));
    }
    let (first_notes, first_sources, has_notes, has_sources): (Option<String>, String, bool, bool) =
        connection
            .prepare_cached(
                "WITH first_selected AS (
                 SELECT item_id FROM picto_selected_root ORDER BY item_id LIMIT 1
             )
             SELECT NULLIF(TRIM(COALESCE(metadata.notes, '')), ''),
                    COALESCE(metadata.source_urls_json, '[]'),
                    EXISTS(
                        SELECT 1
                        FROM root_metadata present
                             INDEXED BY idx_root_metadata_notes_present
                        JOIN picto_selected_root selected
                          ON selected.item_id = present.root_item_id
                        WHERE present.notes IS NOT NULL AND TRIM(present.notes) <> ''
                        LIMIT 1
                    ),
                    EXISTS(
                        SELECT 1
                        FROM root_metadata present
                             INDEXED BY idx_root_metadata_sources_present
                        JOIN picto_selected_root selected
                          ON selected.item_id = present.root_item_id
                        WHERE json_array_length(present.source_urls_json) > 0
                        LIMIT 1
                    )
             FROM first_selected first
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = first.item_id",
            )?
            .query_row([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;

    let notes_differ = has_notes
        && (first_notes.is_none()
            || connection.query_row(
                "SELECT EXISTS(
             SELECT 1
             FROM picto_selected_root selected
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = selected.item_id
             WHERE NULLIF(TRIM(COALESCE(metadata.notes, '')), '') IS NOT ?1
             LIMIT 1
         )",
                [first_notes.as_deref()],
                |row| row.get(0),
            )?);
    let sources_differ = has_sources
        && (first_sources == "[]"
            || connection.query_row(
                "SELECT EXISTS(
             SELECT 1
             FROM picto_selected_root selected
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = selected.item_id
             WHERE COALESCE(metadata.source_urls_json, '[]') IS NOT ?1
             LIMIT 1
         )",
                [&first_sources],
                |row| row.get(0),
            )?);

    let shared_sources = (!sources_differ)
        .then(|| serde_json::from_str::<Vec<String>>(&first_sources))
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok((
        has_notes,
        (!notes_differ).then_some(first_notes).flatten(),
        has_sources,
        shared_sources,
    ))
}

fn trace_selection_stage(stage: &str, started: std::time::Instant) {
    if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some() {
        eprintln!(
            "selection_summary_stage stage={stage} elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
}

/// A selection is expanded exactly once per summary. Keeping the target in a
/// connection-local table prevents Query and Range predicates from being
/// re-run by every aggregate branch while retaining one pinned WAL snapshot.
struct MaterializedSelection<'connection> {
    connection: &'connection Connection,
}

impl<'connection> MaterializedSelection<'connection> {
    fn new(
        connection: &'connection Connection,
        target: &ItemTarget,
        preselected: Option<&roaring::RoaringBitmap>,
    ) -> rusqlite::Result<Self> {
        connection.execute_batch(
            "PRAGMA query_only = OFF;
             CREATE TEMP TABLE IF NOT EXISTS picto_selected_root (
                 item_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID;
             DELETE FROM picto_selected_root;",
        )?;

        let materialized = (|| {
            if let Some(preselected) = preselected {
                let ids = bitmap_json(preselected);
                connection.execute(
                    "INSERT INTO picto_selected_root(item_id)
                     SELECT CAST(value AS INTEGER) FROM json_each(?1)",
                    [ids],
                )?;
            } else {
                let selection = target_selection_sql(connection, target)?;
                let sql = format!(
                    "{}
                     INSERT INTO picto_selected_root(item_id)
                     SELECT item_id FROM selected_roots",
                    selection.with_clause
                );
                let parameters = selection.parameters();
                connection.execute(&sql, parameters.as_slice())?;
            }
            Ok::<_, rusqlite::Error>(())
        })();
        let restored = connection.execute_batch("PRAGMA query_only = ON;");
        restored?;
        materialized?;
        Ok(Self { connection })
    }

    fn recent_ids(&self) -> rusqlite::Result<Vec<i64>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT selected.item_id
             FROM picto_selected_root selected
             JOIN root_summary summary ON summary.root_item_id = selected.item_id
             ORDER BY summary.imported_at DESC, selected.item_id DESC
             LIMIT 6",
        )?;
        let mut ids = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.reverse();
        Ok(ids)
    }

    fn bitmap(&self) -> rusqlite::Result<roaring::RoaringBitmap> {
        self.connection
            .prepare_cached("SELECT item_id FROM picto_selected_root")?
            .query_map([], |row| row.get::<_, u32>(0))?
            .collect()
    }
}

fn bitmap_json(bitmap: &roaring::RoaringBitmap) -> String {
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

impl Drop for MaterializedSelection<'_> {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch(
            "PRAGMA query_only = OFF;
             DELETE FROM picto_selected_root;
             PRAGMA query_only = ON;",
        );
    }
}

fn selection_all_media_are_images(
    connection: &Connection,
    media_count: i64,
) -> rusqlite::Result<bool> {
    if media_count == 0 {
        return Ok(false);
    }
    connection.query_row(
        "SELECT NOT EXISTS (
             SELECT 1
             FROM (
                 SELECT selected.item_id AS media_item_id
                 FROM picto_selected_root selected
                 JOIN root_summary summary ON summary.root_item_id = selected.item_id
                 WHERE summary.kind = 'media'
                 UNION ALL
                 SELECT member.media_item_id
                 FROM picto_selected_root selected
                 JOIN collection_member member ON member.collection_id = selected.item_id
             ) selected_media
             JOIN media_asset asset ON asset.item_id = selected_media.media_item_id
             JOIN media_file file ON file.file_id = asset.file_id
             WHERE file.mime_type NOT LIKE 'image/%'
             LIMIT 1
         )",
        [],
        |row| row.get(0),
    )
}

fn selection_tag_counts(
    connection: &Connection,
    selected_count: i64,
    selected_active_count: i64,
) -> rusqlite::Result<(Vec<SelectionTagCount>, Vec<SelectionTagCount>)> {
    if selected_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let active_count: i64 = connection.query_row(
        "SELECT root_count FROM lifecycle_summary WHERE lifecycle = 'active'",
        [],
        |row| row.get(0),
    )?;
    let full_active_library =
        selected_active_count == selected_count && selected_count == active_count;

    let sql = if full_active_library {
        "SELECT CASE WHEN tag.namespace IN ('', 'general') THEN tag.subtag
                     ELSE tag.namespace || ':' || tag.subtag END,
                summary.visible_root_count
         FROM tag_summary summary
         JOIN tag ON tag.tag_id = summary.tag_id
         WHERE summary.visible_root_count > 0
         ORDER BY summary.visible_root_count DESC, tag.namespace, tag.subtag"
    } else {
        "SELECT CASE WHEN tag.namespace IN ('', 'general') THEN tag.subtag
                     ELSE tag.namespace || ':' || tag.subtag END,
                COUNT(*) AS root_count
         FROM picto_selected_root selected
         CROSS JOIN root_tag selected_tag
         JOIN tag ON tag.tag_id = selected_tag.tag_id
         WHERE selected_tag.root_item_id = selected.item_id
         GROUP BY selected_tag.tag_id, tag.namespace, tag.subtag
         ORDER BY root_count DESC, tag.namespace, tag.subtag"
    };
    let mut statement = connection.prepare_cached(sql)?;
    let rows = statement.query_map([], |row| {
        Ok(SelectionTagCount {
            tag: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    let mut shared = Vec::new();
    let mut top = Vec::with_capacity(20);
    for row in rows {
        let row = row?;
        if row.count == selected_count {
            shared.push(row.clone());
        }
        if top.len() < 20 {
            top.push(row);
        }
    }
    shared.sort_by(|left, right| left.tag.cmp(&right.tag));
    Ok((shared, top))
}

fn selection_tag_counts_projected(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
    roots: &roaring::RoaringBitmap,
    selected_count: i64,
) -> rusqlite::Result<(Vec<SelectionTagCount>, Vec<SelectionTagCount>)> {
    if selected_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let counts = projection.direct_tag_counts(roots);
    if counts.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let encoded_ids =
        serde_json::to_string(&counts.iter().map(|(tag_id, _)| *tag_id).collect::<Vec<_>>())
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let names = connection
        .prepare_cached(
            "SELECT tag.tag_id,
                    CASE WHEN tag.namespace IN ('', 'general') THEN tag.subtag
                         ELSE tag.namespace || ':' || tag.subtag END
             FROM tag
             JOIN json_each(?1) selected
               ON tag.tag_id = CAST(selected.value AS INTEGER)",
        )?
        .query_map([encoded_ids], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;

    let mut rows = counts
        .into_iter()
        .filter_map(|(tag_id, count)| {
            names
                .get(&tag_id)
                .cloned()
                .map(|tag| SelectionTagCount { tag, count })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.tag.cmp(&right.tag))
    });
    let mut shared = rows
        .iter()
        .filter(|row| row.count == selected_count)
        .cloned()
        .collect::<Vec<_>>();
    shared.sort_by(|left, right| left.tag.cmp(&right.tag));
    rows.truncate(20);
    Ok((shared, rows))
}

fn selection_shared_folders(
    connection: &Connection,
    selected_count: i64,
) -> rusqlite::Result<Vec<SelectionFolderInfo>> {
    if selected_count == 0 {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare_cached(
        "WITH first_selected(item_id) AS (
             SELECT item_id FROM picto_selected_root ORDER BY item_id LIMIT 1
         )
         SELECT folder.folder_id, folder.name
         FROM first_selected first
         JOIN folder_item candidate ON candidate.item_id = first.item_id
         JOIN folder ON folder.folder_id = candidate.folder_id
         JOIN folder_item membership ON membership.folder_id = candidate.folder_id
         JOIN picto_selected_root selected ON selected.item_id = membership.item_id
         GROUP BY folder.folder_id, folder.name
         HAVING COUNT(*) = ?1
         ORDER BY folder.folder_id",
    )?;
    let folders = statement
        .query_map([selected_count], |row| {
            Ok(SelectionFolderInfo {
                folder_id: row.get(0)?,
                name: row.get(1)?,
            })
        })?
        .collect();
    folders
}

fn selection_shared_folders_projected(
    connection: &Connection,
    projection: &crate::projection_v2::ProjectionSelectionSnapshot,
    selected: &roaring::RoaringBitmap,
) -> rusqlite::Result<Vec<SelectionFolderInfo>> {
    let Some(first_root) = selected.iter().next() else {
        return Ok(Vec::new());
    };
    let folder_ids = projection
        .folder_ids_for_root(i64::from(first_root))
        .into_iter()
        .filter(|folder_id| (selected - &projection.folder_bitmap(*folder_id)).is_empty())
        .collect::<Vec<_>>();
    if folder_ids.is_empty() {
        return Ok(Vec::new());
    }
    let encoded = serde_json::to_string(&folder_ids)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection
        .prepare_cached(
            "SELECT folder.folder_id, folder.name
             FROM folder
             JOIN json_each(?1) selected
               ON folder.folder_id = CAST(selected.value AS INTEGER)
             ORDER BY folder.folder_id",
        )?
        .query_map([encoded], |row| {
            Ok(SelectionFolderInfo {
                folder_id: row.get(0)?,
                name: row.get(1)?,
            })
        })?
        .collect()
}

fn selection_collection_candidates(
    connection: &Connection,
) -> rusqlite::Result<Vec<SelectionCollectionCandidate>> {
    let mut statement = connection.prepare_cached(
        "SELECT selected.item_id, metadata.name, summary.media_count
         FROM picto_selected_root selected
         JOIN root_summary summary ON summary.root_item_id = selected.item_id
         LEFT JOIN root_metadata metadata ON metadata.root_item_id = selected.item_id
         WHERE summary.kind = 'collection'
         ORDER BY selected.item_id",
    )?;
    let candidates = statement
        .query_map([], |row| {
            Ok(SelectionCollectionCandidate {
                collection_id: ItemId(row.get(0)?),
                label: row.get(1)?,
                member_count: row.get(2)?,
            })
        })?
        .collect();
    candidates
}

pub(crate) struct TargetSelectionSql {
    pub(crate) with_clause: String,
    arguments: Vec<Box<dyn ToSql>>,
}

impl TargetSelectionSql {
    pub(crate) fn parameters(&self) -> Vec<&dyn ToSql> {
        self.arguments.iter().map(|value| value.as_ref()).collect()
    }
}

pub(crate) fn target_selection_sql(
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
                    selected_roots(item_id) AS MATERIALIZED (
                        SELECT lr.item_id
                        FROM json_each(?1) target
                        JOIN library_root lr ON lr.item_id = CAST(target.value AS INTEGER)
                    ),
                    selected_media(root_item_id, media_item_id) AS MATERIALIZED (
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
            if let Some(selection) = summary_range_query_target_sql(query, excluded_item_ids)? {
                return Ok(selection);
            }
            if query.filters == ItemFilters::default() {
                let mut arguments: Vec<Box<dyn ToSql>> = Vec::new();
                let roots_sql = match &query.scope {
                    ItemScope::All => "SELECT summary.root_item_id AS item_id
                         FROM root_summary summary
                         WHERE summary.lifecycle = 'active'"
                        .to_string(),
                    ItemScope::Inbox => "SELECT summary.root_item_id AS item_id
                         FROM root_summary summary
                         WHERE summary.lifecycle = 'inbox'"
                        .to_string(),
                    ItemScope::Trash => "SELECT summary.root_item_id AS item_id
                         FROM root_summary summary
                         WHERE summary.lifecycle = 'trash'"
                        .to_string(),
                    ItemScope::RecentlyViewed => "SELECT summary.root_item_id AS item_id
                         FROM root_summary summary
                         JOIN media_view viewed ON viewed.item_id = summary.root_item_id
                         WHERE summary.lifecycle = 'active'"
                        .to_string(),
                    ItemScope::Untagged => "SELECT summary.root_item_id AS item_id
                         FROM root_summary summary
                         WHERE summary.lifecycle = 'active'
                           AND NOT EXISTS (
                               SELECT 1 FROM root_tag tags
                               WHERE tags.root_item_id = summary.root_item_id
                           )"
                    .to_string(),
                    ItemScope::Uncategorized => "SELECT summary.root_item_id AS item_id
                         FROM root_summary summary
                         WHERE summary.lifecycle = 'active'
                           AND NOT EXISTS (
                               SELECT 1 FROM folder_item folders
                               WHERE folders.item_id = summary.root_item_id
                           )"
                    .to_string(),
                    ItemScope::Folder { folder_id } => {
                        arguments.push(Box::new(*folder_id));
                        "SELECT folders.item_id
                         FROM folder_item folders
                         JOIN root_summary summary ON summary.root_item_id = folders.item_id
                         WHERE folders.folder_id = ?1 AND summary.lifecycle = 'active'"
                            .to_string()
                    }
                    ItemScope::SmartFolder { smart_folder_id } => {
                        arguments.push(Box::new(*smart_folder_id));
                        "SELECT membership.root_item_id AS item_id
                         FROM smart_folder_generation generation
                         JOIN smart_folder_membership membership
                           ON membership.generation_id = generation.generation_id
                         JOIN root_summary summary
                           ON summary.root_item_id = membership.root_item_id
                         WHERE generation.smart_folder_id = ?1
                           AND generation.state = 'active'
                           AND summary.lifecycle = 'active'"
                            .to_string()
                    }
                };
                let exclusion = if excluded_item_ids.is_empty() {
                    String::new()
                } else {
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
                    format!(
                        "WHERE NOT EXISTS (
                             SELECT 1 FROM json_each(?{index}) excluded
                             WHERE CAST(excluded.value AS INTEGER) = candidates.item_id
                         )"
                    )
                };
                return Ok(TargetSelectionSql {
                    with_clause: format!(
                        "WITH
                         selected_roots(item_id) AS MATERIALIZED (
                             SELECT candidates.item_id
                             FROM ({roots_sql}) candidates
                             {exclusion}
                         ),
                         selected_media(root_item_id, media_item_id) AS MATERIALIZED (
                             SELECT selected.item_id, selected.item_id
                             FROM selected_roots selected
                             JOIN media_asset asset ON asset.item_id = selected.item_id
                             UNION ALL
                             SELECT selected.item_id, member.media_item_id
                             FROM selected_roots selected
                             JOIN collection_member member
                               ON member.collection_id = selected.item_id
                         )"
                    ),
                    arguments,
                });
            }

            let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(match &query.scope {
                ItemScope::Folder { folder_id } => *folder_id,
                _ => -1,
            })];
            let mut predicates = vec![scope_predicate(connection, &query.scope, &mut arguments)?];
            apply_filters(connection, &query.filters, &mut predicates, &mut arguments)?;
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
                         SELECT summary.root_item_id AS item_id,
                                summary.lifecycle, li.kind,
                                li.created_at, li.updated_at,
                                fi.position_rank AS folder_position, mv.viewed_at
                         FROM root_summary summary
                         JOIN library_item li ON li.item_id = summary.root_item_id
                         LEFT JOIN folder_item fi
                           ON fi.item_id = summary.root_item_id AND fi.folder_id = ?1
                         LEFT JOIN media_view mv ON mv.item_id = summary.root_item_id
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
                     selected_roots(item_id) AS MATERIALIZED (
                         SELECT ri.item_id FROM root_items ri WHERE {}
                     ),
                     selected_media(root_item_id, media_item_id) AS MATERIALIZED (
                         SELECT rm.root_item_id, rm.media_item_id
                         FROM root_media rm
                         JOIN selected_roots sr ON sr.item_id = rm.root_item_id
                     )",
                    predicates.join(" AND ")
                ),
                arguments,
            })
        }
        ItemTarget::Range {
            query,
            anchor_item_id,
            focus_item_id,
        } => range_target_selection_sql(connection, query, *anchor_item_id, *focus_item_id),
    }
}

fn summary_range_query_target_sql(
    query: &ItemQuery,
    excluded_item_ids: &[ItemId],
) -> rusqlite::Result<Option<TargetSelectionSql>> {
    let lifecycle = match query.scope {
        ItemScope::All => "active",
        ItemScope::Inbox => "inbox",
        ItemScope::Trash => "trash",
        _ => return Ok(None),
    };
    let mut remaining = query.filters.clone();
    remaining.min_size_bytes = None;
    remaining.max_size_bytes = None;
    if remaining != ItemFilters::default()
        || (query.filters.min_size_bytes.is_none() && query.filters.max_size_bytes.is_none())
    {
        return Ok(None);
    }

    let mut arguments: Vec<Box<dyn ToSql>> = Vec::new();
    let mut predicates = vec![format!("summary.lifecycle = '{lifecycle}'")];
    if let Some(minimum) = query.filters.min_size_bytes {
        let index = push_argument(&mut arguments, minimum);
        predicates.push(format!("summary.total_size_bytes >= ?{index}"));
    }
    if let Some(maximum) = query.filters.max_size_bytes {
        let index = push_argument(&mut arguments, maximum);
        predicates.push(format!("summary.total_size_bytes <= ?{index}"));
    }
    if !excluded_item_ids.is_empty() {
        let encoded = serde_json::to_string(
            &excluded_item_ids
                .iter()
                .map(|item_id| item_id.0)
                .collect::<Vec<_>>(),
        )
        .map_err(|error| invalid_target(format!("Could not encode excluded item IDs: {error}")))?;
        let index = push_argument(&mut arguments, encoded);
        predicates.push(format!(
            "NOT EXISTS (
                 SELECT 1 FROM json_each(?{index}) excluded
                 WHERE CAST(excluded.value AS INTEGER) = summary.root_item_id
             )"
        ));
    }

    Ok(Some(TargetSelectionSql {
        with_clause: format!(
            "WITH
             selected_roots(item_id) AS MATERIALIZED (
                 SELECT summary.root_item_id
                 FROM root_summary summary
                 WHERE {}
             ),
             selected_media(root_item_id, media_item_id) AS MATERIALIZED (
                 SELECT selected.item_id, selected.item_id
                 FROM selected_roots selected
                 JOIN media_asset asset ON asset.item_id = selected.item_id
                 UNION ALL
                 SELECT selected.item_id, member.media_item_id
                 FROM selected_roots selected
                 JOIN collection_member member ON member.collection_id = selected.item_id
             )",
            predicates.join(" AND ")
        ),
        arguments,
    }))
}

fn range_target_selection_sql(
    connection: &Connection,
    query: &ItemQuery,
    anchor_item_id: ItemId,
    focus_item_id: ItemId,
) -> rusqlite::Result<TargetSelectionSql> {
    if query.filters == ItemFilters::default()
        && matches!(query.sort.field, crate::app::ItemSortField::Size)
    {
        if let Some(lifecycle) = match query.scope {
            ItemScope::All => Some("active"),
            ItemScope::Inbox => Some("inbox"),
            ItemScope::Trash => Some("trash"),
            _ => None,
        } {
            return size_range_target_selection_sql(
                query,
                lifecycle,
                anchor_item_id,
                focus_item_id,
            );
        }
    }

    let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(match &query.scope {
        ItemScope::Folder { folder_id } => *folder_id,
        _ => -1,
    })];
    let mut predicates = vec![scope_predicate(connection, &query.scope, &mut arguments)?];
    apply_filters(connection, &query.filters, &mut predicates, &mut arguments)?;
    let sort_plan = SortPlan::for_query(query, &mut arguments);
    let anchor_index = push_argument(&mut arguments, anchor_item_id.0);
    let focus_index = push_argument(&mut arguments, focus_item_id.0);
    let expected_endpoint_count = if anchor_item_id == focus_item_id {
        1
    } else {
        2
    };
    let (after_operator, before_operator) = if sort_plan.direction == "ASC" {
        (">", "<")
    } else {
        ("<", ">")
    };
    let after_anchor = format!(
        "(candidate.sort_key {after_operator} endpoints.anchor_key OR
          (candidate.sort_key = endpoints.anchor_key
           AND candidate.item_id >= ?{anchor_index}))"
    );
    let before_anchor = format!(
        "(candidate.sort_key {before_operator} endpoints.anchor_key OR
          (candidate.sort_key = endpoints.anchor_key
           AND candidate.item_id <= ?{anchor_index}))"
    );
    let after_focus = format!(
        "(candidate.sort_key {after_operator} endpoints.focus_key OR
          (candidate.sort_key = endpoints.focus_key
           AND candidate.item_id >= ?{focus_index}))"
    );
    let before_focus = format!(
        "(candidate.sort_key {before_operator} endpoints.focus_key OR
          (candidate.sort_key = endpoints.focus_key
           AND candidate.item_id <= ?{focus_index}))"
    );

    Ok(TargetSelectionSql {
        with_clause: format!(
            "WITH
             root_items AS NOT MATERIALIZED (
                 SELECT summary.root_item_id AS item_id,
                        summary.lifecycle, item.kind,
                        metadata.name AS root_name,
                        metadata.rating AS root_rating,
                        item.created_at, item.updated_at,
                        folder.position_rank AS folder_position,
                        viewed.viewed_at
                 FROM root_summary summary
                 JOIN library_item item ON item.item_id = summary.root_item_id
                 LEFT JOIN root_metadata metadata
                   ON metadata.root_item_id = summary.root_item_id
                 LEFT JOIN folder_item folder
                   ON folder.item_id = summary.root_item_id AND folder.folder_id = ?1
                 LEFT JOIN media_view viewed ON viewed.item_id = summary.root_item_id
             ),
             root_media AS NOT MATERIALIZED (
                 SELECT root.item_id AS root_item_id, root.item_id AS media_item_id
                 FROM root_items root WHERE root.kind = 'media'
                 UNION ALL
                 SELECT root.item_id, member.media_item_id
                 FROM root_items root
                 JOIN collection_member member ON member.collection_id = root.item_id
                 WHERE root.kind = 'collection'
             ),
             candidate_roots AS MATERIALIZED (
                 SELECT ri.* FROM root_items ri
                 WHERE {where_clause}
             ),
             filtered_roots AS MATERIALIZED (
                 SELECT ri.item_id, ri.lifecycle, ri.kind,
                        ri.root_name, ri.root_rating,
                        ri.created_at, ri.updated_at,
                        ri.folder_position, ri.viewed_at,
                        summary.media_count,
                        summary.total_size_bytes,
                        COALESCE(summary.imported_at, ri.created_at) AS imported_at,
                        summary.captured_at,
                        COALESCE(ri.root_name, first_asset.name) AS sort_name,
                        summary.sort_rating
                 FROM candidate_roots ri
                 JOIN root_summary summary ON summary.root_item_id = ri.item_id
                 JOIN media_asset first_asset
                   ON first_asset.item_id = summary.cover_media_item_id
             ),
             ordered_roots(item_id, sort_key) AS MATERIALIZED (
                 SELECT fr.item_id, {sort_expression}
                 FROM filtered_roots fr
             ),
             endpoints(anchor_key, focus_key, endpoint_count) AS MATERIALIZED (
                 SELECT
                     MAX(CASE WHEN item_id = ?{anchor_index} THEN sort_key END),
                     MAX(CASE WHEN item_id = ?{focus_index} THEN sort_key END),
                     COALESCE(SUM(CASE
                         WHEN item_id = ?{anchor_index} OR item_id = ?{focus_index}
                         THEN 1 ELSE 0 END), 0)
                 FROM ordered_roots
             ),
             selected_roots(item_id) AS MATERIALIZED (
                 SELECT candidate.item_id
                 FROM ordered_roots candidate
                 CROSS JOIN endpoints
                 WHERE endpoints.endpoint_count = {expected_endpoint_count}
                   AND (({after_anchor} AND {before_focus})
                     OR ({after_focus} AND {before_anchor}))
             ),
             selected_media(root_item_id, media_item_id) AS MATERIALIZED (
                 SELECT selected.item_id, selected.item_id
                 FROM selected_roots selected
                 JOIN media_asset asset ON asset.item_id = selected.item_id
                 UNION ALL
                 SELECT selected.item_id, member.media_item_id
                 FROM selected_roots selected
                 JOIN collection_member member
                   ON member.collection_id = selected.item_id
             )",
            where_clause = predicates.join(" AND "),
            sort_expression = sort_plan.expression,
        ),
        arguments,
    })
}

fn size_range_target_selection_sql(
    query: &ItemQuery,
    lifecycle: &str,
    anchor_item_id: ItemId,
    focus_item_id: ItemId,
) -> rusqlite::Result<TargetSelectionSql> {
    let ascending = matches!(query.sort.direction, crate::app::SortDirection::Ascending);
    let expected_endpoint_count = if anchor_item_id == focus_item_id {
        1
    } else {
        2
    };
    let (after_operator, before_operator) = if ascending { (">", "<") } else { ("<", ">") };
    let after_anchor = format!(
        "(candidate.total_size_bytes {after_operator} endpoints.anchor_key OR
          (candidate.total_size_bytes = endpoints.anchor_key
           AND candidate.root_item_id >= ?1))"
    );
    let before_anchor = format!(
        "(candidate.total_size_bytes {before_operator} endpoints.anchor_key OR
          (candidate.total_size_bytes = endpoints.anchor_key
           AND candidate.root_item_id <= ?1))"
    );
    let after_focus = format!(
        "(candidate.total_size_bytes {after_operator} endpoints.focus_key OR
          (candidate.total_size_bytes = endpoints.focus_key
           AND candidate.root_item_id >= ?2))"
    );
    let before_focus = format!(
        "(candidate.total_size_bytes {before_operator} endpoints.focus_key OR
          (candidate.total_size_bytes = endpoints.focus_key
           AND candidate.root_item_id <= ?2))"
    );

    Ok(TargetSelectionSql {
        with_clause: format!(
            "WITH
             endpoints(anchor_key, focus_key, endpoint_count) AS MATERIALIZED (
                 SELECT MAX(CASE WHEN root_item_id = ?1 THEN total_size_bytes END),
                        MAX(CASE WHEN root_item_id = ?2 THEN total_size_bytes END),
                        COALESCE(SUM(CASE WHEN root_item_id IN (?1, ?2)
                                          THEN 1 ELSE 0 END), 0)
                 FROM root_summary
                 WHERE lifecycle = '{lifecycle}'
                   AND root_item_id IN (?1, ?2)
             ),
             selected_roots(item_id) AS MATERIALIZED (
                 SELECT candidate.root_item_id
                 FROM root_summary candidate
                 CROSS JOIN endpoints
                 WHERE candidate.lifecycle = '{lifecycle}'
                   AND endpoints.endpoint_count = {expected_endpoint_count}
                   AND (({after_anchor} AND {before_focus})
                     OR ({after_focus} AND {before_anchor}))
             ),
             selected_media(root_item_id, media_item_id) AS MATERIALIZED (
                 SELECT selected.item_id, selected.item_id
                 FROM selected_roots selected
                 JOIN media_asset asset ON asset.item_id = selected.item_id
                 UNION ALL
                 SELECT selected.item_id, member.media_item_id
                 FROM selected_roots selected
                 JOIN collection_member member ON member.collection_id = selected.item_id
             )"
        ),
        arguments: vec![Box::new(anchor_item_id.0), Box::new(focus_item_id.0)],
    })
}

fn selection_display_hashes(
    connection: &Connection,
    item_ids: &[i64],
) -> rusqlite::Result<Vec<String>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    let encoded = serde_json::to_string(item_ids)
        .map_err(|error| invalid_target(format!("Could not encode sample item IDs: {error}")))?;
    connection
        .prepare(
            "WITH requested(position, item_id) AS MATERIALIZED (
                 SELECT CAST(key AS INTEGER), CAST(value AS INTEGER) FROM json_each(?1)
             )
             SELECT mf.file_hash
             FROM requested
             JOIN root_summary summary ON summary.root_item_id = requested.item_id
             JOIN media_asset display_asset
               ON display_asset.item_id = summary.cover_media_item_id
             JOIN media_file mf ON mf.file_id = display_asset.file_id
             WHERE mf.file_hash IS NOT NULL
             ORDER BY requested.position",
        )
        .and_then(|mut statement| {
            statement
                .query_map([encoded], |row| row.get::<_, String>(0))?
                .collect()
        })
}

pub fn sidebar_counts_for_application(
    application: &crate::app::Application,
) -> Result<SidebarCounts, String> {
    application.store().read_snapshot_captured(
        || {
            (
                application.projections().sidebar_snapshot_all(),
                application.projections().selection_snapshot(),
            )
        },
        |connection, revision, (snapshot, selection)| {
            (|| -> rusqlite::Result<SidebarCounts> {
                let mut result = SidebarCounts {
                    all: snapshot.all,
                    inbox: snapshot.inbox,
                    trash: snapshot.trash,
                    untagged: snapshot.untagged,
                    uncategorized: snapshot.uncategorized,
                    revision,
                    ..SidebarCounts::default()
                };
                result.recently_viewed = connection.query_row(
                    "SELECT COUNT(*) FROM media_view mv
                     WHERE EXISTS (
                         SELECT 1 FROM library_root root
                         WHERE root.item_id = mv.item_id AND root.lifecycle = 'active'
                     )",
                    [],
                    |row| row.get(0),
                )?;
                result.duplicates = crate::duplicates_v2::count_candidates(connection, &selection)?;

                let folder_counts = snapshot.folders.into_iter().collect::<HashMap<_, _>>();
                result.folders = connection
                    .prepare("SELECT folder_id FROM folder ORDER BY folder_id")?
                    .query_map([], |row| {
                        let folder_id = row.get::<_, i64>(0)?;
                        Ok(ScopeCount {
                            id: folder_id,
                            count: folder_counts.get(&folder_id).copied().unwrap_or_default(),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                result.smart_folders = connection
                    .prepare(
                        "SELECT smart_folder.smart_folder_id,
                                COALESCE(generation.member_count, 0)
                         FROM smart_folder
                         LEFT JOIN smart_folder_generation generation
                           ON generation.smart_folder_id = smart_folder.smart_folder_id
                          AND generation.state = 'active'
                         ORDER BY smart_folder.smart_folder_id",
                    )?
                    .query_map([], |row| {
                        Ok(ScopeCount {
                            id: row.get(0)?,
                            count: row.get(1)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(result)
            })()
            .map_err(|error| error.to_string())
        },
    )
}

pub fn library_statistics(store: &Store) -> Result<LibraryStatistics, String> {
    store.read_snapshot(|connection| {
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
                    revision: row_revision(row, 16)?,
                })
            },
        )
    })
}

fn details_connection(
    connection: &Connection,
    item_id: i64,
    projection: &ProjectionSelectionSnapshot,
    revision: u64,
) -> rusqlite::Result<ItemDetails> {
    let (kind, lifecycle, label, cover_media_item_id, notes, rating, source_urls_json): (
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        String,
    ) = connection
        .query_row(
            "SELECT li.kind, lr.lifecycle, COALESCE(
                        metadata.name,
                        CASE WHEN li.kind = 'media' THEN direct_asset.name ELSE cover_asset.name END
                    ),
                    CASE WHEN li.kind = 'collection' THEN summary.cover_media_item_id END,
                    metadata.notes, metadata.rating,
                    COALESCE(metadata.source_urls_json, '[]')
             FROM library_root lr JOIN library_item li ON li.item_id = lr.item_id
             JOIN root_summary summary ON summary.root_item_id = lr.item_id
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = lr.item_id
             LEFT JOIN media_asset direct_asset ON direct_asset.item_id = lr.item_id
             LEFT JOIN media_asset cover_asset
               ON cover_asset.item_id = summary.cover_media_item_id
             WHERE lr.item_id = ?1",
            [item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| invalid_target(format!("Item {item_id} is not a library root")))?;
    let kind = parse_kind(&kind)?;
    let lifecycle = parse_lifecycle(&lifecycle)?;
    let source_urls = serde_json::from_str::<Vec<String>>(&source_urls_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let folder_ids = projection.folder_ids_for_root(item_id);
    let media_order = match kind {
        ItemKind::Media => vec![item_id],
        ItemKind::Collection => projection.group_order(item_id).ok_or_else(|| {
            invalid_target(format!(
                "Collection {item_id} has no canonical member order"
            ))
        })?,
    };
    let media_order_json = serde_json::to_string(&media_order)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let mut media = connection
        .prepare(
            "WITH root_media(media_item_id, position) AS (
                 SELECT CAST(value AS INTEGER), CAST(key AS INTEGER)
                 FROM json_each(?1)
             )
             SELECT ma.item_id, mf.file_hash, mf.mime_type, mf.dominant_color_hex,
                    mf.dominant_palette_blob, mf.size_bytes, mf.pixel_width, mf.pixel_height,
                    mf.duration_ms, mf.frame_count, mf.has_audio, ma.name,
                    ma.captured_at, ma.imported_at, rm.position
             FROM root_media rm
             JOIN media_asset ma ON ma.item_id = rm.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             ORDER BY rm.position, ma.item_id",
        )?
        .query_map([media_order_json], |row| {
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
                notes: notes.clone(),
                rating,
                source_urls: source_urls.clone(),
                captured_at: row.get(12)?,
                imported_at: row.get(13)?,
                position: row.get(14)?,
                tags: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let tag_ids = projection.tag_ids_for_root(item_id);
    let tag_ids_json = serde_json::to_string(&tag_ids)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let aggregate_tags = connection
        .prepare(
            "SELECT CASE WHEN tag.namespace IN ('', 'general') THEN tag.subtag
                         ELSE tag.namespace || ':' || tag.subtag END
             FROM tag
             JOIN json_each(?1) selected
               ON tag.tag_id = CAST(selected.value AS INTEGER)
             ORDER BY tag.namespace, tag.subtag",
        )?
        .query_map([tag_ids_json], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for media_item in &mut media {
        media_item.tags.clone_from(&aggregate_tags);
    }
    Ok(ItemDetails {
        item_id: ItemId(item_id),
        kind,
        lifecycle,
        label,
        cover_media_item_id: cover_media_item_id.map(ItemId),
        folder_ids,
        media,
        aggregate_tags,
        revision,
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
        ItemTarget::Query { .. } | ItemTarget::Range { .. } => {
            let selection = target_selection_sql(connection, target)?;
            let sql = format!(
                "{}
                 SELECT item_id FROM selected_roots ORDER BY item_id",
                selection.with_clause
            );
            let references = selection.parameters();
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

const SPARSE_PROJECTED_PAGE_THRESHOLD: u64 = 4_096;
const PROJECTED_SCAN_CHUNK: i64 = 1_024;

fn resolve_projected_sorted_page(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
    revision: u64,
    projection: &ProjectionSelectionSnapshot,
    roots: &roaring::RoaringBitmap,
) -> rusqlite::Result<ItemPage> {
    let ordered = if roots.len() <= SPARSE_PROJECTED_PAGE_THRESHOLD
        || matches!(item_query.sort.field, crate::app::ItemSortField::Random)
    {
        ordered_sparse_projected_roots_by_sort(connection, item_query, roots, &page)?
    } else {
        ordered_dense_projected_roots_by_sort(connection, item_query, roots, &page)?
    };
    let sort_plan = SortPlan::for_query(item_query, &mut Vec::new());
    projected_page_from_ordered(
        connection, page, revision, projection, roots, &sort_plan, ordered,
    )
}

fn projected_page_from_ordered(
    connection: &Connection,
    page: ItemPageRequest,
    revision: u64,
    projection: &ProjectionSelectionSnapshot,
    roots: &roaring::RoaringBitmap,
    sort_plan: &SortPlan,
    ordered: Vec<(i64, CursorKey)>,
) -> rusqlite::Result<ItemPage> {
    let mut entries = hydrate_projected_roots(connection, revision, &ordered)?;
    let metrics = (page.cursor.is_none()).then(|| projection.numeric_aggregates(roots));
    let has_more = entries.len() > page.limit as usize;
    entries.truncate(page.limit as usize);
    let next_cursor = if has_more {
        entries
            .last()
            .map(|(item, key)| sort_plan.encode_cursor(key.clone(), item.item_id.0))
            .transpose()?
    } else {
        None
    };
    Ok(ItemPage {
        items: entries.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        revision,
        visible_item_count: metrics
            .as_ref()
            .map(|metrics| i64_from_u64(metrics.selected_root_count))
            .transpose()?,
        visible_media_count: metrics
            .as_ref()
            .map(|metrics| i64_from_u128(metrics.media_count.sum))
            .transpose()?,
        total_size_bytes: metrics
            .as_ref()
            .map(|metrics| i64_from_u128(metrics.total_size_bytes.sum))
            .transpose()?,
    })
}

fn ordered_sparse_projected_roots_by_sort(
    connection: &Connection,
    item_query: &ItemQuery,
    roots: &roaring::RoaringBitmap,
    page: &ItemPageRequest,
) -> rusqlite::Result<Vec<(i64, CursorKey)>> {
    let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(bitmap_json(roots))];
    let sort_plan = SortPlan::for_query(item_query, &mut arguments);
    let expression = projected_sort_expression(&sort_plan.expression);
    let cursor_clause = projected_sort_cursor_clause(
        &sort_plan,
        &expression,
        page.cursor.as_deref(),
        &mut arguments,
    )?;
    let limit_index = push_argument(&mut arguments, page.limit + 1);
    let sql = format!(
        "WITH selected(root_item_id) AS (
             SELECT CAST(value AS INTEGER) FROM json_each(?1)
         )
         SELECT summary.root_item_id, {expression}
         FROM selected
         JOIN root_summary summary USING (root_item_id)
         WHERE TRUE {cursor_clause}
         ORDER BY {expression} {direction}, summary.root_item_id ASC
         LIMIT ?{limit_index}",
        direction = sort_plan.direction,
    );
    let references = arguments
        .iter()
        .map(|argument| argument.as_ref())
        .collect::<Vec<_>>();
    connection
        .prepare(&sql)?
        .query_map(references.as_slice(), |row| {
            Ok((row.get(0)?, read_cursor_key(row, 1, sort_plan.key_kind)?))
        })?
        .collect()
}

fn ordered_dense_projected_roots_by_sort(
    connection: &Connection,
    item_query: &ItemQuery,
    roots: &roaring::RoaringBitmap,
    page: &ItemPageRequest,
) -> rusqlite::Result<Vec<(i64, CursorKey)>> {
    let mut scan_cursor = page.cursor.clone();
    let mut result = Vec::with_capacity((page.limit + 1) as usize);
    while result.len() < (page.limit + 1) as usize {
        let mut arguments = Vec::<Box<dyn ToSql>>::new();
        let sort_plan = SortPlan::for_query(item_query, &mut arguments);
        let expression = projected_sort_expression(&sort_plan.expression);
        let cursor_clause = projected_sort_cursor_clause(
            &sort_plan,
            &expression,
            scan_cursor.as_deref(),
            &mut arguments,
        )?;
        let chunk_index = push_argument(&mut arguments, PROJECTED_SCAN_CHUNK);
        let index = projected_sort_index(item_query);
        let sql = format!(
            "SELECT summary.root_item_id, {expression}
             FROM root_summary summary INDEXED BY {index}
             WHERE TRUE {cursor_clause}
             ORDER BY {expression} {direction}, summary.root_item_id ASC
             LIMIT ?{chunk_index}",
            direction = sort_plan.direction,
        );
        let references = arguments
            .iter()
            .map(|argument| argument.as_ref())
            .collect::<Vec<_>>();
        let scanned = connection
            .prepare(&sql)?
            .query_map(references.as_slice(), |row| {
                Ok((row.get(0)?, read_cursor_key(row, 1, sort_plan.key_kind)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if scanned.is_empty() {
            break;
        }
        for (item_id, key) in &scanned {
            if u32::try_from(*item_id)
                .ok()
                .is_some_and(|item_id| roots.contains(item_id))
            {
                result.push((*item_id, key.clone()));
                if result.len() == (page.limit + 1) as usize {
                    break;
                }
            }
        }
        if result.len() == (page.limit + 1) as usize
            || scanned.len() < PROJECTED_SCAN_CHUNK as usize
        {
            break;
        }
        let (item_id, key) = scanned.last().expect("non-empty projected scan");
        scan_cursor = Some(sort_plan.encode_cursor(key.clone(), *item_id)?);
    }
    Ok(result)
}

fn projected_sort_expression(expression: &str) -> String {
    expression
        .replace("fr.item_id", "summary.root_item_id")
        .replace("fr.", "summary.")
}

fn projected_sort_index(item_query: &ItemQuery) -> &'static str {
    use crate::app::{ItemSortField, SortDirection};
    match (&item_query.sort.field, &item_query.sort.direction) {
        (ItemSortField::CapturedAt, SortDirection::Ascending) => "idx_root_summary_captured_asc",
        (ItemSortField::CapturedAt, SortDirection::Descending) => "idx_root_summary_captured_desc",
        (ItemSortField::Name, SortDirection::Ascending) => "idx_root_summary_name_asc",
        (ItemSortField::Name, SortDirection::Descending) => "idx_root_summary_name_desc",
        (ItemSortField::Rating, SortDirection::Ascending) => "idx_root_summary_rating_asc",
        (ItemSortField::Rating, SortDirection::Descending) => "idx_root_summary_rating_desc",
        (ItemSortField::Size, SortDirection::Ascending) => "idx_root_summary_size_asc",
        (ItemSortField::Size, SortDirection::Descending) => "idx_root_summary_size_desc",
        _ => "sqlite_autoindex_root_summary_1",
    }
}

fn projected_sort_cursor_clause(
    sort_plan: &SortPlan,
    expression: &str,
    cursor: Option<&str>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<String> {
    let Some(cursor) = cursor else {
        return Ok(String::new());
    };
    let cursor = sort_plan.decode_cursor(cursor)?;
    let key_index = match cursor.key {
        CursorKey::Integer(value) => push_argument(arguments, value),
        CursorKey::Text(value) => push_argument(arguments, value),
    };
    let item_index = push_argument(arguments, cursor.item_id);
    let comparison = if sort_plan.direction == "ASC" {
        ">"
    } else {
        "<"
    };
    Ok(format!(
        "AND ({expression} {comparison} ?{key_index}
              OR ({expression} = ?{key_index}
                  AND summary.root_item_id > ?{item_index}))"
    ))
}

fn read_cursor_key(
    row: &rusqlite::Row<'_>,
    index: usize,
    kind: CursorKeyKind,
) -> rusqlite::Result<CursorKey> {
    match kind {
        CursorKeyKind::Integer => row.get(index).map(CursorKey::Integer),
        CursorKeyKind::Text => row.get(index).map(CursorKey::Text),
    }
}

fn resolve_projected_folder_order_page(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
    revision: u64,
    projection: &ProjectionSelectionSnapshot,
    roots: &roaring::RoaringBitmap,
    folder_id: i64,
) -> rusqlite::Result<ItemPage> {
    let sort_plan = SortPlan::for_query(item_query, &mut Vec::new());
    let mut ordered = projection
        .folder_order(folder_id)
        .unwrap_or_else(|| roots.iter().map(i64::from).collect());
    ordered.retain(|item_id| {
        u32::try_from(*item_id)
            .ok()
            .is_some_and(|item_id| roots.contains(item_id))
    });
    let mut keyed = ordered
        .into_iter()
        .enumerate()
        .map(|(position, item_id)| {
            i64::try_from(position)
                .map(|position| (item_id, CursorKey::Integer(position)))
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if sort_plan.direction == "DESC" {
        keyed.reverse();
    }
    if let Some(encoded) = page.cursor.as_deref() {
        let cursor = sort_plan.decode_cursor(encoded)?;
        let CursorKey::Integer(position) = cursor.key else {
            return Err(invalid_target("Invalid folder-order page cursor"));
        };
        keyed.retain(|(item_id, key)| {
            let CursorKey::Integer(candidate) = key else {
                return false;
            };
            if sort_plan.direction == "ASC" {
                *candidate > position || (*candidate == position && *item_id > cursor.item_id)
            } else {
                *candidate < position || (*candidate == position && *item_id > cursor.item_id)
            }
        });
    }
    keyed.truncate((page.limit + 1) as usize);

    let mut entries = hydrate_projected_roots(connection, revision, &keyed)?;
    let metrics = (page.cursor.is_none()).then(|| projection.numeric_aggregates(roots));
    let has_more = entries.len() > page.limit as usize;
    entries.truncate(page.limit as usize);
    let next_cursor = if has_more {
        entries
            .last()
            .map(|(item, key)| sort_plan.encode_cursor(key.clone(), item.item_id.0))
            .transpose()?
    } else {
        None
    };
    Ok(ItemPage {
        items: entries.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        revision,
        visible_item_count: metrics
            .as_ref()
            .map(|metrics| i64_from_u64(metrics.selected_root_count))
            .transpose()?,
        visible_media_count: metrics
            .as_ref()
            .map(|metrics| i64_from_u128(metrics.media_count.sum))
            .transpose()?,
        total_size_bytes: metrics
            .as_ref()
            .map(|metrics| i64_from_u128(metrics.total_size_bytes.sum))
            .transpose()?,
    })
}

fn resolve_projected_imported_page(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
    revision: u64,
    projection: &ProjectionSelectionSnapshot,
    roots: &roaring::RoaringBitmap,
) -> rusqlite::Result<ItemPage> {
    let sort_plan = SortPlan::for_query(item_query, &mut Vec::new());
    let ordered = if roots.len() <= SPARSE_PROJECTED_PAGE_THRESHOLD {
        ordered_sparse_projected_roots(
            connection,
            roots,
            &sort_plan,
            page.cursor.as_deref(),
            page.limit + 1,
        )?
    } else {
        ordered_dense_projected_roots(
            connection,
            roots,
            projected_scope_lifecycle(&item_query.scope),
            &sort_plan,
            page.cursor.as_deref(),
            page.limit + 1,
        )?
    };
    let mut entries = hydrate_projected_roots(connection, revision, &ordered)?;
    let metrics = (page.cursor.is_none()).then(|| projection.numeric_aggregates(roots));
    let has_more = entries.len() > page.limit as usize;
    entries.truncate(page.limit as usize);
    let next_cursor = if has_more {
        entries
            .last()
            .map(|(item, key)| sort_plan.encode_cursor(key.clone(), item.item_id.0))
            .transpose()?
    } else {
        None
    };
    Ok(ItemPage {
        items: entries.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        revision,
        visible_item_count: metrics
            .as_ref()
            .map(|metrics| i64_from_u64(metrics.selected_root_count))
            .transpose()?,
        visible_media_count: metrics
            .as_ref()
            .map(|metrics| i64_from_u128(metrics.media_count.sum))
            .transpose()?,
        total_size_bytes: metrics
            .as_ref()
            .map(|metrics| i64_from_u128(metrics.total_size_bytes.sum))
            .transpose()?,
    })
}

fn projected_scope_lifecycle(scope: &ItemScope) -> &'static str {
    match scope {
        ItemScope::Inbox => "inbox",
        ItemScope::Trash => "trash",
        ItemScope::All
        | ItemScope::Untagged
        | ItemScope::Uncategorized
        | ItemScope::Folder { .. }
        | ItemScope::SmartFolder { .. } => "active",
        ItemScope::RecentlyViewed => unreachable!("recently viewed is not bitmap compiled"),
    }
}

fn ordered_sparse_projected_roots(
    connection: &Connection,
    roots: &roaring::RoaringBitmap,
    sort_plan: &SortPlan,
    cursor: Option<&str>,
    limit: i64,
) -> rusqlite::Result<Vec<(i64, CursorKey)>> {
    let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(bitmap_json(roots))];
    let cursor_clause = projected_imported_cursor_clause(sort_plan, cursor, &mut arguments)?;
    let limit_index = push_argument(&mut arguments, limit);
    let item_direction = sort_plan.direction;
    let sql = format!(
        "WITH selected(root_item_id) AS (
             SELECT CAST(value AS INTEGER) FROM json_each(?1)
         )
         SELECT summary.root_item_id, summary.imported_at
         FROM selected
         JOIN root_summary summary USING (root_item_id)
         WHERE summary.imported_at IS NOT NULL
           {cursor_clause}
         ORDER BY summary.imported_at {direction},
                  summary.root_item_id {item_direction}
         LIMIT ?{limit_index}",
        direction = sort_plan.direction,
    );
    let references = arguments
        .iter()
        .map(|argument| argument.as_ref())
        .collect::<Vec<_>>();
    connection
        .prepare(&sql)?
        .query_map(references.as_slice(), |row| {
            Ok((row.get(0)?, CursorKey::Text(row.get(1)?)))
        })?
        .collect()
}

fn ordered_dense_projected_roots(
    connection: &Connection,
    roots: &roaring::RoaringBitmap,
    lifecycle: &str,
    sort_plan: &SortPlan,
    cursor: Option<&str>,
    limit: i64,
) -> rusqlite::Result<Vec<(i64, CursorKey)>> {
    let mut scan_cursor = cursor.map(str::to_owned);
    let mut result = Vec::with_capacity(limit as usize);
    while result.len() < limit as usize {
        let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(lifecycle.to_string())];
        let cursor_clause =
            projected_imported_cursor_clause(sort_plan, scan_cursor.as_deref(), &mut arguments)?;
        let chunk_index = push_argument(&mut arguments, PROJECTED_SCAN_CHUNK);
        let sql = format!(
            "SELECT summary.root_item_id, summary.imported_at
             FROM root_summary summary INDEXED BY idx_root_summary_imported_asc
             WHERE summary.lifecycle = ?1
               AND summary.imported_at IS NOT NULL
               {cursor_clause}
             ORDER BY summary.imported_at {direction},
                      summary.root_item_id {direction}
             LIMIT ?{chunk_index}",
            direction = sort_plan.direction,
        );
        let references = arguments
            .iter()
            .map(|argument| argument.as_ref())
            .collect::<Vec<_>>();
        let scanned = connection
            .prepare(&sql)?
            .query_map(references.as_slice(), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if scanned.is_empty() {
            break;
        }
        for (item_id, imported_at) in &scanned {
            if u32::try_from(*item_id)
                .ok()
                .is_some_and(|item_id| roots.contains(item_id))
            {
                result.push((*item_id, CursorKey::Text(imported_at.clone())));
                if result.len() == limit as usize {
                    break;
                }
            }
        }
        if result.len() == limit as usize || scanned.len() < PROJECTED_SCAN_CHUNK as usize {
            break;
        }
        let (item_id, imported_at) = scanned.last().expect("non-empty scan");
        scan_cursor =
            Some(sort_plan.encode_cursor(CursorKey::Text(imported_at.clone()), *item_id)?);
    }
    Ok(result)
}

fn projected_imported_cursor_clause(
    sort_plan: &SortPlan,
    cursor: Option<&str>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<String> {
    let Some(cursor) = cursor else {
        return Ok(String::new());
    };
    let cursor = sort_plan.decode_cursor(cursor)?;
    let CursorKey::Text(imported_at) = cursor.key else {
        return Err(invalid_target("Invalid imported-at page cursor"));
    };
    let imported_index = push_argument(arguments, imported_at);
    let item_index = push_argument(arguments, cursor.item_id);
    let comparison = if sort_plan.direction == "ASC" {
        ">"
    } else {
        "<"
    };
    Ok(format!(
        "AND (summary.imported_at {comparison} ?{imported_index}
              OR (summary.imported_at = ?{imported_index}
                  AND summary.root_item_id {comparison} ?{item_index}))"
    ))
}

fn hydrate_projected_roots(
    connection: &Connection,
    revision: u64,
    ordered: &[(i64, CursorKey)],
) -> rusqlite::Result<Vec<(ItemSummary, CursorKey)>> {
    if ordered.is_empty() {
        return Ok(Vec::new());
    }
    let item_ids = ordered
        .iter()
        .map(|(item_id, _)| *item_id)
        .collect::<Vec<_>>();
    let encoded_ids = serde_json::to_string(&item_ids)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let revision = i64::try_from(revision)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let mut statement = connection.prepare(
        "WITH selected(item_id, position) AS (
             SELECT CAST(value AS INTEGER), CAST(key AS INTEGER)
             FROM json_each(?1)
         )
         SELECT ?2, NULL, NULL, NULL,
                summary.root_item_id, summary.kind, summary.lifecycle,
                COALESCE(metadata.name, display_asset.name),
                display_file.file_hash, display_file.mime_type,
                display_file.pixel_width, display_file.pixel_height,
                display_file.duration_ms, display_file.frame_count,
                display_file.dominant_color_hex, metadata.rating,
                summary.media_count, selected.position
         FROM selected
         JOIN root_summary summary ON summary.root_item_id = selected.item_id
         LEFT JOIN root_metadata metadata
           ON metadata.root_item_id = summary.root_item_id
         JOIN media_asset display_asset
           ON display_asset.item_id = summary.cover_media_item_id
         JOIN media_file display_file ON display_file.file_id = display_asset.file_id
         ORDER BY selected.position",
    )?;
    let summaries = statement
        .query_map(rusqlite::params![encoded_ids, revision], |row| {
            let item_id = row.get::<_, i64>(4)?;
            read_summary(row, item_id)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(summaries
        .into_iter()
        .zip(ordered.iter().map(|(_, key)| key.clone()))
        .collect())
}

fn resolve_connection(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> rusqlite::Result<ItemPage> {
    if item_query.filters == ItemFilters::default()
        && matches!(item_query.sort.field, crate::app::ItemSortField::ImportedAt)
        && matches!(
            item_query.scope,
            ItemScope::All | ItemScope::Inbox | ItemScope::Trash | ItemScope::Folder { .. }
        )
    {
        return resolve_indexed_imported_page(connection, item_query, page);
    }

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
    apply_filters(
        connection,
        &item_query.filters,
        &mut predicates,
        &mut arguments,
    )?;
    let where_clause = predicates.join(" AND ");

    let sort_plan = SortPlan::for_query(item_query, &mut arguments);
    let cursor_clause = if let Some(encoded) = page.cursor.as_deref() {
        let cursor = sort_plan.decode_cursor(encoded)?;
        let key_index = match cursor.key {
            CursorKey::Integer(value) => push_argument(&mut arguments, value),
            CursorKey::Text(value) => push_argument(&mut arguments, value),
        };
        let item_index = push_argument(&mut arguments, cursor.item_id);
        let comparison = if sort_plan.direction == "ASC" {
            ">"
        } else {
            "<"
        };
        format!(
            "WHERE ({expression} {comparison} ?{key_index}\n\
                 OR ({expression} = ?{key_index} AND fr.item_id > ?{item_index}))",
            expression = sort_plan.expression,
        )
    } else {
        String::new()
    };
    let limit_index = arguments.len() + 1;
    arguments.push(Box::new(page.limit + 1));

    let lifecycle_metrics = if item_query.filters == ItemFilters::default() {
        match item_query.scope {
            ItemScope::All => Some("active"),
            ItemScope::Inbox => Some("inbox"),
            ItemScope::Trash => Some("trash"),
            _ => None,
        }
    } else {
        None
    };
    let metrics_cte = if page.cursor.is_some() {
        "metrics AS (
             SELECT revision, NULL AS visible_item_count,
                    NULL AS visible_media_count, NULL AS total_size_bytes
             FROM library_meta WHERE singleton = 1
         )"
        .to_string()
    } else if let Some(lifecycle) = lifecycle_metrics {
        format!(
            "metrics AS (
                 SELECT lm.revision,
                        ls.root_count AS visible_item_count,
                        ls.media_count AS visible_media_count,
                        ls.total_size_bytes
                 FROM library_meta lm
                 JOIN lifecycle_summary ls ON ls.lifecycle = '{lifecycle}'
                 WHERE lm.singleton = 1
             )"
        )
    } else {
        "metrics AS (
             SELECT
                 (SELECT revision FROM library_meta WHERE singleton = 1) AS revision,
                 COUNT(*) AS visible_item_count,
                 COALESCE(SUM(media_count), 0) AS visible_media_count,
                 COALESCE(SUM(total_size_bytes), 0) AS total_size_bytes
             FROM filtered_roots
         )"
        .to_string()
    };
    let sql = format!(
        "WITH
         root_items AS NOT MATERIALIZED (
             SELECT
                 lr.item_id,
                 lr.lifecycle,
                 li.kind,
                 metadata.name AS root_name,
                 metadata.rating AS root_rating,
                 li.created_at,
                 li.updated_at,
                 fi.position_rank AS folder_position,
                 mv.viewed_at
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = lr.item_id
             LEFT JOIN folder_item fi
               ON fi.item_id = lr.item_id AND fi.folder_id = ?1
             LEFT JOIN media_view mv ON mv.item_id = lr.item_id
             WHERE NOT EXISTS (
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
         filtered_roots AS (
             SELECT
                 ri.item_id,
                 ri.lifecycle,
                 ri.kind,
                 ri.root_name,
                 ri.root_rating,
                 ri.created_at,
                 ri.updated_at,
                 ri.folder_position,
                 ri.viewed_at,
                 rs.media_count,
                 rs.total_size_bytes,
                 COALESCE(rs.imported_at, ri.created_at) AS imported_at,
                 rs.captured_at,
                 COALESCE(ri.root_name, first_asset.name) AS sort_name,
                 rs.sort_rating,
                 rs.cover_media_item_id AS resolved_cover_media_item_id
             FROM candidate_roots ri
             JOIN root_summary rs ON rs.root_item_id = ri.item_id
             JOIN media_asset first_asset ON first_asset.item_id = rs.cover_media_item_id
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
                 display_file.dominant_color_hex,
                 {sort_expression} AS sort_key
             FROM filtered_roots fr
             JOIN media_asset display_asset
               ON display_asset.item_id = fr.resolved_cover_media_item_id
             JOIN media_file display_file ON display_file.file_id = display_asset.file_id
             {cursor_clause}
             ORDER BY {sort_expression} {sort_direction}, fr.item_id ASC
             LIMIT ?{limit_index}
         )
         SELECT
             metrics.revision,
             metrics.visible_item_count,
             metrics.visible_media_count,
             metrics.total_size_bytes,
             paged.item_id,
             paged.kind,
             paged.lifecycle,
             COALESCE(paged.root_name, paged.display_name),
             paged.display_file_hash,
             paged.display_mime_type,
             paged.pixel_width,
             paged.pixel_height,
             paged.duration_ms,
             paged.frame_count,
             paged.dominant_color_hex,
             paged.root_rating,
             paged.media_count,
             paged.sort_key
         FROM metrics
         LEFT JOIN paged ON TRUE
         ORDER BY paged.sort_key {sort_direction}, paged.item_id ASC",
        sort_expression = sort_plan.expression,
        sort_direction = sort_plan.direction,
    );

    let references: Vec<&dyn ToSql> = arguments.iter().map(|value| value.as_ref()).collect();
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = statement.query(references.as_slice())?;

    let mut revision = 0;
    let mut visible_item_count = None;
    let mut visible_media_count = None;
    let mut total_size_bytes = None;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        revision = row_revision(row, 0)?;
        visible_item_count = row.get(1)?;
        visible_media_count = row.get(2)?;
        total_size_bytes = row.get(3)?;
        let item_id: Option<i64> = row.get(4)?;
        if let Some(item_id) = item_id {
            let key = match sort_plan.key_kind {
                CursorKeyKind::Integer => CursorKey::Integer(row.get(17)?),
                CursorKeyKind::Text => CursorKey::Text(row.get(17)?),
            };
            entries.push((read_summary(row, item_id)?, key));
        }
    }
    if page.cursor.is_some() {
        visible_item_count = None;
        visible_media_count = None;
        total_size_bytes = None;
    }

    let has_more = entries.len() > page.limit as usize;
    entries.truncate(page.limit as usize);
    let next_cursor = if has_more {
        entries
            .last()
            .map(|(item, key)| sort_plan.encode_cursor(key.clone(), item.item_id.0))
            .transpose()?
    } else {
        None
    };
    Ok(ItemPage {
        items: entries.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        revision,
        visible_item_count,
        visible_media_count,
        total_size_bytes,
    })
}

/// The common All/Inbox/Trash grid must not materialize every matching root
/// before applying its page limit. Walk the imported-at index, test lifecycle,
/// and hydrate only the requested page.
fn resolve_indexed_imported_page(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> rusqlite::Result<ItemPage> {
    use crate::app::SortDirection;

    let (scope_predicate, metrics_cte, result_lifecycle, mut arguments): (
        String,
        String,
        &'static str,
        Vec<Box<dyn ToSql>>,
    ) = match item_query.scope {
        ItemScope::All | ItemScope::Inbox | ItemScope::Trash => {
            let lifecycle = match item_query.scope {
                ItemScope::All => "active",
                ItemScope::Inbox => "inbox",
                ItemScope::Trash => "trash",
                _ => unreachable!(),
            };
            (
                "rs.lifecycle = ?1".to_string(),
                "metrics AS (
                     SELECT lm.revision, ls.root_count AS visible_item_count,
                            ls.media_count AS visible_media_count,
                            ls.total_size_bytes
                     FROM library_meta lm
                     JOIN lifecycle_summary ls ON ls.lifecycle = ?1
                     WHERE lm.singleton = 1
                 )"
                .to_string(),
                lifecycle,
                vec![Box::new(lifecycle.to_string())],
            )
        }
        ItemScope::Folder { folder_id } => (
            "rs.lifecycle = 'active'
             AND EXISTS (
                 SELECT 1 FROM folder_item fi
                 WHERE fi.folder_id = ?1 AND fi.item_id = rs.root_item_id
             )"
            .to_string(),
            "metrics AS (
                 SELECT lm.revision,
                        COALESCE(fs.visible_root_count, 0) AS visible_item_count,
                        COALESCE(fs.media_count, 0) AS visible_media_count,
                        COALESCE(fs.total_size_bytes, 0) AS total_size_bytes
                 FROM library_meta lm
                 LEFT JOIN folder_summary fs ON fs.folder_id = ?1
                 WHERE lm.singleton = 1
             )"
            .to_string(),
            "active",
            vec![Box::new(folder_id)],
        ),
        _ => unreachable!("indexed imported-at path called for an unsupported scope"),
    };
    let direction = match item_query.sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    let index = "idx_root_summary_imported_asc";
    let sort_plan = SortPlan::for_query(item_query, &mut Vec::new());
    let cursor_clause = if let Some(encoded) = page.cursor.as_deref() {
        let cursor = sort_plan.decode_cursor(encoded)?;
        let CursorKey::Text(imported_at) = cursor.key else {
            return Err(invalid_target("Invalid imported-at page cursor"));
        };
        let imported_index = push_argument(&mut arguments, imported_at);
        let item_index = push_argument(&mut arguments, cursor.item_id);
        let comparison = if direction == "ASC" { ">" } else { "<" };
        let item_comparison = if direction == "ASC" { ">" } else { "<" };
        format!(
            "AND (rs.imported_at {comparison} ?{imported_index}
                  OR (rs.imported_at = ?{imported_index}
                      AND rs.root_item_id {item_comparison} ?{item_index}))"
        )
    } else {
        String::new()
    };
    let limit_index = push_argument(&mut arguments, page.limit + 1);
    let sql = format!(
        "WITH {metrics_cte},
         candidates AS MATERIALIZED (
             SELECT rs.root_item_id AS item_id,
                    rs.media_count,
                    rs.total_size_bytes,
                    rs.imported_at,
                    rs.captured_at,
                    rs.sort_rating,
                    li.kind,
                    metadata.name AS root_name,
                    metadata.rating AS root_rating,
                    rs.cover_media_item_id AS display_media_item_id
             FROM root_summary rs INDEXED BY {index}
             JOIN library_item li ON li.item_id = rs.root_item_id
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = rs.root_item_id
             WHERE {scope_predicate}
               AND rs.imported_at IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM collection_member member_root
                   WHERE member_root.media_item_id = rs.root_item_id
               )
               {cursor_clause}
             ORDER BY rs.imported_at {direction}, rs.root_item_id {direction}
             LIMIT ?{limit_index}
         )
         SELECT metrics.revision,
                metrics.visible_item_count, metrics.visible_media_count,
                metrics.total_size_bytes,
                candidate.item_id, candidate.kind, '{result_lifecycle}',
                COALESCE(candidate.root_name, display_asset.name),
                display_file.file_hash, display_file.mime_type,
                display_file.pixel_width, display_file.pixel_height,
                display_file.duration_ms, display_file.frame_count,
                display_file.dominant_color_hex, candidate.root_rating,
                candidate.media_count, candidate.imported_at
         FROM metrics
         LEFT JOIN candidates candidate ON TRUE
         LEFT JOIN media_asset display_asset
           ON display_asset.item_id = candidate.display_media_item_id
         LEFT JOIN media_file display_file
           ON display_file.file_id = display_asset.file_id
         ORDER BY candidate.imported_at {direction}, candidate.item_id {direction}"
    );
    let references: Vec<&dyn ToSql> = arguments.iter().map(|value| value.as_ref()).collect();
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = statement.query(references.as_slice())?;
    let mut revision = 0;
    let mut visible_item_count = None;
    let mut visible_media_count = None;
    let mut total_size_bytes = None;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        revision = row_revision(row, 0)?;
        visible_item_count = row.get(1)?;
        visible_media_count = row.get(2)?;
        total_size_bytes = row.get(3)?;
        if let Some(item_id) = row.get::<_, Option<i64>>(4)? {
            let key = CursorKey::Text(row.get(17)?);
            entries.push((read_summary(row, item_id)?, key));
        }
    }
    if page.cursor.is_some() {
        visible_item_count = None;
        visible_media_count = None;
        total_size_bytes = None;
    }
    let has_more = entries.len() > page.limit as usize;
    entries.truncate(page.limit as usize);
    let next_cursor = if has_more {
        entries
            .last()
            .map(|(item, key)| sort_plan.encode_cursor(key.clone(), item.item_id.0))
            .transpose()?
    } else {
        None
    };
    Ok(ItemPage {
        items: entries.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        revision,
        visible_item_count,
        visible_media_count,
        total_size_bytes,
    })
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

fn row_revision(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn scope_predicate(
    _connection: &Connection,
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
                 SELECT 1 FROM root_tag root_tag
                 WHERE root_tag.root_item_id = ri.item_id
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
            let index = push_argument(arguments, *smart_folder_id);
            format!(
                "ri.lifecycle = 'active' AND EXISTS (
                     SELECT 1
                     FROM smart_folder_generation generation
                     JOIN smart_folder_membership membership
                       ON membership.generation_id = generation.generation_id
                     WHERE generation.smart_folder_id = ?{index}
                       AND generation.state = 'active'
                       AND membership.root_item_id = ri.item_id
                 )"
            )
        }
    };
    Ok(predicate)
}

fn apply_filters(
    connection: &Connection,
    filters: &ItemFilters,
    predicates: &mut Vec<String>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<()> {
    if let Some(text) = filters.text.as_deref().filter(|text| !text.is_empty()) {
        if let Some(query) = crate::predicate_v2::fts_match_query(text) {
            let index = push_argument(arguments, query);
            predicates.push(format!(
                "ri.lifecycle = 'active' AND ri.item_id IN (
                    SELECT CAST(root_name_fts.root_item_id AS INTEGER)
                    FROM root_name_fts
                    WHERE root_name_fts MATCH ?{index}

                    UNION

                    SELECT CAST(root_notes_fts.root_item_id AS INTEGER)
                    FROM root_notes_fts
                    WHERE root_notes_fts MATCH ?{index}

                    UNION

                    SELECT COALESCE(post.root_item_id, member.collection_id, item.media_item_id)
                    FROM source_text_fts
                    JOIN source_post post
                      ON post.source_post_id = source_text_fts.source_post_id
                    LEFT JOIN source_item item
                      ON item.source_post_id = post.source_post_id
                    LEFT JOIN collection_member member
                      ON member.media_item_id = item.media_item_id
                    WHERE source_text_fts MATCH ?{index}
                      AND COALESCE(
                          post.root_item_id, member.collection_id, item.media_item_id
                      ) IS NOT NULL
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
            let threshold_index = push_argument(
                arguments,
                crate::media_processing::colors::FILTER_DELTA_E.powi(2),
            );
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
        "COALESCE((SELECT summary.imported_at FROM root_summary summary WHERE summary.root_item_id = ri.item_id), ri.created_at)",
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
        "COALESCE((SELECT summary.total_size_bytes FROM root_summary summary WHERE summary.root_item_id = ri.item_id), 0)",
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
        "EXISTS (SELECT 1 FROM root_metadata metadata WHERE metadata.root_item_id = ri.item_id AND NULLIF(TRIM(metadata.notes), '') IS NOT NULL)",
        filters.notes_present,
        predicates,
    );
    if let Some(keyword) = nonempty_filter_text(filters.notes_contains.as_deref()) {
        let index = push_argument(arguments, format!("%{keyword}%"));
        predicates.push(format!(
            "EXISTS (SELECT 1 FROM root_metadata metadata WHERE metadata.root_item_id = ri.item_id AND metadata.notes LIKE ?{index})"
        ));
    }
    apply_presence_filter(
        "EXISTS (SELECT 1 FROM root_metadata metadata WHERE metadata.root_item_id = ri.item_id AND json_array_length(metadata.source_urls_json) > 0)",
        filters.source_url_present,
        predicates,
    );
    if let Some(keyword) = nonempty_filter_text(filters.source_url_contains.as_deref()) {
        let index = push_argument(arguments, format!("%{keyword}%"));
        predicates.push(format!(
            "EXISTS (SELECT 1 FROM root_metadata metadata WHERE metadata.root_item_id = ri.item_id AND metadata.source_urls_json LIKE ?{index})"
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
            "COALESCE((SELECT metadata.rating FROM root_metadata metadata
                        WHERE metadata.root_item_id = ri.item_id), 0) IN ({selected})"
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
            .map(|tag| effective_root_tag_predicate(connection, tag, arguments))
            .collect::<rusqlite::Result<Vec<_>>>()?;

        match filters.tag_match_mode {
            FilterMatchMode::Any => {
                predicates.push(format!("({})", effective_matches.join(" OR ")))
            }
            FilterMatchMode::All | FilterMatchMode::Exact => {
                predicates.extend(effective_matches);
                if filters.tag_match_mode == FilterMatchMode::Exact {
                    predicates.push(format!(
                        "(SELECT COUNT(*) FROM root_tag root_tag
                          WHERE root_tag.root_item_id = ri.item_id) = {}",
                        filters.include_tags.len()
                    ));
                }
            }
        }
    }

    for tag in &filters.exclude_tags {
        let effective_match = effective_root_tag_predicate(connection, tag, arguments)?;
        predicates.push(format!("NOT ({effective_match})"));
    }

    Ok(())
}

fn effective_root_tag_predicate(
    connection: &Connection,
    tag: &str,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<String> {
    let (namespace, subtag) = split_tag(tag);
    let tag_ids = crate::tags_v2::effective_query_tag_ids(connection, &namespace, &subtag)?;
    if tag_ids.is_empty() {
        return Ok("0".to_string());
    }
    let placeholders = tag_ids
        .into_iter()
        .map(|tag_id| {
            let index = push_argument(arguments, tag_id);
            format!("?{index}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "ri.item_id IN (
             SELECT root_tag.root_item_id FROM root_tag root_tag
             WHERE root_tag.tag_id IN ({placeholders})
         )"
    ))
}

fn display_file_metric(column: &str) -> String {
    format!(
        "(SELECT mf.{column}
          FROM media_asset ma
          JOIN media_file mf ON mf.file_id = ma.file_id
          WHERE ma.item_id = (
              SELECT summary.cover_media_item_id
              FROM root_summary summary
              WHERE summary.root_item_id = ri.item_id
          ))"
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum CursorKey {
    Integer(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PageCursor {
    version: u8,
    query_hash: i64,
    field: crate::app::ItemSortField,
    direction: crate::app::SortDirection,
    seed: Option<i64>,
    key: CursorKey,
    item_id: i64,
}

struct SortPlan {
    expression: String,
    direction: &'static str,
    field: crate::app::ItemSortField,
    cursor_seed: Option<i64>,
    query_hash: i64,
    key_kind: CursorKeyKind,
}

#[derive(Clone, Copy)]
enum CursorKeyKind {
    Integer,
    Text,
}

impl SortPlan {
    fn for_query(item_query: &ItemQuery, arguments: &mut Vec<Box<dyn ToSql>>) -> Self {
        use crate::app::{ItemSortField, SortDirection};

        let direction = match item_query.sort.direction {
            SortDirection::Ascending => "ASC",
            SortDirection::Descending => "DESC",
        };
        let query_hash = stable_seed(
            &serde_json::to_string(item_query).expect("ItemQuery serialization is infallible"),
        );
        if matches!(item_query.scope, ItemScope::RecentlyViewed)
            && matches!(item_query.sort.field, ItemSortField::ImportedAt)
        {
            return Self {
                expression: "COALESCE(fr.viewed_at, '')".to_string(),
                direction: "DESC",
                field: ItemSortField::ImportedAt,
                cursor_seed: None,
                query_hash,
                key_kind: CursorKeyKind::Text,
            };
        }

        let (expression, cursor_seed, key_kind) = match item_query.sort.field {
            ItemSortField::ImportedAt => (
                "COALESCE(fr.imported_at, '')".to_string(),
                None,
                CursorKeyKind::Text,
            ),
            ItemSortField::CapturedAt => (
                "COALESCE(fr.captured_at, '')".to_string(),
                None,
                CursorKeyKind::Text,
            ),
            ItemSortField::Name => (
                "COALESCE(fr.sort_name, '')".to_string(),
                None,
                CursorKeyKind::Text,
            ),
            ItemSortField::Rating => (
                "COALESCE(fr.sort_rating, -1)".to_string(),
                None,
                CursorKeyKind::Integer,
            ),
            ItemSortField::Size => (
                "fr.total_size_bytes".to_string(),
                None,
                CursorKeyKind::Integer,
            ),
            ItemSortField::Random => {
                let seed = stable_seed(item_query.sort.random_seed.as_deref().unwrap_or_default());
                let index = push_argument(arguments, seed);
                (
                    format!("((fr.item_id * 1103515245 + ?{index}) & 2147483647)"),
                    Some(seed),
                    CursorKeyKind::Integer,
                )
            }
            ItemSortField::FolderOrder => (
                "COALESCE(fr.folder_position, 9223372036854775807)".to_string(),
                None,
                CursorKeyKind::Integer,
            ),
        };
        Self {
            expression,
            direction,
            field: item_query.sort.field.clone(),
            cursor_seed,
            query_hash,
            key_kind,
        }
    }

    fn decode_cursor(&self, encoded: &str) -> rusqlite::Result<PageCursor> {
        if encoded.len() > MAX_CURSOR_LENGTH {
            return Err(invalid_target("Invalid page cursor"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| invalid_target("Invalid page cursor"))?;
        let cursor: PageCursor =
            serde_json::from_slice(&bytes).map_err(|_| invalid_target("Invalid page cursor"))?;
        if cursor.version != 1
            || cursor.query_hash != self.query_hash
            || cursor.field != self.field
            || cursor.direction
                != if self.direction == "ASC" {
                    crate::app::SortDirection::Ascending
                } else {
                    crate::app::SortDirection::Descending
                }
            || cursor.seed != self.cursor_seed
            || !matches!(
                (&cursor.key, self.key_kind),
                (CursorKey::Integer(_), CursorKeyKind::Integer)
                    | (CursorKey::Text(_), CursorKeyKind::Text)
            )
        {
            return Err(invalid_target(
                "Page cursor does not match the requested sort",
            ));
        }
        Ok(cursor)
    }

    fn encode_cursor(&self, key: CursorKey, item_id: i64) -> rusqlite::Result<String> {
        let direction = if self.direction == "ASC" {
            crate::app::SortDirection::Ascending
        } else {
            crate::app::SortDirection::Descending
        };
        let bytes = serde_json::to_vec(&PageCursor {
            version: 1,
            query_hash: self.query_hash,
            field: self.field.clone(),
            direction,
            seed: self.cursor_seed,
            key,
            item_id,
        })
        .map_err(|error| invalid_target(format!("Could not encode page cursor: {error}")))?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }
}

fn stable_seed(value: &str) -> i64 {
    value.bytes().fold(2_166_136_261_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(16_777_619)
    }) as i64
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        details_connection, library_statistics, query, query_for_application, resolve_target_ids,
        selection_summary, selection_summary_for_application, sidebar_counts_for_application,
        ItemPageRequest, SelectionCollectionCandidate,
    };
    use crate::app::{
        Application, FilterMatchMode, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope,
        ItemSort, ItemTarget,
    };
    use crate::canonical_bitmap::{replace_bitmap, BitmapDomain};
    use crate::store::Store;
    use roaring::RoaringBitmap;

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
                insert_media_asset(tx, 11, "member-a", "member-a.jpg", "image/jpeg", 40);
                insert_media_asset(tx, 12, "member-b", "member-b.mp4", "video/mp4", 50);
                insert_collection(tx, 10, "collection", "active", "Album", &[11, 12]);
                // Canonical cover state follows the explicit structural cover.
                tx.execute(
                    "UPDATE library_item SET cover_media_item_id = 12 WHERE item_id = 10",
                    [],
                )?;
                tx.execute("INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at) VALUES (7, 'folder', 'Folder', 'now', 'now')", [])?;
                tx.execute("UPDATE folder SET notes = 'portfolio bucket' WHERE folder_id = 7", [])?;
                tx.execute("INSERT INTO folder_item (folder_id, item_id, position_rank) VALUES (7, 10, 0), (7, 1, 1)", [])?;
                tx.execute("INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'member-tag')", [])?;
                tx.execute("INSERT INTO root_tag (root_item_id, tag_id) VALUES (10, 1)", [])?;
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
                    "UPDATE root_metadata
                     SET notes = 'member annotation', rating = 5,
                         source_urls_json = '[\"https://example.test/member-source\"]'
                     WHERE root_item_id = 10",
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
                    "UPDATE media_file SET pixel_width = 800, pixel_height = 800 WHERE file_id = 12",
                    [],
                )?;
                tx.execute("INSERT INTO media_view (item_id, viewed_at) VALUES (1, '2026-02-01')", [])?;
                crate::canonical_bitmap::seed_test_state(
                    tx,
                    &crate::canonical_bitmap::TestMembership {
                        tags: vec![(1, vec![10])],
                        folders: vec![(7, vec![10, 1])],
                        groups: vec![(10, vec![11, 12])],
                    },
                )?;
                Ok(())
            })
            .unwrap();
        refresh_canonical_search_indexes(&store);
        (directory, store)
    }

    fn refresh_canonical_search_indexes(store: &Store) {
        store
            .transaction_if_changed(|transaction| {
                crate::store::schema::refresh_search_indexes(transaction)?;
                Ok(((), false))
            })
            .unwrap();
    }

    fn insert_collection(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        name: &str,
        member_ids: &[i64],
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
             VALUES (?1, ?2, 'collection', '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key],
        )
        .unwrap();
        for (position, member_id) in member_ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO collection_member (
                     collection_id, media_item_id, position_rank
                 ) VALUES (?1, ?2, ?3)",
                rusqlite::params![item_id, member_id, position as i64],
            )
            .unwrap();
        }
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO root_metadata (
                 root_item_id, name, source_urls_json, updated_at
             ) VALUES (?1, ?2, '[]', '2026-01-01')",
            rusqlite::params![item_id, name],
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
        insert_media_asset(tx, item_id, item_key, name, mime_type, size_bytes);
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO root_metadata (
                 root_item_id, name, rating, source_urls_json, updated_at
             ) VALUES (?1, ?2, ?3, '[]', '2026-01-01')",
            rusqlite::params![item_id, name, rating],
        )
        .unwrap();
    }

    fn insert_media_asset(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        name: &str,
        mime_type: &str,
        size_bytes: i64,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
             VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, '2026-01-01')",
            rusqlite::params![item_id, format!("hash-{item_id}"), mime_type, size_bytes],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_asset (item_id, file_id, name, imported_at, updated_at)
             VALUES (?1, ?1, ?2, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, name],
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
        assert_eq!(collection.display_file_hash.0, "hash-12");
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
    fn application_folder_order_pages_from_canonical_vector() {
        let (_directory, store) = seed_store();
        let application = Application::new(Arc::new(store));
        let mut item_query = query_for(ItemScope::Folder { folder_id: 7 });
        item_query.sort.field = crate::app::ItemSortField::FolderOrder;
        item_query.sort.direction = crate::app::SortDirection::Ascending;

        let first = query_for_application(&application, &item_query, ItemPageRequest::new(None, 1))
            .unwrap();
        let second = query_for_application(
            &application,
            &item_query,
            ItemPageRequest::new(first.next_cursor.clone(), 1),
        )
        .unwrap();
        assert_eq!(first.items[0].item_id, ItemId(10));
        assert_eq!(second.items[0].item_id, ItemId(1));
        assert_eq!(first.visible_item_count, Some(2));
        assert_eq!(second.visible_item_count, None);

        item_query.sort.direction = crate::app::SortDirection::Descending;
        let descending =
            query_for_application(&application, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(
            descending
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1), ItemId(10)]
        );
    }

    #[test]
    fn application_folder_grid_reads_canonical_bitmap_membership() {
        let (_directory, store) = seed_store();
        let application = Application::new(Arc::new(store));
        let folder_id = application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder(folder_key, name, created_at, updated_at)
                     VALUES ('canonical-query-folder', 'Canonical query folder', 'now', 'now')",
                    [],
                )?;
                Ok(transaction.last_insert_rowid())
            })
            .unwrap()
            .0;
        application
            .set_folder_membership(
                &ItemTarget::Explicit {
                    item_ids: vec![ItemId(1), ItemId(10)],
                },
                folder_id,
                true,
            )
            .unwrap();

        let page = query_for_application(
            &application,
            &query_for(ItemScope::Folder { folder_id }),
            ItemPageRequest::default(),
        )
        .unwrap();
        let mut item_ids = page
            .items
            .iter()
            .map(|item| item.item_id)
            .collect::<Vec<_>>();
        item_ids.sort_by_key(|item_id| item_id.0);
        assert_eq!(item_ids, vec![ItemId(1), ItemId(10)]);
        assert_eq!(page.visible_item_count, Some(2));
        assert_eq!(page.visible_media_count, Some(3));

        let mut named_query = query_for(ItemScope::Folder { folder_id });
        named_query.sort.field = crate::app::ItemSortField::Name;
        named_query.sort.direction = crate::app::SortDirection::Ascending;
        let named =
            query_for_application(&application, &named_query, ItemPageRequest::default()).unwrap();
        let mut named_ids = named
            .items
            .iter()
            .map(|item| item.item_id)
            .collect::<Vec<_>>();
        named_ids.sort_by_key(|item_id| item_id.0);
        assert_eq!(named_ids, vec![ItemId(1), ItemId(10)]);
        application
            .store()
            .read(|connection| {
                let legacy_rows: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id = ?1",
                    [folder_id],
                    |row| row.get(0),
                )?;
                assert_eq!(legacy_rows, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn application_text_search_composes_with_canonical_organization() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE projection_write_control
                     SET suppress_root_summary = 1 WHERE singleton = 1",
                    [],
                )?;
                transaction.execute("DELETE FROM collection_member", [])?;
                transaction.execute(
                    "UPDATE root_summary
                     SET imported_at = '2026-02-10T00:00:00Z',
                         updated_at = '2026-04-10T00:00:00Z'
                     WHERE root_item_id = 10",
                    [],
                )?;
                transaction.execute(
                    "UPDATE projection_write_control
                     SET suppress_root_summary = 0 WHERE singleton = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        refresh_canonical_search_indexes(&store);
        let application = Application::new(Arc::new(store));
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE source_post SET root_item_id = NULL WHERE source_post_id = 1",
                    [],
                )?;
                transaction.execute("DELETE FROM root_tag", [])?;
                transaction.execute("DELETE FROM folder_item", [])?;
                Ok(())
            })
            .unwrap();

        let mut item_query = query_for(ItemScope::Folder { folder_id: 7 });
        item_query.filters.text = Some("member-b".to_string());
        item_query.filters.include_tags = vec!["general:member-tag".to_string()];
        item_query.filters.min_width = Some(800);
        item_query.filters.max_width = Some(800);
        item_query.filters.min_height = Some(800);
        item_query.filters.max_height = Some(800);
        item_query.filters.imported_after = Some("2026-02-01T00:00:00Z".into());
        item_query.filters.imported_before = Some("2026-03-01T00:00:00Z".into());
        item_query.filters.modified_after = Some("2026-04-01T00:00:00Z".into());
        item_query.sort.field = crate::app::ItemSortField::Name;
        let page =
            query_for_application(&application, &item_query, ItemPageRequest::default()).unwrap();

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
    fn dense_projected_sort_scans_covering_indexes_without_membership_sql() {
        use crate::app::ItemSortField;

        let (_directory, store) = seed_store();
        let roots = RoaringBitmap::from_iter([1, 10]);
        store
            .read(|connection| {
                for field in [
                    ItemSortField::CapturedAt,
                    ItemSortField::Name,
                    ItemSortField::Rating,
                    ItemSortField::Size,
                ] {
                    let mut item_query = query_for(ItemScope::All);
                    item_query.sort.field = field;
                    let ordered = super::ordered_dense_projected_roots_by_sort(
                        connection,
                        &item_query,
                        &roots,
                        &ItemPageRequest::default(),
                    )?;
                    assert_eq!(ordered.len(), 2);
                    assert!(ordered.iter().all(|(item_id, _)| [1, 10].contains(item_id)));
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn folder_imported_page_uses_exact_incremental_totals() {
        let (_directory, store) = seed_store();
        let query = query_for(ItemScope::Folder { folder_id: 7 });
        let counts = |store: &Store| {
            let page = super::query(store, &query, ItemPageRequest::default()).unwrap();
            (page.visible_item_count, page.visible_media_count)
        };

        assert_eq!(counts(&store), (Some(2), Some(3)));
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(counts(&store), (Some(1), Some(2)));

        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE library_root SET lifecycle = 'active' WHERE item_id = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(counts(&store), (Some(2), Some(3)));

        store
            .transaction(|transaction| {
                transaction.execute(
                    "DELETE FROM folder_item WHERE folder_id = 7 AND item_id = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(counts(&store), (Some(1), Some(2)));

        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder_item(folder_id, item_id) VALUES (7, 1)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(counts(&store), (Some(2), Some(3)));
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
                     SET duration_ms = CASE file_id WHEN 1 THEN 5000 WHEN 12 THEN 10000 END
                     WHERE file_id IN (1, 12)",
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
    fn text_search_covers_media_and_source_facts_but_not_structured_taxonomies() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET name = 'hidden member name' WHERE item_id = 11",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        refresh_canonical_search_indexes(&store);
        let mut item_query = query_for(ItemScope::All);
        for (text, expected) in [
            ("one.jpg", vec![ItemId(1)]),
            ("Album", vec![ItemId(10)]),
            ("Source Creator", vec![ItemId(10)]),
            ("Source Title", vec![ItemId(10)]),
            ("Source Description", vec![ItemId(10)]),
            ("cdn.example", vec![ItemId(10)]),
            ("member annotation", vec![ItemId(10)]),
            ("member-tag", vec![]),
            ("hidden member name", vec![ItemId(10)]),
            ("Folder", vec![]),
            ("portfolio bucket", vec![]),
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
    fn text_search_never_projects_inbox_or_trash_roots() {
        let (_directory, store) = seed_store();
        refresh_canonical_search_indexes(&store);

        for (scope, text) in [(ItemScope::Inbox, "inbox"), (ItemScope::Trash, "trash")] {
            let mut item_query = query_for(scope);
            item_query.filters.text = Some(text.to_string());
            assert!(query(&store, &item_query, ItemPageRequest::default())
                .unwrap()
                .items
                .is_empty());
        }
    }

    #[test]
    fn text_search_indexes_follow_media_and_source_renames_only() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET name = 'renamed asset' WHERE item_id = 1",
                    [],
                )?;
                transaction.execute(
                    "UPDATE root_metadata SET name = 'renamed asset' WHERE root_item_id = 1",
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
        refresh_canonical_search_indexes(&store);

        for (text, expected) in [
            ("renamed asset", vec![ItemId(1)]),
            ("Renamed Person", vec![ItemId(10)]),
            ("renamed-tag", vec![]),
            ("Renamed Folder", vec![]),
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
        let application = Application::new(Arc::new(store));
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.color_hex = Some("#ABCDEF".to_string());
        let page =
            query_for_application(&application, &item_query, ItemPageRequest::default()).unwrap();

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
        let application = Application::new(Arc::new(store));
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.color_hex = Some("#2f9f4b".to_string());

        let page =
            query_for_application(&application, &item_query, ItemPageRequest::default()).unwrap();

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
        assert_eq!(page.revision, 2);

        let first = query(
            &store,
            &query_for(ItemScope::All),
            ItemPageRequest::new(None, 1),
        )
        .unwrap();
        let append = query(
            &store,
            &query_for(ItemScope::All),
            ItemPageRequest::new(first.next_cursor, 1),
        )
        .unwrap();
        assert_eq!(append.items.len(), 1);
        assert_eq!(append.visible_item_count, None);
        assert_eq!(append.visible_media_count, None);
        assert_eq!(append.total_size_bytes, None);
        assert_eq!(append.revision, 2);
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
                    "INSERT INTO root_tag (root_item_id, tag_id) VALUES
                     (1, 5), (10, 5), (10, 6)",
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
    fn every_stable_sort_pages_by_opaque_cursor_without_duplicates() {
        use crate::app::{ItemSortField, SortDirection};

        let (_directory, store) = seed_store();
        for field in [
            ItemSortField::ImportedAt,
            ItemSortField::CapturedAt,
            ItemSortField::Name,
            ItemSortField::Rating,
            ItemSortField::Size,
            ItemSortField::Random,
            ItemSortField::FolderOrder,
        ] {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                let mut item_query = query_for(ItemScope::All);
                item_query.sort.field = field.clone();
                item_query.sort.direction = direction;
                item_query.sort.random_seed = Some("cursor-seed".to_string());
                let first = query(&store, &item_query, ItemPageRequest::new(None, 1)).unwrap();
                let cursor = first.next_cursor.clone().expect("a second page");
                assert!(!cursor.contains('{'), "cursor must be opaque");
                let second =
                    query(&store, &item_query, ItemPageRequest::new(Some(cursor), 1)).unwrap();

                assert_eq!(first.visible_item_count, Some(2));
                assert_eq!(second.visible_item_count, None);
                assert_ne!(first.items[0].item_id, second.items[0].item_id);
                assert!(second.next_cursor.is_none());
            }
        }
    }

    #[test]
    fn cursor_cannot_be_reused_with_another_query() {
        let (_directory, store) = seed_store();
        let first_query = query_for(ItemScope::All);
        let first = query(&store, &first_query, ItemPageRequest::new(None, 1)).unwrap();
        let mut changed_query = first_query;
        changed_query.filters.text = Some("Item".to_string());

        let error = query(
            &store,
            &changed_query,
            ItemPageRequest::new(first.next_cursor, 1),
        )
        .unwrap_err();
        assert!(error.contains("cursor does not match"));
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
                activate_smart_folder(transaction, 9, &[10])?;
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

        let application = Application::new(Arc::new(store));
        let projected = query_for_application(
            &application,
            &query_for(ItemScope::SmartFolder { smart_folder_id: 9 }),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(projected.items, page.items);
        assert_eq!(projected.visible_item_count, page.visible_item_count);
        assert_eq!(projected.visible_media_count, page.visible_media_count);
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
    fn range_target_uses_stable_query_order_and_includes_unloaded_items() {
        use crate::app::{ItemSortField, SortDirection};

        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                for (item_id, name) in [(20, "range-a"), (21, "range-b"), (22, "range-c")] {
                    insert_media(
                        transaction,
                        item_id,
                        &format!("range-{item_id}"),
                        "active",
                        name,
                        "image/jpeg",
                        10,
                        Some(4),
                    );
                }
                Ok(())
            })
            .unwrap();

        let mut range_query = query_for(ItemScope::All);
        range_query.filters.ratings = vec![4];
        range_query.sort.field = ItemSortField::Name;
        range_query.sort.direction = SortDirection::Ascending;
        let first_page = query(&store, &range_query, ItemPageRequest::new(None, 1)).unwrap();
        assert_eq!(first_page.items[0].item_id, ItemId(20));

        let target = ItemTarget::Range {
            query: range_query.clone(),
            anchor_item_id: ItemId(20),
            focus_item_id: ItemId(22),
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![20, 21, 22]);
        assert_eq!(
            selection_summary(&store, &target).unwrap().selected_count,
            3
        );

        let reversed = ItemTarget::Range {
            query: range_query,
            anchor_item_id: ItemId(22),
            focus_item_id: ItemId(20),
        };
        let reversed_ids = store
            .read(|connection| resolve_target_ids(connection, &reversed))
            .unwrap();
        assert_eq!(reversed_ids, ids);
    }

    #[test]
    fn range_target_respects_descending_ties_and_requires_both_query_endpoints() {
        use crate::app::{ItemSortField, SortDirection};

        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                for item_id in [20, 21, 22] {
                    insert_media(
                        transaction,
                        item_id,
                        &format!("range-{item_id}"),
                        "active",
                        "same-name",
                        "image/jpeg",
                        10,
                        Some(4),
                    );
                }
                Ok(())
            })
            .unwrap();

        let mut range_query = query_for(ItemScope::All);
        range_query.filters.ratings = vec![4];
        range_query.sort.field = ItemSortField::Name;
        range_query.sort.direction = SortDirection::Descending;
        let target = ItemTarget::Range {
            query: range_query.clone(),
            anchor_item_id: ItemId(20),
            focus_item_id: ItemId(22),
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![20, 21, 22]);

        let one = ItemTarget::Range {
            query: range_query.clone(),
            anchor_item_id: ItemId(21),
            focus_item_id: ItemId(21),
        };
        assert_eq!(
            store
                .read(|connection| resolve_target_ids(connection, &one))
                .unwrap(),
            vec![21]
        );

        range_query.filters.ratings = vec![5];
        let outside_query = ItemTarget::Range {
            query: range_query,
            anchor_item_id: ItemId(20),
            focus_item_id: ItemId(22),
        };
        assert!(store
            .read(|connection| resolve_target_ids(connection, &outside_query))
            .unwrap()
            .is_empty());
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
        let projection = store
            .read(|connection| {
                crate::projection_v2::ProjectionStore::from_connection(connection)
                    .map_err(rusqlite::Error::InvalidParameterName)
            })
            .unwrap();
        let revision = store.revision().unwrap();
        let details = store
            .read(|connection| {
                details_connection(connection, 10, &projection.selection_snapshot(), revision)
            })
            .unwrap();

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
        assert_eq!(details.media[1].tags, vec!["member-tag"]);
        assert!(details
            .media
            .iter()
            .all(|media| media.notes.as_deref() == Some("member annotation")));
        assert!(details.media.iter().all(|media| media.rating == Some(5)));
        assert!(details.media.iter().all(|media| {
            media.source_urls == vec!["https://example.test/member-source".to_string()]
        }));
    }

    #[test]
    fn root_owned_metadata_wins_over_attached_media_organization() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset
                     SET name = 'obsolete media name'
                     WHERE item_id IN (11, 12)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let projection = store
            .read(|connection| {
                crate::projection_v2::ProjectionStore::from_connection(connection)
                    .map_err(rusqlite::Error::InvalidParameterName)
            })
            .unwrap();
        let revision = store.revision().unwrap();
        let details = store
            .read(|connection| {
                details_connection(connection, 10, &projection.selection_snapshot(), revision)
            })
            .unwrap();
        assert!(details
            .media
            .iter()
            .all(|media| media.notes.as_deref() == Some("member annotation")));
        assert!(details.media.iter().all(|media| media.rating == Some(5)));
        assert!(details.media.iter().all(|media| {
            media.source_urls == vec!["https://example.test/member-source".to_string()]
        }));

        let mut query = query_for(ItemScope::All);
        query.filters.ratings = vec![1];
        assert!(super::query(&store, &query, ItemPageRequest::default())
            .unwrap()
            .items
            .is_empty());
        query.filters.ratings = vec![5];
        assert_eq!(
            super::query(&store, &query, ItemPageRequest::default())
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
    }

    #[test]
    fn inbox_and_trash_root_tags_remain_stored_but_do_not_enter_active_counts() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO root_tag (root_item_id, tag_id) VALUES (2, 1), (3, 1)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let tagged_ids = |scope| {
            let mut query = query_for(scope);
            query.filters.include_tags = vec!["member-tag".to_string()];
            super::query(&store, &query, ItemPageRequest::default())
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(tagged_ids(ItemScope::All), vec![ItemId(10)]);
        assert_eq!(tagged_ids(ItemScope::Inbox), vec![ItemId(2)]);
        assert_eq!(tagged_ids(ItemScope::Trash), vec![ItemId(3)]);
    }

    #[test]
    fn explicit_and_query_selection_summaries_share_root_owned_results() {
        let (_directory, store) = seed_store();
        let explicit = selection_summary(
            &store,
            &ItemTarget::Explicit {
                item_ids: vec![ItemId(1), ItemId(10)],
            },
        )
        .unwrap();
        let query = selection_summary(
            &store,
            &ItemTarget::Query {
                query: query_for(ItemScope::All),
                excluded_item_ids: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(explicit.selected_count, query.selected_count);
        assert_eq!(explicit.shared_tags, query.shared_tags);
        assert_eq!(explicit.top_tags, query.top_tags);
        assert_eq!(explicit.shared_folders, query.shared_folders);
        assert_eq!(explicit.shared_notes, query.shared_notes);
        assert_eq!(explicit.shared_source_urls, query.shared_source_urls);
        assert_eq!(explicit.stats, query.stats);
    }

    #[test]
    fn application_selection_uses_group_aware_bitmap_mime_predicates() {
        let (_directory, store) = seed_store();
        let application = Application::new(Arc::new(store));

        let mut images = query_for(ItemScope::All);
        images.filters.include_mime_types = vec!["image/*".to_string()];
        let first_page =
            query_for_application(&application, &images, ItemPageRequest::new(None, 1)).unwrap();
        let second_page = query_for_application(
            &application,
            &images,
            ItemPageRequest::new(first_page.next_cursor.clone(), 1),
        )
        .unwrap();
        let mut grid_ids = first_page
            .items
            .iter()
            .chain(&second_page.items)
            .map(|item| item.item_id)
            .collect::<Vec<_>>();
        grid_ids.sort_by_key(|item_id| item_id.0);
        assert_eq!(grid_ids, vec![ItemId(1), ItemId(10)]);
        assert_eq!(first_page.visible_item_count, Some(2));
        assert_eq!(first_page.visible_media_count, Some(3));
        assert_eq!(second_page.visible_item_count, None);

        let image_summary = selection_summary_for_application(
            &application,
            &ItemTarget::Query {
                query: images.clone(),
                excluded_item_ids: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(image_summary.selected_count, 2);

        let mut without_video = query_for(ItemScope::All);
        without_video.filters.exclude_mime_types = vec!["video/mp4".to_string()];
        let non_video_grid =
            query_for_application(&application, &without_video, ItemPageRequest::default())
                .unwrap();
        assert_eq!(non_video_grid.items[0].item_id, ItemId(1));
        assert_eq!(non_video_grid.visible_item_count, Some(1));

        let non_video_summary = selection_summary_for_application(
            &application,
            &ItemTarget::Query {
                query: without_video,
                excluded_item_ids: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(
            non_video_summary.selected_count, 1,
            "a group is excluded when any member matches the excluded MIME"
        );

        let mut large = query_for(ItemScope::All);
        large.filters.min_size_bytes = Some(80);
        large.filters.max_size_bytes = Some(100);
        let large_grid =
            query_for_application(&application, &large, ItemPageRequest::default()).unwrap();
        assert_eq!(
            large_grid
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
        assert_eq!(large_grid.total_size_bytes, Some(90));
    }

    #[test]
    fn selection_summary_uses_the_same_root_and_media_counts_as_grid() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE root_metadata
                     SET notes = CASE WHEN root_item_id = 10 THEN 'different' ELSE 'shared note' END,
                         source_urls_json = CASE WHEN root_item_id = 10
                             THEN '[\"https://example.com/two\"]'
                             ELSE '[\"https://example.com/one\"]' END
                     WHERE root_item_id IN (1, 10)",
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
        assert!(!summary.stats.all_media_are_images);
        assert_eq!(summary.stats.total_size_bytes, Some(100));
        assert!(summary.shared_tags.is_empty());
        assert_eq!(summary.top_tags[0].tag, "member-tag");
        assert_eq!(summary.top_tags[0].count, 1);
        assert_eq!(summary.stats.rating_stats.min, Some(2));
        assert_eq!(summary.stats.rating_stats.max, Some(5));
        assert_eq!(summary.stats.rating_stats.shared, None);
        assert!(summary.has_notes);
        assert_eq!(summary.shared_notes, None);
        assert!(summary.has_source_urls);
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
        assert_eq!(summary.revision, 3);

        let excluded = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: vec![ItemId(1)],
        };
        let excluded_summary = selection_summary(&store, &excluded).unwrap();
        assert_eq!(excluded_summary.selected_count, 1);
        assert_eq!(excluded_summary.stats.total_size_bytes, Some(90));
        assert!(!excluded_summary.stats.all_media_are_images);

        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO root_tag (root_item_id, tag_id) VALUES (1, 1)",
                    [],
                )?;
                transaction.execute(
                    "UPDATE root_metadata
                     SET notes = 'shared note',
                         source_urls_json = '[\"https://example.com/one\"]'
                     WHERE root_item_id IN (1, 10)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let summary = selection_summary(&store, &target).unwrap();
        assert_eq!(summary.shared_tags[0].tag, "member-tag");
        assert_eq!(summary.shared_tags[0].count, 2);
        assert_eq!(summary.shared_notes.as_deref(), Some("shared note"));
        assert_eq!(
            summary.shared_source_urls,
            Some(vec!["https://example.com/one".to_string()])
        );

        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE root_metadata
                     SET notes = CASE WHEN root_item_id = 1 THEN NULL ELSE 'shared note' END,
                         source_urls_json = CASE WHEN root_item_id = 1
                             THEN '[]'
                             ELSE '[\"https://example.com/one\"]' END
                     WHERE root_item_id IN (1, 10)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let summary = selection_summary(&store, &target).unwrap();
        assert!(summary.has_notes);
        assert_eq!(summary.shared_notes, None);
        assert!(summary.has_source_urls);
        assert_eq!(summary.shared_source_urls, None);

        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE root_metadata
                     SET notes = CASE WHEN root_item_id = 1 THEN 'one' ELSE 'two' END,
                         source_urls_json = '[\"https://example.com/shared\"]'
                     WHERE root_item_id IN (1, 10)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let summary = selection_summary(&store, &target).unwrap();
        assert_eq!(summary.shared_notes, None);
        assert_eq!(
            summary.shared_source_urls,
            Some(vec!["https://example.com/shared".to_string()])
        );

        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE root_metadata
                     SET notes = 'shared note',
                         source_urls_json = CASE WHEN root_item_id = 1
                             THEN '[\"https://example.com/one\"]'
                             ELSE '[\"https://example.com/two\"]' END
                     WHERE root_item_id IN (1, 10)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let summary = selection_summary(&store, &target).unwrap();
        assert_eq!(summary.shared_notes.as_deref(), Some("shared note"));
        assert_eq!(summary.shared_source_urls, None);
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
                crate::app::FileHash("hash-12".to_string()),
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
                activate_smart_folder(transaction, 9, &[10])?;
                Ok(())
            })
            .unwrap();

        let application = crate::app::Application::try_new(std::sync::Arc::new(store)).unwrap();
        let counts = sidebar_counts_for_application(&application).unwrap();
        assert_eq!(counts.all, 2);
        assert_eq!(counts.inbox, 1);
        assert_eq!(counts.trash, 1);
        assert_eq!(counts.recently_viewed, 1);
        assert_eq!(counts.untagged, 1);
        assert_eq!(counts.uncategorized, 0);
        assert_eq!(counts.folders[0].count, 2);
        assert_eq!(counts.smart_folders[0].count, 1);
        assert_eq!(counts.revision, 3);
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
        assert_eq!(statistics.revision, 3);
    }

    fn activate_smart_folder(
        transaction: &rusqlite::Transaction<'_>,
        smart_folder_id: i64,
        root_item_ids: &[i64],
    ) -> rusqlite::Result<()> {
        transaction.execute(
            "UPDATE smart_folder_generation
             SET state = 'active', activated_at = 'now'
             WHERE smart_folder_id = ?1 AND state = 'building'",
            [smart_folder_id],
        )?;
        for root_item_id in root_item_ids {
            transaction.execute(
                "INSERT INTO smart_folder_membership (generation_id, root_item_id)
                 SELECT generation_id, ?2
                 FROM smart_folder_generation
                 WHERE smart_folder_id = ?1 AND state = 'active'",
                rusqlite::params![smart_folder_id, root_item_id],
            )?;
        }
        let members = root_item_ids
            .iter()
            .map(|root_item_id| u32::try_from(*root_item_id).unwrap())
            .collect::<RoaringBitmap>();
        replace_bitmap(
            transaction,
            BitmapDomain::SmartFolder,
            smart_folder_id,
            3,
            &members,
        )?;
        Ok(())
    }
}
