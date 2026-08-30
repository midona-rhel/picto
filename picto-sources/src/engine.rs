use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::{
    DiscoveryRequest, HttpRuntime, NativeSourceAdapter, RequestCredentials, SourceError,
    SourceErrorKind, SourcePost, SourcePostOutcome,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextPost {
    Post(Box<SourcePost>),
    SourceExhausted,
    PostBudgetReached,
}

/// Pull-based provider session. Calling `next_post` twice without settling the
/// first post is rejected, so adapters cannot advance visible traversal early.
pub struct SourceSession {
    adapter: Arc<dyn NativeSourceAdapter>,
    credentials: RequestCredentials,
    request: DiscoveryRequest,
    active: Option<SourcePost>,
    source_exhausted: bool,
    failed: bool,
    added_count: u32,
    post_limit: u32,
}

/// Runs a provider's partitions in descriptor order under one added-post
/// budget. Each partition keeps an independent durable cursor.
pub struct PartitionedSourceSession {
    adapter: Arc<dyn NativeSourceAdapter>,
    credentials: RequestCredentials,
    query: String,
    page_size: u32,
    partitions: VecDeque<crate::SourcePartition>,
    cursors: BTreeMap<crate::SourcePartition, Option<String>>,
    current: Option<SourceSession>,
    added_count: u32,
    post_limit: u32,
}

impl PartitionedSourceSession {
    pub fn new(
        adapter: Arc<dyn NativeSourceAdapter>,
        credentials: RequestCredentials,
        query: impl Into<String>,
        cursors: BTreeMap<crate::SourcePartition, Option<String>>,
        page_size: u32,
        post_limit: u32,
    ) -> Result<Self, SourceError> {
        let query = query.into();
        adapter.validate_query(&query)?;
        if page_size == 0 || post_limit == 0 {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                "page size and post limit must be greater than zero",
                false,
            ));
        }
        Ok(Self {
            partitions: adapter.partition_order().into(),
            adapter,
            credentials,
            query,
            page_size,
            cursors,
            current: None,
            added_count: 0,
            post_limit,
        })
    }

    pub async fn next_post(
        &mut self,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<NextPost, SourceError> {
        if self.added_count >= self.post_limit {
            return Ok(NextPost::PostBudgetReached);
        }
        loop {
            if self.current.is_none() {
                let Some(partition) = self.partitions.pop_front() else {
                    return Ok(NextPost::SourceExhausted);
                };
                let cursor = self.cursors.get(&partition).cloned().flatten();
                self.current = Some(SourceSession::new(
                    Arc::clone(&self.adapter),
                    self.credentials.clone(),
                    DiscoveryRequest {
                        query: self.query.clone(),
                        partition,
                        cursor,
                        page_size: self.page_size,
                    },
                    self.post_limit - self.added_count,
                )?);
            }
            let current = self.current.as_mut().expect("partition session exists");
            match current.next_post(http, cancel).await? {
                NextPost::SourceExhausted => {
                    let partition = current.request.partition.clone();
                    self.cursors
                        .insert(partition, current.cursor().map(ToOwned::to_owned));
                    self.current = None;
                }
                value => return Ok(value),
            }
        }
    }

    pub fn settle(
        &mut self,
        stable_post_id: &str,
        outcome: SourcePostOutcome,
    ) -> Result<(), SourceError> {
        let current = self.current.as_mut().ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::InvalidResponse,
                "no active source partition",
                false,
            )
        })?;
        let consumed = outcome.counts_as_added_post();
        current.settle(stable_post_id, outcome)?;
        if consumed {
            self.added_count = self.added_count.saturating_add(1);
        }
        self.cursors.insert(
            current.request.partition.clone(),
            current.cursor().map(ToOwned::to_owned),
        );
        Ok(())
    }

    /// Stop refreshing the active partition after its current post settled.
    /// The next pull advances to the following provider-defined partition.
    pub fn finish_current_partition(&mut self) -> Result<(), SourceError> {
        let current = self.current.take().ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::InvalidResponse,
                "no active source partition",
                false,
            )
        })?;
        if current.active.is_some() {
            self.current = Some(current);
            return Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "source partition still has an unsettled post",
                false,
            ));
        }
        self.cursors.insert(
            current.request.partition.clone(),
            current.cursor().map(ToOwned::to_owned),
        );
        Ok(())
    }

    pub fn cursors(&self) -> &BTreeMap<crate::SourcePartition, Option<String>> {
        &self.cursors
    }

    pub fn added_count(&self) -> u32 {
        self.added_count
    }
}

