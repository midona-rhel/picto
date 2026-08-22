use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: FanboxAdapter = FanboxAdapter;

pub(super) struct FanboxAdapter;

impl SiteAdapter for FanboxAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(value) = json.get("tags") {
            append_tag_values(&mut tags, "", value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        field_text(json, "creatorId").or_else(|| {
            json.get("user")
                .and_then(|user| field_text(user, "userId").or_else(|| field_text(user, "name")))
        })
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
    fn extracts_creator_and_tags_from_fanbox_metadata() {
        let metadata = json!({
            "creatorId": "creator-name",
            "tags": ["exclusive", "wip"]
        });

        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("creator-name".to_string())
        );
        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                (String::new(), "exclusive".to_string()),
                (String::new(), "wip".to_string())
            ]
        );
    }

    #[test]
    fn falls_back_to_user_metadata_when_creator_id_is_missing() {
        let metadata = json!({
            "user": {
                "userId": "fallback-user",
                "name": "Fallback Name"
            }
        });

        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("fallback-user".to_string())
        );
    }
}
