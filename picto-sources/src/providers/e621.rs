use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use crate::{
    normalize_source_text, BeforeIdCursor, CanonicalTag, DiscoveryBatch, DiscoveryRequest,
    JsonPageSource, JsonSourceAdapter, MediaDescriptor, MediaDescriptorBuilder, NamespaceMap,
    NativeSourceAdapter, ProviderDescriptor, RatingMap, SearchQueryPolicy, SourceError, SourcePost,
};

const CURSOR: BeforeIdCursor = BeforeIdCursor::new("b");
const QUERY: SearchQueryPolicy = SearchQueryPolicy::new(
    "e621",
    &[
        "page:",
        "limit:",
        "order:",
        "ordfav:",
        "ordfavgroup:",
        "ordpool:",
    ],
);
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

// Request and response handling follow gallery-dl's e621 extractor at Codeberg
// commit 552cb84a08c4f0a4c071a466573fe749269573a3 (2026-11-20).

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

    /// e621's API rules require a user agent that names the application, and
    /// the site blocks requests that claim to be a browser.
    fn user_agent(&self) -> Option<&'static str> {
        Some(crate::http::APPLICATION_USER_AGENT)
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
        // Without `mode=extended` e621 returns each post's tags as one flat
        // list, and the artist, character, copyright and species namespaces
        // are lost.
        query.append_pair("mode", "extended");
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
    let posts = response.into_posts();
    let exhausted = posts.len() < request.page_size as usize;
    let posts = posts
        .into_iter()
        .map(|post| normalize_post(request, post))
        .collect();
    DiscoveryBatch { posts, exhausted }
}

fn normalize_post(request: &DiscoveryRequest, post: ApiPost) -> SourcePost {
    let post_id = post.id.to_string();
    let canonical_url = format!("https://e621.net/posts/{post_id}");
    let categories = post.tags.into_categories();
    let creator = categories
        .get("artist")
        .and_then(|values| values.first())
        .cloned();
    let tags = canonical_tags(&categories, post.rating.as_deref());
    let media = post
        .file
        .as_ref()
        .and_then(|file| media_descriptor(post.id, file, &canonical_url))
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

/// Describes the post's original file. e621 leaves the file URL out for some
/// posts when the request is anonymous; the URL is then built from the MD5
/// hash and the extension, which is where the site stores every original.
fn media_descriptor(post_id: u64, file: &ApiFile, canonical_url: &str) -> Option<MediaDescriptor> {
    let extension = file.ext.as_deref()?.trim();
    let hash = file.md5.as_deref()?.trim();
    if extension.is_empty() || hash.len() < 4 || !hash.is_ascii() {
        return None;
    }
    let url = file
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
        MediaDescriptorBuilder::new(format!("e621:{post_id}:0"), 0, url)
            .canonical_url(canonical_url)
            .file_name(format!("{hash}.{extension}"))
            .expected_size(file.size)
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

/// The body of `posts.json`. Since November 2026 e621 answers with a
/// top-level array of posts. Before that, and still when a request carries
/// `v1=true`, it answers with an object whose `posts` field holds the array.
/// Any other body, such as `{"error": ...}`, fails to parse, so an error
/// response is never mistaken for the end of a search.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PostsResponse {
    Current(Vec<ApiPost>),
    Previous { posts: Vec<ApiPost> },
}

impl PostsResponse {
    fn into_posts(self) -> Vec<ApiPost> {
        match self {
            Self::Current(posts) | Self::Previous { posts } => posts,
        }
    }
}

/// One post as Picto uses it, read from either layout of `posts.json`.
#[derive(Debug, Deserialize)]
#[serde(from = "RawPost")]
struct ApiPost {
    id: u64,
    created_at: Option<String>,
    description: Option<String>,
    rating: Option<String>,
    tags: ApiTags,
    /// `None` when the post has neither a `files` nor a `file` object.
    file: Option<ApiFile>,
}

/// The original file of a post: its URL, extension, size in bytes and MD5
/// hash. Every field can be missing or null.
#[derive(Debug, Default, Deserialize)]
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

/// A post's tags. With `mode=extended`, and in the previous layout, they are
/// grouped by category (`artist`, `character`, `general` and so on). Without
/// it the current layout returns one flat list that does not say which
/// category a tag belongs to.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiTags {
    Categories(BTreeMap<String, Vec<String>>),
    Flat(Vec<String>),
}

impl Default for ApiTags {
    fn default() -> Self {
        Self::Categories(BTreeMap::new())
    }
}

impl ApiTags {
    /// Returns the tags grouped by category. A flat list goes into `general`
    /// because its categories are unknown.
    fn into_categories(self) -> BTreeMap<String, Vec<String>> {
        match self {
            Self::Categories(categories) => categories,
            Self::Flat(tags) => BTreeMap::from([("general".to_string(), tags)]),
        }
    }
}

/// A post exactly as `posts.json` sends it. The current layout puts the
/// original file under `files`; the previous layout puts it under `file`.
#[derive(Debug, Deserialize)]
struct RawPost {
    id: u64,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    rating: Option<String>,
    #[serde(default)]
    tags: ApiTags,
    #[serde(default)]
    files: Option<RawFiles>,
    #[serde(default)]
    file: Option<ApiFile>,
}

