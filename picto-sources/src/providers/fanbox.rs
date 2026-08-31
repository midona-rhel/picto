use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, PostFuture,
    ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const API_DOMAIN: &str = "api.fanbox.cc";
const FIREFOX_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
const CURSOR: OpaqueCursor = OpaqueCursor::new(2_048);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    FanboxSource::default()
}

#[derive(Default)]
struct FanboxSource {
    creator_pages: Mutex<HashMap<String, CreatorPages>>,
}

#[derive(Default)]
struct CreatorPages {
    urls: Vec<String>,
    posts: HashMap<usize, Vec<Value>>,
}

struct CreatorPageRequest<'a> {
    cache_key: &'a str,
    creator: &'a str,
    credentials: &'a RequestCredentials,
    http: &'a HttpRuntime,
    cancel: &'a CancellationToken,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CreatorCursor {
    page: usize,
    item: usize,
}

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
            let cache_key = creator_cache_key(&creator, &credentials);
            self.reset_fresh_creator_pages(&cache_key, request.cursor.as_deref())
                .await;
            let cursor = decode_cursor(request.cursor.as_deref())?;
            self.discover_creator_post(request, &creator, cursor, &credentials, http, cancel)
                .await
        })
    }

    fn resolve_post<'a>(
        &'a self,
        mut post: SourcePost,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> PostFuture<'a> {
        Box::pin(async move {
            let credentials = api_credentials(credentials);
            let mut url =
                Url::parse("https://api.fanbox.cc/post.info").expect("static FANBOX post endpoint");
            url.query_pairs_mut().append_pair("postId", &post.stable_id);
            let Some(response) = http
                .get_browser_optional_json(url, &credentials, cancel)
                .await?
            else {
                // A creator feed can advertise posts outside the account's
                // current support tier. gallery-dl skips those individual
                // post.info failures and continues traversing the feed; doing
                // the same prevents one inaccessible post from aborting every
                // accessible post behind it.
                post.media.clear();
                return Ok(post);
            };
            normalize_post(post, response)
        })
    }
}

impl FanboxSource {
    async fn reset_fresh_creator_pages(&self, cache_key: &str, cursor: Option<&str>) {
        if cursor.is_none_or(str::is_empty) {
            self.creator_pages.lock().await.remove(cache_key);
        }
    }

    async fn discover_creator_post(
        &self,
        request: &DiscoveryRequest,
        creator: &str,
        mut cursor: CreatorCursor,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DiscoveryBatch, SourceError> {
        let cache_key = creator_cache_key(creator, credentials);
        let page_urls = self
            .creator_page_urls(&cache_key, creator, credentials, http, cancel)
            .await?;
        let page_request = CreatorPageRequest {
            cache_key: &cache_key,
            creator,
            credentials,
            http,
            cancel,
        };
        while cursor.page < page_urls.len() {
            let posts = self
                .creator_page(cursor.page, &page_urls[cursor.page], &page_request)
                .await?;
            if cursor.item < posts.len() {
                let next = if cursor.item + 1 < posts.len() {
                    Some(CreatorCursor {
                        page: cursor.page,
                        item: cursor.item + 1,
                    })
                } else if cursor.page + 1 < page_urls.len() {
                    Some(CreatorCursor {
                        page: cursor.page + 1,
                        item: 0,
                    })
                } else {
                    None
                };
                return normalize_listing_post(request, creator, &posts[cursor.item], next);
            }
            cursor = CreatorCursor {
                page: cursor.page + 1,
                item: 0,
            };
        }
        Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        })
    }

    async fn creator_page_urls(
        &self,
        cache_key: &str,
        creator: &str,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, SourceError> {
        let cached_urls = {
            let pages = self.creator_pages.lock().await;
            pages
                .get(cache_key)
                .filter(|pages| !pages.urls.is_empty())
                .map(|pages| pages.urls.clone())
        };
        if let Some(urls) = cached_urls {
            return Ok(urls);
        }
        let mut url = Url::parse("https://api.fanbox.cc/post.paginateCreator")
            .expect("static FANBOX pagination endpoint");
        url.query_pairs_mut().append_pair("creatorId", creator);
        let response = http.get_json::<Value>(url, credentials, cancel).await?;
        let urls = normalize_page_urls(&response, creator)?;
        self.creator_pages
            .lock()
            .await
            .entry(cache_key.to_string())
            .or_default()
            .urls = urls.clone();
        Ok(urls)
    }

    async fn creator_page(
        &self,
        page_index: usize,
        raw_url: &str,
        request: &CreatorPageRequest<'_>,
    ) -> Result<Vec<Value>, SourceError> {
        let cached_posts = {
            let pages = self.creator_pages.lock().await;
            pages
                .get(request.cache_key)
                .and_then(|pages| pages.posts.get(&page_index))
                .cloned()
        };
        if let Some(posts) = cached_posts {
            return Ok(posts);
        }
        let url = validate_listing_url(raw_url, request.creator)?;
        let response = request
            .http
            .get_json::<Value>(url, request.credentials, request.cancel)
            .await?;
        let posts = normalize_page_posts(&response)?;
        self.creator_pages
            .lock()
            .await
            .entry(request.cache_key.to_string())
            .or_default()
            .posts
            .insert(page_index, posts.clone());
        Ok(posts)
    }
}

