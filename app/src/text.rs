//! This file holds the functions for text that more than one view uses.
//! They put names in natural order, rank and highlight search matches, write
//! byte sizes, shorten text that is too wide, give the date formats, and
//! give the short names of common media types.
use std::cmp::Ordering;

// The functions in this section put names in order.

/// Returns the class of a character: 0 for punctuation and symbols, 1 for
/// digits and 2 for letters. Two characters are compared by class first and
/// by case-folded value after that, so punctuation and symbols sort before
/// digits, and digits before letters. Picto 0.6.19 and earlier sorted names
/// in the same order.
fn class(character: char) -> u8 {
    if character.is_numeric() {
        1
    } else if character.is_alphabetic() {
        2
    } else {
        0
    }
}

/// The ASCII punctuation marks and symbols, in the order in which the Unicode
/// root collation sorts them. Any other punctuation mark sorts after all of
/// these, by its code point.
const PUNCTUATION_ORDER: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// Returns the value that orders a character among the characters of its
/// class. Punctuation is ordered by its position in [`PUNCTUATION_ORDER`],
/// and every other character by its case-folded value.
fn within_class(character: char) -> (usize, String) {
    let rank = PUNCTUATION_ORDER
        .find(character)
        .unwrap_or(PUNCTUATION_ORDER.len());
    (rank, character.to_lowercase().collect())
}

/// Compares two names for sorting. Upper and lower case are treated as the
/// same, and a run of digits is compared as a number, so `item9` comes before
/// `item10`.
pub fn natural_order(left: &str, right: &str) -> Ordering {
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
                let order = class(a)
                    .cmp(&class(b))
                    .then_with(|| within_class(a).cmp(&within_class(b)));
                if order != Ordering::Equal {
                    return order;
                }
                left.next();
                right.next();
            }
        }
    }
}

// The functions in this section rank and highlight search matches.

/// Returns how well `text` matches `needle`: 0 when the two are equal, 1 when
/// `text` starts with `needle`, 2 when `needle` is at the start of a word in
/// `text`, and 3 when `needle` is anywhere else in `text`. Returns `None`
/// when `text` does not contain `needle`. The caller passes both in lower
/// case.
pub fn match_rank(text: &str, needle: &str) -> Option<u8> {
    if text == needle {
        return Some(0);
    }
    if text.starts_with(needle) {
        return Some(1);
    }
    // This loop looks at every place where `needle` occurs in `text`,
    // including a place that overlaps an earlier one, because the overlapping
    // place can be the one at the start of a word.
    let mut found = false;
    let mut from = 0;
    while let Some(at) = text[from..].find(needle).map(|at| at + from) {
        found = true;
        let before = text[..at].chars().next_back();
        if before.is_some_and(|character| !character.is_alphanumeric()) {
            return Some(2);
        }
        from = at + text[at..].chars().next().map_or(1, char::len_utf8);
    }
    found.then_some(3)
}

/// Returns the items whose key contains `query`, with the best rank first.
/// Items of the same rank keep the order they came in. A blank query returns
/// every item.
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

/// Returns the byte range of the first match of `query` in `text`, ignoring
/// case. It is used to highlight the match. Returns `None` when there is no
/// match, when the query is blank, and when lower-casing `text` changes its
/// length in bytes, because the range would then be for the wrong bytes.
pub fn match_range(text: &str, query: &str) -> Option<std::ops::Range<usize>> {
    let query = query.trim();
    let lower = text.to_lowercase();
    if query.is_empty() || lower.len() != text.len() {
        return None;
    }
    let start = lower.find(&query.to_lowercase())?;
    Some(start..start + query.len())
}

// The functions in this section write byte sizes. The inspector writes a size
// in one way, and Settings, the Library Manager and the Picto Pack dialog
// write it in another way, so each way has its own function here.

/// Writes a size for the inspector's Size row, in binary units: whole bytes
/// below 1024 bytes, then KB and MB with one decimal, and GB with two
/// decimals.
pub fn size_to_gigabytes(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes < 1024. {
        format!("{bytes} B")
    } else if bytes < 1048576. {
        format!("{:.1} KB", bytes / 1024.)
    } else if bytes < 1073741824. {
        format!("{:.1} MB", bytes / 1048576.)
    } else {
        format!("{:.2} GB", bytes / 1073741824.)
    }
}

/// Writes a size for Settings, the Library Manager and the Picto Pack dialog:
/// whole bytes below 1024 bytes, and otherwise the largest binary unit up to
/// TB, with two decimals when the number is below ten and one decimal when it
/// is ten or more.
pub fn size_to_terabytes(value: u64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    // i18n: units, which are written in English in every language
    let units = ["KB", "MB", "GB", "TB"];
    let mut amount = value as f64 / 1024.;
    let mut unit = 0;
    while amount >= 1024. && unit < units.len() - 1 {
        amount /= 1024.;
        unit += 1;
    }
    let digits = if amount >= 10. { 1 } else { 2 };
    format!("{amount:.digits$} {}", units[unit])
}

