//! `createCookieAdapter`: services whose login is the session cookies a
//! normal page login sets.
use super::scripts::{BROWSER_HEADERS_SCRIPT, SIGNED_IN_SCRIPT, VERIFICATION_SCRIPT, fanbox_session_script};
use super::*;

/// `createCookieAdapter`: a login is its session cookies, once they are set
/// and the page shows a signed-in session.
pub struct CookieAdapter {
    pub(super) site: Site,
    pub(super) navigating_to_verification: Cell<bool>,
    /// The browser's own headers FANBOX's API needs with its cookies.
    pub(super) fanbox_headers: RefCell<Option<BTreeMap<String, String>>>,
}

impl CookieAdapter {
    pub(super) async fn inspect(&self, page: &impl Page) -> Outcome {
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
