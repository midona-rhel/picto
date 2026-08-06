mod gallery_dl;
mod types;

use serde::Serialize;

use crate::subscriptions::gallery_dl_runner::{canonical_site_id, site_by_id, SiteEntry};

pub use gallery_dl::{GalleryDlSourceAdapter, SubscriptionSourceAdapter};
pub use types::{DownloadedItem, FailedDownloadedItem, ParsedMetadata};

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
        other => match site_by_id(other) {
            // Account-only sites (kemono, coomer, pawchive, webtoons, ...)
            // only validate as "user" — "search" would fail validate_query_kind.
            Some(site) if site.supports_account && !site.supports_query => "user",
            _ => "search",
        },
    }
}

/// Normalize user input into the bare token a site's url_template expects:
/// strips a leading '@' for account-style query kinds and extracts the handle
/// from a pasted profile URL for sites where that is unambiguous (twitter/x).
/// Everything else passes through unchanged — booru tag queries legitimately
/// contain ':' and '/' and must never be rewritten.
pub fn normalize_query_text(site_id: &str, query_kind: &str, raw: &str) -> String {
    let trimmed = raw.trim();
    let account_kind = matches!(query_kind, "user" | "creator" | "blog" | "fanclub");

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        if canonical_site_id(site_id) == "twitter" {
            if let Ok(parsed) = url::Url::parse(trimmed) {
                let host = parsed
                    .host_str()
                    .unwrap_or_default()
                    .trim_start_matches("www.")
                    .to_ascii_lowercase();
                if host == "twitter.com" || host == "x.com" {
                    // The handle is the first path segment; trailing segments
                    // like /media or /with_replies fall away with it.
                    let first = parsed
                        .path_segments()
                        .into_iter()
                        .flatten()
                        .find(|segment| !segment.is_empty());
                    if let Some(segment) = first {
                        let handle = segment.trim_start_matches('@');
                        let reserved = [
                            "i",
                            "home",
                            "search",
                            "explore",
                            "notifications",
                            "settings",
                        ];
                        if !handle.is_empty() && !reserved.contains(&handle) {
                            return handle.to_string();
                        }
                    }
                }
            }
        }
        return trimmed.to_string();
    }

    if account_kind {
        return trimmed.strip_prefix('@').unwrap_or(trimmed).to_string();
    }
    trimmed.to_string()
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
    if descriptor
        .query_kinds
        .iter()
        .any(|kind| kind.id == query_kind)
    {
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
    fn normalize_query_text_handles_twitter_inputs() {
        use super::normalize_query_text;
        assert_eq!(normalize_query_text("twitter", "user", "@name"), "name");
        assert_eq!(normalize_query_text("twitter", "user", "  name  "), "name");
        assert_eq!(
            normalize_query_text("twitter", "user", "https://x.com/name?s=21"),
            "name"
        );
        assert_eq!(
            normalize_query_text("twitter", "user", "https://twitter.com/name/media"),
            "name"
        );
        assert_eq!(
            normalize_query_text("twitter", "user", "https://www.x.com/@name"),
            "name"
        );
        // Reserved paths are not handles — conservative pass-through.
        assert_eq!(
            normalize_query_text("twitter", "user", "https://x.com/i/flow/login"),
            "https://x.com/i/flow/login"
        );
        // Booru tag queries are never rewritten.
        assert_eq!(
            normalize_query_text("gelbooru", "search", "1girl rating:safe"),
            "1girl rating:safe"
        );
        // Account-only sites keep path-style queries intact.
        assert_eq!(
            normalize_query_text("pawchive", "user", "fanbox/user/123"),
            "fanbox/user/123"
        );
        // Creator kinds strip a single leading @.
        assert_eq!(
            normalize_query_text("fanbox", "creator", "@creator"),
            "creator"
        );
    }

    #[test]
    fn inferred_kind_is_valid_for_every_site() {
        for site in crate::subscriptions::gallery_dl_runner::SITES {
            let kind = infer_query_kind(site.id);
            assert!(
                validate_query_kind(site.id, kind).is_ok(),
                "site '{}' infers kind '{}' which its descriptor rejects",
                site.id,
                kind
            );
        }
    }

    #[test]
    fn validate_query_kind_matches_site_descriptor() {
        let pixiv = describe_site("pixiv").expect("pixiv descriptor");
        let pixiv_user = describe_site("pixivuser").expect("pixiv user descriptor");
        assert!(pixiv.query_kinds.iter().any(|kind| kind.id == "search"));
        assert!(!pixiv.query_kinds.iter().any(|kind| kind.id == "user"));
        assert!(pixiv_user.query_kinds.iter().any(|kind| kind.id == "user"));
        assert!(!pixiv_user
            .query_kinds
            .iter()
            .any(|kind| kind.id == "search"));
        assert!(validate_query_kind("pixiv", "search").is_ok());
        assert!(validate_query_kind("pixiv", "user").is_err());
        assert!(validate_query_kind("pixivuser", "user").is_ok());
        assert!(validate_query_kind("pixiv", "creator").is_err());
    }

    #[test]
    fn pixiv_user_and_search_build_their_specific_artwork_urls() {
        assert_eq!(
            crate::subscriptions::gallery_dl_runner::build_url("pixivuser", "1234").as_deref(),
            Some("https://www.pixiv.net/en/users/1234/artworks")
        );
        assert_eq!(
            crate::subscriptions::gallery_dl_runner::build_url("pixiv", "landscape").as_deref(),
            Some("https://www.pixiv.net/en/tags/landscape/artworks?s_mode=s_tag")
        );
    }

    #[test]
    fn infer_query_kind_keeps_existing_site_defaults() {
        assert_eq!(infer_query_kind("furaffinity"), "user");
        assert_eq!(infer_query_kind("fanbox"), "creator");
        assert_eq!(infer_query_kind("tumblr"), "blog");
    }
}
