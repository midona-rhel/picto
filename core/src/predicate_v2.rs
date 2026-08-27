//! Shared bitmap predicate compiler for grids and smart folders.
//!
//! This module only accepts predicates that can be answered exactly by the
//! immutable projection snapshot. FTS is resolved to a root bitmap before it
//! composes with structured predicates; derivative predicates retain the
//! existing indexed SQL path until their bitmap components exist.

use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::app::{FilterMatchMode, ItemFilters, ItemQuery, ItemScope, Lifecycle};
use crate::projection_v2::{timestamp_ms, ProjectionSelectionSnapshot};

pub(crate) fn compile_item_query(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
    query: &ItemQuery,
) -> rusqlite::Result<RoaringBitmap> {
    let mut roots = match query.scope {
        ItemScope::All => projection.lifecycle_bitmap(Lifecycle::Active),
        ItemScope::Inbox => projection.lifecycle_bitmap(Lifecycle::Inbox),
        ItemScope::Trash => projection.lifecycle_bitmap(Lifecycle::Trash),
        ItemScope::Untagged => projection.untagged_bitmap(),
        ItemScope::Uncategorized => projection.uncategorized_bitmap(),
        ItemScope::Folder { folder_id } => {
            // Trash and Inbox retain folder organization; the visible folder
            // scope is membership intersected with the active lifecycle.
            projection.folder_bitmap(folder_id) & projection.lifecycle_bitmap(Lifecycle::Active)
        }
        ItemScope::SmartFolder { smart_folder_id } => {
            projection.smart_folder_bitmap(smart_folder_id)
        }
        ItemScope::RecentlyViewed => recently_viewed_bitmap(connection, projection)?,
    };

    apply_folder_filters(projection, &query.filters, &mut roots);
    apply_rating_filters(projection, &query.filters, &mut roots);
    apply_size_filters(projection, &query.filters, &mut roots);
    apply_display_metric_filters(projection, &query.filters, &mut roots);
    apply_timestamp_filters(projection, &query.filters, &mut roots);
    apply_color_filter(connection, projection, &query.filters, &mut roots)?;
    apply_mime_filters(projection, &query.filters, &mut roots);
    apply_metadata_text_filters(connection, &query.filters, &mut roots)?;
    apply_tag_filters(connection, projection, &query.filters, &mut roots)?;
    apply_text_filter(connection, projection, &query.filters, &mut roots)?;
    Ok(roots)
}

fn recently_viewed_bitmap(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
) -> rusqlite::Result<RoaringBitmap> {
    let viewed = connection
        .prepare_cached("SELECT item_id FROM media_view")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .filter_map(|item_id| match item_id {
            Ok(item_id) => u32::try_from(item_id).ok().map(Ok),
            Err(error) => Some(Err(error)),
        })
        .collect::<rusqlite::Result<RoaringBitmap>>()?;
    Ok(viewed & projection.lifecycle_bitmap(Lifecycle::Active))
}

/// Notes and source-URL predicates read root_metadata (a permitted indexed
/// table) and intersect the result as a bitmap.
fn apply_metadata_text_filters(
    connection: &Connection,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) -> rusqlite::Result<()> {
    if let Some(present) = filters.notes_present {
        let matches = metadata_bitmap(
            connection,
            "SELECT root_item_id FROM root_metadata
             WHERE NULLIF(TRIM(notes), '') IS NOT NULL",
            None,
        )?;
        if present {
            *roots &= matches;
        } else {
            *roots -= matches;
        }
    }
    if let Some(keyword) = nonempty_text(filters.notes_contains.as_deref()) {
        let matches = metadata_bitmap(
            connection,
            "SELECT root_item_id FROM root_metadata WHERE notes LIKE ?1",
            Some(format!("%{keyword}%")),
        )?;
        *roots &= matches;
    }
    if let Some(present) = filters.source_url_present {
        let matches = metadata_bitmap(
            connection,
            "SELECT root_item_id FROM root_metadata
             WHERE json_array_length(source_urls_json) > 0",
            None,
        )?;
        if present {
            *roots &= matches;
        } else {
            *roots -= matches;
        }
    }
    if let Some(keyword) = nonempty_text(filters.source_url_contains.as_deref()) {
        let matches = metadata_bitmap(
            connection,
            "SELECT root_item_id FROM root_metadata WHERE source_urls_json LIKE ?1",
            Some(format!("%{keyword}%")),
        )?;
        *roots &= matches;
    }
    Ok(())
}

