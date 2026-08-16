use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: HentaiFoundryAdapter = HentaiFoundryAdapter;

pub(super) struct HentaiFoundryAdapter;

impl SiteAdapter for HentaiFoundryAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(value) = json.get("tags") {
            append_tag_values(&mut tags, "", value);
        }
        if let Some(value) = json.get("categories") {
            append_tag_values(&mut tags, "", value);
        }
        if let Some(value) = json.get("ratings") {
            append_tag_values(&mut tags, "", value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        ["artist", "user"].into_iter().find_map(|field| {
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
    fn keeps_hentai_foundry_taxonomy_as_unnamespaced_tags() {
        let tags = ADAPTER.parse_tags(&json!({
            "tags": ["original", "solo"],
            "categories": ["Manga"],
            "ratings": ["Adult"]
        }));

        assert_eq!(
            tags,
            vec![
                (String::new(), "original".to_string()),
                (String::new(), "solo".to_string()),
                (String::new(), "Manga".to_string()),
                (String::new(), "Adult".to_string())
            ]
        );
    }

    #[test]
    fn prefers_artist_over_user_for_creator_identity() {
        assert_eq!(
            ADAPTER.extract_creator_identifier(&json!({
                "artist": "artist-name",
                "user": "fallback-name"
            })),
            Some("artist-name".to_string())
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&json!({"user": "fallback-name"})),
            Some("fallback-name".to_string())
        );
    }
}
