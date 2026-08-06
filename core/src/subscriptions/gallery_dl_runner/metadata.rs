use super::adapters::adapter_for_json;
use crate::subscriptions::source_adapter::ParsedMetadata;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

pub(super) fn canonical_metadata_category(category: &str) -> &str {
    match category {
        "pixivuser" => "pixiv",
        "x" => "twitter",
        "e926" => "e621",
        "gelbooru_v02" => "gelbooru",
        _ => category,
    }
}

fn value_text(value: &serde_json::Value) -> Option<String> {
    if let Some(value) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_i64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_u64() {
        return Some(value.to_string());
    }
    value.as_f64().map(|value| value.to_string())
}

fn field_text(json: &serde_json::Value, key: &str) -> Option<String> {
    json.get(key).and_then(value_text)
}

fn push_unique_url(urls: &mut Vec<String>, value: Option<String>) {
    let Some(value) = value else { return };
    if !urls.iter().any(|existing| existing == &value) {
        urls.push(value);
    }
}

fn normalize_created_at(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(parsed.with_timezone(&Utc).to_rfc3339());
    }

    for fmt in ["%Y-%m-%d %H:%M:%S%:z", "%Y-%m-%d %H:%M:%S%.f%:z"] {
        if let Ok(parsed) = DateTime::parse_from_str(trimmed, fmt) {
            return Some(parsed.with_timezone(&Utc).to_rfc3339());
        }
    }

    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc).to_rfc3339());
        }
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let parsed = parsed.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc).to_rfc3339());
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y%m%d") {
        let parsed = parsed.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc).to_rfc3339());
    }

    Some(trimmed.to_string())
}

fn parse_created_at(json: &serde_json::Value) -> Option<String> {
    for key in [
        "date",
        "created_at",
        "create_date",
        "published_at",
        "published",
        "upload_date",
    ] {
        let Some(value) = json.get(key) else {
            continue;
        };
        if let Some(raw) = value.as_str() {
            if let Some(normalized) = normalize_created_at(raw) {
                return Some(normalized);
            }
        }
        if let Some(timestamp) = value.as_i64().or_else(|| value.as_f64().map(|v| v as i64)) {
            if let Some(date) = DateTime::from_timestamp(timestamp, 0) {
                return Some(date.to_rfc3339());
            }
        }
    }
    None
}

fn post_id(json: &serde_json::Value) -> Option<String> {
    ["tweet_id", "id", "index"]
        .into_iter()
        .find_map(|key| field_text(json, key))
}

fn canonical_post_url(
    json: &serde_json::Value,
    category: Option<&str>,
    post_id: Option<&str>,
) -> Option<String> {
    match category {
        Some("pixiv") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://www.pixiv.net/en/artworks/{post_id}"));
            }
        }
        Some("gelbooru") => {
            if let Some(post_id) = post_id {
                return Some(format!(
                    "https://gelbooru.com/index.php?page=post&s=view&id={post_id}"
                ));
            }
        }
        Some("twitter") => {
            let handle = json.get("author").and_then(|author| {
                field_text(author, "nick").or_else(|| field_text(author, "name"))
            });
            if let (Some(handle), Some(post_id)) = (handle, post_id) {
                return Some(format!("https://x.com/{handle}/status/{post_id}"));
            }
        }
        _ => {}
    }

    ["url", "post_url", "post", "source"]
        .into_iter()
        .filter_map(|key| field_text(json, key))
        .find(|url| url.starts_with("http://") || url.starts_with("https://"))
}

fn media_url(json: &serde_json::Value, category: Option<&str>) -> Option<String> {
    if category == Some("e621") {
        if let Some(url) = json.get("file").and_then(|file| field_text(file, "url")) {
            return Some(url);
        }
    }
    field_text(json, "file_url")
        .or_else(|| field_text(json, "media_url"))
        .or_else(|| {
            (category == Some("pixiv"))
                .then(|| field_text(json, "url"))
                .flatten()
        })
}

fn source_urls(
    json: &serde_json::Value,
    category: Option<&str>,
    canonical_post_url: Option<String>,
    media_url: Option<String>,
) -> Vec<String> {
    let mut urls = Vec::new();
    push_unique_url(&mut urls, canonical_post_url);
    if category == Some("e621") {
        if let Some(sources) = json.get("sources").and_then(|value| value.as_array()) {
            for source in sources {
                push_unique_url(&mut urls, value_text(source));
            }
        }
    } else {
        push_unique_url(&mut urls, field_text(json, "source"));
    }
    push_unique_url(&mut urls, media_url);
    urls
}

/// Normalize raw gallery-dl metadata into Picto's one subscription metadata shape.
pub fn parse_metadata(json: &serde_json::Value) -> ParsedMetadata {
    parse_metadata_with_url(json, None)
}

