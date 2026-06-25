use super::{push_unique_url, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: E621Adapter = E621Adapter;

const NAMESPACE_MAP: &[(&str, &str)] = &[
    ("artist", "creator"),
    ("character", "character"),
    ("copyright", "series"),
    ("general", ""),
    ("meta", "meta"),
    ("species", "species"),
    ("lore", "lore"),
];

pub(super) struct E621Adapter;

impl SiteAdapter for E621Adapter {
    fn collect_source_urls(&self, json: &Value) -> Vec<String> {
        let mut urls = Vec::new();
        push_unique_url(
            &mut urls,
            json.get("file")
                .and_then(|f| f.get("url"))
                .and_then(|v| v.as_str()),
        );
        if let Some(arr) = json.get("sources").and_then(|v| v.as_array()) {
            for val in arr {
                push_unique_url(&mut urls, val.as_str());
            }
        }
        urls
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();

        // gallery-dl >= 1.32 flattens the categorized `tags` object into a
        // plain list and exposes categories as danbooru-style `tags_<cat>`
        // keys. Prefer those when present.
        let mut found_categorized = false;
        for (category, namespace) in NAMESPACE_MAP {
            let key = format!("tags_{category}");
            if let Some(arr) = json.get(&key).and_then(|v| v.as_array()) {
                found_categorized = true;
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if found_categorized {
            return tags;
        }

        // Legacy shape (gallery-dl < 1.32): `tags` is an object keyed by category.
        let Some(obj) = json.get("tags").and_then(|v| v.as_object()) else {
            return tags;
        };

        for (category, namespace) in NAMESPACE_MAP {
            if let Some(arr) = obj.get(*category).and_then(|v| v.as_array()) {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }

        // Unmapped categories pass through with their original name
        for (category, value) in obj {
            let mapped = NAMESPACE_MAP
                .iter()
                .any(|(cat, _)| *cat == category.as_str());
            if mapped {
                continue;
            }
            if let Some(arr) = value.as_array() {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((category.clone(), tag.to_string()));
                    }
                }
            }
        }

        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_modern_flattened_shape() {
        // gallery-dl >= 1.32: tags_<category> arrays + flat tags list
        let meta = json!({
            "tags": ["canine", "dragon", "wlop"],
            "tags_artist": ["wlop"],
            "tags_species": ["canine", "dragon"],
            "tags_general": ["solo"]
        });
        let tags = ADAPTER.parse_tags(&meta);
        assert!(tags.contains(&("creator".to_string(), "wlop".to_string())));
        assert!(tags.contains(&("species".to_string(), "canine".to_string())));
        assert!(tags.contains(&(String::new(), "solo".to_string())));
        // Flat list must NOT leak through as untagged duplicates
        assert_eq!(tags.len(), 4);
    }

    #[test]
    fn parses_legacy_object_shape() {
        let meta = json!({
            "tags": {
                "artist": ["wlop"],
                "species": ["dragon"],
                "general": ["solo"],
                "invalid": ["wat"]
            }
        });
        let tags = ADAPTER.parse_tags(&meta);
        assert!(tags.contains(&("creator".to_string(), "wlop".to_string())));
        assert!(tags.contains(&("species".to_string(), "dragon".to_string())));
        assert!(tags.contains(&("invalid".to_string(), "wat".to_string())));
    }
}
