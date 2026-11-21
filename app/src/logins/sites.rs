//! The services a sign-in window can open (`electron/windows/authSites.mjs`):
//! where each one's login starts, how the app tells that it has finished
//! and what it keeps.
//!
//! A debug build started with `PICTO_GPUI_LOGIN_ORIGIN=http://127.0.0.1:<port>`
//! moves every address here onto that origin (path and query kept), so the
//! sign-in can be driven end to end against local pages without reaching a
//! real site (`seams::login_origin`).

/// How a service's login is captured (`site.strategy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// The session cookies a normal page login sets.
    Cookies,
    /// The account's API key, read from its options page after the login.
    AccountApi,
    OAuth1,
    OAuth2,
    /// The core's own OAuth flow (`pixiv_oauth_start`, `pixiv_oauth_exchange`).
    Pixiv,
    /// A browser of the user's own, outside the app, or a pasted session.
    OnlyFans,
}

/// The sign-in window's size (`authWindowSize`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
    pub min_width: f32,
    pub min_height: f32,
}

/// The size of every sign-in window but the wide ones.
pub const AUTH_WINDOW: WindowSize = WindowSize {
    width: 520.,
    height: 760.,
    min_width: 420.,
    min_height: 640.,
};

/// `WIDE_AUTH_WINDOW`: services whose login pages need a desktop width.
pub const WIDE_AUTH_WINDOW: WindowSize = WindowSize {
    width: 1000.,
    height: 760.,
    min_width: 760.,
    min_height: 640.,
};

/// One service, with its addresses resolved (`resolveAuthSite`, and the
/// SubscribeStar service chosen).
#[derive(Clone, Debug, PartialEq)]
pub struct Site {
    pub id: &'static str,
    pub label: &'static str,
    pub strategy: Strategy,
    pub login_url: Option<String>,
    pub cookie_url: Option<String>,
    /// The cookies of all these addresses are read together (ExHentai).
    pub cookie_urls: Option<Vec<String>>,
    /// The cookies kept; every cookie of the address when `None`.
    pub cookie_names: Option<&'static [&'static str]>,
    /// The cookies only a signed-in session has; with none, the page must
    /// show it is signed in (`hasAuthenticatedDomSignal`).
    pub authenticated_cookie_names: Option<&'static [&'static str]>,
    /// A page the login must reach before it is saved (ExHentai).
    pub verification_url: Option<String>,
    /// The account page holding the API key (`account_api`).
    pub options_url: Option<String>,
    pub request_token_url: Option<String>,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub consumer_key: Option<&'static str>,
    pub consumer_secret: Option<&'static str>,
    pub client_id: Option<&'static str>,
    pub client_secret: Option<&'static str>,
    /// The token exchange sends the client in an `Authorization: Basic`
    /// header rather than in the body (`tokenAuth: 'basic'`).
    pub token_auth_basic: bool,
    pub redirect_url: Option<String>,
    pub scope: Option<&'static str>,
    /// The field of the token answer kept (`access_token` by default).
    pub token_field: Option<&'static str>,
    /// FANBOX's account API, asked whether the session is signed in.
    pub session_check_url: Option<String>,
    pub window: WindowSize,
}

/// `OAUTH_CALLBACK_URL`: where OAuth services without their own redirect
/// send the browser back.
pub const OAUTH_CALLBACK_URL: &str = "https://picto.app/oauth/callback";

/// The SubscribeStar services a login can be for; one is stored at a time.
pub const SUBSCRIBESTAR_HOSTS: [&str; 3] = [
    "www.subscribestar.com",
    "subscribestar.adult",
    "subscribestar.art",
];

/// Every service, by id, in production's order (`AUTH_SITES`).
#[cfg(test)]
pub const SITE_IDS: [&str; 18] = [
    "pixiv",
    "gelbooru",
    "rule34",
    "tumblr",
    "twitter",
    "newgrounds",
    "hentaifoundry",
    "furaffinity",
    "danbooru",
    "deviantart",
    "patreon",
    "fanbox",
    "subscribestar",
    "onlyfans",
    "yandere",
    "safebooru",
    "e621",
    "ehentai",
];