fn metadata_bitmap(
    connection: &Connection,
    sql: &str,
    argument: Option<String>,
) -> rusqlite::Result<RoaringBitmap> {
    let mut statement = connection.prepare_cached(sql)?;
    let map = |row: &rusqlite::Row<'_>| row.get::<_, i64>(0);
    let rows = match argument {
        Some(argument) => statement.query_map([argument], map)?,
        None => statement.query_map([], map)?,
    };
    rows.filter_map(|root_id| match root_id {
        Ok(root_id) => u32::try_from(root_id).ok().map(Ok),
        Err(error) => Some(Err(error)),
    })
    .collect()
}

fn nonempty_text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn apply_text_filter(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) -> rusqlite::Result<()> {
    let Some(text) = filters.text.as_deref().filter(|text| !text.is_empty()) else {
        return Ok(());
    };
    let Some(query) = fts_match_query(text) else {
        roots.clear();
        return Ok(());
    };

    let mut matches = RoaringBitmap::new();
    let mut root_statement = connection.prepare_cached(
        "SELECT CAST(root_name_fts.root_item_id AS INTEGER)
         FROM root_name_fts WHERE root_name_fts MATCH ?1
         UNION
         SELECT CAST(root_notes_fts.root_item_id AS INTEGER)
         FROM root_notes_fts WHERE root_notes_fts MATCH ?1",
    )?;
    let root_ids = root_statement.query_map([&query], |row| row.get::<_, i64>(0))?;
    for root_id in root_ids {
        if let Ok(root_id) = u32::try_from(root_id?) {
            matches.insert(root_id);
        }
    }

    let mut source_statement = connection.prepare_cached(
        "SELECT post.root_item_id, item.media_item_id
         FROM source_text_fts
         JOIN source_post post
           ON post.source_post_id = source_text_fts.source_post_id
         LEFT JOIN source_item item
           ON item.source_post_id = post.source_post_id
         WHERE source_text_fts MATCH ?1",
    )?;
    let source_roots = source_statement.query_map([&query], |row| {
        Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
    })?;
    for source_root in source_roots {
        let (root_id, media_id) = source_root?;
        let root_id = root_id.or_else(|| media_id.and_then(|id| projection.root_for_media(id)));
        if let Some(root_id) = root_id.and_then(|id| u32::try_from(id).ok()) {
            matches.insert(root_id);
        }
    }

    matches &= projection.lifecycle_bitmap(Lifecycle::Active);
    *roots &= matches;
    Ok(())
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

fn apply_size_filters(
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) {
    if filters.min_size_bytes.is_none() && filters.max_size_bytes.is_none() {
        return;
    }
    if filters.max_size_bytes.is_some_and(|maximum| maximum < 0) {
        roots.clear();
        return;
    }
    let minimum = filters
        .min_size_bytes
        .filter(|minimum| *minimum > 0)
        .map(|minimum| minimum as u64);
    let maximum = filters.max_size_bytes.map(|maximum| maximum as u64);
    *roots = projection.total_size_range_bitmap(minimum, maximum, roots);
}

fn apply_display_metric_filters(
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) {
    apply_nonnegative_range(
        filters.min_duration_ms,
        filters.max_duration_ms,
        roots,
        |minimum, maximum, universe| {
            projection.display_duration_range_bitmap(minimum, maximum, universe)
        },
    );
    apply_nonnegative_range(
        filters.min_width,
        filters.max_width,
        roots,
        |minimum, maximum, universe| {
            projection.display_width_range_bitmap(minimum, maximum, universe)
        },
    );
    apply_nonnegative_range(
        filters.min_height,
        filters.max_height,
        roots,
        |minimum, maximum, universe| {
            projection.display_height_range_bitmap(minimum, maximum, universe)
        },
    );
}

fn apply_nonnegative_range(
    minimum: Option<i64>,
    maximum: Option<i64>,
    roots: &mut RoaringBitmap,
    apply: impl FnOnce(Option<u64>, Option<u64>, &RoaringBitmap) -> RoaringBitmap,
) {
    if minimum.is_none() && maximum.is_none() {
        return;
    }
    if maximum.is_some_and(|maximum| maximum < 0) {
        roots.clear();
        return;
    }
    let minimum = minimum
        .filter(|minimum| *minimum > 0)
        .map(|minimum| minimum as u64);
    let maximum = maximum.map(|maximum| maximum as u64);
    *roots = apply(minimum, maximum, roots);
}

fn apply_timestamp_filters(
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) {
    let Some((imported_after, imported_before)) = parsed_timestamp_range(
        filters.imported_after.as_deref(),
        filters.imported_before.as_deref(),
    ) else {
        roots.clear();
        return;
    };
    apply_timestamp_range(
        imported_after,
        imported_before,
        roots,
        |minimum, maximum, universe| {
            projection.imported_at_range_bitmap(minimum, maximum, universe)
        },
    );

    let Some((modified_after, modified_before)) = parsed_timestamp_range(
        filters.modified_after.as_deref(),
        filters.modified_before.as_deref(),
    ) else {
        roots.clear();
        return;
    };
    apply_timestamp_range(
        modified_after,
        modified_before,
        roots,
        |minimum, maximum, universe| {
            projection.modified_at_range_bitmap(minimum, maximum, universe)
        },
    );
}

fn apply_timestamp_range(
    minimum: Option<i64>,
    exclusive_maximum: Option<i64>,
    roots: &mut RoaringBitmap,
    apply: impl FnOnce(Option<i64>, Option<i64>, &RoaringBitmap) -> RoaringBitmap,
) {
    if minimum.is_none() && exclusive_maximum.is_none() {
        return;
    }
    let maximum = match exclusive_maximum {
        Some(i64::MIN) => {
            roots.clear();
            return;
        }
        Some(maximum) => Some(maximum - 1),
        None => None,
    };
    *roots = apply(minimum, maximum, roots);
}

fn parsed_timestamp_range(
    after: Option<&str>,
    before: Option<&str>,
) -> Option<(Option<i64>, Option<i64>)> {
    Some((
        parse_optional_timestamp(after)?,
        parse_optional_timestamp(before)?,
    ))
}

fn parse_optional_timestamp(value: Option<&str>) -> Option<Option<i64>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Some(None);
    };
    timestamp_ms(value).map(Some)
}

