//! Integration tests for gallery-dl site registry.

use picto_core::subscriptions::gallery_dl_runner::{
    self, advertised_sites, site_by_id, ADVERTISED_SITE_IDS, SITES,
};

/// Verify that the site registry covers all expected sites and URLs are valid.
#[test]
fn test_site_registry_coverage() {
    let required = [
        "danbooru",
        "e621",
        "gelbooru",
        "yandere",
        "rule34",
        "pixiv",
        "safebooru",
    ];
    for id in required {
        assert!(
            site_by_id(id).is_some(),
            "Required site '{}' missing from registry",
            id
        );
    }

    for site in SITES {
        let url = gallery_dl_runner::substitute_query(site.url_template, "test_query");
        assert!(
            url.starts_with("https://"),
            "Site '{}' URL should start with https://: {}",
            site.id,
            url,
        );
    }
}

#[test]
fn advertised_sites_are_an_explicit_registry_subset() {
    let advertised: Vec<_> = advertised_sites().map(|site| site.id).collect();
    assert_eq!(advertised, ADVERTISED_SITE_IDS);
    assert!(advertised.iter().all(|id| site_by_id(id).is_some()));
    assert!(site_by_id("e621").is_some());
    assert!(!advertised.contains(&"e621"));
}
