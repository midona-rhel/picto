mod gallery_dl;
mod types;

use serde::Serialize;

use crate::subscriptions::gallery_dl_runner::{canonical_site_id, site_by_id, SiteEntry};

pub use gallery_dl::{GalleryDlSourceAdapter, SubscriptionSourceAdapter};
pub use types::{
    AssetMetadata, DownloadedItem, FailedDownloadedItem, ParsedMetadata, PostMetadata,
    SourceRunEvent,
};

#[derive(Debug, Clone, Serialize)]
pub struct SiteQueryKind {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteAdapterDescriptor {
    pub site_id: String,
    pub runner_key: String,
    pub display_name: String,
    pub auth_supported: bool,
    pub auth_required_for_full_access: bool,
    pub query_kinds: Vec<SiteQueryKind>,
}

pub fn infer_query_kind(site_id: &str) -> &'static str {
    match canonical_site_id(site_id) {
        "pixivuser" | "furaffinity" | "instagram" | "twitter" | "deviantart" | "artstation" => {
            "user"
        }
        "patreon" | "fanbox" => "creator",
        "fantia" => "fanclub",
        "tumblr" => "blog",
        _ => "search",
    }
}

pub fn resolve_query_kind(site_id: &str, query_kind: Option<&str>) -> String {
    query_kind
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| infer_query_kind(site_id))
        .to_string()
}

pub fn runner_key_for_site(site_id: &str) -> String {
    site_by_id(site_id)
        .map(|site| site.credential_owner_site_id.to_string())
        .unwrap_or_else(|| canonical_site_id(site_id).to_string())
}

pub fn validate_query_kind(site_id: &str, query_kind: &str) -> Result<(), String> {
    let descriptor = describe_site(site_id)
        .ok_or_else(|| format!("Unknown site: {}", canonical_site_id(site_id)))?;
    if descriptor.query_kinds.iter().any(|kind| kind.id == query_kind) {
        return Ok(());
    }
    Err(format!(
        "Unsupported query kind '{}' for site '{}'",
        query_kind, descriptor.site_id
    ))
}

pub fn describe_site(site_id: &str) -> Option<SiteAdapterDescriptor> {
    let site = site_by_id(site_id)?;
    Some(SiteAdapterDescriptor {
        site_id: canonical_site_id(site.id).to_string(),
        runner_key: runner_key_for_site(site.id),
        display_name: site.name.to_string(),
        auth_supported: site.auth_supported,
        auth_required_for_full_access: site.auth_required_for_full_access,
        query_kinds: query_kinds_for_site(site),
    })
}

fn query_kinds_for_site(site: &SiteEntry) -> Vec<SiteQueryKind> {
    let canonical = canonical_site_id(site.id);
    match canonical {
        "patreon" | "fanbox" => vec![SiteQueryKind {
            id: "creator",
            label: "Creator",
        }],
        "fantia" => vec![SiteQueryKind {
            id: "fanclub",
            label: "Fanclub",
        }],
        "tumblr" => vec![SiteQueryKind {
            id: "blog",
            label: "Blog",
        }],
        _ if site.supports_query && site.supports_account => vec![
            SiteQueryKind {
                id: "search",
                label: "Search",
            },
            SiteQueryKind {
                id: "user",
                label: "User",
            },
        ],
        _ if site.supports_account && !site.supports_query => vec![SiteQueryKind {
            id: "user",
            label: "User",
        }],
        _ => vec![SiteQueryKind {
            id: "search",
            label: "Search",
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::{describe_site, infer_query_kind, resolve_query_kind, validate_query_kind};

    #[test]
    fn resolve_query_kind_uses_site_default_when_missing() {
        assert_eq!(resolve_query_kind("pixivuser", None), "user");
        assert_eq!(resolve_query_kind("patreon", Some("   ")), "creator");
        assert_eq!(resolve_query_kind("gelbooru", None), "search");
    }

    #[test]
    fn validate_query_kind_matches_site_descriptor() {
        let pixiv = describe_site("pixiv").expect("pixiv descriptor");
        assert!(pixiv.query_kinds.iter().any(|kind| kind.id == "search"));
        assert!(pixiv.query_kinds.iter().any(|kind| kind.id == "user"));
        assert!(validate_query_kind("pixiv", "search").is_ok());
        assert!(validate_query_kind("pixiv", "user").is_ok());
        assert!(validate_query_kind("pixiv", "creator").is_err());
    }

    #[test]
    fn infer_query_kind_keeps_existing_site_defaults() {
        assert_eq!(infer_query_kind("furaffinity"), "user");
        assert_eq!(infer_query_kind("fanbox"), "creator");
        assert_eq!(infer_query_kind("tumblr"), "blog");
    }
}
