use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, PostFuture,
    ProviderDescriptor, RatingMap, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: OpaqueCursor = OpaqueCursor::new(64);
const MAX_PAGE: u32 = 1_000_000;
const MAX_PAGE_INDEX: u16 = 10_000;
const PAGE_CAPACITY: usize = 48;
const RATINGS: RatingMap = RatingMap::new(&[
    ("general", "safe"),
    ("mature", "questionable"),
    ("adult", "explicit"),
]);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    FurAffinitySource
}

struct FurAffinitySource;

impl NativeSourceAdapter for FurAffinitySource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "furaffinity",
            display_name: "Fur Affinity",
            domain: "furaffinity.net",
            partitions: &["gallery"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_username(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let username = normalize_username(&request.query)?;
            let cursor = current_cursor(request)?;
            let html = http
                .get_text(gallery_url(&username, cursor.page)?, credentials, cancel)
                .await?;
            if is_login_page(&html) {
                return Err(authentication_required());
            }
            normalize_gallery(request, &username, cursor, &html)
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
            let canonical_url = post.canonical_url.as_deref().ok_or_else(|| {
                invalid_response("Fur Affinity post is missing its canonical URL")
            })?;
            let url =
                Url::parse(canonical_url).map_err(|error| invalid_response(error.to_string()))?;
            let html = http.get_text(url, credentials, cancel).await?;
            if is_login_page(&html) {
                return Err(authentication_required());
            }
            resolve_html(post, &html)
        })
    }
}

fn gallery_url(username: &str, page: u32) -> Result<Url, SourceError> {
    validate_cursor(CursorState { page, index: 0 })?;
    let mut url = Url::parse("https://www.furaffinity.net").expect("static Fur Affinity URL");
    url.path_segments_mut()
        .map_err(|_| invalid_response("invalid Fur Affinity gallery URL"))?
        .extend(["gallery", username, &page.to_string()]);
    Ok(url)
}

fn normalize_gallery(
    request: &DiscoveryRequest,
    username: &str,
    cursor: CursorState,
    html: &str,
) -> Result<DiscoveryBatch, SourceError> {
    validate_cursor(cursor)?;
    let page_index = cursor.index as usize;
    let ids = gallery_post_ids(html);
    if ids.is_empty() {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    }
    if page_index > ids.len() {
        return Err(invalid_response(
            "Fur Affinity gallery changed before its persisted cursor",
        ));
    }
    let has_next = ids.len() >= PAGE_CAPACITY || has_next_gallery_page(html, username, cursor.page);
    if page_index == ids.len() {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: !has_next,
        });
    }
    let last_on_page = page_index + 1 == ids.len();
    let next = if last_on_page && has_next {
        CursorState {
            page: cursor.page.saturating_add(1),
            index: 0,
        }
    } else {
        CursorState {
            page: cursor.page,
            index: cursor.index.saturating_add(1),
        }
    };
    let posts = vec![discovered_post(request, username, &ids[page_index], next)?];
    let exhausted = last_on_page && !has_next;
    Ok(DiscoveryBatch { posts, exhausted })
}

fn discovered_post(
    request: &DiscoveryRequest,
    username: &str,
    stable_id: &str,
    next_cursor: CursorState,
) -> Result<SourcePost, SourceError> {
    Ok(SourcePost {
        site_id: "furaffinity".to_string(),
        partition: request.partition.clone(),
        stable_id: stable_id.to_string(),
        canonical_url: Some(format!("https://www.furaffinity.net/view/{stable_id}/")),
        creator: Some(username.to_string()),
        name: None,
        notes: None,
        created_at: None,
        tags: Vec::new(),
        media: Vec::new(),
        resume_cursor_after: Some(encode_cursor(next_cursor)?),
    })
}

