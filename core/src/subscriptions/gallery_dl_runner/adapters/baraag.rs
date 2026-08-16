use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: BaraagAdapter = BaraagAdapter;

pub(super) struct BaraagAdapter;

impl SiteAdapter for BaraagAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        if let Some(value) = json.get("tags") {
            append_tag_values(&mut tags, "", value);
        }
        tags
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let account = json.get("account")?;
        ["username", "acct"].into_iter().find_map(|field| {
            account
                .get(field)
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

    fn baraag_metadata() -> Value {
        json!({
            "category": "baraag",
            "subcategory": "user",
            "id": "117023587543794517",
            "account": {
                "username": "Blue_",
                "acct": "Blue_@baraag.net"
            },
            "tags": ["lolicon", "doodlegirl"],
            "count": 1,
            "num": 1,
            "media": {
                "type": "image",
                "url": "https://baraag.net/system/media_attachments/files/000/000/001/original/image.jpg"
            },
            "url": "https://baraag.net/@Blue_/117023587543794517",
            "uri": "https://baraag.net/users/Blue_/statuses/117023587543794517",
            "content": "<p>A <strong>readable</strong> post.</p>"
        })
    }

    #[test]
    fn parses_baraag_tags_in_the_general_namespace() {
        assert_eq!(
            ADAPTER.parse_tags(&baraag_metadata()),
            vec![
                (String::new(), "lolicon".to_string()),
                (String::new(), "doodlegirl".to_string())
            ]
        );
    }

    #[test]
    fn prefers_account_username_then_falls_back_to_acct() {
        assert_eq!(
            ADAPTER.extract_creator_identifier(&baraag_metadata()),
            Some("Blue_".to_string())
        );
        assert_eq!(
            ADAPTER.extract_creator_identifier(&json!({
                "account": {"username": "  ", "acct": "Blue_@baraag.net"}
            })),
            Some("Blue_@baraag.net".to_string())
        );
    }
}
