use std::collections::HashMap;

use serde_json::Value;
use tracing::warn;

use crate::credential_store::{CredentialType, SiteCredential};
use crate::db::LibraryDatabase;
use crate::subscriptions::gallery_dl_runner::{site_by_id, FailureKind, SiteEntry};
use crate::subscriptions::types::{CredentialDomain, CredentialHealth};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialHealthStatus {
    Unknown,
    Missing,
    Valid,
    Unauthorized,
    Expired,
    Error,
}

impl CredentialHealthStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Missing => "missing",
            Self::Valid => "valid",
            Self::Unauthorized => "unauthorized",
            Self::Expired => "expired",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GalleryDlAuthConfig {
    pub site_category: String,
    pub fragment: Value,
}

/// Outcome of the pre-run credential gate.
#[derive(Debug, Clone, PartialEq)]
pub enum CredentialPreflight {
    /// Run can proceed (credential present and not known-bad, or not needed).
    Ready,
    /// Auth improves results but isn't mandatory — run with a warning.
    MissingOptional,
    /// Site is unusable without auth and none is stored — block the run.
    MissingRequired,
    /// A stored credential is known expired/unauthorized — block the run.
    Blocked { status: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRunCredential {
    pub canonical_site_category: String,
    pub matched_lookup_key: Option<String>,
    pub auth_supported: bool,
    pub auth_required_for_full_access: bool,
    pub gallery_dl_auth: Option<GalleryDlAuthConfig>,
}

impl ResolvedRunCredential {
    pub fn has_credential(&self) -> bool {
        self.gallery_dl_auth.is_some()
    }
}

pub struct SetManualCredentialRequest {
    pub site_category: String,
    pub credential_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub cookies: Option<HashMap<String, String>>,
    pub oauth_token: Option<String>,
    pub display_name: Option<String>,
}

pub trait CredentialStoreBackend: Clone + Send + Sync + 'static {
    fn set_credential(&self, cred: &SiteCredential) -> Result<(), String>;
    fn get_credential(&self, site_category: &str) -> Result<Option<SiteCredential>, String>;
    fn delete_credential(&self, site_category: &str) -> Result<(), String>;
    fn build_extractor_auth(&self, cred: &SiteCredential) -> Value;
}

#[derive(Clone, Copy, Default)]
pub struct SystemCredentialStore;

impl CredentialStoreBackend for SystemCredentialStore {
    fn set_credential(&self, cred: &SiteCredential) -> Result<(), String> {
        crate::credential_store::set_credential(cred)
    }

    fn get_credential(&self, site_category: &str) -> Result<Option<SiteCredential>, String> {
        crate::credential_store::get_credential(site_category)
    }

    fn delete_credential(&self, site_category: &str) -> Result<(), String> {
        crate::credential_store::delete_credential(site_category)
    }

    fn build_extractor_auth(&self, cred: &SiteCredential) -> Value {
        crate::credential_store::build_extractor_auth(cred)
    }
}

pub struct SubscriptionCredentialService<'a, B = SystemCredentialStore> {
    db: &'a LibraryDatabase,
    store: B,
}

impl<'a> SubscriptionCredentialService<'a, SystemCredentialStore> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self {
            db,
            store: SystemCredentialStore,
        }
    }
}

impl<'a, B: CredentialStoreBackend> SubscriptionCredentialService<'a, B> {
    pub fn with_store(db: &'a LibraryDatabase, store: B) -> Self {
        Self { db, store }
    }

