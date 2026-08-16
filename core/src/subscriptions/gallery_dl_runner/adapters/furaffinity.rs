use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: FurAffinityAdapter = FurAffinityAdapter;

pub(super) struct FurAffinityAdapter;

impl SiteAdapter for FurAffinityAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(value) = json.get("tags") {
            append_tag_values(&mut tags, "", value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("artist")
            .or_else(|| json.get("user"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_artist_and_keywords() {
        let raw = json!({"artist": "ExampleArtist", "tags": ["digital_art", "canine"]});
        assert_eq!(
            ADAPTER.extract_creator_identifier(&raw).as_deref(),
            Some("ExampleArtist")
        );
        assert_eq!(
            ADAPTER.parse_tags(&raw),
            [
                (String::new(), "digital_art".to_string()),
                (String::new(), "canine".to_string()),
            ]
        );
    }
}
