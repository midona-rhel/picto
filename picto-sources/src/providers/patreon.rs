use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

type MediaCandidate = (Option<String>, String, Option<String>, Option<u64>);

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: OpaqueCursor = OpaqueCursor::new(16_384);
const DOMAIN: &str = "patreon.com";

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    PatreonSource
}

struct PatreonSource;

impl NativeSourceAdapter for PatreonSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "patreon",
            display_name: "Patreon",
            domain: DOMAIN,
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_creator(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let creator = normalize_creator(&request.query)?;
            let credentials = api_credentials(credentials);
            let url = match request
                .cursor
                .as_deref()
                .filter(|cursor| !cursor.is_empty())
            {
                Some(cursor) => validate_cursor(cursor)?,
                None => {
                    let html = http
                        .get_text(creator_url(&creator), &credentials, cancel)
                        .await?;
                    let campaign_id = extract_campaign_id(&html)?;
                    posts_url(&campaign_id)
                }
            };
            let response = http.get_json::<Value>(url, &credentials, cancel).await?;
            normalize_response(request, &creator, response)
        })
    }
}

fn api_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert(DOMAIN.to_string());
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| {
            "application/vnd.api+json, application/json, text/plain, */*".to_string()
        });
    credentials
        .headers
        .entry("User-Agent".to_string())
        .or_insert_with(|| "Patreon/126.24.0.9 (Android; Android 14; Scale/2.10)".to_string());
    credentials
}

fn creator_url(creator: &str) -> Url {
    let mut url = Url::parse("https://www.patreon.com").expect("static Patreon URL");
    url.path_segments_mut()
        .expect("Patreon URL supports path segments")
        .extend(["c", creator]);
    url
}

fn posts_url(campaign_id: &str) -> Url {
    let mut url =
        Url::parse("https://www.patreon.com/api/posts").expect("static Patreon posts endpoint");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair(
            "include",
            "campaign,attachments,attachments_media,audio,images,media,user,user_defined_tags",
        );
        query.append_pair(
            "fields[post]",
            "content,content_json_string,current_user_can_view,image,post_file,published_at,patreon_url,teaser_text,title,url",
        );
        query.append_pair("fields[post_tag]", "tag_type,value");
        query.append_pair("fields[user]", "full_name,url");
        query.append_pair(
            "fields[media]",
            "id,image_urls,download_url,metadata,file_name,url,name",
        );
        query.append_pair("filter[campaign_id]", campaign_id);
        query.append_pair("filter[contains_exclusive_posts]", "true");
        query.append_pair("filter[is_draft]", "false");
        query.append_pair("page[count]", "1");
        query.append_pair("sort", "-published_at");
        query.append_pair("json-api-version", "1.0");
    }
    url
}

fn validate_cursor(raw: &str) -> Result<Url, SourceError> {
    CURSOR.validate(raw)?;
    let url = Url::parse(raw).map_err(|_| invalid_cursor())?;
    let has_campaign = url
        .query_pairs()
        .any(|(key, value)| key == "filter[campaign_id]" && valid_numeric_id(&value));
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("patreon.com" | "www.patreon.com"))
        || url.path() != "/api/posts"
        || !has_campaign
    {
        return Err(invalid_cursor());
    }
    Ok(url)
}

