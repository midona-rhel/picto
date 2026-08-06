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

use super::gallery_dl_runner::canonical_site_id;

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
    GelbooruV02,
    Moebooru,
}

/// Which sites support autocomplete, and how. Keyed by canonical site id.
/// Sankaku/idolcomplex are omitted (suggest API requires auth tokens), as is
/// e621 (no public tag autocomplete endpoint).
fn autocomplete_style(site_id: &str) -> Option<(AutocompleteStyle, &'static str)> {
    match canonical_site_id(site_id) {
        "danbooru" => Some((AutocompleteStyle::Danbooru, "https://danbooru.donmai.us")),
        "gelbooru" => Some((AutocompleteStyle::Gelbooru, "https://gelbooru.com")),
        "rule34" => Some((AutocompleteStyle::GelbooruV02, "https://rule34.xxx")),
        "safebooru" => Some((AutocompleteStyle::GelbooruV02, "https://safebooru.org")),
        "yandere" => Some((AutocompleteStyle::Moebooru, "https://yande.re")),
        "konachan" => Some((AutocompleteStyle::Moebooru, "https://konachan.com")),
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

    let cache_key = (canonical_site_id(site_id).to_string(), prefix.clone());
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
        AutocompleteStyle::GelbooruV02 => format!("{base}/autocomplete.php?q={encoded}"),
        AutocompleteStyle::Moebooru => {
            format!("{base}/tag.json?name={encoded}*&limit={limit}&order=count")
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
            AutocompleteStyle::GelbooruV02 => {
                let name = item.get("value").and_then(|v| v.as_str())?;
                // Count is embedded in the label: "tag_name (12345)"
                let count = item
                    .get("label")
                    .and_then(|v| v.as_str())
                    .and_then(|label| label.rsplit_once('('))
                    .and_then(|(_, tail)| tail.trim_end_matches(')').parse().ok());
                Some(TagSuggestion {
                    name: name.to_string(),
                    post_count: count,
                    category: item
                        .get("type")
                        .and_then(|v| v.as_str())
                        .and_then(gelbooru_category)
                        .map(str::to_string),
                })
            }
            AutocompleteStyle::Moebooru => {
                let name = item.get("name").and_then(|v| v.as_str())?;
                Some(TagSuggestion {
                    name: name.to_string(),
                    post_count: item.get("count").and_then(|v| v.as_u64()),
                    category: item
                        .get("type")
                        .and_then(|v| v.as_i64())
                        .and_then(danbooru_category)
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
    fn parses_gelbooru_v02_autocomplete_count_from_label() {
        let body = json!([
            { "label": "samus_aran (50000)", "value": "samus_aran", "type": "character" }
        ]);
        let result = parse_suggestions(AutocompleteStyle::GelbooruV02, &body);
        assert_eq!(result[0].name, "samus_aran");
        assert_eq!(result[0].post_count, Some(50_000));
        assert_eq!(result[0].category.as_deref(), Some("character"));
    }

    #[test]
    fn parses_moebooru_tag_json() {
        let body = json!([
            { "id": 1, "name": "landscape", "count": 30000, "type": 0 },
            { "id": 2, "name": "wlop", "count": 900, "type": 1 }
        ]);
        let result = parse_suggestions(AutocompleteStyle::Moebooru, &body);
        assert_eq!(result[0].category.as_deref(), Some("general"));
        assert_eq!(result[1].category.as_deref(), Some("creator"));
        assert_eq!(result[1].post_count, Some(900));
    }

    #[test]
    fn malformed_body_yields_empty() {
        assert!(parse_suggestions(AutocompleteStyle::Danbooru, &json!({"error": true})).is_empty());
        assert!(parse_suggestions(AutocompleteStyle::Moebooru, &json!("nope")).is_empty());
    }

    #[test]
    fn unsupported_sites_have_no_style() {
        assert!(autocomplete_style("pixiv").is_none());
        assert!(autocomplete_style("sankaku").is_none());
        assert!(autocomplete_style("twitter").is_none());
    }

    #[test]
    fn alias_site_ids_resolve_via_canonicalization() {
        assert!(autocomplete_style("rule34.xxx").is_some());
        assert!(autocomplete_style("yande.re").is_some());
    }

    #[test]
    fn e621_has_no_autocomplete() {
        assert!(autocomplete_style("e621").is_none());
        assert!(autocomplete_style("e621.net").is_none());
    }
}
