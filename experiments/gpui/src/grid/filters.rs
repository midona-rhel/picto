//! The grid's filters as the filter bar edits them (`ItemFilters` in
//! `itemFilters.ts`), how they become the library's filter expression
//! (`compileGridQuery`), how many are set (`countActiveGridFilters`), and the
//! labels the bar's controls show (`GridFilterMenu.tsx`).
use picto_library::predicate::{FilterClause, FilterExpr, SetMatchMode};
use picto_library::{FolderId, LabColor, Rating, TagId};

/// `DEFAULT_COLOR_DELTA_E`, `MINIMUM_COLOR_DELTA_E`, `MAXIMUM_COLOR_DELTA_E`.
pub const DEFAULT_DELTA_E: f32 = 16.;
pub const MINIMUM_DELTA_E: f32 = 4.;
pub const MAXIMUM_DELTA_E: f32 = 32.;

const DAY_MS: i64 = 86_400_000;

/// A tag chosen in the filter, with the name its label shows.
#[derive(Clone, Debug, PartialEq)]
pub struct TagChoice {
    pub tag_id: u32,
    pub name: String,
}

/// The filter bar's state. Dates are UTC milliseconds: an "after" is the
/// first moment included, a "before" the first moment left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Filters {
    pub include_tags: Vec<TagChoice>,
    pub exclude_tags: Vec<TagChoice>,
    pub tag_mode: SetMatchMode,
    pub include_folders: Vec<u32>,
    pub exclude_folders: Vec<u32>,
    pub folder_mode: SetMatchMode,
    /// Stars, 0 for unrated, in the order they were chosen.
    pub ratings: Vec<u8>,
    /// MIME types, or families written `image/*`.
    pub include_mimes: Vec<String>,
    pub exclude_mimes: Vec<String>,
    /// `#RRGGBB`.
    pub color: Option<String>,
    pub color_delta_e: f32,
    pub imported_after: Option<i64>,
    pub imported_before: Option<i64>,
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    pub min_duration_ms: Option<u64>,
    pub max_duration_ms: Option<u64>,
    pub min_size_bytes: Option<u64>,
    pub max_size_bytes: Option<u64>,
    pub min_width: Option<u64>,
    pub max_width: Option<u64>,
    pub min_height: Option<u64>,
    pub max_height: Option<u64>,
}

impl Default for Filters {
    /// `createEmptyItemFilters`.
    fn default() -> Self {
        Self {
            include_tags: Vec::new(),
            exclude_tags: Vec::new(),
            tag_mode: SetMatchMode::Any,
            include_folders: Vec::new(),
            exclude_folders: Vec::new(),
            folder_mode: SetMatchMode::Any,
            ratings: Vec::new(),
            include_mimes: Vec::new(),
            exclude_mimes: Vec::new(),
            color: None,
            color_delta_e: DEFAULT_DELTA_E,
            imported_after: None,
            imported_before: None,
            modified_after: None,
            modified_before: None,
            min_duration_ms: None,
            max_duration_ms: None,
            min_size_bytes: None,
            max_size_bytes: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
        }
    }
}

/// The filter fields the bar can show (`PinnedFilter`), in the order of
/// the pin menu and of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Color,
    Tags,
    Folders,
    Rating,
    Type,
    Imported,
    Modified,
    Resolution,
    Duration,
    Size,
}

impl Field {
    pub const ALL: [Field; 10] = [
        Field::Color,
        Field::Tags,
        Field::Folders,
        Field::Rating,
        Field::Type,
        Field::Imported,
        Field::Modified,
        Field::Resolution,
        Field::Duration,
        Field::Size,
    ];
    /// `DEFAULT_PINNED_FILTERS`.
    pub const DEFAULT_PINNED: [Field; 5] = [
        Field::Color,
        Field::Tags,
        Field::Folders,
        Field::Rating,
        Field::Type,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Color => "Color",
            Field::Tags => "Tags",
            Field::Folders => "Folders",
            Field::Rating => "Rating",
            Field::Type => "Type",
            Field::Imported => "Date Imported",
            Field::Modified => "Date Modified",
            Field::Resolution => "Resolution",
            Field::Duration => "Duration",
            Field::Size => "File Size",
        }
    }
}

