//! Domain-aware rate limiter for network requests.
//!
//! Provides polite request throttling per domain. Each domain gets a minimum
//! interval between requests (default 1s). Domains like Pixiv get a stricter limit.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Thread-safe, domain-aware rate limiter.
///
/// Tracks the last request time per domain and sleeps to enforce minimum intervals.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
}

struct RateLimiterInner {
    intervals: HashMap<String, Duration>,
    last_request: HashMap<String, Instant>,
    default_interval: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter with sensible defaults.
    pub fn new() -> Self {
        let mut intervals = HashMap::new();
        // Pixiv is stricter
        intervals.insert("www.pixiv.net".to_string(), Duration::from_millis(2000));

        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                intervals,
                last_request: HashMap::new(),
                default_interval: Duration::from_millis(1000), // 1 req/sec default
            })),
        }
    }

    /// Wait until it's safe to make a request to the given domain.
    pub async fn wait_for_slot(&self, domain: &str) {
        let delay = {
            let mut inner = crate::poison::mutex_or_recover(&self.inner, "rate_limiter");
            let interval = inner
                .intervals
                .get(domain)
                .copied()
                .unwrap_or(inner.default_interval);

            let now = Instant::now();
            if let Some(last) = inner.last_request.get(domain) {
                let elapsed = now.duration_since(*last);
                if elapsed < interval {
                    Some(interval - elapsed)
                } else {
                    inner.last_request.insert(domain.to_string(), now);
                    None
                }
            } else {
                inner.last_request.insert(domain.to_string(), now);
                None
            }
        };

        if let Some(d) = delay {
            tokio::time::sleep(d).await;
            let mut inner = crate::poison::mutex_or_recover(&self.inner, "rate_limiter");
            inner
                .last_request
                .insert(domain.to_string(), Instant::now());
        }
    }
}

pub struct RateLimitGuard;

impl RateLimitGuard {
    pub async fn acquire(limiter: &RateLimiter, domain: &str) -> Self {
        limiter.wait_for_slot(domain).await;
        Self
    }
}