    pub async fn list_credentials(&self) -> Result<Vec<CredentialDomain>, String> {
        self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT site_category, credential_type, display_name, date_added
                 FROM credential_domain
                 ORDER BY site_category",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(CredentialDomain {
                    site_category: row.get(0)?,
                    credential_type: row.get(1)?,
                    display_name: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Recorded health exactly as written by run observations. Run gating
    /// (preflight) must use THIS view so only run-observed auth failures block
    /// a supported source.
    pub async fn list_credential_health_raw(&self) -> Result<Vec<CredentialHealth>, String> {
        self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT site_category, health_status, last_checked_at, last_error
                 FROM credential_health
                 ORDER BY site_category",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(CredentialHealth {
                    site_category: row.get(0)?,
                    health_status: row.get(1)?,
                    last_checked_at: row.get(2)?,
                    last_error: row.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    pub async fn list_credential_health(&self) -> Result<Vec<CredentialHealth>, String> {
        self.list_credential_health_raw().await
    }

    pub async fn set_manual_credential(
        &self,
        request: SetManualCredentialRequest,
    ) -> Result<String, String> {
        let owner = credential_owner_site(&request.site_category)
            .ok_or_else(|| format!("Unsupported authentication site: {}", request.site_category))?;
        let site_category = owner.id;
        if !owner
            .manual_credential_types
            .contains(&request.credential_type.as_str())
        {
            return Err(format!(
                "Credential type '{}' is not supported for {}",
                request.credential_type, owner.name
            ));
        }
        let credential_type = CredentialType::from_str(&request.credential_type)
            .ok_or_else(|| format!("Invalid credential_type: {}", request.credential_type))?;
        validate_credential_fields(
            credential_type,
            request.username.as_deref(),
            request.password.as_deref(),
            request.cookies.as_ref(),
            request.oauth_token.as_deref(),
        )?;

        let cred = SiteCredential {
            site_category: site_category.to_string(),
            credential_type,
            username: request.username,
            password: request.password,
            cookies: request.cookies,
            oauth_token: request.oauth_token,
        };

        self.store.set_credential(&cred)?;
        self.upsert_credential_domain(
            site_category,
            &request.credential_type,
            request.display_name.as_deref(),
        )
        .await?;
        self.set_health(site_category, CredentialHealthStatus::Unknown, None)
            .await;

        Ok(site_category.to_string())
    }

    pub async fn store_pixiv_oauth_credential(
        &self,
        refresh_token: String,
        phpsessid: Option<String>,
    ) -> Result<String, String> {
        let cookies = phpsessid
            .filter(|value| !value.trim().is_empty())
            .map(|sessid| {
                let mut map = HashMap::new();
                map.insert("PHPSESSID".to_string(), sessid);
                map
            });

        self.set_manual_credential(SetManualCredentialRequest {
            site_category: "pixiv".to_string(),
            credential_type: "oauth_token".to_string(),
            username: None,
            password: None,
            cookies,
            oauth_token: Some(refresh_token),
            display_name: Some("Pixiv".to_string()),
        })
        .await
    }

    pub async fn delete_credential(&self, site_category: &str) -> Result<String, String> {
        let owner = credential_owner_site_category(site_category)
            .ok_or_else(|| format!("Unsupported authentication site: {site_category}"))?;
        let _ = self.store.delete_credential(owner);
        let _ = self.delete_credential_domain(owner).await;
        let _ = self.delete_credential_health(owner).await;
        Ok(owner.to_string())
    }

    /// Pre-run credential gate. Blocks runs that would predictably fail:
    /// a strictly-auth-gated site with no credential, or a stored credential
    /// already known to be expired/unauthorized. Read-only.
    pub async fn preflight_for_run(&self, site_id: &str, url: &str) -> CredentialPreflight {
        let resolved = self.resolve_credential(site_id, url).await;
        let strictly_required = site_by_id(site_id).is_some_and(|site| site.auth_strictly_required);

        if resolved.gallery_dl_auth.is_some() {
            // Credential present — block only on run-observed auth failures.
            // Only run-observed auth failures block a stored credential.
            let category = resolved.canonical_site_category.clone();
            if let Ok(rows) = self.list_credential_health_raw().await {
                if let Some(row) = rows.iter().find(|h| h.site_category == category) {
                    if row.health_status == "expired" || row.health_status == "unauthorized" {
                        return CredentialPreflight::Blocked {
                            status: row.health_status.clone(),
                        };
                    }
                }
            }
            return CredentialPreflight::Ready;
        }

        if !resolved.auth_supported {
            return CredentialPreflight::Ready;
        }
        if strictly_required {
            return CredentialPreflight::MissingRequired;
        }
        if resolved.auth_required_for_full_access {
            return CredentialPreflight::MissingOptional;
        }
        CredentialPreflight::Ready
    }

    /// Record a pre-flight block as a visible subscription issue.
    pub async fn note_preflight_block(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        site_id: &str,
        failure_kind: FailureKind,
        message: &str,
    ) {
        let _ = site_id;
        self.upsert_issue(
            subscription_id,
            query_id,
            failure_kind,
            message,
            Some("Add or refresh the credential for this site, then run again."),
        )
        .await;
    }

    /// Credential lookup only — no health writes, no issue upserts.
    /// Used by the run path (via `resolve_for_run`) and by read-only callers
    /// like site verification that must not mutate subscription state.
    pub async fn resolve_credential(&self, site_id: &str, url: &str) -> ResolvedRunCredential {
        let _ = url;
        let source = site_by_id(site_id.trim());
        let owner = source.and_then(|site| credential_owner_site(site.id));
        let canonical_site_category = owner.map_or(site_id.trim(), |site| site.id).to_string();
        let auth_supported = source.is_some_and(|site| site.auth_supported);
        let auth_required = source.is_some_and(|site| site.auth_required_for_full_access);
        let mut matched_lookup_key = None;
        let mut gallery_dl_auth = None;

        let credential_is_enabled = match self.list_credentials().await {
            Ok(credentials) => credentials
                .iter()
                .any(|credential| credential.site_category == canonical_site_category),
            Err(error) => {
                warn!(site = %canonical_site_category, error = %error, "Failed to inspect configured credentials");
                false
            }
        };

        if credential_is_enabled {
            let Some(owner) = owner else {
                return ResolvedRunCredential {
                    canonical_site_category,
                    matched_lookup_key,
                    auth_supported,
                    auth_required_for_full_access: auth_required,
                    gallery_dl_auth,
                };
            };
            match self.store.get_credential(owner.id) {
                Ok(Some(cred)) => {
                    matched_lookup_key = Some(owner.id.to_string());
                    gallery_dl_auth = Some(GalleryDlAuthConfig {
                        site_category: canonical_site_category.clone(),
                        fragment: self.store.build_extractor_auth(&cred),
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    warn!(site = owner.id, error = %error, "Failed to load credential");
                }
            }
        }

        ResolvedRunCredential {
            canonical_site_category,
            matched_lookup_key,
            auth_supported,
            auth_required_for_full_access: auth_required,
            gallery_dl_auth,
        }
    }

    pub async fn resolve_for_run(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        site_id: &str,
        url: &str,
    ) -> ResolvedRunCredential {
        let resolved = self.resolve_credential(site_id, url).await;
        let canonical_site_category = resolved.canonical_site_category.clone();
        let auth_supported = resolved.auth_supported;
        let auth_required = resolved.auth_required_for_full_access;
        let gallery_dl_auth = &resolved.gallery_dl_auth;

        if auth_supported && gallery_dl_auth.is_none() && auth_required {
            self.set_health(
                &canonical_site_category,
                CredentialHealthStatus::Missing,
                Some("No credential configured for a site that commonly requires auth"),
            )
            .await;
            self.upsert_issue(
                subscription_id,
                query_id,
                FailureKind::CredentialMissing,
                "No credential configured for a site that commonly requires auth",
                Some("Configure a credential for this site to access the full subscription result set."),
            )
            .await;
        } else if gallery_dl_auth.is_some() {
            self.resolve_issue(subscription_id, query_id, FailureKind::CredentialMissing)
                .await;
        }

        resolved
    }

    pub async fn note_run_auth_failure(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        site_id: &str,
        failure_kind: FailureKind,
        detail: Option<&str>,
    ) {
        let Some(canonical_site_category) = credential_owner_site_category(site_id) else {
            return;
        };
        let status = match failure_kind {
            FailureKind::Unauthorized => CredentialHealthStatus::Unauthorized,
            FailureKind::Expired => CredentialHealthStatus::Expired,
            _ => return,
        };
        let message = match failure_kind {
            FailureKind::Unauthorized => {
                "Credential was rejected by the site during subscription sync"
            }
            FailureKind::Expired => {
                "Credential expired or was rejected by the site during subscription sync"
            }
            _ => return,
        };

        // An auth-shaped failure can only indict a credential that exists —
        // otherwise (e.g. a 403 from a CDN on an anonymous site) it is a run
        // problem, and marking health would block every future run.
        let has_credential = self
            .store
            .get_credential(canonical_site_category)
            .ok()
            .flatten()
            .is_some();
        if has_credential {
            self.set_health(canonical_site_category, status, detail)
                .await;
            self.upsert_issue(
                subscription_id,
                query_id,
                FailureKind::CredentialBlocked,
                message,
                detail,
            )
            .await;
        } else {
            self.upsert_issue(
                subscription_id,
                query_id,
                FailureKind::CredentialBlocked,
                "The site rejected the request as unauthorized, but no account is stored — add one in Accounts",
                detail,
            )
            .await;
        }
    }

    pub async fn note_run_success(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        site_id: &str,
        used_credential: bool,
    ) {
        if !used_credential {
            return;
        }
        let Some(canonical_site_category) = credential_owner_site_category(site_id) else {
            return;
        };
        self.set_health(
            &canonical_site_category,
            CredentialHealthStatus::Valid,
            None,
        )
        .await;
        self.resolve_issue(subscription_id, query_id, FailureKind::CredentialMissing)
            .await;
        self.resolve_issue(subscription_id, query_id, FailureKind::CredentialBlocked)
            .await;
    }

    async fn set_health(
        &self,
        site_category: &str,
        status: CredentialHealthStatus,
        detail: Option<&str>,
    ) {
        if let Err(error) = self
            .upsert_credential_health(site_category, status.as_db_str(), detail)
            .await
        {
            warn!(
                site = %site_category,
                status = status.as_db_str(),
                error = %error,
                "Failed to persist credential health"
            );
        }
    }

    async fn upsert_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
        message: &str,
        detail: Option<&str>,
    ) {
        let _ = self
            .upsert_issue_db(subscription_id, query_id, failure_kind, message, detail)
            .await;
    }

    async fn resolve_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
    ) {
        let _ = self
            .resolve_issue_db(subscription_id, query_id, failure_kind)
            .await;
    }

    async fn upsert_credential_domain(
        &self,
        site_category: &str,
        credential_type: &str,
        display_name: Option<&str>,
    ) -> Result<(), String> {
        self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO credential_domain (site_category, credential_type, display_name, date_added)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(site_category) DO UPDATE
                 SET credential_type = excluded.credential_type,
                     display_name = excluded.display_name",
                rusqlite::params![
                    site_category,
                    credential_type,
                    display_name,
                    chrono::Utc::now().to_rfc3339()
                ],
            )?;
            Ok(())
        })
    }

    async fn delete_credential_domain(&self, site_category: &str) -> Result<(), String> {
        self.db.with_write(|conn| {
            conn.execute(
                "DELETE FROM credential_domain WHERE site_category = ?1",
                [site_category],
            )?;
            Ok(())
        })
    }

    async fn upsert_credential_health(
        &self,
        site_category: &str,
        health_status: &str,
        detail: Option<&str>,
    ) -> Result<(), String> {
        self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(site_category) DO UPDATE
                 SET health_status = excluded.health_status,
                     last_checked_at = excluded.last_checked_at,
                     last_error = excluded.last_error",
                rusqlite::params![
                    site_category,
                    health_status,
                    chrono::Utc::now().to_rfc3339(),
                    detail
                ],
            )?;
            Ok(())
        })
    }

    async fn delete_credential_health(&self, site_category: &str) -> Result<(), String> {
        self.db.with_write(|conn| {
            conn.execute(
                "DELETE FROM credential_health WHERE site_category = ?1",
                [site_category],
            )?;
            Ok(())
        })
    }

    async fn upsert_issue_db(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
        message: &str,
        detail: Option<&str>,
    ) -> Result<(), String> {
        self.db.with_write(|conn| {
            crate::subscriptions::runtime_db::upsert_subscription_issue(
                conn,
                subscription_id,
                query_id,
                failure_kind,
                message,
                detail,
            )?;
            Ok(())
        })
    }

    async fn resolve_issue_db(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
    ) -> Result<(), String> {
        self.db.with_write(|conn| {
            crate::subscriptions::runtime_db::resolve_subscription_issues(
                conn,
                subscription_id,
                query_id,
                failure_kind,
            )
        })
    }
}

