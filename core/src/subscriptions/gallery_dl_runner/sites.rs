use serde::Serialize;
use url::Url;

#[derive(Debug, Clone, Serialize)]
pub struct SiteEntry {
    /// Internal identifier (gallery-dl category name).
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Canonical site domain shown in UI and used as credential fallback key.
    pub domain: &'static str,
    /// Site category that owns stored credentials for this source.
    pub credential_owner_site_id: &'static str,
    /// Example query to show in the UI.
    pub example_query: &'static str,
    /// Whether this source supports tag/text query style URLs.
    pub supports_query: bool,
    /// Whether account/profile style queries are supported.
    pub supports_account: bool,
    /// Whether auth is commonly required/recommended for full access.
    pub auth_required_for_full_access: bool,
    /// Whether the site is unusable without credentials — runs are blocked
    /// (not just warned) when no credential is stored.
    pub auth_strictly_required: bool,
    /// Credential payloads Picto may capture from this site's login flow.
    pub credential_types: &'static [&'static str],
    /// OAuth provider identifier when the site uses a dedicated OAuth flow.
    pub oauth_provider: Option<&'static str>,
}

const API_KEY_CREDENTIAL_TYPES: &[&str] = &["api_key"];
const COOKIE_CREDENTIAL_TYPES: &[&str] = &["cookies"];
const OAUTH_CREDENTIAL_TYPES: &[&str] = &["oauth_token"];

/// Sole production registry for supported subscription sources.
pub static SITES: &[SiteEntry] = &[
    SiteEntry {
        id: "pixiv",
        domain: "pixiv.net",
        credential_owner_site_id: "pixiv",
        name: "Pixiv",
        example_query: "風景",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: true,
        auth_strictly_required: true,
        credential_types: OAUTH_CREDENTIAL_TYPES,
        oauth_provider: Some("pixiv"),
    },
    SiteEntry {
        id: "pixivuser",
        domain: "pixiv.net",
        credential_owner_site_id: "pixiv",
        name: "Pixiv (user)",
        example_query: "12345",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: true,
        auth_strictly_required: true,
        credential_types: OAUTH_CREDENTIAL_TYPES,
        oauth_provider: Some("pixiv"),
    },
    SiteEntry {
        id: "gelbooru",
        domain: "gelbooru.com",
        credential_owner_site_id: "gelbooru",
        name: "Gelbooru",
        example_query: "1girl solo",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: true,
        auth_strictly_required: true,
        credential_types: API_KEY_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "rule34",
        domain: "rule34.xxx",
        credential_owner_site_id: "rule34",
        name: "Rule34.xxx",
        example_query: "1girl solo",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: true,
        auth_strictly_required: true,
        credential_types: API_KEY_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "danbooru",
        domain: "danbooru.donmai.us",
        credential_owner_site_id: "danbooru",
        name: "Danbooru",
        // Anonymous danbooru searches allow at most 2 tags (more requires Gold).
        example_query: "1girl solo",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "webtoons",
        domain: "webtoons.com",
        credential_owner_site_id: "webtoons",
        name: "Webtoons",
        example_query: "https://www.webtoons.com/en/fantasy/title/list?title_no=123",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "hentaifoundry",
        domain: "hentai-foundry.com",
        credential_owner_site_id: "hentaifoundry",
        name: "Hentai Foundry",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: true,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "baraag",
        domain: "baraag.net",
        credential_owner_site_id: "baraag",
        name: "Baraag",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: OAUTH_CREDENTIAL_TYPES,
        oauth_provider: Some("mastodon"),
    },
    SiteEntry {
        id: "deviantart",
        domain: "deviantart.com",
        credential_owner_site_id: "deviantart",
        name: "DeviantArt",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "tumblr",
        domain: "tumblr.com",
        credential_owner_site_id: "tumblr",
        name: "Tumblr",
        example_query: "nasa",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: OAUTH_CREDENTIAL_TYPES,
        oauth_provider: Some("tumblr"),
    },
    SiteEntry {
        id: "furaffinity",
        domain: "furaffinity.net",
        credential_owner_site_id: "furaffinity",
        name: "Fur Affinity",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_required_for_full_access: true,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "idolcomplex",
        domain: "idolcomplex.com",
        credential_owner_site_id: "idolcomplex",
        name: "Idol Complex",
        example_query: "solo",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: true,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "sankaku",
        domain: "sankaku.app",
        credential_owner_site_id: "sankaku",
        name: "Sankaku",
        example_query: "solo rating:safe",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: true,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "yandere",
        domain: "yande.re",
        credential_owner_site_id: "yandere",
        name: "Yande.re",
        example_query: "landscape",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "konachan",
        domain: "konachan.com",
        credential_owner_site_id: "konachan",
        name: "Konachan",
        example_query: "landscape",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "safebooru",
        domain: "safebooru.org",
        credential_owner_site_id: "safebooru",
        name: "Safebooru",
        example_query: "1girl solo",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
    SiteEntry {
        id: "e621",
        domain: "e621.net",
        credential_owner_site_id: "e621",
        name: "e621",
        example_query: "canine solo",
        supports_query: true,
        supports_account: false,
        auth_required_for_full_access: false,
        auth_strictly_required: false,
        credential_types: COOKIE_CREDENTIAL_TYPES,
        oauth_provider: None,
    },
];

/// Look up a supported site by its exact ID.
pub fn site_by_id(id: &str) -> Option<&'static SiteEntry> {
    SITES.iter().find(|site| site.id == id)
}

