//! Subscription import and metadata policy helpers.

use crate::subscriptions::source_adapter::ParsedMetadata;

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
    // gallery-dl's `num` is 1-based: a single-image post has num=1, count=1.
    // Only a second file (num > 1) or an advertised multi-file count marks a
    // real multi-image post — `page_num > 0` would route every single-image
    // tweet down the collection path.
    let is_multi_post = metadata.page_count.is_some_and(|count| count > 1)
        || metadata.page_num.is_some_and(|page_num| page_num > 1);
    if !is_multi_post {
        return None;
    }

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

pub fn validate_metadata_for_site(
    _site_id: &str,
    _metadata: &ParsedMetadata,
) -> Result<(), String> {
    // Import policy accepts all metadata — the job of the importer is to
    // correctly extract whatever tags/fields the source provides, not to
    // reject posts for missing optional fields like creator or rating.
    Ok(())
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
            page_count: Some(3),
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
            page_count: Some(2),
            ..Default::default()
        };
        let parts = collection_group_parts("pixiv", &metadata).expect("group parts");
        assert_eq!(parts.0, "pixiv");
        assert_eq!(parts.1, "77");
        assert_eq!(parts.2, "Nice title");
    }

    #[test]
    fn collection_group_parts_ignores_single_image_posts() {
        let metadata = ParsedMetadata {
            post_id: Some("42".to_string()),
            category: Some("gelbooru".to_string()),
            page_count: Some(1),
            page_num: Some(0),
            ..Default::default()
        };
        assert!(collection_group_parts("gelbooru", &metadata).is_none());

        // Single-image tweet: gallery-dl numbers files 1-based (num=1, count=1).
        let single_tweet = ParsedMetadata {
            post_id: Some("2078142284424782203".to_string()),
            category: Some("twitter".to_string()),
            page_count: Some(1),
            page_num: Some(1),
            ..Default::default()
        };
        assert!(collection_group_parts("twitter", &single_tweet).is_none());

        // Second file of a post IS a collection candidate.
        let second_file = ParsedMetadata {
            post_id: Some("2078142284424782203".to_string()),
            category: Some("twitter".to_string()),
            page_count: Some(1),
            page_num: Some(2),
            ..Default::default()
        };
        assert!(collection_group_parts("twitter", &second_file).is_some());
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
    fn validate_metadata_accepts_all_sites() {
        let minimal = ParsedMetadata {
            post_id: Some("42".to_string()),
            tags: vec![(String::new(), "1girl".to_string())],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("pixiv", &minimal).is_ok());
        assert!(validate_metadata_for_site("danbooru", &minimal).is_ok());
        assert!(validate_metadata_for_site("gelbooru", &minimal).is_ok());
        assert!(validate_metadata_for_site("unknown_site", &minimal).is_ok());
    }
}
