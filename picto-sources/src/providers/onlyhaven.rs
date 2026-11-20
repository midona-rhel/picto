//! OnlyHaven creator posts use the public API described by gallery-dl 1.32.20.
//! Downloads and run settlement remain owned by the shared source engine.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, page_cache::PageCache, AdapterFuture, CanonicalTagSet, DiscoveryBatch,
    DiscoveryRequest, HttpRuntime, MediaDescriptor, MediaDescriptorBuilder, NativeSourceAdapter,
    OpaqueCursor, ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const ROOT: &str = "https://cum.st";
const PAGE_SIZE: u32 = 50;

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    OnlyHaven::default()
}

#[derive(Default)]
struct OnlyHaven {
    pages: PageCache<PostPage>,
    creators: PageCache<Creator>,
}

#[derive(Clone, Deserialize)]
struct Creator {
    id: String,
    service: String,
    name: String,
    #[serde(default, rename = "communityTags")]
    tags: Vec<CommunityTag>,
}

#[derive(Clone, Deserialize)]
struct CommunityTag {
    label: String,
}

#[derive(Deserialize)]
struct CreatorSearch {
    total: u64,
    creators: Vec<Creator>,
}

#[derive(Clone, Deserialize)]
struct PostPage {
    total: u64,
    posts: Vec<Post>,
}

#[derive(Clone, Deserialize)]
struct Post {
    id: String,
    service: String,
    #[serde(default, rename = "creatorId")]
    creator_id: Option<String>,
    #[serde(default, rename = "captionHtml")]
    caption_html: Option<String>,
    #[serde(default, alias = "captionText")]
    caption: Option<String>,
    #[serde(default)]
    published: Option<i64>,
    #[serde(default)]
    added: Option<i64>,
    attachments: Vec<Attachment>,
}

#[derive(Clone, Deserialize)]
struct Attachment {
    id: String,
    #[serde(default, rename = "storageKey")]
    storage_key: Option<String>,
    #[serde(default, rename = "originalFilename")]
    original_filename: Option<String>,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    variants: Vec<Variant>,
}

#[derive(Clone, Deserialize)]
struct Variant {
    name: String,
    #[serde(default)]
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    query: String,
    creator_id: String,
    offset: u32,
    after: String,
    next: Option<String>,
}

fn error(kind: SourceErrorKind, message: &str) -> SourceError {
    SourceError::new(kind, format!("OnlyHaven: {message}"), false)
}

