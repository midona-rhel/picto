//! Streaming subscription execution for the replacement backend.
//!
//! A source runner owns source-specific I/O. This module owns the durable
//! boundary: every downloaded item is recorded and queued before the source
//! query can be marked successful.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::mpsc::{self, Sender};
use tokio_util::sync::CancellationToken;

use crate::app::{resources, Application, MutationReceipt};
use crate::ingest_queue_v2::{self, IngestJobSpec};
use crate::ingest_v2::PreparedMediaInput;
use crate::subscriptions_v2::{self, ClaimedQueryRun, DomainSchedule, NormalizedPost};

const CHANNEL_CAPACITY: usize = 32;
const QUERY_INGEST_BATCH_SIZE: usize = 8;
const MAX_ATTEMPTS: i64 = 3;
const RETRY_BASE_SECONDS: i64 = 60;
const RUN_STATE_POLL: std::time::Duration = std::time::Duration::from_millis(250);
const PROGRESS_PUBLISH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

pub type RunnerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RunnerSuccess, RunnerFailure>> + Send + 'a>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunnerSuccess {
    pub resume_cursor: Option<String>,
    pub cleanup_paths: Vec<PathBuf>,
}

/// A source-normalized item that is ready for durable ingest.
#[derive(Debug, Clone)]
pub struct DownloadedItem {
    pub post: NormalizedPost,
    pub source_path: PathBuf,
    pub input: PreparedMediaInput,
    pub delete_after_ingest: bool,
}

#[derive(Debug, Clone)]
pub enum SourceEvent {
    PostTraversed(NormalizedPost),
    MediaDownloaded(DownloadedItem),
}

struct SourceProgressPublisher {
    last_publish: Instant,
    dirty: bool,
}

impl SourceProgressPublisher {
    fn new() -> Self {
        Self {
            last_publish: Instant::now(),
            dirty: false,
        }
    }

    fn changed(&mut self, application: &Application) -> Result<(), String> {
        self.dirty = true;
        if self.last_publish.elapsed() >= PROGRESS_PUBLISH_INTERVAL {
            self.flush(application)?;
        }
        Ok(())
    }

    fn flush(&mut self, application: &Application) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        publish_source_progress(application)?;
        self.last_publish = Instant::now();
        self.dirty = false;
        Ok(())
    }
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
    Interrupted,
    InboxFull,
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
            Self::Interrupted => "interrupted",
            Self::InboxFull => "inbox_full",
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
        output: Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a>;
}

/// Durable subscription worker with process-local domain scheduling.
pub struct SubscriptionWorker<'a, R> {
    application: &'a Application,
    runner: R,
    schedule: Arc<Mutex<DomainSchedule>>,
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
        Self::with_shared_schedule(
            application,
            runner,
            Arc::new(Mutex::new(DomainSchedule::new())),
            cancel,
        )
    }

    pub(crate) fn with_shared_schedule(
        application: &'a Application,
        runner: R,
        schedule: Arc<Mutex<DomainSchedule>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            application,
            runner,
            schedule,
            cancel,
        }
    }

    pub async fn tick(&self, now: &str) -> Result<Option<MutationReceipt>, String> {
        let inbox_limit = crate::settings_v2::subscription_inbox_item_limit(self.application)?;
        let inbox_full = crate::settings_v2::subscription_inbox_is_full(self.application)?;
        if subscriptions_v2::set_pending_runs_waiting_for_inbox(
            self.application.store(),
            inbox_full,
            inbox_limit,
        )? {
            publish_subscription_receipt(self.application)?;
        }
        if inbox_full {
            return Ok(None);
        }
        let query = {
            let mut schedule = self
                .schedule
                .lock()
                .map_err(|_| "subscription domain schedule lock is poisoned".to_string())?;
            subscriptions_v2::claim_next_query_run(self.application.store(), &mut schedule, now)?
        };
        let Some(query) = query else {
            return Ok(None);
        };
        let domain_key = query.domain_key.clone();
        let result =
            run_claimed_query(self.application, &self.runner, &self.cancel, query, now).await;
        self.schedule
            .lock()
            .map_err(|_| "subscription domain schedule lock is poisoned".to_string())?
            .mark_finished(domain_key, Utc::now().timestamp_millis());
        result
    }
}

