use crate::NativeSourceAdapter;

use super::kemono::{archive_adapter, ArchiveProvider};

const COOMER: ArchiveProvider = ArchiveProvider {
    id: "coomer",
    display_name: "Coomer",
    domain: "coomer.st",
    query_domains: &["coomer.st", "www.coomer.st"],
    site_root: "https://coomer.st",
    api_root: "https://coomer.st/api/v1",
    media_root: "https://coomer.st",
    creator_posts_suffix: "/posts",
    accept: "text/css",
    file_first: true,
};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    archive_adapter(COOMER)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::providers::kemono::{
        decode_cursor, normalize_creator_query, normalize_detail, normalize_page, CursorState,
    };
    use crate::{CanonicalTag, DiscoveryRequest, SourcePartition};

    fn request(cursor: Option<String>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "fansly:800702847065796609".to_string(),
            partition: SourcePartition::new("posts"),
            cursor,
            page_size: 1,
        }
    }

    #[test]
    fn remains_a_distinct_anonymous_provider_with_strict_queries() {
        let descriptor = adapter().descriptor();
        assert_eq!(descriptor.id, "coomer");
        assert_eq!(descriptor.domain, "coomer.st");
        assert!(descriptor.anonymous);
        assert!(normalize_creator_query(
            COOMER,
            "https://coomer.st/fansly/user/800702847065796609"
        )
        .is_ok());
        assert!(normalize_creator_query(
            COOMER,
            "https://kemono.cr/fansly/user/800702847065796609"
        )
        .is_err());
    }

    #[test]
    fn paginates_one_post_and_maps_complete_detail() {
        let creator = normalize_creator_query(COOMER, &request(None).query).unwrap();
        let page: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/coomer/page.json")).unwrap();
        let first = normalize_page(
            COOMER,
            &request(None),
            &creator,
            CursorState {
                offset: 0,
                index: 0,
            },
            page.clone(),
        )
        .unwrap();
        assert_eq!(first.posts.len(), 1);
        let cursor = first.posts[0].resume_cursor_after.clone().unwrap();
        let state = decode_cursor(COOMER, &creator, Some(&cursor)).unwrap();
        let second = normalize_page(COOMER, &request(Some(cursor)), &creator, state, page).unwrap();
        assert_eq!(second.posts.len(), 1);
        assert!(second.exhausted);

        let detail: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/coomer/post.json")).unwrap();
        let profile: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/coomer/profile.json")).unwrap();
        let post = normalize_detail(
            COOMER,
            first.posts.into_iter().next().unwrap(),
            &creator,
            detail,
            profile,
        )
        .unwrap();
        assert_eq!(post.creator.as_deref(), Some("TinyMiracle"));
        assert_eq!(post.media.len(), 2);
        assert!(post.tags.contains(&CanonicalTag::new("", "young")));
        assert!(post.tags.contains(&CanonicalTag::new("rating", "explicit")));
    }
}
