use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, PageCursor, PostFuture,
    ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const DOMAIN: &str = "deviantart.com";
const CURSOR: PageCursor = PageCursor::new(1_000_000_000);
const CLIENT_AUTHORIZATION: &str = "Basic NTM4ODo3NmIwOGM2OWNmYjI3ZjI2ZDYxNjFmOWFiNmQwNjFhMQ==";

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    DeviantArtSource::default()
}

#[derive(Default)]
struct DeviantArtSource {
    access_token: Mutex<Option<CachedAccessToken>>,
}

struct CachedAccessToken {
    credential_key: Option<String>,
    value: String,
    valid_until: Instant,
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
            let offset = current_offset(request)?;
            let access_token = self.access_token(credentials, http, cancel).await?;
            let response = http
                .get_json::<GalleryResponse>(
                    gallery_url(&username, offset),
                    &api_credentials(credentials, &access_token),
                    cancel,
                )
                .await?;
            normalize_gallery(request, offset, response)
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
            let response = http
                .get_json::<MetadataResponse>(
                    metadata_url(&post.stable_id)?,
                    &api_credentials(credentials, &access_token),
                    cancel,
                )
                .await?;
            apply_metadata(&mut post, response)?;
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
        {
            let token = self.access_token.lock().await;
            if let Some(token) = token.as_ref() {
                if token.credential_key == credential_key
                    && token.valid_until > Instant::now() + Duration::from_secs(30)
                {
                    return Ok(token.value.clone());
                }
            }
        }

        let mut form = BTreeMap::new();
        match credential_key.as_deref() {
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
        let response: AccessTokenResponse = serde_json::from_str(&raw)
            .map_err(|error| invalid_response(format!("invalid DeviantArt token: {error}")))?;
        let access_token = response
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(authentication_required)?;
        let lifetime = Duration::from_secs(response.expires_in.unwrap_or(3_600).max(60));
        *self.access_token.lock().await = Some(CachedAccessToken {
            credential_key,
            value: access_token.clone(),
            valid_until: Instant::now() + lifetime,
        });
        Ok(access_token)
    }
}

fn current_offset(request: &DiscoveryRequest) -> Result<u32, SourceError> {
    request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| CURSOR.validate(cursor))
        .transpose()
        .map(|offset| offset.unwrap_or(0))
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
        .append_pair("limit", "1")
        .append_pair("mature_content", "true");
    url
}

fn metadata_url(deviation_id: &str) -> Result<Url, SourceError> {
    validate_deviation_id(deviation_id)?;
    let mut url = Url::parse("https://www.deviantart.com/api/v1/oauth2/deviation/metadata")
        .expect("static DeviantArt metadata URL");
    url.query_pairs_mut()
        .append_pair("deviationids[0]", deviation_id)
        .append_pair("mature_content", "true");
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
    offset: u32,
    response: GalleryResponse,
) -> Result<DiscoveryBatch, SourceError> {
    if response.results.len() > 1 {
        return Err(invalid_response(
            "DeviantArt returned more than one deviation for a one-deviation request",
        ));
    }
    let exhausted = response.results.is_empty() || !response.has_more;
    let next_offset = response
        .next_offset
        .unwrap_or_else(|| offset.saturating_add(1));
    let posts = response
        .results
        .into_iter()
        .map(|deviation| normalize_deviation(request, next_offset, deviation))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DiscoveryBatch { posts, exhausted })
}

fn normalize_deviation(
    request: &DiscoveryRequest,
    next_offset: u32,
    deviation: ApiDeviation,
) -> Result<SourcePost, SourceError> {
    let stable_id = validate_deviation_id(&deviation.deviationid)?.to_string();
    let creator = normalize_creator(&deviation.author.username)?;
    let canonical_url = canonical_deviation_url(&deviation.url, &creator, &stable_id);
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &creator);
    add_general_tags(&mut tags, deviation.tags.unwrap_or_default());

    let mut media = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(content) = deviation.content {
        push_media(&mut media, &mut seen, &stable_id, &canonical_url, content);
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
            &canonical_url,
            video.into_content(),
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
        resume_cursor_after: Some(CURSOR.encode(next_offset)?),
    })
}

