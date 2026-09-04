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

use chrono::Utc;
use rusqlite::OptionalExtension;
use tokio::sync::mpsc::{self, Sender};
use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;
use crate::library_subscription_state as state;
use crate::media_capabilities::ThumbnailBackend;
use crate::subscriptions::{ClaimedQueryRun, DomainSchedule, NormalizedPost};
use picto_library::{
    MutationReceipt, PreparedCollectionImport, PreparedImport, PreparedIngestJob,
    PreparedIngestPayload,
};
use picto_sources::SourcePostOutcome;

const CHANNEL_CAPACITY: usize = 32;
const RUN_STATE_POLL: std::time::Duration = std::time::Duration::from_millis(250);
const RUNNER_CANCEL_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub type RunnerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RunnerSuccess, RunnerFailure>> + Send + 'a>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunnerSuccess {
    pub resume_cursor: Option<String>,
    pub cleanup_paths: Vec<PathBuf>,
    /// Stop this run at the committed cursor even if its settled-post target was
    /// not reached (for example after the bounded no-media/duplicate scan).
    pub stop_after_current_execution: bool,
}

/// A source-normalized item that is ready for durable ingest.
#[derive(Debug, Clone)]
pub struct DownloadedItem {
    pub post: NormalizedPost,
    pub input: PreparedImport,
    pub post_complete: bool,
    pub force_collection: bool,
    pub delete_after_ingest: bool,
}

#[derive(Debug, Clone)]
pub struct FailedMediaItem {
    pub post: NormalizedPost,
    pub item_key: String,
    pub error_message: String,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SourceEvent {
    PostTraversed(NormalizedPost),
    MediaDownloaded(DownloadedItem),
    MediaFailed(FailedMediaItem),
    PostComplete {
        post_key: String,
        acknowledge: tokio::sync::oneshot::Sender<SourcePostOutcome>,
    },
    CursorCheckpoint {
        resume_cursor: String,
        acknowledge: tokio::sync::oneshot::Sender<()>,
    },
}

/// Failure returned by a source runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerFailure {
    pub kind: RunnerFailureKind,
    pub message: String,
    pub retryable: bool,
    pub cleanup_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerFailureKind {
    Interrupted,
    InboxFull,
    Network,
    RateLimited,
    Authentication,
    AccessDenied,
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
            cleanup_paths: Vec::new(),
        }
    }

    pub fn terminal(kind: RunnerFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
            cleanup_paths: Vec::new(),
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
            Self::AccessDenied => "access",
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
    application: &'a LibraryApplication,
    runner: R,
    schedule: Arc<Mutex<DomainSchedule>>,
    cancel: CancellationToken,
}

impl<'a, R: SourceRunner> SubscriptionWorker<'a, R> {
    pub fn new(application: &'a LibraryApplication, runner: R) -> Self {
        Self::with_cancellation(application, runner, CancellationToken::new())
    }

    pub fn with_cancellation(
        application: &'a LibraryApplication,
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
        application: &'a LibraryApplication,
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
        let inbox_limit = crate::settings::subscription_inbox_item_limit_library(self.application)?;
        let inbox_full = crate::settings::subscription_inbox_is_full_library(self.application)?;
        state::set_inbox_wait_state(self.application, inbox_full, inbox_limit)?;
        if inbox_full {
            return Ok(None);
        }
        let query = {
            let mut schedule = self
                .schedule
                .lock()
                .map_err(|_| "subscription domain schedule lock is poisoned".to_string())?;
            state::claim_next_query(self.application, &mut schedule, now)?
        };
        let Some(query) = query else {
            return Ok(None);
        };
        let domain_key = query.domain_key.clone();
        let result = run_claimed_query(self.application, &self.runner, &self.cancel, query).await;
        self.schedule
            .lock()
            .map_err(|_| "subscription domain schedule lock is poisoned".to_string())?
            .mark_finished(domain_key, Utc::now().timestamp_millis());
        result
    }
}

/// Claims and runs at most one query.
pub async fn tick<R: SourceRunner>(
    application: &LibraryApplication,
    schedule: &mut DomainSchedule,
    runner: &R,
    now: &str,
) -> Result<Option<MutationReceipt>, String> {
    let inbox_limit = crate::settings::subscription_inbox_item_limit_library(application)?;
    let inbox_full = crate::settings::subscription_inbox_is_full_library(application)?;
    state::set_inbox_wait_state(application, inbox_full, inbox_limit)?;
    if inbox_full {
        return Ok(None);
    }
    let Some(query) = state::claim_next_query(application, schedule, now)? else {
        return Ok(None);
    };
    run_claimed_query(application, runner, &CancellationToken::new(), query).await
}

async fn run_claimed_query<R: SourceRunner>(
    application: &LibraryApplication,
    runner: &R,
    cancel: &CancellationToken,
    query: ClaimedQueryRun,
) -> Result<Option<MutationReceipt>, String> {
    let runner_result = run_stream(application, &query, runner, cancel).await;
    let settled_at = Utc::now().to_rfc3339();
    match runner_result {
        Ok(Ok(success)) => {
            if let Err(failure) = wait_for_query_ingest(application, &query, cancel).await {
                settle_runner_failure(application, &query, failure, &settled_at)?;
                return Ok(None);
            }
            let completion = if success.stop_after_current_execution {
                state::complete_query_terminal(
                    application,
                    &query,
                    success.resume_cursor.as_deref(),
                    &settled_at,
                )
            } else {
                state::complete_query(
                    application,
                    &query,
                    success.resume_cursor.as_deref(),
                    &settled_at,
                )
            };
            match completion {
                Ok(receipt) => receipt,
                Err(error) => {
                    settle_runner_failure(
                        application,
                        &query,
                        RunnerFailure::retryable(
                            RunnerFailureKind::Runtime,
                            format!("settling completed source query failed: {error}"),
                        ),
                        &settled_at,
                    )?;
                    return Ok(None);
                }
            };
            if let Err(error) =
                state::mark_credential_success(application, &query.site_id, &settled_at)
            {
                tracing::warn!(site_id = %query.site_id, error = %error, "Failed to record credential health");
            }
        }
        Ok(Err(failure)) => {
            settle_runner_failure(application, &query, failure, &settled_at)?;
        }
        Err(error) => {
            settle_runner_failure(
                application,
                &query,
                RunnerFailure::terminal(RunnerFailureKind::Runtime, error),
                &settled_at,
            )?;
        }
    }

    Ok(None)
}

async fn wait_for_query_ingest(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    cancel: &CancellationToken,
) -> Result<(), RunnerFailure> {
    loop {
        crate::library_ingest_runtime::run_batch(application, 64)
            .map_err(|error| RunnerFailure::retryable(RunnerFailureKind::Runtime, error))?;
        match state::query_ingest_settlement(application, query.run_query_id)
            .map_err(|error| RunnerFailure::retryable(RunnerFailureKind::Runtime, error))?
        {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) => {
                return Err(RunnerFailure::retryable(RunnerFailureKind::Runtime, error));
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => {
                return Err(RunnerFailure::retryable(
                    RunnerFailureKind::Interrupted,
                    "Subscription run interrupted while ingesting the current post",
                ));
            }
            _ = tokio::time::sleep(RUN_STATE_POLL) => {}
        }
    }
}

