use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptor, MediaDescriptorBuilder, NativeSourceAdapter, PostFuture,
    ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const DONE_CURSOR: &str = "done";
const MAX_GALLERY_IMAGES: usize = 5_000;
const MAX_GALLERY_PAGES: usize = 250;

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    EHentaiSource
}

struct EHentaiSource;

impl NativeSourceAdapter for EHentaiSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "ehentai",
            display_name: "E-Hentai / ExHentai",
            domain: "e-hentai.org",
            partitions: &["gallery"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_gallery_url(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        _http: &'a HttpRuntime,
        _cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let gallery = normalize_gallery_url(&request.query)?;
            require_private_gallery_session(&gallery, credentials)?;
            match request
                .cursor
                .as_deref()
                .filter(|cursor| !cursor.is_empty())
            {
                None => Ok(DiscoveryBatch {
                    posts: vec![discovered_post(request, &gallery)],
                    exhausted: true,
                }),
                Some(DONE_CURSOR) => Ok(DiscoveryBatch {
                    posts: Vec::new(),
                    exhausted: true,
                }),
                Some(_) => Err(invalid_query("invalid E-Hentai gallery cursor")),
            }
        })
    }

    fn resolve_post<'a>(
        &'a self,
        post: SourcePost,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> PostFuture<'a> {
        Box::pin(async move { resolve_gallery(post, credentials, http, cancel).await })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GalleryAddress {
    url: Url,
    gallery_id: u64,
    private: bool,
}

