//! gallery-dl adapter for the replacement subscription state machine.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::Lifecycle;
use crate::import::pipeline::{ImportOptions, ImportPipeline};
use crate::ingest_v2::{PreparedMediaInput, SourcePostInput};
use crate::subscription_runtime_v2::{
    DownloadedItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess, SourceRunner,
};
use crate::subscriptions::credential_service::GalleryDlAuthConfig;
use crate::subscriptions::gallery_dl_runner::{
    self, FailureKind, GalleryDlRunner, RunOptions, RunSummary,
};
use crate::subscriptions::import_policy::preferred_import_name;
use crate::subscriptions::source_adapter::{self, ParsedMetadata};
use crate::subscriptions_v2::{ClaimedQueryRun, NormalizedItem, NormalizedPost};

const CHANNEL_CAPACITY: usize = 32;
const PERIODIC_ABORT_THRESHOLD: u32 = 25;

pub struct GalleryDlSourceRunner {
    library_root: PathBuf,
    runner: GalleryDlRunner,
}

impl GalleryDlSourceRunner {
    pub fn open(library_root: &Path) -> Result<Self, String> {
        let binary = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
        Ok(Self::new(library_root.to_path_buf(), binary))
    }

    pub fn new(library_root: PathBuf, binary: PathBuf) -> Self {
        Self {
            library_root,
            runner: GalleryDlRunner::new(binary),
        }
    }

