//! OS keychain-backed credential storage for Picto's supported sources.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    pub fn from_str(s: &str) -> Option<Self> {
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

#[derive(Default, Serialize, Deserialize)]
struct DevelopmentCredentialCache {
    credentials: HashMap<String, SiteCredential>,
}

struct DevelopmentCredentialStore {
    path: PathBuf,
    cache: DevelopmentCredentialCache,
}

/// Development Electron is ad-hoc signed, so macOS cannot retain the same
/// Keychain trust across runtime upgrades. Keep a user-private cache after the
/// first authorized read; packaged builds never set this path.
fn development_store(
) -> Result<Option<&'static std::sync::Mutex<DevelopmentCredentialStore>>, String> {
    static STORE: std::sync::OnceLock<
        Result<Option<std::sync::Mutex<DevelopmentCredentialStore>>, String>,
    > = std::sync::OnceLock::new();
    match STORE.get_or_init(|| {
        let Some(path) = std::env::var_os("PICTO_DEVELOPMENT_CREDENTIAL_CACHE") else {
            return Ok(None);
        };
        let path = PathBuf::from(path);
        let cache = load_development_cache(&path)?;
        Ok(Some(std::sync::Mutex::new(DevelopmentCredentialStore {
            path,
            cache,
        })))
    }) {
        Ok(Some(store)) => Ok(Some(store)),
        Ok(None) => Ok(None),
        Err(error) => Err(error.clone()),
    }
}

fn load_development_cache(path: &Path) -> Result<DevelopmentCredentialCache, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Development credential cache is invalid: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(DevelopmentCredentialCache::default())
        }
        Err(error) => Err(format!("Development credential cache read error: {error}")),
    }
}

fn save_development_cache(store: &DevelopmentCredentialStore) -> Result<(), String> {
    use std::io::Write;

    if let Some(parent) = store.path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Development credential cache directory error: {error}"))?;
    }
    let temporary_path = store.path.with_extension("tmp");
    match std::fs::remove_file(&temporary_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Development credential cache cleanup error: {error}"
            ));
        }
    }
    let bytes = serde_json::to_vec(&store.cache)
        .map_err(|error| format!("Development credential cache serialization error: {error}"))?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary_path)
        .map_err(|error| format!("Development credential cache write error: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Development credential cache write error: {error}"))?;
    std::fs::rename(&temporary_path, &store.path)
        .map_err(|error| format!("Development credential cache replace error: {error}"))
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
    if let Some(store) = development_store()? {
        let mut store = store
            .lock()
            .map_err(|_| "development credential cache poisoned".to_string())?;
        store
            .cache
            .credentials
            .insert(cred.site_category.clone(), cred.clone());
        save_development_cache(&store)?;
    }
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
    if let Some(store) = development_store()? {
        if let Some(credential) = store
            .lock()
            .map_err(|_| "development credential cache poisoned".to_string())?
            .cache
            .credentials
            .get(site_category)
            .cloned()
        {
            unlocked_credential_cache()
                .lock()
                .map_err(|_| "credential cache poisoned".to_string())?
                .insert(site_category.to_string(), Some(credential.clone()));
            return Ok(Some(credential));
        }
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
    if let Some(store) = development_store()? {
        let mut store = store
            .lock()
            .map_err(|_| "development credential cache poisoned".to_string())?;
        store
            .cache
            .credentials
            .insert(site_category.to_string(), credential.clone());
        save_development_cache(&store)?;
    }
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
    if let Some(store) = development_store()? {
        let mut store = store
            .lock()
            .map_err(|_| "development credential cache poisoned".to_string())?;
        store.cache.credentials.remove(site_category);
        save_development_cache(&store)?;
    }
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
                let token_key = match cred.site_category.as_str() {
                    "baraag" => "access-token",
                    "tumblr" => "access-token",
                    _ => "refresh-token",
                };
                obj.insert(token_key.into(), serde_json::Value::String(token.clone()));
            }
            if cred.site_category == "tumblr" {
                if let Some(ref secret) = cred.password {
                    obj.insert(
                        "access-token-secret".into(),
                        serde_json::Value::String(secret.clone()),
                    );
                }
            }
            if let Some(ref cookies) = cred.cookies {
                let cookie_obj: serde_json::Map<String, serde_json::Value> = cookies
                    .iter()
                    .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                    .collect();
                obj.insert("cookies".into(), serde_json::Value::Object(cookie_obj));
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
            headers: None,
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
            headers: None,
            oauth_token: Some("refresh-token".to_string()),
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["refresh-token"], "refresh-token");
        assert_eq!(auth["cookies"]["PHPSESSID"], "session");
    }

    #[test]
    fn build_extractor_auth_for_baraag_oauth() {
        let credential = SiteCredential {
            site_category: "baraag".to_string(),
            credential_type: CredentialType::OAuthToken,
            username: None,
            password: None,
            cookies: None,
            headers: None,
            oauth_token: Some("baraag-access-token".to_string()),
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["access-token"], "baraag-access-token");
        assert!(auth.get("refresh-token").is_none());
    }

    #[test]
    fn build_extractor_auth_for_tumblr_oauth() {
        let credential = SiteCredential {
            site_category: "tumblr".to_string(),
            credential_type: CredentialType::OAuthToken,
            username: None,
            password: Some("tumblr-access-token-secret".to_string()),
            cookies: None,
            headers: None,
            oauth_token: Some("tumblr-access-token".to_string()),
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["access-token"], "tumblr-access-token");
        assert_eq!(auth["access-token-secret"], "tumblr-access-token-secret");
        assert!(auth.get("refresh-token").is_none());
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
            headers: None,
            oauth_token: None,
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["cookies"]["a"], "session-a");
        assert_eq!(auth["cookies"]["b"], "session-b");
        assert_eq!(auth.as_object().unwrap().len(), 1);
    }

    #[test]
    fn fanbox_auth_preserves_the_direct_site_session() {
        let credential = SiteCredential {
            site_category: "fanbox".to_string(),
            credential_type: CredentialType::Cookies,
            username: None,
            password: None,
            cookies: Some(HashMap::from([
                ("FANBOXSESSID".to_string(), "session".to_string()),
                ("cf_clearance".to_string(), "browser-bound".to_string()),
            ])),
            headers: None,
            oauth_token: None,
        };
        let auth = build_extractor_auth(&credential);
        assert_eq!(auth["cookies"]["FANBOXSESSID"], "session");
        assert_eq!(auth["cookies"]["cf_clearance"], "browser-bound");
    }

    #[test]
    fn development_cache_round_trips_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credentials.json");
        let credential = SiteCredential {
            site_category: "example".to_string(),
            credential_type: CredentialType::Cookies,
            username: None,
            password: None,
            cookies: Some(HashMap::from([(
                "session".to_string(),
                "secret".to_string(),
            )])),
            headers: None,
            oauth_token: None,
        };
        let store = DevelopmentCredentialStore {
            path: path.clone(),
            cache: DevelopmentCredentialCache {
                credentials: HashMap::from([("example".to_string(), credential.clone())]),
            },
        };

        save_development_cache(&store).unwrap();
        let loaded = load_development_cache(&path).unwrap();
        assert_eq!(loaded.credentials.get("example"), Some(&credential));
    }
}
