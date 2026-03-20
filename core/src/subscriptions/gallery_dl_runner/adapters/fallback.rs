use serde_json::Value;
use super::SiteAdapter;

pub(super) static ADAPTER: FallbackAdapter = FallbackAdapter;

pub(super) struct FallbackAdapter;

impl SiteAdapter for FallbackAdapter {
    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        for key in ["artist", "user", "uploader"] {
            if let Some(name) = json
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(name.to_string());
            }
        }
        None
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        let Some(tags_val) = json.get("tags") else {
            return tags;
        };

        if let Some(arr) = tags_val.as_array() {
            for tag_val in arr {
                if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                    tags.push((String::new(), tag.to_string()));
                }
            }
            return tags;
        }

        if let Some(tag_str) = tags_val.as_str() {
            for tag in tag_str.split_whitespace() {
                if !tag.is_empty() {
                    tags.push((String::new(), tag.to_string()));
                }
            }
        }

        tags
    }
}