fn credential_owner_site(site_id: &str) -> Option<&'static SiteEntry> {
    let source = site_by_id(site_id.trim())?;
    source
        .auth_supported
        .then(|| site_by_id(source.credential_owner_site_id))
        .flatten()
}

fn credential_owner_site_category(site_id: &str) -> Option<&'static str> {
    credential_owner_site(site_id).map(|site| site.id)
}

fn validate_credential_fields(
    credential_type: CredentialType,
    username: Option<&str>,
    password: Option<&str>,
    cookies: Option<&HashMap<String, String>>,
    oauth_token: Option<&str>,
) -> Result<(), String> {
    let present = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    match credential_type {
        CredentialType::ApiKey if !present(username) || !present(password) => {
            Err("API-key credentials require both user_id and api_key".to_string())
        }
        CredentialType::ApiKey
            if !username
                .is_some_and(|value| value.trim().bytes().all(|byte| byte.is_ascii_digit()))
                || !password.is_some_and(|value| {
                    let value = value.trim();
                    value.len() >= 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                }) =>
        {
            Err("API-key credentials contain an invalid user_id or api_key".to_string())
        }
        CredentialType::OAuthToken if !present(oauth_token) => {
            Err("Pixiv credentials require an OAuth refresh token".to_string())
        }
        CredentialType::Cookies
            if !cookies.is_some_and(|cookies| {
                !cookies.is_empty()
                    && cookies
                        .iter()
                        .all(|(name, value)| !name.trim().is_empty() && !value.trim().is_empty())
            }) =>
        {
            Err("Cookie credentials require captured session cookies".to_string())
        }
        CredentialType::UsernamePassword if !present(username) || !present(password) => {
            Err("Username/password credentials require both fields".to_string())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct InMemoryCredentialStore {
        entries: Arc<Mutex<HashMap<String, SiteCredential>>>,
    }

    impl InMemoryCredentialStore {
        fn contains(&self, site_category: &str) -> bool {
            self.entries.lock().unwrap().contains_key(site_category)
        }

        fn insert(&self, credential: SiteCredential) {
            self.entries
                .lock()
                .unwrap()
                .insert(credential.site_category.clone(), credential);
        }
    }

    impl CredentialStoreBackend for InMemoryCredentialStore {
        fn set_credential(&self, credential: &SiteCredential) -> Result<(), String> {
            self.entries
                .lock()
                .unwrap()
                .insert(credential.site_category.clone(), credential.clone());
            Ok(())
        }

        fn get_credential(&self, site_category: &str) -> Result<Option<SiteCredential>, String> {
            Ok(self.entries.lock().unwrap().get(site_category).cloned())
        }

        fn delete_credential(&self, site_category: &str) -> Result<(), String> {
            self.entries.lock().unwrap().remove(site_category);
            Ok(())
        }

        fn build_extractor_auth(&self, credential: &SiteCredential) -> Value {
            crate::credential_store::build_extractor_auth(credential)
        }
    }

    #[derive(Clone, Copy)]
    struct RejectCredentialReads;

    impl CredentialStoreBackend for RejectCredentialReads {
        fn set_credential(&self, _credential: &SiteCredential) -> Result<(), String> {
            panic!("anonymous source attempted to write the credential store")
        }

        fn get_credential(&self, site_category: &str) -> Result<Option<SiteCredential>, String> {
            panic!("anonymous source attempted to read credential '{site_category}'")
        }

        fn delete_credential(&self, site_category: &str) -> Result<(), String> {
            panic!("anonymous source attempted to delete credential '{site_category}'")
        }

        fn build_extractor_auth(&self, _credential: &SiteCredential) -> Value {
            panic!("anonymous source attempted to build credential configuration")
        }
    }

    async fn test_db() -> std::sync::Arc<LibraryDatabase> {
        let dir = tempfile::tempdir().unwrap();
        std::sync::Arc::new(LibraryDatabase::open(dir.path()).unwrap())
    }

    #[tokio::test]
    async fn pixivuser_resolves_the_shared_pixiv_credential() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        store.insert(SiteCredential {
            site_category: "pixiv".to_string(),
            credential_type: CredentialType::OAuthToken,
            username: None,
            password: None,
            cookies: None,
            oauth_token: Some("refresh".to_string()),
        });
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store);

        service
            .upsert_credential_domain("pixiv", "oauth_token", Some("Pixiv"))
            .await
            .unwrap();
        let resolved = service
            .resolve_credential("pixivuser", "https://www.pixiv.net/en/users/12345")
            .await;

        assert_eq!(resolved.canonical_site_category, "pixiv");
        assert_eq!(resolved.matched_lookup_key.as_deref(), Some("pixiv"));
        assert_eq!(
            resolved.gallery_dl_auth.unwrap().fragment["refresh-token"],
            "refresh"
        );
    }

    #[tokio::test]
    async fn gelbooru_resolves_user_id_and_api_key() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "gelbooru".to_string(),
                credential_type: "api_key".to_string(),
                username: Some("123".to_string()),
                password: Some("0123456789abcdef".to_string()),
                cookies: None,
                oauth_token: None,
                display_name: Some("Gelbooru".to_string()),
            })
            .await
            .unwrap();

        let resolved = service
            .resolve_credential("gelbooru", "https://gelbooru.com/index.php?page=post")
            .await;
        let auth = resolved.gallery_dl_auth.unwrap().fragment;
        assert_eq!(auth["user-id"], "123");
        assert_eq!(auth["api-key"], "0123456789abcdef");
        assert!(store.contains("gelbooru"));
    }

    #[tokio::test]
    async fn manual_credentials_are_validated_against_the_source_contract() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(
            db.as_ref(),
            InMemoryCredentialStore::default(),
        );

        let wrong_type = service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "gelbooru".to_string(),
                credential_type: "oauth_token".to_string(),
                username: None,
                password: None,
                cookies: None,
                oauth_token: Some("token".to_string()),
                display_name: None,
            })
            .await
            .unwrap_err();
        assert!(wrong_type.contains("not supported for Gelbooru"));

        let incomplete = service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "gelbooru".to_string(),
                credential_type: "api_key".to_string(),
                username: Some("123".to_string()),
                password: None,
                cookies: None,
                oauth_token: None,
                display_name: None,
            })
            .await
            .unwrap_err();
        assert!(incomplete.contains("user_id and api_key"));

        let malformed = service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "rule34".to_string(),
                credential_type: "api_key".to_string(),
                username: Some("456".to_string()),
                password: Some("No".to_string()),
                cookies: None,
                oauth_token: None,
                display_name: None,
            })
            .await
            .unwrap_err();
        assert!(malformed.contains("invalid user_id or api_key"));
    }

    #[tokio::test]
    async fn rule34_resolves_its_own_user_id_and_api_key() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "rule34".to_string(),
                credential_type: "api_key".to_string(),
                username: Some("456".to_string()),
                password: Some("fedcba9876543210".to_string()),
                cookies: None,
                oauth_token: None,
                display_name: Some("Rule34.xxx".to_string()),
            })
            .await
            .unwrap();

        let resolved = service
            .resolve_credential(
                "rule34",
                "https://rule34.xxx/index.php?page=post&s=list&tags=test",
            )
            .await;
        let auth = resolved.gallery_dl_auth.unwrap().fragment;
        assert_eq!(auth["user-id"], "456");
        assert_eq!(auth["api-key"], "fedcba9876543210");
        assert!(store.contains("rule34"));
    }

    #[tokio::test]
    async fn furaffinity_resolves_captured_browser_cookies() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "furaffinity".to_string(),
                credential_type: "cookies".to_string(),
                username: None,
                password: None,
                cookies: Some(HashMap::from([
                    ("a".to_string(), "session-a".to_string()),
                    ("b".to_string(), "session-b".to_string()),
                ])),
                oauth_token: None,
                display_name: Some("Fur Affinity".to_string()),
            })
            .await
            .unwrap();

        let resolved = service
            .resolve_credential("furaffinity", "https://www.furaffinity.net/gallery/artist")
            .await;
        let auth = resolved.gallery_dl_auth.unwrap().fragment;
        assert_eq!(auth["cookies"]["a"], "session-a");
        assert_eq!(auth["cookies"]["b"], "session-b");
        assert!(store.contains("furaffinity"));
    }

    #[tokio::test]
    async fn idolcomplex_resolves_username_and_password() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "idolcomplex".to_string(),
                credential_type: "username_password".to_string(),
                username: Some("artist".to_string()),
                password: Some("secret".to_string()),
                cookies: None,
                oauth_token: None,
                display_name: Some("Idol Complex".to_string()),
            })
            .await
            .unwrap();

        let resolved = service
            .resolve_credential(
                "idolcomplex",
                "https://www.idolcomplex.com/en/posts?tags=solo",
            )
            .await;
        let auth = resolved.gallery_dl_auth.unwrap().fragment;
        assert_eq!(auth["username"], "artist");
        assert_eq!(auth["password"], "secret");
        assert!(store.contains("idolcomplex"));
    }

    #[tokio::test]
    async fn danbooru_is_anonymous_and_never_reads_credential_store() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(db.as_ref(), RejectCredentialReads);

        let resolved = service
            .resolve_credential("danbooru", "https://danbooru.donmai.us/posts?tags=test")
            .await;
        assert_eq!(resolved.canonical_site_category, "danbooru");
        assert!(!resolved.auth_supported);
        assert!(!resolved.has_credential());
        assert_eq!(
            service.preflight_for_run("danbooru", "").await,
            CredentialPreflight::Ready
        );
        service
            .note_run_auth_failure(0, None, "danbooru", FailureKind::Unauthorized, None)
            .await;
    }

    #[tokio::test]
    async fn pixiv_oauth_save_uses_shared_persistence_path() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        let site = service
            .store_pixiv_oauth_credential("refresh".to_string(), Some("phpsessid".to_string()))
            .await
            .unwrap();

        assert_eq!(site, "pixiv");
        assert!(store.contains("pixiv"));
        let resolved = service
            .resolve_credential("pixiv", "https://www.pixiv.net/en/tags/test")
            .await;
        assert!(resolved.has_credential());
    }

    #[tokio::test]
    async fn optional_auth_without_library_configuration_never_reads_credential_store() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(db.as_ref(), RejectCredentialReads);

        let resolved = service
            .resolve_credential(
                "idolcomplex",
                "https://www.idolcomplex.com/en/posts?tags=solo",
            )
            .await;

        assert!(resolved.auth_supported);
        assert!(resolved.auth_required_for_full_access);
        assert!(!resolved.has_credential());
    }
}
