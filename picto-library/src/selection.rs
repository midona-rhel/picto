use roaring::RoaringBitmap;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::model::{CollectionNoteDraft, FolderId, Rating, RootId, RootKind, TagId};
use crate::projection::ProjectionSnapshot;
use crate::query::{self, RootQuery};
use crate::{LibraryError, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SelectionTarget {
    Explicit {
        root_ids: Vec<RootId>,
    },
    Query {
        query: RootQuery,
        #[serde(default)]
        excluded_root_ids: Vec<RootId>,
    },
    /// Shift-click selections over a view the renderer only partly holds:
    /// the union of `spans` in the query's order, plus roots added one by one,
    /// minus roots toggled off.
    Range {
        query: RootQuery,
        spans: Vec<RangeSpan>,
        /// Roots outside every span that were added with Cmd/Ctrl-click.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        included_root_ids: Vec<RootId>,
        /// Roots inside a span that were toggled off.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        excluded_root_ids: Vec<RootId>,
    },
}

/// One contiguous run of a view, from the root clicked first to the root
/// shift-clicked. Its ends are the roots; the indices are where the grid
/// showed them, trusted only while both ends still hold those roots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeSpan {
    pub anchor_root_id: RootId,
    pub focus_root_id: RootId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_index: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionSummary {
    pub selected_count: u64,
    pub taggable_root_count: u64,
    pub total_size_bytes: u128,
    pub media_count: u128,
    pub shared_rating: Option<Rating>,
    pub minimum_rating: Option<Rating>,
    pub maximum_rating: Option<Rating>,
    pub shared_tags: Vec<TagId>,
    pub shared_folders: Vec<FolderId>,
    pub sample_hashes: Vec<String>,
    pub collection_candidates: Vec<SelectionCollectionCandidate>,
    pub shared_notes: Option<String>,
    pub has_notes: bool,
    pub shared_source_urls: Option<Vec<String>>,
    pub has_source_urls: bool,
    pub all_selected_roots_have_images: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionCollectionCandidate {
    pub collection_id: RootId,
    pub label: String,
    pub member_count: u32,
}

pub fn resolve(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
) -> Result<RoaringBitmap> {
    resolve_with(connection, snapshot, target, None)
}

/// `resolve`, reusing the grid's match cache: a select-all over a search the
/// grid just showed costs a cache lookup instead of evaluating it again inside
/// the write transaction.
pub(crate) fn resolve_with(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
    caches: Option<(&query::MatchCache, &query::OrderCache)>,
) -> Result<RoaringBitmap> {
    let cache = caches.map(|(matches, _)| matches);
    Ok(match target {
        // Check only the listed roots instead of unioning every root kind.
        SelectionTarget::Explicit { root_ids } => root_ids
            .iter()
            .map(|id| id.0)
            .filter(|id| snapshot.root_kinds.values().any(|kind| kind.contains(*id)))
            .collect(),
        SelectionTarget::Query {
            query,
            excluded_root_ids,
        } => {
            let media = query::MediaMatches::resolve(connection, snapshot, &query.scope)?;
            let mut selected = query::matching_roots_cached(snapshot, query, &media, cache)?;
            selected -= excluded_root_ids
                .iter()
                .map(|id| id.0)
                .collect::<RoaringBitmap>();
            selected
        }
        SelectionTarget::Range {
            query,
            spans,
            included_root_ids,
            excluded_root_ids,
        } => {
            let media = query::MediaMatches::resolve(connection, snapshot, &query.scope)?;
            let mut selected = RoaringBitmap::new();
            // The complete order is built at most once, and only when a span's
            // positions no longer hold its two roots.
            let mut complete: Option<CompleteOrder> = None;
            for span in spans {
                if let Some(ids) = span_by_position(snapshot, query, &media, cache, span)? {
                    selected.extend(ids.iter().map(|id| id.0));
                    continue;
                }
                if complete.is_none() {
                    complete = Some(match caches {
                        Some((matches, orders)) => CompleteOrder::Cached(
                            query::ordered_ids_cached(snapshot, query, &media, matches, orders)?,
                        ),
                        None => CompleteOrder::Owned(query::all_ordered_ids(snapshot, query, &media)?),
                    });
                }
                let ordered = complete.as_ref().expect("complete order was just built").ids();
                let locate = |root: RootId, end: &str| {
                    ordered.iter().position(|id| *id == root).ok_or_else(|| {
                        LibraryError::InvalidInput(format!("range {end} is outside query"))
                    })
                };
                let anchor = locate(span.anchor_root_id, "anchor")?;
                let focus = locate(span.focus_root_id, "focus")?;
                let (start, end) = (anchor.min(focus), anchor.max(focus));
                selected.extend(ordered[start..=end].iter().map(|id| id.0));
            }
            selected |= resolve(
                connection,
                snapshot,
                &SelectionTarget::Explicit {
                    root_ids: included_root_ids.clone(),
                },
            )?;
            selected
                - excluded_root_ids
                    .iter()
                    .map(|id| id.0)
                    .collect::<RoaringBitmap>()
        }
    })
}

enum CompleteOrder {
    Cached(std::sync::Arc<query::OrderedResult>),
    Owned(Vec<RootId>),
}

impl CompleteOrder {
    fn ids(&self) -> &[RootId] {
        match self {
            Self::Cached(order) => &order.ids,
            Self::Owned(ids) => ids,
        }
    }
}

/// A span read straight from the resident order at its recorded positions,
/// when both ends still hold the span's roots; `None` otherwise.
fn span_by_position(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &query::MediaMatches,
    cache: Option<&query::MatchCache>,
    span: &RangeSpan,
) -> Result<Option<Vec<RootId>>> {
    let (Some(anchor), Some(focus)) = (span.anchor_index, span.focus_index) else {
        return Ok(None);
    };
    let (start, end, first, last) = if anchor <= focus {
        (anchor, focus, span.anchor_root_id, span.focus_root_id)
    } else {
        (focus, anchor, span.focus_root_id, span.anchor_root_id)
    };
    let ids = query::ordinal_range(snapshot, query, media, cache, start as usize, end as usize)?;
    Ok(ids.filter(|ids| ids.first() == Some(&first) && ids.last() == Some(&last)))
}

/// Where each root sits in the query's current order, or `None` when the
/// view no longer holds it. The grid uses this to keep a range's highlights,
/// count and ends in step with what an operation on it would change.
pub(crate) fn root_positions(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    roots: &[RootId],
    caches: (&query::MatchCache, &query::OrderCache),
) -> Result<Vec<Option<u64>>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let media = query::MediaMatches::resolve(connection, snapshot, &query.scope)?;
    let ordered = query::ordered_ids_cached(snapshot, query, &media, caches.0, caches.1)?;
    let wanted = roots.iter().map(|id| id.0).collect::<RoaringBitmap>();
    let dense = crate::bitmap::DenseIds::new(&wanted);
    let mut found = std::collections::HashMap::with_capacity(roots.len());
    for (index, id) in ordered.ids.iter().enumerate() {
        if dense.contains(id.0) {
            found.insert(id.0, index as u64);
            if found.len() == wanted.len() as usize {
                break;
            }
        }
    }
    Ok(roots.iter().map(|id| found.get(&id.0).copied()).collect())
}

pub fn resolve_ordered(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
) -> Result<Vec<RootId>> {
    match target {
        SelectionTarget::Explicit { root_ids } => {
            let selected = resolve(connection, snapshot, target)?;
            Ok(root_ids
                .iter()
                .copied()
                .filter(|root_id| selected.contains(root_id.0))
                .collect())
        }
        SelectionTarget::Query {
            query,
            excluded_root_ids,
        } => {
            let excluded = excluded_root_ids
                .iter()
                .map(|id| id.0)
                .collect::<RoaringBitmap>();
            let media = query::MediaMatches::resolve(connection, snapshot, &query.scope)?;
            Ok(query::all_ordered_ids(snapshot, query, &media)?
                .into_iter()
                .filter(|root_id| !excluded.contains(root_id.0))
                .collect())
        }
        SelectionTarget::Range {
            query,
            included_root_ids,
            ..
        } => {
            let mut selected = resolve(connection, snapshot, target)?;
            let media = query::MediaMatches::resolve(connection, snapshot, &query.scope)?;
            let mut ordered = query::all_ordered_ids(snapshot, query, &media)?
                .into_iter()
                .filter(|root_id| selected.remove(root_id.0))
                .collect::<Vec<_>>();
            // Added roots that have since left the view keep their click order.
            ordered.extend(
                included_root_ids
                    .iter()
                    .copied()
                    .filter(|root_id| selected.remove(root_id.0)),
            );
            Ok(ordered)
        }
    }
}

pub fn resolve_ordered_image_roots(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
) -> Result<Vec<RootId>> {
    Ok(resolve_ordered(connection, snapshot, target)?
        .into_iter()
        .filter(|root_id| snapshot.roots_with_images.contains(root_id.0))
        .collect())
}

pub fn summarize(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
    selection: &RoaringBitmap,
) -> Result<SelectionSummary> {
    let shared_rating = (!selection.is_empty())
        .then(|| {
            Rating::ALL
                .into_iter()
                .find(|rating| (snapshot.rating(*rating) & selection).len() == selection.len())
        })
        .flatten();
    let first = selection.min();
    let rated = Rating::ALL
        .into_iter()
        .filter(|rating| *rating != Rating::Unrated)
        .filter(|rating| !(snapshot.rating(*rating) & selection).is_empty())
        .collect::<Vec<_>>();
    let minimum_rating = rated.first().copied();
    let maximum_rating = rated.last().copied();
    let shared_tags = first.map_or_else(Vec::new, |first| {
        snapshot
            .tags
            .iter()
            .filter_map(|(tag_id, members)| {
                (members.contains(first) && members.is_superset(selection)).then_some(*tag_id)
            })
            .collect()
    });
    let shared_folders = first.map_or_else(Vec::new, |first| {
        snapshot
            .folders
            .iter()
            .filter_map(|(folder_id, members)| {
                (members.contains(first) && members.is_superset(selection)).then_some(*folder_id)
            })
            .collect()
    });
    let notes_members = &*snapshot.notes_present & selection;
    let urls_members = &*snapshot.urls_present & selection;
    let has_notes = !notes_members.is_empty();
    let has_source_urls = !urls_members.is_empty();
    let compare_notes = !selection.is_empty() && notes_members.len() == selection.len();
    let compare_urls = !selection.is_empty() && urls_members.len() == selection.len();
    let (shared_notes, compared_urls) =
        shared_text(connection, selection, compare_notes, compare_urls)?;
    let shared_source_urls = if selection.is_empty() {
        None
    } else if urls_members.is_empty() {
        Some(Vec::new())
    } else if urls_members.len() != selection.len() {
        None
    } else {
        compared_urls
    };
    let sample_hashes = preview_hashes(connection, snapshot, target, selection)?;
    let collection_candidates = collection_candidates(connection, snapshot, selection)?;
    let all_selected_roots_have_images = snapshot.roots_with_images.is_superset(selection);
    Ok(SelectionSummary {
        selected_count: selection.len(),
        taggable_root_count: (&*snapshot.roots_with_images & selection).len(),
        total_size_bytes: snapshot.total_bytes.sum(selection),
        media_count: snapshot.media_count.sum(selection),
        shared_rating,
        minimum_rating,
        maximum_rating,
        shared_tags,
        shared_folders,
        sample_hashes,
        collection_candidates,
        shared_notes,
        has_notes,
        shared_source_urls,
        has_source_urls,
        all_selected_roots_have_images,
        revision: snapshot.revision,
    })
}

pub fn collection_note_draft(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
) -> Result<CollectionNoteDraft> {
    let ordered = resolve_ordered(connection, snapshot, target)?;
    // Read the notes of the noted roots in one query, then keep selection order.
    let noted = ordered
        .iter()
        .map(|root_id| root_id.0)
        .filter(|id| snapshot.notes_present.contains(*id))
        .collect::<RoaringBitmap>();
    let mut by_root = std::collections::HashMap::new();
    if !noted.is_empty() {
        let mut statement = connection.prepare_cached(
            "SELECT root_id, notes FROM library_root
             WHERE root_id IN (SELECT value FROM json_each(?1))",
        )?;
        let rows = statement.query_map([id_list(&noted)?], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        for row in rows {
            let (root_id, notes) = row?;
            if let Some(notes) = notes {
                by_root.insert(root_id, notes);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut notes = Vec::new();
    for root_id in ordered {
        let Some(value) = by_root.remove(&root_id.0).filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        if seen.insert(value.clone()) {
            notes.push(value);
        }
    }
    let notes = notes.join("\n\n");
    Ok(CollectionNoteDraft {
        source_count: seen.len() as u64,
        byte_length: notes.len() as u64,
        maximum_bytes: crate::model::MAX_ROOT_NOTES_BYTES as u64,
        notes,
    })
}

fn preview_hashes(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    target: &SelectionTarget,
    selection: &RoaringBitmap,
) -> Result<Vec<String>> {
    const PREVIEW_COUNT: usize = 6;
    let root_ids = match target {
        SelectionTarget::Explicit { root_ids } => root_ids
            .iter()
            .rev()
            .filter(|root_id| selection.contains(root_id.0))
            .take(PREVIEW_COUNT)
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>(),
        SelectionTarget::Query { .. } | SelectionTarget::Range { .. } => {
            // The six newest by import date, oldest first, without scanning the
            // whole selection.
            if let Some(newest) = snapshot.imported_at.ordered_window(selection, 0, PREVIEW_COUNT, true, false)? {
                return cover_hashes(connection, newest.into_iter().rev().map(RootId));
            }
            let mut recent = Vec::with_capacity(PREVIEW_COUNT);
            for root_id in selection {
                let candidate = (
                    snapshot.imported_at.value(root_id).unwrap_or_default(),
                    RootId(root_id),
                );
                if recent.len() < PREVIEW_COUNT {
                    recent.push(candidate);
                } else if let Some((oldest, _)) =
                    recent.iter().enumerate().min_by_key(|(_, value)| **value)
                {
                    if candidate > recent[oldest] {
                        recent[oldest] = candidate;
                    }
                }
            }
            recent.sort_unstable();
            recent.into_iter().map(|(_, root_id)| root_id).collect()
        }
    };
    cover_hashes(connection, root_ids)
}

fn cover_hashes(
    connection: &Connection,
    root_ids: impl IntoIterator<Item = RootId>,
) -> Result<Vec<String>> {
    let mut statement = connection.prepare_cached(
        "SELECT file.content_hash
         FROM library_root root
         JOIN media_item media ON media.media_id = root.cover_media_id
         JOIN media_file file ON file.file_id = media.file_id
         WHERE root.root_id = ?1",
    )?;
    root_ids
        .into_iter()
        .map(|root_id| {
            statement
                .query_row([root_id.0], |row| row.get(0))
                .map_err(Into::into)
        })
        .collect()
}

fn collection_candidates(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    selection: &RoaringBitmap,
) -> Result<Vec<SelectionCollectionCandidate>> {
    let selected_collections = snapshot
        .root_kinds
        .get(&RootKind::Collection)
        .map_or_else(RoaringBitmap::new, |collections| collections & selection);
    if selected_collections.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare_cached(
        "SELECT root_id, name, media_count FROM library_root
         WHERE root_id IN (SELECT value FROM json_each(?1)) ORDER BY root_id",
    )?;
    let rows = statement.query_map([id_list(&selected_collections)?], |row| {
        Ok(SelectionCollectionCandidate {
            collection_id: RootId(row.get(0)?),
            label: row.get(1)?,
            member_count: row.get(2)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

fn shared_text(
    connection: &Connection,
    selection: &RoaringBitmap,
    compare_notes: bool,
    compare_urls: bool,
) -> Result<(Option<String>, Option<Vec<String>>)> {
    // One query per field: the shared value exists exactly when the selection
    // holds a single distinct value, so two distinct rows end the scan.
    let ids = (compare_notes || compare_urls).then(|| id_list(selection)).transpose()?;
    let distinct = |column: &str| -> Result<Option<Option<String>>> {
        let mut statement = connection.prepare_cached(&format!(
            "SELECT DISTINCT {column} FROM library_root
             WHERE root_id IN (SELECT value FROM json_each(?1)) LIMIT 2"
        ))?;
        let values = statement
            .query_map([ids.as_deref().unwrap_or("[]")], |row| row.get::<_, Option<String>>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok((values.len() == 1).then(|| values.into_iter().next().flatten()))
    };
    let notes = if compare_notes { distinct("notes")?.flatten() } else { None };
    let urls = if compare_urls {
        distinct("source_urls_json")?
            .flatten()
            .map(|encoded| serde_json::from_str::<Vec<String>>(&encoded))
            .transpose()?
    } else {
        None
    };
    Ok((notes, urls))
}

/// A JSON array of ids for `json_each`, so one query covers a whole set.
fn id_list(ids: &RoaringBitmap) -> Result<String> {
    Ok(serde_json::to_string(&ids.iter().collect::<Vec<_>>())?)
}
