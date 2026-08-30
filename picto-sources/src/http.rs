use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rand::Rng;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, COOKIE, RETRY_AFTER, USER_AGENT};
use reqwest::Method;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{DownloadedMedia, MediaDescriptor, RequestCredentials, SourceError, SourceErrorKind};

const DEFAULT_USER_AGENT: &str = "Picto/0.6.0-alpha (+https://github.com/midona-rhel/picto)";

#[derive(Debug, Clone)]
pub struct HttpPolicy {
    pub maximum_concurrency: usize,
    pub minimum_interval: Duration,
    pub maximum_interval: Duration,
    pub request_timeout: Duration,
    pub retries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainPolicy {
    pub minimum_interval: Duration,
    pub maximum_interval: Duration,
    pub request_timeout: Duration,
    pub retries: u32,
}

impl DomainPolicy {
    fn validate(&self) -> Result<(), SourceError> {
        if self.minimum_interval > self.maximum_interval || self.request_timeout.is_zero() {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                "invalid domain HTTP policy",
                false,
            ));
        }
        Ok(())
    }
}

impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            maximum_concurrency: 4,
            minimum_interval: Duration::from_millis(500),
            maximum_interval: Duration::from_secs(2),
            request_timeout: Duration::from_secs(45),
            retries: 3,
        }
    }
}

pub struct HttpRuntime {
    client: reqwest::Client,
    cookies: Arc<Jar>,
    default_policy: DomainPolicy,
    domain_policies: BTreeMap<String, DomainPolicy>,
    permits: Arc<Semaphore>,
    next_request_by_domain: Mutex<HashMap<String, Instant>>,
}

struct ResponseLease {
    response: reqwest::Response,
    _permit: OwnedSemaphorePermit,
}

#[derive(Clone, Copy)]
enum RequestPurpose {
    Metadata,
    Media,
}

impl HttpRuntime {
    pub fn new(policy: HttpPolicy) -> Result<Self, SourceError> {
        Self::with_domain_policies(policy, BTreeMap::new())
    }

