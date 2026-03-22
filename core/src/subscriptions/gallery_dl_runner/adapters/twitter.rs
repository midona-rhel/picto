use super::SiteAdapter;
use serde_json::Value;

pub(super) static ADAPTER: TwitterAdapter = TwitterAdapter;

pub(super) struct TwitterAdapter;

impl SiteAdapter for TwitterAdapter {
    fn collect_source_urls(&self, json: &Value) -> Vec<String> {
        let author = json
            .get("author")
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tweet_id = json
            .get("tweet_id")
            .and_then(|v| v.as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        if !author.is_empty() && !tweet_id.is_empty() {
            vec![format!("https://x.com/{}/status/{}", author, tweet_id)]
        } else {
            Vec::new()
        }
    }

    fn parse_tags(&self, _json: &Value) -> Vec<(String, String)> {
        Vec::new()
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("author")
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
    }
}
