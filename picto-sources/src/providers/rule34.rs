use crate::NativeSourceAdapter;

use super::gelbooru::{self, GelbooruFamilyConfig};

pub(super) const CONFIG: GelbooruFamilyConfig = GelbooruFamilyConfig {
    id: "rule34",
    display_name: "Rule34.xxx",
    domain: "rule34.xxx",
    api_root: "https://api.rule34.xxx",
    web_root: "https://rule34.xxx",
    rule34_image_host: Some("wimg.rule34.xxx"),
    request_tag_info: true,
};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    gelbooru::family_adapter(CONFIG)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::providers::gelbooru::{normalize_fixture, request_url};
    use crate::{CanonicalTag, DiscoveryRequest, RequestCredentials, SourcePartition};

    fn request() -> DiscoveryRequest {
        DiscoveryRequest {
            query: "solo".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: None,
            page_size: 10,
        }
    }

    #[test]
    fn uses_rule34_api_and_optional_request_credentials() {
        let url = request_url(
            CONFIG,
            &request(),
            &RequestCredentials {
                username: Some("42".to_string()),
                api_key: Some("secret".to_string()),
                allowed_domains: BTreeSet::from(["rule34.xxx".to_string()]),
                ..RequestCredentials::default()
            },
        )
        .unwrap();
        assert_eq!(url.host_str(), Some("api.rule34.xxx"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "user_id" && value == "42"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "api_key" && value == "secret"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "fields" && value == "tag_info"));
    }

    #[test]
    fn maps_api_categories_and_uses_image_cdn_without_rewriting_video() {
        let batch = normalize_fixture(
            CONFIG,
            &request(),
            10,
            include_str!("../../tests/fixtures/rule34/search.json"),
        )
        .unwrap();
        let image = &batch.posts[0];
        assert_eq!(image.creator.as_deref(), Some("artist_name"));
        assert!(image
            .tags
            .contains(&CanonicalTag::new("creator", "artist_name")));
        assert!(image
            .tags
            .contains(&CanonicalTag::new("character", "character_name")));
        assert!(image
            .tags
            .contains(&CanonicalTag::new("series", "series_name")));
        assert!(image.tags.contains(&CanonicalTag::new("", "highres")));
        assert_eq!(
            image.media[0].url,
            "https://wimg.rule34.xxx/images/ab/cd/hash.jpg"
        );
        assert_eq!(image.resume_cursor_after.as_deref(), Some("b987654"));

        let video = &batch.posts[1];
        assert_eq!(
            video.media[0].url,
            "https://api-cdn.rule34.xxx/images/ef/video.mp4"
        );
        assert_eq!(video.resume_cursor_after.as_deref(), Some("b987653"));
    }
}