    pub fn with_domain_policies(
        policy: HttpPolicy,
        domain_policies: BTreeMap<String, DomainPolicy>,
    ) -> Result<Self, SourceError> {
        if policy.maximum_concurrency == 0 {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                "invalid native HTTP policy",
                false,
            ));
        }
        let default_policy = DomainPolicy {
            minimum_interval: policy.minimum_interval,
            maximum_interval: policy.maximum_interval,
            request_timeout: policy.request_timeout,
            retries: policy.retries,
        };
        default_policy.validate()?;
        for domain_policy in domain_policies.values() {
            domain_policy.validate()?;
        }
        let cookies = Arc::new(Jar::default());
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .cookie_provider(Arc::clone(&cookies))
            .build()
            .map_err(|error| SourceError::new(SourceErrorKind::Network, error.to_string(), true))?;
        Ok(Self {
            client,
            cookies,
            permits: Arc::new(Semaphore::new(policy.maximum_concurrency)),
            default_policy,
            domain_policies,
            next_request_by_domain: Mutex::new(HashMap::new()),
        })
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<T, SourceError> {
        let response = self
            .get(
                url,
                credentials,
                &HeaderMap::new(),
                RequestPurpose::Metadata,
                cancel,
            )
            .await?;
        response.response.json::<T>().await.map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })
    }

    pub async fn get_text(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<String, SourceError> {
        let response = self
            .get(
                url,
                credentials,
                &HeaderMap::new(),
                RequestPurpose::Metadata,
                cancel,
            )
            .await?;
        response.response.text().await.map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })
    }

    /// Fetch a post detail page where a source-level 403/404 means the post is
    /// inaccessible, not that the entire query or captured session is invalid.
    pub async fn get_optional_text(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, SourceError> {
        let response = self.get_with_inaccessible(url, credentials, cancel).await?;
        if is_inaccessible(response.response.status()) {
            return Ok(None);
        }
        response.response.text().await.map(Some).map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })
    }

    pub async fn head(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<(), SourceError> {
        self.request(
            Method::HEAD,
            url,
            credentials,
            &HeaderMap::new(),
            RequestPurpose::Metadata,
            None,
            false,
            cancel,
        )
        .await?;
        Ok(())
    }

    pub async fn post_form_text(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        form: &BTreeMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<String, SourceError> {
        let response = self
            .request(
                Method::POST,
                url,
                credentials,
                &HeaderMap::new(),
                RequestPurpose::Metadata,
                Some(form),
                false,
                cancel,
            )
            .await?;
        response.response.text().await.map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })
    }

    pub fn cookie_value(&self, url: &Url, name: &str) -> Option<String> {
        let values = self.cookies.cookies(url)?;
        values.to_str().ok()?.split(';').find_map(|entry| {
            let (key, value) = entry.trim().split_once('=')?;
            (key == name).then(|| value.to_string())
        })
    }

    pub async fn download(
        &self,
        descriptor: &MediaDescriptor,
        credentials: &RequestCredentials,
        destination: &Path,
        cancel: &CancellationToken,
    ) -> Result<DownloadedMedia, SourceError> {
        let url = Url::parse(&descriptor.url).map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })?;
        let headers = parse_headers(&descriptor.headers)?;
        let response = self
            .get(url, credentials, &headers, RequestPurpose::Media, cancel)
            .await?;
        let temporary = destination.with_extension("picto-part");
        if let Some(parent) = temporary.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                SourceError::new(SourceErrorKind::Download, error.to_string(), true)
            })?;
        }
        let mut file = tokio::fs::File::create(&temporary).await.map_err(|error| {
            SourceError::new(SourceErrorKind::Download, error.to_string(), true)
        })?;
        let mut stream = response.response.bytes_stream();
        let mut size_bytes = 0_u64;
        while let Some(chunk) = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(SourceError::new(SourceErrorKind::Cancelled, "download cancelled", true));
            }
            chunk = stream.next() => chunk
        } {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&temporary).await;
                    return Err(SourceError::new(
                        SourceErrorKind::Download,
                        error.to_string(),
                        true,
                    ));
                }
            };
            if let Err(error) = file.write_all(&chunk).await {
                drop(file);
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(SourceError::new(
                    SourceErrorKind::Download,
                    error.to_string(),
                    true,
                ));
            }
            size_bytes = size_bytes.saturating_add(chunk.len() as u64);
        }
        if let Err(error) = file.flush().await {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(SourceError::new(
                SourceErrorKind::Download,
                error.to_string(),
                true,
            ));
        }
        drop(file);
        if let Err(error) = tokio::fs::rename(&temporary, destination).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(SourceError::new(
                SourceErrorKind::Download,
                error.to_string(),
                true,
            ));
        }
        Ok(DownloadedMedia {
            descriptor: descriptor.clone(),
            path: destination.to_path_buf(),
            size_bytes,
        })
    }

    async fn get(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        request_headers: &HeaderMap,
        purpose: RequestPurpose,
        cancel: &CancellationToken,
    ) -> Result<ResponseLease, SourceError> {
        self.request(
            Method::GET,
            url,
            credentials,
            request_headers,
            purpose,
            None,
            false,
            cancel,
        )
        .await
    }

    async fn get_with_inaccessible(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<ResponseLease, SourceError> {
        self.request(
            Method::GET,
            url,
            credentials,
            &HeaderMap::new(),
            RequestPurpose::Metadata,
            None,
            true,
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request(
        &self,
        method: Method,
        url: Url,
        credentials: &RequestCredentials,
        request_headers: &HeaderMap,
        purpose: RequestPurpose,
        form: Option<&BTreeMap<String, String>>,
        allow_inaccessible: bool,
        cancel: &CancellationToken,
    ) -> Result<ResponseLease, SourceError> {
        let domain = url
            .host_str()
            .ok_or_else(|| {
                SourceError::new(SourceErrorKind::InvalidQuery, "URL has no domain", false)
            })?
            .to_string();
        let policy = self.policy_for_domain(&domain);
        let retries = retry_count(purpose, policy);
        let mut last_error = None;
        for attempt in 0..=retries {
            self.wait_for_domain(&domain, policy, cancel).await?;
            let permit = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(SourceError::new(SourceErrorKind::Cancelled, "request cancelled", true));
                }
                permit = self.permits.clone().acquire_owned() => permit.map_err(|_| {
                    SourceError::new(SourceErrorKind::Cancelled, "native HTTP runtime stopped", true)
                })?
            };
            let headers = credential_headers(credentials, &domain, request_headers)?;
            trace_request(&domain, method.as_str(), attempt, policy.minimum_interval);
            let response = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(SourceError::new(SourceErrorKind::Cancelled, "request cancelled", true));
                }
                response = {
                    let mut request = self.client
                        .request(method.clone(), url.clone())
                        .headers(headers)
                        .timeout(policy.request_timeout);
                    if let Some(form) = form {
                        request = request.form(form);
                    }
                    request.send()
                } => response
            };
            match response {
                Ok(response) if response.status().is_success() => {
                    return Ok(ResponseLease {
                        response,
                        _permit: permit,
                    });
                }
                Ok(response) if allow_inaccessible && is_inaccessible(response.status()) => {
                    return Ok(ResponseLease {
                        response,
                        _permit: permit,
                    });
                }
                Ok(response)
                    if response.status().as_u16() == 401 || response.status().as_u16() == 403 =>
                {
                    return Err(access_error(purpose, &domain, response.status()));
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    let delay =
                        retry_after(response.headers()).unwrap_or_else(|| retry_delay(attempt));
                    last_error = Some(SourceError::new(
                        SourceErrorKind::RateLimited,
                        format!("{} rate limited the request", domain),
                        true,
                    ));
                    if attempt < retries {
                        wait_or_cancel(delay, cancel).await?;
                    }
                }
                Ok(response) if response.status().is_server_error() => {
                    last_error = Some(SourceError::new(
                        SourceErrorKind::Network,
                        format!("{} returned {}", domain, response.status()),
                        true,
                    ));
                    if attempt < retries {
                        wait_or_cancel(retry_delay(attempt), cancel).await?;
                    }
                }
                Ok(response) => {
                    return Err(SourceError::new(
                        SourceErrorKind::InvalidResponse,
                        format!("{} returned {}", domain, response.status()),
                        false,
                    ));
                }
                Err(error) => {
                    last_error = Some(SourceError::new(
                        SourceErrorKind::Network,
                        error.to_string(),
                        true,
                    ));
                    if attempt < retries {
                        wait_or_cancel(retry_delay(attempt), cancel).await?;
                    }
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| SourceError::new(SourceErrorKind::Network, "request failed", true)))
    }

    async fn wait_for_domain(
        &self,
        domain: &str,
        policy: &DomainPolicy,
        cancel: &CancellationToken,
    ) -> Result<(), SourceError> {
        let delay = random_duration(policy.minimum_interval, policy.maximum_interval);
        let wait = {
            let mut schedule = self.next_request_by_domain.lock().await;
            let now = Instant::now();
            let allowed = schedule.get(domain).copied().unwrap_or(now);
            let start = allowed.max(now);
            schedule.insert(domain.to_string(), start + delay);
            start.saturating_duration_since(now)
        };
        wait_or_cancel(wait, cancel).await
    }

    fn policy_for_domain(&self, domain: &str) -> &DomainPolicy {
        self.domain_policies
            .get(domain)
            .or_else(|| {
                self.domain_policies.iter().find_map(|(parent, policy)| {
                    domain
                        .strip_suffix(parent)
                        .is_some_and(|prefix| prefix.ends_with('.'))
                        .then_some(policy)
                })
            })
            .unwrap_or(&self.default_policy)
    }
}

