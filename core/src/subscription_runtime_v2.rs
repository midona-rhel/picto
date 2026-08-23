//! Streaming subscription execution for the replacement backend.
//!
//! A source runner owns source-specific I/O. This module owns the durable
//! boundary: every downloaded item is recorded and queued before the source
//! query can be marked successful.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::mpsc::{self, Sender};
use tokio_util::sync::CancellationToken;

use crate::app::{resources, Application, MutationReceipt};
use crate::ingest_queue_v2::{self, IngestJobSpec};
use crate::ingest_v2::PreparedMediaInput;
use crate::subscriptions_v2::{self, ClaimedQueryRun, DomainSchedule, NormalizedPost};

const CHANNEL_CAPACITY: usize = 32;
const STREAM_INGEST_BATCH_SIZE: usize = 8;
const MAX_ATTEMPTS: i64 = 3;
const RETRY_BASE_SECONDS: i64 = 60;

pub type RunnerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RunnerSuccess, RunnerFailure>> + Send + 'a>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunnerSuccess {
    pub resume_cursor: Option<String>,
}

/// A source-normalized item that is ready for durable ingest.
#[derive(Debug, Clone)]
pub struct DownloadedItem {
    pub post: NormalizedPost,
    pub source_path: PathBuf,
    pub input: PreparedMediaInput,
    pub delete_after_ingest: bool,
}

/// Failure returned by a source runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerFailure {
    pub kind: RunnerFailureKind,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerFailureKind {
    Network,
    RateLimited,
    Authentication,
    InvalidQuery,
    InvalidOutput,
    Download,
    Runtime,
}

impl RunnerFailure {
    pub fn retryable(kind: RunnerFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: true,
        }
    }

    pub fn terminal(kind: RunnerFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
        }
    }
}

impl Display for RunnerFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl Error for RunnerFailure {}

impl Display for RunnerFailureKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Network => "network",
            Self::RateLimited => "rate_limited",
            Self::Authentication => "auth",
            Self::InvalidQuery => "invalid_query",
            Self::InvalidOutput => "invalid_output",
            Self::Download => "download",
            Self::Runtime => "runtime",
        })
    }
}

/// Runs one claimed query and streams completed downloads into `output`.
pub trait SourceRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: Sender<DownloadedItem>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a>;
}

/// Durable subscription worker with process-local domain scheduling.
pub struct SubscriptionWorker<'a, R> {
    application: &'a Application,
    runner: R,
    schedule: DomainSchedule,
    cancel: CancellationToken,
}

impl<'a, R: SourceRunner> SubscriptionWorker<'a, R> {
    pub fn new(application: &'a Application, runner: R) -> Self {
        Self::with_cancellation(application, runner, CancellationToken::new())
    }

    pub fn with_cancellation(
        application: &'a Application,
        runner: R,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            application,
            runner,
            schedule: DomainSchedule::new(),
            cancel,
        }
    }

    pub fn schedule(&self) -> &DomainSchedule {
        &self.schedule
    }

    pub async fn tick(&mut self, now: &str) -> Result<Option<MutationReceipt>, String> {
        tick_with_cancellation(
            self.application,
            &mut self.schedule,
            &self.runner,
            &self.cancel,
            now,
        )
        .await
    }
}

/// Claims and runs at most one query.
pub async fn tick<R: SourceRunner>(
    application: &Application,
    schedule: &mut DomainSchedule,
    runner: &R,
    now: &str,
) -> Result<Option<MutationReceipt>, String> {
    tick_with_cancellation(
        application,
        schedule,
        runner,
        &CancellationToken::new(),
        now,
    )
    .await
}

