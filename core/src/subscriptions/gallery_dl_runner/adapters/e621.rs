use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: E621Adapter = E621Adapter;

const NAMESPACE_MAP: &[(&str, &str)] = &[
    ("artist", "creator"),
    ("character", "character"),
    ("copyright", "series"),
    ("general", ""),
    ("species", "species"),
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
        if let Some(rating) = json.get("rating").and_then(Value::as_str) {
            let rating = match rating.trim().to_ascii_lowercase().as_str() {
                "s" | "safe" => Some("safe"),
                "q" | "questionable" => Some("questionable"),
                "e" | "explicit" => Some("explicit"),
                _ => None,
            };
            if let Some(rating) = rating {
                tags.push(("rating".to_string(), rating.to_string()));
            }
        }
        if found_categorized {
            return tags;
        }

        let Some(categories) = json.get("tags").and_then(Value::as_object) else {
            return tags;
        };
        for (category, value) in categories {
            let Some(namespace) = NAMESPACE_MAP
                .iter()
                .find_map(|(known, namespace)| (*known == category).then_some(*namespace))
            else {
                continue;
            };
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
            "tags_general": ["solo"],
            "tags_meta": ["highres"],
            "rating": "s"
        }));

        assert_eq!(
            tags,
            vec![
                ("creator".to_string(), "wlop".to_string()),
                (String::new(), "solo".to_string()),
                ("species".to_string(), "canine".to_string()),
                ("rating".to_string(), "safe".to_string())
            ]
        );
    }

    #[test]
    fn parses_legacy_category_object_and_drops_non_library_categories() {
        let tags = ADAPTER.parse_tags(&json!({
            "tags": {
                "artist": ["wlop"],
                "species": ["canine"],
                "general": ["solo"],
                "invalid": ["wat"]
            }
        }));

        assert!(tags.contains(&("creator".to_string(), "wlop".to_string())));
        assert!(tags.contains(&("species".to_string(), "canine".to_string())));
        assert!(tags.contains(&(String::new(), "solo".to_string())));
        assert!(!tags.iter().any(|(namespace, _)| namespace == "invalid"));
    }
}
