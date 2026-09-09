//! One bounded, short-lived metadata page; never a post or media prefetch queue.
use std::{
    future::Future,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use crate::{RequestCredentials, SourceError};

pub(crate) struct PageCache<T> {
    page: Mutex<Option<CachedPage<T>>>,
}

struct CachedPage<T> {
    scope: String,
    credentials: RequestCredentials,
    loaded_at: Instant,
    value: T,
}

impl<T> Default for PageCache<T> {
    fn default() -> Self {
        Self {
            page: Mutex::new(None),
        }
    }
}

impl<T: Clone> PageCache<T> {
    pub(crate) async fn clear(&self) {
        *self.page.lock().await = None;
    }

    pub(crate) async fn get_or_fetch(
        &self,
        scope: &str,
        credentials: &RequestCredentials,
        reuse: bool,
        fetch: impl Future<Output = Result<T, SourceError>>,
    ) -> Result<T, SourceError> {
        let mut page = self.page.lock().await;
        if reuse {
            if let Some(cached) = page.as_ref().filter(|page| {
                page.scope == scope
                    && page.loaded_at.elapsed() < Duration::from_secs(300)
                    && same_credentials(&page.credentials, credentials)
            }) {
                return Ok(cached.value.clone());
            }
        }
        *page = None;
        let value = fetch.await?;
        *page = Some(CachedPage {
            scope: scope.to_owned(),
            credentials: credentials.clone(),
            loaded_at: Instant::now(),
            value: value.clone(),
        });
        Ok(value)
    }
}

fn same_credentials(a: &RequestCredentials, b: &RequestCredentials) -> bool {
    a.headers == b.headers
        && a.cookies == b.cookies
        && a.username == b.username
        && a.api_key == b.api_key
        && a.oauth_token == b.oauth_token
        && a.oauth_token_secret == b.oauth_token_secret
        && a.allowed_domains == b.allowed_domains
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn reuses_only_the_current_page_for_the_same_login_and_traversal() {
        let cache = PageCache::default();
        let mut credentials = RequestCredentials::default();
        let requests = AtomicUsize::new(0);
        for (scope, reuse, expected) in [
            ("page1", false, 1),
            ("page1", true, 1),
            ("page2", true, 2),
            ("page1", true, 3),
            ("page1", false, 4),
        ] {
            let value = cache
                .get_or_fetch(scope, &credentials, reuse, async {
                    Ok(requests.fetch_add(1, Ordering::SeqCst) + 1)
                })
                .await
                .unwrap();
            assert_eq!(value, expected);
        }
        credentials
            .cookies
            .insert("session".into(), "different-login".into());
        let value = cache
            .get_or_fetch("page1", &credentials, true, async { Ok(5) })
            .await
            .unwrap();
        assert_eq!(value, 5);
    }

    #[tokio::test]
    async fn failed_fetch_does_not_leave_a_stale_page() {
        let cache = PageCache::default();
        let credentials = RequestCredentials::default();
        cache
            .get_or_fetch("page", &credentials, false, async { Ok(1) })
            .await
            .unwrap();
        assert!(cache
            .get_or_fetch("page", &credentials, false, async {
                Err(SourceError::new(
                    crate::SourceErrorKind::Network,
                    "offline",
                    true,
                ))
            })
            .await
            .is_err());
        assert_eq!(
            cache
                .get_or_fetch("page", &credentials, true, async { Ok(2) })
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn expired_pages_are_refetched() {
        let cache = PageCache::default();
        let credentials = RequestCredentials::default();
        cache
            .get_or_fetch("page", &credentials, false, async { Ok(1) })
            .await
            .unwrap();
        cache.page.lock().await.as_mut().unwrap().loaded_at =
            Instant::now() - Duration::from_secs(301);
        assert_eq!(
            cache
                .get_or_fetch("page", &credentials, true, async { Ok(2) })
                .await
                .unwrap(),
            2
        );
    }
}
