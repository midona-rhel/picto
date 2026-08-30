use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    AdapterFuture, CanonicalTag, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest, HttpRuntime,
    MediaDescriptorBuilder, NativeSourceAdapter, PageCursor, PostFuture, ProviderDescriptor,
    RatingMap, RequestCredentials, SearchQueryPolicy, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: PageCursor = PageCursor::new(1_000_000);
const EMPTY_PAGE_ATTEMPTS: usize = 2;
const QUERY: SearchQueryPolicy =
    SearchQueryPolicy::new("Safebooru", &["id:", "limit:", "order:", "page:", "pid:"]);
const RATINGS: RatingMap = RatingMap::new(&[
    ("s", "safe"),
    ("safe", "safe"),
    ("g", "general"),
    ("general", "general"),
    ("q", "questionable"),
    ("questionable", "questionable"),
    ("e", "explicit"),
    ("explicit", "explicit"),
]);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    SafebooruSource
}

struct SafebooruSource;

impl NativeSourceAdapter for SafebooruSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "safebooru",
            display_name: "Safebooru",
            domain: "safebooru.org",
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
            for attempt in 0..EMPTY_PAGE_ATTEMPTS {
                let posts = match http
                    .get_json::<Vec<ApiPost>>(request_url(request)?, credentials, cancel)
                    .await
                {
                    Ok(posts) => posts,
                    Err(error)
                        if error.kind == SourceErrorKind::InvalidResponse
                            && attempt + 1 < EMPTY_PAGE_ATTEMPTS =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let batch = normalize(request, posts)?;
                if !batch.posts.is_empty() || attempt + 1 == EMPTY_PAGE_ATTEMPTS {
                    return Ok(batch);
                }
            }
            unreachable!("bounded Safebooru retry always returns")
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
            let canonical_url = post.canonical_url.as_deref().ok_or_else(|| {
                SourceError::new(
                    SourceErrorKind::InvalidResponse,
                    "Safebooru post is missing its canonical URL",
                    false,
                )
            })?;
            let url = Url::parse(canonical_url).map_err(|error| {
                SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
            })?;
            let html = http.get_text(url, credentials, cancel).await?;
            let mut tags = parse_canonical_tags(&html)?;
            let rating = post
                .tags
                .iter()
                .find(|tag| tag.namespace == "rating")
                .cloned();
            if let Some(rating) = rating {
                tags.push(rating);
                tags.sort_by(|left, right| {
                    (&left.namespace, &left.value).cmp(&(&right.namespace, &right.value))
                });
                tags.dedup();
            }
            post.creator = tags
                .iter()
                .find(|tag| tag.namespace == "creator")
                .map(|tag| tag.value.clone());
            post.tags = tags;
            Ok(post)
        })
    }
}

fn request_url(request: &DiscoveryRequest) -> Result<Url, SourceError> {
    let page = current_page(request)?;
    let mut url = Url::parse("https://safebooru.org/index.php").expect("static Safebooru API URL");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("page", "dapi");
        query.append_pair("s", "post");
        query.append_pair("q", "index");
        query.append_pair("json", "1");
        query.append_pair("tags", request.query.trim());
        // One source post per request keeps detail resolution and settlement serial.
        query.append_pair("limit", "1");
        query.append_pair("pid", &page.to_string());
    }
    Ok(url)
}

fn normalize(
    request: &DiscoveryRequest,
    response: Vec<ApiPost>,
) -> Result<DiscoveryBatch, SourceError> {
    let page = current_page(request)?;
    if response.len() > 1 {
        return Err(SourceError::new(
            SourceErrorKind::InvalidResponse,
            "Safebooru returned more than one post for a one-post request",
            false,
        ));
    }
    let exhausted = response.is_empty();
    let next_cursor = CURSOR.encode(page.saturating_add(1))?;
    let posts = response
        .into_iter()
        .map(|post| normalize_post(request, post, next_cursor.clone()))
        .collect();
    Ok(DiscoveryBatch { posts, exhausted })
}

