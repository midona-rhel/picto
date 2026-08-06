use super::{push_unique_url, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: PixivAdapter = PixivAdapter;

pub(super) struct PixivAdapter;

impl SiteAdapter for PixivAdapter {
    fn collect_source_urls(&self, json: &Value) -> Vec<String> {
        let mut urls = Vec::new();
        let post_id = json.get("id").and_then(|value| {
            value
                .as_u64()
                .map(|id| id.to_string())
                .or_else(|| value.as_str().map(ToOwned::to_owned))
        });
        if let Some(post_id) = post_id {
            urls.push(format!("https://www.pixiv.net/en/artworks/{post_id}"));
        }
        push_unique_url(&mut urls, json.get("file_url").and_then(|v| v.as_str()));
        push_unique_url(&mut urls, json.get("url").and_then(|v| v.as_str()));
        urls
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_urls_prefer_the_pixiv_artwork_page() {
        let metadata = json!({
            "id": 114223105,
            "url": "https://i.pximg.net/img-original/example_p0.png"
        });

        assert_eq!(
            ADAPTER.collect_source_urls(&metadata),
            vec![
                "https://www.pixiv.net/en/artworks/114223105".to_string(),
                "https://i.pximg.net/img-original/example_p0.png".to_string(),
            ]
        );
    }
}