impl SourceSession {
    pub fn new(
        adapter: Arc<dyn NativeSourceAdapter>,
        credentials: RequestCredentials,
        request: DiscoveryRequest,
        post_limit: u32,
    ) -> Result<Self, SourceError> {
        if post_limit == 0 {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                "post limit must be greater than zero",
                false,
            ));
        }
        adapter.validate_query(&request.query)?;
        Ok(Self {
            adapter,
            credentials,
            request,
            active: None,
            source_exhausted: false,
            failed: false,
            added_count: 0,
            post_limit,
        })
    }

    pub async fn next_post(
        &mut self,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<NextPost, SourceError> {
        if self.active.is_some() {
            return Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "settle the active post before requesting another",
                false,
            ));
        }
        if self.failed {
            return Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "source session has failed",
                false,
            ));
        }
        if self.added_count >= self.post_limit {
            return Ok(NextPost::PostBudgetReached);
        }

        loop {
            if self.source_exhausted {
                return Ok(NextPost::SourceExhausted);
            }

            let mut batch = self
                .adapter
                .discover(&self.request, &self.credentials, http, cancel)
                .await?;
            if batch.posts.len() > 1 {
                return Err(SourceError::new(
                    SourceErrorKind::InvalidResponse,
                    "source returned more than one post for a serial discovery request",
                    false,
                ));
            }
            if batch.posts.is_empty() && !batch.exhausted {
                return Err(SourceError::new(
                    SourceErrorKind::InvalidResponse,
                    "source returned an empty non-terminal page",
                    true,
                ));
            }
            self.source_exhausted = batch.exhausted;
            let Some(post) = batch.posts.pop() else {
                continue;
            };
            let post = self
                .adapter
                .resolve_post(post, &self.credentials, http, cancel)
                .await?;
            self.active = Some(post.clone());
            return Ok(NextPost::Post(Box::new(post)));
        }
    }

    pub fn settle(
        &mut self,
        stable_post_id: &str,
        outcome: SourcePostOutcome,
    ) -> Result<(), SourceError> {
        let active = self.active.take().ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::InvalidResponse,
                "no active source post to settle",
                false,
            )
        })?;
        if active.stable_id != stable_post_id {
            self.active = Some(active);
            return Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "source post settlement identity mismatch",
                false,
            ));
        }

        if outcome.counts_as_added_post() {
            self.added_count = self.added_count.saturating_add(1);
        }
        if matches!(outcome, SourcePostOutcome::Failed { .. }) {
            self.failed = true;
        }
        if matches!(
            outcome,
            SourcePostOutcome::Added { .. } | SourcePostOutcome::Skipped { .. }
        ) {
            self.request.cursor = active.resume_cursor_after;
        }
        Ok(())
    }

    pub fn added_count(&self) -> u32 {
        self.added_count
    }

    pub fn cursor(&self) -> Option<&str> {
        self.request.cursor.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use crate::{AdapterFuture, DiscoveryBatch, HttpPolicy, ProviderDescriptor, SourcePartition};

    use super::*;

    struct FixtureSource {
        discoveries: AtomicUsize,
        resolutions: AtomicUsize,
        exhausted: bool,
    }

    struct PartitionFixtureSource {
        exhausted: bool,
    }
    struct MultiPostSource;

    impl NativeSourceAdapter for MultiPostSource {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "multi-post-fixture",
                display_name: "Multi-post fixture",
                domain: "example.test",
                partitions: &["posts"],
                anonymous: true,
            }
        }

        fn validate_query(&self, _query: &str) -> Result<(), SourceError> {
            Ok(())
        }

        fn discover<'a>(
            &'a self,
            request: &'a DiscoveryRequest,
            _credentials: &'a RequestCredentials,
            _http: &'a HttpRuntime,
            _cancel: &'a CancellationToken,
        ) -> AdapterFuture<'a> {
            Box::pin(async move {
                Ok(DiscoveryBatch {
                    posts: ["1", "2"]
                        .into_iter()
                        .map(|id| SourcePost {
                            site_id: "multi-post-fixture".into(),
                            partition: request.partition.clone(),
                            stable_id: id.into(),
                            canonical_url: None,
                            creator: None,
                            name: None,
                            notes: None,
                            created_at: None,
                            tags: vec![],
                            media: vec![],
                            resume_cursor_after: Some(id.into()),
                        })
                        .collect(),
                    exhausted: true,
                })
            })
        }
    }

    impl NativeSourceAdapter for PartitionFixtureSource {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "partition-fixture",
                display_name: "Partition fixture",
                domain: "example.test",
                partitions: &["purchases", "messages", "feed"],
                anonymous: false,
            }
        }

        fn validate_query(&self, _query: &str) -> Result<(), SourceError> {
            Ok(())
        }

        fn discover<'a>(
            &'a self,
            request: &'a DiscoveryRequest,
            _credentials: &'a RequestCredentials,
            _http: &'a HttpRuntime,
            _cancel: &'a CancellationToken,
        ) -> AdapterFuture<'a> {
            Box::pin(async move {
                let id = request.partition.0.clone();
                Ok(DiscoveryBatch {
                    posts: vec![SourcePost {
                        site_id: "partition-fixture".into(),
                        partition: request.partition.clone(),
                        stable_id: id.clone(),
                        canonical_url: None,
                        creator: None,
                        name: None,
                        notes: None,
                        created_at: None,
                        tags: vec![],
                        media: vec![],
                        resume_cursor_after: Some(format!("after-{id}")),
                    }],
                    exhausted: self.exhausted,
                })
            })
        }
    }

    impl NativeSourceAdapter for FixtureSource {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "fixture",
                display_name: "Fixture",
                domain: "example.test",
                partitions: &["posts"],
                anonymous: true,
            }
        }

        fn validate_query(&self, _query: &str) -> Result<(), SourceError> {
            Ok(())
        }

        fn discover<'a>(
            &'a self,
            request: &'a DiscoveryRequest,
            _credentials: &'a RequestCredentials,
            _http: &'a HttpRuntime,
            _cancel: &'a CancellationToken,
        ) -> AdapterFuture<'a> {
            Box::pin(async move {
                self.discoveries.fetch_add(1, Ordering::SeqCst);
                let start = request
                    .cursor
                    .as_deref()
                    .unwrap_or("0")
                    .parse::<u32>()
                    .unwrap();
                let id = start + 1;
                let posts = vec![SourcePost {
                    site_id: "fixture".into(),
                    partition: SourcePartition::new("posts"),
                    stable_id: id.to_string(),
                    canonical_url: None,
                    creator: None,
                    name: None,
                    notes: None,
                    created_at: None,
                    tags: vec![],
                    media: vec![],
                    resume_cursor_after: Some(id.to_string()),
                }];
                Ok(DiscoveryBatch {
                    posts,
                    exhausted: self.exhausted,
                })
            })
        }

        fn resolve_post<'a>(
            &'a self,
            post: SourcePost,
            _credentials: &'a RequestCredentials,
            _http: &'a HttpRuntime,
            _cancel: &'a CancellationToken,
        ) -> crate::PostFuture<'a> {
            Box::pin(async move {
                self.resolutions.fetch_add(1, Ordering::SeqCst);
                Ok(post)
            })
        }
    }

    fn runtime() -> HttpRuntime {
        HttpRuntime::new(HttpPolicy {
            maximum_concurrency: 1,
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(1),
            retries: 0,
        })
        .unwrap()
    }

    fn session(source: Arc<FixtureSource>, post_limit: u32) -> SourceSession {
        SourceSession::new(
            source,
            RequestCredentials::default(),
            DiscoveryRequest {
                query: "fixture".into(),
                partition: SourcePartition::new("posts"),
                cursor: None,
                page_size: 2,
            },
            post_limit,
        )
        .unwrap()
    }

    async fn post(session: &mut SourceSession, runtime: &HttpRuntime) -> SourcePost {
        match session
            .next_post(runtime, &CancellationToken::new())
            .await
            .unwrap()
        {
            NextPost::Post(post) => *post,
            other => panic!("expected post, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn requires_settlement_before_exposing_the_next_post() {
        let source = Arc::new(FixtureSource {
            discoveries: AtomicUsize::new(0),
            resolutions: AtomicUsize::new(0),
            exhausted: false,
        });
        let runtime = runtime();
        let mut session = session(source.clone(), 2);

        let first = post(&mut session, &runtime).await;
        assert_eq!(first.stable_id, "1");
        assert!(session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .is_err());
        assert_eq!(source.discoveries.load(Ordering::SeqCst), 1);
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 1);

        session
            .settle(
                &first.stable_id,
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::ExactDuplicate,
                },
            )
            .unwrap();
        let second = post(&mut session, &runtime).await;
        assert_eq!(second.stable_id, "2");
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rejects_provider_batches_that_expose_more_than_one_post() {
        let mut session = SourceSession::new(
            Arc::new(MultiPostSource),
            RequestCredentials::default(),
            DiscoveryRequest {
                query: "fixture".into(),
                partition: SourcePartition::new("posts"),
                cursor: None,
                page_size: 1,
            },
            2,
        )
        .unwrap();

        let error = session
            .next_post(&runtime(), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, SourceErrorKind::InvalidResponse);
        assert!(error.message.contains("more than one post"));
    }

    #[tokio::test]
    async fn skipped_posts_advance_the_cursor_without_consuming_the_added_budget() {
        let source = Arc::new(FixtureSource {
            discoveries: AtomicUsize::new(0),
            resolutions: AtomicUsize::new(0),
            exhausted: false,
        });
        let runtime = runtime();
        let mut session = session(source.clone(), 1);

        let skipped = post(&mut session, &runtime).await;
        session
            .settle(
                &skipped.stable_id,
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::NoUsableMedia,
                },
            )
            .unwrap();
        assert_eq!(session.added_count(), 0);
        assert_eq!(session.cursor(), Some("1"));
        let added = post(&mut session, &runtime).await;
        assert_eq!(added.stable_id, "2");
        session
            .settle(
                &added.stable_id,
                SourcePostOutcome::Added { root_ids: vec![2] },
            )
            .unwrap();
        assert_eq!(session.added_count(), 1);
        assert_eq!(session.cursor(), Some("2"));
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn added_posts_consume_the_run_budget() {
        let source = Arc::new(FixtureSource {
            discoveries: AtomicUsize::new(0),
            resolutions: AtomicUsize::new(0),
            exhausted: false,
        });
        let runtime = runtime();
        let mut session = session(source.clone(), 1);

        let added = post(&mut session, &runtime).await;
        session
            .settle(
                &added.stable_id,
                SourcePostOutcome::Added { root_ids: vec![7] },
            )
            .unwrap();

        assert_eq!(session.added_count(), 1);
        assert_eq!(session.cursor(), Some("1"));
        assert_eq!(
            session
                .next_post(&runtime, &CancellationToken::new())
                .await
                .unwrap(),
            NextPost::PostBudgetReached,
        );
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mixed_outcomes_stop_after_twenty_added_posts() {
        let source = Arc::new(FixtureSource {
            discoveries: AtomicUsize::new(0),
            resolutions: AtomicUsize::new(0),
            exhausted: false,
        });
        let runtime = runtime();
        let mut session = session(source.clone(), 20);

        for expected in 1..=25 {
            let current = post(&mut session, &runtime).await;
            assert_eq!(current.stable_id, expected.to_string());
            let outcome = if expected <= 5 {
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::ExactDuplicate,
                }
            } else {
                SourcePostOutcome::Added {
                    root_ids: vec![expected],
                }
            };
            session.settle(&current.stable_id, outcome).unwrap();
            assert_eq!(session.cursor(), Some(expected.to_string().as_str()));
        }

        assert_eq!(session.added_count(), 20);
        assert_eq!(
            session
                .next_post(&runtime, &CancellationToken::new())
                .await
                .unwrap(),
            NextPost::PostBudgetReached,
        );
        assert_eq!(source.discoveries.load(Ordering::SeqCst), 25);
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 25);
        assert_eq!(session.cursor(), Some("25"));
    }

    #[tokio::test]
    async fn wrong_settlement_identity_keeps_the_active_post() {
        let source = Arc::new(FixtureSource {
            discoveries: AtomicUsize::new(0),
            resolutions: AtomicUsize::new(0),
            exhausted: false,
        });
        let runtime = runtime();
        let mut session = session(source.clone(), 2);

        let first = post(&mut session, &runtime).await;
        let error = session
            .settle(
                "not-the-active-post",
                SourcePostOutcome::Added { root_ids: vec![7] },
            )
            .unwrap_err();
        assert_eq!(error.kind, SourceErrorKind::InvalidResponse);
        assert_eq!(session.added_count(), 0);
        assert_eq!(session.cursor(), None);

        let error = session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, SourceErrorKind::InvalidResponse);
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 1);

        session
            .settle(
                &first.stable_id,
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::ExactDuplicate,
                },
            )
            .unwrap();
        let second = post(&mut session, &runtime).await;
        assert_eq!(second.stable_id, "2");
    }

    #[tokio::test]
    async fn terminal_pull_reports_source_exhaustion_after_settlement() {
        let source = Arc::new(FixtureSource {
            discoveries: AtomicUsize::new(0),
            resolutions: AtomicUsize::new(0),
            exhausted: true,
        });
        let runtime = runtime();
        let mut session = session(source.clone(), 3);

        let current = post(&mut session, &runtime).await;
        assert_eq!(current.stable_id, "1");
        session
            .settle(
                &current.stable_id,
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::NoUsableMedia,
                },
            )
            .unwrap();

        assert_eq!(
            session
                .next_post(&runtime, &CancellationToken::new())
                .await
                .unwrap(),
            NextPost::SourceExhausted,
        );
        assert_eq!(source.discoveries.load(Ordering::SeqCst), 1);
        assert_eq!(source.resolutions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn partitions_share_one_post_budget_and_keep_independent_cursors() {
        let runtime = runtime();
        let mut session = PartitionedSourceSession::new(
            Arc::new(PartitionFixtureSource { exhausted: true }),
            RequestCredentials::default(),
            "creator",
            BTreeMap::new(),
            10,
            3,
        )
        .unwrap();

        let first = match session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .unwrap()
        {
            NextPost::Post(post) => post,
            other => panic!("expected post, got {other:?}"),
        };
        assert_eq!(first.partition.0, "purchases");
        session
            .settle(
                &first.stable_id,
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::ExactDuplicate,
                },
            )
            .unwrap();

        let second = match session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .unwrap()
        {
            NextPost::Post(post) => post,
            other => panic!("expected post, got {other:?}"),
        };
        assert_eq!(second.partition.0, "messages");
        session
            .settle(
                &second.stable_id,
                SourcePostOutcome::Added { root_ids: vec![1] },
            )
            .unwrap();

        let third = match session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .unwrap()
        {
            NextPost::Post(post) => post,
            other => panic!("expected post, got {other:?}"),
        };
        assert_eq!(third.partition.0, "feed");
        session
            .settle(
                &third.stable_id,
                SourcePostOutcome::Added { root_ids: vec![2] },
            )
            .unwrap();

        assert_eq!(session.added_count(), 2);
        assert_eq!(
            session
                .next_post(&runtime, &CancellationToken::new())
                .await
                .unwrap(),
            NextPost::SourceExhausted,
        );
        assert_eq!(
            session
                .cursors()
                .get(&crate::SourcePartition::new("purchases"))
                .and_then(|cursor| cursor.as_deref()),
            Some("after-purchases"),
        );
        assert_eq!(
            session
                .cursors()
                .get(&crate::SourcePartition::new("messages"))
                .and_then(|cursor| cursor.as_deref()),
            Some("after-messages"),
        );
        assert_eq!(
            session
                .cursors()
                .get(&crate::SourcePartition::new("feed"))
                .and_then(|cursor| cursor.as_deref()),
            Some("after-feed"),
        );
    }

    #[tokio::test]
    async fn bounded_refresh_advances_a_partition_before_source_exhaustion() {
        let runtime = runtime();
        let mut session = PartitionedSourceSession::new(
            Arc::new(PartitionFixtureSource { exhausted: false }),
            RequestCredentials::default(),
            "creator",
            BTreeMap::new(),
            1,
            10,
        )
        .unwrap();

        let first = match session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .unwrap()
        {
            NextPost::Post(post) => post,
            other => panic!("expected post, got {other:?}"),
        };
        assert_eq!(first.partition.0, "purchases");
        session
            .settle(
                &first.stable_id,
                SourcePostOutcome::Skipped {
                    reason: crate::SkipReason::ExactDuplicate,
                },
            )
            .unwrap();
        session.finish_current_partition().unwrap();

        let second = match session
            .next_post(&runtime, &CancellationToken::new())
            .await
            .unwrap()
        {
            NextPost::Post(post) => post,
            other => panic!("expected post, got {other:?}"),
        };
        assert_eq!(second.partition.0, "messages");
    }
}