fn normalize_gallery_url(raw: &str) -> Result<GalleryAddress, SourceError> {
    let mut url = Url::parse(raw.trim())
        .map_err(|_| invalid_query("E-Hentai imports require a concrete gallery URL"))?;
    let (host, private) = match url.host_str() {
        Some("e-hentai.org" | "www.e-hentai.org") => ("e-hentai.org", false),
        Some("exhentai.org" | "www.exhentai.org") => ("exhentai.org", true),
        _ => {
            return Err(invalid_query(
                "E-Hentai imports require an e-hentai.org or exhentai.org gallery URL",
            ))
        }
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_query(
            "E-Hentai imports require a canonical /g/<id>/<token>/ URL",
        ));
    }
    let segments = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let ["g", gallery_id, token] = segments.as_slice() else {
        return Err(invalid_query(
            "E-Hentai imports require a /g/<id>/<token>/ URL",
        ));
    };
    let gallery_id = gallery_id
        .parse::<u64>()
        .ok()
        .filter(|gallery_id| *gallery_id > 0)
        .ok_or_else(|| invalid_query("E-Hentai gallery ID must be a positive integer"))?;
    if token.len() != 10 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_query(
            "E-Hentai gallery token must contain exactly 10 hexadecimal characters",
        ));
    }
    let token = token.to_ascii_lowercase();
    url.set_scheme("https")
        .map_err(|_| invalid_query("invalid E-Hentai gallery URL"))?;
    url.set_host(Some(host))
        .map_err(|_| invalid_query("invalid E-Hentai gallery URL"))?;
    url.set_path(&format!("/g/{gallery_id}/{token}/"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(GalleryAddress {
        url,
        gallery_id,
        private,
    })
}

fn discovered_post(request: &DiscoveryRequest, gallery: &GalleryAddress) -> SourcePost {
    SourcePost {
        site_id: "ehentai".to_string(),
        partition: request.partition.clone(),
        stable_id: gallery.gallery_id.to_string(),
        canonical_url: Some(gallery.url.to_string()),
        creator: None,
        name: None,
        notes: None,
        created_at: None,
        tags: Vec::new(),
        media: Vec::new(),
        resume_cursor_after: Some(DONE_CURSOR.to_string()),
    }
}

async fn resolve_gallery(
    mut post: SourcePost,
    credentials: &RequestCredentials,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<SourcePost, SourceError> {
    let canonical_url = post
        .canonical_url
        .as_deref()
        .ok_or_else(|| invalid_response("E-Hentai post is missing its canonical gallery URL"))?;
    let gallery = normalize_gallery_url(canonical_url)
        .map_err(|_| invalid_response("E-Hentai post has an invalid canonical gallery URL"))?;
    if post.stable_id != gallery.gallery_id.to_string() {
        return Err(invalid_response(
            "E-Hentai post identity does not match its canonical gallery URL",
        ));
    }
    require_private_gallery_session(&gallery, credentials)?;
    let credentials = site_credentials(credentials, &gallery);

    let first_page = http
        .get_text(gallery.url.clone(), &credentials, cancel)
        .await?;
    validate_gallery_page(&first_page, &gallery)?;
    let metadata = parse_gallery_metadata(&first_page)?;
    let mut image_pages = BTreeMap::new();
    collect_image_pages(&first_page, &gallery, metadata.file_count, &mut image_pages)?;

    let mut page = 1_usize;
    while image_pages.len() < metadata.file_count {
        if page >= MAX_GALLERY_PAGES || page >= metadata.file_count {
            return Err(invalid_response(format!(
                "E-Hentai gallery pagination exceeded its bound before resolving {} images",
                metadata.file_count
            )));
        }
        let html = http
            .get_text(gallery_page_url(&gallery, page), &credentials, cancel)
            .await?;
        validate_gallery_page(&html, &gallery)?;
        let added = collect_image_pages(&html, &gallery, metadata.file_count, &mut image_pages)?;
        if added == 0 {
            return Err(invalid_response(
                "E-Hentai gallery pagination repeated without exposing the remaining images",
            ));
        }
        page += 1;
    }
    ensure_complete_image_sequence(&image_pages, metadata.file_count)?;

    let mut media = Vec::with_capacity(metadata.file_count);
    for image_page in image_pages.values() {
        let Some(html) = http
            .get_optional_text(image_page.url.clone(), &credentials, cancel)
            .await?
        else {
            continue;
        };
        validate_image_page(&html, &gallery)?;
        media.push(media_from_image_page(
            &gallery,
            image_page,
            &html,
            canonical_url,
        )?);
    }

    post.creator = metadata.creator;
    post.name = metadata.name;
    post.created_at = metadata.created_at;
    post.tags = metadata.tags;
    post.media = media;
    Ok(post)
}

fn site_credentials(
    credentials: &RequestCredentials,
    gallery: &GalleryAddress,
) -> RequestCredentials {
    let mut credentials = credentials.clone();
    let host = gallery
        .url
        .host_str()
        .expect("normalized gallery has a host");
    credentials.allowed_domains.insert(host.to_string());
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8".to_string());
    if !gallery.private {
        credentials
            .cookies
            .entry("nw".to_string())
            .or_insert_with(|| "1".to_string());
    }
    credentials
}

fn require_private_gallery_session(
    gallery: &GalleryAddress,
    credentials: &RequestCredentials,
) -> Result<(), SourceError> {
    if !gallery.private {
        return Ok(());
    }
    for name in ["ipb_member_id", "ipb_pass_hash"] {
        if credentials
            .cookies
            .get(name)
            .map(String::as_str)
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            return Err(authentication_required(
                "ExHentai requires captured ipb_member_id and ipb_pass_hash session cookies",
            ));
        }
    }
    Ok(())
}

fn gallery_page_url(gallery: &GalleryAddress, page: usize) -> Url {
    let mut url = gallery.url.clone();
    if page > 0 {
        url.query_pairs_mut().append_pair("p", &page.to_string());
    }
    url
}

#[derive(Debug, PartialEq, Eq)]
struct GalleryMetadata {
    file_count: usize,
    creator: Option<String>,
    name: Option<String>,
    created_at: Option<String>,
    tags: Vec<crate::CanonicalTag>,
}

fn parse_gallery_metadata(html: &str) -> Result<GalleryMetadata, SourceError> {
    let file_count = capture(html, file_count_regex(), 1)
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count > 0 && *count <= MAX_GALLERY_IMAGES)
        .ok_or_else(|| {
            invalid_response(format!(
                "E-Hentai gallery has no valid bounded image count (maximum {MAX_GALLERY_IMAGES})"
            ))
        })?;
    let raw_tags = raw_gallery_tags(html);
    let creator = raw_tags
        .iter()
        .find(|(namespace, value)| {
            namespace.eq_ignore_ascii_case("artist") && is_authored_creator(value)
        })
        .map(|(_, value)| value.clone());
    Ok(GalleryMetadata {
        file_count,
        creator,
        name: capture(html, title_regex(), 1).and_then(normalize_source_text),
        created_at: capture(html, posted_regex(), 1).and_then(normalize_source_text),
        tags: canonical_tags(&raw_tags),
    })
}