/// Claims and runs at most one query.
pub async fn tick<R: SourceRunner>(
    application: &Application,
    schedule: &mut DomainSchedule,
    runner: &R,
    now: &str,
) -> Result<Option<MutationReceipt>, String> {
    let inbox_limit = crate::settings_v2::subscription_inbox_item_limit(application)?;
    let inbox_full = crate::settings_v2::subscription_inbox_is_full(application)?;
    if subscriptions_v2::set_pending_runs_waiting_for_inbox(
        application.store(),
        inbox_full,
        inbox_limit,
    )? {
        publish_subscription_receipt(application)?;
    }
    if inbox_full {
        return Ok(None);
    }
    let Some(query) = subscriptions_v2::claim_next_query_run(application.store(), schedule, now)?
    else {
        return Ok(None);
    };
    run_claimed_query(application, runner, &CancellationToken::new(), query, now).await
}

async fn run_claimed_query<R: SourceRunner>(
    application: &Application,
    runner: &R,
    cancel: &CancellationToken,
    query: ClaimedQueryRun,
    now: &str,
) -> Result<Option<MutationReceipt>, String> {
    let runner_result = run_stream(application, &query, runner, cancel).await;
    match runner_result {
        Ok(Ok(success)) => {
            match subscriptions_v2::complete_query_run_with_cursor(
                application.store(),
                query.run_query_id,
                success.resume_cursor.as_deref(),
                now,
            ) {
                Ok(transition) => transition,
                Err(error) => {
                    settle_runner_failure(
                        application,
                        &query,
                        RunnerFailure::retryable(
                            RunnerFailureKind::Runtime,
                            format!("settling completed source query failed: {error}"),
                        ),
                        now,
                    )?;
                    return publish_subscription_receipt(application);
                }
            };
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

    publish_subscription_receipt(application)
}

fn publish_subscription_receipt(
    application: &Application,
) -> Result<Option<MutationReceipt>, String> {
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
    let destination = crate::subscription_catalog_v2::subscription_destination(
        application,
        query.subscription_id,
    )?;
    let (output, mut input) = mpsc::channel(CHANNEL_CAPACITY);
    let runner_cancel = cancel.child_token();
    let runner_future = runner.run(query, output, runner_cancel.clone());
    tokio::pin!(runner_future);
    let mut state_poll = tokio::time::interval(RUN_STATE_POLL);
    state_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input_open = true;
    let atomic_gallery = query.site_id == "ehentai";
    let mut gallery_posts = Vec::new();
    let mut gallery_items = Vec::new();
    let mut recorded_source_items = BTreeSet::new();
    let mut progress = SourceProgressPublisher::new();

    let runner_result = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                runner_cancel.cancel();
                return Ok(Err(RunnerFailure::retryable(
                    RunnerFailureKind::Interrupted,
                    "Subscription run interrupted",
                )));
            }
            _ = state_poll.tick() => {
                if crate::settings_v2::subscription_inbox_is_full(application)? {
                    runner_cancel.cancel();
                    return Ok(Err(RunnerFailure::retryable(
                        RunnerFailureKind::InboxFull,
                        "Inbox reached its configured subscription limit",
                    )));
                }
                let running = subscriptions_v2::get_query_run(
                    application.store(),
                    query.run_query_id,
                )?.is_some_and(|record| record.state == subscriptions_v2::RunState::Running);
                if !running {
                    runner_cancel.cancel();
                    return Ok(Err(RunnerFailure::retryable(
                        RunnerFailureKind::Interrupted,
                        "Subscription run stopped",
                    )));
                }
            }
            result = &mut runner_future => {
                break result;
            }
            event = input.recv(), if input_open => match event {
                Some(event) => handle_source_event(
                    application,
                    query,
                    &destination,
                    event,
                    atomic_gallery,
                    &mut gallery_posts,
                    &mut gallery_items,
                    &mut recorded_source_items,
                    &mut progress,
                ).await?,
                None => {
                    input_open = false;
                }
            },
        }
    };

    while let Some(event) = input.recv().await {
        handle_source_event(
            application,
            query,
            &destination,
            event,
            atomic_gallery,
            &mut gallery_posts,
            &mut gallery_items,
            &mut recorded_source_items,
            &mut progress,
        )
        .await?;
    }

    if atomic_gallery {
        if runner_result.is_ok() {
            validate_complete_gallery(&gallery_items)?;
            let mut gallery_post = gallery_posts
                .pop()
                .unwrap_or_else(|| gallery_items[0].post.clone());
            gallery_post.items = gallery_items
                .iter()
                .flat_map(|item| item.post.items.iter().cloned())
                .collect();
            subscriptions_v2::record_post(
                application.store(),
                query.run_query_id,
                &gallery_post,
                &Utc::now().to_rfc3339(),
            )
            .map_err(|error| format!("recording complete gallery failed: {error}"))?;
            for item in &gallery_items {
                if let Err(error) = process_item_after_post(application, query, item, &destination)
                {
                    release_post_archive(application, query, &item.post.post_key).await;
                    return Err(error);
                }
            }
            progress.changed(application)?;
        } else {
            let post_keys = gallery_items
                .iter()
                .map(|item| item.post.post_key.as_str())
                .collect::<BTreeSet<_>>();
            for post_key in post_keys {
                release_post_archive(application, query, post_key).await;
            }
            for item in gallery_items {
                if item.delete_after_ingest {
                    let _ = std::fs::remove_file(item.source_path);
                }
            }
        }
    }
    let ingest =
        ingest_queue_v2::drain_query(application, query.run_query_id, QUERY_INGEST_BATCH_SIZE)
            .map_err(|error| format!("settling subscription ingest failed: {error}"))?;
    if ingest.claimed != 0 {
        progress.changed(application)?;
    }
    progress.flush(application)?;
    if let Ok(success) = &runner_result {
        for path in &success.cleanup_paths {
            if let Err(error) = std::fs::remove_dir_all(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), error = %error, "Could not clean completed source run");
                }
            }
        }
    }
    Ok(runner_result)
}

