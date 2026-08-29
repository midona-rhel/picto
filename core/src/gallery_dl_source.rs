//! gallery-dl adapter for the replacement subscription state machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::subscription_runtime::{
    DownloadedItem, FailedMediaItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess,
    SourceEvent, SourceRunner,
};
use crate::subscriptions::gallery_dl_runner::{
    self, FailureKind, GalleryDlAuthConfig, GalleryDlRunner, RunOptions, RunSummary, StreamEvent,
};
use crate::subscriptions::import_policy::preferred_import_name;
use crate::subscriptions::source_adapter::{self, ParsedMetadata};
use crate::subscriptions::{ClaimedQueryRun, NormalizedItem, NormalizedPost};
use picto_library::{ImmutableMediaFacts, Lifecycle, PreparedImport, Rating, SourceIdentity};

const CHANNEL_CAPACITY: usize = 32;
const PERIODIC_ABORT_THRESHOLD: u32 = 25;

pub struct GalleryDlSourceRunner {
    library_root: PathBuf,
    binary_override: Option<PathBuf>,
    source_post_batch_size: Option<u32>,
}

impl GalleryDlSourceRunner {
    pub fn open(library_root: &Path) -> Self {
        Self {
            library_root: library_root.to_path_buf(),
            binary_override: None,
            source_post_batch_size: None,
        }
    }

    pub fn new(library_root: PathBuf, binary: PathBuf) -> Self {
        Self {
            library_root,
            binary_override: Some(binary),
            source_post_batch_size: None,
        }
    }

    /// Use a smaller source window for live adapter certification without
    /// changing the production 100-post continuation contract.
    pub fn with_batch_size(library_root: &Path, source_post_batch_size: u32) -> Self {
        Self {
            library_root: library_root.to_path_buf(),
            binary_override: None,
            source_post_batch_size: Some(source_post_batch_size.max(1)),
        }
    }

