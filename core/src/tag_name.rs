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
    fn local_namespaces_remain_user_controlled() {
        assert_eq!(
            parse_local("personal:Favorite").unwrap(),
            ("personal".to_string(), "favorite".to_string())
        );
    }
}
