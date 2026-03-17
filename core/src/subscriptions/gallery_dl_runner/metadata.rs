use super::ParsedMetadata;
use super::adapters::adapter_for_json;

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

    let source_url = json
        .get("file_url")
        .or_else(|| json.get("url"))
        .or_else(|| json.get("source"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

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

    ParsedMetadata {
        tags,
        description,
        source_url,
        rating,
        title,
        post_id,
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
