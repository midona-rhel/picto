//! The adapter for the boorus whose login is the API key that the
//! account's options page shows.
use super::scripts::ACCOUNT_API_SCRIPT;
use super::*;

/// The adapter that keeps a booru's API key and user id as the login. Once
/// the page shows that the user is signed in, it sends the window to the
/// account's options page and reads the two values there.
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
            // A browser check can send the window back to the account page
            // instead of the options page, so the options page is requested
            // up to five times.
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
