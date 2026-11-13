//! The smart-folder rule model (`fieldConfig.ts`, `queryModel.ts`): groups
//! of rules as the editor shows them, compiled to the library's
//! `FilterExpr` and read back from it.
use crate::grid::filters::{day_key, day_start, hex_to_lab};
use picto_library::predicate::{FilterClause, FilterExpr, SetMatchMode, TextField};
use picto_library::{LabColor, Rating, TagId};

/// `MAX_LOCAL_RULES`.
pub const MAX_RULES: usize = 10;
/// `DEFAULT_COLOR_DELTA_E`.
const COLOR_DELTA_E: f32 = 16.;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Tags,
    Text,
    Number,
    Date,
    Select,
    FileSize,
    Color,
}

/// One field a rule can test (`FieldDef`).
pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
    pub operators: &'static [(&'static str, &'static str)],
    pub kind: ValueType,
    pub unit: Option<&'static str>,
}

const TAG_OPERATORS: &[(&str, &str)] = &[
    ("include_all", "include all"),
    ("include_any", "include any"),
    ("do_not_include", "do not include"),
];
const NUMERIC_OPERATORS: &[(&str, &str)] = &[
    ("eq", "equals"),
    ("neq", "not equals"),
    ("gt", "greater than"),
    ("gte", "at least"),
    ("lt", "less than"),
    ("lte", "at most"),
    ("between", "between"),
];
const TEXT_OPERATORS: &[(&str, &str)] = &[
    ("contains", "contains"),
    ("is_empty", "is empty"),
    ("is_not_empty", "is not empty"),
];
const DATE_OPERATORS: &[(&str, &str)] = &[
    ("eq", "is"),
    ("gt", "after"),
    ("gte", "on or after"),
    ("lt", "before"),
    ("lte", "on or before"),
    ("between", "between"),
];

/// `FIELD_DEFS`, in the editor's order.
pub const FIELDS: &[FieldDef] = &[
    FieldDef {
        key: "tags",
        label: "Tags",
        operators: TAG_OPERATORS,
        kind: ValueType::Tags,
        unit: None,
    },
    FieldDef {
        key: "file_type",
        label: "File Type",
        operators: &[("is", "is"), ("is_not", "is not")],
        kind: ValueType::Select,
        unit: None,
    },
    FieldDef {
        key: "rating",
        label: "Rating",
        operators: NUMERIC_OPERATORS,
        kind: ValueType::Number,
        unit: None,
    },
    FieldDef {
        key: "file_size",
        label: "File Size",
        operators: NUMERIC_OPERATORS,
        kind: ValueType::FileSize,
        unit: None,
    },
    FieldDef {
        key: "date_added",
        label: "Date Imported",
        operators: DATE_OPERATORS,
        kind: ValueType::Date,
        unit: None,
    },
    FieldDef {
        key: "date_created",
        label: "Date Created",
        operators: DATE_OPERATORS,
        kind: ValueType::Date,
        unit: None,
    },
    FieldDef {
        key: "date_modified",
        label: "Date Modified",
        operators: DATE_OPERATORS,
        kind: ValueType::Date,
        unit: None,
    },
    FieldDef {
        key: "name",
        label: "Name",
        operators: &[("contains", "contains")],
        kind: ValueType::Text,
        unit: None,
    },
    FieldDef {
        key: "width",
        label: "Width",
        operators: NUMERIC_OPERATORS,
        kind: ValueType::Number,
        unit: Some("px"),
    },
    FieldDef {
        key: "height",
        label: "Height",
        operators: NUMERIC_OPERATORS,
        kind: ValueType::Number,
        unit: Some("px"),
    },
    FieldDef {
        key: "duration",
        label: "Duration",
        operators: NUMERIC_OPERATORS,
        kind: ValueType::Number,
        unit: Some("s"),
    },
    FieldDef {
        key: "notes",
        label: "Notes",
        operators: TEXT_OPERATORS,
        kind: ValueType::Text,
        unit: None,
    },
    FieldDef {
        key: "source_url",
        label: "Source URL",
        operators: TEXT_OPERATORS,
        kind: ValueType::Text,
        unit: None,
    },
    FieldDef {
        key: "color",
        label: "Color",
        operators: &[("contains", "contains")],
        kind: ValueType::Color,
        unit: None,
    },
];