    async fn execute(
        &self,
        query: &ClaimedQueryRun,
        output: mpsc::Sender<DownloadedItem>,
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

        let options = RunOptions {
            subscription_id: Some(query.subscription_id),
            query_id: Some(query.query_id),
            site_id: query.site_id.clone(),
            url,
            post_limit: selected_post_limit(query),
            range_start: 1,
            source_cursor: query.resume_cursor.clone(),
            abort_threshold: query
                .initial_run_complete
                .then_some(PERIODIC_ABORT_THRESHOLD),
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
        let run = self.runner.run(&options, legacy_output);
        tokio::pin!(run);
        let mut downloaded = 0usize;
        let summary = loop {
            tokio::select! {
                result = &mut run => break result.map_err(|error| {
                    RunnerFailure::terminal(RunnerFailureKind::Runtime, error)
                })?,
                item = legacy_input.recv() => match item {
                    Some(item) => {
                        let normalized = self.normalize_item(query, item).await?;
                        output.send(normalized).await.map_err(|_| {
                            RunnerFailure::terminal(RunnerFailureKind::Runtime, "subscription receiver closed")
                        })?;
                        downloaded += 1;
                    }
                    None => break run.await.map_err(|error| {
                        RunnerFailure::terminal(RunnerFailureKind::Runtime, error)
                    })?,
                }
            }
        };
        while let Some(item) = legacy_input.recv().await {
            let normalized = self.normalize_item(query, item).await?;
            output.send(normalized).await.map_err(|_| {
                RunnerFailure::terminal(RunnerFailureKind::Runtime, "subscription receiver closed")
            })?;
            downloaded += 1;
        }
        settle_summary(summary, downloaded)
    }

    async fn normalize_item(
        &self,
        query: &ClaimedQueryRun,
        item: crate::subscriptions::source_adapter::DownloadedItem,
    ) -> Result<DownloadedItem, RunnerFailure> {
        let post_key = item.metadata.post_id.clone();
        match normalize_download(&query.site_id, item.file_path, item.metadata).await {
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

impl SourceRunner for GalleryDlSourceRunner {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: mpsc::Sender<DownloadedItem>,
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

fn selected_post_limit(query: &ClaimedQueryRun) -> Option<u32> {
    let limit = if query.initial_run_complete {
        query.periodic_post_limit
    } else {
        query.initial_post_limit
    };
    limit
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

async fn normalize_download(
    site_id: &str,
    file_path: PathBuf,
    metadata: ParsedMetadata,
) -> Result<DownloadedItem, RunnerFailure> {
    let options = ImportOptions {
        tags: metadata.tags.clone(),
        source_urls: metadata.source_urls.clone(),
        created_at: metadata.created_at.clone(),
        name: preferred_import_name(&metadata),
        skip_thumbnail: true,
        ..ImportOptions::default()
    };
    let prepared = ImportPipeline::prepare_blob_import(&file_path, &options)
        .await
        .map_err(|error| {
            RunnerFailure::terminal(
                RunnerFailureKind::InvalidOutput,
                format!("{}: {error}", file_path.display()),
            )
        })?;
    let post_key = metadata
        .post_id
        .clone()
        .or_else(|| metadata.canonical_post_url.clone())
        .unwrap_or_else(|| prepared.hex_hash.clone());
    let position = i64::from(metadata.page_num.unwrap_or(0));
    let item_key = metadata
        .item_key
        .clone()
        .or_else(|| metadata.media_url.clone())
        .unwrap_or_else(|| format!("{post_key}:{position}:{}", prepared.hex_hash));
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
        canonical_post_url: metadata.canonical_post_url.clone(),
        canonical_media_url: metadata.media_url.clone(),
        creator_name: creator_name.clone(),
        title: metadata.title.clone(),
        description: metadata.description.clone(),
        captured_at: metadata.created_at.clone(),
        metadata_json: metadata_json.clone(),
    };
    Ok(DownloadedItem {
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
            file_hash: prepared.hex_hash,
            mime_type: prepared.mime,
            size_bytes: prepared.size as i64,
            pixel_width: prepared.pixel_width,
            pixel_height: prepared.pixel_height,
            duration_ms: prepared.duration_ms,
            frame_count: prepared.num_frames,
            has_audio: prepared.has_audio,
            name: prepared.name,
            notes: metadata.description,
            rating: None,
            source_urls: metadata.source_urls,
            tags: prepared.tags_applied,
            provenance_mask: 1,
            lifecycle: Lifecycle::Inbox,
            captured_at: prepared.created_at,
            source: Some(source),
            target_folder_id: None,
        },
        delete_after_ingest: true,
    })
}

fn settle_summary(summary: RunSummary, downloaded: usize) -> Result<RunnerSuccess, RunnerFailure> {
    if summary.exit_code != 0 {
        let kind = gallery_dl_runner::classify_failure(&summary.stderr_output);
        let message = gallery_dl_runner::final_error_line(&summary.stderr_output)
            .unwrap_or_else(|| format!("gallery-dl exited with code {}", summary.exit_code));
        return Err(map_failure(kind, message));
    }
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
    if downloaded == 0 && summary.discovered_items > summary.skipped_archive_items {
        return Err(RunnerFailure::terminal(
            RunnerFailureKind::InvalidOutput,
            format!(
                "gallery-dl discovered {} items but produced no media",
                summary.discovered_items - summary.skipped_archive_items
            ),
        ));
    }
    Ok(RunnerSuccess {
        resume_cursor: summary.source_cursor,
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
    use image::{ImageBuffer, ImageFormat, Rgba};

    use super::*;

    #[tokio::test]
    async fn normalized_download_keeps_source_identity_metadata_and_media_facts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("page.png");
        let image = ImageBuffer::from_pixel(3, 2, Rgba([1_u8, 2, 3, 255]));
        image.save_with_format(&path, ImageFormat::Png).unwrap();
        let raw = serde_json::json!({"category":"pixiv", "user":{"name":"Artist"}});
        let item = normalize_download(
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
        .unwrap();

        assert_eq!(item.post.post_key, "42");
        assert_eq!(item.post.items[0].position, 1);
        assert_eq!(item.input.pixel_width, Some(3));
        assert_eq!(item.input.pixel_height, Some(2));
        assert_eq!(item.input.tags, vec!["character:hero"]);
        assert_eq!(item.input.notes.as_deref(), Some("Description"));
        assert_eq!(item.input.source.as_ref().unwrap().item_key, "pixiv:42:1");
        assert!(item.delete_after_ingest);
    }

    #[test]
    fn post_limits_switch_after_the_initial_run() {
        let mut query = claimed_query();
        query.initial_post_limit = Some(100);
        query.periodic_post_limit = Some(20);
        assert_eq!(selected_post_limit(&query), Some(100));
        query.initial_run_complete = true;
        assert_eq!(selected_post_limit(&query), Some(20));
    }

    #[test]
    fn summary_distinguishes_up_to_date_from_broken_output() {
        let up_to_date = summary(0, 10, 10);
        assert_eq!(settle_summary(up_to_date, 0).unwrap().resume_cursor, None);

        let broken = settle_summary(summary(0, 10, 0), 0).unwrap_err();
        assert_eq!(broken.kind, RunnerFailureKind::InvalidOutput);
        assert!(!broken.retryable);
    }

    #[test]
    fn source_failures_have_one_retry_policy() {
        assert!(map_failure(FailureKind::Network, "offline".into()).retryable);
        assert!(map_failure(FailureKind::RateLimited, "slow down".into()).retryable);
        assert!(!map_failure(FailureKind::Unauthorized, "login".into()).retryable);
        assert!(!map_failure(FailureKind::NotFound, "missing".into()).retryable);
    }

    fn summary(exit_code: i32, discovered: usize, skipped: usize) -> RunSummary {
        RunSummary {
            exit_code,
            stderr_output: String::new(),
            temp_dir: PathBuf::new(),
            had_download_errors: false,
            failed_items: Vec::new(),
            discovered_items: discovered,
            discovered_post_ids: Vec::new(),
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
            site_id: "pixiv".into(),
            domain_key: "pixiv.net".into(),
            query_kind: "search".into(),
            query_text: "landscape".into(),
            initial_post_limit: None,
            periodic_post_limit: None,
            initial_run_complete: false,
            resume_cursor: None,
            attempt_count: 1,
        }
    }
}