/// Build a source URL without interpolating untrusted text into a URL string.
pub fn build_url(site_id: &str, query: &str) -> Option<String> {
    match site_id {
        "pixiv" => build_pixiv_url("tags", query),
        "pixivuser" => build_pixiv_url("users", query),
        "gelbooru" => build_booru_url("https://gelbooru.com/index.php?page=post&s=list", query),
        "rule34" => build_booru_url("https://rule34.xxx/index.php?page=post&s=list", query),
        "danbooru" => build_booru_url("https://danbooru.donmai.us/posts", query),
        "artstation" => build_artstation_url(query),
        "webtoons" => normalize_webtoons_url(query).ok(),
        "hentaifoundry" => normalize_hentaifoundry_username(query)
            .ok()
            .and_then(|username| build_hentaifoundry_url(&username)),
        "baraag" => normalize_mastodon_username(query, "baraag.net")
            .ok()
            .and_then(|username| build_mastodon_user_url("baraag.net", &username)),
        "deviantart" => normalize_deviantart_username(query)
            .ok()
            .and_then(|username| build_deviantart_gallery_url(&username)),
        "tumblr" => normalize_tumblr_blog(query)
            .ok()
            .and_then(|blog| build_tumblr_blog_url(&blog)),
        "furaffinity" => normalize_furaffinity_username(query)
            .ok()
            .and_then(|username| build_furaffinity_gallery_url(&username)),
        "idolcomplex" => build_booru_url("https://www.idolcomplex.com/en/posts", query),
        "sankaku" => build_booru_url("https://sankaku.app/", query),
        "yandere" => build_booru_url("https://yande.re/post", query),
        "konachan" => build_booru_url("https://konachan.com/post", query),
        "safebooru" => build_booru_url("https://safebooru.org/index.php?page=post&s=list", query),
        "e621" => build_booru_url("https://e621.net/posts", query),
        _ => None,
    }
}

pub fn normalize_baraag_username(raw: &str) -> Result<String, String> {
    normalize_mastodon_username(raw, "baraag.net")
}

