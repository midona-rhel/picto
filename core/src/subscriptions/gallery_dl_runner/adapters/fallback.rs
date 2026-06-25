use super::SiteAdapter;
use serde_json::Value;

pub(super) static ADAPTER: FallbackAdapter = FallbackAdapter;

pub(super) struct FallbackAdapter;

/// Pull a usable name out of a creator-ish JSON value: either a plain string
/// or an object carrying one of the conventional name fields.
fn creator_name(value: &Value) -> Option<String> {
    if let Some(name) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }
    if value.is_object() {
        for key in ["username", "name", "nick"] {
            if let Some(name) = value
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

impl SiteAdapter for FallbackAdapter {
    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        // String-or-object fields, covering ArtStation/DeviantArt/Patreon/
        // fanbox shapes (user: {username}, author: {username}, creator: {...})
        // and nijie's flat user_name.
        for key in [
            "artist", "username", "user", "uploader", "blog_name", "user_name", "author",
            "creator",
        ] {
            if let Some(name) = json.get(key).and_then(creator_name) {
                return Some(name);
            }
        }
        None
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        let Some(tags_val) = json.get("tags") else {
            return tags;
        };

        if let Some(arr) = tags_val.as_array() {
            for tag_val in arr {
                // Plain string entries, or object entries with a name/tag
                // field (Mastodon sites and ArtStation emit object arrays).
                let tag = tag_val.as_str().or_else(|| {
                    tag_val
                        .get("name")
                        .or_else(|| tag_val.get("tag"))
                        .and_then(|v| v.as_str())
                });
                if let Some(tag) = tag.map(str::trim).filter(|s| !s.is_empty()) {
                    tags.push((String::new(), tag.to_string()));
                }
            }
            return tags;
        }

        if let Some(tag_str) = tags_val.as_str() {
            for tag in tag_str.split_whitespace() {
                if !tag.is_empty() {
                    tags.push((String::new(), tag.to_string()));
                }
            }
        }

        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn adapter() -> &'static FallbackAdapter {
        &ADAPTER
    }

    #[test]
    fn tags_from_string_array() {
        let meta = json!({ "tags": ["landscape", "  sunset ", ""] });
        let tags = adapter().parse_tags(&meta);
        assert_eq!(
            tags,
            vec![
                (String::new(), "landscape".to_string()),
                (String::new(), "sunset".to_string()),
            ]
        );
    }

    #[test]
    fn tags_from_object_array_mastodon_shape() {
        // baraag/pawoo (Mastodon) emit [{name, url}] tag objects
        let meta = json!({ "tags": [
            { "name": "art", "url": "https://baraag.net/tags/art" },
            { "name": "oc" },
            { "url": "https://baraag.net/tags/skipped" }
        ]});
        let tags = adapter().parse_tags(&meta);
        assert_eq!(
            tags,
            vec![
                (String::new(), "art".to_string()),
                (String::new(), "oc".to_string()),
            ]
        );
    }

    #[test]
    fn tags_from_object_array_tag_key() {
        let meta = json!({ "tags": [{ "tag": "fantasy" }] });
        let tags = adapter().parse_tags(&meta);
        assert_eq!(tags, vec![(String::new(), "fantasy".to_string())]);
    }

    #[test]
    fn tags_from_whitespace_string() {
        let meta = json!({ "tags": "red blue  green" });
        let tags = adapter().parse_tags(&meta);
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn no_tags_field_yields_empty() {
        let meta = json!({ "title": "untagged" });
        assert!(adapter().parse_tags(&meta).is_empty());
    }

    #[test]
    fn creator_from_flat_string() {
        let meta = json!({ "artist": "painter" });
        assert_eq!(
            adapter().extract_creator_identifier(&meta),
            Some("painter".to_string())
        );
    }

    #[test]
    fn creator_from_artstation_user_object() {
        let meta = json!({ "user": { "username": "concept_dude", "full_name": "Dude" } });
        assert_eq!(
            adapter().extract_creator_identifier(&meta),
            Some("concept_dude".to_string())
        );
    }

    #[test]
    fn creator_from_deviantart_author_object() {
        let meta = json!({ "author": { "username": "deviant_one" } });
        assert_eq!(
            adapter().extract_creator_identifier(&meta),
            Some("deviant_one".to_string())
        );
    }

    #[test]
    fn creator_from_nijie_user_name() {
        let meta = json!({ "user_name": "nijie_artist" });
        assert_eq!(
            adapter().extract_creator_identifier(&meta),
            Some("nijie_artist".to_string())
        );
    }

    #[test]
    fn creator_from_fanbox_user_name_field() {
        let meta = json!({ "user": { "name": "fanbox_creator" } });
        assert_eq!(
            adapter().extract_creator_identifier(&meta),
            Some("fanbox_creator".to_string())
        );
    }

    #[test]
    fn creator_priority_prefers_artist() {
        let meta = json!({ "artist": "primary", "author": { "username": "secondary" } });
        assert_eq!(
            adapter().extract_creator_identifier(&meta),
            Some("primary".to_string())
        );
    }

    #[test]
    fn creator_absent_yields_none() {
        let meta = json!({ "id": 5 });
        assert_eq!(adapter().extract_creator_identifier(&meta), None);
    }
}
