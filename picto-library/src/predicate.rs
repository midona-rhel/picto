use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};

use crate::model::{FolderId, LabColor, Rating, TagId};
use crate::projection::{cell_center, ProjectionSnapshot};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum FilterExpr {
    All(Vec<FilterExpr>),
    Any(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
    Clause(FilterClause),
}

impl Default for FilterExpr {
    fn default() -> Self {
        Self::All(Vec::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetMatchMode {
    Any,
    All,
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextField {
    Global,
    Notes,
    SourceUrl,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "clause")]
pub enum FilterClause {
    Tags {
        tag_ids: Vec<TagId>,
        mode: SetMatchMode,
    },
    Folders {
        folder_ids: Vec<FolderId>,
        mode: SetMatchMode,
    },
    Ratings {
        ratings: Vec<Rating>,
    },
    Mime {
        values: Vec<String>,
        families: Vec<String>,
    },
    ImportedAt {
        minimum_ms: Option<u64>,
        maximum_ms: Option<u64>,
    },
    ModifiedAt {
        minimum_ms: Option<u64>,
        maximum_ms: Option<u64>,
    },
    Width {
        minimum: Option<u64>,
        maximum: Option<u64>,
    },
    Height {
        minimum: Option<u64>,
        maximum: Option<u64>,
    },
    Duration {
        minimum_ms: Option<u64>,
        maximum_ms: Option<u64>,
    },
    TotalSize {
        minimum_bytes: Option<u64>,
        maximum_bytes: Option<u64>,
    },
    NotesPresent {
        present: bool,
    },
    SourceUrlsPresent {
        present: bool,
    },
    Color {
        color: LabColor,
        delta_e: f32,
    },
    Text {
        field: TextField,
        query: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    ImportedAt,
    CapturedAt,
    Name,
    Rating,
    TotalSize,
    Random,
    FolderOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSort {
    pub field: SortField,
    pub direction: SortDirection,
    pub random_seed: Option<String>,
}

impl Default for ItemSort {
    fn default() -> Self {
        Self {
            field: SortField::ImportedAt,
            direction: SortDirection::Descending,
            random_seed: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ViewQuerySpec {
    #[serde(default)]
    pub filter: FilterExpr,
    #[serde(default)]
    pub sort: ItemSort,
}

pub fn evaluate(
    expression: &FilterExpr,
    universe: &RoaringBitmap,
    snapshot: &ProjectionSnapshot,
    text: &mut impl FnMut(TextField, &str) -> Result<RoaringBitmap>,
) -> Result<RoaringBitmap> {
    match expression {
        FilterExpr::All(children) => {
            let mut result = universe.clone();
            for child in children {
                result &= evaluate(child, universe, snapshot, text)?;
                if result.is_empty() {
                    break;
                }
            }
            Ok(result)
        }
        FilterExpr::Any(children) => {
            let mut result = RoaringBitmap::new();
            for child in children {
                result |= evaluate(child, universe, snapshot, text)?;
            }
            result &= universe;
            Ok(result)
        }
        FilterExpr::Not(child) => {
            let mut result = universe.clone();
            result -= evaluate(child, universe, snapshot, text)?;
            Ok(result)
        }
        FilterExpr::Clause(clause) => evaluate_clause(clause, universe, snapshot, text),
    }
}

fn evaluate_clause(
    clause: &FilterClause,
    universe: &RoaringBitmap,
    snapshot: &ProjectionSnapshot,
    text: &mut impl FnMut(TextField, &str) -> Result<RoaringBitmap>,
) -> Result<RoaringBitmap> {
    let mut result = match clause {
        FilterClause::Tags { tag_ids, mode } => set_match(
            tag_ids
                .iter()
                .map(|id| snapshot.tags.get(id).cloned().unwrap_or_default()),
            *mode,
            universe,
            &snapshot.tag_count,
            tag_ids.len() as u64,
        ),
        FilterClause::Folders { folder_ids, mode } => set_match(
            folder_ids
                .iter()
                .map(|id| snapshot.folders.get(id).cloned().unwrap_or_default()),
            *mode,
            universe,
            &snapshot.folder_count,
            folder_ids.len() as u64,
        ),
        FilterClause::Ratings { ratings } => ratings.iter().fold(
            RoaringBitmap::new(),
            |mut values, rating| {
                values |= snapshot.rating(*rating);
                values
            },
        ),
        FilterClause::Mime { values, families } => {
            let mut matches = RoaringBitmap::new();
            for value in values {
                if let Some(bitmap) = snapshot.mime.get(value) {
                    matches |= bitmap;
                }
            }
            for family in families {
                if let Some(bitmap) = snapshot.mime_family.get(family) {
                    matches |= bitmap;
                }
            }
            matches
        }
        FilterClause::ImportedAt { minimum_ms, maximum_ms } => {
            snapshot.imported_at.between(*minimum_ms, *maximum_ms)
        }
        FilterClause::ModifiedAt { minimum_ms, maximum_ms } => {
            snapshot.modified_at.between(*minimum_ms, *maximum_ms)
        }
        FilterClause::Width { minimum, maximum } => snapshot.width.between(*minimum, *maximum),
        FilterClause::Height { minimum, maximum } => snapshot.height.between(*minimum, *maximum),
        FilterClause::Duration { minimum_ms, maximum_ms } => {
            snapshot.duration.between(*minimum_ms, *maximum_ms)
        }
        FilterClause::TotalSize { minimum_bytes, maximum_bytes } => {
            snapshot.total_bytes.between(*minimum_bytes, *maximum_bytes)
        }
        FilterClause::NotesPresent { present } => presence(
            universe,
            &snapshot.notes_present,
            *present,
        ),
        FilterClause::SourceUrlsPresent { present } => presence(
            universe,
            &snapshot.urls_present,
            *present,
        ),
        FilterClause::Color { color, delta_e } => color_matches(snapshot, color, *delta_e),
        FilterClause::Text { field, query } => text(*field, query)?,
    };
    result &= universe;
    Ok(result)
}

fn set_match(
    values: impl Iterator<Item = RoaringBitmap>,
    mode: SetMatchMode,
    universe: &RoaringBitmap,
    counts: &crate::projection::NumericIndex,
    requested_count: u64,
) -> RoaringBitmap {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return universe.clone();
    }
    let mut result = match mode {
        SetMatchMode::Any => values
            .into_iter()
            .fold(RoaringBitmap::new(), |mut result, value| {
                result |= value;
                result
            }),
        SetMatchMode::All | SetMatchMode::Exact => {
            let mut iterator = values.into_iter();
            let mut result = iterator.next().unwrap_or_default();
            for value in iterator {
                result &= value;
            }
            result
        }
    };
    if mode == SetMatchMode::Exact {
        result &= counts.between(Some(requested_count), Some(requested_count));
    }
    result
}

fn presence(
    universe: &RoaringBitmap,
    present_values: &RoaringBitmap,
    expected: bool,
) -> RoaringBitmap {
    if expected {
        present_values.clone()
    } else {
        let mut result = universe.clone();
        result -= present_values;
        result
    }
}

fn color_matches(
    snapshot: &ProjectionSnapshot,
    requested: &LabColor,
    delta_e: f32,
) -> RoaringBitmap {
    let cell_radius = 7.0_f32;
    let mut candidates = RoaringBitmap::new();
    for (cell, roots) in snapshot.color_cells.iter() {
        let (l, a, b) = cell_center(*cell);
        let distance = ((l - requested.l).powi(2)
            + (a - requested.a).powi(2)
            + (b - requested.b).powi(2))
        .sqrt();
        if distance <= delta_e + cell_radius {
            candidates |= roots;
        }
    }
    candidates
        .into_iter()
        .filter(|root_id| {
            snapshot
                .cover_palettes
                .get(&crate::model::RootId(*root_id))
                .is_some_and(|palette| {
                    palette.iter().any(|color| {
                        ((color.l - requested.l).powi(2)
                            + (color.a - requested.a).powi(2)
                            + (color.b - requested.b).powi(2))
                        .sqrt()
                            <= delta_e
                    })
                })
        })
        .collect()
}
