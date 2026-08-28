//! Typed application-shell boundary for the greenfield media library.
//!
//! This module translates the renderer's stable command DTOs into the closed
//! `picto_library` model. It contains no persistence or product behavior.

use chrono::DateTime;
use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, SortDirection, SortField, TextField,
    ViewQuerySpec,
};
use picto_library::query::{ItemScope, RootQuery};
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, LabColor, Library, Rating, RootId, TagId};

use crate::app::{
    FilterMatchMode, ItemFilters, ItemQuery, ItemScope as AppScope, ItemSortField, ItemTarget,
    Lifecycle, MutationReceipt, SortDirection as AppSortDirection,
};

pub fn query(library: &Library, value: &ItemQuery) -> Result<RootQuery, String> {
    let snapshot = library.projections().snapshot();
    let tag_id = |name: &str| snapshot.tag_ids_by_name.get(name).copied();
    Ok(RootQuery {
        scope: scope(&value.scope)?,
        view: ViewQuerySpec {
            filter: filters(&value.filters, tag_id)?,
            sort: sort(&value.sort),
        },
    })
}

pub fn target(library: &Library, value: &ItemTarget) -> Result<SelectionTarget, String> {
    Ok(match value {
        ItemTarget::Explicit { item_ids } => SelectionTarget::Explicit {
            root_ids: item_ids
                .iter()
                .copied()
                .map(|id| root_id(id.0))
                .collect::<Result<_, _>>()?,
        },
        ItemTarget::Query {
            query: value,
            excluded_item_ids,
        } => SelectionTarget::Query {
            query: query(library, value)?,
            excluded_root_ids: excluded_item_ids
                .iter()
                .copied()
                .map(|id| root_id(id.0))
                .collect::<Result<_, _>>()?,
        },
        ItemTarget::Range {
            query: value,
            anchor_item_id,
            focus_item_id,
        } => SelectionTarget::Range {
            query: query(library, value)?,
            anchor_root_id: root_id(anchor_item_id.0)?,
            focus_root_id: root_id(focus_item_id.0)?,
        },
    })
}

pub fn lifecycle(value: Lifecycle) -> picto_library::Lifecycle {
    match value {
        Lifecycle::Active => picto_library::Lifecycle::Active,
        Lifecycle::Inbox => picto_library::Lifecycle::Inbox,
        Lifecycle::Trash => picto_library::Lifecycle::Trash,
    }
}

fn app_lifecycle(value: picto_library::Lifecycle) -> Lifecycle {
    match value {
        picto_library::Lifecycle::Active => Lifecycle::Active,
        picto_library::Lifecycle::Inbox => Lifecycle::Inbox,
        picto_library::Lifecycle::Trash => Lifecycle::Trash,
    }
}

pub fn receipt(value: picto_library::MutationReceipt) -> MutationReceipt {
    MutationReceipt {
        revision: value.revision,
        resources: value.resources,
        item_ids: value
            .item_ids
            .into_iter()
            .map(|id| crate::app::ItemId(i64::from(id.0)))
            .collect(),
    }
}

