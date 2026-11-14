//! How each kind of service's login is read (`createCookieAdapter`,
//! `createAccountApiAdapter`, `createOAuthAdapter`, `createPixivAdapter` in
//! `electron/windows/authSessions.mjs`): what page the window opens, what
//! a page must show before the login counts, and the credential kept.
//!
//! Adapters see the sign-in window only through [`Page`] and the network
//! and the application only through [`Services`], so their rules are
//! checked against recorded pages and cookies.
use super::cookies::Cookie;
use super::sites::{OAUTH_CALLBACK_URL, Site, Strategy};
use super::{Credential, Status};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;

/// The sign-in window's page as an adapter reads it.
pub trait Page {
    /// The address the page is at (`webContents.getURL()`).
    fn current_url(&self) -> String;
    /// The cookies a request to `url` would carry.
    fn cookies(&self, url: &str) -> impl Future<Output = Vec<Cookie>>;
    /// Runs `script`, an expression whose value is a JSON string, in the
    /// page, and hands back that JSON; nothing when it could not run.
    fn eval(&self, script: &str) -> impl Future<Output = Option<Value>>;
}

/// A form `POST` (OAuth's token requests).
#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// What a login reaches outside its window.
pub trait Services {
    fn post(&self, request: HttpRequest) -> impl Future<Output = Result<HttpResponse, String>>;
    /// A command of the application (`core/src/ipc.rs`).
    fn dispatch(&self, command: &str, args: Value) -> impl Future<Output = Result<Value, String>>;
}

/// The window's first page and what the Accounts window says meanwhile.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub url: String,
    pub message: String,
}

/// What a look at the page or a completed callback decided
/// (`applyResult`'s argument).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub credential: Option<Credential>,
    /// The application already saved the login (Pixiv).
    pub already_persisted: bool,
    pub status: Option<Status>,
    pub message: Option<String>,
    /// An address the window goes to next.
    pub navigate: Option<String>,
    pub hide: bool,
    pub show: bool,
}

impl Outcome {
    fn waiting(status: Status, message: String) -> Self {
        Self {
            status: Some(status),
            message: Some(message),
            ..Self::default()
        }
    }
    fn active(message: String) -> Self {
        Self::waiting(Status::Active, message)
    }
    fn captured(credential: Credential, message: String) -> Self {
        Self {
            credential: Some(credential),
            message: Some(message),
            ..Self::default()
        }
    }
}

/// One login's adapter, which keeps what it has seen between looks.
pub enum Adapter {
    Cookies(CookieAdapter),
    AccountApi(AccountApiAdapter),
    OAuth(OAuthAdapter),
    Pixiv(PixivAdapter),
}

/// `createAdapter`. OnlyFans has none: its login runs in a browser outside
/// the app or is pasted.
pub fn adapter(site: Site) -> Result<Adapter, String> {
    Ok(match site.strategy {
        Strategy::Cookies => Adapter::Cookies(CookieAdapter {
            site,
            navigating_to_verification: Cell::new(false),
            fanbox_headers: RefCell::new(None),
        }),
        Strategy::AccountApi => Adapter::AccountApi(AccountApiAdapter {
            site,
            options_navigation_attempts: Cell::new(0),
        }),
        Strategy::OAuth1 | Strategy::OAuth2 => Adapter::OAuth(OAuthAdapter {
            site,
            pending: RefCell::new(None),
        }),
        Strategy::Pixiv => Adapter::Pixiv(PixivAdapter {
            site,
            code_verifier: RefCell::new(None),
        }),
        // i18n: production's error text (English)
        Strategy::OnlyFans => return Err("Unsupported authentication strategy: onlyfans".into()),
    })
}

impl Adapter {
    pub fn site(&self) -> &Site {
        match self {
            Adapter::Cookies(adapter) => &adapter.site,
            Adapter::AccountApi(adapter) => &adapter.site,
            Adapter::OAuth(adapter) => &adapter.site,
            Adapter::Pixiv(adapter) => &adapter.site,
        }
    }

    pub async fn prepare(&self, services: &impl Services) -> Result<Prepared, String> {
        let site = self.site();
        match self {
            Adapter::Cookies(_) | Adapter::AccountApi(_) => Ok(Prepared {
                url: site.login_url.clone().unwrap_or_default(),
                message: format!("Log in with {} in the popup window.", site.label),
            }),
            Adapter::OAuth(adapter) => adapter.prepare(services).await,
            Adapter::Pixiv(adapter) => adapter.prepare(services).await,
        }
    }

    /// Whether the window's pages are looked at every 750 ms (OAuth logins
    /// only wait for their callback).
    pub fn inspects(&self) -> bool {
        matches!(self, Adapter::Cookies(_) | Adapter::AccountApi(_))
    }

    pub async fn inspect(&self, page: &impl Page) -> Outcome {
        match self {
            Adapter::Cookies(adapter) => adapter.inspect(page).await,
            Adapter::AccountApi(adapter) => adapter.inspect(page).await,
            Adapter::OAuth(_) | Adapter::Pixiv(_) => Outcome::default(),
        }
    }

    /// Whether `url` is the login's callback, which the window does not
    /// load but hands to [`Adapter::complete`].
    pub fn handles(&self, url: &str) -> bool {
        match self {
            Adapter::OAuth(adapter) => adapter.handles(url),
            Adapter::Pixiv(_) => url.starts_with("pixiv://"),
            Adapter::Cookies(_) | Adapter::AccountApi(_) => false,
        }
    }

    pub async fn complete(
        &self,
        url: &str,
        page: &impl Page,
        services: &impl Services,
    ) -> Result<Outcome, String> {
        match self {
            Adapter::OAuth(adapter) => adapter.complete(url, page, services).await,
            Adapter::Pixiv(adapter) => adapter.complete(url, page, services).await,
            Adapter::Cookies(_) | Adapter::AccountApi(_) => Ok(Outcome::default()),
        }
    }
}

/// `createCookieAdapter`: a login is its session cookies, once they are set
/// and the page shows a signed-in session.
pub struct CookieAdapter {
    site: Site,
    navigating_to_verification: Cell<bool>,
    /// The browser's own headers FANBOX's API needs with its cookies.
    fanbox_headers: RefCell<Option<BTreeMap<String, String>>>,
}

