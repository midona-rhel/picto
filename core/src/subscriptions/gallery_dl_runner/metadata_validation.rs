use serde::{Deserialize, Serialize};

use super::sites::canonical_site_id;
use super::{extract_creator_identifier, parse_metadata};
use crate::subscriptions::source_adapter::ParsedMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMetadataSchema {
    pub site_id: String,
    pub required_raw_keys: Vec<String>,
    pub required_normalized_fields: Vec<String>,
    pub namespace_mapping: std::collections::HashMap<String, String>,
    pub failure_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMetadataValidationResult {
    pub valid: bool,
    pub missing_required_fields: Vec<String>,
    pub invalid_fields: Vec<String>,
    pub normalized_preview: Option<serde_json::Value>,
    pub warnings: Vec<String>,
}

pub fn get_site_metadata_schema(site_id: &str) -> Option<SiteMetadataSchema> {
    match canonical_site_id(site_id) {
        "pixiv" | "pixivuser" => Some(pixiv_metadata_schema()),
        "gelbooru" => Some(gelbooru_metadata_schema()),
        "danbooru" => Some(danbooru_metadata_schema()),
        _ => None,
    }
}

fn pixiv_metadata_schema() -> SiteMetadataSchema {
    let namespace_mapping = std::collections::HashMap::from([
        ("user.name".to_string(), "creator".to_string()),
        ("user.id".to_string(), "creator".to_string()),
        ("tags[*].name".to_string(), "".to_string()),
    ]);

    SiteMetadataSchema {
        site_id: "pixiv".to_string(),
        required_raw_keys: vec![
            "id".to_string(),
            "user.id|user.name".to_string(),
            "title|caption".to_string(),
            "tags".to_string(),
            "page_count|meta_pages".to_string(),
            "url|file_url".to_string(),
        ],
        required_normalized_fields: vec![
            "remote_post_id".to_string(),
            "creator".to_string(),
            "title".to_string(),
            "description".to_string(),
            "source_urls[]".to_string(),
            "tags[]".to_string(),
        ],
        namespace_mapping,
        failure_policy: "skip_invalid_metadata_row".to_string(),
    }
}

fn gelbooru_metadata_schema() -> SiteMetadataSchema {
    let namespace_mapping = std::collections::HashMap::from([
        ("tag_string_artist".to_string(), "creator".to_string()),
        ("tag_string_character".to_string(), "character".to_string()),
        ("tag_string_copyright".to_string(), "series".to_string()),
        ("tag_string_general".to_string(), "".to_string()),
        ("tag_string_meta".to_string(), "meta".to_string()),
        ("tag_string".to_string(), "".to_string()),
    ]);

    SiteMetadataSchema {
        site_id: "gelbooru".to_string(),
        required_raw_keys: vec![
            "id".to_string(),
            "tags|tag_string".to_string(),
            "file_url".to_string(),
            "source".to_string(),
            "rating".to_string(),
            "md5(if_present)".to_string(),
        ],
        required_normalized_fields: vec![
            "remote_post_id".to_string(),
            "source_urls[]".to_string(),
            "tags[]".to_string(),
            "rating".to_string(),
            "creator(if_present)".to_string(),
        ],
        namespace_mapping,
        failure_policy: "skip_invalid_metadata_row".to_string(),
    }
}

fn danbooru_metadata_schema() -> SiteMetadataSchema {
    let namespace_mapping = std::collections::HashMap::from([
        ("tags_artist".to_string(), "creator".to_string()),
        ("tags_character".to_string(), "character".to_string()),
        ("tags_copyright".to_string(), "series".to_string()),
        ("tags_general".to_string(), "".to_string()),
        ("tags_meta".to_string(), "meta".to_string()),
        ("tag_string_artist".to_string(), "creator".to_string()),
        ("tag_string_character".to_string(), "character".to_string()),
        ("tag_string_copyright".to_string(), "series".to_string()),
        ("tag_string_general".to_string(), "".to_string()),
        ("tag_string_meta".to_string(), "meta".to_string()),
        (
            "artist_commentary.original_title".to_string(),
            "title".to_string(),
        ),
        (
            "artist_commentary.original_description".to_string(),
            "description".to_string(),
        ),
    ]);

    SiteMetadataSchema {
        site_id: "danbooru".to_string(),
        required_raw_keys: vec![
            "id".to_string(),
            "tags_artist|tags_general|category_tags".to_string(),
            "file_url".to_string(),
            "source".to_string(),
            "rating".to_string(),
            "artist_commentary(if_present)".to_string(),
        ],
        required_normalized_fields: vec![
            "remote_post_id".to_string(),
            "creator".to_string(),
            "title".to_string(),
            "description".to_string(),
            "source_urls[]".to_string(),
            "tags[]".to_string(),
            "rating".to_string(),
        ],
        namespace_mapping,
        failure_policy: "skip_invalid_metadata_row".to_string(),
    }
}

fn unsupported_site_metadata_validation(site_id: &str) -> SiteMetadataValidationResult {
    SiteMetadataValidationResult {
        valid: false,
        missing_required_fields: vec!["unsupported_site".to_string()],
        invalid_fields: vec![format!("Unsupported site_id: {site_id}")],
        normalized_preview: None,
        warnings: vec![],
    }
}

fn missing_sample_metadata_validation() -> SiteMetadataValidationResult {
    SiteMetadataValidationResult {
        valid: false,
        missing_required_fields: vec!["sample_metadata_json".to_string()],
        invalid_fields: vec![],
        normalized_preview: None,
        warnings: vec![
            "No sample_metadata_json provided; runtime validation cannot run.".to_string(),
        ],
    }
}

fn collect_source_urls(
    parsed: &ParsedMetadata,
    raw: &serde_json::Value,
    sample_url: &str,
) -> Vec<String> {
    let mut source_urls = Vec::new();
    let mut push_unique = |url: &str| {
        let trimmed = url.trim();
        if trimmed.is_empty() || source_urls.iter().any(|v| v == trimmed) {
            return;
        }
        source_urls.push(trimmed.to_string());
    };

    for url in &parsed.source_urls {
        push_unique(url);
    }
    if let Some(url) = raw.get("file_url").and_then(|v| v.as_str()) {
        push_unique(url);
    }
    if let Some(url) = raw.get("url").and_then(|v| v.as_str()) {
        push_unique(url);
    }
    if let Some(url) = raw.get("source").and_then(|v| v.as_str()) {
        push_unique(url);
    }
    push_unique(sample_url);
    source_urls
}

fn metadata_tags_preview(parsed: &ParsedMetadata) -> Vec<String> {
    parsed
        .tags
        .iter()
        .map(|(ns, st)| {
            if ns.is_empty() {
                st.clone()
            } else {
                format!("{ns}:{st}")
            }
        })
        .collect::<Vec<_>>()
}

fn creator_from_parsed_tags(parsed: &ParsedMetadata) -> Option<String> {
    parsed.tags.iter().find_map(|(ns, tag)| {
        ((ns == "creator" || ns == "artist") && !tag.trim().is_empty()).then(|| tag.clone())
    })
}

fn rating_from_raw(raw: &serde_json::Value) -> Option<String> {
    raw.get("rating").and_then(|v| {
        v.as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| v.as_i64().map(|value| value.to_string()))
    })
}