    async fn execute(
        &self,
        query: &ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> Result<RunnerSuccess, RunnerFailure> {
        source_adapter::validate_query_kind(&query.site_id, &query.query_kind)
            .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::InvalidQuery, error))?;
        source_adapter::validate_query_text(&query.site_id, &query.query_text)
            .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::InvalidQuery, error))?;
        let url =
            gallery_dl_runner::build_url(&query.site_id, &query.query_text).ok_or_else(|| {
                RunnerFailure::terminal(
                    RunnerFailureKind::InvalidQuery,
                    format!("Unsupported gallery-dl source: {}", query.site_id),
                )
            })?;
        let site = gallery_dl_runner::site_by_id(&query.site_id).ok_or_else(|| {
            RunnerFailure::terminal(
                RunnerFailureKind::InvalidQuery,
                format!("Unknown source: {}", query.site_id),
            )
        })?;
        let auth = load_auth(site.credential_owner_site_id)?;
        if auth.is_none() && site.auth_strictly_required {
            return Err(RunnerFailure::terminal(
                RunnerFailureKind::Authentication,
                format!("{} requires a connected account", site.name),
            ));
        }

        let binary = self
            .binary_override
            .clone()
            .map(Ok)
            .unwrap_or_else(|| crate::media_processing::gallery_dl_path::gallery_dl_path().cloned())
            .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Runtime, error))?;
        let batch_size = effective_batch_size(query, self.source_post_batch_size);
        let batch = BatchPosition::for_query(query, batch_size);
        // A concrete E-Hentai gallery is an explicit, transient import. Its
        // durable identity and deduplication belong to Picto after the whole
        // gallery has downloaded. A gallery-dl archive can only make retries
        // incomplete (for example, seven archived pages plus one new page),
        // which must never be published as a gallery.
        let use_download_archive = query.site_id != "ehentai";
        let options = RunOptions {
            subscription_id: Some(query.subscription_id),
            query_id: Some(query.query_id),
            site_id: query.site_id.clone(),
            url,
            post_limit: Some(batch_size),
            range_start: batch.range_start,
            source_cursor: batch.source_cursor.clone(),
            abort_threshold: batch.history_complete.then_some(PERIODIC_ABORT_THRESHOLD),
            auth,
            archive_path: use_download_archive
                .then(|| self.library_root.join("gdl-archive.sqlite3"))
                .unwrap_or_default(),
            archive_prefix: use_download_archive.then(|| {
                crate::subscriptions::archive::subscription_query_archive_prefix(
                    query.subscription_id,
                    query.query_id,
                )
            }),
            cancel,
        };

        let (bridge_output, mut bridge_input) = mpsc::channel(CHANNEL_CAPACITY);
        let runner = GalleryDlRunner::new(binary);
        let run = runner.run(&options, bridge_output);
        tokio::pin!(run);
        let mut downloaded = 0usize;
        let mut ignored_non_media = 0usize;
        let mut pending_item = None;
        let mut next_position_by_post = HashMap::new();
        let summary = loop {
            tokio::select! {
                result = &mut run => break result.map_err(map_runner_error)?,
                event = bridge_input.recv() => match event {
                    Some(event) => self.handle_bridge_event(
                        query,
                        &output,
                        event,
                        &mut pending_item,
                        &mut next_position_by_post,
                        &mut downloaded,
                        &mut ignored_non_media,
                    ).await?,
                    None => break run.await.map_err(map_runner_error)?,
                },
            }
        };
        while let Some(event) = bridge_input.recv().await {
            self.handle_bridge_event(
                query,
                &output,
                event,
                &mut pending_item,
                &mut next_position_by_post,
                &mut downloaded,
                &mut ignored_non_media,
            )
            .await?;
        }
        for failed in &summary.failed_items {
            let failed = normalize_failed_download(
                &query.site_id,
                failed.metadata.clone(),
                failed.item_url.clone(),
                failed.error_message.clone(),
            )?;
            output
                .send(SourceEvent::MediaFailed(failed))
                .await
                .map_err(|_| {
                    RunnerFailure::terminal(
                        RunnerFailureKind::Runtime,
                        "subscription receiver closed",
                    )
                })?;
        }
        let result = settle_summary(
            summary,
            downloaded,
            ignored_non_media,
            &batch,
            query.site_id == "ehentai",
        );
        if let Some(mut item) = pending_item {
            set_post_complete(&mut item, result.is_ok());
            send_download(&output, item).await?;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_bridge_event(
        &self,
        query: &ClaimedQueryRun,
        output: &mpsc::Sender<SourceEvent>,
        event: StreamEvent,
        pending_item: &mut Option<DownloadedItem>,
        next_position_by_post: &mut HashMap<String, i64>,
        downloaded: &mut usize,
        ignored_non_media: &mut usize,
    ) -> Result<(), RunnerFailure> {
        match event {
            StreamEvent::PostTraversed(metadata) => {
                if let Some(post) = normalize_traversed_post(&query.site_id, metadata)? {
                    output
                        .send(SourceEvent::PostTraversed(post))
                        .await
                        .map_err(|_| {
                            RunnerFailure::terminal(
                                RunnerFailureKind::Runtime,
                                "subscription receiver closed",
                            )
                        })?;
                }
            }
            StreamEvent::MediaDownloaded(item) => {
                let normalized = self.normalize_item(query, item).await?;
                if normalized.is_empty() {
                    *ignored_non_media += 1;
                } else {
                    *downloaded +=
                        queue_downloads(output, pending_item, normalized, next_position_by_post)
                            .await?;
                }
            }
            StreamEvent::PostComplete(acknowledge) => {
                if let Some(mut item) = pending_item.take() {
                    set_post_complete(&mut item, true);
                    send_download(output, item).await?;
                }
                output
                    .send(SourceEvent::PostComplete(acknowledge))
                    .await
                    .map_err(|_| {
                        RunnerFailure::terminal(
                            RunnerFailureKind::Runtime,
                            "subscription receiver closed",
                        )
                    })?;
            }
        }
        Ok(())
    }

    async fn normalize_item(
        &self,
        query: &ClaimedQueryRun,
        item: crate::subscriptions::source_adapter::DownloadedItem,
    ) -> Result<Vec<DownloadedItem>, RunnerFailure> {
        let post_key = item.metadata.post_id.clone();
        match normalize_downloads(&query.site_id, item.file_path, item.metadata).await {
            Ok(item) => Ok(item),
            Err(error) => {
                if let Some(post_key) = post_key {
                    let prefix = crate::subscriptions::archive::subscription_query_archive_prefix(
                        query.subscription_id,
                        query.query_id,
                    );
                    let _ = crate::subscriptions::archive::clear_post_archive_entries_at_root(
                        &self.library_root,
                        &prefix,
                        &[post_key],
                    )
                    .await;
                }
                Err(error)
            }
        }
    }
}

fn effective_batch_size(query: &ClaimedQueryRun, override_size: Option<u32>) -> u32 {
    override_size.unwrap_or_else(|| {
        provider_process_post_limit(&query.site_id)
            .expect("validated gallery provider has an execution policy")
    })
}

fn provider_process_post_limit(site_id: &str) -> Option<u32> {
    // gallery-dl may pipeline extraction ahead of download hooks. Every
    // supported list source therefore gets an explicit one-post process
    // window; site-specific extractor adapters remain isolated in the bridge.
    match site_id {
        "pixiv" | "pixivuser" | "gelbooru" | "rule34" | "danbooru" | "webtoons"
        | "hentaifoundry" | "baraag" | "deviantart" | "tumblr" | "twitter" | "newgrounds"
        | "furaffinity" | "patreon" | "fanbox" | "subscribestar" | "idolcomplex" | "sankaku"
        | "yandere" | "konachan" | "safebooru" | "e621" | "ehentai" => Some(1),
        _ => None,
    }
}

fn map_runner_error(error: String) -> RunnerFailure {
    if error.contains("made no progress") {
        RunnerFailure::retryable(RunnerFailureKind::Network, error)
    } else {
        RunnerFailure::terminal(RunnerFailureKind::Runtime, error)
    }
}