impl CookieAdapter {
    async fn inspect(&self, page: &impl Page) -> Outcome {
        let site = &self.site;
        let label = site.label;
        let log_in = || Outcome::active(format!("Log in with {label} to continue."));
        let current = page.current_url();
        let mut subscribestar_origin = None;
        if site.id == "subscribestar" {
            match subscribestar_origin_of(&current, site.cookie_url.as_deref().unwrap_or_default())
            {
                Some(origin) => subscribestar_origin = Some(origin),
                None => {
                    return Outcome::active(
                        "Sign in on your SubscribeStar service to continue.".into(),
                    );
                }
            }
        }
        // These services have separate accounts; never combine their
        // cookies.
        let cookie_urls = match (&subscribestar_origin, &site.cookie_urls) {
            (Some(origin), _) => vec![origin.clone()],
            (None, Some(urls)) => urls.clone(),
            (None, None) => site.cookie_url.iter().cloned().collect(),
        };
        let mut values: Vec<(String, String)> = Vec::new();
        for url in &cookie_urls {
            for cookie in page.cookies(url).await {
                // `new Map(...)`: a later cookie of the same name wins, in
                // the first one's place.
                match values.iter_mut().find(|(name, _)| *name == cookie.name) {
                    Some(entry) => entry.1 = cookie.value,
                    None => values.push((cookie.name, cookie.value)),
                }
            }
        }
        let value_of = |name: &str| {
            values
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.trim().to_owned())
                .unwrap_or_default()
        };
        let authenticated = site.authenticated_cookie_names.unwrap_or(&[]);
        if authenticated.iter().any(|name| value_of(name).is_empty()) {
            return log_in();
        }
        if let Some(verification) = &site.verification_url {
            if !current.starts_with(verification.as_str()) {
                let message = format!("Verifying {label} access...");
                if !self.navigating_to_verification.get() {
                    self.navigating_to_verification.set(true);
                    return Outcome {
                        navigate: Some(verification.clone()),
                        ..Outcome::waiting(Status::Loading, message)
                    };
                }
                return Outcome::waiting(Status::Loading, message);
            }
            let verdict = page.eval(VERIFICATION_SCRIPT).await.unwrap_or(Value::Null);
            if verdict["challenge"].as_bool() == Some(true) {
                return Outcome::active(format!(
                    "Complete the {label} browser check; Picto will continue automatically."
                ));
            }
            if verdict["denied"].as_bool() == Some(true) || verdict["blank"].as_bool() == Some(true)
            {
                self.navigating_to_verification.set(false);
                return Outcome::active(format!(
                    "{label} access was not accepted. Sign in again to refresh it."
                ));
            }
        }
        if authenticated.is_empty()
            && page.eval(SIGNED_IN_SCRIPT).await.and_then(|v| v.as_bool()) != Some(true)
        {
            return log_in();
        }
        let names: Vec<String> = match site.cookie_names {
            Some(names) => names.iter().map(|name| (*name).to_owned()).collect(),
            None => values.iter().map(|(name, _)| name.clone()).collect(),
        };
        let cookies: BTreeMap<String, String> = names
            .into_iter()
            .map(|name| {
                let value = value_of(&name);
                (name, value)
            })
            .filter(|(_, value)| !value.is_empty())
            .collect();
        if cookies.is_empty() {
            return log_in();
        }
        if site.id == "fanbox" {
            let host = url::Url::parse(&current)
                .ok()
                .and_then(|url| url.host_str().map(str::to_lowercase))
                .unwrap_or_default();
            let on_fanbox = host == "fanbox.cc"
                || host.ends_with(".fanbox.cc")
                || crate::seams::login_origin().is_some_and(|origin| current.starts_with(&origin));
            if !on_fanbox {
                return log_in();
            }
            let check = site.session_check_url.clone().unwrap_or_default();
            let signed_in = page.eval(&fanbox_session_script(&check)).await;
            if signed_in.and_then(|value| value.as_bool()) != Some(true) {
                return log_in();
            }
            if let Some(Value::Object(headers)) = page.eval(BROWSER_HEADERS_SCRIPT).await {
                let headers: BTreeMap<String, String> = headers
                    .into_iter()
                    .filter_map(|(name, value)| {
                        let value = value.as_str()?.trim().to_owned();
                        (!value.is_empty()).then_some((name, value))
                    })
                    .collect();
                if !headers.is_empty() {
                    *self.fanbox_headers.borrow_mut() = Some(headers);
                }
            }
        }
        let headers = match &subscribestar_origin {
            Some(origin) => Some(BTreeMap::from([("Origin".to_owned(), origin.clone())])),
            None if site.id == "fanbox" => self.fanbox_headers.borrow().clone(),
            None => None,
        };
        Outcome::captured(
            Credential {
                site_category: site.id.to_owned(),
                credential_type: "cookies".into(),
                cookies: Some(cookies),
                headers,
                ..Credential::default()
            },
            format!("{label} session captured."),
        )
    }
}

/// The origin of a SubscribeStar page, when it is the chosen service's own
/// over plain HTTPS.
fn subscribestar_origin_of(current: &str, cookie_url: &str) -> Option<String> {
    let url = url::Url::parse(current).ok()?;
    let expected = url::Url::parse(cookie_url).ok()?;
    let host = url.host_str()?;
    let bare = |host: &str| host.strip_prefix("www.").unwrap_or(host).to_owned();
    if let Some(origin) = crate::seams::login_origin() {
        // A local login stands in for the service.
        return current
            .starts_with(&origin)
            .then(|| url.origin().ascii_serialization());
    }
    let service = matches!(
        bare(host).as_str(),
        "subscribestar.com" | "subscribestar.adult" | "subscribestar.art"
    );
    (url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && service
        && bare(host) == bare(expected.host_str()?))
    .then(|| url.origin().ascii_serialization())
}

/// `createAccountApiAdapter`: a booru's API key and user id, read from its
/// account options once the page shows the account signed in.
pub struct AccountApiAdapter {
    site: Site,
    options_navigation_attempts: Cell<u32>,
}

impl AccountApiAdapter {
    async fn inspect(&self, page: &impl Page) -> Outcome {
        let label = self.site.label;
        let found = page.eval(ACCOUNT_API_SCRIPT).await.unwrap_or(Value::Null);
        let flag = |name: &str| found[name].as_bool() == Some(true);
        let text = |name: &str| {
            found[name]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        if flag("acceptedConsent") {
            return Outcome::active(format!("Preparing {label} login…"));
        }
        if flag("hasChallenge") {
            return Outcome::active(format!(
                "Complete the {label} browser check; Picto will continue automatically."
            ));
        }
        if let (Some(api_key), Some(user_id)) = (text("apiKey"), text("userId")) {
            return Outcome::captured(
                Credential {
                    site_category: self.site.id.to_owned(),
                    credential_type: "api_key".into(),
                    username: Some(user_id),
                    password: Some(api_key),
                    ..Credential::default()
                },
                format!("{label} API key captured."),
            );
        }
        let attempts = self.options_navigation_attempts.get();
        if flag("authenticated") && !flag("onOptions") && attempts < 5 {
            // A challenge page can bounce the first hop back to the account
            // page; keep trying rather than giving up.
            self.options_navigation_attempts.set(attempts + 1);
            return Outcome {
                navigate: self.site.options_url.clone(),
                hide: true,
                ..Outcome::waiting(
                    Status::Loading,
                    format!("Authenticated. Reading {label} API credentials…"),
                )
            };
        }
        if flag("onOptions") && attempts > 0 {
            return Outcome {
                show: true,
                ..Outcome::waiting(
                    Status::Error,
                    format!("Logged in, but Picto could not find {label} user_id and api_key."),
                )
            };
        }
        Outcome::active(if flag("hasLoginForm") {
            format!("Log in with {label} to continue.")
        } else {
            format!("Waiting for {label} login…")
        })
    }
}

/// What an OAuth login keeps between opening the authorization page and
/// its callback.
enum OAuthPending {
    Two {
        state: String,
        client_id: String,
        client_secret: String,
    },
    One {
        token: String,
        token_secret: String,
    },
}

/// `createOAuthAdapter`: OAuth 2 (an authorization code exchanged for a
/// token) or OAuth 1 (a request token, then an access token).
pub struct OAuthAdapter {
    site: Site,
    pending: RefCell<Option<OAuthPending>>,
}

impl OAuthAdapter {
    fn callback(&self) -> String {
        self.site
            .redirect_url
            .clone()
            .unwrap_or_else(|| OAUTH_CALLBACK_URL.to_owned())
    }

