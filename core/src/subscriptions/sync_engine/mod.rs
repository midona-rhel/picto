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
mod importing;
mod progress;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::AppSettings;
use crate::subscriptions::archive::subscription_query_archive_prefix;
use crate::subscriptions::types::OwnedSubscriptionDownloadAttemptUpsert;
use crate::subscriptions::gallery_dl_runner::{self, FailureKind, GalleryDlRunner, RunOptions};
use crate::subscriptions::import_policy::{
    collection_group_parts, preferred_import_name, validate_metadata_for_site,
};
use crate::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site, effective_inbox_limit,
    range_start_from_cursor, resolve_query_name,
};
use crate::tags::logging::{preview_tag_strings, summarize_tag_strings};

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
    pub metadata: gallery_dl_runner::ParsedMetadata,
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

        let cursor = compute_incremental_cursor(
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
            .and_then(|member| detect_gallery_dl_root(&member.file_path));
        let items: Vec<(
            PathBuf,
            Option<i64>,
            crate::ingest_queue::IngestQueueItemPayload,
            bool,
        )> = pc
            .members
            .into_iter()
            .map(|member| {
                let request = build_subscription_ingest_request(
                    subscription_id,
                    &member.file_path,
                    &member.metadata,
                    false,
                    0,
                );
                log_subscription_ingest_request_shape(
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
        metadata: &gallery_dl_runner::ParsedMetadata,
    ) {
        let cleanup_root = detect_gallery_dl_root(file_path);
        let request =
            build_subscription_ingest_request(subscription_id, file_path, metadata, false, 0);
        log_subscription_ingest_request_shape(
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

    /// Sync a single subscription query via gallery-dl.
    pub async fn sync_query(
        &mut self,
        subscription_run_id: Option<i64>,
        subscription_id: i64,
        query_id: i64,
        query_text: &str,
        query_display_name: Option<&str>,
        site_id: &str,
        post_limit: Option<u32>,
        completed_initial_run: bool,
        resume_cursor: Option<&str>,
        resume_strategy: Option<&str>,
        cancel: CancellationToken,
    ) -> SyncProgress {
        self.current_query_id = Some(query_id);
        self.current_query_name =
            Some(resolve_query_name(query_id, query_text, query_display_name));
        let mut progress = SyncProgress::default();
        let sub_id_str = subscription_id.to_string();

        // Record last_check_time at start; load existing counters so we accumulate across runs
        let (prior_files, prior_posts) = {
            let q = self.runtime_service().get_subscription_query(query_id).await;
            match q {
                Ok(Some(q)) => (q.files_found as usize, q.posts_found as usize),
                _ => (0, 0),
            }
        };
        progress.files_downloaded = prior_files;
        progress.posts_processed = prior_posts;
        {
            let now = Utc::now().to_rfc3339();
            let _ = self
                .runtime_service()
                .update_query_progress(query_id, &now, prior_files as i64, prior_posts as i64)
                .await;
        }
        let inbox_limit = effective_inbox_limit(self.settings.sub_inbox_pause_limit);

        let inbox_count = self
            .db
            .get_scope_counts()
            .map(|counts| counts.inbox.max(0) as u64)
            .unwrap_or(0);
        if inbox_count >= inbox_limit as u64 {
            progress.failure_kind = Some("inbox_full".to_string());
            self.emit_progress(
                &sub_id_str,
                &progress,
                &format!("Inbox cap reached ({inbox_limit}); waiting for review"),
            );
            progress.cancelled = true;
            return progress;
        }

        let sync_start = std::time::Instant::now();

        let resume_strategy = resume_strategy
            .map(str::to_string)
            .or_else(|| default_resume_strategy_for_site(site_id).map(str::to_string));
        let query_for_run = match (
            resume_cursor,
            resume_strategy.as_deref(),
            completed_initial_run,
        ) {
            (Some(cursor), Some(strategy), false) if !cursor.trim().is_empty() => {
                apply_resume_to_query(query_text, cursor.trim(), strategy)
            }
            _ => query_text.to_string(),
        };
        let url = match gallery_dl_runner::build_url(site_id, &query_for_run) {
            Some(u) => u,
            None => {
                progress.errors.push(format!("Unknown site: {site_id}"));
                return progress;
            }
        };

        info!(
            elapsed_ms = sync_start.elapsed().as_millis(),
            "sync_query: URL built"
        );

        let credential = self
            .load_run_credential(
                subscription_id,
                query_id,
                site_id,
                &url,
                &sub_id_str,
                &progress,
            )
            .await;
        let has_credential = credential.has_credential();
        let mut _domain_run_guard = None;
        if let Some(rate_limiter) = &self.rate_limiter {
            if let Some(domain) = gallery_dl_runner::extract_domain(&url) {
                _domain_run_guard = Some(rate_limiter.acquire_domain_run(&domain).await);
                rate_limiter.wait_for_slot(&domain).await;
            }
        }

        info!(
            elapsed_ms = sync_start.elapsed().as_millis(),
            "sync_query: credential loaded"
        );

        let archive_path = self.library_root.join("gdl-archive.sqlite3");
        let archive_prefix = subscription_query_archive_prefix(subscription_id, query_id);

        let abort_threshold = if completed_initial_run {
            Some(self.settings.sub_abort_threshold)
        } else {
            None
        };

        // For range_offset strategy, compute the starting post index from cursor
        let range_start = if !completed_initial_run {
            range_start_from_cursor(resume_cursor, resume_strategy.as_deref())
        } else {
            1
        };

        info!(
            elapsed_ms = sync_start.elapsed().as_millis(),
            "sync_query: pre-spawn ready"
        );
        let query_run_id = self
            .runtime_service()
            .create_subscription_query_run(subscription_run_id, subscription_id, query_id)
            .await
            .ok();

        self.set_phase("starting");
        self.emit_progress(
            &sub_id_str,
            &progress,
            &format!("Starting gallery-dl for '{}'...", query_text),
        );

        let use_archive = completed_initial_run;
        info!(
            query_id,
            url = %url,
            post_limit = ?post_limit,
            range_start,
            abort_threshold = ?abort_threshold,
            completed_initial_run,
            use_archive,
            archive_prefix = %archive_prefix,
            resume_cursor = ?resume_cursor,
            resume_strategy = ?resume_strategy,
            has_credential,
            sleep_request = self.settings.sub_rate_limit_secs,
            "sync_query: run options"
        );

        let opts = RunOptions {
            subscription_id: Some(subscription_id),
            query_id: Some(query_id),
            site_id: site_id.to_string(),
            url: url.clone(),
            post_limit,
            range_start,
            abort_threshold,
            sleep_request: self.settings.sub_rate_limit_secs,
            auth: credential.gallery_dl_auth.clone(),
            archive_path: if use_archive {
                archive_path
            } else {
                PathBuf::new()
            },
            archive_prefix: if use_archive {
                Some(archive_prefix)
            } else {
                None
            },
            cancel: cancel.clone(),
        };

        // ── Streaming queue handoff: downloads are enqueued for background ingest ──
        let (item_tx, mut item_rx) =
            tokio::sync::mpsc::channel::<gallery_dl_runner::DownloadedItem>(32);

        let runner_handle = {
            let runner = gallery_dl_runner::GalleryDlRunner::new(self.runner.binary_path().clone());
            tokio::spawn(async move { runner.run(&opts, item_tx).await })
        };

        let mut pending_collections: HashMap<String, PendingCollection> = HashMap::new();
        let mut current_pending_collection_key: Option<String> = None;
        let mut all_post_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut total_items: usize = 0;
        let mut posts_processed_this_run: usize = 0;

        info!(query_id, "sync_query: waiting for gallery-dl items...");

        while let Some(item) = item_rx.recv().await {
            if cancel.is_cancelled() {
                info!(query_id, "sync_query: cancelled by user");
                progress.cancelled = true;
                break;
            }
            let inbox_count = self
                .db
                .get_scope_counts()
                .map(|counts| counts.inbox.max(0) as u64)
                .unwrap_or(0);
            if inbox_count >= inbox_limit as u64 {
                info!(
                    query_id,
                    inbox_count, inbox_limit, "sync_query: inbox full, stopping"
                );
                progress.failure_kind = Some("inbox_full".to_string());
                progress.cancelled = true;
                break;
            }

            let file_path_display = item.file_path.display().to_string();
            if let Err(e) = validate_metadata_for_site(site_id, &item.metadata) {
                info!(query_id, path = %file_path_display, error = %e, "sync_query: metadata validation failed, skipping");
                progress.metadata_invalid += 1;
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    "malformed_metadata",
                    "gallery-dl item metadata failed validation",
                    Some(&e),
                )
                .await;
                continue;
            }
            progress.metadata_validated += 1;
            total_items += 1;

            if let Some(pid) = item.metadata.post_id.as_deref() {
                all_post_ids.insert(pid.to_string());
            }

            let collection_parts = collection_group_parts(site_id, &item.metadata);
            let is_collection_member = self.auto_collections && collection_parts.is_some();

            let post_id_display = item.metadata.post_id.as_deref().unwrap_or("unknown");

            info!(
                post_id = post_id_display,
                auto_collections = self.auto_collections,
                has_collection_parts = collection_parts.is_some(),
                is_collection_member,
                "sync_engine: routing item"
            );

            if is_collection_member {
                let (category, post_id, preferred_name) = collection_parts.unwrap();
                let key = format!("{category}:{post_id}");

                if current_pending_collection_key
                    .as_deref()
                    .is_some_and(|current| current != key)
                {
                    if let Some(previous_key) = current_pending_collection_key.take() {
                        self.flush_pending_collection(
                            &previous_key,
                            &mut pending_collections,
                            subscription_id,
                            query_id,
                            query_run_id,
                            &sub_id_str,
                            &mut progress,
                        )
                        .await;
                    }
                    self.finalize_current_post_progress(
                        query_id,
                        &mut progress,
                        &mut posts_processed_this_run,
                        completed_initial_run,
                        &resume_strategy,
                        range_start,
                        &all_post_ids,
                    )
                    .await;
                }

                let is_new_post = progress.current_post_id.as_deref() != Some(&post_id);
                if is_new_post {
                    progress.current_post_id = Some(post_id.clone());
                    progress.current_post_items = 0;
                }
                current_pending_collection_key = Some(key.clone());
                progress.current_post_items += 1;
                progress.files_downloaded += 1;

                let short_id = if post_id.len() > 4 {
                    &post_id[post_id.len() - 4..]
                } else {
                    &post_id
                };
                self.set_phase("stashing");
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!(
                        "Stashing post ..{short_id} ({})",
                        progress.current_post_items
                    ),
                );
                let page_count = item.metadata.page_count.unwrap_or(0);
                let page_num = item.metadata.page_num.unwrap_or(u32::MAX);

                let pending =
                    pending_collections
                        .entry(key.clone())
                        .or_insert_with(|| PendingCollection {
                            category,
                            post_id,
                            preferred_name,
                            expected_count: None,
                            members: Vec::new(),
                        });
                pending.expected_count = Some(pending.expected_count.unwrap_or(0).max(page_count))
                    .filter(|count| *count > 0);

                pending.members.push(PendingMember {
                    file_path: item.file_path,
                    metadata: item.metadata,
                    page_num,
                });
            } else {
                if let Some(previous_key) = current_pending_collection_key.take() {
                    self.flush_pending_collection(
                        &previous_key,
                        &mut pending_collections,
                        subscription_id,
                        query_id,
                        query_run_id,
                        &sub_id_str,
                        &mut progress,
                    )
                    .await;
                    self.finalize_current_post_progress(
                        query_id,
                        &mut progress,
                        &mut posts_processed_this_run,
                        completed_initial_run,
                        &resume_strategy,
                        range_start,
                        &all_post_ids,
                    )
                    .await;
                }

                progress.current_post_id = Some(
                    item.metadata
                        .post_id
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                progress.current_post_items = 1;

                // Single image (or multi-image with auto_collections off): import immediately
                self.set_phase("queueing");
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Queueing {post_id_display}..."),
                );

                // When auto_collections is off and this is a multi-image post,
                // append _p{N} suffix so each page has a distinct name.
                let is_multi = item.metadata.page_count.map_or(false, |c| c > 1);
                let import_metadata;
                let metadata_ref = if !self.auto_collections && is_multi {
                    let base = preferred_import_name(&item.metadata).unwrap_or_else(|| {
                        format!(
                            "{}_{}",
                            item.metadata.category.as_deref().unwrap_or("unknown"),
                            post_id_display,
                        )
                    });
                    let page = item.metadata.page_num.map(|n| n + 1).unwrap_or(1);
                    import_metadata = {
                        let mut m = item.metadata.clone();
                        m.title = Some(format!("{}_p{}", base, page));
                        m
                    };
                    &import_metadata
                } else {
                    &item.metadata
                };

                self.enqueue_single_subscription_item(
                    subscription_id,
                    query_id,
                    query_run_id,
                    &item.file_path,
                    metadata_ref,
                )
                .await;
                progress.queued_for_ingest += 1;
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Queued {} files for ingest", progress.queued_for_ingest),
                );

                self.finalize_current_post_progress(
                    query_id,
                    &mut progress,
                    &mut posts_processed_this_run,
                    completed_initial_run,
                    &resume_strategy,
                    range_start,
                    &all_post_ids,
                )
                .await;
            }
        }

        // Wait for gallery-dl to finish BEFORE finalizing collections — we need
        // the cancel token and run summary to decide whether to materialize.
        let run_summary = match runner_handle.await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                progress.errors.push(format!("gallery-dl failed: {e}"));
                progress.failure_kind = Some("unknown".to_string());
                self.note_runtime_error(site_id, has_credential, Some(&e))
                    .await;
                let _ = self
                    .runtime_service()
                    .set_query_terminal_state(
                        query_id,
                        None,
                        Some(Utc::now().to_rfc3339()),
                        progress.failure_kind.clone(),
                        progress.errors.last().cloned(),
                    )
                    .await;
                // Don't materialize incomplete collections on runner failure
                let _ = self
                    .db
                    .mark_all_pending_ingest_stale_for_subscription(subscription_id)
                    .await;
                if let Some(query_run_id) = query_run_id {
                    let _ = self
                        .runtime_service()
                        .finish_subscription_query_run(
                            query_run_id,
                            "failed",
                            progress.failure_kind.clone(),
                            progress.errors.last().cloned(),
                            progress.posts_processed as i64,
                            progress.files_downloaded as i64,
                            progress.files_skipped as i64,
                        )
                        .await;
                }
                return progress;
            }
            Err(e) => {
                progress
                    .errors
                    .push(format!("gallery-dl task panicked: {e}"));
                progress.failure_kind = Some("unknown".to_string());
                let _ = self
                    .runtime_service()
                    .set_query_terminal_state(
                        query_id,
                        None,
                        Some(Utc::now().to_rfc3339()),
                        progress.failure_kind.clone(),
                        progress.errors.last().cloned(),
                    )
                    .await;
                let _ = self
                    .db
                    .mark_all_pending_ingest_stale_for_subscription(subscription_id)
                    .await;
                if let Some(query_run_id) = query_run_id {
                    let _ = self
                        .runtime_service()
                        .finish_subscription_query_run(
                            query_run_id,
                            "failed",
                            progress.failure_kind.clone(),
                            progress.errors.last().cloned(),
                            progress.posts_processed as i64,
                            progress.files_downloaded as i64,
                            progress.files_skipped as i64,
                        )
                        .await;
                }
                return progress;
            }
        };

        if cancel.is_cancelled() {
            progress.cancelled = true;
        }
        let mut failed_post_members: HashMap<String, Vec<gallery_dl_runner::ParsedMetadata>> =
            HashMap::new();
        if run_summary.had_download_errors {
            progress.failure_kind = Some("download_error".to_string());
            progress
                .errors
                .push("One or more subscription downloads failed after retries".to_string());
            for failed in &run_summary.failed_items {
                self.persist_failed_download_attempt(
                    subscription_id,
                    query_id,
                    query_run_id,
                    failed,
                )
                .await;
                if let Some((category, post_id, _)) =
                    collection_group_parts(site_id, &failed.metadata)
                {
                    let key = format!("{category}:{post_id}");
                    failed_post_members
                        .entry(key)
                        .or_default()
                        .push(failed.metadata.clone());
                }
            }
            self.record_issue(
                subscription_id,
                Some(query_id),
                "download_failure",
                "One or more subscription items failed after gallery-dl retries",
                run_summary
                    .failed_items
                    .first()
                    .map(|item| item.error_message.as_str()),
            )
            .await;
        }
        if run_summary.exit_code != 0 && !progress.cancelled {
            let failure_kind = gallery_dl_runner::classify_failure(&run_summary.stderr_output);
            let failure_kind_str = match failure_kind {
                FailureKind::Unauthorized => "unauthorized",
                FailureKind::Expired => "expired",
                FailureKind::RateLimited => "rate_limited",
                FailureKind::Network => "network",
                FailureKind::Unknown => "unknown",
            };
            progress.failure_kind = Some(failure_kind_str.to_string());
            let summary = format!(
                "gallery-dl exited with code {} ({failure_kind_str})",
                run_summary.exit_code
            );
            progress.errors.push(summary.clone());
            let health_status = match failure_kind {
                FailureKind::Unauthorized => {
                    Some(crate::subscriptions::credential_service::AuthFailureKind::Unauthorized)
                }
                FailureKind::Expired => {
                    Some(crate::subscriptions::credential_service::AuthFailureKind::Expired)
                }
                _ => None,
            };
            let err = run_summary
                .stderr_output
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(summary.as_str())
                .trim()
                .to_string();
            warn!(site_id = %site_id, query_id, failure_kind = failure_kind_str, error = %err, "gallery-dl query execution failed");
            if let Some(kind) = health_status {
                self.note_run_auth_failure(subscription_id, query_id, site_id, kind, Some(&err))
                    .await;
            } else {
                self.note_runtime_error(site_id, has_credential, Some(&err))
                    .await;
            }
            if health_status.is_none() {
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    failure_kind_str,
                    &summary,
                    Some(&err),
                )
                .await;
            }
        } else if run_summary.exit_code == 0 && has_credential {
            self.note_run_success(subscription_id, query_id, site_id, has_credential)
                .await;
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(subscription_id, Some(query_id), "unauthorized")
                .await;
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(subscription_id, Some(query_id), "expired")
                .await;
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(subscription_id, Some(query_id), "rate_limited")
                .await;
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(subscription_id, Some(query_id), "network")
                .await;
        }

        // Finalize pending collections — only after we know the full run outcome.
        // If cancelled or errored, mark queue entries as stale instead of materializing
        // incomplete collections.
        info!(
            query_id,
            total_items,
            pending_collections = pending_collections.len(),
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            cancelled = progress.cancelled,
            "sync_query: finalizing pending collections"
        );
        if !progress.cancelled {
            if let Some(last_key) = current_pending_collection_key.take() {
                self.flush_pending_collection(
                    &last_key,
                    &mut pending_collections,
                    subscription_id,
                    query_id,
                    query_run_id,
                    &sub_id_str,
                    &mut progress,
                )
                .await;
                self.finalize_current_post_progress(
                    query_id,
                    &mut progress,
                    &mut posts_processed_this_run,
                    completed_initial_run,
                    &resume_strategy,
                    range_start,
                    &all_post_ids,
                )
                .await;
            }

            let bridge_discovered_without_downloads = !completed_initial_run
                && !progress.cancelled
                && run_summary.exit_code == 0
                && run_summary.discovered_items > 0
                && total_items == 0;
            if bridge_discovered_without_downloads {
                let detail = format!(
                "gallery-dl discovered {} items across {} posts but produced no downloadable files",
                run_summary.discovered_items,
                run_summary.discovered_post_ids.len(),
            );
                warn!(
                    query_id,
                    discovered_items = run_summary.discovered_items,
                    discovered_posts = run_summary.discovered_post_ids.len(),
                    skipped_archive_items = run_summary.skipped_archive_items,
                    "sync_query: bridge discovered items but no downloads were emitted"
                );
                progress.failure_kind = Some("bridge_no_downloads".to_string());
                progress.errors.push(detail.clone());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    "bridge_no_downloads",
                    "gallery-dl discovered items but emitted no downloads",
                    Some(&detail),
                )
                .await;
            }
            for failed_members in failed_post_members.into_values() {
                for failed in failed_members {
                    self.persist_post_member_state(
                        subscription_id,
                        site_id,
                        &failed,
                        None,
                        "failed",
                    )
                    .await;
                }
            }
        } else {
            // Cancelled or errored — don't materialize incomplete collections.
            // Mark queue entries as stale for potential later recovery.
            let _ = self
                .db
                .mark_all_pending_ingest_stale_for_subscription(subscription_id)
                .await;
        }

        maybe_cleanup_subscription_temp_root(self.db, &run_summary.temp_dir).await;

        self.set_phase("finalizing");
        self.emit_progress_force(&sub_id_str, &progress, "Finalizing...");
        let completed_cleanly =
            run_summary.exit_code == 0 && !progress.cancelled && progress.errors.is_empty();
        // Resume cursor based on ACTUAL posts processed, not the theoretical range end.
        // This prevents skipping posts that weren't downloaded due to errors or cancellation.
        let next_resume_cursor = compute_incremental_cursor(
            resume_strategy.as_deref(),
            range_start,
            posts_processed_this_run,
            &all_post_ids,
        );
        let unique_post_count = all_post_ids.len();
        let continue_initial_pagination = should_continue_initial_pagination(
            completed_initial_run,
            completed_cleanly,
            post_limit,
            unique_post_count,
            next_resume_cursor.as_deref(),
        );

        if !completed_initial_run {
            let persisted_cursor = if completed_cleanly {
                if continue_initial_pagination {
                    next_resume_cursor.clone()
                } else {
                    None
                }
            } else {
                next_resume_cursor
                    .clone()
                    .or_else(|| resume_cursor.map(|s| s.to_string()))
            };
            let _ = self
                .runtime_service()
                .set_query_resume_state(query_id, persisted_cursor, resume_strategy.clone())
                .await;
        }

        if completed_cleanly {
            let now = Utc::now().to_rfc3339();
            let _ = self
                .runtime_service()
                .update_query_progress(
                    query_id,
                    &now,
                    progress.files_downloaded as i64,
                    progress.posts_processed as i64,
                )
                .await;
        }

        info!(
            query_id,
            completed_cleanly,
            completed_initial_run,
            continue_initial_pagination,
            unique_post_count,
            next_resume_cursor = ?next_resume_cursor,
            exit_code = run_summary.exit_code,
            "sync_query: pagination decision"
        );

        if !completed_initial_run && completed_cleanly && !continue_initial_pagination {
            info!(query_id, "sync_query: marking initial run as complete");
            let _ = self
                .runtime_service()
                .set_query_completed_initial_run(query_id, true)
                .await;
            let _ = self
                .runtime_service()
                .set_query_resume_state(query_id, None, None)
                .await;
        } else if continue_initial_pagination {
            info!(query_id, next_resume_cursor = ?next_resume_cursor, fetched_items = total_items,
                post_limit = ?post_limit, "sync_query: initial run continues; resuming next chunk");
        }

        info!(
            query_id,
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            exit_code = run_summary.exit_code,
            cancelled = progress.cancelled,
            stderr_lines = run_summary.stderr_output.lines().count(),
            "sync_query: finished"
        );

        if let Some(query_run_id) = query_run_id {
            let status = if progress.cancelled {
                "cancelled"
            } else if progress.errors.is_empty() {
                "succeeded"
            } else {
                "failed"
            };
            let _ = self
                .runtime_service()
                .finish_subscription_query_run(
                    query_run_id,
                    status,
                    progress.failure_kind.clone(),
                    progress.errors.last().cloned(),
                    progress.posts_processed as i64,
                    progress.files_downloaded as i64,
                    progress.files_skipped as i64,
                )
                .await;
        }

        if progress.errors.is_empty() && !progress.cancelled {
            let _ = self
                .runtime_service()
                .set_query_terminal_state(query_id, Some(Utc::now().to_rfc3339()), None, None, None)
                .await;
        } else if !progress.cancelled {
            let _ = self
                .runtime_service()
                .set_query_terminal_state(
                    query_id,
                    None,
                    Some(Utc::now().to_rfc3339()),
                    progress.failure_kind.clone(),
                    progress.errors.last().cloned(),
                )
                .await;
        }

        progress
    }

    pub async fn retry_failed_post(
        &mut self,
        subscription_id: i64,
        query_id: i64,
        site_id: &str,
        retry_url: &str,
        expected_post_id: &str,
        cancel: CancellationToken,
    ) -> SyncProgress {
        self.current_query_id = Some(query_id);
        self.current_query_name = Some(format!("retry:{expected_post_id}"));
        let mut progress = SyncProgress::default();
        let sub_id_str = subscription_id.to_string();

        let credential = self
            .load_run_credential(
                subscription_id,
                query_id,
                site_id,
                retry_url,
                &sub_id_str,
                &progress,
            )
            .await;
        let mut _domain_run_guard = None;
        if let Some(rate_limiter) = &self.rate_limiter {
            if let Some(domain) = gallery_dl_runner::extract_domain(retry_url) {
                _domain_run_guard = Some(rate_limiter.acquire_domain_run(&domain).await);
                rate_limiter.wait_for_slot(&domain).await;
            }
        }

        let query_run_id = self
            .runtime_service()
            .create_subscription_query_run(None, subscription_id, query_id)
            .await
            .ok();

        let opts = RunOptions {
            subscription_id: Some(subscription_id),
            query_id: Some(query_id),
            site_id: site_id.to_string(),
            url: retry_url.to_string(),
            post_limit: None,
            range_start: 1,
            abort_threshold: None,
            sleep_request: self.settings.sub_rate_limit_secs,
            auth: credential.gallery_dl_auth.clone(),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: cancel.clone(),
        };

        let (item_tx, mut item_rx) =
            tokio::sync::mpsc::channel::<gallery_dl_runner::DownloadedItem>(32);
        let runner_handle = {
            let runner = gallery_dl_runner::GalleryDlRunner::new(self.runner.binary_path().clone());
            tokio::spawn(async move { runner.run(&opts, item_tx).await })
        };

        let mut pending_collections: HashMap<String, PendingCollection> = HashMap::new();
        let mut changed_collection_ids: Vec<i64> = Vec::new();

        while let Some(item) = item_rx.recv().await {
            if cancel.is_cancelled() {
                progress.cancelled = true;
                break;
            }
            if let Some(post_id) = item.metadata.post_id.as_deref() {
                if post_id != expected_post_id {
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        "unexpected_retry_item",
                        &format!(
                            "Retry for post {expected_post_id} yielded item from post {post_id}"
                        ),
                        item.metadata.canonical_post_url.as_deref(),
                    )
                    .await;
                    continue;
                }
            }
            if let Err(error) = validate_metadata_for_site(site_id, &item.metadata) {
                progress.metadata_invalid += 1;
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    "malformed_metadata",
                    "gallery-dl item metadata failed validation during retry",
                    Some(&error),
                )
                .await;
                continue;
            }
            progress.metadata_validated += 1;
            let collection_parts = collection_group_parts(site_id, &item.metadata);
            let is_collection_member = self.auto_collections && collection_parts.is_some();
            if is_collection_member {
                let (category, post_id, preferred_name) = collection_parts.unwrap();
                let key = format!("{category}:{post_id}");
                let pending = pending_collections
                    .entry(key)
                    .or_insert_with(|| PendingCollection {
                        category,
                        post_id,
                        preferred_name,
                        expected_count: None,
                        members: Vec::new(),
                    });
                pending.expected_count = Some(
                    pending
                        .expected_count
                        .unwrap_or(0)
                        .max(item.metadata.page_count.unwrap_or(0)),
                )
                .filter(|count| *count > 0);
                let page_num = item.metadata.page_num.unwrap_or(u32::MAX);
                pending.members.push(PendingMember {
                    file_path: item.file_path,
                    metadata: item.metadata,
                    page_num,
                });
                continue;
            }

            match self
                .import_item(
                    &item.file_path,
                    &item.metadata,
                    subscription_id,
                    retry_url,
                    false,
                )
                .await
            {
                Ok(outcome) => {
                    if let Some(item_key) = metadata_item_key(&item.metadata) {
                        let _ = self
                            .runtime_service()
                            .resolve_subscription_download_attempt(
                                subscription_id,
                                Some(query_id),
                                &item_key,
                            )
                            .await;
                    }
                    self.persist_post_member_state(
                        subscription_id,
                        site_id,
                        &item.metadata,
                        Some(outcome.entity_hash.as_str()),
                        "imported",
                    )
                    .await;
                    if outcome.imported {
                        progress.files_downloaded += 1;
                    } else {
                        progress.files_skipped += 1;
                    }
                }
                Err(error) => {
                    progress.errors.push(error.clone());
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        "import_failure",
                        &format!("Retry import failed for post {expected_post_id}"),
                        Some(&error),
                    )
                    .await;
                }
            }
        }

        let run_summary = match runner_handle.await {
            Ok(Ok(summary)) => summary,
            Ok(Err(error)) => {
                progress
                    .errors
                    .push(format!("gallery-dl retry failed: {error}"));
                progress.failure_kind = Some("unknown".to_string());
                return progress;
            }
            Err(error) => {
                progress
                    .errors
                    .push(format!("gallery-dl retry task panicked: {error}"));
                progress.failure_kind = Some("unknown".to_string());
                return progress;
            }
        };

        let mut failed_post_members: HashMap<String, Vec<gallery_dl_runner::ParsedMetadata>> =
            HashMap::new();
        if run_summary.had_download_errors {
            progress.failure_kind = Some("download_error".to_string());
            for failed in &run_summary.failed_items {
                self.persist_failed_download_attempt(
                    subscription_id,
                    query_id,
                    query_run_id,
                    failed,
                )
                .await;
                if let Some((category, post_id, _)) =
                    collection_group_parts(site_id, &failed.metadata)
                {
                    let key = format!("{category}:{post_id}");
                    failed_post_members
                        .entry(key)
                        .or_default()
                        .push(failed.metadata.clone());
                }
            }
        }

        if !progress.cancelled {
            for (key, pc) in pending_collections {
                let failed_members = failed_post_members.remove(&key).unwrap_or_default();
                self.materialize_collection(
                    pc,
                    subscription_id,
                    &sub_id_str,
                    &mut progress,
                    &mut changed_collection_ids,
                    &failed_members,
                )
                .await;
            }
        }

        if !changed_collection_ids.is_empty() {
            changed_collection_ids.sort_unstable();
            changed_collection_ids.dedup();
            let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new();
            if let Ok(state) = crate::state::get_state() {
                for collection_id in &changed_collection_ids {
                    let folder_ids = state
                        .engine
                        .db()
                        .get_collection_folder_ids(*collection_id)
                        .unwrap_or_default();
                    impact = impact.merge(
                        crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
                            *collection_id,
                            &folder_ids,
                        ),
                    );
                }
            }
            crate::events::emit_state_changed("subscription_retry_post", impact);
        }

        gallery_dl_runner::cleanup_temp_dir(&run_summary.temp_dir).await;
        if run_summary.failed_items.is_empty() {
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(subscription_id, Some(query_id), "download_failure")
                .await;
        }
        if let Some(query_run_id) = query_run_id {
            let status = if progress.cancelled {
                "cancelled"
            } else if progress.errors.is_empty() && run_summary.failed_items.is_empty() {
                "succeeded"
            } else {
                "failed"
            };
            let _ = self
                .runtime_service()
                .finish_subscription_query_run(
                    query_run_id,
                    status,
                    progress.failure_kind.clone(),
                    progress.errors.last().cloned(),
                    progress.posts_processed as i64,
                    progress.files_downloaded as i64,
                    progress.files_skipped as i64,
                )
                .await;
        }
        progress
    }

    async fn persist_post_member_state(
        &self,
        subscription_id: i64,
        site_id: &str,
        metadata: &gallery_dl_runner::ParsedMetadata,
        entity_hash: Option<&str>,
        status: &str,
    ) {
        let Some(post_id) = metadata
            .post_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };
        let category = metadata
            .category
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(site_id);
        let item_key = metadata_item_key(metadata)
            .unwrap_or_else(|| format!("{category}:{post_id}:{}", metadata.page_num.unwrap_or(0)));
        let _ = self
            .runtime_service()
            .upsert_subscription_post_member(
                crate::subscriptions::types::OwnedSubscriptionPostMemberUpsert {
                    subscription_id,
                    site_id: category.to_string(),
                    post_id: post_id.to_string(),
                    item_key,
                    page_num: metadata.page_num.map(i64::from),
                    canonical_post_url: metadata.canonical_post_url.clone(),
                    media_url: metadata.media_url.clone(),
                    entity_hash: entity_hash.map(ToOwned::to_owned),
                    status: status.to_string(),
                },
            )
            .await;
    }

    async fn reconcile_post_collection_order(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
        collection_id: i64,
    ) {
        let members = match self
            .runtime_service()
            .list_subscription_post_members(subscription_id, site_id, post_id)
            .await
        {
            Ok(members) => members,
            Err(_) => return,
        };
        let ordered_hashes: Vec<String> = members
            .into_iter()
            .filter(|member| member.status == "imported")
            .filter_map(|member| member.entity_hash)
            .collect();
        if ordered_hashes.is_empty() {
            return;
        }
        if let Ok(state) = crate::state::get_state() {
            let _ = state
                .engine
                .db()
                .reorder_collection_members_by_hashes(collection_id, &ordered_hashes);
        }
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

    async fn persist_failed_download_attempt(
        &self,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        failed: &gallery_dl_runner::FailedDownloadedItem,
    ) {
        let next_retry_at = (Utc::now() + Duration::minutes(15)).to_rfc3339();
        let metadata = &failed.metadata;
        let item_key = metadata_item_key(metadata)
            .unwrap_or_else(|| format!("unknown:{}:{}", query_id, Utc::now().timestamp_millis()));
        let _ = self
            .runtime_service()
            .upsert_subscription_download_attempt(OwnedSubscriptionDownloadAttemptUpsert {
                subscription_id,
                query_id: Some(query_id),
                query_run_id,
                item_key,
                site_category: metadata.category.clone(),
                post_id: metadata.post_id.clone(),
                page_num: metadata.page_num.map(i64::from),
                canonical_post_url: metadata.canonical_post_url.clone(),
                media_url: metadata.media_url.clone(),
                retry_url: metadata
                    .canonical_post_url
                    .clone()
                    .or_else(|| metadata.source_url.clone()),
                failure_kind: Some("download_failure".to_string()),
                last_error: Some(failed.error_message.clone()),
                next_retry_at: Some(next_retry_at),
            })
            .await;
        let site_id = metadata.category.as_deref().unwrap_or("unknown");
        self.persist_post_member_state(subscription_id, site_id, metadata, None, "failed")
            .await;
    }
}

