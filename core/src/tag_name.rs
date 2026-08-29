//! Canonical replacement tag-name parsing.

const EXTERNAL_NAMESPACES: &[&str] = &[
    "general",
    "creator",
    "character",
    "series",
    "species",
    "rating",
];

pub fn parse_local(value: &str) -> Result<(String, String), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Tag name is required".to_string());
    }
    let (namespace, subtag) = value
        .split_once(':')
        .map(|(namespace, subtag)| (namespace.trim(), subtag.trim()))
        .unwrap_or(("general", value));
    if namespace.is_empty() || subtag.is_empty() {
        return Err("Tag namespace and subtag must not be empty".to_string());
    }
    Ok((namespace.to_lowercase(), subtag.to_lowercase()))
}

/// External adapters may use known aliases but cannot create namespaces.
pub fn parse_external(value: &str) -> Result<(String, String), String> {
    let (namespace, subtag) = parse_local(value)?;
    let namespace = match namespace.as_str() {
        "artist" | "contributor" => "creator",
        "copyright" => "series",
        "tag" | "ungrouped" => "general",
        value => value,
    };
    if EXTERNAL_NAMESPACES.contains(&namespace) {
        Ok((namespace.to_string(), subtag))
    } else {
        Err(format!(
            "External tag namespace `{namespace}` is not supported"
        ))
    }
}

/// Map an external source category pair onto the canonical namespaces.
/// Known aliases specialize; every unmapped category falls back to `general`
/// — sources cannot invent namespaces, and their tags are never dropped.
pub fn normalize_external_pair(namespace: &str, subtag: &str) -> Option<(String, String)> {
    let subtag = subtag.trim().to_lowercase();
    if subtag.is_empty() {
        return None;
    }
    let namespace = namespace.trim().to_lowercase();
    let namespace = match namespace.as_str() {
        "" | "tag" | "ungrouped" => "general",
        "artist" | "contributor" => "creator",
        "copyright" => "series",
        value if EXTERNAL_NAMESPACES.contains(&value) => value,
        _ => "general",
    };
    let subtag = if namespace == "general" {
        // The bare general form carries no prefix, so a colon inside the
        // value would later read as a namespace separator.
        subtag.replace(':', "_")
    } else {
        subtag
    };
    Some((namespace.to_string(), subtag))
}

pub fn format(namespace: &str, subtag: &str) -> String {
    if namespace.is_empty() || namespace.eq_ignore_ascii_case("general") {
        subtag.to_string()
    } else {
        format!("{namespace}:{subtag}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_namespaces_are_mapped_or_rejected() {
        assert_eq!(
            parse_external("artist:Someone").unwrap(),
            ("creator".to_string(), "someone".to_string())
        );
        assert!(parse_external("category:Original").is_err());
        assert!(parse_external("metadata:Highres").is_err());
        assert_eq!(
            parse_external("tag:Blue Eyes").unwrap(),
            ("general".to_string(), "blue eyes".to_string())
        );
        assert_eq!(
            parse_external("contributor:Editor").unwrap(),
            ("creator".to_string(), "editor".to_string())
        );
        assert!(parse_external("lore:Backstory").is_err());
        assert!(parse_external("studio:Example").is_err());
    }

    #[test]
    fn general_tags_are_serialized_without_a_namespace() {
        assert_eq!(format("", "solo"), "solo");
        assert_eq!(format("general", "solo"), "solo");
        assert_eq!(format("character", "hero"), "character:hero");
    }

    #[test]
    fn unmapped_external_categories_fall_back_to_general() {
        assert_eq!(
            normalize_external_pair("meta", "Highres").unwrap(),
            ("general".into(), "highres".into())
        );
        assert_eq!(
            normalize_external_pair("lore", "Backstory").unwrap(),
            ("general".into(), "backstory".into())
        );
        assert_eq!(
            normalize_external_pair("artist", "Someone").unwrap(),
            ("creator".into(), "someone".into())
        );
        assert_eq!(
            normalize_external_pair("copyright", "Original").unwrap(),
            ("series".into(), "original".into())
        );
        // A colon inside a general value would read as a namespace separator
        // once stored without a prefix.
        assert_eq!(
            normalize_external_pair("", "re:zero").unwrap(),
            ("general".into(), "re_zero".into())
        );
        assert_eq!(normalize_external_pair("meta", "   "), None);
    }

    #[test]
    fn local_namespaces_remain_user_controlled() {
        assert_eq!(
            parse_local("personal:Favorite").unwrap(),
            ("personal".to_string(), "favorite".to_string())
        );
    }
}
