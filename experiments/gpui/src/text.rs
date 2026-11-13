//! Text rules more than one view shares: the natural name order, search
//! ranking and highlighting, byte sizes, the date formats, and the short
//! names of common media types.
use std::cmp::Ordering;

// ── Name order ──────────────────────────────────────────────────

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

/// The ASCII punctuation and symbols in the order of the Unicode root
/// collation, which Chromium's `localeCompare` follows; any other
/// punctuation sorts after them by code point.
const PUNCTUATION_ORDER: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// Where a character sorts within its class: punctuation by the root
/// collation's order, everything else by its case-folded value.
fn within_class(character: char) -> (usize, String) {
    let rank = PUNCTUATION_ORDER
        .find(character)
        .unwrap_or(PUNCTUATION_ORDER.len());
    (rank, character.to_lowercase().collect())
}

/// `localeCompare(…, { sensitivity: 'base', numeric: true })`: case is
/// ignored and runs of digits compare by their value.
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

// ── Search ──────────────────────────────────────────────────────

/// `matchRank` in `searchRank.ts`: 0 exact, 1 prefix, 2 at the start of a
/// word, 3 anywhere; `None` when absent. Both sides are lower case.
pub fn match_rank(text: &str, needle: &str) -> Option<u8> {
    if text == needle {
        return Some(0);
    }
    if text.starts_with(needle) {
        return Some(1);
    }
    // Every occurrence, overlapping ones included, as `indexOf` steps.
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

/// The first case-insensitive match of `query` in `text`, as a byte range,
/// for `highlightMatch`; none where lower-casing would move the bytes.
pub fn match_range(text: &str, query: &str) -> Option<std::ops::Range<usize>> {
    let query = query.trim();
    let lower = text.to_lowercase();
    if query.is_empty() || lower.len() != text.len() {
        return None;
    }
    let start = lower.find(&query.to_lowercase())?;
    Some(start..start + query.len())
}

// ── Byte sizes ──────────────────────────────────────────────────
//
// Production writes sizes three ways in three places; each keeps its own.

/// The inspector's Size row: bytes, then KB and MB to one decimal and GB to
/// two, in binary units.
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

/// `formatBytes` in the duplicates compare view: bytes, then KB and MB to
/// one decimal, in binary units, never larger than MB.
pub fn size_to_megabytes(bytes: i64) -> String {
    let value = bytes as f64;
    if value < 1024. {
        format!("{bytes} B")
    } else if value < 1024. * 1024. {
        format!("{:.1} KB", value / 1024.)
    } else {
        format!("{:.1} MB", value / (1024. * 1024.))
    }
}

/// `formatBytes` in Settings: bytes, then the largest binary unit up to TB,
/// with two decimals under ten and one above.
pub fn size_to_terabytes(value: i64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
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

// ── Dates ───────────────────────────────────────────────────────

/// The `chrono` formats the views write dates in, each named for what it
/// ports.
pub mod date_format {
    /// The inspector's date rows: `YYYY/MM/DD HH:MM`.
    pub const INSPECTOR: &str = "%Y/%m/%d %H:%M";
    /// `toLocaleDateString()` in en-US: month/day/year without zeros.
    pub const LOCALE_DATE: &str = "%-m/%-d/%Y";
    /// `toLocaleString()` in en-US: the date, then the time with seconds.
    pub const LOCALE_DATE_TIME: &str = "%-m/%-d/%Y, %-I:%M:%S %p";
    /// `formatDateTime`: en-US medium date and short time.
    pub const MEDIUM_DATE_TIME: &str = "%b %-d, %Y, %-I:%M %p";
    /// The date picker's field: en-US medium date.
    pub const MEDIUM_DATE: &str = "%b %-d, %Y";
    /// The calendar's heading: month and year.
    pub const MONTH_YEAR: &str = "%B %Y";
    /// A day as date fields store it (`YYYY-MM-DD`).
    pub const DAY: &str = "%Y-%m-%d";
}

// ── Media types ─────────────────────────────────────────────────

/// Types both the inspector and the type filter call by a short name.
const TYPE_NAMES: [(&str, &str); 4] = [
    ("image/jpeg", "JPG"),
    ("image/svg+xml", "SVG"),
    ("video/quicktime", "MOV"),
    ("video/x-matroska", "MKV"),
];

/// A subtype's plain name: in capitals without its `x-`.
fn subtype_name(subtype: &str) -> String {
    subtype.strip_prefix("x-").unwrap_or(subtype).to_uppercase()
}

/// `fmtExt` in the inspector's Type row: by subtype alone.
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

/// `mimeLabel` in the filter bar's type menu: a family's name, a few
/// common types by their usual names, else the subtype's plain name.
pub fn filter_type(mime: &str) -> String {
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
    fn names_sort_without_case_and_with_numbers_by_value() {
        let mut names = vec!["b", "A", "item10", "item9", "_x", "2d", "a"];
        names.sort_by(|left, right| natural_order(left, right));
        assert_eq!(names, ["_x", "2d", "A", "a", "b", "item9", "item10"]);
        assert_eq!(natural_order("a02", "a2"), Ordering::Equal);
        assert_eq!(natural_order("Artist 9", "artist 10"), Ordering::Less);
        assert_eq!(natural_order("art", "artist"), Ordering::Less);
        assert_eq!(natural_order("apple", "Banana"), Ordering::Less);
        assert_eq!(natural_order("artist 010", "Artist 10"), Ordering::Equal);
        // Punctuation sorts before digits, as the collator puts it.
        assert_eq!(natural_order("a-b", "a1"), Ordering::Less);
        assert_eq!(natural_order("a_", "a1"), Ordering::Less);
        // Punctuation among itself follows the root collation, as the
        // Tags screen's `#` section shows it.
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
        // An overlapping later match can start a word, as `indexOf` finds.
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
        assert_eq!(size_to_megabytes(3 * 1073741824 / 2), "1536.0 MB");
        assert_eq!(size_to_terabytes(1536), "1.50 KB");
        assert_eq!(size_to_terabytes(20 * 1048576), "20.0 MB");
    }

    #[test]
    fn type_names_share_the_short_names() {
        assert_eq!(inspector_type("image/jpeg"), "JPG");
        assert_eq!(inspector_type("video/quicktime"), "MOV");
        assert_eq!(inspector_type("video/x-matroska"), "MKV");
        assert_eq!(inspector_type("image/svg+xml"), "SVG");
        assert_eq!(
            inspector_type("application/x-shockwave-flash"),
            "SHOCKWAVE-FLASH"
        );
        assert_eq!(inspector_type("application/epub+zip"), "EPUB");
        assert_eq!(inspector_type("audio/mpeg"), "MPEG");
        assert_eq!(filter_type("audio/mpeg"), "MP3");
        assert_eq!(filter_type("image/jpeg"), "JPG");
        assert_eq!(filter_type("image/*"), "Images");
        assert_eq!(filter_type("video/x-msvideo"), "MSVIDEO");
        assert_eq!(filter_type("application/epub+zip"), "EPUB+ZIP");
    }
}
