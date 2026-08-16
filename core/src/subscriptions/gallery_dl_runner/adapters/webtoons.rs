use super::SiteAdapter;
use serde_json::Value;

pub(super) static ADAPTER: WebtoonsAdapter = WebtoonsAdapter;

pub(super) struct WebtoonsAdapter;

impl SiteAdapter for WebtoonsAdapter {
    fn parse_tags(&self, _json: &Value) -> Vec<(String, String)> {
        Vec::new()
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        ["author_name", "username"].into_iter().find_map(|field| {
            json.get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn does_not_turn_structural_metadata_into_tags() {
        let tags = ADAPTER.parse_tags(&json!({
            "author_name": "Creator",
            "genre": "Fantasy",
            "lang": "en",
            "language": "English",
            "title": "Episode"
        }));

        assert!(tags.is_empty());
        assert_eq!(
            ADAPTER.extract_creator_identifier(&json!({"author_name": "Creator"})),
            Some("Creator".to_string())
        );
    }
}
