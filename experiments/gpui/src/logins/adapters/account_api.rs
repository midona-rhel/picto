//! `createAccountApiAdapter`: boorus whose login is the API key on the
//! account's options page.
use super::scripts::ACCOUNT_API_SCRIPT;
use super::*;

/// `createAccountApiAdapter`: a booru's API key and user id, read from its
/// account options once the page shows the account signed in.
pub struct AccountApiAdapter {
    pub(super) site: Site,
    pub(super) options_navigation_attempts: Cell<u32>,
}

impl AccountApiAdapter {
    pub(super) async fn inspect(&self, page: &impl Page) -> Outcome {
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
