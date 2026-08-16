//! OS keychain-backed credential storage for Picto's supported sources.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "picto";
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    ApiKey,
    Cookies,
    OAuthToken,
    UsernamePassword,
}

impl CredentialType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Cookies => "cookies",
            Self::OAuthToken => "oauth_token",
            Self::UsernamePassword => "username_password",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "api_key" => Some(Self::ApiKey),
            "cookies" => Some(Self::Cookies),
            "oauth_token" => Some(Self::OAuthToken),
            "username_password" => Some(Self::UsernamePassword),
            _ => None,
        }
    }
}

/// A credential for one of the supported authenticated sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteCredential {
    /// Exact supported source credential owner, such as "pixiv" or "rule34".
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

pub fn set_credential(cred: &SiteCredential) -> Result<(), String> {
    let json = serde_json::to_string(cred)
        .map_err(|error| format!("Credential serialization error: {error}"))?;
    set_platform_credential(&cred.site_category, &json)
}

#[cfg(target_os = "macos")]
fn set_platform_credential(site_category: &str, json: &str) -> Result<(), String> {
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::os::macos::passwords::find_generic_password;

    match find_generic_password(None, SERVICE_NAME, site_category) {
        Ok((_password, mut item)) => item
            .set_password(json.as_bytes())
            .map_err(|error| format!("Keyring set error: {error}")),
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => SecKeychain::default()
            .and_then(|keychain| {
                keychain.add_generic_password(SERVICE_NAME, site_category, json.as_bytes())
            })
            .map_err(|error| format!("Keyring set error: {error}")),
        Err(error) => Err(format!("Keyring set error: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn set_platform_credential(site_category: &str, json: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE_NAME, site_category)
        .map_err(|error| format!("Keyring entry error: {error}"))?;
    delete_matching_credentials(&entry)?;
    entry
        .set_password(json)
        .map_err(|error| format!("Keyring set error: {error}"))
}

pub fn get_credential(site_category: &str) -> Result<Option<SiteCredential>, String> {
    let Some(json) = get_platform_credential(site_category)? else {
        return Ok(None);
    };
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|error| format!("Credential deserialization error: {error}"))
}

#[cfg(target_os = "macos")]
fn get_platform_credential(site_category: &str) -> Result<Option<String>, String> {
    use security_framework::os::macos::passwords::find_generic_password;

    match find_generic_password(None, SERVICE_NAME, site_category) {
        Ok((password, _item)) => String::from_utf8(password.to_vec())
            .map(Some)
            .map_err(|error| format!("Credential deserialization error: {error}")),
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(format!("Keyring get error: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn get_platform_credential(site_category: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE_NAME, site_category)
        .map_err(|error| format!("Keyring entry error: {error}"))?;
    match entry.get_password() {
        Ok(json) => Ok(Some(json)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Keyring get error: {error}")),
    }
}

pub fn delete_credential(site_category: &str) -> Result<(), String> {
    delete_platform_credential(site_category)
}

#[cfg(target_os = "macos")]
fn delete_platform_credential(site_category: &str) -> Result<(), String> {
    use security_framework::os::macos::passwords::find_generic_password;

    match find_generic_password(None, SERVICE_NAME, site_category) {
        Ok((_password, item)) => {
            item.delete();
            Ok(())
        }
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(error) => Err(format!("Keyring delete error: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn delete_platform_credential(site_category: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE_NAME, site_category)
        .map_err(|error| format!("Keyring entry error: {error}"))?;
    delete_matching_credentials(&entry)
}

#[cfg(not(target_os = "macos"))]
fn delete_matching_credentials(entry: &keyring::Entry) -> Result<(), String> {
    for _ in 0..32 {
        match entry.delete_credential() {
            Ok(()) => continue,
            Err(keyring::Error::NoEntry) => return Ok(()),
            Err(error) => return Err(format!("Keyring delete error: {error}")),
        }
    }
    Err("Keyring delete error: too many matching credentials".to_string())
}

/// Convert a supported credential into a gallery-dl extractor config fragment.
pub fn build_extractor_auth(cred: &SiteCredential) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    match cred.credential_type {
        CredentialType::ApiKey => {
            if let Some(ref key) = cred.password {
                obj.insert("api-key".into(), serde_json::Value::String(key.clone()));
            }
            if let Some(user_id) = cred
                .username
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                obj.insert(
                    "user-id".into(),
                    serde_json::Value::String(user_id.to_string()),
                );
            }
        }
        CredentialType::Cookies => {
            if let Some(ref cookies) = cred.cookies {
                let cookie_obj: serde_json::Map<String, serde_json::Value> = cookies
                    .iter()
                    .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                    .collect();
                obj.insert("cookies".into(), serde_json::Value::Object(cookie_obj));
            }
        }
        CredentialType::OAuthToken => {
            if let Some(ref token) = cred.oauth_token {
                obj.insert(
                    "refresh-token".into(),
                    serde_json::Value::String(token.clone()),
                );
            }
            if let Some(ref cookies) = cred.cookies {
                let cookie_obj: serde_json::Map<String, serde_json::Value> = cookies
                    .iter()
                    .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                    .collect();
                obj.insert("cookies".into(), serde_json::Value::Object(cookie_obj));
            }
        }
        CredentialType::UsernamePassword => {
            if let Some(ref username) = cred.username {
                obj.insert(
                    "username".into(),
                    serde_json::Value::String(username.clone()),
                );
            }
            if let Some(ref password) = cred.password {
                obj.insert(
                    "password".into(),
                    serde_json::Value::String(password.clone()),
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
    fn api_key_auth_ignores_browser_cookies() {
        let credential = SiteCredential {
            site_category: "gelbooru".to_string(),
            credential_type: CredentialType::ApiKey,
            username: Some("123456".to_string()),
            password: Some("api-key-value".to_string()),
            cookies: Some(HashMap::from([
                ("user_id".to_string(), "123456".to_string()),
                ("pass_hash".to_string(), "session".to_string()),
            ])),
            oauth_token: None,
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["api-key"], "api-key-value");
        assert_eq!(auth["user-id"], "123456");
        assert!(auth.get("cookies").is_none());
    }

    #[test]
    fn build_extractor_auth_for_pixiv_oauth() {
        let credential = SiteCredential {
            site_category: "pixiv".to_string(),
            credential_type: CredentialType::OAuthToken,
            username: None,
            password: None,
            cookies: Some(HashMap::from([(
                "PHPSESSID".to_string(),
                "session".to_string(),
            )])),
            oauth_token: Some("refresh-token".to_string()),
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["refresh-token"], "refresh-token");
        assert_eq!(auth["cookies"]["PHPSESSID"], "session");
    }

    #[test]
    fn cookie_auth_maps_only_the_captured_cookie_values() {
        let credential = SiteCredential {
            site_category: "furaffinity".to_string(),
            credential_type: CredentialType::Cookies,
            username: None,
            password: None,
            cookies: Some(HashMap::from([
                ("a".to_string(), "session-a".to_string()),
                ("b".to_string(), "session-b".to_string()),
            ])),
            oauth_token: None,
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["cookies"]["a"], "session-a");
        assert_eq!(auth["cookies"]["b"], "session-b");
        assert_eq!(auth.as_object().unwrap().len(), 1);
    }

    #[test]
    fn username_password_auth_maps_to_gallery_dl_fields() {
        let credential = SiteCredential {
            site_category: "idolcomplex".to_string(),
            credential_type: CredentialType::UsernamePassword,
            username: Some("artist".to_string()),
            password: Some("secret".to_string()),
            cookies: None,
            oauth_token: None,
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["username"], "artist");
        assert_eq!(auth["password"], "secret");
    }
}