fn blank(id: &'static str, label: &'static str, strategy: Strategy) -> Site {
    Site {
        id,
        label,
        strategy,
        login_url: None,
        cookie_url: None,
        cookie_urls: None,
        cookie_names: None,
        authenticated_cookie_names: None,
        verification_url: None,
        options_url: None,
        request_token_url: None,
        authorize_url: None,
        token_url: None,
        consumer_key: None,
        consumer_secret: None,
        client_id: None,
        client_secret: None,
        token_auth_basic: false,
        redirect_url: None,
        scope: None,
        token_field: None,
        session_check_url: None,
        window: AUTH_WINDOW,
    }
}

/// `cookieSite`: a service whose page login leaves session cookies.
fn cookie_site(id: &'static str, label: &'static str, login: &str, cookies: &str) -> Site {
    Site {
        login_url: Some(login.to_owned()),
        cookie_url: Some(cookies.to_owned()),
        ..blank(id, label, Strategy::Cookies)
    }
}

fn some(url: &str) -> Option<String> {
    Some(url.to_owned())
}

/// The service as production declares it, before any origin is moved.
fn declared(id: &str) -> Option<Site> {
    // i18n: service names, URLs and protocol fields; not shown through t()
    Some(match id {
        "pixiv" => blank("pixiv", "Pixiv", Strategy::Pixiv),
        "gelbooru" => Site {
            login_url: some("https://gelbooru.com/index.php?code=00&page=account&s=login"),
            options_url: some("https://gelbooru.com/index.php?page=account&s=options"),
            ..blank("gelbooru", "Gelbooru", Strategy::AccountApi)
        },
        "rule34" => Site {
            login_url: some("https://rule34.xxx/index.php?code=00&page=account&s=login"),
            options_url: some("https://rule34.xxx/index.php?page=account&s=options"),
            ..blank("rule34", "Rule34.xxx", Strategy::AccountApi)
        },
        "tumblr" => Site {
            login_url: some("https://www.tumblr.com/login"),
            request_token_url: some("https://www.tumblr.com/oauth/request_token"),
            authorize_url: some("https://www.tumblr.com/oauth/authorize"),
            token_url: some("https://www.tumblr.com/oauth/access_token"),
            consumer_key: Some("O3hU2tMi5e4Qs5t3vezEi6L0qRORJ5y9oUpSGsrWu8iA3UCc3B"),
            consumer_secret: Some("sFdsK3PDdP2QpYMRAoq0oDnw0sFS24XigXmdfnaeNZpJpqAn03"),
            ..blank("tumblr", "Tumblr", Strategy::OAuth1)
        },
        "twitter" => Site {
            authenticated_cookie_names: Some(&["auth_token", "ct0"]),
            ..cookie_site(
                "twitter",
                "Twitter / X",
                "https://x.com/i/flow/login",
                "https://x.com",
            )
        },
        "newgrounds" => Site {
            cookie_names: Some(&["ng_session"]),
            authenticated_cookie_names: Some(&["ng_session"]),
            window: WIDE_AUTH_WINDOW,
            ..cookie_site(
                "newgrounds",
                "Newgrounds",
                "https://www.newgrounds.com/login",
                "https://www.newgrounds.com",
            )
        },
        "hentaifoundry" => Site {
            cookie_names: Some(&["PHPSESSID"]),
            ..cookie_site(
                "hentaifoundry",
                "Hentai Foundry",
                "https://www.hentai-foundry.com/site/index",
                "https://www.hentai-foundry.com",
            )
        },
        "furaffinity" => Site {
            cookie_names: Some(&["a", "b"]),
            authenticated_cookie_names: Some(&["a", "b"]),
            ..cookie_site(
                "furaffinity",
                "Fur Affinity",
                "https://www.furaffinity.net/login/",
                "https://www.furaffinity.net",
            )
        },
        "danbooru" => cookie_site(
            "danbooru",
            "Danbooru",
            "https://danbooru.donmai.us/session/new",
            "https://danbooru.donmai.us",
        ),
        // gallery-dl's public application and callback, so the refresh token
        // goes straight back to gallery-dl.
        "deviantart" => Site {
            client_id: Some("5388"),
            client_secret: Some("76b08c69cfb27f26d6161f9ab6d061a1"),
            authorize_url: some("https://www.deviantart.com/oauth2/authorize"),
            token_url: some("https://www.deviantart.com/oauth2/token"),
            token_auth_basic: true,
            redirect_url: some("https://mikf.github.io/gallery-dl/oauth-redirect.html"),
            scope: Some("browse user.manage"),
            token_field: Some("refresh_token"),
            cookie_url: some("https://www.deviantart.com"),
            cookie_names: Some(&["auth", "auth_secure", "userinfo"]),
            window: WIDE_AUTH_WINDOW,
            ..blank("deviantart", "DeviantArt", Strategy::OAuth2)
        },
        "patreon" => Site {
            cookie_names: Some(&["session_id"]),
            authenticated_cookie_names: Some(&["session_id"]),
            ..cookie_site(
                "patreon",
                "Patreon",
                "https://www.patreon.com/login?l=en-GB",
                "https://www.patreon.com",
            )
        },
        "fanbox" => Site {
            authenticated_cookie_names: Some(&["FANBOXSESSID"]),
            session_check_url: some("https://api.fanbox.cc/creator.listFollowing"),
            ..cookie_site(
                "fanbox",
                "pixivFANBOX",
                "https://accounts.pixiv.net/login?prompt=select_account&return_to=https%3A%2F%2Fwww.fanbox.cc%2Fauth%2Fstart&source=fanbox",
                "https://api.fanbox.cc",
            )
        },
        "subscribestar" => Site {
            cookie_names: Some(&["_personalization_id", "18_plus_agreement_generic"]),
            authenticated_cookie_names: Some(&["_personalization_id"]),
            ..cookie_site(
                "subscribestar",
                "SubscribeStar",
                "https://www.subscribestar.com/login",
                "https://www.subscribestar.com",
            )
        },
        "onlyfans" => Site {
            login_url: some("https://onlyfans.com/"),
            cookie_url: some("https://onlyfans.com"),
            cookie_names: Some(&["sess", "auth_id", "auth_uid"]),
            ..blank("onlyfans", "OnlyFans", Strategy::OnlyFans)
        },
        "yandere" => cookie_site(
            "yandere",
            "Yande.re",
            "https://yande.re/user/login",
            "https://yande.re",
        ),
        "safebooru" => cookie_site(
            "safebooru",
            "Safebooru",
            "https://safebooru.org/index.php?page=account&s=login&code=00",
            "https://safebooru.org",
        ),
        "e621" => cookie_site(
            "e621",
            "e621",
            "https://e621.net/session/new",
            "https://e621.net",
        ),
        "ehentai" => Site {
            verification_url: some("https://exhentai.org/"),
            cookie_urls: Some(vec![
                "https://forums.e-hentai.org".to_owned(),
                "https://e-hentai.org".to_owned(),
                "https://exhentai.org".to_owned(),
            ]),
            cookie_names: Some(&["ipb_member_id", "ipb_pass_hash", "igneous"]),
            authenticated_cookie_names: Some(&["ipb_member_id", "ipb_pass_hash"]),
            ..cookie_site(
                "ehentai",
                // i18n: a service's name, as production shows it
                "ExHentai",
                "https://forums.e-hentai.org/index.php?act=Login&CODE=00",
                "https://forums.e-hentai.org",
            )
        },
        _ => return None,
    })
}

