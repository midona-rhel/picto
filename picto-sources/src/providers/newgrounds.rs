use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, MediaFallback, NativeSourceAdapter, PostFuture,
    ProviderDescriptor, RatingMap, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const MAX_PAGE: u32 = 1_000_000;
const MAX_OFFSET: usize = 10_000;
const MAX_EMPTY_PAGES: u32 = 8;
const RATINGS: RatingMap = RatingMap::new(&[
    ("e", "safe"),
    ("t", "questionable"),
    ("m", "explicit"),
    ("a", "explicit"),
]);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    NewgroundsSource
}

struct NewgroundsSource;

impl NativeSourceAdapter for NewgroundsSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "newgrounds",
            display_name: "Newgrounds",
            domain: "newgrounds.com",
            partitions: &["art"],
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
            let mut cursor = decode_cursor(request.cursor.as_deref())?;
            let credentials = profile_credentials(credentials);

            for _ in 0..MAX_EMPTY_PAGES {
                let response = http
                    .get_json::<ProfilePage>(
                        profile_url(&username, cursor.page)?,
                        &credentials,
                        cancel,
                    )
                    .await?;
                let page = normalize_profile_page(&username, cursor, response)?;
                if !page.posts.is_empty() || page.exhausted {
                    return Ok(page);
                }
                cursor = Cursor {
                    page: cursor.page.saturating_add(1),
                    offset: 0,
                };
                validate_cursor(cursor)?;
            }

            Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "Newgrounds returned too many empty non-terminal profile pages",
                true,
            ))
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
            let canonical_url = post
                .canonical_url
                .as_deref()
                .ok_or_else(|| invalid_response("Newgrounds post is missing its canonical URL"))?;
            let url = validate_post_url(canonical_url)?;
            let credentials = site_credentials(credentials);
            let Some(html) = http.get_optional_text(url, &credentials, cancel).await? else {
                return Ok(post);
            };
            resolve_html(post, &html)
        })
    }
}

fn site_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert("newgrounds.com".into());
    credentials
}

fn profile_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = site_credentials(credentials);
    credentials
        .headers
        .insert("X-Requested-With".into(), "XMLHttpRequest".into());
    credentials
}

fn normalize_username(raw: &str) -> Result<String, SourceError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_query("Newgrounds subscriptions require a username"));
    }

    let username = if let Ok(url) = Url::parse(trimmed) {
        let host = url.host_str().unwrap_or_default();
        let Some(username) = host.strip_suffix(".newgrounds.com") else {
            return Err(invalid_query(
                "Newgrounds subscriptions require a canonical profile URL",
            ));
        };
        if !matches!(url.scheme(), "http" | "https")
            || username.is_empty()
            || username.eq_ignore_ascii_case("www")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.path().trim_end_matches('/'), "" | "/art")
        {
            return Err(invalid_query(
                "Newgrounds subscriptions require a canonical profile URL",
            ));
        }
        username.to_string()
    } else {
        trimmed.trim_start_matches('@').to_string()
    };

    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid_query(
            "Newgrounds subscriptions require a safe username",
        ));
    }
    Ok(username.to_ascii_lowercase())
}

fn profile_url(username: &str, page: u32) -> Result<Url, SourceError> {
    let mut url = Url::parse("https://newgrounds.com/art").expect("static Newgrounds profile URL");
    url.set_host(Some(&format!("{username}.newgrounds.com")))
        .map_err(|_| invalid_query("invalid Newgrounds username"))?;
    url.query_pairs_mut()
        .append_pair("page", &page.to_string())
        .append_pair("isAjaxRequest", "1");
    Ok(url)
}

