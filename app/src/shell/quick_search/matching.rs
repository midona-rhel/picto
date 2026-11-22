//! Matches the text typed in the Quick Search panel against a name, and
//! orders a list of names by how well they match.
//!
//! A name matches when it contains the letters of the query in the same
//! order. The letters do not have to be next to each other, and upper and
//! lower case are the same. The matches are ranked in the order that the
//! search field of the context menus uses (`score` in `ui/menu/layout.rs`):
//!
//! 1. A name that starts with the query.
//! 2. A name that contains the query as one piece. The earlier the piece
//!    starts, the better.
//! 3. A name that contains the query's letters with other letters between
//!    them. The first letter of the query must then be the first letter of
//!    a word of the name. "imf" matches "Import Files", and "tag" does not
//!    match "First Image". The fewer letters between the matched letters,
//!    the better.
//!
//! The context menus' own function returns only a rank. This file has its
//! own function because the panel also needs the positions of the matched
//! letters, to draw them marked.
use std::ops::Range;

/// The result of matching a query against one name.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    /// The rank of the match. A lower number is a better match.
    pub rank: u32,
    /// The matched letters, as byte ranges of the name. Letters that are
    /// next to each other are one range. The ranges are in order and do not
    /// overlap.
    pub ranges: Vec<Range<usize>>,
}

/// The rank of the first name that only contains the query as one piece.
const CONTAINS: u32 = 10;
/// The rank of the first name that only contains the query's letters in
/// order.
const SCATTERED: u32 = 10_000;

/// One letter of a name: where it starts in the name, how many bytes it
/// takes, and the letter in lower case.
struct Letter {
    start: usize,
    len: usize,
    lower: char,
}

/// Returns `letter` in lower case. A letter whose lower case is more than
/// one character is returned as the first of them, so that every letter of
/// a name stays one letter and the byte positions stay those of the name.
fn lower(letter: char) -> char {
    letter.to_lowercase().next().unwrap_or(letter)
}

/// Matches `query` against `name` by the rules at the top of this file.
/// Returns `None` when `name` does not match. A query that is empty, or has
/// only spaces, matches every name with rank 0 and marks no letters.
pub fn find(name: &str, query: &str) -> Option<Match> {
    let query: Vec<char> = query.trim().chars().map(lower).collect();
    if query.is_empty() {
        return Some(Match {
            rank: 0,
            ranges: Vec::new(),
        });
    }
    let letters: Vec<Letter> = name
        .char_indices()
        .map(|(start, letter)| Letter {
            start,
            len: letter.len_utf8(),
            lower: lower(letter),
        })
        .collect();
    if letters.len() < query.len() {
        return None;
    }
    // Look for the query as one piece first.
    let piece = (0..=letters.len() - query.len()).find(|at| {
        query
            .iter()
            .enumerate()
            .all(|(offset, wanted)| letters[at + offset].lower == *wanted)
    });
    if let Some(at) = piece {
        let last = &letters[at + query.len() - 1];
        return Some(Match {
            rank: if at == 0 {
                0
            } else {
                (CONTAINS + at as u32).min(SCATTERED - 1)
            },
            ranges: vec![letters[at].start..last.start + last.len],
        });
    }
    // Otherwise the query's letters may have other letters between them.
    // Try each word of the name whose first letter is the query's first
    // letter, and keep the try with the fewest letters between the matched
    // letters.
    let word_starts = (0..letters.len()).filter(|at| {
        letters[*at].lower == query[0]
            && (*at == 0 || !name[letters[at - 1].start..].starts_with(char::is_alphanumeric))
    });
    word_starts
        .filter_map(|start| scattered(&letters[start..], &query))
        .min_by_key(|found| found.rank)
}

/// Takes each letter of `query` at the first place where it occurs in
/// `letters` after the letter before it. The first letter of `query` is the
/// first of `letters`. Returns `None` when a letter of `query` is not found.
fn scattered(letters: &[Letter], query: &[char]) -> Option<Match> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let (mut found, mut between) = (0, 0u32);
    for letter in letters {
        if found == query.len() {
            break;
        }
        if letter.lower == query[found] {
            found += 1;
            match ranges.last_mut() {
                Some(range) if range.end == letter.start => range.end += letter.len,
                _ => ranges.push(letter.start..letter.start + letter.len),
            }
        } else {
            between += 1;
        }
    }
    (found == query.len()).then_some(Match {
        rank: SCATTERED + between,
        ranges,
    })
}

/// One name that matched: its position in the list that was searched, and
/// the matched letters.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub index: usize,
    pub ranges: Vec<Range<usize>>,
}

/// The names of a list that match a query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hits {
    /// The best matches, the best first, at most as many as the limit that
    /// was asked for.
    pub hits: Vec<Hit>,
    /// How many of `hits`, counted from the start, are recent choices. It is
    /// 0 when the query is not empty.
    pub recent: usize,
    /// How many names of the list match, including those left out of `hits`.
    pub total: usize,
}

