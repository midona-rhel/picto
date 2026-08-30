use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
#[cfg(debug_assertions)]
use std::time::Instant;

use regex::Regex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptor, MediaDescriptorBuilder, MediaFallback, MediaFuture,
    NativeSourceAdapter, PostFuture, ProviderDescriptor, RequestCredentials, SourceError,
    SourceErrorKind, SourcePost,
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

    fn credential_domains(&self) -> &'static [&'static str] {
        &["e-hentai.org", "exhentai.org"]
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

    fn resolve_media<'a>(
        &'a self,
        media: MediaDescriptor,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> MediaFuture<'a> {
        Box::pin(async move { resolve_gallery_media(media, credentials, http, cancel).await })
    }

    fn media_concurrency(&self) -> usize {
        1
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
    {
        return Err(invalid_query("Enter an E-Hentai or ExHentai gallery URL"));
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
    #[cfg(debug_assertions)]
    let started = Instant::now();
    #[cfg(debug_assertions)]
    tracing::debug!(
        target: "picto_sources::providers::ehentai",
        gallery_id = gallery.gallery_id,
        private = gallery.private,
        "Gallery resolution started"
    );

    let first_page = http
        .get_text(gallery.url.clone(), &credentials, cancel)
        .await?;
    validate_gallery_page(&first_page, &gallery)?;
    let metadata = parse_gallery_metadata(&first_page)?;
    let mut image_pages = BTreeMap::new();
    collect_image_pages(&first_page, &gallery, metadata.file_count, &mut image_pages)?;
    #[cfg(debug_assertions)]
    tracing::debug!(
        target: "picto_sources::providers::ehentai",
        gallery_id = gallery.gallery_id,
        total_images = metadata.file_count,
        discovered_image_pages = image_pages.len(),
        "Gallery metadata resolved"
    );

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
        #[cfg(debug_assertions)]
        tracing::debug!(
            target: "picto_sources::providers::ehentai",
            gallery_id = gallery.gallery_id,
            gallery_page = page,
            added_image_pages = added,
            discovered_image_pages = image_pages.len(),
            total_images = metadata.file_count,
            "Gallery index page resolved"
        );
        page += 1;
    }
    ensure_complete_image_sequence(&image_pages, metadata.file_count)?;

    let media = image_pages
        .values()
        .map(|image_page| deferred_media_from_image_page(&gallery, image_page, canonical_url))
        .collect::<Vec<_>>();

    #[cfg(debug_assertions)]
    tracing::debug!(
        target: "picto_sources::providers::ehentai",
        gallery_id = gallery.gallery_id,
        queued_media = media.len(),
        total_images = metadata.file_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Gallery resolution completed"
    );

    post.creator = metadata.creator;
    post.name = metadata.name;
    post.created_at = metadata.created_at;
    post.tags = metadata.tags;
    post.media = media;
    Ok(post)
}