fn resolve_html(mut post: SourcePost, html: &str) -> Result<SourcePost, SourceError> {
    let canonical_url = post
        .canonical_url
        .as_deref()
        .ok_or_else(|| invalid_response("Fur Affinity post is missing its canonical URL"))?;
    post.creator = capture(html, creator_regex(), 1)
        .and_then(normalize_source_text)
        .or(post.creator);
    post.name = capture(html, title_regex(), 1).and_then(normalize_source_text);
    post.notes = capture(html, description_regex(), 1).and_then(normalize_source_text);
    post.created_at = capture(html, timestamp_regex(), 1).map(ToOwned::to_owned);
    post.tags = canonical_tags(html, post.creator.as_deref());
    post.media = capture(html, download_regex(), 1)
        .map(decode_attribute)
        .map(|raw| media_url(&raw))
        .transpose()?
        .map(|source| {
            let file_name = file_name(&source)
                .unwrap_or_else(|| format!("furaffinity_{}.media", post.stable_id));
            let headers = BTreeMap::from([("Referer".to_string(), canonical_url.to_string())]);
            MediaDescriptorBuilder::new(format!("furaffinity:{}:0", post.stable_id), 0, source)
                .canonical_url(canonical_url)
                .file_name(file_name)
                .headers(headers)
                .build()
        })
        .into_iter()
        .collect();
    Ok(post)
}

fn canonical_tags(html: &str, creator: Option<&str>) -> Vec<crate::CanonicalTag> {
    let mut tags = CanonicalTagSet::default();
    if let Some(creator) = creator {
        tags.insert("creator", creator);
    }
    RATINGS.add(&mut tags, capture(html, rating_regex(), 1));

    if let Some(attributes) = capture(html, submission_image_regex(), 1) {
        if let Some(raw) = capture(attributes, data_tags_regex(), 1) {
            for value in raw.split_whitespace() {
                if let Some(value) = value.strip_prefix("u_") {
                    if creator.is_none() {
                        tags.insert("creator", value);
                    }
                } else if let Some(value) = value.strip_prefix("s_") {
                    tags.insert("species", value);
                } else if let Some(value) = value
                    .strip_prefix("c_")
                    .or_else(|| value.strip_prefix("t_"))
                {
                    tags.insert("", value);
                } else {
                    tags.insert("", value);
                }
            }
        }
    }
    for captures in keyword_regex().captures_iter(html) {
        if let Some(value) = captures
            .get(1)
            .and_then(|value| normalize_source_text(value.as_str()))
        {
            tags.insert("", value);
        }
    }
    tags.into_vec()
}

fn gallery_post_ids(html: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    gallery_id_regex()
        .captures_iter(html)
        .filter_map(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn has_next_gallery_page(html: &str, username: &str, page: u32) -> bool {
    let expected = page.saturating_add(1);
    gallery_page_regex().captures_iter(html).any(|captures| {
        captures
            .get(1)
            .is_some_and(|value| value.as_str().eq_ignore_ascii_case(username))
            && captures
                .get(2)
                .and_then(|value| value.as_str().parse::<u32>().ok())
                == Some(expected)
    })
}

fn normalize_username(raw: &str) -> Result<String, SourceError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_query(
            "Fur Affinity subscriptions require a username",
        ));
    }
    let username = if let Ok(url) = Url::parse(raw) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("furaffinity.net" | "www.furaffinity.net")
            )
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "Fur Affinity subscriptions require a canonical gallery URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            ["gallery", username] | ["user", username] => (*username).to_string(),
            _ => {
                return Err(invalid_query(
                    "Fur Affinity subscriptions require a user or gallery URL",
                ))
            }
        }
    } else {
        raw.strip_prefix('@').unwrap_or(raw).to_string()
    };
    if username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid_query(
            "Fur Affinity subscriptions require a safe username",
        ));
    }
    Ok(username)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CursorState {
    page: u32,
    index: u16,
}

fn current_cursor(request: &DiscoveryRequest) -> Result<CursorState, SourceError> {
    let Some(raw) = request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
    else {
        return Ok(CursorState { page: 1, index: 0 });
    };
    let raw = CURSOR.validate(raw)?;
    let cursor = serde_json::from_str(raw).map_err(|_| invalid_cursor())?;
    validate_cursor(cursor)?;
    Ok(cursor)
}

fn encode_cursor(cursor: CursorState) -> Result<String, SourceError> {
    validate_cursor(cursor)?;
    let raw = serde_json::to_string(&cursor).map_err(|_| invalid_cursor())?;
    CURSOR.validate(&raw)?;
    Ok(raw)
}

