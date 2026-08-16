mod gallery_dl;
mod types;

use serde::Serialize;
use url::Url;

use crate::subscriptions::gallery_dl_runner::{
    build_url, normalize_baraag_username, normalize_furaffinity_username, normalize_tumblr_blog,
    normalize_webtoons_url, site_by_id, SiteEntry,
};

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
    match site_by_id(site_id) {
        Some(site) if site.supports_account => "user",
        _ => "search",
    }
}

/// Normalize account input while leaving booru tag queries unchanged.
pub fn normalize_query_text(site_id: &str, query_kind: &str, raw: &str) -> String {
    let trimmed = raw.trim();
    if site_id == "webtoons" && query_kind == "user" {
        return normalize_webtoons_url(trimmed).unwrap_or_else(|_| trimmed.to_string());
    }
    if site_id == "artstation" && query_kind == "user" {
        if let Ok(url) = Url::parse(trimmed) {
            let is_artstation_host = url
                .host_str()
                .is_some_and(|host| host == "artstation.com" || host.ends_with(".artstation.com"));
            let segments: Vec<_> = url
                .path_segments()
                .into_iter()
                .flatten()
                .filter(|segment| !segment.is_empty())
                .collect();
            if is_artstation_host
                && segments.len() == 1
                && url.query().is_none()
                && url.fragment().is_none()
            {
                return segments[0].to_string();
            }
        }
        return trimmed.strip_prefix('@').unwrap_or(trimmed).to_string();
    }
    if site_id == "hentaifoundry" && query_kind == "user" {
        return build_url("hentaifoundry", trimmed)
            .and_then(|url| {
                Url::parse(&url)
                    .ok()?
                    .path_segments()?
                    .filter(|segment| !segment.is_empty())
                    .next_back()
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| trimmed.to_string());
    }
    if site_id == "baraag" && query_kind == "user" {
        return normalize_baraag_username(trimmed).unwrap_or_else(|_| trimmed.to_string());
    }
    if site_id == "deviantart" && query_kind == "user" {
        return build_url("deviantart", trimmed)
            .and_then(|url| {
                Url::parse(&url)
                    .ok()?
                    .path_segments()?
                    .filter(|segment| !segment.is_empty())
                    .next()
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| trimmed.to_string());
    }
    if site_id == "tumblr" && query_kind == "user" {
        return normalize_tumblr_blog(trimmed).unwrap_or_else(|_| trimmed.to_string());
    }
    if site_id == "furaffinity" && query_kind == "user" {
        return normalize_furaffinity_username(trimmed).unwrap_or_else(|_| trimmed.to_string());
    }
    if site_id == "pixivuser" && query_kind == "user" {
        if let Ok(url) = Url::parse(trimmed) {
            let segments: Vec<_> = url
                .path_segments()
                .into_iter()
                .flatten()
                .filter(|segment| !segment.is_empty())
                .collect();
            if url
                .host_str()
                .is_some_and(|host| host.ends_with("pixiv.net"))
            {
                if let Some(index) = segments.iter().position(|segment| *segment == "users") {
                    if let Some(user_id) = segments.get(index + 1) {
                        return (*user_id).to_string();
                    }
                }
            }
        }
        return trimmed.strip_prefix('@').unwrap_or(trimmed).to_string();
    }
    trimmed.to_string()
}

/// Validate the user-authored part of a subscription query. Pagination and
/// ordering belong to Picto so interrupted runs can resume deterministically.
pub fn validate_query_text(site_id: &str, query_text: &str) -> Result<(), String> {
    let query_text = query_text.trim();
    if query_text.is_empty() {
        return Err("Subscription query cannot be empty".to_string());
    }
    if site_id == "webtoons" {
        normalize_webtoons_url(query_text)?;
    }
    if site_id == "pixivuser"
        && !query_text
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err("Pixiv user subscriptions require a numeric user ID".to_string());
    }
    if site_id == "artstation"
        && (!query_text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || matches!(query_text, "artwork" | "projects" | "search"))
    {
        return Err("ArtStation subscriptions require a profile slug".to_string());
    }
    if site_id == "hentaifoundry" {
        if build_url("hentaifoundry", query_text).is_none() {
            return Err(
                "Hentai Foundry subscriptions require a safe username slug or canonical user URL"
                    .to_string(),
            );
        }
    }
    if site_id == "baraag" {
        normalize_baraag_username(query_text)?;
    }
    if site_id == "deviantart" && build_url("deviantart", query_text).is_none() {
        return Err(
            "DeviantArt subscriptions require a safe username or canonical profile/gallery URL"
                .to_string(),
        );
    }
    if site_id == "tumblr" {
        normalize_tumblr_blog(query_text)?;
    }
    if site_id == "furaffinity" {
        normalize_furaffinity_username(query_text)?;
    }
    if matches!(
        site_id,
        "gelbooru"
            | "rule34"
            | "danbooru"
            | "yandere"
            | "konachan"
            | "safebooru"
            | "e621"
            | "idolcomplex"
            | "sankaku"
    ) {
        if let Some(token) = query_text.split_whitespace().find(|token| {
            let token = token.to_ascii_lowercase();
            token.starts_with("id:")
                || token.starts_with("id_range:")
                || token.starts_with("order:")
                || token.starts_with("sort:")
        }) {
            return Err(format!(
                "Subscription query cannot control pagination or ordering: {token}"
            ));
        }
    }
    Ok(())
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
        .unwrap_or_else(|| site_id.to_string())
}

pub fn validate_query_kind(site_id: &str, query_kind: &str) -> Result<(), String> {
    let descriptor = describe_site(site_id).ok_or_else(|| format!("Unknown site: {site_id}"))?;
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
        site_id: site.id.to_string(),
        runner_key: runner_key_for_site(site.id),
        display_name: site.name.to_string(),
        auth_supported: site.auth_supported,
        auth_required_for_full_access: site.auth_required_for_full_access,
        query_kinds: query_kinds_for_site(site),
    })
}