fn normalize_mastodon_username(raw: &str, host: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Mastodon subscriptions require a username".to_string());
    }

    let username = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str() != Some(host)
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Mastodon subscriptions require a canonical profile URL".to_string());
        }
        let segments: Vec<_> = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect();
        match segments.as_slice() {
            [profile] if profile.starts_with('@') => profile[1..].to_string(),
            [profile, "media"] if profile.starts_with('@') => profile[1..].to_string(),
            ["users", profile] => (*profile).to_string(),
            _ => return Err("Mastodon subscriptions require a canonical profile URL".to_string()),
        }
    } else {
        trimmed.strip_prefix('@').unwrap_or(trimmed).to_string()
    };

    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("Mastodon subscriptions require a safe local username".to_string());
    }
    Ok(username)
}

fn build_mastodon_user_url(host: &str, username: &str) -> Option<String> {
    let mut url = Url::parse(&format!("https://{host}")).ok()?;
    url.path_segments_mut()
        .ok()?
        .push(&format!("@{username}"))
        .push("media");
    Some(url.to_string())
}

/// Normalize a Hentai Foundry public user-gallery input to its username slug.
pub fn normalize_hentaifoundry_username(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Hentai Foundry subscriptions require a username".to_string());
    }

    if let Ok(url) = Url::parse(trimmed) {
        let has_explicit_port = trimmed
            .split_once("://")
            .and_then(|(_, authority_and_path)| authority_and_path.split('/').next())
            .map(|authority| {
                authority
                    .rsplit_once('@')
                    .map_or(authority, |(_, host)| host)
                    .contains(':')
            })
            .unwrap_or(false);
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("hentai-foundry.com" | "www.hentai-foundry.com")
            )
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
            || has_explicit_port
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Hentai Foundry subscriptions require a public user URL".to_string());
        }

        let segments: Vec<_> = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect();
        let username = match segments.as_slice() {
            ["pictures", "user", username] | ["user", username, "profile"] => *username,
            _ => {
                return Err(
                    "Hentai Foundry subscriptions require a user gallery or profile URL"
                        .to_string(),
                )
            }
        };
        return validate_hentaifoundry_username(username);
    }

    validate_hentaifoundry_username(trimmed)
}

fn validate_hentaifoundry_username(username: &str) -> Result<String, String> {
    if username.len() > 128
        || username.is_empty()
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("Hentai Foundry subscriptions require a safe username slug".to_string());
    }
    Ok(username.to_string())
}

fn build_hentaifoundry_url(username: &str) -> Option<String> {
    let mut url = Url::parse("https://www.hentai-foundry.com").ok()?;
    url.path_segments_mut()
        .ok()?
        .push("pictures")
        .push("user")
        .push(username);
    Some(url.to_string())
}

pub fn normalize_deviantart_username(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("DeviantArt subscriptions require a username".to_string());
    }

    let username = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("deviantart.com" | "www.deviantart.com")
            )
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("DeviantArt subscriptions require a canonical profile URL".to_string());
        }

        let segments: Vec<_> = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect();
        match segments.as_slice() {
            [username] | [username, "gallery"] => (*username).to_string(),
            _ => {
                return Err("DeviantArt subscriptions require a profile or gallery URL".to_string())
            }
        }
    } else {
        trimmed.to_string()
    };

    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("DeviantArt subscriptions require a safe username".to_string());
    }
    Ok(username.to_string())
}

pub fn normalize_tumblr_blog(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Tumblr subscriptions require a blog name".to_string());
    }

    let blog = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Tumblr subscriptions require a canonical public blog URL".to_string());
        }
        let host = url.host_str().unwrap_or_default();
        let segments: Vec<_> = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect();
        if matches!(host, "tumblr.com" | "www.tumblr.com") {
            match segments.as_slice() {
                [blog] => (*blog).to_string(),
                _ => return Err("Tumblr subscriptions require a blog profile URL".to_string()),
            }
        } else if let Some(blog) = host.strip_suffix(".tumblr.com") {
            if !segments.is_empty() {
                return Err("Tumblr subscriptions require a canonical public blog URL".to_string());
            }
            blog.to_string()
        } else {
            return Err("Tumblr subscriptions require a tumblr.com URL".to_string());
        }
    } else {
        let blog = trimmed.strip_prefix('@').unwrap_or(trimmed);
        blog.strip_suffix(".tumblr.com").unwrap_or(blog).to_string()
    };

    if blog.is_empty()
        || blog.len() > 64
        || !blog
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("Tumblr subscriptions require a safe public blog name".to_string());
    }
    Ok(blog)
}

