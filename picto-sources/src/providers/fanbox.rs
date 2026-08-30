use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, PostFuture,
    ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const API_DOMAIN: &str = "api.fanbox.cc";
const FIREFOX_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:140.0) Gecko/20100101 Firefox/140.0";
const CURSOR: OpaqueCursor = OpaqueCursor::new(2_048);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    FanboxSource
}

struct FanboxSource;

impl NativeSourceAdapter for FanboxSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "fanbox",
            display_name: "pixivFANBOX",
            domain: "fanbox.cc",
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
            let url = listing_url(request.cursor.as_deref(), &creator)?;
            let response = fanbox_json(http, url, &credentials, cancel).await?;
            normalize_listing(request, &creator, response)
        })
    }

    fn resolve_post<'a>(
        &'a self,
        post: SourcePost,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> PostFuture<'a> {
        Box::pin(async move {
            let credentials = api_credentials(credentials);
            let mut url =
                Url::parse("https://api.fanbox.cc/post.info").expect("static FANBOX post endpoint");
            url.query_pairs_mut().append_pair("postId", &post.stable_id);
            let response = fanbox_json(http, url, &credentials, cancel).await?;
            normalize_post(post, response)
        })
    }
}

async fn fanbox_json(
    http: &HttpRuntime,
    url: Url,
    credentials: &RequestCredentials,
    cancel: &CancellationToken,
) -> Result<Value, SourceError> {
    match http
        .get_json::<Value>(url.clone(), credentials, cancel)
        .await
    {
        Err(error)
            if error.kind == SourceErrorKind::Authentication && !credentials.cookies.is_empty() =>
        {
            let anonymous = api_credentials(&RequestCredentials::default());
            http.get_json::<Value>(url, &anonymous, cancel).await
        }
        result => result,
    }
}

fn api_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert("fanbox.cc".to_string());
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| "application/json, text/plain, */*".to_string());
    credentials
        .headers
        .entry("Origin".to_string())
        .or_insert_with(|| "https://www.fanbox.cc".to_string());
    if !credentials
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("user-agent"))
    {
        credentials
            .headers
            .insert("User-Agent".to_string(), FIREFOX_USER_AGENT.to_string());
    }
    for (name, value) in [
        ("Referer", "https://www.fanbox.cc/"),
        ("Sec-Fetch-Dest", "empty"),
        ("Sec-Fetch-Mode", "cors"),
        ("Sec-Fetch-Site", "same-site"),
    ] {
        credentials
            .headers
            .entry(name.to_string())
            .or_insert_with(|| value.to_string());
    }
    credentials
}

fn listing_url(cursor: Option<&str>, creator: &str) -> Result<Url, SourceError> {
    if let Some(cursor) = cursor.filter(|cursor| !cursor.is_empty()) {
        CURSOR.validate(cursor)?;
        let url = Url::parse(cursor).map_err(|_| invalid_cursor())?;
        if url.scheme() != "https"
            || url.host_str() != Some(API_DOMAIN)
            || url.path() != "/post.listCreator"
            || url
                .query_pairs()
                .find(|(key, _)| key == "creatorId")
                .is_none_or(|(_, value)| value != creator)
        {
            return Err(invalid_cursor());
        }
        return Ok(url);
    }

    let mut url = Url::parse("https://api.fanbox.cc/post.listCreator")
        .expect("static FANBOX listing endpoint");
    url.query_pairs_mut()
        .append_pair("creatorId", creator)
        .append_pair("limit", "1");
    Ok(url)
}

