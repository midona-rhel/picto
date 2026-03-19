//! Post-download tag enrichment for sites that return flat tag strings.
//!
//! Rule34/Gelbooru gallery-dl output has a flat `tags` string with no category
//! information. This module queries the site's tag API to resolve each tag's
//! category (artist, character, copyright, general, meta) and rewrites them
//! into namespaced (namespace, subtag) pairs.

use std::collections::HashMap;

use tracing::{debug, warn};

use super::ParsedMetadata;

/// Gelbooru/Rule34 tag type IDs → our namespace convention.
fn gelbooru_type_to_namespace(tag_type: u8) -> &'static str {
    match tag_type {
        1 => "creator",    // artist
        3 => "series",     // copyright
        4 => "character",  // character
        5 => "meta",       // metadata
        _ => "",           // 0 = general, anything else = general
    }
}

/// Enrich flat tags for Rule34/Gelbooru items by querying the tag API.
///
/// - Collects all unique tags across all items
/// - Queries the tag API one-by-one with rate limiting
/// - Caches results so each tag is queried at most once
/// - Rewrites `metadata.tags` from `("", tag)` to `(namespace, tag)`
pub async fn enrich_gelbooru_tags(
    items: &mut [super::DownloadedItem],
    site_id: &str,
    credential: Option<&crate::credential_store::SiteCredential>,
    sleep_between_requests: std::time::Duration,
) {
    // Collect unique tags that need lookup (only unnamespaced ones)
    let mut unique_tags: Vec<String> = Vec::new();
    for item in items.iter() {
        for (ns, tag) in &item.metadata.tags {
            if ns.is_empty() && !unique_tags.contains(tag) {
                unique_tags.push(tag.clone());
            }
        }
    }

    if unique_tags.is_empty() {
        return;
    }

    debug!(
        site = site_id,
        tag_count = unique_tags.len(),
        "Enriching tags with category info from site API"
    );

    // Resolve the API base URL
    let api_base = match super::sites::canonical_site_id(site_id) {
        "rule34" => "https://api.rule34.xxx/index.php",
        "gelbooru" => "https://gelbooru.com/index.php",
        "safebooru" => "https://safebooru.org/index.php",
        other => {
            debug!(site = other, "No tag enrichment API for this site");
            return;
        }
    };

    // Build auth query params
    let auth_params = build_auth_params(credential);

    // Query each tag and build a type cache
    let client = reqwest::Client::builder()
        .user_agent("PictoApp/1.0 (tag-enrichment)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let mut type_cache: HashMap<String, u8> = HashMap::new();

    for (i, tag) in unique_tags.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(sleep_between_requests).await;
        }

        let encoded_tag: String = tag
            .bytes()
            .flat_map(|b| {
                if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                    vec![b as char]
                } else {
                    format!("%{:02X}", b).chars().collect()
                }
            })
            .collect();
        let url = format!(
            "{}?page=dapi&s=tag&q=index&name={}&limit=1{}",
            api_base,
            encoded_tag,
            auth_params,
        );

        match client.get(&url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(xml) => {
                    if let Some(tag_type) = parse_tag_type_from_xml(&xml, tag) {
                        type_cache.insert(tag.clone(), tag_type);
                    }
                }
                Err(e) => warn!(tag = tag.as_str(), error = %e, "Failed to read tag API response"),
            },
            Err(e) => warn!(tag = tag.as_str(), error = %e, "Failed to query tag API"),
        }
    }

    debug!(
        resolved = type_cache.len(),
        total = unique_tags.len(),
        "Tag enrichment complete"
    );

    // Rewrite tags in all items
    for item in items.iter_mut() {
        enrich_metadata_tags(&mut item.metadata, &type_cache);
    }
}

/// Rewrite flat tags in a single metadata using the resolved type cache.
fn enrich_metadata_tags(metadata: &mut ParsedMetadata, type_cache: &HashMap<String, u8>) {
    for (ns, tag) in metadata.tags.iter_mut() {
        if ns.is_empty() {
            if let Some(&tag_type) = type_cache.get(tag.as_str()) {
                let resolved_ns = gelbooru_type_to_namespace(tag_type);
                *ns = resolved_ns.to_string();
            }
        }
    }
}

