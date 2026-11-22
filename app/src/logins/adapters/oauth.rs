//! The adapter for the services whose login is an OAuth authorization
//! (OAuth 1.0a or OAuth 2), and the adapter for Pixiv, whose OAuth flow the
//! application core runs. The file also holds the signing of OAuth 1.0a
//! requests as RFC 5849 defines it: an HMAC-SHA1 over the method, the
//! address and every parameter.
use super::*;
use hmac::{Hmac, Mac};

/// The values that an OAuth login keeps from the moment it opens the
/// authorization page, for as long as the sign-in window is open. Every
/// callback is checked against them. They are not cleared after a
/// callback, so if the user refuses the authorization and then grants it
/// in the same window, the second callback is checked against them too.
#[derive(Clone)]
pub(super) enum OAuthPending {
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

/// The adapter for OAuth logins. With OAuth 2 it exchanges the
/// authorization code of the callback for a token. With OAuth 1 it obtains
/// a request token first and exchanges it for an access token after the
/// callback.
pub struct OAuthAdapter {
    pub(super) site: Site,
    pub(super) pending: RefCell<Option<OAuthPending>>,
}

impl OAuthAdapter {
    pub(super) fn callback(&self) -> String {
        self.site
            .redirect_url
            .clone()
            .unwrap_or_else(|| OAUTH_CALLBACK_URL.to_owned())
    }