fn normalize_traversed_post(
    site_id: &str,
    metadata: ParsedMetadata,
) -> Result<Option<NormalizedPost>, RunnerFailure> {
    let Some(post_key) = metadata
        .post_id
        .clone()
        .or_else(|| metadata.canonical_post_url.clone())
    else {
        tracing::warn!(
            site_id,
            "gallery-dl traversed a post without stable identity"
        );
        return Ok(None);
    };
    let creator_name = metadata
        .raw_metadata
        .as_ref()
        .and_then(gallery_dl_runner::extract_creator_identifier);
    let metadata_json = metadata
        .raw_metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
        })?;
    Ok(Some(NormalizedPost {
        site_id: site_id.to_string(),
        post_key,
        canonical_url: metadata.canonical_post_url,
        creator_name,
        title: metadata.title,
        description: metadata.description,
        captured_at: metadata.created_at,
        metadata_json,
        items: Vec::new(),
    }))
}

fn normalize_failed_download(
    site_id: &str,
    metadata: ParsedMetadata,
    item_url: Option<String>,
    error_message: String,
) -> Result<FailedMediaItem, RunnerFailure> {
    let post_key = metadata
        .post_id
        .clone()
        .or_else(|| metadata.canonical_post_url.clone())
        .ok_or_else(|| {
            RunnerFailure::terminal(
                RunnerFailureKind::InvalidOutput,
                "Failed subscription media has no stable post identity",
            )
        })?;
    let position = i64::from(metadata.page_num.unwrap_or(0));
    let media_url = item_url.or_else(|| metadata.media_url.clone());
    let item_key = metadata
        .item_key
        .clone()
        .or_else(|| media_url.clone())
        .or_else(|| metadata.source_url.clone())
        .unwrap_or_else(|| format!("{post_key}:{position}"));
    let creator_name = metadata
        .raw_metadata
        .as_ref()
        .and_then(gallery_dl_runner::extract_creator_identifier);
    let metadata_json = metadata
        .raw_metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
        })?;
    Ok(FailedMediaItem {
        post: NormalizedPost {
            site_id: site_id.to_string(),
            post_key,
            canonical_url: metadata.canonical_post_url,
            creator_name,
            title: metadata.title,
            description: metadata.description,
            captured_at: metadata.created_at,
            metadata_json,
            items: vec![NormalizedItem {
                item_key: item_key.clone(),
                position,
                media_url,
                canonical_url: metadata.source_url,
            }],
        },
        item_key,
        error_message,
    })
}

async fn queue_downloads(
    output: &mpsc::Sender<SourceEvent>,
    pending: &mut Option<DownloadedItem>,
    items: Vec<DownloadedItem>,
    next_position_by_post: &mut HashMap<String, i64>,
) -> Result<usize, RunnerFailure> {
    let count = items.len();
    for mut item in items {
        let position = next_position_by_post
            .entry(item.post.post_key.clone())
            .and_modify(|value| *value += 1)
            .or_insert(1);
        item.post.items[0].position = *position;
        queue_download(output, pending, item).await?;
    }
    Ok(count)
}

async fn queue_download(
    output: &mpsc::Sender<SourceEvent>,
    pending: &mut Option<DownloadedItem>,
    next: DownloadedItem,
) -> Result<(), RunnerFailure> {
    if let Some(mut previous) = pending.take() {
        if previous.post.post_key != next.post.post_key {
            return Err(RunnerFailure::terminal(
                RunnerFailureKind::InvalidOutput,
                "source advanced before completing the current post",
            ));
        }
        set_post_complete(&mut previous, false);
        send_download(output, previous).await?;
    }
    *pending = Some(next);
    Ok(())
}

fn set_post_complete(item: &mut DownloadedItem, complete: bool) {
    item.post_complete = complete;
}

async fn send_download(
    output: &mpsc::Sender<SourceEvent>,
    item: DownloadedItem,
) -> Result<(), RunnerFailure> {
    output
        .send(SourceEvent::MediaDownloaded(item))
        .await
        .map_err(|_| {
            RunnerFailure::terminal(RunnerFailureKind::Runtime, "subscription receiver closed")
        })
}

impl SourceRunner for GalleryDlSourceRunner {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a> {
        Box::pin(async move { self.execute(query, output, cancel).await })
    }
}

fn load_auth(owner_site_id: &str) -> Result<Option<GalleryDlAuthConfig>, RunnerFailure> {
    let credential = crate::credential_store::get_credential(owner_site_id)
        .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Authentication, error))?;
    Ok(credential.map(|credential| GalleryDlAuthConfig {
        site_category: owner_site_id.to_string(),
        fragment: crate::credential_store::build_extractor_auth(&credential),
    }))
}

