//! Gallery-dl subprocess runner.
//!
//! Manages gallery-dl invocations: generates temp config files, spawns the
//! subprocess, scans output directories, and parses metadata sidecar JSON files.
//!
//! Gallery-dl reference: `external/gallery-dl/` (source code).
//! Key source files consulted:
//! - `gallery_dl/option.py` — CLI flag definitions (argparse)
//! - `gallery_dl/job.py` — DownloadJob, skip/abort logic (lines 621-632)
//! - `gallery_dl/postprocessor/metadata.py` — sidecar JSON writer
//! - `gallery_dl/archive.py` — SQLite download archive
//! - `gallery_dl/extractor/danbooru.py` — tag_string_* fields
//! - `gallery_dl/extractor/e621.py` — nested tags dict

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::credential_store::SiteCredential;

/// A known gallery-dl site with its URL template.
#[derive(Debug, Clone, Serialize)]
pub struct SiteEntry {
    /// Internal identifier (gallery-dl category name).
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Canonical site domain shown in UI and used as credential fallback key.
    pub domain: &'static str,
    /// URL template — `{query}` is replaced with the user's search tags.
    pub url_template: &'static str,
    /// Example query to show in the UI.
    pub example_query: &'static str,
    /// Whether this source supports tag/text query style URLs.
    pub supports_query: bool,
    /// Whether account/profile style queries are supported.
    pub supports_account: bool,
    /// Whether we support storing auth material for this source.
    pub auth_supported: bool,
    /// Whether auth is commonly required/recommended for full access.
    pub auth_required_for_full_access: bool,
}

