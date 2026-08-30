use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const PAGE_LENGTH: u32 = 50;
const MAX_CURSOR_BYTES: usize = 1_024;

#[derive(Clone, Copy)]
pub(super) struct ArchiveProvider {
    pub id: &'static str,
    pub display_name: &'static str,
    pub domain: &'static str,
    pub query_domains: &'static [&'static str],
    pub site_root: &'static str,
    pub api_root: &'static str,
    pub media_root: &'static str,
    pub creator_posts_suffix: &'static str,
    pub accept: &'static str,
    pub file_first: bool,
}

pub(super) const fn archive_adapter(provider: ArchiveProvider) -> ArchiveSource {
    ArchiveSource(provider)
}

pub(super) struct ArchiveSource(ArchiveProvider);

impl NativeSourceAdapter for ArchiveSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.0.id,
            display_name: self.0.display_name,
            domain: self.0.domain,
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_creator_query(self.0, query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let creator = normalize_creator_query(self.0, &request.query)?;
            let cursor = decode_cursor(self.0, &creator, request.cursor.as_deref())?;
            let credentials = api_credentials(self.0, credentials);
            let response = http
                .get_json::<Value>(
                    listing_url(self.0, &creator, cursor.offset)?,
                    &credentials,
                    cancel,
                )
                .await?;
            normalize_page(self.0, request, &creator, cursor, response)
        })
    }

    fn resolve_post<'a>(
        &'a self,
        post: SourcePost,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> crate::PostFuture<'a> {
        Box::pin(async move {
            let locator = locator_from_post(self.0, &post)?;
            let credentials = api_credentials(self.0, credentials);
            let detail_url = detail_url(self.0, &locator, &post.stable_id)?;
            let detail = http
                .get_optional_json::<Value>(detail_url, &credentials, cancel)
                .await?;
            let Some(detail) = detail else {
                return Ok(post_without_detail(post));
            };
            let profile_url = profile_url(self.0, &locator)?;
            let profile = http
                .get_optional_json::<Value>(profile_url, &credentials, cancel)
                .await?;
            normalize_detail(self.0, post, &locator, detail, profile)
        })
    }
}

