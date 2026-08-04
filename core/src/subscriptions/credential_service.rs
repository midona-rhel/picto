use std::collections::HashMap;

use serde_json::Value;
use tracing::warn;

use crate::credential_store::{CredentialType, SiteCredential};
use crate::db::LibraryDatabase;
use crate::subscriptions::gallery_dl_runner::{self, FailureKind};
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
    /// RFC3339 timestamp when the captured session/cookies expire, if known.
    pub expires_at: Option<String>,
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
                "SELECT site_category, credential_type, display_name, date_added, expires_at
                 FROM credential_domain ORDER BY site_category",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(CredentialDomain {
                    site_category: row.get(0)?,
                    credential_type: row.get(1)?,
                    display_name: row.get(2)?,
                    created_at: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Recorded health exactly as written by run observations. Run gating
    /// (preflight) must use THIS view — a timestamp-based cookie expiry is a
    /// warning, not a reason to refuse runs.
    pub async fn list_credential_health_raw(&self) -> Result<Vec<CredentialHealth>, String> {
        self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT site_category, health_status, last_checked_at, last_error
                 FROM credential_health ORDER BY site_category",
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

    /// Display view: a stored cookie expiry in the past overrides recorded
    /// health so the UI can warn "log in again" before any run fails. Runs
    /// still proceed — only run-observed auth failures block (see
    /// `preflight_for_run`, which reads the raw view).
    pub async fn list_credential_health(&self) -> Result<Vec<CredentialHealth>, String> {
        self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT h.site_category, h.health_status, h.last_checked_at, h.last_error,
                        d.expires_at
                 FROM credential_health h
                 LEFT JOIN credential_domain d ON d.site_category = h.site_category
                 ORDER BY h.site_category",
            )?;
            let now = chrono::Utc::now();
            let rows = stmt.query_map([], move |row| {
                let status: String = row.get(1)?;
                let last_error: Option<String> = row.get(3)?;
                let expires_at: Option<String> = row.get(4)?;
                let cookie_expired = expires_at
                    .as_deref()
                    .and_then(|exp| chrono::DateTime::parse_from_rfc3339(exp).ok())
                    .is_some_and(|exp| exp.with_timezone(&chrono::Utc) < now)
                    && status != "missing";
                Ok(CredentialHealth {
                    site_category: row.get(0)?,
                    health_status: if cookie_expired {
                        "expired".to_string()
                    } else {
                        status
                    },
                    last_checked_at: row.get(2)?,
                    last_error: if cookie_expired {
                        Some("Saved session cookies may have expired — log in again.".to_string())
                    } else {
                        last_error
                    },
                })
            })?;
            rows.collect()
        })
    }

    pub async fn set_manual_credential(
        &self,
        request: SetManualCredentialRequest,
    ) -> Result<String, String> {
        let canonical_site_category = canonical_credential_site_category(&request.site_category);
        let site = gallery_dl_runner::site_by_id(&canonical_site_category)
            .ok_or_else(|| format!("Unknown site: {}", request.site_category))?;
        let credential_type = CredentialType::from_str(&request.credential_type)
            .ok_or_else(|| format!("Invalid credential_type: {}", request.credential_type))?;

        validate_manual_credential(site, credential_type, &request.username, &request.password)?;

        let cred = SiteCredential {
            site_category: canonical_site_category.clone(),
            credential_type,
            username: request.username,
            password: request.password,
            cookies: request.cookies,
            oauth_token: request.oauth_token,
        };

        self.store.set_credential(&cred)?;
        self.upsert_credential_domain(
            &canonical_site_category,
            &request.credential_type,
            request.display_name.as_deref(),
            request.expires_at.as_deref(),
        )
        .await?;
        self.set_health(
            &canonical_site_category,
            CredentialHealthStatus::Unknown,
            None,
        )
        .await;

        Ok(canonical_site_category)
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
            expires_at: None,
        })
        .await
    }

    pub async fn delete_credential(&self, site_category: &str) -> Result<String, String> {
        let canonical_site_category = canonical_credential_site_category(site_category);
        for category in credential_delete_categories(site_category) {
            let _ = self.store.delete_credential(&category);
            let _ = self.delete_credential_domain(&category).await;
            let _ = self.delete_credential_health(&category).await;
        }
        Ok(canonical_site_category)
    }

    /// Pre-run credential gate. Blocks runs that would predictably fail:
    /// a strictly-auth-gated site with no credential, or a stored credential
    /// already known to be expired/unauthorized. Read-only.
    pub async fn preflight_for_run(&self, site_id: &str, url: &str) -> CredentialPreflight {
        let resolved = self.resolve_credential(site_id, url);
        let strictly_required =
            gallery_dl_runner::site_by_id(site_id).is_some_and(|site| site.auth_strictly_required);

        if resolved.gallery_dl_auth.is_some() {
            // Credential present — block only on run-observed auth failures.
            // The raw view deliberately ignores timestamp-based cookie expiry:
            // that is a display warning, and a wrongly-computed expiry must
            // never silently kill all runs for a site.
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
    pub fn resolve_credential(&self, site_id: &str, url: &str) -> ResolvedRunCredential {
        let canonical_site_category = canonical_credential_site_category(site_id);
        let site_entry = gallery_dl_runner::site_by_id(&canonical_site_category)
            .or_else(|| gallery_dl_runner::site_by_id(site_id));
        let auth_supported = site_entry.is_some_and(|site| site.auth_supported);
        let auth_required = site_entry.is_some_and(|site| site.auth_required_for_full_access);

        let mut matched_lookup_key = None;
        let mut gallery_dl_auth = None;

        if auth_supported {
            for category in credential_lookup_categories(site_id, url) {
                match self.store.get_credential(&category) {
                    Ok(Some(cred)) => {
                        matched_lookup_key = Some(category);
                        gallery_dl_auth = Some(GalleryDlAuthConfig {
                            site_category: canonical_site_category.clone(),
                            fragment: self.store.build_extractor_auth(&cred),
                        });
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        warn!(site = %category, error = %error, "Failed to load credential");
                    }
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
        let resolved = self.resolve_credential(site_id, url);
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
        let canonical_site_category = canonical_credential_site_category(site_id);
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
            .get_credential(&canonical_site_category)
            .ok()
            .flatten()
            .is_some();
        if has_credential {
            self.set_health(&canonical_site_category, status, detail)
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
        let canonical_site_category = canonical_credential_site_category(site_id);
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
        expires_at: Option<&str>,
    ) -> Result<(), String> {
        self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO credential_domain (site_category, credential_type, display_name, date_added, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(site_category) DO UPDATE
                 SET credential_type = excluded.credential_type,
                     display_name = excluded.display_name,
                     expires_at = excluded.expires_at",
                rusqlite::params![
                    site_category,
                    credential_type,
                    display_name,
                    chrono::Utc::now().to_rfc3339(),
                    expires_at
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

pub fn canonical_credential_site_category(site_category: &str) -> String {
    let canonical = gallery_dl_runner::canonical_site_id(site_category);
    gallery_dl_runner::site_by_id(canonical)
        .map(|site| site.credential_owner_site_id.to_string())
        .unwrap_or_else(|| canonical.to_string())
}

pub fn credential_lookup_categories(site_id: &str, url: &str) -> Vec<String> {
    let mut categories = Vec::new();
    let canonical_site_id = gallery_dl_runner::canonical_site_id(site_id);
    let canonical_credential_site = canonical_credential_site_category(site_id);

    categories.push(canonical_credential_site.clone());
    categories.push(canonical_site_id.to_string());
    categories.push(site_id.trim().to_string());

    if let Some(site) = gallery_dl_runner::site_by_id(site_id) {
        categories.push(site.domain.to_string());
        categories.push(site.domain.trim_start_matches("www.").to_string());
    }
    if let Some(owner_site) = gallery_dl_runner::site_by_id(&canonical_credential_site) {
        categories.push(owner_site.domain.to_string());
        categories.push(owner_site.domain.trim_start_matches("www.").to_string());
    }
    if let Some(domain) = gallery_dl_runner::extract_domain(url) {
        categories.push(domain.clone());
        categories.push(domain.trim_start_matches("www.").to_string());
    }
    categories.extend(
        gallery_dl_runner::credential_site_aliases(canonical_site_id)
            .iter()
            .map(|alias| (*alias).to_string()),
    );

    categories.sort();
    categories.dedup();
    categories.retain(|value| !value.trim().is_empty());
    categories
}

pub fn credential_delete_categories(site_id: &str) -> Vec<String> {
    let canonical_site_id = gallery_dl_runner::canonical_site_id(site_id);
    let canonical_credential_site = canonical_credential_site_category(site_id);
    let mut categories = vec![
        site_id.trim().to_string(),
        canonical_site_id.to_string(),
        canonical_credential_site.clone(),
    ];

    categories.extend(
        gallery_dl_runner::credential_site_aliases(canonical_site_id)
            .iter()
            .map(|alias| (*alias).to_string()),
    );
    if let Some(site) = gallery_dl_runner::site_by_id(canonical_site_id) {
        categories.push(site.domain.to_string());
        categories.push(site.domain.trim_start_matches("www.").to_string());
    }
    if let Some(owner_site) = gallery_dl_runner::site_by_id(&canonical_credential_site) {
        categories.push(owner_site.domain.to_string());
        categories.push(owner_site.domain.trim_start_matches("www.").to_string());
    }

    categories.sort();
    categories.dedup();
    categories.retain(|value| !value.trim().is_empty());
    categories
}

fn validate_manual_credential(
    site: &gallery_dl_runner::SiteEntry,
    credential_type: CredentialType,
    username: &Option<String>,
    password: &Option<String>,
) -> Result<(), String> {
    if !site.auth_supported {
        return Err(format!("{} does not support stored credentials", site.name));
    }

    let credential_type_str = match credential_type {
        CredentialType::UsernamePassword => "username_password",
        CredentialType::Cookies => "cookies",
        CredentialType::ApiKey => "api_key",
        CredentialType::OAuthToken => "oauth_token",
    };
    if !site
        .manual_credential_types
        .iter()
        .any(|allowed| *allowed == credential_type_str)
    {
        return Err(format!(
            "{} does not accept `{}` credentials",
            site.name, credential_type_str
        ));
    }

    if site.id == "rule34" {
        let user_id_ok = username
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        let api_key_ok = password
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if !user_id_ok || !api_key_ok {
            return Err(
                "rule34.xxx requires both `user-id` and `api-key` (use username=user-id, password=api-key)"
                    .to_string(),
            );
        }
    }

    Ok(())
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
        fn insert(&self, credential: SiteCredential) {
            self.entries
                .lock()
                .unwrap()
                .insert(credential.site_category.clone(), credential);
        }

        fn contains(&self, site_category: &str) -> bool {
            self.entries.lock().unwrap().contains_key(site_category)
        }
    }

    impl CredentialStoreBackend for InMemoryCredentialStore {
        fn set_credential(&self, cred: &SiteCredential) -> Result<(), String> {
            self.entries
                .lock()
                .unwrap()
                .insert(cred.site_category.clone(), cred.clone());
            Ok(())
        }

        fn get_credential(&self, site_category: &str) -> Result<Option<SiteCredential>, String> {
            Ok(self.entries.lock().unwrap().get(site_category).cloned())
        }

        fn delete_credential(&self, site_category: &str) -> Result<(), String> {
            self.entries.lock().unwrap().remove(site_category);
            Ok(())
        }

        fn build_extractor_auth(&self, cred: &SiteCredential) -> Value {
            crate::credential_store::build_extractor_auth(cred)
        }
    }

    async fn test_db() -> std::sync::Arc<LibraryDatabase> {
        let dir = tempfile::tempdir().unwrap();
        std::sync::Arc::new(LibraryDatabase::open(dir.path()).unwrap())
    }

    async fn create_subscription_query(db: &LibraryDatabase, site_id: &str) -> (i64, i64) {
        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db,
            std::path::Path::new("/tmp"),
        );
        let subscription = runtime
            .create_subscription("Test".to_string(), None, None, None)
            .await
            .unwrap();
        let query = runtime
            .add_subscription_query(
                subscription.id.clone(),
                site_id.to_string(),
                None,
                "query".to_string(),
                None,
            )
            .await
            .unwrap();
        (subscription.id.parse().unwrap(), query.id.parse().unwrap())
    }

    #[tokio::test]
    async fn set_manual_credential_for_alias_site_stores_under_owner_category() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        let canonical = service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "pixivuser".to_string(),
                credential_type: "oauth_token".to_string(),
                username: None,
                password: None,
                cookies: None,
                oauth_token: Some("refresh".to_string()),
                display_name: Some("Pixiv".to_string()),
                expires_at: None,
            })
            .await
            .unwrap();

        assert_eq!(canonical, "pixiv");
        assert!(store.contains("pixiv"));

        let credentials = service.list_credentials().await.unwrap();
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].site_category, "pixiv");
    }

    #[tokio::test]
    async fn past_cookie_expiry_overrides_health_to_expired() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store);

        let mut cookies = HashMap::new();
        cookies.insert("auth_token".to_string(), "x".to_string());
        cookies.insert("ct0".to_string(), "y".to_string());
        service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "twitter".to_string(),
                credential_type: "cookies".to_string(),
                username: None,
                password: None,
                cookies: Some(cookies),
                oauth_token: None,
                display_name: None,
                expires_at: Some("2000-01-01T00:00:00+00:00".to_string()),
            })
            .await
            .unwrap();

        let health = service.list_credential_health().await.unwrap();
        let row = health
            .iter()
            .find(|h| h.site_category == "twitter")
            .expect("twitter health row");
        assert_eq!(row.health_status, "expired");
        assert!(row.last_error.as_deref().unwrap_or("").contains("expired"));

        let credentials = service.list_credentials().await.unwrap();
        assert_eq!(
            credentials[0].expires_at.as_deref(),
            Some("2000-01-01T00:00:00+00:00")
        );
    }

    #[tokio::test]
    async fn delete_credential_removes_alias_visible_records_through_service() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        store.insert(SiteCredential {
            site_category: "rule34".to_string(),
            credential_type: CredentialType::ApiKey,
            username: Some("123".to_string()),
            password: Some("secret".to_string()),
            cookies: None,
            oauth_token: None,
        });
        store.insert(SiteCredential {
            site_category: "rule34.xxx".to_string(),
            credential_type: CredentialType::ApiKey,
            username: Some("123".to_string()),
            password: Some("secret".to_string()),
            cookies: None,
            oauth_token: None,
        });
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO credential_domain (site_category, credential_type, display_name, date_added)
                 VALUES ('rule34', 'api_key', 'Rule34', ?1)",
                [chrono::Utc::now().to_rfc3339()],
            )?;
            conn.execute(
                "INSERT INTO credential_domain (site_category, credential_type, display_name, date_added)
                 VALUES ('rule34.xxx', 'api_key', 'Rule34', ?1)",
                [chrono::Utc::now().to_rfc3339()],
            )?;
            conn.execute(
                "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
                 VALUES ('rule34', 'unknown', ?1, NULL)",
                [chrono::Utc::now().to_rfc3339()],
            )?;
            conn.execute(
                "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
                 VALUES ('rule34.xxx', 'unknown', ?1, NULL)",
                [chrono::Utc::now().to_rfc3339()],
            )?;
            Ok::<_, rusqlite::Error>(())
        })
        .unwrap();

        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());
        let canonical = service.delete_credential("rule34.xxx").await.unwrap();

        assert_eq!(canonical, "rule34");
        assert!(!store.contains("rule34"));
        assert!(!store.contains("rule34.xxx"));
        assert!(service.list_credentials().await.unwrap().is_empty());
        assert!(service.list_credential_health().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn pixivuser_run_resolves_stored_pixiv_credential() {
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
        let (subscription_id, query_id) = create_subscription_query(&db, "pixivuser").await;

        let resolved = service
            .resolve_for_run(
                subscription_id,
                Some(query_id),
                "pixivuser",
                "https://www.pixiv.net/en/users/12345",
            )
            .await;

        assert_eq!(resolved.canonical_site_category, "pixiv");
        assert_eq!(resolved.matched_lookup_key.as_deref(), Some("pixiv"));
        assert!(resolved.has_credential());
    }

    #[tokio::test]
    async fn pixivuser_requires_the_shared_pixiv_credential() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(
            db.as_ref(),
            InMemoryCredentialStore::default(),
        );

        let preflight = service
            .preflight_for_run("pixivuser", "https://www.pixiv.net/en/users/12345")
            .await;

        assert!(matches!(preflight, CredentialPreflight::MissingRequired));
    }

    #[tokio::test]
    async fn missing_credential_marks_health_and_creates_issue() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(
            db.as_ref(),
            InMemoryCredentialStore::default(),
        );
        let (subscription_id, query_id) = create_subscription_query(&db, "gelbooru").await;

        let resolved = service
            .resolve_for_run(
                subscription_id,
                Some(query_id),
                "gelbooru",
                "https://gelbooru.com/index.php?page=post&s=list&tags=test",
            )
            .await;

        assert!(!resolved.has_credential());
        let health = service.list_credential_health().await.unwrap();
        assert_eq!(health[0].site_category, "gelbooru");
        assert_eq!(health[0].health_status, "missing");

        let issues = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            std::path::Path::new("/tmp"),
        )
        .list_subscription_issues(subscription_id, Some(query_id), 10)
        .await
        .unwrap();
        assert!(issues
            .iter()
            .any(|issue| issue.issue_kind == "credential_missing"));
    }

    #[tokio::test]
    async fn auth_failure_without_credential_never_touches_health() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(
            db.as_ref(),
            InMemoryCredentialStore::default(),
        );
        let (subscription_id, query_id) = create_subscription_query(&db, "gelbooru").await;

        // No stored credential: an auth-shaped failure (e.g. a CDN 403) must
        // not indict a credential that does not exist.
        service
            .note_run_auth_failure(
                subscription_id,
                Some(query_id),
                "gelbooru",
                FailureKind::Unauthorized,
                Some("401 Unauthorized"),
            )
            .await;

        let health = service.list_credential_health().await.unwrap();
        assert!(health.is_empty());
        let issues = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            std::path::Path::new("/tmp"),
        )
        .list_subscription_issues(subscription_id, Some(query_id), 10)
        .await
        .unwrap();
        assert!(issues
            .iter()
            .any(|issue| issue.issue_kind == "credential_blocked"));
    }

    #[tokio::test]
    async fn auth_failure_marks_blocked_and_success_clears_matching_issues() {
        let db = test_db().await;
        let service = SubscriptionCredentialService::with_store(
            db.as_ref(),
            InMemoryCredentialStore::default(),
        );
        let (subscription_id, query_id) = create_subscription_query(&db, "gelbooru").await;

        service
            .set_manual_credential(SetManualCredentialRequest {
                site_category: "gelbooru".to_string(),
                credential_type: "api_key".to_string(),
                username: Some("123".to_string()),
                password: Some("key".to_string()),
                cookies: None,
                oauth_token: None,
                display_name: None,
                expires_at: None,
            })
            .await
            .unwrap();

        service
            .note_run_auth_failure(
                subscription_id,
                Some(query_id),
                "gelbooru",
                FailureKind::Unauthorized,
                Some("401 Unauthorized"),
            )
            .await;

        let health = service.list_credential_health().await.unwrap();
        assert_eq!(health[0].health_status, "unauthorized");
        let issues = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            std::path::Path::new("/tmp"),
        )
        .list_subscription_issues(subscription_id, Some(query_id), 10)
        .await
        .unwrap();
        assert!(issues
            .iter()
            .any(|issue| issue.issue_kind == "credential_blocked"));

        service
            .note_run_success(subscription_id, Some(query_id), "gelbooru", true)
            .await;

        let health = service.list_credential_health().await.unwrap();
        assert_eq!(health[0].health_status, "valid");
        let issues = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            std::path::Path::new("/tmp"),
        )
        .list_subscription_issues(subscription_id, Some(query_id), 10)
        .await
        .unwrap();
        assert!(issues
            .iter()
            .any(|issue| issue.issue_kind == "credential_blocked" && issue.status == "resolved"));
    }

    #[tokio::test]
    async fn pixiv_oauth_save_uses_shared_persistence_path() {
        let db = test_db().await;
        let store = InMemoryCredentialStore::default();
        let service = SubscriptionCredentialService::with_store(db.as_ref(), store.clone());

        let canonical = service
            .store_pixiv_oauth_credential("refresh".to_string(), Some("phpsessid".to_string()))
            .await
            .unwrap();

        assert_eq!(canonical, "pixiv");
        let credentials = service.list_credentials().await.unwrap();
        assert_eq!(credentials[0].site_category, "pixiv");
        assert_eq!(credentials[0].credential_type, "oauth_token");
        let health = service.list_credential_health().await.unwrap();
        assert_eq!(health[0].health_status, "unknown");

        let resolved = service
            .resolve_for_run(
                1,
                None,
                "pixiv",
                "https://www.pixiv.net/en/tags/test/artworks",
            )
            .await;
        assert!(resolved.has_credential());
        assert!(store.contains("pixiv"));
    }
}