/// Built-in site registry. The user picks one of these; we substitute `{query}`.
pub static SITES: &[SiteEntry] = &[
    SiteEntry {
        id: "pixiv",
        domain: "pixiv.net",
        name: "Pixiv",
        url_template: "https://www.pixiv.net/en/tags/{query}/artworks?s_mode=s_tag",
        example_query: "風景",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "gelbooru",
        domain: "gelbooru.com",
        name: "Gelbooru",
        url_template: "https://gelbooru.com/index.php?page=post&s=list&tags={query}",
        example_query: "1girl solo",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "danbooru",
        domain: "danbooru.donmai.us",
        name: "Danbooru",
        url_template: "https://danbooru.donmai.us/posts?tags={query}",
        example_query: "1girl solo blue_eyes",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "3dbooru",
        domain: "3dbooru.org",
        name: "3DBooru",
        url_template: "https://3dbooru.org/index.php?page=post&s=list&tags={query}",
        example_query: "solo",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "artstation",
        domain: "artstation.com",
        name: "ArtStation",
        url_template: "https://www.artstation.com/{query}",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "sankaku",
        domain: "sankakucomplex.com",
        name: "Sankaku",
        url_template: "https://chan.sankakucomplex.com/?tags={query}&commit=Search",
        example_query: "1girl",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "idolcomplex",
        domain: "idol.sankakucomplex.com",
        name: "IdolComplex",
        url_template: "https://idol.sankakucomplex.com/?tags={query}&commit=Search",
        example_query: "idol",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "twitter",
        domain: "twitter.com",
        name: "Twitter/X",
        url_template: "https://twitter.com/{query}",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "deviantart",
        domain: "deviantart.com",
        name: "DeviantArt",
        url_template: "https://deviantart.com/{query}",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "patreon",
        domain: "patreon.com",
        name: "Patreon",
        url_template: "https://www.patreon.com/{query}/posts",
        example_query: "creatorname",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "nijie",
        domain: "nijie.info",
        name: "Nijie",
        url_template: "https://nijie.info/members_illust.php?id={query}",
        example_query: "12345",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "tumblr",
        domain: "tumblr.com",
        name: "Tumblr",
        url_template: "https://{query}.tumblr.com",
        example_query: "blogname",
        supports_query: true,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "fantia",
        domain: "fantia.jp",
        name: "Fantia",
        url_template: "https://fantia.jp/fanclubs/{query}/posts",
        example_query: "12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "fanbox",
        domain: "fanbox.cc",
        name: "Fanbox",
        url_template: "https://{query}.fanbox.cc",
        example_query: "creatorname",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "webtoons",
        domain: "webtoons.com",
        name: "Webtoons",
        url_template: "https://www.webtoons.com/en/{query}",
        example_query: "genre/title/list?title_no=12345",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "kemono",
        domain: "kemono.su",
        name: "Kemono.party",
        url_template: "https://kemono.su/{query}",
        example_query: "service/user/12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "coomer",
        domain: "coomer.su",
        name: "Coomer.party",
        url_template: "https://coomer.su/{query}",
        example_query: "service/user/12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "seiso",
        domain: "seiso.party",
        name: "Seiso.party",
        url_template: "https://seiso.party/{query}",
        example_query: "service/user/12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "baraag",
        domain: "baraag.net",
        name: "Baraag",
        url_template: "https://baraag.net/{query}",
        example_query: "@username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "pawoo",
        domain: "pawoo.net",
        name: "Pawoo",
        url_template: "https://pawoo.net/{query}",
        example_query: "@username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "hentaifoundry",
        domain: "hentai-foundry.com",
        name: "Hentai Foundry",
        url_template: "https://www.hentai-foundry.com/user/{query}/profile",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "yandere",
        domain: "yande.re",
        name: "Yande.re",
        url_template: "https://yande.re/post?tags={query}",
        example_query: "landscape",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "rule34",
        domain: "rule34.xxx",
        name: "Rule34.xxx",
        url_template: "https://rule34.xxx/index.php?page=post&s=list&tags={query}",
        example_query: "solo",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "e621",
        domain: "e621.net",
        name: "e621",
        url_template: "https://e621.net/posts?tags={query}",
        example_query: "solo canine rating:safe",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "furaffinity",
        domain: "furaffinity.net",
        name: "FurAffinity",
        url_template: "https://www.furaffinity.net/user/{query}/",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "instagram",
        domain: "instagram.com",
        name: "Instagram",
        url_template: "https://instagram.com/{query}",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    // Existing maintained extras.
    SiteEntry {
        id: "konachan",
        domain: "konachan.com",
        name: "Konachan",
        url_template: "https://konachan.com/post?tags={query}",
        example_query: "landscape",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "safebooru",
        domain: "safebooru.org",
        name: "Safebooru",
        url_template: "https://safebooru.org/index.php?page=post&s=list&tags={query}",
        example_query: "1girl smile",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "lolibooru",
        domain: "lolibooru.moe",
        name: "Lolibooru",
        url_template: "https://lolibooru.moe/post?tags={query}",
        example_query: "landscape",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "pixivuser",
        domain: "pixiv.net",
        name: "Pixiv (user)",
        url_template: "https://www.pixiv.net/en/users/{query}",
        example_query: "12345",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
];

/// Canonicalize legacy/alias site ids to current internal ids.
pub fn canonical_site_id(id: &str) -> &str {
    match id {
        "rule34xxx" | "rule34.xxx" => "rule34",
        "e621.net" => "e621",
        "furaffinity.net" => "furaffinity",
        "yande.re" => "yandere",
        "kemono.party" => "kemono",
        "coomer.party" => "coomer",
        "baraag.net" => "baraag",
        "pawoo.net" => "pawoo",
        _ => id,
    }
}

/// Look up a site entry by ID.
pub fn site_by_id(id: &str) -> Option<&'static SiteEntry> {
    let canonical = canonical_site_id(id);
    SITES.iter().find(|s| s.id == canonical)
}

/// Build a full URL from a site ID and query string.
pub fn build_url(site_id: &str, query: &str) -> Option<String> {
    site_by_id(site_id).map(|site| substitute_query(site.url_template, query))
}

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

    if let Some(url) = parsed.source_url.as_deref() {
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
    parsed
        .tags
        .iter()
        .find_map(|(ns, tag)| (ns == "creator" && !tag.trim().is_empty()).then(|| tag.clone()))
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
        .filter_map(|(ns, tag)| (ns == "creator" && !tag.trim().is_empty()).then(|| tag.clone()))
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

/// Options for a single gallery-dl invocation.
pub struct RunOptions {
    /// Full URL to download from (after query substitution).
    pub url: String,
    /// Max files to download (maps to `--range 1-N`). None = unlimited.
    pub file_limit: Option<u32>,
    /// Abort after N consecutive skipped files (maps to `-A N`).
    /// None = no abort (first run / initial sync).
    pub abort_threshold: Option<u32>,
    /// Seconds between HTTP requests during extraction (`sleep-request`).
    pub sleep_request: f64,
    /// Optional credential for site authentication.
    pub credential: Option<SiteCredential>,
    /// Path to the download archive SQLite DB.
    pub archive_path: PathBuf,
    /// Optional archive key prefix (used to support targeted reset per subscription/query).
    pub archive_prefix: Option<String>,
    /// Cancellation token — kills the subprocess when cancelled.
    pub cancel: CancellationToken,
}

/// Result of a gallery-dl invocation.
pub struct RunResult {
    pub items: Vec<DownloadedItem>,
    pub exit_code: i32,
    pub stderr_output: String,
}

/// A single file downloaded by gallery-dl, paired with its parsed metadata.
pub struct DownloadedItem {
    pub file_path: PathBuf,
    pub metadata: ParsedMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Unauthorized,
    Expired,
    RateLimited,
    Network,
    Unknown,
}

/// Normalized metadata extracted from a gallery-dl JSON sidecar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedMetadata {
    /// Tags as (namespace, subtag) pairs.
    pub tags: Vec<(String, String)>,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub rating: Option<String>,
    pub title: Option<String>,
    pub post_id: Option<String>,
    /// Gallery-dl extractor category (e.g. "danbooru", "pixiv").
    pub category: Option<String>,
}

/// The gallery-dl subprocess runner.
pub struct GalleryDlRunner {
    binary_path: PathBuf,
}

impl GalleryDlRunner {
    pub fn new(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }

    /// Run gallery-dl and return downloaded items with parsed metadata.
    pub async fn run(&self, opts: &RunOptions) -> Result<RunResult, String> {
        self.ensure_runtime_dependencies().await?;

        // 1. Create temp download directory
        let temp_dir =
            std::env::temp_dir().join(format!("picto_gdl_{:016x}", rand::random::<u64>()));
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| format!("Failed to create temp dir: {e}"))?;

        // 2. Build and write temp config
        let config = build_config(opts, &temp_dir);
        let config_path = temp_dir.join("config.json");
        let config_json = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Config serialization error: {e}"))?;
        tokio::fs::write(&config_path, &config_json)
            .await
            .map_err(|e| format!("Config write error: {e}"))?;

        // 3. Build command arguments
        let mut args = vec![
            "--config".to_string(),
            config_path.display().to_string(),
            "--config-ignore".to_string(), // don't read user's default configs
            "--write-metadata".to_string(),
            "--no-input".to_string(),
            "-d".to_string(),
            temp_dir.display().to_string(),
        ];

        if let Some(limit) = opts.file_limit {
            args.push("--range".to_string());
            args.push(format!("1-{limit}"));
        }

        if let Some(threshold) = opts.abort_threshold {
            args.push("-A".to_string());
            args.push(threshold.to_string());
        }

        if !opts.archive_path.as_os_str().is_empty() {
            args.push("--download-archive".to_string());
            args.push(opts.archive_path.display().to_string());
        }

        args.push(opts.url.clone());

        info!(
            url = %opts.url,
            file_limit = ?opts.file_limit,
            abort_threshold = ?opts.abort_threshold,
            "Spawning gallery-dl"
        );
        debug!(binary = %self.binary_path.display(), args = ?args, "gallery-dl command");

        // 4. Spawn subprocess
        let mut child = tokio::process::Command::new(&self.binary_path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn gallery-dl: {e}"))?;

        // 5. Capture stderr handle, then wait for exit or cancellation
        let child_stderr = child.stderr.take();
        let child_stdout = child.stdout.take();

        let status = tokio::select! {
            _ = opts.cancel.cancelled() => {
                info!("Gallery-dl cancelled, killing subprocess");
                let _ = child.kill().await;
                child.wait().await
                    .map_err(|e| format!("Failed to wait for gallery-dl after kill: {e}"))?
            }
            result = child.wait() => {
                result.map_err(|e| format!("Gallery-dl process error: {e}"))?
            }
        };

        let exit_code = status.code().unwrap_or(-1);

        // Read stderr for logging
        let stderr = if let Some(mut se) = child_stderr {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = se.read_to_end(&mut buf).await;
            String::from_utf8_lossy(&buf).to_string()
        } else {
            String::new()
        };
        drop(child_stdout);

        if !stderr.is_empty() {
            for line in stderr.lines().take(20) {
                debug!(line, "gallery-dl stderr");
            }
        }

        info!(exit_code, "gallery-dl finished");

        // 6. Scan output directory for downloaded files + metadata sidecars
        let items = scan_output_dir(&temp_dir).await?;

        // 7. Clean up temp config (leave downloaded files for caller to import)
        let _ = tokio::fs::remove_file(&config_path).await;

        Ok(RunResult {
            items,
            exit_code,
            stderr_output: stderr,
        })
    }

    async fn ensure_runtime_dependencies(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let vendor_marker = format!(
                "{}vendor{}gallery-dl{}gallery-dl",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            );
            let bin = self.binary_path.to_string_lossy();
            if !bin.contains(&vendor_marker) {
                return Ok(());
            }

            let vendor_dir = self
                .binary_path
                .parent()
                .ok_or_else(|| "Invalid gallery-dl vendor path".to_string())?;
            let wheel_dir = vendor_dir.join("wheel");
            let existing_py = std::env::var("PYTHONPATH").unwrap_or_default();
            let merged_py = if existing_py.is_empty() {
                wheel_dir.display().to_string()
            } else {
                format!("{}:{}", wheel_dir.display(), existing_py)
            };

            let check_status = tokio::process::Command::new("python3")
                .arg("-c")
                .arg("import requests")
                .env("PYTHONPATH", &merged_py)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|e| format!("Failed to validate gallery-dl python deps: {e}"))?;
            if check_status.success() {
                return Ok(());
            }

            warn!("gallery-dl dependency bootstrap: installing missing Python package 'requests'");
            let install_status = tokio::process::Command::new("python3")
                .args([
                    "-m",
                    "pip",
                    "install",
                    "--disable-pip-version-check",
                    "--quiet",
                    "--target",
                ])
                .arg(&wheel_dir)
                .arg("requests")
                .status()
                .await
                .map_err(|e| format!("Failed to run pip for gallery-dl dependencies: {e}"))?;
            if !install_status.success() {
                return Err(
                    "gallery-dl is missing Python dependency 'requests' and auto-install failed. Run `bash scripts/download-gallery-dl.sh`."
                        .to_string(),
                );
            }

            let recheck_status = tokio::process::Command::new("python3")
                .arg("-c")
                .arg("import requests")
                .env("PYTHONPATH", &merged_py)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|e| format!("Failed to re-validate gallery-dl python deps: {e}"))?;
            if !recheck_status.success() {
                return Err(
                    "gallery-dl dependency check still failing after install. Run `bash scripts/download-gallery-dl.sh`."
                        .to_string(),
                );
            }
        }

        Ok(())
    }
}

