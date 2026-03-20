use serde_json::Value;
use super::{SiteAdapter, push_unique_url};

pub(super) static ADAPTER: DanbooruAdapter = DanbooruAdapter;

const CATEGORIES: &[(&str, &str)] = &[
    ("tags_artist", "creator"),
    ("tags_character", "character"),
    ("tags_copyright", "series"),
    ("tags_general", ""),
    ("tags_meta", "meta"),
];

const TAG_STRINGS: &[(&str, &str)] = &[
    ("tag_string_artist", "creator"),
    ("tag_string_character", "character"),
    ("tag_string_copyright", "series"),
    ("tag_string_general", ""),
    ("tag_string_meta", "meta"),
];

pub(super) struct DanbooruAdapter;

impl SiteAdapter for DanbooruAdapter {
    fn collect_source_urls(&self, json: &Value) -> Vec<String> {
        let mut urls = Vec::new();
        push_unique_url(&mut urls, json.get("file_url").and_then(|v| v.as_str()));
        push_unique_url(&mut urls, json.get("url").and_then(|v| v.as_str()));
        push_unique_url(&mut urls, json.get("source").and_then(|v| v.as_str()));
        urls
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();

        // Array format: tags_artist, tags_character, etc.
        for (key, namespace) in CATEGORIES {
            if let Some(arr) = json.get(*key).and_then(|v| v.as_array()) {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if !tags.is_empty() {
            return tags;
        }

        // String format: tag_string_artist, tag_string_character, etc.
        for (key, namespace) in TAG_STRINGS {
            if let Some(tag_string) = json.get(*key).and_then(|v| v.as_str()) {
                for tag in tag_string.split_whitespace() {
                    if !tag.is_empty() {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if !tags.is_empty() {
            return tags;
        }

        // Flat fallback: tag_string (Danbooru) or tags (Gelbooru/Rule34)
        for field in ["tag_string", "tags"] {
            if let Some(tags_val) = json.get(field) {
                if let Some(tag_str) = tags_val.as_str() {
                    for tag in tag_str.split_whitespace() {
                        if !tag.is_empty() {
                            tags.push((String::new(), tag.to_string()));
                        }
                    }
                } else if let Some(arr) = tags_val.as_array() {
                    for tag_val in arr {
                        if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                            tags.push((String::new(), tag.to_string()));
                        }
                    }
                }
                if !tags.is_empty() {
                    return tags;
                }
            }
        }

        tags
    }
}