fn extract_campaign_id(html: &str) -> Result<String, SourceError> {
    static CAMPAIGN_PATH: OnceLock<Regex> = OnceLock::new();
    static CAMPAIGN_JSON: OnceLock<Regex> = OnceLock::new();
    let candidate = CAMPAIGN_PATH
        .get_or_init(|| Regex::new(r#"/campaign/(\d{1,32})/"#).expect("valid campaign regex"))
        .captures(html)
        .and_then(|captures| captures.get(1))
        .or_else(|| {
            CAMPAIGN_JSON
                .get_or_init(|| {
                    Regex::new(r#"(?s)campaign.{0,80}?data.{0,40}?id\\?\"\s*:\s*\\?\"(\d{1,32})"#)
                        .expect("valid campaign JSON regex")
                })
                .captures(html)
                .and_then(|captures| captures.get(1))
        })
        .map(|value| value.as_str().to_string());
    candidate.ok_or_else(|| {
        SourceError::new(
            SourceErrorKind::InvalidResponse,
            "Patreon creator page did not expose a campaign ID",
            false,
        )
    })
}

fn normalize_response(
    request: &DiscoveryRequest,
    creator: &str,
    response: Value,
) -> Result<DiscoveryBatch, SourceError> {
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("Patreon response is missing its posts"))?;
    if data.len() > 1 {
        return Err(invalid_response(
            "Patreon returned more than one post for a one-post request",
        ));
    }
    let next = response
        .pointer("/links/next")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| validate_cursor(value).map(|url| url.to_string()))
        .transpose()?;
    let exhausted = data.is_empty() || next.is_none();
    let Some(post) = data.first() else {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    };
    let included = Included::new(response.get("included"));
    Ok(DiscoveryBatch {
        posts: vec![normalize_post(request, creator, post, &included, next)?],
        exhausted,
    })
}

fn normalize_post(
    request: &DiscoveryRequest,
    creator_slug: &str,
    post: &Value,
    included: &Included<'_>,
    next: Option<String>,
) -> Result<SourcePost, SourceError> {
    let stable_id = post
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_numeric_id(id))
        .ok_or_else(|| invalid_response("Patreon post has an invalid ID"))?;
    let attributes = post
        .get("attributes")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_response("Patreon post is missing its attributes"))?;
    let canonical_url = canonical_post_url(attributes, stable_id);
    let current_user_can_view = attributes
        .get("current_user_can_view")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let content = post_content(attributes);
    let creator = relationship(post, "user")
        .and_then(|reference| included.get(reference.kind, reference.id))
        .and_then(|user| user.get("attributes"))
        .and_then(|attributes| creator_name(attributes, creator_slug))
        .unwrap_or_else(|| creator_slug.to_string());

    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", creator_slug);
    if let Some(tag_refs) = relationship_many(post, "user_defined_tags") {
        for reference in tag_refs {
            let value = included
                .get(reference.kind, reference.id)
                .and_then(|tag| tag.pointer("/attributes/value"))
                .and_then(Value::as_str)
                .or_else(|| reference.id.strip_prefix("user_defined;"));
            if let Some(value) = value {
                tags.insert("", value);
            }
        }
    }

    Ok(SourcePost {
        site_id: "patreon".to_string(),
        partition: request.partition.clone(),
        stable_id: stable_id.to_string(),
        canonical_url: Some(canonical_url.clone()),
        creator: Some(creator),
        name: object_text(attributes, "title").and_then(normalize_source_text),
        notes: content.notes,
        created_at: object_text(attributes, "published_at").map(ToOwned::to_owned),
        tags: tags.into_vec(),
        media: if current_user_can_view {
            post_media(
                post,
                attributes,
                included,
                stable_id,
                &canonical_url,
                &content.inline_media,
            )?
        } else {
            Vec::new()
        },
        resume_cursor_after: next,
    })
}

