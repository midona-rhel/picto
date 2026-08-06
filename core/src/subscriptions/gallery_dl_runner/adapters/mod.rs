mod danbooru;
mod e621;
mod fallback;
mod pixiv;
mod twitter;

use super::sites::canonical_site_id;
use serde_json::Value;

pub(super) trait SiteAdapter: Sync {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)>;
    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let _ = json;
        None
    }
    fn collect_source_urls(&self, json: &Value) -> Vec<String> {
        let mut urls = Vec::new();
        push_unique_url(&mut urls, json.get("file_url").and_then(|v| v.as_str()));
        push_unique_url(&mut urls, json.get("url").and_then(|v| v.as_str()));
        push_unique_url(&mut urls, json.get("source").and_then(|v| v.as_str()));
        urls
    }
}

pub(super) fn push_unique_url(urls: &mut Vec<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return;
    };
    if !urls.iter().any(|existing| existing == value) {
        urls.push(value.to_string());
    }
}

/// Select the adapter for a gallery-dl sidecar JSON by its `category` field.
pub(super) fn adapter_for_json(json: &Value) -> &'static dyn SiteAdapter {
    let cat = json
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(canonical_site_id)
        .unwrap_or("");

    match cat {
        "danbooru" | "gelbooru" | "rule34" | "sankaku" | "idolcomplex" | "safebooru"
        | "yandere" | "konachan" => &danbooru::ADAPTER,
        "e621" | "e926" => &e621::ADAPTER,
        "twitter" => &twitter::ADAPTER,
        "pixiv" | "pixivuser" => &pixiv::ADAPTER,
        _ => &fallback::ADAPTER,
    }
}