fn api_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert("fanbox.cc".to_string());
    // A captured Chromium session retains the exact request identity that
    // FANBOX already verified. Anonymous/fallback requests use gallery-dl's
    // provider-owned Firefox profile.
    if !credentials
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("user-agent"))
    {
        credentials
            .headers
            .insert("User-Agent".to_string(), FIREFOX_USER_AGENT.to_string());
    }
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| "application/json, text/plain, */*".to_string());
    credentials
        .headers
        .entry("Accept-Language".to_string())
        .or_insert_with(|| "en-US,en;q=0.5".to_string());
    credentials
        .headers
        .entry("Origin".to_string())
        .or_insert_with(|| "https://www.fanbox.cc".to_string());
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

fn creator_cache_key(creator: &str, credentials: &RequestCredentials) -> String {
    format!(
        "{creator}\0{}",
        credentials
            .cookies
            .get("FANBOXSESSID")
            .map(String::as_str)
            .unwrap_or("anonymous")
    )
}

fn decode_cursor(raw: Option<&str>) -> Result<CreatorCursor, SourceError> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Ok(CreatorCursor::default());
    };
    CURSOR.validate(raw)?;
    let Some((page, item)) = raw.strip_prefix('p').and_then(|raw| raw.split_once('i')) else {
        return Err(invalid_cursor());
    };
    let page = page.parse::<usize>().map_err(|_| invalid_cursor())?;
    let item = item.parse::<usize>().map_err(|_| invalid_cursor())?;
    if page > 1_000_000 || item > 10_000 {
        return Err(invalid_cursor());
    }
    Ok(CreatorCursor { page, item })
}

fn encode_cursor(cursor: CreatorCursor) -> Result<String, SourceError> {
    let value = format!("p{}i{}", cursor.page, cursor.item);
    CURSOR.validate(&value)?;
    Ok(value)
}

fn normalize_page_urls(response: &Value, creator: &str) -> Result<Vec<String>, SourceError> {
    response
        .pointer("/body/pageUrls")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("FANBOX pagination response is missing its pages"))?
        .iter()
        .map(|value| {
            let raw = value
                .as_str()
                .ok_or_else(|| invalid_response("FANBOX returned an invalid page URL"))?;
            validate_listing_url(raw, creator).map(|url| url.to_string())
        })
        .collect()
}