    async fn prepare(&self, services: &impl Services) -> Result<Prepared, String> {
        let site = &self.site;
        let authorize = site.authorize_url.clone().unwrap_or_default();
        let url = if site.strategy == Strategy::OAuth2 {
            let state = uuid::Uuid::new_v4().to_string();
            let (Some(client_id), Some(client_secret)) = (site.client_id, site.client_secret)
            else {
                return Err(format!("{} has no OAuth client configured.", site.label));
            };
            let url = format!(
                "{authorize}?{}",
                form(&[
                    ("client_id", client_id),
                    ("redirect_uri", &self.callback()),
                    ("response_type", "code"),
                    ("scope", site.scope.unwrap_or("read")),
                    ("state", &state),
                ])
            );
            *self.pending.borrow_mut() = Some(OAuthPending::Two {
                state,
                client_id: client_id.to_owned(),
                client_secret: client_secret.to_owned(),
            });
            url
        } else {
            let request_url = site.request_token_url.clone().unwrap_or_default();
            let callback = self.callback();
            let params = [("oauth_callback", callback.as_str())];
            let authorization = super::oauth::authorization_header(
                &super::oauth::Signing {
                    method: "POST",
                    url: &request_url,
                    params: &params,
                    consumer_key: site.consumer_key.unwrap_or_default(),
                    consumer_secret: site.consumer_secret.unwrap_or_default(),
                    token: None,
                    token_secret: None,
                },
                &super::oauth::nonce(),
                &super::oauth::timestamp(),
            );
            let answer = services
                .post(HttpRequest {
                    url: request_url,
                    headers: form_headers(authorization),
                    body: form(&params),
                })
                .await?;
            let token = response_form(&answer, &format!("{} request token", site.label))?;
            let (Some(request_token), Some(request_secret)) = (
                token.get("oauth_token").filter(|value| !value.is_empty()),
                token
                    .get("oauth_token_secret")
                    .filter(|value| !value.is_empty()),
            ) else {
                return Err(format!(
                    "{} request token response was incomplete.",
                    site.label
                ));
            };
            let url = format!(
                "{authorize}?{}",
                form(&[("oauth_token", request_token), ("perms", "read")])
            );
            *self.pending.borrow_mut() = Some(OAuthPending::One {
                token: request_token.clone(),
                token_secret: request_secret.clone(),
            });
            url
        };
        Ok(Prepared {
            url,
            message: format!("Authorize Picto with {} in the popup window.", site.label),
        })
    }

    fn handles(&self, url: &str) -> bool {
        let (Ok(url), Ok(callback)) = (url::Url::parse(url), url::Url::parse(&self.callback()))
        else {
            return false;
        };
        url.origin() == callback.origin() && url.path() == callback.path()
    }

    async fn complete(
        &self,
        url: &str,
        page: &impl Page,
        services: &impl Services,
    ) -> Result<Outcome, String> {
        let site = &self.site;
        let label = site.label;
        let params: BTreeMap<String, String> = url::Url::parse(url)
            .map(|url| url.query_pairs().into_owned().collect())
            .unwrap_or_default();
        let pending = self.pending.borrow_mut().take();
        match pending {
            Some(OAuthPending::Two {
                state,
                client_id,
                client_secret,
            }) => {
                if params.get("state") != Some(&state) {
                    return Err(format!("{label} OAuth state did not match."));
                }
                let Some(code) = params.get("code").filter(|code| !code.is_empty()) else {
                    return Err(format!("No code in {label} OAuth callback."));
                };
                let callback = self.callback();
                let mut headers = vec![
                    ("accept".to_owned(), "application/json".to_owned()),
                    (
                        "content-type".to_owned(),
                        "application/x-www-form-urlencoded".to_owned(),
                    ),
                ];
                let mut body = Vec::new();
                if site.token_auth_basic {
                    use base64::Engine as _;
                    let pair = format!("{client_id}:{client_secret}");
                    headers.push((
                        "Authorization".to_owned(),
                        format!(
                            "Basic {}",
                            base64::engine::general_purpose::STANDARD.encode(pair)
                        ),
                    ));
                } else {
                    body.push(("client_id", client_id.as_str()));
                    body.push(("client_secret", client_secret.as_str()));
                }
                body.push(("grant_type", "authorization_code"));
                body.push(("code", code));
                body.push(("redirect_uri", &callback));
                let answer = services
                    .post(HttpRequest {
                        url: site.token_url.clone().unwrap_or_default(),
                        headers,
                        body: form(&body),
                    })
                    .await?;
                let token = response_json(&answer, &format!("{label} token exchange"))?;
                let Some(oauth_token) = token[site.token_field.unwrap_or("access_token")]
                    .as_str()
                    .filter(|value| !value.is_empty())
                else {
                    return Err(format!("{label} token response was incomplete."));
                };
                let mut cookies = None;
                if let Some(cookie_url) = &site.cookie_url {
                    let stored = page.cookies(cookie_url).await;
                    let kept: BTreeMap<String, String> = stored
                        .into_iter()
                        .filter(|cookie| {
                            !cookie.value.is_empty()
                                && site
                                    .cookie_names
                                    .is_none_or(|names| names.contains(&cookie.name.as_str()))
                        })
                        .map(|cookie| (cookie.name, cookie.value))
                        .collect();
                    if !kept.is_empty() {
                        cookies = Some(kept);
                    }
                }
                Ok(Outcome::captured(
                    Credential {
                        site_category: site.id.to_owned(),
                        credential_type: "oauth_token".into(),
                        oauth_token: Some(oauth_token.to_owned()),
                        cookies,
                        ..Credential::default()
                    },
                    format!("{label} authorization completed."),
                ))
            }
            Some(OAuthPending::One {
                token,
                token_secret,
            }) => {
                if params.get("oauth_token") != Some(&token) {
                    return Err(format!("{label} OAuth token did not match."));
                }
                let Some(verifier) = params
                    .get("oauth_verifier")
                    .filter(|value| !value.is_empty())
                else {
                    return Err(format!("No verifier in {label} OAuth callback."));
                };
                let token_url = site.token_url.clone().unwrap_or_default();
                let values = [
                    ("oauth_token", token.as_str()),
                    ("oauth_verifier", verifier.as_str()),
                ];
                let authorization = super::oauth::authorization_header(
                    &super::oauth::Signing {
                        method: "POST",
                        url: &token_url,
                        params: &values,
                        consumer_key: site.consumer_key.unwrap_or_default(),
                        consumer_secret: site.consumer_secret.unwrap_or_default(),
                        token: Some(&token),
                        token_secret: Some(&token_secret),
                    },
                    &super::oauth::nonce(),
                    &super::oauth::timestamp(),
                );
                let answer = services
                    .post(HttpRequest {
                        url: token_url,
                        headers: form_headers(authorization),
                        body: form(&values),
                    })
                    .await?;
                let access = response_form(&answer, &format!("{label} access token exchange"))?;
                let (Some(access_token), Some(access_secret)) = (
                    access.get("oauth_token").filter(|value| !value.is_empty()),
                    access
                        .get("oauth_token_secret")
                        .filter(|value| !value.is_empty()),
                ) else {
                    return Err(format!("{label} access token response was incomplete."));
                };
                Ok(Outcome::captured(
                    Credential {
                        site_category: site.id.to_owned(),
                        credential_type: "oauth_token".into(),
                        oauth_token: Some(access_token.clone()),
                        password: Some(access_secret.clone()),
                        ..Credential::default()
                    },
                    format!("{label} authorization completed."),
                ))
            }
            // A callback before the authorization page was prepared.
            None => Err(format!("{label} OAuth state did not match.")),
        }
    }
}

/// `createPixivAdapter`: the application starts Pixiv's OAuth flow and
/// exchanges the code the login redirects to; it saves the login itself.
pub struct PixivAdapter {
    site: Site,
    code_verifier: RefCell<Option<String>>,
}

impl PixivAdapter {
    async fn prepare(&self, services: &impl Services) -> Result<Prepared, String> {
        let challenge = services.dispatch("pixiv_oauth_start", json!({})).await?;
        let login = challenge["login_url"]
            .as_str()
            .filter(|url| !url.is_empty());
        let verifier = challenge["code_verifier"]
            .as_str()
            .filter(|value| !value.is_empty());
        let (Some(login), Some(verifier)) = (login, verifier) else {
            return Err("Pixiv OAuth challenge was incomplete.".into());
        };
        *self.code_verifier.borrow_mut() = Some(verifier.to_owned());
        let url = match crate::seams::login_origin() {
            Some(origin) => super::sites::move_origin(login, &origin),
            None => login.to_owned(),
        };
        Ok(Prepared {
            url,
            message: "Sign in with Pixiv in the popup window.".into(),
        })
    }

