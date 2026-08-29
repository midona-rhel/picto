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
    Range {
        query: RootQuery,
        anchor_root_id: RootId,
        focus_root_id: RootId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionSummary {
    pub selected_count: u64,
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
    let all_roots =
        snapshot
            .root_kinds
            .values()
            .fold(RoaringBitmap::new(), |mut result, values| {
                result |= values;
                result
            });
    Ok(match target {
        SelectionTarget::Explicit { root_ids } => {
            let mut selected = root_ids.iter().map(|id| id.0).collect::<RoaringBitmap>();
            selected &= all_roots;
            selected
        }
        SelectionTarget::Query {
            query,
            excluded_root_ids,
        } => {
            let mut selected = query::matching_roots(connection, snapshot, query)?;
            selected -= excluded_root_ids
                .iter()
                .map(|id| id.0)
                .collect::<RoaringBitmap>();
            selected
        }
        SelectionTarget::Range {
            query,
            anchor_root_id,
            focus_root_id,
        } => {
            let ordered = query::all_ordered_ids(connection, snapshot, query)?;
            let anchor = ordered
                .iter()
                .position(|id| id == anchor_root_id)
                .ok_or_else(|| {
                    LibraryError::InvalidInput("range anchor is outside query".into())
                })?;
            let focus = ordered
                .iter()
                .position(|id| id == focus_root_id)
                .ok_or_else(|| LibraryError::InvalidInput("range focus is outside query".into()))?;
            let (start, end) = if anchor <= focus {
                (anchor, focus)
            } else {
                (focus, anchor)
            };
            ordered[start..=end].iter().map(|id| id.0).collect()
        }
    })
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
            Ok(query::all_ordered_ids(connection, snapshot, query)?
                .into_iter()
                .filter(|root_id| !excluded.contains(root_id.0))
                .collect())
        }
        SelectionTarget::Range { .. } => {
            let selected = resolve(connection, snapshot, target)?;
            let SelectionTarget::Range { query, .. } = target else {
                unreachable!()
            };
            Ok(query::all_ordered_ids(connection, snapshot, query)?
                .into_iter()
                .filter(|root_id| selected.contains(root_id.0))
                .collect())
        }
    }
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
    let mut statement =
        connection.prepare_cached("SELECT notes FROM library_root WHERE root_id = ?1")?;
    let mut seen = std::collections::HashSet::new();
    let mut notes = Vec::new();
    for root_id in ordered {
        let value = statement.query_row([root_id.0], |row| row.get::<_, Option<String>>(0))?;
        let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
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
    let mut statement = connection
        .prepare_cached("SELECT name, media_count FROM library_root WHERE root_id = ?1")?;
    selected_collections
        .iter()
        .map(|root_id| {
            statement
                .query_row([root_id], |row| {
                    Ok(SelectionCollectionCandidate {
                        collection_id: RootId(root_id),
                        label: row.get(0)?,
                        member_count: row.get(1)?,
                    })
                })
                .map_err(Into::into)
        })
        .collect()
}

fn shared_text(
    connection: &Connection,
    selection: &RoaringBitmap,
    compare_notes: bool,
    compare_urls: bool,
) -> Result<(Option<String>, Option<Vec<String>>)> {
    if !compare_notes && !compare_urls {
        return Ok((None, None));
    }
    let mut statement = connection
        .prepare_cached("SELECT notes, source_urls_json FROM library_root WHERE root_id = ?1")?;
    let mut notes: Option<Option<String>> = None;
    let mut urls: Option<Vec<String>> = None;
    let mut notes_match = compare_notes;
    let mut urls_match = compare_urls;
    for root_id in selection {
        let (current_notes, current_urls): (Option<String>, String) =
            statement.query_row([root_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if notes_match {
            if let Some(expected) = &notes {
                notes_match = expected == &current_notes;
            } else {
                notes = Some(current_notes);
            }
        }
        if urls_match {
            let current_urls = serde_json::from_str::<Vec<String>>(&current_urls)?;
            if let Some(expected) = &urls {
                urls_match = expected == &current_urls;
            } else {
                urls = Some(current_urls);
            }
        }
        if !notes_match && !urls_match {
            break;
        }
    }
    Ok((
        notes_match.then(|| notes.flatten()).flatten(),
        urls_match.then_some(urls).flatten(),
    ))
}