/// The `files` object of the current layout. `meta` holds the MD5 hash, the
/// extension and the size of the original file, and `original` holds its
/// URL. The `preview` and `sample` renditions are not read because Picto
/// downloads only the original.
#[derive(Debug, Deserialize)]
struct RawFiles {
    #[serde(default)]
    meta: Option<RawFileMeta>,
    #[serde(default)]
    original: Option<RawOriginal>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFileMeta {
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawOriginal {
    #[serde(default)]
    url: Option<String>,
}

impl From<RawPost> for ApiPost {
    fn from(post: RawPost) -> Self {
        let file = match post.files {
            Some(files) => {
                let meta = files.meta.unwrap_or_default();
                Some(ApiFile {
                    url: files.original.unwrap_or_default().url,
                    ext: meta.ext,
                    size: meta.size,
                    md5: meta.md5,
                })
            }
            None => post.file,
        };
        Self {
            id: post.id,
            created_at: post.created_at,
            description: post.description,
            rating: post.rating,
            tags: post.tags,
            file,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::SourcePartition;

    #[test]
    fn malformed_envelopes_cannot_exhaust_a_search() {
        for response in [
            json!({}),
            json!({"error": "unavailable"}),
            json!({"posts": null}),
        ] {
            assert!(serde_json::from_value::<PostsResponse>(response).is_err());
        }
        for empty in [json!([]), json!({"posts": []})] {
            let response = serde_json::from_value(empty).unwrap();
            assert!(parse_posts(&request(), response).exhausted);
        }
    }

    #[test]
    fn ordered_collections_cannot_use_the_id_cursor() {
        for query in ["ordpool:42", "ordfav:user", "ordfavgroup:42"] {
            assert!(adapter().validate_query(query).is_err());
        }
    }

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
        assert_eq!(
            pairs.get("mode").map(|value| value.as_ref()),
            Some("extended")
        );
    }

    #[test]
    fn reads_the_current_layout_with_files_and_a_top_level_array() {
        let response: PostsResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/e621/posts.json")).unwrap();
        let batch = parse_posts(&request(), response);
        assert_eq!(batch.posts.len(), 2);
        assert!(batch.exhausted);

        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "6360238");
        assert_eq!(
            post.canonical_url.as_deref(),
            Some("https://e621.net/posts/6360238")
        );
        assert_eq!(post.creator.as_deref(), Some("example_artist"));
        assert_eq!(post.notes.as_deref(), Some("A post"));
        assert_eq!(
            post.created_at.as_deref(),
            Some("2026-11-20T16:58:50.553-05:00")
        );
        assert_eq!(post.resume_cursor_after.as_deref(), Some("b6360238"));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "example_artist")));
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
        assert_eq!(post.media.len(), 1);
        assert_eq!(
            post.media[0].url,
            "https://static1.e621.net/data/46/67/4667e8118159b612601f1b72ae010693.png",
        );
        assert_eq!(
            post.media[0].file_name.as_deref(),
            Some("4667e8118159b612601f1b72ae010693.png")
        );
        assert_eq!(post.media[0].expected_size, Some(5354344));

        // The second post has no file URL, so the URL is built from its hash.
        let hidden = &batch.posts[1];
        assert_eq!(hidden.resume_cursor_after.as_deref(), Some("b6360237"));
        assert_eq!(
            hidden.media[0].url,
            "https://static1.e621.net/data/01/23/0123456789abcdef0123456789abcdef.webm",
        );
        assert!(hidden.tags.contains(&CanonicalTag::new("rating", "safe")));
    }

    #[test]
    fn a_flat_tag_list_becomes_general_tags() {
        let response: PostsResponse = serde_json::from_value(json!([{
            "id": 9,
            "rating": "q",
            "tags": ["canine", "example_artist"],
            "files": {
                "meta": {"md5": "0123456789abcdef0123456789abcdef", "ext": "jpg", "size": 10},
                "original": {"url": null}
            }
        }]))
        .unwrap();
        let batch = parse_posts(&request(), response);
        let post = &batch.posts[0];
        assert_eq!(post.creator, None);
        assert!(post.tags.contains(&CanonicalTag::new("", "canine")));
        assert!(post.tags.contains(&CanonicalTag::new("", "example_artist")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("rating", "questionable")));
        assert_eq!(post.media.len(), 1);
    }

    #[test]
    fn reads_the_previous_layout_with_a_posts_object_and_a_file_object() {
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
        assert_eq!(post.name.as_deref(), Some("e621_6360238"));
        assert_eq!(post.notes.as_deref(), Some("A post"));
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

        let response: PostsResponse = serde_json::from_value(json!([
            {"id": 8, "tags": [], "files": {"meta": {"md5": null, "ext": null}}},
            {"id": 7, "tags": []}
        ]))
        .unwrap();
        let batch = parse_posts(&request(), response);
        assert_eq!(batch.posts.len(), 2);
        assert!(batch.posts.iter().all(|post| post.media.is_empty()));
    }

    #[test]
    fn rejects_user_pagination_controls() {
        assert!(QUERY.validate("canine page:2").is_err());
        assert!(QUERY.validate("canine order:random").is_err());
    }
}