/// The file-type choices (`selectOptions`).
pub const FILE_TYPES: &[(&str, &str)] = &[
    ("image", "Image"),
    ("video", "Video"),
    ("audio", "Audio"),
    ("image/png", "PNG"),
    ("image/jpeg", "JPEG"),
    ("video/mp4", "MP4"),
];

/// `FILESIZE_UNITS`.
pub const FILE_SIZE_UNITS: &[(&str, &str)] = &[
    ("B", "Bytes"),
    ("KB", "Kilobytes"),
    ("MB", "Megabytes"),
    ("GB", "Gigabytes"),
];

/// `RATING_OPTIONS`.
pub const RATING_OPTIONS: &[(&str, &str)] = &[
    ("0", "Unrated"),
    ("1", "★☆☆☆☆"),
    ("2", "★★☆☆☆"),
    ("3", "★★★☆☆"),
    ("4", "★★★★☆"),
    ("5", "★★★★★"),
];

/// `getFieldDef`: the tags field for a key it does not know.
pub fn field(key: &str) -> &'static FieldDef {
    FIELDS
        .iter()
        .find(|field| field.key == key)
        .unwrap_or(&FIELDS[0])
}

/// One rule (`SmartFolderPredicateRule`). Values are kept as the editor
/// shows them and read when the rule is compiled.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub field: &'static str,
    pub op: &'static str,
    pub value: String,
    pub value2: Option<String>,
    pub values: Vec<String>,
    pub unit: Option<&'static str>,
}

/// One group of rules (`SmartFolderPredicateGroup`).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub any: bool,
    pub negate: bool,
    pub rules: Vec<Rule>,
}

/// `defaultValue`, as text.
fn default_value(key: &str, today: &str) -> String {
    match field(key).kind {
        ValueType::Tags | ValueType::Color | ValueType::Text => String::new(),
        ValueType::Number | ValueType::FileSize => "0".into(),
        ValueType::Date => today.to_owned(),
        ValueType::Select => FILE_TYPES[0].0.into(),
    }
}

impl Rule {
    /// The rule a field starts as when chosen (`handleFieldChange`).
    pub fn for_field(key: &str, today: &str) -> Self {
        let field = field(key);
        Self {
            field: field.key,
            op: field.operators[0].0,
            value: default_value(field.key, today),
            value2: None,
            values: Vec::new(),
            unit: (field.key == "file_size").then_some("MB"),
        }
    }

    /// `handleOpChange`: `between` takes a second value, starting at the
    /// first.
    pub fn with_op(&self, op: &'static str) -> Self {
        Self {
            op,
            value2: (op == "between").then(|| self.value2.clone().unwrap_or(self.value.clone())),
            ..self.clone()
        }
    }

    /// Whether the operator takes a value (`is empty` does not).
    pub fn shows_value(&self) -> bool {
        !matches!(self.op, "is_empty" | "is_not_empty")
    }
}

/// `defaultRule`.
pub fn default_rule() -> Rule {
    Rule {
        field: "tags",
        op: "include_any",
        value: String::new(),
        value2: None,
        values: Vec::new(),
        unit: None,
    }
}

/// `defaultGroup`.
pub fn default_group() -> Group {
    Group {
        any: false,
        negate: false,
        rules: vec![default_rule()],
    }
}

/// `normalizeRule`: an operator the field does not have becomes its first.
fn normalize_rule(mut rule: Rule) -> Rule {
    let def = field(rule.field);
    rule.field = def.key;
    rule.op = def
        .operators
        .iter()
        .find(|(op, _)| *op == rule.op)
        .map(|(op, _)| *op)
        .unwrap_or(def.operators[0].0);
    if def.key == "file_size" && rule.unit.is_none() {
        rule.unit = Some("MB");
    }
    if def.key != "file_size" {
        rule.unit = None;
    }
    rule
}

