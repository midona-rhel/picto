use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, PageCursor, PostFuture,
    ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: PageCursor = PageCursor::new(1_000_000);
const POSTS_PER_PAGE: u32 = 25;

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    HentaiFoundrySource
}

struct HentaiFoundrySource;

impl NativeSourceAdapter for HentaiFoundrySource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "hentaifoundry",
            display_name: "Hentai Foundry",
            domain: "hentai-foundry.com",
            partitions: &["pictures"],
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
            let html =
                visible_text(gallery_url(&username, offset)?, credentials, http, cancel).await?;
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
                invalid_response("Hentai Foundry post is missing its canonical URL")
            })?;
            let mut request_url =
                Url::parse(canonical_url).map_err(|error| invalid_response(error.to_string()))?;
            request_url.query_pairs_mut().append_pair("enterAgree", "1");
            let html = visible_text(request_url, credentials, http, cancel).await?;
            resolve_html(post, &html)
        })
    }
}

async fn visible_text(
    url: Url,
    credentials: &RequestCredentials,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<String, SourceError> {
    ensure_site_filters(credentials, http, cancel, false).await?;
    let mut html = http.get_text(url.clone(), credentials, cancel).await?;
    if is_entry_page(&html) {
        // An expired captured PHP session overrides reqwest's fresh cookie jar.
        // Recover with the new anonymous session the entry request established.
        let recovered = without_stored_session(credentials);
        ensure_site_filters(&recovered, http, cancel, true).await?;
        html = http.get_text(url, &recovered, cancel).await?;
    }
    if is_entry_page(&html) {
        return Err(authentication_required());
    }
    Ok(html)
}

async fn ensure_site_filters(
    credentials: &RequestCredentials,
    http: &HttpRuntime,
    cancel: &CancellationToken,
    force: bool,
) -> Result<(), SourceError> {
    let entry = Url::parse("https://www.hentai-foundry.com/?enterAgree=1")
        .expect("static Hentai Foundry entry URL");
    if !force
        && (credentials.cookies.contains_key("PHPSESSID")
            || http.cookie_value(&entry, "PHPSESSID").is_some())
    {
        return Ok(());
    }
    http.head(entry.clone(), credentials, cancel).await?;
    let csrf = credentials
        .cookies
        .get("YII_CSRF_TOKEN")
        .cloned()
        .or_else(|| http.cookie_value(&entry, "YII_CSRF_TOKEN"))
        .map(|value| decode_cookie_value(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::Authentication,
                "Hentai Foundry did not establish a content-filter session",
                false,
            )
        })?;
    let mut form = BTreeMap::new();
    for field in [
        "rating_nudity",
        "rating_violence",
        "rating_profanity",
        "rating_racism",
        "rating_sex",
        "rating_spoilers",
    ] {
        form.insert(field.to_string(), "3".to_string());
    }
    for field in [
        "rating_yaoi",
        "rating_yuri",
        "rating_teen",
        "rating_guro",
        "rating_furry",
        "rating_beast",
        "rating_male",
        "rating_female",
        "rating_futa",
        "rating_other",
        "rating_scat",
        "rating_incest",
        "rating_rape",
    ] {
        form.insert(field.to_string(), "1".to_string());
    }
    form.insert("filter_order".into(), "date_new".into());
    form.insert("filter_type".into(), "0".into());
    form.insert("YII_CSRF_TOKEN".into(), csrf);
    form.insert("yt0".into(), "Apply".into());
    http.post_form_text(
        Url::parse("https://www.hentai-foundry.com/?enterAgree=1")
            .expect("static Hentai Foundry filters URL"),
        credentials,
        &form,
        cancel,
    )
    .await?;
    Ok(())
}

fn without_stored_session(credentials: &RequestCredentials) -> RequestCredentials {
    let mut recovered = credentials.clone();
    recovered.cookies.clear();
    recovered
        .headers
        .retain(|name, _| !name.eq_ignore_ascii_case("cookie"));
    recovered
}