fn canonical_post_url(attributes: &serde_json::Map<String, Value>, post_id: &str) -> String {
    for key in ["patreon_url", "url"] {
        let Some(raw) = object_text(attributes, key) else {
            continue;
        };
        let Ok(mut url) = Url::parse(raw) else {
            continue;
        };
        if matches!(url.scheme(), "http" | "https")
            && matches!(url.host_str(), Some("patreon.com" | "www.patreon.com"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
        {
            let _ = url.set_scheme("https");
            let _ = url.set_host(Some("www.patreon.com"));
            url.set_query(None);
            url.set_fragment(None);
            return url.to_string();
        }
    }
    format!("https://www.patreon.com/posts/{post_id}")
}

fn post_media(
    post: &Value,
    attributes: &serde_json::Map<String, Value>,
    included: &Included<'_>,
    post_id: &str,
    canonical_url: &str,
    inline_media: &[String],
) -> Result<Vec<crate::MediaDescriptor>, SourceError> {
    let mut candidates = Vec::new();
    for relationship_name in ["images", "attachments", "attachments_media", "media"] {
        if let Some(references) = relationship_many(post, relationship_name) {
            for reference in references {
                if let Some(item) = included.get(reference.kind, reference.id) {
                    push_media_candidate(&mut candidates, item, Some(reference.id));
                }
            }
        }
    }
    for key in ["post_file", "image"] {
        if let Some(value) = attributes.get(key) {
            push_media_candidate(&mut candidates, value, None);
        }
    }
    for url in inline_media {
        candidates.push((None, url.clone(), None, None));
    }

    let mut media = Vec::new();
    let mut seen = BTreeSet::new();
    let headers = BTreeMap::from([(
        "Referer".to_string(),
        "https://www.patreon.com/".to_string(),
    )]);
    for (source_id, raw_url, raw_name, expected_size) in candidates {
        let Some(url) = canonical_media_url(&raw_url) else {
            continue;
        };
        let dedup_key = patreon_file_hash(&url)
            .map(|hash| format!("hash:{hash}"))
            .unwrap_or_else(|| format!("url:{url}"));
        if !seen.insert(dedup_key) {
            continue;
        }
        let manifest = url.to_ascii_lowercase().ends_with(".m3u8");
        let file_name = if manifest {
            format!("patreon_{post_id}_{}.mp4", media.len())
        } else {
            raw_name
                .filter(|name| !name.trim().is_empty())
                .or_else(|| file_name_from_url(&url))
                .unwrap_or_else(|| format!("patreon_{post_id}_{}", media.len()))
        };
        if crate::media::is_unsupported_archive(&file_name)
            || crate::media::is_unsupported_archive(&url)
        {
            continue;
        }
        let stable_id = source_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("patreon:{post_id}:{id}"))
            .unwrap_or_else(|| format!("patreon:{post_id}:{}", media.len()));
        media.push(
            MediaDescriptorBuilder::new(stable_id, media.len() as u32, url)
                .canonical_url(canonical_url)
                .file_name(file_name)
                .expected_size(expected_size)
                .headers(headers.clone())
                .build(),
        );
    }
    Ok(media)
}

#[derive(Default)]
struct PostContent {
    notes: Option<String>,
    inline_media: Vec<String>,
}

fn post_content(attributes: &serde_json::Map<String, Value>) -> PostContent {
    if let Some(content) = object_text(attributes, "content") {
        let inline_media = content_media_regex()
            .captures_iter(content)
            .filter_map(|captures| captures.get(1))
            .map(|url| decode_html(url.as_str()))
            .collect();
        return PostContent {
            notes: normalize_source_text(content),
            inline_media,
        };
    }
    if let Some(raw) = object_text(attributes, "content_json_string") {
        if let Some(content) = parse_tiptap_content(raw) {
            return content;
        }
    }
    PostContent {
        notes: object_text(attributes, "teaser_text").and_then(normalize_source_text),
        inline_media: Vec::new(),
    }
}

fn parse_tiptap_content(raw: &str) -> Option<PostContent> {
    let root: Value = serde_json::from_str(raw).ok()?;
    let mut text = String::new();
    let mut inline_media = Vec::new();
    let mut nodes = 0_usize;
    tiptap_node(&root, 0, &mut nodes, &mut text, &mut inline_media);
    Some(PostContent {
        notes: normalize_source_text(&text),
        inline_media,
    })
}

fn tiptap_node(
    node: &Value,
    depth: usize,
    nodes: &mut usize,
    text: &mut String,
    inline_media: &mut Vec<String>,
) {
    if depth >= 64 || *nodes >= 4_096 {
        return;
    }
    *nodes += 1;
    let kind = node.get("type").and_then(Value::as_str).unwrap_or("doc");
    match kind {
        "text" => {
            if let Some(value) = node.get("text").and_then(Value::as_str) {
                text.push_str(value);
            }
        }
        "image" => {
            if let Some(url) = node.pointer("/attrs/src").and_then(Value::as_str) {
                inline_media.push(url.to_string());
            }
        }
        "hardBreak" => text.push('\n'),
        "horizontalRule" => text.push('\n'),
        "doc" | "paragraph" | "heading" | "listItem" | "bulletList" | "orderedList"
        | "blockquote" | "link" => {
            if let Some(children) = node.get("content").and_then(Value::as_array) {
                for child in children {
                    tiptap_node(child, depth + 1, nodes, text, inline_media);
                }
            }
            if matches!(
                kind,
                "paragraph" | "heading" | "listItem" | "bulletList" | "orderedList" | "blockquote"
            ) {
                text.push('\n');
            }
        }
        _ => {}
    }
}

fn patreon_file_hash(raw: &str) -> Option<&str> {
    let path = raw.split('?').next().unwrap_or(raw);
    path.rsplit('/').find(|component| component.len() == 32)
}

fn push_media_candidate(
    candidates: &mut Vec<MediaCandidate>,
    value: &Value,
    fallback_id: Option<&str>,
) {
    let attributes = value.get("attributes").unwrap_or(value);
    let url = attributes
        .pointer("/image_urls/original")
        .and_then(Value::as_str)
        .or_else(|| value_text(attributes, "download_url"))
        .or_else(|| value_text(attributes, "url"))
        .or_else(|| value_text(attributes, "large_url"))
        .or_else(|| {
            attributes
                .pointer("/image_urls/large")
                .and_then(Value::as_str)
        });
    let Some(url) = url else { return };
    let source_id = value_text(attributes, "id")
        .or(fallback_id)
        .map(ToOwned::to_owned);
    let name = ["file_name", "name"]
        .into_iter()
        .find_map(|key| value_text(attributes, key))
        .map(ToOwned::to_owned);
    let expected_size = attributes
        .get("size_bytes")
        .or_else(|| attributes.pointer("/metadata/size_bytes"))
        .and_then(Value::as_u64);
    candidates.push((source_id, url.to_string(), name, expected_size));
}

#[derive(Clone, Copy)]
struct RelationshipRef<'a> {
    kind: &'a str,
    id: &'a str,
}

