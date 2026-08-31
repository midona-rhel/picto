use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, MediaFallback, NativeSourceAdapter, OpaqueCursor,
    PostFuture, ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const DOMAIN: &str = "deviantart.com";
const CURSOR: OpaqueCursor = OpaqueCursor::new(64);
const GALLERY_PAGE_SIZE: u32 = 24;
const CLIENT_AUTHORIZATION: &str = "Basic NTM4ODo3NmIwOGM2OWNmYjI3ZjI2ZDYxNjFmOWFiNmQwNjFhMQ==";

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    DeviantArtSource::default()
}

#[derive(Default)]
struct DeviantArtSource {
    gallery_pages: Mutex<HashMap<GalleryPageKey, GalleryResponse>>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct GalleryPageKey {
    username: String,
    offset: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GalleryCursor {
    offset: u32,
    item: usize,
}

struct CachedAccessToken {
    credential_key: Option<String>,
    value: String,
    valid_until: Instant,
}

fn access_token_cache() -> &'static Mutex<Option<CachedAccessToken>> {
    static CACHE: OnceLock<Mutex<Option<CachedAccessToken>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

impl NativeSourceAdapter for DeviantArtSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "deviantart",
            display_name: "DeviantArt",
            domain: DOMAIN,
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
            let mut cursor = decode_cursor(request.cursor.as_deref())?;
            let access_token = self.access_token(credentials, http, cancel).await?;
            loop {
                let key = GalleryPageKey {
                    username: username.clone(),
                    offset: cursor.offset,
                };
                let cached = self.gallery_pages.lock().await.get(&key).cloned();
                let response = match cached {
                    Some(response) => response,
                    None => {
                        let mut response = http
                            .get_json::<GalleryResponse>(
                                gallery_url(&username, cursor.offset),
                                &api_credentials(credentials, &access_token),
                                cancel,
                            )
                            .await?;
                        let mut seen = BTreeSet::new();
                        response
                            .results
                            .retain(|item| seen.insert(item.deviationid.clone()));
                        self.gallery_pages
                            .lock()
                            .await
                            .insert(key.clone(), response.clone());
                        response
                    }
                };
                if let Some(index) = next_accessible_index(&response.results, cursor.item) {
                    let finishes_page =
                        next_accessible_index(&response.results, index.saturating_add(1)).is_none();
                    let batch = normalize_gallery(request, cursor, response)?;
                    if finishes_page {
                        self.gallery_pages.lock().await.remove(&key);
                    }
                    return Ok(batch);
                }
                self.gallery_pages.lock().await.remove(&key);
                if !response.has_more {
                    return Ok(DiscoveryBatch {
                        posts: Vec::new(),
                        exhausted: true,
                    });
                }
                cursor = GalleryCursor {
                    offset: next_page_offset(cursor.offset, &response),
                    item: 0,
                };
            }
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
            let access_token = self.access_token(credentials, http, cancel).await?;
            if post
                .media
                .iter()
                .any(|media| media.stable_id.ends_with(":download"))
            {
                match http
                    .get_optional_json::<ApiContent>(
                        download_url(&post.stable_id)?,
                        &api_credentials(credentials, &access_token),
                        cancel,
                    )
                    .await
                {
                    Ok(Some(original)) => apply_original_media(&mut post, original),
                    Ok(None) => remove_unresolved_download_placeholder(&mut post),
                    Err(error) if error.kind == SourceErrorKind::InvalidResponse => {
                        // DeviantArt occasionally advertises a downloadable
                        // deviation but rejects its original-file endpoint.
                        // Keep the gallery image when one exists instead of
                        // failing or repeatedly importing the same post.
                        remove_unresolved_download_placeholder(&mut post);
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(post)
        })
    }
}

impl DeviantArtSource {
    async fn access_token(
        &self,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<String, SourceError> {
        let credential_key = credentials
            .oauth_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let mut token_cache = access_token_cache().lock().await;
        if let Some(token) = token_cache.as_ref() {
            if token.credential_key == credential_key
                && token.valid_until > Instant::now() + Duration::from_secs(30)
            {
                return Ok(token.value.clone());
            }
        }

        let response = match credential_key.as_deref() {
            Some(refresh_token) => {
                request_access_token(http, credentials, Some(refresh_token), cancel)
                    .await
                    .map_err(map_refresh_error)?
            }
            None => request_access_token(http, credentials, None, cancel).await?,
        };
        if let Some(refresh_token) = response
            .refresh_token
            .filter(|value| !value.trim().is_empty())
        {
            if let Some(update) = credentials.oauth_token_update.as_ref() {
                update(refresh_token).map_err(invalid_response)?;
            }
        }
        let access_token = response
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(authentication_required)?;
        let lifetime = Duration::from_secs(response.expires_in.unwrap_or(3_600).max(60));
        *token_cache = Some(CachedAccessToken {
            credential_key,
            value: access_token.clone(),
            valid_until: Instant::now() + lifetime,
        });
        Ok(access_token)
    }
}

async fn request_access_token(
    http: &HttpRuntime,
    credentials: &RequestCredentials,
    refresh_token: Option<&str>,
    cancel: &CancellationToken,
) -> Result<AccessTokenResponse, SourceError> {
    let mut form = BTreeMap::new();
    match refresh_token {
        Some(refresh_token) => {
            form.insert("grant_type".to_string(), "refresh_token".to_string());
            form.insert("refresh_token".to_string(), refresh_token.to_string());
        }
        None => {
            form.insert("grant_type".to_string(), "client_credentials".to_string());
        }
    }
    let mut token_credentials = credentials.clone();
    token_credentials.allowed_domains.insert(DOMAIN.to_string());
    token_credentials.headers.insert(
        "Authorization".to_string(),
        CLIENT_AUTHORIZATION.to_string(),
    );
    let raw = http
        .post_form_text(token_url(), &token_credentials, &form, cancel)
        .await?;
    serde_json::from_str(&raw)
        .map_err(|error| invalid_response(format!("invalid DeviantArt token: {error}")))
}

fn decode_cursor(raw: Option<&str>) -> Result<GalleryCursor, SourceError> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return Ok(GalleryCursor::default());
    };
    let raw = CURSOR.validate(raw)?;
    if let Ok(offset) = raw.parse::<u32>() {
        return Ok(GalleryCursor { offset, item: 0 });
    }
    let Some((offset, item)) = raw.strip_prefix('o').and_then(|raw| raw.split_once('i')) else {
        return Err(invalid_query("invalid DeviantArt source cursor"));
    };
    let cursor = GalleryCursor {
        offset: offset
            .parse()
            .map_err(|_| invalid_query("invalid DeviantArt source cursor"))?,
        item: item
            .parse()
            .map_err(|_| invalid_query("invalid DeviantArt source cursor"))?,
    };
    validate_cursor(cursor)
}

