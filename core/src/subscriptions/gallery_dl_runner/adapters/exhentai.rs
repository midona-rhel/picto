use serde_json::Value;

use super::{append_tag_values, SiteAdapter};

pub(super) static ADAPTER: ExhentaiAdapter = ExhentaiAdapter;

pub(super) struct ExhentaiAdapter;

impl SiteAdapter for ExhentaiAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        append_tag_values(&mut tags, "creator", &json["tags_artist"]);
        append_tag_values(&mut tags, "series", &json["tags_parody"]);
        tags.retain(|(namespace, tag)| {
            namespace != "series" || !tag.trim().eq_ignore_ascii_case("original")
        });
        append_tag_values(&mut tags, "character", &json["tags_character"]);
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("tags_artist")?
            .as_array()?
            .iter()
            .find_map(Value::as_str)
            .map(str::trim)
            .filter(|artist| !artist.is_empty())
            .map(ToOwned::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_only_picto_gallery_namespaces() {
        let metadata = json!({
            "tags_artist": ["leonardo"],
            "tags_parody": ["renaissance", "original", " Original "],
            "tags_character": ["mona lisa"],
            "tags_language": ["english"],
            "tags_female": ["female tag"],
            "tags_male": ["male tag"],
            "tags_other": ["other tag"]
        });
        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                ("creator".to_string(), "leonardo".to_string()),
                ("series".to_string(), "renaissance".to_string()),
                ("character".to_string(), "mona lisa".to_string()),
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata).as_deref(),
            Some("leonardo")
        );
    }
}