fn post_without_detail(mut post: SourcePost) -> SourcePost {
    post.media.clear();
    post
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreatorLocator {
    service: String,
    creator_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CursorState {
    pub offset: u32,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CursorAnchor {
    after_id: String,
    next_id: String,
}

fn api_credentials(
    provider: ArchiveProvider,
    credentials: &RequestCredentials,
) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials
        .allowed_domains
        .insert(provider.domain.to_string());
    credentials
        .headers
        .entry("Accept".to_string())
        .or_insert_with(|| provider.accept.to_string());
    credentials
        .headers
        .entry("Accept-Encoding".to_string())
        .or_insert_with(|| "identity".to_string());
    credentials
}

pub(super) fn normalize_creator_query(
    provider: ArchiveProvider,
    raw: &str,
) -> Result<CreatorLocator, SourceError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_query(
            provider,
            "subscriptions require a creator URL or service:user ID",
        ));
    }

    if raw.starts_with("http://") || raw.starts_with("https://") {
        let url = Url::parse(raw)
            .map_err(|_| invalid_query(provider, "subscriptions require a valid creator URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !provider
                .query_domains
                .iter()
                .any(|domain| url.host_str() == Some(*domain))
        {
            return Err(invalid_query(
                provider,
                "subscriptions require a canonical creator URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() != 3 || segments[1] != "user" {
            return Err(invalid_query(
                provider,
                "subscriptions require a creator URL, not a post URL",
            ));
        }
        return creator_locator(provider, segments[0], segments[2]);
    }

    let Some((service, creator_id)) = raw.split_once(':') else {
        return Err(invalid_query(
            provider,
            "subscriptions require service:user for compact queries",
        ));
    };
    if creator_id.contains(':') {
        return Err(invalid_query(
            provider,
            "subscriptions require exactly one service and user ID",
        ));
    }
    creator_locator(provider, service, creator_id)
}

fn creator_locator(
    provider: ArchiveProvider,
    service: &str,
    creator_id: &str,
) -> Result<CreatorLocator, SourceError> {
    let service = service.trim().to_ascii_lowercase();
    let creator_id = creator_id.trim();
    if !valid_component(&service, 32) || !valid_component(creator_id, 160) {
        return Err(invalid_query(
            provider,
            "creator service or user ID contains unsupported characters",
        ));
    }
    Ok(CreatorLocator {
        service,
        creator_id: creator_id.to_string(),
    })
}

fn valid_component(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn listing_url(
    provider: ArchiveProvider,
    creator: &CreatorLocator,
    offset: u32,
) -> Result<Url, SourceError> {
    let mut url = api_url(
        provider,
        &[
            creator.service.as_str(),
            "user",
            creator.creator_id.as_str(),
        ],
        provider.creator_posts_suffix,
    )?;
    url.query_pairs_mut().append_pair("o", &offset.to_string());
    Ok(url)
}

fn detail_url(
    provider: ArchiveProvider,
    creator: &CreatorLocator,
    post_id: &str,
) -> Result<Url, SourceError> {
    if !valid_component(post_id, 256) {
        return Err(invalid_response(provider, "post has an invalid ID"));
    }
    let mut url = api_url(
        provider,
        &[
            creator.service.as_str(),
            "user",
            creator.creator_id.as_str(),
            "post",
            post_id,
        ],
        "",
    )?;
    // Cached detail/profile responses can ignore identity encoding and arrive as raw gzip.
    url.query_pairs_mut().append_pair("_picto", "1");
    Ok(url)
}

fn profile_url(provider: ArchiveProvider, creator: &CreatorLocator) -> Result<Url, SourceError> {
    let mut url = api_url(
        provider,
        &[
            creator.service.as_str(),
            "user",
            creator.creator_id.as_str(),
            "profile",
        ],
        "",
    )?;
    url.query_pairs_mut().append_pair("_picto", "1");
    Ok(url)
}

fn api_url(provider: ArchiveProvider, segments: &[&str], suffix: &str) -> Result<Url, SourceError> {
    let mut url = Url::parse(provider.api_root).expect("static archive provider API URL");
    url.path_segments_mut()
        .map_err(|_| invalid_response(provider, "provider API URL cannot hold path segments"))?
        .extend(segments);
    if !suffix.is_empty() {
        let path = format!("{}{}", url.path().trim_end_matches('/'), suffix);
        url.set_path(&path);
    }
    Ok(url)
}

fn canonical_post_url(
    provider: ArchiveProvider,
    creator: &CreatorLocator,
    post_id: &str,
) -> Result<String, SourceError> {
    let mut url = Url::parse(provider.site_root).expect("static archive provider site URL");
    url.path_segments_mut()
        .map_err(|_| invalid_response(provider, "provider site URL cannot hold path segments"))?
        .extend([
            creator.service.as_str(),
            "user",
            creator.creator_id.as_str(),
            "post",
            post_id,
        ]);
    Ok(url.to_string())
}

fn encode_anchored_cursor(
    provider: ArchiveProvider,
    creator: &CreatorLocator,
    cursor: CursorState,
    after_id: &str,
    next_id: &str,
) -> Result<String, SourceError> {
    if cursor.index >= PAGE_LENGTH
        || !valid_component(after_id, 256)
        || (!next_id.is_empty() && !valid_component(next_id, 256))
    {
        return Err(invalid_cursor(provider));
    }
    let value = format!(
        "v1|{}|{}|{}|{}|{}|{}",
        creator.service, creator.creator_id, cursor.offset, cursor.index, after_id, next_id
    );
    if value.len() > MAX_CURSOR_BYTES {
        return Err(invalid_cursor(provider));
    }
    Ok(value)
}

fn decode_cursor_anchor(
    provider: ArchiveProvider,
    creator: &CreatorLocator,
    raw: Option<&str>,
) -> Result<Option<CursorAnchor>, SourceError> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parts = raw.split('|').collect::<Vec<_>>();
    if parts.len() != 7
        || parts[0] != "v1"
        || parts[1] != creator.service
        || parts[2] != creator.creator_id
        || !valid_component(parts[5], 256)
        || (!parts[6].is_empty() && !valid_component(parts[6], 256))
    {
        return Err(invalid_cursor(provider));
    }
    Ok(Some(CursorAnchor {
        after_id: parts[5].to_string(),
        next_id: parts[6].to_string(),
    }))
}

pub(super) fn decode_cursor(
    provider: ArchiveProvider,
    creator: &CreatorLocator,
    raw: Option<&str>,
) -> Result<CursorState, SourceError> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return Ok(CursorState {
            offset: 0,
            index: 0,
        });
    };
    if raw.len() > MAX_CURSOR_BYTES || raw.chars().any(char::is_control) {
        return Err(invalid_cursor(provider));
    }
    let parts = raw.split('|').collect::<Vec<_>>();
    if parts.len() != 7
        || parts[0] != "v1"
        || parts[1] != creator.service
        || parts[2] != creator.creator_id
    {
        return Err(invalid_cursor(provider));
    }
    let offset = parts[3]
        .parse::<u32>()
        .map_err(|_| invalid_cursor(provider))?;
    let index = parts[4]
        .parse::<u32>()
        .map_err(|_| invalid_cursor(provider))?;
    let cursor = CursorState { offset, index };
    encode_anchored_cursor(provider, creator, cursor, parts[5], parts[6])?;
    Ok(cursor)
}

pub(super) fn normalize_page(
    provider: ArchiveProvider,
    request: &DiscoveryRequest,
    creator: &CreatorLocator,
    cursor: CursorState,
    response: Value,
) -> Result<DiscoveryBatch, SourceError> {
    let anchor = decode_cursor_anchor(provider, creator, request.cursor.as_deref())?;
    let posts = response
        .as_array()
        .or_else(|| response.get("posts").and_then(Value::as_array))
        .ok_or_else(|| invalid_response(provider, "creator response is missing its posts"))?;
    if posts.len() > PAGE_LENGTH as usize {
        return Err(invalid_response(
            provider,
            "creator response exceeded the fixed page bound",
        ));
    }
    if posts.is_empty() {
        if anchor
            .as_ref()
            .is_some_and(|anchor| !anchor.next_id.is_empty())
            || (cursor.index != 0 && anchor.is_none())
        {
            return Err(invalid_response(
                provider,
                "persisted cursor no longer identifies a page item",
            ));
        }
        return Ok(DiscoveryBatch {
            posts: Vec::new(),
            exhausted: true,
        });
    }
    let summary_index = if let Some(anchor) = anchor.as_ref() {
        let after_index = posts
            .iter()
            .position(|post| text(post, "id") == Some(anchor.after_id.as_str()));
        if after_index == Some(posts.len() - 1) && posts.len() < PAGE_LENGTH as usize {
            return Ok(DiscoveryBatch {
                posts: Vec::new(),
                exhausted: true,
            });
        }
        (!anchor.next_id.is_empty())
            .then(|| {
                posts
                    .iter()
                    .position(|post| text(post, "id") == Some(anchor.next_id.as_str()))
            })
            .flatten()
            .or_else(|| {
                after_index
                    .and_then(|index| index.checked_add(1))
                    .filter(|index| *index < posts.len())
            })
            .or_else(|| anchor.next_id.is_empty().then_some(0))
            .ok_or_else(|| {
                invalid_response(
                    provider,
                    "persisted cursor anchors no longer identify the next page item",
                )
            })?
    } else {
        cursor.index as usize
    };
    let summary = posts.get(summary_index).ok_or_else(|| {
        invalid_response(
            provider,
            "persisted cursor no longer identifies a page item",
        )
    })?;
    validate_post_owner(provider, summary, creator)?;
    let stable_id = required_component(provider, summary, "id", 256, "post")?;
    let next = if summary_index + 1 < posts.len() {
        Some(CursorState {
            offset: cursor.offset,
            index: (summary_index + 1) as u32,
        })
    } else if posts.len() == PAGE_LENGTH as usize {
        Some(CursorState {
            offset: cursor
                .offset
                .checked_add(PAGE_LENGTH - 1)
                .ok_or_else(|| invalid_cursor(provider))?,
            index: 1,
        })
    } else {
        None
    };
    let resume_cursor_after = match next {
        Some(next) if next.offset == cursor.offset => {
            let next_id =
                required_component(provider, &posts[summary_index + 1], "id", 256, "post")?;
            Some(encode_anchored_cursor(
                provider, creator, next, stable_id, next_id,
            )?)
        }
        Some(next) => Some(encode_anchored_cursor(
            provider, creator, next, stable_id, "",
        )?),
        None => None,
    };
    let canonical_url = canonical_post_url(provider, creator, stable_id)?;
    let post = SourcePost {
        site_id: provider.id.to_string(),
        partition: request.partition.clone(),
        stable_id: stable_id.to_string(),
        canonical_url: Some(canonical_url),
        creator: Some(creator.creator_id.clone()),
        name: text(summary, "title").and_then(normalize_source_text),
        notes: text(summary, "substring").and_then(normalize_source_text),
        created_at: text(summary, "published")
            .or_else(|| text(summary, "added"))
            .map(ToOwned::to_owned),
        tags: Vec::new(),
        media: Vec::new(),
        resume_cursor_after,
    };
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted: next.is_none(),
    })
}

