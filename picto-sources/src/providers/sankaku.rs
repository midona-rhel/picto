use std::collections::BTreeMap;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    AdapterFuture, CanonicalTag, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest, HttpRuntime,
    MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, PostFuture, ProviderDescriptor,
    RatingMap, RequestCredentials, SearchQueryPolicy, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: OpaqueCursor = OpaqueCursor::new(2_048);
const QUERY: SearchQueryPolicy = SearchQueryPolicy::new(
    "Sankaku",
    &[
        "page:",
        "limit:",
        "order:",
        "sort:",
        "id:",
        "id_range:",
        "next:",
    ],
);
const RATINGS: RatingMap = RatingMap::new(&[
    ("s", "safe"),
    ("safe", "safe"),
    ("q", "questionable"),
    ("questionable", "questionable"),
    ("e", "explicit"),
    ("explicit", "explicit"),
]);
const MAX_TAG_PAGES: u32 = 10;

#[derive(Clone, Copy)]
pub(super) struct SankakuConfig {
    pub id: &'static str,
    pub display_name: &'static str,
    pub domain: &'static str,
    pub root: &'static str,
    pub api_root: &'static str,
}

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    adapter_for(SankakuConfig {
        id: "sankaku",
        display_name: "Sankaku",
        domain: "sankaku.app",
        root: "https://sankaku.app",
        api_root: "https://sankakuapi.com",
    })
}

pub(super) fn adapter_for(config: SankakuConfig) -> impl NativeSourceAdapter {
    SankakuAdapter { config }
}

struct SankakuAdapter {
    config: SankakuConfig,
}

impl NativeSourceAdapter for SankakuAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.config.id,
            display_name: self.config.display_name,
            domain: self.config.domain,
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        QUERY.validate(query)
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            self.validate_query(&request.query)?;
            let credentials = api_credentials(self.config, credentials);
            let response = http
                .get_json::<ApiPage>(request_url(self.config, request)?, &credentials, cancel)
                .await?;
            normalize_page(self.config, request, response)
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
            if post.media.is_empty() {
                return Ok(post);
            }
            let credentials = api_credentials(self.config, credentials);
            let records =
                fetch_tags(self.config, &post.stable_id, &credentials, http, cancel).await?;
            if !records.is_empty() {
                let (tags, creator) = canonical_tags(&records, existing_rating(&post.tags));
                post.tags = tags;
                post.creator = creator;
            }
            Ok(post)
        })
    }
}

fn api_credentials(config: SankakuConfig, credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    if let Some(access_token) = credentials.cookies.remove("accessToken") {
        credentials
            .headers
            .insert("Authorization".into(), format!("Bearer {access_token}"));
    }
    credentials.cookies.remove("refreshToken");
    credentials.cookies.remove("ssoLoginValid");
    let api_domain = Url::parse(config.api_root)
        .expect("static Sankaku API URL")
        .host_str()
        .expect("Sankaku API URL has a host")
        .to_string();
    credentials.allowed_domains.insert(api_domain);
    credentials.headers.insert(
        "Accept".into(),
        "application/vnd.sankaku.api+json;v=2".into(),
    );
    let root = Url::parse(config.root).expect("static Sankaku provider URL");
    let origin = format!(
        "{}://{}{}",
        root.scheme(),
        root.host_str().expect("Sankaku provider URL has a host"),
        root.port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default()
    );
    credentials.headers.insert("Origin".into(), origin);
    credentials
}

pub(super) fn request_url(
    config: SankakuConfig,
    request: &DiscoveryRequest,
) -> Result<Url, SourceError> {
    let mut url = Url::parse(&format!("{}/v2/posts/keyset", config.api_root))
        .expect("static Sankaku keyset URL");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("lang", "en");
        // Discovery is deliberately one post wide: no later post may be
        // prefetched before the current post settles.
        query.append_pair("limit", "1");
        query.append_pair("tags", request.query.trim());
        if let Some(cursor) = request
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
        {
            query.append_pair("next", CURSOR.validate(cursor)?);
        }
    }
    Ok(url)
}