fn validate_cursor(cursor: CursorState) -> Result<(), SourceError> {
    if cursor.page == 0 || cursor.page > MAX_PAGE || cursor.index > MAX_PAGE_INDEX {
        return Err(invalid_cursor());
    }
    Ok(())
}

fn media_url(raw: &str) -> Result<String, SourceError> {
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let url = Url::parse(&raw).map_err(|error| invalid_response(error.to_string()))?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !host.starts_with('d')
        || !(host.ends_with(".furaffinity.net") || host.ends_with(".facdn.net"))
    {
        return Err(invalid_response(
            "Fur Affinity returned a non-download media URL",
        ));
    }
    Ok(url.to_string())
}

fn file_name(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn decode_attribute(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
}

fn is_login_page(html: &str) -> bool {
    !html.contains("id=\"sid-")
        && (html.contains("action=\"/login/") || html.contains("name=\"action\" value=\"login\""))
}

fn capture<'a>(text: &'a str, regex: &Regex, group: usize) -> Option<&'a str> {
    regex.captures(text)?.get(group).map(|value| value.as_str())
}

macro_rules! regex_fn {
    ($name:ident, $pattern:literal) => {
        fn $name() -> &'static Regex {
            static VALUE: OnceLock<Regex> = OnceLock::new();
            VALUE.get_or_init(|| Regex::new($pattern).expect("valid provider regex"))
        }
    };
}

