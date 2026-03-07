//! HTTP client for the Hydrus PTR (Public Tag Repository) server.
//!
//! Supports the native Hydrus repository protocol:
//! - Session key authentication
//! - Metadata fetching (what updates exist)
//! - Individual update downloading (definitions + content)

use crate::ptr_types::{self, PtrMetadata, PtrUpdate};

/// Default PTR server URL (the public Hydrus PTR).
/// Port 45871 is the standard Hydrus repository port.
pub const DEFAULT_PTR_URL: &str = "https://ptr.hydrus.network:45871";

/// Well-known public access key for the Hydrus PTR (read-only).
pub const DEFAULT_PTR_ACCESS_KEY: &str =
    "4a285629721ca442541ef2c15ea17d1f7f7578b0c3f4f5f2a05f8f0ab297786f";

pub struct PtrClient {
    base_url: String,
    access_key: String,
    http: reqwest::Client,
    session_key: tokio::sync::Mutex<Option<String>>,
}

impl PtrClient {
    pub fn new(base_url: &str, access_key: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Picto/1.0 (hydrus-compatible)")
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(15))
            // PTR server uses a self-signed certificate — Hydrus sets verify=False
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            access_key: access_key.to_string(),
            http,
            session_key: tokio::sync::Mutex::new(None),
        }
    }

    /// Get or create a session key from the server.
    ///
    /// The Hydrus PTR server returns the session key as a `Set-Cookie` header
    /// with an empty body (NOT JSON). Format: `session_key=<hex>; Max-Age=...; Path=/`
    ///
    /// Holds the lock across the HTTP request so only one session request fires
    /// even under concurrent access.
    async fn ensure_session_key(&self) -> Result<String, String> {
        let mut guard = self.session_key.lock().await;
        if let Some(ref key) = *guard {
            return Ok(key.clone());
        }

        tracing::info!("PTR client: requesting session key");
        let url = format!("{}/session_key", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Hydrus-Key", &self.access_key)
            .send()
            .await
            .map_err(|e| format!("Session key request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Session key request returned HTTP {}",
                resp.status()
            ));
        }

        // Extract session_key from Set-Cookie header
        let session_key = resp
            .cookies()
            .find(|c| c.name() == "session_key")
            .map(|c| c.value().to_string())
            .ok_or_else(|| "No session_key cookie in response".to_string())?;

        *guard = Some(session_key.clone());

        Ok(session_key)
    }

    /// Clear session key (e.g. on 403 to force re-auth).
    async fn clear_session_key(&self) {
        let mut guard = self.session_key.lock().await;
        *guard = None;
    }

    /// Build the Cookie header value for authenticated requests.
    fn session_cookie(session_key: &str) -> String {
        format!("session_key={}", session_key)
    }

    /// Fetch metadata about available updates since a given index.
    pub async fn get_metadata(&self, since: u64) -> Result<PtrMetadata, String> {
        let session_key = self.ensure_session_key().await?;
        let url = format!("{}/metadata?since={}", self.base_url, since);

        let resp = self
            .http
            .get(&url)
            .header("Cookie", Self::session_cookie(&session_key))
            .send()
            .await
            .map_err(|e| format!("Metadata request failed: {}", e))?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            self.clear_session_key().await;
            return Err("Authentication failed (403). Session expired.".into());
        }

        if !resp.status().is_success() {
            return Err(format!("Metadata request returned HTTP {}", resp.status()));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Metadata read error: {}", e))?;

        let json_str = ptr_types::decompress_network_bytes(&bytes)?;
        ptr_types::parse_metadata(&json_str)
    }

    /// Download a single update file by its hash.
    pub async fn get_update(&self, update_hash: &str) -> Result<PtrUpdate, String> {
        let session_key = self.ensure_session_key().await?;
        let url = format!("{}/update?update_hash={}", self.base_url, update_hash);

        let resp = self
            .http
            .get(&url)
            .header("Cookie", Self::session_cookie(&session_key))
            .send()
            .await
            .map_err(|e| format!("Update request failed: {}", e))?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            self.clear_session_key().await;
            return Err("Authentication failed (403). Session expired.".into());
        }

        if !resp.status().is_success() {
            return Err(format!(
                "Update request returned HTTP {} for hash {}",
                resp.status(),
                update_hash
            ));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Update read error: {}", e))?;

        // Decompress + parse on a blocking thread to avoid starving the tokio runtime
        tokio::task::spawn_blocking(move || {
            let json_str = ptr_types::decompress_network_bytes(&bytes)?;
            ptr_types::parse_update(&json_str)
        })
        .await
        .map_err(|e| format!("Parse task panicked: {}", e))?
    }
}