impl Filters {
    /// `countActiveGridFilters`: each tag, folder, rating and type counts
    /// once; a colour, each date range and each numeric range count once.
    pub fn count(&self) -> usize {
        self.ratings.len()
            + self.include_mimes.len()
            + self.exclude_mimes.len()
            + self.color.is_some() as usize
            + self.include_tags.len()
            + self.exclude_tags.len()
            + self.include_folders.len()
            + self.exclude_folders.len()
            + (self.imported_after.is_some() || self.imported_before.is_some()) as usize
            + (self.modified_after.is_some() || self.modified_before.is_some()) as usize
            + (self.min_duration_ms.is_some() || self.max_duration_ms.is_some()) as usize
            + (self.min_size_bytes.is_some() || self.max_size_bytes.is_some()) as usize
            + self.has_resolution() as usize
    }

    pub fn has_resolution(&self) -> bool {
        self.min_width.is_some()
            || self.max_width.is_some()
            || self.min_height.is_some()
            || self.max_height.is_some()
    }

    /// Whether a field holds a value, which keeps its control on the bar
    /// when it is not pinned.
    pub fn is_set(&self, field: Field) -> bool {
        match field {
            Field::Color => self.color.is_some(),
            Field::Tags => !self.include_tags.is_empty() || !self.exclude_tags.is_empty(),
            Field::Folders => !self.include_folders.is_empty() || !self.exclude_folders.is_empty(),
            Field::Rating => !self.ratings.is_empty(),
            Field::Type => !self.include_mimes.is_empty() || !self.exclude_mimes.is_empty(),
            Field::Imported => self.imported_after.is_some() || self.imported_before.is_some(),
            Field::Modified => self.modified_after.is_some() || self.modified_before.is_some(),
            Field::Resolution => self.has_resolution(),
            Field::Duration => self.min_duration_ms.is_some() || self.max_duration_ms.is_some(),
            Field::Size => self.min_size_bytes.is_some() || self.max_size_bytes.is_some(),
        }
    }

    /// Clears one field, as its control's clear button does.
    pub fn clear(&mut self, field: Field) {
        match field {
            Field::Color => self.color = None,
            Field::Tags => {
                self.include_tags.clear();
                self.exclude_tags.clear();
            }
            Field::Folders => {
                self.include_folders.clear();
                self.exclude_folders.clear();
            }
            Field::Rating => self.ratings.clear(),
            Field::Type => {
                self.include_mimes.clear();
                self.exclude_mimes.clear();
            }
            Field::Imported => {
                self.imported_after = None;
                self.imported_before = None;
            }
            Field::Modified => {
                self.modified_after = None;
                self.modified_before = None;
            }
            Field::Resolution => {
                self.min_width = None;
                self.max_width = None;
                self.min_height = None;
                self.max_height = None;
            }
            Field::Duration => {
                self.min_duration_ms = None;
                self.max_duration_ms = None;
            }
            Field::Size => {
                self.min_size_bytes = None;
                self.max_size_bytes = None;
            }
        }
    }

