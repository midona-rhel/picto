//! gallery-dl adapter for the replacement subscription state machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::Lifecycle;
use crate::ingest_v2::{PreparedMediaInput, SourcePostInput};
use crate::subscription_runtime_v2::{
    DownloadedItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess, SourceEvent,
    SourceRunner,
};
use crate::subscriptions::gallery_dl_runner::{
    self, FailureKind, GalleryDlAuthConfig, GalleryDlRunner, RunOptions, RunSummary,
};
use crate::subscriptions::import_policy::preferred_import_name;
use crate::subscriptions::source_adapter::{self, ParsedMetadata};
use crate::subscriptions_v2::{ClaimedQueryRun, NormalizedItem, NormalizedPost};

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
            archive_path: self.library_root.join("gdl-archive.sqlite3"),
            archive_prefix: Some(
                crate::subscriptions::archive::subscription_query_archive_prefix(
                    query.subscription_id,
                    query.query_id,
                ),
            ),
            cancel,
        };

        let (legacy_output, mut legacy_input) = mpsc::channel(CHANNEL_CAPACITY);
        let (post_output, mut post_input) = mpsc::channel(CHANNEL_CAPACITY);
        let runner = GalleryDlRunner::new(binary);
        let run = runner.run(&options, legacy_output, Some(post_output));
        tokio::pin!(run);
        let mut downloaded = 0usize;
        let mut ignored_non_media = 0usize;
        let mut pending_item = None;
        let mut next_position_by_post = HashMap::new();
        let summary = loop {
            tokio::select! {
                result = &mut run => break result.map_err(map_runner_error)?,
                item = legacy_input.recv() => match item {
                    Some(item) => {
                        let normalized = self.normalize_item(query, item).await?;
                        if normalized.is_empty() {
                            ignored_non_media += 1;
                        } else {
                            downloaded += queue_downloads(
                                &output,
                                &mut pending_item,
                                normalized,
                                &mut next_position_by_post,
                            ).await?;
                        }
                    }
                    None => break run.await.map_err(map_runner_error)?,
                },
                post = post_input.recv() => if let Some(metadata) = post {
                    if let Some(post) = normalize_traversed_post(&query.site_id, metadata)? {
                        output.send(SourceEvent::PostTraversed(post)).await.map_err(|_| {
                            RunnerFailure::terminal(RunnerFailureKind::Runtime, "subscription receiver closed")
                        })?;
                    }
                }
            }
        };
        while let Some(item) = legacy_input.recv().await {
            let normalized = self.normalize_item(query, item).await?;
            if normalized.is_empty() {
                ignored_non_media += 1;
            } else {
                downloaded += queue_downloads(
                    &output,
                    &mut pending_item,
                    normalized,
                    &mut next_position_by_post,
                )
                .await?;
            }
        }
        while let Some(metadata) = post_input.recv().await {
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
        let configured = if query.initial_run_complete {
            query.periodic_post_limit.or(query.initial_post_limit)
        } else {
            query.initial_post_limit.or(query.periodic_post_limit)
        };
        configured
            .filter(|value| *value > 0)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE)
    })
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
        if let Some(source) = item.input.source.as_mut() {
            source.position = *position;
        }
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
        let post_complete = previous.post.post_key != next.post.post_key;
        set_post_complete(&mut previous, post_complete);
        send_download(output, previous).await?;
    }
    *pending = Some(next);
    Ok(())
}