fn build_tumblr_blog_url(blog: &str) -> Option<String> {
    let mut url = Url::parse("https://www.tumblr.com").ok()?;
    url.path_segments_mut().ok()?.push(blog);
    Some(url.to_string())
}

pub fn normalize_furaffinity_username(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Fur Affinity subscriptions require a username".to_string());
    }

    let username = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("furaffinity.net" | "www.furaffinity.net")
            )
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Fur Affinity subscriptions require a canonical gallery URL".to_string());
        }
        let segments: Vec<_> = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect();
        match segments.as_slice() {
            ["gallery", username] | ["user", username] => (*username).to_string(),
            _ => return Err("Fur Affinity subscriptions require a user or gallery URL".to_string()),
        }
    } else {
        trimmed.strip_prefix('@').unwrap_or(trimmed).to_string()
    };

    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("Fur Affinity subscriptions require a safe username".to_string());
    }
    Ok(username)
}

fn build_furaffinity_gallery_url(username: &str) -> Option<String> {
    let mut url = Url::parse("https://www.furaffinity.net").ok()?;
    url.path_segments_mut().ok()?.push("gallery").push(username);
    Some(url.to_string())
}

fn build_deviantart_gallery_url(username: &str) -> Option<String> {
    let mut url = Url::parse("https://www.deviantart.com").ok()?;
    url.set_path(&format!("/{username}/gallery/"));
    Some(url.to_string())
}

/// Normalize a user-supplied Webtoons comic list URL.
///
/// Webtoons subscriptions are deliberately URL-only: accepting arbitrary text
/// here would make the source look like a generic search while gallery-dl only
/// supports a specific comic list extractor.
pub fn normalize_webtoons_url(raw: &str) -> Result<String, String> {
    let parsed = Url::parse(raw.trim())
        .map_err(|_| "Webtoons subscriptions require a comic list URL".to_string())?;
    let host = parsed.host_str().unwrap_or_default();
    if !matches!(host, "webtoons.com" | "www.webtoons.com") {
        return Err("Webtoons subscriptions require a webtoons.com URL".to_string());
    }
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return Err("Webtoons subscriptions require a public HTTPS comic list URL".to_string());
    }

    let segments: Vec<_> = parsed
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() != 4 || segments[3] != "list" || !parsed.path().ends_with("/list") {
        return Err("Webtoons subscriptions require a {lang}/{genre}/{comic}/list URL".to_string());
    }

    let title_numbers: Vec<_> = parsed
        .query_pairs()
        .filter(|(key, _)| key == "title_no")
        .map(|(_, value)| value.to_string())
        .collect();
    if title_numbers.len() != 1 {
        return Err("Webtoons comic list URLs require one positive title_no".to_string());
    }
    let title_no = title_numbers[0]
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| "Webtoons comic list URLs require a positive title_no".to_string())?;

    let mut canonical = parsed;
    canonical
        .set_scheme("https")
        .map_err(|_| "Invalid Webtoons URL".to_string())?;
    canonical
        .set_host(Some("www.webtoons.com"))
        .map_err(|_| "Invalid Webtoons URL".to_string())?;
    canonical
        .set_username("")
        .map_err(|_| "Invalid Webtoons URL".to_string())?;
    canonical
        .set_password(None)
        .map_err(|_| "Invalid Webtoons URL".to_string())?;
    canonical
        .set_port(None)
        .map_err(|_| "Invalid Webtoons URL".to_string())?;
    canonical.set_query(Some(&format!("title_no={title_no}")));
    canonical.set_fragment(None);
    Ok(canonical.to_string())
}