fn invalid(message: &str) -> SourceError {
    error(SourceErrorKind::InvalidResponse, message)
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// Accept a creator name, service:creator, or canonical creator URL.
/// A name without a service refers to an OnlyFans creator.
pub fn normalize_query(raw: &str) -> Result<String, SourceError> {
    let raw = raw.trim();
    let (service, creator) = if raw.contains("://") {
        let url = Url::parse(raw)
            .map_err(|_| error(SourceErrorKind::InvalidQuery, "Invalid creator URL."))?;
        let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if !matches!(url.scheme(), "https" | "http")
            || !matches!(url.host_str(), Some("cum.st" | "www.cum.st"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || parts.len() != 3
            || parts[0] != "creators"
        {
            return Err(error(
                SourceErrorKind::InvalidQuery,
                "Use a creator name or a cum.st/creators/service/creator URL, not a post URL.",
            ));
        }
        (parts[1].to_owned(), parts[2].to_owned())
    } else {
        let (service, creator) = raw.split_once(':').unwrap_or(("onlyfans", raw));
        (
            service.to_owned(),
            creator.trim_start_matches('@').to_owned(),
        )
    };
    if !component(&service) || !component(&creator) {
        return Err(error(
            SourceErrorKind::InvalidQuery,
            "The creator name or service contains unsupported characters.",
        ));
    }
    Ok(format!("{}:{creator}", service.to_ascii_lowercase()))
}

pub fn creator_url(raw: &str) -> Result<String, SourceError> {
    let query = normalize_query(raw)?;
    let (service, creator) = query.split_once(':').unwrap();
    Ok(format!("{ROOT}/creators/{service}/{creator}"))
}

fn api_url(service: &str, creator: &str, endpoint: &str) -> Url {
    Url::parse(&format!(
        "{ROOT}/api/v1/{service}/user/{creator}/{endpoint}"
    ))
    .expect("Validated OnlyHaven API path")
}

impl NativeSourceAdapter for OnlyHaven {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "onlyhaven",
            display_name: "OnlyHaven",
            domain: "cum.st",
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_query(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            if request.partition.0 != "posts" {
                return Err(error(
                    SourceErrorKind::InvalidQuery,
                    "Only creator posts are supported.",
                ));
            }
            let query = normalize_query(&request.query)?;
            let (service, input) = query.split_once(':').unwrap();
            let cursor = decode_cursor(request.cursor.as_deref(), &query)?;
            let mut credentials = credentials.clone();
            credentials.allowed_domains.insert("cum.st".into());
            credentials
                .headers
                .insert("Accept".into(), "application/json".into());
            credentials
                .headers
                .insert("Accept-Encoding".into(), "identity".into());
            let creator = self
                .creators
                .get_or_fetch(&query, &credentials, cursor.is_some(), async {
                    let id = if let Some(cursor) = &cursor {
                        cursor.creator_id.clone()
                    } else if input.bytes().all(|b| b.is_ascii_digit()) {
                        input.to_owned()
                    } else {
                        let mut url = Url::parse(&format!("{ROOT}/api/v1/creators")).unwrap();
                        url.query_pairs_mut().append_pair("q", input);
                        let search: CreatorSearch =
                            http.get_json(url, &credentials, cancel).await?;
                        resolve_name(&search, service, input)?.id.clone()
                    };
                    let creator: Creator = http
                        .get_json(api_url(service, &id, "profile"), &credentials, cancel)
                        .await?;
                    if creator.id != id || creator.service != service || !component(&creator.id) {
                        return Err(invalid("The profile belongs to a different creator."));
                    }
                    Ok(creator)
                })
                .await?;
            let mut offset = cursor.as_ref().map_or(0, |c| c.offset);
            let mut seek = cursor.as_ref();
            // A saved post ID locates the next item even when new posts shift a page.
            // If both anchors disappear, replay from the start rather than lose posts.
            loop {
                let mut url = api_url(service, &creator.id, "posts");
                url.query_pairs_mut()
                    .append_pair("o", &offset.to_string())
                    .append_pair("n", "50");
                let page = self
                    .pages
                    .get_or_fetch(
                        url.as_str(),
                        &credentials,
                        cursor.is_some(),
                        http.get_json::<PostPage>(url.clone(), &credentials, cancel),
                    )
                    .await?;
                validate_page(&page, service, &creator.id)?;
                if page.posts.len() < PAGE_SIZE as usize
                    && u64::from(offset) + (page.posts.len() as u64) < page.total
                {
                    return Err(invalid("The API returned an incomplete page before the end of the creator listing."));
                }
                let index = match seek {
                    Some(saved) => saved
                        .next
                        .as_ref()
                        .and_then(|id| page.posts.iter().position(|p| &p.id == id))
                        .or_else(|| {
                            page.posts
                                .iter()
                                .position(|p| p.id == saved.after)
                                .map(|i| i + 1)
                        }),
                    None => Some(0),
                };
                if let Some(index) = index {
                    if index < page.posts.len() {
                        return make_batch(request, &query, &creator, offset, index, &page);
                    }
                    if page.posts.is_empty()
                        || u64::from(offset) + page.posts.len() as u64 >= page.total
                    {
                        return Ok(DiscoveryBatch {
                            posts: vec![],
                            exhausted: true,
                        });
                    }
                    offset = offset
                        .checked_add(PAGE_SIZE)
                        .ok_or_else(|| invalid("The page offset is too large."))?;
                    seek = None;
                } else if offset > 0
                    && cursor
                        .as_ref()
                        .is_some_and(|c| c.offset - offset < PAGE_SIZE * 4)
                {
                    offset = offset.saturating_sub(PAGE_SIZE);
                } else {
                    offset = 0;
                    seek = None;
                }
            }
        })
    }
}

fn resolve_name<'a>(
    search: &'a CreatorSearch,
    service: &str,
    name: &str,
) -> Result<&'a Creator, SourceError> {
    // Never choose a fuzzy result or an incomplete result set on the user's behalf.
    if search.total != search.creators.len() as u64 {
        return Err(error(
            SourceErrorKind::InvalidQuery,
            "The creator search is incomplete. Use service:creator ID or the creator URL.",
        ));
    }
    let mut matches = search
        .creators
        .iter()
        .filter(|c| c.service == service && c.name.eq_ignore_ascii_case(name));
    let result = matches.next().ok_or_else(|| {
        error(
            SourceErrorKind::InvalidQuery,
            "No creator with that exact name was found. Use service:creator ID or the creator URL.",
        )
    })?;
    if matches.next().is_some() || !component(&result.id) {
        return Err(error(
            SourceErrorKind::InvalidQuery,
            "That creator name is ambiguous. Use the creator URL.",
        ));
    }
    Ok(result)
}