fn metadata_item_key(metadata: &gallery_dl_runner::ParsedMetadata) -> Option<String> {
    metadata.item_key.clone().or_else(|| {
        let category = metadata.category.as_deref()?;
        let target = metadata
            .post_id
            .as_deref()
            .or(metadata.canonical_post_url.as_deref())
            .or(metadata.media_url.as_deref())?;
        Some(format!(
            "{category}:{target}:{}",
            metadata.page_num.unwrap_or(0)
        ))
    })
}

fn build_subscription_ingest_request(
    subscription_id: i64,
    file_path: &std::path::Path,
    metadata: &gallery_dl_runner::ParsedMetadata,
    skip_thumbnail: bool,
    initial_status: i64,
) -> crate::ingest::SingleIngestRequest {
    crate::ingest::SingleIngestRequest {
        source_kind: crate::ingest::IngestSourceKind::Subscription,
        path: file_path.to_path_buf(),
        tag_strings: crate::ingest::normalize_subscription_tags(metadata),
        source_urls: crate::ingest::dedupe_urls(metadata.source_urls.clone()),
        name: preferred_import_name(metadata),
        notes: crate::ingest::metadata_notes_text(metadata),
        created_at: metadata.created_at.clone(),
        initial_status,
        skip_thumbnail,
        tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
        subscription_id: Some(subscription_id),
    }
}