/// Searches `names` for `query` and returns at most `limit` matches.
///
/// When the query is empty, every name matches. The names at the positions
/// in `recent` come first, in the order of `recent`, and the other names
/// follow in the order of the list.
///
/// When the query is not empty, the matches are ordered by rank, and matches
/// of the same rank stay in the order of the list.
pub fn search<'a>(
    names: impl Iterator<Item = &'a str>,
    query: &str,
    recent: &[usize],
    limit: usize,
) -> Hits {
    if query.trim().is_empty() {
        let total = names.count();
        let recent: Vec<usize> = recent
            .iter()
            .copied()
            .filter(|index| *index < total)
            .take(limit)
            .collect();
        let rest = (0..total)
            .filter(|index| !recent.contains(index))
            .take(limit - recent.len());
        let hits = recent
            .iter()
            .copied()
            .chain(rest)
            .map(|index| Hit {
                index,
                ranges: Vec::new(),
            })
            .collect();
        return Hits {
            hits,
            recent: recent.len(),
            total,
        };
    }
    let mut matched: Vec<(u32, usize, Vec<Range<usize>>)> = names
        .enumerate()
        .filter_map(|(index, name)| {
            find(name, query).map(|found| (found.rank, index, found.ranges))
        })
        .collect();
    let total = matched.len();
    matched.sort_by_key(|(rank, index, _)| (*rank, *index));
    matched.truncate(limit);
    Hits {
        hits: matched
            .into_iter()
            .map(|(_, index, ranges)| Hit { index, ranges })
            .collect(),
        recent: 0,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::{Hit, find, search};

    fn ranges(name: &str, query: &str) -> Vec<&'static str> {
        // The matched pieces of the name, as text.
        let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        find(name, query)
            .unwrap_or_else(|| panic!("{query} does not match {name}"))
            .ranges
            .into_iter()
            .map(|range| &name[range])
            .collect()
    }

    #[test]
    fn a_name_matches_when_it_has_the_querys_letters_in_order() {
        assert!(find("Import Files", "imf").is_some());
        assert!(find("Import Files", "IMPORT").is_some());
        assert!(find("Import Files", "fi").is_some());
        // The letters are there, but not in this order.
        assert!(find("Import Files", "fim").is_none());
        // The letters are there in this order with other letters between
        // them, but no word of the name starts with the first of them.
        assert!(find("First Image", "tag").is_none());
        assert!(find("First Image", "fig").is_some());
        assert!(find("dog_tags", "ts").is_some());
        assert!(find("Tags", "tagsx").is_none());
        // A query of spaces is an empty query.
        assert!(find("Tags", "  ").unwrap().ranges.is_empty());
    }

    #[test]
    fn the_matched_letters_are_reported_as_pieces_of_the_name() {
        assert_eq!(ranges("Import Files", "imp"), ["Imp"]);
        assert_eq!(ranges("Import Files", "files"), ["Files"]);
        assert_eq!(ranges("Import Files", "imf"), ["Im", "F"]);
        assert_eq!(ranges("Übersicht Ägypten", "äg"), ["Äg"]);
        assert_eq!(ranges("夏の写真", "写真"), ["写真"]);
    }

    #[test]
    fn a_better_match_has_a_lower_rank() {
        let rank = |name| find(name, "tag").unwrap().rank;
        // The name starts with the query, contains it later, contains it
        // even later, and has the letters apart.
        assert!(rank("Tags") < rank("Add Tags"));
        assert!(rank("Add Tags") < rank("Set Folder Auto Tags"));
        assert!(rank("Set Folder Auto Tags") < rank("Toggle Navigator"));
        // Fewer letters between the matched letters is better.
        assert!(find("Trash Bag", "tag").unwrap().rank > find("Tea Bag", "tag").unwrap().rank);
    }

    #[test]
    fn a_search_orders_by_rank_and_keeps_the_lists_order_among_equals() {
        let names = ["Untagged", "Tags", "Add Tags", "Trash", "Tag Groups"];
        let found = search(names.iter().copied(), "tag", &[], 10);
        let order: Vec<&str> = found.hits.iter().map(|hit| names[hit.index]).collect();
        assert_eq!(order, ["Tags", "Tag Groups", "Untagged", "Add Tags"]);
        assert_eq!((found.total, found.recent), (4, 0));
        // The limit cuts the list and not the count.
        let found = search(names.iter().copied(), "tag", &[], 2);
        assert_eq!((found.hits.len(), found.total), (2, 4));
    }

    #[test]
    fn an_empty_query_lists_the_recent_choices_first() {
        let names = ["All", "Inbox", "Trash", "Tags"];
        let found = search(names.iter().copied(), "", &[3, 1, 9], 10);
        let order: Vec<usize> = found.hits.iter().map(|hit| hit.index).collect();
        // Position 9 is not in the list, so it is left out.
        assert_eq!(order, [3, 1, 0, 2]);
        assert_eq!((found.recent, found.total), (2, 4));
        assert_eq!(
            found.hits[0],
            Hit {
                index: 3,
                ranges: Vec::new()
            }
        );
        let found = search(names.iter().copied(), " ", &[3, 1], 3);
        assert_eq!(found.hits.len(), 3);
    }
}
