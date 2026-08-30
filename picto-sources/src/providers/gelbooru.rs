use std::collections::BTreeSet;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    AdapterFuture, BeforeIdCursor, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest, HttpRuntime,
    MediaDescriptorBuilder, NativeSourceAdapter, ProviderDescriptor, RatingMap, RequestCredentials,
    SearchQueryPolicy, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: BeforeIdCursor = BeforeIdCursor::new("b");
const QUERY: SearchQueryPolicy = SearchQueryPolicy::new(
    "Gelbooru-family",
    &["id:", "limit:", "order:", "page:", "pid:", "sort:"],
);
const RATINGS: RatingMap = RatingMap::new(&[
    ("s", "safe"),
    ("safe", "safe"),
    ("q", "questionable"),
    ("questionable", "questionable"),
    ("e", "explicit"),
    ("explicit", "explicit"),
]);

pub(super) const CONFIG: GelbooruFamilyConfig = GelbooruFamilyConfig {
    id: "gelbooru",
    display_name: "Gelbooru",
    domain: "gelbooru.com",
    api_root: "https://gelbooru.com",
    web_root: "https://gelbooru.com",
    rule34_image_host: None,
    request_tag_info: false,
};

#[derive(Clone, Copy)]
pub(super) struct GelbooruFamilyConfig {
    pub id: &'static str,
    pub display_name: &'static str,
    pub domain: &'static str,
    pub api_root: &'static str,
    pub web_root: &'static str,
    pub rule34_image_host: Option<&'static str>,
    pub request_tag_info: bool,
}

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    family_adapter(CONFIG)
}

pub(super) fn family_adapter(config: GelbooruFamilyConfig) -> impl NativeSourceAdapter {
    GelbooruFamilyAdapter { config }
}

struct GelbooruFamilyAdapter {
    config: GelbooruFamilyConfig,
}

impl NativeSourceAdapter for GelbooruFamilyAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.config.id,
            display_name: self.config.display_name,
            domain: self.config.domain,
            partitions: &["posts"],
            anonymous: false,
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
            let page_size = page_size(request);
            let response = http
                .get_json::<ApiResponse>(
                    request_url(self.config, request, credentials)?,
                    credentials,
                    cancel,
                )
                .await?;
            normalize(self.config, request, page_size, response)
        })
    }
}

pub(super) fn request_url(
    config: GelbooruFamilyConfig,
    request: &DiscoveryRequest,
    credentials: &RequestCredentials,
) -> Result<Url, SourceError> {
    let mut tags = request.query.trim().to_string();
    if let Some(cursor) = request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
    {
        tags.push_str(" id:<");
        tags.push_str(CURSOR.validate(cursor)?);
    }

    let mut url = Url::parse(config.api_root).expect("static Gelbooru-family API root");
    url.set_path("/index.php");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("page", "dapi");
        query.append_pair("s", "post");
        query.append_pair("q", "index");
        query.append_pair("json", "1");
        query.append_pair("tags", &tags);
        query.append_pair("limit", &page_size(request).to_string());
        query.append_pair("pid", "0");
        if config.request_tag_info {
            query.append_pair("fields", "tag_info");
        }
        if credentials.permits(config.domain) {
            if let Some(api_key) = credentials
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query.append_pair("api_key", api_key);
            }
            if let Some(user_id) = credentials
                .username
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query.append_pair("user_id", user_id);
            }
        }
    }
    Ok(url)
}

fn normalize(
    config: GelbooruFamilyConfig,
    request: &DiscoveryRequest,
    page_size: u32,
    response: ApiResponse,
) -> Result<DiscoveryBatch, SourceError> {
    let posts = match response {
        ApiResponse::List(posts) => posts,
        ApiResponse::Wrapped { post } => (*post).into_vec(),
        ApiResponse::Error { message, .. } => {
            let authentication = message.to_ascii_lowercase().contains("authentication");
            return Err(SourceError::new(
                if authentication {
                    SourceErrorKind::Authentication
                } else {
                    SourceErrorKind::InvalidResponse
                },
                message,
                !authentication,
            ));
        }
    };
    let exhausted = posts.len() < page_size as usize;
    let posts = posts
        .into_iter()
        .map(|post| normalize_post(config, request, post))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DiscoveryBatch { posts, exhausted })
}

