//! The Tags screen's rules apart from drawing them: tag and group names, the
//! groups' colours and order, the browse list's sections and rows, search
//! ranking, and the starred tags and group colours kept in the library's
//! application settings.
use picto_library::TagRecord;
use std::{cmp::Ordering, collections::BTreeMap, ops::Range};

// ── Names ───────────────────────────────────────────────────────

/// `tagNamespace`: the group a namespace names, or none for General.
pub fn group_of(namespace: &str) -> Option<String> {
    let normalized = namespace.trim().to_lowercase();
    (!normalized.is_empty() && normalized != "general").then_some(normalized)
}

/// `tagName`: `namespace:subname`, or the subname alone in General.
pub fn tag_name(namespace: &str, subname: &str) -> String {
    match group_of(namespace) {
        Some(group) => format!("{group}:{subname}"),
        None => subname.to_owned(),
    }
}

/// `tagNameInGroup`: the tag's name once moved into `group`.
pub fn name_in_group(subname: &str, group: Option<&str>) -> String {
    match group {
        Some(group) => format!("{group}:{subname}"),
        None => subname.to_owned(),
    }
}

/// What the group dialogs store for what was typed: trimmed, lower case,
/// with each run of spaces made one underscore.
pub fn normalized_group_name(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

/// Whether a group dialog may submit `name` (already normalized).
pub fn valid_group_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(':')
}

// ── Groups ──────────────────────────────────────────────────────

/// `tagGroupPresentation`: a group's colour and where it sorts.
fn preset(namespace: &str) -> (u32, u32) {
    match namespace.to_lowercase().as_str() {
        "creator" => (0xff6667, 0),
        "series" => (0xc499ff, 1),
        "character" => (0x30d159, 2),
        "species" => (0x00aaff, 3),
        "rating" => (0xffd60a, 4),
        _ => (0xbdbec0, 10),
    }
}

/// `tagGroupOrder`
pub fn group_order(namespace: &str) -> u32 {
    preset(namespace).1
}

/// `tagGroupColor`: the colour chosen for the group, else its preset.
pub fn group_color(namespace: &str, preferences: &Preferences) -> u32 {
    preferences
        .colors
        .get(namespace)
        .and_then(|color| parse_color(color))
        .unwrap_or(preset(namespace).0)
}

/// `TagChip`'s colour for a namespace.
pub fn chip_color(namespace: &str) -> u32 {
    match namespace {
        "creator" | "studio" => 0xff6667,
        "character" | "person" => 0x30d159,
        "series" => 0xc499ff,
        "species" => 0x00aaff,
        "rating" => 0xffd60a,
        "system" => 0xff9f0a,
        _ => 0xbdbec0,
    }
}

/// A stored colour: `#rgb`, `#rrggbb` or `rgb(r, g, b)`.
pub fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let digits = match hex.len() {
            3 => hex.chars().flat_map(|digit| [digit, digit]).collect(),
            6 => hex.to_owned(),
            _ => return None,
        };
        return u32::from_str_radix(&digits, 16).ok();
    }
    let inner = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let channels = inner
        .split(',')
        .map(|channel| channel.trim().parse::<u8>().ok())
        .collect::<Option<Vec<_>>>()?;
    match channels[..] {
        [r, g, b] => Some((r as u32) << 16 | (g as u32) << 8 | b as u32),
        _ => None,
    }
}

/// `ColorPicker`'s colours (`FOLDER_COLORS`): a name and the stored value,
/// none for the default.
pub const GROUP_COLORS: [(&str, Option<&str>); 10] = [
    ("Default", None),
    ("Blue", Some("#339AF0")),
    ("Red", Some("#FA5252")),
    ("Orange", Some("#FD7E14")),
    ("Yellow", Some("#FAB005")),
    ("Green", Some("#40C057")),
    ("Teal", Some("#12B886")),
    ("Indigo", Some("#5C7CFA")),
    ("Purple", Some("#7950F2")),
    ("Pink", Some("#E64980")),
];

// ── Ordering ────────────────────────────────────────────────────

/// Where a character sorts before its case-folded value: punctuation and
/// symbols, then digits, then letters, as the Electron collator orders them.
fn class(character: char) -> u8 {
    if character.is_numeric() {
        1
    } else if character.is_alphabetic() {
        2
    } else {
        0
    }
}