/// Parse `<tag type="N" name="..." />` from Gelbooru-style XML response.
fn parse_tag_type_from_xml(xml: &str, expected_name: &str) -> Option<u8> {
    // Simple regex-free parsing: find `name="expected_name"` and extract `type="N"`
    // Format: <tag type="1" count="196" name="observerdoz" ambiguous="false" id="1167382"/>
    for segment in xml.split("<tag ") {
        if !segment.contains(&format!("name=\"{}\"", expected_name)) {
            continue;
        }
        // Extract type="N"
        if let Some(type_start) = segment.find("type=\"") {
            let rest = &segment[type_start + 6..];
            if let Some(type_end) = rest.find('"') {
                if let Ok(t) = rest[..type_end].parse::<u8>() {
                    return Some(t);
                }
            }
        }
    }
    None
}

fn build_auth_params(credential: Option<&crate::credential_store::SiteCredential>) -> String {
    let Some(cred) = credential else {
        return String::new();
    };
    match cred.credential_type {
        crate::credential_store::CredentialType::ApiKey => {
            let api_key = cred.password.as_deref().unwrap_or("");
            let user_id = cred.username.as_deref().unwrap_or("");
            if !api_key.is_empty() && !user_id.is_empty() {
                format!("&api_key={}&user_id={}", api_key, user_id)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_type_from_xml_extracts_correct_type() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><tags type="array"><tag type="1" count="196" name="observerdoz" ambiguous="false" id="1167382"/></tags>"#;
        assert_eq!(parse_tag_type_from_xml(xml, "observerdoz"), Some(1));
    }

    #[test]
    fn parse_tag_type_from_xml_handles_multiple_tags() {
        let xml = r#"<tags type="array"><tag type="4" count="62" name="stigmata_(observerdoz)" ambiguous="false" id="2353089"/><tag type="1" count="196" name="observerdoz" ambiguous="false" id="1167382"/></tags>"#;
        assert_eq!(parse_tag_type_from_xml(xml, "observerdoz"), Some(1));
        assert_eq!(parse_tag_type_from_xml(xml, "stigmata_(observerdoz)"), Some(4));
    }

    #[test]
    fn parse_tag_type_from_xml_returns_none_for_missing() {
        let xml = r#"<tags type="array"></tags>"#;
        assert_eq!(parse_tag_type_from_xml(xml, "nonexistent"), None);
    }

    #[test]
    fn gelbooru_type_mapping() {
        assert_eq!(gelbooru_type_to_namespace(0), "");
        assert_eq!(gelbooru_type_to_namespace(1), "creator");
        assert_eq!(gelbooru_type_to_namespace(3), "series");
        assert_eq!(gelbooru_type_to_namespace(4), "character");
        assert_eq!(gelbooru_type_to_namespace(5), "meta");
    }

    #[test]
    fn enrich_metadata_tags_rewrites_namespaces() {
        let mut cache = HashMap::new();
        cache.insert("observerdoz".to_string(), 1u8);
        cache.insert("twokinds".to_string(), 3u8);
        cache.insert("adira_riftwall".to_string(), 4u8);
        cache.insert("hi_res".to_string(), 5u8);
        cache.insert("anthro".to_string(), 0u8);

        let mut metadata = ParsedMetadata {
            tags: vec![
                (String::new(), "observerdoz".to_string()),
                (String::new(), "twokinds".to_string()),
                (String::new(), "adira_riftwall".to_string()),
                (String::new(), "hi_res".to_string()),
                (String::new(), "anthro".to_string()),
                (String::new(), "unknown_tag".to_string()),
            ],
            ..Default::default()
        };

        enrich_metadata_tags(&mut metadata, &cache);

        assert!(metadata.tags.contains(&("creator".to_string(), "observerdoz".to_string())));
        assert!(metadata.tags.contains(&("series".to_string(), "twokinds".to_string())));
        assert!(metadata.tags.contains(&("character".to_string(), "adira_riftwall".to_string())));
        assert!(metadata.tags.contains(&("meta".to_string(), "hi_res".to_string())));
        assert!(metadata.tags.contains(&(String::new(), "anthro".to_string())));
        // Tags not in cache stay unnamespaced
        assert!(metadata.tags.contains(&(String::new(), "unknown_tag".to_string())));
    }
}
