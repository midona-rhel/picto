use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const DOMAIN: &str = "subscribestar.art";
const CURSOR: OpaqueCursor = OpaqueCursor::new(4_096);
const MAX_PAGES_PER_DISCOVERY: usize = 4_096;

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    SubscribeStarSource::default()
}

#[derive(Default)]
struct SubscribeStarSource {
    page: crate::page_cache::PageCache<(Url, String)>,
}

impl NativeSourceAdapter for SubscribeStarSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "subscribestar",
            display_name: "SubscribeStar",
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
        Box::pin(
            self.discover_with(request, credentials, move |url, credentials| async move {
                http.get_text_with_final_url(url, &credentials, cancel)
                    .await
            }),
        )
    }
}

impl SubscribeStarSource {
    async fn discover_with<F, Fut>(
        &self,
        request: &DiscoveryRequest,
        credentials: &RequestCredentials,
        mut fetch: F,
    ) -> Result<DiscoveryBatch, SourceError>
    where
        F: FnMut(Url, RequestCredentials) -> Fut,
        Fut: std::future::Future<Output = Result<(Url, String), SourceError>>,
    {
        let creator = normalize_creator(&request.query)?;
        let cursor = decode_cursor(request.cursor.as_deref())?;
        let first_page = creator_url(&request.query)?;
        let domain = first_page.host_str().expect("validated creator host");
        let credentials = site_credentials(credentials, domain)?;
        let mut page_url = match cursor.page.as_deref() {
            Some(raw) => {
                let url = Url::parse(raw).map_err(|_| invalid_cursor())?;
                validate_page_url(&url, domain)?;
                url
            }
            None => first_page.clone(),
        };
        let mut visited = BTreeSet::new();
        for _ in 0..MAX_PAGES_PER_DISCOVERY {
            if !visited.insert(page_url.to_string()) {
                return Err(invalid_response("SubscribeStar pagination did not advance"));
            }
            let scope = format!("{creator}\n{page_url}");
            let (final_url, response) = self
                .page
                .get_or_fetch(
                    &scope,
                    &credentials,
                    request
                        .cursor
                        .as_deref()
                        .is_some_and(|value| !value.is_empty()),
                    fetch(page_url.clone(), credentials.clone()),
                )
                .await?;
            detect_access_url(&final_url)?;
            validate_page_url(&final_url, domain)?;
            let html = unwrap_html_response(&response)?;
            match normalize_page(request, &creator, &cursor, &final_url, &html)? {
                PageResult::Post(batch) => return Ok(batch),
                PageResult::Continue(Some(next)) => page_url = next,
                PageResult::Continue(None) => {
                    return Ok(DiscoveryBatch {
                        posts: Vec::new(),
                        exhausted: true,
                    });
                }
            }
        }
        Err(invalid_response(
            "SubscribeStar pagination exceeded its safety bound",
        ))
    }
}

fn site_credentials(
    credentials: &RequestCredentials,
    domain: &str,
) -> Result<RequestCredentials, SourceError> {
    if !credentials.is_empty() {
        let origin = credentials
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("origin"));
        let captured = match origin {
            Some((_, value)) => Url::parse(value)
                .ok()
                .filter(|url| {
                    url.scheme() == "https"
                        && url.port().is_none()
                        && url.username().is_empty()
                        && url.password().is_none()
                })
                .and_then(|url| service_domain(url.host_str().unwrap_or_default())),
            None => Some(DOMAIN),
        };
        if captured != service_domain(domain) || !credentials.permits(domain) {
            return Err(SourceError::new(SourceErrorKind::Authentication,
                format!("Connect a SubscribeStar session for {domain}; the saved session belongs to another service"), false));
        }
    }
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert(domain.to_string());
    credentials
        .cookies
        .entry("18_plus_agreement_generic".to_string())
        .or_insert_with(|| "true".to_string());
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| {
            "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8".to_string()
        });
    Ok(credentials)
}

fn profile_url(creator: &str) -> Url {
    let mut url = Url::parse("https://subscribestar.art").expect("static SubscribeStar URL");
    url.path_segments_mut()
        .expect("SubscribeStar URL supports path segments")
        .push(creator);
    url
}