/// After import, call this to remove the temp download directory.
pub async fn cleanup_temp_dir(temp_dir: &Path) {
    if let Err(e) = tokio::fs::remove_dir_all(temp_dir).await {
        warn!(path = %temp_dir.display(), error = %e, "Failed to clean up temp dir");
    }
}

fn build_config(opts: &RunOptions, _temp_dir: &Path) -> serde_json::Value {
    let mut extractor = serde_json::Map::new();

    extractor.insert(
        "sleep-request".into(),
        serde_json::Value::Number(
            serde_json::Number::from_f64(opts.sleep_request).unwrap_or(serde_json::Number::from(2)),
        ),
    );

    extractor.insert("metadata".into(), serde_json::Value::Bool(true));

    if let Some(prefix) = opts
        .archive_prefix
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        extractor.insert(
            "archive-prefix".into(),
            serde_json::Value::String(prefix.clone()),
        );
    }

    if let Some(ref cred) = opts.credential {
        let auth = crate::credential_store::build_extractor_auth(cred);
        extractor.insert(cred.site_category.clone(), auth);
    }

    let mut root = serde_json::Map::new();
    root.insert("extractor".into(), serde_json::Value::Object(extractor));

    let mut output = serde_json::Map::new();
    output.insert("progress".into(), serde_json::Value::Bool(false));
    root.insert("output".into(), serde_json::Value::Object(output));

    serde_json::Value::Object(root)
}

