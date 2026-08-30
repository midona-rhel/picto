use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
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

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    SubscribeStarSource
}

struct SubscribeStarSource;

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
        Box::pin(async move {
            let creator = normalize_creator(&request.query)?;
            let cursor = request
                .cursor
                .as_deref()
                .filter(|cursor| !cursor.is_empty())
                .map(decode_cursor)
                .transpose()?
                .unwrap_or_else(|| CursorState {
                    url: profile_url(&creator),
                    index: 0,
                });
            let credentials = site_credentials(credentials);
            let response = http
                .get_text(cursor.url.clone(), &credentials, cancel)
                .await?;
            let html = unwrap_html_response(&response)?;
            normalize_page(request, &creator, &cursor, &html)
        })
    }
}

fn site_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert(DOMAIN.to_string());
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| {
            "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8".to_string()
        });
    credentials
}

fn profile_url(creator: &str) -> Url {
    let mut url = Url::parse("https://subscribestar.art").expect("static SubscribeStar URL");
    url.path_segments_mut()
        .expect("SubscribeStar URL supports path segments")
        .push(creator);
    url
}

fn unwrap_html_response(response: &str) -> Result<String, SourceError> {
    if response.trim_start().starts_with('{') {
        let value: Value = serde_json::from_str(response).map_err(|_| {
            invalid_response("SubscribeStar returned an invalid pagination response")
        })?;
        return value
            .get("html")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| invalid_response("SubscribeStar pagination response is missing HTML"));
    }
    Ok(response.to_string())
}

fn normalize_page(
    request: &DiscoveryRequest,
    creator: &str,
    cursor: &CursorState,
    html: &str,
) -> Result<DiscoveryBatch, SourceError> {
    let posts = post_fragments(html);
    if posts.is_empty() {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    }
    if cursor.index >= posts.len() {
        return Err(invalid_response(
            "SubscribeStar page changed before its persisted cursor",
        ));
    }
    let next_page = next_page_url(html, &cursor.url)?;
    let next_cursor = if cursor.index + 1 < posts.len() {
        Some(encode_cursor(&cursor.url, cursor.index + 1)?)
    } else {
        next_page
            .as_ref()
            .map(|url| encode_cursor(url, 0))
            .transpose()?
    };
    let exhausted = cursor.index + 1 == posts.len() && next_page.is_none();
    let post = normalize_post(request, creator, posts[cursor.index], next_cursor)?;
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted,
    })
}

fn normalize_post(
    request: &DiscoveryRequest,
    creator: &str,
    html: &str,
    resume_cursor_after: Option<String>,
) -> Result<SourcePost, SourceError> {
    let post_id = capture(html, post_id_regex(), 1)
        .filter(|id| valid_numeric_id(id))
        .ok_or_else(|| invalid_response("SubscribeStar post has an invalid ID"))?;
    let author = capture(html, author_regex(), 1)
        .map(decode_html)
        .and_then(|author| normalize_creator(&author).ok())
        .unwrap_or_else(|| creator.to_string());
    if author != creator {
        return Err(invalid_response(
            "SubscribeStar returned a post for a different creator",
        ));
    }
    let canonical_url = format!("https://subscribestar.art/posts/{post_id}");
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
        stable_id: post_id.to_string(),
        canonical_url: Some(canonical_url.clone()),
        creator: Some(creator.to_string()),
        name: capture(content, title_regex(), 1).and_then(normalize_source_text),
        notes: normalize_source_text(content),
        created_at: capture(html, date_regex(), 1).and_then(normalize_source_text),
        tags: tags.into_vec(),
        media: post_media(html, post_id, &canonical_url)?,
        resume_cursor_after,
    })
}

fn post_media(
    html: &str,
    post_id: &str,
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
        let Some(url) = canonical_media_url(&raw_url) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let file_name = name
            .filter(|name| !name.trim().is_empty())
            .or_else(|| file_name_from_url(&url))
            .unwrap_or_else(|| format!("subscribestar_{post_id}_{}", media.len()));
        if is_archive(&file_name) || is_archive(&url) {
            continue;
        }
        let stable_id = source_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("subscribestar:{post_id}:{id}"))
            .unwrap_or_else(|| format!("subscribestar:{post_id}:{}", media.len()));
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
    validate_page_url(&url)?;
    Ok(Some(url))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CursorState {
    url: Url,
    index: usize,
}

fn encode_cursor(url: &Url, index: usize) -> Result<String, SourceError> {
    validate_page_url(url)?;
    let mut url = url.clone();
    url.set_fragment(Some(&format!("picto={index}")));
    let cursor = url.to_string();
    CURSOR.validate(&cursor)?;
    Ok(cursor)
}

fn decode_cursor(raw: &str) -> Result<CursorState, SourceError> {
    CURSOR.validate(raw)?;
    let mut url = Url::parse(raw).map_err(|_| invalid_cursor())?;
    let fragment = url.fragment().ok_or_else(invalid_cursor)?;
    let index = fragment
        .strip_prefix("picto=")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|index| *index < 100)
        .ok_or_else(invalid_cursor)?;
    url.set_fragment(None);
    validate_page_url(&url)?;
    Ok(CursorState { url, index })
}

fn validate_page_url(url: &Url) -> Result<(), SourceError> {
    if url.scheme() != "https"
        || url.host_str() != Some(DOMAIN)
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
            || !matches!(
                url.host_str(),
                Some("subscribestar.art" | "subscribestar.com" | "www.subscribestar.com")
            )
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

fn canonical_media_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = if raw.starts_with('/') {
        Url::parse("https://subscribestar.art")
            .ok()?
            .join(raw)
            .ok()?
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

fn is_archive(raw: &str) -> bool {
    let path = raw.split('?').next().unwrap_or(raw).to_ascii_lowercase();
    [".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz"]
        .iter()
        .any(|extension| path.ends_with(extension))
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
    fn page_exposes_exactly_one_post_and_advances_within_the_same_page() {
        let html = include_str!("../../tests/fixtures/subscribestar/profile.html");
        let cursor = CursorState {
            url: profile_url("creator-name"),
            index: 0,
        };
        let batch = normalize_page(&request(), "creator-name", &cursor, html).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert_eq!(batch.posts[0].stable_id, "778899");
        let next = decode_cursor(batch.posts[0].resume_cursor_after.as_deref().unwrap()).unwrap();
        assert_eq!(next.index, 1);
        assert_eq!(next.url, cursor.url);
    }

    #[test]
    fn maps_gallery_documents_audio_tags_and_text() {
        let html = include_str!("../../tests/fixtures/subscribestar/profile.html");
        let cursor = CursorState {
            url: profile_url("creator-name"),
            index: 0,
        };
        let post = normalize_page(&request(), "creator-name", &cursor, html)
            .unwrap()
            .posts
            .remove(0);
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
    fn cursor_rejects_cross_domain_pages_and_large_indexes() {
        assert!(decode_cursor("https://evil.test/page#picto=0").is_err());
        assert!(decode_cursor("https://subscribestar.art/page#picto=100").is_err());
    }

    #[test]
    fn pagination_json_is_unwrapped_without_a_second_transport() {
        let json = include_str!("../../tests/fixtures/subscribestar/page.json");
        let html = unwrap_html_response(json).unwrap();
        assert!(html.contains("data-id=\"9900\""));
    }
}
