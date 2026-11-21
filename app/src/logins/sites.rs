//! The services that the user can sign in to: the address where the login
//! of each one starts, how the app finds out that the login has finished,
//! and what the app keeps of it.
//!
//! A debug build started with `PICTO_GPUI_LOGIN_ORIGIN=http://127.0.0.1:<port>`
//! replaces the scheme, host and port of every address here with those of
//! that origin and keeps the path and the query (`seams::login_origin`). A
//! whole sign-in can then be run against local pages, and no real site is
//! reached.

/// The way in which the app obtains the login of a service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// The app keeps the session cookies that the service sets when the user
    /// signs in on its page.
    Cookies,
    /// The app reads the account's API key from the account's options page
    /// after the user has signed in.
    AccountApi,
    OAuth1,
    OAuth2,
    /// The application core runs Pixiv's OAuth flow (`pixiv_oauth_start`,
    /// `pixiv_oauth_exchange`).
    Pixiv,
    /// The user signs in with a browser that is installed on the computer,
    /// outside the app, or pastes the values of a session.
    OnlyFans,
}

/// The size at which a sign-in window opens and the smallest size the user
/// can give it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
    pub min_width: f32,
    pub min_height: f32,
}

/// The size of the sign-in window of every service that does not use
/// [`WIDE_AUTH_WINDOW`].
pub const AUTH_WINDOW: WindowSize = WindowSize {
    width: 520.,
    height: 760.,
    min_width: 420.,
    min_height: 640.,
};

/// The size of the sign-in window of the services whose login pages need
/// the width of a desktop browser window.
pub const WIDE_AUTH_WINDOW: WindowSize = WindowSize {
    width: 1000.,
    height: 760.,
    min_width: 760.,
    min_height: 640.,
};

/// One service and the addresses that its login uses. [`resolve`] returns
/// it, and [`with_service_host`] sets the addresses of the SubscribeStar
/// service that the user chose.
#[derive(Clone, Debug, PartialEq)]
pub struct Site {
    pub id: &'static str,
    pub label: &'static str,
    pub strategy: Strategy,
    pub login_url: Option<String>,
    pub cookie_url: Option<String>,
    /// When this is set, the cookies of all these addresses are read
    /// together. Only ExHentai sets it.
    pub cookie_urls: Option<Vec<String>>,
    /// The names of the cookies that are kept. When this is `None`, every
    /// cookie of the address is kept.
    pub cookie_names: Option<&'static [&'static str]>,
    /// The names of cookies that only a signed-in session has. When this is
    /// `None`, the login counts as finished only when the page itself shows
    /// that the user is signed in (`adapters::scripts::SIGNED_IN_SCRIPT`).
    pub authenticated_cookie_names: Option<&'static [&'static str]>,
    /// A page that the sign-in window must have loaded before the login is
    /// saved. Only ExHentai sets it.
    pub verification_url: Option<String>,
    /// The address of the account page that shows the API key
    /// (`Strategy::AccountApi`).
    pub options_url: Option<String>,
    pub request_token_url: Option<String>,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub consumer_key: Option<&'static str>,
    pub consumer_secret: Option<&'static str>,
    pub client_id: Option<&'static str>,
    pub client_secret: Option<&'static str>,
    /// Whether the token exchange sends the client's id and secret in an
    /// `Authorization: Basic` header. When this is false they are sent in
    /// the body of the request.
    pub token_auth_basic: bool,
    pub redirect_url: Option<String>,
    pub scope: Option<&'static str>,
    /// The name of the field of the token response whose value is kept.
    /// When this is `None`, the field is `access_token`.
    pub token_field: Option<&'static str>,
    /// An address of FANBOX's account API. The app requests it from the
    /// sign-in page to find out whether the session is signed in.
    pub session_check_url: Option<String>,
    pub window: WindowSize,
}

/// The address to which an OAuth service sends the sign-in window back
/// after the user has authorized the app, for the services that have no
/// `redirect_url` of their own.
pub const OAUTH_CALLBACK_URL: &str = "https://picto.app/oauth/callback";

/// The hosts of the three SubscribeStar services. A login is for one of
/// them, and the app stores one SubscribeStar login at a time.
pub const SUBSCRIBESTAR_HOSTS: [&str; 3] = [
    "www.subscribestar.com",
    "subscribestar.adult",
    "subscribestar.art",
];

/// The ids of all services.
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

/// Returns a service with the strategy `Strategy::Cookies`: the user signs
/// in on the page at `login`, and the cookies of `cookies` are kept.
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

/// Returns the service with the id `id` with its real addresses, before
/// `PICTO_GPUI_LOGIN_ORIGIN` replaces their origin.
fn declared(id: &str) -> Option<Site> {
    // i18n: service names, URLs and protocol fields, not translated
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
        // The client id, the client secret and the redirect address are
        // those of gallery-dl's public DeviantArt application. The refresh
        // token that the login obtains therefore belongs to that
        // application and is refreshed with its client id and secret.
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
                // i18n: the name of a service, the same in every language
                "ExHentai",
                "https://forums.e-hentai.org/index.php?act=Login&CODE=00",
                "https://forums.e-hentai.org",
            )
        },
        _ => return None,
    })
}

/// Returns the service for a site id. Upper and lower case are not
/// distinguished, and spaces around the id are ignored. The id `pixivuser`
/// gives Pixiv, and `exhentai` gives the service with the id `ehentai`.
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

/// Returns `site` with the login and cookie addresses of the SubscribeStar
/// service at `host`. Returns an error if `site` is not SubscribeStar or
/// `host` is not one of [`SUBSCRIBESTAR_HOSTS`].
pub fn with_service_host(mut site: Site, host: &str) -> Result<Site, String> {
    if site.id != "subscribestar" || !SUBSCRIBESTAR_HOSTS.contains(&host) {
        // i18n: an error text, shown in English in every language
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

/// Returns the id and the login address of every service that has a login
/// address.
#[cfg(test)]
pub fn login_routes() -> Vec<(String, String)> {
    SITE_IDS
        .iter()
        .filter_map(|id| declared(id))
        .filter_map(|site| Some((site.id.to_owned(), site.login_url?)))
        .collect()
}

/// Returns `site` with the scheme, host and port of every one of its
/// addresses replaced by those of `origin`.
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
    // An OAuth service without a `redirect_url` of its own uses
    // `OAUTH_CALLBACK_URL`. Its origin is replaced like the others, so that
    // the callback also stays on the local pages.
    if matches!(site.strategy, Strategy::OAuth1 | Strategy::OAuth2) && site.redirect_url.is_none() {
        site.redirect_url = Some(move_origin(OAUTH_CALLBACK_URL, origin));
    }
    site
}

/// Returns `url` with its scheme, host and port replaced by those of
/// `origin`. The path and the query stay. Returns `url` unchanged if either
/// of the two is not a valid address.
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

    // Each of the three SubscribeStar hosts gives its own login and cookie
    // addresses, and every other host is refused.
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