fn decode_cookie_value(value: &str) -> String {
    let encoded = format!("value={value}");
    let decoded = url::form_urlencoded::parse(encoded.as_bytes())
        .find_map(|(key, value)| (key == "value").then(|| value.into_owned()))
        .unwrap_or_default();
    decoded
        .split_once('"')
        .and_then(|(_, value)| value.split_once('"').map(|(value, _)| value))
        .unwrap_or(decoded.trim_matches('"'))
        .to_string()
}

fn gallery_url(username: &str, offset: u32) -> Result<Url, SourceError> {
    let page = offset / POSTS_PER_PAGE + 1;
    let mut url = Url::parse("https://www.hentai-foundry.com").expect("static Hentai Foundry URL");
    url.path_segments_mut()
        .map_err(|_| invalid_response("invalid Hentai Foundry gallery URL"))?
        .extend(["pictures", "user", username, "page", &page.to_string()]);
    url.query_pairs_mut().append_pair("enterAgree", "1");
    Ok(url)
}

fn normalize_gallery(
    request: &DiscoveryRequest,
    username: &str,
    offset: u32,
    html: &str,
) -> Result<DiscoveryBatch, SourceError> {
    let page_index = (offset % POSTS_PER_PAGE) as usize;
    let mut paths = gallery_paths(html, username);
    if paths.is_empty() {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    }
    if page_index >= paths.len() {
        return Err(invalid_response(
            "Hentai Foundry gallery changed before its persisted cursor",
        ));
    }

    let page_len = paths.len();
    let available = page_len - page_index;
    let take = available.min(request.page_size.max(1) as usize);
    let posts = paths
        .drain(page_index..page_index + take)
        .enumerate()
        .map(|(index, path)| {
            discovered_post(
                request,
                username,
                path,
                offset.saturating_add(index as u32).saturating_add(1),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let page_is_last = page_len < POSTS_PER_PAGE as usize;
    let exhausted = page_is_last && page_index + take >= page_len;
    Ok(DiscoveryBatch { posts, exhausted })
}

fn discovered_post(
    request: &DiscoveryRequest,
    username: &str,
    path: String,
    next_offset: u32,
) -> Result<SourcePost, SourceError> {
    let stable_id = post_id_from_path(&path)
        .ok_or_else(|| invalid_response("invalid Hentai Foundry post path"))?;
    let canonical_url = Url::parse("https://www.hentai-foundry.com")
        .expect("static Hentai Foundry URL")
        .join(&path)
        .map_err(|error| invalid_response(error.to_string()))?
        .to_string();
    Ok(SourcePost {
        site_id: "hentaifoundry".to_string(),
        partition: request.partition.clone(),
        stable_id,
        canonical_url: Some(canonical_url),
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
    let picture = capture(html, picture_regex(), 1)
        .ok_or_else(|| invalid_response("Hentai Foundry post has no picture section"))?;
    let source = capture(picture, movie_regex(), 1)
        .or_else(|| capture(picture, image_regex(), 1))
        .map(decode_attribute);
    let canonical_url = post
        .canonical_url
        .as_deref()
        .ok_or_else(|| invalid_response("Hentai Foundry post is missing its canonical URL"))?;

    post.creator = capture(html, creator_regex(), 1)
        .and_then(normalize_source_text)
        .or(post.creator);
    post.name = capture(html, title_regex(), 1).and_then(normalize_source_text);
    post.notes = capture(html, description_regex(), 1).and_then(normalize_source_text);
    post.created_at = capture(html, date_regex(), 1).map(ToOwned::to_owned);
    post.tags = canonical_tags(html, post.creator.as_deref());
    post.media = source
        .map(|source| media_url(&source))
        .transpose()?
        .map(|source| {
            let file_name = file_name(&source)
                .unwrap_or_else(|| format!("hentaifoundry_{}.media", post.stable_id));
            let headers = BTreeMap::from([("Referer".to_string(), canonical_url.to_string())]);
            MediaDescriptorBuilder::new(format!("hentaifoundry:{}:0", post.stable_id), 0, source)
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
    if let Some(body) = capture(html, ratings_regex(), 1) {
        for captures in rating_regex().captures_iter(body) {
            if let Some(value) = captures
                .get(1)
                .and_then(|value| normalize_source_text(value.as_str()))
            {
                tags.insert("rating", value);
            }
        }
    }
    for regex in [categories_regex(), tags_regex()] {
        if let Some(body) = capture(html, regex, 1) {
            for captures in anchor_regex().captures_iter(body) {
                if let Some(value) = captures
                    .get(1)
                    .and_then(|value| normalize_source_text(value.as_str()))
                {
                    tags.insert("", value);
                }
            }
        }
    }
    tags.into_vec()
}

fn gallery_paths(html: &str, username: &str) -> Vec<String> {
    gallery_path_regex()
        .captures_iter(html)
        .filter_map(|captures| {
            let path = captures.get(1)?.as_str();
            let path_user = captures.get(2)?.as_str();
            path_user
                .eq_ignore_ascii_case(username)
                .then(|| path.to_string())
        })
        .collect()
}

fn normalize_username(raw: &str) -> Result<String, SourceError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_query(
            "Hentai Foundry subscriptions require a username",
        ));
    }
    let username = if let Ok(url) = Url::parse(raw) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("hentai-foundry.com" | "www.hentai-foundry.com")
            )
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "Hentai Foundry subscriptions require a canonical user URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            ["pictures", "user", username] | ["user", username, "profile"] => {
                (*username).to_string()
            }
            _ => {
                return Err(invalid_query(
                    "Hentai Foundry subscriptions require a user gallery or profile URL",
                ))
            }
        }
    } else {
        raw.to_string()
    };
    if username.len() > 128
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid_query(
            "Hentai Foundry subscriptions require a safe username slug",
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

fn post_id_from_path(path: &str) -> Option<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .nth(3)
        .filter(|id| id.bytes().all(|byte| byte.is_ascii_digit()))
        .map(ToOwned::to_owned)
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

fn is_entry_page(html: &str) -> bool {
    html.contains("id=\"entryButtonContainer\"")
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

regex_fn!(
    gallery_path_regex,
    r#"thumbTitle\"><a href=\"((?:/pictures/user/([^/\"]+)/\d+/)[^\"]*)\""#
);
regex_fn!(
    picture_regex,
    r#"(?s)<section[^>]+id=\"picBox\"[^>]*>(.*?)</section>"#
);
regex_fn!(movie_regex, r#"name=\"movie\"\s+value=\"([^\"]+)\""#);
regex_fn!(image_regex, r#"<img[^>]+src=\"([^\"]+)\""#);
regex_fn!(title_regex, r#"class=\"imageTitle\">([^<]+)"#);
regex_fn!(
    creator_regex,
    r#"<small>by</small>\s*<a href=\"/user/([^/\"]+)/profile\""#
);
regex_fn!(
    description_regex,
    r#"(?s)<div class=['\"]picDescript['\"]>(.*?)</div>"#
);
regex_fn!(date_regex, r#"<time datetime=['\"]([^'\"]+)['\"]"#);
regex_fn!(
    ratings_regex,
    r#"(?s)<div class=['\"]ratings_box['\"]>(.*?)</div>"#
);
regex_fn!(rating_regex, r#"title=['\"]([^'\"]+)['\"]"#);
regex_fn!(
    categories_regex,
    r#"(?s)<span class=\"categoryBreadcrumbs\">(.*?)</span>"#
);
regex_fn!(
    tags_regex,
    r#"(?s)<div class=['\"]tagsContainer['\"]>(.*?)</div>"#
);
regex_fn!(anchor_regex, r#"(?s)<a[^>]*>(.*?)</a>"#);

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, true)
}

fn authentication_required() -> SourceError {
    SourceError::new(
        SourceErrorKind::Authentication,
        "Hentai Foundry requires an accepted content session",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const GALLERY: &str = include_str!("../../tests/fixtures/hentaifoundry/gallery.html");
    const POST: &str = include_str!("../../tests/fixtures/hentaifoundry/post.html");

    fn request(cursor: Option<&str>, page_size: u32) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "Example-Artist".to_string(),
            partition: SourcePartition::new("pictures"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size,
        }
    }

    #[test]
    fn accepts_only_safe_usernames_and_canonical_urls() {
        assert_eq!(
            normalize_username("Example-Artist").unwrap(),
            "Example-Artist"
        );
        assert_eq!(
            normalize_username("https://www.hentai-foundry.com/pictures/user/Example-Artist")
                .unwrap(),
            "Example-Artist"
        );
        assert!(normalize_username("artist/name").is_err());
        assert!(
            normalize_username("https://www.hentai-foundry.com/pictures/user/artist?escape=1")
                .is_err()
        );
    }

    #[test]
    fn maps_bounded_gallery_pages_to_settlement_cursors() {
        let batch =
            normalize_gallery(&request(Some("1"), 1), "Example-Artist", 1, GALLERY).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(batch.exhausted);
        assert_eq!(batch.posts[0].stable_id, "1200001");
        assert_eq!(batch.posts[0].resume_cursor_after.as_deref(), Some("2"));
        assert_eq!(
            gallery_url("Example-Artist", 25).unwrap().path(),
            "/pictures/user/Example-Artist/page/2"
        );
    }

    #[test]
    fn maps_post_media_and_canonical_groups() {
        let discovered = normalize_gallery(&request(None, 10), "Example-Artist", 0, GALLERY)
            .unwrap()
            .posts
            .remove(0);
        let post = resolve_html(discovered, POST).unwrap();
        assert_eq!(post.name.as_deref(), Some("Picture title"));
        assert_eq!(post.creator.as_deref(), Some("Example-Artist"));
        assert_eq!(post.notes.as_deref(), Some("Prose & more"));
        assert_eq!(
            post.created_at.as_deref(),
            Some("2026-08-29T17:18:59-07:00")
        );
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "Example-Artist")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "Adult")));
        assert!(post.tags.contains(&CanonicalTag::new("", "Original")));
        assert!(post.tags.contains(&CanonicalTag::new("", "solo")));
        assert_eq!(post.media.len(), 1);
        assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/png"));
        assert_eq!(
            post.media[0].headers.get("Referer"),
            post.canonical_url.as_ref()
        );
    }

    #[test]
    fn decodes_the_site_csrf_cookie() {
        assert_eq!(decode_cookie_value("%22token-value%22"), "token-value");
        assert_eq!(
            decode_cookie_value("hash%3A88%3A%22serialized-token%22%3B"),
            "serialized-token"
        );
    }

    #[test]
    fn recovery_drops_only_the_stale_stored_session() {
        let credentials = RequestCredentials {
            headers: BTreeMap::from([
                ("Cookie".into(), "PHPSESSID=stale".into()),
                ("Accept".into(), "text/html".into()),
            ]),
            cookies: BTreeMap::from([("PHPSESSID".into(), "stale".into())]),
            allowed_domains: ["hentai-foundry.com".into()].into_iter().collect(),
            ..RequestCredentials::default()
        };
        let recovered = without_stored_session(&credentials);
        assert!(recovered.cookies.is_empty());
        assert!(!recovered
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("cookie")));
        assert_eq!(
            recovered.headers.get("Accept").map(String::as_str),
            Some("text/html")
        );
        assert_eq!(recovered.allowed_domains, credentials.allowed_domains);
    }

    #[test]
    fn rejects_invalid_or_out_of_range_cursors() {
        assert!(current_offset(&request(Some("-1"), 1)).is_err());
        assert!(current_offset(&request(Some("1000001"), 1)).is_err());
    }
}
