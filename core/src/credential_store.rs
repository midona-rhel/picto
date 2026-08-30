//! OS keychain-backed credential storage for Picto's supported sources.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "picto";
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
#[cfg(target_os = "macos")]
const ERR_SEC_DUPLICATE_ITEM: i32 = -25299;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    ApiKey,
    Cookies,
    OAuthToken,
}

impl CredentialType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Cookies => "cookies",
            Self::OAuthToken => "oauth_token",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "api_key" => Some(Self::ApiKey),
            "cookies" => Some(Self::Cookies),
            "oauth_token" => Some(Self::OAuthToken),
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
    /// Browser request headers required to reuse a captured direct-site session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_token: Option<String>,
}

/// Process-local ephemeral credential store, enabled by setting
/// `PICTO_EPHEMERAL_CREDENTIALS=1` before the first credential access.
/// Certification and other automated runs use it so they can never read,
/// write, or prompt the OS keychain.
fn ephemeral_store() -> Option<&'static std::sync::Mutex<HashMap<String, String>>> {
    static STORE: std::sync::OnceLock<Option<std::sync::Mutex<HashMap<String, String>>>> =
        std::sync::OnceLock::new();
    STORE
        .get_or_init(|| {
            (std::env::var("PICTO_EPHEMERAL_CREDENTIALS").ok().as_deref() == Some("1"))
                .then(|| std::sync::Mutex::new(HashMap::new()))
        })
        .as_ref()
}

/// Credentials already unlocked during this application launch. macOS may
/// prompt whenever a process reads a Keychain item, so repeated subscription
/// queries must reuse the credential instead of reopening the same item.
fn unlocked_credential_cache() -> &'static std::sync::Mutex<HashMap<String, Option<SiteCredential>>>
{
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Option<SiteCredential>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

pub fn set_credential(cred: &SiteCredential) -> Result<(), String> {
    let json = serde_json::to_string(cred)
        .map_err(|error| format!("Credential serialization error: {error}"))?;
    if let Some(store) = ephemeral_store() {
        store
            .lock()
            .map_err(|_| "ephemeral credential store poisoned".to_string())?
            .insert(cred.site_category.clone(), json);
        return Ok(());
    }
    set_platform_credential(&cred.site_category, &json)?;
    unlocked_credential_cache()
        .lock()
        .map_err(|_| "credential cache poisoned".to_string())?
        .insert(cred.site_category.clone(), Some(cred.clone()));
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_platform_credential(site_category: &str, json: &str) -> Result<(), String> {
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::os::macos::passwords::find_generic_password;

    let add = || {
        SecKeychain::default()
            .and_then(|keychain| {
                keychain.add_generic_password(SERVICE_NAME, site_category, json.as_bytes())
            })
            .map_err(|error| format!("Keyring set error: {error}"))
    };

    match find_generic_password(None, SERVICE_NAME, site_category) {
        Ok((_password, mut item)) => match item.set_password(json.as_bytes()) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ERR_SEC_DUPLICATE_ITEM => {
                // Old builds could leave duplicate generic-password entries.
                // Collapse them before writing the single canonical credential.
                for _ in 0..32 {
                    match find_generic_password(None, SERVICE_NAME, site_category) {
                        Ok((_password, item)) => item.delete(),
                        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => return add(),
                        Err(error) => return Err(format!("Keyring delete error: {error}")),
                    }
                }
                Err("Keyring delete error: too many matching credentials".to_string())
            }
            Err(error) => Err(format!("Keyring set error: {error}")),
        },
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => add(),
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
    if let Some(store) = ephemeral_store() {
        let json = store
            .lock()
            .map_err(|_| "ephemeral credential store poisoned".to_string())?
            .get(site_category)
            .cloned();
        let Some(json) = json else {
            return Ok(None);
        };
        return serde_json::from_str(&json)
            .map(Some)
            .map_err(|error| format!("Credential deserialization error: {error}"));
    }
    if let Some(credential) = unlocked_credential_cache()
        .lock()
        .map_err(|_| "credential cache poisoned".to_string())?
        .get(site_category)
        .cloned()
    {
        return Ok(credential);
    }
    let Some(json) = get_platform_credential(site_category)? else {
        unlocked_credential_cache()
            .lock()
            .map_err(|_| "credential cache poisoned".to_string())?
            .insert(site_category.to_string(), None);
        return Ok(None);
    };
    let credential: SiteCredential = serde_json::from_str(&json)
        .map_err(|error| format!("Credential deserialization error: {error}"))?;
    unlocked_credential_cache()
        .lock()
        .map_err(|_| "credential cache poisoned".to_string())?
        .insert(site_category.to_string(), Some(credential.clone()));
    Ok(Some(credential))
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
    if let Some(store) = ephemeral_store() {
        store
            .lock()
            .map_err(|_| "ephemeral credential store poisoned".to_string())?
            .remove(site_category);
        return Ok(());
    }
    delete_platform_credential(site_category)?;
    unlocked_credential_cache()
        .lock()
        .map_err(|_| "credential cache poisoned".to_string())?
        .remove(site_category);
    Ok(())
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