fn has_danbooru_category_tags(raw: &serde_json::Value) -> bool {
    [
        "tags_artist",
        "tags_character",
        "tags_copyright",
        "tags_general",
        "tags_meta",
        "tag_string_artist",
        "tag_string_character",
        "tag_string_copyright",
        "tag_string_general",
        "tag_string_meta",
    ]
    .iter()
    .any(|key| raw.get(*key).is_some())
}

fn validate_pixiv_site_metadata(
    sample_url: &str,
    raw: &serde_json::Value,
) -> SiteMetadataValidationResult {
    let mut missing_required_fields = Vec::new();
    let mut invalid_fields = Vec::new();
    let mut warnings = Vec::new();

    if raw.get("id").is_none() {
        missing_required_fields.push("id".to_string());
    }
    let user = raw.get("user");
    let has_user_id = user.and_then(|u| u.get("id")).is_some();
    let has_user_name = user
        .and_then(|u| u.get("name"))
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.trim().is_empty());
    if !has_user_id && !has_user_name {
        missing_required_fields.push("user.id|user.name".to_string());
    }
    let has_title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.trim().is_empty());
    let has_caption = raw
        .get("caption")
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.trim().is_empty());
    if !has_title && !has_caption {
        missing_required_fields.push("title|caption".to_string());
    }
    if raw.get("tags").is_none() {
        missing_required_fields.push("tags".to_string());
    }
    if raw.get("page_count").is_none() && raw.get("meta_pages").is_none() {
        missing_required_fields.push("page_count|meta_pages".to_string());
    }
    if raw.get("url").is_none() && raw.get("file_url").is_none() {
        missing_required_fields.push("url|file_url".to_string());
    }

    if let Some(tags) = raw.get("tags") {
        if !tags.is_array() && !tags.is_string() && !tags.is_object() {
            invalid_fields.push("tags (expected array/string/object)".to_string());
        }
    }
    if let Some(id) = raw.get("id") {
        if !id.is_string() && !id.is_number() {
            invalid_fields.push("id (expected number or string)".to_string());
        }
    }

    let parsed = parse_metadata(raw);
    let creator = extract_creator_identifier(raw).or_else(|| creator_from_parsed_tags(&parsed));
    let source_urls = collect_source_urls(&parsed, raw, sample_url);
    let normalized_title = parsed.title.clone().or_else(|| parsed.description.clone());
    let preview = serde_json::json!({
        "site_id": "pixiv",
        "remote_post_id": parsed.post_id,
        "creator": creator,
        "title": normalized_title,
        "description": parsed.description,
        "source_urls": source_urls,
        "tags": metadata_tags_preview(&parsed),
        "post_published_at": raw.get("date").cloned(),
        "validation_version": 1
    });

    if preview
        .get("remote_post_id")
        .and_then(|v| v.as_str())
        .is_none()
        && preview
            .get("remote_post_id")
            .and_then(|v| v.as_number())
            .is_none()
    {
        invalid_fields.push("remote_post_id".to_string());
    }
    if preview.get("creator").and_then(|v| v.as_str()).is_none() {
        invalid_fields.push("creator".to_string());
    }
    if preview.get("title").and_then(|v| v.as_str()).is_none() {
        invalid_fields.push("title".to_string());
    }
    if preview
        .get("description")
        .and_then(|v| v.as_str())
        .is_none()
    {
        warnings.push("description missing; caption/description not present in sample".to_string());
    }
    if preview
        .get("source_urls")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("source_urls[]".to_string());
    }
    if preview
        .get("tags")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("tags[]".to_string());
    }

    let valid = missing_required_fields.is_empty() && invalid_fields.is_empty();
    SiteMetadataValidationResult {
        valid,
        missing_required_fields,
        invalid_fields,
        normalized_preview: Some(preview),
        warnings,
    }
}

