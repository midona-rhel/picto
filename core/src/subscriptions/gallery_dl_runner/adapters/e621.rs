use serde_json::Value;
use super::{SiteAdapter, push_unique_url};

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
