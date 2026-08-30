use crate::NativeSourceAdapter;

use super::archive_feed::{archive_adapter, ArchiveProvider};

const PAWCHIVE: ArchiveProvider = ArchiveProvider {
    id: "pawchive",
    display_name: "Pawchive",
    domain: "pawchive.pw",
    query_domains: &[
        "pawchive.pw",
        "www.pawchive.pw",
        "pawchive.st",
        "www.pawchive.st",
    ],
    site_root: "https://pawchive.pw",
    api_root: "https://pawchive.pw/api/v1",
    media_root: "https://file.pawchive.pw",
    creator_posts_suffix: "",
    accept: "application/json",
    file_first: true,
};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    archive_adapter(PAWCHIVE)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::providers::archive_feed::{
        decode_cursor, normalize_creator_query, normalize_detail, normalize_page, CursorState,
    };
    use crate::{CanonicalTag, DiscoveryRequest, SourcePartition};

    fn request(cursor: Option<String>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "patreon:90730375".to_string(),
            partition: SourcePartition::new("posts"),
            cursor,
            page_size: 1,
        }
    }

    #[test]
    fn accepts_current_and_redirecting_creator_domains_only() {
        let descriptor = adapter().descriptor();
        assert_eq!(descriptor.id, "pawchive");
        assert_eq!(descriptor.domain, "pawchive.pw");
        assert!(descriptor.anonymous);
        assert!(
            normalize_creator_query(PAWCHIVE, "https://pawchive.pw/patreon/user/90730375").is_ok()
        );
        assert!(
            normalize_creator_query(PAWCHIVE, "https://pawchive.st/patreon/user/90730375/").is_ok()
        );
        assert!(
            normalize_creator_query(PAWCHIVE, "https://example.invalid/patreon/user/90730375")
                .is_err()
        );
    }

    #[test]
    fn paginates_one_post_and_maps_pawchive_file_host_media() {
        let creator = normalize_creator_query(PAWCHIVE, &request(None).query).unwrap();
        let page: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/page.json")).unwrap();
        let first = normalize_page(
            PAWCHIVE,
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
        let state = decode_cursor(PAWCHIVE, &creator, Some(&cursor)).unwrap();
        let second =
            normalize_page(PAWCHIVE, &request(Some(cursor)), &creator, state, page).unwrap();
        assert_eq!(second.posts.len(), 1);
        assert!(second.exhausted);

        let detail: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/post.json")).unwrap();
        let profile: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/profile.json"))
                .unwrap();
        let post = normalize_detail(
            PAWCHIVE,
            first.posts.into_iter().next().unwrap(),
            &creator,
            detail,
            profile,
        )
        .unwrap();
        assert_eq!(post.creator.as_deref(), Some("Marinerx Art"));
        assert_eq!(post.media.len(), 4);
        assert!(post.media.iter().any(|media| media.url.ends_with(".zip")));
        assert!(post
            .media
            .iter()
            .all(|media| media.url.starts_with("https://file.pawchive.pw/data/")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("series", "Example Series")));
        assert!(post.tags.contains(&CanonicalTag::new("", "sketch")));
    }

    #[test]
    fn keeps_available_originals_when_the_full_archive_is_pending_and_rejects_previews() {
        let creator = normalize_creator_query(PAWCHIVE, &request(None).query).unwrap();
        let page: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/page.json")).unwrap();
        let source_post = normalize_page(
            PAWCHIVE,
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
        .into_iter()
        .next()
        .unwrap();
        let profile: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/profile.json"))
                .unwrap();

        let mut unavailable: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/post.json")).unwrap();
        unavailable["has_full"] = Value::Bool(false);
        unavailable["preview_state"] = Value::String("pending".into());
        let post = normalize_detail(
            PAWCHIVE,
            source_post.clone(),
            &creator,
            unavailable,
            profile.clone(),
        )
        .unwrap();
        assert!(!post.media.is_empty());
        assert!(post
            .media
            .iter()
            .all(|media| media.url.starts_with("https://file.pawchive.pw/data/")));

        let mut preview: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pawchive/post.json")).unwrap();
        preview["file"] = serde_json::json!({
            "name": "preview.jpg",
            "path": "/aa/bb/preview.jpg",
            "server": "https://img.pawchive.pw"
        });
        let post = normalize_detail(PAWCHIVE, source_post, &creator, preview, profile).unwrap();
        assert!(
            post.media
                .iter()
                .all(|media| !media.url.contains("/thumbnail/")
                    && !media.url.contains("img.pawchive"))
        );
    }
}