fn set_post_complete(item: &mut DownloadedItem, complete: bool) {
    if let Some(source) = item.input.source.as_mut() {
        source.post_complete = complete;
    }
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
                    if let Some(source) = item.input.source.as_mut() {
                        source.force_collection = true;
                    }
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
    let needs_identity_hash = (metadata.post_id.is_none() && metadata.canonical_post_url.is_none())
        || (metadata.item_key.is_none() && metadata.media_url.is_none());
    let identity_hash = if needs_identity_hash {
        Some(hex::encode(
            crate::media_processing::get_hash_from_path(&file_path).map_err(|error| {
                RunnerFailure::terminal(
                    RunnerFailureKind::InvalidOutput,
                    format!(
                        "Could not hash downloaded media {}: {error}",
                        file_path.display()
                    ),
                )
            })?,
        ))
    } else {
        None
    };
    let size_bytes = prepared.size_bytes.unwrap_or_default() as i64;
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
            let tag = crate::tag_name_v2::format(namespace, subtag);
            crate::tag_name_v2::parse_external(&tag)
                .ok()
                .map(|(namespace, subtag)| crate::tag_name_v2::format(&namespace, &subtag))
        })
        .collect();
    let post_key = metadata
        .post_id
        .clone()
        .or_else(|| metadata.canonical_post_url.clone())
        .unwrap_or_else(|| identity_hash.clone().expect("fallback hash computed"));
    let position = i64::from(metadata.page_num.unwrap_or(0));
    let item_key = metadata
        .item_key
        .clone()
        .or_else(|| metadata.media_url.clone())
        .unwrap_or_else(|| {
            format!(
                "{post_key}:{position}:{}",
                identity_hash.as_deref().expect("fallback hash computed")
            )
        });
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
    let source = SourcePostInput {
        site_id: site_id.to_string(),
        post_key: post_key.clone(),
        item_key: item_key.clone(),
        position,
        post_complete: false,
        force_collection: false,
        group_post: true,
        canonical_post_url: metadata.canonical_post_url.clone(),
        canonical_media_url: metadata.media_url.clone(),
        creator_name: creator_name.clone(),
        title: metadata.title.clone(),
        description: metadata.description.clone(),
        captured_at: metadata.created_at.clone(),
        metadata_json: metadata_json.clone(),
    };
    Ok(Some(DownloadedItem {
        post: NormalizedPost {
            site_id: site_id.to_string(),
            post_key,
            canonical_url: metadata.canonical_post_url.clone(),
            creator_name,
            title: metadata.title.clone(),
            description: metadata.description.clone(),
            captured_at: metadata.created_at.clone(),
            metadata_json,
            items: vec![NormalizedItem {
                item_key,
                position,
                media_url: metadata.media_url.clone(),
                canonical_url: metadata.source_url.clone(),
            }],
        },
        source_path: file_path,
        input: PreparedMediaInput {
            file_hash: String::new(),
            mime_type: prepared.mime_type,
            size_bytes,
            pixel_width: prepared.pixel_width.map(i64::from),
            pixel_height: prepared.pixel_height.map(i64::from),
            duration_ms: prepared.duration_ms.map(|value| value as i64),
            frame_count: prepared.num_frames.map(i64::from),
            has_audio: prepared.has_audio,
            name: preferred_import_name(&metadata),
            notes: metadata.description,
            rating: None,
            source_urls: metadata.source_urls,
            tags,
            provenance_mask: 1,
            lifecycle: Lifecycle::Inbox,
            captured_at: created_at,
            source: Some(source),
            target_folder_id: None,
            target_folder_ids: Vec::new(),
        },
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
        format!("range:{}", self.range_start.saturating_add(self.batch_size))
    }
}

fn settle_summary(
    summary: RunSummary,
    downloaded: usize,
    ignored_non_media: usize,
    batch: &BatchPosition,
    require_media: bool,
) -> Result<RunnerSuccess, RunnerFailure> {
    if summary.had_download_errors || !summary.failed_items.is_empty() {
        return Err(RunnerFailure::retryable(
            RunnerFailureKind::Download,
            summary
                .failed_items
                .first()
                .map(|item| item.error_message.clone())
                .unwrap_or_else(|| "One or more downloads failed".to_string()),
        ));
    }
    if summary.exit_code != 0 && gallery_dl_runner::has_error_lines(&summary.stderr_output) {
        let kind = gallery_dl_runner::classify_failure(&summary.stderr_output);
        let message = gallery_dl_runner::final_error_line(&summary.stderr_output)
            .unwrap_or_else(|| format!("gallery-dl exited with code {}", summary.exit_code));
        return Err(map_failure(kind, message));
    }
    if summary.exit_code != 0 && summary.source_page_items == 0 && summary.discovered_items == 0 {
        return Err(RunnerFailure::terminal(
            RunnerFailureKind::Runtime,
            format!("gallery-dl exited with code {}", summary.exit_code),
        ));
    }
    if downloaded == 0
        && summary.discovered_items
            > summary
                .skipped_archive_items
                .saturating_add(ignored_non_media)
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
        resume_cursor: Some(batch.next_cursor(&summary)),
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
        assert_eq!(item.input.pixel_width, Some(3));
        assert_eq!(item.input.pixel_height, Some(2));
        assert_eq!(item.input.tags, vec!["character:hero"]);
        assert_eq!(item.input.notes.as_deref(), Some("Description"));
        assert_eq!(item.input.source.as_ref().unwrap().item_key, "pixiv:42:1");
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
        assert!(!received.input.source.unwrap().post_complete);

        let mut next_post = item;
        next_post.post.post_key = "43".to_string();
        next_post.input.source.as_mut().unwrap().post_key = "43".to_string();
        queue_download(&output, &mut pending, next_post)
            .await
            .unwrap();
        let SourceEvent::MediaDownloaded(received) = input.recv().await.unwrap() else {
            panic!("expected media event");
        };
        assert!(received.input.source.unwrap().post_complete);
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
        let source = normalized[0].input.source.as_ref().unwrap();
        assert_eq!(source.post_key, "58577141");
        assert!(source.item_key.contains(":zip:0:pages/001.png"));
        assert_eq!(normalized[0].input.mime_type, "image/png");
        assert_eq!(normalized[1].input.mime_type, "text/plain");
        assert!(normalized
            .iter()
            .all(|item| item.input.source.as_ref().unwrap().force_collection));
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
        assert_eq!(normalized[0].input.mime_type, "audio/mpeg");
        assert!(
            normalized[0]
                .input
                .source
                .as_ref()
                .unwrap()
                .force_collection
        );
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
            batch_size: crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE,
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
        assert!(empty_gallery
            .message
            .contains("without discovering any media"));
    }

    #[test]
    fn subscription_batches_continue_instead_of_rescanning_the_first_page() {
        let mut query = claimed_query();
        assert_eq!(
            BatchPosition::for_query(
                &query,
                crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE,
            )
            .range_start,
            1
        );

        query.initial_run_complete = true;
        assert_eq!(
            BatchPosition::for_query(
                &query,
                crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE,
            )
            .range_start,
            101
        );

        query.resume_cursor = Some("range:201".into());
        assert_eq!(
            BatchPosition::for_query(
                &query,
                crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE,
            )
            .range_start,
            201
        );

        query.resume_cursor = Some("opaque-patreon-cursor".into());
        assert_eq!(
            BatchPosition::for_query(
                &query,
                crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE,
            )
            .source_cursor
            .as_deref(),
            Some("opaque-patreon-cursor")
        );

        query.resume_cursor = Some(String::new());
        assert!(
            BatchPosition::for_query(
                &query,
                crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE,
            )
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
    fn every_source_uses_the_subscription_batch_limit() {
        let mut query = claimed_query();
        query.site_id = "twitter".into();
        assert_eq!(
            effective_batch_size(&query, None),
            crate::subscriptions_v2::DEFAULT_SOURCE_POST_BATCH_SIZE
        );

        query.initial_post_limit = Some(3);
        assert_eq!(effective_batch_size(&query, None), 3);
        assert_eq!(effective_batch_size(&query, Some(1)), 1);
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
            initial_run_complete: false,
            resume_cursor: None,
            attempt_count: 0,
        }
    }
}
