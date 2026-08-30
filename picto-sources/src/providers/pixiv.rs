use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, PageCursor, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const API_ROOT: &str = "https://app-api.pixiv.net";
const TOKEN_URL: &str = "https://oauth.secure.pixiv.net/auth/token";
const CLIENT_ID: &str = "MOBrBDS8blbauoSck0ZfDbtuzpyT";
const CLIENT_SECRET: &str = "lsACyCD94FhDUtGTXi3QzcFE2uU1hqtDaKeqrdwj";
const CURSOR: PageCursor = PageCursor::new(10_000_000);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    PixivSearchSource::default()
}

#[derive(Default)]
struct PixivSearchSource {
    api: PixivApi,
}

impl NativeSourceAdapter for PixivSearchSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "pixiv",
            display_name: "Pixiv",
            domain: "pixiv.net",
            partitions: &["illustrations"],
            anonymous: false,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        validate_search_query(query)
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            validate_search_query(&request.query)?;
            let offset = current_offset(request)?;
            let mut url = api_url("/v1/search/illust");
            url.query_pairs_mut()
                .append_pair("word", request.query.trim())
                .append_pair("search_target", "partial_match_for_tags")
                .append_pair("sort", "date_desc")
                .append_pair("offset", &offset.to_string());
            self.api
                .discover_one("pixiv", request, offset, url, credentials, http, cancel)
                .await
        })
    }
}

pub(super) struct PixivApi {
    access_token: Mutex<Option<CachedToken>>,
}

impl Default for PixivApi {
    fn default() -> Self {
        Self {
            access_token: Mutex::new(None),
        }
    }
}

struct CachedToken {
    refresh_token: String,
    value: String,
    refresh_after: Instant,
}

impl CachedToken {
    fn is_valid_for(&self, refresh_token: &str) -> bool {
        self.refresh_token == refresh_token && self.refresh_after > Instant::now()
    }
}

impl PixivApi {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn discover_one(
        &self,
        site_id: &str,
        request: &DiscoveryRequest,
        offset: u32,
        url: Url,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DiscoveryBatch, SourceError> {
        let api_credentials = self.api_credentials(credentials, http, cancel).await?;
        let response = http
            .get_json::<Value>(url, &api_credentials, cancel)
            .await?;
        normalize_response(site_id, request, offset, response)
    }

    async fn api_credentials(
        &self,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<RequestCredentials, SourceError> {
        let refresh_token = credentials
            .oauth_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| authentication("Pixiv login is required"))?;

        let mut cached = self.access_token.lock().await;
        if let Some(token) = cached
            .as_ref()
            .filter(|token| token.is_valid_for(refresh_token))
        {
            return Ok(api_credentials(&token.value));
        }

        let form = BTreeMap::from([
            ("client_id".into(), CLIENT_ID.into()),
            ("client_secret".into(), CLIENT_SECRET.into()),
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), refresh_token.into()),
            ("get_secure_url".into(), "1".into()),
        ]);
        let refresh_credentials = RequestCredentials {
            headers: pixiv_app_headers(),
            allowed_domains: ["pixiv.net".into()].into_iter().collect(),
            ..RequestCredentials::default()
        };
        let body = http
            .post_form_text(
                Url::parse(TOKEN_URL).expect("static Pixiv OAuth URL"),
                &refresh_credentials,
                &form,
                cancel,
            )
            .await
            .map_err(|error| {
                if matches!(
                    error.kind,
                    SourceErrorKind::Authentication | SourceErrorKind::InvalidResponse
                ) {
                    authentication("Pixiv OAuth session could not be refreshed")
                } else {
                    error
                }
            })?;
        let response: Value = serde_json::from_str(&body).map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })?;
        let token = response
            .get("access_token")
            .or_else(|| response.pointer("/response/access_token"))
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| authentication("Pixiv OAuth response omitted its access token"))?
            .to_string();
        let expires_in = response
            .get("expires_in")
            .or_else(|| response.pointer("/response/expires_in"))
            .and_then(Value::as_u64)
            .unwrap_or(3_600);
        let refresh_after =
            Instant::now() + Duration::from_secs(expires_in.saturating_sub(60).max(60));
        *cached = Some(CachedToken {
            refresh_token: refresh_token.to_string(),
            value: token.clone(),
            refresh_after,
        });
        Ok(api_credentials(&token))
    }
}

pub(super) fn current_offset(request: &DiscoveryRequest) -> Result<u32, SourceError> {
    request
        .cursor
        .as_deref()
        .map(|cursor| CURSOR.validate(cursor))
        .transpose()
        .map(|offset| offset.unwrap_or(0))
}

pub(super) fn api_url(path: &str) -> Url {
    let mut url = Url::parse(API_ROOT).expect("static Pixiv API root");
    url.set_path(path);
    url
}

