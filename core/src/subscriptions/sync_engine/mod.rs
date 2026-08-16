//! Subscription sync engine — downloads files via gallery-dl subprocess.
//!
//! Steps per query:
//! 1. Build URL from subscription's gallery-dl URL template + query text
//! 2. Load credentials from OS keychain (if configured for that site)
//! 3. Spawn gallery-dl subprocess with appropriate flags
//! 4. Queue each committed download for background ingest
//! 5. Merge metadata for already-imported files (tags, URLs, notes, name)
//! 6. Track `completed_initial_run` for smart-stop behavior

mod credentials;
mod helpers;
mod persistence;
mod progress;
mod query;
mod retry;

use std::path::{Path, PathBuf};

use crate::db::LibraryDatabase;
use crate::ingest_queue::SubscriptionIngestCheckpoint;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::AppSettings;
use crate::subscriptions::gallery_dl_runner::{FailureKind, GalleryDlRunner};
use crate::subscriptions::source_adapter::ParsedMetadata;

#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    pub files_downloaded: usize,
    pub files_skipped: usize,
    pub queued_for_ingest: usize,
    pub pages_fetched: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub failure_kind: Option<String>,
    pub metadata_validated: usize,
    pub metadata_invalid: usize,
    pub last_metadata_error: Option<String>,
    pub current_post_id: Option<String>,
    pub current_post_items: usize,
    pub posts_processed: usize,
    pub resume_cursor: Option<String>,
}

pub(crate) fn query_run_completion(
    status: &str,
    progress: &SyncProgress,
) -> crate::subscriptions::types::SubscriptionQueryRunCompletion {
    crate::subscriptions::types::SubscriptionQueryRunCompletion {
        status: status.to_string(),
        failure_kind: progress.failure_kind.clone(),
        error_message: progress.errors.last().cloned(),
        posts_processed: progress.posts_processed as i64,
        files_downloaded: progress.files_downloaded as i64,
        files_skipped: progress.files_skipped as i64,
        metadata_validated: progress.metadata_validated as i64,
        metadata_invalid: progress.metadata_invalid as i64,
    }
}

pub struct SubscriptionSyncEngine<'a> {
    db: &'a LibraryDatabase,
    library_root: PathBuf,
    rate_limiter: Option<RateLimiter>,
    runner: GalleryDlRunner,
    settings: AppSettings,
    subscription_name: String,
    progress_mode: String,
    current_query_id: Option<i64>,
    current_query_name: Option<String>,
    current_phase: String,
    last_progress_emit: std::time::Instant,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
    auto_merge_require_matching_dimensions: bool,
}

impl<'a> SubscriptionSyncEngine<'a> {
    pub fn new(
        db: &'a LibraryDatabase,
        settings: &AppSettings,
        library_root: &Path,
    ) -> Result<Self, String> {
        let binary_path = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
        let runner = GalleryDlRunner::new(binary_path);

        Ok(Self {
            db,
            library_root: library_root.to_path_buf(),
            rate_limiter: None,
            runner,
            settings: settings.clone(),
            subscription_name: String::new(),
            progress_mode: "subscription".to_string(),
            current_query_id: None,
            current_query_name: None,
            current_phase: "starting".to_string(),
            last_progress_emit: std::time::Instant::now(),
            auto_merge_enabled: false,
            auto_merge_distance: crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD,
            auto_merge_require_matching_dimensions: false,
        })
    }

    pub(super) fn runtime_service(
        &self,
    ) -> crate::subscriptions::runtime_service::SubscriptionRuntimeService<'_> {
        crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            self.db,
            self.library_root.as_path(),
        )
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.subscription_name = name;
        self
    }

    pub fn with_progress_mode(mut self, mode: &str) -> Self {
        self.progress_mode = mode.to_string();
        self
    }

    pub fn with_rate_limiter(mut self, rate_limiter: RateLimiter) -> Self {
        self.rate_limiter = Some(rate_limiter);
        self
    }

    pub fn with_auto_merge(
        mut self,
        enabled: bool,
        distance: u32,
        require_matching_dimensions: bool,
    ) -> Self {
        self.auto_merge_enabled = enabled;
        self.auto_merge_distance = distance;
        self.auto_merge_require_matching_dimensions = require_matching_dimensions;
        self
    }

    fn finalize_current_post_progress(
        &self,
        progress: &mut SyncProgress,
        posts_processed_this_run: &mut usize,
    ) {
        progress.posts_processed += 1;
        *posts_processed_this_run += 1;

        progress.current_post_id = None;
        progress.current_post_items = 0;
    }

    async fn enqueue_single_subscription_item(
        &self,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        posts_processed: i64,
        file_path: &std::path::Path,
        metadata: &ParsedMetadata,
    ) -> Result<i64, String> {
        let staged = crate::ingest_queue::stage_ingest_sources(
            &self.library_root,
            &[file_path.to_path_buf()],
        )
        .await?;
        let staged_path = staged.paths[0].clone();
        let request = helpers::build_subscription_ingest_request(
            subscription_id,
            &staged_path,
            metadata,
            false,
            0,
        );
        helpers::log_subscription_ingest_request_shape(
            query_id,
            subscription_id,
            metadata,
            &request.tag_strings,
        );
        let result = self
            .db
            .enqueue_ingest_queue(
                "subscription",
                Some(subscription_id),
                Some(query_id),
                query_run_id,
                Some(&staged.root),
                metadata.post_id.as_deref(),
                metadata.category.as_deref(),
                vec![(
                    staged_path,
                    metadata.page_num.map(i64::from),
                    crate::ingest_queue::IngestQueueItemPayload {
                        request,
                        subscription_metadata: Some(metadata.clone()),
                        target_folder_id: None,
                    },
                    true,
                )],
                query_run_id.map(|query_run_id| SubscriptionIngestCheckpoint {
                    query_run_id,
                    files_downloaded: 1,
                    posts_processed,
                    metadata_validated: 1,
                }),
            )
            .await;
        if result.is_ok() {
            helpers::release_producer_sources(&[file_path.to_path_buf()]).await;
        } else {
            let _ = tokio::fs::remove_dir_all(&staged.root).await;
        }
        result
    }

    async fn record_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
        message: &str,
        detail: Option<&str>,
    ) {
        let _ = self
            .runtime_service()
            .upsert_subscription_issue(subscription_id, query_id, failure_kind, message, detail)
            .await;
    }
}