fn encode_cursor(cursor: GalleryCursor) -> Result<String, SourceError> {
    let cursor = validate_cursor(cursor)?;
    let encoded = format!("o{}i{}", cursor.offset, cursor.item);
    CURSOR.validate(&encoded)?;
    Ok(encoded)
}

fn validate_cursor(cursor: GalleryCursor) -> Result<GalleryCursor, SourceError> {
    if cursor.offset > 1_000_000_000 || cursor.item >= GALLERY_PAGE_SIZE as usize {
        return Err(invalid_query("invalid DeviantArt source cursor"));
    }
    Ok(cursor)
}

fn token_url() -> Url {
    Url::parse("https://www.deviantart.com/oauth2/token").expect("static DeviantArt token URL")
}

fn gallery_url(username: &str, offset: u32) -> Url {
    let mut url = Url::parse("https://www.deviantart.com/api/v1/oauth2/gallery/all")
        .expect("static DeviantArt gallery URL");
    url.query_pairs_mut()
        .append_pair("username", username)
        .append_pair("offset", &offset.to_string())
        .append_pair("limit", &GALLERY_PAGE_SIZE.to_string())
        .append_pair("mature_content", "true");
    url
}

fn download_url(deviation_id: &str) -> Result<Url, SourceError> {
    validate_deviation_id(deviation_id)?;
    let mut url = Url::parse(&format!(
        "https://www.deviantart.com/api/v1/oauth2/deviation/download/{deviation_id}"
    ))
    .expect("validated DeviantArt download URL");
    url.query_pairs_mut().append_pair("mature_content", "true");
    Ok(url)
}

fn api_credentials(credentials: &RequestCredentials, access_token: &str) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert(DOMAIN.to_string());
    credentials.headers.insert(
        "Authorization".to_string(),
        format!("Bearer {access_token}"),
    );
    credentials
        .headers
        .insert("dA-minor-version".to_string(), "20210526".to_string());
    credentials
}

