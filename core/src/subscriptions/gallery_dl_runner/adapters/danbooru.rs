use super::{append_tag_values, SiteAdapter};
use serde_json::Value;

pub(super) static ADAPTER: DanbooruAdapter = DanbooruAdapter;

const CATEGORIZED_FIELDS: &[(&str, &str)] = &[
    ("tags_artist", "creator"),
    ("tags_character", "character"),
    ("tags_copyright", "series"),
    ("tags_general", ""),
    ("tags_meta", "meta"),
    ("tags_metadata", "meta"),
    ("tag_string_artist", "creator"),
    ("tag_string_character", "character"),
    ("tag_string_copyright", "series"),
    ("tag_string_general", ""),
    ("tag_string_meta", "meta"),
    ("tag_string_metadata", "meta"),
];

pub(super) struct DanbooruAdapter;

impl SiteAdapter for DanbooruAdapter {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();

        for (key, namespace) in CATEGORIZED_FIELDS {
            if let Some(val) = json.get(*key) {
                append_tag_values(&mut tags, namespace, val);
            }
        }
        if !tags.is_empty() {
            return tags;
        }

        for field in ["tag_string", "tags"] {
            if let Some(value) = json.get(field) {
                append_tag_values(&mut tags, "", value);
            }
        }
        tags
    }
}