fn service_domain(host: &str) -> Option<&'static str> {
    match host {
        "subscribestar.com" | "www.subscribestar.com" => Some("subscribestar.com"),
        "subscribestar.adult" | "www.subscribestar.adult" => Some("subscribestar.adult"),
        "subscribestar.art" | "www.subscribestar.art" => Some(DOMAIN),
        _ => None,
    }
}

fn creator_url(query: &str) -> Result<Url, SourceError> {
    let creator = normalize_creator(query)?;
    let domain = Url::parse(query.trim())
        .ok()
        .and_then(|url| service_domain(url.host_str().unwrap_or_default()))
        .unwrap_or(DOMAIN);
    let host = if domain == "subscribestar.com" {
        "www.subscribestar.com"
    } else {
        domain
    };
    if host == DOMAIN {
        return Ok(profile_url(&creator));
    }
    Url::parse(&format!("https://{host}/{creator}"))
        .map_err(|_| invalid_query("Invalid SubscribeStar creator URL"))
}

fn unwrap_html_response(response: &str) -> Result<String, SourceError> {
    let html = if response.trim_start().starts_with('{') {
        let value: Value = serde_json::from_str(response).map_err(|_| {
            invalid_response("SubscribeStar returned an invalid pagination response")
        })?;
        value
            .get("html")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| invalid_response("SubscribeStar pagination response is missing HTML"))?
    } else {
        response.to_string()
    };
    detect_access_page(&html)?;
    Ok(html)
}

fn detect_access_page(html: &str) -> Result<(), SourceError> {
    let lower = html.to_ascii_lowercase();
    if [
        "/verify_subscriber",
        "/age_confirmation_warning",
        "18_plus_agreement_generic",
        "age confirmation warning",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err(access_verification_required());
    }
    if html.len() < 250 && lower.contains(">redirected<") {
        return Err(invalid_response(
            "SubscribeStar returned an unresolved HTML redirect",
        ));
    }
    Ok(())
}

fn detect_access_url(url: &Url) -> Result<(), SourceError> {
    if ["/verify_subscriber", "/age_confirmation_warning"]
        .iter()
        .any(|marker| url.path() == *marker || url.path().starts_with(&format!("{marker}/")))
    {
        return Err(access_verification_required());
    }
    Ok(())
}

fn access_verification_required() -> SourceError {
    SourceError::new(
        SourceErrorKind::AccessDenied,
        "SubscribeStar requires subscriber or age verification in the managed login",
        false,
    )
}

fn normalize_page(
    request: &DiscoveryRequest,
    creator: &str,
    cursor: &CursorState,
    page_url: &Url,
    html: &str,
) -> Result<PageResult, SourceError> {
    detect_access_page(html)?;
    let posts = post_fragments(html);
    if posts.is_empty() {
        return Ok(PageResult::Continue(next_page_url(html, page_url)?));
    }
    let next_page = next_page_url(html, page_url)?;
    let Some(index) = next_post_index(&posts, cursor.anchor.as_deref())? else {
        return Ok(PageResult::Continue(next_page));
    };
    let post_id = fragment_post_id(posts[index])?;
    let post = normalize_post(
        request,
        creator,
        posts[index],
        Some(encode_cursor(post_id, page_url)?),
        page_url,
    )?;
    Ok(PageResult::Post(DiscoveryBatch {
        posts: vec![post],
        exhausted: index + 1 == posts.len() && next_page.is_none(),
    }))
}

enum PageResult {
    Post(DiscoveryBatch),
    Continue(Option<Url>),
}