fn log_subscription_ingest_request_shape(
    query_id: i64,
    subscription_id: i64,
    metadata: &gallery_dl_runner::ParsedMetadata,
    tag_strings: &[String],
) {
    let summary = summarize_tag_strings(tag_strings);
    info!(
        query_id,
        subscription_id,
        post_id = metadata.post_id.as_deref().unwrap_or("?"),
        category = metadata.category.as_deref().unwrap_or("?"),
        item_key = metadata.item_key.as_deref().unwrap_or("?"),
        request_tag_count = summary.total,
        request_creator_tag_count = summary.creator,
        request_character_tag_count = summary.character,
        request_series_tag_count = summary.series,
        request_general_tag_count = summary.general,
        request_meta_tag_count = summary.meta,
        request_other_namespaced_tag_count = summary.other_namespaced,
        request_tag_preview = ?preview_tag_strings(tag_strings, 5),
        "subscription ingest request built"
    );
}

fn detect_gallery_dl_root(path: &std::path::Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let name = ancestor.file_name()?.to_str()?;
        if name.starts_with("picto_gdl_") {
            Some(ancestor.to_path_buf())
        } else {
            None
        }
    })
}

async fn maybe_cleanup_subscription_temp_root(db: &LibraryDatabase, temp_root: &std::path::Path) {
    match db.has_retained_ingest_sources_for_root(temp_root).await {
        Ok(true) => {}
        Ok(false) => gallery_dl_runner::cleanup_temp_dir(temp_root).await,
        Err(error) => {
            warn!(path = %temp_root.display(), error = %error, "Failed to inspect temp-root ingest ownership")
        }
    }
}