fn validate_listing_url(raw: &str, creator: &str) -> Result<Url, SourceError> {
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let url =
        Url::parse(&raw).map_err(|_| invalid_response("FANBOX returned an invalid page URL"))?;
    if url.scheme() != "https"
        || url.host_str() != Some(API_DOMAIN)
        || url.path() != "/post.listCreator"
        || url
            .query_pairs()
            .find(|(key, _)| key == "creatorId")
            .is_none_or(|(_, value)| value != creator)
    {
        return Err(invalid_response("FANBOX returned an unsafe page URL"));
    }
    Ok(url)
}

fn normalize_page_posts(response: &Value) -> Result<Vec<Value>, SourceError> {
    response
        .pointer("/body/posts")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| invalid_response("FANBOX page response is missing its posts"))
}

fn normalize_listing_post(
    request: &DiscoveryRequest,
    creator: &str,
    summary: &Value,
    next: Option<CreatorCursor>,
) -> Result<DiscoveryBatch, SourceError> {
    let stable_id = required_id(summary, "FANBOX post")?;
    let response_creator = text(summary, "creatorId").unwrap_or(creator);
    if response_creator != creator {
        return Err(invalid_response(
            "FANBOX returned a post for a different creator",
        ));
    }
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
        resume_cursor_after: next.map(encode_cursor).transpose()?,
    };
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted: next.is_none(),
    })
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
        candidates.push((None, original_cover_url(url), None));
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
    for (key, block_id) in [("imageMap", "imageId"), ("fileMap", "fileId")] {
        for value in ordered_map_values(body, key, block_id) {
            push_media_candidate(&mut candidates, value);
        }
    }
    if let Some(html) = body
        .and_then(|body| body.get("html"))
        .and_then(Value::as_str)
    {
        for captures in html_media_regex().captures_iter(html) {
            let url = captures
                .get(1)
                .and_then(|value| safe_html_media_url(value.as_str(), HtmlMediaKind::Href))
                .or_else(|| {
                    captures.get(2).and_then(|value| {
                        safe_html_media_url(value.as_str(), HtmlMediaKind::OriginalImage)
                    })
                });
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
        if crate::media::is_unsupported_archive(&file_name)
            || crate::media::is_unsupported_archive(&url)
        {
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

fn ordered_map_values<'a>(body: Option<&'a Value>, key: &str, block_id: &str) -> Vec<&'a Value> {
    let Some(body) = body else { return Vec::new() };
    let Some(values) = body.get(key).and_then(Value::as_object) else {
        return Vec::new();
    };
    let Some(blocks) = body.get("blocks").and_then(Value::as_array) else {
        return values.values().collect();
    };
    blocks
        .iter()
        .filter_map(|block| block.get(block_id).and_then(Value::as_str))
        .filter_map(|id| values.get(id))
        .collect()
}

#[derive(Clone, Copy)]
enum HtmlMediaKind {
    Href,
    OriginalImage,
}

fn safe_html_media_url(raw: &str, kind: HtmlMediaKind) -> Option<String> {
    let raw = decode_html(raw);
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw
    };
    let url = Url::parse(&raw).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let trusted = match kind {
        HtmlMediaKind::Href => {
            host == "downloads.fanbox.cc"
                || (host == "fanbox.pixiv.net" && url.path().starts_with("/images/entry"))
        }
        HtmlMediaKind::OriginalImage => ["fanbox.cc", "pixiv.net", "pximg.net"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}"))),
    };
    trusted.then(|| url.to_string())
}

