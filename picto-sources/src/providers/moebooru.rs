use serde::Deserialize;
use url::Url;

use crate::{
    CanonicalTagSet, DiscoveryBatch, DiscoveryRequest, JsonPageSource, JsonSourceAdapter,
    MediaDescriptorBuilder, NativeSourceAdapter, PageCursor, ProviderDescriptor, RatingMap,
    SearchQueryPolicy, SourceError, SourcePost,
};

const CURSOR: PageCursor = PageCursor::new(1_000_000);
const QUERY: SearchQueryPolicy = SearchQueryPolicy::new("Moebooru", &["page:", "limit:", "order:"]);
const RATINGS: RatingMap = RatingMap::new(&[
    ("s", "safe"),
    ("safe", "safe"),
    ("q", "questionable"),
    ("questionable", "questionable"),
    ("e", "explicit"),
    ("explicit", "explicit"),
]);

#[derive(Clone, Copy)]
pub(super) struct MoebooruConfig {
    pub id: &'static str,
    pub display_name: &'static str,
    pub domain: &'static str,
    pub root: &'static str,
}

pub(super) fn adapter(config: MoebooruConfig) -> impl NativeSourceAdapter {
    JsonSourceAdapter::new(MoebooruSource { config })
}

struct MoebooruSource {
    config: MoebooruConfig,
}

impl JsonPageSource for MoebooruSource {
    type Response = Vec<ApiPost>;

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

    fn request_url(&self, request: &DiscoveryRequest) -> Result<Url, SourceError> {
        request_url(self.config, request)
    }

    fn normalize(
        &self,
        request: &DiscoveryRequest,
        response: Self::Response,
    ) -> Result<DiscoveryBatch, SourceError> {
        normalize(self.config, request, response)
    }
}

fn request_url(config: MoebooruConfig, request: &DiscoveryRequest) -> Result<Url, SourceError> {
    let page = current_page(request)?;
    let mut url =
        Url::parse(&format!("{}/post.json", config.root)).expect("static Moebooru provider URL");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("tags", request.query.trim());
        query.append_pair("limit", &page_limit(request).to_string());
        query.append_pair("page", &page.to_string());
    }
    Ok(url)
}

fn normalize(
    config: MoebooruConfig,
    request: &DiscoveryRequest,
    response: Vec<ApiPost>,
) -> Result<DiscoveryBatch, SourceError> {
    let page = current_page(request)?;
    let current_cursor = CURSOR.encode(page)?;
    let next_cursor = CURSOR.encode(page.saturating_add(1))?;
    let exhausted = response.len() < page_limit(request) as usize;
    let last = response.len().saturating_sub(1);
    let posts = response
        .into_iter()
        .enumerate()
        .map(|(index, post)| {
            let resume_cursor = if index == last {
                &next_cursor
            } else {
                &current_cursor
            };
            Ok(normalize_post(config, request, post, resume_cursor.clone()))
        })
        .collect::<Result<Vec<_>, SourceError>>()?;
    Ok(DiscoveryBatch { posts, exhausted })
}

fn normalize_post(
    config: MoebooruConfig,
    request: &DiscoveryRequest,
    post: ApiPost,
    resume_cursor_after: String,
) -> SourcePost {
    let post_id = post.id.to_string();
    let canonical_url = format!("{}/post/show/{post_id}", config.root);
    let mut tags = CanonicalTagSet::default();
    for tag in post.tags.split_whitespace() {
        tags.insert("", tag);
    }
    RATINGS.add(&mut tags, post.rating.as_deref());

    let media = post
        .file_url
        .filter(|url| !url.trim().is_empty())
        .map(|url| {
            let file_name = post
                .md5
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .zip(
                    post.file_ext
                        .as_deref()
                        .filter(|value| !value.trim().is_empty()),
                )
                .map(|(hash, extension)| format!("{hash}.{extension}"))
                .unwrap_or_else(|| format!("{}_{post_id}.media", config.id));
            MediaDescriptorBuilder::new(format!("{}:{post_id}:0", config.id), 0, url)
                .canonical_url(&canonical_url)
                .file_name(file_name)
                .expected_size(post.file_size)
                .build()
        })
        .into_iter()
        .collect();

    SourcePost {
        site_id: config.id.to_string(),
        partition: request.partition.clone(),
        stable_id: post_id.clone(),
        canonical_url: Some(canonical_url),
        creator: None,
        name: Some(format!("{}_{post_id}", config.id)),
        notes: None,
        created_at: post.created_at.map(|value| value.to_string()),
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(resume_cursor_after),
    }
}

