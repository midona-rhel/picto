use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: TumblrAdapter = TumblrAdapter;

pub(super) struct TumblrAdapter;

impl SiteAdapter for TumblrAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(values) = json.get("tags") {
            append_tag_values(&mut tags, "", values);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        json.get("blog")
            .and_then(|blog| blog.get("name"))
            .or_else(|| json.get("blog_name"))
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
    fn parses_tumblr_tags_and_blog_creator() {
        let metadata = json!({
            "tags": ["space", "science"],
            "blog": {"name": "nasa"}
        });
        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                (String::new(), "space".to_string()),
                (String::new(), "science".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("nasa".to_string())
        );
    }
}