fn original_cover_url(raw: &str) -> String {
    static TRANSFORM: OnceLock<Regex> = OnceLock::new();
    TRANSFORM
        .get_or_init(|| Regex::new(r"/c/[0-9A-Za-z_]+/").expect("valid FANBOX transform regex"))
        .replace(raw, "/")
        .into_owned()
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
    fn preserves_the_browser_identity_captured_with_the_session() {
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
    fn listing_exposes_one_post_and_advances_the_compact_cursor() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fanbox/listing.json")).unwrap();
        let summary = fixture.pointer("/body/items/0").unwrap();
        let batch = normalize_listing_post(
            &request(),
            "creator-name",
            summary,
            Some(CreatorCursor { page: 2, item: 3 }),
        )
        .unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(!batch.exhausted);
        let cursor = batch.posts[0].resume_cursor_after.as_deref().unwrap();
        assert_eq!(
            decode_cursor(Some(cursor)).unwrap(),
            CreatorCursor { page: 2, item: 3 }
        );
        assert!(decode_cursor(Some("https://api.fanbox.cc/post.listCreator")).is_err());
    }

    #[test]
    fn maps_all_direct_media_and_canonical_tags_for_one_post() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fanbox/post.json")).unwrap();
        let listing: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fanbox/listing.json")).unwrap();
        let discovered = normalize_listing_post(
            &request(),
            "creator-name",
            listing.pointer("/body/items/0").unwrap(),
            None,
        )
        .unwrap()
        .posts
        .into_iter()
        .next()
        .unwrap();
        let post = normalize_post(discovered, fixture).unwrap();
        assert_eq!(post.name.as_deref(), Some("A FANBOX post"));
        assert_eq!(post.media.len(), 4);
        assert_eq!(post.media[0].position, 0);
        assert_eq!(
            post.media[0].url,
            "https://downloads.fanbox.cc/images/cover.jpg"
        );
        assert_eq!(post.media[3].position, 3);
        assert!(!crate::media::is_unsupported_archive("attachment.zip"));
        assert!(crate::media::is_unsupported_archive("attachment.rar"));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "creator-name")));
        assert!(post.tags.contains(&CanonicalTag::new("", "exclusive")));
        assert_eq!(
            post.notes.as_deref(),
            Some("Opening prose bonus Second block https://example.test")
        );
    }

    #[tokio::test]
    async fn a_fresh_traversal_invalidates_only_its_creator_cache() {
        let source = FanboxSource::default();
        let credentials = api_credentials(&RequestCredentials::default());
        let key = creator_cache_key("creator-name", &credentials);
        source
            .creator_pages
            .lock()
            .await
            .insert(key.clone(), CreatorPages::default());

        source.reset_fresh_creator_pages(&key, Some("p0i1")).await;
        assert!(source.creator_pages.lock().await.contains_key(&key));
        source.reset_fresh_creator_pages(&key, None).await;
        assert!(!source.creator_pages.lock().await.contains_key(&key));
    }

    #[test]
    fn article_maps_follow_block_order() {
        let post = serde_json::json!({
            "body": {
                "blocks": [
                    { "imageId": "second" },
                    { "imageId": "first" }
                ],
                "imageMap": {
                    "first": { "id": "first", "originalUrl": "https://downloads.fanbox.cc/first.jpg" },
                    "second": { "id": "second", "originalUrl": "https://downloads.fanbox.cc/second.jpg" },
                    "unused": { "id": "unused", "originalUrl": "https://downloads.fanbox.cc/unused.jpg" }
                }
            }
        });
        let media = post_media(&post, "42", "https://creator.fanbox.cc/posts/42").unwrap();
        assert_eq!(media.len(), 2);
        assert!(media[0].url.ends_with("/second.jpg"));
        assert!(media[1].url.ends_with("/first.jpg"));
    }

    #[test]
    fn html_media_is_restricted_to_first_party_hosts() {
        let post = serde_json::json!({
            "body": {
                "html": concat!(
                    "<a href=\"https://evil.test/file.jpg\">bad</a>",
                    "<a href=\"https://downloads.fanbox.cc/file.jpg\">good</a>",
                    "<img data-src-original=\"https://localhost/private.jpg\">",
                    "<img data-src-original=\"https://i.pximg.net/original.jpg\">"
                )
            }
        });
        let media = post_media(&post, "42", "https://creator.fanbox.cc/posts/42").unwrap();
        assert_eq!(media.len(), 2);
        assert!(media.iter().all(|item| !item.url.contains("evil.test")));
        assert!(media.iter().all(|item| !item.url.contains("localhost")));
    }
}