fn decode_cursor(raw: Option<&str>, query: &str) -> Result<Option<Cursor>, SourceError> {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let cursor: Cursor =
        serde_json::from_str(OpaqueCursor::new(2048).validate(raw)?).map_err(|_| {
            error(
                SourceErrorKind::InvalidQuery,
                "The saved cursor is invalid.",
            )
        })?;
    if cursor.query != query
        || !component(&cursor.creator_id)
        || !component(&cursor.after)
        || cursor.next.as_ref().is_some_and(|id| !component(id))
        || !cursor.offset.is_multiple_of(PAGE_SIZE)
    {
        return Err(error(
            SourceErrorKind::InvalidQuery,
            "The saved cursor does not belong to this query.",
        ));
    }
    Ok(Some(cursor))
}

fn validate_page(page: &PostPage, service: &str, creator: &str) -> Result<(), SourceError> {
    if page.total < page.posts.len() as u64 {
        return Err(invalid(
            "The page contains more posts than the reported total.",
        ));
    }
    if page.posts.len() > PAGE_SIZE as usize {
        return Err(invalid("The API returned more than 50 posts in one page."));
    }
    let mut ids = BTreeSet::new();
    for post in &page.posts {
        if !component(&post.id)
            || !ids.insert(&post.id)
            || post.service != service
            || post.creator_id.as_deref().is_some_and(|id| id != creator)
        {
            return Err(invalid(
                "The API returned a duplicate post or a post belonging to another creator.",
            ));
        }
    }
    Ok(())
}

fn make_batch(
    request: &DiscoveryRequest,
    query: &str,
    creator: &Creator,
    offset: u32,
    index: usize,
    page: &PostPage,
) -> Result<DiscoveryBatch, SourceError> {
    let post = &page.posts[index];
    let exhausted = u64::from(offset) + index as u64 + 1 >= page.total;
    let cursor = Cursor {
        query: query.into(),
        creator_id: creator.id.clone(),
        offset,
        after: post.id.clone(),
        next: page.posts.get(index + 1).map(|p| p.id.clone()),
    };
    let canonical = format!(
        "{ROOT}/creators/{}/{}/post/{}",
        creator.service, creator.id, post.id
    );
    let notes = post
        .caption_html
        .as_deref()
        .and_then(normalize_source_text)
        .or_else(|| post.caption.as_deref().and_then(normalize_source_text));
    let name = notes
        .as_deref()
        .and_then(|s| s.lines().next())
        .map(str::to_owned);
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &creator.name);
    for tag in &creator.tags {
        tags.insert("", &tag.label);
    }
    let created_at = post
        .published
        .filter(|t| *t != 0)
        .or(post.added)
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|date| date.to_rfc3339());
    Ok(DiscoveryBatch {
        posts: vec![SourcePost {
            site_id: "onlyhaven".into(),
            partition: request.partition.clone(),
            stable_id: post.id.clone(),
            canonical_url: Some(canonical.clone()),
            creator: Some(creator.name.clone()),
            name,
            notes,
            created_at,
            tags: tags.into_vec(),
            media: media(post, &canonical)?,
            resume_cursor_after: if exhausted {
                None
            } else {
                Some(
                    serde_json::to_string(&cursor)
                        .map_err(|_| invalid("The cursor could not be saved."))?,
                )
            },
        }],
        exhausted,
    })
}