fn validate_gelbooru_site_metadata(
    sample_url: &str,
    raw: &serde_json::Value,
) -> SiteMetadataValidationResult {
    let mut missing_required_fields = Vec::new();
    let mut invalid_fields = Vec::new();

    if raw.get("id").is_none() {
        missing_required_fields.push("id".to_string());
    }
    if raw.get("tags").is_none() && raw.get("tag_string").is_none() {
        missing_required_fields.push("tags|tag_string".to_string());
    }
    if raw.get("file_url").is_none() {
        missing_required_fields.push("file_url".to_string());
    }
    if raw.get("source").is_none() {
        missing_required_fields.push("source".to_string());
    }
    if raw.get("rating").is_none() {
        missing_required_fields.push("rating".to_string());
    }

    if let Some(id) = raw.get("id") {
        if !id.is_string() && !id.is_number() {
            invalid_fields.push("id (expected number or string)".to_string());
        }
    }
    if let Some(tags) = raw.get("tags") {
        if !tags.is_array() && !tags.is_string() && !tags.is_object() {
            invalid_fields.push("tags (expected array/string/object)".to_string());
        }
    }
    if let Some(tag_string) = raw.get("tag_string") {
        if !tag_string.is_string() {
            invalid_fields.push("tag_string (expected string)".to_string());
        }
    }
    if let Some(file_url) = raw.get("file_url") {
        if file_url
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
        {
            invalid_fields.push("file_url (expected non-empty string)".to_string());
        }
    }
    if let Some(source) = raw.get("source") {
        if source
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
        {
            invalid_fields.push("source (expected non-empty string)".to_string());
        }
    }
    if let Some(rating) = raw.get("rating") {
        if !rating.is_string() && !rating.is_number() {
            invalid_fields.push("rating (expected string or number)".to_string());
        }
    }
    if let Some(md5) = raw.get("md5") {
        let is_hex32 = md5
            .as_str()
            .map(str::trim)
            .is_some_and(|value| value.len() == 32 && value.chars().all(|c| c.is_ascii_hexdigit()));
        if !is_hex32 {
            invalid_fields.push("md5 (expected 32-char hex string when present)".to_string());
        }
    }

    let parsed = parse_metadata(raw);
    let source_urls = collect_source_urls(&parsed, raw, sample_url);
    let creator = creator_from_parsed_tags(&parsed);
    let rating = parsed.rating.clone().or_else(|| rating_from_raw(raw));
    let preview = serde_json::json!({
        "site_id": "gelbooru",
        "remote_post_id": parsed.post_id,
        "source_urls": source_urls,
        "tags": metadata_tags_preview(&parsed),
        "rating": rating,
        "creator": creator,
        "md5": raw.get("md5").cloned(),
        "validation_version": 1
    });

    if preview
        .get("remote_post_id")
        .and_then(|v| v.as_str())
        .is_none()
        && preview
            .get("remote_post_id")
            .and_then(|v| v.as_number())
            .is_none()
    {
        invalid_fields.push("remote_post_id".to_string());
    }
    if preview
        .get("source_urls")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("source_urls[]".to_string());
    }
    if preview
        .get("tags")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("tags[]".to_string());
    }
    if preview.get("rating").and_then(|v| v.as_str()).is_none() {
        invalid_fields.push("rating".to_string());
    }

    let valid = missing_required_fields.is_empty() && invalid_fields.is_empty();
    SiteMetadataValidationResult {
        valid,
        missing_required_fields,
        invalid_fields,
        normalized_preview: Some(preview),
        warnings: Vec::new(),
    }
}