/// `localeCompare(…, { sensitivity: 'base', numeric: true })` for tag
/// names: case is ignored and runs of digits compare by their value.
pub fn compare_names(left: &str, right: &str) -> Ordering {
    let (mut left, mut right) = (left.chars().peekable(), right.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(digit) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(digit);
                        chars.next();
                    }
                    digits.trim_start_matches('0').to_owned()
                };
                let (a, b) = (number(&mut left), number(&mut right));
                let order = a.len().cmp(&b.len()).then_with(|| a.cmp(&b));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(a), Some(b)) => {
                let fold = |character: char| character.to_lowercase().collect::<String>();
                let order = class(a).cmp(&class(b)).then_with(|| fold(a).cmp(&fold(b)));
                if order != Ordering::Equal {
                    return order;
                }
                left.next();
                right.next();
            }
        }
    }
}

/// The letter section a tag is listed under: its first character in upper
/// case, or `#` for anything that is not a letter or digit.
pub fn section_label(subname: &str) -> String {
    match subname.trim().chars().next() {
        Some(first) if first.is_alphanumeric() => first.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
}

// ── Search ──────────────────────────────────────────────────────

/// `matchRank`: 0 exact, 1 prefix, 2 at the start of a word, 3 anywhere;
/// none when absent. Both sides are lower case.
pub fn match_rank(text: &str, needle: &str) -> Option<u8> {
    if text == needle {
        return Some(0);
    }
    if text.starts_with(needle) {
        return Some(1);
    }
    let mut found = false;
    for (index, _) in text.match_indices(needle) {
        found = true;
        let before = text[..index].chars().next_back();
        if before.is_some_and(|before| !before.is_alphanumeric()) {
            return Some(2);
        }
    }
    found.then_some(3)
}

/// `rankMatches`: the items whose key contains `query`, best rank first,
/// keeping their order within a rank. A blank query keeps them all.
pub fn rank_matches<T>(items: Vec<T>, query: &str, key: impl Fn(&T) -> &str) -> Vec<T> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return items;
    }
    let mut ranked = items
        .into_iter()
        .filter_map(|item| {
            let rank = match_rank(&key(&item).to_lowercase(), &needle)?;
            Some((rank, item))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, item)| item).collect()
}

// ── The browse list ─────────────────────────────────────────────

/// `.tagSectionHeader`
pub const SECTION_HEIGHT: f32 = 42.;
/// `.tagVirtualRow` and the space the list gives each.
pub const ROW_HEIGHT: f32 = 26.;
pub const ROW_STEP: f32 = 27.;
/// `.tagCanvas`: padding above and below the rows.
pub const LIST_TOP: f32 = 8.;
pub const LIST_BOTTOM: f32 = 24.;
/// Rows drawn beyond each edge of the viewport.
pub const OVERSCAN: usize = 12;

/// How many tags fit across the list (`ResizeObserver` in the screen).
pub fn column_count(width: f32) -> usize {
    (((width - 30.) / 201.).floor() as isize).max(1) as usize
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A letter, or "Matches" for search results, with how many tags it has.
    Section { label: String, count: usize },
    /// Up to one row of tags, as positions in [`Rows::ordered`]; the last
    /// row of a section draws the line under it.
    Tags {
        tags: Range<usize>,
        section_end: bool,
    },
    /// A placeholder for tags not loaded yet.
    Skeleton,
}

#[derive(Default)]
pub struct Rows {
    /// The loaded tags in the order the rows show them.
    pub ordered: Vec<TagRecord>,
    pub rows: Vec<Row>,
    /// Where each row starts, from the top of the list's content.
    pub starts: Vec<f32>,
    pub total: f32,
}

