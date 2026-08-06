use super::SiteAdapter;
use serde_json::Value;

pub(super) static ADAPTER: PixivAdapter = PixivAdapter;

pub(super) struct PixivAdapter;

impl SiteAdapter for PixivAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        json.get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| {
                        // gallery-dl returns plain strings: ["tag1", "tag2"]
                        if let Some(s) = entry.as_str().filter(|s| !s.is_empty()) {
                            return Some((String::new(), s.to_string()));
                        }
                        // Legacy/API format: [{"name": "tag1", ...}]
                        entry
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|name| !name.is_empty())
                            .map(|name| (String::new(), name.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let user = json.get("user")?;
        if let Some(name) = user
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return Some(name.to_string());
        }
        if let Some(id) = user.get("id") {
            if let Some(n) = id.as_i64() {
                return Some(n.to_string());
            }
            if let Some(s) = id.as_str().map(str::trim).filter(|v| !v.is_empty()) {
                return Some(s.to_string());
            }
        }
        None
    }
}