async fn resolve_gallery_media(
    media: MediaDescriptor,
    credentials: &RequestCredentials,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<MediaDescriptor, SourceError> {
    let canonical_url = media
        .canonical_url
        .as_deref()
        .ok_or_else(|| invalid_response("E-Hentai media is missing its gallery URL"))?;
    let gallery = normalize_gallery_url(canonical_url)
        .map_err(|_| invalid_response("E-Hentai media has an invalid gallery URL"))?;
    require_private_gallery_session(&gallery, credentials)?;
    let original_allowed = !credentials.is_empty();
    let number = usize::try_from(media.position)
        .ok()
        .and_then(|position| position.checked_add(1))
        .ok_or_else(|| invalid_response("E-Hentai media has an invalid position"))?;
    let page_url = Url::parse(&media.url)
        .map_err(|_| invalid_response("E-Hentai media page URL is invalid"))?;
    if page_url.host_str() != gallery.url.host_str()
        || !page_url.path().starts_with("/s/")
        || !page_url
            .path()
            .ends_with(&format!("/{}-{number}", gallery.gallery_id))
    {
        return Err(invalid_response(
            "E-Hentai media page does not belong to its gallery",
        ));
    }
    let token = page_url
        .path_segments()
        .and_then(|mut segments| segments.nth(1))
        .unwrap_or_default()
        .to_string();
    let image_page = ImagePage {
        number,
        token,
        url: page_url,
    };
    let credentials = site_credentials(credentials, &gallery);
    let html = http
        .get_optional_text(image_page.url.clone(), &credentials, cancel)
        .await?
        .ok_or_else(|| {
            invalid_response(format!(
                "E-Hentai image page {number} is no longer available"
            ))
        })?;
    validate_image_page(&html, &gallery)?;
    let resolved = media_from_image_page(
        &gallery,
        &image_page,
        &html,
        canonical_url,
        original_allowed,
    )?;
    #[cfg(debug_assertions)]
    tracing::debug!(
        target: "picto_sources::providers::ehentai",
        gallery_id = gallery.gallery_id,
        resolved_media = number,
        original = resolved
            .url
            .parse::<Url>()
            .is_ok_and(|url| url.path().starts_with("/fullimg")),
        "Gallery media URL resolved"
    );
    Ok(resolved)
}

fn deferred_media_from_image_page(
    gallery: &GalleryAddress,
    image_page: &ImagePage,
    canonical_gallery_url: &str,
) -> MediaDescriptor {
    MediaDescriptorBuilder::new(
        format!("ehentai:{}:{}", gallery.gallery_id, image_page.number),
        (image_page.number - 1) as u32,
        image_page.url.to_string(),
    )
    .canonical_url(canonical_gallery_url)
    .build()
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
    original_allowed: bool,
) -> Result<MediaDescriptor, SourceError> {
    let displayed_image = capture(html, image_element_regex(), 0)
        .and_then(|element| capture(element, src_attribute_regex(), 1))
        .map(decode_html_attribute)
        .ok_or_else(|| {
            invalid_response(format!(
                "E-Hentai image page {} has no accessible gallery image",
                image_page.number
            ))
        })?;
    let original_image = capture(html, original_image_regex(), 1).map(decode_html_attribute);
    if original_allowed && original_image.is_none() && original_download_regex().is_match(html) {
        return Err(invalid_response(format!(
            "E-Hentai image page {} advertises an original file but its URL could not be resolved",
            image_page.number
        )));
    }
    let selected_original = original_allowed && original_image.is_some();
    let mut url = match original_image.filter(|_| original_allowed) {
        Some(original_image) => original_image_url(gallery, &image_page.url, &original_image)?,
        None if displayed_image.starts_with("//") => {
            Url::parse(&format!("https:{displayed_image}")).map_err(|_| {
                invalid_response("E-Hentai image page returned an invalid media URL")
            })?
        }
        None => image_page
            .url
            .join(&displayed_image)
            .map_err(|_| invalid_response("E-Hentai image page returned an invalid media URL"))?,
    };
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
        return Err(image_quota_error());
    }
    if is_archive(url.path()) {
        return Err(invalid_response(
            "E-Hentai image page returned an archive instead of an image",
        ));
    }
    let mut displayed_url = if displayed_image.starts_with("//") {
        Url::parse(&format!("https:{displayed_image}"))
    } else {
        image_page.url.join(&displayed_image)
    }
    .ok();
    if let Some(displayed_url) = displayed_url.as_mut() {
        if displayed_url.scheme() == "http" {
            let _ = displayed_url.set_scheme("https");
        }
        if displayed_url.path().ends_with("/509.gif") {
            return Err(image_quota_error());
        }
    }
    let file_name = selected_original
        .then(|| media_file_name(&url))
        .flatten()
        .or_else(|| displayed_url.as_ref().and_then(media_file_name))
        .unwrap_or_else(|| {
            format!(
                "ehentai_{}_{:04}.image",
                gallery.gallery_id, image_page.number
            )
        });
    let headers = BTreeMap::from([("Referer".to_string(), image_page.url.to_string())]);
    let display_fallback = selected_original
        .then_some(displayed_url.as_ref())
        .flatten()
        .filter(|url| {
            url.scheme() == "https"
                && url
                    .port()
                    .is_none_or(|_| is_hath_host(url.host_str().unwrap_or_default()))
                && allowed_image_host(url.host_str().unwrap_or_default())
        })
        .map(|url| {
            let file_name = media_file_name(url);
            let mime_hint = file_name
                .as_deref()
                .and_then(|name| mime_guess::from_path(name).first_raw())
                .map(ToOwned::to_owned);
            MediaFallback {
                url: url.to_string(),
                file_name,
                mime_hint,
                expected_size: None,
                html_marker: Some("requires gp".into()),
            }
        });
    let mut builder = MediaDescriptorBuilder::new(
        format!("ehentai:{}:{}", gallery.gallery_id, image_page.number),
        (image_page.number - 1) as u32,
        url.to_string(),
    )
    .canonical_url(canonical_gallery_url)
    .file_name(file_name)
    .headers(headers);
    if selected_original {
        if let Some(nl) = capture(html, nl_token_regex(), 1).map(decode_html_attribute) {
            let mut retry_url = url.clone();
            retry_url.query_pairs_mut().append_pair("nl", &nl);
            for _ in 0..2 {
                let retry_file_name = media_file_name(&url);
                let retry_mime_hint = retry_file_name
                    .as_deref()
                    .and_then(|name| mime_guess::from_path(name).first_raw())
                    .map(ToOwned::to_owned);
                builder = builder.fallback(MediaFallback {
                    url: retry_url.to_string(),
                    file_name: retry_file_name,
                    mime_hint: retry_mime_hint,
                    expected_size: None,
                    html_marker: None,
                });
            }
        }
    }
    if let Some(fallback) = display_fallback {
        builder = builder.fallback(fallback);
    }
    Ok(builder.build())
}