async fn run_stream<R: SourceRunner>(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    runner: &R,
    cancel: &CancellationToken,
) -> Result<Result<RunnerSuccess, RunnerFailure>, String> {
    let destination = crate::subscription_catalog::subscription_destination_library(
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
    let mut grouped_items = std::collections::BTreeMap::<String, Vec<DownloadedItem>>::new();
    let mut atomic_items = Vec::new();
    let mut recorded_source_items = BTreeSet::new();

    let runner_result = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                runner_cancel.cancel();
                let mut failure = RunnerFailure::retryable(
                    RunnerFailureKind::Interrupted,
                    "Subscription run interrupted",
                );
                collect_cancelled_runner_cleanup(&mut failure, &mut runner_future).await;
                break Err(failure);
            }
            _ = state_poll.tick() => {
                if crate::settings::subscription_inbox_is_full_library(application)? {
                    runner_cancel.cancel();
                    let mut failure = RunnerFailure::retryable(
                        RunnerFailureKind::InboxFull,
                        "Inbox reached its configured subscription limit",
                    );
                    collect_cancelled_runner_cleanup(&mut failure, &mut runner_future).await;
                    break Err(failure);
                }
                if application.library().auxiliary_read(
                    picto_library::database::WorkPriority::VisibleRead,
                    |connection| {
                        Ok(crate::subscription_catalog::subscriptions_globally_paused(
                            connection,
                        )?)
                    },
                ).map_err(|error| error.to_string())? {
                    runner_cancel.cancel();
                    let mut failure = RunnerFailure::retryable(
                        RunnerFailureKind::Interrupted,
                        "Subscriptions globally paused",
                    );
                    collect_cancelled_runner_cleanup(&mut failure, &mut runner_future).await;
                    break Err(failure);
                }
                if !state::query_is_running(application, query.run_query_id)? {
                    tracing::warn!(
                        run_query_id = query.run_query_id,
                        "source query row left the running state mid-run"
                    );
                    runner_cancel.cancel();
                    let mut failure = RunnerFailure::retryable(
                        RunnerFailureKind::Interrupted,
                        "Subscription run stopped",
                    );
                    collect_cancelled_runner_cleanup(&mut failure, &mut runner_future).await;
                    break Err(failure);
                }
            }
            result = &mut runner_future => break result,
            event = input.recv(), if input_open => match event {
                Some(event) => handle_source_event(
                    application,
                    query,
                    &runner_cancel,
                    &destination,
                    event,
                    atomic_gallery,
                    &mut grouped_items,
                    &mut atomic_items,
                    &mut recorded_source_items,
                ).await?,
                None => input_open = false,
            },
        }
    };

    let accept_remaining_events =
        !cancel.is_cancelled() && state::query_is_running(application, query.run_query_id)?;
    if accept_remaining_events {
        while let Some(event) = input.recv().await {
            handle_source_event(
                application,
                query,
                &runner_cancel,
                &destination,
                event,
                atomic_gallery,
                &mut grouped_items,
                &mut atomic_items,
                &mut recorded_source_items,
            )
            .await?;
        }
    } else {
        input.close();
    }

    cleanup_runner_paths(&runner_result).await;

    if atomic_gallery && !atomic_items.is_empty() && runner_result.is_ok() {
        return Ok(Err(RunnerFailure::retryable(
            RunnerFailureKind::InvalidOutput,
            "Gallery source ended before canonical ingestion completed",
        )));
    }
    if runner_result.is_ok() && !grouped_items.is_empty() {
        return Ok(Err(RunnerFailure::retryable(
            RunnerFailureKind::InvalidOutput,
            "Source run ended before a grouped post completed",
        )));
    }
    Ok(runner_result)
}

async fn collect_cancelled_runner_cleanup<F>(
    failure: &mut RunnerFailure,
    runner_future: &mut std::pin::Pin<&mut F>,
) where
    F: std::future::Future<Output = Result<RunnerSuccess, RunnerFailure>>,
{
    if let Ok(result) = tokio::time::timeout(RUNNER_CANCEL_ACK_TIMEOUT, runner_future).await {
        match result {
            Ok(success) => failure.cleanup_paths.extend(success.cleanup_paths),
            Err(cancelled) => failure.cleanup_paths.extend(cancelled.cleanup_paths),
        }
    }
}