async fn handle_source_event(
    application: &Application,
    query: &ClaimedQueryRun,
    destination: &crate::subscription_catalog_v2::SubscriptionDestinationPolicy,
    event: SourceEvent,
    atomic_gallery: bool,
    gallery_posts: &mut Vec<NormalizedPost>,
    gallery_items: &mut Vec<DownloadedItem>,
    recorded_source_items: &mut BTreeSet<(String, String)>,
    progress: &mut SourceProgressPublisher,
) -> Result<(), String> {
    let durable_change = match event {
        SourceEvent::PostTraversed(post) if atomic_gallery => {
            gallery_posts.push(post);
            false
        }
        SourceEvent::MediaDownloaded(item) if atomic_gallery => {
            gallery_items.push(item);
            false
        }
        SourceEvent::PostTraversed(post) => {
            subscriptions_v2::record_post(
                application.store(),
                query.run_query_id,
                &post,
                &Utc::now().to_rfc3339(),
            )
            .map_err(|error| format!("recording traversed source post failed: {error}"))?;
            recorded_source_items.extend(
                post.items
                    .iter()
                    .map(|item| (post.post_key.clone(), item.item_key.clone())),
            );
            true
        }
        SourceEvent::MediaDownloaded(item) => {
            ensure_source_item_recorded(application, query, &item, recorded_source_items)?;
            if let Err(error) = process_item_after_post(application, query, &item, destination) {
                release_post_archive(application, query, &item.post.post_key).await;
                return Err(error);
            }
            true
        }
    };
    if durable_change {
        progress.changed(application)?;
    }
    Ok(())
}

fn ensure_source_item_recorded(
    application: &Application,
    query: &ClaimedQueryRun,
    item: &DownloadedItem,
    recorded_source_items: &mut BTreeSet<(String, String)>,
) -> Result<(), String> {
    let source = item
        .input
        .source
        .as_ref()
        .ok_or_else(|| "A subscription item needs source identity".to_string())?;
    let identity = (source.post_key.clone(), source.item_key.clone());
    if recorded_source_items.contains(&identity) {
        return Ok(());
    }
    subscriptions_v2::record_post(
        application.store(),
        query.run_query_id,
        &item.post,
        &Utc::now().to_rfc3339(),
    )
    .map_err(|error| format!("recording fallback source post failed: {error}"))?;
    recorded_source_items.extend(
        item.post
            .items
            .iter()
            .map(|post_item| (item.post.post_key.clone(), post_item.item_key.clone())),
    );
    Ok(())
}