/// `resolveAuthSite`: the service for a site id, `pixivuser` signing in to
/// Pixiv and `exhentai` to E-Hentai, ignoring case and spaces around it.
pub fn resolve(site_id: &str) -> Option<Site> {
    let requested = site_id.trim().to_lowercase();
    let canonical = match requested.as_str() {
        "pixivuser" => "pixiv",
        "exhentai" => "ehentai",
        other => other,
    };
    let site = declared(canonical)?;
    Some(match crate::seams::login_origin() {
        Some(origin) => moved(site, &origin),
        None => site,
    })
}

/// The service with one SubscribeStar service chosen, or the error
/// production gives for any other host.
pub fn with_service_host(mut site: Site, host: &str) -> Result<Site, String> {
    if site.id != "subscribestar" || !SUBSCRIBESTAR_HOSTS.contains(&host) {
        // i18n: production's error text, shown as it is (English)
        return Err("Choose a supported SubscribeStar service.".to_owned());
    }
    let login = format!("https://{host}/login");
    let cookies = format!("https://{host}");
    match crate::seams::login_origin() {
        Some(origin) => {
            site.login_url = Some(move_origin(&login, &origin));
            site.cookie_url = Some(move_origin(&cookies, &origin));
        }
        None => {
            site.login_url = Some(login);
            site.cookie_url = Some(cookies);
        }
    }
    Ok(site)
}

