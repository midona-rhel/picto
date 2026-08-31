use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use crate::{
    normalize_source_text, BeforeIdCursor, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    JsonPageSource, JsonSourceAdapter, MediaDescriptorBuilder, NativeSourceAdapter,
    ProviderDescriptor, RatingMap, SearchQueryPolicy, SourceError, SourcePost,
};

const CURSOR: BeforeIdCursor = BeforeIdCursor::new("b");
const ONLY_FIELDS: &str = "id,created_at,rating,file_url,large_file_url,file_ext,file_size,md5,tag_string_artist,tag_string_character,tag_string_copyright,tag_string_general,tag_string_meta,artist_commentary";
const QUERY: SearchQueryPolicy = SearchQueryPolicy::new("Danbooru", &["page:", "limit:", "order:"]);
const RATINGS: RatingMap = RatingMap::new(&[
    ("g", "general"),
    ("general", "general"),
    ("s", "sensitive"),
    ("sensitive", "sensitive"),
    ("q", "questionable"),
    ("questionable", "questionable"),
    ("e", "explicit"),
    ("explicit", "explicit"),
]);

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    JsonSourceAdapter::new(DanbooruSource)
}

struct DanbooruSource;

impl JsonPageSource for DanbooruSource {
    type Response = Vec<ApiPost>;

    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "danbooru",
            display_name: "Danbooru",
            domain: "danbooru.donmai.us",
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn user_agent(&self) -> Option<&'static str> {
        Some(crate::http::APPLICATION_USER_AGENT)
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        QUERY.validate(query)
    }

    fn request_url(&self, request: &DiscoveryRequest) -> Result<Url, SourceError> {
        let mut url =
            Url::parse("https://danbooru.donmai.us/posts.json").expect("static Danbooru URL");
        let mut query = url.query_pairs_mut();
        query.append_pair("tags", request.query.trim());
        query.append_pair("limit", &request.page_size.clamp(1, 200).to_string());
        query.append_pair("only", ONLY_FIELDS);
        if let Some(cursor) = request
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
        {
            CURSOR.validate(cursor)?;
            query.append_pair("page", cursor);
        }
        drop(query);
        Ok(url)
    }

    fn normalize(
        &self,
        request: &DiscoveryRequest,
        response: Self::Response,
    ) -> Result<DiscoveryBatch, SourceError> {
        let exhausted = response.len() < request.page_size as usize;
        let posts = response
            .into_iter()
            .map(|post| normalize_post(request, post))
            .collect();
        Ok(DiscoveryBatch { posts, exhausted })
    }
}

fn normalize_post(request: &DiscoveryRequest, post: ApiPost) -> SourcePost {
    let post_id = post.id.to_string();
    let canonical_url = format!("https://danbooru.donmai.us/posts/{post_id}");
    let mut tags = CanonicalTagSet::default();
    add_words(&mut tags, "creator", &post.tag_string_artist);
    add_words(&mut tags, "character", &post.tag_string_character);
    add_words(&mut tags, "series", &post.tag_string_copyright);
    add_words(&mut tags, "", &post.tag_string_general);
    add_words(&mut tags, "", &post.tag_string_meta);
    RATINGS.add(&mut tags, post.rating.as_deref());

    let media_url = match post.file_ext.as_deref() {
        Some("zip") => post
            .large_file_url
            .as_deref()
            .and_then(normalize_media_url)
            .filter(|url| {
                let path = Url::parse(url)
                    .ok()
                    .map(|url| url.path().to_ascii_lowercase())
                    .unwrap_or_default();
                path.ends_with(".webm") || path.ends_with(".mp4")
            }),
        _ => post.file_url.as_deref().and_then(normalize_media_url),
    };
    let media = media_url
        .filter(|url| !url.trim().is_empty())
        .map(|url| {
            let selected_extension = if post.file_ext.as_deref() == Some("zip") {
                Url::parse(&url).ok().and_then(|url| {
                    url.path_segments()
                        .and_then(|mut segments| segments.next_back())
                        .and_then(|name| name.rsplit_once('.'))
                        .map(|(_, extension)| extension.to_string())
                })
            } else {
                post.file_ext.clone()
            };
            let file_name = post
                .md5
                .as_deref()
                .zip(selected_extension.as_deref())
                .map(|(hash, extension)| format!("{hash}.{extension}"))
                .unwrap_or_else(|| format!("danbooru_{post_id}.media"));
            MediaDescriptorBuilder::new(format!("danbooru:{post_id}:0"), 0, url)
                .canonical_url(&canonical_url)
                .file_name(file_name)
                .expected_size(post.file_size)
                .headers(BTreeMap::new())
                .build()
        })
        .into_iter()
        .collect();

    let commentary_title = post
        .artist_commentary
        .as_ref()
        .and_then(|commentary| commentary.original_title.as_deref())
        .and_then(normalize_source_text);
    let commentary_notes = post
        .artist_commentary
        .as_ref()
        .and_then(|commentary| commentary.original_description.as_deref())
        .and_then(normalize_source_text);
    SourcePost {
        site_id: "danbooru".into(),
        partition: request.partition.clone(),
        stable_id: post_id.clone(),
        canonical_url: Some(canonical_url),
        creator: first_word(&post.tag_string_artist),
        name: commentary_title.or_else(|| Some(format!("danbooru_{post_id}"))),
        notes: commentary_notes,
        created_at: post.created_at,
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(CURSOR.encode(post_id)),
    }
}

