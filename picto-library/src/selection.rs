use roaring::RoaringBitmap;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::model::{FolderId, Rating, RootId, TagId};
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
    pub shared_tags: Vec<TagId>,
    pub shared_folders: Vec<FolderId>,
    pub shared_notes: Option<String>,
    pub shared_source_urls: Option<Vec<String>>,
    pub revision: u64,
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
    selection: &RoaringBitmap,
) -> Result<SelectionSummary> {
    let shared_rating = Rating::ALL
        .into_iter()
        .find(|rating| (snapshot.rating(*rating) & selection).len() == selection.len());
    let first = selection.min();
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
    Ok(SelectionSummary {
        selected_count: selection.len(),
        total_size_bytes: snapshot.total_bytes.sum(selection),
        media_count: snapshot.media_count.sum(selection),
        shared_rating,
        shared_tags,
        shared_folders,
        shared_notes,
        shared_source_urls,
        revision: snapshot.revision,
    })
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
