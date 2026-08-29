mod artstation;
mod baraag;
mod danbooru;
mod deviantart;
mod e621;
mod exhentai;
mod fanbox;
mod furaffinity;
mod hentaifoundry;
mod newgrounds;
mod patreon;
mod pixiv;
mod sankaku;
mod subscribestar;
mod tumblr;
mod twitter;

use serde_json::Value;

pub(super) trait SiteAdapter: Sync {
    fn parse_tags(&self, json: &Value) -> Vec<(String, String)>;
    fn extract_creator_identifier(&self, json: &Value) -> Option<String> {
        let _ = json;
        None
    }
}

pub(super) fn append_tag_values(tags: &mut Vec<(String, String)>, namespace: &str, value: &Value) {
    if let Some(raw) = value.as_str() {
        tags.extend(
            raw.split_whitespace()
                .map(|tag| (namespace.to_string(), tag.to_string())),
        );
        return;
    }
    let Some(values) = value.as_array() else {
        return;
    };
    for value in values {
        let tag = value
            .as_str()
            .or_else(|| value.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|tag| !tag.is_empty());
        if let Some(tag) = tag {
            tags.push((namespace.to_string(), tag.to_string()));
        }
    }
}

/// Select the adapter for raw gallery-dl metadata by its `category` field.
pub(super) fn adapter_for_json(json: &Value) -> Option<&'static dyn SiteAdapter> {
    let cat = json
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(super::metadata::canonical_metadata_category)
        .unwrap_or("");

    match cat {
        "artstation" => Some(&artstation::ADAPTER),
        "baraag" => Some(&baraag::ADAPTER),
        "danbooru" | "gelbooru" | "rule34" | "yandere" | "konachan" | "safebooru" => {
            Some(&danbooru::ADAPTER)
        }
        "deviantart" => Some(&deviantart::ADAPTER),
        "e621" => Some(&e621::ADAPTER),
        "exhentai" => Some(&exhentai::ADAPTER),
        "fanbox" => Some(&fanbox::ADAPTER),
        "furaffinity" => Some(&furaffinity::ADAPTER),
        "hentaifoundry" => Some(&hentaifoundry::ADAPTER),
        "newgrounds" => Some(&newgrounds::ADAPTER),
        "idolcomplex" | "sankaku" => Some(&sankaku::ADAPTER),
        "patreon" => Some(&patreon::ADAPTER),
        "pixiv" => Some(&pixiv::ADAPTER),
        "subscribestar" => Some(&subscribestar::ADAPTER),
        "tumblr" => Some(&tumblr::ADAPTER),
        "twitter" => Some(&twitter::ADAPTER),
        _ => None,
    }
}