fn normalize_media_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Url::parse("https://danbooru.donmai.us")
        .expect("static Danbooru root")
        .join(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.to_string())
}

fn add_words(tags: &mut CanonicalTagSet, namespace: &str, words: &str) {
    for word in words.split_whitespace() {
        tags.insert(namespace, word);
    }
}

fn first_word(words: &str) -> Option<String> {
    words.split_whitespace().next().map(ToOwned::to_owned)
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
    large_file_url: Option<String>,
    #[serde(default)]
    file_ext: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    tag_string_artist: String,
    #[serde(default)]
    tag_string_character: String,
    #[serde(default)]
    tag_string_copyright: String,
    #[serde(default)]
    tag_string_general: String,
    #[serde(default)]
    tag_string_meta: String,
    #[serde(default)]
    artist_commentary: Option<ArtistCommentary>,
}

#[derive(Debug, Deserialize)]
struct ArtistCommentary {
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    original_description: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    #[test]
    fn maps_danbooru_fields_through_shared_canonical_types() {
        let request = DiscoveryRequest {
            query: "canine".into(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 10,
        };
        let response: Vec<ApiPost> = serde_json::from_value(json!([{
            "id": 42,
            "created_at": "2026-08-30T00:00:00Z",
            "rating": "e",
            "file_url": "https://cdn.donmai.us/file.png",
            "file_ext": "png",
            "file_size": 100,
            "md5": "0123456789abcdef0123456789abcdef",
            "tag_string_artist": "artist_name",
            "tag_string_character": "character_name",
            "tag_string_copyright": "series_name",
            "tag_string_general": "solo canine",
            "tag_string_meta": "highres",
            "artist_commentary": {
                "original_title": "A human title",
                "original_description": "<p>Readable &amp; clear.</p>"
            }
        }]))
        .unwrap();

        let url = DanbooruSource.request_url(&request).unwrap();
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "only")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some(ONLY_FIELDS)
        );

        let batch = DanbooruSource.normalize(&request, response).unwrap();
        let post = &batch.posts[0];
        assert_eq!(post.resume_cursor_after.as_deref(), Some("b42"));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("creator", "artist_name")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("character", "character_name")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("series", "series_name")));
        assert!(post.tags.contains(&CanonicalTag::new("", "highres")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "explicit")));
        assert_eq!(post.media[0].mime_hint.as_deref(), Some("image/png"));
        assert_eq!(post.media[0].url, "https://cdn.donmai.us/file.png");
        assert_eq!(post.name.as_deref(), Some("A human title"));
        assert_eq!(post.notes.as_deref(), Some("Readable & clear."));
    }

    #[test]
    fn never_substitutes_the_large_sample_for_a_missing_original() {
        let request = DiscoveryRequest {
            query: "canine".into(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 10,
        };
        let response: Vec<ApiPost> = serde_json::from_value(json!([{
            "id": 43,
            "large_file_url": "https://cdn.donmai.us/sample/sample-43.jpg"
        }]))
        .unwrap();
        let batch = DanbooruSource.normalize(&request, response).unwrap();
        assert!(batch.posts[0].media.is_empty());
    }

    #[test]
    fn ugoira_uses_the_derived_video_instead_of_the_source_archive() {
        let request = DiscoveryRequest {
            query: "animated".into(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 10,
        };
        let response: Vec<ApiPost> = serde_json::from_value(json!([{
            "id": 44,
            "file_url": "https://cdn.donmai.us/original/44.zip",
            "large_file_url": "https://cdn.donmai.us/original/44.webm",
            "file_ext": "zip",
            "md5": "0123456789abcdef0123456789abcdef"
        }]))
        .unwrap();
        let batch = DanbooruSource.normalize(&request, response).unwrap();
        assert_eq!(batch.posts[0].media.len(), 1);
        assert_eq!(
            batch.posts[0].media[0].url,
            "https://cdn.donmai.us/original/44.webm"
        );
        assert_eq!(
            batch.posts[0].media[0].file_name.as_deref(),
            Some("0123456789abcdef0123456789abcdef.webm")
        );
    }

    #[test]
    fn normalizes_scheme_relative_original_urls() {
        assert_eq!(
            normalize_media_url("//cdn.donmai.us/original/file.png").as_deref(),
            Some("https://cdn.donmai.us/original/file.png")
        );
    }
}