fn validate_post_url(raw: &str) -> Result<Url, SourceError> {
    let mut url =
        Url::parse(raw).map_err(|_| invalid_response("Newgrounds returned an invalid post URL"))?;
    let host = url.host_str().unwrap_or_default();
    let segments = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>());
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(host, "newgrounds.com" | "www.newgrounds.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(segments.as_deref(), Some(["art", "view", _, _]))
    {
        return Err(invalid_response("Newgrounds returned an invalid art URL"));
    }
    url.set_scheme("https")
        .map_err(|_| invalid_response("Newgrounds returned an invalid art URL"))?;
    url.set_host(Some("www.newgrounds.com"))
        .map_err(|_| invalid_response("Newgrounds returned an invalid art URL"))?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cursor {
    page: u32,
    offset: usize,
}

fn decode_cursor(raw: Option<&str>) -> Result<Cursor, SourceError> {
    let Some(raw) = raw.filter(|cursor| !cursor.is_empty()) else {
        return Ok(Cursor { page: 1, offset: 0 });
    };
    let Some((page, offset)) = raw.strip_prefix('p').and_then(|raw| raw.split_once('i')) else {
        return Err(invalid_query("invalid Newgrounds source cursor"));
    };
    let cursor = Cursor {
        page: page
            .parse()
            .map_err(|_| invalid_query("invalid Newgrounds source cursor"))?,
        offset: offset
            .parse()
            .map_err(|_| invalid_query("invalid Newgrounds source cursor"))?,
    };
    validate_cursor(cursor)
}

fn encode_cursor(cursor: Cursor) -> Result<String, SourceError> {
    let cursor = validate_cursor(cursor)?;
    Ok(format!("p{}i{}", cursor.page, cursor.offset))
}

fn validate_cursor(cursor: Cursor) -> Result<Cursor, SourceError> {
    if cursor.page == 0 || cursor.page > MAX_PAGE || cursor.offset > MAX_OFFSET {
        return Err(invalid_query("invalid Newgrounds source cursor"));
    }
    Ok(cursor)
}

#[derive(Debug, Deserialize)]
struct ProfilePage {
    #[serde(default)]
    items: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    load_more: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<String>,
}

fn normalize_profile_page(
    username: &str,
    cursor: Cursor,
    response: ProfilePage,
) -> Result<DiscoveryBatch, SourceError> {
    if !response.errors.is_empty() {
        return Err(invalid_response(response.errors.join(", ")));
    }

    let links = profile_links(response.items)?;
    let has_more = has_more(response.load_more.as_ref());
    if cursor.offset >= links.len() {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: !has_more,
        });
    }

    let link = &links[cursor.offset];
    let next = if cursor.offset + 1 < links.len() {
        Cursor {
            page: cursor.page,
            offset: cursor.offset + 1,
        }
    } else if has_more {
        Cursor {
            page: cursor.page.saturating_add(1),
            offset: 0,
        }
    } else {
        Cursor {
            page: cursor.page,
            offset: links.len(),
        }
    };
    let canonical_url = validate_post_url(&link.url)?.to_string();
    let stable_id = stable_post_id(&canonical_url)?;

    Ok(DiscoveryBatch {
        posts: vec![SourcePost {
            site_id: "newgrounds".into(),
            partition: crate::SourcePartition::new("art"),
            stable_id: stable_id.clone(),
            canonical_url: Some(canonical_url),
            creator: Some(username.to_string()),
            name: link.title.clone(),
            notes: None,
            created_at: None,
            tags: Vec::new(),
            media: Vec::new(),
            resume_cursor_after: Some(encode_cursor(next)?),
        }],
        exhausted: !has_more && cursor.offset + 1 == links.len(),
    })
}

#[derive(Debug)]
struct ProfileLink {
    url: String,
    title: Option<String>,
}

fn profile_links(items: BTreeMap<String, Vec<String>>) -> Result<Vec<ProfileLink>, SourceError> {
    let mut years = items.into_iter().collect::<Vec<_>>();
    years.sort_by(|(left, _), (right, _)| {
        right
            .parse::<u32>()
            .unwrap_or_default()
            .cmp(&left.parse::<u32>().unwrap_or_default())
    });

    let link = regex(&PROFILE_LINK, r#"(?is)<a\s+[^>]*>"#);
    let href = regex(&PROFILE_HREF, r#"(?is)href="([^"]+)""#);
    let title = regex(&PROFILE_TITLE, r#"(?is)title="([^"]*)""#);
    let mut links = Vec::new();
    for (_, entries) in years {
        for entry in entries {
            let Some(anchor) = link.find(&entry) else {
                continue;
            };
            let Some(url) = capture(href, anchor.as_str()) else {
                continue;
            };
            let url = decode_json_html(&url);
            let title = title
                .captures(anchor.as_str())
                .and_then(|captures| captures.get(1))
                .and_then(|value| normalize_source_text(value.as_str()))
                .filter(|title| title != "Restricted Art");
            links.push(ProfileLink { url, title });
        }
    }
    Ok(links)
}

fn has_more(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::String(value)) => value.len() >= 8,
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::Null) | None => false,
        Some(_) => true,
    }
}

