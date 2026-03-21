//! Subscription sync engine — downloads files via gallery-dl subprocess.
//!
//! Steps per query:
//! 1. Build URL from subscription's gallery-dl URL template + query text
//! 2. Load credentials from OS keychain (if configured for that site)
//! 3. Spawn gallery-dl subprocess with appropriate flags
//! 4. Import each downloaded file via the existing ImportPipeline
//! 5. Merge metadata for already-imported files (tags, URLs, notes, name)
//! 6. Track `completed_initial_run` for smart-stop behavior

mod credentials;
mod importing;
mod progress;

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::blob_store::BlobStore;
use crate::settings::store::AppSettings;
use crate::sqlite::SqliteDatabase;
use crate::subscriptions::archive::subscription_query_archive_prefix;
use crate::subscriptions::gallery_dl_runner::{self, FailureKind, GalleryDlRunner, RunOptions};
use crate::subscriptions::import_policy::{collection_group_parts, preferred_import_name, validate_metadata_for_site};
use crate::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site,
    effective_inbox_limit, range_start_from_cursor, resolve_query_name,
};

#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    pub files_downloaded: usize,
    pub files_skipped: usize,
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
    db: &'a SqliteDatabase,
    blob_store: &'a BlobStore,
    runner: GalleryDlRunner,
    settings: AppSettings,
    subscription_name: String,
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
    pub expected: u32,
    pub members: Vec<PendingMember>,
    pub queue_id: Option<i64>,
}