fn normalize_gallery(
    request: &DiscoveryRequest,
    cursor: GalleryCursor,
    response: GalleryResponse,
) -> Result<DiscoveryBatch, SourceError> {
    let Some(index) = next_accessible_index(&response.results, cursor.item) else {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: !response.has_more,
        });
    };
    let next_accessible = next_accessible_index(&response.results, index + 1);
    let exhausted = next_accessible.is_none() && !response.has_more;
    let next = if let Some(item) = next_accessible {
        GalleryCursor {
            offset: cursor.offset,
            item,
        }
    } else {
        GalleryCursor {
            offset: next_page_offset(cursor.offset, &response),
            item: 0,
        }
    };
    let deviation = response
        .results
        .into_iter()
        .nth(index)
        .expect("validated gallery cursor");
    let post = normalize_deviation(request, encode_cursor(next)?, deviation)?;
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted,
    })
}

fn next_accessible_index(deviations: &[ApiDeviation], start: usize) -> Option<usize> {
    deviations
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, deviation)| deviation_is_accessible(deviation).then_some(index))
}

fn deviation_is_accessible(deviation: &ApiDeviation) -> bool {
    !deviation.is_deleted
        && deviation.author.is_some()
        && !deviation
            .tier_access
            .as_deref()
            .is_some_and(|access| access.eq_ignore_ascii_case("locked"))
        // Premium-folder placeholders require a separate entitlement endpoint.
        // Never publish their gallery preview as if it were the paid original.
        && deviation.premium_folder_data.is_none()
}

fn next_page_offset(offset: u32, response: &GalleryResponse) -> u32 {
    response
        .next_offset
        .unwrap_or_else(|| offset.saturating_add(response.results.len().max(1) as u32))
}

fn normalize_deviation(
    request: &DiscoveryRequest,
    resume_cursor_after: String,
    deviation: ApiDeviation,
) -> Result<SourcePost, SourceError> {
    let stable_id = validate_deviation_id(&deviation.deviationid)?.to_string();
    let creator = normalize_creator(
        &deviation
            .author
            .as_ref()
            .ok_or_else(|| invalid_response("DeviantArt deviation is missing its creator"))?
            .username,
    )?;
    let canonical_url = canonical_deviation_url(&deviation.url, &creator, &stable_id);
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &creator);
    add_general_tags(&mut tags, deviation.tags.unwrap_or_default());

    let mut media = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(mut content) = deviation.content {
        let role = if deviation.is_downloadable {
            "download"
        } else {
            "content"
        };
        let fallback = content.clone();
        let (url, transformed) = transformed_content_url(&content.src, &stable_id);
        content.src = url;
        push_media(
            &mut media,
            &mut seen,
            &stable_id,
            role,
            &canonical_url,
            content,
            transformed.then_some(fallback),
        );
    }
    if let Some(video) = deviation
        .videos
        .unwrap_or_default()
        .into_iter()
        .max_by_key(|video| video.quality_value())
    {
        push_media(
            &mut media,
            &mut seen,
            &stable_id,
            "video",
            &canonical_url,
            video.into_content(),
            None,
        );
    }
    if deviation.is_downloadable
        && !media.iter().any(|item| {
            item.stable_id.ends_with(":content") || item.stable_id.ends_with(":download")
        })
    {
        push_media(
            &mut media,
            &mut seen,
            &stable_id,
            "download",
            &canonical_url,
            ApiContent {
                src: download_url(&stable_id)?.to_string(),
                filename: None,
                filesize: None,
            },
            None,
        );
    }

    Ok(SourcePost {
        site_id: "deviantart".to_string(),
        partition: request.partition.clone(),
        stable_id,
        canonical_url: Some(canonical_url),
        creator: Some(creator),
        name: normalize_source_text(&deviation.title),
        notes: deviation
            .description
            .as_deref()
            .and_then(normalize_source_text),
        created_at: deviation.published_time.and_then(scalar_string),
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(resume_cursor_after),
    })
}

