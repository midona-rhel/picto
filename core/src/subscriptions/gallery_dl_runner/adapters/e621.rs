use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: E621Adapter = E621Adapter;

const NAMESPACE_MAP: &[(&str, &str)] = &[
    ("artist", "artist"),
    ("character", "character"),
    ("copyright", "copyright"),
    ("general", ""),
    ("meta", "meta"),
    ("species", "species"),
    ("lore", "lore"),
];

pub(super) struct E621Adapter;

impl SiteAdapter for E621Adapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        let mut found_categorized = false;

        for (category, namespace) in NAMESPACE_MAP {
            let key = format!("tags_{category}");
            if let Some(value) = json.get(&key) {
                found_categorized = true;
                append_tag_values(&mut tags, namespace, value);
            }
        }
        if found_categorized {
            if let Some(fields) = json.as_object() {
                for (key, value) in fields {
                    let Some(category) = key.strip_prefix("tags_") else {
                        continue;
                    };
                    if NAMESPACE_MAP.iter().any(|(known, _)| *known == category) {
                        continue;
                    }
                    append_tag_values(&mut tags, category, value);
                }
            }
            return tags;
        }

        let Some(categories) = json.get("tags").and_then(Value::as_object) else {
            return tags;
        };
        for (category, value) in categories {
            let namespace = NAMESPACE_MAP
                .iter()
                .find_map(|(known, namespace)| (*known == category).then_some(*namespace))
                .unwrap_or(category.as_str());
            append_tag_values(&mut tags, namespace, value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        if let Some(value) = json.get("tags_artist") {
            let mut values = Vec::new();
            append_tag_values(&mut values, "", value);
            if let Some((_, creator)) = values.into_iter().next() {
                return Some(creator);
            }
        }
        json.get("tags")
            .and_then(Value::as_object)
            .and_then(|tags| tags.get("artist"))
            .and_then(|value| {
                let mut values = Vec::new();
                append_tag_values(&mut values, "", value);
                values.into_iter().next().map(|(_, creator)| creator)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_categorized_api_fields_without_flat_duplicates() {
        let tags = ADAPTER.parse_tags(&json!({
            "tags": ["canine", "solo", "wlop"],
            "tags_artist": ["wlop"],
            "tags_contributor": ["reviewer"],
            "tags_invalid": ["legacy_tag"],
            "tags_species": ["canine"],
            "tags_general": ["solo"]
        }));

        assert_eq!(
            tags,
            vec![
                ("artist".to_string(), "wlop".to_string()),
                ("".to_string(), "solo".to_string()),
                ("species".to_string(), "canine".to_string()),
                ("contributor".to_string(), "reviewer".to_string()),
                ("invalid".to_string(), "legacy_tag".to_string())
            ]
        );
    }

    #[test]
    fn parses_legacy_category_object_and_preserves_unknown_categories() {
        let tags = ADAPTER.parse_tags(&json!({
            "tags": {
                "artist": ["wlop"],
                "species": ["canine"],
                "general": ["solo"],
                "invalid": ["wat"]
            }
        }));

        assert!(tags.contains(&("artist".to_string(), "wlop".to_string())));
        assert!(tags.contains(&("species".to_string(), "canine".to_string())));
        assert!(tags.contains(&(String::new(), "solo".to_string())));
        assert!(tags.contains(&("invalid".to_string(), "wat".to_string())));
    }
}