impl<'a> SubscriptionSyncEngine<'a> {
    pub fn new(
        db: &'a SqliteDatabase,
        blob_store: &'a BlobStore,
        settings: &AppSettings,
    ) -> Result<Self, String> {
        let binary_path = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
        let runner = GalleryDlRunner::new(binary_path);

        Ok(Self {
            db,
            blob_store,
            runner,
            settings: settings.clone(),
            subscription_name: String::new(),
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

    pub fn with_name(mut self, name: String) -> Self {
        self.subscription_name = name;
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
            let q = self.db.get_subscription_query(query_id).await;
            match q {
                Ok(Some(q)) => (q.files_found as usize, q.posts_found as usize),
                _ => (0, 0),
            }
        };
        progress.files_downloaded = prior_files;
        progress.posts_processed = prior_posts;
        {
            let now = Utc::now().to_rfc3339();
            let _ = self.db.update_query_progress(query_id, &now, prior_files as i64, prior_posts as i64).await;
        }
        let inbox_limit = effective_inbox_limit(self.settings.sub_inbox_pause_limit);

        let inbox_count = self
            .db
            .bitmaps
            .len(&crate::sqlite::bitmaps::BitmapKey::Status(0));
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

        info!(elapsed_ms = sync_start.elapsed().as_millis(), "sync_query: URL built");

        let credential = self
            .load_run_credential(site_id, &url, &sub_id_str, &progress)
            .await;
        let has_credential = credential.is_some();

        info!(elapsed_ms = sync_start.elapsed().as_millis(), "sync_query: credential loaded");

        let archive_path = self
            .db
            .db_dir()
            .parent()
            .map(|r| r.join("gdl-archive.sqlite3"))
            .unwrap_or_else(|| PathBuf::from("gdl-archive.sqlite3"));
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

        info!(elapsed_ms = sync_start.elapsed().as_millis(), "sync_query: pre-spawn ready");

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
            url: url.clone(),
            post_limit,
            range_start,
            abort_threshold,
            sleep_request: self.settings.sub_rate_limit_secs,
            credential,
            archive_path: if use_archive { archive_path } else { PathBuf::new() },
            archive_prefix: if use_archive { Some(archive_prefix) } else { None },
            cancel: cancel.clone(),
        };

        // ── Streaming import: process files as gallery-dl downloads them ──
        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<gallery_dl_runner::DownloadedItem>(32);

        let runner_handle = {
            let runner = gallery_dl_runner::GalleryDlRunner::new(self.runner.binary_path().clone());
            tokio::spawn(async move { runner.run(&opts, item_tx).await })
        };

        let mut pending_collections: HashMap<String, PendingCollection> = HashMap::new();
        let mut changed_collection_ids: Vec<i64> = Vec::new();
        let mut all_post_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut total_items: usize = 0;

        info!(query_id, "sync_query: waiting for gallery-dl items...");

        while let Some(item) = item_rx.recv().await {
            if cancel.is_cancelled() {
                info!(query_id, "sync_query: cancelled by user");
                progress.cancelled = true;
                break;
            }
            let inbox_count = self
                .db
                .bitmaps
                .len(&crate::sqlite::bitmaps::BitmapKey::Status(0));
            if inbox_count >= inbox_limit as u64 {
                info!(query_id, inbox_count, inbox_limit, "sync_query: inbox full, stopping");
                progress.failure_kind = Some("inbox_full".to_string());
                progress.cancelled = true;
                break;
            }

            let file_path_display = item.file_path.display().to_string();
            if let Err(e) = validate_metadata_for_site(site_id, &item.metadata) {
                info!(query_id, path = %file_path_display, error = %e, "sync_query: metadata validation failed, skipping");
                progress.metadata_invalid += 1;
                continue;
            }
            progress.metadata_validated += 1;
            total_items += 1;

            if let Some(pid) = item.metadata.post_id.as_deref() {
                all_post_ids.insert(pid.to_string());
            }

            let collection_parts = collection_group_parts(site_id, &item.metadata);
            let is_collection_member =
                self.auto_collections && collection_parts.is_some();

            let post_id_display = item.metadata.post_id.as_deref().unwrap_or("unknown");

            info!(
                post_id = post_id_display,
                auto_collections = self.auto_collections,
                has_collection_parts = collection_parts.is_some(),
                is_collection_member,
                "sync_engine: routing item"
            );

            if is_collection_member {
                // Don't import yet — just stash the file for later.
                // Files are only imported once the full collection is ready.
                let (category, post_id, preferred_name) = collection_parts.unwrap();
                let key = format!("{category}:{post_id}");

                // Track current post for progress display
                let is_new_post = progress.current_post_id.as_deref() != Some(&post_id);
                if is_new_post {
                    if progress.current_post_id.is_some() {
                        progress.posts_processed += 1;

                        // Persist cursor + file count after each completed post
                        let cursor = compute_incremental_cursor(
                            resume_strategy.as_deref(), range_start, progress.posts_processed, &all_post_ids,
                        );
                        progress.resume_cursor = cursor.clone();
                        if !completed_initial_run {
                            let _ = self.db.set_query_resume_state(query_id, cursor, resume_strategy.clone()).await;
                        }
                        let _ = self.db.update_query_progress(query_id, &Utc::now().to_rfc3339(), progress.files_downloaded as i64, progress.posts_processed as i64).await;
                    }
                    progress.current_post_id = Some(post_id.clone());
                    progress.current_post_items = 0;
                }
                progress.current_post_items += 1;
                progress.files_downloaded += 1;

                let short_id = if post_id.len() > 4 { &post_id[post_id.len()-4..] } else { &post_id };
                self.set_phase("stashing");
                self.emit_progress(&sub_id_str, &progress,
                    &format!("Stashing post ..{short_id} ({})", progress.current_post_items));
                let page_count = item.metadata.page_count.unwrap_or(0);
                let page_num = item.metadata.page_num.unwrap_or(u32::MAX);

                // gallery-dl processes posts sequentially — a new post_id means
                // the previous post is complete. Materialize it now.
                let finished: Vec<_> = pending_collections
                    .keys()
                    .filter(|k| *k != &key)
                    .cloned()
                    .collect();
                for k in finished {
                    let pc = pending_collections.remove(&k).unwrap();
                    self.materialize_collection(
                        pc, subscription_id, &sub_id_str, &mut progress, &mut changed_collection_ids,
                    ).await;
                }

                let pending = pending_collections
                    .entry(key.clone())
                    .or_insert_with(|| PendingCollection {
                        category,
                        post_id,
                        preferred_name,
                        expected: page_count,
                        members: Vec::new(),
                        queue_id: None,
                    });

                // Persist to download queue for crash recovery
                if pending.queue_id.is_none() {
                    if let Ok(qid) = self.db.create_or_get_queue_entry(
                        subscription_id, Some(query_id), &pending.post_id, &pending.category,
                        Some(&pending.preferred_name), Some(page_count as i64),
                    ).await {
                        pending.queue_id = Some(qid);
                    }
                }
                if let Some(qid) = pending.queue_id {
                    let meta_json = serde_json::to_string(&item.metadata).ok();
                    let _ = self.db.add_queue_item(qid, Some(page_num as i64), meta_json.as_deref()).await;
                }

                pending.members.push(PendingMember {
                    file_path: item.file_path,
                    metadata: item.metadata,
                    page_num,
                });
            } else {
                // A non-collection item means any pending collection is complete
                for (_, pc) in pending_collections.drain() {
                    self.materialize_collection(
                        pc, subscription_id, &sub_id_str, &mut progress, &mut changed_collection_ids,
                    ).await;
                }

                // Single image (or multi-image with auto_collections off): import immediately
                self.set_phase("downloading");
                self.emit_progress(&sub_id_str, &progress, &format!("Importing {post_id_display}..."));

                // When auto_collections is off and this is a multi-image post,
                // append _p{N} suffix so each page has a distinct name.
                let is_multi = item.metadata.page_count.map_or(false, |c| c > 1);
                let import_metadata;
                let metadata_ref = if !self.auto_collections && is_multi {
                    let base = preferred_import_name(&item.metadata)
                        .unwrap_or_else(|| format!(
                            "{}_{}",
                            item.metadata.category.as_deref().unwrap_or("unknown"),
                            post_id_display,
                        ));
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

                // Track as a processed post
                progress.posts_processed += 1;

                match self
                    .import_item(&item.file_path, metadata_ref, subscription_id, &url, false)
                    .await
                {
                    Ok(outcome) => {
                        if outcome.imported_new {
                            progress.files_downloaded += 1;
                            info!(query_id, post_id = post_id_display, total_downloaded = progress.files_downloaded, "sync_query: imported new file");
                            self.set_phase("downloading");
                            self.emit_progress(&sub_id_str, &progress,
                                &format!("Downloaded {} files", progress.files_downloaded));
                        } else {
                            progress.files_skipped += 1;
                            info!(query_id, post_id = post_id_display, total_skipped = progress.files_skipped, "sync_query: skipped (already exists)");
                        }
                    }
                    Err(e) => {
                        warn!(query_id, post_id = post_id_display, error = %e, "sync_query: import error");
                        progress.errors.push(format!("Import error for post {post_id_display}: {e}"));
                    }
                }

                // Persist cursor + file count after each single-image post
                let cursor = compute_incremental_cursor(
                    resume_strategy.as_deref(), range_start, progress.posts_processed, &all_post_ids,
                );
                progress.resume_cursor = cursor.clone();
                if !completed_initial_run {
                    let _ = self.db.set_query_resume_state(query_id, cursor, resume_strategy.clone()).await;
                }
                let _ = self.db.update_query_progress(query_id, &Utc::now().to_rfc3339(), progress.files_downloaded as i64, progress.posts_processed as i64).await;
            }
        }

        // Finalize any incomplete collections (gallery-dl exited before all pages arrived).
        // If cancelled, mark queue entries as stale instead of importing.
        info!(
            query_id,
            total_items,
            pending_collections = pending_collections.len(),
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            cancelled = progress.cancelled,
            "sync_query: gallery-dl stream ended, finalizing"
        );
        if !progress.cancelled {
            for (_, pc) in pending_collections {
                self.materialize_collection(
                    pc, subscription_id, &sub_id_str, &mut progress, &mut changed_collection_ids,
                ).await;
            }
        } else {
            // Mark all pending queue entries as stale for potential later recovery
            let _ = self.db.mark_all_pending_stale_for_subscription(subscription_id).await;
        }

        // Wait for gallery-dl to finish
        let run_summary = match runner_handle.await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                progress.errors.push(format!("gallery-dl failed: {e}"));
                progress.failure_kind = Some("unknown".to_string());
                self.update_credential_health(site_id, "error", Some(&e)).await;
                return progress;
            }
            Err(e) => {
                progress.errors.push(format!("gallery-dl task panicked: {e}"));
                progress.failure_kind = Some("unknown".to_string());
                return progress;
            }
        };

        if cancel.is_cancelled() {
            progress.cancelled = true;
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
            let summary = format!("gallery-dl exited with code {} ({failure_kind_str})", run_summary.exit_code);
            progress.errors.push(summary.clone());
            let health_status = match failure_kind {
                FailureKind::Unauthorized => "unauthorized",
                FailureKind::Expired => "expired",
                _ => "error",
            };
            let err = run_summary.stderr_output.lines().rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(summary.as_str()).trim().to_string();
            warn!(site_id = %site_id, query_id, failure_kind = failure_kind_str, error = %err, "gallery-dl query execution failed");
            self.update_credential_health(site_id, health_status, Some(&err)).await;
        } else if run_summary.exit_code == 0 && has_credential {
            self.update_credential_health(site_id, "valid", None).await;
        }

        // Emit collection + final mutation events
        if !changed_collection_ids.is_empty() {
            changed_collection_ids.sort_unstable();
            changed_collection_ids.dedup();
            let mut scopes: Vec<String> = vec!["system:inbox".to_string()];
            scopes.extend(changed_collection_ids.iter().map(|id| format!("collection:{id}")));
            crate::events::emit_mutation(
                "subscription_import_collections",
                crate::runtime_contract::mutation_builder::MutationImpact::new()
                    .folder_membership_changed(changed_collection_ids)
                    .extra_grid_scopes(scopes),
            );
        }
        if progress.files_downloaded > 0 {
            crate::events::emit_mutation(
                "subscription_import",
                crate::runtime_contract::mutation_builder::MutationImpact::file_lifecycle(self.db)
                    .extra_grid_scopes(vec!["system:inbox".into()]),
            );
        }

        gallery_dl_runner::cleanup_temp_dir(&run_summary.temp_dir).await;

        self.set_phase("finalizing");
        self.emit_progress_force(&sub_id_str, &progress, "Finalizing...");
        let completed_cleanly = run_summary.exit_code == 0 && !progress.cancelled;
        let range_end = post_limit.map(|limit| range_start.saturating_add(limit).saturating_sub(1));
        let next_resume_cursor = resume_strategy.as_deref().map(|strategy| {
            match strategy {
                "range_offset" => range_end.map(|end| end.to_string()),
                "tag_id_lt" => {
                    let mut min_id: Option<u64> = None;
                    for pid in &all_post_ids {
                        if let Ok(n) = pid.parse::<u64>() {
                            min_id = Some(min_id.map_or(n, |cur| cur.min(n)));
                        }
                    }
                    min_id.map(|id| id.to_string())
                }
                _ => None,
            }
        }).flatten();
        let unique_post_count = all_post_ids.len();
        let continue_initial_pagination = should_continue_initial_pagination(
            completed_initial_run, completed_cleanly, post_limit, unique_post_count, next_resume_cursor.as_deref(),
        );

        if !completed_initial_run {
            let persisted_cursor = if completed_cleanly {
                if continue_initial_pagination { next_resume_cursor.clone() } else { None }
            } else {
                next_resume_cursor.clone().or_else(|| resume_cursor.map(|s| s.to_string()))
            };
            let _ = self.db.set_query_resume_state(query_id, persisted_cursor, resume_strategy.clone()).await;
        }

        if completed_cleanly {
            let now = Utc::now().to_rfc3339();
            let _ = self.db.update_query_progress(query_id, &now, progress.files_downloaded as i64, progress.posts_processed as i64).await;
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
            let _ = self.db.set_query_completed_initial_run(query_id, true).await;
        } else if continue_initial_pagination {
            info!(query_id, next_resume_cursor = ?next_resume_cursor, fetched_items = total_items,
                post_limit = ?post_limit, "sync_query: initial run continues; resuming next chunk");
        }

        info!(query_id, downloaded = progress.files_downloaded, skipped = progress.files_skipped,
            errors = progress.errors.len(), exit_code = run_summary.exit_code,
            cancelled = progress.cancelled,
            stderr_lines = run_summary.stderr_output.lines().count(),
            "sync_query: finished");

        progress
    }
}

/// Compute the resume cursor incrementally based on posts processed so far.
fn compute_incremental_cursor(
    resume_strategy: Option<&str>,
    range_start: u32,
    posts_processed: usize,
    all_post_ids: &std::collections::HashSet<String>,
) -> Option<String> {
    match resume_strategy {
        Some("range_offset") => {
            Some((range_start as usize + posts_processed).to_string())
        }
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

#[cfg(test)]
mod tests {
    use super::*;
}
