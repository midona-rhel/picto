//! Booru tag autocomplete — public suggestion endpoints per site family.
//!
//! Every failure path returns an empty list: suggestions are a typing aid,
//! never an error surface. Results are cached per (site, prefix) for the
//! session — tag corpora barely change.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tracing::warn;
use ts_rs::TS;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 25;
const CACHE_CAP: usize = 512;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct TagSuggestion {
    pub name: String,
    pub post_count: Option<u64>,
    /// Picto namespace ("general", "creator", "character", "series", "meta",
    /// "species", "lore") when the site reports a category.
    pub category: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum AutocompleteStyle {
    Danbooru,
    Gelbooru,
}

/// Which supported sites expose public autocomplete, keyed by canonical site id.
fn autocomplete_style(site_id: &str) -> Option<(AutocompleteStyle, &'static str)> {
    match site_id {
        "danbooru" => Some((AutocompleteStyle::Danbooru, "https://danbooru.donmai.us")),
        "gelbooru" => Some((AutocompleteStyle::Gelbooru, "https://gelbooru.com")),
        _ => None,
    }
}

/// Danbooru-family numeric tag categories → picto namespaces.
fn danbooru_category(value: i64) -> Option<&'static str> {
    match value {
        0 => Some("general"),
        1 => Some("creator"),
        3 => Some("series"),
        4 => Some("character"),
        5 => Some("meta"),
        _ => None,
    }
}

/// Gelbooru reports category names as strings.
fn gelbooru_category(value: &str) -> Option<&'static str> {
    match value {
        "tag" | "general" => Some("general"),
        "artist" => Some("creator"),
        "copyright" => Some("series"),
        "character" => Some("character"),
        "metadata" | "meta" => Some("meta"),
        _ => None,
    }
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // Some boorus reject requests without a descriptive UA.
            .user_agent(concat!("Picto/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("autocomplete http client")
    })
}

fn cache() -> &'static Mutex<HashMap<(String, String), Vec<TagSuggestion>>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Vec<TagSuggestion>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Fetch tag suggestions for a prefix. Empty on any failure or for sites
/// without a public autocomplete endpoint.
pub async fn suggest_tags(site_id: &str, prefix: &str, limit: Option<u32>) -> Vec<TagSuggestion> {
    let prefix = prefix.trim().to_lowercase();
    if prefix.is_empty() {
        return Vec::new();
    }
    let Some((style, base_url)) = autocomplete_style(site_id) else {
        return Vec::new();
    };
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let cache_key = (site_id.to_string(), prefix.clone());
    if let Ok(map) = cache().lock() {
        if let Some(hit) = map.get(&cache_key) {
            return hit.clone();
        }
    }

    let url = build_request_url(style, base_url, &prefix, limit);
    let body = match client().get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(text) => text,
            Err(error) => {
                warn!(site = site_id, %error, "tag autocomplete body read failed");
                return Vec::new();
            }
        },
        Ok(resp) => {
            warn!(site = site_id, status = %resp.status(), "tag autocomplete request rejected");
            return Vec::new();
        }
        Err(error) => {
            warn!(site = site_id, %error, "tag autocomplete request failed");
            return Vec::new();
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(json) => json,
        Err(error) => {
            warn!(site = site_id, %error, "tag autocomplete parse failed");
            return Vec::new();
        }
    };

    let mut suggestions = parse_suggestions(style, &json);
    suggestions.truncate(limit as usize);

    if let Ok(mut map) = cache().lock() {
        if map.len() >= CACHE_CAP {
            map.clear();
        }
        map.insert(cache_key, suggestions.clone());
    }
    suggestions
}

fn build_request_url(style: AutocompleteStyle, base: &str, prefix: &str, limit: u32) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(prefix.as_bytes()).collect();
    match style {
        AutocompleteStyle::Danbooru => format!(
            "{base}/autocomplete.json?search[query]={encoded}&search[type]=tag_query&limit={limit}"
        ),
        AutocompleteStyle::Gelbooru => {
            format!(
                "{base}/index.php?page=autocomplete2&term={encoded}&type=tag_query&limit={limit}"
            )
        }
    }
}

fn parse_suggestions(style: AutocompleteStyle, json: &serde_json::Value) -> Vec<TagSuggestion> {
    let Some(items) = json.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match style {
            AutocompleteStyle::Danbooru => {
                let name = item.get("value").and_then(|v| v.as_str())?;
                Some(TagSuggestion {
                    name: name.to_string(),
                    post_count: item.get("post_count").and_then(|v| v.as_u64()),
                    category: item
                        .get("category")
                        .and_then(|v| v.as_i64())
                        .and_then(danbooru_category)
                        .map(str::to_string),
                })
            }
            AutocompleteStyle::Gelbooru => {
                let name = item.get("value").and_then(|v| v.as_str())?;
                let count = item
                    .get("post_count")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
                Some(TagSuggestion {
                    name: name.to_string(),
                    post_count: count,
                    category: item
                        .get("category")
                        .and_then(|v| v.as_str())
                        .and_then(gelbooru_category)
                        .map(str::to_string),
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_danbooru_autocomplete() {
        let body = json!([
            { "type": "tag-word", "label": "blue eyes", "value": "blue_eyes", "category": 0, "post_count": 1500000 },
            { "type": "tag-word", "label": "blue sky", "value": "blue_sky", "category": 4, "post_count": 40000 }
        ]);
        let result = parse_suggestions(AutocompleteStyle::Danbooru, &body);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "blue_eyes");
        assert_eq!(result[0].post_count, Some(1_500_000));
        assert_eq!(result[0].category.as_deref(), Some("general"));
        assert_eq!(result[1].category.as_deref(), Some("character"));
    }

    #[test]
    fn parses_gelbooru_autocomplete() {
        let body = json!([
            { "type": "tag", "label": "1girl (6000000)", "value": "1girl", "post_count": "6000000", "category": "tag" }
        ]);
        let result = parse_suggestions(AutocompleteStyle::Gelbooru, &body);
        assert_eq!(result[0].name, "1girl");
        assert_eq!(result[0].post_count, Some(6_000_000));
        assert_eq!(result[0].category.as_deref(), Some("general"));
    }

    #[test]
    fn malformed_body_yields_empty() {
        assert!(parse_suggestions(AutocompleteStyle::Danbooru, &json!({"error": true})).is_empty());
        assert!(parse_suggestions(AutocompleteStyle::Gelbooru, &json!("nope")).is_empty());
    }

    #[test]
    fn non_booru_supported_sources_have_no_style() {
        assert!(autocomplete_style("pixiv").is_none());
        assert!(autocomplete_style("pixivuser").is_none());
    }
}