fn normalize_listing(
    request: &DiscoveryRequest,
    creator: &str,
    response: Value,
) -> Result<DiscoveryBatch, SourceError> {
    let body = response
        .get("body")
        .ok_or_else(|| invalid_response("FANBOX response is missing its body"))?;
    let posts = body
        .get("items")
        .or_else(|| body.get("posts"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("FANBOX response is missing its posts"))?;
    if posts.len() > 1 {
        return Err(invalid_response(
            "FANBOX returned more than one post for a one-post request",
        ));
    }
    let Some(summary) = posts.first() else {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    };
    let stable_id = required_id(summary, "FANBOX post")?;
    let response_creator = text(summary, "creatorId").unwrap_or(creator);
    if response_creator != creator {
        return Err(invalid_response(
            "FANBOX returned a post for a different creator",
        ));
    }
    let next_url = body
        .get("nextUrl")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| canonical_next_url(value, creator))
        .transpose()?;
    let canonical_url = format!("https://{creator}.fanbox.cc/posts/{stable_id}");
    let post = SourcePost {
        site_id: "fanbox".to_string(),
        partition: request.partition.clone(),
        stable_id: stable_id.to_string(),
        canonical_url: Some(canonical_url),
        creator: Some(creator.to_string()),
        name: text(summary, "title").and_then(normalize_source_text),
        notes: None,
        created_at: text(summary, "publishedDatetime").map(ToOwned::to_owned),
        tags: Vec::new(),
        media: Vec::new(),
        resume_cursor_after: next_url,
    };
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted: body
            .get("nextUrl")
            .is_none_or(|value| value.as_str().is_none_or(|value| value.trim().is_empty())),
    })
}

fn canonical_next_url(raw: &str, creator: &str) -> Result<String, SourceError> {
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let url =
        Url::parse(&raw).map_err(|_| invalid_response("FANBOX returned an invalid cursor"))?;
    if url.scheme() != "https"
        || url.host_str() != Some(API_DOMAIN)
        || url.path() != "/post.listCreator"
        || url
            .query_pairs()
            .find(|(key, _)| key == "creatorId")
            .is_none_or(|(_, value)| value != creator)
    {
        return Err(invalid_response("FANBOX returned an unsafe cursor"));
    }
    CURSOR.validate(url.as_str())?;
    Ok(url.to_string())
}

fn normalize_post(mut post: SourcePost, response: Value) -> Result<SourcePost, SourceError> {
    let value = response
        .pointer("/body/post")
        .or_else(|| response.get("body"))
        .ok_or_else(|| invalid_response("FANBOX post response is missing its body"))?;
    if required_id(value, "FANBOX post")? != post.stable_id {
        return Err(invalid_response("FANBOX resolved a different post"));
    }
    let creator = text(value, "creatorId")
        .or(post.creator.as_deref())
        .ok_or_else(|| invalid_response("FANBOX post is missing its creator"))?
        .to_string();
    if post
        .creator
        .as_deref()
        .is_some_and(|expected| expected != creator)
    {
        return Err(invalid_response("FANBOX resolved a different creator"));
    }
    let canonical_url = format!("https://{creator}.fanbox.cc/posts/{}", post.stable_id);
    post.canonical_url = Some(canonical_url.clone());
    post.creator = Some(creator.clone());
    post.name = text(value, "title").and_then(normalize_source_text);
    post.notes = post_notes(value);
    post.created_at = text(value, "publishedDatetime").map(ToOwned::to_owned);

    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &creator);
    for tag in value
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(tag) = tag.as_str() {
            tags.insert("", tag);
        }
    }
    post.tags = tags.into_vec();
    post.media = post_media(value, &post.stable_id, &canonical_url)?;
    Ok(post)
}

