use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: SankakuAdapter = SankakuAdapter;

const CATEGORIZED_FIELDS: &[(&str, &str)] = &[
    ("tags_artist", "artist"),
    ("tags_character", "character"),
    ("tags_copyright", "copyright"),
    ("tags_general", ""),
    ("tags_genre", "genre"),
    ("tags_medium", "medium"),
    ("tags_meta", "meta"),
    ("tags_studio", "studio"),
    ("tag_string_artist", "artist"),
    ("tag_string_character", "character"),
    ("tag_string_copyright", "copyright"),
    ("tag_string_general", ""),
    ("tag_string_genre", "genre"),
    ("tag_string_medium", "medium"),
    ("tag_string_meta", "meta"),
    ("tag_string_studio", "studio"),
];

pub(super) struct SankakuAdapter;

impl SiteAdapter for SankakuAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        let mut found_categorized = false;

        for (field, namespace) in CATEGORIZED_FIELDS {
            if let Some(value) = json.get(*field) {
                found_categorized = true;
                append_tag_values(&mut tags, namespace, value);
            }
        }

        if !found_categorized {
            for field in ["tags", "tag_string"] {
                if let Some(value) = json.get(field) {
                    append_tag_values(&mut tags, "", value);
                }
            }
        }

        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        for field in ["tags_artist", "tag_string_artist"] {
            let mut values = Vec::new();
            if let Some(value) = json.get(field) {
                append_tag_values(&mut values, "", value);
            }
            if let Some((_, creator)) = values.into_iter().next() {
                return Some(creator);
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
    fn parses_categorized_idol_complex_metadata() {
        let metadata = json!({
            "category": "idolcomplex",
            "id": "60rvNVpQr3A",
            "rating": "e",
            "tags_artist": ["babykatie666"],
            "tags_character": ["character_name"],
            "tags_copyright": ["series_name"],
            "tags_general": ["solo"],
            "tags_genre": ["fantasy"],
            "tags_medium": ["landscape"],
            "tags_meta": ["highres"],
            "tags_studio": ["studio_name"]
        });

        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                ("artist".to_string(), "babykatie666".to_string()),
                ("character".to_string(), "character_name".to_string()),
                ("copyright".to_string(), "series_name".to_string()),
                (String::new(), "solo".to_string()),
                ("genre".to_string(), "fantasy".to_string()),
                ("medium".to_string(), "landscape".to_string()),
                ("meta".to_string(), "highres".to_string()),
                ("studio".to_string(), "studio_name".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("babykatie666".to_string())
        );
    }

    #[test]
    fn supports_legacy_fields_and_flat_fallback() {
        assert_eq!(
            ADAPTER.parse_tags(&json!({
                "tag_string_artist": "artist_one artist_two",
                "tag_string_medium": "landscape"
            })),
            vec![
                ("artist".to_string(), "artist_one".to_string()),
                ("artist".to_string(), "artist_two".to_string()),
                ("medium".to_string(), "landscape".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.parse_tags(&json!({"tags": ["solo", "landscape"]})),
            vec![
                (String::new(), "solo".to_string()),
                (String::new(), "landscape".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&json!({
                "tag_string_artist": "artist_one artist_two"
            })),
            Some("artist_one".to_string())
        );
    }
}