fn transformed_content_url(raw: &str, deviation_id: &str) -> (String, bool) {
    let Ok(mut url) = Url::parse(raw) else {
        return (raw.to_string(), false);
    };
    if !url
        .host_str()
        .is_some_and(|host| host.starts_with("images-wixmp-"))
    {
        return (raw.to_string(), false);
    }
    static INTERMEDIARY: OnceLock<Regex> = OnceLock::new();
    static QUALITY: OnceLock<Regex> = OnceLock::new();
    static BLUR: OnceLock<Regex> = OnceLock::new();
    let mut uses_intermediary = false;
    if deviation_id
        .parse::<u64>()
        .is_ok_and(|id| id <= 790_677_560)
    {
        let original_path = url.path().to_string();
        let path = INTERMEDIARY
            .get_or_init(|| Regex::new(r"^(/f/[^/]+/[^/]+)/v[0-9]+/.*$").expect("valid Wix path"))
            .replace(url.path(), "/intermediary$1")
            .into_owned();
        uses_intermediary = path != original_path;
        url.set_path(&path);
    }
    let value = QUALITY
        .get_or_init(|| Regex::new(r",q_[0-9]+").expect("valid Wix quality"))
        .replace(url.as_str(), ",q_100")
        .into_owned();
    (
        BLUR.get_or_init(|| Regex::new(r",blur_[0-9]+").expect("valid Wix blur"))
            .replace(&value, "")
            .into_owned(),
        uses_intermediary,
    )
}

fn push_media(
    media: &mut Vec<crate::MediaDescriptor>,
    seen: &mut BTreeSet<String>,
    deviation_id: &str,
    role: &str,
    canonical_url: &str,
    content: ApiContent,
    fallback: Option<ApiContent>,
) {
    let url = content.src.trim();
    if url.is_empty() || !seen.insert(url.to_string()) {
        return;
    }
    let position = media.len() as u32;
    let file_name = content
        .filename
        .filter(|name| !name.trim().is_empty())
        .or_else(|| file_name_from_url(url))
        .unwrap_or_else(|| format!("deviantart_{deviation_id}_{position}.media"));
    let mut descriptor =
        MediaDescriptorBuilder::new(format!("deviantart:{deviation_id}:{role}"), position, url)
            .canonical_url(canonical_url)
            .file_name(file_name)
            .expected_size(content.filesize);
    if let Some(fallback) = fallback.filter(|fallback| fallback.src.trim() != url) {
        descriptor = descriptor.fallback(MediaFallback {
            file_name: fallback
                .filename
                .filter(|name| !name.trim().is_empty())
                .or_else(|| file_name_from_url(&fallback.src)),
            url: fallback.src,
            mime_hint: None,
            expected_size: fallback.filesize,
            html_marker: None,
        });
    }
    media.push(descriptor.build());
}

fn apply_original_media(post: &mut SourcePost, original: ApiContent) {
    let mut replacement = Vec::new();
    let mut seen = BTreeSet::new();
    push_media(
        &mut replacement,
        &mut seen,
        &post.stable_id,
        "content",
        post.canonical_url.as_deref().unwrap_or_default(),
        original,
        None,
    );
    let Some(mut original) = replacement.pop() else {
        return;
    };
    if let Some(index) = post.media.iter().position(|media| {
        media.stable_id.ends_with(":download") || media.stable_id.ends_with(":content")
    }) {
        original.stable_id.clone_from(&post.media[index].stable_id);
        original.position = post.media[index].position;
        post.media[index] = original;
    } else {
        for media in &mut post.media {
            media.position = media.position.saturating_add(1);
        }
        original.position = 0;
        post.media.insert(0, original);
    }
}

fn remove_unresolved_download_placeholder(post: &mut SourcePost) {
    post.media.retain(|media| {
        !(media.stable_id.ends_with(":download") && media.url.contains("/deviation/download/"))
    });
}

fn add_general_tags(tags: &mut CanonicalTagSet, values: Vec<ApiTag>) {
    for tag in values {
        if let Some(value) = tag.value() {
            tags.insert("", value);
        }
    }
}

fn normalize_username(raw: &str) -> Result<String, SourceError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_query("DeviantArt subscriptions require a username"));
    }
    let username = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("deviantart.com" | "www.deviantart.com")
            )
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "DeviantArt subscriptions require a canonical profile URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            [username] | [username, "gallery"] => (*username).to_string(),
            _ => {
                return Err(invalid_query(
                    "DeviantArt subscriptions require a profile or gallery URL",
                ))
            }
        }
    } else {
        trimmed.to_string()
    };
    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid_query(
            "DeviantArt subscriptions require a safe username",
        ));
    }
    Ok(username)
}

