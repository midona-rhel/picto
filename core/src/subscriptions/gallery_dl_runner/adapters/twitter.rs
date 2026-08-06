use super::SiteAdapter;
use serde_json::Value;

pub(super) static ADAPTER: TwitterAdapter = TwitterAdapter;

pub(super) struct TwitterAdapter;

impl SiteAdapter for TwitterAdapter {
    fn parse_tags(&self, _json: &Value) -> Vec<(String, String)> {
        Vec::new()
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("author")
            .and_then(|author| author.get("name").or_else(|| author.get("nick")))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
    }
}