impl Rows {
    /// The rows of the browse list. Browsing, tags sort by name into letter
    /// sections; search results keep their ranked order in one section.
    /// Skeleton rows stand in for the tags still to load, up to `expected`.
    pub fn build(tags: &[TagRecord], searching: bool, columns: usize, expected: usize) -> Self {
        let columns = columns.max(1);
        let mut sections: Vec<(String, Vec<&TagRecord>)> = Vec::new();
        if searching {
            if !tags.is_empty() {
                sections.push(("Matches".to_owned(), tags.iter().collect()));
            }
        } else {
            let mut sorted = tags.iter().collect::<Vec<_>>();
            sorted.sort_by(|left, right| compare_names(&left.subname, &right.subname));
            let mut position = std::collections::HashMap::new();
            for tag in sorted {
                let label = section_label(&tag.subname);
                let at = *position.entry(label.clone()).or_insert_with(|| {
                    sections.push((label, Vec::new()));
                    sections.len() - 1
                });
                sections[at].1.push(tag);
            }
        }
        let mut rows = Vec::new();
        let mut ordered = Vec::with_capacity(tags.len());
        for (label, section) in sections {
            rows.push(Row::Section {
                label,
                count: section.len(),
            });
            for chunk in section.chunks(columns) {
                let start = ordered.len();
                ordered.extend(chunk.iter().map(|tag| (*tag).clone()));
                rows.push(Row::Tags {
                    tags: start..ordered.len(),
                    section_end: false,
                });
            }
            if let Some(Row::Tags { section_end, .. }) = rows.last_mut() {
                *section_end = true;
            }
        }
        let remaining = expected.saturating_sub(tags.len());
        rows.extend(std::iter::repeat_n(
            Row::Skeleton,
            remaining.div_ceil(columns),
        ));
        let mut starts = Vec::with_capacity(rows.len());
        let mut total = 0.;
        for row in &rows {
            starts.push(total);
            total += match row {
                Row::Section { .. } => SECTION_HEIGHT,
                _ => ROW_STEP,
            };
        }
        Self {
            ordered,
            rows,
            starts,
            total,
        }
    }

    /// The rows that reach into `top..bottom`, with [`OVERSCAN`] more on
    /// each side.
    pub fn visible(&self, top: f32, bottom: f32) -> Range<usize> {
        let first = self
            .starts
            .partition_point(|start| *start <= top)
            .saturating_sub(1);
        let last = self.starts.partition_point(|start| *start < bottom);
        first.saturating_sub(OVERSCAN)..(last + OVERSCAN).min(self.rows.len())
    }

    /// Where the loaded rows end and the placeholders begin.
    pub fn load_boundary(&self) -> usize {
        self.rows
            .iter()
            .position(|row| *row == Row::Skeleton)
            .unwrap_or(self.rows.len())
    }
}

/// `.tagSkeleton`'s shimmer: a 3% to 8% to 3% contrast gradient twice the
/// tag's width slides across it once each 800 ms on `ease-in-out`. At any
/// moment the tag shows two straight pieces; this gives the bend's distance
/// from the left edge (a fraction of the width) and the strength, 0 at 3%
/// and 1 at 8%, at the left edge, the bend and the right edge.
pub fn shimmer(eased: f32) -> (f32, [f32; 3]) {
    // The gradient's period is twice the tag's width; its brightest point is
    // at `2 × eased` widths, and its darkest one width before that.
    let peak = 2. * eased;
    let (bend, bend_value) = if peak <= 1. {
        (peak, 1.)
    } else {
        (peak - 1., 0.)
    };
    // The strength at x (in widths), a triangle wave of period 2.
    let value = |x: f32| {
        let distance = (x - peak).rem_euclid(2.);
        1. - distance.min(2. - distance)
    };
    (bend, [value(0.), bend_value, value(1.)])
}

// ── Preferences ─────────────────────────────────────────────────

/// The tag preferences of the library's application settings
/// (`tagPreferences.ts`): starred tags by name, and colours chosen for
/// groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub starred: Vec<String>,
    pub colors: BTreeMap<String, String>,
}

impl Preferences {
    /// The preferences as the settings model holds them.
    pub fn from_settings(settings: &crate::settings::model::AppSettings) -> Self {
        Self {
            starred: settings.starred_tags.clone(),
            colors: settings
                .tag_group_colors
                .iter()
                .filter_map(|(group, color)| Some((group.clone(), color.as_str()?.to_owned())))
                .collect(),
        }
    }

    /// The group colours as the settings model stores them.
    pub fn colors_json(&self) -> serde_json::Map<String, serde_json::Value> {
        self.colors
            .iter()
            .map(|(group, color)| (group.clone(), serde_json::Value::from(color.clone())))
            .collect()
    }

    pub fn is_starred(&self, name: &str) -> bool {
        self.starred.iter().any(|starred| starred == name)
    }

    /// `setTagStarred`: the starred list, sorted, with `name` in or out.
    pub fn with_star(&self, name: &str, starred: bool) -> Vec<String> {
        let mut tags = self.starred.clone();
        tags.retain(|tag| tag != name);
        if starred {
            tags.push(name.to_owned());
            tags.sort();
        }
        tags
    }

