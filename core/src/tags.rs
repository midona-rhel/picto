//! Simplified tag utilities for Picto.
//!
//! Keeps: split_tag, combine_tag, clean_tag, clean_tags (essential for PTR tag normalization).
//! Drops: TagFilter whitelist/blacklist, human_text_sort_key (→ natord), display string helpers.
//!
//! Ported from HydrusTags.py — tag cleaning logic is preserved exactly for PTR compatibility.

use std::sync::OnceLock;

use regex::Regex;

// ---------------------------------------------------------------------------
// Regex patterns (lazy-initialised, mirrors HydrusText.py globals)
// ---------------------------------------------------------------------------

static RE_UNDESIRED_CONTROL_CHARACTERS: OnceLock<Regex> = OnceLock::new();
static RE_ONE_OR_MORE_WHITESPACE: OnceLock<Regex> = OnceLock::new();
static RE_LEADING_GARBAGE: OnceLock<Regex> = OnceLock::new();
// RE_LEADING_SINGLE_COLON_AND_NO_MORE_COLONS replaced with is_leading_single_colon_no_more()
static RE_OOPS_ALL_ZERO_WIDTH_JOINERS: OnceLock<Regex> = OnceLock::new();
static RE_ZERO_WIDTH_JOINERS: OnceLock<Regex> = OnceLock::new();
static RE_THIS_IS_ALL_LATIN_AND_ZERO_WIDTH: OnceLock<Regex> = OnceLock::new();
static RE_LOOKS_LIKE_HANGUL: OnceLock<Regex> = OnceLock::new();

const HANGUL_FILLER_CHARACTER: char = '\u{3164}';

fn re_undesired_control_characters() -> &'static Regex {
    RE_UNDESIRED_CONTROL_CHARACTERS.get_or_init(|| {
        Regex::new(
            r"[\x00-\x1F\x7F-\x9F\u{200B}\u{200E}\u{200F}\u{202A}-\u{202E}\u{2066}-\u{2069}\u{FEFF}\u{E000}-\u{F8FF}\u{F0000}-\u{FFFFD}\u{100000}-\u{10FFFD}]"
        ).unwrap()
    })
}

fn re_one_or_more_whitespace() -> &'static Regex {
    RE_ONE_OR_MORE_WHITESPACE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn re_leading_garbage() -> &'static Regex {
    RE_LEADING_GARBAGE.get_or_init(|| Regex::new(r"^(-|system:)+").unwrap())
}

/// Checks if a tag starts with a single colon and has no more colons.
/// Replaces the regex `^:(?=[^:]+$)` which uses look-ahead (not supported by regex crate).
fn is_leading_single_colon_no_more(s: &str) -> bool {
    s.starts_with(':') && s[1..].chars().all(|c| c != ':') && s.len() > 1
}

fn re_oops_all_zero_width_joiners() -> &'static Regex {
    RE_OOPS_ALL_ZERO_WIDTH_JOINERS.get_or_init(|| Regex::new(r"^[\u{200C}\u{200D}]+$").unwrap())
}

fn re_zero_width_joiners() -> &'static Regex {
    RE_ZERO_WIDTH_JOINERS.get_or_init(|| Regex::new(r"[\u{200C}\u{200D}]").unwrap())
}

fn re_this_is_all_latin_and_zero_width() -> &'static Regex {
    RE_THIS_IS_ALL_LATIN_AND_ZERO_WIDTH.get_or_init(|| {
        Regex::new(r"^[\u{0020}-\u{007E}\u{00A0}-\u{024F}\u{200C}\u{200D}]+$").unwrap()
    })
}

fn re_looks_like_hangul() -> &'static Regex {
    RE_LOOKS_LIKE_HANGUL
        .get_or_init(|| Regex::new(r"[\u{1100}-\u{11FF}\u{AC00}-\u{D7AF}]").unwrap())
}

// ---------------------------------------------------------------------------
// Core tag functions
// ---------------------------------------------------------------------------