fn retry_count(purpose: RequestPurpose, policy: &DomainPolicy) -> u32 {
    match purpose {
        RequestPurpose::Metadata => policy.retries,
        RequestPurpose::Media => policy.retries.min(1),
    }
}

fn trace_request(domain: &str, method: &str, attempt: u32, minimum_interval: Duration) {
    let Some(path) = std::env::var_os("PICTO_TRACE_REQUESTS") else {
        return;
    };
    static START: OnceLock<Instant> = OnceLock::new();
    let monotonic_ms = START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(i64::MAX as u128) as i64;
    let entry = serde_json::json!({
        "host": domain,
        "method": method,
        "attempt": attempt,
        "minimum_interval_ms": minimum_interval.as_millis(),
        "monotonic_ms": monotonic_ms,
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{entry}");
    }
}

fn is_inaccessible(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 403 | 404)
}

fn access_error(purpose: RequestPurpose, domain: &str, status: reqwest::StatusCode) -> SourceError {
    match purpose {
        RequestPurpose::Metadata => SourceError::new(
            SourceErrorKind::Authentication,
            format!("{domain} returned {status}"),
            false,
        ),
        RequestPurpose::Media => SourceError::new(
            SourceErrorKind::Download,
            format!("media host {domain} returned {status}"),
            false,
        ),
    }
}

fn credential_headers(
    credentials: &RequestCredentials,
    domain: &str,
    request_headers: &HeaderMap,
) -> Result<HeaderMap, SourceError> {
    let mut headers = if credentials.permits(domain) {
        parse_headers(&credentials.headers)?
    } else {
        HeaderMap::new()
    };
    headers.extend(request_headers.clone());
    if credentials.permits(domain) && !credentials.cookies.is_empty() {
        let cookie = credentials
            .cookies
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&cookie).map_err(invalid_header)?,
        );
    }
    if !headers.contains_key(USER_AGENT) {
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
    }
    Ok(headers)
}

