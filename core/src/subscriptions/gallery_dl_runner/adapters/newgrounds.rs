use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: NewgroundsAdapter = NewgroundsAdapter;

pub(super) struct NewgroundsAdapter;

impl SiteAdapter for NewgroundsAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(values) = json.get("tags") {
            append_tag_values(&mut tags, "", values);
        }
        if let Some(values) = json.get("artist") {
            append_tag_values(&mut tags, "creator", values);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("user")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                json.get("artist")
                    .and_then(Value::as_array)
                    .and_then(|artists| artists.first())
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
    fn parses_site_tags_and_creator_without_inventing_namespaces() {
        let metadata = json!({
            "tags": ["animation", "pixel-art"],
            "artist": ["collaborator"],
            "user": "profile-owner"
        });

        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                (String::new(), "animation".to_string()),
                (String::new(), "pixel-art".to_string()),
                ("creator".to_string(), "collaborator".to_string()),
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("profile-owner".to_string())
        );
    }
}