/// `getStaticAuthLoginRoutes`: every service with a login page, and where.
#[cfg(test)]
pub fn login_routes() -> Vec<(String, String)> {
    SITE_IDS
        .iter()
        .filter_map(|id| declared(id))
        .filter_map(|site| Some((site.id.to_owned(), site.login_url?)))
        .collect()
}

/// Every address of `site` with its scheme, host and port replaced by
/// `origin`'s.
fn moved(mut site: Site, origin: &str) -> Site {
    let each = |url: &mut Option<String>| {
        if let Some(value) = url {
            *value = move_origin(value, origin);
        }
    };
    each(&mut site.login_url);
    each(&mut site.cookie_url);
    each(&mut site.verification_url);
    each(&mut site.options_url);
    each(&mut site.request_token_url);
    each(&mut site.authorize_url);
    each(&mut site.token_url);
    each(&mut site.redirect_url);
    each(&mut site.session_check_url);
    if let Some(urls) = &mut site.cookie_urls {
        for url in urls {
            *url = move_origin(url, origin);
        }
    }
    // OAuth services without a redirect of their own come back to the
    // fixed callback, which moves with the rest.
    if matches!(site.strategy, Strategy::OAuth1 | Strategy::OAuth2) && site.redirect_url.is_none() {
        site.redirect_url = Some(move_origin(OAUTH_CALLBACK_URL, origin));
    }
    site
}

/// `url` with `origin`'s scheme, host and port, its path and query kept.
pub fn move_origin(url: &str, origin: &str) -> String {
    let (Ok(mut parsed), Ok(target)) = (url::Url::parse(url), url::Url::parse(origin)) else {
        return url.to_owned();
    };
    let _ = parsed.set_scheme(target.scheme());
    let _ = parsed.set_host(target.host_str());
    let _ = parsed.set_port(target.port());
    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_to_one_service_and_unknown_ones_to_none() {
        assert_eq!(resolve("pixivuser").unwrap().id, "pixiv");
        assert_eq!(resolve(" ExHentai ").unwrap().id, "ehentai");
        assert_eq!(resolve("exhentai"), resolve("ehentai"));
        assert!(resolve("missing").is_none());
        assert!(resolve("").is_none());
    }

    // 'captures only the selected SubscribeStar host' and 'rejects an
    // unsupported SubscribeStar login host'
    #[test]
    fn a_subscribestar_service_is_one_of_three_hosts() {
        for host in SUBSCRIBESTAR_HOSTS {
            let site = with_service_host(resolve("subscribestar").unwrap(), host).unwrap();
            assert_eq!(
                site.login_url.as_deref(),
                Some(format!("https://{host}/login").as_str())
            );
            assert_eq!(
                site.cookie_url.as_deref(),
                Some(format!("https://{host}").as_str())
            );
        }
        for host in [
            "evil.test",
            "https://subscribestar.com",
            "subscribestar.com.evil.test",
        ] {
            assert_eq!(
                with_service_host(resolve("subscribestar").unwrap(), host).unwrap_err(),
                "Choose a supported SubscribeStar service."
            );
        }
        assert!(with_service_host(resolve("patreon").unwrap(), "www.subscribestar.com").is_err());
    }

    #[test]
    fn an_origin_moves_with_path_and_query_kept() {
        assert_eq!(
            move_origin(
                "https://gelbooru.com/index.php?page=account&s=options",
                "http://127.0.0.1:8123"
            ),
            "http://127.0.0.1:8123/index.php?page=account&s=options"
        );
        assert_eq!(
            move_origin("https://x.com", "http://127.0.0.1:8123"),
            "http://127.0.0.1:8123/"
        );
        let site = moved(resolve("deviantart").unwrap(), "http://127.0.0.1:9");
        assert!(
            site.redirect_url
                .unwrap()
                .starts_with("http://127.0.0.1:9/gallery-dl/")
        );
        let tumblr = moved(resolve("tumblr").unwrap(), "http://127.0.0.1:9");
        assert_eq!(
            tumblr.redirect_url.as_deref(),
            Some("http://127.0.0.1:9/oauth/callback")
        );
    }
}