pub(super) fn validate_numeric_user(query: &str) -> Result<String, SourceError> {
    let query = query.trim();
    if query.is_empty()
        || query.len() > 20
        || !query.bytes().all(|byte| byte.is_ascii_digit())
        || query.bytes().all(|byte| byte == b'0')
    {
        return Err(invalid_query(
            "Pixiv user subscriptions require a numeric user ID",
        ));
    }
    Ok(query.to_string())
}

fn validate_search_query(query: &str) -> Result<(), SourceError> {
    let query = query.trim();
    if query.is_empty() || query.len() > 512 || query.chars().any(char::is_control) {
        return Err(invalid_query("Pixiv search requires a tag or phrase"));
    }
    Ok(())
}

pub(super) fn normalize_response(
    site_id: &str,
    request: &DiscoveryRequest,
    offset: u32,
    response: Value,
) -> Result<DiscoveryBatch, SourceError> {
    if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
        return Err(SourceError::new(
            SourceErrorKind::InvalidResponse,
            format!("Pixiv API rejected the request: {error}"),
            false,
        ));
    }
    let illustrations = response
        .get("illusts")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("Pixiv response omitted its illustrations"))?;
    let Some(illustration) = illustrations.first() else {
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    };
    let next_offset = offset
        .checked_add(1)
        .ok_or_else(|| invalid_query("Pixiv source cursor is exhausted"))?;
    let resume_cursor_after = CURSOR.encode(next_offset)?;
    let post = normalize_illustration(site_id, request, illustration, resume_cursor_after)?;
    let exhausted = response.get("next_url").is_none_or(Value::is_null) && illustrations.len() <= 1;
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted,
    })
}

fn normalize_illustration(
    site_id: &str,
    request: &DiscoveryRequest,
    illustration: &Value,
    resume_cursor_after: String,
) -> Result<SourcePost, SourceError> {
    let id = scalar_text(illustration.get("id"))
        .filter(|id| id.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| invalid_response("Pixiv illustration omitted its stable ID"))?;
    let canonical_url = format!("https://www.pixiv.net/en/artworks/{id}");
    let creator_id = scalar_text(illustration.pointer("/user/id"))
        .filter(|id| id.bytes().any(|byte| byte != b'0'));
    let creator = illustration
        .pointer("/user/name")
        .and_then(Value::as_str)
        .and_then(normalize_source_text)
        .or(creator_id.clone());

    let mut tags = CanonicalTagSet::default();
    if let Some(creator) = creator.as_ref() {
        tags.insert("creator", creator);
    }
    if let Some(values) = illustration.get("tags").and_then(Value::as_array) {
        for tag in values {
            if let Some(name) = tag
                .get("name")
                .and_then(Value::as_str)
                .and_then(normalize_source_text)
            {
                tags.insert("", name);
            }
        }
    }
    if let Some(series) = illustration
        .get("series")
        .or_else(|| illustration.get("illust_series"))
        .and_then(|series| series.get("title"))
        .and_then(Value::as_str)
        .and_then(normalize_source_text)
    {
        tags.insert("series", series);
    }
    match illustration.get("x_restrict").and_then(Value::as_u64) {
        Some(0) => tags.insert("rating", "safe"),
        Some(1 | 2) => tags.insert("rating", "explicit"),
        _ => {}
    }

    let kind = illustration
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("illust");
    let media = if kind == "ugoira" || creator_id.is_none() {
        Vec::new()
    } else {
        collect_pages(illustration, &id, &canonical_url)
    };

    Ok(SourcePost {
        site_id: site_id.to_string(),
        partition: request.partition.clone(),
        stable_id: id,
        canonical_url: Some(canonical_url),
        creator,
        name: illustration
            .get("title")
            .and_then(Value::as_str)
            .and_then(normalize_source_text),
        notes: illustration
            .get("caption")
            .and_then(Value::as_str)
            .and_then(normalize_source_text),
        created_at: illustration
            .get("create_date")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(resume_cursor_after),
    })
}