pub(super) fn normalize_page(
    config: SankakuConfig,
    request: &DiscoveryRequest,
    response: ApiPage,
) -> Result<DiscoveryBatch, SourceError> {
    if response.success == Some(false) {
        return Err(SourceError::new(
            SourceErrorKind::InvalidResponse,
            response
                .code
                .unwrap_or_else(|| format!("{} rejected the search", config.display_name)),
            false,
        ));
    }
    if response.data.len() > 1 {
        return Err(SourceError::new(
            SourceErrorKind::InvalidResponse,
            format!(
                "{} returned more than one post for a single-post request",
                config.display_name
            ),
            false,
        ));
    }

    let next = response
        .meta
        .and_then(|meta| meta.next)
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| CURSOR.validate(&cursor).map(ToOwned::to_owned))
        .transpose()?;
    let exhausted = next.is_none();
    let current = request.cursor.clone();
    let last = response.data.len().saturating_sub(1);
    let posts = response
        .data
        .into_iter()
        .enumerate()
        .map(|(index, post)| {
            let resume_cursor_after = if index == last {
                next.clone().or_else(|| current.clone())
            } else {
                current.clone()
            };
            normalize_post(config, request, post, resume_cursor_after)
        })
        .collect::<Result<Vec<_>, SourceError>>()?;

    Ok(DiscoveryBatch { posts, exhausted })
}

fn normalize_post(
    config: SankakuConfig,
    request: &DiscoveryRequest,
    post: ApiPost,
    resume_cursor_after: Option<String>,
) -> Result<SourcePost, SourceError> {
    let post_id = post.id.as_string();
    if post_id.is_empty() {
        return Err(SourceError::new(
            SourceErrorKind::InvalidResponse,
            format!("{} returned a post without an ID", config.display_name),
            false,
        ));
    }
    let canonical_url = format!("{}/posts/{post_id}", config.root);
    let mut tags = CanonicalTagSet::default();
    for tag in &post.tag_names {
        tags.insert("", normalize_tag_name(tag));
    }
    RATINGS.add(&mut tags, post.rating.as_deref());

    let media = post
        .file_url
        .as_deref()
        .and_then(normalize_media_url)
        .map(|url| {
            let file_name = file_name(config.id, &post_id, &post, &url);
            MediaDescriptorBuilder::new(format!("{}:{post_id}:0", config.id), 0, url)
                .canonical_url(&canonical_url)
                .file_name(file_name)
                .expected_size(post.file_size)
                .headers(BTreeMap::from([("Referer".into(), canonical_url.clone())]))
                .build()
        })
        .into_iter()
        .collect();

    Ok(SourcePost {
        site_id: config.id.into(),
        partition: request.partition.clone(),
        stable_id: post_id.clone(),
        canonical_url: Some(canonical_url),
        creator: None,
        name: Some(format!("{}_{post_id}", config.id)),
        notes: None,
        created_at: post.created_at.and_then(TimeValue::into_string),
        tags: tags.into_vec(),
        media,
        resume_cursor_after,
    })
}

async fn fetch_tags(
    config: SankakuConfig,
    post_id: &str,
    credentials: &RequestCredentials,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<Vec<ApiTag>, SourceError> {
    let mut records = Vec::new();
    for page in 1..=MAX_TAG_PAGES {
        let mut url = Url::parse(&format!("{}/posts/{post_id}/tags", config.api_root))
            .expect("static Sankaku tags URL");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("lang", "en");
            query.append_pair("page", &page.to_string());
            query.append_pair("limit", "100");
        }
        let response = http
            .get_json::<ApiTagPage>(url, credentials, cancel)
            .await?;
        if response.success == Some(false) {
            return Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                response
                    .code
                    .unwrap_or_else(|| format!("{} rejected tag metadata", config.display_name)),
                false,
            ));
        }
        let count = response.data.len();
        records.extend(response.data);
        if count == 0 || records.len() >= response.total.unwrap_or(records.len()) || count < 100 {
            return Ok(records);
        }
    }
    Err(SourceError::new(
        SourceErrorKind::InvalidResponse,
        format!(
            "{} returned more than {} tags for one post",
            config.display_name,
            MAX_TAG_PAGES * 100
        ),
        false,
    ))
}

fn canonical_tags(records: &[ApiTag], rating: Option<&str>) -> (Vec<CanonicalTag>, Option<String>) {
    let mut tags = CanonicalTagSet::default();
    let mut creator = None;
    for record in records {
        let Some(name) = record.name.as_deref().map(normalize_tag_name) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let namespace = match record.kind.as_ref().and_then(Scalar::as_i64) {
            Some(1) => "creator",
            Some(3) => "series",
            Some(4) => "character",
            _ => "",
        };
        if namespace == "creator" && creator.is_none() {
            creator = Some(name.clone());
        }
        tags.insert(namespace, name);
    }
    RATINGS.add(&mut tags, rating);
    (tags.into_vec(), creator)
}

fn existing_rating(tags: &[CanonicalTag]) -> Option<&str> {
    tags.iter()
        .find(|tag| tag.namespace == "rating")
        .map(|tag| tag.value.as_str())
}

