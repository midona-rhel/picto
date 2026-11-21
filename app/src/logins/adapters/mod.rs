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

mod account_api;
mod cookie;
mod oauth;
mod scripts;
#[cfg(test)]
pub mod tests;

pub use account_api::AccountApiAdapter;
pub use cookie::CookieAdapter;
pub use oauth::{OAuthAdapter, PixivAdapter};

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