fn relationship<'a>(post: &'a Value, name: &str) -> Option<RelationshipRef<'a>> {
    let value = post.pointer(&format!("/relationships/{name}/data"))?;
    relationship_ref(value)
}

fn relationship_many<'a>(post: &'a Value, name: &str) -> Option<Vec<RelationshipRef<'a>>> {
    let values = post
        .pointer(&format!("/relationships/{name}/data"))?
        .as_array()?;
    Some(values.iter().filter_map(relationship_ref).collect())
}

fn relationship_ref(value: &Value) -> Option<RelationshipRef<'_>> {
    Some(RelationshipRef {
        kind: value.get("type")?.as_str()?,
        id: value.get("id")?.as_str()?,
    })
}

struct Included<'a>(BTreeMap<(&'a str, &'a str), &'a Value>);

impl<'a> Included<'a> {
    fn new(value: Option<&'a Value>) -> Self {
        let mut values = BTreeMap::new();
        for item in value.and_then(Value::as_array).into_iter().flatten() {
            if let (Some(kind), Some(id)) = (
                item.get("type").and_then(Value::as_str),
                item.get("id").and_then(Value::as_str),
            ) {
                values.insert((kind, id), item);
            }
        }
        Self(values)
    }

    fn get(&self, kind: &str, id: &str) -> Option<&'a Value> {
        self.0.get(&(kind, id)).copied()
    }
}