fn normalize_creator(raw: &str) -> Result<String, SourceError> {
    let creator = raw.trim();
    if creator.is_empty() || creator.len() > 64 {
        return Err(invalid_response(
            "DeviantArt deviation is missing its creator",
        ));
    }
    Ok(creator.to_string())
}

fn validate_deviation_id(raw: &str) -> Result<&str, SourceError> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid_response(
            "DeviantArt returned an invalid deviation ID",
        ));
    }
    Ok(value)
}

fn canonical_deviation_url(raw: &str, creator: &str, deviation_id: &str) -> String {
    if Url::parse(raw).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && matches!(
                url.host_str(),
                Some("deviantart.com" | "www.deviantart.com")
            )
    }) {
        return raw.to_string();
    }
    format!("https://www.deviantart.com/{creator}/art/{deviation_id}")
}

fn file_name_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn scalar_string(value: Value) -> Option<String> {
    match value {
        Value::String(value) => (!value.trim().is_empty()).then_some(value),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

fn authentication_required() -> SourceError {
    SourceError::new(
        SourceErrorKind::Authentication,
        "DeviantArt authentication did not produce an access token",
        false,
    )
}

fn rejected_login() -> SourceError {
    SourceError::new(
        SourceErrorKind::Authentication,
        "DeviantArt rejected the saved login; reconnect the account",
        false,
    )
}

fn map_refresh_error(error: SourceError) -> SourceError {
    match error.kind {
        SourceErrorKind::Authentication | SourceErrorKind::InvalidResponse => rejected_login(),
        _ => error,
    }
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Clone, Deserialize)]
struct GalleryResponse {
    #[serde(default)]
    results: Vec<ApiDeviation>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_offset: Option<u32>,
}

#[derive(Clone, Deserialize)]
struct ApiDeviation {
    deviationid: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    published_time: Option<Value>,
    #[serde(default)]
    author: Option<ApiAuthor>,
    #[serde(default)]
    is_deleted: bool,
    #[serde(default)]
    tier_access: Option<String>,
    #[serde(default)]
    premium_folder_data: Option<Value>,
    #[serde(default)]
    is_downloadable: bool,
    #[serde(default)]
    content: Option<ApiContent>,
    #[serde(default)]
    videos: Option<Vec<ApiVideo>>,
    #[serde(default)]
    tags: Option<Vec<ApiTag>>,
}

#[derive(Clone, Deserialize)]
struct ApiAuthor {
    username: String,
}

#[derive(Clone, Deserialize)]
struct ApiContent {
    src: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    filesize: Option<u64>,
}

#[derive(Clone, Deserialize)]
struct ApiVideo {
    src: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    filesize: Option<u64>,
}

impl ApiVideo {
    fn quality_value(&self) -> u32 {
        self.quality
            .trim_end_matches('p')
            .parse::<u32>()
            .unwrap_or(0)
    }

    fn into_content(self) -> ApiContent {
        ApiContent {
            src: self.src,
            filename: None,
            filesize: self.filesize,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum ApiTag {
    Name(String),
    Object {
        #[serde(default)]
        tag_name: Option<String>,
        #[serde(default)]
        name: Option<String>,
    },
}

impl ApiTag {
    fn value(&self) -> Option<&str> {
        match self {
            Self::Name(value) => Some(value.as_str()),
            Self::Object { tag_name, name } => tag_name.as_deref().or(name.as_deref()),
        }
        .map(str::trim)
        .filter(|value| !value.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const GALLERY: &str = include_str!("../../tests/fixtures/deviantart/gallery.json");

    fn request(cursor: Option<&str>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "Artist-Name".to_string(),
            partition: SourcePartition::new("gallery"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 1,
        }
    }

    #[test]
    fn accepts_existing_username_and_profile_forms() {
        for value in [
            "Artist-Name",
            "https://deviantart.com/Artist-Name",
            "https://www.deviantart.com/Artist-Name/gallery/",
        ] {
            assert_eq!(normalize_username(value).unwrap(), "Artist-Name");
        }
        assert!(normalize_username("https://deviantart.com/a/gallery/?page=2").is_err());
        assert!(normalize_username("https://deviantart.com.evil.test/a").is_err());
    }

    #[test]
    fn maps_one_deviation_and_all_current_media() {
        let response: GalleryResponse = serde_json::from_str(GALLERY).unwrap();
        let batch = normalize_gallery(&request(None), GalleryCursor::default(), response).unwrap();
        assert_eq!(batch.posts.len(), 1);
        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "123456789");
        assert_eq!(post.resume_cursor_after.as_deref(), Some("o1i0"));
        assert_eq!(post.media.len(), 2);
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "ArtistName")));
        assert!(post.tags.contains(&CanonicalTag::new("", "landscape")));
        assert_eq!(post.media[1].mime_hint.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn replaces_the_gallery_preview_with_the_downloadable_original() {
        let response: GalleryResponse = serde_json::from_str(GALLERY).unwrap();
        let mut post = normalize_gallery(&request(None), GalleryCursor::default(), response)
            .unwrap()
            .posts
            .remove(0);
        let video_url = post.media[1].url.clone();

        apply_original_media(
            &mut post,
            ApiContent {
                src: "https://images-wixmp.example/f/rendered-study-original.png".into(),
                filename: Some("rendered-study-original.png".into()),
                filesize: Some(40_000),
            },
        );

        assert_eq!(post.media.len(), 2);
        assert_eq!(post.media[0].stable_id, "deviantart:123456789:download");
        assert_eq!(
            post.media[0].url,
            "https://images-wixmp.example/f/rendered-study-original.png"
        );
        assert_eq!(
            post.media[0].file_name.as_deref(),
            Some("rendered-study-original.png")
        );
        assert_eq!(post.media[0].expected_size, Some(40_000));
        assert_eq!(post.media[1].url, video_url);
    }

    #[test]
    fn original_download_request_is_scoped_to_one_valid_deviation() {
        assert_eq!(
            download_url("123456789").unwrap().as_str(),
            "https://www.deviantart.com/api/v1/oauth2/deviation/download/123456789?mature_content=true"
        );
        assert!(download_url("../other").is_err());
    }

    #[test]
    fn downloadable_without_gallery_content_still_requests_the_original() {
        let deviation: ApiDeviation = serde_json::from_value(serde_json::json!({
            "deviationid": "123456789",
            "url": "https://www.deviantart.com/artist/art/work-123456789",
            "title": "Work &amp; title",
            "description": "<p>Readable &amp; clear.</p>",
            "is_downloadable": true,
            "author": { "username": "artist" }
        }))
        .unwrap();
        let post = normalize_deviation(&request(None), "o1i0".into(), deviation).unwrap();
        assert_eq!(post.name.as_deref(), Some("Work & title"));
        assert_eq!(post.notes.as_deref(), Some("Readable & clear."));
        assert_eq!(post.media.len(), 1);
        assert_eq!(post.media[0].stable_id, "deviantart:123456789:download");
        assert_eq!(
            post.media[0].url,
            "https://www.deviantart.com/api/v1/oauth2/deviation/download/123456789?mature_content=true"
        );
    }

    #[test]
    fn rejected_original_keeps_gallery_media_but_removes_an_api_placeholder() {
        let response: GalleryResponse = serde_json::from_str(GALLERY).unwrap();
        let mut gallery_post =
            normalize_gallery(&request(None), GalleryCursor::default(), response)
                .unwrap()
                .posts
                .remove(0);
        remove_unresolved_download_placeholder(&mut gallery_post);
        assert!(!gallery_post.media.is_empty());

        let deviation: ApiDeviation = serde_json::from_value(serde_json::json!({
            "deviationid": "123456789",
            "url": "https://www.deviantart.com/artist/art/work-123456789",
            "title": "Work",
            "is_downloadable": true,
            "author": { "username": "artist" }
        }))
        .unwrap();
        let mut placeholder =
            normalize_deviation(&request(None), "o1i0".into(), deviation).unwrap();
        remove_unresolved_download_placeholder(&mut placeholder);
        assert!(placeholder.media.is_empty());
    }

    #[test]
    fn non_downloadable_wix_content_uses_gallery_dl_intermediary_quality() {
        assert_eq!(
            transformed_content_url(
                "https://images-wixmp-ed30.example/f/uuid/file.png/v1/fill/w_800,h_600,q_70,blur_5/file.png?token=x",
                "790677560",
            ).0,
            "https://images-wixmp-ed30.example/intermediary/f/uuid/file.png?token=x"
        );
        assert_eq!(
            transformed_content_url(
                "https://images-wixmp-ed30.example/f/uuid/file.png,q_70,blur_5?token=x",
                "123456789",
            )
            .0,
            "https://images-wixmp-ed30.example/f/uuid/file.png,q_100?token=x"
        );
        assert_eq!(
            transformed_content_url(
                "https://images-wixmp-ed30.example/f/uuid/file.png/v1/fill/w_800,h_600,q_70,blur_5/file.png?token=x",
                "790677561",
            ).0,
            "https://images-wixmp-ed30.example/f/uuid/file.png/v1/fill/w_800,h_600,q_100/file.png?token=x"
        );
    }

    #[test]
    fn skips_deleted_locked_and_premium_gallery_placeholders() {
        let response: GalleryResponse = serde_json::from_value(serde_json::json!({
            "results": [
                { "deviationid": "1", "is_deleted": true },
                {
                    "deviationid": "2",
                    "tier_access": "locked",
                    "author": { "username": "artist" },
                    "content": { "src": "https://images-wixmp.example/preview-2.jpg" }
                },
                {
                    "deviationid": "3",
                    "premium_folder_data": { "has_access": false },
                    "author": { "username": "artist" },
                    "content": { "src": "https://images-wixmp.example/preview-3.jpg" }
                },
                {
                    "deviationid": "4",
                    "author": { "username": "artist" },
                    "content": { "src": "https://images-wixmp.example/original-4.jpg" }
                }
            ],
            "has_more": false
        }))
        .unwrap();

        let batch = normalize_gallery(&request(None), GalleryCursor::default(), response).unwrap();
        assert_eq!(batch.posts[0].stable_id, "4");
        assert!(batch.exhausted);
    }

    #[test]
    fn intermediary_keeps_only_the_same_post_original_as_fallback() {
        let original = "https://images-wixmp-ed30.example/f/uuid/file.png/v1/fill/w_800,h_600,q_70/file.png?token=x";
        let deviation: ApiDeviation = serde_json::from_value(serde_json::json!({
            "deviationid": "790677560",
            "author": { "username": "artist" },
            "content": { "src": original, "filesize": 4000 }
        }))
        .unwrap();

        let post = normalize_deviation(&request(None), "o1i0".into(), deviation).unwrap();
        assert_eq!(post.media.len(), 1);
        assert_eq!(post.media[0].fallbacks.len(), 1);
        assert_eq!(post.media[0].fallbacks[0].url, original);
        assert_eq!(post.media[0].fallbacks[0].expected_size, Some(4000));

        let quality_only: ApiDeviation = serde_json::from_value(serde_json::json!({
            "deviationid": "790677561",
            "author": { "username": "artist" },
            "content": {
                "src": "https://images-wixmp-ed30.example/f/uuid/file.png,q_70?token=x"
            }
        }))
        .unwrap();
        let post = normalize_deviation(&request(None), "o1i0".into(), quality_only).unwrap();
        assert!(post.media[0].fallbacks.is_empty());
    }

    #[test]
    fn rejected_refresh_is_auth_but_transport_failures_remain_retryable() {
        let rejected = map_refresh_error(SourceError::new(
            SourceErrorKind::InvalidResponse,
            "400 Bad Request",
            false,
        ));
        assert_eq!(rejected.kind, SourceErrorKind::Authentication);
        let network =
            map_refresh_error(SourceError::new(SourceErrorKind::Network, "offline", true));
        assert_eq!(network.kind, SourceErrorKind::Network);
        assert!(network.retryable);
    }

    #[test]
    fn cursor_is_bounded_and_applied_only_after_the_post() {
        assert_eq!(
            decode_cursor(Some("42")).unwrap(),
            GalleryCursor {
                offset: 42,
                item: 0
            }
        );
        assert_eq!(
            decode_cursor(Some("o42i7")).unwrap(),
            GalleryCursor {
                offset: 42,
                item: 7
            }
        );
        assert!(decode_cursor(Some("o1000000001i0")).is_err());
        assert!(decode_cursor(Some("o0i24")).is_err());
        assert!(decode_cursor(Some("next")).is_err());
    }

    #[test]
    fn gallery_requests_full_pages_instead_of_one_item_per_request() {
        let url = gallery_url("artist", 48);
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "limit" && value == "24"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "offset" && value == "48"));
    }
}