fn validate_complete_gallery(items: &[DownloadedItem]) -> Result<(), String> {
    let first = items
        .first()
        .ok_or_else(|| "Gallery download completed without media".to_string())?;
    if items
        .iter()
        .any(|item| item.post.post_key != first.post.post_key)
    {
        return Err("A gallery import produced more than one source post".to_string());
    }
    if !items.last().is_some_and(|item| {
        item.input
            .source
            .as_ref()
            .is_some_and(|source| source.post_complete)
    }) {
        return Err("Gallery download ended before the complete post was available".to_string());
    }
    let expected = items.iter().find_map(|item| {
        item.post
            .metadata_json
            .as_deref()
            .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
            .and_then(|metadata| {
                metadata
                    .get("filecount")
                    .or_else(|| metadata.get("count"))
                    .and_then(|value| {
                        value
                            .as_u64()
                            .or_else(|| value.as_str()?.parse::<u64>().ok())
                    })
            })
    });
    if let Some(expected) = expected.filter(|expected| *expected != items.len() as u64) {
        return Err(format!(
            "Gallery download was incomplete: received {} of {} media files",
            items.len(),
            expected
        ));
    }
    Ok(())
}

fn publish_source_progress(application: &Application) -> Result<(), String> {
    application.publish(&MutationReceipt {
        revision: application.store().revision()?,
        // Per-item traversal/download progress does not change subscription
        // configuration. Publishing the catalog resource here made the UI run
        // its expensive full subscription query for every source event.
        resources: vec![resources::TASKS.to_string()],
        item_ids: Vec::new(),
    });
    Ok(())
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

fn process_item_after_post(
    application: &Application,
    query: &ClaimedQueryRun,
    item: &DownloadedItem,
    destination: &crate::subscription_catalog_v2::SubscriptionDestinationPolicy,
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

    let mut input = item.input.clone();
    if let Some(source) = input.source.as_mut() {
        source.group_post = query.group_posts;
    }
    input.target_folder_id = None;
    input.target_folder_ids = destination.target_folder_ids.clone();
    for tag in &destination.automatic_tags {
        if !input.tags.contains(tag) {
            input.tags.push(tag.clone());
        }
    }
    let spec = IngestJobSpec::subscription(
        item.source_path.display().to_string(),
        item.delete_after_ingest,
        input,
    )?;
    if let Err(error) = ingest_queue_v2::enqueue(application, &spec) {
        if crate::ingest_v2::is_deleted_source_item_error(&error) {
            if item.delete_after_ingest {
                let _ = std::fs::remove_file(&item.source_path);
            }
            return Ok(());
        }
        return Err(format!("enqueueing subscription item failed: {error}"));
    }

    Ok(())
}

fn settle_runner_failure(
    application: &Application,
    query: &ClaimedQueryRun,
    failure: RunnerFailure,
    now: &str,
) -> Result<(), String> {
    if failure.kind == RunnerFailureKind::Interrupted {
        return subscriptions_v2::interrupt_query_run(application.store(), query.run_query_id, now);
    }
    if failure.kind == RunnerFailureKind::InboxFull {
        let limit = crate::settings_v2::subscription_inbox_item_limit(application)?;
        return subscriptions_v2::wait_query_run_for_inbox(
            application.store(),
            query.run_query_id,
            limit,
            now,
        );
    }
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

    use rusqlite::params;

    use super::*;
    use crate::app::{Application, Lifecycle};
    use crate::ingest_v2::SourcePostInput;
    use crate::store::Store;
    use crate::subscriptions_v2::{
        create_query, create_run, create_subscription, QueryInput, SubscriptionInput,
    };

    const FIRST_NOW: &str = "2026-01-01T00:00:00Z";

    struct FakeRun {
        posts: Vec<NormalizedPost>,
        items: Vec<DownloadedItem>,
        result: Result<RunnerSuccess, RunnerFailure>,
    }

    #[derive(Default)]
    struct FakeRunner {
        runs: Mutex<VecDeque<FakeRun>>,
    }

    struct CancellationAwareRunner;

    struct PersistedCancellationAwareRunner {
        library_root: PathBuf,
        subscription_id: i64,
    }

    impl SourceRunner for CancellationAwareRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            _output: Sender<SourceEvent>,
            cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                assert!(cancel.is_cancelled());
                Err(RunnerFailure::retryable(
                    RunnerFailureKind::Interrupted,
                    "cancelled",
                ))
            })
        }
    }

    impl SourceRunner for PersistedCancellationAwareRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            _output: Sender<SourceEvent>,
            cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                let store = Store::open(&self.library_root)
                    .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Runtime, error))?;
                subscriptions_v2::cancel_subscription_run(
                    &store,
                    self.subscription_id,
                    "2026-01-01T00:00:01Z",
                )
                .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Runtime, error))?;
                cancel.cancelled().await;
                Err(RunnerFailure::retryable(
                    RunnerFailureKind::Interrupted,
                    "cancelled",
                ))
            })
        }
    }

    impl SourceRunner for FakeRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            output: Sender<SourceEvent>,
            _cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                let run = self.runs.lock().unwrap().pop_front().unwrap();
                for post in run.posts {
                    output
                        .send(SourceEvent::PostTraversed(post))
                        .await
                        .map_err(|_| {
                            RunnerFailure::terminal(RunnerFailureKind::Runtime, "receiver closed")
                        })?;
                }
                for item in run.items {
                    output
                        .send(SourceEvent::MediaDownloaded(item))
                        .await
                        .map_err(|_| {
                            RunnerFailure::terminal(RunnerFailureKind::Runtime, "receiver closed")
                        })?;
                }
                run.result
            })
        }
    }

    fn fixture() -> (tempfile::TempDir, Application, i64) {
        fixture_for_site("example", "example.test", "https://example.test")
    }

    fn fixture_for_site(
        site_id: &str,
        domain_key: &str,
        query_text: &str,
    ) -> (tempfile::TempDir, Application, i64) {
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
                site_id: site_id.to_string(),
                domain_key: domain_key.to_string(),
                query_kind: "url".to_string(),
                query_text: query_text.to_string(),
                display_name: None,
                notes: None,
            },
        )
        .unwrap();
        create_run(application.store(), subscription, "test", FIRST_NOW).unwrap();
        (directory, application, subscription)
    }

    fn drain_maintenance_ingest(application: &Application) {
        for _ in 0..32 {
            let report = ingest_queue_v2::run_batch(application, 64).unwrap();
            if report.claimed == 0 {
                return;
            }
        }
        panic!("maintenance ingest did not drain within the test bound");
    }

    fn item(root: &Path, post_key: &str, item_key: &str) -> DownloadedItem {
        item_at(root, post_key, item_key, 0)
    }

    fn empty_post(post_key: &str) -> NormalizedPost {
        NormalizedPost {
            site_id: "example".to_string(),
            post_key: post_key.to_string(),
            canonical_url: None,
            creator_name: None,
            title: None,
            description: None,
            captured_at: None,
            metadata_json: None,
            items: Vec::new(),
        }
    }

    fn item_at(root: &Path, post_key: &str, item_key: &str, position: i64) -> DownloadedItem {
        let seed = crate::media_processing::get_hash_from_bytes(
            format!("media-{post_key}-{item_key}").as_bytes(),
        );
        let image =
            image::RgbaImage::from_pixel(2, 2, image::Rgba([seed[0], seed[1], seed[2], 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let bytes = encoded.into_inner();
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
                lifecycle: Lifecycle::Inbox,
                captured_at: None,
                source: Some(SourcePostInput {
                    site_id: "example".to_string(),
                    post_key: post_key.to_string(),
                    item_key: item_key.to_string(),
                    position,
                    post_complete: true,
                    force_collection: false,
                    group_post: true,
                    canonical_post_url: None,
                    canonical_media_url: None,
                    creator_name: None,
                    title: None,
                    description: None,
                    captured_at: None,
                    metadata_json: None,
                }),
                target_folder_id: None,
                target_folder_ids: Vec::new(),
            },
            delete_after_ingest: false,
        }
    }

    #[tokio::test]
    async fn streams_two_items_before_successful_completion() {
        let (directory, application, _subscription) = fixture();
        let cleanup = directory.path().join("completed-source-run");
        std::fs::create_dir_all(&cleanup).unwrap();
        std::fs::write(cleanup.join("source.bin"), b"source").unwrap();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![
                    item(directory.path(), "post-a", "item-a"),
                    item(directory.path(), "post-b", "item-b"),
                ],
                result: Ok(RunnerSuccess {
                    resume_cursor: Some("cursor-2".to_string()),
                    cleanup_paths: vec![cleanup.clone()],
                }),
            }])),
        };
        let worker = SubscriptionWorker::new(&application, runner);

        let receipt = worker.tick(FIRST_NOW).await.unwrap().unwrap();
        assert!(!cleanup.exists());
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
    async fn gallery_waits_for_the_complete_download_before_ingest() {
        let (directory, application, _subscription) =
            fixture_for_site("ehentai", "e-hentai.org", "https://e-hentai.org/g/1/token/");
        application
            .store()
            .transaction(|transaction| {
                transaction.execute("UPDATE subscription_query SET group_posts = 1", [])?;
                Ok(())
            })
            .unwrap();
        let mut first = item_at(directory.path(), "gallery", "page-1", 1);
        first.post.site_id = "ehentai".to_string();
        first.input.source.as_mut().unwrap().site_id = "ehentai".to_string();
        first.input.source.as_mut().unwrap().post_complete = false;
        let mut second = item_at(directory.path(), "gallery", "page-2", 2);
        second.post.site_id = "ehentai".to_string();
        second.input.source.as_mut().unwrap().site_id = "ehentai".to_string();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![first, second],
                result: Ok(RunnerSuccess::default()),
            }])),
        };

        SubscriptionWorker::new(&application, runner)
            .tick(FIRST_NOW)
            .await
            .unwrap()
            .unwrap();
        drain_maintenance_ingest(&application);

        application
            .store()
            .read(|connection| {
                let roots = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE lifecycle = 'inbox'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                let members =
                    connection.query_row("SELECT COUNT(*) FROM collection_member", [], |row| {
                        row.get::<_, i64>(0)
                    })?;
                let succeeded = connection.query_row(
                    "SELECT COUNT(*) FROM ingest_job WHERE status = 'succeeded'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                let failed = connection.query_row(
                    "SELECT COUNT(*) FROM ingest_job WHERE status = 'failed'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                let (status, error): (String, Option<String>) = connection.query_row(
                    "SELECT status, error_message FROM subscription_run_query",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(
                    (roots, members, succeeded, failed, status, error),
                    (1, 2, 2, 0, "succeeded".to_string(), None)
                );
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn leaves_subscription_work_pending_while_inbox_is_full() {
        let (_directory, application, _subscription) = fixture();
        application
            .patch_application_settings(&serde_json::json!({
                "subscriptionInboxItemLimit": 1
            }))
            .unwrap();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (1, 'inbox-item', 'collection', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (1, 'inbox')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let worker = SubscriptionWorker::new(&application, FakeRunner::default());

        assert!(worker.tick(FIRST_NOW).await.unwrap().is_none());
        let state: (String, Option<String>, String, Option<String>) = application
            .store()
            .read(|connection| {
                let run = connection.query_row(
                    "SELECT status, failure_kind FROM subscription_run",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                let query = connection.query_row(
                    "SELECT status, failure_kind FROM subscription_run_query",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                Ok((run.0, run.1, query.0, query.1))
            })
            .unwrap();
        assert_eq!(
            state,
            (
                "pending".into(),
                Some("inbox_full".into()),
                "pending".into(),
                Some("inbox_full".into()),
            )
        );
    }

    #[tokio::test]
    async fn traversed_post_without_media_is_persisted_without_an_ingest_job() {
        let (_directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: vec![empty_post("locked-post")],
                items: Vec::new(),
                result: Ok(RunnerSuccess::default()),
            }])),
        };

        SubscriptionWorker::new(&application, runner)
            .tick(FIRST_NOW)
            .await
            .unwrap();

        let counts: (i64, i64, i64) = application
            .store()
            .read(|connection| {
                Ok((
                    connection
                        .query_row("SELECT COUNT(*) FROM source_post", [], |row| row.get(0))?,
                    connection.query_row(
                        "SELECT COUNT(*) FROM subscription_source_post",
                        [],
                        |row| row.get(0),
                    )?,
                    connection
                        .query_row("SELECT COUNT(*) FROM ingest_job", [], |row| row.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(counts, (1, 1, 0));
    }

    #[tokio::test]
    async fn one_processable_post_file_creates_a_standalone_root() {
        let (directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![item(directory.path(), "single-post", "only-file")],
                result: Ok(RunnerSuccess::default()),
            }])),
        };

        SubscriptionWorker::new(&application, runner)
            .tick(FIRST_NOW)
            .await
            .unwrap();
        drain_maintenance_ingest(&application);

        let (kind, members): (String, i64) = application
            .store()
            .read(|connection| {
                let kind = connection.query_row(
                    "SELECT li.kind FROM library_root lr JOIN library_item li ON li.item_id = lr.item_id",
                    [],
                    |row| row.get(0),
                )?;
                let members = connection.query_row(
                    "SELECT COUNT(*) FROM collection_member",
                    [],
                    |row| row.get(0),
                )?;
                Ok((kind, members))
            })
            .unwrap();
        assert_eq!((kind.as_str(), members), ("media", 0));
    }

    #[tokio::test]
    async fn destination_policy_is_applied_before_subscription_ingest() {
        let (directory, application, subscription) = fixture();
        let (folder_id, second_folder_id) = application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder (folder_key, name, created_at, updated_at)
                     VALUES ('subscription-folder', 'Subscription folder', 'now', 'now')",
                    [],
                )?;
                let first = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO folder (folder_key, name, created_at, updated_at)
                     VALUES ('subscription-folder-2', 'Second subscription folder', 'now', 'now')",
                    [],
                )?;
                Ok((first, transaction.last_insert_rowid()))
            })
            .unwrap()
            .0;
        application
            .set_subscription_destination(
                subscription,
                &crate::subscription_catalog_v2::SubscriptionDestinationPolicy {
                    target_folder_ids: vec![folder_id, second_folder_id],
                    target_folder_id: None,
                    automatic_tags: vec!["creator:alice".into()],
                },
            )
            .unwrap();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![item(directory.path(), "post-a", "item-a")],
                result: Ok(RunnerSuccess::default()),
            }])),
        };

        SubscriptionWorker::new(&application, runner)
            .tick(FIRST_NOW)
            .await
            .unwrap();
        drain_maintenance_ingest(&application);

        application
            .store()
            .read(|connection| {
                let folder_members: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id IN (?1, ?2)",
                    [folder_id, second_folder_id],
                    |row| row.get(0),
                )?;
                let tags: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM root_tag rt
                     JOIN tag t ON t.tag_id = rt.tag_id
                     WHERE t.namespace = 'creator' AND t.subtag = 'alice'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!((folder_members, tags), (2, 1));
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_reaches_the_source_and_persists_retry_state() {
        let (_directory, application, _subscription) = fixture();
        let cancel = CancellationToken::new();
        let worker = SubscriptionWorker::with_cancellation(
            &application,
            CancellationAwareRunner,
            cancel.clone(),
        );
        cancel.cancel();

        worker.tick(FIRST_NOW).await.unwrap();
        let state: (String, i64, Option<String>) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT status, attempt_count, failure_kind FROM subscription_run_query",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(state, ("pending".to_string(), 0, None));
    }

    #[tokio::test]
    async fn persisted_stop_cancels_the_in_flight_source() {
        let (_directory, application, subscription_id) = fixture();
        let worker = SubscriptionWorker::new(
            &application,
            PersistedCancellationAwareRunner {
                library_root: application.store().library_root().to_path_buf(),
                subscription_id,
            },
        );

        worker.tick(FIRST_NOW).await.unwrap();

        let states: (String, String) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT r.status, qr.status
                     FROM subscription_run r
                     JOIN subscription_run_query qr ON qr.run_id = r.run_id",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(states, ("cancelled".to_string(), "cancelled".to_string()));
    }

    #[tokio::test]
    async fn retryable_runner_failure_preserves_streamed_items_and_retries() {
        let (directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([
                FakeRun {
                    posts: Vec::new(),
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
                    posts: Vec::new(),
                    items: Vec::new(),
                    result: Ok(RunnerSuccess::default()),
                },
            ])),
        };
        let worker = SubscriptionWorker::new(&application, runner);

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

        // This worker intentionally keeps a real-time process-local domain
        // cooldown. Use a future wall-clock value rather than the fixture's
        // historical timestamp for the second claim.
        worker.tick("2100-01-01T00:00:00Z").await.unwrap().unwrap();
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
    async fn deleted_source_item_is_skipped_without_failing_the_run() {
        let (directory, application, _subscription) = fixture();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('example', 'deleted-post', ?1, ?1)",
                    [FIRST_NOW],
                )?;
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, state, created_at, updated_at
                     ) VALUES (?1, 'deleted-item', 0, 'deleted', ?2, ?2)",
                    params![transaction.last_insert_rowid(), FIRST_NOW],
                )?;
                Ok(())
            })
            .unwrap();

        let mut deleted = item(directory.path(), "deleted-post", "deleted-item");
        deleted.delete_after_ingest = true;
        let deleted_path = deleted.source_path.clone();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![deleted, item(directory.path(), "live-post", "live-item")],
                result: Ok(RunnerSuccess::default()),
            }])),
        };
        let worker = SubscriptionWorker::new(&application, runner);

        worker.tick(FIRST_NOW).await.unwrap().unwrap();
        drain_maintenance_ingest(&application);

        assert!(!deleted_path.exists());
        let state: (String, i64, i64, i64) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT
                         qr.status,
                         SUM(CASE WHEN si.state = 'deleted' THEN 1 ELSE 0 END),
                         SUM(CASE WHEN si.state = 'ingested' THEN 1 ELSE 0 END),
                         (SELECT COUNT(*) FROM subscription_issue WHERE status = 'open')
                     FROM subscription_run_query qr
                     JOIN subscription_run_source_item rsi
                       ON rsi.run_query_id = qr.run_query_id
                     JOIN source_item si ON si.source_item_id = rsi.source_item_id
                     GROUP BY qr.run_query_id",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            })
            .unwrap();
        assert_eq!(state, ("succeeded".to_string(), 1, 1, 0));
    }

    #[tokio::test]
    async fn streamed_multi_item_post_promotes_to_one_inbox_collection() {
        let (directory, application, _subscription) = fixture();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![
                    item_at(directory.path(), "post", "page-1", 0),
                    item_at(directory.path(), "post", "page-2", 1),
                ],
                result: Ok(RunnerSuccess::default()),
            }])),
        };
        let worker = SubscriptionWorker::new(&application, runner);
        worker.tick(FIRST_NOW).await.unwrap().unwrap();
        drain_maintenance_ingest(&application);

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
    async fn ungrouped_query_streams_multi_item_post_as_independent_roots() {
        let (directory, application, _subscription) = fixture();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute("UPDATE subscription_query SET group_posts = 0", [])?;
                Ok(())
            })
            .unwrap();
        let runner = FakeRunner {
            runs: Mutex::new(VecDeque::from([FakeRun {
                posts: Vec::new(),
                items: vec![
                    item_at(directory.path(), "post", "page-1", 0),
                    item_at(directory.path(), "post", "page-2", 1),
                ],
                result: Ok(RunnerSuccess::default()),
            }])),
        };

        SubscriptionWorker::new(&application, runner)
            .tick(FIRST_NOW)
            .await
            .unwrap();
        drain_maintenance_ingest(&application);

        application
            .store()
            .read(|connection| {
                let roots: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))?;
                let members: i64 =
                    connection.query_row("SELECT COUNT(*) FROM collection_member", [], |row| {
                        row.get(0)
                    })?;
                assert_eq!(roots, 2);
                assert_eq!(members, 0);
                Ok(())
            })
            .unwrap();
        assert_eq!(application.projections().inbox_bitmap().len(), 2);
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
                posts: Vec::new(),
                items: vec![failed_item],
                result: Ok(RunnerSuccess::default()),
            }])),
        };
        let worker = SubscriptionWorker::new(&application, runner);

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
