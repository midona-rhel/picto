use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
use wreq::cookie::CookieStore as WreqCookieStore;
use wreq_util::Emulation;

use crate::{DownloadedMedia, MediaDescriptor, RequestCredentials, SourceError, SourceErrorKind};

pub(crate) const APPLICATION_USER_AGENT: &str =
    "Picto/0.6.0-alpha (+https://github.com/midona-rhel/picto)";
const DEFAULT_BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0";
const DEFAULT_RATE_LIMIT_DELAY: Duration = Duration::from_secs(60);

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
    pub media_minimum_interval: Duration,
    pub media_maximum_interval: Duration,
    pub request_timeout: Duration,
    pub retries: u32,
}

impl DomainPolicy {
    fn validate(&self) -> Result<(), SourceError> {
        if self.minimum_interval > self.maximum_interval
            || self.media_minimum_interval > self.media_maximum_interval
            || self.request_timeout.is_zero()
        {
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
            minimum_interval: Duration::from_secs(1),
            maximum_interval: Duration::from_secs(2),
            request_timeout: Duration::from_secs(45),
            retries: 4,
        }
    }
}

pub struct HttpRuntime {
    client: reqwest::Client,
    browser_client: wreq::Client,
    browser_cookies: Arc<wreq::cookie::Jar>,
    cookies: Arc<Jar>,
    default_policy: DomainPolicy,
    domain_policies: BTreeMap<String, DomainPolicy>,
    permits: Arc<Semaphore>,
    next_request_by_domain: Mutex<HashMap<String, Instant>>,
    adaptive_delay_by_domain: Mutex<HashMap<String, Duration>>,
}

struct ResponseLease {
    response: reqwest::Response,
    _permit: OwnedSemaphorePermit,
}