    async fn complete(
        &self,
        url: &str,
        page: &impl Page,
        services: &impl Services,
    ) -> Result<Outcome, String> {
        let code = url::Url::parse(url)
            .ok()
            .and_then(|url| {
                url.query_pairs()
                    .find(|(name, _)| name == "code")
                    .map(|(_, value)| value.into_owned())
            })
            .filter(|code| !code.is_empty())
            .ok_or_else(|| "No code in Pixiv callback.".to_owned())?;
        let session_url = match crate::seams::login_origin() {
            Some(origin) => origin,
            None => "https://www.pixiv.net".to_owned(),
        };
        let phpsessid = page
            .cookies(&session_url)
            .await
            .into_iter()
            .find(|cookie| cookie.name == "PHPSESSID")
            .map(|cookie| cookie.value);
        let verifier = self.code_verifier.borrow().clone().unwrap_or_default();
        services
            .dispatch(
                "pixiv_oauth_exchange",
                json!({ "code": code, "code_verifier": verifier, "phpsessid": phpsessid }),
            )
            .await?;
        Ok(Outcome {
            already_persisted: true,
            message: Some("Pixiv authorization completed.".into()),
            ..Outcome::default()
        })
    }
}

/// `formBody`: `application/x-www-form-urlencoded`, as `URLSearchParams`
/// writes it.
pub fn form(values: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(values)
        .finish()
}

fn form_headers(authorization: String) -> Vec<(String, String)> {
    vec![
        (
            "accept".to_owned(),
            "application/x-www-form-urlencoded".to_owned(),
        ),
        ("Authorization".to_owned(), authorization),
        (
            "content-type".to_owned(),
            "application/x-www-form-urlencoded".to_owned(),
        ),
    ]
}

/// `responseForm`: a form answer, or why it failed.
fn response_form(
    response: &HttpResponse,
    operation: &str,
) -> Result<BTreeMap<String, String>, String> {
    if !response.ok() {
        return Err(format!("{operation} failed: HTTP {}", response.status));
    }
    let values: BTreeMap<String, String> = url::form_urlencoded::parse(response.body.as_bytes())
        .into_owned()
        .collect();
    if let Some(error) = values.get("error") {
        return Err(format!("{operation} failed: {error}"));
    }
    Ok(values)
}

/// `responseJson`: a JSON answer, or why it failed.
fn response_json(response: &HttpResponse, operation: &str) -> Result<Value, String> {
    let body: Value =
        serde_json::from_str(&response.body).unwrap_or_else(|_| json!({ "error": response.body }));
    if !response.ok() {
        let reason = [&body["error_description"], &body["error"]]
            .into_iter()
            .find_map(|value| value.as_str().filter(|text| !text.is_empty()))
            .map(str::to_owned)
            .unwrap_or_else(|| response.status.to_string());
        return Err(format!("{operation} failed: {reason}"));
    }
    Ok(body)
}

/// `inspectAccountApiPage`: the account page's state, its consent banner
/// accepted, and the API key and user id wherever the page shows them.
const ACCOUNT_API_SCRIPT: &str = r#"JSON.stringify((() => {
  const text = document.body?.innerText || '';
  const href = location.href;
  const consent = /agree to the usage of cookies according to our cookie policy/i.test(text)
    ? Array.from(document.querySelectorAll('button, a, [role="button"], input[type="button"], input[type="submit"]'))
        .find((element) => ((element.textContent || element.value || '').trim().toLowerCase() === 'accept'))
    : null;
  if (consent) consent.click();
  const inputs = Array.from(document.querySelectorAll('input, textarea'));
  const readInput = (...needles) => {
    for (const input of inputs) {
      const key = (input.name || input.id || '').toLowerCase();
      const value = (input.value || '').trim();
      if (value && needles.some((needle) => key.includes(needle))) return value;
    }
    return null;
  };
  const snippets = [
    text,
    href,
    ...inputs.map((input) => (input.value || '').trim()),
    ...Array.from(document.querySelectorAll('a[href]')).map((anchor) => anchor.getAttribute('href') || ''),
  ];
  const combined = snippets.map((value) => {
    const apiKey = value.match(/api[_-]?key=([a-f0-9]{16,})/i)?.[1] || null;
    const userId = value.match(/user[_-]?id=(\d+)/i)?.[1] || null;
    return apiKey ? { apiKey, userId } : null;
  }).find(Boolean);
  const apiKeyInput = readInput('api_key', 'api-key');
  const apiKey = /^[a-f0-9]{16,}$/i.test(apiKeyInput || '')
    ? apiKeyInput
    : text.match(/api[_ -]?key\s*[:\n]\s*([a-f0-9]{16,})/i)?.[1] || combined?.apiKey || null;
  const userId = readInput('user_id', 'user-id')
    || text.match(/user[_ -]?id\s*[:\n]\s*(\d+)/i)?.[1]
    || combined?.userId
    || null;
  const labels = Array.from(document.querySelectorAll('a')).map((anchor) =>
    (anchor.textContent || '').trim().toLowerCase().replace(/[^a-z]+/g, ' ').trim()
  );
  const hasChallenge = Boolean(document.querySelector('#challenge-running, #challenge-stage, .cf-challenge'))
    || /just a moment|verify (?:that )?you are human|checking your browser|cloudflare/i.test((document.title || '') + '\n' + text);
  return {
    href,
    title: document.title || null,
    acceptedConsent: Boolean(consent),
    hasChallenge,
    hasLoginForm: Boolean(document.querySelector('input[name="user"], input[name="username"], input[type="password"]')),
    authenticated: labels.some((label) => {
      const collapsed = label.replace(/ /g, '');
      return collapsed === 'logout' || collapsed.endsWith('logout');
    })
      || /account home/i.test(text)
      || (/[?&]page=account\b/i.test(href) && /[?&]s=home\b/i.test(href)),
    onOptions: /[?&]s=options(?:&|$)/i.test(href),
    apiKey,
    userId,
  };
})())"#;