/// Compute the resume cursor based on posts actually processed in THIS run.
/// `posts_this_run` must be the count of new posts processed (not accumulated total).
/// Returns None if nothing was processed — caller should not advance the cursor.
fn compute_incremental_cursor(
    resume_strategy: Option<&str>,
    range_start: u32,
    posts_this_run: usize,
    all_post_ids: &std::collections::HashSet<String>,
) -> Option<String> {
    if posts_this_run == 0 {
        return None; // Don't advance cursor if nothing was downloaded
    }
    match resume_strategy {
        Some("range_offset") => Some((range_start as usize + posts_this_run - 1).to_string()),
        Some("tag_id_lt") => {
            let mut min_id: Option<u64> = None;
            for pid in all_post_ids {
                if let Ok(n) = pid.parse::<u64>() {
                    min_id = Some(min_id.map_or(n, |cur| cur.min(n)));
                }
            }
            min_id.map(|id| id.to_string())
        }
        _ => None,
    }
}

fn should_continue_initial_pagination(
    completed_initial_run: bool,
    completed_cleanly: bool,
    post_limit: Option<u32>,
    fetched_items: usize,
    next_resume_cursor: Option<&str>,
) -> bool {
    if completed_initial_run || !completed_cleanly {
        return false;
    }
    let Some(limit) = post_limit else {
        return false;
    };
    if limit == 0 || fetched_items < limit as usize {
        return false;
    }
    next_resume_cursor.is_some_and(|cursor| !cursor.trim().is_empty())
}
