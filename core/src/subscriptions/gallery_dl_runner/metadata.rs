use super::ParsedMetadata;
use super::adapters::adapter_for_json;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};


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
    for key in ["created_at", "date", "published_at", "published", "upload_date"] {
        if let Some(value) = json.get(key).and_then(|v| v.as_str()) {
            if let Some(normalized) = normalize_created_at(value) {
                return Some(normalized);
            }
        }
    }
    None
}

/// Parse a gallery-dl metadata sidecar JSON into normalized metadata.
///
/// Handles site-specific tag formats:
/// - Danbooru: `tags_artist`, `tags_general`, etc. (arrays from space-split `tag_string_*`)
/// - E621: `tags` dict with category arrays (`{"general": [...], "artist": [...]}`)
/// - Pixiv: `tags` array of objects (`[{"name": "...", "translated_name": "..."}]`)
/// - Fallback: `tags` as flat array of strings or space-separated string
pub fn parse_metadata(json: &serde_json::Value) -> ParsedMetadata {
    let adapter = adapter_for_json(json);
    let mut tags = adapter.parse_tags(json);
    if let Some(creator) = adapter.extract_creator_identifier(json) {
        if !tags
            .iter()
            .any(|(ns, subtag)| ns == "creator" && subtag == &creator)
        {
            tags.push(("creator".to_string(), creator));
        }
    }

    // Try artist_commentary (Danbooru with metadata: true), then direct fields.
    let description = json
        .get("artist_commentary")
        .and_then(|ac| {
            ac.get("original_description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("caption")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("body")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("content")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(String::from);

    let source_urls = adapter.collect_source_urls(json);
    let source_url = source_urls.first().cloned();

    let rating = json.get("rating").and_then(|v| {
        v.as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| v.as_i64().map(|value| value.to_string()))
    });

    let title = json
        .get("artist_commentary")
        .and_then(|ac| {
            ac.get("original_title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(String::from);

    let post_id = json
        .get("id")
        .map(|v| {
            if let Some(n) = v.as_i64() {
                n.to_string()
            } else {
                v.as_str().unwrap_or("").to_string()
            }
        })
        .filter(|s| !s.is_empty());

    let category = json
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let created_at = parse_created_at(json);

    ParsedMetadata {
        tags,
        description,
        source_url,
        source_urls,
        rating,
        title,
        post_id,
        created_at,
        category,
    }
}

/// Parse tags from a gallery-dl metadata sidecar.
///
/// Priority order:
/// 1. Danbooru-style: `tags_artist`, `tags_character`, `tags_copyright`,
///    `tags_general`, `tags_meta` (arrays)
/// 2. E621/nested: `tags` as object with category keys → arrays
/// 3. Pixiv: `tags` as array of `{"name": "...", "translated_name": "..."}` objects
/// 4. Fallback: `tags` as flat array of strings or space-separated string
pub fn parse_tags(json: &serde_json::Value) -> Vec<(String, String)> {
    adapter_for_json(json).parse_tags(json)
}

pub fn extract_creator_identifier(json: &serde_json::Value) -> Option<String> {
    adapter_for_json(json).extract_creator_identifier(json)
}