fn normalize_post(
    config: GelbooruFamilyConfig,
    request: &DiscoveryRequest,
    post: ApiPost,
) -> Result<SourcePost, SourceError> {
    let post_id = post.id.to_string();
    let canonical_url = format!(
        "{}/index.php?page=post&s=view&id={post_id}",
        config.web_root
    );
    let (tags, creator) = canonical_tags(&post);
    let media = post
        .file_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .map(|url| media_descriptor(config, &post, &post_id, &canonical_url, url))
        .transpose()?
        .into_iter()
        .collect();

    Ok(SourcePost {
        site_id: config.id.to_string(),
        partition: request.partition.clone(),
        stable_id: post_id.clone(),
        canonical_url: Some(canonical_url),
        creator,
        name: Some(format!("{}_{post_id}", config.id)),
        notes: None,
        created_at: post.created_at,
        tags,
        media,
        resume_cursor_after: Some(CURSOR.encode(post_id)),
    })
}

fn media_descriptor(
    config: GelbooruFamilyConfig,
    post: &ApiPost,
    post_id: &str,
    canonical_url: &str,
    raw_url: &str,
) -> Result<crate::MediaDescriptor, SourceError> {
    let mut url = Url::parse(raw_url).map_err(|error| {
        SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
    })?;
    if let Some(image_host) = config.rule34_image_host {
        let path = url.path().to_ascii_lowercase();
        if !path.ends_with(".webm") && !path.ends_with(".mp4") {
            url.set_host(Some(image_host)).map_err(|_| {
                SourceError::new(
                    SourceErrorKind::InvalidResponse,
                    "invalid Rule34 media host",
                    false,
                )
            })?;
        }
    }
    let extension = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        .filter(|extension| !extension.is_empty());
    let file_name = post
        .md5
        .as_deref()
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .zip(extension)
        .map(|(hash, extension)| format!("{hash}.{extension}"))
        .or_else(|| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("{}_{post_id}.media", config.id));

    Ok(
        MediaDescriptorBuilder::new(format!("{}:{post_id}:0", config.id), 0, url.to_string())
            .canonical_url(canonical_url)
            .file_name(file_name)
            .build(),
    )
}

fn canonical_tags(post: &ApiPost) -> (Vec<crate::CanonicalTag>, Option<String>) {
    let mut tags = CanonicalTagSet::default();
    let mut categorized = BTreeSet::new();
    let mut creator = post
        .tags_artist
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned);

    add_words(&mut tags, &mut categorized, "creator", &post.tags_artist);
    add_words(
        &mut tags,
        &mut categorized,
        "character",
        &post.tags_character,
    );
    add_words(&mut tags, &mut categorized, "series", &post.tags_copyright);
    add_words(&mut tags, &mut categorized, "species", &post.tags_species);
    add_general_words(&mut tags, &mut categorized, &post.tags_general);
    add_general_words(&mut tags, &mut categorized, &post.tags_metadata);
    add_general_words(&mut tags, &mut categorized, &post.tags_meta);

    for info in &post.tag_info {
        let Some(value) = info.name() else {
            continue;
        };
        let namespace = match info.kind.as_deref().unwrap_or_default() {
            "artist" => "creator",
            "character" => "character",
            "copyright" => "series",
            "species" => "species",
            _ => "",
        };
        if namespace == "creator" && creator.is_none() {
            creator = Some(value.to_string());
        }
        tags.insert(namespace, value);
        categorized.insert(value.to_string());
    }

    for value in post.tags.split_whitespace() {
        if !categorized.contains(value) {
            tags.insert("", value);
        }
    }
    RATINGS.add(&mut tags, post.rating.as_deref());
    (tags.into_vec(), creator)
}

fn add_words(
    tags: &mut CanonicalTagSet,
    categorized: &mut BTreeSet<String>,
    namespace: &str,
    words: &str,
) {
    for word in words.split_whitespace() {
        tags.insert(namespace, word);
        categorized.insert(word.to_string());
    }
}

fn add_general_words(tags: &mut CanonicalTagSet, categorized: &mut BTreeSet<String>, words: &str) {
    for word in words.split_whitespace() {
        tags.insert("", word);
        categorized.insert(word.to_string());
    }
}