fn normalize_post(
    request: &DiscoveryRequest,
    post: ApiPost,
    resume_cursor_after: String,
) -> SourcePost {
    let post_id = post.id.to_string();
    let canonical_url = format!("https://safebooru.org/index.php?page=post&s=view&id={post_id}");
    let mut tags = CanonicalTagSet::default();
    RATINGS.add(&mut tags, post.rating.as_deref());

    let media = post
        .file_url
        .as_deref()
        .and_then(normalize_media_url)
        .map(|url| {
            let file_name = post
                .image
                .filter(|value| !value.trim().is_empty())
                .or_else(|| file_name_from_url(&url))
                .unwrap_or_else(|| format!("safebooru_{post_id}.media"));
            MediaDescriptorBuilder::new(format!("safebooru:{post_id}:0"), 0, url)
                .canonical_url(&canonical_url)
                .file_name(file_name)
                .build()
        })
        .into_iter()
        .collect();

    SourcePost {
        site_id: "safebooru".to_string(),
        partition: request.partition.clone(),
        stable_id: post_id.clone(),
        canonical_url: Some(canonical_url),
        creator: None,
        name: Some(format!("safebooru_{post_id}")),
        notes: None,
        created_at: post.change.map(|value| value.to_string()),
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(resume_cursor_after),
    }
}

fn normalize_media_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Url::parse("https://safebooru.org")
        .expect("static Safebooru root")
        .join(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.to_string())
}

fn parse_canonical_tags(html: &str) -> Result<Vec<CanonicalTag>, SourceError> {
    let Some(sidebar) = html
        .split_once("<ul id=\"tag-sidebar\">")
        .map(|(_, rest)| rest)
    else {
        return Err(invalid_tag_response());
    };
    let Some(sidebar) = sidebar.split_once("</ul>").map(|(sidebar, _)| sidebar) else {
        return Err(invalid_tag_response());
    };

    let mut tags = CanonicalTagSet::default();
    for item in sidebar.split("<li class=\"tag-type-").skip(1) {
        let Some((category, body)) = item.split_once(' ') else {
            continue;
        };
        let Some(body) = body.split_once("</li>").map(|(body, _)| body) else {
            continue;
        };
        let Some(encoded) = body
            .split("tags=")
            .nth(1)
            .and_then(|value| value.split(['\"', '&']).next())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let value = decode_query_value(encoded)?;
        tags.insert(canonical_namespace(category), value);
    }
    let tags = tags.into_vec();
    if tags.is_empty() {
        return Err(invalid_tag_response());
    }
    Ok(tags)
}

fn canonical_namespace(category: &str) -> &'static str {
    match category {
        "artist" => "creator",
        "character" => "character",
        "copyright" => "series",
        "species" => "species",
        _ => "",
    }
}

fn decode_query_value(value: &str) -> Result<String, SourceError> {
    let url = Url::parse(&format!("https://safebooru.org/?tag={value}"))
        .map_err(|_| invalid_tag_response())?;
    url.query_pairs()
        .find_map(|(key, value)| (key == "tag").then(|| value.into_owned()))
        .ok_or_else(invalid_tag_response)
}

fn invalid_tag_response() -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidResponse,
        "Safebooru post did not contain a valid tag sidebar",
        true,
    )
}

fn current_page(request: &DiscoveryRequest) -> Result<u32, SourceError> {
    request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| CURSOR.validate(cursor))
        .unwrap_or(Ok(0))
}

fn file_name_from_url(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Deserialize)]
struct ApiPost {
    id: u64,
    #[serde(default)]
    file_url: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    change: Option<i64>,
    #[serde(default)]
    rating: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::SourcePartition;