fn next_post_index(posts: &[&str], anchor: Option<&str>) -> Result<Option<usize>, SourceError> {
    let Some(anchor) = anchor else {
        return Ok((!posts.is_empty()).then_some(0));
    };
    for (index, post) in posts.iter().enumerate() {
        let post_id = fragment_post_id(post)?;
        if numeric_id_is_older(post_id, anchor) {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn fragment_post_id(html: &str) -> Result<&str, SourceError> {
    capture(html, post_id_regex(), 1)
        .filter(|id| valid_numeric_id(id))
        .ok_or_else(|| invalid_response("SubscribeStar post has an invalid ID"))
}

fn numeric_id_is_older(candidate: &str, anchor: &str) -> bool {
    let candidate = candidate.trim_start_matches('0');
    let anchor = anchor.trim_start_matches('0');
    candidate.len() < anchor.len() || (candidate.len() == anchor.len() && candidate < anchor)
}

fn normalize_post(
    request: &DiscoveryRequest,
    creator: &str,
    html: &str,
    resume_cursor_after: Option<String>,
    page_url: &Url,
) -> Result<SourcePost, SourceError> {
    let post_id = fragment_post_id(html)?;
    let author = capture(html, author_regex(), 1)
        .map(decode_html)
        .and_then(|author| normalize_creator(&author).ok())
        .unwrap_or_else(|| creator.to_string());
    if author != creator {
        return Err(invalid_response(
            "SubscribeStar returned a post for a different creator",
        ));
    }
    let canonical_url = page_url
        .join(&format!("/posts/{post_id}"))
        .map_err(|_| invalid_response("Invalid SubscribeStar post URL"))?
        .to_string();
    let domain =
        service_domain(page_url.host_str().unwrap_or_default()).ok_or_else(invalid_cursor)?;
    let stable_id = if domain == DOMAIN {
        post_id.to_string()
    } else {
        format!("{domain}:{post_id}")
    };
    let content = capture(html, content_regex(), 1).unwrap_or_default();
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", creator);
    for captures in tag_regex().captures_iter(html) {
        if let Some(value) = captures.get(1) {
            let decoded = url::form_urlencoded::parse(value.as_str().as_bytes())
                .next()
                .map(|(key, value)| if value.is_empty() { key } else { value });
            if let Some(value) = decoded {
                tags.insert("", value.as_ref());
            }
        }
    }
    Ok(SourcePost {
        site_id: "subscribestar".to_string(),
        partition: request.partition.clone(),
        stable_id: stable_id.clone(),
        canonical_url: Some(canonical_url.clone()),
        creator: Some(creator.to_string()),
        name: capture(content, title_regex(), 1).and_then(normalize_source_text),
        notes: normalize_source_text(content),
        created_at: capture(html, date_regex(), 1).and_then(normalize_source_text),
        tags: tags.into_vec(),
        media: post_media(html, post_id, &stable_id, &canonical_url)?,
        resume_cursor_after,
    })
}

fn post_media(
    html: &str,
    post_id: &str,
    stable_post_id: &str,
    canonical_url: &str,
) -> Result<Vec<crate::MediaDescriptor>, SourceError> {
    let mut candidates = Vec::new();
    if let Some(gallery) = capture(html, gallery_regex(), 1) {
        let gallery = decode_html(gallery);
        let values: Vec<Value> = serde_json::from_str(&gallery)
            .map_err(|_| invalid_response("SubscribeStar post has invalid gallery metadata"))?;
        for value in values {
            let Some(url) = value.get("url").and_then(Value::as_str) else {
                continue;
            };
            if url.contains("/previews") {
                continue;
            }
            candidates.push((
                value.get("id").map(value_id),
                url.to_string(),
                value
                    .get("name")
                    .or_else(|| value.get("original_filename"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            ));
        }
    }
    for captures in document_regex().captures_iter(html) {
        candidates.push((
            captures.get(1).map(|value| value.as_str().to_string()),
            captures
                .get(3)
                .map(|value| decode_html(value.as_str()))
                .unwrap_or_default(),
            captures.get(2).map(|value| decode_html(value.as_str())),
        ));
    }
    for captures in audio_regex().captures_iter(html) {
        candidates.push((
            captures.get(1).map(|value| value.as_str().to_string()),
            captures
                .get(3)
                .map(|value| decode_html(value.as_str()))
                .unwrap_or_default(),
            captures.get(2).map(|value| decode_html(value.as_str())),
        ));
    }

    let mut seen = BTreeSet::new();
    let mut media = Vec::new();
    let headers = BTreeMap::from([("Referer".to_string(), canonical_url.to_string())]);
    for (source_id, raw_url, name) in candidates {
        let Some(url) = canonical_media_url(&raw_url, canonical_url) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let file_name = name
            .filter(|name| !name.trim().is_empty())
            .or_else(|| file_name_from_url(&url))
            .unwrap_or_else(|| format!("subscribestar_{post_id}_{}", media.len()));
        if crate::media::is_unsupported_archive(&file_name)
            || crate::media::is_unsupported_archive(&url)
        {
            continue;
        }
        let stable_id = source_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("subscribestar:{stable_post_id}:{id}"))
            .unwrap_or_else(|| format!("subscribestar:{stable_post_id}:{}", media.len()));
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

fn post_fragments(html: &str) -> Vec<&str> {
    html.split("<div class=\"post ").skip(1).collect()
}

fn next_page_url(html: &str, current: &Url) -> Result<Option<Url>, SourceError> {
    let Some(raw) = capture(html, next_page_regex(), 1) else {
        return Ok(None);
    };
    let raw = decode_html(raw);
    let url = current
        .join(&raw)
        .map_err(|_| invalid_response("SubscribeStar returned an invalid next page"))?;
    validate_page_url(&url, current.host_str().unwrap_or_default())?;
    Ok(Some(url))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct CursorState {
    anchor: Option<String>,
    #[serde(default)]
    page: Option<String>,
}

fn encode_cursor(post_id: &str, page: &Url) -> Result<String, SourceError> {
    if !valid_numeric_id(post_id) {
        return Err(invalid_cursor());
    }
    let cursor = serde_json::to_string(&CursorState {
        anchor: Some(post_id.to_owned()),
        page: Some(page.to_string()),
    })
    .map_err(|_| invalid_cursor())?;
    CURSOR.validate(&cursor)?;
    Ok(cursor)
}

fn decode_cursor(raw: Option<&str>) -> Result<CursorState, SourceError> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Ok(CursorState::default());
    };
    CURSOR.validate(raw)?;
    if raw.starts_with('{') {
        let cursor: CursorState = serde_json::from_str(raw).map_err(|_| invalid_cursor())?;
        if !cursor.anchor.as_deref().is_some_and(valid_numeric_id) || cursor.page.is_none() {
            return Err(invalid_cursor());
        }
        return Ok(cursor);
    }
    let anchor = raw
        .strip_prefix('p')
        .filter(|value| valid_numeric_id(value))
        .map(ToOwned::to_owned)
        .ok_or_else(invalid_cursor)?;
    Ok(CursorState {
        anchor: Some(anchor),
        page: None,
    })
}

fn validate_page_url(url: &Url, domain: &str) -> Result<(), SourceError> {
    if url.scheme() != "https"
        || service_domain(url.host_str().unwrap_or_default()).is_none()
        || service_domain(url.host_str().unwrap_or_default()) != service_domain(domain)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid_cursor());
    }
    Ok(())
}

fn normalize_creator(raw: &str) -> Result<String, SourceError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_query(
            "SubscribeStar subscriptions require a creator slug",
        ));
    }
    let creator = if let Ok(url) = Url::parse(raw) {
        if !matches!(url.scheme(), "http" | "https")
            || service_domain(url.host_str().unwrap_or_default()).is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "SubscribeStar requires a canonical creator URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            [creator] if *creator != "posts" => (*creator).to_string(),
            _ => {
                return Err(invalid_query(
                    "SubscribeStar requires a creator profile URL",
                ))
            }
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
        return Err(invalid_query("SubscribeStar requires a safe creator slug"));
    }
    Ok(creator)
}