fn parse_headers(
    values: &std::collections::BTreeMap<String, String>,
) -> Result<HeaderMap, SourceError> {
    let mut headers = HeaderMap::new();
    for (name, value) in values {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).map_err(invalid_header)?,
            HeaderValue::from_str(value).map_err(invalid_header)?,
        );
    }
    Ok(headers)
}

fn invalid_header(error: impl std::fmt::Display) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, error.to_string(), false)
}

fn random_duration(minimum: Duration, maximum: Duration) -> Duration {
    if minimum >= maximum {
        return minimum;
    }
    let min_ms = minimum.as_millis().min(u64::MAX as u128) as u64;
    let max_ms = maximum.as_millis().min(u64::MAX as u128) as u64;
    Duration::from_millis(rand::thread_rng().gen_range(min_ms..=max_ms))
}

fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(500_u64.saturating_mul(1_u64 << attempt.min(6)))
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

async fn wait_or_cancel(delay: Duration, cancel: &CancellationToken) -> Result<(), SourceError> {
    if delay.is_zero() {
        return Ok(());
    }
    tokio::select! {
        _ = cancel.cancelled() => Err(SourceError::new(SourceErrorKind::Cancelled, "request cancelled", true)),
        _ = tokio::time::sleep(delay) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_domain_overrides_independently_of_global_concurrency() {
        let mut domains = BTreeMap::new();
        domains.insert(
            "api.example.test".into(),
            DomainPolicy {
                minimum_interval: Duration::from_secs(2),
                maximum_interval: Duration::from_secs(1),
                request_timeout: Duration::from_secs(30),
                retries: 1,
            },
        );
        assert!(HttpRuntime::with_domain_policies(HttpPolicy::default(), domains).is_err());
    }

    #[test]
    fn media_failures_do_not_amplify_into_metadata_retry_windows() {
        let policy = DomainPolicy {
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(45),
            retries: 3,
        };
        assert_eq!(retry_count(RequestPurpose::Metadata, &policy), 3);
        assert_eq!(retry_count(RequestPurpose::Media, &policy), 1);
    }

    #[test]
    fn parent_domain_policy_applies_to_provider_cdn_subdomains() {
        let override_policy = DomainPolicy {
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(30),
            retries: 2,
        };
        let runtime = HttpRuntime::with_domain_policies(
            HttpPolicy::default(),
            BTreeMap::from([("onlyfans.com".into(), override_policy.clone())]),
        )
        .unwrap();
        assert_eq!(runtime.policy_for_domain("onlyfans.com"), &override_policy);
        assert_eq!(
            runtime.policy_for_domain("cdn2.onlyfans.com"),
            &override_policy
        );
        assert_eq!(
            runtime.policy_for_domain("notonlyfans.com"),
            &runtime.default_policy
        );
    }

    #[test]
    fn credentials_are_only_attached_to_allowed_first_party_domains() {
        let credentials = RequestCredentials {
            headers: [("authorization".into(), "secret".into())]
                .into_iter()
                .collect(),
            cookies: [("session".into(), "secret".into())].into_iter().collect(),
            allowed_domains: ["example.test".into()].into_iter().collect(),
            ..RequestCredentials::default()
        };
        let first_party =
            credential_headers(&credentials, "api.example.test", &HeaderMap::new()).unwrap();
        assert!(first_party.contains_key("authorization"));
        assert!(first_party.contains_key(COOKIE));

        let third_party =
            credential_headers(&credentials, "cdn.example.invalid", &HeaderMap::new()).unwrap();
        assert!(!third_party.contains_key("authorization"));
        assert!(!third_party.contains_key(COOKIE));
        assert_eq!(
            third_party
                .get(USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some(DEFAULT_USER_AGENT)
        );
    }

    #[test]
    fn runtime_cookie_jar_exposes_first_party_session_values() {
        let runtime = HttpRuntime::new(HttpPolicy::default()).unwrap();
        let url = Url::parse("https://example.test/path").unwrap();
        runtime
            .cookies
            .add_cookie_str("session=stored; Path=/; HttpOnly", &url);
        assert_eq!(
            runtime.cookie_value(&url, "session").as_deref(),
            Some("stored")
        );
        assert_eq!(runtime.cookie_value(&url, "missing"), None);
    }
}