fn locator_from_post(
    provider: ArchiveProvider,
    post: &SourcePost,
) -> Result<CreatorLocator, SourceError> {
    if post.site_id != provider.id {
        return Err(invalid_response(
            provider,
            "cannot resolve a post from another provider",
        ));
    }
    let url = post
        .canonical_url
        .as_deref()
        .and_then(|value| Url::parse(value).ok())
        .ok_or_else(|| invalid_response(provider, "post is missing its canonical URL"))?;
    let segments = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if url.host_str() != Some(provider.domain)
        || segments.len() != 5
        || segments[1] != "user"
        || segments[3] != "post"
        || segments[4] != post.stable_id
    {
        return Err(invalid_response(
            provider,
            "post has an invalid canonical URL",
        ));
    }
    creator_locator(provider, segments[0], segments[2])
        .map_err(|_| invalid_response(provider, "post has an invalid creator identity"))
}

pub(super) fn normalize_detail(
    provider: ArchiveProvider,
    mut source_post: SourcePost,
    creator: &CreatorLocator,
    response: Value,
    profile: impl Into<Option<Value>>,
) -> Result<SourcePost, SourceError> {
    let post = response.get("post").unwrap_or(&response);
    validate_post_owner(provider, post, creator)?;
    if required_component(provider, post, "id", 256, "post")? != source_post.stable_id {
        return Err(invalid_response(
            provider,
            "detail response resolved a different post",
        ));
    }
    let profile = profile.into();
    let creator_name = if let Some(profile) = profile.as_ref() {
        let profile = profile.get("profile").unwrap_or(profile);
        let profile_id = required_component(provider, profile, "id", 160, "creator profile")?;
        let profile_service =
            required_component(provider, profile, "service", 32, "creator profile")?;
        if profile_id != creator.creator_id
            || !profile_service.eq_ignore_ascii_case(&creator.service)
        {
            return Err(invalid_response(
                provider,
                "profile response resolved a different creator",
            ));
        }
        text(profile, "name")
            .unwrap_or(&creator.creator_id)
            .to_string()
    } else {
        creator.creator_id.clone()
    };
    let canonical_url = canonical_post_url(provider, creator, &source_post.stable_id)?;

    source_post.canonical_url = Some(canonical_url.clone());
    source_post.creator = Some(creator_name.clone());
    source_post.name = text(post, "title").and_then(normalize_source_text);
    source_post.notes = text(post, "content")
        .or_else(|| text(post, "substring"))
        .and_then(normalize_source_text);
    source_post.created_at = text(post, "published")
        .or_else(|| text(post, "added"))
        .map(ToOwned::to_owned);
    source_post.tags = post_tags(post, &creator_name).into_vec();
    source_post.media = post_media(
        provider,
        post,
        &response,
        &source_post.stable_id,
        &canonical_url,
    )?;
    Ok(source_post)
}