    pub(super) async fn prepare(&self, services: &impl Services) -> Result<Prepared, String> {
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
            let authorization = authorization_header(
                &Signing {
                    method: "POST",
                    url: &request_url,
                    params: &params,
                    consumer_key: site.consumer_key.unwrap_or_default(),
                    consumer_secret: site.consumer_secret.unwrap_or_default(),
                    token: None,
                    token_secret: None,
                },
                &nonce(),
                &timestamp(),
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

    pub(super) fn handles(&self, url: &str) -> bool {
        let (Ok(url), Ok(callback)) = (url::Url::parse(url), url::Url::parse(&self.callback()))
        else {
            return false;
        };
        url.origin() == callback.origin() && url.path() == callback.path()
    }

    pub(super) async fn complete(
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
        let pending = self.pending.borrow().clone();
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
                let authorization = authorization_header(
                    &Signing {
                        method: "POST",
                        url: &token_url,
                        params: &values,
                        consumer_key: site.consumer_key.unwrap_or_default(),
                        consumer_secret: site.consumer_secret.unwrap_or_default(),
                        token: Some(&token),
                        token_secret: Some(&token_secret),
                    },
                    &nonce(),
                    &timestamp(),
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
            // The callback arrived before `prepare` stored the values to
            // check it against.
            None => Err(format!("{label} OAuth state did not match.")),
        }
    }
}

/// The adapter for Pixiv. The application starts Pixiv's OAuth flow
/// (`pixiv_oauth_start`), exchanges the code that the callback carries
/// (`pixiv_oauth_exchange`), and saves the login itself.
pub struct PixivAdapter {
    pub(super) site: Site,
    pub(super) code_verifier: RefCell<Option<String>>,
}

impl PixivAdapter {
    pub(super) async fn prepare(&self, services: &impl Services) -> Result<Prepared, String> {
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
            Some(origin) => crate::logins::sites::move_origin(login, &origin),
            None => login.to_owned(),
        };
        Ok(Prepared {
            url,
            message: "Sign in with Pixiv in the popup window.".into(),
        })
    }

    pub(super) async fn complete(
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

/// Returns `values` encoded as `application/x-www-form-urlencoded`.
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

/// Returns the fields of a form-encoded response. Returns an error that
/// starts with `operation` if the status is not a success or the response
/// has a field `error`.
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

/// Returns the JSON of a response. Returns an error that starts with
/// `operation` if the status is not a success; the error then gives the
/// response's `error_description` or `error`, or else the status.
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


/// The parts of a request that its OAuth 1.0a signature is computed from,
/// and the keys that sign it.
pub struct Signing<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub params: &'a [(&'a str, &'a str)],
    pub consumer_key: &'a str,
    pub consumer_secret: &'a str,
    pub token: Option<&'a str>,
    pub token_secret: Option<&'a str>,
}

/// Returns `value` percent-encoded as RFC 3986 defines it: every byte is
/// encoded except ASCII letters, digits and `-`, `.`, `_`, `~`.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Returns a new nonce: a random UUID written without its dashes.
pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Returns the current time in seconds since 1970, as text.
pub fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
        .to_string()
}

/// Returns the value of the `Authorization` header that signs the request
/// `signing` with the given nonce and timestamp.
pub fn authorization_header(signing: &Signing, nonce: &str, timestamp: &str) -> String {
    // i18n: OAuth protocol fields
    let mut oauth: Vec<(&str, &str)> = vec![
        ("oauth_consumer_key", signing.consumer_key),
        ("oauth_nonce", nonce),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_timestamp", timestamp),
        ("oauth_version", "1.0"),
    ];
    if let Some(token) = signing.token {
        oauth.push(("oauth_token", token));
    }
    // RFC 5849 signs every parameter, percent-encoded and sorted by name
    // and then by value, in byte order.
    let mut pairs: Vec<(String, String)> = signing
        .params
        .iter()
        .chain(oauth.iter())
        .map(|(name, value)| (encode(name), encode(value)))
        .collect();
    pairs.sort();
    let parameters = pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let base = [
        signing.method.to_uppercase(),
        signing.url.to_owned(),
        parameters,
    ]
    .iter()
    .map(|part| encode(part))
    .collect::<Vec<_>>()
    .join("&");
    let key = format!(
        "{}&{}",
        encode(signing.consumer_secret),
        encode(signing.token_secret.unwrap_or_default())
    );
    let signature = {
        use base64::Engine as _;
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(key.as_bytes())
            .expect("HMAC takes a key of any length");
        mac.update(base.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    };
    let fields = oauth
        .iter()
        .map(|(name, value)| (*name, (*value).to_owned()))
        .chain(std::iter::once(("oauth_signature", signature)))
        .map(|(name, value)| format!("{}=\"{}\"", encode(name), encode(&value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("OAuth {fields}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(super) fn encoding_keeps_only_unreserved_characters() {
        assert_eq!(encode("a-b._~Z9"), "a-b._~Z9");
        assert_eq!(encode("Ladies + Gentlemen"), "Ladies%20%2B%20Gentlemen");
        assert_eq!(encode("!'()*"), "%21%27%28%29%2A");
        assert_eq!(encode("é"), "%C3%A9");
    }

    /// Signs a published example request and compares the result with the
    /// published signature.
    #[test]
    pub(super) fn a_request_is_signed_as_rfc_5849_signs_it() {
        // The request, the keys, the nonce, the timestamp and the expected
        // signature are those of the example "Creating a signature" in
        // Twitter's API documentation.
        let header = authorization_header(
            &Signing {
                method: "post",
                url: "https://api.twitter.com/1.1/statuses/update.json",
                params: &[
                    ("include_entities", "true"),
                    (
                        "status",
                        "Hello Ladies + Gentlemen, a signed OAuth request!",
                    ),
                ],
                consumer_key: "xvz1evFS4wEEPTGEFPHBog",
                consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw",
                token: Some("370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb"),
                token_secret: Some("LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE"),
            },
            "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
            "1318622958",
        );
        assert!(header.starts_with(
            "OAuth oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\", oauth_nonce=\"kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg\", oauth_signature_method=\"HMAC-SHA1\", oauth_timestamp=\"1318622958\", oauth_version=\"1.0\", oauth_token=\"370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb\""
        ));
        assert!(
            header.ends_with("oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\""),
            "{header}"
        );
    }

    #[test]
    pub(super) fn a_nonce_is_thirty_two_hex_digits() {
        let nonce = nonce();
        assert_eq!(nonce.len(), 32);
        assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