struct BrowserResponseLease {
    response: wreq::Response,
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
            media_minimum_interval: policy.minimum_interval,
            media_maximum_interval: policy.maximum_interval,
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
        let browser_cookies = Arc::new(wreq::cookie::Jar::default());
        let browser_client = wreq::Client::builder()
            .emulation(Emulation::Firefox139)
            .cookie_provider(Arc::clone(&browser_cookies))
            .build()
            .map_err(|error| SourceError::new(SourceErrorKind::Network, error.to_string(), true))?;
        Ok(Self {
            client,
            browser_client,
            browser_cookies,
            cookies,
            permits: Arc::new(Semaphore::new(policy.maximum_concurrency)),
            default_policy,
            domain_policies,
            next_request_by_domain: Mutex::new(HashMap::new()),
            adaptive_delay_by_domain: Mutex::new(HashMap::new()),
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

    /// Fetch JSON for an optional provider resource. A 403/404 means the
    /// resource is unavailable, not that the whole source session failed.
    pub async fn get_optional_json<T: DeserializeOwned>(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<Option<T>, SourceError> {
        let response = self.get_with_inaccessible(url, credentials, cancel).await?;
        if is_inaccessible(response.response.status()) {
            return Ok(None);
        }
        response
            .response
            .json::<T>()
            .await
            .map(Some)
            .map_err(|error| {
                SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
            })
    }

    /// Fetch JSON with Firefox TLS emulation while retaining shared pacing,
    /// retries, credentials, and response cookies.
    pub async fn get_browser_json<T: DeserializeOwned>(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<T, SourceError> {
        let response = self.browser_get(url, credentials, false, cancel).await?;
        response.response.json::<T>().await.map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })
    }

    pub async fn get_browser_optional_json<T: DeserializeOwned>(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<Option<T>, SourceError> {
        let response = self.browser_get(url, credentials, true, cancel).await?;
        if is_inaccessible(response.response.status()) {
            return Ok(None);
        }
        response
            .response
            .json::<T>()
            .await
            .map(Some)
            .map_err(|error| {
                SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
            })
    }

    pub async fn get_browser_text(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<String, SourceError> {
        let response = self.browser_get(url, credentials, false, cancel).await?;
        response.response.text().await.map_err(|error| {
            SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
        })
    }

    async fn browser_get(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        accepts_inaccessible: bool,
        cancel: &CancellationToken,
    ) -> Result<BrowserResponseLease, SourceError> {
        let domain = url
            .host_str()
            .ok_or_else(|| {
                SourceError::new(SourceErrorKind::InvalidQuery, "URL has no host", false)
            })?
            .to_ascii_lowercase();
        let policy = self.policy_for_domain(&domain);
        let retries = retry_count(RequestPurpose::Metadata, policy);
        let mut last_error = None;
        for attempt in 0..=retries {
            let wait = self
                .wait_for_domain(
                    &domain,
                    policy.minimum_interval,
                    policy.maximum_interval,
                    cancel,
                )
                .await?;
            #[cfg(debug_assertions)]
            tracing::debug!(
                target: "picto_sources::http",
                host = %domain,
                purpose = "metadata",
                attempt,
                wait_ms = wait.as_millis() as u64,
                browser = "firefox",
                "HTTP request started"
            );
            let permit = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(SourceError::new(SourceErrorKind::Cancelled, "request cancelled", true));
                }
                permit = Arc::clone(&self.permits).acquire_owned() => {
                    permit.map_err(|_| SourceError::new(SourceErrorKind::Cancelled, "native HTTP runtime stopped", true))?
                }
            };
            let mut request = self
                .browser_client
                .get(url.as_str())
                .timeout(policy.request_timeout);
            if credentials.permits(&domain) {
                for (name, value) in &credentials.headers {
                    request = request.header(name.as_str(), value.as_str());
                }
                if !credentials.cookies.is_empty() {
                    let cookie = credentials
                        .cookies
                        .iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join("; ");
                    request = request.header("Cookie", cookie);
                }
            }
            let response = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(SourceError::new(SourceErrorKind::Cancelled, "request cancelled", true));
                }
                response = request.send() => response
            };
            #[cfg(debug_assertions)]
            match &response {
                Ok(response) => tracing::debug!(
                    target: "picto_sources::http",
                    host = %domain,
                    purpose = "metadata",
                    attempt,
                    status = response.status().as_u16(),
                    browser = "firefox",
                    "HTTP response received"
                ),
                Err(_) => tracing::debug!(
                    target: "picto_sources::http",
                    host = %domain,
                    purpose = "metadata",
                    attempt,
                    browser = "firefox",
                    "HTTP request failed before a response"
                ),
            }
            match response {
                Ok(response) if response.status().is_success() => {
                    if let Some(delay) = proactive_rate_limit_delay(response.headers()) {
                        self.defer_domain(&domain, delay).await;
                    }
                    return Ok(BrowserResponseLease {
                        response,
                        _permit: permit,
                    });
                }
                Ok(response) if accepts_inaccessible && is_inaccessible(response.status()) => {
                    return Ok(BrowserResponseLease {
                        response,
                        _permit: permit,
                    });
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    let delay = retry_after(response.headers())
                        .or_else(|| rate_limit_reset(response.headers()))
                        .unwrap_or(DEFAULT_RATE_LIMIT_DELAY);
                    last_error = Some(SourceError::new(
                        SourceErrorKind::RateLimited,
                        format!("{} rate limited the request", domain),
                        true,
                    ));
                    drop(permit);
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
                    drop(permit);
                    if attempt < retries {
                        wait_or_cancel(retry_delay(attempt), cancel).await?;
                    }
                }
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    let status = response.status();
                    let provider_message = response
                        .text()
                        .await
                        .ok()
                        .as_deref()
                        .and_then(provider_error_message);
                    #[cfg(debug_assertions)]
                    tracing::debug!(
                        target: "picto_sources::http",
                        host = %domain,
                        status = status.as_u16(),
                        provider_message = provider_message.as_deref().unwrap_or("unavailable"),
                        "Authenticated browser request was rejected"
                    );
                    return Err(access_error_with_provider_message(
                        RequestPurpose::Metadata,
                        &domain,
                        status,
                        provider_message.as_deref(),
                    ));
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
                    drop(permit);
                    if attempt < retries {
                        wait_or_cancel(retry_delay(attempt), cancel).await?;
                    }
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| SourceError::new(SourceErrorKind::Network, "request failed", true)))
    }

