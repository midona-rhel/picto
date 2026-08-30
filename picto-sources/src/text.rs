use std::sync::OnceLock;

pub fn normalize_source_text(raw: &str) -> Option<String> {
    static BRACKET_TAGS: OnceLock<regex::Regex> = OnceLock::new();
    static WIKI_LINK_LABELED: OnceLock<regex::Regex> = OnceLock::new();
    static WIKI_LINK: OnceLock<regex::Regex> = OnceLock::new();
    static TAG_SEARCH: OnceLock<regex::Regex> = OnceLock::new();
    static LABELED_URL: OnceLock<regex::Regex> = OnceLock::new();
    static DTEXT_HEADER: OnceLock<regex::Regex> = OnceLock::new();

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let plain = BRACKET_TAGS
        .get_or_init(|| {
            regex::Regex::new(
                r"(?i)\[/?(?:b|i|u|s|code|quote|section|spoiler|url|color|sup|sub|size|center|left|right|justify|tn|table|thead|tbody|tr|td|th|hr)(?:[=,][^\]]*)?\]",
            )
            .expect("valid bracketed markup regex")
        })
        .replace_all(trimmed, " ");
    let plain = WIKI_LINK_LABELED
        .get_or_init(|| regex::Regex::new(r"\[\[[^\]|]*\|([^\]]+)\]\]").expect("valid wiki regex"))
        .replace_all(&plain, "$1");
    let plain = WIKI_LINK
        .get_or_init(|| regex::Regex::new(r"\[\[([^\]]+)\]\]").expect("valid wiki regex"))
        .replace_all(&plain, "$1");
    let plain = TAG_SEARCH
        .get_or_init(|| regex::Regex::new(r"\{\{([^}]+)\}\}").expect("valid tag-search regex"))
        .replace_all(&plain, "$1");
    let plain = LABELED_URL
        .get_or_init(|| {
            regex::Regex::new(r#""([^"\n]+)":(?:\[(?:https?://|/)[^\]]*\]|(?:https?://|/)\S+)"#)
                .expect("valid labeled-url regex")
        })
        .replace_all(&plain, "$1");
    let plain = DTEXT_HEADER
        .get_or_init(|| regex::Regex::new(r"(?m)^h[1-6]\.\s*").expect("valid header regex"))
        .replace_all(&plain, "");
    html_to_plain_text(&plain)
}

fn html_to_plain_text(raw: &str) -> Option<String> {
    static TAGS: OnceLock<regex::Regex> = OnceLock::new();
    static NUMERIC_ENTITIES: OnceLock<regex::Regex> = OnceLock::new();
    static NAMED_ENTITIES: OnceLock<regex::Regex> = OnceLock::new();
    static SPACE_BEFORE_PUNCTUATION: OnceLock<regex::Regex> = OnceLock::new();

    let plain = TAGS
        .get_or_init(|| regex::Regex::new(r"(?s)<[^>]*>").expect("valid HTML regex"))
        .replace_all(raw, " ");
    let plain = NAMED_ENTITIES
        .get_or_init(|| regex::Regex::new(r"&([A-Za-z][A-Za-z0-9]+);").expect("valid entity regex"))
        .replace_all(&plain, |captures: &regex::Captures| {
            match &captures[1] {
                "nbsp" => " ",
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "quot" | "ldquo" | "rdquo" => "\"",
                "apos" | "lsquo" | "rsquo" => "'",
                "ndash" | "mdash" => "-",
                "hellip" => "...",
                "bull" => "*",
                "middot" => "·",
                "copy" => "©",
                "reg" => "®",
                "trade" => "™",
                "laquo" => "«",
                "raquo" => "»",
                _ => return captures[0].to_string(),
            }
            .to_string()
        });
    let plain = NUMERIC_ENTITIES
        .get_or_init(|| regex::Regex::new(r"&#(x?[0-9A-Fa-f]+);").expect("valid entity regex"))
        .replace_all(&plain, |captures: &regex::Captures| {
            let raw = &captures[1];
            raw.strip_prefix(['x', 'X'])
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| raw.parse::<u32>().ok())
                .and_then(char::from_u32)
                .map(|value| value.to_string())
                .unwrap_or_default()
        });
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = SPACE_BEFORE_PUNCTUATION
        .get_or_init(|| regex::Regex::new(r"\s+([.,!?;:])").expect("valid punctuation regex"))
        .replace_all(&plain, "$1")
        .into_owned();
    (!plain.is_empty()).then_some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_cross_provider_markup_without_dropping_prose() {
        assert_eq!(
            normalize_source_text(
                "[quote]Midna 🐺💦[/quote] <b>[[follow|Follow-up]]</b> &amp; more",
            )
            .as_deref(),
            Some("Midna 🐺💦 Follow-up & more"),
        );
    }

    #[test]
    fn preserves_non_markup_bracketed_text() {
        assert_eq!(
            normalize_source_text("[18+] prose").as_deref(),
            Some("[18+] prose")
        );
    }
}
