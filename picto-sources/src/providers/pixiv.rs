use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Days, Utc};
use md5::{Digest, Md5};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptor, MediaDescriptorBuilder, MediaFallback, MediaFuture,
    MediaPostprocess, NativeSourceAdapter, OpaqueCursor, PageCursor, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost, UgoiraFrame,
};

const API_ROOT: &str = "https://app-api.pixiv.net";
const TOKEN_URL: &str = "https://oauth.secure.pixiv.net/auth/token";
const CLIENT_ID: &str = "MOBrBDS8blbauoSck0ZfDbtuzpyT";
const CLIENT_SECRET: &str = "lsACyCD94FhDUtGTXi3QzcFE2uU1hqtDaKeqrdwj";
const HASH_SECRET: &str = "28c1fdd170a5204386cb1313c7077b34f83e4aaf4aa829ce78c231e05b0bae2c";
const PAGE_CURSOR: PageCursor = PageCursor::new(10_000_000);
const SEARCH_CURSOR: OpaqueCursor = OpaqueCursor::new(4_096);
const MAX_SEARCH_PAGES_PER_DISCOVERY: usize = 128;

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
            self.api
                .discover_search(request, credentials, http, cancel)
                .await
        })
    }

    fn resolve_media<'a>(
        &'a self,
        media: MediaDescriptor,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> MediaFuture<'a> {
        Box::pin(async move {
            self.api
                .resolve_ugoira(media, credentials, http, cancel)
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
    async fn discover_search(
        &self,
        request: &DiscoveryRequest,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DiscoveryBatch, SourceError> {
        let api_credentials = self.api_credentials(credentials, http, cancel).await?;
        let mut cursor = search_cursor(request)?;
        for _ in 0..MAX_SEARCH_PAGES_PER_DISCOVERY {
            let response = http
                .get_json::<Value>(cursor.url.clone(), &api_credentials, cancel)
                .await?;
            match normalize_search_response(request, &cursor, response)? {
                SearchPageResult::Post(batch) => return Ok(batch),
                SearchPageResult::Continue(next) => cursor = next,
                SearchPageResult::Exhausted => {
                    return Ok(DiscoveryBatch {
                        posts: Vec::new(),
                        exhausted: true,
                    });
                }
            }
        }
        Err(invalid_response(
            "Pixiv search pagination exceeded its safety bound",
        ))
    }

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
            headers: pixiv_refresh_headers(SystemTime::now()),
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

    pub(super) async fn resolve_ugoira(
        &self,
        media: MediaDescriptor,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<MediaDescriptor, SourceError> {
        if media.mime_hint.as_deref() != Some("application/x-ugoira") {
            return Ok(media);
        }
        let metadata_url = validate_ugoira_metadata_url(&media)?;
        let api_credentials = self.api_credentials(credentials, http, cancel).await?;
        let response = http
            .get_json::<Value>(metadata_url, &api_credentials, cancel)
            .await?;
        if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
            return Err(invalid_response(format!(
                "Pixiv rejected ugoira metadata: {error}"
            )));
        }
        apply_ugoira_metadata(media, &response)
    }
}

fn apply_ugoira_metadata(
    mut media: MediaDescriptor,
    response: &Value,
) -> Result<MediaDescriptor, SourceError> {
    let zip_url = ["original", "medium"]
        .into_iter()
        .find_map(|quality| {
            response
                .pointer(&format!("/ugoira_metadata/zip_urls/{quality}"))
                .and_then(Value::as_str)
                .and_then(safe_pixiv_media_url)
        })
        .ok_or_else(|| invalid_response("Pixiv ugoira metadata omitted its frame archive"))?;
    let frames = response
        .pointer("/ugoira_metadata/frames")
        .cloned()
        .ok_or_else(|| invalid_response("Pixiv ugoira metadata omitted its frame timings"))
        .and_then(|frames| {
            serde_json::from_value::<Vec<UgoiraFrame>>(frames).map_err(|error| {
                invalid_response(format!(
                    "Pixiv returned invalid ugoira frame timings: {error}"
                ))
            })
        })?;
    if frames.is_empty() {
        return Err(invalid_response(
            "Pixiv ugoira metadata contained no frames",
        ));
    }
    media.url = zip_url;
    let id = media
        .stable_id
        .strip_prefix("pixiv:")
        .and_then(|value| value.strip_suffix(":ugoira"))
        .expect("validated Pixiv ugoira stable ID");
    media.file_name = Some(format!("{id}.webm"));
    media.mime_hint = Some("video/webm".into());
    media.expected_size = None;
    media.postprocess = Some(MediaPostprocess::UgoiraToWebm { frames });
    Ok(media)
}

pub(super) fn current_offset(request: &DiscoveryRequest) -> Result<u32, SourceError> {
    request
        .cursor
        .as_deref()
        .map(|cursor| PAGE_CURSOR.validate(cursor))
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchCursor {
    url: Url,
    item: usize,
    boundary: Option<String>,
}

enum SearchPageResult {
    Post(DiscoveryBatch),
    Continue(SearchCursor),
    Exhausted,
}

fn search_cursor(request: &DiscoveryRequest) -> Result<SearchCursor, SourceError> {
    let word = request.query.trim();
    let Some(raw) = request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
    else {
        let mut url = api_url("/v1/search/illust");
        url.query_pairs_mut()
            .append_pair("word", word)
            .append_pair("search_target", "partial_match_for_tags")
            .append_pair("sort", "date_desc");
        return Ok(SearchCursor {
            url,
            item: 0,
            boundary: None,
        });
    };
    SEARCH_CURSOR.validate(raw)?;
    let mut url = Url::parse(raw).map_err(|_| invalid_search_cursor())?;
    let fragment = url
        .fragment()
        .ok_or_else(invalid_search_cursor)?
        .to_string();
    url.set_fragment(None);
    validate_search_url(&url, word)?;
    let mut item = None;
    let mut boundary = None;
    for (key, value) in url::form_urlencoded::parse(fragment.as_bytes()) {
        match key.as_ref() {
            "item" if item.is_none() => {
                item = value.parse::<usize>().ok().filter(|item| *item <= 100)
            }
            "boundary" if boundary.is_none() => {
                DateTime::parse_from_rfc3339(&value).map_err(|_| invalid_search_cursor())?;
                boundary = Some(value.into_owned());
            }
            _ => return Err(invalid_search_cursor()),
        }
    }
    Ok(SearchCursor {
        url,
        item: item.ok_or_else(invalid_search_cursor)?,
        boundary,
    })
}

fn encode_search_cursor(cursor: &SearchCursor, word: &str) -> Result<String, SourceError> {
    validate_search_url(&cursor.url, word)?;
    if cursor.item > 100 {
        return Err(invalid_search_cursor());
    }
    let mut fragment = url::form_urlencoded::Serializer::new(String::new());
    fragment.append_pair("item", &cursor.item.to_string());
    if let Some(boundary) = cursor.boundary.as_deref() {
        DateTime::parse_from_rfc3339(boundary).map_err(|_| invalid_search_cursor())?;
        fragment.append_pair("boundary", boundary);
    }
    let mut url = cursor.url.clone();
    url.set_fragment(Some(&fragment.finish()));
    let cursor = url.to_string();
    SEARCH_CURSOR.validate(&cursor)?;
    Ok(cursor)
}

fn validate_search_url(url: &Url, word: &str) -> Result<(), SourceError> {
    let words = url
        .query_pairs()
        .filter(|(key, _)| key == "word")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if url.scheme() != "https"
        || url.host_str() != Some("app-api.pixiv.net")
        || url.path() != "/v1/search/illust"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || words.as_slice() != [word]
    {
        return Err(invalid_search_cursor());
    }
    Ok(())
}

fn normalize_search_response(
    request: &DiscoveryRequest,
    cursor: &SearchCursor,
    response: Value,
) -> Result<SearchPageResult, SourceError> {
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
    if cursor.item > illustrations.len() || illustrations.len() > 100 {
        return Err(invalid_response(
            "Pixiv search cursor is outside its response",
        ));
    }

    let mut index = cursor.item;
    let mut boundary = cursor.boundary.clone();
    while boundary.is_some() && index < illustrations.len() {
        let work_date = illustrations[index]
            .get("create_date")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("Pixiv rollover work omitted its creation date"))?;
        index += 1;
        if search_boundary_reached(boundary.as_deref().expect("checked boundary"), work_date)? {
            boundary = None;
        }
    }

    if index >= illustrations.len() {
        return match next_search_page(request, &response, boundary)? {
            Some(next) => Ok(SearchPageResult::Continue(next)),
            None => Ok(SearchPageResult::Exhausted),
        };
    }

    let next_page = if index + 1 < illustrations.len() {
        Some(SearchCursor {
            url: cursor.url.clone(),
            item: index + 1,
            boundary: None,
        })
    } else {
        next_search_page(request, &response, None)?
    };
    let exhausted = next_page.is_none();
    let resume = next_page.unwrap_or_else(|| SearchCursor {
        url: cursor.url.clone(),
        item: illustrations.len(),
        boundary: None,
    });
    let resume_cursor_after = encode_search_cursor(&resume, request.query.trim())?;
    let post =
        normalize_illustration("pixiv", request, &illustrations[index], resume_cursor_after)?;
    Ok(SearchPageResult::Post(DiscoveryBatch {
        posts: vec![post],
        exhausted,
    }))
}

fn next_search_page(
    request: &DiscoveryRequest,
    response: &Value,
    pending_boundary: Option<String>,
) -> Result<Option<SearchCursor>, SourceError> {
    let Some(raw) = response
        .get("next_url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
    else {
        return Ok(None);
    };
    let mut url =
        Url::parse(raw).map_err(|_| invalid_response("Pixiv returned an invalid next URL"))?;
    validate_search_url(&url, request.query.trim())?;
    let mut boundary = pending_boundary;
    let offset = url
        .query_pairs()
        .find(|(key, _)| key == "offset")
        .and_then(|(_, value)| value.parse::<u32>().ok())
        .unwrap_or(0);
    if boundary.is_none() && offset >= 5_000 {
        let date_last = response
            .get("illusts")
            .and_then(Value::as_array)
            .and_then(|works| works.last())
            .and_then(|work| work.get("create_date"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("Pixiv rollover page omitted its boundary work"))?;
        let end_date = rollover_end_date(date_last)?;
        let pairs = url
            .query_pairs()
            .filter(|(key, _)| key != "offset" && key != "end_date")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        url.set_query(None);
        url.query_pairs_mut()
            .extend_pairs(pairs)
            .append_pair("end_date", &end_date);
        boundary = Some(date_last.to_string());
    }
    let cursor = SearchCursor {
        url,
        item: 0,
        boundary,
    };
    encode_search_cursor(&cursor, request.query.trim())?;
    Ok(Some(cursor))
}

fn search_boundary_reached(boundary: &str, work_date: &str) -> Result<bool, SourceError> {
    let boundary = DateTime::parse_from_rfc3339(boundary)
        .map_err(|_| invalid_response("Pixiv returned an invalid rollover boundary"))?;
    let work_date = DateTime::parse_from_rfc3339(work_date)
        .map_err(|_| invalid_response("Pixiv returned an invalid work creation date"))?;
    Ok(work_date <= boundary)
}

fn rollover_end_date(date_last: &str) -> Result<String, SourceError> {
    let date = DateTime::parse_from_rfc3339(date_last)
        .map_err(|_| invalid_response("Pixiv returned an invalid rollover boundary"))?
        .date_naive()
        .checked_add_days(Days::new(1))
        .ok_or_else(|| invalid_response("Pixiv rollover date is outside its supported range"))?;
    Ok(date.format("%Y-%m-%d").to_string())
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
    let resume_cursor_after = PAGE_CURSOR.encode(next_offset)?;
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
    let media = if creator_id.is_none() {
        Vec::new()
    } else if kind == "ugoira" {
        vec![ugoira_descriptor(&id, &canonical_url)]
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

fn ugoira_descriptor(id: &str, canonical_url: &str) -> MediaDescriptor {
    let mut url = api_url("/v1/ugoira/metadata");
    url.query_pairs_mut().append_pair("illust_id", id);
    let mut descriptor =
        MediaDescriptorBuilder::new(format!("pixiv:{id}:ugoira"), 0, url.to_string())
            .canonical_url(canonical_url)
            .file_name(format!("{id}.ugoira"))
            .headers(BTreeMap::from([(
                "referer".into(),
                "https://www.pixiv.net/".into(),
            )]))
            .build();
    descriptor.mime_hint = Some("application/x-ugoira".into());
    descriptor
}

fn validate_ugoira_metadata_url(media: &MediaDescriptor) -> Result<Url, SourceError> {
    let url = Url::parse(&media.url)
        .map_err(|_| invalid_response("Pixiv returned an invalid ugoira metadata URL"))?;
    let expected_id = media
        .stable_id
        .strip_prefix("pixiv:")
        .and_then(|value| value.strip_suffix(":ugoira"))
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| invalid_response("Pixiv ugoira omitted its stable illustration ID"))?;
    let ids = url
        .query_pairs()
        .filter(|(key, _)| key == "illust_id")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if url.scheme() != "https"
        || url.host_str() != Some("app-api.pixiv.net")
        || url.path() != "/v1/ugoira/metadata"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || ids.len() != 1
        || ids[0] != expected_id
    {
        return Err(invalid_response(
            "Pixiv returned an unsafe ugoira metadata URL",
        ));
    }
    Ok(url)
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
        .filter_map(pixiv_page_candidate)
        .collect::<Vec<_>>();
    let pages = if pages.is_empty() {
        illustration
            .pointer("/meta_single_page/original_image_url")
            .and_then(Value::as_str)
            .and_then(pixiv_single_page_candidate)
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        pages
    };

    pages
        .into_iter()
        .enumerate()
        .filter_map(|(position, page)| {
            let parsed = Url::parse(&page.original).ok()?;
            let file_name = parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{id}_p{position}.jpg"));
            let mut descriptor = MediaDescriptorBuilder::new(
                format!("pixiv:{id}:{position}"),
                position as u32,
                page.original,
            )
            .canonical_url(canonical_url)
            .file_name(file_name)
            .headers(BTreeMap::from([(
                "referer".into(),
                "https://www.pixiv.net/".into(),
            )]));
            for fallback in page.fallbacks {
                descriptor = descriptor.fallback(MediaFallback {
                    file_name: file_name_from_url(&fallback),
                    url: fallback,
                    mime_hint: None,
                    expected_size: None,
                    html_marker: None,
                });
            }
            Some(descriptor.build())
        })
        .collect()
}

struct PixivPageCandidate {
    original: String,
    fallbacks: Vec<String>,
}

fn pixiv_single_page_candidate(original: &str) -> Option<PixivPageCandidate> {
    if !usable_original_url(original) {
        return None;
    }
    let original = safe_pixiv_media_url(original)?;
    let fallbacks = master_fallback_url(&original).into_iter().collect();
    Some(PixivPageCandidate {
        original,
        fallbacks,
    })
}

fn pixiv_page_candidate(page: &Value) -> Option<PixivPageCandidate> {
    let urls = page.get("image_urls")?;
    let original = urls.get("original")?.as_str()?;
    if !usable_original_url(original) {
        return None;
    }
    let original = safe_pixiv_media_url(original)?;
    let mut fallbacks = Vec::new();
    if let Some(fallback) = master_fallback_url(&original) {
        fallbacks.push(fallback);
    }
    for key in ["large", "medium", "square_medium"] {
        if let Some(fallback) = urls
            .get(key)
            .and_then(Value::as_str)
            .and_then(safe_pixiv_media_url)
            .filter(|fallback| fallback != &original && !fallbacks.contains(fallback))
        {
            fallbacks.push(fallback);
        }
    }
    Some(PixivPageCandidate {
        original,
        fallbacks,
    })
}

fn master_fallback_url(original: &str) -> Option<String> {
    let (base, _) = original.rsplit_once('.')?;
    safe_pixiv_media_url(&format!(
        "{}_master1200.jpg",
        base.replacen("-original/", "-master/", 1)
    ))
}

fn safe_pixiv_media_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    (url.scheme() == "https"
        && (host == "pximg.net" || host.ends_with(".pximg.net"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none())
    .then(|| url.to_string())
}

fn file_name_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
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

fn pixiv_refresh_headers(now: SystemTime) -> BTreeMap<String, String> {
    let mut headers = pixiv_app_headers();
    let time = DateTime::<Utc>::from(now)
        .format("%Y-%m-%dT%H:%M:%S+00:00")
        .to_string();
    headers.insert(
        "X-Client-Hash".into(),
        md5_hex(&format!("{time}{HASH_SECRET}")),
    );
    headers.insert("X-Client-Time".into(), time);
    headers
}

fn md5_hex(input: &str) -> String {
    format!("{:x}", Md5::digest(input.as_bytes()))
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

fn invalid_search_cursor() -> SourceError {
    invalid_query("invalid Pixiv search cursor")
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
            query: "original".into(),
            partition: SourcePartition::new("illustrations"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 10,
        }
    }

    #[test]
    fn maps_every_manga_page_and_canonical_metadata() {
        let response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixiv/search.json")).unwrap();
        let req = request(None);
        let cursor = search_cursor(&req).unwrap();
        let SearchPageResult::Post(batch) =
            normalize_search_response(&req, &cursor, response).unwrap()
        else {
            panic!("expected one work");
        };
        let post = &batch.posts[0];

        assert_eq!(post.stable_id, "12345678");
        assert_eq!(post.creator.as_deref(), Some("Aoi Artist"));
        assert_eq!(post.name.as_deref(), Some("Blue sky"));
        assert_eq!(post.notes.as_deref(), Some("First line Second line"));
        assert_eq!(post.media.len(), 3);
        assert_eq!(post.media[2].stable_id, "pixiv:12345678:2");
        let resumed =
            search_cursor(&request(Some(post.resume_cursor_after.as_deref().unwrap()))).unwrap();
        assert_eq!(resumed.item, 1);
        assert_eq!(resumed.url, cursor.url);
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
        assert_eq!(post.media[0].fallbacks.len(), 1);
        assert!(post.media[0].fallbacks[0].url.contains("/img-master/"));
        assert!(post.media[0].fallbacks[0].url.ends_with("_master1200.jpg"));
    }

    #[test]
    fn search_preserves_text_semantics_and_rejects_invalid_queries() {
        assert!(validate_search_query("風景 original").is_ok());
        assert!(validate_search_query("   ").is_err());
        assert!(validate_search_query("line\nbreak").is_err());
    }

    #[test]
    fn search_cursor_is_bounded_and_query_scoped() {
        assert!(search_cursor(&request(Some("42"))).is_err());
        let cursor = SearchCursor {
            url: api_url("/v1/search/illust"),
            item: 0,
            boundary: None,
        };
        assert!(encode_search_cursor(&cursor, "original").is_err());
    }

    #[test]
    fn ugoira_publishes_the_typed_animation_container() {
        let mut response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixiv/search.json")).unwrap();
        response["illusts"][0]["type"] = Value::String("ugoira".into());
        let req = request(None);
        let SearchPageResult::Post(batch) =
            normalize_search_response(&req, &search_cursor(&req).unwrap(), response).unwrap()
        else {
            panic!("expected one work");
        };
        let media = &batch.posts[0].media;
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].stable_id, "pixiv:12345678:ugoira");
        assert_eq!(media[0].mime_hint.as_deref(), Some("application/x-ugoira"));
        assert_eq!(media[0].file_name.as_deref(), Some("12345678.ugoira"));
        assert!(media[0]
            .url
            .ends_with("/v1/ugoira/metadata?illust_id=12345678"));
    }

    #[test]
    fn ugoira_metadata_resolves_to_a_timed_vp9_conversion() {
        let descriptor = ugoira_descriptor("12345678", "https://www.pixiv.net/artworks/12345678");
        let response = serde_json::json!({
            "ugoira_metadata": {
                "zip_urls": {
                    "original": "https://i.pximg.net/img-zip-ugoira/12345678.zip"
                },
                "frames": [
                    {"file": "000000.jpg", "delay": 60},
                    {"file": "000001.jpg", "delay": 100}
                ]
            }
        });

        let resolved = apply_ugoira_metadata(descriptor, &response).unwrap();

        assert_eq!(resolved.file_name.as_deref(), Some("12345678.webm"));
        assert_eq!(resolved.mime_hint.as_deref(), Some("video/webm"));
        assert!(matches!(
            resolved.postprocess,
            Some(MediaPostprocess::UgoiraToWebm { ref frames })
                if frames.len() == 2 && frames[0].delay == 60 && frames[1].delay == 100
        ));
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
        let req = request(None);
        let SearchPageResult::Post(batch) =
            normalize_search_response(&req, &search_cursor(&req).unwrap(), response).unwrap()
        else {
            panic!("expected one work");
        };
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

    #[test]
    fn refresh_headers_match_pixivs_signed_timestamp_contract() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        let headers =
            pixiv_refresh_headers(std::time::UNIX_EPOCH + Duration::from_secs(1_704_067_200));
        assert_eq!(
            headers.get("X-Client-Time").map(String::as_str),
            Some("2024-01-01T00:00:00+00:00")
        );
        assert_eq!(
            headers.get("X-Client-Hash").map(String::as_str),
            Some("2f075776ebea3e46c4741f1a188d93dc")
        );
    }

    #[test]
    fn structured_preview_fallbacks_are_limited_to_pixiv_cdns() {
        let page = serde_json::json!({
            "image_urls": {
                "original": "https://i.pximg.net/img-original/work.png",
                "large": "https://evil.test/preview.jpg",
                "medium": "https://i.pximg.net/img-master/work_medium.jpg"
            }
        });
        let candidate = pixiv_page_candidate(&page).unwrap();
        assert_eq!(candidate.fallbacks.len(), 2);
        assert!(candidate
            .fallbacks
            .iter()
            .all(|url| url.contains("pximg.net")));
    }

    #[test]
    fn search_follows_next_url_until_the_4999_offset() {
        let req = request(None);
        let cursor = search_cursor(&req).unwrap();
        let response = search_page(
            vec![search_work("5001", "2026-08-20T12:30:00+09:00")],
            Some("https://app-api.pixiv.net/v1/search/illust?word=original&filter=for_ios&offset=4999"),
        );
        let SearchPageResult::Post(batch) =
            normalize_search_response(&req, &cursor, response).unwrap()
        else {
            panic!("expected one work");
        };
        let resumed = search_cursor(&request(Some(
            batch.posts[0].resume_cursor_after.as_deref().unwrap(),
        )))
        .unwrap();
        assert_eq!(
            resumed
                .url
                .query_pairs()
                .find(|(key, _)| key == "offset")
                .map(|(_, value)| value.into_owned()),
            Some("4999".to_string())
        );
        assert!(resumed
            .url
            .query_pairs()
            .any(|(key, value)| key == "filter" && value == "for_ios"));
        assert!(resumed.boundary.is_none());
    }

    #[test]
    fn search_rolls_at_5000_and_skips_the_boundary_work() {
        let req = request(None);
        let cursor = search_cursor(&req).unwrap();
        let boundary_date = "2026-08-20T12:30:00+09:00";
        let response = search_page(
            vec![search_work("5001", boundary_date)],
            Some("https://app-api.pixiv.net/v1/search/illust?word=original&filter=for_ios&offset=5000"),
        );
        let SearchPageResult::Post(batch) =
            normalize_search_response(&req, &cursor, response).unwrap()
        else {
            panic!("expected the boundary work");
        };
        let rollover = search_cursor(&request(Some(
            batch.posts[0].resume_cursor_after.as_deref().unwrap(),
        )))
        .unwrap();
        assert_eq!(rollover.boundary.as_deref(), Some(boundary_date));
        assert!(!rollover.url.query_pairs().any(|(key, _)| key == "offset"));
        assert!(rollover
            .url
            .query_pairs()
            .any(|(key, value)| key == "end_date" && value == "2026-08-21"));

        let rollover_response = search_page(
            vec![
                search_work("5001", boundary_date),
                search_work("5000", "2026-08-20T12:29:59+09:00"),
            ],
            None,
        );
        let SearchPageResult::Post(batch) =
            normalize_search_response(&req, &rollover, rollover_response).unwrap()
        else {
            panic!("expected the first work after the overlap");
        };
        assert_eq!(batch.posts[0].stable_id, "5000");
    }

    fn search_page(works: Vec<Value>, next_url: Option<&str>) -> Value {
        serde_json::json!({
            "illusts": works,
            "next_url": next_url
        })
    }

    fn search_work(id: &str, create_date: &str) -> Value {
        let mut response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixiv/search.json")).unwrap();
        let mut work = response["illusts"][0].take();
        work["id"] = Value::String(id.to_string());
        work["create_date"] = Value::String(create_date.to_string());
        work
    }
}