fn raw_gallery_tags(html: &str) -> Vec<(String, String)> {
    let mut tags = Vec::new();
    let mut seen = BTreeSet::new();
    for captures in tag_regex().captures_iter(html) {
        let Some(raw) = captures.get(1) else {
            continue;
        };
        let decoded = url::form_urlencoded::parse(raw.as_str().as_bytes())
            .next()
            .map(|(key, value)| if value.is_empty() { key } else { value })
            .map(|value| value.into_owned());
        let Some((namespace, value)) = decoded.as_deref().and_then(|tag| tag.split_once(':'))
        else {
            continue;
        };
        let namespace = namespace.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if !namespace.is_empty()
            && !value.is_empty()
            && seen.insert((namespace.clone(), value.clone()))
        {
            tags.push((namespace, value));
        }
    }
    tags
}

fn canonical_tags(raw_tags: &[(String, String)]) -> Vec<crate::CanonicalTag> {
    let mut tags = CanonicalTagSet::default();
    for (namespace, value) in raw_tags {
        match namespace.as_str() {
            "artist" if is_authored_creator(value) => tags.insert("creator", value),
            "character" => tags.insert("character", value),
            "parody" if !value.eq_ignore_ascii_case("original") => tags.insert("series", value),
            "species" => tags.insert("species", value),
            "rating" => match value.to_ascii_lowercase().as_str() {
                "safe" | "general" => tags.insert("rating", "safe"),
                "questionable" | "mature" => tags.insert("rating", "questionable"),
                "explicit" | "adult" => tags.insert("rating", "explicit"),
                _ => tags.insert("", value),
            },
            _ => tags.insert("", value),
        }
    }
    tags.into_vec()
}

fn is_authored_creator(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "unknown" | "anonymous" | "n/a"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImagePage {
    number: usize,
    token: String,
    url: Url,
}

fn collect_image_pages(
    html: &str,
    gallery: &GalleryAddress,
    file_count: usize,
    pages: &mut BTreeMap<usize, ImagePage>,
) -> Result<usize, SourceError> {
    let before = pages.len();
    for captures in image_page_regex().captures_iter(html) {
        let Some(token) = captures.get(1).map(|value| value.as_str()) else {
            continue;
        };
        let Some(gallery_id) = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<u64>().ok())
        else {
            continue;
        };
        let Some(number) = captures
            .get(3)
            .and_then(|value| value.as_str().parse::<usize>().ok())
        else {
            continue;
        };
        if gallery_id != gallery.gallery_id || number == 0 || number > file_count {
            continue;
        }
        let token = token.to_ascii_lowercase();
        if let Some(existing) = pages.get(&number) {
            if existing.token != token {
                return Err(invalid_response(format!(
                    "E-Hentai gallery exposed conflicting tokens for image {number}"
                )));
            }
            continue;
        }
        let mut url = gallery.url.clone();
        url.set_path(&format!("/s/{token}/{}-{number}", gallery.gallery_id));
        pages.insert(number, ImagePage { number, token, url });
    }
    Ok(pages.len() - before)
}

fn ensure_complete_image_sequence(
    pages: &BTreeMap<usize, ImagePage>,
    file_count: usize,
) -> Result<(), SourceError> {
    if pages.len() != file_count {
        return Err(invalid_response(format!(
            "E-Hentai gallery exposed {} of {file_count} image pages",
            pages.len()
        )));
    }
    for number in 1..=file_count {
        if !pages.contains_key(&number) {
            return Err(invalid_response(format!(
                "E-Hentai gallery is missing image page {number}"
            )));
        }
    }
    Ok(())
}