fn apply_color_filter(
    connection: &Connection,
    projection: &ProjectionSelectionSnapshot,
    filters: &ItemFilters,
    roots: &mut RoaringBitmap,
) -> rusqlite::Result<()> {
    let Some(color_hex) = nonempty_text(filters.color_hex.as_deref()) else {
        return Ok(());
    };
    match crate::media_processing::colors::lab_components_from_hex(color_hex) {
        Some((l, a, b)) => {
            *roots = projection.color_match_bitmap(
                l,
                a,
                b,
                crate::media_processing::colors::FILTER_DELTA_E,
                roots,
            );
        }
        None => {
            // Not a decodable color: match the literal palette hex of any
            // owned member, as the SQL filter always has.
            let mut matches = RoaringBitmap::new();
            let mut statement = connection.prepare_cached(
                "SELECT ma.item_id
                 FROM file_color fc
                 JOIN media_asset ma ON ma.file_id = fc.file_id
                 WHERE lower(fc.hex) = ?1",
            )?;
            let media_ids = statement
                .query_map([color_hex.to_ascii_lowercase()], |row| {
                    row.get::<_, i64>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for media_id in media_ids {
                if let Some(root_id) = projection.root_for_media(media_id) {
                    if let Ok(root_id) = u32::try_from(root_id) {
                        matches.insert(root_id);
                    }
                }
            }
            *roots &= matches;
        }
    }
    Ok(())
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
        if filters.tag_match_mode != FilterMatchMode::Any
            && include_tag_ids.len() != filters.include_tags.len()
        {
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

pub(crate) fn fts_match_query(text: &str) -> Option<String> {
    let terms = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\"*"))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" AND "))
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
