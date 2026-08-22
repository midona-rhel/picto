use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: PatreonAdapter = PatreonAdapter;

pub(super) struct PatreonAdapter;

impl SiteAdapter for PatreonAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(values) = json.get("tags") {
            append_tag_values(&mut tags, "", values);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let creator = json.get("creator")?;
        creator
            .get("url")
            .and_then(Value::as_str)
            .and_then(extract_creator_slug_from_url)
            .or_else(|| {
                creator
                    .get("full_name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
    }
}

fn extract_creator_slug_from_url(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(url.host_str(), Some("patreon.com" | "www.patreon.com"))
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }

    let segments: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .collect();
    match segments.as_slice() {
        ["c", creator] | [creator] | [creator, "posts"] => Some((*creator).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_creator_slug_and_tags_from_patreon_metadata() {
        let metadata = json!({
            "tags": ["behind-the-scenes", "wip"],
            "creator": {
                "url": "https://www.patreon.com/c/creator-name",
                "full_name": "Creator Name"
            }
        });

        assert_eq!(
            ADAPTER.parse_tags(&metadata),
            vec![
                (String::new(), "behind-the-scenes".to_string()),
                (String::new(), "wip".to_string())
            ]
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("creator-name".to_string())
        );
    }

    #[test]
    fn falls_back_to_full_name_when_creator_url_is_missing() {
        let metadata = json!({
            "creator": {
                "full_name": "Creator Name"
            }
        });

        assert_eq!(
            ADAPTER.extract_creator_identifier(&metadata),
            Some("Creator Name".to_string())
        );
    }
}