fn post_notes(post: &Value) -> Option<String> {
    let body = post.get("body");
    let mut parts = Vec::new();
    for value in [
        post.get("excerpt"),
        body.and_then(|body| body.get("text")),
        body.and_then(|body| body.get("html")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    {
        parts.push(value);
    }
    if let Some(blocks) = body
        .and_then(|body| body.get("blocks"))
        .and_then(Value::as_array)
    {
        for block in blocks {
            if let Some(value) = block.get("text").and_then(Value::as_str) {
                parts.push(value);
            }
            if let Some(links) = block.get("links").and_then(Value::as_array) {
                parts.extend(
                    links
                        .iter()
                        .filter_map(|link| link.get("url").and_then(Value::as_str)),
                );
            }
        }
    }
    normalize_source_text(&parts.join("\n"))
}

fn post_media(
    post: &Value,
    post_id: &str,
    canonical_url: &str,
) -> Result<Vec<crate::MediaDescriptor>, SourceError> {
    let mut candidates = Vec::new();
    if let Some(url) = text(post, "coverImageUrl") {
        candidates.push((None, url.to_string(), None));
    }
    let body = post.get("body");
    for key in ["images", "files"] {
        if let Some(values) = body
            .and_then(|body| body.get(key))
            .and_then(Value::as_array)
        {
            for value in values {
                push_media_candidate(&mut candidates, value);
            }
        }
    }
    for key in ["imageMap", "fileMap"] {
        if let Some(values) = body
            .and_then(|body| body.get(key))
            .and_then(Value::as_object)
        {
            for value in values.values() {
                push_media_candidate(&mut candidates, value);
            }
        }
    }
    if let Some(html) = body
        .and_then(|body| body.get("html"))
        .and_then(Value::as_str)
    {
        for captures in html_media_regex().captures_iter(html) {
            let url = captures
                .get(1)
                .or_else(|| captures.get(2))
                .map(|value| decode_html(value.as_str()));
            if let Some(url) = url {
                candidates.push((None, url, None));
            }
        }
    }

    let mut seen = BTreeSet::new();
    let headers = BTreeMap::from([("Referer".to_string(), canonical_url.to_string())]);
    let mut media = Vec::new();
    for (source_id, raw_url, raw_name) in candidates {
        let url = canonical_media_url(&raw_url)?;
        if !seen.insert(url.clone()) {
            continue;
        }
        let file_name = raw_name
            .filter(|name| !name.trim().is_empty())
            .or_else(|| file_name_from_url(&url))
            .unwrap_or_else(|| format!("fanbox_{post_id}_{}", media.len()));
        if is_archive(&file_name) || is_archive(&url) {
            continue;
        }
        let stable_id = source_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("fanbox:{post_id}:{id}"))
            .unwrap_or_else(|| format!("fanbox:{post_id}:{}", media.len()));
        media.push(
            MediaDescriptorBuilder::new(stable_id, media.len() as u32, url)
                .canonical_url(canonical_url)
                .file_name(file_name)
                .headers(headers.clone())
                .build(),
        );
    }
    Ok(media)
}

fn push_media_candidate(
    candidates: &mut Vec<(Option<String>, String, Option<String>)>,
    value: &Value,
) {
    let url = text(value, "originalUrl").or_else(|| text(value, "url"));
    let Some(url) = url else { return };
    let source_id = text(value, "id").map(ToOwned::to_owned);
    let mut name = text(value, "name").map(ToOwned::to_owned);
    if let Some(extension) = text(value, "extension") {
        if let Some(current) = name.as_mut() {
            if !current
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", extension.to_ascii_lowercase()))
            {
                current.push('.');
                current.push_str(extension);
            }
        }
    }
    candidates.push((source_id, url.to_string(), name));
}

fn normalize_creator(raw: &str) -> Result<String, SourceError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_query(
            "pixivFANBOX subscriptions require a creator slug",
        ));
    }
    let creator = if let Ok(url) = Url::parse(raw) {
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "pixivFANBOX subscriptions require a canonical creator URL",
            ));
        }
        let host = url.host_str().unwrap_or_default();
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if matches!(host, "fanbox.cc" | "www.fanbox.cc") {
            match segments.as_slice() {
                [creator] if creator.starts_with('@') => creator[1..].to_string(),
                _ => return Err(invalid_query("pixivFANBOX requires a creator profile URL")),
            }
        } else if let Some(creator) = host.strip_suffix(".fanbox.cc") {
            if !segments.is_empty() {
                return Err(invalid_query("pixivFANBOX requires a creator profile URL"));
            }
            creator.to_string()
        } else {
            return Err(invalid_query("pixivFANBOX requires a fanbox.cc URL"));
        }
    } else {
        raw.strip_prefix('@').unwrap_or(raw).to_string()
    };
    validate_slug(&creator, "pixivFANBOX requires a safe creator slug")
}

fn validate_slug(value: &str, message: &str) -> Result<String, SourceError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid_query(message));
    }
    Ok(value.to_string())
}