fn validate_post_owner(
    provider: ArchiveProvider,
    post: &Value,
    creator: &CreatorLocator,
) -> Result<(), SourceError> {
    let service = required_component(provider, post, "service", 32, "post")?;
    let creator_id = required_component(provider, post, "user", 160, "post")?;
    if !service.eq_ignore_ascii_case(&creator.service) || creator_id != creator.creator_id {
        return Err(invalid_response(
            provider,
            "response returned a post for another creator",
        ));
    }
    Ok(())
}

fn post_tags(post: &Value, creator_name: &str) -> CanonicalTagSet {
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", creator_name);
    for raw in flat_tags(post.get("tags")) {
        let (group, value) = raw
            .split_once(':')
            .map(|(group, value)| (group.trim().to_ascii_lowercase(), value.trim()))
            .unwrap_or_else(|| (String::new(), raw.trim()));
        if value.is_empty() {
            continue;
        }
        let namespace = match group.as_str() {
            "artist" | "creator" => "creator",
            "character" => "character",
            "copyright" | "series" => "series",
            "species" => "species",
            "rating" => "rating",
            _ => "",
        };
        tags.insert(namespace, value);
    }
    tags
}

fn flat_tags(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("name").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
            })
            .collect(),
        Value::String(raw) => {
            if let Ok(Value::Array(values)) = serde_json::from_str::<Value>(raw) {
                return values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect();
            }
            raw.trim_matches(['[', ']'])
                .split(',')
                .map(|tag| tag.trim().trim_matches('"').to_string())
                .filter(|tag| !tag.is_empty())
                .collect()
        }
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone)]
struct MediaCandidate {
    path: String,
    name: Option<String>,
    server: Option<String>,
}