fn media_file_name(url: &Url) -> Option<String> {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("fullimg.php"))
        .map(ToOwned::to_owned)
}

fn original_image_url(
    gallery: &GalleryAddress,
    image_page_url: &Url,
    original_image: &str,
) -> Result<Url, SourceError> {
    let mut url = if original_image.starts_with("//") {
        Url::parse(&format!("https:{original_image}"))
    } else {
        image_page_url.join(original_image)
    }
    .map_err(|_| invalid_response("E-Hentai image page returned an invalid original URL"))?;
    if !matches!(url.host_str(), Some("e-hentai.org" | "exhentai.org"))
        || !(url.path().starts_with("/fullimg.php") || url.path().starts_with("/fullimg/"))
    {
        return Err(invalid_response(
            "E-Hentai image page returned an unsupported original URL",
        ));
    }

    // Match gallery-dl: apply the original path to the selected gallery host
    // so a private gallery keeps using its captured ExHentai session.
    url.set_scheme("https")
        .map_err(|_| invalid_response("E-Hentai original URL cannot use HTTPS"))?;
    url.set_host(gallery.url.host_str())
        .map_err(|_| invalid_response("E-Hentai original URL has an invalid host"))?;
    url.set_port(None)
        .map_err(|_| invalid_response("E-Hentai original URL has an invalid port"))?;
    Ok(url)
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
    let lower = html.to_ascii_lowercase();
    if lower.contains("temporarily banned") {
        return Err(SourceError::new(
            SourceErrorKind::RateLimited,
            "E-Hentai temporarily blocked image downloads; retry later",
            true,
        ));
    }
    if lower.contains("exceeded your image viewing limit") || lower.contains("image limit exceeded")
    {
        return Err(image_quota_error());
    }
    Ok(())
}

