//! Domain-aware rate limiter for network requests.
//!
//! Provides polite request throttling per domain. Each domain gets a one-second
//! minimum interval and only one subscription run at a time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Thread-safe, domain-aware rate limiter.
///
/// Tracks the last request time per domain and sleeps to enforce minimum intervals.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
    run_locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

struct RateLimiterInner {
    last_request: HashMap<String, Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter with sensible defaults.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                last_request: HashMap::new(),
            })),
            run_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Wait until it's safe to make a request to the given domain.
    pub async fn wait_for_slot(&self, domain: &str) {
        let delay = {
            let mut inner = crate::poison::mutex_or_recover(&self.inner, "rate_limiter");
            let interval = Duration::from_secs(1);

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

    pub async fn acquire_domain_run(&self, domain: &str) -> DomainRunGuard {
        let domain_lock = {
            let mut locks = self.run_locks.lock().await;
            locks
                .entry(domain.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        DomainRunGuard {
            _guard: domain_lock.lock_owned().await,
        }
    }
}

pub struct DomainRunGuard {
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_domain_runs_are_serial() {
        let limiter = RateLimiter::new();
        let first = limiter.acquire_domain_run("example.com").await;
        let waiting_limiter = limiter.clone();
        let second = tokio::spawn(async move {
            let _guard = waiting_limiter.acquire_domain_run("example.com").await;
        });

        tokio::task::yield_now().await;
        assert!(!second.is_finished());

        drop(first);
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("second run should start after the first finishes")
            .expect("second run task should complete");
    }
}