pub(crate) async fn normalize_downloads(
    site_id: &str,
    file_path: PathBuf,
    metadata: ParsedMetadata,
) -> Result<Vec<DownloadedItem>, RunnerFailure> {
    if is_zip_path(&file_path) {
        let entries = crate::media_processing::archive::extract_library_files(&file_path).map_err(
            |error| {
                RunnerFailure::terminal(
                    RunnerFailureKind::InvalidOutput,
                    format!("{}: {error}", file_path.display()),
                )
            },
        )?;
        let mut normalized = Vec::with_capacity(entries.len());
        for (index, entry) in entries.into_iter().enumerate() {
            let mut entry_metadata = metadata.clone();
            let base_item_key = entry_metadata.item_key.clone().unwrap_or_else(|| {
                entry_metadata
                    .post_id
                    .clone()
                    .unwrap_or_else(|| "archive".to_string())
            });
            entry_metadata.item_key = Some(format!(
                "{base_item_key}:zip:{index}:{}",
                entry.archive_name
            ));
            entry_metadata.media_url = None;
            match normalize_media_download(site_id, entry.path, entry_metadata).await? {
                Some(mut item) => {
                    item.force_collection = true;
                    normalized.push(item)
                }
                None => tracing::warn!(
                    archive = %file_path.display(),
                    entry = entry.archive_name,
                    "Ignoring unsupported media-looking ZIP entry"
                ),
            }
        }
        return Ok(normalized);
    }

    Ok(normalize_media_download(site_id, file_path, metadata)
        .await?
        .into_iter()
        .collect())
}

fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
}