fn canonical_media_url(raw: &str) -> Result<String, SourceError> {
    let raw = decode_html(raw);
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw
    };
    let url =
        Url::parse(&raw).map_err(|_| invalid_response("FANBOX returned an invalid media URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid_response("FANBOX returned an unsafe media URL"));
    }
    Ok(url.to_string())
}

fn file_name_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn is_archive(raw: &str) -> bool {
    let path = raw.split('?').next().unwrap_or(raw).to_ascii_lowercase();
    [".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz"]
        .iter()
        .any(|extension| path.ends_with(extension))
}

fn html_media_regex() -> &'static Regex {
    static REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?:href=\"([^\"]+)\"|data-src-original=\"([^\"]+)\")"#)
            .expect("valid FANBOX media regex")
    })
}

fn decode_html(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn required_id<'a>(value: &'a Value, subject: &str) -> Result<&'a str, SourceError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| {
            !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_digit())
        })
        .ok_or_else(|| invalid_response(format!("{subject} has an invalid ID")))?;
    Ok(id)
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_cursor() -> SourceError {
    invalid_query("invalid FANBOX source cursor")
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    #[test]
    fn supports_anonymous_access_and_preserves_the_fanbox_browser_request_context() {
        assert!(adapter().descriptor().anonymous);
        let credentials = api_credentials(&RequestCredentials::default());
        assert_eq!(
            credentials.headers.get("Origin").map(String::as_str),
            Some("https://www.fanbox.cc")
        );
        assert_eq!(
            credentials.headers.get("Referer").map(String::as_str),
            Some("https://www.fanbox.cc/")
        );
        assert_eq!(
            credentials
                .headers
                .get("Sec-Fetch-Site")
                .map(String::as_str),
            Some("same-site")
        );
        assert_eq!(
            credentials.headers.get("User-Agent").map(String::as_str),
            Some(FIREFOX_USER_AGENT)
        );
    }

    #[test]
    fn preserves_the_user_agent_captured_with_the_session() {
        let mut credentials = RequestCredentials::default();
        credentials
            .headers
            .insert("user-agent".into(), "captured browser".into());
        let credentials = api_credentials(&credentials);
        assert_eq!(
            credentials.headers.get("user-agent").map(String::as_str),
            Some("captured browser")
        );
    }

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
        assert_eq!(normalize_creator("@creator-name").unwrap(), "creator-name");
        assert_eq!(
            normalize_creator("https://creator-name.fanbox.cc/").unwrap(),
            "creator-name"
        );
        assert_eq!(
            normalize_creator("https://www.fanbox.cc/@creator-name").unwrap(),
            "creator-name"
        );
        assert!(normalize_creator("https://creator-name.fanbox.cc/posts/1").is_err());
    }

    #[test]
    fn listing_exposes_one_post_and_validates_the_provider_cursor() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fanbox/listing.json")).unwrap();
        let batch = normalize_listing(&request(), "creator-name", fixture).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(!batch.exhausted);
        let cursor = batch.posts[0].resume_cursor_after.as_deref().unwrap();
        assert!(listing_url(Some(cursor), "creator-name").is_ok());
        assert!(listing_url(Some(cursor), "other-creator").is_err());
    }

    #[test]
    fn maps_all_direct_media_and_canonical_tags_for_one_post() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fanbox/post.json")).unwrap();
        let discovered = normalize_listing(
            &request(),
            "creator-name",
            serde_json::from_str(include_str!("../../tests/fixtures/fanbox/listing.json")).unwrap(),
        )
        .unwrap()
        .posts
        .into_iter()
        .next()
        .unwrap();
        let post = normalize_post(discovered, fixture).unwrap();
        assert_eq!(post.media.len(), 4);
        assert_eq!(post.media[0].position, 0);
        assert_eq!(post.media[3].position, 3);
        assert!(!post
            .media
            .iter()
            .any(|media| media.url.ends_with("archive.zip")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "creator-name")));
        assert!(post.tags.contains(&CanonicalTag::new("", "exclusive")));
        assert_eq!(
            post.notes.as_deref(),
            Some("Opening prose bonus Second block https://example.test")
        );
    }
}