fn validate_danbooru_site_metadata(
    sample_url: &str,
    raw: &serde_json::Value,
) -> SiteMetadataValidationResult {
    let mut missing_required_fields = Vec::new();
    let mut invalid_fields = Vec::new();
    let mut warnings = Vec::new();

    if raw.get("id").is_none() {
        missing_required_fields.push("id".to_string());
    }
    if !has_danbooru_category_tags(raw) {
        missing_required_fields.push("tags_artist|tags_general|category_tags".to_string());
    }
    if raw.get("file_url").is_none() {
        missing_required_fields.push("file_url".to_string());
    }
    if raw.get("source").is_none() {
        missing_required_fields.push("source".to_string());
    }
    if raw.get("rating").is_none() {
        missing_required_fields.push("rating".to_string());
    }

    if let Some(id) = raw.get("id") {
        if !id.is_string() && !id.is_number() {
            invalid_fields.push("id (expected number or string)".to_string());
        }
    }
    for key in [
        "tags_artist",
        "tags_character",
        "tags_copyright",
        "tags_general",
        "tags_meta",
    ] {
        if let Some(value) = raw.get(key) {
            if !value.is_array() {
                invalid_fields.push(format!("{key} (expected array of strings)"));
            }
        }
    }
    for key in [
        "tag_string_artist",
        "tag_string_character",
        "tag_string_copyright",
        "tag_string_general",
        "tag_string_meta",
    ] {
        if let Some(value) = raw.get(key) {
            if !value.is_string() {
                invalid_fields.push(format!("{key} (expected space-separated string)"));
            }
        }
    }
    if let Some(file_url) = raw.get("file_url") {
        if file_url
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
        {
            invalid_fields.push("file_url (expected non-empty string)".to_string());
        }
    }
    if let Some(source) = raw.get("source") {
        if source
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
        {
            invalid_fields.push("source (expected non-empty string)".to_string());
        }
    }
    if let Some(rating) = raw.get("rating") {
        if !rating.is_string() && !rating.is_number() {
            invalid_fields.push("rating (expected string or number)".to_string());
        }
    }
    if let Some(artist_commentary) = raw.get("artist_commentary") {
        if !artist_commentary.is_object() {
            invalid_fields.push("artist_commentary (expected object when present)".to_string());
        } else if let Some(object) = artist_commentary.as_object() {
            for key in ["original_title", "original_description"] {
                if let Some(value) = object.get(key) {
                    if !value.is_string() {
                        invalid_fields.push(format!("artist_commentary.{key} (expected string)"));
                    }
                }
            }
        }
    }

    let parsed = parse_metadata(raw);
    let source_urls = collect_source_urls(&parsed, raw, sample_url);
    let creator_tags = parsed
        .tags
        .iter()
        .filter_map(|(ns, tag)| {
            ((ns == "creator" || ns == "artist") && !tag.trim().is_empty()).then(|| tag.clone())
        })
        .collect::<Vec<_>>();
    let rating = parsed.rating.clone().or_else(|| rating_from_raw(raw));
    let preview = serde_json::json!({
        "site_id": "danbooru",
        "remote_post_id": parsed.post_id,
        "creator": creator_tags,
        "title": parsed.title,
        "description": parsed.description,
        "source_urls": source_urls,
        "tags": metadata_tags_preview(&parsed),
        "rating": rating,
        "validation_version": 1
    });

    if preview
        .get("remote_post_id")
        .and_then(|v| v.as_str())
        .is_none()
        && preview
            .get("remote_post_id")
            .and_then(|v| v.as_number())
            .is_none()
    {
        invalid_fields.push("remote_post_id".to_string());
    }
    if preview
        .get("creator")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("creator".to_string());
    }
    if preview.get("title").and_then(|v| v.as_str()).is_none() {
        warnings.push(
            "title missing; artist_commentary/direct title not present in sample".to_string(),
        );
    }
    if preview
        .get("description")
        .and_then(|v| v.as_str())
        .is_none()
    {
        warnings.push(
            "description missing; artist_commentary/direct description not present in sample"
                .to_string(),
        );
    }
    if preview
        .get("source_urls")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("source_urls[]".to_string());
    }
    if preview
        .get("tags")
        .and_then(|v| v.as_array())
        .map_or(true, |arr| arr.is_empty())
    {
        invalid_fields.push("tags[]".to_string());
    }
    if preview.get("rating").and_then(|v| v.as_str()).is_none() {
        invalid_fields.push("rating".to_string());
    }

    let valid = missing_required_fields.is_empty() && invalid_fields.is_empty();
    SiteMetadataValidationResult {
        valid,
        missing_required_fields,
        invalid_fields,
        normalized_preview: Some(preview),
        warnings,
    }
}

pub fn validate_site_metadata(
    site_id: &str,
    sample_url: &str,
    sample_metadata_json: Option<&serde_json::Value>,
) -> SiteMetadataValidationResult {
    let Some(raw) = sample_metadata_json else {
        return missing_sample_metadata_validation();
    };

    match canonical_site_id(site_id) {
        "pixiv" | "pixivuser" => validate_pixiv_site_metadata(sample_url, raw),
        "gelbooru" => validate_gelbooru_site_metadata(sample_url, raw),
        "danbooru" => validate_danbooru_site_metadata(sample_url, raw),
        _ => unsupported_site_metadata_validation(site_id),
    }
}