fn post_media(
    provider: ArchiveProvider,
    post: &Value,
    detail: &Value,
    post_id: &str,
    canonical_url: &str,
) -> Result<Vec<crate::MediaDescriptor>, SourceError> {
    let mut servers = BTreeMap::new();
    for key in ["attachments", "videos"] {
        for value in detail
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(path), Some(server)) = (text(value, "path"), text(value, "server")) {
                servers.insert(normalized_path(path), server.to_string());
            }
        }
    }

    let mut candidates = Vec::new();
    let file = post.get("file");
    let attachments = post.get("attachments").and_then(Value::as_array);
    if provider.file_first {
        push_candidate(&mut candidates, file, &servers);
        push_candidates(&mut candidates, attachments, &servers);
    } else {
        push_candidates(&mut candidates, attachments, &servers);
        push_candidate(&mut candidates, file, &servers);
    }
    for key in ["attachments", "videos"] {
        push_candidates(
            &mut candidates,
            detail.get(key).and_then(Value::as_array),
            &servers,
        );
    }
    for path in inline_paths(text(post, "content").unwrap_or_default()) {
        candidates.push(MediaCandidate {
            name: file_name_from_path(&path),
            server: None,
            path,
        });
    }

    let headers = BTreeMap::from([
        ("Accept-Encoding".to_string(), "identity".to_string()),
        ("Referer".to_string(), canonical_url.to_string()),
    ]);
    let mut seen = BTreeSet::new();
    let mut media = Vec::new();
    for candidate in candidates {
        if is_preview_candidate(&candidate) {
            continue;
        }
        let url = media_url(provider, &candidate)?;
        let identity = content_hash(&candidate.path)
            .map(|hash| format!("hash:{hash}"))
            .unwrap_or_else(|| format!("url:{url}"));
        if !seen.insert(identity) {
            continue;
        }
        let file_name = candidate
            .name
            .as_deref()
            .and_then(safe_file_name)
            .or_else(|| file_name_from_path(&candidate.path))
            .unwrap_or_else(|| format!("{}_{}_{}", provider.id, post_id, media.len()));
        if crate::media::is_unsupported_archive(&file_name)
            || crate::media::is_unsupported_archive(&url)
        {
            continue;
        }
        let stable_tail = content_hash(&candidate.path)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| media.len().to_string());
        media.push(
            MediaDescriptorBuilder::new(
                format!("{}:{}:{}", provider.id, post_id, stable_tail),
                media.len() as u32,
                url,
            )
            .canonical_url(canonical_url)
            .file_name(file_name)
            .headers(headers.clone())
            .build(),
        );
    }
    Ok(media)
}