async fn tick_with_cancellation<R: SourceRunner>(
    application: &Application,
    schedule: &mut DomainSchedule,
    runner: &R,
    cancel: &CancellationToken,
    now: &str,
) -> Result<Option<MutationReceipt>, String> {
    let Some(query) = subscriptions_v2::claim_next_query_run(application.store(), schedule, now)?
    else {
        return Ok(None);
    };

    let runner_result = run_stream(application, &query, runner, cancel).await;
    match runner_result {
        Ok(Ok(success)) => {
            subscriptions_v2::complete_query_run_with_cursor(
                application.store(),
                query.run_query_id,
                success.resume_cursor.as_deref(),
                now,
            )?;
            if let Err(error) =
                crate::auth_v2::mark_run_success(application.store(), &query.site_id, now)
            {
                tracing::warn!(site_id = %query.site_id, error = %error, "Failed to record credential health");
            }
        }
        Ok(Err(failure)) => {
            settle_runner_failure(application, &query, failure, now)?;
        }
        Err(error) => {
            settle_runner_failure(
                application,
                &query,
                RunnerFailure::terminal(RunnerFailureKind::Runtime, error),
                now,
            )?;
        }
    }

    let receipt = MutationReceipt {
        revision: application.store().revision()?,
        resources: vec![
            resources::SUBSCRIPTIONS.to_string(),
            resources::TASKS.to_string(),
        ],
        item_ids: Vec::new(),
    };
    application.publish(&receipt);
    Ok(Some(receipt))
}

async fn run_stream<R: SourceRunner>(
    application: &Application,
    query: &ClaimedQueryRun,
    runner: &R,
    cancel: &CancellationToken,
) -> Result<Result<RunnerSuccess, RunnerFailure>, String> {
    let (output, mut input) = mpsc::channel(CHANNEL_CAPACITY);
    let runner_future = runner.run(query, output, cancel.child_token());
    tokio::pin!(runner_future);

    let runner_result = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return Ok(Err(RunnerFailure::retryable(
                    RunnerFailureKind::Runtime,
                    "Subscription run interrupted",
                )));
            }
            result = &mut runner_future => {
                break result;
            }
            item = input.recv() => match item {
                Some(item) => {
                    if let Err(error) = process_item(application, query, &item) {
                        release_post_archive(application, query, &item.post.post_key).await;
                        return Err(error);
                    }
                }
                None => {
                    break runner_future.await;
                }
            },
        }
    };

    while let Some(item) = input.recv().await {
        if let Err(error) = process_item(application, query, &item) {
            release_post_archive(application, query, &item.post.post_key).await;
            return Err(error);
        }
    }
    Ok(runner_result)
}

async fn release_post_archive(application: &Application, query: &ClaimedQueryRun, post_key: &str) {
    let prefix = crate::subscriptions::archive::subscription_query_archive_prefix(
        query.subscription_id,
        query.query_id,
    );
    let _ = crate::subscriptions::archive::clear_post_archive_entries_at_root(
        application.store().library_root(),
        &prefix,
        &[post_key.to_string()],
    )
    .await;
}

fn process_item(
    application: &Application,
    query: &ClaimedQueryRun,
    item: &DownloadedItem,
) -> Result<(), String> {
    let source = item
        .input
        .source
        .as_ref()
        .ok_or_else(|| "A subscription item needs source identity".to_string())?;
    if source.site_id != item.post.site_id
        || source.post_key != item.post.post_key
        || !item
            .post
            .items
            .iter()
            .any(|post_item| post_item.item_key == source.item_key)
    {
        return Err("Downloaded item identity does not match its normalized post".to_string());
    }

    subscriptions_v2::record_post(
        application.store(),
        query.run_query_id,
        &item.post,
        &Utc::now().to_rfc3339(),
    )
    .map_err(|error| format!("recording subscription post failed: {error}"))?;
    let spec = IngestJobSpec::subscription(
        item.source_path.display().to_string(),
        item.delete_after_ingest,
        item.input.clone(),
    )?;
    ingest_queue_v2::enqueue(application, &spec)
        .map_err(|error| format!("enqueueing subscription item failed: {error}"))?;

    // A streamed download should become visible while the source run is still
    // active. The durable queue remains authoritative if processing retries.
    ingest_queue_v2::run_batch(application, STREAM_INGEST_BATCH_SIZE)
        .map_err(|error| format!("processing subscription ingest failed: {error}"))?;
    Ok(())
}