/// Walk the temp directory and pair each media file with its `.json` sidecar.
async fn scan_output_dir(dir: &Path) -> Result<Vec<DownloadedItem>, String> {
    let mut items = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .map_err(|e| format!("Read dir error: {e}"))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            // Skip JSON sidecars and config files — we'll find them when processing media files
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "json" {
                continue;
            }

            // Look for matching sidecar: {filename}.json
            let sidecar_path = path.with_extension(format!(
                "{}.json",
                path.extension().and_then(|e| e.to_str()).unwrap_or("")
            ));

            let metadata = if sidecar_path.is_file() {
                match tokio::fs::read_to_string(&sidecar_path).await {
                    Ok(json_str) => match serde_json::from_str::<serde_json::Value>(&json_str) {
                        Ok(json) => parse_metadata(&json),
                        Err(e) => {
                            warn!(path = %sidecar_path.display(), error = %e, "Sidecar parse error");
                            ParsedMetadata::default()
                        }
                    },
                    Err(e) => {
                        warn!(path = %sidecar_path.display(), error = %e, "Sidecar read error");
                        ParsedMetadata::default()
                    }
                }
            } else {
                debug!(path = %path.display(), "No sidecar found");
                ParsedMetadata::default()
            };

            items.push(DownloadedItem {
                file_path: path,
                metadata,
            });
        }
    }

    Ok(items)
}

/// Parse a gallery-dl metadata sidecar JSON into normalized metadata.
///
/// Handles site-specific tag formats:
/// - Danbooru: `tags_artist`, `tags_general`, etc. (arrays from space-split `tag_string_*`)
/// - E621: `tags` dict with category arrays (`{"general": [...], "artist": [...]}`)
/// - Pixiv: `tags` array of objects (`[{"name": "...", "translated_name": "..."}]`)
/// - Fallback: `tags` as flat array of strings or space-separated string
pub fn parse_metadata(json: &serde_json::Value) -> ParsedMetadata {
    let mut tags = parse_tags(json);
    if let Some(creator) = extract_creator_identifier(json) {
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
    let mut tags = Vec::new();

    // 1. Danbooru-style: tags_artist, tags_general, etc.
    static DANBOORU_CATEGORIES: &[(&str, &str)] = &[
        ("tags_artist", "creator"),
        ("tags_character", "character"),
        ("tags_copyright", "series"),
        ("tags_general", ""),
        ("tags_meta", "meta"),
    ];

    let has_danbooru = DANBOORU_CATEGORIES
        .iter()
        .any(|(key, _)| json.get(*key).is_some());

    if has_danbooru {
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
    }

    // 2. Danbooru/Gelbooru legacy strings: tag_string_*.
    static DANBOORU_TAG_STRINGS: &[(&str, &str)] = &[
        ("tag_string_artist", "creator"),
        ("tag_string_character", "character"),
        ("tag_string_copyright", "series"),
        ("tag_string_general", ""),
        ("tag_string_meta", "meta"),
    ];
    let has_tag_strings = DANBOORU_TAG_STRINGS
        .iter()
        .any(|(key, _)| json.get(*key).is_some());
    if has_tag_strings {
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
    }

    // 3. Try `tags` field.
    if let Some(tags_val) = json.get("tags") {
        // 3a. E621-style: tags is an object with category arrays
        if let Some(obj) = tags_val.as_object() {
            // Check if values are arrays (E621) vs other structure
            let is_category_dict = obj.values().any(|v| v.is_array());
            if is_category_dict {
                static E621_NAMESPACE_MAP: &[(&str, &str)] = &[
                    ("artist", "creator"),
                    ("character", "character"),
                    ("copyright", "series"),
                    ("general", ""),
                    ("meta", "meta"),
                    ("species", "species"),
                    ("lore", "lore"),
                ];
                for (category, namespace) in E621_NAMESPACE_MAP {
                    if let Some(arr) = obj.get(*category).and_then(|v| v.as_array()) {
                        for tag_val in arr {
                            if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                                tags.push((namespace.to_string(), tag.to_string()));
                            }
                        }
                    }
                }
                // Also collect any categories not in our map
                for (category, value) in obj {
                    let mapped = E621_NAMESPACE_MAP
                        .iter()
                        .any(|(cat, _)| *cat == category.as_str());
                    if !mapped {
                        if let Some(arr) = value.as_array() {
                            for tag_val in arr {
                                if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                                    tags.push((category.clone(), tag.to_string()));
                                }
                            }
                        }
                    }
                }
                return tags;
            }
        }

        // 3b. Pixiv-style: tags is array of objects with "name" field
        if let Some(arr) = tags_val.as_array() {
            if arr.first().and_then(|v| v.as_object()).is_some() {
                // Array of tag objects
                for tag_obj in arr {
                    if let Some(name) = tag_obj.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            tags.push((String::new(), name.to_string()));
                        }
                    }
                }
                return tags;
            }

            // 3c. Flat array of strings
            for tag_val in arr {
                if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                    tags.push((String::new(), tag.to_string()));
                }
            }
            return tags;
        }

        // 3d. Space-separated string (rare but possible)
        if let Some(tag_str) = tags_val.as_str() {
            for tag in tag_str.split_whitespace() {
                if !tag.is_empty() {
                    tags.push((String::new(), tag.to_string()));
                }
            }
            if !tags.is_empty() {
                return tags;
            }
        }
    }

    // 4. Gelbooru fallback: plain tag_string with no namespace metadata.
    if let Some(tag_str) = json.get("tag_string").and_then(|v| v.as_str()) {
        for tag in tag_str.split_whitespace() {
            if !tag.is_empty() {
                tags.push((String::new(), tag.to_string()));
            }
        }
    }

    tags
}

