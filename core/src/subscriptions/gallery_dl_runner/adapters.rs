use serde_json::Value;

use super::sites::canonical_site_id;

pub(super) trait SiteAdapter: Sync {
    fn matches(&self, json: &Value) -> bool;
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)>;
    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let _ = json;
        None
    }
}

struct DanbooruAdapter;
struct E621Adapter;
struct PixivAdapter;
struct FallbackAdapter;

static DANBOORU_ADAPTER: DanbooruAdapter = DanbooruAdapter;
static E621_ADAPTER: E621Adapter = E621Adapter;
static PIXIV_ADAPTER: PixivAdapter = PixivAdapter;
static FALLBACK_ADAPTER: FallbackAdapter = FallbackAdapter;
static ADAPTERS: [&dyn SiteAdapter; 4] = [
    &DANBOORU_ADAPTER,
    &E621_ADAPTER,
    &PIXIV_ADAPTER,
    &FALLBACK_ADAPTER,
];

pub(super) fn adapter_for_json(json: &Value) -> &'static dyn SiteAdapter {
    ADAPTERS
        .into_iter()
        .find(|adapter| adapter.matches(json))
        .unwrap_or(&FALLBACK_ADAPTER)
}

impl SiteAdapter for DanbooruAdapter {
    fn matches(&self, json: &Value) -> bool {
        matches!(category(json).as_deref(), Some("danbooru" | "gelbooru" | "rule34" | "3dbooru" | "sankaku" | "idolcomplex"))
            || DANBOORU_CATEGORIES.iter().any(|(key, _)| json.get(*key).is_some())
            || DANBOORU_TAG_STRINGS.iter().any(|(key, _)| json.get(*key).is_some())
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();

        for (key, namespace) in DANBOORU_CATEGORIES {
            if let Some(arr) = json.get(*key).and_then(|v| v.as_array()) {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if !tags.is_empty() {
            return tags;
        }

        for (key, namespace) in DANBOORU_TAG_STRINGS {
            if let Some(tag_string) = json.get(*key).and_then(|v| v.as_str()) {
                for tag in tag_string.split_whitespace() {
                    if !tag.is_empty() {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if !tags.is_empty() {
            return tags;
        }

        if let Some(tag_str) = json.get("tag_string").and_then(|v| v.as_str()) {
            for tag in tag_str.split_whitespace() {
                if !tag.is_empty() {
                    tags.push((String::new(), tag.to_string()));
                }
            }
        }

        tags
    }
}

impl SiteAdapter for E621Adapter {
    fn matches(&self, json: &Value) -> bool {
        if matches!(category(json).as_deref(), Some("e621" | "e926")) {
            return true;
        }
        let Some(obj) = json.get("tags").and_then(|v| v.as_object()) else {
            return false;
        };
        obj.values().any(|v| v.is_array())
            && obj.contains_key("general")
            && (obj.contains_key("artist") || obj.contains_key("character"))
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        let Some(obj) = json.get("tags").and_then(|v| v.as_object()) else {
            return tags;
        };

        for (category, namespace) in E621_NAMESPACE_MAP {
            if let Some(arr) = obj.get(*category).and_then(|v| v.as_array()) {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }

        for (category, value) in obj {
            let mapped = E621_NAMESPACE_MAP
                .iter()
                .any(|(cat, _)| *cat == category.as_str());
            if mapped {
                continue;
            }
            if let Some(arr) = value.as_array() {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((category.clone(), tag.to_string()));
                    }
                }
            }
        }

        tags
    }
}

impl SiteAdapter for PixivAdapter {
    fn matches(&self, json: &Value) -> bool {
        if matches!(category(json).as_deref(), Some("pixiv" | "pixivuser")) {
            return true;
        }
        json.get("tags")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .and_then(|obj| obj.get("name"))
            .is_some()
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        json.get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tag_obj| {
                        tag_obj
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|name| !name.is_empty())
                            .map(|name| (String::new(), name.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let user = json.get("user")?;
        if let Some(name) = user
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return Some(name.to_string());
        }
        if let Some(id) = user.get("id") {
            if let Some(n) = id.as_i64() {
                return Some(n.to_string());
            }
            if let Some(s) = id.as_str().map(str::trim).filter(|v| !v.is_empty()) {
                return Some(s.to_string());
            }
        }
        None
    }
}

impl SiteAdapter for FallbackAdapter {
    fn matches(&self, _json: &Value) -> bool {
        true
    }

    fn parse_tags(&self, json: &Value) -> Vec<(String, String)> {
        let mut tags = Vec::new();
        let Some(tags_val) = json.get("tags") else {
            return tags;
        };

        if let Some(arr) = tags_val.as_array() {
            for tag_val in arr {
                if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
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

const DANBOORU_CATEGORIES: &[(&str, &str)] = &[
    ("tags_artist", "creator"),
    ("tags_character", "character"),
    ("tags_copyright", "series"),
    ("tags_general", ""),
    ("tags_meta", "meta"),
];

const DANBOORU_TAG_STRINGS: &[(&str, &str)] = &[
    ("tag_string_artist", "creator"),
    ("tag_string_character", "character"),
    ("tag_string_copyright", "series"),
    ("tag_string_general", ""),
    ("tag_string_meta", "meta"),
];

const E621_NAMESPACE_MAP: &[(&str, &str)] = &[
    ("artist", "creator"),
    ("character", "character"),
    ("copyright", "series"),
    ("general", ""),
    ("meta", "meta"),
    ("species", "species"),
    ("lore", "lore"),
];

fn category(json: &Value) -> Option<String> {
    json.get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(canonical_site_id)
        .map(str::to_string)
}