fn normalize_tag_name(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(' ', "_")
}

fn normalize_media_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(path) = value.strip_prefix("//") {
        return Some(format!("https://{path}"));
    }
    if let Some(path) = value.strip_prefix("http://") {
        return Some(format!("https://{path}"));
    }
    Url::parse(value).ok().map(|url| url.to_string())
}

fn file_name(config_id: &str, post_id: &str, post: &ApiPost, media_url: &str) -> String {
    let extension = post
        .file_type
        .as_deref()
        .and_then(extension_from_type)
        .or_else(|| extension_from_url(media_url));
    match (
        post.md5.as_deref().filter(|value| !value.is_empty()),
        extension,
    ) {
        (Some(hash), Some(extension)) => format!("{hash}.{extension}"),
        (_, Some(extension)) => format!("{config_id}_{post_id}.{extension}"),
        _ => format!("{config_id}_{post_id}.media"),
    }
}

fn extension_from_type(value: &str) -> Option<&str> {
    let value = value.trim().trim_start_matches('.');
    if value.is_empty() {
        None
    } else if let Some((_, subtype)) = value.split_once('/') {
        match subtype {
            "jpeg" => Some("jpg"),
            "svg+xml" => Some("svg"),
            subtype if subtype.bytes().all(|byte| byte.is_ascii_alphanumeric()) => Some(subtype),
            _ => None,
        }
    } else if value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Some(value)
    } else {
        None
    }
}

fn extension_from_url(value: &str) -> Option<&str> {
    let path = value.split(['?', '#']).next()?;
    let extension = path.rsplit_once('.')?.1;
    extension_from_type(extension)
}

#[derive(Debug, Deserialize)]
pub(super) struct ApiPage {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    data: Vec<ApiPost>,
    #[serde(default)]
    meta: Option<ApiMeta>,
}