/// Returns `text` whole when it fits in the width `max`, and otherwise as
/// many of its first characters as fit, with "…" after them. Punctuation just
/// before the cut is kept, which GPUI's own truncation does not do. `measure`
/// returns the width of a string.
///
/// Each call to `measure` shapes the text. A window of new tiles (after a
/// jump or a fast scroll) shapes every name it shows, so this function
/// calls `measure` as few times as it can. `guess` returns the number of
/// characters that probably fit, which the caller works out from the
/// shaping of the whole text. The function measures the cut at the guess
/// and the cut that is one character longer or shorter, and it searches
/// the rest by halving only when the guess was wrong. The search relies on
/// a longer cut never being narrower than a shorter one.
pub fn truncate_to(
    text: &str,
    max: f32,
    measure: impl Fn(&str) -> f32,
    guess: impl FnOnce() -> usize,
) -> String {
    if measure(text) <= max {
        return text.to_owned();
    }
    let chars = text.chars().collect::<Vec<_>>();
    let cut = |end: usize| chars[..end].iter().collect::<String>() + "…";
    let fit = |end: usize| measure(&cut(end)) <= max;
    // The longest cut that fits has its end in `fits..=last`. When no cut
    // fits, the result is "…" alone.
    let (mut fits, mut last) = (0, chars.len().saturating_sub(1));
    let guess = guess().min(last);
    if guess > 0 && !fit(guess) {
        last = guess - 1;
        if last > 0 && fit(last) {
            fits = last;
        }
    } else {
        fits = guess;
        if guess < last {
            if fit(guess + 1) {
                fits = guess + 1;
            } else {
                last = guess;
            }
        }
    }
    while fits < last {
        let middle = (fits + last).div_ceil(2);
        if fit(middle) {
            fits = middle;
        } else {
            last = middle - 1;
        }
    }
    cut(fits)
}

// The date formats are in this section.

/// The `chrono` format strings for the dates that the views write in a fixed
/// format and not in the system's format. The system's formats are in
/// `crate::i18n::format`.
pub mod date_format {
    /// The format of the inspector's date rows: `YYYY/MM/DD HH:MM`.
    pub const INSPECTOR: &str = "%Y/%m/%d %H:%M";
    /// The format in which date fields store a day: `YYYY-MM-DD`.
    pub const DAY: &str = "%Y-%m-%d";
}

// The functions in this section give media types their short names.

/// The media types that both the inspector and the type filter show by a
/// short name.
const TYPE_NAMES: [(&str, &str); 4] = [
    ("image/jpeg", "JPG"),
    ("image/svg+xml", "SVG"),
    ("video/quicktime", "MOV"),
    ("video/x-matroska", "MKV"),
];

/// Returns the name for a subtype that has no short name of its own: the
/// subtype in capital letters, without a leading `x-`.
fn subtype_name(subtype: &str) -> String {
    subtype.strip_prefix("x-").unwrap_or(subtype).to_uppercase()
}

/// Returns the name that the inspector's Type row shows for a media type.
/// Only the subtype, which is the part after the `/`, decides the name.
pub fn inspector_type(mime: &str) -> String {
    let subtype = mime.split_once('/').map_or("", |(_, subtype)| subtype);
    if subtype == "epub+zip" {
        return "EPUB".into();
    }
    TYPE_NAMES
        .iter()
        .find(|(known, _)| known.split_once('/').is_some_and(|(_, sub)| sub == subtype))
        .map_or_else(|| subtype_name(subtype), |(_, name)| (*name).into())
}