/// `hasAuthenticatedDomSignal`: whether the page shows a signed-in session.
const SIGNED_IN_SCRIPT: &str = r#"JSON.stringify((() => {
  const normalize = (value) => (value || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const hasLoginForm = Boolean(document.querySelector('input[type="password"], input[name*="password" i]'));
  const controls = Array.from(document.querySelectorAll('a, button, [role="button"], [role="link"], [role="menuitem"]'));
  const describe = (element) => ({
    label: normalize([element.textContent, element.getAttribute('aria-label'), element.getAttribute('title')].filter(Boolean).join(' ')),
    href: normalize(element.getAttribute('href')),
  });
  const logoutPattern = /\b(?:log ?out|sign ?out)\b/i;
  if (controls.some((element) => {
    const { label, href } = describe(element);
    return logoutPattern.test(label) || /\/(?:logout|sign_out)(?:[/?#]|$)/i.test(href);
  })) return true;
  if (hasLoginForm) return false;
  const authPattern = /\b(?:my account|account home|account settings|profile|dashboard)\b/i;
  return controls.some((element) => {
    const { label, href } = describe(element);
    const consent = /(?:cookie|privacy|tracking|consent)/i.test(label)
      && /(?:accept|agree|allow|reject|settings|preferences)/i.test(label);
    return !consent && (authPattern.test(label) || /\/(?:profile|account|dashboard)(?:[/?#]|$)/i.test(href));
  }) || authPattern.test(normalize(document.body?.innerText));
})())"#;

/// ExHentai's verification page: a browser check, a refusal, or a blank
/// page.
const VERIFICATION_SCRIPT: &str = r#"JSON.stringify((() => {
  const text = [
    document.title,
    document.body?.innerText || '',
    ...Array.from(document.images).flatMap((image) => [image.src, image.alt, image.title]),
  ].join(' ');
  return {
    challenge: /just a moment|verify (?:that )?you are human|checking your browser|cloudflare/i.test(text),
    denied: /sad\s*panda|sadpanda|kokomade/i.test(text),
    blank: (document.body?.innerText || '').trim().length === 0
      && document.links.length === 0
      && document.images.length === 0,
  };
})())"#;

/// The headers the web view sends with its requests that FANBOX's API
/// checks against the session: its user agent.
const BROWSER_HEADERS_SCRIPT: &str =
    r#"JSON.stringify({ 'user-agent': navigator.userAgent || '' })"#;

