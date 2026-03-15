//! Subscription import and metadata policy helpers.

use crate::subscriptions::gallery_dl_runner::ParsedMetadata;

pub fn normalized_title(metadata: &ParsedMetadata) -> Option<String> {
    metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

pub fn generated_subscription_name(metadata: &ParsedMetadata) -> Option<String> {
    match (metadata.category.as_deref(), metadata.post_id.as_deref()) {
        (Some(category), Some(post_id)) => {
            let category = category.trim();
            let post_id = post_id.trim();
            if category.is_empty() || post_id.is_empty() {
                None
            } else {
                Some(format!("{category}_{post_id}"))
            }
        }
        _ => None,
    }
}

pub fn preferred_import_name(metadata: &ParsedMetadata) -> Option<String> {
    normalized_title(metadata).or_else(|| generated_subscription_name(metadata))
}

pub fn should_replace_existing_name(existing_name: &str, metadata: &ParsedMetadata) -> bool {
    let trimmed = existing_name.trim();
    trimmed.is_empty() || is_generated_subscription_name(trimmed, metadata)
}

pub fn collection_group_parts(
    site_id: &str,
    metadata: &ParsedMetadata,
) -> Option<(String, String, String)> {
    let post_id = metadata.post_id.as_deref()?.trim().to_string();
    if post_id.is_empty() {
        return None;
    }

    let category = metadata
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(site_id)
        .to_string();
    if category.is_empty() {
        return None;
    }

    let preferred_name =
        preferred_import_name(metadata).unwrap_or_else(|| format!("{category}_{post_id}"));
    Some((category, post_id, preferred_name))
}

pub fn validate_metadata_for_site(site_id: &str, metadata: &ParsedMetadata) -> Result<(), String> {
    match crate::subscriptions::gallery_dl_runner::canonical_site_id(site_id) {
        "pixiv" | "pixivuser" => {
            if metadata
                .post_id
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing remote post id".to_string());
            }
            if metadata
                .source_url
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing source url".to_string());
            }
            let has_title_or_description = metadata
                .title
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
                || metadata
                    .description
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|v| !v.is_empty());
            if !has_title_or_description {
                return Err("missing title/description".to_string());
            }
            if metadata.tags.is_empty() {
                return Err("missing tags".to_string());
            }
            let has_creator = metadata
                .tags
                .iter()
                .any(|(ns, subtag)| ns == "creator" && !subtag.trim().is_empty());
            if !has_creator {
                return Err("missing creator".to_string());
            }
            Ok(())
        }
        "gelbooru" => {
            if metadata
                .post_id
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing remote post id".to_string());
            }
            if metadata
                .source_url
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing source url".to_string());
            }
            if metadata.tags.is_empty() {
                return Err("missing tags".to_string());
            }
            if metadata
                .rating
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing rating".to_string());
            }
            Ok(())
        }
        "danbooru" => {
            if metadata
                .post_id
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing remote post id".to_string());
            }
            if metadata
                .source_url
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing source url".to_string());
            }
            if metadata.tags.is_empty() {
                return Err("missing tags".to_string());
            }
            if metadata
                .rating
                .as_deref()
                .map(str::trim)
                .map_or(true, |v| v.is_empty())
            {
                return Err("missing rating".to_string());
            }
            let has_creator = metadata
                .tags
                .iter()
                .any(|(ns, subtag)| ns == "creator" && !subtag.trim().is_empty());
            if !has_creator {
                return Err("missing creator".to_string());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn is_generated_subscription_name(name: &str, metadata: &ParsedMetadata) -> bool {
    generated_subscription_name(metadata)
        .as_deref()
        .is_some_and(|generated| generated == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_group_parts_uses_category_and_post_id() {
        let metadata = ParsedMetadata {
            post_id: Some("1234".to_string()),
            category: Some("danbooru".to_string()),
            ..Default::default()
        };
        let parts = collection_group_parts("ignored", &metadata).expect("group parts");
        assert_eq!(parts.0, "danbooru");
        assert_eq!(parts.1, "1234");
        assert_eq!(parts.2, "danbooru_1234");
    }

    #[test]
    fn collection_group_parts_falls_back_to_site_id_and_title() {
        let metadata = ParsedMetadata {
            post_id: Some("77".to_string()),
            title: Some("  Nice title  ".to_string()),
            ..Default::default()
        };
        let parts = collection_group_parts("pixiv", &metadata).expect("group parts");
        assert_eq!(parts.0, "pixiv");
        assert_eq!(parts.1, "77");
        assert_eq!(parts.2, "Nice title");
    }

    #[test]
    fn merge_policy_replaces_generated_name_with_real_title() {
        let metadata = ParsedMetadata {
            category: Some("danbooru".to_string()),
            post_id: Some("42".to_string()),
            title: Some("Real source title".to_string()),
            ..Default::default()
        };
        assert!(should_replace_existing_name("danbooru_42", &metadata));
        assert_eq!(
            preferred_import_name(&metadata).as_deref(),
            Some("Real source title")
        );
    }

    #[test]
    fn merge_policy_preserves_user_assigned_name() {
        let metadata = ParsedMetadata {
            category: Some("danbooru".to_string()),
            post_id: Some("42".to_string()),
            title: Some("Real source title".to_string()),
            ..Default::default()
        };
        assert!(!should_replace_existing_name("My custom label", &metadata));
    }

    #[test]
    fn pixiv_validation_requires_creator_and_source_url() {
        let missing = ParsedMetadata {
            post_id: Some("42".to_string()),
            title: Some("Pixiv title".to_string()),
            tags: vec![(String::new(), "tag".to_string())],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("pixiv", &missing).is_err());

        let valid = ParsedMetadata {
            post_id: Some("42".to_string()),
            title: Some("Pixiv title".to_string()),
            source_url: Some("https://www.pixiv.net/artworks/42".to_string()),
            tags: vec![
                (String::new(), "tag".to_string()),
                ("creator".to_string(), "artist".to_string()),
            ],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("pixiv", &valid).is_ok());
    }

    #[test]
    fn gelbooru_validation_requires_rating_and_source_url() {
        let missing = ParsedMetadata {
            post_id: Some("42".to_string()),
            tags: vec![(String::new(), "1girl".to_string())],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("gelbooru", &missing).is_err());

        let valid = ParsedMetadata {
            post_id: Some("42".to_string()),
            source_url: Some("https://gelbooru.com/images/abc.jpg".to_string()),
            rating: Some("safe".to_string()),
            tags: vec![(String::new(), "1girl".to_string())],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("gelbooru", &valid).is_ok());
    }

    #[test]
    fn danbooru_validation_requires_creator_and_rating() {
        let missing = ParsedMetadata {
            post_id: Some("42".to_string()),
            source_url: Some("https://danbooru.donmai.us/posts/42".to_string()),
            tags: vec![(String::new(), "1girl".to_string())],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("danbooru", &missing).is_err());

        let valid = ParsedMetadata {
            post_id: Some("42".to_string()),
            source_url: Some("https://danbooru.donmai.us/posts/42".to_string()),
            rating: Some("s".to_string()),
            tags: vec![
                (String::new(), "1girl".to_string()),
                ("creator".to_string(), "artist_name".to_string()),
            ],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("danbooru", &valid).is_ok());
    }
}