    pub async fn get_text(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<String, SourceError> {
        self.get_text_with_final_url(url, credentials, cancel)
            .await
            .map(|(_, body)| body)
    }

    pub async fn get_text_with_final_url(
        &self,
        url: Url,
        credentials: &RequestCredentials,
        cancel: &CancellationToken,
    ) -> Result<(Url, String), SourceError> {
        let response = self
            .get(
                url,
                credentials,
                &HeaderMap::new(),
                RequestPurpose::Metadata,
                cancel,
            )
            .await?;
        let final_url = response.response.url().clone();
        response
            .response
            .text()
            .await
            .map(|body| (final_url, body))
            .map_err(|error| {
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
        self.cookies
            .cookies(url)
            .and_then(|values| cookie_header_value(values.to_str().ok()?, name))
            .or_else(|| {
                self.browser_cookies
                    .cookies(url)
                    .and_then(|values| cookie_header_value(values.to_str().ok()?, name))
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
        let accepts_unavailable = descriptor
            .fallbacks
            .iter()
            .any(|fallback| fallback.html_marker.is_none());
        let response = self
            .request(
                Method::GET,
                url,
                credentials,
                &headers,
                RequestPurpose::Media,
                None,
                accepts_unavailable,
                cancel,
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.kind == SourceErrorKind::Network => {
                if let Some(fallback_index) = descriptor
                    .fallbacks
                    .iter()
                    .position(|fallback| fallback.html_marker.is_none())
                {
                    let fallback = descriptor
                        .fallback_descriptor(fallback_index)
                        .expect("checked media fallback exists");
                    trace_media_fallback(&fallback);
                    return Box::pin(self.download(&fallback, credentials, destination, cancel))
                        .await;
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let ResponseLease {
            response,
            _permit: permit,
        } = response;
        if is_inaccessible(response.status()) {
            drop(permit);
            if let Some(fallback_index) = descriptor
                .fallbacks
                .iter()
                .position(|fallback| fallback.html_marker.is_none())
            {
                let fallback = descriptor
                    .fallback_descriptor(fallback_index)
                    .expect("checked media fallback exists");
                trace_media_fallback(&fallback);
                return Box::pin(self.download(&fallback, credentials, destination, cancel)).await;
            }
            return Err(access_error(
                RequestPurpose::Media,
                response.url().host_str().unwrap_or_default(),
                response.status(),
            ));
        }
        if rejects_final_url(descriptor, response.url()) {
            drop(permit);
            return Err(SourceError::new(
                SourceErrorKind::Download,
                "The provider media URL expired before download; retry the post",
                true,
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        #[cfg(debug_assertions)]
        tracing::debug!(
            target: "picto_sources::http",
            host = response.url().host_str().unwrap_or_default(),
            content_length = response.content_length(),
            content_type,
            "Media response opened"
        );
        if content_type
            .split(';')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/html"))
        {
            let body = response.text().await.map_err(|error| {
                SourceError::new(SourceErrorKind::InvalidResponse, error.to_string(), false)
            })?;
            drop(permit);
            let body_lower = body.to_ascii_lowercase();
            if let Some(error) = terminal_html_media_error(&body_lower) {
                return Err(error);
            }
            if let Some(fallback_index) = descriptor.fallbacks.iter().position(|fallback| {
                fallback
                    .html_marker
                    .as_ref()
                    .is_none_or(|marker| body_lower.contains(&marker.to_ascii_lowercase()))
            }) {
                let fallback = descriptor
                    .fallback_descriptor(fallback_index)
                    .expect("checked media fallback exists");
                trace_media_fallback(&fallback);
                return Box::pin(self.download(&fallback, credentials, destination, cancel)).await;
            }
            return Err(html_media_error(&body_lower));
        }
        let temporary = destination.with_extension("picto-part");
        if let Some(parent) = temporary.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                SourceError::new(SourceErrorKind::Download, error.to_string(), true)
            })?;
        }
        let mut file = tokio::fs::File::create(&temporary).await.map_err(|error| {
            SourceError::new(SourceErrorKind::Download, error.to_string(), true)
        })?;
        let mut stream = response.bytes_stream();
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
        if size_bytes == 0 {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(SourceError::new(
                SourceErrorKind::Download,
                "media response was empty",
                true,
            ));
        }
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
        let (minimum_interval, maximum_interval) = request_interval(purpose, policy);
        let retries = retry_count(purpose, policy);
        let attempt_limit = retry_attempt_limit(&domain, purpose, retries);
        let mut last_error = None;
        for attempt in 0..=attempt_limit {
            let (minimum_interval, maximum_interval) = self
                .adaptive_request_interval(&domain, purpose, minimum_interval, maximum_interval)
                .await;
            let wait = self
                .wait_for_domain(&domain, minimum_interval, maximum_interval, cancel)
                .await?;
            #[cfg(debug_assertions)]
            tracing::debug!(
                target: "picto_sources::http",
                host = %domain,
                purpose = purpose.as_str(),
                attempt,
                wait_ms = wait.as_millis() as u64,
                minimum_interval_ms = minimum_interval.as_millis() as u64,
                maximum_interval_ms = maximum_interval.as_millis() as u64,
                "HTTP request started"
            );
            let permit = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(SourceError::new(SourceErrorKind::Cancelled, "request cancelled", true));
                }
                permit = self.permits.clone().acquire_owned() => permit.map_err(|_| {
                    SourceError::new(SourceErrorKind::Cancelled, "native HTTP runtime stopped", true)
                })?
            };
            let headers = credential_headers(credentials, &domain, request_headers)?;
            trace_request(&domain, method.as_str(), attempt, minimum_interval);
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
            #[cfg(debug_assertions)]
            match &response {
                Ok(response) => tracing::debug!(
                    target: "picto_sources::http",
                    host = %domain,
                    purpose = purpose.as_str(),
                    attempt,
                    status = response.status().as_u16(),
                    "HTTP response received"
                ),
                Err(_) => tracing::debug!(
                    target: "picto_sources::http",
                    host = %domain,
                    purpose = purpose.as_str(),
                    attempt,
                    "HTTP request failed before a response"
                ),
            }
            match response {
                Ok(response) if response.status().is_success() => {
                    self.note_successful_request(&domain, purpose).await;
                    if let Some(delay) = proactive_rate_limit_delay(response.headers()) {
                        self.defer_domain(&domain, delay).await;
                    }
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
                    if response.status().as_u16() == 403 && is_deviantart_domain(&domain) =>
                {
                    let blocked = response
                        .text()
                        .await
                        .ok()
                        .is_some_and(|body| is_deviantart_request_block(&body));
                    if !blocked {
                        return Err(access_error(
                            purpose,
                            &domain,
                            reqwest::StatusCode::FORBIDDEN,
                        ));
                    }
                    last_error = Some(SourceError::new(
                        SourceErrorKind::RateLimited,
                        "DeviantArt temporarily blocked the request; retrying after its cooldown",
                        true,
                    ));
                    if attempt < attempt_limit {
                        wait_or_cancel(Duration::from_secs(300), cancel).await?;
                    }
                }
                Ok(response)
                    if response.status().as_u16() == 401 || response.status().as_u16() == 403 =>
                {
                    return Err(access_error(purpose, &domain, response.status()));
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    let delay = if is_deviantart_domain(&domain)
                        && matches!(purpose, RequestPurpose::Metadata)
                    {
                        self.increase_adaptive_delay(&domain).await
                    } else {
                        retry_after(response.headers())
                            .or_else(|| rate_limit_reset(response.headers()))
                            .unwrap_or(DEFAULT_RATE_LIMIT_DELAY)
                    };
                    last_error = Some(SourceError::new(
                        SourceErrorKind::RateLimited,
                        format!("{} rate limited the request", domain),
                        true,
                    ));
                    if attempt < attempt_limit {
                        if is_deviantart_domain(&domain)
                            && matches!(purpose, RequestPurpose::Metadata)
                        {
                            self.defer_domain(&domain, delay).await;
                        } else {
                            wait_or_cancel(delay, cancel).await?;
                        }
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
                    } else {
                        return Err(last_error.take().expect("server error was recorded"));
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
                    } else {
                        return Err(last_error.take().expect("network error was recorded"));
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
        minimum_interval: Duration,
        maximum_interval: Duration,
        cancel: &CancellationToken,
    ) -> Result<Duration, SourceError> {
        let delay = random_duration(minimum_interval, maximum_interval);
        let wait = {
            let mut schedule = self.next_request_by_domain.lock().await;
            let now = Instant::now();
            let allowed = schedule.get(domain).copied().unwrap_or(now);
            let start = allowed.max(now);
            schedule.insert(domain.to_string(), start + delay);
            start.saturating_duration_since(now)
        };
        wait_or_cancel(wait, cancel).await?;
        Ok(wait)
    }

    async fn defer_domain(&self, domain: &str, delay: Duration) {
        let Some(deadline) = Instant::now().checked_add(delay) else {
            return;
        };
        let mut schedule = self.next_request_by_domain.lock().await;
        let next = schedule.entry(domain.to_string()).or_insert(deadline);
        *next = (*next).max(deadline);
    }

    async fn adaptive_request_interval(
        &self,
        domain: &str,
        purpose: RequestPurpose,
        minimum_interval: Duration,
        maximum_interval: Duration,
    ) -> (Duration, Duration) {
        if !is_deviantart_domain(domain) || !matches!(purpose, RequestPurpose::Metadata) {
            return (minimum_interval, maximum_interval);
        }
        let delay = self
            .adaptive_delay_by_domain
            .lock()
            .await
            .get(domain)
            .copied()
            .unwrap_or_default();
        (minimum_interval.max(delay), maximum_interval.max(delay))
    }

    async fn increase_adaptive_delay(&self, domain: &str) -> Duration {
        let mut delays = self.adaptive_delay_by_domain.lock().await;
        let delay = delays.entry(domain.to_string()).or_default();
        *delay = (*delay + Duration::from_secs(1)).min(Duration::from_secs(30));
        *delay
    }

    async fn note_successful_request(&self, domain: &str, purpose: RequestPurpose) {
        if !is_deviantart_domain(domain) || !matches!(purpose, RequestPurpose::Metadata) {
            return;
        }
        let mut delays = self.adaptive_delay_by_domain.lock().await;
        let Some(delay) = delays.get_mut(domain) else {
            return;
        };
        if *delay > Duration::from_secs(2) {
            *delay -= Duration::from_secs(1);
        }
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

fn trace_media_fallback(fallback: &MediaDescriptor) {
    #[cfg(debug_assertions)]
    tracing::debug!(
        target: "picto_sources::http",
        host = Url::parse(&fallback.url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_default(),
        "Original media was unavailable; using the provider fallback"
    );
}

fn rejects_final_url(descriptor: &MediaDescriptor, url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    descriptor
        .rejected_final_paths
        .iter()
        .any(|suffix| path.ends_with(&suffix.to_ascii_lowercase()))
}

impl RequestPurpose {
    #[cfg(debug_assertions)]
    fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Media => "media",
        }
    }
}

fn request_interval(purpose: RequestPurpose, policy: &DomainPolicy) -> (Duration, Duration) {
    match purpose {
        RequestPurpose::Metadata => (policy.minimum_interval, policy.maximum_interval),
        RequestPurpose::Media => (policy.media_minimum_interval, policy.media_maximum_interval),
    }
}

fn cookie_header_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|entry| {
        let (key, value) = entry.trim().split_once('=')?;
        (key == name).then(|| value.to_string())
    })
}

fn retry_count(_purpose: RequestPurpose, policy: &DomainPolicy) -> u32 {
    policy.retries
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

fn is_deviantart_domain(domain: &str) -> bool {
    domain == "deviantart.com" || domain.ends_with(".deviantart.com")
}

fn retry_attempt_limit(domain: &str, purpose: RequestPurpose, retries: u32) -> u32 {
    if is_deviantart_domain(domain) && matches!(purpose, RequestPurpose::Metadata) {
        u32::MAX
    } else {
        retries
    }
}

fn is_deviantart_request_block(body: &str) -> bool {
    body.to_ascii_lowercase().contains("request blocked")
}

fn access_error(purpose: RequestPurpose, domain: &str, status: reqwest::StatusCode) -> SourceError {
    access_error_with_provider_message(purpose, domain, status, None)
}

fn access_error_with_provider_message(
    purpose: RequestPurpose,
    domain: &str,
    status: reqwest::StatusCode,
    provider_message: Option<&str>,
) -> SourceError {
    let message = provider_message
        .map(|message| format!("{domain} returned {status}: {message}"))
        .unwrap_or_else(|| format!("{domain} returned {status}"));
    match purpose {
        RequestPurpose::Metadata => {
            SourceError::new(SourceErrorKind::Authentication, message, false)
        }
        RequestPurpose::Media => SourceError::new(
            SourceErrorKind::Download,
            format!("media host {message}"),
            false,
        ),
    }
}

fn provider_error_message(body: &str) -> Option<String> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = payload
        .get("errors")?
        .as_array()?
        .iter()
        .find_map(|error| error.get("message")?.as_str())?;
    let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
    (!message.is_empty()).then(|| message.chars().take(240).collect())
}

fn credential_headers(
    credentials: &RequestCredentials,
    domain: &str,
    request_headers: &HeaderMap,
) -> Result<HeaderMap, SourceError> {
    let permitted = credentials.permits(domain);
    let mut headers = if permitted {
        parse_headers(&credentials.headers)?
    } else {
        let mut public = HeaderMap::new();
        if let Some((_, value)) = credentials
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(USER_AGENT.as_str()))
        {
            public.insert(
                USER_AGENT,
                HeaderValue::from_str(value).map_err(invalid_header)?,
            );
        }
        public
    };
    headers.extend(request_headers.clone());
    if permitted && !credentials.cookies.is_empty() {
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
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(DEFAULT_BROWSER_USER_AGENT),
        );
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

fn html_media_error(body: &str) -> SourceError {
    if let Some(error) = terminal_html_media_error(body) {
        return error;
    }
    if body.contains("requires gp") {
        return SourceError::new(
            SourceErrorKind::AccessDenied,
            "The original image requires GP and no display-image fallback was available",
            false,
        );
    }
    SourceError::new(
        SourceErrorKind::InvalidResponse,
        "The media host returned a web page instead of a file",
        true,
    )
}

fn terminal_html_media_error(body: &str) -> Option<SourceError> {
    if body.contains("temporarily banned") {
        return Some(SourceError::new(
            SourceErrorKind::RateLimited,
            "The media host temporarily blocked downloads; retry later",
            true,
        ));
    }
    if body.contains("exceeded your image viewing limit") || body.contains("image limit exceeded") {
        return Some(SourceError::new(
            SourceErrorKind::RateLimited,
            "The image viewing limit was reached; retry after it resets",
            true,
        ));
    }
    None
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
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    retry_after_from(headers, now)
}

fn retry_after_from(headers: &HeaderMap, now: u64) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    absolute_reset(value, now)
}

fn rate_limit_reset(headers: &HeaderMap) -> Option<Duration> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    rate_limit_reset_from(headers, now)
}

fn rate_limit_reset_from(headers: &HeaderMap, now: u64) -> Option<Duration> {
    let value = headers
        .get("x-ratelimit-reset")
        .or_else(|| headers.get("x-rate-limit-reset"))?
        .to_str()
        .ok()?;
    absolute_reset(value, now)
}

fn proactive_rate_limit_delay(headers: &HeaderMap) -> Option<Duration> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    proactive_rate_limit_delay_from(headers, now)
}

fn proactive_rate_limit_delay_from(headers: &HeaderMap, now: u64) -> Option<Duration> {
    let remaining = headers
        .get("x-ratelimit-remaining")
        .or_else(|| headers.get("x-rate-limit-remaining"))?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()?;
    (remaining <= 1)
        .then(|| rate_limit_reset_from(headers, now))
        .flatten()
}

fn absolute_reset(value: &str, now: u64) -> Option<Duration> {
    let reset = value.parse::<u64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value)
            .or_else(|_| chrono::DateTime::parse_from_rfc2822(value))
            .ok()
            .and_then(|value| u64::try_from(value.timestamp()).ok())
    })?;
    Some(Duration::from_secs(reset.saturating_sub(now)))
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
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn default_policy_never_repeats_a_domain_below_one_second() {
        let policy = HttpPolicy::default();
        assert!(policy.minimum_interval >= Duration::from_secs(1));
        assert!(policy.maximum_interval >= policy.minimum_interval);
    }

    #[test]
    fn validates_domain_overrides_independently_of_global_concurrency() {
        let mut domains = BTreeMap::new();
        domains.insert(
            "api.example.test".into(),
            DomainPolicy {
                minimum_interval: Duration::from_secs(2),
                maximum_interval: Duration::from_secs(1),
                media_minimum_interval: Duration::ZERO,
                media_maximum_interval: Duration::ZERO,
                request_timeout: Duration::from_secs(30),
                retries: 1,
            },
        );
        assert!(HttpRuntime::with_domain_policies(HttpPolicy::default(), domains).is_err());
    }

    #[test]
    fn metadata_and_media_share_the_configured_retry_budget() {
        let policy = DomainPolicy {
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            media_minimum_interval: Duration::ZERO,
            media_maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(45),
            retries: 3,
        };
        assert_eq!(retry_count(RequestPurpose::Metadata, &policy), 3);
        assert_eq!(retry_count(RequestPurpose::Media, &policy), 3);
    }

    #[test]
    fn provider_error_messages_extract_only_bounded_structured_text() {
        assert_eq!(
            provider_error_message(
                r#"{"errors":[{"code":32,"message":"Could not  authenticate\n you"}]}"#
            )
            .as_deref(),
            Some("Could not authenticate you")
        );
        assert_eq!(provider_error_message("<html>denied</html>"), None);
        assert_eq!(
            provider_error_message(r#"{"token":"secret","errors":[]}"#),
            None
        );
    }

    #[test]
    fn parent_domain_policy_applies_to_provider_cdn_subdomains() {
        let override_policy = DomainPolicy {
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            media_minimum_interval: Duration::ZERO,
            media_maximum_interval: Duration::ZERO,
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
    fn metadata_and_media_use_independent_domain_intervals() {
        let policy = DomainPolicy {
            minimum_interval: Duration::from_millis(500),
            maximum_interval: Duration::from_millis(500),
            media_minimum_interval: Duration::ZERO,
            media_maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(45),
            retries: 3,
        };
        assert_eq!(
            request_interval(RequestPurpose::Metadata, &policy),
            (Duration::from_millis(500), Duration::from_millis(500))
        );
        assert_eq!(
            request_interval(RequestPurpose::Media, &policy),
            (Duration::ZERO, Duration::ZERO)
        );
    }

    #[test]
    fn absolute_rate_limit_resets_become_relative_waits() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-reset", HeaderValue::from_static("1060"));
        assert_eq!(
            rate_limit_reset_from(&headers, 1000),
            Some(Duration::from_secs(60))
        );

        headers.remove("x-ratelimit-reset");
        headers.insert("x-rate-limit-reset", HeaderValue::from_static("1060"));
        assert_eq!(
            rate_limit_reset_from(&headers, 1000),
            Some(Duration::from_secs(60))
        );
        assert_eq!(rate_limit_reset_from(&headers, 1100), Some(Duration::ZERO));

        headers.insert(
            "x-ratelimit-reset",
            HeaderValue::from_static("1970-01-01T00:17:40Z"),
        );
        assert_eq!(
            rate_limit_reset_from(&headers, 1000),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("60"));
        assert_eq!(
            retry_after_from(&headers, 1_000),
            Some(Duration::from_secs(60))
        );

        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Thu, 01 Jan 1970 00:17:40 +0000"),
        );
        assert_eq!(
            retry_after_from(&headers, 1_000),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn successful_last_quota_response_defers_the_next_request() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("1"));
        headers.insert("x-ratelimit-reset", HeaderValue::from_static("1060"));
        assert_eq!(
            proactive_rate_limit_delay_from(&headers, 1_000),
            Some(Duration::from_secs(60))
        );

        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("2"));
        assert_eq!(proactive_rate_limit_delay_from(&headers, 1_000), None);

        headers.remove("x-ratelimit-remaining");
        headers.remove("x-ratelimit-reset");
        headers.insert("x-rate-limit-remaining", HeaderValue::from_static("1"));
        headers.insert("x-rate-limit-reset", HeaderValue::from_static("1060"));
        assert_eq!(
            proactive_rate_limit_delay_from(&headers, 1_000),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn quota_pages_are_terminal_before_media_fallbacks() {
        let error = terminal_html_media_error("you have exceeded your image viewing limit")
            .expect("quota page should be classified");
        assert_eq!(error.kind, SourceErrorKind::RateLimited);
        assert!(error.retryable);
    }

    #[test]
    fn only_deviantart_block_pages_use_the_long_cooldown() {
        assert!(is_deviantart_domain("www.deviantart.com"));
        assert!(!is_deviantart_domain("notdeviantart.com"));
        assert!(is_deviantart_request_block(
            "<html><title>Request blocked.</title></html>"
        ));
        assert!(!is_deviantart_request_block("forbidden"));
    }

    #[test]
    fn deviantart_metadata_rate_limits_keep_retrying_until_cancelled_or_successful() {
        assert_eq!(
            retry_attempt_limit("www.deviantart.com", RequestPurpose::Metadata, 4),
            u32::MAX
        );
        assert_eq!(
            retry_attempt_limit("www.deviantart.com", RequestPurpose::Media, 4),
            4
        );
        assert_eq!(
            retry_attempt_limit("example.com", RequestPurpose::Metadata, 4),
            4
        );
    }

    #[tokio::test]
    async fn deviantart_rate_limit_delay_grows_and_recovers_like_the_reference_client() {
        let runtime = HttpRuntime::new(HttpPolicy::default()).unwrap();
        let zero = Duration::ZERO;

        assert_eq!(
            runtime
                .adaptive_request_interval(
                    "www.deviantart.com",
                    RequestPurpose::Metadata,
                    zero,
                    zero,
                )
                .await,
            (zero, zero)
        );
        assert_eq!(
            runtime.increase_adaptive_delay("www.deviantart.com").await,
            Duration::from_secs(1)
        );
        runtime
            .note_successful_request("www.deviantart.com", RequestPurpose::Metadata)
            .await;
        assert_eq!(
            runtime
                .adaptive_request_interval(
                    "www.deviantart.com",
                    RequestPurpose::Metadata,
                    zero,
                    zero,
                )
                .await,
            (Duration::from_secs(1), Duration::from_secs(1))
        );

        runtime.increase_adaptive_delay("www.deviantart.com").await;
        runtime.increase_adaptive_delay("www.deviantart.com").await;
        runtime
            .note_successful_request("www.deviantart.com", RequestPurpose::Metadata)
            .await;
        assert_eq!(
            runtime
                .adaptive_request_interval(
                    "www.deviantart.com",
                    RequestPurpose::Metadata,
                    zero,
                    zero,
                )
                .await,
            (Duration::from_secs(2), Duration::from_secs(2))
        );
        assert_eq!(
            runtime
                .adaptive_request_interval("www.deviantart.com", RequestPurpose::Media, zero, zero,)
                .await,
            (zero, zero)
        );
    }

    #[test]
    fn provider_placeholder_redirects_are_rejected_declaratively() {
        let descriptor = crate::MediaDescriptorBuilder::new("signed", 0, "https://media.test/a")
            .reject_final_path("/expired.png")
            .build();
        assert!(rejects_final_url(
            &descriptor,
            &Url::parse("https://media.test/expired.png?token=old").unwrap()
        ));
        assert!(!rejects_final_url(
            &descriptor,
            &Url::parse("https://media.test/original.png").unwrap()
        ));
    }

    #[test]
    fn credentials_are_only_attached_to_allowed_first_party_domains() {
        let credentials = RequestCredentials {
            headers: [
                ("authorization".into(), "secret".into()),
                ("User-Agent".into(), "provider-agent".into()),
            ]
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
            Some("provider-agent")
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

    #[tokio::test]
    async fn provider_fallback_retries_original_then_uses_display_without_deadlocking() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let mut paths = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_string();
                paths.push(path.clone());
                let (content_type, body) = if path == "/original" {
                    ("text/html", b"Keep trying".as_slice())
                } else if path.starts_with("/original?nl=") {
                    (
                        "text/html",
                        b"Downloading original files requires GP".as_slice(),
                    )
                } else {
                    ("image/jpeg", b"fallback-image".as_slice())
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
            paths
        });
        let runtime = HttpRuntime::new(HttpPolicy {
            maximum_concurrency: 1,
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(2),
            retries: 0,
        })
        .unwrap();
        let descriptor = MediaDescriptor {
            stable_id: "gallery:1".into(),
            position: 0,
            url: format!("http://{address}/original"),
            canonical_url: None,
            file_name: Some("original.jpg".into()),
            mime_hint: Some("image/jpeg".into()),
            expected_size: None,
            headers: BTreeMap::new(),
            fallbacks: vec![
                crate::MediaFallback {
                    url: format!("http://{address}/display"),
                    file_name: Some("display.jpg".into()),
                    mime_hint: Some("image/jpeg".into()),
                    expected_size: None,
                    html_marker: Some("requires gp".into()),
                },
                crate::MediaFallback {
                    url: format!("http://{address}/original?nl=retry-token"),
                    file_name: Some("original.jpg".into()),
                    mime_hint: Some("image/jpeg".into()),
                    expected_size: None,
                    html_marker: None,
                },
            ],
            rejected_final_paths: Vec::new(),
            postprocess: None,
        };
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("media.jpg");
        let downloaded = runtime
            .download(
                &descriptor,
                &RequestCredentials::default(),
                &destination,
                &CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(&destination).await.unwrap(),
            b"fallback-image"
        );
        assert!(downloaded.descriptor.url.ends_with("/display"));
        assert_eq!(
            server.join().unwrap(),
            vec!["/original", "/original?nl=retry-token", "/display"]
        );
    }

    #[tokio::test]
    async fn exhausted_original_server_error_uses_declared_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let mut paths = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_string();
                paths.push(path.clone());
                let (status, body) = if path == "/original" {
                    ("503 Service Unavailable", b"unavailable".as_slice())
                } else {
                    ("200 OK", b"full-fallback".as_slice())
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
            paths
        });
        let runtime = HttpRuntime::new(HttpPolicy {
            maximum_concurrency: 1,
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(2),
            retries: 0,
        })
        .unwrap();
        let descriptor =
            crate::MediaDescriptorBuilder::new("post:1", 0, format!("http://{address}/original"))
                .fallback(crate::MediaFallback {
                    url: format!("http://{address}/fallback"),
                    file_name: Some("fallback.jpg".into()),
                    mime_hint: Some("image/jpeg".into()),
                    expected_size: None,
                    html_marker: None,
                })
                .build();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("media.jpg");

        runtime
            .download(
                &descriptor,
                &RequestCredentials::default(),
                &destination,
                &CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(&destination).await.unwrap(),
            b"full-fallback"
        );
        assert_eq!(server.join().unwrap(), vec!["/original", "/fallback"]);
    }

    #[tokio::test]
    async fn browser_emulated_json_uses_the_shared_retry_path() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).unwrap();
                let (status, body) = if attempt == 0 {
                    ("503 Service Unavailable", b"retry".as_slice())
                } else {
                    ("200 OK", br#"{"body":{"id":"post"}}"#.as_slice())
                };
                let cookie = if attempt == 1 {
                    "Set-Cookie: ct0=rotated; Path=/; HttpOnly\r\n"
                } else {
                    ""
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let runtime = HttpRuntime::new(HttpPolicy {
            maximum_concurrency: 1,
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(2),
            retries: 1,
        })
        .unwrap();

        let url = Url::parse(&format!("http://{address}/post")).unwrap();
        let value = runtime
            .get_browser_optional_json::<serde_json::Value>(
                url.clone(),
                &RequestCredentials::default(),
                &CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            value.pointer("/body/id").and_then(|value| value.as_str()),
            Some("post")
        );
        assert_eq!(
            runtime.cookie_value(&url, "ct0").as_deref(),
            Some("rotated")
        );
        server.join().unwrap();
    }
}