/// `normalizePredicate`: at least one group, each with at least one rule.
pub fn normalize(groups: Vec<Group>) -> Vec<Group> {
    if groups.is_empty() {
        return vec![default_group()];
    }
    groups
        .into_iter()
        .map(|group| Group {
            rules: if group.rules.is_empty() {
                vec![default_rule()]
            } else {
                group.rules.into_iter().map(normalize_rule).collect()
            },
            ..group
        })
        .collect()
}

// ── Compiling ──────────────────────────────────────────────────────

fn number(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        0.
    } else {
        trimmed.parse::<f64>().unwrap_or(0.)
    }
}

struct Range {
    minimum: Option<u64>,
    maximum: Option<u64>,
    negate: bool,
}

/// `numberRange`.
fn number_range(rule: &Rule, scale: f64) -> Result<Range, String> {
    let value = (number(&rule.value) * scale).round().max(0.) as u64;
    let value2 = (number(rule.value2.as_deref().unwrap_or(&rule.value)) * scale)
        .round()
        .max(0.) as u64;
    Ok(match rule.op {
        "eq" => Range {
            minimum: Some(value),
            maximum: Some(value),
            negate: false,
        },
        "neq" => Range {
            minimum: Some(value),
            maximum: Some(value),
            negate: true,
        },
        "gt" => Range {
            minimum: Some(value + 1),
            maximum: None,
            negate: false,
        },
        "gte" => Range {
            minimum: Some(value),
            maximum: None,
            negate: false,
        },
        "lt" => Range {
            minimum: None,
            maximum: Some(value.saturating_sub(1)),
            negate: false,
        },
        "lte" => Range {
            minimum: None,
            maximum: Some(value),
            negate: false,
        },
        "between" => Range {
            minimum: Some(value.min(value2)),
            maximum: Some(value.max(value2)),
            negate: false,
        },
        op => return Err(format!("Unsupported numeric operator: {op}")),
    })
}

/// Today in UTC, as the date field's default.
pub fn today() -> String {
    day_key(crate::now_ms())
}

/// `dateRange`.
fn date_range(rule: &Rule) -> Result<(Option<u64>, Option<u64>), String> {
    let parse =
        |text: &str| day_start(text.trim()).ok_or("Smart-folder date is invalid.".to_owned());
    let value = parse(&rule.value)?;
    let value2 = parse(rule.value2.as_deref().unwrap_or(&rule.value))?;
    let clamp = |ms: i64| Some(ms.max(0) as u64);
    Ok(match rule.op {
        "eq" => (clamp(value), clamp(value + DAY_MS - 1)),
        "gt" => (clamp(value + DAY_MS), None),
        "gte" => (clamp(value), None),
        "lt" => (None, clamp(value - 1)),
        "lte" => (None, clamp(value + DAY_MS - 1)),
        "between" => (
            clamp(value.min(value2)),
            clamp(value.max(value2) + DAY_MS - 1),
        ),
        op => return Err(format!("Unsupported date operator: {op}")),
    })
}

fn wrap_not(expression: FilterExpr, negate: bool) -> FilterExpr {
    if negate {
        FilterExpr::Not(Box::new(expression))
    } else {
        expression
    }
}

const RATINGS: [Rating; 6] = Rating::ALL;