    /// `replaceStarredTag`: the list with a renamed tag's new name, or none
    /// when the old name was not starred.
    pub fn renamed_star(&self, previous: &str, next: &str) -> Option<Vec<String>> {
        if previous == next || !self.is_starred(previous) {
            return None;
        }
        let mut tags = self
            .starred
            .iter()
            .filter(|tag| *tag != previous)
            .cloned()
            .chain([next.to_owned()])
            .collect::<Vec<_>>();
        tags.sort();
        tags.dedup();
        Some(tags)
    }

    /// `setTagGroupColor`
    pub fn with_color(&self, group: &str, color: Option<&str>) -> BTreeMap<String, String> {
        let mut colors = self.colors.clone();
        match color {
            Some(color) => colors.insert(group.to_owned(), color.to_owned()),
            None => colors.remove(group),
        };
        colors
    }

    /// `replaceTagGroupColor`: the colours with a renamed group's moved to
    /// its new name, or none when it had none.
    pub fn renamed_color(&self, previous: &str, next: &str) -> Option<BTreeMap<String, String>> {
        if previous == next {
            return None;
        }
        let color = self.colors.get(previous)?;
        let mut colors = self.colors.clone();
        colors.insert(next.to_owned(), color.clone());
        colors.remove(previous);
        Some(colors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picto_library::{TagId, TagNamespaceId};

    fn tag(id: u32, namespace: &str, subname: &str) -> TagRecord {
        TagRecord {
            tag_id: TagId(id),
            namespace_id: TagNamespaceId(0),
            namespace: namespace.into(),
            subname: subname.into(),
            active_count: 0,
            assignment_count: 0,
        }
    }

    #[test]
    fn names_leave_out_the_general_group() {
        assert_eq!(tag_name("", "cat"), "cat");
        assert_eq!(tag_name("General", "cat"), "cat");
        assert_eq!(tag_name("Creator", "someone"), "creator:someone");
        assert_eq!(name_in_group("cat", Some("species")), "species:cat");
        assert_eq!(name_in_group("cat", None), "cat");
        assert_eq!(normalized_group_name("  Art   Style "), "art_style");
        assert!(!valid_group_name("a:b") && !valid_group_name(""));
    }

    #[test]
    fn groups_take_their_preset_colour_and_order_unless_one_was_chosen() {
        let mut preferences = Preferences::default();
        assert_eq!(group_color("character", &preferences), 0x30d159);
        assert_eq!(group_color("anything", &preferences), 0xbdbec0);
        preferences
            .colors
            .insert("character".into(), "#FA5252".into());
        assert_eq!(group_color("character", &preferences), 0xfa5252);
        assert_eq!(group_order("creator"), 0);
        assert_eq!(group_order("rating"), 4);
        assert_eq!(group_order("meta"), 10);
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(0x010203));
        assert_eq!(parse_color("#abc"), Some(0xaabbcc));
        assert_eq!(parse_color("blue"), None);
    }

    #[test]
    fn names_sort_without_case_and_with_numbers_by_value() {
        let mut names = vec!["b", "A", "item10", "item9", "_x", "2d", "a"];
        names.sort_by(|left, right| compare_names(left, right));
        assert_eq!(names, ["_x", "2d", "A", "a", "b", "item9", "item10"]);
        assert_eq!(section_label("apple"), "A");
        assert_eq!(section_label(" 3d"), "3");
        assert_eq!(section_label("(paren)"), "#");
        assert_eq!(section_label("ßeta"), "SS");
    }

    #[test]
    fn search_ranks_exact_then_prefix_then_word_start_then_substring() {
        assert_eq!(match_rank("cat", "cat"), Some(0));
        assert_eq!(match_rank("catgirl", "cat"), Some(1));
        assert_eq!(match_rank("black_cat", "cat"), Some(2));
        assert_eq!(match_rank("bobcat", "cat"), Some(3));
        assert_eq!(match_rank("dog", "cat"), None);
        let ranked = rank_matches(
            vec!["bobcat", "black_cat", "cat", "dog", "catgirl"],
            " CAT ",
            |name| name,
        );
        assert_eq!(ranked, ["cat", "catgirl", "black_cat", "bobcat"]);
    }

    #[test]
    fn browsing_groups_tags_into_letter_sections_of_rows() {
        let tags = [
            tag(1, "", "beta"),
            tag(2, "", "alpha"),
            tag(3, "", "apple"),
            tag(4, "", "avocado"),
        ];
        let rows = Rows::build(&tags, false, 2, 4);
        assert_eq!(
            rows.rows,
            [
                Row::Section {
                    label: "A".into(),
                    count: 3
                },
                Row::Tags {
                    tags: 0..2,
                    section_end: false
                },
                Row::Tags {
                    tags: 2..3,
                    section_end: true
                },
                Row::Section {
                    label: "B".into(),
                    count: 1
                },
                Row::Tags {
                    tags: 3..4,
                    section_end: true
                },
            ]
        );
        let names = rows
            .ordered
            .iter()
            .map(|tag| tag.subname.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["alpha", "apple", "avocado", "beta"]);
        assert_eq!(rows.starts, [0., 42., 69., 96., 138.]);
        assert_eq!(rows.total, 165.);
        assert_eq!(rows.load_boundary(), 5);
    }

    #[test]
    fn search_results_keep_their_order_and_unloaded_tags_are_placeholders() {
        let tags = [tag(1, "", "zeta"), tag(2, "", "alpha")];
        let rows = Rows::build(&tags, true, 3, 9);
        assert_eq!(
            rows.rows[0],
            Row::Section {
                label: "Matches".into(),
                count: 2
            }
        );
        assert_eq!(rows.ordered[0].subname, "zeta");
        // Seven more expected, three to a row.
        assert_eq!(
            rows.rows[2..],
            [Row::Skeleton, Row::Skeleton, Row::Skeleton]
        );
        assert_eq!(rows.load_boundary(), 2);
        assert!(Rows::build(&[], true, 3, 0).rows.is_empty());
    }

    #[test]
    fn the_visible_rows_include_the_overscan() {
        let tags = (0..100)
            .map(|index| tag(index, "", &format!("a{index:03}")))
            .collect::<Vec<_>>();
        let rows = Rows::build(&tags, false, 1, 100);
        // Row 0 is the section; row n ≥ 1 starts at 42 + 27 × (n − 1).
        let range = rows.visible(42. + 27. * 50., 42. + 27. * 60.);
        assert_eq!(range, (51 - OVERSCAN)..(61 + OVERSCAN));
        assert_eq!(rows.visible(0., 100.).start, 0);
        assert_eq!(rows.visible(0., 1e9).end, rows.rows.len());
    }

    #[test]
    fn columns_follow_the_list_width() {
        assert_eq!(column_count(100.), 1);
        assert_eq!(column_count(30. + 201. * 3.), 3);
        assert_eq!(column_count(30. + 201. * 3. - 1.), 2);
    }

    #[test]
    fn the_shimmer_bends_where_the_gradient_peaks_or_dips() {
        assert_eq!(shimmer(0.), (0., [1., 1., 0.]));
        assert_eq!(shimmer(0.25), (0.5, [0.5, 1., 0.5]));
        let (bend, values) = shimmer(0.75);
        assert_eq!(bend, 0.5);
        assert_eq!(values, [0.5, 0., 0.5]);
    }

    #[test]
    fn preferences_read_star_and_rename_as_the_settings_store_them() {
        let settings = crate::settings::model::AppSettings::from_json(&serde_json::json!({
            "starredTags": ["b", "a"],
            "tagGroupColors": {"species": "#40C057"},
        }));
        let preferences = Preferences::from_settings(&settings);
        assert!(preferences.is_starred("a"));
        assert_eq!(preferences.with_star("c", true), ["a", "b", "c"]);
        assert_eq!(preferences.with_star("b", false), ["a"]);
        assert_eq!(
            preferences.renamed_star("b", "species:b"),
            Some(vec!["a".to_owned(), "species:b".to_owned()])
        );
        assert_eq!(preferences.renamed_star("x", "y"), None);
        let moved = preferences.renamed_color("species", "animal").unwrap();
        assert_eq!(moved.get("animal").map(String::as_str), Some("#40C057"));
        assert!(!moved.contains_key("species"));
        assert!(preferences.with_color("species", None).is_empty());
        assert_eq!(
            preferences.colors_json().get("species"),
            Some(&serde_json::json!("#40C057"))
        );
    }
}
