use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SiteEntry {
    /// Internal identifier (gallery-dl category name).
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Canonical site domain shown in UI and used as credential fallback key.
    pub domain: &'static str,
    /// URL template — `{query}` is replaced with the user's search tags.
    pub url_template: &'static str,
    /// Example query to show in the UI.
    pub example_query: &'static str,
    /// Whether this source supports tag/text query style URLs.
    pub supports_query: bool,
    /// Whether account/profile style queries are supported.
    pub supports_account: bool,
    /// Whether we support storing auth material for this source.
    pub auth_supported: bool,
    /// Whether auth is commonly required/recommended for full access.
    pub auth_required_for_full_access: bool,
}

/// Built-in site registry. The user picks one of these; we substitute `{query}`.
pub static SITES: &[SiteEntry] = &[
    SiteEntry {
        id: "pixiv",
        domain: "pixiv.net",
        name: "Pixiv",
        url_template: "https://www.pixiv.net/en/tags/{query}/artworks?s_mode=s_tag",
        example_query: "風景",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "pixivuser",
        domain: "pixiv.net",
        name: "Pixiv (user)",
        url_template: "https://www.pixiv.net/en/users/{query}",
        example_query: "12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "gelbooru",
        domain: "gelbooru.com",
        name: "Gelbooru",
        url_template: "https://gelbooru.com/index.php?page=post&s=list&tags={query}",
        example_query: "1girl solo",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "danbooru",
        domain: "danbooru.donmai.us",
        name: "Danbooru",
        url_template: "https://danbooru.donmai.us/posts?tags={query}",
        example_query: "1girl solo blue_eyes",
        supports_query: true,
        supports_account: false,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "3dbooru",
        domain: "3dbooru.org",
        name: "3DBooru",
        url_template: "https://3dbooru.org/index.php?page=post&s=list&tags={query}",
        example_query: "solo",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "artstation",
        domain: "artstation.com",
        name: "ArtStation",
        url_template: "https://www.artstation.com/{query}",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "sankaku",
        domain: "sankakucomplex.com",
        name: "Sankaku",
        url_template: "https://chan.sankakucomplex.com/?tags={query}&commit=Search",
        example_query: "1girl",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "idolcomplex",
        domain: "idol.sankakucomplex.com",
        name: "IdolComplex",
        url_template: "https://idol.sankakucomplex.com/?tags={query}&commit=Search",
        example_query: "idol",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "twitter",
        domain: "twitter.com",
        name: "Twitter/X",
        url_template: "https://twitter.com/{query}",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "deviantart",
        domain: "deviantart.com",
        name: "DeviantArt",
        url_template: "https://deviantart.com/{query}",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "patreon",
        domain: "patreon.com",
        name: "Patreon",
        url_template: "https://www.patreon.com/{query}/posts",
        example_query: "creatorname",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "nijie",
        domain: "nijie.info",
        name: "Nijie",
        url_template: "https://nijie.info/members_illust.php?id={query}",
        example_query: "12345",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "tumblr",
        domain: "tumblr.com",
        name: "Tumblr",
        url_template: "https://{query}.tumblr.com",
        example_query: "blogname",
        supports_query: true,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "fantia",
        domain: "fantia.jp",
        name: "Fantia",
        url_template: "https://fantia.jp/fanclubs/{query}/posts",
        example_query: "12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "fanbox",
        domain: "fanbox.cc",
        name: "Fanbox",
        url_template: "https://{query}.fanbox.cc",
        example_query: "creatorname",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "webtoons",
        domain: "webtoons.com",
        name: "Webtoons",
        url_template: "https://www.webtoons.com/en/{query}",
        example_query: "genre/title/list?title_no=12345",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "kemono",
        domain: "kemono.cr",
        name: "Kemono",
        url_template: "https://kemono.cr/{query}",
        example_query: "patreon/user/12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "coomer",
        domain: "coomer.st",
        name: "Coomer",
        url_template: "https://coomer.st/{query}",
        example_query: "onlyfans/user/12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "seiso",
        domain: "seiso.party",
        name: "Seiso.party",
        url_template: "https://seiso.party/{query}",
        example_query: "service/user/12345",
        supports_query: false,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "baraag",
        domain: "baraag.net",
        name: "Baraag",
        url_template: "https://baraag.net/{query}",
        example_query: "@username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "pawoo",
        domain: "pawoo.net",
        name: "Pawoo",
        url_template: "https://pawoo.net/{query}",
        example_query: "@username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "hentaifoundry",
        domain: "hentai-foundry.com",
        name: "Hentai Foundry",
        url_template: "https://www.hentai-foundry.com/user/{query}/profile",
        example_query: "username",
        supports_query: true,
        supports_account: true,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "yandere",
        domain: "yande.re",
        name: "Yande.re",
        url_template: "https://yande.re/post?tags={query}",
        example_query: "landscape",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "rule34",
        domain: "rule34.xxx",
        name: "Rule34.xxx",
        url_template: "https://rule34.xxx/index.php?page=post&s=list&tags={query}",
        example_query: "solo",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "e621",
        domain: "e621.net",
        name: "e621",
        url_template: "https://e621.net/posts?tags={query}",
        example_query: "solo canine rating:safe",
        supports_query: true,
        supports_account: false,
        auth_supported: false,
        auth_required_for_full_access: false,
    },
    SiteEntry {
        id: "furaffinity",
        domain: "furaffinity.net",
        name: "FurAffinity",
        url_template: "https://www.furaffinity.net/user/{query}/",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "instagram",
        domain: "instagram.com",
        name: "Instagram",
        url_template: "https://instagram.com/{query}",
        example_query: "username",
        supports_query: false,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    // Existing maintained extras.
    SiteEntry {
        id: "konachan",
        domain: "konachan.com",
        name: "Konachan",
        url_template: "https://konachan.com/post?tags={query}",
        example_query: "landscape",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
    SiteEntry {
        id: "safebooru",
        domain: "safebooru.org",
        name: "Safebooru",
        url_template: "https://safebooru.org/index.php?page=post&s=list&tags={query}",
        example_query: "1girl smile",
        supports_query: true,
        supports_account: true,
        auth_supported: true,
        auth_required_for_full_access: true,
    },
];

/// Canonicalize legacy/alias site ids to current internal ids.
pub fn canonical_site_id(id: &str) -> &str {
    match id {
        "rule34xxx" | "rule34.xxx" => "rule34",
        "e621.net" => "e621",
        "furaffinity.net" => "furaffinity",
        "yande.re" => "yandere",
        "kemono.party" | "kemono.su" => "kemono",
        "coomer.party" | "coomer.su" => "coomer",
        "baraag.net" => "baraag",
        "pawoo.net" => "pawoo",
        _ => id,
    }
}

/// Look up a site entry by ID.
pub fn site_by_id(id: &str) -> Option<&'static SiteEntry> {
    let canonical = canonical_site_id(id);
    SITES.iter().find(|s| s.id == canonical)
}

/// Build a full URL from a site ID and query string.
pub fn build_url(site_id: &str, query: &str) -> Option<String> {
    site_by_id(site_id).map(|site| substitute_query(site.url_template, query))
}

pub fn substitute_query(template: &str, query: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    template.replace("{query}", &encoded)
}

/// Extract the domain from a URL for rate limiting / credential lookup.
pub fn extract_domain(url_str: &str) -> Option<String> {
    url::Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
}
