//! The adapters, one for each way in which the app obtains a login
//! (`sites::Strategy`). An adapter decides which page the sign-in window
//! opens first, what a page must show before the user counts as signed in,
//! and which credential is kept.
//!
//! An adapter reads the sign-in window only through the trait [`Page`], and
//! reaches the network and the application only through the trait
//! [`Services`]. The tests implement both traits with recorded pages,
//! cookies and responses, so they check the adapters without a window and
//! without a network.
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

/// What an adapter can read of the page in the sign-in window.
pub trait Page {
    /// Returns the address of the page that the window shows.
    fn current_url(&self) -> String;
    /// Returns the cookies that the web view would send with a request to
    /// `url`.
    fn cookies(&self, url: &str) -> impl Future<Output = Vec<Cookie>>;
    /// Runs `script` in the page. `script` is a JavaScript expression whose
    /// value is a JSON string. Returns the parsed JSON, or `None` if the
    /// script could not be run.
    fn eval(&self, script: &str) -> impl Future<Output = Option<Value>>;
}

/// A `POST` request with a form as its body. The OAuth adapters send
/// their token requests with it.
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

/// What an adapter can reach outside the sign-in window: the network and
/// the application.
pub trait Services {
    fn post(&self, request: HttpRequest) -> impl Future<Output = Result<HttpResponse, String>>;
    /// Runs a command of the application (`core/src/ipc.rs`) and returns
    /// its result.
    fn dispatch(&self, command: &str, args: Value) -> impl Future<Output = Result<Value, String>>;
}

/// The address of the first page that the sign-in window loads, and the
/// message that the Accounts dialog shows while the user signs in.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub url: String,
    pub message: String,
}

/// What an adapter decided after it looked at the page (`inspect`) or
/// handled the callback (`complete`). `Machine::apply` carries it out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub credential: Option<Credential>,
    /// Whether the application has already saved the login, so that it is
    /// not saved a second time. Only the Pixiv adapter sets it.
    pub already_persisted: bool,
    pub status: Option<Status>,
    pub message: Option<String>,
    /// An address that the sign-in window loads next.
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

/// The adapter of one login. It keeps values from one look at the page to
/// the next, so a new one is made for every login.
pub enum Adapter {
    Cookies(CookieAdapter),
    AccountApi(AccountApiAdapter),
    OAuth(OAuthAdapter),
    Pixiv(PixivAdapter),
}

/// Returns the adapter for `site`. OnlyFans has no adapter, because its
/// login runs in a browser outside the app or is pasted, so for OnlyFans
/// this returns an error.
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
        // i18n: an error text, shown in English in every language
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

    /// Returns whether the page in the sign-in window is looked at every
    /// 750 ms. The OAuth and Pixiv adapters are not: they only wait for the
    /// window to be sent to their callback address.
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

    /// Returns whether `url` is the callback address of the login. The
    /// window does not load that address; it is given to
    /// [`Adapter::complete`] instead.
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
