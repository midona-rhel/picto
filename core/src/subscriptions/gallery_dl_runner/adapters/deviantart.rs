use super::SiteAdapter;
use serde_json::Value;

pub(super) static ADAPTER: DeviantArtAdapter = DeviantArtAdapter;

pub(super) struct DeviantArtAdapter;

impl SiteAdapter for DeviantArtAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(value) = json.get("tags") {
            append_tags(&mut tags, value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("author")
            .and_then(|author| author.get("username"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|username| !username.is_empty())
            .map(ToOwned::to_owned)
    }
}

fn append_tags(tags: &mut Vec<(String, String)>, value: &Value) {
    if let Some(tag) = value.as_str().map(str::trim).filter(|tag| !tag.is_empty()) {
        tags.push((String::new(), tag.to_string()));
        return;
    }

    if let Some(tag) = value
        .get("tag_name")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
    {
        tags.push((String::new(), tag.to_string()));
        return;
    }

    if let Some(values) = value.as_array() {
        for value in values {
            append_tags(tags, value);
        }
    } else if let Some(values) = value.as_object() {
        for value in values.values() {
            append_tags(tags, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_string_and_object_tags_and_author_username() {
        let metadata = json!({
            "tags": [
                "  landscape  ",
                {"tag_name": "concept-art"},
                {"name": "featured"}
            ],
            "author": {"username": "ArtistName"}
        });

        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                (String::new(), "landscape".to_string()),
                (String::new(), "concept-art".to_string()),
                (String::new(), "featured".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("ArtistName".to_string())
        );
    }
}
