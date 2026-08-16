use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: ArtStationAdapter = ArtStationAdapter;

pub(super) struct ArtStationAdapter;

impl SiteAdapter for ArtStationAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        for field in ["tags", "tag_names", "keywords"] {
            if let Some(value) = json.get(field) {
                append_tag_values(&mut tags, "", value);
            }
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        ["userinfo", "user", "artist"]
            .into_iter()
            .find_map(|field| {
                json.get(field)
                    .and_then(|value| value.get("username"))
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
    fn parses_project_tags_and_userinfo_creator() {
        let tags = ADAPTER.parse_tags(&json!({
            "tags": ["environment", {"name": "concept-art"}],
            "tag_names": ["environment"]
        }));
        assert_eq!(
            tags,
            vec![
                (String::new(), "environment".to_string()),
                (String::new(), "concept-art".to_string()),
                (String::new(), "environment".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&json!({
                "userinfo": {"username": "ArtistName"}
            })),
            Some("ArtistName".to_string())
        );
    }
}