regex_fn!(gallery_id_regex, r#"id=\"sid-([0-9]+)\""#);
regex_fn!(
    gallery_page_regex,
    r#"(?i)href=\"(?:https://www\.furaffinity\.net)?/gallery/([^/\"?#]+)/([0-9]+)/?[^\"]*\""#
);
regex_fn!(
    download_regex,
    r#"(?s)<a[^>]+href=\"((?:https:)?//d[^\"]+)\"[^>]*>"#
);
regex_fn!(title_regex, r#"data-artwork-title=\"([^\"]+)\""#);
regex_fn!(
    creator_regex,
    r#"(?s)submission-description-artist.*?<a href=\"/user/([^/\"]+)/\""#
);
regex_fn!(
    description_regex,
    r#"(?s)submission-description-text user-submitted-links\">(.*?)(?:<div class=\"submission-footer\"|</div>\s*</div>\s*</section>)"#
);
regex_fn!(timestamp_regex, r#"(?s)Posted.*?data-time=\"([0-9]+)\""#);
regex_fn!(rating_regex, r#"c-contentRating--([A-Za-z]+)"#);
regex_fn!(
    submission_image_regex,
    r#"(?s)<img([^>]*id=\"submissionImg\"[^>]*)>"#
);
regex_fn!(data_tags_regex, r#"data-tags=\"([^\"]*)\""#);
regex_fn!(keyword_regex, r#"data-tag-name=\"([^\"]+)\""#);

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, true)
}

fn invalid_cursor() -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidQuery,
        "invalid Fur Affinity cursor",
        false,
    )
}

fn authentication_required() -> SourceError {
    SourceError::new(
        SourceErrorKind::Authentication,
        "Fur Affinity requires a valid site session",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const GALLERY: &str = include_str!("../../tests/fixtures/furaffinity/gallery.html");
    const POST: &str = include_str!("../../tests/fixtures/furaffinity/post.html");

    fn request(cursor: Option<&str>, page_size: u32) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "Example_Artist".to_string(),
            partition: SourcePartition::new("gallery"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size,
        }
    }

    #[test]
    fn accepts_only_safe_usernames_and_canonical_urls() {
        assert_eq!(
            normalize_username("@Example_Artist").unwrap(),
            "Example_Artist"
        );
        assert_eq!(
            normalize_username("https://www.furaffinity.net/gallery/Example_Artist/").unwrap(),
            "Example_Artist"
        );
        assert!(normalize_username("artist/name").is_err());
        assert!(normalize_username("https://www.furaffinity.net/gallery/artist?escape=1").is_err());
    }

    #[test]
    fn maps_bounded_gallery_pages_to_settlement_cursors() {
        let cursor = CursorState { page: 1, index: 1 };
        let batch = normalize_gallery(
            &request(Some(&encode_cursor(cursor).unwrap()), 1),
            "Example_Artist",
            cursor,
            GALLERY,
        )
        .unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(batch.exhausted);
        assert_eq!(batch.posts[0].stable_id, "123455");
        assert_eq!(
            decode_test_cursor(batch.posts[0].resume_cursor_after.as_deref().unwrap()),
            CursorState { page: 1, index: 2 }
        );
        assert_eq!(
            gallery_url("Example_Artist", 2).unwrap().path(),
            "/gallery/Example_Artist/2"
        );
    }

    #[test]
    fn short_nonterminal_page_advances_to_the_explicit_next_page() {
        let html = r#"
            <figure id="sid-10"></figure>
            <figure id="sid-9"></figure>
            <a href="/gallery/Example_Artist/2/">Next</a>
        "#;
        let batch = normalize_gallery(
            &request(None, 1),
            "Example_Artist",
            CursorState { page: 1, index: 1 },
            html,
        )
        .unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(!batch.exhausted);
        assert_eq!(batch.posts[0].stable_id, "9");
        assert_eq!(
            decode_test_cursor(batch.posts[0].resume_cursor_after.as_deref().unwrap()),
            CursorState { page: 2, index: 0 }
        );
    }

    #[test]
    fn maps_post_media_text_and_canonical_groups() {
        let discovered = normalize_gallery(
            &request(None, 10),
            "Example_Artist",
            CursorState { page: 1, index: 0 },
            GALLERY,
        )
        .unwrap()
        .posts
        .remove(0);
        let post = resolve_html(discovered, POST).unwrap();
        assert_eq!(post.name.as_deref(), Some("Example work"));
        assert_eq!(post.creator.as_deref(), Some("ExampleArtist"));
        assert_eq!(post.notes.as_deref(), Some("Prose & more"));
        assert_eq!(post.created_at.as_deref(), Some("1788000000"));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "ExampleArtist")));
        assert!(post.tags.contains(&CanonicalTag::new("species", "canine")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "explicit")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("", "artwork_digital")));
        assert!(post.tags.contains(&CanonicalTag::new("", "night")));
        assert_eq!(post.media.len(), 1);
        assert_eq!(
            post.media[0].url,
            "https://d.furaffinity.net/art/example/1788000000/example.png"
        );
        assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/png"));
        assert_eq!(
            post.media[0].headers.get("Referer"),
            post.canonical_url.as_ref()
        );
    }

    #[test]
    fn ignores_a_generic_download_label_that_is_not_the_direct_media_host() {
        let discovered = normalize_gallery(
            &request(None, 10),
            "Example_Artist",
            CursorState { page: 1, index: 0 },
            GALLERY,
        )
        .unwrap()
        .posts
        .remove(0);
        let post = resolve_html(
            discovered,
            r#"<a href="https://www.furaffinity.net/login/">Download</a>"#,
        )
        .unwrap();
        assert!(post.media.is_empty());
    }

    #[test]
    fn missing_download_is_a_traversed_post_without_usable_media() {
        let discovered = normalize_gallery(
            &request(None, 10),
            "Example_Artist",
            CursorState { page: 1, index: 0 },
            GALLERY,
        )
        .unwrap()
        .posts
        .remove(0);
        let post = resolve_html(discovered, "<html><body>System Message</body></html>").unwrap();
        assert!(post.media.is_empty());
    }

    #[test]
    fn rejects_invalid_or_out_of_range_cursors() {
        assert!(current_cursor(&request(Some("-1"), 1)).is_err());
        assert!(current_cursor(&request(Some(r#"{"page":0,"index":0}"#), 1)).is_err());
        assert!(current_cursor(&request(Some(r#"{"page":1000001,"index":0}"#), 1)).is_err());
    }

    fn decode_test_cursor(raw: &str) -> CursorState {
        serde_json::from_str(CURSOR.validate(raw).unwrap()).unwrap()
    }
}