pub fn extract_creator_identifier(json: &serde_json::Value) -> Option<String> {
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

/// Substitute `{query}` placeholder in a URL template.
pub fn substitute_query(template: &str, query: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    template.replace("{query}", &encoded)
}

/// Extract the domain from a URL for rate limiting / credential lookup.
pub fn extract_domain(url_str: &str) -> Option<String> {
    url::Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
}

/// Classify gallery-dl stderr output into a coarse failure kind used by
/// subscription runtime and UI health diagnostics.
pub fn classify_failure(stderr: &str) -> FailureKind {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("unauthorized")
        || lower.contains("authrequired")
        || lower.contains("missing authentication")
        || lower.contains("login required")
        || lower.contains("authentication required")
    {
        return FailureKind::Unauthorized;
    }
    if lower.contains("expired")
        || lower.contains("token invalid")
        || lower.contains("session invalid")
        || lower.contains("session has expired")
    {
        return FailureKind::Expired;
    }
    if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests")
    {
        return FailureKind::RateLimited;
    }
    if lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("network is unreachable")
        || lower.contains("dns")
    {
        return FailureKind::Network;
    }
    FailureKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_danbooru_tags() {
        let json = serde_json::json!({
            "id": 12345,
            "tags_artist": ["artist_name"],
            "tags_character": ["char_a", "char_b"],
            "tags_copyright": ["series_name"],
            "tags_general": ["1girl", "solo", "blue_eyes"],
            "tags_meta": ["highres"]
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 8); // 1 artist + 2 char + 1 copyright + 3 general + 1 meta
        assert!(tags.contains(&("creator".to_string(), "artist_name".to_string())));
        assert!(tags.contains(&("character".to_string(), "char_a".to_string())));
        assert!(tags.contains(&("character".to_string(), "char_b".to_string())));
        assert!(tags.contains(&("series".to_string(), "series_name".to_string())));
        assert!(tags.contains(&(String::new(), "1girl".to_string())));
        assert!(tags.contains(&("meta".to_string(), "highres".to_string())));
    }

    #[test]
    fn test_parse_e621_tags() {
        let json = serde_json::json!({
            "id": 67890,
            "tags": {
                "general": ["anthro", "solo"],
                "artist": ["artist_x"],
                "character": ["char_y"],
                "copyright": ["series_z"],
                "species": ["canine"],
                "meta": ["hi_res"]
            }
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 7); // 2 + 1 + 1 + 1 + 1 + 1
        assert!(tags.contains(&("creator".to_string(), "artist_x".to_string())));
        assert!(tags.contains(&("species".to_string(), "canine".to_string())));
        assert!(tags.contains(&(String::new(), "anthro".to_string())));
    }

    #[test]
    fn test_parse_pixiv_tags() {
        let json = serde_json::json!({
            "id": 99999,
            "tags": [
                {"name": "オリジナル", "translated_name": "original"},
                {"name": "女の子", "translated_name": "girl"},
                {"name": "風景", "translated_name": null}
            ]
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&(String::new(), "オリジナル".to_string())));
        assert!(tags.contains(&(String::new(), "女の子".to_string())));
        assert!(tags.contains(&(String::new(), "風景".to_string())));
    }

    #[test]
    fn test_parse_flat_array_tags() {
        let json = serde_json::json!({
            "id": 111,
            "tags": ["tag_a", "tag_b", "tag_c"]
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&(String::new(), "tag_a".to_string())));
    }

    #[test]
    fn test_parse_space_separated_tags() {
        let json = serde_json::json!({
            "id": 222,
            "tags": "alpha beta gamma"
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&(String::new(), "beta".to_string())));
    }

    #[test]
    fn test_parse_tag_string_fallback() {
        let json = serde_json::json!({
            "id": 333,
            "tag_string": "1girl smile"
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&(String::new(), "1girl".to_string())));
        assert!(tags.contains(&(String::new(), "smile".to_string())));
    }

    #[test]
    fn test_parse_metadata_description() {
        let json = serde_json::json!({
            "id": 1,
            "description": "A beautiful scene",
            "file_url": "https://example.com/img.jpg",
            "title": "Sunset",
            "category": "danbooru"
        });
        let meta = parse_metadata(&json);
        assert_eq!(meta.description.as_deref(), Some("A beautiful scene"));
        assert_eq!(
            meta.source_url.as_deref(),
            Some("https://example.com/img.jpg")
        );
        assert_eq!(meta.title.as_deref(), Some("Sunset"));
        assert_eq!(meta.post_id.as_deref(), Some("1"));
        assert_eq!(meta.category.as_deref(), Some("danbooru"));
    }

    #[test]
    fn test_parse_metadata_pixiv_caption() {
        let json = serde_json::json!({
            "id": 2,
            "caption": "<p>Some HTML caption</p>",
            "url": "https://i.pximg.net/img/12345.jpg"
        });
        let meta = parse_metadata(&json);
        assert_eq!(
            meta.description.as_deref(),
            Some("<p>Some HTML caption</p>")
        );
        assert_eq!(
            meta.source_url.as_deref(),
            Some("https://i.pximg.net/img/12345.jpg")
        );
    }

    #[test]
    fn test_parse_metadata_artist_commentary() {
        // Danbooru with metadata: true provides artist_commentary object
        let json = serde_json::json!({
            "id": 10873290,
            "tag_string_artist": "h4sh1rnoto",
            "tag_string_general": "1girl blonde_hair",
            "tag_string_character": "princess_peach",
            "tag_string_copyright": "mario_(series)",
            "tag_string_meta": "highres",
            "artist_commentary": {
                "original_title": "ピーチ姫",
                "original_description": "マリオシリーズ\r\n#イラスト #illustration",
                "translated_title": "",
                "translated_description": ""
            },
            "file_url": "https://cdn.donmai.us/original/test.jpg",
            "category": "danbooru"
        });
        let meta = parse_metadata(&json);
        assert_eq!(meta.title.as_deref(), Some("ピーチ姫"));
        assert_eq!(
            meta.description.as_deref(),
            Some("マリオシリーズ\r\n#イラスト #illustration")
        );
        assert_eq!(meta.post_id.as_deref(), Some("10873290"));
    }

    #[test]
    fn test_parse_metadata_artist_commentary_empty_falls_back() {
        // When artist_commentary fields are empty, fall back to direct fields
        let json = serde_json::json!({
            "id": 1,
            "artist_commentary": {
                "original_title": "",
                "original_description": ""
            },
            "description": "A direct description",
            "title": "Direct title",
            "category": "danbooru"
        });
        let meta = parse_metadata(&json);
        assert_eq!(meta.title.as_deref(), Some("Direct title"));
        assert_eq!(meta.description.as_deref(), Some("A direct description"));
    }

    #[test]
    fn test_substitute_query() {
        assert_eq!(
            substitute_query(
                "https://danbooru.donmai.us/posts?tags={query}",
                "1girl solo"
            ),
            "https://danbooru.donmai.us/posts?tags=1girl+solo"
        );
        assert_eq!(
            substitute_query(
                "https://e621.net/posts?tags={query}",
                "rating:safe order:score"
            ),
            "https://e621.net/posts?tags=rating%3Asafe+order%3Ascore"
        );
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://danbooru.donmai.us/posts?tags=1girl"),
            Some("danbooru.donmai.us".to_string())
        );
        assert_eq!(
            extract_domain("https://www.pixiv.net/artworks/12345"),
            Some("www.pixiv.net".to_string())
        );
        assert_eq!(extract_domain("not-a-url"), None);
    }

    #[test]
    fn test_config_generation() {
        let opts = RunOptions {
            url: "https://example.com".into(),
            file_limit: Some(50),
            abort_threshold: Some(10),
            sleep_request: 2.5,
            credential: None,
            archive_path: PathBuf::from("/tmp/archive.sqlite3"),
            archive_prefix: None,
            cancel: CancellationToken::new(),
        };
        let config = build_config(&opts, Path::new("/tmp"));
        let extractor = config.get("extractor").unwrap();
        assert_eq!(
            extractor.get("sleep-request").unwrap().as_f64().unwrap(),
            2.5
        );
        assert_eq!(extractor.get("metadata").unwrap().as_bool().unwrap(), true);
    }

    #[test]
    fn test_config_with_archive_prefix() {
        let opts = RunOptions {
            url: "https://example.com".into(),
            file_limit: None,
            abort_threshold: None,
            sleep_request: 1.0,
            credential: None,
            archive_path: PathBuf::from("/tmp/archive.sqlite3"),
            archive_prefix: Some("picto_s1_q2_".to_string()),
            cancel: CancellationToken::new(),
        };
        let config = build_config(&opts, Path::new("/tmp"));
        let extractor = config.get("extractor").unwrap();
        assert_eq!(
            extractor
                .get("archive-prefix")
                .and_then(|v| v.as_str())
                .unwrap(),
            "picto_s1_q2_"
        );
    }

    #[test]
    fn test_config_with_auth() {
        let cred = SiteCredential {
            site_category: "danbooru".into(),
            credential_type: crate::credential_store::CredentialType::UsernamePassword,
            username: Some("user".into()),
            password: Some("apikey123".into()),
            cookies: None,
            oauth_token: None,
        };
        let opts = RunOptions {
            url: "https://danbooru.donmai.us/posts".into(),
            file_limit: None,
            abort_threshold: None,
            sleep_request: 1.0,
            credential: Some(cred),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: CancellationToken::new(),
        };
        let config = build_config(&opts, Path::new("/tmp"));
        let danbooru = config.get("extractor").unwrap().get("danbooru").unwrap();
        assert_eq!(danbooru.get("username").unwrap().as_str().unwrap(), "user");
        assert_eq!(
            danbooru.get("password").unwrap().as_str().unwrap(),
            "apikey123"
        );
    }

    #[test]
    fn test_site_registry_has_entries() {
        assert!(!SITES.is_empty());
        assert!(SITES.len() >= 10, "Should have at least 10 built-in sites");
    }

    #[test]
    fn test_site_by_id() {
        let dan = site_by_id("danbooru").unwrap();
        assert_eq!(dan.name, "Danbooru");
        assert!(dan.url_template.contains("{query}"));

        assert!(site_by_id("nonexistent_site_xyz").is_none());
        assert_eq!(site_by_id("rule34xxx").unwrap().id, "rule34");
        assert_eq!(canonical_site_id("rule34xxx"), "rule34");
    }

    #[test]
    fn test_build_url() {
        assert_eq!(
            build_url("danbooru", "1girl solo").unwrap(),
            "https://danbooru.donmai.us/posts?tags=1girl+solo"
        );
        assert_eq!(
            build_url("e621", "canine rating:safe").unwrap(),
            "https://e621.net/posts?tags=canine+rating%3Asafe"
        );
        assert_eq!(
            build_url("pixiv", "風景").unwrap(),
            "https://www.pixiv.net/en/tags/%E9%A2%A8%E6%99%AF/artworks?s_mode=s_tag"
        );
        assert!(build_url("nonexistent", "query").is_none());
    }

    #[test]
    fn test_all_sites_have_query_placeholder() {
        for site in SITES {
            assert!(
                site.url_template.contains("{query}"),
                "Site '{}' URL template missing {{query}} placeholder: {}",
                site.id,
                site.url_template,
            );
        }
    }

    #[test]
    fn test_site_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for site in SITES {
            assert!(seen.insert(site.id), "Duplicate site ID: {}", site.id);
        }
    }

    #[test]
    fn test_classify_failure_unauthorized() {
        let kind = classify_failure("HTTP Error 403 Forbidden: Login required");
        assert_eq!(kind, FailureKind::Unauthorized);
    }

    #[test]
    fn test_classify_failure_authrequired_marker() {
        let kind = classify_failure(
            "[rule34][error] AuthRequired: 'api-key' & 'user-id' needed ('Missing authentication')",
        );
        assert_eq!(kind, FailureKind::Unauthorized);
    }

    #[test]
    fn test_classify_failure_expired() {
        let kind = classify_failure("refresh token expired and session invalid");
        assert_eq!(kind, FailureKind::Expired);
    }

    #[test]
    fn test_classify_failure_rate_limited() {
        let kind = classify_failure("HTTP Error 429 Too Many Requests");
        assert_eq!(kind, FailureKind::RateLimited);
    }

    #[test]
    fn test_classify_failure_network() {
        let kind = classify_failure("Connection reset by peer while downloading");
        assert_eq!(kind, FailureKind::Network);
    }

    #[test]
    fn test_config_with_cookie_auth() {
        let mut cookies = std::collections::HashMap::new();
        cookies.insert("sessionid".to_string(), "abc".to_string());
        let cred = SiteCredential {
            site_category: "pixiv".into(),
            credential_type: crate::credential_store::CredentialType::Cookies,
            username: None,
            password: None,
            cookies: Some(cookies),
            oauth_token: None,
        };
        let opts = RunOptions {
            url: "https://www.pixiv.net/en/tags/test/artworks?s_mode=s_tag".into(),
            file_limit: None,
            abort_threshold: None,
            sleep_request: 1.0,
            credential: Some(cred),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: CancellationToken::new(),
        };
        let config = build_config(&opts, Path::new("/tmp"));
        let auth = config.get("extractor").unwrap().get("pixiv").unwrap();
        assert_eq!(
            auth.get("cookies")
                .and_then(|v| v.get("sessionid"))
                .and_then(|v| v.as_str()),
            Some("abc")
        );
    }

    #[test]
    fn test_config_with_api_key_auth() {
        let cred = SiteCredential {
            site_category: "danbooru".into(),
            credential_type: crate::credential_store::CredentialType::ApiKey,
            username: None,
            password: Some("apikey123".into()),
            cookies: None,
            oauth_token: None,
        };
        let opts = RunOptions {
            url: "https://danbooru.donmai.us/posts".into(),
            file_limit: None,
            abort_threshold: None,
            sleep_request: 1.0,
            credential: Some(cred),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: CancellationToken::new(),
        };
        let config = build_config(&opts, Path::new("/tmp"));
        let auth = config.get("extractor").unwrap().get("danbooru").unwrap();
        assert_eq!(
            auth.get("api-key").and_then(|v| v.as_str()),
            Some("apikey123")
        );
    }

    #[test]
    fn test_config_with_oauth_auth() {
        let cred = SiteCredential {
            site_category: "fanbox".into(),
            credential_type: crate::credential_store::CredentialType::OAuthToken,
            username: None,
            password: None,
            cookies: None,
            oauth_token: Some("refresh-token-xyz".into()),
        };
        let opts = RunOptions {
            url: "https://x.fanbox.cc".into(),
            file_limit: None,
            abort_threshold: None,
            sleep_request: 1.0,
            credential: Some(cred),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: CancellationToken::new(),
        };
        let config = build_config(&opts, Path::new("/tmp"));
        let auth = config.get("extractor").unwrap().get("fanbox").unwrap();
        assert_eq!(
            auth.get("refresh-token").and_then(|v| v.as_str()),
            Some("refresh-token-xyz")
        );
    }

    #[test]
    fn test_site_capability_contract_representative_matrix() {
        let pixiv = site_by_id("pixiv").expect("pixiv");
        assert!(pixiv.supports_query);
        assert!(pixiv.supports_account);
        assert!(pixiv.auth_supported);
        assert!(pixiv.auth_required_for_full_access);

        let tumblr = site_by_id("tumblr").expect("tumblr");
        assert!(tumblr.supports_query);
        assert!(tumblr.supports_account);
        assert!(!tumblr.auth_supported);
        assert!(!tumblr.auth_required_for_full_access);

        let patreon = site_by_id("patreon").expect("patreon");
        assert!(!patreon.supports_query);
        assert!(patreon.supports_account);
        assert!(patreon.auth_supported);
    }

    #[test]
    fn test_site_contract_auth_required_implies_auth_supported() {
        for site in SITES {
            assert!(
                !site.auth_required_for_full_access || site.auth_supported,
                "site {} requires auth for full access but is marked auth unsupported",
                site.id
            );
        }
    }

    #[test]
    fn test_build_url_contract_for_query_and_account_templates() {
        assert_eq!(
            build_url("patreon", "creatorname").as_deref(),
            Some("https://www.patreon.com/creatorname/posts")
        );
        assert_eq!(
            build_url("tumblr", "myblog").as_deref(),
            Some("https://myblog.tumblr.com")
        );
        assert_eq!(
            build_url("rule34xxx", "solo").as_deref(),
            Some("https://rule34.xxx/index.php?page=post&s=list&tags=solo")
        );
    }

    #[test]
    fn test_parse_metadata_extracts_pixiv_creator_tag() {
        let json = serde_json::json!({
            "id": 100,
            "title": "Pixiv work",
            "url": "https://www.pixiv.net/artworks/100",
            "tags": [{"name":"landscape","translated_name":null}],
            "user": {"id": 77, "name": "artist_name"},
            "page_count": 1,
            "category": "pixiv"
        });
        let meta = parse_metadata(&json);
        assert!(meta
            .tags
            .iter()
            .any(|(ns, subtag)| ns == "creator" && subtag == "artist_name"));
    }

    #[test]
    fn test_validate_site_metadata_pixiv_valid_payload() {
        let json = serde_json::json!({
            "id": 123,
            "title": "Pixiv title",
            "caption": "Pixiv caption",
            "url": "https://www.pixiv.net/artworks/123",
            "tags": [{"name":"tag_a","translated_name":null}],
            "user": {"id": 55, "name": "pixiv_user"},
            "page_count": 3,
            "category": "pixiv"
        });
        let res =
            validate_site_metadata("pixiv", "https://www.pixiv.net/artworks/123", Some(&json));
        assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
        assert!(res.missing_required_fields.is_empty());
        assert!(res.invalid_fields.is_empty());
        assert!(res.normalized_preview.is_some());
    }

    #[test]
    fn test_validate_site_metadata_pixiv_missing_required_keys() {
        let json = serde_json::json!({
            "id": 123,
            "tags": [],
            "user": {},
            "category": "pixiv"
        });
        let res = validate_site_metadata("pixiv", "", Some(&json));
        assert!(!res.valid);
        assert!(res
            .missing_required_fields
            .contains(&"title|caption".to_string()));
        assert!(res
            .missing_required_fields
            .contains(&"page_count|meta_pages".to_string()));
        assert!(res
            .missing_required_fields
            .contains(&"url|file_url".to_string()));
    }

    #[test]
    fn test_validate_site_metadata_gelbooru_valid_payload() {
        let json = serde_json::json!({
            "id": 42,
            "tag_string": "1girl smile",
            "file_url": "https://img3.gelbooru.com/images/a/b/example.jpg",
            "source": "https://twitter.com/example/status/1",
            "rating": "safe",
            "md5": "0123456789abcdef0123456789abcdef",
            "category": "gelbooru"
        });
        let res = validate_site_metadata(
            "gelbooru",
            "https://gelbooru.com/index.php?page=post&s=view&id=42",
            Some(&json),
        );
        assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
        assert!(res.missing_required_fields.is_empty());
        assert!(res.invalid_fields.is_empty());
        assert!(res.normalized_preview.is_some());
    }

    #[test]
    fn test_get_site_metadata_schema_gelbooru() {
        let schema = get_site_metadata_schema("gelbooru").expect("gelbooru schema");
        assert_eq!(schema.site_id, "gelbooru");
        assert!(
            schema
                .required_raw_keys
                .iter()
                .any(|k| k == "tags|tag_string"),
            "schema should accept tags or tag_string"
        );
    }

    #[test]
    fn test_validate_site_metadata_gelbooru_missing_required_keys() {
        let json = serde_json::json!({
            "id": 42,
            "tag_string": "",
            "rating": "safe",
            "category": "gelbooru"
        });
        let res = validate_site_metadata("gelbooru", "", Some(&json));
        assert!(!res.valid);
        assert!(res
            .missing_required_fields
            .contains(&"file_url".to_string()));
        assert!(res.missing_required_fields.contains(&"source".to_string()));
        assert!(res.invalid_fields.contains(&"tags[]".to_string()));
    }

    #[test]
    fn test_get_site_metadata_schema_danbooru() {
        let schema = get_site_metadata_schema("danbooru").expect("danbooru schema");
        assert_eq!(schema.site_id, "danbooru");
        assert!(
            schema
                .required_raw_keys
                .iter()
                .any(|k| k == "tags_artist|tags_general|category_tags"),
            "schema should require category tags"
        );
    }

    #[test]
    fn test_validate_site_metadata_danbooru_valid_payload() {
        let json = serde_json::json!({
            "id": 10873290,
            "tags_artist": ["h4sh1rnoto"],
            "tags_character": ["princess_peach"],
            "tags_copyright": ["mario_(series)"],
            "tags_general": ["1girl", "blonde_hair"],
            "tags_meta": ["highres"],
            "artist_commentary": {
                "original_title": "ピーチ姫",
                "original_description": "マリオシリーズ"
            },
            "file_url": "https://cdn.donmai.us/original/test.jpg",
            "source": "https://x.com/example/status/1",
            "rating": "s",
            "category": "danbooru"
        });
        let res = validate_site_metadata(
            "danbooru",
            "https://danbooru.donmai.us/posts/10873290",
            Some(&json),
        );
        assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
        assert!(res.missing_required_fields.is_empty());
        assert!(res.invalid_fields.is_empty());
        assert!(res.normalized_preview.is_some());
    }

    #[test]
    fn test_validate_site_metadata_danbooru_missing_required_keys() {
        let json = serde_json::json!({
            "id": 10873290,
            "tags_general": ["1girl"],
            "file_url": "https://cdn.donmai.us/original/test.jpg",
            "category": "danbooru"
        });
        let res = validate_site_metadata("danbooru", "", Some(&json));
        assert!(!res.valid);
        assert!(res.missing_required_fields.contains(&"source".to_string()));
        assert!(res.missing_required_fields.contains(&"rating".to_string()));
        assert!(res.invalid_fields.contains(&"creator".to_string()));
    }
}