async fn normalize_media_download(
    site_id: &str,
    file_path: PathBuf,
    metadata: ParsedMetadata,
) -> Result<Option<DownloadedItem>, RunnerFailure> {
    let prepared =
        match crate::media_processing::PreparedMediaSource::prepare_ingest(&file_path).await {
            Ok(prepared) => prepared,
            Err(crate::media_processing::FileError::UnsupportedFile(reason)) => {
                tracing::info!(
                    path = %file_path.display(),
                    reason,
                    "Ignoring non-media subscription attachment"
                );
                return Ok(None);
            }
            Err(error) => {
                return Err(RunnerFailure::terminal(
                    RunnerFailureKind::InvalidOutput,
                    format!("{}: {error}", file_path.display()),
                ));
            }
        };
    if !prepared.caps.ingest_supported || prepared.mime_type == "application/zip" {
        tracing::info!(
            path = %file_path.display(),
            mime_type = prepared.mime_type,
            "Ignoring non-media subscription attachment"
        );
        return Ok(None);
    }
    let content_hash = hex::encode(
        crate::media_processing::get_hash_from_path(&file_path).map_err(|error| {
            RunnerFailure::terminal(
                RunnerFailureKind::InvalidOutput,
                format!(
                    "Could not hash downloaded media {}: {error}",
                    file_path.display()
                ),
            )
        })?,
    );
    let size_bytes = prepared.size_bytes.unwrap_or_default();
    let created_at = metadata.created_at.clone().or_else(|| {
        std::fs::metadata(&file_path).ok().and_then(|file| {
            let timestamp = file.created().or_else(|_| file.modified()).ok()?;
            Some(chrono::DateTime::<chrono::Utc>::from(timestamp).to_rfc3339())
        })
    });
    let tags = metadata
        .tags
        .iter()
        .filter_map(|(namespace, subtag)| {
            let tag = crate::tag_name::format(namespace, subtag);
            crate::tag_name::parse_external(&tag)
                .ok()
                .map(|(namespace, subtag)| crate::tag_name::format(&namespace, &subtag))
        })
        .collect();
    let post_key = metadata
        .post_id
        .clone()
        .or_else(|| metadata.canonical_post_url.clone())
        .unwrap_or_else(|| content_hash.clone());
    let position = i64::from(metadata.page_num.unwrap_or(0));
    let item_key = metadata
        .item_key
        .clone()
        .or_else(|| metadata.media_url.clone())
        .unwrap_or_else(|| format!("{post_key}:{position}:{}", content_hash));
    let creator_name = metadata
        .raw_metadata
        .as_ref()
        .and_then(gallery_dl_runner::extract_creator_identifier);
    let metadata_json = metadata
        .raw_metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
        })?;
    let captured_at_ms = created_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis());
    let source_key = format!("{site_id}:{post_key}");
    let stable_key = format!("source:{source_key}:{item_key}");
    let media_name = preferred_import_name(&metadata).unwrap_or_else(|| {
        file_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled")
            .to_owned()
    });
    let mut source_urls = metadata.source_urls.clone();
    if let Some(url) = metadata.canonical_post_url.as_ref() {
        if !source_urls.contains(url) {
            source_urls.push(url.clone());
        }
    }
    if let Some(url) = metadata.source_url.as_ref() {
        if !source_urls.contains(url) {
            source_urls.push(url.clone());
        }
    }
    Ok(Some(DownloadedItem {
        post: NormalizedPost {
            site_id: site_id.to_string(),
            post_key,
            canonical_url: metadata.canonical_post_url.clone(),
            creator_name,
            title: metadata.title.clone(),
            description: metadata.description.clone(),
            captured_at: metadata.created_at.clone(),
            metadata_json: metadata_json.clone(),
            items: vec![NormalizedItem {
                item_key: item_key.clone(),
                position,
                media_url: metadata.media_url.clone(),
                canonical_url: metadata.source_url.clone(),
            }],
        },
        input: PreparedImport {
            stable_key,
            media_name,
            file_path: file_path.to_string_lossy().into_owned(),
            facts: ImmutableMediaFacts {
                mime: prepared.mime_type,
                size_bytes,
                width: prepared.pixel_width,
                height: prepared.pixel_height,
                duration_ms: prepared.duration_ms,
                frame_count: prepared.num_frames,
                content_hash,
                perceptual_hash: None,
                palette: Vec::new(),
            },
            tags,
            lifecycle: Lifecycle::Inbox,
            rating: Rating::Unrated,
            notes: metadata.description,
            folders: Vec::new(),
            source_urls,
            source_identity: Some(SourceIdentity {
                source_key,
                source_item_key: item_key,
                source_text: metadata_json,
            }),
            imported_at_ms: chrono::Utc::now().timestamp_millis(),
            captured_at_ms,
        },
        post_complete: false,
        force_collection: false,
        delete_after_ingest: true,
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct BatchPosition {
    range_start: u32,
    source_cursor: Option<String>,
    history_complete: bool,
    batch_size: u32,
}

impl BatchPosition {
    fn for_query(query: &ClaimedQueryRun, batch_size: u32) -> Self {
        match query.resume_cursor.as_deref() {
            Some("") => Self {
                range_start: 1,
                source_cursor: None,
                history_complete: true,
                batch_size,
            },
            Some(cursor) if cursor.starts_with("range:") => Self {
                range_start: cursor[6..].parse().unwrap_or(1),
                source_cursor: None,
                history_complete: false,
                batch_size,
            },
            Some(cursor) => Self {
                range_start: 1,
                source_cursor: Some(cursor.to_string()),
                history_complete: false,
                batch_size,
            },
            None => Self {
                // Existing queries completed before cursors were persisted. Their first
                // batch is already archived, so continue with the second batch.
                range_start: if query.initial_run_complete {
                    batch_size + 1
                } else {
                    1
                },
                source_cursor: None,
                history_complete: false,
                batch_size,
            },
        }
    }

    fn next_cursor(&self, summary: &RunSummary) -> String {
        if let Some(cursor) = &summary.source_cursor {
            return cursor.clone();
        }
        if self.history_complete
            || (summary.source_page_items == 0 && summary.discovered_items == 0)
        {
            return String::new();
        }
        let traversed = u32::try_from(summary.source_page_items)
            .unwrap_or(u32::MAX)
            .max(1);
        format!("range:{}", self.range_start.saturating_add(traversed))
    }
}

fn settle_summary(
    summary: RunSummary,
    downloaded: usize,
    ignored_non_media: usize,
    batch: &BatchPosition,
    require_media: bool,
) -> Result<RunnerSuccess, RunnerFailure> {
    if summary.had_download_errors && summary.failed_items.is_empty() {
        return Err(RunnerFailure::retryable(
            RunnerFailureKind::Download,
            "One or more downloads failed",
        ));
    }
    if summary.failed_items.is_empty()
        && summary.exit_code != 0
        && gallery_dl_runner::has_error_lines(&summary.stderr_output)
    {
        let kind = gallery_dl_runner::classify_failure(&summary.stderr_output);
        let message = gallery_dl_runner::final_error_line(&summary.stderr_output)
            .unwrap_or_else(|| format!("gallery-dl exited with code {}", summary.exit_code));
        return Err(map_failure(kind, message));
    }
    if summary.exit_code != 0 && summary.source_page_items == 0 && summary.discovered_items == 0 {
        let message = gallery_dl_runner::final_error_line(&summary.stderr_output)
            .unwrap_or_else(|| format!("gallery-dl exited with code {}", summary.exit_code));
        return Err(RunnerFailure::terminal(RunnerFailureKind::Runtime, message));
    }
    if downloaded == 0
        && summary.discovered_items
            > summary
                .skipped_archive_items
                .saturating_add(ignored_non_media)
                .saturating_add(summary.failed_items.len())
    {
        return Err(RunnerFailure::terminal(
            RunnerFailureKind::InvalidOutput,
            format!(
                "gallery-dl discovered {} items but produced no media",
                summary.discovered_items
                    - summary
                        .skipped_archive_items
                        .saturating_add(ignored_non_media)
            ),
        ));
    }
    if require_media && downloaded == 0 && summary.discovered_items == 0 {
        return Err(RunnerFailure::terminal(
            RunnerFailureKind::InvalidOutput,
            "The gallery completed without discovering any media".to_string(),
        ));
    }
    Ok(RunnerSuccess {
        resume_cursor: Some(if require_media {
            String::new()
        } else {
            batch.next_cursor(&summary)
        }),
        cleanup_paths: Vec::new(),
    })
}

fn map_failure(kind: FailureKind, message: String) -> RunnerFailure {
    match kind {
        FailureKind::RateLimited => {
            RunnerFailure::retryable(RunnerFailureKind::RateLimited, message)
        }
        FailureKind::Network => RunnerFailure::retryable(RunnerFailureKind::Network, message),
        FailureKind::DownloadFailure => {
            RunnerFailure::retryable(RunnerFailureKind::Download, message)
        }
        FailureKind::CredentialMissing
        | FailureKind::CredentialBlocked
        | FailureKind::Unauthorized
        | FailureKind::Expired => {
            RunnerFailure::terminal(RunnerFailureKind::Authentication, message)
        }
        FailureKind::NotFound | FailureKind::InvalidQueryKind => {
            RunnerFailure::terminal(RunnerFailureKind::InvalidQuery, message)
        }
        FailureKind::MalformedMetadata | FailureKind::BridgeNoDownloads => {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, message)
        }
        FailureKind::IngestQueueFailure => {
            RunnerFailure::retryable(RunnerFailureKind::Runtime, message)
        }
        _ => RunnerFailure::terminal(RunnerFailureKind::Runtime, message),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image::{ImageBuffer, ImageFormat, Rgba};
    use zip::write::SimpleFileOptions;

    use super::*;

    #[tokio::test]
    async fn normalized_download_keeps_source_identity_metadata_and_media_facts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("page.png");
        let image = ImageBuffer::from_pixel(3, 2, Rgba([1_u8, 2, 3, 255]));
        image.save_with_format(&path, ImageFormat::Png).unwrap();
        let raw = serde_json::json!({"category":"pixiv", "user":{"name":"Artist"}});
        let item = normalize_downloads(
            "pixiv",
            path,
            ParsedMetadata {
                tags: vec![("character".into(), "hero".into())],
                description: Some("Description".into()),
                source_urls: vec!["https://www.pixiv.net/artworks/42".into()],
                media_url: Some("https://i.pximg.net/page.png".into()),
                title: Some("Artwork".into()),
                post_id: Some("42".into()),
                created_at: Some("2026-01-01T00:00:00Z".into()),
                page_num: Some(1),
                canonical_post_url: Some("https://www.pixiv.net/artworks/42".into()),
                item_key: Some("pixiv:42:1".into()),
                raw_metadata: Some(raw),
                ..ParsedMetadata::default()
            },
        )
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

        assert_eq!(item.post.post_key, "42");
        assert_eq!(item.post.items[0].position, 1);
        assert_eq!(item.input.facts.width, Some(3));
        assert_eq!(item.input.facts.height, Some(2));
        assert_eq!(item.input.tags, vec!["character:hero"]);
        assert_eq!(item.input.notes.as_deref(), Some("Description"));
        assert_eq!(
            item.input.source_identity.as_ref().unwrap().source_item_key,
            "pixiv:42:1"
        );
        assert!(item.delete_after_ingest);

        let (output, mut input) = mpsc::channel(4);
        let mut pending = None;
        queue_download(&output, &mut pending, item.clone())
            .await
            .unwrap();
        assert!(input.try_recv().is_err());
        queue_download(&output, &mut pending, item.clone())
            .await
            .unwrap();
        let SourceEvent::MediaDownloaded(received) = input.recv().await.unwrap() else {
            panic!("expected media event");
        };
        assert!(!received.post_complete);

        let mut completed = pending.take().unwrap();
        set_post_complete(&mut completed, true);
        send_download(&output, completed).await.unwrap();
        let SourceEvent::MediaDownloaded(received) = input.recv().await.unwrap() else {
            panic!("expected media event");
        };
        assert!(received.post_complete);

        let mut next_post = item;
        next_post.post.post_key = "43".to_string();
        queue_download(&output, &mut pending, next_post)
            .await
            .unwrap();
        assert!(input.try_recv().is_err());
    }

    #[tokio::test]
    async fn external_tags_keep_general_and_core_groups_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("post.png");
        ImageBuffer::from_pixel(1, 1, Rgba([1_u8, 2, 3, 255]))
            .save_with_format(&path, ImageFormat::Png)
            .unwrap();
        let item = normalize_downloads(
            "e621",
            path,
            ParsedMetadata {
                tags: vec![
                    (String::new(), "solo".into()),
                    ("artist".into(), "example".into()),
                    ("meta".into(), "highres".into()),
                ],
                post_id: Some("42".into()),
                item_key: Some("e621:42:0".into()),
                ..ParsedMetadata::default()
            },
        )
        .await
        .unwrap()
        .remove(0);

        assert_eq!(item.input.tags, ["solo", "creator:example"]);
    }

    #[tokio::test]
    async fn subscription_zip_imports_every_accepted_entry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("soundtrack.zip");
        let image_path = directory.path().join("page.png");
        ImageBuffer::from_pixel(3, 2, Rgba([1_u8, 2, 3, 255]))
            .save_with_format(&image_path, ImageFormat::Png)
            .unwrap();
        let image_bytes = std::fs::read(image_path).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("pages/001.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&image_bytes).unwrap();
        zip.start_file("readme.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"not a media asset").unwrap();
        zip.finish().unwrap();

        let normalized = normalize_downloads(
            "patreon",
            path,
            ParsedMetadata {
                post_id: Some("58577141".into()),
                item_key: Some("patreon:58577141:soundtrack".into()),
                ..ParsedMetadata::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(normalized.len(), 2);
        let source = normalized[0].input.source_identity.as_ref().unwrap();
        assert_eq!(source.source_key, "patreon:58577141");
        assert!(source.source_item_key.contains(":zip:0:pages/001.png"));
        assert_eq!(normalized[0].input.facts.mime, "image/png");
        assert_eq!(normalized[1].input.facts.mime, "text/plain");
        assert!(normalized.iter().all(|item| item.force_collection));
    }

    #[tokio::test]
    async fn subscription_zip_accepts_audio_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("soundtrack.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("soundtrack.mp3", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"audio").unwrap();
        zip.finish().unwrap();

        let normalized = normalize_downloads(
            "patreon",
            path,
            ParsedMetadata {
                post_id: Some("58577141".into()),
                item_key: Some("patreon:58577141:soundtrack".into()),
                ..ParsedMetadata::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].input.facts.mime, "audio/mpeg");
        assert!(normalized[0].force_collection);
    }

    #[tokio::test]
    async fn subscription_zip_bomb_is_rejected_with_a_clear_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bomb.zip");
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("page.png", options).unwrap();
        zip.write_all(&vec![0_u8; 1024 * 1024]).unwrap();
        zip.finish().unwrap();

        let failure = normalize_downloads(
            "patreon",
            path,
            ParsedMetadata {
                post_id: Some("unsafe-post".into()),
                ..ParsedMetadata::default()
            },
        )
        .await
        .unwrap_err();

        assert_eq!(failure.kind, RunnerFailureKind::InvalidOutput);
        assert!(!failure.retryable);
        assert!(failure.message.contains("Unsafe ZIP archive"));
        assert!(failure.message.contains("compression-ratio limit"));
    }

    #[test]
    fn summary_distinguishes_up_to_date_from_broken_output() {
        let complete = BatchPosition {
            range_start: 1,
            source_cursor: None,
            history_complete: true,
            batch_size: crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,
        };
        let up_to_date = summary(0, 10, 10);
        assert_eq!(
            settle_summary(up_to_date, 0, 0, &complete, false)
                .unwrap()
                .resume_cursor,
            Some(String::new())
        );

        let broken = settle_summary(summary(0, 10, 0), 0, 0, &complete, false).unwrap_err();
        assert_eq!(broken.kind, RunnerFailureKind::InvalidOutput);
        assert!(!broken.retryable);

        let attachments_only = settle_summary(summary(0, 1, 0), 0, 1, &complete, false).unwrap();
        assert_eq!(attachments_only.resume_cursor, Some(String::new()));

        let empty_gallery = settle_summary(summary(0, 0, 0), 0, 0, &complete, true).unwrap_err();
        assert_eq!(empty_gallery.kind, RunnerFailureKind::InvalidOutput);

        let gallery = settle_summary(summary(0, 30, 0), 30, 0, &complete, true).unwrap();
        assert_eq!(gallery.resume_cursor, Some(String::new()));
        assert!(empty_gallery
            .message
            .contains("without discovering any media"));

        let mut missing_attachment = summary(1, 1, 0);
        missing_attachment.had_download_errors = true;
        missing_attachment.stderr_output =
            "[patreon][error] 404 Not Found for deleted attachment".into();
        missing_attachment.failed_items.push(
            crate::subscriptions::source_adapter::FailedDownloadedItem {
                metadata: ParsedMetadata {
                    post_id: Some("post-1".into()),
                    item_key: Some("media-1".into()),
                    ..ParsedMetadata::default()
                },
                item_url: Some("https://cdn.example.invalid/deleted.png".into()),
                error_message: "404 Not Found".into(),
            },
        );
        assert!(settle_summary(missing_attachment, 0, 0, &complete, false).is_ok());
    }

    #[test]
    fn failed_download_preserves_the_attachment_and_post_urls_separately() {
        let failed = normalize_failed_download(
            "patreon",
            ParsedMetadata {
                post_id: Some("52678238".into()),
                title: Some("Maku 3 - Colored".into()),
                canonical_post_url: Some(
                    "https://www.patreon.com/corablue/posts/maku-3-colored-52678238".into(),
                ),
                media_url: Some(
                    "https://www.patreon.com/corablue/posts/maku-3-colored-52678238".into(),
                ),
                ..ParsedMetadata::default()
            },
            Some("https://cdn.discordapp.com/attachments/deleted.png".into()),
            "404 Not Found".into(),
        )
        .unwrap();

        assert_eq!(
            failed.post.canonical_url.as_deref(),
            Some("https://www.patreon.com/corablue/posts/maku-3-colored-52678238")
        );
        assert_eq!(
            failed.post.items[0].media_url.as_deref(),
            Some("https://cdn.discordapp.com/attachments/deleted.png")
        );
        assert_eq!(failed.error_message, "404 Not Found");
    }

    #[test]
    fn empty_failed_bridge_preserves_its_actionable_stderr_tail() {
        let complete = BatchPosition {
            range_start: 1,
            source_cursor: None,
            history_complete: true,
            batch_size: crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,
        };
        let mut failed = summary(4, 0, 0);
        failed.stderr_output = "bridge startup\nExHentai rejected the saved cookies\n".to_string();

        let failure = settle_summary(failed, 0, 0, &complete, true).unwrap_err();
        assert_eq!(failure.message, "ExHentai rejected the saved cookies");
    }

    #[test]
    fn subscription_batches_continue_instead_of_rescanning_the_first_page() {
        let mut query = claimed_query();
        assert_eq!(
            BatchPosition::for_query(&query, crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,)
                .range_start,
            1
        );

        query.initial_run_complete = true;
        assert_eq!(
            BatchPosition::for_query(&query, crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,)
                .range_start,
            101
        );

        query.resume_cursor = Some("range:201".into());
        assert_eq!(
            BatchPosition::for_query(&query, crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,)
                .range_start,
            201
        );

        query.resume_cursor = Some("opaque-patreon-cursor".into());
        assert_eq!(
            BatchPosition::for_query(&query, crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,)
                .source_cursor
                .as_deref(),
            Some("opaque-patreon-cursor")
        );

        query.resume_cursor = Some(String::new());
        assert!(
            BatchPosition::for_query(&query, crate::subscriptions::DEFAULT_SOURCE_POST_BATCH_SIZE,)
                .history_complete
        );
    }

    #[test]
    fn source_failures_have_one_retry_policy() {
        assert!(map_failure(FailureKind::Network, "offline".into()).retryable);
        assert!(map_failure(FailureKind::RateLimited, "slow down".into()).retryable);
        assert!(!map_failure(FailureKind::Unauthorized, "login".into()).retryable);
        assert!(!map_failure(FailureKind::NotFound, "missing".into()).retryable);
    }

    #[test]
    fn every_gallery_provider_has_an_explicit_single_post_process_window() {
        for site in crate::subscriptions::gallery_dl_runner::SITES {
            if site.id != "onlyfans" {
                assert_eq!(
                    provider_process_post_limit(site.id),
                    Some(1),
                    "{} may extract ahead of canonical settlement",
                    site.id
                );
            }
        }

        let query = claimed_query();
        assert_eq!(effective_batch_size(&query, None), 1);
        assert_eq!(effective_batch_size(&query, Some(2)), 2);
    }

    #[test]
    fn inaccessible_posts_advance_history_without_becoming_terminal() {
        let batch = BatchPosition {
            range_start: 1,
            source_cursor: None,
            history_complete: false,
            batch_size: 5,
        };
        let mut result = summary(4, 0, 0);
        result.stderr_output =
            "[fanbox][WARNING] Skipping post 123 (HttpError: '403 Forbidden')".into();
        result.source_page_items = 5;

        assert_eq!(
            settle_summary(result, 0, 0, &batch, false)
                .unwrap()
                .resume_cursor,
            Some("range:6".into())
        );
    }

    #[test]
    fn accepted_post_limit_does_not_hide_additional_traversed_candidates() {
        let batch = BatchPosition {
            range_start: 41,
            source_cursor: None,
            history_complete: false,
            batch_size: 2,
        };
        let mut result = summary(0, 2, 6);
        result.source_page_items = 8;

        assert_eq!(
            settle_summary(result, 2, 0, &batch, false)
                .unwrap()
                .resume_cursor,
            Some("range:49".into())
        );
    }

    #[test]
    fn inaccessible_posts_are_revisited_by_periodic_scans() {
        let mut query = claimed_query();
        query.resume_cursor = Some(String::new());

        let batch = BatchPosition::for_query(&query, 5);
        assert!(batch.history_complete);
        assert_eq!(batch.range_start, 1);
    }

    fn summary(exit_code: i32, discovered: usize, skipped: usize) -> RunSummary {
        RunSummary {
            exit_code,
            stderr_output: String::new(),
            temp_dir: PathBuf::new(),
            had_download_errors: false,
            failed_items: Vec::new(),
            discovered_items: discovered,
            skipped_archive_items: skipped,
            source_cursor: None,
            source_page_items: discovered,
        }
    }

    fn claimed_query() -> ClaimedQueryRun {
        ClaimedQueryRun {
            run_query_id: 1,
            run_id: 1,
            query_id: 1,
            subscription_id: 1,
            site_id: "patreon".into(),
            domain_key: "patreon.com".into(),
            query_kind: "creator".into(),
            query_text: "creator".into(),
            group_posts: true,
            requested_by: "manual".into(),
            initial_post_limit: None,
            periodic_post_limit: None,
            run_post_limit: None,
            initial_run_complete: false,
            resume_cursor: None,
            attempt_count: 0,
        }
    }
}