fn media(post: &Post, canonical: &str) -> Result<Vec<MediaDescriptor>, SourceError> {
    let mut ids = BTreeSet::new();
    post.attachments
        .iter()
        .enumerate()
        .map(|(position, file)| {
            if !component(&file.id) || !ids.insert(&file.id) {
                return Err(invalid("An attachment has an invalid or repeated ID."));
            }
            let id = format!("{}:{}", post.id, file.id);
            let Some(variant) = file.variants.iter().max_by_key(|v| v.bytes) else {
                if file.locked {
                    return Ok(MediaDescriptorBuilder::new(id, position as u32, "")
                        .canonical_url(canonical)
                        .unavailable("OnlyHaven marks this attachment as locked.")
                        .build());
                }
                return Err(invalid("An attachment has no downloadable variants."));
            };
            let storage = file
                .storage_key
                .as_deref()
                .filter(|s| component(s))
                .ok_or_else(|| invalid("An attachment has no valid storage key."))?;
            if !component(&variant.name) {
                return Err(invalid("An attachment has an invalid variant name."));
            }
            let url = format!("https://cdn.cum.st/media/{storage}/{}", variant.name);
            let mut name = file
                .original_filename
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&variant.name)
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&variant.name)
                .to_owned();
            if std::path::Path::new(&name).extension().is_none() {
                if let Some(extension) = std::path::Path::new(&variant.name)
                    .extension()
                    .and_then(|s| s.to_str())
                {
                    name.push('.');
                    name.push_str(extension);
                }
            }
            Ok(MediaDescriptorBuilder::new(id, position as u32, url)
                .canonical_url(canonical)
                .file_name(name)
                .expected_size((variant.bytes > 0).then_some(variant.bytes))
                .headers(BTreeMap::from([(
                    "Accept-Encoding".into(),
                    "identity".into(),
                )]))
                .build())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};
    use serde_json::json;

    fn creator() -> Creator {
        serde_json::from_value(json!({"id":"42", "service":"onlyfans", "name":"example",
            "communityTags":[{"label":"color art"}]}))
        .unwrap()
    }

    fn post(id: usize) -> Post {
        serde_json::from_value(json!({"id":id.to_string(), "service":"onlyfans",
            "captionHtml":"<p>A drawing</p>", "published":1700000000,
            "attachments":[{"id":format!("file-{id}"), "storageKey":"abc123",
                "originalFilename":"drawing", "variants":[
                    {"name":"original.png", "bytes":4000},
                    {"name":"small.png", "bytes":100}]}]}))
        .unwrap()
    }

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "example".into(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 50,
        }
    }

    #[test]
    fn accepts_creator_names_and_urls_but_not_post_urls_or_other_hosts() {
        for input in [
            "example",
            "@example",
            "onlyfans:example",
            "https://cum.st/creators/onlyfans/example/",
        ] {
            assert_eq!(normalize_query(input).unwrap(), "onlyfans:example");
        }
        assert_eq!(normalize_query("patreon:42").unwrap(), "patreon:42");
        for input in [
            "",
            "..",
            "a/b",
            "a:b:c",
            "https://cum.st.evil.test/creators/onlyfans/42",
            "https://cum.st/creators/onlyfans/42/post/5",
            "https://cum.st/creators/onlyfans/42?o=50",
            "https://user@cum.st/creators/onlyfans/42",
            "https://cum.st/creators/onlyfans/%2e%2e",
        ] {
            assert!(normalize_query(input).is_err(), "{input}");
        }
    }

    #[test]
    fn creator_search_requires_one_exact_match_in_the_requested_service() {
        let mut search = CreatorSearch {
            total: 1,
            creators: vec![creator()],
        };
        assert_eq!(
            resolve_name(&search, "onlyfans", "EXAMPLE").unwrap().id,
            "42"
        );
        assert!(resolve_name(&search, "patreon", "example").is_err());
        assert!(resolve_name(&search, "onlyfans", "exam").is_err());
        search.total = 2;
        assert!(resolve_name(&search, "onlyfans", "example").is_err());
        search.creators.push(creator());
        assert!(resolve_name(&search, "onlyfans", "example").is_err());
    }

    #[test]
    fn emits_one_post_with_original_media_and_canonical_metadata() {
        let page = PostPage {
            total: 2,
            posts: vec![post(10), post(11)],
        };
        let batch = make_batch(&request(), "onlyfans:example", &creator(), 0, 0, &page).unwrap();
        assert_eq!(batch.posts.len(), 1);
        assert!(!batch.exhausted);
        let item = &batch.posts[0];
        assert_eq!(item.name.as_deref(), Some("A drawing"));
        assert_eq!(
            item.created_at.as_deref(),
            Some("2023-11-14T22:13:20+00:00")
        );
        assert!(item.tags.contains(&CanonicalTag::new("creator", "example")));
        assert!(item.tags.contains(&CanonicalTag::new("", "color art")));
        assert_eq!(
            item.media[0].url,
            "https://cdn.cum.st/media/abc123/original.png"
        );
        assert_eq!(item.media[0].file_name.as_deref(), Some("drawing.png"));
        assert_eq!(item.media[0].expected_size, Some(4000));
        assert_eq!(item.media[0].headers["Accept-Encoding"], "identity");
        let cursor = decode_cursor(item.resume_cursor_after.as_deref(), "onlyfans:example")
            .unwrap()
            .unwrap();
        assert_eq!(cursor.next.as_deref(), Some("11"));
        assert!(decode_cursor(item.resume_cursor_after.as_deref(), "onlyfans:other").is_err());
        let final_post =
            make_batch(&request(), "onlyfans:example", &creator(), 0, 1, &page).unwrap();
        assert!(final_post.exhausted);
        assert!(final_post.posts[0].resume_cursor_after.is_none());
    }

    #[test]
    fn page_boundary_retains_the_last_post_as_a_resume_anchor() {
        let page = PostPage {
            total: 51,
            posts: (0..50).map(post).collect(),
        };
        let last = make_batch(&request(), "onlyfans:example", &creator(), 0, 49, &page).unwrap();
        let cursor = decode_cursor(
            last.posts[0].resume_cursor_after.as_deref(),
            "onlyfans:example",
        )
        .unwrap()
        .unwrap();
        assert_eq!(cursor.offset, 0);
        assert_eq!(cursor.after, "49");
        assert!(cursor.next.is_none());
        assert!(!last.exhausted);
    }

    #[test]
    fn rejects_wrong_owners_duplicate_posts_and_unsafe_media_paths() {
        let mut page = PostPage {
            total: 1,
            posts: vec![post(1)],
        };
        assert!(validate_page(&page, "onlyfans", "42").is_ok());
        page.posts[0].creator_id = Some("43".into());
        assert!(validate_page(&page, "onlyfans", "42").is_err());
        page.posts[0].creator_id = None;
        page.posts.push(post(1));
        assert!(validate_page(&page, "onlyfans", "42").is_err());
        let mut item = post(1);
        item.attachments[0].storage_key = Some("../secret".into());
        assert!(media(&item, "https://cum.st").is_err());
        item.attachments[0].storage_key = Some("abc".into());
        item.attachments[0].variants[0].name = "https://other.test/file".into();
        assert!(media(&item, "https://cum.st").is_err());
    }

    #[test]
    fn locked_files_are_reported_without_using_the_preview() {
        let mut item = post(1);
        item.attachments[0].variants.clear();
        assert!(media(&item, "https://cum.st").is_err());
        item.attachments[0].locked = true;
        let files = media(&item, "https://cum.st").unwrap();
        assert!(files[0].unavailable_reason.is_some());
        assert!(files[0].url.is_empty());
        item.caption_html = None;
        item.caption = None;
        item.attachments.clear();
        let page = PostPage {
            total: 1,
            posts: vec![item],
        };
        let batch = make_batch(&request(), "onlyfans:example", &creator(), 0, 0, &page).unwrap();
        assert!(batch.posts[0].media.is_empty());
        assert!(batch.posts[0].name.is_none());
    }

    #[tokio::test]
    #[ignore = "Uses the public API and downloads one image. Set PICTO_ONLYHAVEN_SMOKE_QUERY to opt in."]
    async fn live_creator_pagination_resume_and_download() {
        let query =
            std::env::var("PICTO_ONLYHAVEN_SMOKE_QUERY").expect("Set a public creator query");
        let http = HttpRuntime::new(crate::HttpPolicy::default()).unwrap();
        let credentials = RequestCredentials::default();
        let cancel = CancellationToken::new();
        let mut source = OnlyHaven::default();
        let mut request = DiscoveryRequest { query, ..request() };
        let mut seen = BTreeSet::new();
        let mut image = None;
        for index in 0..55 {
            if index == 20 {
                source = OnlyHaven::default();
            }
            let batch = source
                .discover(&request, &credentials, &http, &cancel)
                .await
                .unwrap();
            assert_eq!(batch.posts.len(), 1);
            let post = &batch.posts[0];
            assert!(
                seen.insert(post.stable_id.clone()),
                "A resumed post was repeated"
            );
            if image.is_none() {
                image = post
                    .media
                    .iter()
                    .find(|m| {
                        m.mime_hint
                            .as_deref()
                            .is_some_and(|mime| mime.starts_with("image/"))
                            && m.expected_size.is_some_and(|size| size < 5_000_000)
                            && m.unavailable_reason.is_none()
                    })
                    .cloned();
            }
            request.cursor = post.resume_cursor_after.clone();
            if batch.exhausted {
                break;
            }
        }
        assert!(
            seen.len() > 50,
            "Choose a creator with more than one page of posts"
        );
        let image = image.expect("Choose a creator with at least one small image");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download");
        http.download(&image, &credentials, &path, &cancel)
            .await
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, image.expected_size.unwrap());
        image::load_from_memory(&bytes).unwrap();
        println!("Verified {} distinct posts, a fresh-adapter resume, a page boundary, and a {}-byte image. Temporary download removed on exit.",seen.len(),bytes.len());
    }
}