/// A valid namespace is either empty (leading-colon = no namespace) or starts
/// with a letter and contains only letters, digits, underscores, hyphens, or
/// spaces.  This rejects emoticon fragments like `>`, `)`, `<3` that happen
/// to appear before a colon (e.g. `>:(`).
fn is_valid_namespace(s: &str) -> bool {
    if s.is_empty() {
        return true; // leading colon — no namespace
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ')
}

/// Split a "namespace:subtag" tag into (namespace, subtag).
/// If no colon, or the part before the colon is not a valid namespace,
/// namespace is "" and subtag is the whole tag.
pub fn split_tag(tag: &str) -> (&str, &str) {
    if let Some(colon_pos) = tag.find(':') {
        let candidate = &tag[..colon_pos];
        if is_valid_namespace(candidate) {
            (candidate, &tag[colon_pos + 1..])
        } else {
            ("", tag)
        }
    } else {
        ("", tag)
    }
}

/// Combine namespace and subtag into a single tag string.
/// If namespace is empty and subtag contains ':', prefixes with ':' to disambiguate.
pub fn combine_tag(namespace: &str, subtag: &str) -> String {
    if namespace.is_empty() {
        if subtag.contains(':') {
            format!(":{}", subtag)
        } else {
            subtag.to_string()
        }
    } else {
        format!("{}:{}", namespace, subtag)
    }
}

/// Strip unwanted characters and normalise text.
/// Matches Python StripTagTextOfGumpf exactly.
fn strip_tag_text_of_gumpf(t: &str) -> String {
    let mut t = re_undesired_control_characters()
        .replace_all(t, "")
        .to_string();
    t = re_one_or_more_whitespace().replace_all(&t, " ").to_string();
    t = t.trim().to_string();
    t = re_leading_garbage().replace_all(&t, "").to_string();
    t = t.trim().to_string();

    if re_looks_like_hangul().find(&t).is_none() {
        t = t.replace(HANGUL_FILLER_CHARACTER, "");
    }

    if re_this_is_all_latin_and_zero_width().is_match(&t) {
        t = re_zero_width_joiners().replace_all(&t, "").to_string();
    }

    t = re_oops_all_zero_width_joiners()
        .replace_all(&t, "")
        .to_string();
    t = re_one_or_more_whitespace().replace_all(&t, " ").to_string();
    t = t.trim().to_string();
    t
}

/// Clean a tag according to Hydrus rules (lowercase, strip garbage, normalise).
/// Essential for PTR tag normalization — must match Python behavior exactly.
pub fn clean_tag(tag: &str) -> Result<String, String> {
    if tag.is_empty() {
        return Err("Received an empty tag".to_string());
    }

    // Truncate to 1024 characters
    let tag_str: &str = if tag.len() > 1024 {
        let end = tag
            .char_indices()
            .nth(1024)
            .map(|(i, _)| i)
            .unwrap_or(tag.len());
        &tag[..end]
    } else {
        tag
    };

    let mut tag = tag_str.to_lowercase();

    if is_leading_single_colon_no_more(&tag) {
        tag = format!(":{}", tag);
    }

    if tag.contains(':') {
        tag = strip_tag_text_of_gumpf(&tag);

        let (namespace, subtag) = {
            let (ns, st) = split_tag(&tag);
            (ns.to_string(), st.to_string())
        };

        let namespace = strip_tag_text_of_gumpf(&namespace);
        let subtag = strip_tag_text_of_gumpf(&subtag);

        tag = combine_tag(&namespace, &subtag);
    } else {
        tag = strip_tag_text_of_gumpf(&tag);
    }

    Ok(tag)
}

/// Check that a tag's subtag part is non-empty.
pub fn check_tag_not_empty(tag: &str) -> Result<(), String> {
    let (_, subtag) = split_tag(tag);
    if subtag.is_empty() {
        Err("Received a zero-length tag!".to_string())
    } else {
        Ok(())
    }
}

/// Parse a raw tag string into (namespace, subtag) after cleaning and validation.
/// Returns `None` for invalid/empty tags.
pub fn parse_tag(raw: &str) -> Option<(String, String)> {
    let cleaned = clean_tag(raw).ok()?;
    if check_tag_not_empty(&cleaned).is_err() {
        return None;
    }
    let (ns, st) = split_tag(&cleaned);
    Some((ns.to_string(), st.to_string()))
}

/// Parse multiple raw tag strings into (namespace, subtag) pairs.
/// Silently skips invalid/empty tags.
pub fn parse_tags(raw_tags: &[String]) -> Vec<(String, String)> {
    raw_tags.iter().filter_map(|s| parse_tag(s)).collect()
}

/// Namespaces accepted on external ingest paths (import/subscription/PTR-like feeds).
/// Any other `ns:tag` input is coerced to an unnamespaced tag literal `ns:tag`.
pub fn is_ingest_namespace_allowed(namespace: &str) -> bool {
    matches!(
        namespace,
        "creator"
            | "studio"
            | "character"
            | "person"
            | "series"
            | "species"
            | "meta"
            | "system"
            | "artist"
            | "copyright"
            | "general"
            | "rating"
            | "source"
    )
}

/// Parse a raw tag for external ingest.
/// Unknown namespaces are treated as part of the literal tag text, not as namespace keys.
pub fn parse_tag_ingest(raw: &str) -> Option<(String, String)> {
    let cleaned = clean_tag(raw).ok()?;
    if check_tag_not_empty(&cleaned).is_err() {
        return None;
    }
    let (ns, st) = split_tag(&cleaned);
    if ns.is_empty() || is_ingest_namespace_allowed(ns) {
        return Some((ns.to_string(), st.to_string()));
    }
    Some(("".to_string(), format!("{}:{}", ns, st)))
}

/// Parse multiple raw tags for external ingest.
pub fn parse_tags_ingest(raw_tags: &[String]) -> Vec<(String, String)> {
    raw_tags
        .iter()
        .filter_map(|s| parse_tag_ingest(s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_tag() {
        assert_eq!(split_tag("artist:bob"), ("artist", "bob"));
        assert_eq!(split_tag("solo"), ("", "solo"));
        assert_eq!(split_tag(":has:colons"), ("", "has:colons"));
        // Emoticon tags — non-alpha prefix rejected as namespace
        assert_eq!(split_tag(">:("), ("", ">:("));
        assert_eq!(split_tag("</3"), ("", "</3"));
        assert_eq!(split_tag("<3:heart"), ("", "<3:heart"));
        // Leading colon — empty namespace is valid (disambiguation form)
        assert_eq!(split_tag(":("), ("", "("));
        assert_eq!(split_tag(":)"), ("", ")"));
        // Valid namespaces still work
        assert_eq!(split_tag("character:samus"), ("character", "samus"));
        assert_eq!(split_tag("my ns:tag"), ("my ns", "tag"));
        assert_eq!(split_tag("meta-data:value"), ("meta-data", "value"));
    }

    #[test]
    fn test_combine_tag() {
        assert_eq!(combine_tag("artist", "bob"), "artist:bob");
        assert_eq!(combine_tag("", "solo"), "solo");
        assert_eq!(combine_tag("", "has:colons"), ":has:colons");
    }

    #[test]
    fn test_clean_tag_basic() {
        assert_eq!(clean_tag("  Artist:Bob  ").unwrap(), "artist:bob");
        assert_eq!(clean_tag("UPPERCASE").unwrap(), "uppercase");
        assert!(clean_tag("").is_err());
    }

    #[test]
    fn test_parse_tag() {
        assert_eq!(
            parse_tag("Artist:Bob"),
            Some(("artist".into(), "bob".into()))
        );
        assert_eq!(parse_tag("  solo  "), Some(("".into(), "solo".into())));
        assert_eq!(parse_tag(""), None);
        // Emoticon tags stay unnamespaced
        assert_eq!(parse_tag(">:("), Some(("".into(), ">:(".into())));
    }

    #[test]
    fn test_parse_tags() {
        let tags = vec![
            "Artist:Bob".into(),
            "".into(),
            "  solo  ".into(),
            "UPPERCASE".into(),
        ];
        let parsed = parse_tags(&tags);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], ("artist".into(), "bob".into()));
    }

    #[test]
    fn test_parse_tag_ingest_unknown_namespace_is_literal() {
        assert_eq!(
            parse_tag_ingest("http://example.com"),
            Some(("".into(), "http://example.com".into()))
        );
        assert_eq!(
            parse_tag_ingest("foo:bar"),
            Some(("".into(), "foo:bar".into()))
        );
    }

    #[test]
    fn test_parse_tag_ingest_allowed_namespace_stays_namespaced() {
        assert_eq!(
            parse_tag_ingest("creator:foo"),
            Some(("creator".into(), "foo".into()))
        );
        assert_eq!(
            parse_tag_ingest("character:samus"),
            Some(("character".into(), "samus".into()))
        );
    }
}