fn image_quota_error() -> SourceError {
    SourceError::new(
        SourceErrorKind::RateLimited,
        "E-Hentai image quota is exhausted",
        true,
    )
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
regex_fn!(
    original_image_regex,
    r#"(?is)<a\b[^>]*\bhref=[\"']([^\"']*/fullimg(?:\.php|/)[^\"']*)[\"'][^>]*>"#
);
regex_fn!(original_download_regex, r#"(?i)download\s+original\b"#);
regex_fn!(nl_token_regex, r#"(?i)\bnl\(\s*['\"]([^'\"]+)"#);

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
        assert_eq!(EHentaiSource.media_concurrency(), 1);
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
        let paged =
            normalize_gallery_url("https://exhentai.org/g/1449482/9051983a03/?p=1#page").unwrap();
        assert_eq!(
            paged.url.as_str(),
            "https://exhentai.org/g/1449482/9051983a03/"
        );

        for invalid in [
            "https://e-hentai.org/?f_search=fixture",
            "https://e-hentai.org/favorites.php",
            "https://e-hentai.org/mpv/12345/67890abcde/",
            "https://e-hentai.org/s/1111111111/12345-1",
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
    fn prefers_the_original_image_over_the_displayed_resample() {
        let gallery = public_gallery();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://e-hentai.org/s/1111111111/12345-1").unwrap(),
        };
        let media = media_from_image_page(
            &gallery,
            &image_page,
            IMAGE_PAGE_1,
            gallery.url.as_str(),
            true,
        )
        .unwrap();
        assert_eq!(media.position, 0);
        assert_eq!(media.stable_id, "ehentai:12345:1");
        assert_eq!(media.file_name.as_deref(), Some("fixture-01.jpg"));
        assert_eq!(media.mime_hint.as_deref(), Some("image/jpeg"));
        assert_eq!(
            media.url,
            "https://e-hentai.org/fullimg.php?gid=12345&page=1&key=fixture-key"
        );
        assert_eq!(media.canonical_url.as_deref(), Some(gallery.url.as_str()));
        assert_eq!(
            media.headers.get("Referer").map(String::as_str),
            Some(image_page.url.as_str())
        );
        assert!(media.url.contains("fullimg"));
        assert!(!media.url.contains("archiver"));
        let fallback = media.fallbacks.last().expect("display fallback");
        assert!(fallback.url.contains("hath.network"));
        assert_eq!(fallback.file_name.as_deref(), Some("fixture-01.jpg"));
        assert_eq!(fallback.html_marker.as_deref(), Some("requires gp"));
        assert_eq!(media.fallbacks.len(), 3);
        assert!(media.fallbacks[0].url.contains("nl=fixture-nl"));
        assert!(media.fallbacks[1].url.contains("nl=fixture-nl"));
        assert!(media.fallbacks[..2]
            .iter()
            .all(|fallback| fallback.html_marker.is_none()));
    }

    #[test]
    fn accepts_path_style_originals_and_pins_them_to_the_gallery_domain() {
        let gallery = normalize_gallery_url("https://exhentai.org/g/12345/67890abcde/").unwrap();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://exhentai.org/s/1111111111/12345-1").unwrap(),
        };
        let html = IMAGE_PAGE_1
            .replace(
                "https://e-hentai.org/fullimg.php?gid=12345&amp;page=1&amp;key=fixture-key",
                "https://e-hentai.org/fullimg/12345/fixture-key/fixture-01.jpg",
            )
            .replace(
                "fixture-01.jpg?token=temporary",
                "fixture-01.webp?token=temporary",
            );

        let media = media_from_image_page(&gallery, &image_page, &html, gallery.url.as_str(), true)
            .unwrap();

        assert_eq!(
            media.url,
            "https://exhentai.org/fullimg/12345/fixture-key/fixture-01.jpg"
        );
        assert_eq!(media.file_name.as_deref(), Some("fixture-01.jpg"));
        assert_eq!(media.mime_hint.as_deref(), Some("image/jpeg"));
        assert!(!media.fallbacks.is_empty());
        assert!(!media.url.contains("hath.network"));
    }

    #[test]
    fn does_not_silently_use_a_resample_when_an_original_link_is_unrecognized() {
        let gallery = public_gallery();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://e-hentai.org/s/1111111111/12345-1").unwrap(),
        };
        let html = IMAGE_PAGE_1.replace("fullimg.php", "original.php");

        let error = media_from_image_page(&gallery, &image_page, &html, gallery.url.as_str(), true)
            .unwrap_err();

        assert_eq!(error.kind, SourceErrorKind::InvalidResponse);
        assert!(error.message.contains("original file"));
    }

    #[test]
    fn anonymous_public_galleries_use_the_displayed_file_like_gallery_dl() {
        let gallery = public_gallery();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://e-hentai.org/s/1111111111/12345-1").unwrap(),
        };

        let media = media_from_image_page(
            &gallery,
            &image_page,
            IMAGE_PAGE_1,
            gallery.url.as_str(),
            false,
        )
        .unwrap();

        assert!(media.url.contains("hath.network"));
        assert!(!media.url.contains("fullimg"));
    }

    #[test]
    fn defers_each_image_page_until_the_downloader_is_ready_for_it() {
        let gallery = public_gallery();
        let image_page = ImagePage {
            number: 1,
            token: "1111111111".to_string(),
            url: Url::parse("https://e-hentai.org/s/1111111111/12345-1").unwrap(),
        };
        let deferred = deferred_media_from_image_page(&gallery, &image_page, gallery.url.as_str());

        assert_eq!(deferred.stable_id, "ehentai:12345:1");
        assert_eq!(deferred.url, image_page.url.as_str());
        assert_eq!(
            deferred.canonical_url.as_deref(),
            Some(gallery.url.as_str())
        );
        assert!(deferred.file_name.is_none());
    }

    #[test]
    fn requires_only_the_pinned_extractor_session_cookies_for_exhentai() {
        assert_eq!(
            EHentaiSource.credential_domains(),
            &["e-hentai.org", "exhentai.org"]
        );
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
            false,
        )
        .unwrap_err();
        assert_eq!(quota.kind, SourceErrorKind::RateLimited);
        let quota_with_original = media_from_image_page(
            &gallery,
            &image_page,
            &IMAGE_PAGE_1.replace(
                "https://a.example.hath.network:41330/h/fixture-01.jpg?token=temporary&amp;expires=1",
                "https://ehgt.org/g/509.gif",
            ),
            gallery.url.as_str(),
            true,
        )
        .unwrap_err();
        assert_eq!(quota_with_original.kind, SourceErrorKind::RateLimited);
        assert!(media_from_image_page(
            &gallery,
            &image_page,
            "<img id=\"img\" src=\"https://ehgt.org/g/gallery.zip\">",
            gallery.url.as_str(),
            false,
        )
        .is_err());
    }

    #[test]
    fn image_page_bans_and_limits_are_retryable_rate_limits() {
        let gallery = public_gallery();
        for html in [
            "Your IP address has been temporarily banned",
            "You have exceeded your image viewing limit",
            "Image limit exceeded",
        ] {
            let error = validate_image_page(html, &gallery).unwrap_err();
            assert_eq!(error.kind, SourceErrorKind::RateLimited);
            assert!(error.retryable);
        }
    }
}