/// `compileRule`.
fn compile_rule(
    rule: &Rule,
    resolve_tag: &dyn Fn(&str) -> Option<TagId>,
) -> Result<FilterExpr, String> {
    let mut negate = false;
    let clause = match rule.field {
        "tags" => {
            let tag_ids = rule
                .values
                .iter()
                .map(|name| {
                    resolve_tag(name).ok_or(format!(
                        "Smart folder references a tag that does not exist: {name}"
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;
            negate = rule.op == "do_not_include";
            FilterClause::Tags {
                tag_ids,
                mode: if rule.op == "include_all" {
                    SetMatchMode::All
                } else {
                    SetMatchMode::Any
                },
            }
        }
        "file_type" => {
            let exact = rule.value.contains('/');
            negate = rule.op == "is_not";
            FilterClause::Mime {
                values: if exact {
                    vec![rule.value.clone()]
                } else {
                    Vec::new()
                },
                families: if exact {
                    Vec::new()
                } else {
                    vec![rule.value.clone()]
                },
            }
        }
        "rating" => {
            let range = number_range(rule, 1.)?;
            negate = range.negate;
            FilterClause::Ratings {
                ratings: RATINGS
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        let index = *index as u64;
                        range.minimum.is_none_or(|minimum| index >= minimum)
                            && range.maximum.is_none_or(|maximum| index <= maximum)
                    })
                    .map(|(_, rating)| *rating)
                    .collect(),
            }
        }
        "file_size" => {
            let scale = match rule.unit.unwrap_or("MB") {
                "B" => 1.,
                "KB" => 1024.,
                "GB" => 1024. * 1024. * 1024.,
                _ => 1024. * 1024.,
            };
            let range = number_range(rule, scale)?;
            negate = range.negate;
            FilterClause::TotalSize {
                minimum_bytes: range.minimum,
                maximum_bytes: range.maximum,
            }
        }
        "date_added" => {
            let (minimum_ms, maximum_ms) = date_range(rule)?;
            FilterClause::ImportedAt {
                minimum_ms,
                maximum_ms,
            }
        }
        "date_created" => {
            let (minimum_ms, maximum_ms) = date_range(rule)?;
            FilterClause::CapturedAt {
                minimum_ms,
                maximum_ms,
            }
        }
        "date_modified" => {
            let (minimum_ms, maximum_ms) = date_range(rule)?;
            FilterClause::ModifiedAt {
                minimum_ms,
                maximum_ms,
            }
        }
        "width" | "height" => {
            let range = number_range(rule, 1.)?;
            negate = range.negate;
            if rule.field == "width" {
                FilterClause::Width {
                    minimum: range.minimum,
                    maximum: range.maximum,
                }
            } else {
                FilterClause::Height {
                    minimum: range.minimum,
                    maximum: range.maximum,
                }
            }
        }
        "duration" => {
            let range = number_range(rule, 1000.)?;
            negate = range.negate;
            FilterClause::Duration {
                minimum_ms: range.minimum,
                maximum_ms: range.maximum,
            }
        }
        "name" => FilterClause::Text {
            field: TextField::Name,
            query: rule.value.clone(),
        },
        "notes" | "source_url" => {
            let present = match rule.op {
                "is_empty" => Some(false),
                "is_not_empty" => Some(true),
                _ => None,
            };
            match (rule.field, present) {
                ("notes", Some(present)) => FilterClause::NotesPresent { present },
                ("notes", None) => FilterClause::Text {
                    field: TextField::Notes,
                    query: rule.value.clone(),
                },
                (_, Some(present)) => FilterClause::SourceUrlsPresent { present },
                (_, None) => FilterClause::Text {
                    field: TextField::SourceUrl,
                    query: rule.value.clone(),
                },
            }
        }
        "color" => {
            let value = rule
                .values
                .first()
                .ok_or("Smart-folder color is empty.".to_owned())?;
            FilterClause::Color {
                color: hex_to_lab(value),
                delta_e: COLOR_DELTA_E,
            }
        }
        other => return Err(format!("Unsupported smart-folder field: {other}")),
    };
    Ok(wrap_not(FilterExpr::Clause(clause), negate))
}

/// `compileSmartFolderPredicate`: every group must match; a group matches
/// all or any of its rules, or is excluded.
pub fn compile(
    groups: &[Group],
    resolve_tag: &dyn Fn(&str) -> Option<TagId>,
) -> Result<FilterExpr, String> {
    let groups = groups
        .iter()
        .map(|group| {
            let rules = group
                .rules
                .iter()
                .map(|rule| compile_rule(rule, resolve_tag))
                .collect::<Result<Vec<_>, _>>()?;
            let expression = if group.any {
                FilterExpr::Any(rules)
            } else {
                FilterExpr::All(rules)
            };
            Ok(wrap_not(expression, group.negate))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(FilterExpr::All(groups))
}

// ── Reading back ───────────────────────────────────────────────────

/// `String(number)`: whole numbers without a fraction.
fn number_text(value: f64) -> String {
    if value.fract() == 0. {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn rule(field: &'static str, op: &'static str, value: String) -> Rule {
    Rule {
        field,
        op,
        value,
        value2: None,
        values: Vec::new(),
        unit: None,
    }
}

/// `rangeRule`.
fn range_rule(field: &'static str, minimum: Option<u64>, maximum: Option<u64>, scale: f64) -> Rule {
    let text = |value: u64| number_text(value as f64 / scale);
    match (minimum, maximum) {
        (Some(minimum), Some(maximum)) if minimum == maximum => rule(field, "eq", text(minimum)),
        (Some(minimum), Some(maximum)) => Rule {
            value2: Some(text(maximum)),
            ..rule(field, "between", text(minimum))
        },
        (Some(minimum), None) => rule(field, "gte", text(minimum)),
        (None, maximum) => rule(field, "lte", text(maximum.unwrap_or(0))),
    }
}

/// `dateRule`.
fn date_rule(field: &'static str, minimum: Option<u64>, maximum: Option<u64>) -> Rule {
    let date = |ms: u64| day_key(ms as i64);
    match (minimum, maximum) {
        (Some(minimum), Some(maximum)) if maximum.saturating_sub(minimum) < DAY_MS as u64 => {
            rule(field, "eq", date(minimum))
        }
        (Some(minimum), Some(maximum)) => Rule {
            value2: Some(date(maximum)),
            ..rule(field, "between", date(minimum))
        },
        (Some(minimum), None) => rule(field, "gte", date(minimum)),
        (None, maximum) => rule(field, "lte", date(maximum.unwrap_or(0))),
    }
}

/// `ruleFromClause`.
fn rule_from_clause(
    clause: &FilterClause,
    tag_name: &dyn Fn(TagId) -> Option<String>,
) -> Result<Rule, String> {
    Ok(match clause {
        FilterClause::Tags { tag_ids, mode } => Rule {
            values: tag_ids
                .iter()
                .map(|id| tag_name(*id).unwrap_or(id.0.to_string()))
                .collect(),
            ..rule(
                "tags",
                if *mode == SetMatchMode::All {
                    "include_all"
                } else {
                    "include_any"
                },
                String::new(),
            )
        },
        FilterClause::Mime { values, families } => rule(
            "file_type",
            "is",
            values
                .first()
                .or(families.first())
                .cloned()
                .unwrap_or("image".into()),
        ),
        FilterClause::Ratings { ratings } => {
            let mut indexes = ratings
                .iter()
                .filter_map(|rating| RATINGS.iter().position(|known| known == rating))
                .map(|index| index as u64)
                .collect::<Vec<_>>();
            indexes.sort_unstable();
            range_rule(
                "rating",
                Some(indexes.first().copied().unwrap_or(0)),
                Some(indexes.last().copied().unwrap_or(0)),
                1.,
            )
        }
        FilterClause::ImportedAt {
            minimum_ms,
            maximum_ms,
        } => date_rule("date_added", *minimum_ms, *maximum_ms),
        FilterClause::CapturedAt {
            minimum_ms,
            maximum_ms,
        } => date_rule("date_created", *minimum_ms, *maximum_ms),
        FilterClause::ModifiedAt {
            minimum_ms,
            maximum_ms,
        } => date_rule("date_modified", *minimum_ms, *maximum_ms),
        FilterClause::Width { minimum, maximum } => range_rule("width", *minimum, *maximum, 1.),
        FilterClause::Height { minimum, maximum } => range_rule("height", *minimum, *maximum, 1.),
        FilterClause::Duration {
            minimum_ms,
            maximum_ms,
        } => range_rule("duration", *minimum_ms, *maximum_ms, 1000.),
        FilterClause::TotalSize {
            minimum_bytes,
            maximum_bytes,
        } => Rule {
            unit: Some("MB"),
            ..range_rule("file_size", *minimum_bytes, *maximum_bytes, 1024. * 1024.)
        },
        FilterClause::NotesPresent { present } => rule(
            "notes",
            if *present { "is_not_empty" } else { "is_empty" },
            String::new(),
        ),
        FilterClause::SourceUrlsPresent { present } => rule(
            "source_url",
            if *present { "is_not_empty" } else { "is_empty" },
            String::new(),
        ),
        FilterClause::Color { color, .. } => Rule {
            values: vec![lab_to_hex(color)],
            ..rule("color", "contains", String::new())
        },
        FilterClause::Text { field, query } => match field {
            TextField::Global => {
                return Err("Global text rules are not exposed by the smart-folder editor.".into());
            }
            TextField::Name => rule("name", "contains", query.clone()),
            TextField::Notes => rule("notes", "contains", query.clone()),
            TextField::SourceUrl => rule("source_url", "contains", query.clone()),
        },
        FilterClause::Folders { .. } => {
            return Err("Folder rules are not exposed by the smart-folder editor.".into());
        }
    })
}

/// `unwrapGroup`.
fn group_from(
    expression: &FilterExpr,
    tag_name: &dyn Fn(TagId) -> Option<String>,
) -> Result<Group, String> {
    let (negate, inner) = match expression {
        FilterExpr::Not(inner) => (true, inner.as_ref()),
        other => (false, other),
    };
    let children = match inner {
        FilterExpr::All(children) | FilterExpr::Any(children) => children.iter().collect(),
        other => vec![other],
    };
    let rules = children
        .into_iter()
        .map(|child| {
            let (negated, clause) = match child {
                FilterExpr::Not(inner) => (true, inner.as_ref()),
                other => (false, other),
            };
            let FilterExpr::Clause(clause) = clause else {
                return Err("Nested smart-folder expression cannot be edited.".to_owned());
            };
            let mut rule = rule_from_clause(clause, tag_name)?;
            if negated {
                rule.op = match (rule.field, rule.op) {
                    ("tags", _) => "do_not_include",
                    ("file_type", _) => "is_not",
                    (_, "eq") => "neq",
                    _ => return Err("Negated smart-folder rule cannot be edited.".into()),
                };
            }
            Ok(rule)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Group {
        any: matches!(inner, FilterExpr::Any(_)),
        negate,
        rules,
    })
}

/// `editorPredicateFromFilter`, normalized as the editor opens it.
pub fn from_filter(
    filter: &FilterExpr,
    tag_name: &dyn Fn(TagId) -> Option<String>,
) -> Result<Vec<Group>, String> {
    let expressions = match filter {
        FilterExpr::All(children) => children.iter().collect(),
        other => vec![other],
    };
    let groups = expressions
        .into_iter()
        .map(|expression| group_from(expression, tag_name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(normalize(groups))
}

// ── Colour ─────────────────────────────────────────────────────────

/// `labToHex`.
pub fn lab_to_hex(color: &LabColor) -> String {
    let fy = (color.l + 16.) / 116.;
    let fx = color.a / 500. + fy;
    let fz = fy - color.b / 200.;
    let inverse = |part: f32| {
        if part.powi(3) > 0.008856 {
            part.powi(3)
        } else {
            (part - 16. / 116.) / 7.787
        }
    };
    let x = 0.95047 * inverse(fx);
    let y = inverse(fy);
    let z = 1.08883 * inverse(fz);
    [
        x * 3.2406 + y * -1.5372 + z * -0.4986,
        x * -0.9689 + y * 1.8758 + z * 0.0415,
        x * 0.0557 + y * -0.204 + z * 1.057,
    ]
    .iter()
    .map(|channel| {
        let encoded = if *channel <= 0.0031308 {
            channel * 12.92
        } else {
            1.055 * channel.powf(1. / 2.4) - 0.055
        };
        format!("{:02x}", (encoded.clamp(0., 1.) * 255.).round() as u8)
    })
    .fold("#".to_owned(), |hex, part| hex + &part)
}

#[cfg(test)]
mod tests {
    use super::{
        Group, Rule, compile, day_key, day_start, default_group, from_filter, hex_to_lab,
        lab_to_hex, normalize,
    };
    use picto_library::TagId;
    use picto_library::predicate::{FilterClause, FilterExpr, SetMatchMode};

    fn resolve(name: &str) -> Option<TagId> {
        match name {
            "cat" => Some(TagId(1)),
            "artist:ann" => Some(TagId(2)),
            _ => None,
        }
    }

    fn name(id: TagId) -> Option<String> {
        match id.0 {
            1 => Some("cat".into()),
            2 => Some("artist:ann".into()),
            _ => None,
        }
    }

    fn rule(field: &'static str, op: &'static str, value: &str) -> Rule {
        Rule {
            field,
            op,
            value: value.into(),
            value2: None,
            values: Vec::new(),
            unit: None,
        }
    }

    #[test]
    fn dates_are_utc_midnights_as_date_parse_reads_them() {
        assert_eq!(day_start("1970-01-02"), Some(86_400_000));
        assert_eq!(
            day_start("2024-02-29").map(day_key).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(day_start("2024-13-01"), None);
    }

    #[test]
    fn colours_survive_the_trip_through_lab() {
        assert_eq!(lab_to_hex(&hex_to_lab("#339af0")), "#339af0");
        assert_eq!(lab_to_hex(&hex_to_lab("#fff")), "#ffffff");
    }

    #[test]
    fn groups_compile_to_all_of_any_or_all_with_exclusions() {
        let groups = vec![
            Group {
                any: true,
                negate: false,
                rules: vec![
                    Rule {
                        values: vec!["cat".into(), "artist:ann".into()],
                        ..rule("tags", "include_all", "")
                    },
                    rule("width", "gt", "100"),
                ],
            },
            Group {
                any: false,
                negate: true,
                rules: vec![Rule {
                    values: vec!["cat".into()],
                    ..rule("tags", "do_not_include", "")
                }],
            },
        ];
        let compiled = compile(&groups, &resolve).unwrap();
        assert_eq!(
            compiled,
            FilterExpr::All(vec![
                FilterExpr::Any(vec![
                    FilterExpr::Clause(FilterClause::Tags {
                        tag_ids: vec![TagId(1), TagId(2)],
                        mode: SetMatchMode::All,
                    }),
                    FilterExpr::Clause(FilterClause::Width {
                        minimum: Some(101),
                        maximum: None,
                    }),
                ]),
                FilterExpr::Not(Box::new(FilterExpr::All(vec![FilterExpr::Not(Box::new(
                    FilterExpr::Clause(FilterClause::Tags {
                        tag_ids: vec![TagId(1)],
                        mode: SetMatchMode::Any,
                    })
                ))]))),
            ])
        );
        // Read back, a `gt 100` reads as `at least 101`, as in Electron.
        let back = from_filter(&compiled, &name).unwrap();
        assert_eq!(back[0].rules[1], rule("width", "gte", "101"));
        assert_eq!(back[1].rules[0].op, "do_not_include");
        assert!(back[1].negate && back[0].any);
        assert!(compile(&back, &resolve).is_ok());
    }

    #[test]
    fn rules_round_trip_through_the_filter() {
        let groups = normalize(vec![Group {
            any: false,
            negate: false,
            rules: vec![
                Rule {
                    value2: Some("4".into()),
                    ..rule("rating", "between", "2")
                },
                Rule {
                    unit: Some("MB"),
                    ..rule("file_size", "lte", "5")
                },
                rule("date_added", "eq", "2024-05-01"),
                rule("file_type", "is_not", "video"),
                rule("notes", "is_empty", ""),
                rule("name", "contains", "beach"),
                Rule {
                    value2: Some("2024-06-30".into()),
                    ..rule("date_created", "between", "2024-06-01")
                },
                rule("duration", "neq", "30"),
            ],
        }]);
        let compiled = compile(&groups, &resolve).unwrap();
        assert_eq!(from_filter(&compiled, &name).unwrap(), groups);
    }

    #[test]
    fn an_empty_filter_opens_with_one_tag_rule() {
        assert_eq!(
            from_filter(&FilterExpr::All(Vec::new()), &name).unwrap(),
            vec![default_group()]
        );
        assert!(
            compile(
                &[Group {
                    rules: vec![Rule {
                        values: vec!["missing".into()],
                        ..rule("tags", "include_any", "")
                    }],
                    ..default_group()
                }],
                &resolve
            )
            .is_err()
        );
    }
}