pub(super) fn parse_metadata_with_url(
    json: &serde_json::Value,
    item_url: Option<&str>,
) -> ParsedMetadata {
    let category = field_text(json, "category")
        .map(|category| canonical_metadata_category(&category).to_string());
    let adapter = adapter_for_json(json);
    let mut tags = adapter.parse_tags(json);
    if let Some(creator) = adapter.extract_creator_identifier(json) {
        if !tags
            .iter()
            .any(|(namespace, subtag)| namespace == "creator" && subtag == &creator)
        {
            tags.push(("creator".to_string(), creator));
        }
    }

    let description = json
        .get("artist_commentary")
        .and_then(|commentary| field_text(commentary, "original_description"))
        .or_else(|| {
            ["description", "caption", "body", "content", "substring"]
                .into_iter()
                .find_map(|key| field_text(json, key))
        });
    let title = json
        .get("artist_commentary")
        .and_then(|commentary| field_text(commentary, "original_title"))
        .or_else(|| {
            ["title", "subject"]
                .into_iter()
                .find_map(|key| field_text(json, key))
        });
    let post_id = post_id(json);
    let canonical_post_url = canonical_post_url(json, category.as_deref(), post_id.as_deref());
    let media_url = media_url(json, category.as_deref());
    let source_urls = source_urls(
        json,
        category.as_deref(),
        canonical_post_url.clone(),
        media_url.clone(),
    );
    let source_url = canonical_post_url
        .clone()
        .or_else(|| source_urls.first().cloned());
    let page_num = json
        .get("num")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok());
    let page_count = json
        .get("count")
        .or_else(|| json.get("page_count"))
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok());
    let item_target = post_id
        .as_deref()
        .or(canonical_post_url.as_deref())
        .or(media_url.as_deref())
        .or(item_url.map(str::trim).filter(|url| !url.is_empty()));
    let item_key = item_target.map(|target| {
        format!(
            "{}:{target}:{}",
            category.as_deref().unwrap_or("unknown"),
            page_num.unwrap_or(0)
        )
    });

    tracing::debug!(
        category = category.as_deref().unwrap_or("?"),
        post_id = post_id.as_deref().unwrap_or("?"),
        tags = tags.len(),
        page_num = ?page_num,
        page_count = ?page_count,
        "gallery-dl metadata normalized"
    );

    ParsedMetadata {
        tags,
        description,
        source_url,
        source_urls,
        media_url,
        rating: json.get("rating").and_then(value_text),
        title,
        post_id,
        created_at: parse_created_at(json),
        category,
        page_num,
        page_count,
        canonical_post_url,
        item_key,
        raw_metadata: Some(json.clone()),
    }
}

pub fn parse_tags(json: &serde_json::Value) -> Vec<(String, String)> {
    adapter_for_json(json).parse_tags(json)
}

pub fn extract_creator_identifier(json: &serde_json::Value) -> Option<String> {
    adapter_for_json(json).extract_creator_identifier(json)
}

#[cfg(test)]
mod tests {
    use super::{parse_metadata, parse_metadata_with_url};
    use serde_json::json;

    #[test]
    fn gelbooru_metadata_uses_post_identity_namespaced_tags_and_source_order() {
        let parsed = parse_metadata(&json!({
            "category": "gelbooru_v02",
            "id": 13753751,
            "file_url": "https://img2.gelbooru.com/example.jpg",
            "source": "https://x.com/artist/status/1",
            "date": "2026-03-30T03:30:30+00:00",
            "created_at": "2026-03-29T22:30:30-05:00",
            "tags_character": "princess_peach",
            "tags_general": "dress",
            "tags_metadata": "highres"
        }));

        let post_url = "https://gelbooru.com/index.php?page=post&s=view&id=13753751";
        assert_eq!(parsed.category.as_deref(), Some("gelbooru"));
        assert_eq!(parsed.canonical_post_url.as_deref(), Some(post_url));
        assert_eq!(parsed.source_url.as_deref(), Some(post_url));
        assert_eq!(
            parsed.source_urls,
            [
                post_url,
                "https://x.com/artist/status/1",
                "https://img2.gelbooru.com/example.jpg"
            ]
        );
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-03-30T03:30:30+00:00")
        );
        assert!(parsed
            .tags
            .contains(&("meta".to_string(), "highres".to_string())));
        assert!(parsed
            .tags
            .contains(&("character".to_string(), "princess_peach".to_string())));
    }

    #[test]
    fn pixiv_metadata_prefers_artwork_page_for_user_and_search_runs() {
        let raw = json!({
            "category": "pixiv",
            "id": 114223105,
            "url": "https://i.pximg.net/img-original/example_p0.png",
            "tags": ["original"],
            "user": {"id": 1234, "name": "Artist"}
        });
        for query_url in [
            "https://www.pixiv.net/en/users/1234/artworks",
            "https://www.pixiv.net/en/tags/original/artworks?s_mode=s_tag",
        ] {
            let parsed = parse_metadata_with_url(&raw, Some(query_url));
            let post_url = "https://www.pixiv.net/en/artworks/114223105";
            assert_eq!(parsed.source_url.as_deref(), Some(post_url));
            assert_eq!(
                parsed.source_urls,
                [post_url, "https://i.pximg.net/img-original/example_p0.png"]
            );
            assert_eq!(parsed.item_key.as_deref(), Some("pixiv:114223105:0"));
        }
    }

    #[test]
    fn e621_metadata_keeps_sources_before_media_and_categorized_tags() {
        let parsed = parse_metadata(&json!({
            "category": "e926",
            "id": 42,
            "file": {"url": "https://static1.e621.net/data/example.jpg"},
            "sources": ["https://artist.example/post/42"],
            "tags_artist": ["artist_name"],
            "tags_species": ["canine"]
        }));

        assert_eq!(parsed.category.as_deref(), Some("e621"));
        assert_eq!(
            parsed.source_urls,
            [
                "https://artist.example/post/42",
                "https://static1.e621.net/data/example.jpg"
            ]
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://static1.e621.net/data/example.jpg")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "artist_name".to_string())));
        assert!(parsed
            .tags
            .contains(&("species".to_string(), "canine".to_string())));
    }

    #[test]
    fn twitter_metadata_uses_handle_for_post_url_and_name_for_creator() {
        let parsed = parse_metadata(&json!({
            "category": "x",
            "tweet_id": "123456",
            "author": {"name": "Display Name", "nick": "actual_handle"}
        }));

        assert_eq!(parsed.category.as_deref(), Some("twitter"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://x.com/actual_handle/status/123456")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "Display Name".to_string())));
    }
}