fn creator_name(attributes: &Value, fallback: &str) -> Option<String> {
    value_text(attributes, "url")
        .and_then(|url| normalize_creator(url).ok())
        .or_else(|| value_text(attributes, "full_name").map(ToOwned::to_owned))
        .or_else(|| Some(fallback.to_string()))
}

fn normalize_creator(raw: &str) -> Result<String, SourceError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_query(
            "Patreon subscriptions require a creator slug",
        ));
    }
    let creator = if let Ok(url) = Url::parse(raw) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(url.host_str(), Some("patreon.com" | "www.patreon.com"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query("Patreon requires a canonical creator URL"));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            ["c", creator] | [creator] | [creator, "posts"] => (*creator).to_string(),
            _ => return Err(invalid_query("Patreon requires a creator profile URL")),
        }
    } else {
        raw.to_string()
    };
    if creator.is_empty()
        || creator.len() > 128
        || !creator
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid_query("Patreon requires a safe creator slug"));
    }
    Ok(creator)
}

fn content_media_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)<(?:img|video)[^>]+(?:src|data-src)=\"([^\"]+)\""#)
            .expect("valid Patreon content media regex")
    })
}

fn canonical_media_url(raw: &str) -> Option<String> {
    let raw = decode_html(raw);
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw
    };
    let url = Url::parse(&raw).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then(|| url.to_string())
}

