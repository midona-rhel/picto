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
mod importing;
mod persistence;
mod progress;
mod query;
mod retry;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::AppSettings;
use crate::subscriptions::gallery_dl_runner::GalleryDlRunner;
use crate::subscriptions::import_policy::preferred_import_name;
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

pub struct SubscriptionSyncEngine<'a> {
    db: &'a LibraryDatabase,
    blob_store: &'a BlobStore,
    library_root: PathBuf,
    rate_limiter: Option<RateLimiter>,
    runner: GalleryDlRunner,
    settings: AppSettings,
    subscription_name: String,
    progress_mode: String,
    group_name: Option<String>,
    current_query_id: Option<i64>,
    current_query_name: Option<String>,
    current_phase: String,
    last_progress_emit: std::time::Instant,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
    auto_merge_require_matching_dimensions: bool,
    auto_collections: bool,
}

pub(super) struct PendingMember {
    pub file_path: PathBuf,
    pub metadata: ParsedMetadata,
    pub page_num: u32,
}

pub(super) struct PendingCollection {
    pub category: String,
    pub post_id: String,
    pub preferred_name: String,
    pub expected_count: Option<u32>,
    pub members: Vec<PendingMember>,
}

impl<'a> SubscriptionSyncEngine<'a> {
    pub fn new(
        db: &'a LibraryDatabase,
        blob_store: &'a BlobStore,
        settings: &AppSettings,
        library_root: &Path,
    ) -> Result<Self, String> {
        let binary_path = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
        let runner = GalleryDlRunner::new(binary_path);

        Ok(Self {
            db,
            blob_store,
            library_root: library_root.to_path_buf(),
            rate_limiter: None,
            runner,
            settings: settings.clone(),
            subscription_name: String::new(),
            progress_mode: "subscription".to_string(),
            group_name: None,
            current_query_id: None,
            current_query_name: None,
            current_phase: "starting".to_string(),
            last_progress_emit: std::time::Instant::now(),
            auto_merge_enabled: false,
            auto_merge_distance: crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD,
            auto_merge_require_matching_dimensions: false,
            auto_collections: true,
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

    pub fn with_group_name(mut self, group_name: Option<String>) -> Self {
        self.group_name = group_name;
        self
    }

    pub fn with_auto_collections(mut self, auto_collections: bool) -> Self {
        self.auto_collections = auto_collections;
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

    async fn finalize_current_post_progress(
        &self,
        query_id: i64,
        progress: &mut SyncProgress,
        posts_processed_this_run: &mut usize,
        completed_initial_run: bool,
        resume_strategy: &Option<String>,
        range_start: u32,
        all_post_ids: &std::collections::HashSet<String>,
    ) {
        if progress.current_post_id.is_none() {
            return;
        }

        progress.posts_processed += 1;
        *posts_processed_this_run += 1;

        let cursor = helpers::compute_incremental_cursor(
            resume_strategy.as_deref(),
            range_start,
            *posts_processed_this_run,
            all_post_ids,
        );
        progress.resume_cursor = cursor.clone();
        if !completed_initial_run {
            let _ = self
                .runtime_service()
                .set_query_resume_state(query_id, cursor, resume_strategy.clone())
                .await;
        }
        let _ = self
            .runtime_service()
            .update_query_progress(
                query_id,
                &Utc::now().to_rfc3339(),
                progress.files_downloaded as i64,
                progress.posts_processed as i64,
            )
            .await;

        progress.current_post_id = None;
        progress.current_post_items = 0;
    }

    async fn flush_pending_collection(
        &mut self,
        pending_key: &str,
        pending_collections: &mut HashMap<String, PendingCollection>,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        sub_id_str: &str,
        progress: &mut SyncProgress,
    ) {
        let Some(pc) = pending_collections.remove(pending_key) else {
            return;
        };
        self.enqueue_pending_collection(pc, subscription_id, query_id, query_run_id)
            .await;
        progress.queued_for_ingest += 1;
        self.set_phase("queueing");
        self.emit_progress_force(sub_id_str, progress, "Queued post for ingest");
    }

    async fn enqueue_pending_collection(
        &self,
        pc: PendingCollection,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
    ) {
        let cleanup_root = pc
            .members
            .first()
            .and_then(|member| helpers::detect_gallery_dl_root(&member.file_path));
        let items: Vec<(
            PathBuf,
            Option<i64>,
            crate::ingest_queue::IngestQueueItemPayload,
            bool,
        )> = pc
            .members
            .into_iter()
            .map(|member| {
                let request = helpers::build_subscription_ingest_request(
                    subscription_id,
                    &member.file_path,
                    &member.metadata,
                    false,
                    0,
                );
                helpers::log_subscription_ingest_request_shape(
                    query_id,
                    subscription_id,
                    &member.metadata,
                    &request.tag_strings,
                );
                (
                    member.file_path,
                    Some(member.page_num as i64),
                    crate::ingest_queue::IngestQueueItemPayload {
                        request,
                        subscription_metadata: Some(member.metadata),
                        target_folder_id: None,
                    },
                    true,
                )
            })
            .collect();
        let _ = self
            .db
            .enqueue_ingest_queue(
                crate::ingest_queue::IngestQueueKind::Collection,
                "subscription",
                Some(subscription_id),
                Some(query_id),
                query_run_id,
                cleanup_root.as_deref(),
                Some(&pc.post_id),
                Some(&pc.category),
                Some(&pc.preferred_name),
                pc.expected_count.map(i64::from),
                items,
            )
            .await;
    }

    async fn enqueue_single_subscription_item(
        &self,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        file_path: &std::path::Path,
        metadata: &ParsedMetadata,
    ) {
        let cleanup_root = helpers::detect_gallery_dl_root(file_path);
        let request =
            helpers::build_subscription_ingest_request(subscription_id, file_path, metadata, false, 0);
        helpers::log_subscription_ingest_request_shape(
            query_id,
            subscription_id,
            metadata,
            &request.tag_strings,
        );
        let _ = self
            .db
            .enqueue_ingest_queue(
                crate::ingest_queue::IngestQueueKind::Single,
                "subscription",
                Some(subscription_id),
                Some(query_id),
                query_run_id,
                cleanup_root.as_deref(),
                metadata.post_id.as_deref(),
                metadata.category.as_deref(),
                preferred_import_name(metadata).as_deref(),
                metadata.page_count.map(i64::from),
                vec![(
                    file_path.to_path_buf(),
                    metadata.page_num.map(i64::from),
                    crate::ingest_queue::IngestQueueItemPayload {
                        request,
                        subscription_metadata: Some(metadata.clone()),
                        target_folder_id: None,
                    },
                    true,
                )],
            )
            .await;
    }

    async fn record_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        issue_kind: &str,
        message: &str,
        detail: Option<&str>,
    ) {
        let _ = self
            .runtime_service()
            .upsert_subscription_issue(subscription_id, query_id, issue_kind, message, detail)
            .await;
    }
}