fn canonical_media_url(raw: &str, canonical_url: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = if raw.starts_with('/') {
        Url::parse(canonical_url).ok()?.join(raw).ok()?
    } else if raw.starts_with("//") {
        Url::parse(&format!("https:{raw}")).ok()?
    } else {
        Url::parse(raw).ok()?
    };
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

fn value_id(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn decode_html(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn capture<'a>(text: &'a str, regex: &Regex, group: usize) -> Option<&'a str> {
    regex.captures(text)?.get(group).map(|value| value.as_str())
}

macro_rules! regex_fn {
    ($name:ident, $pattern:literal) => {
        fn $name() -> &'static Regex {
            static REGEX: OnceLock<Regex> = OnceLock::new();
            REGEX.get_or_init(|| Regex::new($pattern).expect(concat!("valid ", stringify!($name))))
        }
    };
}

regex_fn!(post_id_regex, r#"data-id=\"(\d+)\""#);
regex_fn!(author_regex, r#"href=\"/([^\"/?#]+)\""#);
regex_fn!(date_regex, r#"(?s)class=\"post-date\"[^>]*>(.*?)</"#);
regex_fn!(
    content_regex,
    r#"(?s)<div class=\"post-content\" data-role=\"post_content-text\">(.*?)</div><div class=\"post-uploads for-youtube\""#
);
regex_fn!(title_regex, r#"(?s)<h1[^>]*>(.*?)</h1>"#);
regex_fn!(tag_regex, r#"[?&]tag=([^\"&]+)"#);
regex_fn!(gallery_regex, r#"data-gallery=\"([^\"]+)\""#);
regex_fn!(
    next_page_regex,
    r#"data-role=\"infinite_scroll-next_page\" href=\"([^\"]+)\""#
);
regex_fn!(
    document_regex,
    r#"(?s)class=\"doc_preview[^\"]*\".*?data-upload-id=\"(\d+)\".*?doc_preview-title\">(.*?)<.*?href=\"([^\"]+)\""#
);
regex_fn!(
    audio_regex,
    r#"(?s)class=\"audio_preview-data[^\"]*\".*?data-upload-id=\"(\d+)\".*?audio_preview-title\">(.*?)<.*?src=\"([^\"]+)\""#
);

fn valid_numeric_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_cursor() -> SourceError {
    invalid_query("invalid SubscribeStar source cursor")
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
            normalize_creator("https://subscribestar.art/creator-name").unwrap(),
            "creator-name"
        );
        assert!(normalize_creator("https://subscribestar.art/posts/123").is_err());
    }

    #[test]
    fn preserves_service_identity_for_queries_posts_and_relative_media() {
        let mut post_ids = BTreeSet::new();
        let mut media_ids = BTreeSet::new();
        for host in [
            "www.subscribestar.com",
            "subscribestar.adult",
            "subscribestar.art",
        ] {
            let query = format!("https://{host}/creator-name");
            let url = creator_url(&query).unwrap();
            assert_eq!(url.as_str(), query);
            let mut request = request();
            request.query = query;
            let html = include_str!("../../tests/fixtures/subscribestar/profile.html");
            let PageResult::Post(batch) = normalize_page(
                &request,
                "creator-name",
                &CursorState::default(),
                &url,
                html,
            )
            .unwrap() else {
                panic!("post expected")
            };
            post_ids.insert(batch.posts[0].stable_id.clone());
            media_ids.insert(batch.posts[0].media[0].stable_id.clone());
            assert_eq!(
                batch.posts[0].canonical_url.as_deref(),
                Some(format!("https://{host}/posts/778899").as_str())
            );
            assert_eq!(
                canonical_media_url("/file.jpg", url.as_ref()).unwrap(),
                format!("https://{host}/file.jpg")
            );
            assert!(
                validate_page_url(&Url::parse("https://evil.test/page").unwrap(), host).is_err()
            );
        }
        assert_eq!(post_ids.len(), 3);
        assert_eq!(media_ids.len(), 3);
        assert!(post_ids.contains("778899"));
        assert!(post_ids.contains("subscribestar.com:778899"));
        assert!(post_ids.contains("subscribestar.adult:778899"));
        assert_eq!(
            creator_url("creator-name").unwrap().host_str(),
            Some(DOMAIN)
        );
    }

    #[test]
    fn credentials_never_cross_subscribestar_services() {
        let mut credentials = RequestCredentials::default();
        credentials
            .cookies
            .insert("_personalization_id".into(), "fixture-session".into());
        credentials.allowed_domains.insert(DOMAIN.into());
        assert!(site_credentials(&credentials, DOMAIN).is_ok());
        assert!(site_credentials(&credentials, "www.subscribestar.com").is_err());
        credentials
            .headers
            .insert("Origin".into(), "https://subscribestar.adult".into());
        credentials.allowed_domains = BTreeSet::from(["subscribestar.adult".into()]);
        assert!(site_credentials(&credentials, "subscribestar.adult").is_ok());
        assert!(site_credentials(&credentials, DOMAIN).is_err());
        credentials
            .headers
            .insert("Origin".into(), "https://subscribestar.com".into());
        credentials.allowed_domains = BTreeSet::from(["subscribestar.com".into()]);
        assert!(site_credentials(&credentials, "www.subscribestar.com").is_ok());
        credentials
            .headers
            .insert("Origin".into(), "https://www.subscribestar.com".into());
        assert!(site_credentials(&credentials, "subscribestar.com").is_ok());
        credentials
            .headers
            .insert("Origin".into(), "https://evil.test".into());
        assert!(site_credentials(&credentials, "subscribestar.adult").is_err());
    }

    #[tokio::test]
    async fn fetches_each_page_once_and_resumes_on_its_saved_page() {
        let source = SubscribeStarSource::default();
        let credentials = RequestCredentials::default();
        let mut request = request();
        let calls = std::sync::Mutex::new(Vec::new());
        let fetch = |url: Url, _: RequestCredentials| async {
            calls.lock().unwrap().push(url.to_string());
            let html = if url.query().is_none() {
                format!("{}<a data-role=\"infinite_scroll-next_page\" href=\"/creator-name?page=2\"></a>", simple_page(&["300", "299"]))
            } else {
                simple_page(&["200", "199"])
            };
            Ok((url, html))
        };
        for id in ["300", "299", "200"] {
            let batch = source
                .discover_with(&request, &credentials, fetch)
                .await
                .unwrap();
            assert_eq!(batch.posts[0].stable_id, id);
            request.cursor = batch.posts[0].resume_cursor_after.clone();
        }
        assert_eq!(calls.lock().unwrap().len(), 2);
        let resumed = SubscribeStarSource::default();
        let batch = resumed
            .discover_with(&request, &credentials, fetch)
            .await
            .unwrap();
        assert_eq!(batch.posts[0].stable_id, "199");
        assert!(batch.exhausted);
        assert_eq!(calls.lock().unwrap().len(), 3);
        assert!(calls.lock().unwrap().last().unwrap().ends_with("?page=2"));
        request.cursor = None;
        let batch = source
            .discover_with(&request, &credentials, fetch)
            .await
            .unwrap();
        assert_eq!(batch.posts[0].stable_id, "300");
        assert_eq!(calls.lock().unwrap().len(), 4);
    }

    #[test]
    fn page_exposes_exactly_one_post_and_advances_within_the_same_page() {
        let html = include_str!("../../tests/fixtures/subscribestar/profile.html");
        let page_url = profile_url("creator-name");
        let cursor = CursorState::default();
        let PageResult::Post(batch) =
            normalize_page(&request(), "creator-name", &cursor, &page_url, html).unwrap()
        else {
            panic!("expected one post");
        };
        assert_eq!(batch.posts.len(), 1);
        assert_eq!(batch.posts[0].stable_id, "778899");
        let next = decode_cursor(batch.posts[0].resume_cursor_after.as_deref()).unwrap();
        assert_eq!(next.anchor.as_deref(), Some("778899"));
    }

    #[test]
    fn maps_gallery_documents_audio_tags_and_text() {
        let html = include_str!("../../tests/fixtures/subscribestar/profile.html");
        let page_url = profile_url("creator-name");
        let PageResult::Post(mut batch) = normalize_page(
            &request(),
            "creator-name",
            &CursorState::default(),
            &page_url,
            html,
        )
        .unwrap() else {
            panic!("expected one post");
        };
        let post = batch.posts.remove(0);
        assert_eq!(post.media.len(), 3);
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "creator-name")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("", "behind the scenes")));
        assert_eq!(post.name.as_deref(), Some("Post title"));
        assert_eq!(post.notes.as_deref(), Some("Post title Visible prose"));
    }

    #[test]
    fn cursor_accepts_only_bounded_post_id_anchors() {
        let encoded = encode_cursor("778899", &profile_url("creator-name")).unwrap();
        assert_eq!(
            decode_cursor(Some(&encoded)).unwrap().anchor.as_deref(),
            Some("778899")
        );
        assert_eq!(decode_cursor(Some("p778899")).unwrap().page, None);
        assert!(decode_cursor(Some("https://subscribestar.art/page#picto=1")).is_err());
        assert!(decode_cursor(Some("pnot-a-post")).is_err());
        assert!(decode_cursor(Some("p123456789012345678901234567890123")).is_err());
    }

    #[test]
    fn pagination_json_is_unwrapped_without_a_second_transport() {
        let json = include_str!("../../tests/fixtures/subscribestar/page.json");
        let html = unwrap_html_response(json).unwrap();
        assert!(html.contains("data-id=\"9900\""));
    }

    #[test]
    fn age_and_subscriber_verification_are_terminal_access_errors() {
        for html in [
            r#"<a href="/verify_subscriber">redirected</a>"#,
            r#"<a href="/age_confirmation_warning">redirected</a>"#,
        ] {
            let error = unwrap_html_response(html).unwrap_err();
            assert_eq!(error.kind, SourceErrorKind::AccessDenied);
            assert!(!error.retryable);
        }
    }

    #[test]
    fn final_verification_redirects_are_terminal_access_errors() {
        for path in [
            "/verify_subscriber",
            "/verify_subscriber/continue",
            "/age_confirmation_warning",
        ] {
            let url = Url::parse(&format!("https://subscribestar.art{path}")).unwrap();
            let error = detect_access_url(&url).unwrap_err();
            assert_eq!(error.kind, SourceErrorKind::AccessDenied);
            assert!(!error.retryable);
        }
        assert!(detect_access_url(&profile_url("creator-name")).is_ok());
    }

    #[test]
    fn provider_credentials_include_the_adult_age_cookie() {
        let credentials = site_credentials(&RequestCredentials::default(), DOMAIN).unwrap();
        assert_eq!(
            credentials
                .cookies
                .get("18_plus_agreement_generic")
                .map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn anchored_cursor_survives_insertions_and_anchor_deletion() {
        let page_url = profile_url("creator-name");
        let persisted = encode_cursor("200", &page_url).unwrap();
        let cursor = decode_cursor(Some(&persisted)).unwrap();
        for html in [
            simple_page(&["300", "200", "199"]),
            simple_page(&["300", "199", "198"]),
        ] {
            let PageResult::Post(batch) =
                normalize_page(&request(), "creator-name", &cursor, &page_url, &html).unwrap()
            else {
                panic!("expected the first older post");
            };
            assert_eq!(batch.posts[0].stable_id, "199");
            assert_eq!(batch.posts.len(), 1);
        }
    }

    #[test]
    fn page_with_only_newer_posts_continues_without_publishing() {
        let html = format!(
            "{}<a data-role=\"infinite_scroll-next_page\" href=\"/creator-name?page=2\"></a>",
            simple_page(&["300", "250"])
        );
        let cursor = CursorState {
            anchor: Some("200".to_string()),
            page: None,
        };
        let PageResult::Continue(Some(next)) = normalize_page(
            &request(),
            "creator-name",
            &cursor,
            &profile_url("creator-name"),
            &html,
        )
        .unwrap() else {
            panic!("expected pagination to continue");
        };
        assert_eq!(
            next.as_str(),
            "https://subscribestar.art/creator-name?page=2"
        );
    }

    fn simple_page(ids: &[&str]) -> String {
        ids.iter()
            .map(|id| {
                format!(
                    concat!(
                        "<div class=\"post visible\" data-id=\"{id}\" data-user-id=\"42\">",
                        "<a href=\"/creator-name\">Creator</a>",
                        "<div class=\"post-date\">Aug 28, 2026</div>",
                        "<div class=\"post-content\" data-role=\"post_content-text\"><p>Post {id}</p></div>",
                        "<div class=\"post-uploads for-youtube\"></div></div>"
                    ),
                    id = id,
                )
            })
            .collect()
    }
}