fn file_name_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn decode_html(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn object_text<'a>(value: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn value_text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn valid_numeric_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_cursor() -> SourceError {
    invalid_query("invalid Patreon source cursor")
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "creator-name".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 1,
        }
    }

    #[test]
    fn accepts_only_creator_queries() {
        assert_eq!(normalize_creator("creator-name").unwrap(), "creator-name");
        assert_eq!(
            normalize_creator("https://www.patreon.com/c/creator-name").unwrap(),
            "creator-name"
        );
        assert!(normalize_creator("https://www.patreon.com/posts/123").is_err());
    }

    #[test]
    fn extracts_campaign_id_without_retaining_bootstrap_state() {
        let html = include_str!("../../tests/fixtures/patreon/creator.html");
        assert_eq!(extract_campaign_id(html).unwrap(), "987654");
    }

    #[test]
    fn maps_one_post_all_files_tags_and_cursor() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/patreon/posts.json")).unwrap();
        let batch = normalize_response(&request(), "creator-name", fixture).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(!batch.exhausted);
        let post = &batch.posts[0];
        assert_eq!(post.media.len(), 4);
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "creator-name")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("", "behind-the-scenes")));
        assert_eq!(post.notes.as_deref(), Some("Visible post prose"));
        assert!(validate_cursor(post.resume_cursor_after.as_deref().unwrap()).is_ok());
    }

    #[test]
    fn original_image_beats_generic_and_large_urls() {
        let value = serde_json::json!({
            "attributes": {
                "download_url": "https://cdn.example/download.jpg",
                "url": "https://cdn.example/preview.jpg",
                "large_url": "https://cdn.example/large.jpg",
                "image_urls": {
                    "original": "https://cdn.example/original.png",
                    "large": "https://cdn.example/image-large.jpg"
                }
            }
        });
        let mut candidates = Vec::new();
        push_media_candidate(&mut candidates, &value, None);
        assert_eq!(candidates[0].1, "https://cdn.example/original.png");
    }

    #[test]
    fn locked_post_metadata_never_publishes_preview_media() {
        let post = serde_json::json!({
            "id": "42",
            "attributes": {
                "title": "Locked post",
                "current_user_can_view": false,
                "image": { "url": "https://cdn.example/teaser.jpg" },
                "post_file": { "download_url": "https://cdn.example/locked.bin" }
            }
        });
        let included = Included::new(None);
        let post = normalize_post(&request(), "creator-name", &post, &included, None).unwrap();
        assert!(post.media.is_empty());
    }

    #[test]
    fn deduplicates_relationship_variants_by_patreon_file_hash() {
        let hash = "0123456789abcdef0123456789abcdef";
        let post = serde_json::json!({});
        let attributes = serde_json::json!({
            "content": format!(
                "<img src=\"https://c1.patreonusercontent.com/a/{hash}/first.jpg\"><img src=\"https://c2.patreonusercontent.com/b/{hash}/second.jpg\">"
            )
        });
        let content = post_content(attributes.as_object().unwrap());
        let media = post_media(
            &post,
            attributes.as_object().unwrap(),
            &Included::new(None),
            "42",
            "https://www.patreon.com/posts/42",
            &content.inline_media,
        )
        .unwrap();
        assert_eq!(media.len(), 1);
        assert_eq!(patreon_file_hash(&media[0].url), Some(hash));
    }

    #[test]
    fn retains_hls_video_for_the_segmented_downloader() {
        let post = serde_json::json!({});
        let attributes = serde_json::json!({
            "post_file": {
                "url": "https://c10.patreonusercontent.com/video/master.m3u8",
                "name": "video"
            }
        });
        let attributes = attributes.as_object().unwrap();
        let included = Included::new(None);
        let media = post_media(
            &post,
            attributes,
            &included,
            "42",
            "https://www.patreon.com/posts/42",
            &[],
        )
        .unwrap();
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].delivery(), crate::MediaDelivery::Hls);
        assert_eq!(media[0].file_name.as_deref(), Some("patreon_42_0.mp4"));
    }

    #[test]
    fn rejects_cross_domain_or_non_post_cursors() {
        assert!(validate_cursor("https://evil.test/api/posts?filter%5Bcampaign_id%5D=1").is_err());
        assert!(
            validate_cursor("https://www.patreon.com/api/users?filter%5Bcampaign_id%5D=1").is_err()
        );
    }

    #[test]
    fn content_json_string_preserves_tiptap_notes_and_inline_images() {
        let tiptap = serde_json::json!({
            "type": "doc",
            "content": [
                { "type": "heading", "attrs": { "level": 2 }, "content": [
                    { "type": "text", "text": "Structured title" }
                ]},
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "First line" },
                    { "type": "hardBreak" },
                    { "type": "text", "text": "second line" }
                ]},
                { "type": "image", "attrs": {
                    "src": "https://c10.patreonusercontent.com/full/original.png",
                    "media_id": "media-1"
                }},
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [
                            { "type": "text", "text": "List item" }
                        ]}
                    ]}
                ]},
                { "type": "blockquote", "content": [
                    { "type": "paragraph", "content": [
                        { "type": "text", "text": "Quote" }
                    ]}
                ]},
                { "type": "link", "attrs": { "href": "https://example.test" }, "content": [
                    { "type": "text", "text": " linked" }
                ]}
            ]
        });
        let post = serde_json::json!({
            "id": "42",
            "attributes": {
                "content_json_string": tiptap.to_string(),
                "current_user_can_view": true
            }
        });
        let normalized = normalize_post(
            &request(),
            "creator-name",
            &post,
            &Included::new(None),
            None,
        )
        .unwrap();
        assert_eq!(
            normalized.notes.as_deref(),
            Some("Structured title First line second line List item Quote linked")
        );
        assert_eq!(normalized.media.len(), 1);
        assert_eq!(
            normalized.media[0].url,
            "https://c10.patreonusercontent.com/full/original.png"
        );
    }

    #[test]
    fn malformed_tiptap_falls_back_to_the_teaser() {
        let attributes = serde_json::json!({
            "content_json_string": "{not-json",
            "teaser_text": "Visible teaser"
        });
        let content = post_content(attributes.as_object().unwrap());
        assert_eq!(content.notes.as_deref(), Some("Visible teaser"));
        assert!(content.inline_media.is_empty());
    }
}