fn media_from_image_page(
    gallery: &GalleryAddress,
    image_page: &ImagePage,
    html: &str,
    canonical_gallery_url: &str,
) -> Result<MediaDescriptor, SourceError> {
    let image = capture(html, image_element_regex(), 0)
        .and_then(|element| capture(element, src_attribute_regex(), 1))
        .map(decode_html_attribute)
        .ok_or_else(|| {
            invalid_response(format!(
                "E-Hentai image page {} has no accessible gallery image",
                image_page.number
            ))
        })?;
    let mut url = if image.starts_with("//") {
        Url::parse(&format!("https:{image}"))
    } else {
        image_page.url.join(&image)
    }
    .map_err(|_| invalid_response("E-Hentai image page returned an invalid media URL"))?;
    if url.scheme() == "http" {
        url.set_scheme("https")
            .map_err(|_| invalid_response("E-Hentai image URL cannot use HTTPS"))?;
    }
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || (url.port().is_some() && !is_hath_host(host))
        || !allowed_image_host(host)
    {
        return Err(invalid_response(
            "E-Hentai image page returned an unsupported media host",
        ));
    }
    if url.path().ends_with("/509.gif") {
        return Err(SourceError::new(
            SourceErrorKind::RateLimited,
            "E-Hentai image quota is exhausted",
            true,
        ));
    }
    if is_archive(url.path()) {
        return Err(invalid_response(
            "E-Hentai image page returned an archive instead of an image",
        ));
    }
    let file_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            format!(
                "ehentai_{}_{:04}.image",
                gallery.gallery_id, image_page.number
            )
        });
    let headers = BTreeMap::from([("Referer".to_string(), image_page.url.to_string())]);
    Ok(MediaDescriptorBuilder::new(
        format!("ehentai:{}:{}", gallery.gallery_id, image_page.number),
        (image_page.number - 1) as u32,
        url.to_string(),
    )
    .canonical_url(canonical_gallery_url)
    .file_name(file_name)
    .headers(headers)
    .build())
}

fn allowed_image_host(host: &str) -> bool {
    matches!(
        host,
        "e-hentai.org" | "exhentai.org" | "ehgt.org" | "hath.network"
    ) || host.ends_with(".e-hentai.org")
        || host.ends_with(".exhentai.org")
        || host.ends_with(".ehgt.org")
        || is_hath_host(host)
}

fn is_hath_host(host: &str) -> bool {
    host == "hath.network" || host.ends_with(".hath.network")
}

fn validate_gallery_page(html: &str, gallery: &GalleryAddress) -> Result<(), SourceError> {
    if html.trim().is_empty() {
        return Err(authentication_required(if gallery.private {
            "ExHentai returned a blank page; the captured session is invalid or expired"
        } else {
            "E-Hentai returned a blank page; the captured session may be invalid or rate limited"
        }));
    }
    if html.contains("Gallery Not Available")
        || html.starts_with("Key missing")
        || html.starts_with("Gallery not found")
    {
        return Err(if gallery.private {
            authentication_required("ExHentai rejected the captured session or gallery access")
        } else {
            invalid_response("E-Hentai gallery is unavailable")
        });
    }
    if !html.contains("id=\"gn\"") && !html.contains("id='gn'") {
        return Err(invalid_response(
            "E-Hentai returned a page without gallery metadata",
        ));
    }
    Ok(())
}

fn validate_image_page(html: &str, gallery: &GalleryAddress) -> Result<(), SourceError> {
    if html.trim().is_empty() {
        return Err(authentication_required(if gallery.private {
            "ExHentai returned a blank image page; the captured session is invalid or expired"
        } else {
            "E-Hentai returned a blank image page"
        }));
    }
    if html.starts_with("Invalid page") || html.starts_with("Keep trying") {
        return Err(invalid_response("E-Hentai returned an invalid image page"));
    }
    Ok(())
}

fn is_archive(raw: &str) -> bool {
    let path = raw.split('?').next().unwrap_or(raw).to_ascii_lowercase();
    [".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz"]
        .iter()
        .any(|extension| path.ends_with(extension))
}

fn decode_html_attribute(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
}

fn capture<'a>(text: &'a str, regex: &Regex, group: usize) -> Option<&'a str> {
    regex.captures(text)?.get(group).map(|value| value.as_str())
}

macro_rules! regex_fn {
    ($name:ident, $pattern:literal) => {
        fn $name() -> &'static Regex {
            static VALUE: OnceLock<Regex> = OnceLock::new();
            VALUE.get_or_init(|| Regex::new($pattern).expect(concat!("valid ", stringify!($name))))
        }
    };
}

