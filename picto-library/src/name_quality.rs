#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NameQuality {
    Empty,
    Numeric,
    HashLike,
    Generated,
    Meaningful,
}

pub(crate) fn quality(name: &str) -> NameQuality {
    let basename = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let stem = basename.rsplit_once('.').map_or(basename, |(stem, _)| stem);
    if stem.is_empty() || stem.eq_ignore_ascii_case("untitled") {
        return NameQuality::Empty;
    }
    let compact = stem
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    if compact.is_empty() {
        return NameQuality::Empty;
    }
    if stem.chars().all(|character| character.is_ascii_digit()) {
        return NameQuality::Numeric;
    }
    if stem.len() >= 12 && stem.chars().all(|character| character.is_ascii_hexdigit()) {
        return NameQuality::HashLike;
    }

    let alphabetic = compact
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let digits = compact.len().saturating_sub(alphabetic);
    let words = stem
        .split(|character: char| !character.is_ascii_alphabetic())
        .filter(|part| part.len() >= 3)
        .count();
    if words == 0 || (words <= 1 && digits >= 4) || (digits >= 4 && alphabetic <= 4) {
        NameQuality::Generated
    } else {
        NameQuality::Meaningful
    }
}

/// Prefer a strictly better incoming name while keeping equal-quality names stable.
pub(crate) fn preferred<'a>(existing: &'a str, incoming: &'a str) -> &'a str {
    if quality(incoming) > quality(existing) {
        incoming
    } else {
        existing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_every_supported_name_shape() {
        for (name, expected) in [
            ("", NameQuality::Empty),
            ("Untitled", NameQuality::Empty),
            ("14583420.png", NameQuality::Numeric),
            ("d92be9442094b7d22424a460cd5d5296", NameQuality::HashLike),
            ("gelbooru_14583420", NameQuality::Generated),
            ("post12345678", NameQuality::Generated),
            ("2085395535410712592_1.jpg", NameQuality::Generated),
            ("f1nn5ter - 2026-08-24", NameQuality::Generated),
            ("Lupa Hairpoon", NameQuality::Meaningful),
            ("wallpaper2", NameQuality::Meaningful),
        ] {
            assert_eq!(quality(name), expected, "unexpected quality for {name}");
        }
    }

    #[test]
    fn only_strictly_better_names_replace_existing_names() {
        assert_eq!(
            preferred("14583420", "gelbooru_14583420"),
            "gelbooru_14583420"
        );
        assert_eq!(preferred("gelbooru_14583420", "Human title"), "Human title");
        assert_eq!(preferred("Human title", "Other title"), "Human title");
        assert_eq!(preferred("Human title", "14583420"), "Human title");
    }
}