    fn request(cursor: Option<&str>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "1girl solo".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 50,
        }
    }

    #[test]
    fn builds_one_post_bounded_requests() {
        let url = request_url(&request(Some("7"))).unwrap();
        let pairs = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(pairs.get("page").map(|value| value.as_ref()), Some("dapi"));
        assert_eq!(pairs.get("s").map(|value| value.as_ref()), Some("post"));
        assert_eq!(pairs.get("q").map(|value| value.as_ref()), Some("index"));
        assert_eq!(pairs.get("json").map(|value| value.as_ref()), Some("1"));
        assert_eq!(
            pairs.get("tags").map(|value| value.as_ref()),
            Some("1girl solo")
        );
        assert_eq!(pairs.get("limit").map(|value| value.as_ref()), Some("1"));
        assert_eq!(pairs.get("pid").map(|value| value.as_ref()), Some("7"));
    }

    #[test]
    fn maps_api_post_and_advances_cursor_after_that_post() {
        let response: Vec<ApiPost> = serde_json::from_value(json!([{
            "id": 7099551,
            "file_url": "https://safebooru.org/images/1101/hash.jpg",
            "image": "hash.jpg",
            "change": 1788048061,
            "rating": "general"
        }]))
        .unwrap();

        let batch = normalize(&request(Some("7")), response).unwrap();
        assert!(!batch.exhausted);
        assert_eq!(batch.posts.len(), 1);
        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "7099551");
        assert_eq!(post.resume_cursor_after.as_deref(), Some("8"));
        assert_eq!(post.media[0].file_name.as_deref(), Some("hash.jpg"));
        assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/jpeg"));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "general")));
    }

    #[test]
    fn normalizes_relative_original_media_urls() {
        assert_eq!(
            normalize_media_url("/images/1101/hash.jpg").as_deref(),
            Some("https://safebooru.org/images/1101/hash.jpg")
        );
    }

    #[test]
    fn maps_sidebar_categories_to_canonical_namespaces() {
        let html = r#"
            <ul id="tag-sidebar">
              <li class="tag-type-copyright tag"><a href="index.php?page=post&amp;s=list&amp;tags=touhou">touhou</a></li>
              <li class="tag-type-character tag"><a href="index.php?page=post&amp;s=list&amp;tags=maribel_hearn">maribel hearn</a></li>
              <li class="tag-type-artist tag"><a href="index.php?page=post&amp;s=list&amp;tags=kashiwada_kiiho">artist</a></li>
              <li class="tag-type-species tag"><a href="index.php?page=post&amp;s=list&amp;tags=wolf">wolf</a></li>
              <li class="tag-type-general tag"><a href="index.php?page=post&amp;s=list&amp;tags=black_ribbon">black ribbon</a></li>
              <li class="tag-type-metadata tag"><a href="index.php?page=post&amp;s=list&amp;tags=highres">highres</a></li>
            </ul>
        "#;
        let tags = parse_canonical_tags(html).unwrap();
        assert!(tags.contains(&CanonicalTag::new("series", "touhou")));
        assert!(tags.contains(&CanonicalTag::new("character", "maribel_hearn")));
        assert!(tags.contains(&CanonicalTag::new("creator", "kashiwada_kiiho")));
        assert!(tags.contains(&CanonicalTag::new("species", "wolf")));
        assert!(tags.contains(&CanonicalTag::new("", "black_ribbon")));
        assert!(tags.contains(&CanonicalTag::new("", "highres")));
    }

    #[test]
    fn rejects_query_owned_traversal_and_invalid_cursors() {
        for query in ["solo id:42", "solo limit:10", "solo page:2", "solo pid:2"] {
            assert!(QUERY.validate(query).is_err());
        }
        assert!(QUERY.validate("1girl solo").is_ok());
        assert!(request_url(&request(Some("-1"))).is_err());
        assert!(request_url(&request(Some("1000001"))).is_err());
    }

    #[test]
    fn empty_page_is_exhausted_without_advancing_a_post() {
        let batch = normalize(&request(Some("4")), Vec::new()).unwrap();
        assert!(batch.exhausted);
        assert!(batch.posts.is_empty());
    }

    #[test]
    fn empty_page_retry_is_bounded() {
        assert_eq!(EMPTY_PAGE_ATTEMPTS, 2);
    }
}