    /// The bar's filters as `compileGridQuery` builds them, every clause
    /// required. The search text is not part of it; the grid adds it.
    pub fn to_expr(&self) -> FilterExpr {
        let mut values = Vec::new();
        let clause = FilterExpr::Clause;
        let negate = |expr: FilterExpr| FilterExpr::Not(Box::new(expr));
        if !self.include_tags.is_empty() {
            values.push(clause(FilterClause::Tags {
                tag_ids: self
                    .include_tags
                    .iter()
                    .map(|tag| TagId(tag.tag_id))
                    .collect(),
                mode: self.tag_mode,
            }));
        }
        if !self.exclude_tags.is_empty() {
            values.push(negate(clause(FilterClause::Tags {
                tag_ids: self
                    .exclude_tags
                    .iter()
                    .map(|tag| TagId(tag.tag_id))
                    .collect(),
                mode: SetMatchMode::Any,
            })));
        }
        if !self.include_folders.is_empty() {
            values.push(clause(FilterClause::Folders {
                folder_ids: self.include_folders.iter().copied().map(FolderId).collect(),
                mode: self.folder_mode,
            }));
        }
        if !self.exclude_folders.is_empty() {
            values.push(negate(clause(FilterClause::Folders {
                folder_ids: self.exclude_folders.iter().copied().map(FolderId).collect(),
                mode: SetMatchMode::Any,
            })));
        }
        if !self.ratings.is_empty() {
            values.push(clause(FilterClause::Ratings {
                ratings: self.ratings.iter().map(|stars| rating(*stars)).collect(),
            }));
        }
        if !self.include_mimes.is_empty() {
            values.push(clause(mime_clause(&self.include_mimes)));
        }
        if !self.exclude_mimes.is_empty() {
            values.push(negate(clause(mime_clause(&self.exclude_mimes))));
        }
        let ms = |value: Option<i64>| value.map(|value| value.max(0) as u64);
        if self.imported_after.is_some() || self.imported_before.is_some() {
            values.push(clause(FilterClause::ImportedAt {
                minimum_ms: ms(self.imported_after),
                maximum_ms: ms(self.imported_before),
            }));
        }
        if self.modified_after.is_some() || self.modified_before.is_some() {
            values.push(clause(FilterClause::ModifiedAt {
                minimum_ms: ms(self.modified_after),
                maximum_ms: ms(self.modified_before),
            }));
        }
        if self.min_duration_ms.is_some() || self.max_duration_ms.is_some() {
            values.push(clause(FilterClause::Duration {
                minimum_ms: self.min_duration_ms,
                maximum_ms: self.max_duration_ms,
            }));
        }
        if self.min_size_bytes.is_some() || self.max_size_bytes.is_some() {
            values.push(clause(FilterClause::TotalSize {
                minimum_bytes: self.min_size_bytes,
                maximum_bytes: self.max_size_bytes,
            }));
        }
        if self.min_width.is_some() || self.max_width.is_some() {
            values.push(clause(FilterClause::Width {
                minimum: self.min_width,
                maximum: self.max_width,
            }));
        }
        if self.min_height.is_some() || self.max_height.is_some() {
            values.push(clause(FilterClause::Height {
                minimum: self.min_height,
                maximum: self.max_height,
            }));
        }
        if let Some(color) = &self.color {
            values.push(clause(FilterClause::Color {
                color: hex_to_lab(color),
                delta_e: self.color_delta_e.clamp(MINIMUM_DELTA_E, MAXIMUM_DELTA_E),
            }));
        }
        FilterExpr::All(values)
    }

    /// The facet query's filters: these without the type filter, so the
    /// type menu offers every type the rest of the filters leave.
    pub fn without_types(&self) -> Filters {
        Filters {
            include_mimes: Vec::new(),
            exclude_mimes: Vec::new(),
            ..self.clone()
        }
    }

    /// Chooses or unchooses a rating, keeping the order chosen.
    pub fn toggle_rating(&mut self, stars: u8) {
        match self.ratings.iter().position(|value| *value == stars) {
            Some(at) => {
                self.ratings.remove(at);
            }
            None => self.ratings.push(stars),
        }
    }

    /// A left click on a type: includes it, or no longer, and never
    /// excludes it.
    pub fn toggle_mime(&mut self, mime: &str) {
        match self.include_mimes.iter().position(|value| value == mime) {
            Some(at) => {
                self.include_mimes.remove(at);
            }
            None => self.include_mimes.push(mime.to_owned()),
        }
        self.exclude_mimes.retain(|value| value != mime);
    }