fn push_media(
    media: &mut Vec<crate::MediaDescriptor>,
    seen: &mut BTreeSet<String>,
    deviation_id: &str,
    canonical_url: &str,
    content: ApiContent,
) {
    let url = content.src.trim();
    if url.is_empty() || !seen.insert(url.to_string()) {
        return;
    }
    let position = media.len() as u32;
    let file_name = file_name_from_url(url)
        .unwrap_or_else(|| format!("deviantart_{deviation_id}_{position}.media"));
    media.push(
        MediaDescriptorBuilder::new(
            format!("deviantart:{deviation_id}:{position}"),
            position,
            url,
        )
        .canonical_url(canonical_url)
        .file_name(file_name)
        .expected_size(content.filesize)
        .build(),
    );
}

fn apply_metadata(post: &mut SourcePost, response: MetadataResponse) -> Result<(), SourceError> {
    if response.metadata.len() > 1 {
        return Err(invalid_response(
            "DeviantArt returned metadata for more than one deviation",
        ));
    }
    let Some(metadata) = response.metadata.into_iter().next() else {
        return Ok(());
    };
    if metadata.deviationid.as_deref() != Some(post.stable_id.as_str()) {
        return Err(invalid_response(
            "DeviantArt metadata did not match the active deviation",
        ));
    }
    let mut tags = CanonicalTagSet::default();
    if let Some(creator) = post.creator.as_deref() {
        tags.insert("creator", creator);
    }
    add_general_tags(&mut tags, metadata.tags);
    for tag in std::mem::take(&mut post.tags) {
        tags.insert(tag.namespace, tag.value);
    }
    post.tags = tags.into_vec();
    if post.notes.is_none() {
        post.notes = metadata
            .description
            .as_deref()
            .and_then(normalize_source_text);
    }
    Ok(())
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

#[derive(Deserialize)]
struct AccessTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct GalleryResponse {
    #[serde(default)]
    results: Vec<ApiDeviation>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_offset: Option<u32>,
}

#[derive(Deserialize)]
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
    author: ApiAuthor,
    #[serde(default)]
    content: Option<ApiContent>,
    #[serde(default)]
    videos: Option<Vec<ApiVideo>>,
    #[serde(default)]
    tags: Option<Vec<ApiTag>>,
}

#[derive(Deserialize)]
struct ApiAuthor {
    username: String,
}

#[derive(Deserialize)]
struct ApiContent {
    src: String,
    #[serde(default)]
    filesize: Option<u64>,
}

#[derive(Deserialize)]
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
            filesize: self.filesize,
        }
    }
}

#[derive(Deserialize)]
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

#[derive(Deserialize)]
struct MetadataResponse {
    #[serde(default)]
    metadata: Vec<ApiMetadata>,
}

#[derive(Deserialize)]
struct ApiMetadata {
    #[serde(default)]
    deviationid: Option<String>,
    #[serde(default)]
    tags: Vec<ApiTag>,
    #[serde(default)]
    description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const GALLERY: &str = include_str!("../../tests/fixtures/deviantart/gallery.json");
    const METADATA: &str = include_str!("../../tests/fixtures/deviantart/metadata.json");

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
        let batch = normalize_gallery(&request(None), 0, response).unwrap();
        assert_eq!(batch.posts.len(), 1);
        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "123456789");
        assert_eq!(post.resume_cursor_after.as_deref(), Some("1"));
        assert_eq!(post.media.len(), 2);
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "ArtistName")));
        assert!(post.tags.contains(&CanonicalTag::new("", "landscape")));
        assert_eq!(post.media[1].mime_hint.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn metadata_is_bounded_to_and_merged_into_the_active_deviation() {
        let response: GalleryResponse = serde_json::from_str(GALLERY).unwrap();
        let mut post = normalize_gallery(&request(None), 0, response)
            .unwrap()
            .posts
            .remove(0);
        let metadata: MetadataResponse = serde_json::from_str(METADATA).unwrap();
        apply_metadata(&mut post, metadata).unwrap();
        assert!(post.tags.contains(&CanonicalTag::new("", "concept-art")));
        assert_eq!(post.notes.as_deref(), Some("Rendered study & notes"));
    }

    #[test]
    fn cursor_is_bounded_and_applied_only_after_the_post() {
        assert_eq!(current_offset(&request(Some("42"))).unwrap(), 42);
        assert!(current_offset(&request(Some("1000000001"))).is_err());
        assert!(current_offset(&request(Some("next"))).is_err());
    }
}