/// Returns the name that the filter bar's type menu shows for a media type:
/// the name of the family for a pattern such as `image/*`, the usual name for
/// a few common types, and otherwise the subtype in capital letters without a
/// leading `x-`.
pub fn filter_type(mime: &str) -> String {
    // i18n: translated where drawn (these are the rows of the filter menu)
    let family = match mime {
        "image/*" => Some("Images"),
        "video/*" => Some("Videos"),
        "audio/*" => Some("Audio"),
        "application/*" => Some("Documents & Files"),
        "text/*" => Some("Text"),
        "model/*" => Some("3D Models"),
        "font/*" => Some("Fonts"),
        "multipart/*" => Some("Web Archives"),
        "audio/mpeg" => Some("MP3"),
        "application/pdf" => Some("PDF"),
        _ => TYPE_NAMES
            .iter()
            .find(|(known, _)| *known == mime)
            .map(|(_, name)| *name),
    };
    if let Some(name) = family {
        return name.to_owned();
    }
    subtype_name(mime.rsplit('/').next().unwrap_or(mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_truncate_with_an_ellipsis_to_what_fits() {
        // Every character is ten units wide in this test.
        let measure = |text: &str| text.chars().count() as f32 * 10.;
        let none = || 0;
        assert_eq!(truncate_to("Tribal", 60., measure, none), "Tribal");
        assert_eq!(
            truncate_to("Main Four - Communal", 100., measure, none),
            "Main Four…"
        );
        // A space just before the cut is kept.
        assert_eq!(truncate_to("Main Four - C", 60., measure, none), "Main …");
        assert_eq!(truncate_to("ab", 5., measure, none), "…");
        // Punctuation just before the cut in a chip's label is kept as
        // well.
        assert_eq!(
            truncate_to("adobe_photoshop_(artwork)", 120., measure, none),
            "adobe_photo…"
        );
        assert_eq!(
            truncate_to("abcdefghij_(x)", 120., measure, none),
            "abcdefghij_…"
        );
    }

    #[test]
    fn sizes_read_as_format_bytes_writes_them() {
        assert_eq!(size_to_terabytes(512), "512 B");
        assert_eq!(size_to_terabytes(1536), "1.50 KB");
        assert_eq!(size_to_terabytes(12 * 1024 * 1024 + 300 * 1024), "12.3 MB");
        assert_eq!(size_to_terabytes(3 * 1024u64.pow(4) * 2048), "6144.0 TB");
    }

    #[test]
    fn names_sort_without_case_and_with_numbers_by_value() {
        let mut names = vec!["b", "A", "item10", "item9", "_x", "2d", "a"];
        names.sort_by(|left, right| natural_order(left, right));
        assert_eq!(names, ["_x", "2d", "A", "a", "b", "item9", "item10"]);
        assert_eq!(natural_order("a02", "a2"), Ordering::Equal);
        assert_eq!(natural_order("Artist 9", "artist 10"), Ordering::Less);
        assert_eq!(natural_order("art", "artist"), Ordering::Less);
        assert_eq!(natural_order("apple", "Banana"), Ordering::Less);
        assert_eq!(natural_order("artist 010", "Artist 10"), Ordering::Equal);
        // Punctuation sorts before digits.
        assert_eq!(natural_order("a-b", "a1"), Ordering::Less);
        assert_eq!(natural_order("a_", "a1"), Ordering::Less);
        // Punctuation marks are ordered among themselves by the Unicode
        // root collation. The `#` section of the Tags screen shows them in
        // this order.
        let mut symbols = vec![
            "!", "!!", ">_(", "...", "?", "^_^", "_/", "_>", "_3", "_d", "_o",
        ];
        symbols.sort_by(|left, right| natural_order(left, right));
        assert_eq!(
            symbols,
            [
                "_/", "_>", "_3", "_d", "_o", "!", "!!", "?", "...", "^_^", ">_("
            ]
        );
    }

    #[test]
    fn search_ranks_exact_then_prefix_then_word_start_then_substring() {
        assert_eq!(match_rank("cat", "cat"), Some(0));
        assert_eq!(match_rank("catgirl", "cat"), Some(1));
        assert_eq!(match_rank("black_cat", "cat"), Some(2));
        assert_eq!(match_rank("bobcat", "cat"), Some(3));
        assert_eq!(match_rank("dog", "cat"), None);
        // A later occurrence that overlaps an earlier one can be at the start
        // of a word, and the loop in `match_rank` finds it.
        assert_eq!(match_rank("xaa-aa", "aa"), Some(2));
        assert_eq!(match_rank("x-aaa", "aa"), Some(2));
        assert_eq!(match_rank("baaa a", "aa"), Some(3));
        let ranked = rank_matches(
            vec!["bobcat", "black_cat", "cat", "dog", "catgirl"],
            " CAT ",
            |name| name,
        );
        assert_eq!(ranked, ["cat", "catgirl", "black_cat", "bobcat"]);
        assert_eq!(match_range("Black Cat", "cat"), Some(6..9));
        assert_eq!(match_range("dog", "cat"), None);
        assert_eq!(match_range("dog", " "), None);
    }

    #[test]
    fn sizes_keep_each_view_s_units() {
        assert_eq!(size_to_gigabytes(512), "512 B");
        assert_eq!(size_to_gigabytes(1536), "1.5 KB");
        assert_eq!(size_to_gigabytes(5 * 1048576), "5.0 MB");
        assert_eq!(size_to_gigabytes(3 * 1073741824 / 2), "1.50 GB");
        assert_eq!(size_to_terabytes(1536), "1.50 KB");
        assert_eq!(size_to_terabytes(20 * 1048576), "20.0 MB");
    }
}
