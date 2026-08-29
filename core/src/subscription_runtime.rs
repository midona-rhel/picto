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

use chrono::Utc;
use tokio::sync::mpsc::{self, Sender};
use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;
use crate::library_subscription_state as state;
use crate::subscriptions::{ClaimedQueryRun, DomainSchedule, NormalizedPost};
use picto_library::{
    MutationReceipt, PreparedCollectionImport, PreparedImport, PreparedIngestJob,
    PreparedIngestPayload,
};

const CHANNEL_CAPACITY: usize = 32;
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
pub enum SourceEvent {
    PostTraversed(NormalizedPost),
    MediaDownloaded(DownloadedItem),
    MediaFailed(FailedMediaItem),
    PostComplete(tokio::sync::oneshot::Sender<()>),
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

    fn changed(&mut self, application: &LibraryApplication) -> Result<(), String> {
        self.dirty = true;
        if self.last_publish.elapsed() >= PROGRESS_PUBLISH_INTERVAL {
            self.flush(application)?;
        }
        Ok(())
    }

    fn flush(&mut self, application: &LibraryApplication) -> Result<(), String> {
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
            match state::complete_query(
                application,
                &query,
                success.resume_cursor.as_deref(),
                &settled_at,
            ) {
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
                if crate::settings::subscription_inbox_is_full_library(application)? {
                    runner_cancel.cancel();
                    return Ok(Err(RunnerFailure::retryable(
                        RunnerFailureKind::InboxFull,
                        "Inbox reached its configured subscription limit",
                    )));
                }
                if !state::query_is_running(application, query.run_query_id)? {
                    runner_cancel.cancel();
                    return Ok(Err(RunnerFailure::retryable(
                        RunnerFailureKind::Interrupted,
                        "Subscription run stopped",
                    )));
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
                    &mut progress,
                ).await?,
                None => input_open = false,
            },
        }
    };

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
            &mut progress,
        )
        .await?;
    }

    if atomic_gallery {
        if runner_result.is_ok() {
            validate_complete_gallery(&atomic_items)?;
            let mut gallery_post = atomic_items[0].post.clone();
            gallery_post.items = atomic_items
                .iter()
                .flat_map(|item| item.post.items.iter().cloned())
                .collect();
            state::record_post(
                application,
                query.run_query_id,
                &gallery_post,
                &Utc::now().to_rfc3339(),
            )?;
            let source_item_ids = atomic_items
                .iter()
                .map(|item| {
                    let source =
                        item.input.source_identity.as_ref().ok_or_else(|| {
                            "A subscription item needs source identity".to_string()
                        })?;
                    state::source_item_id(
                        application,
                        &item.post.site_id,
                        &item.post.post_key,
                        &source.source_item_key,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            state::mark_source_items_downloaded(
                application,
                &source_item_ids,
                &Utc::now().to_rfc3339(),
            )?;
            enqueue_group(application, query, &destination, atomic_items)?;
            progress.changed(application)?;
        } else {
            let post_keys = atomic_items
                .iter()
                .map(|item| item.post.post_key.as_str())
                .collect::<BTreeSet<_>>();
            for post_key in post_keys {
                release_post_archive(application, query, post_key).await;
            }
        }
    }
    if runner_result.is_ok() && !grouped_items.is_empty() {
        return Ok(Err(RunnerFailure::retryable(
            RunnerFailureKind::InvalidOutput,
            "Source run ended before a grouped post completed",
        )));
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
    progress: &mut SourceProgressPublisher,
) -> Result<(), String> {
    let durable_change = match event {
        SourceEvent::PostComplete(acknowledge) => {
            wait_for_query_ingest(application, query, cancel)
                .await
                .map_err(|failure| failure.message)?;
            let _ = acknowledge.send(());
            false
        }
        SourceEvent::PostTraversed(post) if atomic_gallery => {
            state::record_post(
                application,
                query.run_query_id,
                &post,
                &Utc::now().to_rfc3339(),
            )?;
            true
        }
        SourceEvent::MediaDownloaded(mut item) if atomic_gallery => {
            let source_item_id =
                ensure_source_item_recorded(application, query, &item, recorded_source_items)?;
            persist_downloaded_media(application, std::slice::from_mut(&mut item))?;
            state::mark_source_items_downloaded(
                application,
                &[source_item_id],
                &Utc::now().to_rfc3339(),
            )?;
            atomic_items.push(item);
            true
        }
        SourceEvent::MediaFailed(item) if atomic_gallery => {
            return Err(format!(
                "Gallery media `{}` could not be downloaded: {}",
                item.item_key, item.error_message
            ));
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
            true
        }
        SourceEvent::MediaDownloaded(mut item) => {
            let source_item_id =
                ensure_source_item_recorded(application, query, &item, recorded_source_items)?;
            persist_downloaded_media(application, std::slice::from_mut(&mut item))?;
            state::mark_source_items_downloaded(
                application,
                &[source_item_id],
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
                    if let Err(error) = enqueue_group(application, query, destination, items) {
                        release_post_archive(application, query, &post_key).await;
                        return Err(error);
                    }
                }
            } else {
                enqueue_group(application, query, destination, vec![item])?;
            }
            true
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
                query.subscription_id,
                query.query_id,
                source_item_id,
                &item.error_message,
                &Utc::now().to_rfc3339(),
            )?;
            recorded_source_items.insert((item.post.post_key, item.item_key));
            true
        }
    };
    if durable_change {
        progress.changed(application)?;
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
    if !items.last().is_some_and(|item| item.post_complete) {
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

fn publish_source_progress(_application: &LibraryApplication) -> Result<(), String> {
    // Source-state writes already publish coalesced task invalidations.
    Ok(())
}

async fn release_post_archive(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    post_key: &str,
) {
    let prefix = crate::subscriptions::archive::subscription_query_archive_prefix(
        query.subscription_id,
        query.query_id,
    );
    let _ = crate::subscriptions::archive::clear_post_archive_entries_at_root(
        application.root(),
        &prefix,
        &[post_key.to_string()],
    )
    .await;
}

fn enqueue_group(
    application: &LibraryApplication,
    query: &ClaimedQueryRun,
    destination: &crate::subscription_catalog::SubscriptionDestinationPolicy,
    mut items: Vec<DownloadedItem>,
) -> Result<(), String> {
    if items.is_empty() {
        return Err("A completed source post has no media".into());
    }
    let post = items[0].post.clone();
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
    let payload = if items.len() == 1 {
        PreparedIngestPayload::Item(items.remove(0).input)
    } else {
        PreparedIngestPayload::Collection(PreparedCollectionImport {
            members: items.into_iter().map(|item| item.input).collect(),
            cover_index: 0,
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

    struct OneItemRunner<'a> {
        application: &'a LibraryApplication,
        item: DownloadedItem,
        cleanup_paths: Vec<PathBuf>,
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
                    .send(SourceEvent::PostComplete(acknowledge))
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
                assert_eq!(state, "ingested", "post was acknowledged before ingest");
                Ok(RunnerSuccess {
                    resume_cursor: Some(String::new()),
                    cleanup_paths: self.cleanup_paths.clone(),
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
                    .send(SourceEvent::PostComplete(acknowledge))
                    .await
                    .unwrap();
                acknowledged.await.unwrap();
                Ok(RunnerSuccess {
                    resume_cursor: Some(String::new()),
                    cleanup_paths: Vec::new(),
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
        assert!(paused.subscriptions[0].paused);
        assert_eq!(paused.subscriptions[0].status.as_deref(), Some("paused"));

        application.pause_all_subscriptions_library(false).unwrap();
        let claimed = crate::library_subscription_state::claim_next_query(
            &application,
            &mut schedule,
            "2026-08-29T00:00:02Z",
        )
        .unwrap()
        .expect("resumed manual query should become claimable");
        assert_eq!(claimed.subscription_id, subscription_id);
    }

    #[tokio::test]
    async fn canonical_subscription_download_settles_through_the_schema_one_ingest_queue() {
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
            site_id: "twitter".into(),
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
                    stable_key: "source:twitter:post-1:media-1".into(),
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
                        source_key: "twitter:post-1".into(),
                        source_item_key: "media-1".into(),
                        source_text: None,
                    }),
                    imported_at_ms: 1_700_000_000_000,
                    captured_at_ms: None,
                },
                post_complete: true,
                force_collection: false,
                delete_after_ingest: false,
            },
            cleanup_paths: vec![downloads.clone()],
        };
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
}
