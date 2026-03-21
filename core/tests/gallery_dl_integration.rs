//! Integration tests for gallery-dl site registry.

use picto_core::subscriptions::gallery_dl_runner::{self, site_by_id, SITES};

/// Verify that the site registry covers all expected sites and URLs are valid.
#[test]
fn test_site_registry_coverage() {
    let required = [
        "danbooru", "e621", "gelbooru", "yandere", "rule34", "pixiv", "safebooru",
    ];
    for id in required {
        assert!(site_by_id(id).is_some(), "Required site '{}' missing from registry", id);
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