#[derive(Debug, Deserialize)]
struct ApiMeta {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiPost {
    id: Scalar,
    #[serde(default)]
    created_at: Option<TimeValue>,
    #[serde(default)]
    rating: Option<String>,
    #[serde(default)]
    file_url: Option<String>,
    #[serde(default)]
    file_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    tag_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiTagPage {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    data: Vec<ApiTag>,
    #[serde(default)]
    total: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ApiTag {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<Scalar>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Scalar {
    String(String),
    Unsigned(u64),
    Signed(i64),
}

impl Scalar {
    fn as_string(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Unsigned(value) => value.to_string(),
            Self::Signed(value) => value.to_string(),
        }
    }

    fn into_string(self) -> String {
        match self {
            Self::String(value) => value,
            Self::Unsigned(value) => value.to_string(),
            Self::Signed(value) => value.to_string(),
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::String(value) => value.parse().ok(),
            Self::Unsigned(value) => i64::try_from(*value).ok(),
            Self::Signed(value) => Some(*value),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TimeValue {
    Object { s: Scalar },
    Scalar(Scalar),
}

impl TimeValue {
    fn into_string(self) -> Option<String> {
        let value = match self {
            Self::Object { s } | Self::Scalar(s) => s.into_string(),
        };
        (!value.is_empty()).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    fn config() -> SankakuConfig {
        SankakuConfig {
            id: "sankaku",
            display_name: "Sankaku",
            domain: "sankaku.app",
            root: "https://sankaku.app",
            api_root: "https://sankakuapi.com",
        }
    }

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "solo rating:safe".into(),
            partition: SourcePartition::new("posts"),
            cursor: Some("opaque-next".into()),
            page_size: 250,
        }
    }

    #[test]
    fn builds_bounded_keyset_requests_with_an_opaque_cursor() {
        let url = request_url(config(), &request()).unwrap();
        let query = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(query.get("limit").map(|value| value.as_ref()), Some("1"));
        assert_eq!(
            query.get("tags").map(|value| value.as_ref()),
            Some("solo rating:safe")
        );
        assert_eq!(
            query.get("next").map(|value| value.as_ref()),
            Some("opaque-next")
        );
        assert!(request_url(
            config(),
            &DiscoveryRequest {
                cursor: Some("bad\ncursor".into()),
                ..request()
            }
        )
        .is_err());
    }

    #[test]
    fn maps_one_post_and_advances_the_cursor_after_settlement() {
        let response: ApiPage = serde_json::from_value(json!({
            "success": true,
            "data": [{
                "id": 40001234,
                "created_at": {"s": 1786632138},
                "rating": "s",
                "file_url": "http://v.sankakucomplex.com/data/example.jpg?expires=1",
                "file_type": "image/jpeg",
                "file_size": 1234,
                "md5": "0123456789abcdef0123456789abcdef",
                "tag_names": ["Solo", "High Resolution"]
            }],
            "meta": {"next": "next-page"}
        }))
        .unwrap();

        let batch = normalize_page(config(), &request(), response).unwrap();
        assert!(!batch.exhausted);
        assert_eq!(
            batch.posts[0].resume_cursor_after.as_deref(),
            Some("next-page")
        );
        assert_eq!(
            batch.posts[0].canonical_url.as_deref(),
            Some("https://sankaku.app/posts/40001234")
        );
        assert_eq!(
            batch.posts[0].media[0].url,
            "https://v.sankakucomplex.com/data/example.jpg?expires=1"
        );
        assert_eq!(
            batch.posts[0].media[0].file_name.as_deref(),
            Some("0123456789abcdef0123456789abcdef.jpg")
        );
    }

    #[test]
    fn rejects_an_api_response_that_prefetches_a_later_post() {
        let response: ApiPage = serde_json::from_value(json!({
            "data": [{"id": 2}, {"id": 1}],
            "meta": {"next": "next-page"}
        }))
        .unwrap();
        assert!(normalize_page(config(), &request(), response).is_err());
    }

    #[test]
    fn missing_file_url_is_a_traversed_post_without_usable_media() {
        let response: ApiPage = serde_json::from_value(json!({
            "data": [{"id": 40001233, "status": "deleted", "file_url": null}],
            "meta": {"next": "next-page"}
        }))
        .unwrap();
        let batch = normalize_page(config(), &request(), response).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(batch.posts[0].media.is_empty());
    }

    #[test]
    fn maps_only_canonical_tag_groups_and_flattens_every_other_type() {
        let records: Vec<ApiTag> = serde_json::from_value(json!([
            {"name": "Artist Name", "type": 1},
            {"name": "Studio Name", "type": 2},
            {"name": "Series Name", "type": 3},
            {"name": "Character Name", "type": 4},
            {"name": "Genre Name", "type": 5},
            {"name": "Medium Name", "type": "8"},
            {"name": "High Resolution", "type": 9}
        ]))
        .unwrap();
        let (tags, creator) = canonical_tags(&records, Some("explicit"));

        assert_eq!(creator.as_deref(), Some("artist_name"));
        assert!(tags.contains(&CanonicalTag::new("creator", "artist_name")));
        assert!(tags.contains(&CanonicalTag::new("series", "series_name")));
        assert!(tags.contains(&CanonicalTag::new("character", "character_name")));
        assert!(tags.contains(&CanonicalTag::new("rating", "explicit")));
        for value in [
            "studio_name",
            "genre_name",
            "medium_name",
            "high_resolution",
        ] {
            assert!(tags.contains(&CanonicalTag::new("", value)));
        }
        assert!(tags.iter().all(|tag| matches!(
            tag.namespace.as_str(),
            "" | "creator" | "character" | "series" | "species" | "rating"
        )));
    }

    #[test]
    fn rejects_query_owned_traversal_controls() {
        assert!(QUERY.validate("solo rating:safe").is_ok());
        assert!(QUERY.validate("solo order:random").is_err());
        assert!(QUERY.validate("solo id_range:123").is_err());
        assert!(QUERY.validate("solo next:opaque").is_err());
    }

    #[test]
    fn api_headers_are_scoped_to_the_provider_api() {
        let credentials = api_credentials(config(), &RequestCredentials::default());
        assert!(credentials.allowed_domains.contains("sankakuapi.com"));
        assert_eq!(
            credentials.headers.get("Origin").map(String::as_str),
            Some("https://sankaku.app")
        );
    }

    #[test]
    fn captured_sso_token_becomes_api_bearer_auth() {
        let credentials = RequestCredentials {
            cookies: [
                ("accessToken".into(), "captured".into()),
                ("refreshToken".into(), "refresh".into()),
                ("ssoLoginValid".into(), "true".into()),
            ]
            .into_iter()
            .collect(),
            allowed_domains: ["sankaku.app".into()].into_iter().collect(),
            ..RequestCredentials::default()
        };
        let api = api_credentials(
            SankakuConfig {
                id: "sankaku",
                display_name: "Sankaku",
                domain: "sankaku.app",
                root: "https://sankaku.app",
                api_root: "https://sankakuapi.com",
            },
            &credentials,
        );
        assert_eq!(
            api.headers.get("Authorization").map(String::as_str),
            Some("Bearer captured")
        );
        assert!(api.cookies.is_empty());
        assert!(api.allowed_domains.contains("sankakuapi.com"));
    }
}
