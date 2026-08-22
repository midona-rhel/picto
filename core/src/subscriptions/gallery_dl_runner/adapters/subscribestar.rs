use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: SubscribeStarAdapter = SubscribeStarAdapter;

pub(super) struct SubscribeStarAdapter;

impl SiteAdapter for SubscribeStarAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(value) = json.get("tags") {
            append_tag_values(&mut tags, "", value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        ["author_name", "author_nick"]
            .into_iter()
            .find_map(|field| field_text(json, field))
    }
}

fn field_text(json: &Value, key: &str) -> Option<String> {
    json.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_creator_and_tags_from_subscribestar_metadata() {
        let metadata = json!({
            "author_name": "creator-name",
            "author_nick": "Display Name",
            "tags": ["alpha", "beta"]
        });

        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("creator-name".to_string())
        );
        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                (String::new(), "alpha".to_string()),
                (String::new(), "beta".to_string())
            ]
        );
    }
}