async fn cleanup_runner_paths(result: &Result<RunnerSuccess, RunnerFailure>) {
    let paths = match result {
        Ok(success) => &success.cleanup_paths,
        Err(failure) => &failure.cleanup_paths,
    };
    for path in paths {
        if let Err(error) = tokio::fs::remove_dir_all(path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %path.display(), %error, "Could not clean source staging");
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_source_event(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    cancel: &CancellationToken,
    destination: &crate::subscription_catalog::SubscriptionDestinationPolicy,
    event: SourceEvent,
    atomic_gallery: bool,
    grouped_items: &mut std::collections::BTreeMap<String, Vec<DownloadedItem>>,
    atomic_items: &mut Vec<DownloadedItem>,
    recorded_source_items: &mut BTreeSet<(String, String)>,
) -> Result<(), String> {
    match event {
        SourceEvent::CursorCheckpoint {
            resume_cursor,
            acknowledge,
        } => {
            state::checkpoint_query_cursor(application, query, &resume_cursor)?;
            let _ = acknowledge.send(());
        }
        SourceEvent::PostComplete {
            post_key,
            acknowledge,
        } if atomic_gallery => {
            if atomic_items.is_empty() {
                wait_for_query_ingest(application, query, cancel)
                    .await
                    .map_err(|failure| failure.message)?;
                let outcome = state::settled_post_outcome(application, query, &post_key)?;
                let _ = acknowledge.send(outcome);
                return Ok(());
            }
            validate_complete_gallery(atomic_items)?;
            let items = std::mem::take(atomic_items);
            if items[0].post.post_key != post_key {
                return Err("Gallery settlement identity does not match its media".into());
            }
            let gallery_post = items[0].post.clone();
            state::record_post(
                application,
                query.run_query_id,
                &gallery_post,
                &Utc::now().to_rfc3339(),
            )?;
            enqueue_group(application, query, destination, items).await?;
            wait_for_query_ingest(application, query, cancel)
                .await
                .map_err(|failure| failure.message)?;
            let outcome = state::settled_post_outcome(application, query, &post_key)?;
            let _ = acknowledge.send(outcome);
        }
        SourceEvent::PostComplete {
            post_key,
            acknowledge,
        } => {
            if let Some(items) = grouped_items.remove(&post_key) {
                enqueue_group(application, query, destination, items).await?;
            }
            wait_for_query_ingest(application, query, cancel)
                .await
                .map_err(|failure| failure.message)?;
            let outcome = state::settled_post_outcome(application, query, &post_key)?;
            let _ = acknowledge.send(outcome);
        }
        SourceEvent::PostTraversed(post) if atomic_gallery => {
            state::record_post(
                application,
                query.run_query_id,
                &post,
                &Utc::now().to_rfc3339(),
            )?;
        }
        SourceEvent::MediaDownloaded(mut item) if atomic_gallery => {
            let source_item_id =
                ensure_source_item_recorded(application, query, &item, recorded_source_items)?;
            attach_source_attempt(application, query, &mut item)?;
            persist_downloaded_media(application, std::slice::from_mut(&mut item))?;
            state::mark_source_item_staged(
                application,
                query.run_query_id,
                source_item_id,
                &item.input.facts.content_hash,
                &item.input.file_path,
                item.input.facts.size_bytes,
                &Utc::now().to_rfc3339(),
            )?;
            atomic_items.push(item);
        }
        SourceEvent::MediaFailed(item) if atomic_gallery => {
            let ids = state::record_post(
                application,
                query.run_query_id,
                &item.post,
                &Utc::now().to_rfc3339(),
            )?;
            let source_item_id = ids
                .get(&item.item_key)
                .copied()
                .ok_or_else(|| "Failed gallery media was not recorded".to_string())?;
            let failure = item.error_message;
            state::mark_source_item_failed(
                application,
                query.run_query_id,
                query.subscription_id,
                query.query_id,
                source_item_id,
                &failure,
                &Utc::now().to_rfc3339(),
            )?;
            recorded_source_items.insert((item.post.post_key, item.item_key));
            return Err(format!("Gallery download failed: {failure}"));
        }
        SourceEvent::PostTraversed(post) => {
            state::record_post(
                application,
                query.run_query_id,
                &post,
                &Utc::now().to_rfc3339(),
            )?;
            recorded_source_items.extend(
                post.items
                    .iter()
                    .map(|item| (post.post_key.clone(), item.item_key.clone())),
            );
        }
        SourceEvent::MediaDownloaded(mut item) => {
            let source_item_id =
                ensure_source_item_recorded(application, query, &item, recorded_source_items)?;
            attach_source_attempt(application, query, &mut item)?;
            persist_downloaded_media(application, std::slice::from_mut(&mut item))?;
            state::mark_source_item_staged(
                application,
                query.run_query_id,
                source_item_id,
                &item.input.facts.content_hash,
                &item.input.file_path,
                item.input.facts.size_bytes,
                &Utc::now().to_rfc3339(),
            )?;
            if query.group_posts || item.force_collection {
                let post_key = item.post.post_key.clone();
                let complete = item.post_complete;
                grouped_items
                    .entry(post_key.clone())
                    .or_default()
                    .push(item);
                if complete {
                    let items = grouped_items
                        .remove(&post_key)
                        .expect("completed source group exists");
                    enqueue_group(application, query, destination, items).await?;
                }
            } else {
                enqueue_group(application, query, destination, vec![item]).await?;
            }
        }
        SourceEvent::MediaFailed(item) => {
            let ids = state::record_post(
                application,
                query.run_query_id,
                &item.post,
                &Utc::now().to_rfc3339(),
            )?;
            let source_item_id = ids
                .get(&item.item_key)
                .copied()
                .ok_or_else(|| "Failed subscription media was not recorded".to_string())?;
            state::mark_source_item_failed(
                application,
                query.run_query_id,
                query.subscription_id,
                query.query_id,
                source_item_id,
                &item.error_message,
                &Utc::now().to_rfc3339(),
            )?;
            recorded_source_items.insert((item.post.post_key, item.item_key));
        }
    }
    Ok(())
}

fn ensure_source_item_recorded(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    item: &DownloadedItem,
    recorded_source_items: &mut BTreeSet<(String, String)>,
) -> Result<i64, String> {
    let source = item
        .input
        .source_identity
        .as_ref()
        .ok_or_else(|| "A subscription item needs source identity".to_string())?;
    let identity = (item.post.post_key.clone(), source.source_item_key.clone());
    if recorded_source_items.contains(&identity) {
        return state::source_item_id(
            application,
            &item.post.site_id,
            &item.post.post_key,
            &source.source_item_key,
        );
    }
    state::record_post(
        application,
        query.run_query_id,
        &item.post,
        &Utc::now().to_rfc3339(),
    )?;
    recorded_source_items.extend(
        item.post
            .items
            .iter()
            .map(|post_item| (item.post.post_key.clone(), post_item.item_key.clone())),
    );
    state::source_item_id(
        application,
        &item.post.site_id,
        &item.post.post_key,
        &source.source_item_key,
    )
}

fn attach_source_attempt(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    item: &mut DownloadedItem,
) -> Result<(), String> {
    let attempt_id =
        state::source_attempt_id(application, query.run_query_id, &item.post.post_key)?;
    item.input
        .source_identity
        .as_mut()
        .ok_or_else(|| "A subscription item needs source identity".to_string())?
        .source_attempt_id = Some(attempt_id);
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
    let expected = first
        .post
        .items
        .iter()
        .map(|item| item.item_key.as_str())
        .collect::<BTreeSet<_>>();
    if expected.len() != first.post.items.len() {
        return Err("Gallery post contains duplicate media identities".into());
    }
    let actual = items
        .iter()
        .map(|item| {
            item.input
                .source_identity
                .as_ref()
                .map(|source| source.source_item_key.as_str())
                .ok_or_else(|| "Gallery media is missing source identity".to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual.len() != items.len() || actual != expected {
        return Err(format!(
            "Gallery download was incomplete: received {} of {} declared media files",
            actual.len(),
            expected.len()
        ));
    }
    Ok(())
}

async fn enqueue_group(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    destination: &crate::subscription_catalog::SubscriptionDestinationPolicy,
    mut items: Vec<DownloadedItem>,
) -> Result<(), String> {
    if items.is_empty() {
        return Err("A completed source post has no media".into());
    }
    let source_positions = items[0]
        .post
        .items
        .iter()
        .map(|item| (item.item_key.clone(), item.position))
        .collect::<std::collections::BTreeMap<_, _>>();
    items.sort_by_key(|item| {
        item.input
            .source_identity
            .as_ref()
            .and_then(|source| source_positions.get(&source.source_item_key))
            .copied()
            .unwrap_or(i64::MAX)
    });
    ensure_subscription_thumbnails(application, &items).await?;
    let post = items[0].post.clone();
    let existing_root = source_post_root(application, &post.site_id, &post.post_key)?;
    let folders = destination
        .target_folder_ids
        .iter()
        .map(|value| {
            u32::try_from(*value)
                .map(picto_library::FolderId)
                .map_err(|_| format!("Invalid destination folder ID: {value}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut source_item_ids = Vec::with_capacity(items.len());
    for item in &mut items {
        let source = item
            .input
            .source_identity
            .as_ref()
            .ok_or_else(|| "A subscription item needs source identity".to_string())?;
        if !item
            .post
            .items
            .iter()
            .any(|post_item| post_item.item_key == source.source_item_key)
        {
            return Err("Downloaded item identity does not match its normalized post".into());
        }
        item.input.folders = folders.clone();
        if let Some((_, lifecycle)) = existing_root {
            item.input.lifecycle = lifecycle;
        }
        for tag in &destination.automatic_tags {
            if !item.input.tags.contains(tag) {
                item.input.tags.push(tag.clone());
            }
        }
        source_item_ids.push(state::source_item_id(
            application,
            &post.site_id,
            &post.post_key,
            &source.source_item_key,
        )?);
    }
    let source_path = items[0].input.file_path.clone();
    let payload = if items.len() == 1 && existing_root.is_none() {
        PreparedIngestPayload::Item(items.remove(0).input)
    } else {
        PreparedIngestPayload::Collection(PreparedCollectionImport {
            members: items.into_iter().map(|item| item.input).collect(),
            cover_index: 0,
            existing_root_id: existing_root.map(|(root_id, _)| root_id),
            name: post.title.clone(),
            modified_at_ms: Utc::now().timestamp_millis(),
        })
    };
    application
        .library()
        .enqueue_ingest_job(
            &PreparedIngestJob {
                job_key: format!(
                    "subscription:{}:{}:{}",
                    query.subscription_id, post.site_id, post.post_key
                ),
                source_kind: "subscription".into(),
                source_path,
                source_item_id: source_item_ids.first().copied(),
                delete_after_ingest: false,
                payload,
            },
            &Utc::now().to_rfc3339(),
        )
        .map_err(|error| format!("enqueueing subscription media failed: {error}"))?;
    Ok(())
}

fn source_post_root(
    application: &LibraryApplication,
    site_id: &str,
    post_key: &str,
) -> Result<Option<(picto_library::RootId, picto_library::Lifecycle)>, String> {
    let root_id = application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                connection
                    .query_row(
                        "SELECT post.root_item_id
                         FROM source_post post
                         JOIN library_root root ON root.root_id = post.root_item_id
                         WHERE post.site_id = ?1 AND post.post_key = ?2",
                        rusqlite::params![site_id, post_key],
                        |row| row.get::<_, u32>(0),
                    )
                    .optional()
                    .map_err(Into::into)
            },
        )
        .map_err(|error| error.to_string())?;
    let Some(root_id) = root_id else {
        return Ok(None);
    };
    let snapshot = application.library().projections().snapshot();
    let lifecycle = picto_library::Lifecycle::ALL
        .into_iter()
        .find(|lifecycle| snapshot.lifecycle(*lifecycle).contains(root_id))
        .ok_or_else(|| format!("source post root {root_id} has no lifecycle"))?;
    Ok(Some((picto_library::RootId(root_id), lifecycle)))
}

async fn ensure_subscription_thumbnails(
    application: &LibraryApplication,
    items: &[DownloadedItem],
) -> Result<(), String> {
    for item in items {
        if application
            .blobs()
            .find_thumbnail_path(&item.input.facts.content_hash)
            .map_err(|error| format!("Subscription thumbnail lookup failed: {error}"))?
            .is_some()
        {
            continue;
        }

        let mut source = crate::media_processing::PreparedMediaSource::from_stored_metadata(
            PathBuf::from(&item.input.file_path),
            &item.input.facts.mime,
            item.input
                .facts
                .duration_ms
                .and_then(|value| i64::try_from(value).ok()),
            item.input.facts.frame_count.map(i64::from),
        );
        if !source.caps.can_thumbnail()
            || source.caps.thumbnail_backend == Some(ThumbnailBackend::Inline)
        {
            continue;
        }
        let (bytes, extension) = source
            .render_thumbnail_bytes(crate::media_processing::DEFAULT_THUMBNAIL_DIMENSIONS, 35)
            .await
            .map_err(|error| {
                format!(
                    "Subscription thumbnail generation failed for `{}`: {error}",
                    item.input.media_name
                )
            })?;
        application
            .blobs()
            .write_thumbnail(&item.input.facts.content_hash, &bytes, &extension)
            .map_err(|error| format!("Subscription thumbnail write failed: {error}"))?;
    }
    Ok(())
}

fn persist_downloaded_media(
    application: &LibraryApplication,
    items: &mut [DownloadedItem],
) -> Result<(), String> {
    let mut owned_sources = Vec::new();
    for item in items {
        let source = PathBuf::from(&item.input.file_path);
        let extension = crate::blob_store::mime_to_extension(&item.input.facts.mime);
        application
            .blobs()
            .write_original_from_path(&item.input.facts.content_hash, &source, Some(extension))
            .map_err(|error| format!("persisting subscription media failed: {error}"))?;
        let stored = application
            .blobs()
            .original_path_with_ext(&item.input.facts.content_hash, Some(extension))
            .map_err(|error| format!("resolving persisted subscription media failed: {error}"))?;
        if item.delete_after_ingest && source != stored {
            owned_sources.push(source);
        }
        item.input.file_path = stored.to_string_lossy().into_owned();
    }
    for source in &owned_sources {
        if let Err(error) = std::fs::remove_file(source) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %source.display(), error = %error, "Could not clean persisted subscription download");
            }
        }
    }
    Ok(())
}

fn settle_runner_failure(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    failure: RunnerFailure,
    now: &str,
) -> Result<(), String> {
    if failure.kind == RunnerFailureKind::Interrupted {
        tracing::warn!(
            run_query_id = query.run_query_id,
            message = %failure.message,
            "source query interrupted; returning to pending"
        );
        return state::interrupt_query(application, query, now).map(|_| ());
    }
    let authentication_failed = failure.kind == RunnerFailureKind::Authentication;
    let failure_message = failure.message.clone();
    state::fail_query(
        application,
        query,
        &failure.kind.to_string(),
        &failure.message,
        failure.retryable,
        now,
    )?;
    if authentication_failed {
        if let Err(error) =
            state::mark_credential_failure(application, &query.site_id, now, &failure_message)
        {
            tracing::warn!(site_id = %query.site_id, error = %error, "Failed to record credential health");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use picto_library::{ImmutableMediaFacts, Lifecycle, Rating, SourceIdentity};
    use sha2::{Digest, Sha256};

    #[tokio::test]
    async fn failed_runner_cleanup_removes_its_download_directory() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("failed-run");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("partial-download"), b"partial").unwrap();
        let mut failure = RunnerFailure::retryable(RunnerFailureKind::Download, "failed");
        failure.cleanup_paths.push(path.clone());

        cleanup_runner_paths(&Err(failure)).await;

        assert!(!path.exists());
    }

    struct CancellationCleanupRunner {
        cleanup_path: PathBuf,
    }

    impl SourceRunner for CancellationCleanupRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            _output: Sender<SourceEvent>,
            cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                cancel.cancelled().await;
                let mut failure =
                    RunnerFailure::retryable(RunnerFailureKind::Interrupted, "cancelled");
                failure.cleanup_paths.push(self.cleanup_path.clone());
                Err(failure)
            })
        }
    }

    #[tokio::test]
    async fn cancellation_acknowledgement_cleans_runner_staging() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let cleanup_path = directory.path().join("native-staging");
        std::fs::create_dir_all(&cleanup_path).unwrap();
        std::fs::write(cleanup_path.join("partial"), b"partial").unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Example".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "twitter".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-30T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:00:00Z")
            .unwrap();

        let cancel = CancellationToken::new();
        cancel.cancel();
        let worker = SubscriptionWorker::with_cancellation(
            &application,
            CancellationCleanupRunner {
                cleanup_path: cleanup_path.clone(),
            },
            cancel,
        );
        worker.tick("2026-08-30T00:00:01Z").await.unwrap();

        assert!(!cleanup_path.exists());
    }

    #[tokio::test]
    async fn global_pause_interrupts_io_without_changing_subscription_holds() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let cleanup_path = directory.path().join("global-pause-staging");
        std::fs::create_dir_all(&cleanup_path).unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Global pause".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "twitter".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-30T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:00:00Z")
            .unwrap();

        let worker = SubscriptionWorker::new(
            &application,
            CancellationCleanupRunner {
                cleanup_path: cleanup_path.clone(),
            },
        );
        let pause = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            application.pause_all_subscriptions_library(true).unwrap();
        };
        let (result, ()) = tokio::join!(worker.tick("2026-08-30T00:00:01Z"), pause);
        result.unwrap();

        let paused = crate::subscription_catalog::list_library(&application).unwrap();
        assert!(paused.global_paused);
        assert!(!paused.subscriptions[0].paused);
        assert_eq!(paused.subscriptions[0].status.as_deref(), Some("pending"));
        assert!(!cleanup_path.exists());

        application.pause_all_subscriptions_library(false).unwrap();
        let resumed_at = (Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
        assert!(crate::library_subscription_state::claim_next_query(
            &application,
            &mut crate::subscriptions::DomainSchedule::new(),
            &resumed_at,
        )
        .unwrap()
        .is_some());
    }

    fn gallery_item(post: &NormalizedPost, item_key: &str) -> DownloadedItem {
        DownloadedItem {
            post: post.clone(),
            input: PreparedImport {
                stable_key: format!("source:ehentai:gallery:{item_key}"),
                media_name: item_key.into(),
                file_path: format!("/{item_key}.jpg"),
                facts: ImmutableMediaFacts {
                    mime: "image/jpeg".into(),
                    size_bytes: 1,
                    width: Some(1),
                    height: Some(1),
                    duration_ms: None,
                    frame_count: Some(1),
                    content_hash: format!("hash-{item_key}"),
                    perceptual_hash: None,
                    palette: Vec::new(),
                },
                lifecycle: Lifecycle::Inbox,
                rating: Rating::Unrated,
                notes: None,
                tags: Vec::new(),
                folders: Vec::new(),
                source_urls: Vec::new(),
                source_identity: Some(SourceIdentity {
                    source_key: "ehentai:gallery".into(),
                    source_item_key: item_key.into(),
                    source_text: None,
                    source_attempt_id: None,
                }),
                imported_at_ms: 1_700_000_000_000,
                captured_at_ms: None,
            },
            post_complete: false,
            force_collection: true,
            delete_after_ingest: true,
        }
    }

    #[test]
    fn native_gallery_completion_requires_the_exact_declared_media_set() {
        let post = NormalizedPost {
            site_id: "ehentai".into(),
            post_key: "gallery".into(),
            canonical_url: Some("https://e-hentai.org/g/1/0123456789/".into()),
            creator_name: None,
            title: Some("Gallery".into()),
            description: None,
            captured_at: None,
            metadata_json: None,
            items: vec![
                crate::subscriptions::NormalizedItem {
                    item_key: "page-1".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                },
                crate::subscriptions::NormalizedItem {
                    item_key: "page-2".into(),
                    position: 1,
                    media_url: None,
                    canonical_url: None,
                },
            ],
        };
        let complete = vec![gallery_item(&post, "page-2"), gallery_item(&post, "page-1")];
        assert!(validate_complete_gallery(&complete).is_ok());
        assert!(validate_complete_gallery(&complete[..1]).is_err());
        assert!(validate_complete_gallery(&[
            gallery_item(&post, "page-1"),
            gallery_item(&post, "page-1"),
        ])
        .is_err());
    }

    struct OneItemRunner<'a> {
        application: &'a LibraryApplication,
        item: DownloadedItem,
        cleanup_paths: Vec<PathBuf>,
        expected_state: &'static str,
    }

    impl SourceRunner for OneItemRunner<'_> {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            output: Sender<SourceEvent>,
            _cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                output
                    .send(SourceEvent::PostTraversed(self.item.post.clone()))
                    .await
                    .unwrap();
                output
                    .send(SourceEvent::MediaDownloaded(self.item.clone()))
                    .await
                    .unwrap();
                let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
                output
                    .send(SourceEvent::PostComplete {
                        post_key: self.item.post.post_key.clone(),
                        acknowledge,
                    })
                    .await
                    .unwrap();
                acknowledged.await.unwrap();
                let state = self
                    .application
                    .library()
                    .auxiliary_read(
                        picto_library::database::WorkPriority::VisibleRead,
                        |connection| {
                            Ok(connection.query_row(
                                "SELECT state FROM source_item WHERE item_key = 'media-1'",
                                [],
                                |row| row.get::<_, String>(0),
                            )?)
                        },
                    )
                    .unwrap();
                assert_eq!(
                    state, self.expected_state,
                    "post was acknowledged before canonical settlement"
                );
                Ok(RunnerSuccess {
                    resume_cursor: Some(String::new()),
                    cleanup_paths: self.cleanup_paths.clone(),
                    stop_after_current_execution: false,
                })
            })
        }
    }

    struct MultiItemRunner<'a> {
        application: &'a LibraryApplication,
        items: Vec<DownloadedItem>,
        cleanup_path: PathBuf,
        expected_source_items: i64,
    }

    impl SourceRunner for MultiItemRunner<'_> {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            output: Sender<SourceEvent>,
            _cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                output
                    .send(SourceEvent::PostTraversed(self.items[0].post.clone()))
                    .await
                    .unwrap();
                for item in &self.items {
                    output
                        .send(SourceEvent::MediaDownloaded(item.clone()))
                        .await
                        .unwrap();
                }
                let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
                output
                    .send(SourceEvent::PostComplete {
                        post_key: self.items[0].post.post_key.clone(),
                        acknowledge,
                    })
                    .await
                    .unwrap();
                acknowledged.await.unwrap();
                let ingested = self
                    .application
                    .library()
                    .auxiliary_read(
                        picto_library::database::WorkPriority::VisibleRead,
                        |connection| {
                            Ok(connection.query_row(
                                "SELECT COUNT(*) FROM source_item WHERE state = 'ingested'",
                                [],
                                |row| row.get::<_, i64>(0),
                            )?)
                        },
                    )
                    .unwrap();
                assert_eq!(ingested, self.expected_source_items);
                Ok(RunnerSuccess {
                    resume_cursor: Some(String::new()),
                    cleanup_paths: vec![self.cleanup_path.clone()],
                    stop_after_current_execution: false,
                })
            })
        }
    }

    struct KnownPostRunner {
        post: NormalizedPost,
    }

    impl SourceRunner for KnownPostRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            output: Sender<SourceEvent>,
            _cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                output
                    .send(SourceEvent::PostTraversed(self.post.clone()))
                    .await
                    .unwrap();
                let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
                output
                    .send(SourceEvent::PostComplete {
                        post_key: self.post.post_key.clone(),
                        acknowledge,
                    })
                    .await
                    .unwrap();
                assert_eq!(
                    acknowledged.await.unwrap(),
                    SourcePostOutcome::Skipped {
                        reason: picto_sources::SkipReason::ExactDuplicate,
                    }
                );
                Ok(RunnerSuccess {
                    resume_cursor: Some(String::new()),
                    cleanup_paths: Vec::new(),
                    stop_after_current_execution: false,
                })
            })
        }
    }

    struct FailedItemRunner {
        post: NormalizedPost,
    }

    impl SourceRunner for FailedItemRunner {
        fn run<'a>(
            &'a self,
            _query: &'a ClaimedQueryRun,
            output: Sender<SourceEvent>,
            _cancel: CancellationToken,
        ) -> RunnerFuture<'a> {
            Box::pin(async move {
                output
                    .send(SourceEvent::PostTraversed(self.post.clone()))
                    .await
                    .unwrap();
                output
                    .send(SourceEvent::MediaFailed(FailedMediaItem {
                        post: self.post.clone(),
                        item_key: "media-404".into(),
                        error_message: "404 Not Found".into(),
                    }))
                    .await
                    .unwrap();
                let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
                output
                    .send(SourceEvent::PostComplete {
                        post_key: self.post.post_key.clone(),
                        acknowledge,
                    })
                    .await
                    .unwrap();
                acknowledged.await.unwrap();
                Ok(RunnerSuccess {
                    resume_cursor: Some(String::new()),
                    cleanup_paths: Vec::new(),
                    stop_after_current_execution: false,
                })
            })
        }
    }

    #[test]
    fn global_pause_stops_manual_query_claims_until_resumed() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Example".into(),
            schedule: "manual".into(),
            initial_post_limit: None,
            periodic_post_limit: None,
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "twitter".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        let query_id = crate::subscription_catalog::list_library(&application)
            .unwrap()
            .subscriptions[0]
            .queries[0]
            .query_id;
        application
            .request_subscription_query_run_library(query_id, "2026-08-29T00:00:00Z")
            .unwrap();
        application.pause_all_subscriptions_library(true).unwrap();

        let mut schedule = crate::subscriptions::DomainSchedule::new();
        assert!(crate::library_subscription_state::claim_next_query(
            &application,
            &mut schedule,
            "2026-08-29T00:00:01Z",
        )
        .unwrap()
        .is_none());

        let paused = crate::subscription_catalog::list_library(&application).unwrap();
        assert!(paused.global_paused);
        assert!(!paused.subscriptions[0].paused);
        assert_eq!(paused.subscriptions[0].status.as_deref(), Some("pending"));

        application.pause_all_subscriptions_library(false).unwrap();
        assert!(
            !crate::subscription_catalog::list_library(&application)
                .unwrap()
                .global_paused
        );
        let claimed = crate::library_subscription_state::claim_next_query(
            &application,
            &mut schedule,
            "2026-08-29T00:00:02Z",
        )
        .unwrap()
        .expect("resumed manual query should become claimable");
        assert_eq!(claimed.subscription_id, subscription_id);
    }

    #[test]
    fn paused_run_resumes_but_stopped_run_is_terminal() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Lifecycle".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(2),
            periodic_post_limit: Some(2),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "twitter".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        let (run, _) = application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:01Z")
            .unwrap();
        let claimed = crate::library_subscription_state::claim_next_query(
            &application,
            &mut crate::subscriptions::DomainSchedule::new(),
            "2026-08-29T00:00:02Z",
        )
        .unwrap()
        .unwrap();

        application
            .pause_subscription_run_library(subscription_id)
            .unwrap();
        crate::library_subscription_state::interrupt_query(
            &application,
            &claimed,
            "2026-08-29T00:00:03Z",
        )
        .unwrap();
        assert_eq!(
            crate::subscription_catalog::list_library(&application)
                .unwrap()
                .subscriptions[0]
                .status
                .as_deref(),
            Some("paused")
        );
        assert!(crate::library_subscription_state::claim_next_query(
            &application,
            &mut crate::subscriptions::DomainSchedule::new(),
            "2026-08-29T00:00:04Z",
        )
        .unwrap()
        .is_none());

        application
            .resume_subscription_run_library(subscription_id, "2026-08-29T00:00:05Z")
            .unwrap();
        assert!(crate::library_subscription_state::claim_next_query(
            &application,
            &mut crate::subscriptions::DomainSchedule::new(),
            "2026-08-29T00:00:06Z",
        )
        .unwrap()
        .is_some());

        application
            .cancel_subscription_run_library(subscription_id, "2026-08-29T00:00:07Z")
            .unwrap();
        assert!(crate::library_subscription_state::claim_next_query(
            &application,
            &mut crate::subscriptions::DomainSchedule::new(),
            "2026-08-29T00:00:08Z",
        )
        .unwrap()
        .is_none());
        let (replacement, _) = application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:09Z")
            .unwrap();
        assert!(replacement.created);
        assert_ne!(replacement.run_id, run.run_id);
    }

    #[tokio::test]
    async fn canonical_subscription_download_settles_through_the_schema_one_ingest_queue() {
        for site_id in ["twitter", "ehentai"] {
            let directory = tempfile::tempdir().unwrap();
            let application = LibraryApplication::create(directory.path().join("library")).unwrap();
            let downloads = directory.path().join("downloads");
            std::fs::create_dir_all(&downloads).unwrap();
            let source_path = downloads.join("download.png");
            image::RgbaImage::from_pixel(1, 1, image::Rgba([8, 16, 24, 255]))
                .save(&source_path)
                .unwrap();
            let bytes = std::fs::read(&source_path).unwrap();
            let post = NormalizedPost {
                site_id: site_id.into(),
                post_key: "post-1".into(),
                canonical_url: Some("https://x.com/example/status/post-1".into()),
                creator_name: Some("example".into()),
                title: Some("Post one".into()),
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![crate::subscriptions::NormalizedItem {
                    item_key: "media-1".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                }],
            };
            let runner = OneItemRunner {
                application: &application,
                item: DownloadedItem {
                    post,
                    input: PreparedImport {
                        stable_key: format!("source:{site_id}:post-1:media-1"),
                        media_name: "download".into(),
                        file_path: source_path.to_string_lossy().into_owned(),
                        facts: ImmutableMediaFacts {
                            mime: "image/png".into(),
                            size_bytes: bytes.len() as u64,
                            width: Some(1),
                            height: Some(1),
                            duration_ms: None,
                            frame_count: Some(1),
                            content_hash: hex::encode(Sha256::digest(&bytes)),
                            perceptual_hash: None,
                            palette: Vec::new(),
                        },
                        lifecycle: Lifecycle::Inbox,
                        rating: Rating::Unrated,
                        notes: None,
                        tags: vec!["creator:example".into()],
                        folders: Vec::new(),
                        source_urls: Vec::new(),
                        source_identity: Some(SourceIdentity {
                            source_key: format!("{site_id}:post-1"),
                            source_item_key: "media-1".into(),
                            source_text: None,
                            source_attempt_id: None,
                        }),
                        imported_at_ms: 1_700_000_000_000,
                        captured_at_ms: None,
                    },
                    post_complete: true,
                    force_collection: false,
                    delete_after_ingest: false,
                },
                cleanup_paths: vec![downloads.clone()],
                expected_state: "ingested",
            };
            let definition = crate::subscription_catalog::NewSubscription {
                name: "Example".into(),
                schedule: "manual".into(),
                initial_post_limit: None,
                periodic_post_limit: None,
                queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                    site_id: site_id.into(),
                    query_text: if site_id == "ehentai" {
                        "https://exhentai.org/g/1449482/9051983a03/".into()
                    } else {
                        "example".into()
                    },
                    display_name: None,
                    notes: None,
                    group_posts: true,
                }],
            };
            let (subscription_id, _) = application
                .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
                .unwrap();
            application
                .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
                .unwrap();
            let mut schedule = DomainSchedule::new();
            tick(&application, &mut schedule, &runner, "2026-08-29T00:00:01Z")
                .await
                .unwrap();
            assert!(!downloads.exists());
            let status = application
                .library()
                .auxiliary_read(
                    picto_library::database::WorkPriority::VisibleRead,
                    |connection| {
                        Ok(connection.query_row(
                            "SELECT status FROM subscription_run ORDER BY run_id DESC LIMIT 1",
                            [],
                            |row| row.get::<_, String>(0),
                        )?)
                    },
                )
                .unwrap();
            assert_eq!(status, "succeeded");
            let report = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
            assert_eq!(report.ingested, 0);
            let state = application
                .library()
                .auxiliary_read(
                    picto_library::database::WorkPriority::VisibleRead,
                    |connection| {
                        Ok(connection.query_row(
                            "SELECT state FROM source_item WHERE item_key = 'media-1'",
                            [],
                            |row| row.get::<_, String>(0),
                        )?)
                    },
                )
                .unwrap();
            assert_eq!(state, "ingested");
            let status = application
                .library()
                .auxiliary_read(
                    picto_library::database::WorkPriority::VisibleRead,
                    |connection| {
                        Ok(connection.query_row(
                            "SELECT status FROM subscription_run ORDER BY run_id DESC LIMIT 1",
                            [],
                            |row| row.get::<_, String>(0),
                        )?)
                    },
                )
                .unwrap();
            assert_eq!(status, "succeeded");

            std::fs::create_dir_all(&downloads).unwrap();
            image::RgbaImage::from_pixel(1, 1, image::Rgba([8, 16, 24, 255]))
                .save(&source_path)
                .unwrap();
            application
                .reset_subscription_library(subscription_id)
                .await
                .unwrap();
            application
                .request_subscription_run_library(subscription_id, "2026-08-29T00:01:00Z")
                .unwrap();
            tick(&application, &mut schedule, &runner, "2026-08-29T00:01:01Z")
                .await
                .unwrap();
            let rerun =
                crate::subscription_activity::list_runs_library(&application, subscription_id, 1)
                    .unwrap();
            assert_eq!(rerun.runs[0].status, "succeeded");
            assert_eq!(rerun.runs[0].counts.posts_added, 0);
            assert_eq!(rerun.runs[0].counts.posts_skipped, 1);
            assert_eq!(rerun.runs[0].counts.fetched, 1);
            assert_eq!(rerun.runs[0].counts.downloaded, 0);
            assert_eq!(rerun.runs[0].counts.ingested, 0);
        }
    }

    #[tokio::test]
    async fn native_gallery_publishes_one_ordered_collection_after_every_file_settles() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let downloads = directory.path().join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let post = NormalizedPost {
            site_id: "ehentai".into(),
            post_key: "gallery-1".into(),
            canonical_url: Some("https://e-hentai.org/g/1/token/".into()),
            creator_name: Some("example".into()),
            title: Some("Gallery one".into()),
            description: Some("Two pages".into()),
            captured_at: None,
            metadata_json: None,
            items: vec![
                crate::subscriptions::NormalizedItem {
                    item_key: "page-1".into(),
                    position: 0,
                    media_url: None,
                    canonical_url: None,
                },
                crate::subscriptions::NormalizedItem {
                    item_key: "page-2".into(),
                    position: 1,
                    media_url: None,
                    canonical_url: None,
                },
            ],
        };
        let mut items = Vec::new();
        for (item_key, color) in [("page-2", 32_u8), ("page-1", 16_u8)] {
            let path = downloads.join(format!("{item_key}.png"));
            image::RgbaImage::from_pixel(1, 1, image::Rgba([color, 8, 4, 255]))
                .save(&path)
                .unwrap();
            let bytes = std::fs::read(&path).unwrap();
            items.push(DownloadedItem {
                post: post.clone(),
                input: PreparedImport {
                    stable_key: format!("source:ehentai:gallery-1:{item_key}"),
                    media_name: item_key.into(),
                    file_path: path.to_string_lossy().into_owned(),
                    facts: ImmutableMediaFacts {
                        mime: "image/png".into(),
                        size_bytes: bytes.len() as u64,
                        width: Some(1),
                        height: Some(1),
                        duration_ms: None,
                        frame_count: Some(1),
                        content_hash: hex::encode(Sha256::digest(&bytes)),
                        perceptual_hash: None,
                        palette: Vec::new(),
                    },
                    lifecycle: Lifecycle::Inbox,
                    rating: Rating::Unrated,
                    notes: None,
                    tags: vec!["creator:example".into()],
                    folders: Vec::new(),
                    source_urls: vec!["https://e-hentai.org/g/1/0123456789/".into()],
                    source_identity: Some(SourceIdentity {
                        source_key: "ehentai:gallery-1".into(),
                        source_item_key: item_key.into(),
                        source_text: None,
                        source_attempt_id: None,
                    }),
                    imported_at_ms: chrono::DateTime::parse_from_rfc3339("2026-08-30T00:00:01Z")
                        .unwrap()
                        .timestamp_millis(),
                    captured_at_ms: None,
                },
                post_complete: false,
                force_collection: true,
                delete_after_ingest: false,
            });
        }
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Gallery".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "ehentai".into(),
                query_text: "https://e-hentai.org/g/1/0123456789/".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-30T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:00:00Z")
            .unwrap();
        let runner = MultiItemRunner {
            application: &application,
            items,
            cleanup_path: downloads.clone(),
            expected_source_items: 2,
        };
        let mut schedule = DomainSchedule::new();
        tick(&application, &mut schedule, &runner, "2026-08-30T00:00:01Z")
            .await
            .unwrap();

        assert!(!downloads.exists());
        let root_id = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok(connection.query_row(
                        "SELECT local_id FROM library_item WHERE item_kind = 2",
                        [],
                        |row| row.get::<_, u32>(0),
                    )?)
                },
            )
            .unwrap();
        let details = application.details(picto_library::RootId(root_id)).unwrap();
        assert_eq!(details.root.media_count, 2);
        assert_eq!(
            details
                .media
                .iter()
                .map(|media| media.media_name.as_str())
                .collect::<Vec<_>>(),
            ["page-1", "page-2"]
        );
        let activity =
            crate::subscription_activity::list_runs_library(&application, subscription_id, 1)
                .unwrap();
        assert_eq!(activity.runs[0].counts.posts_traversed, 1);
        assert_eq!(activity.runs[0].counts.posts_added, 1);
        assert_eq!(activity.runs[0].counts.downloaded, 2);

        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:01:00Z")
            .unwrap();
        tick(
            &application,
            &mut schedule,
            &KnownPostRunner { post: post.clone() },
            "2026-08-30T00:01:01Z",
        )
        .await
        .unwrap();
        let rerun =
            crate::subscription_activity::list_runs_library(&application, subscription_id, 1)
                .unwrap();
        assert_eq!(
            rerun.runs[0].status, "succeeded",
            "{:?}: {:?}",
            rerun.runs[0].failure_kind, rerun.runs[0].error_message
        );
        assert_eq!(rerun.runs[0].counts.posts_added, 0);
        assert_eq!(rerun.runs[0].counts.posts_skipped, 1);
        assert_eq!(rerun.runs[0].counts.downloaded, 0);
        assert_eq!(rerun.runs[0].counts.files_already_in_library, 2);

        let (members, _) = application
            .library()
            .ungroup_collection(picto_library::RootId(root_id), 1_800_000_000_000)
            .unwrap();
        assert_eq!(members.len(), 2);
        for regroup in [false, true] {
            if regroup {
                application
                    .library()
                    .organize_into_collection(&picto_library::GroupRequest {
                        target: picto_library::selection::SelectionTarget::Explicit {
                            root_ids: members.clone(),
                        },
                        cover_root_id: members[0],
                        winning_collection_id: None,
                        name: Some("Regrouped".into()),
                        notes: None,
                        modified_at_ms: 1_800_000_000_001,
                    })
                    .unwrap();
            }
            let requested_at = if regroup {
                "2026-08-30T00:03:00Z"
            } else {
                "2026-08-30T00:02:00Z"
            };
            let tick_at = if regroup {
                "2026-08-30T00:03:01Z"
            } else {
                "2026-08-30T00:02:01Z"
            };
            application
                .request_subscription_run_library(subscription_id, requested_at)
                .unwrap();
            // Known-post discovery can skip fetching the attachment listing.
            // Count its persisted media even when this run has no file attempts.
            let mut known = post.clone();
            known.items.clear();
            tick(
                &application,
                &mut schedule,
                &KnownPostRunner { post: known },
                tick_at,
            )
            .await
            .unwrap();
            let history =
                crate::subscription_activity::list_runs_library(&application, subscription_id, 1)
                    .unwrap();
            assert_eq!(history.runs[0].status, "succeeded");
            assert_eq!(history.runs[0].counts.posts_skipped, 1);
            assert_eq!(history.runs[0].counts.posts_added, 0);
            assert_eq!(history.runs[0].counts.downloaded, 0);
            assert_eq!(history.runs[0].counts.files_already_in_library, 2);
            let catalog = crate::subscription_catalog::list_library(&application).unwrap();
            assert_eq!(
                catalog.subscriptions[0].progress.files_already_in_library,
                2
            );
        }
    }

    #[tokio::test]
    async fn subscription_zip_settles_one_remote_attachment_as_a_collection() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let downloads = directory.path().join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let first = downloads.join("first.png");
        let second = downloads.join("second.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([8, 16, 24, 255]))
            .save(&first)
            .unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([24, 16, 8, 255]))
            .save(&second)
            .unwrap();
        let archive = downloads.join("attachment.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("first.png", options).unwrap();
        std::io::copy(&mut std::fs::File::open(&first).unwrap(), &mut zip).unwrap();
        zip.start_file("second.png", options).unwrap();
        std::io::copy(&mut std::fs::File::open(&second).unwrap(), &mut zip).unwrap();
        zip.finish().unwrap();
        let descriptor = picto_sources::MediaDescriptor {
            stable_id: "archive-1".into(),
            position: 0,
            url: "https://downloads.fanbox.cc/files/attachment.zip".into(),
            canonical_url: Some("https://creator.fanbox.cc/posts/1".into()),
            file_name: Some("attachment.zip".into()),
            mime_hint: Some("application/zip".into()),
            expected_size: None,
            headers: std::collections::BTreeMap::new(),
            fallbacks: Vec::new(),
            rejected_final_paths: Vec::new(),
            postprocess: None,
        };
        let source_post = picto_sources::SourcePost {
            site_id: "fanbox".into(),
            partition: picto_sources::SourcePartition::new("posts"),
            stable_id: "post-1".into(),
            canonical_url: Some("https://creator.fanbox.cc/posts/1".into()),
            creator: Some("creator".into()),
            name: Some("Archive post".into()),
            notes: None,
            created_at: None,
            tags: Vec::new(),
            media: vec![descriptor.clone()],
            resume_cursor_after: Some("post-1".into()),
        };
        let prepared = crate::native_source_import::prepare_source_post(
            &source_post,
            picto_sources::PostDownload {
                downloaded: vec![picto_sources::DownloadedMedia {
                    descriptor: descriptor.clone(),
                    path: archive.clone(),
                    size_bytes: std::fs::metadata(&archive).unwrap().len(),
                }],
                failures: Vec::new(),
            },
            1,
        )
        .await
        .unwrap();
        let post = NormalizedPost {
            site_id: "fanbox".into(),
            post_key: "post-1".into(),
            canonical_url: Some("https://creator.fanbox.cc/posts/1".into()),
            creator_name: Some("creator".into()),
            title: Some("Archive post".into()),
            description: None,
            captured_at: None,
            metadata_json: None,
            items: vec![crate::subscriptions::NormalizedItem {
                item_key: "archive-1".into(),
                position: 0,
                media_url: Some("https://downloads.fanbox.cc/files/attachment.zip".into()),
                canonical_url: Some("https://creator.fanbox.cc/posts/1".into()),
            }],
        };
        let items = prepared
            .members
            .into_iter()
            .map(|input| {
                let expanded_post = crate::native_source::post_with_prepared_source_item(
                    &post,
                    &descriptor,
                    &input,
                )
                .unwrap();
                DownloadedItem {
                    post: expanded_post,
                    input,
                    post_complete: false,
                    force_collection: true,
                    delete_after_ingest: true,
                }
            })
            .collect();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "FANBOX archive".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "fanbox".into(),
                query_text: "creator".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-30T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:00:00Z")
            .unwrap();
        let runner = MultiItemRunner {
            application: &application,
            items,
            cleanup_path: downloads.clone(),
            expected_source_items: 2,
        };
        tick(
            &application,
            &mut DomainSchedule::new(),
            &runner,
            "2026-08-30T00:00:01Z",
        )
        .await
        .unwrap();

        let activity =
            crate::subscription_activity::list_runs_library(&application, subscription_id, 1)
                .unwrap();
        assert_eq!(
            activity.runs[0].status, "succeeded",
            "{:?}: {:?}",
            activity.runs[0].failure_kind, activity.runs[0].error_message
        );
        assert_eq!(
            activity.runs[0].counts.posts_added, 1,
            "run counts: {:?}",
            activity.runs[0].counts
        );
        let root_id = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok(connection.query_row(
                        "SELECT local_id FROM library_item WHERE item_kind = 2",
                        [],
                        |row| row.get::<_, u32>(0),
                    )?)
                },
            )
            .unwrap();
        let details = application.details(picto_library::RootId(root_id)).unwrap();
        assert_eq!(details.root.media_count, 2);
        assert_eq!(activity.runs[0].counts.posts_added, 1);
        assert_eq!(activity.runs[0].counts.downloaded, 2);
    }

    #[tokio::test]
    async fn subscription_item_without_a_blob_is_downloaded_again() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let downloads = directory.path().join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let source_path = downloads.join("deleted.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([8, 16, 24, 255]))
            .save(&source_path)
            .unwrap();
        let bytes = std::fs::read(&source_path).unwrap();
        let stable_key = "source:twitter:deleted-post:media-1";
        application
            .library()
            .auxiliary_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                ["tests".to_owned()],
                [],
                |transaction, revision| {
                    transaction.execute(
                        "INSERT INTO deletion_tombstone(stable_key, revision, deleted_at_ms)
                         VALUES (?1, ?2, ?3)",
                        rusqlite::params![stable_key, revision as i64, 1_700_000_000_000_i64],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        let post = NormalizedPost {
            site_id: "twitter".into(),
            post_key: "deleted-post".into(),
            canonical_url: Some("https://x.com/example/status/deleted-post".into()),
            creator_name: Some("example".into()),
            title: Some("Deleted post".into()),
            description: None,
            captured_at: None,
            metadata_json: None,
            items: vec![crate::subscriptions::NormalizedItem {
                item_key: "media-1".into(),
                position: 0,
                media_url: None,
                canonical_url: None,
            }],
        };
        let runner = OneItemRunner {
            application: &application,
            item: DownloadedItem {
                post,
                input: PreparedImport {
                    stable_key: stable_key.into(),
                    media_name: "deleted".into(),
                    file_path: source_path.to_string_lossy().into_owned(),
                    facts: ImmutableMediaFacts {
                        mime: "image/png".into(),
                        size_bytes: bytes.len() as u64,
                        width: Some(1),
                        height: Some(1),
                        duration_ms: None,
                        frame_count: Some(1),
                        content_hash: hex::encode(Sha256::digest(&bytes)),
                        perceptual_hash: None,
                        palette: Vec::new(),
                    },
                    lifecycle: Lifecycle::Inbox,
                    rating: Rating::Unrated,
                    notes: None,
                    tags: vec!["creator:example".into()],
                    folders: Vec::new(),
                    source_urls: Vec::new(),
                    source_identity: Some(SourceIdentity {
                        source_key: "twitter:deleted-post".into(),
                        source_item_key: "media-1".into(),
                        source_text: None,
                        source_attempt_id: None,
                    }),
                    imported_at_ms: 1_700_000_000_000,
                    captured_at_ms: None,
                },
                post_complete: true,
                force_collection: false,
                delete_after_ingest: false,
            },
            cleanup_paths: vec![downloads.clone()],
            expected_state: "ingested",
        };
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Example".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "twitter".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();

        let mut schedule = DomainSchedule::new();
        tick(&application, &mut schedule, &runner, "2026-08-29T00:00:01Z")
            .await
            .unwrap();

        let (run_status, query_failure, item_state, job_status, roots) = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok((
                        connection.query_row(
                            "SELECT status FROM subscription_run ORDER BY run_id DESC LIMIT 1",
                            [],
                            |row| row.get::<_, String>(0),
                        )?,
                        connection.query_row(
                            "SELECT last_failure_message FROM subscription_query LIMIT 1",
                            [],
                            |row| row.get::<_, Option<String>>(0),
                        )?,
                        connection.query_row(
                            "SELECT state FROM source_item WHERE item_key = 'media-1'",
                            [],
                            |row| row.get::<_, String>(0),
                        )?,
                        connection.query_row(
                            "SELECT status FROM ingest_job LIMIT 1",
                            [],
                            |row| row.get::<_, String>(0),
                        )?,
                        connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| {
                            row.get::<_, i64>(0)
                        })?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(run_status, "succeeded");
        assert_eq!(query_failure, None);
        assert_eq!(item_state, "ingested");
        assert_eq!(job_status, "succeeded");
        assert_eq!(roots, 1);
        assert!(!downloads.exists());
    }

    #[tokio::test]
    async fn one_missing_attachment_is_a_post_problem_not_a_query_failure() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Example".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "patreon".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let runner = FailedItemRunner {
            post: NormalizedPost {
                site_id: "patreon".into(),
                post_key: "post-with-dead-file".into(),
                canonical_url: Some("https://www.patreon.com/posts/example-1".into()),
                creator_name: Some("example".into()),
                title: Some("Post with dead file".into()),
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![crate::subscriptions::NormalizedItem {
                    item_key: "media-404".into(),
                    position: 0,
                    media_url: Some("https://cdn.example.invalid/deleted.png".into()),
                    canonical_url: None,
                }],
            },
        };

        let mut schedule = DomainSchedule::new();
        tick(&application, &mut schedule, &runner, "2026-08-29T00:00:01Z")
            .await
            .unwrap();
        state::settle_ingest_runs(&application, "2026-08-29T00:00:02Z").unwrap();

        let (run_status, query_error, item_state, item_error, open_item_issues) = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok((
                        connection.query_row(
                            "SELECT status FROM subscription_run ORDER BY run_id DESC LIMIT 1",
                            [],
                            |row| row.get::<_, String>(0),
                        )?,
                        connection.query_row(
                            "SELECT last_failure_message FROM subscription_query LIMIT 1",
                            [],
                            |row| row.get::<_, Option<String>>(0),
                        )?,
                        connection.query_row(
                            "SELECT state FROM source_item WHERE item_key = 'media-404'",
                            [],
                            |row| row.get::<_, String>(0),
                        )?,
                        connection.query_row(
                            "SELECT last_error FROM source_item WHERE item_key = 'media-404'",
                            [],
                            |row| row.get::<_, Option<String>>(0),
                        )?,
                        connection.query_row(
                            "SELECT COUNT(*) FROM subscription_issue
                             WHERE issue_kind = 'download_item' AND status = 'open'",
                            [],
                            |row| row.get::<_, i64>(0),
                        )?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(run_status, "succeeded");
        assert_eq!(query_error, None);
        assert_eq!(item_state, "failed");
        assert_eq!(item_error.as_deref(), Some("404 Not Found"));
        assert_eq!(open_item_issues, 1);

        let issues = crate::subscription_activity::list_issues_library(
            &application,
            &crate::subscription_activity::IssuePageRequest {
                subscription_id,
                query_id: None,
                unresolved_only: true,
                cursor: None,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(issues.issues.len(), 1);
        let issue = &issues.issues[0];
        assert_eq!(issue.source_item_key.as_deref(), Some("media-404"));
        assert_eq!(
            issue.source_post_key.as_deref(),
            Some("post-with-dead-file")
        );
        assert_eq!(
            issue.source_post_title.as_deref(),
            Some("Post with dead file")
        );
        assert_eq!(
            issue.canonical_post_url.as_deref(),
            Some("https://www.patreon.com/posts/example-1")
        );
        assert_eq!(
            issue.media_url.as_deref(),
            Some("https://cdn.example.invalid/deleted.png")
        );
        assert_eq!(issue.message, "404 Not Found");

        crate::library_subscription_state::acknowledge_subscription_issues(
            &application,
            subscription_id,
        )
        .unwrap();
        let acknowledged = crate::subscription_activity::list_issues_library(
            &application,
            &crate::subscription_activity::IssuePageRequest {
                subscription_id,
                query_id: None,
                unresolved_only: true,
                cursor: None,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(acknowledged.issues.len(), 1);
        assert_eq!(acknowledged.issues[0].status, "acknowledged");
        assert_eq!(
            crate::subscription_catalog::list_library(&application)
                .unwrap()
                .subscriptions[0]
                .open_issue_count,
            0
        );
    }

    #[tokio::test]
    async fn gallery_media_failure_is_concise_and_keeps_the_diagnostic_in_problems() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: "Gallery".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: "ehentai".into(),
                query_text: "https://e-hentai.org/g/1/0123456789/".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-30T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:00:00Z")
            .unwrap();
        let runner = FailedItemRunner {
            post: NormalizedPost {
                site_id: "ehentai".into(),
                post_key: "gallery-1".into(),
                canonical_url: Some("https://e-hentai.org/g/1/0123456789/".into()),
                creator_name: None,
                title: Some("Gallery one".into()),
                description: None,
                captured_at: None,
                metadata_json: None,
                items: vec![crate::subscriptions::NormalizedItem {
                    item_key: "media-404".into(),
                    position: 0,
                    media_url: Some("https://cdn.example.invalid/missing.png".into()),
                    canonical_url: None,
                }],
            },
        };

        let mut schedule = DomainSchedule::new();
        tick(&application, &mut schedule, &runner, "2026-08-30T00:00:01Z")
            .await
            .unwrap();

        let catalog = crate::subscription_catalog::list_library(&application).unwrap();
        assert_eq!(catalog.subscriptions[0].status.as_deref(), Some("runtime"));
        assert_eq!(
            catalog.subscriptions[0].queries[0]
                .last_failure_message
                .as_deref(),
            Some("Gallery download failed: 404 Not Found")
        );
        let issues = crate::subscription_activity::list_issues_library(
            &application,
            &crate::subscription_activity::IssuePageRequest {
                subscription_id,
                query_id: None,
                unresolved_only: true,
                cursor: None,
                limit: 10,
            },
        )
        .unwrap();
        let issue = issues
            .issues
            .iter()
            .find(|issue| issue.issue_kind == "download_item")
            .expect("gallery media failure is listed in Problems");
        assert_eq!(issue.message, "404 Not Found");
        assert_eq!(issue.source_post_key.as_deref(), Some("gallery-1"));
        assert_eq!(
            issue.canonical_post_url.as_deref(),
            Some("https://e-hentai.org/g/1/0123456789/")
        );
    }
}
