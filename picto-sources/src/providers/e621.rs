use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use crate::{
    normalize_source_text, BeforeIdCursor, CanonicalTag, DiscoveryBatch, DiscoveryRequest,
    JsonPageSource, JsonSourceAdapter, MediaDescriptor, MediaDescriptorBuilder, NamespaceMap,
    NativeSourceAdapter, ProviderDescriptor, RatingMap, SearchQueryPolicy, SourceError, SourcePost,
};

const CURSOR: BeforeIdCursor = BeforeIdCursor::new("b");
const QUERY: SearchQueryPolicy = SearchQueryPolicy::new("e621", &["page:", "limit:", "order:"]);
const NAMESPACES: NamespaceMap = NamespaceMap::new(&[
    ("artist", "creator"),
    ("character", "character"),
    ("copyright", "series"),
    ("species", "species"),
]);
const RATINGS: RatingMap = RatingMap::new(&[
    ("s", "safe"),
    ("safe", "safe"),
    ("q", "questionable"),
    ("questionable", "questionable"),
    ("e", "explicit"),
    ("explicit", "explicit"),
]);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    JsonSourceAdapter::new(E621Source)
}

struct E621Source;

impl JsonPageSource for E621Source {
    type Response = PostsResponse;

    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "e621",
            display_name: "e621",
            domain: "e621.net",
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        QUERY.validate(query)
    }

    fn request_url(&self, request: &DiscoveryRequest) -> Result<Url, SourceError> {
        request_url(request)
    }

    fn normalize(
        &self,
        request: &DiscoveryRequest,
        response: Self::Response,
    ) -> Result<DiscoveryBatch, SourceError> {
        Ok(parse_posts(request, response))
    }
}

fn request_url(request: &DiscoveryRequest) -> Result<Url, SourceError> {
    let mut url = Url::parse("https://e621.net/posts.json").expect("static e621 URL");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("tags", request.query.trim());
        query.append_pair("limit", &request.page_size.clamp(1, 320).to_string());
        if let Some(cursor) = request
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
        {
            CURSOR.validate(cursor)?;
            query.append_pair("page", cursor);
        }
    }
    Ok(url)
}

fn parse_posts(request: &DiscoveryRequest, response: PostsResponse) -> DiscoveryBatch {
    let exhausted = response.posts.len() < request.page_size as usize;
    let posts = response
        .posts
        .into_iter()
        .map(|post| normalize_post(request, post))
        .collect();
    DiscoveryBatch { posts, exhausted }
}

fn normalize_post(request: &DiscoveryRequest, post: ApiPost) -> SourcePost {
    let post_id = post.id.to_string();
    let canonical_url = format!("https://e621.net/posts/{post_id}");
    let creator = post
        .tags
        .get("artist")
        .and_then(|values| values.first())
        .cloned();
    let tags = canonical_tags(&post.tags, post.rating.as_deref());
    let media = media_descriptor(&post, &canonical_url)
        .into_iter()
        .collect();
    SourcePost {
        site_id: "e621".to_string(),
        partition: request.partition.clone(),
        stable_id: post_id.clone(),
        canonical_url: Some(canonical_url),
        creator,
        name: Some(format!("e621_{post_id}")),
        notes: post.description.as_deref().and_then(normalize_source_text),
        created_at: post.created_at,
        tags,
        media,
        resume_cursor_after: Some(CURSOR.encode(&post_id)),
    }
}

fn media_descriptor(post: &ApiPost, canonical_url: &str) -> Option<MediaDescriptor> {
    let extension = post.file.ext.as_deref()?.trim();
    let hash = post.file.md5.as_deref()?.trim();
    if extension.is_empty() || hash.len() < 4 {
        return None;
    }
    let url = post
        .file
        .url
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            format!(
                "https://static1.e621.net/data/{}/{}/{}.{}",
                &hash[0..2],
                &hash[2..4],
                hash,
                extension,
            )
        });
    Some(
        MediaDescriptorBuilder::new(format!("e621:{}:0", post.id), 0, url)
            .canonical_url(canonical_url)
            .file_name(format!("{hash}.{extension}"))
            .expected_size(post.file.size)
            .build(),
    )
}

fn canonical_tags(
    categories: &BTreeMap<String, Vec<String>>,
    rating: Option<&str>,
) -> Vec<CanonicalTag> {
    let mut tags = NAMESPACES.normalize(categories);
    RATINGS.add(&mut tags, rating);
    tags.into_vec()
}

#[derive(Debug, Deserialize)]
struct PostsResponse {
    #[serde(default)]
    posts: Vec<ApiPost>,
}

#[derive(Debug, Deserialize)]
struct ApiPost {
    id: u64,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    rating: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, Vec<String>>,
    file: ApiFile,
}

#[derive(Debug, Deserialize)]
struct ApiFile {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    md5: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::SourcePartition;

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "canine solo".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 10,
        }
    }

    #[test]
    fn builds_bounded_keyset_requests_without_query_owned_pagination() {
        let mut request = request();
        request.cursor = Some("b6360238".to_string());
        let url = request_url(&request).unwrap();
        let pairs = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            pairs.get("tags").map(|value| value.as_ref()),
            Some("canine solo")
        );
        assert_eq!(pairs.get("limit").map(|value| value.as_ref()), Some("10"));
        assert_eq!(
            pairs.get("page").map(|value| value.as_ref()),
            Some("b6360238")
        );
    }

    #[test]
    fn normalizes_categories_and_treats_noncanonical_groups_as_general() {
        let response: PostsResponse = serde_json::from_value(json!({
            "posts": [{
                "id": 6360238,
                "created_at": "2026-08-29T12:00:00Z",
                "description": "A post",
                "rating": "e",
                "tags": {
                    "artist": ["midna"],
                    "character": ["example_character"],
                    "copyright": ["example_series"],
                    "species": ["canine"],
                    "general": ["solo"],
                    "meta": ["highres"]
                },
                "file": {
                    "url": null,
                    "ext": "png",
                    "size": 5354344,
                    "md5": "4667e8118159b612601f1b72ae010693"
                }
            }]
        }))
        .unwrap();

        let batch = parse_posts(&request(), response);
        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "6360238");
        assert_eq!(post.resume_cursor_after.as_deref(), Some("b6360238"));
        assert!(post.tags.contains(&CanonicalTag::new("creator", "midna")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("character", "example_character")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("series", "example_series")));
        assert!(post.tags.contains(&CanonicalTag::new("species", "canine")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "explicit")));
        assert!(post.tags.contains(&CanonicalTag::new("", "solo")));
        assert!(post.tags.contains(&CanonicalTag::new("", "highres")));
        assert_eq!(
            post.media[0].url,
            "https://static1.e621.net/data/46/67/4667e8118159b612601f1b72ae010693.png",
        );
    }

    #[test]
    fn missing_file_identity_is_a_traversed_post_without_usable_media() {
        let response: PostsResponse = serde_json::from_value(json!({
            "posts": [{
                "id": 7,
                "tags": {},
                "file": {"url": null, "ext": null, "md5": null}
            }]
        }))
        .unwrap();
        let batch = parse_posts(&request(), response);
        assert_eq!(batch.posts.len(), 1);
        assert!(batch.posts[0].media.is_empty());
    }

    #[test]
    fn rejects_user_pagination_controls() {
        assert!(QUERY.validate("canine page:2").is_err());
        assert!(QUERY.validate("canine order:random").is_err());
    }
}
