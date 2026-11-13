//! The Tags screen's rules apart from drawing them: what the group dialogs
//! accept, and the browse list's sections and rows. Tag names, group colours
//! and the tag preferences are in [`crate::tags`].
use picto_library::TagRecord;
use std::ops::Range;

// ── Names ───────────────────────────────────────────────────────

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

/// The letter section a tag is listed under: its first character in upper
/// case, or `#` for anything that is not a letter or digit.
pub fn section_label(subname: &str) -> String {
    match subname.trim().chars().next() {
        Some(first) if first.is_alphanumeric() => first.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
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
            sorted.sort_by(|left, right| crate::text::natural_order(&left.subname, &right.subname));
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
    fn group_names_are_normalized_before_they_are_stored() {
        assert_eq!(normalized_group_name("  Art   Style "), "art_style");
        assert!(!valid_group_name("a:b") && !valid_group_name(""));
    }

    #[test]
    fn sections_are_named_for_the_first_letter() {
        assert_eq!(section_label("apple"), "A");
        assert_eq!(section_label(" 3d"), "3");
        assert_eq!(section_label("(paren)"), "#");
        assert_eq!(section_label("ßeta"), "SS");
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
}