fn build_artstation_url(query: &str) -> Option<String> {
    let slug = query.trim().strip_prefix('@').unwrap_or(query.trim());
    if slug.is_empty()
        || matches!(slug, "artwork" | "projects" | "search")
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }

    let mut url = Url::parse("https://www.artstation.com").ok()?;
    url.path_segments_mut().ok()?.push(slug);
    Some(url.to_string())
}

fn build_pixiv_url(kind: &str, query: &str) -> Option<String> {
    let mut url = Url::parse("https://www.pixiv.net/en").ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.push(kind).push(query).push("artworks");
    }
    if kind == "tags" {
        url.query_pairs_mut().append_pair("s_mode", "s_tag");
    }
    Some(url.to_string())
}

fn build_booru_url(base: &str, query: &str) -> Option<String> {
    let mut url = Url::parse(base).ok()?;
    url.query_pairs_mut().append_pair("tags", query);
    Some(url.to_string())
}

/// Extract the domain from a URL for rate limiting / credential lookup.
pub fn extract_domain(url_str: &str) -> Option<String> {
    Url::parse(url_str)
        .ok()
        .and_then(|url| url.host_str().map(String::from))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn production_registry_contains_exactly_the_supported_sources() {
        let ids: HashSet<_> = SITES.iter().map(|site| site.id).collect();
        assert_eq!(
            ids,
            HashSet::from([
                "pixiv",
                "pixivuser",
                "gelbooru",
                "rule34",
                "danbooru",
                "webtoons",
                "hentaifoundry",
                "baraag",
                "deviantart",
                "tumblr",
                "furaffinity",
                "idolcomplex",
                "sankaku",
                "yandere",
                "konachan",
                "safebooru",
                "e621",
            ])
        );
        assert_eq!(SITES.len(), 17);
    }

    #[test]
    fn supported_sources_have_coherent_login_contracts() {
        for source in SITES {
            let owner = site_by_id(source.credential_owner_site_id).unwrap_or_else(|| {
                panic!(
                    "source {} has unknown credential owner {}",
                    source.id, source.credential_owner_site_id
                )
            });
            assert_eq!(
                owner.credential_owner_site_id, source.credential_owner_site_id,
                "credential owner {} is not canonical",
                source.credential_owner_site_id
            );

            assert!(
                !source.credential_types.is_empty(),
                "source {} has no captured credential type",
                source.id
            );
            if source.auth_strictly_required {
                assert!(source.auth_required_for_full_access);
            }
        }
    }

    #[test]
    fn baraag_and_tumblr_use_their_gallery_dl_oauth_contracts() {
        let baraag = site_by_id("baraag").unwrap();
        assert_eq!(baraag.credential_types, OAUTH_CREDENTIAL_TYPES);
        assert_eq!(baraag.oauth_provider, Some("mastodon"));

        let tumblr = site_by_id("tumblr").unwrap();
        assert_eq!(tumblr.credential_types, OAUTH_CREDENTIAL_TYPES);
        assert_eq!(tumblr.oauth_provider, Some("tumblr"));
    }

    #[test]
    fn pixiv_path_segments_are_encoded() {
        assert_eq!(
            build_url("pixivuser", "artist/name?preview=1").as_deref(),
            Some("https://www.pixiv.net/en/users/artist%2Fname%3Fpreview=1/artworks")
        );
    }

    #[test]
    fn booru_query_values_are_encoded() {
        assert_eq!(
            build_url("gelbooru", "1girl solo&rating:safe").as_deref(),
            Some("https://gelbooru.com/index.php?page=post&s=list&tags=1girl+solo%26rating%3Asafe")
        );
        assert_eq!(
            build_url("rule34", "1girl solo&rating:safe").as_deref(),
            Some("https://rule34.xxx/index.php?page=post&s=list&tags=1girl+solo%26rating%3Asafe")
        );
        assert_eq!(
            build_url("danbooru", "artist:name").as_deref(),
            Some("https://danbooru.donmai.us/posts?tags=artist%3Aname")
        );
        assert_eq!(
            build_url("artstation", "artist-name").as_deref(),
            Some("https://www.artstation.com/artist-name")
        );
        assert_eq!(
            build_url(
                "webtoons",
                "http://webtoons.com/en/fantasy/title/list?title_no=123&page=2#episode"
            )
            .as_deref(),
            Some("https://www.webtoons.com/en/fantasy/title/list?title_no=123")
        );
        assert_eq!(
            build_url("hentaifoundry", "artist-name").as_deref(),
            Some("https://www.hentai-foundry.com/pictures/user/artist-name")
        );
        assert_eq!(
            build_url("baraag", "Blue_").as_deref(),
            Some("https://baraag.net/@Blue_/media")
        );
        assert_eq!(
            build_url("deviantart", "https://deviantart.com/artist-name/gallery/").as_deref(),
            Some("https://www.deviantart.com/artist-name/gallery/")
        );
        assert_eq!(
            build_url("tumblr", "https://nasa.tumblr.com/").as_deref(),
            Some("https://www.tumblr.com/nasa")
        );
        assert_eq!(
            build_url(
                "furaffinity",
                "https://www.furaffinity.net/user/Artist_Name/"
            )
            .as_deref(),
            Some("https://www.furaffinity.net/gallery/Artist_Name")
        );
        assert_eq!(
            build_url("idolcomplex", "solo rating:safe").as_deref(),
            Some("https://www.idolcomplex.com/en/posts?tags=solo+rating%3Asafe")
        );
        assert_eq!(
            build_url("sankaku", "solo rating:safe").as_deref(),
            Some("https://sankaku.app/?tags=solo+rating%3Asafe")
        );
        assert_eq!(
            build_url("yandere", "1girl solo&rating:safe").as_deref(),
            Some("https://yande.re/post?tags=1girl+solo%26rating%3Asafe")
        );
        assert_eq!(
            build_url("konachan", "landscape").as_deref(),
            Some("https://konachan.com/post?tags=landscape")
        );
        assert_eq!(
            build_url("safebooru", "1girl solo").as_deref(),
            Some("https://safebooru.org/index.php?page=post&s=list&tags=1girl+solo")
        );
        assert_eq!(
            build_url("e621", "canine").as_deref(),
            Some("https://e621.net/posts?tags=canine")
        );
    }

    #[test]
    fn tumblr_accepts_only_public_blog_names_and_canonical_profiles() {
        for input in [
            "nasa",
            "@nasa",
            "nasa.tumblr.com",
            "https://nasa.tumblr.com/",
            "https://www.tumblr.com/nasa",
        ] {
            assert_eq!(
                normalize_tumblr_blog(input).as_deref(),
                Ok("nasa"),
                "{input}"
            );
        }
        for input in [
            "nasa/posts",
            "https://tumblr.com.evil.example/nasa",
            "https://www.tumblr.com/nasa/123",
            "https://nasa.tumblr.com/post/123",
            "https://www.tumblr.com/nasa?ref=feed",
        ] {
            assert!(normalize_tumblr_blog(input).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn unsupported_sources_and_aliases_are_not_registered() {
        assert!(site_by_id("rule34xxx").is_none());
    }

    #[test]
    fn artstation_url_builder_rejects_path_injection() {
        assert!(build_url("artstation", "artist/name").is_none());
        assert!(build_url("artstation", "artist?preview=1").is_none());
        assert!(build_url("artstation", "projects").is_none());
    }

    #[test]
    fn webtoons_url_builder_rejects_arbitrary_hosts_and_invalid_lists() {
        for value in [
            "https://example.com/en/fantasy/title/list?title_no=123",
            "https://www.webtoons.com/en/title/list?title_no=123",
            "https://www.webtoons.com/en/fantasy/title/extra/list?title_no=123",
            "https://www.webtoons.com/en/fantasy/title/viewer?title_no=123",
            "https://www.webtoons.com/en/fantasy/title/list?title_no=0",
            "https://www.webtoons.com/en/fantasy/title/list",
            "https://www.webtoons.com.evil.example/en/fantasy/title/list?title_no=123",
        ] {
            assert!(build_url("webtoons", value).is_none(), "accepted {value}");
        }
    }

    #[test]
    fn hentaifoundry_accepts_only_safe_user_inputs() {
        for value in [
            "http://hentai-foundry.com/pictures/user/artist-name",
            "https://hentai-foundry.com/pictures/user/artist-name",
            "http://www.hentai-foundry.com/user/artist-name/profile",
            "https://www.hentai-foundry.com/pictures/user/artist-name",
            "https://www.hentai-foundry.com/user/artist-name/profile",
            "artist-name",
        ] {
            assert_eq!(
                normalize_hentaifoundry_username(value).as_deref(),
                Ok("artist-name"),
                "accepted input should normalize: {value}"
            );
        }
    }

    #[test]
    fn baraag_accepts_only_local_profile_inputs() {
        for value in [
            "Blue_",
            "@Blue_",
            "http://baraag.net/@Blue_",
            "https://baraag.net/@Blue_/media",
            "https://baraag.net/users/Blue_",
        ] {
            assert_eq!(
                normalize_baraag_username(value).as_deref(),
                Ok("Blue_"),
                "accepted input should normalize: {value}"
            );
        }
        for value in [
            "artist/name",
            "https://example.com/@Blue_",
            "https://baraag.net/@Blue_/12345",
            "https://user:pass@baraag.net/@Blue_",
            "https://baraag.net:8443/@Blue_",
            "https://baraag.net/@Blue_?page=2",
        ] {
            assert!(
                normalize_baraag_username(value).is_err(),
                "unsafe input accepted: {value}"
            );
        }
    }

    #[test]
    fn hentaifoundry_rejects_urls_that_can_escape_the_user_path() {
        for value in [
            "https://www.hentai-foundry.com/pictures/user/artist-name?x=1",
            "https://www.hentai-foundry.com/pictures/user/artist-name#profile",
            "https://user:pass@www.hentai-foundry.com/pictures/user/artist-name",
            "https://www.hentai-foundry.com:443/pictures/user/artist-name",
            "https://www.hentai-foundry.com/pictures/user/artist-name/post/1",
            "https://www.hentai-foundry.com/pictures/user/artist/name",
            "artist/name",
            "artist name",
        ] {
            assert!(
                normalize_hentaifoundry_username(value).is_err(),
                "accepted unsafe input: {value}"
            );
        }
    }

    #[test]
    fn deviantart_accepts_only_safe_profile_and_gallery_inputs() {
        for value in [
            "artist-name",
            "http://deviantart.com/artist-name",
            "https://www.deviantart.com/artist-name/",
            "https://www.deviantart.com/artist-name/gallery/",
        ] {
            assert_eq!(
                normalize_deviantart_username(value).as_deref(),
                Ok("artist-name"),
                "accepted input should normalize: {value}"
            );
        }
        assert_eq!(
            build_url("deviantart", "artist-name").as_deref(),
            Some("https://www.deviantart.com/artist-name/gallery/")
        );
    }

    #[test]
    fn deviantart_rejects_hostile_urls_and_query_text() {
        for value in [
            "artist/name",
            "artist name",
            "@artist",
            "https://example.com/artist-name/gallery/",
            "https://www.deviantart.com.evil.example/artist-name/gallery/",
            "https://user:pass@www.deviantart.com/artist-name/gallery/",
            "https://www.deviantart.com:8443/artist-name/gallery/",
            "https://www.deviantart.com/artist-name/gallery/post",
            "https://www.deviantart.com/artist-name/gallery/?page=2",
            "https://www.deviantart.com/artist-name/gallery/#recent",
        ] {
            assert!(
                normalize_deviantart_username(value).is_err(),
                "accepted unsafe input: {value}"
            );
            assert!(
                build_url("deviantart", value).is_none(),
                "built unsafe URL: {value}"
            );
        }
    }
}