/// `hasAuthenticatedFanboxSession`: whether FANBOX's API answers for the
/// session. A web view hands back no promise's value, so the page keeps the
/// answer of the request it starts and the next look reads it (`null`
/// while it is on its way).
fn fanbox_session_script(check_url: &str) -> String {
    let url = serde_json::to_string(check_url).unwrap_or_else(|_| "''".into());
    format!(
        r#"JSON.stringify((() => {{
  const answer = window.__pictoFanboxSession;
  if (answer === true || answer === false) {{
    window.__pictoFanboxSession = undefined;
    return answer;
  }}
  if (answer !== null) {{
    window.__pictoFanboxSession = null;
    fetch({url}, {{ credentials: 'include', headers: {{ accept: 'application/json' }} }})
      .then((response) => response.ok ? response.json() : null)
      .then((payload) => {{ window.__pictoFanboxSession = Boolean(payload && payload.body !== null && payload.body !== undefined); }})
      .catch(() => {{ window.__pictoFanboxSession = false; }});
  }}
  return null;
}})())"#
    )
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::logins::sites::{resolve, with_service_host};
    use futures::executor::block_on;
    use std::rc::Rc;

    /// A recorded page: its address, the cookies its store holds by the
    /// address asked for, and what its scripts answer.
    #[derive(Default)]
    pub struct FakePage {
        pub url: RefCell<String>,
        pub cookies: RefCell<Vec<(String, String)>>,
        /// Answers by a fragment of the script asked for.
        pub scripts: RefCell<Vec<(&'static str, Value)>>,
        pub asked: RefCell<Vec<String>>,
    }

    impl FakePage {
        pub fn at(url: &str) -> Self {
            Self {
                url: RefCell::new(url.to_owned()),
                ..Self::default()
            }
        }
        pub fn with_cookies(self, cookies: &[(&str, &str)]) -> Self {
            *self.cookies.borrow_mut() = cookies
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect();
            self
        }
        pub fn answering(self, fragment: &'static str, value: Value) -> Self {
            self.scripts.borrow_mut().push((fragment, value));
            self
        }
    }

    impl Page for FakePage {
        fn current_url(&self) -> String {
            self.url.borrow().clone()
        }
        async fn cookies(&self, url: &str) -> Vec<Cookie> {
            self.asked.borrow_mut().push(url.to_owned());
            self.cookies
                .borrow()
                .iter()
                .map(|(name, value)| Cookie {
                    name: name.clone(),
                    value: value.clone(),
                    domain: String::new(),
                    path: "/".into(),
                    secure: false,
                })
                .collect()
        }
        async fn eval(&self, script: &str) -> Option<Value> {
            self.scripts
                .borrow()
                .iter()
                .find(|(fragment, _)| script.contains(fragment))
                .map(|(_, value)| value.clone())
        }
    }

    /// Recorded answers to the network and the application.
    #[derive(Default)]
    pub struct FakeServices {
        pub posts: RefCell<Vec<HttpRequest>>,
        pub answers: RefCell<Vec<HttpResponse>>,
        pub commands: RefCell<Vec<(String, Value)>>,
        pub command_answers: RefCell<Vec<Value>>,
    }

    impl Services for FakeServices {
        async fn post(&self, request: HttpRequest) -> Result<HttpResponse, String> {
            self.posts.borrow_mut().push(request);
            Ok(self.answers.borrow_mut().remove(0))
        }
        async fn dispatch(&self, command: &str, args: Value) -> Result<Value, String> {
            self.commands.borrow_mut().push((command.to_owned(), args));
            Ok(if self.command_answers.borrow().is_empty() {
                Value::Null
            } else {
                self.command_answers.borrow_mut().remove(0)
            })
        }
    }

    fn cookies_of(outcome: &Outcome) -> Vec<(String, String)> {
        outcome
            .credential
            .as_ref()
            .and_then(|credential| credential.cookies.clone())
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn inspect(adapter: &Adapter, page: &FakePage) -> Outcome {
        block_on(adapter.inspect(page))
    }

    // 'captures only the selected SubscribeStar host'
    #[test]
    fn subscribestar_keeps_the_chosen_services_cookies_and_origin() {
        for host in [
            "www.subscribestar.com",
            "subscribestar.adult",
            "subscribestar.art",
        ] {
            let site = with_service_host(resolve("subscribestar").unwrap(), host).unwrap();
            let adapter = adapter(site).unwrap();
            let page = FakePage::at(&format!("https://{host}/account"))
                .with_cookies(&[("_personalization_id", "fixture-session")]);
            let outcome = inspect(&adapter, &page);
            assert!(
                page.asked
                    .borrow()
                    .iter()
                    .all(|url| *url == format!("https://{host}"))
            );
            let credential = outcome.credential.unwrap();
            assert_eq!(credential.site_category, "subscribestar");
            assert_eq!(
                credential.headers,
                Some(BTreeMap::from([(
                    "Origin".into(),
                    format!("https://{host}")
                )]))
            );
            assert_eq!(
                credential.cookies,
                Some(BTreeMap::from([(
                    "_personalization_id".into(),
                    "fixture-session".into()
                )]))
            );
        }
    }

    // 'does not capture a SubscribeStar session from a different origin'
    // and 'does not capture a different SubscribeStar service after a
    // redirect'
    #[test]
    fn subscribestar_reads_no_cookies_off_its_service() {
        for (host, current) in [
            (
                "www.subscribestar.com",
                "https://subscribestar.com.evil.test/account",
            ),
            (
                "subscribestar.adult",
                "https://www.subscribestar.com/account",
            ),
        ] {
            let site = with_service_host(resolve("subscribestar").unwrap(), host).unwrap();
            let adapter = adapter(site).unwrap();
            let page = FakePage::at(current).with_cookies(&[("_personalization_id", "fixture")]);
            let outcome = inspect(&adapter, &page);
            assert!(page.asked.borrow().is_empty());
            assert!(outcome.credential.is_none());
            assert_eq!(
                outcome.message.as_deref(),
                Some("Sign in on your SubscribeStar service to continue.")
            );
        }
    }

    // 'verifies ExHentai in the managed login popup before persisting its
    // gallery-dl cookies'
    #[test]
    fn exhentai_is_verified_before_its_cookies_are_kept() {
        let adapter = adapter(resolve("ehentai").unwrap()).unwrap();
        let page = FakePage::at("https://forums.e-hentai.org/index.php?act=Login&CODE=00")
            .with_cookies(&[
                ("ipb_member_id", "member"),
                ("ipb_pass_hash", "hash"),
                ("igneous", "igneous"),
            ]);
        let first = inspect(&adapter, &page);
        assert_eq!(first.navigate.as_deref(), Some("https://exhentai.org/"));
        assert_eq!(first.status, Some(Status::Loading));
        assert_eq!(
            first.message.as_deref(),
            Some("Verifying ExHentai access...")
        );
        // A second look before the page moved does not navigate again.
        assert_eq!(inspect(&adapter, &page).navigate, None);
        assert_eq!(
            *page.asked.borrow(),
            [
                "https://forums.e-hentai.org",
                "https://e-hentai.org",
                "https://exhentai.org",
                "https://forums.e-hentai.org",
                "https://e-hentai.org",
                "https://exhentai.org"
            ]
        );

        *page.url.borrow_mut() = "https://exhentai.org/".into();
        let done = inspect(&adapter, &page);
        assert_eq!(
            cookies_of(&done),
            pairs(&[
                ("igneous", "igneous"),
                ("ipb_member_id", "member"),
                ("ipb_pass_hash", "hash")
            ])
        );
        assert_eq!(done.message.as_deref(), Some("ExHentai session captured."));
    }

    // 'accepts an existing ExHentai session when the optional igneous
    // cookie is absent'
    #[test]
    fn exhentai_needs_only_the_member_cookies() {
        let adapter = adapter(resolve("ehentai").unwrap()).unwrap();
        let page = FakePage::at("https://exhentai.org/")
            .with_cookies(&[("ipb_member_id", "member"), ("ipb_pass_hash", "hash")]);
        assert_eq!(
            cookies_of(&inspect(&adapter, &page)),
            pairs(&[("ipb_member_id", "member"), ("ipb_pass_hash", "hash")])
        );
    }

    #[test]
    fn a_refused_exhentai_check_asks_to_sign_in_again() {
        let adapter = adapter(resolve("ehentai").unwrap()).unwrap();
        let page = FakePage::at("https://exhentai.org/")
            .with_cookies(&[("ipb_member_id", "member"), ("ipb_pass_hash", "hash")])
            .answering(
                "sadpanda",
                json!({ "challenge": false, "denied": true, "blank": false }),
            );
        let outcome = inspect(&adapter, &page);
        assert!(outcome.credential.is_none());
        assert_eq!(
            outcome.message.as_deref(),
            Some("ExHentai access was not accepted. Sign in again to refresh it.")
        );
    }

    // 'persists cookie credentials in the host and never sends secrets to
    // the renderer': a service without required cookies keeps every cookie
    // once the page shows the session.
    #[test]
    fn danbooru_keeps_every_cookie_once_the_page_shows_a_session() {
        let adapter = adapter(resolve("danbooru").unwrap()).unwrap();
        let page = FakePage::at("https://danbooru.donmai.us/")
            .with_cookies(&[("_session", "secret"), ("user", "42")]);
        assert_eq!(
            inspect(&adapter, &page).message.as_deref(),
            Some("Log in with Danbooru to continue.")
        );
        let page = page.answering("logoutPattern", json!(true));
        let outcome = inspect(&adapter, &page);
        assert_eq!(
            outcome.credential,
            Some(Credential {
                site_category: "danbooru".into(),
                credential_type: "cookies".into(),
                cookies: Some(BTreeMap::from([
                    ("_session".into(), "secret".into()),
                    ("user".into(), "42".into())
                ])),
                ..Credential::default()
            })
        );
    }

    // 'captures only cookies required by a source'
    #[test]
    fn fur_affinity_keeps_only_its_two_cookies() {
        let adapter = adapter(resolve("furaffinity").unwrap()).unwrap();
        let page = FakePage::at("https://www.furaffinity.net/").with_cookies(&[
            ("a", "required-a"),
            ("b", "required-b"),
            ("analytics", "ignored"),
        ]);
        assert_eq!(
            cookies_of(&inspect(&adapter, &page)),
            pairs(&[("a", "required-a"), ("b", "required-b")])
        );
        let half = FakePage::at("https://www.furaffinity.net/").with_cookies(&[("a", "x")]);
        assert!(inspect(&adapter, &half).credential.is_none());
    }

    #[test]
    fn newgrounds_waits_for_its_session_cookie() {
        let adapter = adapter(resolve("newgrounds").unwrap()).unwrap();
        let before = FakePage::at("https://www.newgrounds.com/login");
        let outcome = inspect(&adapter, &before);
        assert_eq!(outcome.status, Some(Status::Active));
        assert_eq!(
            outcome.message.as_deref(),
            Some("Log in with Newgrounds to continue.")
        );
        let after = FakePage::at("https://www.newgrounds.com/")
            .with_cookies(&[("ng_session", " signed-in "), ("vmk1du5I8m", "x")]);
        let outcome = inspect(&adapter, &after);
        assert_eq!(cookies_of(&outcome), pairs(&[("ng_session", "signed-in")]));
        assert_eq!(
            outcome.message.as_deref(),
            Some("Newgrounds session captured.")
        );
    }

    // 'does not save a FANBOX cookie before the login redirects to FANBOX'
    #[test]
    fn fanbox_waits_until_the_login_reaches_fanbox() {
        let adapter = adapter(resolve("fanbox").unwrap()).unwrap();
        let page = FakePage::at("https://accounts.pixiv.net/login")
            .with_cookies(&[("FANBOXSESSID", "session")]);
        let outcome = inspect(&adapter, &page);
        assert!(outcome.credential.is_none());
        assert_eq!(outcome.status, Some(Status::Active));
    }

    // 'saves a FANBOX cookie after the login redirects to FANBOX' and 'does
    // not treat FANBOX guest cookies as an authenticated login'
    #[test]
    fn fanbox_keeps_its_cookies_only_when_its_api_answers_for_the_session() {
        let adapter = adapter(resolve("fanbox").unwrap()).unwrap();
        let guest = FakePage::at("https://www.fanbox.cc/")
            .with_cookies(&[("FANBOXSESSID", "guest")])
            .answering("__pictoFanboxSession", json!(false));
        assert!(inspect(&adapter, &guest).credential.is_none());
        let pending = FakePage::at("https://www.fanbox.cc/")
            .with_cookies(&[("FANBOXSESSID", "valid")])
            .answering("__pictoFanboxSession", Value::Null);
        assert!(inspect(&adapter, &pending).credential.is_none());

        let page = FakePage::at("https://www.fanbox.cc/")
            .with_cookies(&[("FANBOXSESSID", "valid"), ("__cf_bm", "browser-session")])
            .answering("__pictoFanboxSession", json!(true))
            .answering(
                "navigator.userAgent",
                json!({ "user-agent": "WebKit browser" }),
            );
        let outcome = inspect(&adapter, &page);
        let credential = outcome.credential.unwrap();
        assert_eq!(
            credential.cookies,
            Some(BTreeMap::from([
                ("FANBOXSESSID".into(), "valid".into()),
                ("__cf_bm".into(), "browser-session".into())
            ]))
        );
        assert_eq!(
            credential.headers,
            Some(BTreeMap::from([(
                "user-agent".into(),
                "WebKit browser".into()
            )]))
        );
        assert_eq!(*page.asked.borrow(), ["https://api.fanbox.cc"]);
    }

    fn account_page(url: &str, fields: Value) -> FakePage {
        let mut answer = json!({
            "href": url, "title": "Account", "acceptedConsent": false, "hasChallenge": false,
            "hasLoginForm": false, "authenticated": false, "onOptions": false,
            "apiKey": null, "userId": null,
        });
        for (key, value) in fields.as_object().unwrap() {
            answer[key] = value.clone();
        }
        FakePage::at(url).answering("inputs", answer)
    }

    // 'reads booru API credentials from account settings after login'
    #[test]
    fn a_booru_login_reads_its_api_key_from_the_options_page() {
        let adapter = adapter(resolve("rule34").unwrap()).unwrap();
        let home = account_page(
            "https://rule34.xxx/index.php?page=account&s=home",
            json!({ "authenticated": true }),
        );
        let outcome = inspect(&adapter, &home);
        assert!(outcome.hide);
        assert!(
            outcome
                .navigate
                .as_deref()
                .is_some_and(|url| url.contains("s=options"))
        );
        assert_eq!(
            outcome.message.as_deref(),
            Some("Authenticated. Reading Rule34.xxx API credentials…")
        );
        let options = account_page(
            "https://rule34.xxx/index.php?page=account&s=options",
            json!({ "authenticated": true, "onOptions": true, "apiKey": "a".repeat(32), "userId": "599664" }),
        );
        let outcome = inspect(&adapter, &options);
        assert_eq!(
            outcome.credential,
            Some(Credential {
                site_category: "rule34".into(),
                credential_type: "api_key".into(),
                username: Some("599664".into()),
                password: Some("a".repeat(32)),
                ..Credential::default()
            })
        );
        assert_eq!(
            outcome.message.as_deref(),
            Some("Rule34.xxx API key captured.")
        );
    }

    #[test]
    fn a_booru_options_page_without_a_key_is_an_error_shown_in_the_window() {
        let adapter = adapter(resolve("gelbooru").unwrap()).unwrap();
        let home = account_page("https://gelbooru.com/", json!({ "authenticated": true }));
        inspect(&adapter, &home);
        let options = account_page(
            "https://gelbooru.com/index.php?page=account&s=options",
            json!({ "authenticated": true, "onOptions": true }),
        );
        let outcome = inspect(&adapter, &options);
        assert!(outcome.show);
        assert_eq!(outcome.status, Some(Status::Error));
        assert_eq!(
            outcome.message.as_deref(),
            Some("Logged in, but Picto could not find Gelbooru user_id and api_key.")
        );
    }

    // 'keeps browser challenges active without saving a false login'
    #[test]
    fn a_browser_check_keeps_the_login_waiting() {
        let adapter = adapter(resolve("rule34").unwrap()).unwrap();
        let page = account_page(
            "https://rule34.xxx/challenge",
            json!({ "hasChallenge": true, "title": "Just a moment" }),
        );
        let outcome = inspect(&adapter, &page);
        assert!(outcome.credential.is_none());
        assert_eq!(outcome.status, Some(Status::Active));
        assert!(outcome.message.unwrap().contains("browser check"));
        let form = account_page("https://rule34.xxx/", json!({ "hasLoginForm": true }));
        assert_eq!(
            inspect(&adapter, &form).message.as_deref(),
            Some("Log in with Rule34.xxx to continue.")
        );
        let blank = account_page("https://rule34.xxx/", json!({}));
        assert_eq!(
            inspect(&adapter, &blank).message.as_deref(),
            Some("Waiting for Rule34.xxx login…")
        );
    }

    // 'uses gallery-dl DeviantArt OAuth and keeps its refresh token plus
    // session cookies'
    #[test]
    fn deviantart_exchanges_its_code_for_the_refresh_token_with_basic_auth() {
        let adapter = adapter(resolve("deviantart").unwrap()).unwrap();
        let services = FakeServices::default();
        let prepared = block_on(adapter.prepare(&services)).unwrap();
        let authorize = url::Url::parse(&prepared.url).unwrap();
        assert_eq!(
            format!(
                "{}{}",
                authorize.origin().ascii_serialization(),
                authorize.path()
            ),
            "https://www.deviantart.com/oauth2/authorize"
        );
        let query: BTreeMap<String, String> = authorize.query_pairs().into_owned().collect();
        assert_eq!(
            query["redirect_uri"],
            "https://mikf.github.io/gallery-dl/oauth-redirect.html"
        );
        assert_eq!(query["scope"], "browse user.manage");
        assert_eq!(query["response_type"], "code");
        assert_eq!(
            prepared.message,
            "Authorize Picto with DeviantArt in the popup window."
        );
        let callback = format!(
            "https://mikf.github.io/gallery-dl/oauth-redirect.html?code=code&state={}",
            query["state"]
        );
        assert!(adapter.handles(&callback));
        assert!(!adapter.handles("https://mikf.github.io/gallery-dl/other.html?code=x"));

        services.answers.borrow_mut().push(HttpResponse {
            status: 200,
            body: r#"{"access_token":"temporary-access","refresh_token":"durable-refresh"}"#.into(),
        });
        let page = FakePage::at(&callback).with_cookies(&[
            ("auth", "auth-cookie"),
            ("auth_secure", "secure-cookie"),
            ("userinfo", "user-cookie"),
            ("unrelated", "ignored"),
        ]);
        let outcome = block_on(adapter.complete(&callback, &page, &services)).unwrap();
        let posts = services.posts.borrow();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].url, "https://www.deviantart.com/oauth2/token");
        {
            use base64::Engine as _;
            let basic = base64::engine::general_purpose::STANDARD
                .encode("5388:76b08c69cfb27f26d6161f9ab6d061a1");
            assert!(
                posts[0]
                    .headers
                    .contains(&("Authorization".into(), format!("Basic {basic}")))
            );
        }
        let body: BTreeMap<String, String> = url::form_urlencoded::parse(posts[0].body.as_bytes())
            .into_owned()
            .collect();
        assert!(!body.contains_key("client_secret"));
        assert_eq!(body["grant_type"], "authorization_code");
        let credential = outcome.credential.unwrap();
        assert_eq!(credential.credential_type, "oauth_token");
        assert_eq!(credential.oauth_token.as_deref(), Some("durable-refresh"));
        assert_eq!(
            credential.cookies,
            Some(BTreeMap::from([
                ("auth".into(), "auth-cookie".into()),
                ("auth_secure".into(), "secure-cookie".into()),
                ("userinfo".into(), "user-cookie".into())
            ]))
        );
    }

    #[test]
    fn an_oauth2_callback_with_another_state_is_refused() {
        let adapter = adapter(resolve("deviantart").unwrap()).unwrap();
        let services = FakeServices::default();
        block_on(adapter.prepare(&services)).unwrap();
        let page = FakePage::default();
        assert_eq!(
            block_on(adapter.complete(
                "https://mikf.github.io/gallery-dl/oauth-redirect.html?code=c&state=forged",
                &page,
                &services
            ))
            .unwrap_err(),
            "DeviantArt OAuth state did not match."
        );
    }

    // 'persists both OAuth1 token values through the same host boundary'
    #[test]
    fn tumblr_keeps_both_access_token_values() {
        let adapter = adapter(resolve("tumblr").unwrap()).unwrap();
        let services = FakeServices::default();
        services.answers.borrow_mut().extend([
            HttpResponse {
                status: 200,
                body: "oauth_token=request&oauth_token_secret=request-secret".into(),
            },
            HttpResponse {
                status: 200,
                body: "oauth_token=access&oauth_token_secret=access-secret".into(),
            },
        ]);
        let prepared = block_on(adapter.prepare(&services)).unwrap();
        assert_eq!(
            prepared.url,
            "https://www.tumblr.com/oauth/authorize?oauth_token=request&perms=read"
        );
        let callback =
            "https://picto.app/oauth/callback?oauth_token=request&oauth_verifier=verified";
        assert!(adapter.handles(callback));
        let outcome =
            block_on(adapter.complete(callback, &FakePage::default(), &services)).unwrap();
        let credential = outcome.credential.unwrap();
        assert_eq!(credential.oauth_token.as_deref(), Some("access"));
        assert_eq!(credential.password.as_deref(), Some("access-secret"));
        let posts = services.posts.borrow();
        assert_eq!(posts[0].url, "https://www.tumblr.com/oauth/request_token");
        assert_eq!(
            posts[0].body,
            "oauth_callback=https%3A%2F%2Fpicto.app%2Foauth%2Fcallback"
        );
        assert!(posts[1].body.contains("oauth_verifier=verified"));
        let authorization = &posts[1]
            .headers
            .iter()
            .find(|(name, _)| name == "Authorization")
            .unwrap()
            .1;
        assert!(authorization.starts_with("OAuth "));
        assert!(authorization.contains("oauth_token=\"request\""));
    }

    // 'uses one host-owned Pixiv OAuth flow for Pixiv and Pixiv-user
    // queries'
    #[test]
    fn pixiv_hands_its_code_and_session_to_the_application() {
        let adapter = adapter(resolve("pixivuser").unwrap()).unwrap();
        let services = FakeServices::default();
        services.command_answers.borrow_mut().push(json!({
            "login_url": "https://app-api.pixiv.net/web/v1/login",
            "code_verifier": "verifier",
        }));
        let prepared = block_on(adapter.prepare(&services)).unwrap();
        assert_eq!(prepared.url, "https://app-api.pixiv.net/web/v1/login");
        assert!(adapter.handles("pixiv://account/login?code=oauth-code"));
        let page = FakePage::default().with_cookies(&[("PHPSESSID", "pixiv-session")]);
        let outcome =
            block_on(adapter.complete("pixiv://account/login?code=oauth-code", &page, &services))
                .unwrap();
        assert!(outcome.already_persisted);
        assert_eq!(
            outcome.message.as_deref(),
            Some("Pixiv authorization completed.")
        );
        let commands = services.commands.borrow();
        assert_eq!(commands[0].0, "pixiv_oauth_start");
        assert_eq!(
            commands[1],
            (
                "pixiv_oauth_exchange".to_owned(),
                json!({ "code": "oauth-code", "code_verifier": "verifier", "phpsessid": "pixiv-session" })
            )
        );
    }

    #[test]
    fn only_page_logins_are_looked_at_and_onlyfans_has_no_adapter() {
        assert!(adapter(resolve("patreon").unwrap()).unwrap().inspects());
        assert!(adapter(resolve("gelbooru").unwrap()).unwrap().inspects());
        assert!(!adapter(resolve("tumblr").unwrap()).unwrap().inspects());
        assert!(!adapter(resolve("pixiv").unwrap()).unwrap().inspects());
        assert!(adapter(resolve("onlyfans").unwrap()).is_err());
        let _ = Rc::new(());
    }

    #[test]
    fn the_page_scripts_are_expressions_that_return_json() {
        for script in [
            ACCOUNT_API_SCRIPT,
            SIGNED_IN_SCRIPT,
            VERIFICATION_SCRIPT,
            BROWSER_HEADERS_SCRIPT,
            &fanbox_session_script("https://api.fanbox.cc/creator.listFollowing"),
        ] {
            assert!(script.starts_with("JSON.stringify("), "{script}");
            assert!(script.ends_with(')'), "{script}");
        }
    }
}