fn collect_pages(
    illustration: &Value,
    id: &str,
    canonical_url: &str,
) -> Vec<crate::MediaDescriptor> {
    let pages = illustration
        .get("meta_pages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|page| page.pointer("/image_urls/original").and_then(Value::as_str))
        .filter(|url| usable_original_url(url))
        .collect::<Vec<_>>();
    let urls = if pages.is_empty() {
        illustration
            .pointer("/meta_single_page/original_image_url")
            .and_then(Value::as_str)
            .filter(|url| usable_original_url(url))
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        pages
    };

    urls.into_iter()
        .enumerate()
        .filter_map(|(position, url)| {
            let parsed = Url::parse(url).ok()?;
            let file_name = parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{id}_p{position}.jpg"));
            Some(
                MediaDescriptorBuilder::new(format!("pixiv:{id}:{position}"), position as u32, url)
                    .canonical_url(canonical_url)
                    .file_name(file_name)
                    .headers(BTreeMap::from([(
                        "referer".into(),
                        "https://www.pixiv.net/".into(),
                    )]))
                    .build(),
            )
        })
        .collect()
}

fn usable_original_url(url: &str) -> bool {
    !url.is_empty() && !url.contains("/common/images/limit_")
}

fn api_credentials(access_token: &str) -> RequestCredentials {
    let mut headers = pixiv_app_headers();
    headers.insert("authorization".into(), format!("Bearer {access_token}"));
    RequestCredentials {
        headers,
        allowed_domains: ["pixiv.net".into()].into_iter().collect(),
        ..RequestCredentials::default()
    }
}

fn pixiv_app_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app-os".into(), "ios".into()),
        ("app-os-version".into(), "16.7.2".into()),
        ("app-version".into(), "7.19.1".into()),
        (
            "user-agent".into(),
            "PixivIOSApp/7.19.1 (iOS 16.7.2; iPhone12,8)".into(),
        ),
        ("referer".into(), "https://app-api.pixiv.net/".into()),
    ])
}

fn scalar_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn authentication(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::Authentication, message, false)
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePartition;

    fn request(cursor: Option<&str>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "original landscape".into(),
            partition: SourcePartition::new("illustrations"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 10,
        }
    }

    #[test]
    fn maps_every_manga_page_and_canonical_metadata() {
        let response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixiv/search.json")).unwrap();
        let batch = normalize_response("pixiv", &request(None), 0, response).unwrap();
        let post = &batch.posts[0];

        assert_eq!(post.stable_id, "12345678");
        assert_eq!(post.creator.as_deref(), Some("Aoi Artist"));
        assert_eq!(post.name.as_deref(), Some("Blue sky"));
        assert_eq!(post.notes.as_deref(), Some("First line Second line"));
        assert_eq!(post.media.len(), 3);
        assert_eq!(post.media[2].stable_id, "pixiv:12345678:2");
        assert_eq!(post.resume_cursor_after.as_deref(), Some("1"));
        assert!(post
            .tags
            .contains(&crate::CanonicalTag::new("creator", "Aoi Artist")));
        assert!(post
            .tags
            .contains(&crate::CanonicalTag::new("series", "Sky Book")));
        assert!(post
            .tags
            .contains(&crate::CanonicalTag::new("rating", "safe")));
        assert!(post.tags.contains(&crate::CanonicalTag::new("", "風景")));
    }

    #[test]
    fn search_preserves_text_semantics_and_rejects_invalid_queries() {
        assert!(validate_search_query("風景 original").is_ok());
        assert!(validate_search_query("   ").is_err());
        assert!(validate_search_query("line\nbreak").is_err());
    }

    #[test]
    fn cursor_is_bounded_and_advances_one_settled_post() {
        assert_eq!(current_offset(&request(Some("42"))).unwrap(), 42);
        assert!(current_offset(&request(Some("-1"))).is_err());
        assert!(current_offset(&request(Some("10000001"))).is_err());
    }

    #[test]
    fn ugoira_does_not_publish_an_archive_as_library_media() {
        let mut response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixiv/search.json")).unwrap();
        response["illusts"][0]["type"] = Value::String("ugoira".into());
        let batch = normalize_response("pixiv", &request(None), 0, response).unwrap();
        assert!(batch.posts[0].media.is_empty());
    }

    #[test]
    fn inaccessible_placeholder_settles_as_a_no_media_post() {
        let mut response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixiv/search.json")).unwrap();
        response["illusts"][0]["user"]["id"] = Value::from(0);
        response["illusts"][0]["user"]["name"] = Value::String(String::new());
        response["illusts"][0]["meta_pages"] = Value::Array(Vec::new());
        response["illusts"][0]["meta_single_page"]["original_image_url"] =
            Value::String("https://s.pximg.net/common/images/limit_unviewable_360.png".into());
        let batch = normalize_response("pixiv", &request(None), 0, response).unwrap();
        assert!(batch.posts[0].creator.is_none());
        assert!(batch.posts[0].media.is_empty());
    }

    #[test]
    fn cached_access_tokens_are_scoped_to_the_current_login() {
        let token = CachedToken {
            refresh_token: "first-login".into(),
            value: "access-token".into(),
            refresh_after: Instant::now() + Duration::from_secs(60),
        };

        assert!(token.is_valid_for("first-login"));
        assert!(!token.is_valid_for("replacement-login"));
    }
}