fn page_size(request: &DiscoveryRequest) -> u32 {
    request.page_size.clamp(1, 100)
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiResponse {
    Error {
        #[serde(rename = "success")]
        _success: bool,
        message: String,
    },
    Wrapped {
        #[serde(default)]
        post: Box<OneOrMany<ApiPost>>,
    },
    List(Vec<ApiPost>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
    #[default]
    Empty,
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
            Self::Empty => Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiPost {
    id: u64,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    rating: Option<String>,
    #[serde(default)]
    file_url: Option<String>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    tags_artist: String,
    #[serde(default)]
    tags_character: String,
    #[serde(default)]
    tags_copyright: String,
    #[serde(default)]
    tags_species: String,
    #[serde(default)]
    tags_general: String,
    #[serde(default)]
    tags_metadata: String,
    #[serde(default)]
    tags_meta: String,
    #[serde(default)]
    tag_info: Vec<ApiTagInfo>,
}

#[derive(Debug, Deserialize)]
struct ApiTagInfo {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl ApiTagInfo {
    fn name(&self) -> Option<&str> {
        self.tag
            .as_deref()
            .or(self.name.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[cfg(test)]
pub(super) fn normalize_fixture(
    config: GelbooruFamilyConfig,
    request: &DiscoveryRequest,
    page_size: u32,
    fixture: &str,
) -> Result<DiscoveryBatch, SourceError> {
    let response = serde_json::from_str(fixture).map_err(|error| {
        SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
    })?;
    normalize(config, request, page_size, response)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "1girl solo rating:safe".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 200,
        }
    }

    #[test]
    fn builds_authenticated_bounded_keyset_request() {
        let mut request = request();
        request.cursor = Some("b13753751".to_string());
        let credentials = RequestCredentials {
            username: Some("277923".to_string()),
            api_key: Some("secret".to_string()),
            allowed_domains: BTreeSet::from(["gelbooru.com".to_string()]),
            ..RequestCredentials::default()
        };
        let url = request_url(CONFIG, &request, &credentials).unwrap();
        let pairs = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(pairs.get("limit").map(|value| value.as_ref()), Some("100"));
        assert_eq!(pairs.get("pid").map(|value| value.as_ref()), Some("0"));
        assert_eq!(
            pairs.get("tags").map(|value| value.as_ref()),
            Some("1girl solo rating:safe id:<13753751")
        );
        assert_eq!(
            pairs.get("api_key").map(|value| value.as_ref()),
            Some("secret")
        );
        assert_eq!(
            pairs.get("user_id").map(|value| value.as_ref()),
            Some("277923")
        );
    }

    #[test]
    fn maps_fixture_to_canonical_post_and_per_post_cursor() {
        let batch = normalize_fixture(
            CONFIG,
            &request(),
            100,
            include_str!("../../tests/fixtures/gelbooru/search.json"),
        )
        .unwrap();
        assert!(batch.exhausted);
        let post = &batch.posts[0];
        assert_eq!(post.resume_cursor_after.as_deref(), Some("b13753751"));
        assert_eq!(post.creator.as_deref(), Some("artist_name"));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "artist_name")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("character", "princess_peach")));
        assert!(post.tags.contains(&CanonicalTag::new("series", "mario")));
        assert!(post.tags.contains(&CanonicalTag::new("", "highres")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "safe")));
        assert!(!post.tags.contains(&CanonicalTag::new("", "artist_name")));
        assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/jpeg"));
    }

    #[test]
    fn rejects_query_owned_traversal_and_invalid_cursors() {
        for query in ["solo id:<42", "solo limit:10", "sort:id:asc", "solo pid:2"] {
            assert!(QUERY.validate(query).is_err(), "{query}");
        }
        let mut request = request();
        request.cursor = Some("42".to_string());
        assert!(request_url(CONFIG, &request, &RequestCredentials::default()).is_err());
    }

    #[test]
    fn never_places_credentials_on_a_disallowed_domain() {
        let credentials = RequestCredentials {
            username: Some("277923".to_string()),
            api_key: Some("secret".to_string()),
            allowed_domains: BTreeSet::from(["unrelated.example".to_string()]),
            ..RequestCredentials::default()
        };
        let url = request_url(CONFIG, &request(), &credentials).unwrap();
        let keys = url
            .query_pairs()
            .map(|(key, _)| key.into_owned())
            .collect::<BTreeSet<_>>();
        assert!(!keys.contains("api_key"));
        assert!(!keys.contains("user_id"));
    }
}
