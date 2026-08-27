//! Shared bitmap predicate compiler for grids and smart folders.
//!
//! This module only accepts predicates that can be answered exactly by the
//! immutable projection snapshot. Callers keep the existing indexed SQL path
//! for text and derivative predicates until their bitmap components exist.

use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::app::{FilterMatchMode, ItemFilters, ItemQuery, ItemScope, Lifecycle};
use crate::projection_v2::ProjectionSelectionSnapshot;

pub(crate) fn compile_item_query(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
    query: &ItemQuery,
) -> rusqlite::Result<Option<RoaringBitmap>> {
    if has_sql_only_filters(&query.filters) {
        return Ok(None);
    }

    let active_scope = matches!(
        query.scope,
        ItemScope::All | ItemScope::Untagged | ItemScope::Uncategorized | ItemScope::Folder { .. }
    );
    if !active_scope
        && (!query.filters.include_tags.is_empty()
            || !query.filters.exclude_tags.is_empty()
            || !query.filters.include_folder_ids.is_empty()
            || !query.filters.exclude_folder_ids.is_empty())
    {
        return Ok(None);
    }

    let mut roots = match query.scope {
        ItemScope::All => projection.lifecycle_bitmap(Lifecycle::Active),
        ItemScope::Inbox => projection.lifecycle_bitmap(Lifecycle::Inbox),
        ItemScope::Trash => projection.lifecycle_bitmap(Lifecycle::Trash),
        ItemScope::Untagged => projection.untagged_bitmap(),
        ItemScope::Uncategorized => projection.uncategorized_bitmap(),
        ItemScope::Folder { folder_id } => projection.folder_bitmap(folder_id),
        ItemScope::SmartFolder { smart_folder_id } => {
            projection.smart_folder_bitmap(smart_folder_id)
        }
        ItemScope::RecentlyViewed => return Ok(None),
    };

    apply_folder_filters(projection, &query.filters, &mut roots);
    apply_rating_filters(projection, &query.filters, &mut roots);
    apply_mime_filters(projection, &query.filters, &mut roots);
    apply_tag_filters(connection, projection, &query.filters, &mut roots)?;
    Ok(Some(roots))
}

fn apply_folder_filters(
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) {
    if !filters.include_folder_ids.is_empty() {
        let included = combine(
            filters
                .include_folder_ids
                .iter()
                .map(|folder_id| projection.folder_bitmap(*folder_id)),
            filters.folder_match_mode == FilterMatchMode::Any,
        );
        *roots &= included;
        if filters.folder_match_mode == FilterMatchMode::Exact {
            projection.retain_exact_folders(roots, filters.include_folder_ids.len());
        }
    }
    for folder_id in &filters.exclude_folder_ids {
        *roots -= projection.folder_bitmap(*folder_id);
    }
}

fn apply_rating_filters(
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) {
    if filters.ratings.is_empty() {
        return;
    }
    let selected = combine(
        filters
            .ratings
            .iter()
            .map(|rating| projection.rating_bitmap(*rating)),
        true,
    );
    *roots &= selected;
}

fn apply_mime_filters(
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) {
    if !filters.include_mime_types.is_empty() {
        let included = combine(
            filters
                .include_mime_types
                .iter()
                .map(|mime| mime_bitmap(projection, mime)),
            true,
        );
        *roots &= included;
    }
    for mime in &filters.exclude_mime_types {
        *roots -= mime_bitmap(projection, mime);
    }
}

fn mime_bitmap(projection: &ProjectionSelectionSnapshot, mime: &str) -> RoaringBitmap {
    if mime.ends_with("/*") || !mime.contains('/') {
        projection.mime_family_bitmap(mime)
    } else {
        projection.mime_bitmap(mime)
    }
}

fn apply_tag_filters(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) -> rusqlite::Result<()> {
    let include_tag_ids = resolve_tag_ids(connection, &filters.include_tags)?;
    if !filters.include_tags.is_empty() {
        if include_tag_ids.len() != filters.include_tags.len() {
            roots.clear();
            return Ok(());
        }
        let included = combine(
            include_tag_ids
                .iter()
                .map(|tag_id| projection.tag_bitmap(*tag_id)),
            filters.tag_match_mode == FilterMatchMode::Any,
        );
        *roots &= included;
        if filters.tag_match_mode == FilterMatchMode::Exact {
            let mut exact = include_tag_ids;
            exact.sort_unstable();
            exact.dedup();
            projection.retain_exact_tags(roots, &exact);
        }
    }
    for tag_id in resolve_tag_ids(connection, &filters.exclude_tags)? {
        *roots -= projection.tag_bitmap(tag_id);
    }
    Ok(())
}

fn resolve_tag_ids(connection: &Connection, names: &[String]) -> rusqlite::Result<Vec<i64>> {
    names
        .iter()
        .filter_map(|name| {
            let (namespace, subtag) = split_tag(name);
            connection
                .query_row(
                    "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                    rusqlite::params![namespace, subtag],
                    |row| row.get(0),
                )
                .optional()
                .transpose()
        })
        .collect()
}

fn combine(bitmaps: impl IntoIterator<Item = RoaringBitmap>, union: bool) -> RoaringBitmap {
    let mut bitmaps = bitmaps.into_iter();
    let Some(mut result) = bitmaps.next() else {
        return RoaringBitmap::new();
    };
    for bitmap in bitmaps {
        if union {
            result |= bitmap;
        } else {
            result &= bitmap;
        }
    }
    result
}

fn has_sql_only_filters(filters: &ItemFilters) -> bool {
    filters.text.is_some()
        || filters.color_hex.is_some()
        || filters.imported_after.is_some()
        || filters.imported_before.is_some()
        || filters.modified_after.is_some()
        || filters.modified_before.is_some()
        || filters.min_duration_ms.is_some()
        || filters.max_duration_ms.is_some()
        || filters.min_size_bytes.is_some()
        || filters.max_size_bytes.is_some()
        || filters.min_width.is_some()
        || filters.max_width.is_some()
        || filters.min_height.is_some()
        || filters.max_height.is_some()
        || filters.notes_present.is_some()
        || filters.notes_contains.is_some()
        || filters.source_url_present.is_some()
        || filters.source_url_contains.is_some()
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

use rusqlite::OptionalExtension;
