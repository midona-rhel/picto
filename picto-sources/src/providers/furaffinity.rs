use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, PageCursor, PostFuture,
    ProviderDescriptor, RatingMap, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: PageCursor = PageCursor::new(1_000_000);
const POSTS_PER_PAGE: u32 = 48;
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
            let offset = current_offset(request)?;
            let html = http
                .get_text(gallery_url(&username, offset)?, credentials, cancel)
                .await?;
            if is_login_page(&html) {
                return Err(authentication_required());
            }
            normalize_gallery(request, &username, offset, &html)
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

fn gallery_url(username: &str, offset: u32) -> Result<Url, SourceError> {
    let page = offset / POSTS_PER_PAGE + 1;
    let mut url = Url::parse("https://www.furaffinity.net").expect("static Fur Affinity URL");
    url.path_segments_mut()
        .map_err(|_| invalid_response("invalid Fur Affinity gallery URL"))?
        .extend(["gallery", username, &page.to_string()]);
    Ok(url)
}

fn normalize_gallery(
    request: &DiscoveryRequest,
    username: &str,
    offset: u32,
    html: &str,
) -> Result<DiscoveryBatch, SourceError> {
    let page_index = (offset % POSTS_PER_PAGE) as usize;
    let ids = gallery_post_ids(html);
    if ids.is_empty() {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    }
    if page_index >= ids.len() {
        return Err(invalid_response(
            "Fur Affinity gallery changed before its persisted cursor",
        ));
    }
    let take = (ids.len() - page_index).min(request.page_size.max(1) as usize);
    let posts = ids[page_index..page_index + take]
        .iter()
        .enumerate()
        .map(|(index, id)| {
            discovered_post(
                request,
                username,
                id,
                offset.saturating_add(index as u32).saturating_add(1),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let exhausted = ids.len() < POSTS_PER_PAGE as usize && page_index + take >= ids.len();
    Ok(DiscoveryBatch { posts, exhausted })
}

fn discovered_post(
    request: &DiscoveryRequest,
    username: &str,
    stable_id: &str,
    next_offset: u32,
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
        resume_cursor_after: Some(CURSOR.encode(next_offset)?),
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

fn current_offset(request: &DiscoveryRequest) -> Result<u32, SourceError> {
    request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| CURSOR.validate(cursor))
        .unwrap_or(Ok(0))
}

fn media_url(raw: &str) -> Result<String, SourceError> {
    if raw.starts_with("//") {
        return Ok(format!("https:{raw}"));
    }
    Url::parse(raw)
        .map(|url| url.to_string())
        .map_err(|error| invalid_response(error.to_string()))
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
    download_regex,
    r#"(?s)<a[^>]+href=\"([^\"]+)\"[^>]*>\s*Download\s*</a>"#
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
        let batch =
            normalize_gallery(&request(Some("1"), 1), "Example_Artist", 1, GALLERY).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(batch.exhausted);
        assert_eq!(batch.posts[0].stable_id, "123455");
        assert_eq!(batch.posts[0].resume_cursor_after.as_deref(), Some("2"));
        assert_eq!(
            gallery_url("Example_Artist", 48).unwrap().path(),
            "/gallery/Example_Artist/2"
        );
    }

    #[test]
    fn maps_post_media_text_and_canonical_groups() {
        let discovered = normalize_gallery(&request(None, 10), "Example_Artist", 0, GALLERY)
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
        assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/png"));
        assert_eq!(
            post.media[0].headers.get("Referer"),
            post.canonical_url.as_ref()
        );
    }

    #[test]
    fn missing_download_is_a_traversed_post_without_usable_media() {
        let discovered = normalize_gallery(&request(None, 10), "Example_Artist", 0, GALLERY)
            .unwrap()
            .posts
            .remove(0);
        let post = resolve_html(discovered, "<html><body>System Message</body></html>").unwrap();
        assert!(post.media.is_empty());
    }

    #[test]
    fn rejects_invalid_or_out_of_range_cursors() {
        assert!(current_offset(&request(Some("-1"), 1)).is_err());
        assert!(current_offset(&request(Some("1000001"), 1)).is_err());
    }
}
