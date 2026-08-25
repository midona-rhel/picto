//! Canonical replacement tag-name parsing.

const EXTERNAL_NAMESPACES: &[&str] = &[
    "general",
    "creator",
    "character",
    "series",
    "species",
    "meta",
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
/// Unknown prefixes remain searchable literal text under `general`.
pub fn parse_external(value: &str) -> Result<(String, String), String> {
    let (namespace, subtag) = parse_local(value)?;
    let namespace = match namespace.as_str() {
        "artist" | "contributor" => "creator",
        "copyright" => "series",
        "metadata" => "meta",
        "tag" | "ungrouped" => "general",
        value => value,
    };
    if EXTERNAL_NAMESPACES.contains(&namespace) {
        Ok((namespace.to_string(), subtag))
    } else {
        Ok(("general".to_string(), format!("{namespace}:{subtag}")))
    }
}

pub fn format(namespace: &str, subtag: &str) -> String {
    format!("{namespace}:{subtag}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_namespaces_are_mapped_or_folded_into_general() {
        assert_eq!(
            parse_external("artist:Someone").unwrap(),
            ("creator".to_string(), "someone".to_string())
        );
        assert_eq!(
            parse_external("category:Original").unwrap(),
            ("general".to_string(), "category:original".to_string())
        );
        assert_eq!(
            parse_external("metadata:Highres").unwrap(),
            ("meta".to_string(), "highres".to_string())
        );
        assert_eq!(
            parse_external("tag:Blue Eyes").unwrap(),
            ("general".to_string(), "blue eyes".to_string())
        );
        assert_eq!(
            parse_external("contributor:Editor").unwrap(),
            ("creator".to_string(), "editor".to_string())
        );
        assert_eq!(
            parse_external("lore:Backstory").unwrap(),
            ("general".to_string(), "lore:backstory".to_string())
        );
        assert_eq!(
            parse_external("studio:Example").unwrap(),
            ("general".to_string(), "studio:example".to_string())
        );
    }

    #[test]
    fn local_namespaces_remain_user_controlled() {
        assert_eq!(
            parse_local("personal:Favorite").unwrap(),
            ("personal".to_string(), "favorite".to_string())
        );
    }
}