    /// A right click on a type: excludes it, or no longer, and never
    /// includes it.
    pub fn toggle_excluded_mime(&mut self, mime: &str) {
        self.include_mimes.retain(|value| value != mime);
        match self.exclude_mimes.iter().position(|value| value == mime) {
            Some(at) => {
                self.exclude_mimes.remove(at);
            }
            None => self.exclude_mimes.push(mime.to_owned()),
        }
    }

    // ── Labels ──────────────────────────────────────────────────

    /// The control's label for a field: its name while empty, else what it
    /// holds.
    pub fn label(&self, field: Field, folder_name: impl Fn(u32) -> Option<String>) -> String {
        match field {
            Field::Color => self
                .color
                .as_ref()
                .map(|hex| hex.to_uppercase())
                .unwrap_or_else(|| "Color".into()),
            Field::Tags => {
                let labels = self
                    .include_tags
                    .iter()
                    .map(|tag| tag.name.clone())
                    .chain(
                        self.exclude_tags
                            .iter()
                            .map(|tag| format!("Not {}", tag.name)),
                    )
                    .collect::<Vec<_>>();
                if labels.is_empty() {
                    "Tags".into()
                } else {
                    labels.join(", ")
                }
            }
            Field::Folders => {
                let name = |id: u32| folder_name(id).unwrap_or_else(|| format!("Folder {id}"));
                let labels = self
                    .include_folders
                    .iter()
                    .map(|id| name(*id))
                    .chain(
                        self.exclude_folders
                            .iter()
                            .map(|id| format!("Not {}", name(*id))),
                    )
                    .collect::<Vec<_>>();
                if labels.is_empty() {
                    "Folders".into()
                } else {
                    labels.join(", ")
                }
            }
            Field::Rating => {
                if self.ratings.is_empty() {
                    "Rating".into()
                } else {
                    self.ratings
                        .iter()
                        .map(|stars| match stars {
                            0 => "Unrated".to_owned(),
                            stars => stars.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            }
            Field::Type => {
                let labels = self
                    .include_mimes
                    .iter()
                    .map(|mime| crate::text::filter_type(mime))
                    .chain(
                        self.exclude_mimes
                            .iter()
                            .map(|mime| format!("-{}", crate::text::filter_type(mime))),
                    )
                    .collect::<Vec<_>>();
                if labels.is_empty() {
                    "Type".into()
                } else {
                    labels.join(", ")
                }
            }
            Field::Imported => {
                date_range_label("Date Imported", self.imported_after, self.imported_before)
            }
            Field::Modified => {
                date_range_label("Date Modified", self.modified_after, self.modified_before)
            }
            Field::Resolution => {
                if !self.has_resolution() {
                    "Resolution".into()
                } else {
                    let bound = |value: Option<u64>, empty: &str| {
                        value.map_or_else(|| empty.to_owned(), |value| value.to_string())
                    };
                    format!(
                        "{}–{} × {}–{}",
                        bound(self.min_width, "0"),
                        bound(self.max_width, "∞"),
                        bound(self.min_height, "0"),
                        bound(self.max_height, "∞"),
                    )
                }
            }
            Field::Duration => range_label(
                "Duration",
                self.min_duration_ms,
                self.max_duration_ms,
                1000,
                "s",
            ),
            Field::Size => range_label(
                "File Size",
                self.min_size_bytes,
                self.max_size_bytes,
                1_000_000,
                "MB",
            ),
        }
    }
}

fn rating(stars: u8) -> Rating {
    match stars {
        1 => Rating::One,
        2 => Rating::Two,
        3 => Rating::Three,
        4 => Rating::Four,
        5 => Rating::Five,
        _ => Rating::Unrated,
    }
}

/// Splits `image/*` families from exact types.
fn mime_clause(entries: &[String]) -> FilterClause {
    let mut values = Vec::new();
    let mut families = Vec::new();
    for entry in entries {
        match entry.strip_suffix("/*") {
            Some(family) => families.push(family.to_owned()),
            None => values.push(entry.clone()),
        }
    }
    FilterClause::Mime { values, families }
}

/// `hexToLab` in `labColor.ts`: sRGB to CIE Lab under D65.
pub fn hex_to_lab(value: &str) -> LabColor {
    let hex = value.trim_start_matches('#');
    let normalized = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        format!("{hex:0<6}").chars().take(6).collect()
    };
    let channel = |offset: usize| {
        let value = u8::from_str_radix(normalized.get(offset..offset + 2).unwrap_or("0"), 16)
            .unwrap_or(0) as f64
            / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (channel(0), channel(2), channel(4));
    let x = (r * 0.4124 + g * 0.3576 + b * 0.1805) / 0.95047;
    let y = r * 0.2126 + g * 0.7152 + b * 0.0722;
    let z = (r * 0.0193 + g * 0.1192 + b * 0.9505) / 1.08883;
    let convert = |part: f64| {
        if part > 0.008856 {
            part.cbrt()
        } else {
            7.787 * part + 16. / 116.
        }
    };
    let (fx, fy, fz) = (convert(x), convert(y), convert(z));
    LabColor {
        l: (116. * fy - 16.) as f32,
        a: (500. * (fx - fy)) as f32,
        b: (200. * (fy - fz)) as f32,
        weight: 1.,
    }
}

/// The type menu's rows: families first, then exact types, each sorted by
/// label; the types offered are the facet's and any already chosen.
pub fn type_rows(facets: &[String], filters: &Filters) -> (Vec<String>, Vec<String>) {
    let chosen = || filters.include_mimes.iter().chain(&filters.exclude_mimes);
    let mut types = facets
        .iter()
        .cloned()
        .chain(chosen().filter(|mime| !mime.ends_with("/*")).cloned())
        .filter(|mime| !mime.is_empty())
        .collect::<Vec<_>>();
    let mut families = facets
        .iter()
        .map(|mime| format!("{}/*", mime.split('/').next().unwrap_or(mime)))
        .chain(chosen().filter(|mime| mime.ends_with("/*")).cloned())
        .collect::<Vec<_>>();
    for list in [&mut types, &mut families] {
        let mut seen = std::collections::HashSet::new();
        list.retain(|mime| seen.insert(mime.clone()));
        list.sort_by_key(|mime| crate::text::filter_type(mime));
    }
    (families, types)
}

/// `scaledValue`: a whole number of units, with up to two decimals.
pub fn scaled_value(value: u64, divisor: u64) -> String {
    let whole = value / divisor;
    let remainder = value % divisor;
    if remainder == 0 {
        return whole.to_string();
    }
    let decimal = format!("{:.2}", remainder as f64 / divisor as f64);
    let digits = decimal.get(2..).unwrap_or("").trim_end_matches('0');
    format!("{whole}.{digits}")
}

/// `rangeLabel`: `min–max unit`, the name while neither end is set.
pub fn range_label(
    label: &str,
    minimum: Option<u64>,
    maximum: Option<u64>,
    divisor: u64,
    unit: &str,
) -> String {
    if minimum.is_none() && maximum.is_none() {
        return label.to_owned();
    }
    let min = minimum.map_or_else(|| "0".to_owned(), |value| scaled_value(value, divisor));
    let max = maximum.map_or_else(|| "∞".to_owned(), |value| scaled_value(value, divisor));
    format!("{min}–{max} {unit}")
}

/// A UTC day as `YYYY-MM-DD`.
pub fn day_key(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|date| date.format(crate::text::date_format::DAY).to_string())
        .unwrap_or_default()
}

/// Midnight UTC of a `YYYY-MM-DD` day.
pub fn day_start(key: &str) -> Option<i64> {
    let date = chrono::NaiveDate::parse_from_str(key, crate::text::date_format::DAY).ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

/// `dateRangeLabel`: the first and last days included, `…` for an open
/// end; the name while neither is set.
pub fn date_range_label(label: &str, after: Option<i64>, before: Option<i64>) -> String {
    if after.is_none() && before.is_none() {
        return label.to_owned();
    }
    let first = after.map_or_else(|| "…".to_owned(), day_key);
    let last = before.map_or_else(|| "…".to_owned(), |before| day_key(before - DAY_MS));
    format!("{first}–{last}")
}

/// The presets of a date menu (`setDays`): a span of days ending at the
/// start of the day `end_offset` days after today, in UTC.
pub fn date_preset(now_ms: i64, days: i64, end_offset: i64) -> (i64, i64) {
    let today = now_ms.div_euclid(DAY_MS) * DAY_MS;
    let end = today + end_offset * DAY_MS;
    (end - days * DAY_MS, end)
}

/// The date menu's presets: label, days, and the end's offset from today.
pub const DATE_PRESETS: [(&str, i64, i64); 6] = [
    ("Today", 1, 1),
    ("Yesterday", 1, 0),
    ("Last 7 Days", 7, 1),
    ("Last 30 Days", 30, 1),
    ("Last 90 Days", 90, 1),
    ("Last 365 Days", 365, 1),
];

/// A date range editor's fields as days: the first included day and the
/// last included day.
pub fn range_days(after: Option<i64>, before: Option<i64>) -> (String, String) {
    (
        after.map(day_key).unwrap_or_default(),
        before
            .map(|before| day_key(before - DAY_MS))
            .unwrap_or_default(),
    )
}

/// The range a date editor commits for its two days: from the start of the
/// first to the start of the day after the last.
pub fn range_from_days(first: &str, last: &str) -> (Option<i64>, Option<i64>) {
    (
        day_start(first),
        day_start(last).map(|start| start + DAY_MS),
    )
}

/// `integerOrNull`: a non-negative number typed in a range field, in the
/// unit chosen, as a whole number of the base unit; empty or invalid is no
/// bound.
pub fn amount(text: &str, multiplier: u64) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let value = text.parse::<f64>().ok()?;
    (value.is_finite() && value >= 0.).then(|| (value * multiplier as f64).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: u32, name: &str) -> TagChoice {
        TagChoice {
            tag_id: id,
            name: name.into(),
        }
    }

    #[test]
    fn empty_filters_are_an_empty_conjunction() {
        let filters = Filters::default();
        assert_eq!(filters.count(), 0);
        assert_eq!(filters.to_expr(), FilterExpr::All(Vec::new()));
        assert_eq!(filters.color_delta_e, DEFAULT_DELTA_E);
    }

    #[test]
    fn the_expression_follows_compile_grid_query() {
        let filters = Filters {
            include_tags: vec![tag(1, "cat"), tag(2, "dog")],
            exclude_tags: vec![tag(3, "bird")],
            tag_mode: SetMatchMode::All,
            include_folders: vec![7],
            exclude_folders: vec![8],
            folder_mode: SetMatchMode::Exact,
            ratings: vec![5, 0],
            include_mimes: vec!["image/*".into(), "video/mp4".into()],
            exclude_mimes: vec!["image/gif".into()],
            imported_after: Some(1000),
            min_size_bytes: Some(5),
            max_width: Some(1920),
            min_height: Some(100),
            color: Some("#FF0000".into()),
            color_delta_e: 99.,
            ..Filters::default()
        };
        let FilterExpr::All(values) = filters.to_expr() else {
            panic!("a conjunction");
        };
        let not = |clause| FilterExpr::Not(Box::new(FilterExpr::Clause(clause)));
        assert_eq!(
            values[0],
            FilterExpr::Clause(FilterClause::Tags {
                tag_ids: vec![TagId(1), TagId(2)],
                mode: SetMatchMode::All
            })
        );
        // Excluded tags and folders leave out any of them.
        assert_eq!(
            values[1],
            not(FilterClause::Tags {
                tag_ids: vec![TagId(3)],
                mode: SetMatchMode::Any
            })
        );
        assert_eq!(
            values[2],
            FilterExpr::Clause(FilterClause::Folders {
                folder_ids: vec![FolderId(7)],
                mode: SetMatchMode::Exact
            })
        );
        assert_eq!(
            values[3],
            not(FilterClause::Folders {
                folder_ids: vec![FolderId(8)],
                mode: SetMatchMode::Any
            })
        );
        assert_eq!(
            values[4],
            FilterExpr::Clause(FilterClause::Ratings {
                ratings: vec![Rating::Five, Rating::Unrated]
            })
        );
        assert_eq!(
            values[5],
            FilterExpr::Clause(FilterClause::Mime {
                values: vec!["video/mp4".into()],
                families: vec!["image".into()]
            })
        );
        assert_eq!(
            values[6],
            not(FilterClause::Mime {
                values: vec!["image/gif".into()],
                families: vec![]
            })
        );
        assert_eq!(
            values[7],
            FilterExpr::Clause(FilterClause::ImportedAt {
                minimum_ms: Some(1000),
                maximum_ms: None
            })
        );
        assert_eq!(
            values[8],
            FilterExpr::Clause(FilterClause::TotalSize {
                minimum_bytes: Some(5),
                maximum_bytes: None
            })
        );
        assert_eq!(
            values[9],
            FilterExpr::Clause(FilterClause::Width {
                minimum: None,
                maximum: Some(1920)
            })
        );
        assert_eq!(
            values[10],
            FilterExpr::Clause(FilterClause::Height {
                minimum: Some(100),
                maximum: None
            })
        );
        // The tolerance is held to its range.
        let FilterExpr::Clause(FilterClause::Color { color, delta_e }) = &values[11] else {
            panic!("a colour clause last");
        };
        assert_eq!(*delta_e, MAXIMUM_DELTA_E);
        assert!(
            (color.l - 53.24).abs() < 0.1 && (color.a - 80.).abs() < 0.2,
            "{color:?}"
        );
        assert_eq!(values.len(), 12);
    }

    #[test]
    fn counting_counts_values_and_ranges() {
        let mut filters = Filters {
            include_tags: vec![tag(1, "a"), tag(2, "b")],
            exclude_folders: vec![4],
            ratings: vec![3],
            color: Some("#000000".into()),
            imported_before: Some(5),
            min_duration_ms: Some(1),
            max_duration_ms: Some(2),
            max_height: Some(5),
            ..Filters::default()
        };
        // Two tags, a folder, a rating, a colour, a date range, a duration
        // range and a resolution range.
        assert_eq!(filters.count(), 8);
        filters.clear(Field::Tags);
        filters.clear(Field::Resolution);
        assert_eq!(filters.count(), 5);
        assert!(!filters.is_set(Field::Tags));
        assert!(filters.is_set(Field::Duration));
    }

    #[test]
    fn toggling_types_and_ratings() {
        let mut filters = Filters::default();
        filters.toggle_mime("image/png");
        filters.toggle_excluded_mime("image/gif");
        assert_eq!(filters.include_mimes, ["image/png"]);
        assert_eq!(filters.exclude_mimes, ["image/gif"]);
        // Excluding an included type moves it; including an excluded one too.
        filters.toggle_excluded_mime("image/png");
        assert!(filters.include_mimes.is_empty());
        filters.toggle_mime("image/gif");
        assert_eq!(filters.include_mimes, ["image/gif"]);
        assert_eq!(filters.exclude_mimes, ["image/png"]);
        filters.toggle_rating(4);
        filters.toggle_rating(0);
        filters.toggle_rating(4);
        assert_eq!(filters.ratings, [0]);
    }

    #[test]
    fn labels_follow_the_bar() {
        let filters = Filters {
            include_tags: vec![tag(1, "cat")],
            exclude_tags: vec![tag(2, "dog")],
            include_folders: vec![1],
            exclude_folders: vec![9],
            ratings: vec![5, 0],
            include_mimes: vec!["image/jpeg".into()],
            exclude_mimes: vec!["video/*".into()],
            min_size_bytes: Some(1_500_000),
            max_duration_ms: Some(90_000),
            min_width: Some(800),
            max_height: Some(600),
            color: Some("#ff8800".into()),
            ..Filters::default()
        };
        let names = |id: u32| (id == 1).then(|| "Wallpapers".to_owned());
        assert_eq!(filters.label(Field::Tags, names), "cat, Not dog");
        assert_eq!(
            filters.label(Field::Folders, names),
            "Wallpapers, Not Folder 9"
        );
        assert_eq!(filters.label(Field::Rating, names), "5, Unrated");
        assert_eq!(filters.label(Field::Type, names), "JPG, -Videos");
        assert_eq!(filters.label(Field::Size, names), "1.5–∞ MB");
        assert_eq!(filters.label(Field::Duration, names), "0–90 s");
        assert_eq!(filters.label(Field::Resolution, names), "800–∞ × 0–600");
        assert_eq!(filters.label(Field::Color, names), "#FF8800");
        assert_eq!(filters.label(Field::Imported, names), "Date Imported");
        assert_eq!(Filters::default().label(Field::Tags, names), "Tags");
    }

    #[test]
    fn type_rows_put_families_first_and_keep_chosen_types() {
        let filters = Filters {
            include_mimes: vec!["audio/flac".into()],
            exclude_mimes: vec!["font/*".into()],
            ..Filters::default()
        };
        let facets = [
            "image/png".to_owned(),
            "image/jpeg".into(),
            "video/mp4".into(),
        ];
        let (families, types) = type_rows(&facets, &filters);
        assert_eq!(families, ["font/*", "image/*", "video/*"]);
        assert_eq!(
            types,
            ["audio/flac", "image/jpeg", "video/mp4", "image/png"]
        );
    }

    #[test]
    fn scaled_values_and_amounts() {
        assert_eq!(scaled_value(3000, 1000), "3");
        assert_eq!(scaled_value(1250, 1000), "1.25");
        assert_eq!(scaled_value(2_500_000, 1_000_000), "2.5");
        assert_eq!(amount("2.5", 1_000_000), Some(2_500_000));
        assert_eq!(amount(" 3 ", 60_000), Some(180_000));
        assert_eq!(amount("", 1), None);
        assert_eq!(amount("-1", 1), None);
        assert_eq!(amount("abc", 1), None);
    }

    #[test]
    fn dates_are_whole_utc_days() {
        // 2025-11-13 15:00 UTC.
        let now = 1_763_046_000_000;
        assert_eq!(day_key(now), "2025-11-13");
        let (start, end) = date_preset(now, 1, 1);
        assert_eq!(
            (day_key(start), day_key(end)),
            ("2025-11-13".into(), "2025-11-14".into())
        );
        let (start, end) = date_preset(now, 1, 0);
        assert_eq!(
            (day_key(start), day_key(end)),
            ("2025-11-12".into(), "2025-11-13".into())
        );
        // Seven days: today and the six before it.
        let (start, end) = date_preset(now, 7, 1);
        assert_eq!(day_key(start), "2025-11-07");
        // The label shows the last day included.
        assert_eq!(
            date_range_label("Date Imported", Some(start), Some(end)),
            "2025-11-07–2025-11-13"
        );
        assert_eq!(date_range_label("x", None, Some(end)), "…–2025-11-13");
        let (after, before) = range_from_days("2025-01-01", "2025-01-31");
        assert_eq!(
            range_days(after, before),
            ("2025-01-01".into(), "2025-01-31".into())
        );
        assert_eq!(range_from_days("", "nope"), (None, None));
    }

    #[test]
    fn hex_to_lab_matches_lab_color_ts() {
        let white = hex_to_lab("#FFFFFF");
        assert!((white.l - 100.).abs() < 0.01 && white.a.abs() < 0.05 && white.b.abs() < 0.05);
        let black = hex_to_lab("000");
        assert!(black.l.abs() < 0.01);
        let blue = hex_to_lab("#2B76E7");
        assert!((blue.l - 50.6).abs() < 0.5);
    }
}