fn current_page(request: &DiscoveryRequest) -> Result<u32, SourceError> {
    request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| CURSOR.validate(cursor))
        .unwrap_or(Ok(1))
}

fn page_limit(request: &DiscoveryRequest) -> u32 {
    request.page_size.clamp(1, 100)
}

#[derive(Debug, Deserialize)]
struct ApiPost {
    id: u64,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    rating: Option<String>,
    #[serde(default)]
    file_url: Option<String>,
    #[serde(default)]
    file_ext: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    md5: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::providers::{konachan, yandere};
    use crate::{CanonicalTag, SourcePartition};

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "landscape rating:s".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 2,
        }
    }

    fn response() -> Vec<ApiPost> {
        serde_json::from_value(json!([
            {
                "id": 42,
                "tags": "landscape artist_name",
                "created_at": 1788018233,
                "rating": "s",
                "file_url": "https://files.example/42.jpg",
                "file_ext": "jpg",
                "file_size": 15314756,
                "md5": "8a796db7a449fe90d2037fe8b92d461a"
            },
            {
                "id": 41,
                "tags": "landscape night",
                "rating": "q",
                "file_url": null
            }
        ]))
        .unwrap()
    }

    #[test]
    fn builds_direct_bounded_page_requests_for_each_site() {
        let mut request = request();
        request.cursor = Some("7".to_string());

        for (config, expected) in [
            (yandere::CONFIG, "https://yande.re/post.json"),
            (konachan::CONFIG, "https://konachan.com/post.json"),
        ] {
            let url = request_url(config, &request).unwrap();
            assert_eq!(url.as_str().split('?').next(), Some(expected));
            let pairs = url.query_pairs().collect::<BTreeMap<_, _>>();
            assert_eq!(
                pairs.get("tags").map(|value| value.as_ref()),
                Some("landscape rating:s")
            );
            assert_eq!(pairs.get("limit").map(|value| value.as_ref()), Some("2"));
            assert_eq!(pairs.get("page").map(|value| value.as_ref()), Some("7"));
        }
    }

    #[test]
    fn maps_shared_fields_and_site_identity() {
        for config in [yandere::CONFIG, konachan::CONFIG] {
            let batch = normalize(config, &request(), response()).unwrap();
            let post = &batch.posts[0];
            assert_eq!(post.site_id, config.id);
            assert_eq!(
                post.canonical_url.as_deref(),
                Some(format!("{}/post/show/42", config.root).as_str())
            );
            assert_eq!(post.created_at.as_deref(), Some("1788018233"));
            assert!(post.tags.contains(&CanonicalTag::new("", "landscape")));
            assert!(post.tags.contains(&CanonicalTag::new("", "artist_name")));
            assert!(post.tags.contains(&CanonicalTag::new("rating", "safe")));
            assert_eq!(post.media[0].stable_id, format!("{}:42:0", config.id));
            assert_eq!(
                post.media[0].file_name.as_deref(),
                Some("8a796db7a449fe90d2037fe8b92d461a.jpg")
            );
            assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/jpeg"));
            assert!(batch.posts[1].media.is_empty());
        }
    }

    #[test]
    fn advances_page_only_after_the_last_post_settles() {
        let mut request = request();
        request.cursor = Some("7".to_string());
        let batch = normalize(yandere::CONFIG, &request, response()).unwrap();

        assert!(!batch.exhausted);
        assert_eq!(batch.posts[0].resume_cursor_after.as_deref(), Some("7"));
        assert_eq!(batch.posts[1].resume_cursor_after.as_deref(), Some("8"));
    }

    #[test]
    fn rejects_invalid_cursors_and_query_owned_traversal() {
        let mut request = request();
        request.cursor = Some("page-2".to_string());
        assert!(request_url(yandere::CONFIG, &request).is_err());

        for query in ["landscape page:2", "landscape limit:50", "order:random"] {
            assert!(QUERY.validate(query).is_err());
        }
        assert!(QUERY.validate("landscape rating:s").is_ok());
    }
}