fn query_kinds_for_site(site: &SiteEntry) -> Vec<SiteQueryKind> {
    if site.supports_account {
        vec![SiteQueryKind {
            id: "user",
            label: "User",
        }]
    } else {
        vec![SiteQueryKind {
            id: "search",
            label: "Search",
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        describe_site, infer_query_kind, resolve_query_kind, validate_query_kind,
        validate_query_text,
    };

    #[test]
    fn resolve_query_kind_uses_site_default_when_missing() {
        assert_eq!(resolve_query_kind("pixivuser", None), "user");
        assert_eq!(resolve_query_kind("pixiv", Some("   ")), "search");
        assert_eq!(resolve_query_kind("gelbooru", None), "search");
    }

    #[test]
    fn normalize_query_text_handles_pixiv_user_inputs() {
        use super::normalize_query_text;
        assert_eq!(normalize_query_text("pixivuser", "user", "@name"), "name");
        assert_eq!(
            normalize_query_text("pixivuser", "user", "  name  "),
            "name"
        );
        assert_eq!(
            normalize_query_text("gelbooru", "search", "1girl rating:safe"),
            "1girl rating:safe"
        );
        assert_eq!(
            normalize_query_text(
                "pixivuser",
                "user",
                "https://www.pixiv.net/en/users/173530/artworks"
            ),
            "173530"
        );
        assert_eq!(
            normalize_query_text(
                "artstation",
                "user",
                "https://www.artstation.com/artist-name"
            ),
            "artist-name"
        );
        assert_eq!(
            normalize_query_text("artstation", "user", "@artist-name"),
            "artist-name"
        );
        assert_eq!(
            normalize_query_text(
                "hentaifoundry",
                "user",
                "https://www.hentai-foundry.com/user/artist-name/profile"
            ),
            "artist-name"
        );
        assert_eq!(
            normalize_query_text(
                "webtoons",
                "user",
                "http://webtoons.com/en/fantasy/title/list?title_no=123&page=2#episode"
            ),
            "https://www.webtoons.com/en/fantasy/title/list?title_no=123"
        );
        assert_eq!(
            normalize_query_text("baraag", "user", "https://baraag.net/@Blue_/media"),
            "Blue_"
        );
        assert_eq!(
            normalize_query_text(
                "deviantart",
                "user",
                "https://www.deviantart.com/artist-name/gallery/"
            ),
            "artist-name"
        );
        assert_eq!(
            normalize_query_text("tumblr", "user", "https://nasa.tumblr.com/"),
            "nasa"
        );
    }

    #[test]
    fn query_text_reserves_pagination_and_ordering_for_the_runner() {
        assert!(validate_query_text("gelbooru", "1girl rating:safe").is_ok());
        assert!(validate_query_text("rule34", "1girl rating:safe").is_ok());
        assert!(validate_query_text("rule34", "1girl id:<123").is_err());
        assert!(validate_query_text("danbooru", "1girl id:<123").is_err());
        assert!(validate_query_text("gelbooru", "1girl sort:id:asc").is_err());
        assert!(validate_query_text("danbooru", "1girl order:id_asc").is_err());
        assert!(validate_query_text("idolcomplex", "solo").is_ok());
        assert!(validate_query_text("idolcomplex", "solo id_range:123").is_err());
        assert!(validate_query_text("sankaku", "solo rating:safe").is_ok());
        assert!(validate_query_text("sankaku", "solo order:random").is_err());
        assert!(validate_query_text("pixivuser", "173530").is_ok());
        assert!(validate_query_text("pixivuser", "artist-name").is_err());
        assert!(validate_query_text("artstation", "artist-name").is_ok());
        assert!(validate_query_text("artstation", "artist/name").is_err());
        assert!(validate_query_text("artstation", "projects").is_err());
        assert!(validate_query_text("hentaifoundry", "artist-name").is_ok());
        assert!(validate_query_text(
            "hentaifoundry",
            "https://www.hentai-foundry.com/pictures/user/artist-name"
        )
        .is_ok());
        assert!(validate_query_text("hentaifoundry", "artist/name").is_err());
        assert!(validate_query_text("baraag", "@Blue_").is_ok());
        assert!(validate_query_text("baraag", "https://baraag.net/@Blue_/media").is_ok());
        assert!(validate_query_text("baraag", "https://example.com/@Blue_").is_err());
        assert!(validate_query_text("deviantart", "artist-name").is_ok());
        assert!(validate_query_text(
            "deviantart",
            "https://www.deviantart.com/artist-name/gallery/"
        )
        .is_ok());
        for value in [
            "artist/name",
            "https://example.com/artist-name/gallery/",
            "https://www.deviantart.com/artist-name/gallery/?page=2",
            "https://www.deviantart.com/artist-name/gallery/post",
        ] {
            assert!(
                validate_query_text("deviantart", value).is_err(),
                "accepted {value}"
            );
        }
        assert!(validate_query_text("tumblr", "nasa").is_ok());
        assert!(validate_query_text("tumblr", "https://www.tumblr.com/nasa").is_ok());
        for value in [
            "nasa/posts",
            "https://tumblr.com.evil.example/nasa",
            "https://www.tumblr.com/nasa/123",
            "https://www.tumblr.com/nasa?ref=feed",
        ] {
            assert!(
                validate_query_text("tumblr", value).is_err(),
                "accepted {value}"
            );
        }
        assert!(validate_query_text(
            "hentaifoundry",
            "https://www.hentai-foundry.com/pictures/user/artist-name?x=1"
        )
        .is_err());
        assert!(validate_query_text(
            "webtoons",
            "https://www.webtoons.com/en/fantasy/title/list?title_no=123"
        )
        .is_ok());
        assert!(validate_query_text("webtoons", "https://example.com/list?title_no=123").is_err());
        assert!(validate_query_text("pixiv", "  ").is_err());
    }

    #[test]
    fn inferred_kind_is_valid_for_every_site() {
        for site in crate::subscriptions::gallery_dl_runner::SITES {
            assert_ne!(
                site.supports_query, site.supports_account,
                "site '{}' must expose exactly one query behavior",
                site.id
            );
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
    fn unsupported_sites_have_no_descriptor() {
        assert_eq!(infer_query_kind("danbooru"), "search");
        assert_eq!(infer_query_kind("artstation"), "user");
        assert!(describe_site("artstation")
            .expect("ArtStation descriptor")
            .query_kinds
            .iter()
            .any(|kind| kind.id == "user"));
        assert_eq!(infer_query_kind("webtoons"), "user");
        assert_eq!(describe_site("webtoons").unwrap().display_name, "Webtoons");
    }

    #[test]
    fn public_booru_sources_are_search_only_and_reserve_order_tokens() {
        for site_id in ["rule34", "yandere", "konachan", "safebooru", "e621"] {
            assert_eq!(infer_query_kind(site_id), "search");
            assert!(validate_query_kind(site_id, "search").is_ok());
            assert!(validate_query_kind(site_id, "user").is_err());
            assert!(validate_query_text(site_id, "artist solo").is_ok());
            assert!(validate_query_text(site_id, "artist id:<123").is_err());
            assert!(validate_query_text(site_id, "artist order:id_asc").is_err());
            assert!(validate_query_text(site_id, "artist sort:id:asc").is_err());
        }
    }
}
