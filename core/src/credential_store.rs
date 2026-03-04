//! OS keychain-backed credential storage for gallery-dl site authentication.
//!
//! Secrets are stored in the platform's native keychain:
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Linux: D-Bus Secret Service (GNOME Keyring)
//!
//! Each credential is keyed by gallery-dl site category (e.g. "danbooru", "pixiv")
//! and stored as a JSON blob. The keyring crate handles platform differences.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "picto";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    UsernamePassword,
    Cookies,
    ApiKey,
    OAuthToken,
}

impl CredentialType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UsernamePassword => "username_password",
            Self::Cookies => "cookies",
            Self::ApiKey => "api_key",
            Self::OAuthToken => "oauth_token",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "username_password" => Some(Self::UsernamePassword),
            "cookies" => Some(Self::Cookies),
            "api_key" => Some(Self::ApiKey),
            "oauth_token" => Some(Self::OAuthToken),
            _ => None,
        }
    }
}

/// A credential for a specific gallery-dl site.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteCredential {
    /// Gallery-dl extractor category (e.g. "danbooru", "pixiv", "e621").
    pub site_category: String,
    pub credential_type: CredentialType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookies: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_token: Option<String>,
}

/// Store a credential in the OS keychain.
pub fn set_credential(cred: &SiteCredential) -> Result<(), String> {
    let json =
        serde_json::to_string(cred).map_err(|e| format!("Credential serialization error: {e}"))?;
    let entry = keyring::Entry::new(SERVICE_NAME, &cred.site_category)
        .map_err(|e| format!("Keyring entry error: {e}"))?;
    entry
        .set_password(&json)
        .map_err(|e| format!("Keyring set error: {e}"))?;
    Ok(())
}

/// Retrieve a credential from the OS keychain.
/// Returns `Ok(None)` if no credential exists for this site.
pub fn get_credential(site_category: &str) -> Result<Option<SiteCredential>, String> {
    let entry = keyring::Entry::new(SERVICE_NAME, site_category)
        .map_err(|e| format!("Keyring entry error: {e}"))?;
    match entry.get_password() {
        Ok(json) => {
            let cred: SiteCredential = serde_json::from_str(&json)
                .map_err(|e| format!("Credential deserialization error: {e}"))?;
            Ok(Some(cred))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Keyring get error: {e}")),
    }
}

/// Delete a credential from the OS keychain.
pub fn delete_credential(site_category: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE_NAME, site_category)
        .map_err(|e| format!("Keyring entry error: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // already gone
        Err(e) => Err(format!("Keyring delete error: {e}")),
    }
}

/// Convert a credential into a gallery-dl extractor config fragment.
///
/// Returns a JSON object to merge into `extractor.{site_category}` in the
/// gallery-dl config file. Format depends on credential type:
///
/// - UsernamePassword → `{"username": "...", "password": "..."}`
/// - Cookies → `{"cookies": {"key": "val", ...}}`
/// - ApiKey → `{"api-key": "..."}`
/// - OAuthToken → `{"refresh-token": "..."}`
pub fn build_extractor_auth(cred: &SiteCredential) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    let category = cred.site_category.trim().to_ascii_lowercase();
    let is_rule34 = matches!(category.as_str(), "rule34" | "rule34xxx" | "rule34.xxx");

    match cred.credential_type {
        CredentialType::UsernamePassword => {
            if let Some(ref u) = cred.username {
                obj.insert("username".into(), serde_json::Value::String(u.clone()));
            }
            if let Some(ref p) = cred.password {
                obj.insert("password".into(), serde_json::Value::String(p.clone()));
            }
        }
        CredentialType::Cookies => {
            if let Some(ref cookies) = cred.cookies {
                let cookie_obj: serde_json::Map<String, serde_json::Value> = cookies
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                obj.insert("cookies".into(), serde_json::Value::Object(cookie_obj));
            }
        }
        CredentialType::ApiKey => {
            if let Some(ref key) = cred.password {
                obj.insert("api-key".into(), serde_json::Value::String(key.clone()));
            }
            if is_rule34 {
                let user_id = cred
                    .username
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if let Some(user_id) = user_id {
                    obj.insert(
                        "user-id".into(),
                        serde_json::Value::String(user_id.to_string()),
                    );
                }
            }
        }
        CredentialType::OAuthToken => {
            if let Some(ref token) = cred.oauth_token {
                obj.insert(
                    "refresh-token".into(),
                    serde_json::Value::String(token.clone()),
                );
            }
        }
    }

    serde_json::Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_type_roundtrip_strings() {
        let all = [
            CredentialType::UsernamePassword,
            CredentialType::Cookies,
            CredentialType::ApiKey,
            CredentialType::OAuthToken,
        ];
        for ty in all {
            let raw = ty.as_str();
            assert_eq!(CredentialType::from_str(raw), Some(ty));
        }
        assert_eq!(CredentialType::from_str("invalid"), None);
    }

    #[test]
    fn build_extractor_auth_contract_by_credential_type() {
        let userpass = SiteCredential {
            site_category: "danbooru".to_string(),
            credential_type: CredentialType::UsernamePassword,
            username: Some("user".to_string()),
            password: Some("secret".to_string()),
            cookies: None,
            oauth_token: None,
        };
        let userpass_obj = build_extractor_auth(&userpass);
        assert_eq!(
            userpass_obj.get("username").and_then(|v| v.as_str()),
            Some("user")
        );
        assert_eq!(
            userpass_obj.get("password").and_then(|v| v.as_str()),
            Some("secret")
        );

        let mut cookies = HashMap::new();
        cookies.insert("sessionid".to_string(), "abc".to_string());
        let cookie_cred = SiteCredential {
            site_category: "pixiv".to_string(),
            credential_type: CredentialType::Cookies,
            username: None,
            password: None,
            cookies: Some(cookies),
            oauth_token: None,
        };
        let cookie_obj = build_extractor_auth(&cookie_cred);
        assert_eq!(
            cookie_obj
                .get("cookies")
                .and_then(|v| v.get("sessionid"))
                .and_then(|v| v.as_str()),
            Some("abc")
        );

        let api_key = SiteCredential {
            site_category: "e621".to_string(),
            credential_type: CredentialType::ApiKey,
            username: None,
            password: Some("api-key-value".to_string()),
            cookies: None,
            oauth_token: None,
        };
        let api_obj = build_extractor_auth(&api_key);
        assert_eq!(
            api_obj.get("api-key").and_then(|v| v.as_str()),
            Some("api-key-value")
        );
        assert!(api_obj.get("user-id").is_none());

        let rule34_api = SiteCredential {
            site_category: "rule34".to_string(),
            credential_type: CredentialType::ApiKey,
            username: Some("123456".to_string()),
            password: Some("rule34-api-key".to_string()),
            cookies: None,
            oauth_token: None,
        };
        let rule34_obj = build_extractor_auth(&rule34_api);
        assert_eq!(
            rule34_obj.get("api-key").and_then(|v| v.as_str()),
            Some("rule34-api-key")
        );
        assert_eq!(
            rule34_obj.get("user-id").and_then(|v| v.as_str()),
            Some("123456")
        );

        let oauth = SiteCredential {
            site_category: "fanbox".to_string(),
            credential_type: CredentialType::OAuthToken,
            username: None,
            password: None,
            cookies: None,
            oauth_token: Some("refresh-token".to_string()),
        };
        let oauth_obj = build_extractor_auth(&oauth);
        assert_eq!(
            oauth_obj.get("refresh-token").and_then(|v| v.as_str()),
            Some("refresh-token")
        );
    }
}