regex_fn!(title_regex, r#"(?s)<h1\s+id=[\"']gn[\"'][^>]*>(.*?)</h1>"#);
regex_fn!(
    posted_regex,
    r#"(?s)>\s*Posted:\s*</td>\s*<td[^>]*class=[\"']gdt2[\"'][^>]*>(.*?)</td>"#
);
regex_fn!(
    file_count_regex,
    r#"(?s)>\s*Length:\s*</td>\s*<td[^>]*class=[\"']gdt2[\"'][^>]*>\s*([0-9]+)\s+(?:pages?|files?)"#
);
regex_fn!(tag_regex, r#"(?i)(?:hentai\.org)?/tag/([^\"'?#<]+)"#);
regex_fn!(
    image_page_regex,
    r#"(?i)/s/([0-9a-f]{10})/([0-9]+)-([0-9]+)"#
);
regex_fn!(
    image_element_regex,
    r#"(?is)<img\b[^>]*\bid=[\"']img[\"'][^>]*>"#
);
regex_fn!(src_attribute_regex, r#"(?i)\bsrc=[\"']([^\"']+)[\"']"#);

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, true)
}

fn authentication_required(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::Authentication, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const GALLERY_PAGE_0: &str = include_str!("../../tests/fixtures/ehentai/gallery_page_0.html");
    const GALLERY_PAGE_1: &str = include_str!("../../tests/fixtures/ehentai/gallery_page_1.html");
    const IMAGE_PAGE_1: &str = include_str!("../../tests/fixtures/ehentai/image_page_1.html");

    fn request(query: &str, cursor: Option<&str>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: query.to_string(),
            partition: SourcePartition::new("gallery"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 20,
        }
    }

    fn public_gallery() -> GalleryAddress {
        normalize_gallery_url("https://e-hentai.org/g/12345/67890abcde/").unwrap()
    }

    #[test]
    fn accepts_only_concrete_gallery_urls_and_preserves_the_selected_host() {
        let public = normalize_gallery_url("http://www.e-hentai.org/g/12345/67890ABCDE/").unwrap();
        assert_eq!(
            public.url.as_str(),
            "https://e-hentai.org/g/12345/67890abcde/"
        );
        let private =
            normalize_gallery_url("https://www.exhentai.org/g/12345/67890ABCDE/").unwrap();
        assert_eq!(
            private.url.as_str(),
            "https://exhentai.org/g/12345/67890abcde/"
        );
        assert!(private.private);

        for invalid in [
            "https://e-hentai.org/?f_search=fixture",
            "https://e-hentai.org/favorites.php",
            "https://e-hentai.org/mpv/12345/67890abcde/",
            "https://e-hentai.org/s/1111111111/12345-1",
            "https://e-hentai.org/g/12345/67890abcde/?p=1",
            "https://example.com/g/12345/67890abcde/",
        ] {
            assert!(
                normalize_gallery_url(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[tokio::test]
    async fn discovers_one_terminal_gallery_post_once() {
        let source = EHentaiSource;
        let http = HttpRuntime::new(crate::HttpPolicy::default()).unwrap();
        let cancel = CancellationToken::new();
        let query = "https://e-hentai.org/g/12345/67890abcde/";
        let batch = source
            .discover(
                &request(query, None),
                &RequestCredentials::default(),
                &http,
                &cancel,
            )
            .await
            .unwrap();
        assert!(batch.exhausted);
        assert_eq!(batch.posts.len(), 1);
        assert_eq!(batch.posts[0].stable_id, "12345");
        assert_eq!(batch.posts[0].canonical_url.as_deref(), Some(query));
        assert_eq!(
            batch.posts[0].resume_cursor_after.as_deref(),
            Some(DONE_CURSOR)
        );

        let exhausted = source
            .discover(
                &request(query, Some(DONE_CURSOR)),
                &RequestCredentials::default(),
                &http,
                &cancel,
            )
            .await
            .unwrap();
        assert!(exhausted.exhausted);
        assert!(exhausted.posts.is_empty());
    }

    #[test]
    fn maps_authored_tags_and_keeps_other_namespaces_general() {
        let metadata = parse_gallery_metadata(GALLERY_PAGE_0).unwrap();
        assert_eq!(metadata.file_count, 3);
        assert_eq!(metadata.name.as_deref(), Some("Fixture & Gallery"));
        assert_eq!(metadata.creator.as_deref(), Some("fixture artist"));
        assert_eq!(metadata.created_at.as_deref(), Some("2026-08-29 17:18"));
        assert!(metadata
            .tags
            .contains(&CanonicalTag::new("creator", "fixture artist")));
        assert!(metadata
            .tags
            .contains(&CanonicalTag::new("series", "fixture series")));
        assert!(metadata
            .tags
            .contains(&CanonicalTag::new("character", "fixture hero")));
        assert!(metadata
            .tags
            .contains(&CanonicalTag::new("species", "wolf")));
        for general in ["fixture circle", "original", "english", "solo"] {
            assert!(metadata.tags.contains(&CanonicalTag::new("", general)));
        }
        assert!(!metadata.tags.iter().any(|tag| tag.value == "uploader-only"));

        let without_artist = GALLERY_PAGE_0.replace(
            "<a href=\"https://e-hentai.org/tag/artist%3Afixture+artist\">fixture artist</a>",
            "",
        );
        assert_eq!(
            parse_gallery_metadata(&without_artist).unwrap().creator,
            None
        );

        let placeholder = canonical_tags(&[("artist".to_string(), "unknown".to_string())]);
        assert_eq!(placeholder, vec![CanonicalTag::new("", "unknown")]);
    }

    #[test]
    fn collects_a_complete_ordered_sequence_across_bounded_pages() {
        let gallery = public_gallery();
        let mut pages = BTreeMap::new();
        assert_eq!(
            collect_image_pages(GALLERY_PAGE_0, &gallery, 3, &mut pages).unwrap(),
            2
        );
        assert_eq!(
            collect_image_pages(GALLERY_PAGE_1, &gallery, 3, &mut pages).unwrap(),
            1
        );
        ensure_complete_image_sequence(&pages, 3).unwrap();
        assert_eq!(pages.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(pages[&3].url.path(), "/s/3333333333/12345-3");

        pages.remove(&2);
        assert!(ensure_complete_image_sequence(&pages, 3).is_err());
    }

    #[test]
    fn resolves_the_displayed_image_without_archive_or_original_paths() {
        let gallery = public_gallery();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://e-hentai.org/s/1111111111/12345-1").unwrap(),
        };
        let media =
            media_from_image_page(&gallery, &image_page, IMAGE_PAGE_1, gallery.url.as_str())
                .unwrap();
        assert_eq!(media.position, 0);
        assert_eq!(media.stable_id, "ehentai:12345:1");
        assert_eq!(media.file_name.as_deref(), Some("fixture-01.jpg"));
        assert_eq!(media.mime_hint.as_deref(), Some("image/jpeg"));
        assert_eq!(
            media.url,
            "https://a.example.hath.network:41330/h/fixture-01.jpg?token=temporary&expires=1"
        );
        assert_eq!(media.canonical_url.as_deref(), Some(gallery.url.as_str()));
        assert_eq!(
            media.headers.get("Referer").map(String::as_str),
            Some(image_page.url.as_str())
        );
        assert!(!media.url.contains("fullimg"));
        assert!(!media.url.contains("archiver"));
    }

    #[test]
    fn requires_only_the_pinned_extractor_session_cookies_for_exhentai() {
        let gallery = normalize_gallery_url("https://exhentai.org/g/12345/67890abcde/").unwrap();
        let mut credentials = RequestCredentials::default();
        assert_eq!(
            require_private_gallery_session(&gallery, &credentials)
                .unwrap_err()
                .kind,
            SourceErrorKind::Authentication
        );
        credentials
            .cookies
            .insert("ipb_member_id".to_string(), "fixture-member".to_string());
        assert!(require_private_gallery_session(&gallery, &credentials).is_err());
        credentials
            .cookies
            .insert("ipb_pass_hash".to_string(), "fixture-hash".to_string());
        require_private_gallery_session(&gallery, &credentials).unwrap();
    }

    #[test]
    fn rejects_quota_placeholders_and_archive_media() {
        let gallery = public_gallery();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://e-hentai.org/s/1111111111/12345-1").unwrap(),
        };
        let quota = media_from_image_page(
            &gallery,
            &image_page,
            "<img id=\"img\" src=\"https://ehgt.org/g/509.gif\">",
            gallery.url.as_str(),
        )
        .unwrap_err();
        assert_eq!(quota.kind, SourceErrorKind::RateLimited);
        assert!(media_from_image_page(
            &gallery,
            &image_page,
            "<img id=\"img\" src=\"https://ehgt.org/g/gallery.zip\">",
            gallery.url.as_str(),
        )
        .is_err());
    }
}