pub fn page(value: picto_library::query::RootPage) -> Result<crate::query_v2::ItemPage, String> {
    Ok(crate::query_v2::ItemPage {
        items: value
            .items
            .into_iter()
            .map(|item| {
                Ok(crate::query_v2::ItemSummary {
                    item_id: crate::app::ItemId(i64::from(item.root_id.0)),
                    kind: match item.kind {
                        picto_library::RootKind::Media => crate::app::ItemKind::Media,
                        picto_library::RootKind::Collection => crate::app::ItemKind::Collection,
                    },
                    lifecycle: match item.lifecycle {
                        picto_library::Lifecycle::Active => crate::app::Lifecycle::Active,
                        picto_library::Lifecycle::Inbox => crate::app::Lifecycle::Inbox,
                        picto_library::Lifecycle::Trash => crate::app::Lifecycle::Trash,
                    },
                    name: Some(item.name),
                    display_file_hash: crate::app::FileHash(item.content_hash),
                    display_mime_type: item.mime,
                    pixel_width: item.width.map(i64::from),
                    pixel_height: item.height.map(i64::from),
                    duration_ms: item
                        .duration_ms
                        .map(|value| checked_i64(u128::from(value)))
                        .transpose()?,
                    frame_count: item.frame_count.map(i64::from),
                    dominant_color_hex: item.palette.first().map(lab_hex),
                    rating: match item.rating {
                        Rating::Unrated => None,
                        Rating::One => Some(1),
                        Rating::Two => Some(2),
                        Rating::Three => Some(3),
                        Rating::Four => Some(4),
                        Rating::Five => Some(5),
                    },
                    media_count: i64::from(item.media_count),
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        next_cursor: value.next_cursor,
        revision: value.revision,
        visible_item_count: Some(checked_i64(u128::from(value.total))?),
        visible_media_count: Some(checked_i64(value.media_count)?),
        total_size_bytes: Some(checked_i64(value.total_size_bytes)?),
    })
}

pub fn details(
    library: &Library,
    value: picto_library::RootDetails,
) -> Result<crate::query_v2::ItemDetails, String> {
    let tag_names = library
        .tags()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|tag| {
            let name = if tag.namespace.is_empty() || tag.namespace == "general" {
                tag.subname
            } else {
                format!("{}:{}", tag.namespace, tag.subname)
            };
            (tag.tag_id, name)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let aggregate_tags = value
        .tag_ids
        .iter()
        .filter_map(|tag_id| tag_names.get(tag_id).cloned())
        .collect::<Vec<_>>();
    let rating = rating_number(value.rating);
    let imported_at = timestamp_string(value.root.imported_at_ms)?;
    let captured_at = value
        .root
        .captured_at_ms
        .map(timestamp_string)
        .transpose()?;
    let notes = value.root.notes.clone();
    let source_urls = value.root.source_urls.clone();
    let media = value
        .media
        .into_iter()
        .enumerate()
        .map(|(position, media)| {
            let dominant_colors = media.facts.palette.iter().map(lab_hex).collect::<Vec<_>>();
            Ok(crate::query_v2::MediaDetails {
                media_item_id: crate::app::ItemId(i64::from(media.media_id.0)),
                file_hash: crate::app::FileHash(media.facts.content_hash),
                mime_type: media.facts.mime,
                dominant_color_hex: dominant_colors.first().cloned(),
                dominant_colors,
                size_bytes: checked_i64(u128::from(media.facts.size_bytes))?,
                pixel_width: media.facts.width.map(i64::from),
                pixel_height: media.facts.height.map(i64::from),
                duration_ms: media
                    .facts
                    .duration_ms
                    .map(|duration| checked_i64(u128::from(duration)))
                    .transpose()?,
                frame_count: media.facts.frame_count.map(i64::from),
                name: Some(media.media_name),
                notes: notes.clone(),
                rating,
                source_urls: source_urls.clone(),
                captured_at: captured_at.clone(),
                imported_at: imported_at.clone(),
                position: checked_i64(position as u128)?,
                tags: aggregate_tags.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(crate::query_v2::ItemDetails {
        item_id: crate::app::ItemId(i64::from(value.root.root_id.0)),
        kind: match value.root.kind {
            picto_library::RootKind::Media => crate::app::ItemKind::Media,
            picto_library::RootKind::Collection => crate::app::ItemKind::Collection,
        },
        lifecycle: app_lifecycle(value.lifecycle),
        label: Some(value.root.name),
        cover_media_item_id: Some(crate::app::ItemId(i64::from(value.root.cover_media_id.0))),
        folder_ids: value
            .folder_ids
            .into_iter()
            .map(|folder_id| i64::from(folder_id.0))
            .collect(),
        media,
        aggregate_tags,
        revision: value.revision,
    })
}

pub fn selection_summary(
    library: &Library,
    value: picto_library::selection::SelectionSummary,
) -> Result<crate::query_v2::SelectionSummary, String> {
    let selected_count = checked_i64(u128::from(value.selected_count))?;
    let tag_names = library
        .tags()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|tag| {
            let name = if tag.namespace.is_empty() || tag.namespace == "general" {
                tag.subname
            } else {
                format!("{}:{}", tag.namespace, tag.subname)
            };
            (tag.tag_id, name)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let folder_names = library
        .folders()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|folder| (folder.folder_id, folder.name))
        .collect::<std::collections::HashMap<_, _>>();

    Ok(crate::query_v2::SelectionSummary {
        total_count: selected_count,
        selected_count,
        sample_hashes: value
            .sample_hashes
            .into_iter()
            .map(crate::app::FileHash)
            .collect(),
        shared_tags: value
            .shared_tags
            .into_iter()
            .filter_map(|tag_id| tag_names.get(&tag_id).cloned())
            .map(|tag| crate::query_v2::SelectionTagCount {
                tag,
                count: selected_count,
            })
            .collect(),
        shared_folders: value
            .shared_folders
            .into_iter()
            .filter_map(|folder_id| {
                folder_names.get(&folder_id).cloned().map(|name| {
                    crate::query_v2::SelectionFolderInfo {
                        folder_id: i64::from(folder_id.0),
                        name,
                    }
                })
            })
            .collect(),
        selected_collection_candidates: value
            .collection_candidates
            .into_iter()
            .map(|candidate| crate::query_v2::SelectionCollectionCandidate {
                collection_id: crate::app::ItemId(i64::from(candidate.collection_id.0)),
                label: Some(candidate.label),
                member_count: i64::from(candidate.member_count),
            })
            .collect(),
        shared_notes: value.shared_notes,
        has_notes: value.has_notes,
        shared_source_urls: value.shared_source_urls,
        has_source_urls: value.has_source_urls,
        stats: crate::query_v2::SelectionSummaryStats {
            total_size_bytes: Some(checked_i64(value.total_size_bytes)?),
            media_count: checked_i64(value.media_count)?,
            all_media_are_images: value.all_selected_roots_have_images,
            rating_stats: crate::query_v2::SelectionRatingStats {
                min: value.minimum_rating.and_then(rating_number),
                max: value.maximum_rating.and_then(rating_number),
                shared: value.shared_rating.and_then(rating_number),
            },
        },
        revision: value.revision,
    })
}

fn scope(value: &AppScope) -> Result<ItemScope, String> {
    Ok(match value {
        AppScope::All => ItemScope::All,
        AppScope::Inbox => ItemScope::Inbox,
        AppScope::Trash => ItemScope::Trash,
        AppScope::RecentlyViewed => ItemScope::RecentlyViewed,
        AppScope::Untagged => ItemScope::Untagged,
        AppScope::Uncategorized => ItemScope::Uncategorized,
        AppScope::Folder { folder_id } => ItemScope::Folder {
            folder_id: FolderId(local_id(*folder_id, "folder")?),
        },
        AppScope::SmartFolder { smart_folder_id } => ItemScope::SmartFolder {
            smart_folder_id: picto_library::SmartFolderId(local_id(
                *smart_folder_id,
                "smart folder",
            )?),
        },
    })
}

fn filters(
    value: &ItemFilters,
    mut tag_id: impl FnMut(&str) -> Option<TagId>,
) -> Result<FilterExpr, String> {
    let mut clauses = Vec::new();
    let include_tags = value
        .include_tags
        .iter()
        .filter_map(|name| tag_id(name))
        .collect::<Vec<_>>();
    if !value.include_tags.is_empty() {
        let mode = set_mode(&value.tag_match_mode);
        clauses.push(
            if include_tags.is_empty()
                || (!matches!(mode, SetMatchMode::Any)
                    && include_tags.len() != value.include_tags.len())
            {
                FilterExpr::Any(Vec::new())
            } else {
                FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: include_tags,
                    mode,
                })
            },
        );
    }
    let exclude_tags = value
        .exclude_tags
        .iter()
        .filter_map(|name| tag_id(name))
        .collect::<Vec<_>>();
    if !exclude_tags.is_empty() {
        clauses.push(FilterExpr::Not(Box::new(FilterExpr::Clause(
            FilterClause::Tags {
                tag_ids: exclude_tags,
                mode: SetMatchMode::Any,
            },
        ))));
    }

    let include_folders = folder_ids(&value.include_folder_ids)?;
    if !include_folders.is_empty() {
        clauses.push(FilterExpr::Clause(FilterClause::Folders {
            folder_ids: include_folders,
            mode: set_mode(&value.folder_match_mode),
        }));
    }
    let exclude_folders = folder_ids(&value.exclude_folder_ids)?;
    if !exclude_folders.is_empty() {
        clauses.push(FilterExpr::Not(Box::new(FilterExpr::Clause(
            FilterClause::Folders {
                folder_ids: exclude_folders,
                mode: SetMatchMode::Any,
            },
        ))));
    }

    let ratings = value
        .ratings
        .iter()
        .map(|rating| match rating {
            0 => Ok(Rating::Unrated),
            1 => Ok(Rating::One),
            2 => Ok(Rating::Two),
            3 => Ok(Rating::Three),
            4 => Ok(Rating::Four),
            5 => Ok(Rating::Five),
            value => Err(format!("rating {value} is outside 0..=5")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !ratings.is_empty() {
        clauses.push(FilterExpr::Clause(FilterClause::Ratings { ratings }));
    }
    add_mime_clause(&mut clauses, &value.include_mime_types, false);
    add_mime_clause(&mut clauses, &value.exclude_mime_types, true);

    add_range(
        &mut clauses,
        timestamp(value.imported_after.as_deref())?,
        timestamp(value.imported_before.as_deref())?,
        |minimum_ms, maximum_ms| FilterClause::ImportedAt {
            minimum_ms,
            maximum_ms,
        },
    );
    add_range(
        &mut clauses,
        timestamp(value.modified_after.as_deref())?,
        timestamp(value.modified_before.as_deref())?,
        |minimum_ms, maximum_ms| FilterClause::ModifiedAt {
            minimum_ms,
            maximum_ms,
        },
    );
    add_range(
        &mut clauses,
        nonnegative(value.min_width, "minimum width")?,
        nonnegative(value.max_width, "maximum width")?,
        |minimum, maximum| FilterClause::Width { minimum, maximum },
    );
    add_range(
        &mut clauses,
        nonnegative(value.min_height, "minimum height")?,
        nonnegative(value.max_height, "maximum height")?,
        |minimum, maximum| FilterClause::Height { minimum, maximum },
    );
    add_range(
        &mut clauses,
        nonnegative(value.min_duration_ms, "minimum duration")?,
        nonnegative(value.max_duration_ms, "maximum duration")?,
        |minimum_ms, maximum_ms| FilterClause::Duration {
            minimum_ms,
            maximum_ms,
        },
    );
    add_range(
        &mut clauses,
        nonnegative(value.min_size_bytes, "minimum size")?,
        nonnegative(value.max_size_bytes, "maximum size")?,
        |minimum_bytes, maximum_bytes| FilterClause::TotalSize {
            minimum_bytes,
            maximum_bytes,
        },
    );

    if let Some(present) = value.notes_present {
        clauses.push(FilterExpr::Clause(FilterClause::NotesPresent { present }));
    }
    if let Some(present) = value.source_url_present {
        clauses.push(FilterExpr::Clause(FilterClause::SourceUrlsPresent {
            present,
        }));
    }
    add_text(&mut clauses, TextField::Global, value.text.as_deref());
    add_text(
        &mut clauses,
        TextField::Notes,
        value.notes_contains.as_deref(),
    );
    add_text(
        &mut clauses,
        TextField::SourceUrl,
        value.source_url_contains.as_deref(),
    );
    if let Some(color_hex) = value.color_hex.as_deref() {
        let (l, a, b) = crate::media_processing::colors::lab_components_from_hex(color_hex)
            .ok_or_else(|| format!("invalid color {color_hex}"))?;
        clauses.push(FilterExpr::Clause(FilterClause::Color {
            color: LabColor {
                l: l as f32,
                a: a as f32,
                b: b as f32,
                weight: 1.0,
            },
            delta_e: crate::media_processing::colors::FILTER_DELTA_E as f32,
        }));
    }
    Ok(FilterExpr::All(clauses))
}

fn sort(value: &crate::app::ItemSort) -> ItemSort {
    ItemSort {
        field: match value.field {
            ItemSortField::ImportedAt => SortField::ImportedAt,
            ItemSortField::CapturedAt => SortField::CapturedAt,
            ItemSortField::Name => SortField::Name,
            ItemSortField::Rating => SortField::Rating,
            ItemSortField::Size => SortField::TotalSize,
            ItemSortField::Random => SortField::Random,
            ItemSortField::FolderOrder => SortField::FolderOrder,
        },
        direction: match value.direction {
            AppSortDirection::Ascending => SortDirection::Ascending,
            AppSortDirection::Descending => SortDirection::Descending,
        },
        random_seed: value.random_seed.clone(),
    }
}

fn add_mime_clause(clauses: &mut Vec<FilterExpr>, values: &[String], excluded: bool) {
    if values.is_empty() {
        return;
    }
    let mut exact = Vec::new();
    let mut families = Vec::new();
    for value in values {
        if let Some(family) = value.strip_suffix("/*") {
            families.push(family.to_string());
        } else {
            exact.push(value.clone());
        }
    }
    let clause = FilterExpr::Clause(FilterClause::Mime {
        values: exact,
        families,
    });
    clauses.push(if excluded {
        FilterExpr::Not(Box::new(clause))
    } else {
        clause
    });
}

fn add_range(
    clauses: &mut Vec<FilterExpr>,
    minimum: Option<u64>,
    maximum: Option<u64>,
    clause: impl FnOnce(Option<u64>, Option<u64>) -> FilterClause,
) {
    if minimum.is_some() || maximum.is_some() {
        clauses.push(FilterExpr::Clause(clause(minimum, maximum)));
    }
}

fn add_text(clauses: &mut Vec<FilterExpr>, field: TextField, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        clauses.push(FilterExpr::Clause(FilterClause::Text {
            field,
            query: value.to_string(),
        }));
    }
}

fn set_mode(value: &FilterMatchMode) -> SetMatchMode {
    match value {
        FilterMatchMode::Any => SetMatchMode::Any,
        FilterMatchMode::All => SetMatchMode::All,
        FilterMatchMode::Exact => SetMatchMode::Exact,
    }
}

fn folder_ids(values: &[i64]) -> Result<Vec<FolderId>, String> {
    values
        .iter()
        .map(|value| local_id(*value, "folder").map(FolderId))
        .collect()
}

fn root_id(value: i64) -> Result<RootId, String> {
    local_id(value, "root").map(RootId)
}

fn local_id(value: i64, kind: &str) -> Result<u32, String> {
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{kind} ID {value} is outside the local ID domain"))
}

fn nonnegative(value: Option<i64>, label: &str) -> Result<Option<u64>, String> {
    value
        .map(|value| u64::try_from(value).map_err(|_| format!("{label} cannot be negative")))
        .transpose()
}

fn timestamp(value: Option<&str>) -> Result<Option<u64>, String> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map_err(|error| format!("invalid timestamp {value}: {error}"))
                .and_then(|value| {
                    u64::try_from(value.timestamp_millis())
                        .map_err(|_| format!("timestamp {value} predates the Unix epoch"))
                })
        })
        .transpose()
}

fn checked_i64(value: u128) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| format!("value {value} exceeds the renderer integer domain"))
}

fn rating_number(value: Rating) -> Option<i64> {
    match value {
        Rating::Unrated => None,
        Rating::One => Some(1),
        Rating::Two => Some(2),
        Rating::Three => Some(3),
        Rating::Four => Some(4),
        Rating::Five => Some(5),
    }
}

fn timestamp_string(value: i64) -> Result<String, String> {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| format!("timestamp {value} is outside the supported date range"))
}

fn lab_hex(value: &LabColor) -> String {
    use palette::{IntoColor, Lab, LinSrgb, Srgb};

    let linear: LinSrgb = Lab::new(value.l, value.a, value.b).into_color();
    let encoded: Srgb<u8> = Srgb::<f32>::from_linear(linear).into_format();
    format!(
        "#{:02x}{:02x}{:02x}",
        encoded.red, encoded.green, encoded.blue
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ItemSort as AppItemSort, ItemSortField};

    #[test]
    fn inbox_query_keeps_text_and_structured_filters_inside_the_inbox_scope() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::create(directory.path().join("library.sqlite")).unwrap();
        let converted = query(
            &library,
            &ItemQuery {
                scope: AppScope::Inbox,
                filters: ItemFilters {
                    text: Some("fox".into()),
                    include_mime_types: vec!["image/*".into()],
                    min_size_bytes: Some(10),
                    ..ItemFilters::default()
                },
                sort: AppItemSort {
                    field: ItemSortField::Size,
                    direction: AppSortDirection::Descending,
                    random_seed: None,
                },
            },
        )
        .unwrap();

        assert_eq!(converted.scope, ItemScope::Inbox);
        assert_eq!(converted.view.sort.field, SortField::TotalSize);
        let FilterExpr::All(clauses) = converted.view.filter else {
            panic!("flat UI filters compile into one conjunction");
        };
        assert_eq!(clauses.len(), 3);
    }

    #[test]
    fn invalid_local_ids_are_rejected_before_the_library_is_called() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::create(directory.path().join("library.sqlite")).unwrap();
        let error = target(
            &library,
            &ItemTarget::Explicit {
                item_ids: vec![crate::app::ItemId(-1)],
            },
        )
        .unwrap_err();
        assert!(error.contains("outside the local ID domain"));
    }

    #[test]
    fn grid_page_preserves_scope_lifecycle_and_bitmap_aggregates() {
        let converted = page(picto_library::query::RootPage {
            items: vec![picto_library::query::RootSummary {
                root_id: RootId(7),
                kind: picto_library::RootKind::Collection,
                lifecycle: picto_library::Lifecycle::Inbox,
                name: "Post".into(),
                cover_media_id: picto_library::MediaId(8),
                content_hash: "abc".into(),
                mime: "image/png".into(),
                width: Some(100),
                height: Some(200),
                duration_ms: None,
                frame_count: Some(1),
                palette: vec![LabColor {
                    l: 50.0,
                    a: 0.0,
                    b: 0.0,
                    weight: 1.0,
                }],
                imported_at_ms: 10,
                captured_at_ms: None,
                modified_at_ms: 11,
                media_count: 4,
                total_size_bytes: 99,
                rating: Rating::Three,
            }],
            next_cursor: Some("next".into()),
            total: 12,
            media_count: 30,
            total_size_bytes: 999,
            revision: 4,
        })
        .unwrap();

        assert_eq!(converted.items[0].lifecycle, crate::app::Lifecycle::Inbox);
        assert_eq!(converted.items[0].media_count, 4);
        assert_eq!(converted.visible_item_count, Some(12));
        assert_eq!(converted.visible_media_count, Some(30));
        assert_eq!(converted.total_size_bytes, Some(999));
        assert_eq!(converted.revision, 4);
    }

    #[test]
    fn details_keep_member_facts_separate_from_root_organization() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::create(directory.path().join("library.sqlite")).unwrap();
        let converted = details(
            &library,
            picto_library::RootDetails {
                root: picto_library::RootRecord {
                    root_id: RootId(7),
                    stable_key: "root".into(),
                    kind: picto_library::RootKind::Collection,
                    name: "Collection".into(),
                    notes: Some("Root note".into()),
                    source_urls: vec!["https://example.invalid/post".into()],
                    cover_media_id: picto_library::MediaId(8),
                    imported_at_ms: 1_700_000_000_000,
                    captured_at_ms: None,
                    modified_at_ms: 1_700_000_000_100,
                    media_count: 1,
                    total_size_bytes: 42,
                },
                lifecycle: picto_library::Lifecycle::Inbox,
                rating: Rating::Four,
                folder_ids: vec![FolderId(3)],
                tag_ids: Vec::new(),
                media: vec![picto_library::MediaRecord {
                    media_id: picto_library::MediaId(8),
                    media_name: "member.png".into(),
                    file_id: picto_library::FileId(9),
                    file_path: "/tmp/member.png".into(),
                    facts: picto_library::ImmutableMediaFacts {
                        mime: "image/png".into(),
                        size_bytes: 42,
                        width: Some(10),
                        height: Some(20),
                        duration_ms: None,
                        frame_count: Some(1),
                        content_hash: "hash".into(),
                        perceptual_hash: None,
                        palette: Vec::new(),
                    },
                }],
                revision: 2,
            },
        )
        .unwrap();

        assert_eq!(converted.label.as_deref(), Some("Collection"));
        assert_eq!(converted.lifecycle, Lifecycle::Inbox);
        assert_eq!(converted.folder_ids, vec![3]);
        assert_eq!(converted.media[0].name.as_deref(), Some("member.png"));
        assert_eq!(converted.media[0].notes.as_deref(), Some("Root note"));
        assert_eq!(converted.media[0].rating, Some(4));
        assert_eq!(converted.media[0].position, 0);
    }

    #[test]
    fn selection_summary_preserves_six_previews_and_collection_actions() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::create(directory.path().join("library.sqlite")).unwrap();
        let converted = selection_summary(
            &library,
            picto_library::selection::SelectionSummary {
                selected_count: 2,
                total_size_bytes: 42,
                media_count: 3,
                shared_rating: Some(Rating::Four),
                minimum_rating: Some(Rating::Two),
                maximum_rating: Some(Rating::Four),
                shared_tags: Vec::new(),
                shared_folders: Vec::new(),
                sample_hashes: (1..=6).map(|index| format!("hash-{index}")).collect(),
                collection_candidates: vec![
                    picto_library::selection::SelectionCollectionCandidate {
                        collection_id: RootId(7),
                        label: "Existing collection".into(),
                        member_count: 2,
                    },
                ],
                shared_notes: None,
                has_notes: true,
                shared_source_urls: Some(Vec::new()),
                has_source_urls: false,
                all_selected_roots_have_images: true,
                revision: 9,
            },
        )
        .unwrap();

        assert_eq!(converted.sample_hashes.len(), 6);
        assert_eq!(converted.selected_collection_candidates.len(), 1);
        assert_eq!(
            converted.selected_collection_candidates[0].collection_id,
            crate::app::ItemId(7)
        );
        assert_eq!(converted.stats.rating_stats.min, Some(2));
        assert_eq!(converted.stats.rating_stats.max, Some(4));
        assert_eq!(converted.stats.rating_stats.shared, Some(4));
    }
}