fn is_preview_candidate(candidate: &MediaCandidate) -> bool {
    let path = candidate.path.to_ascii_lowercase();
    if path.contains("/thumbnail/") || path.contains("/preview/") {
        return true;
    }
    [
        &candidate.path,
        candidate.server.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .filter_map(|raw| Url::parse(raw).ok())
    .filter_map(|url| url.host_str().map(ToOwned::to_owned))
    .any(|host| host.starts_with("img."))
}

fn push_candidates(
    candidates: &mut Vec<MediaCandidate>,
    values: Option<&Vec<Value>>,
    servers: &BTreeMap<String, String>,
) {
    for value in values.into_iter().flatten() {
        push_candidate(candidates, Some(value), servers);
    }
}

fn push_candidate(
    candidates: &mut Vec<MediaCandidate>,
    value: Option<&Value>,
    servers: &BTreeMap<String, String>,
) {
    let Some(value) = value else { return };
    let Some(path) = text(value, "path").or_else(|| value.as_str()) else {
        return;
    };
    let path = normalized_path(path);
    candidates.push(MediaCandidate {
        name: text(value, "name").map(ToOwned::to_owned),
        server: text(value, "server")
            .map(ToOwned::to_owned)
            .or_else(|| servers.get(&path).cloned()),
        path,
    });
}

fn normalized_path(path: &str) -> String {
    path.trim().replace('\\', "/")
}

fn media_url(provider: ArchiveProvider, candidate: &MediaCandidate) -> Result<String, SourceError> {
    if let Ok(url) = Url::parse(&candidate.path) {
        validate_media_host(provider, &url)?;
        return Ok(url.to_string());
    }
    if !candidate.path.starts_with('/') || candidate.path.chars().any(char::is_control) {
        return Err(invalid_response(
            provider,
            "post contains an invalid media path",
        ));
    }
    let root = candidate.server.as_deref().unwrap_or(provider.media_root);
    let mut url = Url::parse(root)
        .map_err(|_| invalid_response(provider, "post contains an invalid media server"))?;
    validate_media_host(provider, &url)?;
    let path = if candidate.path.starts_with("/data/") {
        candidate.path.clone()
    } else {
        format!("/data{}", candidate.path)
    };
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn validate_media_host(provider: ArchiveProvider, url: &Url) -> Result<(), SourceError> {
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !(host == provider.domain || host.ends_with(&format!(".{}", provider.domain)))
    {
        return Err(invalid_response(
            provider,
            "post contains an unsafe media URL",
        ));
    }
    Ok(())
}

fn inline_paths(content: &str) -> Vec<String> {
    static INLINE: OnceLock<Regex> = OnceLock::new();
    INLINE
        .get_or_init(|| {
            Regex::new(
                r#"(?i)src\s*=\s*[\"'](?:https?://(?:[a-z0-9-]+\.)?pawchive\.(?:pw|st))?((?:/data)?/(?:inline/|[0-9a-f]{2}/[0-9a-f]{2}/)[^\"'?#\s]+)"#,
            )
            .expect("valid archive inline-media regex")
        })
        .captures_iter(content)
        .filter_map(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .collect()
}

fn content_hash(path: &str) -> Option<&str> {
    static HASH: OnceLock<Regex> = OnceLock::new();
    HASH.get_or_init(|| {
        Regex::new(r"(?i)/(?:[0-9a-f]{2})/(?:[0-9a-f]{2})/([0-9a-f]{64})(?:\.|$)")
            .expect("valid content hash regex")
    })
    .captures(path)
    .and_then(|captures| captures.get(1))
    .map(|value| value.as_str())
}

fn safe_file_name(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    name.rsplit('/')
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .map(ToOwned::to_owned)
}

fn file_name_from_path(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    safe_file_name(path)
}

fn required_component<'a>(
    provider: ArchiveProvider,
    value: &'a Value,
    key: &str,
    maximum: usize,
    subject: &str,
) -> Result<&'a str, SourceError> {
    let value = text(value, key)
        .filter(|value| valid_component(value, maximum))
        .ok_or_else(|| invalid_response(provider, format!("{subject} has an invalid {key}")))?;
    Ok(value)
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn invalid_query(provider: ArchiveProvider, message: impl Into<String>) -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidQuery,
        format!("{} {}", provider.display_name, message.into()),
        false,
    )
}

fn invalid_cursor(provider: ArchiveProvider) -> SourceError {
    invalid_query(provider, "source cursor is invalid")
}

