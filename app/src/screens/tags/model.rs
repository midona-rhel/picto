//! The calculations of the Tags screen that need no window: which names the
//! group dialogs accept, and how the list of tags is divided into headings
//! and rows. The names of tags, the colours of groups and the tag
//! preferences are in [`crate::tags`].
use crate::i18n::t;
use picto_library::TagRecord;
use std::ops::Range;

// The functions below work on the names of groups and tags.

/// Returns the name that the group dialogs store for the text that was
/// typed: the text in lower case, without the white space at its ends, and
/// with each run of white space inside it replaced by one underscore.
pub fn normalized_group_name(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

/// Returns whether a group dialog may submit `name`, which is already
/// normalized. The name must not be empty and must not contain a colon.
pub fn valid_group_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(':')
}

/// Returns the label of the heading that a tag is listed under: the first
/// character of the tag's name in upper case, or `#` when that character is
/// not a letter or a digit.
pub fn section_label(subname: &str) -> String {
    match subname.trim().chars().next() {
        Some(first) if first.is_alphanumeric() => first.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
}

// The constants and types below describe the rows of the list of tags.

/// The height in pixels of a heading in the list of tags.
pub const SECTION_HEIGHT: f32 = 42.;
/// The height in pixels of a row of tag cards. Each such row takes
/// `ROW_STEP` pixels of the list's height, which leaves 1 px between two
/// rows.
pub const ROW_HEIGHT: f32 = 26.;
pub const ROW_STEP: f32 = 27.;
/// The padding in pixels above the rows of the list. `LIST_BOTTOM` is the
/// padding in pixels below them.
pub const LIST_TOP: f32 = 8.;
pub const LIST_BOTTOM: f32 = 24.;
/// The number of rows that are built beyond each edge of the viewport.
pub const OVERSCAN: usize = 12;

/// Returns the number of columns of tag cards in a list that is `width`
/// pixels wide: one column for every 201 px after the first 30 px, and at
/// least one column.
pub fn column_count(width: f32) -> usize {
    (((width - 30.) / 201.).floor() as isize).max(1) as usize
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A heading. Its label is a letter, or "Matches" above the results of
    /// a search. `count` is the number of tags under the heading.
    Section { label: String, count: usize },
    /// One row of tag cards. `tags` is the range of the tags' positions in
    /// [`Rows::ordered`]. `section_end` is true for the last row under a
    /// heading, and a line is drawn under that row.
    Tags {
        tags: Range<usize>,
        section_end: bool,
    },
    /// One row of placeholders for tags that are not loaded yet.
    Skeleton,
}

#[derive(Default)]
pub struct Rows {
    /// The loaded tags in the order in which the rows show them.
    pub ordered: Vec<TagRecord>,
    pub rows: Vec<Row>,
    /// For each row, the distance in pixels from the top of the first row
    /// to the top of that row. `total` is the height of all rows together.
    pub starts: Vec<f32>,
    pub total: f32,
}

impl Rows {
    /// Builds the rows of the list of tags. When `searching` is false, the
    /// tags are sorted by name and put under a heading for each first
    /// letter. When it is true, the tags are the results of a search: they
    /// keep the order in which they were given, under the one heading
    /// "Matches". After the rows of the loaded tags come rows of
    /// placeholders for the tags that are not loaded yet, so that the rows
    /// have room for `expected` tags in all.
    pub fn build(tags: &[TagRecord], searching: bool, columns: usize, expected: usize) -> Self {
        let columns = columns.max(1);
        let mut sections: Vec<(String, Vec<&TagRecord>)> = Vec::new();
        if searching {
            if !tags.is_empty() {
                sections.push((t("Matches").to_owned(), tags.iter().collect()));
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

    /// Returns the range of the rows that are at least partly between `top`
    /// and `bottom`, with [`OVERSCAN`] more rows before and after them.
    /// `top` and `bottom` are in pixels below the top of the first row.
    pub fn visible(&self, top: f32, bottom: f32) -> Range<usize> {
        let first = self
            .starts
            .partition_point(|start| *start <= top)
            .saturating_sub(1);
        let last = self.starts.partition_point(|start| *start < bottom);
        first.saturating_sub(OVERSCAN)..(last + OVERSCAN).min(self.rows.len())
    }

    /// Returns the index of the first row of placeholders, or the number of
    /// rows when there is no such row.
    pub fn load_boundary(&self) -> usize {
        self.rows
            .iter()
            .position(|row| *row == Row::Skeleton)
            .unwrap_or(self.rows.len())
    }
}

/// Returns the state, at one moment, of the pattern that fills the
/// placeholder of a tag that is not loaded yet. The pattern goes from 3%
/// contrast to 8% contrast and back to 3% over twice the placeholder's
/// width, and it moves from left to right by its whole length every 800 ms.
/// `eased` is how far it has moved, from 0 to 1.
///
/// At every moment the part of the pattern that is inside the placeholder
/// is two straight pieces. The first value returned is the place where the
/// two pieces meet, as a fraction of the placeholder's width from its left
/// edge. The three other values are the strength of the fill at the left
/// edge, at that place and at the right edge. A strength of 0 stands for 3%
/// contrast and a strength of 1 for 8% contrast.
pub fn shimmer(eased: f32) -> (f32, [f32; 3]) {
    // The pattern repeats every two widths of the placeholder. Its strongest
    // point is `2 × eased` widths from the left edge, and its weakest point
    // is one width to the left of that.
    let peak = 2. * eased;
    let (bend, bend_value) = if peak <= 1. {
        (peak, 1.)
    } else {
        (peak - 1., 0.)
    };
    // Returns the strength of the pattern `x` widths from the left edge.
    // The strength falls in a straight line from 1 at the strongest point
    // to 0 one width away from it.
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
        // Seven more tags are expected. With three tags in a row, they need
        // three rows of placeholders.
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
        // Row 0 is the heading. Row n, for n ≥ 1, starts at
        // 42 + 27 × (n − 1).
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