fn settle_runner_failure(
    application: &Application,
    query: &ClaimedQueryRun,
    failure: RunnerFailure,
    now: &str,
) -> Result<(), String> {
    let authentication_failed = failure.kind == RunnerFailureKind::Authentication;
    let failure_message = failure.message.clone();
    let retry_at = if failure.retryable && query.attempt_count < MAX_ATTEMPTS {
        Some(next_retry_at(now, query.attempt_count)?)
    } else {
        None
    };
    subscriptions_v2::fail_query_run(
        application.store(),
        query.run_query_id,
        &failure.kind.to_string(),
        &failure.message,
        retry_at.as_deref(),
        now,
    )?;
    if authentication_failed {
        if let Err(error) = crate::auth_v2::mark_auth_failure(
            application.store(),
            &query.site_id,
            now,
            &failure_message,
        ) {
            tracing::warn!(site_id = %query.site_id, error = %error, "Failed to record credential health");
        }
    }
    Ok(())
}

fn next_retry_at(now: &str, attempt_count: i64) -> Result<String, String> {
    let timestamp = DateTime::parse_from_rfc3339(now)
        .map_err(|error| format!("invalid retry timestamp {now}: {error}"))?;
    let exponent = attempt_count.saturating_sub(1).clamp(0, 3) as u32;
    let seconds = RETRY_BASE_SECONDS * 2_i64.pow(exponent);
    Ok((timestamp + Duration::seconds(seconds)).to_rfc3339())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::app::{Application, Lifecycle};
    use crate::ingest_v2::SourcePostInput;
    use crate::store::Store;
    use crate::subscriptions_v2::{
        create_query, create_run, create_subscription, QueryInput, SubscriptionInput,
    };

    const FIRST_NOW: &str = "2026-01-01T00:00:00Z";
    const RETRY_NOW: &str = "2026-01-01T00:01:00Z";

    struct FakeRun {
        items: Vec<DownloadedItem>,
        result: Result<RunnerSuccess, RunnerFailure>,
    }

    #[derive(Default)]
    struct FakeRunner {
        runs: Mutex<VecDeque<FakeRun>>,
    }

    struct CancellationAwareRunner;

    impl SourceRunner for CancellationAwareRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            _output: Sender<DownloadedItem>,
            cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                assert!(cancel.is_cancelled());
                Err(RunnerFailure::retryable(
                    RunnerFailureKind::Runtime,
                    "cancelled",
                ))
            })
        }
    }

    impl SourceRunner for FakeRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            output: Sender<DownloadedItem>,
            _cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                let run = self.runs.lock().unwrap().pop_front().unwrap();
                for item in run.items {
                    output.send(item).await.map_err(|_| {
                        RunnerFailure::terminal(RunnerFailureKind::Runtime, "receiver closed")
                    })?;
                }
                run.result
            })
        }
    }

    fn fixture() -> (tempfile::TempDir, Application, i64) {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let subscription = create_subscription(
            application.store(),
            &SubscriptionInput {
                subscription_key: "subscription".to_string(),
                name: "Subscription".to_string(),
                schedule: "manual".to_string(),
                paused: false,
                initial_post_limit: None,
                periodic_post_limit: None,
            },
            FIRST_NOW,
        )
        .unwrap();
        create_query(
            application.store(),
            subscription,
            &QueryInput {
                query_key: "query".to_string(),
                site_id: "example".to_string(),
                domain_key: "example.test".to_string(),
                query_kind: "url".to_string(),
                query_text: "https://example.test".to_string(),
                display_name: None,
                notes: None,
            },
        )
        .unwrap();
        create_run(application.store(), subscription, "test", FIRST_NOW).unwrap();
        (directory, application, subscription)
    }

    fn item(root: &Path, post_key: &str, item_key: &str) -> DownloadedItem {
        item_at(root, post_key, item_key, 0)
    }

    fn item_at(root: &Path, post_key: &str, item_key: &str, position: i64) -> DownloadedItem {
        let bytes = format!("media-{post_key}-{item_key}").into_bytes();
        let source_path = root.join(format!("{post_key}-{item_key}.png"));
        std::fs::write(&source_path, &bytes).unwrap();
        DownloadedItem {
            post: NormalizedPost {
                site_id: "example".to_string(),
                post_key: post_key.to_string(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![crate::subscriptions_v2::NormalizedItem {
                    item_key: item_key.to_string(),
                    position,
                    media_url: None,
                    canonical_url: None,
                }],
            },
            source_path,
            input: PreparedMediaInput {
                file_hash: hex::encode(crate::media_processing::get_hash_from_bytes(&bytes)),
                mime_type: "image/png".to_string(),
                size_bytes: bytes.len() as i64,
                pixel_width: Some(1),
                pixel_height: Some(1),
                duration_ms: None,
                frame_count: Some(1),
                has_audio: false,
                name: Some(item_key.to_string()),
                notes: None,
                rating: None,
                source_urls: Vec::new(),
                tags: Vec::new(),
                provenance_mask: 1,
                lifecycle: Lifecycle::Inbox,
                captured_at: None,
                source: Some(SourcePostInput {
                    site_id: "example".to_string(),
                    post_key: post_key.to_string(),
                    item_key: item_key.to_string(),
                    position,
                    canonical_post_url: None,
                    canonical_media_url: None,
                    creator_name: None,
                    title: None,
                    description: None,
                    captured_at: None,
                    metadata_json: None,
                }),
                target_folder_id: None,
            },
            delete_after_ingest: false,
        }
    }

    #[tokio::test]
    async fn streams_two_items_before_successful_completion() {
        let (directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                items: vec![
                    item(directory.path(), "post-a", "item-a"),
                    item(directory.path(), "post-b", "item-b"),
                ],
                result: Ok(RunnerSuccess {
                    resume_cursor: Some("cursor-2".to_string()),
                }),
            }])),
        };
        let mut worker = SubscriptionWorker::new(&application, runner);

        let receipt = worker.tick(FIRST_NOW).await.unwrap().unwrap();
        assert!(receipt.item_ids.is_empty());
        assert_eq!(
            receipt.resources,
            vec![resources::SUBSCRIPTIONS, resources::TASKS]
        );

        let (posts, jobs, state, cursor): (i64, i64, String, String) = application
            .store()
            .read(|connection| {
                let posts =
                    connection
                        .query_row("SELECT COUNT(*) FROM source_post", [], |row| row.get(0))?;
                let jobs = connection
                    .query_row("SELECT COUNT(*) FROM ingest_job", [], |row| row.get(0))?;
                let state = connection.query_row(
                    "SELECT status FROM subscription_run_query",
                    [],
                    |row| row.get(0),
                )?;
                let cursor = connection.query_row(
                    "SELECT resume_cursor FROM subscription_query",
                    [],
                    |row| row.get(0),
                )?;
                Ok((posts, jobs, state, cursor))
            })
            .unwrap();
        assert_eq!(
            (posts, jobs, state, cursor),
            (2, 2, "succeeded".to_string(), "cursor-2".to_string())
        );
    }

    #[tokio::test]
    async fn cancellation_reaches_the_source_and_persists_retry_state() {
        let (_directory, application, _subscription) = fixture();
        let cancel = CancellationToken::new();
        let mut worker = SubscriptionWorker::with_cancellation(
            &application,
            CancellationAwareRunner,
            cancel.clone(),
        );
        cancel.cancel();

        worker.tick(FIRST_NOW).await.unwrap();
        let state: (String, String) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT status, failure_kind FROM subscription_run_query",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(state, ("pending".to_string(), "runtime".to_string()));
    }

    #[tokio::test]
    async fn retryable_runner_failure_preserves_streamed_items_and_retries() {
        let (directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([
                FakeRun {
                    items: vec![
                        item(directory.path(), "post-a", "item-a"),
                        item(directory.path(), "post-b", "item-b"),
                    ],
                    result: Err(RunnerFailure::retryable(
                        RunnerFailureKind::Network,
                        "temporary source failure",
                    )),
                },
                FakeRun {
                    items: Vec::new(),
                    result: Ok(RunnerSuccess::default()),
                },
            ])),
        };
        let mut worker = SubscriptionWorker::new(&application, runner);

        worker.tick(FIRST_NOW).await.unwrap().unwrap();
        let first_state: (String, String, i64) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT qr.status, qr.failure_kind, qr.attempt_count
                     FROM subscription_run_query qr",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(
            first_state,
            ("pending".to_string(), "network".to_string(), 1)
        );

        worker.tick(RETRY_NOW).await.unwrap().unwrap();
        let final_state: (String, i64, i64) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT qr.status, qr.attempt_count,
                            (SELECT COUNT(*) FROM ingest_job)
                     FROM subscription_run_query qr",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(final_state, ("succeeded".to_string(), 2, 2));
        let issue_status: String = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT status FROM subscription_issue", [], |row| {
                    row.get(0)
                })
            })
            .unwrap();
        assert_eq!(issue_status, "resolved");
    }

    #[tokio::test]
    async fn streamed_multi_item_post_promotes_to_one_inbox_collection() {
        let (directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                items: vec![
                    item_at(directory.path(), "post", "page-1", 0),
                    item_at(directory.path(), "post", "page-2", 1),
                ],
                result: Ok(RunnerSuccess::default()),
            }])),
        };
        let mut worker = SubscriptionWorker::new(&application, runner);
        worker.tick(FIRST_NOW).await.unwrap().unwrap();

        application
            .store()
            .read(|connection| {
                let ingested: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM ingest_job WHERE status = 'succeeded'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(ingested, 2);
                let root: (i64, String, String) = connection.query_row(
                    "SELECT lr.item_id, lr.lifecycle, li.kind
                     FROM library_root lr
                     JOIN library_item li ON li.item_id = lr.item_id",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!((root.1.as_str(), root.2.as_str()), ("inbox", "collection"));
                let members = connection
                    .prepare(
                        "SELECT cm.media_item_id
                         FROM collection_member cm
                         JOIN source_item si ON si.media_item_id = cm.media_item_id
                         WHERE cm.collection_id = ?1
                         ORDER BY cm.position_rank",
                    )?
                    .query_map([root.0], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(members.len(), 2);
                Ok(())
            })
            .unwrap();
        assert_eq!(application.projections().inbox_bitmap().len(), 1);
    }

    #[tokio::test]
    async fn durable_enqueue_failure_releases_gallery_archive_for_retry() {
        let (directory, application, subscription_id) = fixture();
        let failed_item = item(directory.path(), "post-a", "item-a");
        std::fs::remove_file(&failed_item.source_path).unwrap();
        let archive =
            rusqlite::Connection::open(directory.path().join("gdl-archive.sqlite3")).unwrap();
        archive
            .execute_batch(
                "CREATE TABLE archive (entry TEXT PRIMARY KEY);
                 INSERT INTO archive VALUES ('picto_s1_q1_example_post-a_item-a');",
            )
            .unwrap();
        drop(archive);
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                items: vec![failed_item],
                result: Ok(RunnerSuccess::default()),
            }])),
        };
        let mut worker = SubscriptionWorker::new(&application, runner);

        worker.tick(FIRST_NOW).await.unwrap().unwrap();

        let archive =
            rusqlite::Connection::open(directory.path().join("gdl-archive.sqlite3")).unwrap();
        let remaining: i64 = archive
            .query_row("SELECT COUNT(*) FROM archive", [], |row| row.get(0))
            .unwrap();
        assert_eq!(subscription_id, 1);
        assert_eq!(remaining, 0);
    }
}
