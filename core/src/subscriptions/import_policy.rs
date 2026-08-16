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

pub fn individual_import_metadata(metadata: &ParsedMetadata) -> &ParsedMetadata {
    metadata
}

pub fn should_replace_existing_name(existing_name: &str, metadata: &ParsedMetadata) -> bool {
    let trimmed = existing_name.trim();
    trimmed.is_empty() || is_generated_subscription_name(trimmed, metadata)
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
    fn individual_import_metadata_names_multi_image_pages_consistently() {
        let metadata = ParsedMetadata {
            category: Some("pixiv".to_string()),
            post_id: Some("42".to_string()),
            title: Some("Artwork".to_string()),
            page_count: Some(3),
            page_num: Some(1),
            ..Default::default()
        };

        assert_eq!(
            individual_import_metadata(&metadata).title.as_deref(),
            Some("Artwork")
        );
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
    fn validate_metadata_accepts_supported_sites() {
        let minimal = ParsedMetadata {
            post_id: Some("42".to_string()),
            tags: vec![(String::new(), "1girl".to_string())],
            ..Default::default()
        };
        assert!(validate_metadata_for_site("pixiv", &minimal).is_ok());
        assert!(validate_metadata_for_site("danbooru", &minimal).is_ok());
        assert!(validate_metadata_for_site("gelbooru", &minimal).is_ok());
    }
}