fn stable_post_id(canonical_url: &str) -> Result<String, SourceError> {
    let url = validate_post_url(canonical_url)?;
    let segments = url
        .path_segments()
        .expect("validated Newgrounds URL has path segments")
        .collect::<Vec<_>>();
    Ok(format!("art:{}:{}", segments[2], segments[3]))
}

fn resolve_html(mut post: SourcePost, html: &str) -> Result<SourcePost, SourceError> {
    let canonical_url = post
        .canonical_url
        .as_deref()
        .ok_or_else(|| invalid_response("Newgrounds post is missing its canonical URL"))?;
    let title = capture_attribute(html, "og:title").and_then(|value| normalize_source_text(&value));
    let notes = element_body(html, "author_comments").and_then(normalize_source_text);
    let created_at = capture(
        regex(
            &DATE_PUBLISHED,
            r#"(?is)itemprop="datePublished"\s+content="([^"]+)""#,
        ),
        html,
    );

    let mut tags = CanonicalTagSet::default();
    for tag in parse_tags(html) {
        tags.insert("", tag);
    }
    let creators = parse_creators(html);
    for creator in &creators {
        tags.insert("creator", creator);
    }
    let rating = capture(regex(&RATING, r#"(?i)class="rated-([etma])(?:\s|")"#), html);
    RATINGS.add(&mut tags, rating.as_deref());

    let media_urls = parse_media_urls(html)?;
    let media = media_urls
        .into_iter()
        .enumerate()
        .map(|(position, selected)| {
            let position = u32::try_from(position)
                .map_err(|_| invalid_response("Newgrounds post contains too many media files"))?;
            let file_name = file_name_from_url(&selected.url)
                .unwrap_or_else(|| format!("newgrounds_{}_{}.media", post.stable_id, position));
            let mut builder = MediaDescriptorBuilder::new(
                format!("newgrounds:{}:{position}", post.stable_id),
                position,
                selected.url,
            )
            .canonical_url(canonical_url)
            .file_name(&file_name);
            for fallback in selected.fallbacks {
                let fallback_name =
                    file_name_from_url(&fallback).unwrap_or_else(|| file_name.clone());
                builder = builder.fallback(MediaFallback {
                    url: fallback,
                    mime_hint: mime_guess::from_path(&fallback_name)
                        .first_raw()
                        .map(ToOwned::to_owned),
                    file_name: Some(fallback_name),
                    expected_size: None,
                    html_marker: None,
                });
            }
            Ok(builder.build())
        })
        .collect::<Result<Vec<_>, SourceError>>()?;

    post.creator = creators.first().cloned().or_else(|| post.creator.take());
    post.name = title.or(post.name);
    post.notes = notes;
    post.created_at = created_at;
    post.tags = tags.into_vec();
    post.media = media;
    Ok(post)
}

#[derive(Debug, PartialEq, Eq)]
struct MediaUrl {
    url: String,
    fallbacks: Vec<String>,
}

fn parse_media_urls(html: &str) -> Result<Vec<MediaUrl>, SourceError> {
    let mut urls = Vec::new();
    if let Some(encoded) = capture(
        regex(&FULL_IMAGE, r#"(?s)"full_image_text":"((?:\\.|[^"\\])*)""#),
        html,
    ) {
        if let Ok(decoded) = serde_json::from_str::<String>(&format!("\"{encoded}\"")) {
            if let Some(url) = capture(
                regex(&IMAGE_SRC, r#"(?is)<img\s+[^>]*src="([^"]+)""#),
                &decoded,
            )
            .or_else(|| {
                capture(
                    regex(&IMAGE_HREF, r#"(?is)<a\s+[^>]*href="([^"]+)""#),
                    &decoded,
                )
            }) {
                push_media_url(&mut urls, &url, Vec::new())?;
            }
        }
    }
    if urls.is_empty() {
        if let Some(url) = capture(
            regex(
                &DIRECT_ART_LINK,
                r#"(?is)<a\s+[^>]*href="(https?://(?:art|audio)\.ngfiles\.com/[^"]+)""#,
            ),
            html,
        ) {
            push_media_url(&mut urls, &url, Vec::new())?;
        }
    }

    let mut parsed_image_data = false;
    if let Some(raw) = between(html, "let imageData =", "\n];") {
        if let Ok(values) =
            serde_json::from_str::<Vec<serde_json::Value>>(&format!("{}]", raw.trim()))
        {
            parsed_image_data = true;
            let skip_primary = usize::from(!urls.is_empty());
            for value in values.into_iter().skip(skip_primary) {
                if let Some(url) = value.get("image").and_then(serde_json::Value::as_str) {
                    push_media_url(&mut urls, url, Vec::new())?;
                }
            }
        }
    }
    let art_images = (!parsed_image_data)
        .then(|| {
            between(
                html,
                "<div class=\"art-images",
                "</div>\n\n        <script>",
            )
        })
        .flatten();
    if let Some(container) = art_images {
        let primary_extension = urls
            .first()
            .and_then(|media| media_extension(&media.url))
            .map(ToOwned::to_owned);
        let lazy = regex(&LAZY_IMAGE, r#"(?is)data-smartload-src="([^"]+)""#);
        for captures in lazy.captures_iter(container) {
            let mut url = captures[1].replace("/medium_views/", "/images/");
            let mut fallbacks = Vec::new();
            if media_extension(&url) == Some("webp") {
                if let Some(extension) = primary_extension.as_deref() {
                    let webp = url.clone();
                    url = replace_extension(&webp, extension);
                    fallbacks.extend(
                        ["jpg", "png", "gif"]
                            .into_iter()
                            .filter(|candidate| *candidate != extension)
                            .map(|candidate| replace_extension(&webp, candidate)),
                    );
                    fallbacks.push(webp);
                }
            }
            push_media_url(&mut urls, &url, fallbacks)?;
        }
    }

    if let Some(comments) = element_body(html, "author_comments") {
        let comment_media = regex(
            &COMMENT_MEDIA,
            r#"(?is)(?:data-smartload-)?src="(https?://[^"]+)""#,
        );
        for captures in comment_media.captures_iter(comments) {
            push_media_url(&mut urls, &captures[1], Vec::new())?;
        }
    }
    Ok(urls)
}

fn media_extension(raw: &str) -> Option<&str> {
    raw.split(['?', '#'])
        .next()?
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| !extension.is_empty())
}

fn replace_extension(raw: &str, extension: &str) -> String {
    let suffix_at = raw.find(['?', '#']).unwrap_or(raw.len());
    let (path, suffix) = raw.split_at(suffix_at);
    let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
    format!("{stem}.{extension}{suffix}")
}

fn push_media_url(
    urls: &mut Vec<MediaUrl>,
    raw: &str,
    fallbacks: Vec<String>,
) -> Result<(), SourceError> {
    let url = Url::parse(&decode_json_html(raw))
        .map_err(|_| invalid_response("Newgrounds returned an invalid media URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid_response("Newgrounds returned an invalid media URL"));
    }
    let value = url.to_string();
    if !urls.iter().any(|media| media.url == value) {
        urls.push(MediaUrl {
            url: value,
            fallbacks,
        });
    }
    Ok(())
}

fn parse_tags(html: &str) -> Vec<String> {
    let Some(tags) = between(html, "<dd class=\"tags\">", "</dd>") else {
        return Vec::new();
    };
    regex(&TAG_LINK, r#"(?is)<a\s+[^>]*>([^<]+)</a>"#)
        .captures_iter(tags)
        .filter_map(|captures| normalize_source_text(&captures[1]))
        .collect()
}

fn parse_creators(html: &str) -> Vec<String> {
    let creator = regex(
        &CREATOR_LINK,
        r#"(?is)<div class="item-user">.{0,2000}?href="https://([a-z0-9_-]+)\.newgrounds\.com(?:[/?#"]|$)"#,
    );
    let mut creators = Vec::new();
    for captures in creator.captures_iter(html) {
        let value = captures[1].to_ascii_lowercase();
        if !creators.contains(&value) {
            creators.push(value);
        }
    }
    creators
}

fn capture_attribute(html: &str, property: &str) -> Option<String> {
    let pattern = Regex::new(&format!(
        r#"(?is)<meta\s+[^>]*property="{}"[^>]*content="([^"]*)""#,
        regex::escape(property)
    ))
    .expect("valid static metadata property");
    capture(&pattern, html).map(|value| decode_json_html(&value))
}

fn element_body<'a>(html: &'a str, id: &str) -> Option<&'a str> {
    let marker = format!("id=\"{id}\"");
    let (_, rest) = html.split_once(&marker)?;
    let (_, body) = rest.split_once('>')?;
    body.split_once("</div>").map(|(body, _)| body)
}

fn capture(pattern: &Regex, value: &str) -> Option<String> {
    pattern
        .captures(value)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn between<'a>(value: &'a str, start: &str, end: &str) -> Option<&'a str> {
    value
        .split_once(start)
        .and_then(|(_, value)| value.split_once(end).map(|(value, _)| value))
}

fn decode_json_html(value: &str) -> String {
    value
        .replace("\\/", "/")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn file_name_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

fn regex<'a>(slot: &'a OnceLock<Regex>, pattern: &str) -> &'a Regex {
    slot.get_or_init(|| Regex::new(pattern).expect("valid Newgrounds parser regex"))
}

static PROFILE_LINK: OnceLock<Regex> = OnceLock::new();
static PROFILE_HREF: OnceLock<Regex> = OnceLock::new();
static PROFILE_TITLE: OnceLock<Regex> = OnceLock::new();
static DATE_PUBLISHED: OnceLock<Regex> = OnceLock::new();
static RATING: OnceLock<Regex> = OnceLock::new();
static FULL_IMAGE: OnceLock<Regex> = OnceLock::new();
static IMAGE_SRC: OnceLock<Regex> = OnceLock::new();
static IMAGE_HREF: OnceLock<Regex> = OnceLock::new();
static DIRECT_ART_LINK: OnceLock<Regex> = OnceLock::new();
static LAZY_IMAGE: OnceLock<Regex> = OnceLock::new();
static COMMENT_MEDIA: OnceLock<Regex> = OnceLock::new();
static TAG_LINK: OnceLock<Regex> = OnceLock::new();
static CREATOR_LINK: OnceLock<Regex> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanonicalTag;

    fn profile_fixture() -> ProfilePage {
        serde_json::from_str(include_str!("../../tests/fixtures/newgrounds/profile.json")).unwrap()
    }

    #[test]
    fn preserves_existing_username_and_profile_url_semantics() {
        for input in [
            "Artist_Name",
            "@Artist_Name",
            "https://Artist_Name.newgrounds.com/",
            "https://Artist_Name.newgrounds.com/art",
            "https://Artist_Name.newgrounds.com/art/",
        ] {
            assert_eq!(normalize_username(input).unwrap(), "artist_name");
        }
        for input in [
            "",
            "artist/name",
            "https://www.newgrounds.com/",
            "https://artist.newgrounds.com/?page=2",
            "https://newgrounds.com.evil.example/",
        ] {
            assert!(normalize_username(input).is_err(), "{input}");
        }
    }

    #[test]
    fn exposes_only_one_profile_post_and_advances_after_settlement() {
        let first = normalize_profile_page(
            "artist_name",
            decode_cursor(None).unwrap(),
            profile_fixture(),
        )
        .unwrap();
        assert_eq!(first.posts.len(), 1);
        assert_eq!(first.posts[0].stable_id, "art:artist-name:first-work");
        assert_eq!(first.posts[0].name.as_deref(), Some("First & Best Work"));
        assert_eq!(first.posts[0].resume_cursor_after.as_deref(), Some("p1i1"));
        assert!(!first.exhausted);

        let second = normalize_profile_page(
            "artist_name",
            decode_cursor(Some("p1i1")).unwrap(),
            profile_fixture(),
        )
        .unwrap();
        assert_eq!(second.posts.len(), 1);
        assert_eq!(second.posts[0].stable_id, "art:artist-name:second-work");
        assert_eq!(second.posts[0].resume_cursor_after.as_deref(), Some("p2i0"));
    }

    #[test]
    fn resolves_all_post_media_and_canonical_metadata() {
        let post = normalize_profile_page(
            "artist_name",
            decode_cursor(None).unwrap(),
            profile_fixture(),
        )
        .unwrap()
        .posts
        .into_iter()
        .next()
        .unwrap();
        let post = resolve_html(
            post,
            include_str!("../../tests/fixtures/newgrounds/post.html"),
        )
        .unwrap();

        assert_eq!(post.creator.as_deref(), Some("artist-name"));
        assert_eq!(post.name.as_deref(), Some("First & Best Work"));
        assert_eq!(post.notes.as_deref(), Some("Hello world & more."));
        assert_eq!(
            post.created_at.as_deref(),
            Some("2026-08-25T12:00:00-04:00")
        );
        assert_eq!(post.media.len(), 3);
        assert_eq!(post.media[0].position, 0);
        assert_eq!(post.media[2].position, 2);
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "artist-name")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "collaborator")));
        assert!(post.tags.contains(&CanonicalTag::new("", "pixel-art")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("rating", "questionable")));
    }

    #[test]
    fn restricted_post_remains_traversable_with_no_usable_media() {
        let post = SourcePost {
            site_id: "newgrounds".into(),
            partition: crate::SourcePartition::new("art"),
            stable_id: "art:artist-name:restricted".into(),
            canonical_url: Some(
                "https://www.newgrounds.com/art/view/artist-name/restricted".into(),
            ),
            creator: Some("artist-name".into()),
            name: None,
            notes: None,
            created_at: None,
            tags: Vec::new(),
            media: Vec::new(),
            resume_cursor_after: Some("p1i2".into()),
        };
        let resolved = resolve_html(post, "<html><div id=\"adults_only\"></div></html>").unwrap();
        assert!(resolved.media.is_empty());
    }

    #[test]
    fn falls_back_to_the_direct_anchor_and_restores_multi_image_extensions() {
        let urls = parse_media_urls(
            r#"
            <script>PHP.merge({"full_image_text":"<a href=\"https:\/\/art.ngfiles.com\/images\/1\/primary.png\">full<\/a>"});</script>
            <div class="art-images">
              <img data-smartload-src="https://art.ngfiles.com/medium_views/1/secondary.webp">
            </div>

        <script>"#,
        )
        .unwrap();
        assert_eq!(urls[0].url, "https://art.ngfiles.com/images/1/primary.png");
        assert_eq!(
            urls[1].url,
            "https://art.ngfiles.com/images/1/secondary.png"
        );
        assert_eq!(
            urls[1].fallbacks,
            [
                "https://art.ngfiles.com/images/1/secondary.jpg",
                "https://art.ngfiles.com/images/1/secondary.gif",
                "https://art.ngfiles.com/images/1/secondary.webp",
            ]
        );
    }

    #[test]
    fn keeps_the_first_image_when_full_image_metadata_is_missing() {
        let urls = parse_media_urls(
            "<script>let imageData = [{\"image\":\"https://art.ngfiles.com/images/1/first.png\"},{\"image\":\"https://art.ngfiles.com/images/1/second.png\"}\n];</script>",
        )
        .unwrap();
        assert_eq!(
            urls.into_iter().map(|media| media.url).collect::<Vec<_>>(),
            [
                "https://art.ngfiles.com/images/1/first.png",
                "https://art.ngfiles.com/images/1/second.png",
            ]
        );
    }

    #[test]
    fn malformed_optional_image_data_keeps_the_valid_primary() {
        let urls = parse_media_urls(
            r#"
            <script>PHP.merge({"full_image_text":"<img src=\"https:\/\/art.ngfiles.com\/images\/1\/primary.png\">"});</script>
            <script>let imageData = [{not valid json}
];</script>
            "#,
        )
        .unwrap();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "https://art.ngfiles.com/images/1/primary.png");
    }

    #[test]
    fn cursors_and_first_party_urls_are_bounded() {
        assert_eq!(
            decode_cursor(Some("p42i7")).unwrap(),
            Cursor {
                page: 42,
                offset: 7
            }
        );
        assert!(decode_cursor(Some("p0i0")).is_err());
        assert!(decode_cursor(Some("p1i10001")).is_err());
        assert!(decode_cursor(Some("oops")).is_err());

        let url = profile_url("artist-name", 7).unwrap();
        assert_eq!(url.host_str(), Some("artist-name.newgrounds.com"));
        assert!(url.as_str().contains("page=7"));
        assert!(validate_post_url("https://newgrounds.com.evil.test/art/view/a/b").is_err());
    }

    #[test]
    fn ajax_header_is_limited_to_profile_discovery() {
        let credentials = RequestCredentials::default();
        let profile = profile_credentials(&credentials);
        let post = site_credentials(&credentials);

        assert_eq!(
            profile.headers.get("X-Requested-With").map(String::as_str),
            Some("XMLHttpRequest")
        );
        assert!(!post.headers.contains_key("X-Requested-With"));
        assert!(post.allowed_domains.contains("newgrounds.com"));
    }
}