fn invalid_response(provider: ArchiveProvider, message: impl Into<String>) -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidResponse,
        format!("{} {}", provider.display_name, message.into()),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const ARCHIVE_FIXTURE: ArchiveProvider = ArchiveProvider {
        id: "archive_fixture",
        display_name: "Archive fixture",
        domain: "pawchive.pw",
        query_domains: &["pawchive.pw", "www.pawchive.pw"],
        site_root: "https://pawchive.pw",
        api_root: "https://pawchive.pw/api/v1",
        media_root: "https://file.pawchive.pw",
        creator_posts_suffix: "",
        accept: "application/json",
        file_first: true,
    };

    fn request(cursor: Option<String>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "patreon:90822862".to_string(),
            partition: SourcePartition::new("posts"),
            cursor,
            page_size: 1,
        }
    }

    #[test]
    fn accepts_only_first_party_archive_creator_queries() {
        assert_eq!(
            normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap(),
            CreatorLocator {
                service: "patreon".to_string(),
                creator_id: "90822862".to_string(),
            }
        );
        assert!(normalize_creator_query(
            ARCHIVE_FIXTURE,
            "https://pawchive.pw/patreon/user/90822862/"
        )
        .is_ok());
        assert!(normalize_creator_query(
            ARCHIVE_FIXTURE,
            "https://example.invalid/patreon/user/90822862"
        )
        .is_err());
        assert!(normalize_creator_query(
            ARCHIVE_FIXTURE,
            "https://pawchive.pw/patreon/user/90822862/post/1"
        )
        .is_err());
    }

    #[test]
    fn exposes_one_post_at_a_time_with_a_query_bound_cursor() {
        let creator = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap();
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/page.json"))
                .unwrap();
        let first = normalize_page(
            ARCHIVE_FIXTURE,
            &request(None),
            &creator,
            CursorState {
                offset: 0,
                index: 0,
            },
            fixture.clone(),
        )
        .unwrap();
        assert_eq!(first.posts.len(), 1);
        assert_eq!(first.posts[0].stable_id, "147648418");
        let cursor = first.posts[0].resume_cursor_after.clone().unwrap();
        let state = decode_cursor(ARCHIVE_FIXTURE, &creator, Some(&cursor)).unwrap();
        let second = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(cursor)),
            &creator,
            state,
            fixture,
        )
        .unwrap();
        assert_eq!(second.posts.len(), 1);
        assert_eq!(second.posts[0].stable_id, "139274914");
        assert!(second.exhausted);
        let other = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:44096704").unwrap();
        assert!(decode_cursor(
            ARCHIVE_FIXTURE,
            &other,
            first.posts[0].resume_cursor_after.as_deref()
        )
        .is_err());
    }

    #[test]
    fn anchored_cursor_survives_first_page_inserts_and_deletes() {
        let creator = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap();
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/page.json"))
                .unwrap();
        let first = normalize_page(
            ARCHIVE_FIXTURE,
            &request(None),
            &creator,
            CursorState {
                offset: 0,
                index: 0,
            },
            fixture.clone(),
        )
        .unwrap();
        let cursor = first.posts[0].resume_cursor_after.clone().unwrap();
        assert!(cursor.starts_with("v1|"));
        let state = decode_cursor(ARCHIVE_FIXTURE, &creator, Some(&cursor)).unwrap();

        let mut inserted = fixture.clone();
        let posts = inserted.as_array_mut().unwrap();
        let mut new_post = posts[0].clone();
        new_post["id"] = Value::String("150000000".into());
        posts.insert(0, new_post);
        let resumed = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(cursor.clone())),
            &creator,
            state,
            inserted,
        )
        .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "139274914");

        let mut deleted = fixture.clone();
        deleted.as_array_mut().unwrap().remove(0);
        let resumed = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(cursor.clone())),
            &creator,
            state,
            deleted,
        )
        .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "139274914");

        let mut next_deleted = fixture;
        let posts = next_deleted.as_array_mut().unwrap();
        let mut older = posts[1].clone();
        older["id"] = Value::String("130000000".into());
        posts.push(older);
        posts.remove(1);
        let resumed = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(cursor)),
            &creator,
            state,
            next_deleted,
        )
        .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "130000000");
    }

    #[test]
    fn anchored_cursor_survives_mutation_at_the_first_page_boundary() {
        let creator = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap();
        let template: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/page.json"))
                .unwrap();
        let template = template.as_array().unwrap()[0].clone();
        let all = (0..52)
            .map(|index| {
                let mut post = template.clone();
                post["id"] = Value::String(format!("post-{index:03}"));
                post
            })
            .collect::<Vec<_>>();
        let first_page = Value::Array(all[..PAGE_LENGTH as usize].to_vec());

        let mut cursor = None;
        let mut state = CursorState {
            offset: 0,
            index: 0,
        };
        for index in 0..PAGE_LENGTH as usize {
            let batch = normalize_page(
                ARCHIVE_FIXTURE,
                &request(cursor.clone()),
                &creator,
                state,
                first_page.clone(),
            )
            .unwrap();
            assert_eq!(batch.posts[0].stable_id, format!("post-{index:03}"));
            assert!(batch.posts[0].media.is_empty());
            cursor = batch.posts[0].resume_cursor_after.clone();
            state = decode_cursor(ARCHIVE_FIXTURE, &creator, cursor.as_deref()).unwrap();
        }
        let boundary_cursor = cursor.unwrap();
        assert_eq!(state.offset, PAGE_LENGTH - 1);

        let unchanged = Value::Array(all[(PAGE_LENGTH - 1) as usize..].to_vec());
        let resumed = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(boundary_cursor.clone())),
            &creator,
            state,
            unchanged,
        )
        .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "post-050");

        let mut inserted = all.clone();
        let mut new_post = template.clone();
        new_post["id"] = Value::String("post-new".into());
        inserted.insert(0, new_post);
        let inserted_page = Value::Array(inserted[(PAGE_LENGTH - 1) as usize..].to_vec());
        let resumed = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(boundary_cursor.clone())),
            &creator,
            state,
            inserted_page,
        )
        .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "post-050");

        let mut deleted = all;
        deleted.remove(0);
        let deleted_page = Value::Array(deleted[(PAGE_LENGTH - 1) as usize..].to_vec());
        let resumed = normalize_page(
            ARCHIVE_FIXTURE,
            &request(Some(boundary_cursor)),
            &creator,
            state,
            deleted_page,
        )
        .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "post-050");
    }

    #[test]
    fn maps_complete_detail_media_and_only_canonical_namespaces() {
        let creator = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap();
        let page: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/page.json"))
                .unwrap();
        let discovered = normalize_page(
            ARCHIVE_FIXTURE,
            &request(None),
            &creator,
            CursorState {
                offset: 0,
                index: 0,
            },
            page,
        )
        .unwrap()
        .posts
        .remove(0);
        let detail: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/post.json"))
                .unwrap();
        let profile: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/archive_feed/profile.json"
        ))
        .unwrap();
        let post =
            normalize_detail(ARCHIVE_FIXTURE, discovered, &creator, detail, profile).unwrap();

        assert_eq!(post.creator.as_deref(), Some("Rehab Room"));
        assert_eq!(post.media.len(), 4);
        assert!(post.media.iter().any(|media| media.url.ends_with(".zip")));
        assert_eq!(post.media[0].position, 0);
        assert_eq!(post.media[3].position, 3);
        assert!(post.tags.contains(&CanonicalTag::new("creator", "Alice")));
        assert!(post.tags.contains(&CanonicalTag::new("character", "Hero")));
        assert!(post.tags.contains(&CanonicalTag::new("series", "Saga")));
        assert!(post.tags.contains(&CanonicalTag::new("species", "wolf")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "explicit")));
        assert!(post.tags.contains(&CanonicalTag::new("", "scan")));
        assert!(post.tags.iter().all(|tag| matches!(
            tag.namespace.as_str(),
            "" | "creator" | "character" | "series" | "species" | "rating"
        )));
    }

    #[test]
    fn missing_profile_uses_the_stable_creator_identity() {
        let creator = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap();
        let page: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/page.json"))
                .unwrap();
        let discovered = normalize_page(
            ARCHIVE_FIXTURE,
            &request(None),
            &creator,
            CursorState {
                offset: 0,
                index: 0,
            },
            page,
        )
        .unwrap()
        .posts
        .remove(0);
        let detail: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/post.json"))
                .unwrap();

        let post = normalize_detail(ARCHIVE_FIXTURE, discovered, &creator, detail, None).unwrap();

        assert_eq!(post.creator.as_deref(), Some("90822862"));
        assert!(!post.media.is_empty());
    }

    #[test]
    fn missing_detail_settles_only_that_post_without_media() {
        let creator = normalize_creator_query(ARCHIVE_FIXTURE, "patreon:90822862").unwrap();
        let page: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/archive_feed/page.json"))
                .unwrap();
        let mut discovered = normalize_page(
            ARCHIVE_FIXTURE,
            &request(None),
            &creator,
            CursorState {
                offset: 0,
                index: 0,
            },
            page,
        )
        .unwrap()
        .posts
        .remove(0);
        let resume_cursor = discovered.resume_cursor_after.clone();
        discovered.media.push(
            MediaDescriptorBuilder::new("fixture", 0, "https://pawchive.pw/data/fixture.jpg")
                .build(),
        );

        let unavailable = post_without_detail(discovered);

        assert_eq!(unavailable.stable_id, "147648418");
        assert_eq!(unavailable.resume_cursor_after, resume_cursor);
        assert!(unavailable.media.is_empty());
    }
}
