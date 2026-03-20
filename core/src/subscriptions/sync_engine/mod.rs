//! Subscription sync engine — downloads files via gallery-dl subprocess.
//!
//! Steps per query:
//! 1. Build URL from subscription's gallery-dl URL template + query text
//! 2. Load credentials from OS keychain (if configured for that site)
//! 3. Spawn gallery-dl subprocess with appropriate flags
//! 4. Import each downloaded file via the existing ImportPipeline
//! 5. Merge metadata for already-imported files (tags, URLs, notes, name)
//! 6. Track `completed_initial_run` for smart-stop behavior

mod collections;
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
use crate::subscriptions::import_policy::{collection_group_parts, validate_metadata_for_site};
use crate::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site, derive_resume_cursor,
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
}

#[derive(Debug, Clone)]
struct CollectionGroup {
    category: String,
    post_id: String,
    preferred_name: String,
    /// (hash, page_num) pairs — page_num preserves original page order.
    members: Vec<(String, u32)>,
    /// Expected total pages from metadata (gallery-dl `count` field).
    expected_count: u32,
}

pub struct SubscriptionSyncEngine<'a> {
    db: &'a SqliteDatabase,
    blob_store: &'a BlobStore,
    runner: GalleryDlRunner,
    settings: AppSettings,
    subscription_name: String,
    current_query_id: Option<i64>,
    current_query_name: Option<String>,
    last_progress_emit: std::time::Instant,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
    auto_merge_require_matching_dimensions: bool,
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
            current_query_id: None,
            current_query_name: None,
            last_progress_emit: std::time::Instant::now(),
            auto_merge_enabled: false,
            auto_merge_distance: crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD,
            auto_merge_require_matching_dimensions: false,
        })
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.subscription_name = name;
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
        file_limit: Option<u32>,
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

        self.emit_progress(
            &sub_id_str,
            &progress,
            &format!("Starting gallery-dl for '{}'...", query_text),
        );

        let opts = RunOptions {
            url: url.clone(),
            file_limit,
            range_start,
            abort_threshold,
            sleep_request: self.settings.sub_rate_limit_secs,
            credential,
            // Initial runs: skip archive so stale entries from previous runs
            // don't prevent downloads. Import pipeline deduplicates by hash.
            archive_path: if completed_initial_run { archive_path } else { PathBuf::new() },
            archive_prefix: if completed_initial_run { Some(archive_prefix) } else { None },
            cancel: cancel.clone(),
        };

        // ── Streaming import: process files as gallery-dl downloads them ──
        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<gallery_dl_runner::DownloadedItem>(32);

        let runner_handle = {
            let runner = gallery_dl_runner::GalleryDlRunner::new(self.runner.binary_path().clone());
            tokio::spawn(async move { runner.run(&opts, item_tx).await })
        };

        struct PendingCollection {
            category: String,
            post_id: String,
            preferred_name: String,
            expected: u32,
            members: Vec<(String, u32)>,
        }
        let mut pending_collections: HashMap<String, PendingCollection> = HashMap::new();
        let mut changed_collection_ids: Vec<i64> = Vec::new();
        let mut all_post_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut total_items: usize = 0;

        while let Some(item) = item_rx.recv().await {
            if cancel.is_cancelled() {
                progress.cancelled = true;
                break;
            }
            let inbox_count = self
                .db
                .bitmaps
                .len(&crate::sqlite::bitmaps::BitmapKey::Status(0));
            if inbox_count >= inbox_limit as u64 {
                progress.failure_kind = Some("inbox_full".to_string());
                progress.cancelled = true;
                break;
            }

            if let Err(_) = validate_metadata_for_site(site_id, &item.metadata) {
                progress.metadata_invalid += 1;
                continue;
            }
            progress.metadata_validated += 1;
            total_items += 1;

            if let Some(pid) = item.metadata.post_id.as_deref() {
                all_post_ids.insert(pid.to_string());
            }

            let collection_parts = collection_group_parts(site_id, &item.metadata);
            let is_multi = item.metadata.page_count.map_or(false, |c| c > 1);
            let is_collection_member = collection_parts.is_some() && is_multi;

            let post_id_display = item.metadata.post_id.as_deref().unwrap_or("unknown");

            if is_collection_member {
                let (category, post_id, preferred_name) = collection_parts.unwrap();
                let key = format!("{category}:{post_id}");
                let is_first = !pending_collections.contains_key(&key);

                if is_first {
                    self.db.hold_events();
                }

                let pending = pending_collections
                    .entry(key.clone())
                    .or_insert_with(|| PendingCollection {
                        category,
                        post_id,
                        preferred_name,
                        expected: item.metadata.page_count.unwrap_or(0),
                        members: Vec::new(),
                    });

                match self
                    .import_item(&item.file_path, &item.metadata, subscription_id, &url, true)
                    .await
                {
                    Ok(outcome) => {
                        if outcome.imported_new {
                            progress.files_downloaded += 1;
                        } else {
                            progress.files_skipped += 1;
                        }
                        let page_num = item.metadata.page_num.unwrap_or(u32::MAX);
                        if !pending.members.iter().any(|(h, _)| h == &outcome.hex_hash) {
                            pending.members.push((outcome.hex_hash, page_num));
                        }
                    }
                    Err(e) => {
                        progress.errors.push(format!("Import error for post {post_id_display}: {e}"));
                    }
                }

                // Check if collection is complete
                let pending = pending_collections.get(&key).unwrap();
                if pending.expected > 0 && pending.members.len() as u32 >= pending.expected {
                    let mut pc = pending_collections.remove(&key).unwrap();
                    pc.members.sort_by_key(|(_, num)| *num);
                    let hashes: Vec<String> = pc.members.iter().map(|(h, _)| h.clone()).collect();

                    if hashes.len() >= 2 {
                        let existing_id = self.db
                            .get_subscription_post_collection(subscription_id, &pc.category, &pc.post_id)
                            .await.ok().flatten();
                        let collection_id = match existing_id {
                            Some(id) => id,
                            None => match self.db.create_collection(&pc.preferred_name).await {
                                Ok(id) => id,
                                Err(e) => {
                                    progress.errors.push(format!("Collection create failed: {e}"));
                                    self.db.release_events();
                                    continue;
                                }
                            },
                        };
                        let _ = self.db.add_collection_members_by_hashes(collection_id, &hashes).await;
                        let _ = self.db.upsert_subscription_post_collection(
                            subscription_id, &pc.category, &pc.post_id, collection_id,
                        ).await;
                        changed_collection_ids.push(collection_id);
                    }
                    self.db.release_events();
                }
            } else {
                // Single image: import and emit immediately
                self.emit_progress(&sub_id_str, &progress, &format!("Importing {post_id_display}..."));
                match self
                    .import_item(&item.file_path, &item.metadata, subscription_id, &url, false)
                    .await
                {
                    Ok(outcome) => {
                        if outcome.imported_new {
                            progress.files_downloaded += 1;
                            self.emit_progress(&sub_id_str, &progress,
                                &format!("Downloaded {} files", progress.files_downloaded));
                        } else {
                            progress.files_skipped += 1;
                        }
                    }
                    Err(e) => {
                        progress.errors.push(format!("Import error for post {post_id_display}: {e}"));
                    }
                }
            }
        }

        // Finalize any incomplete collections (gallery-dl exited before all pages arrived)
        for (_, mut pc) in pending_collections {
            pc.members.sort_by_key(|(_, num)| *num);
            let hashes: Vec<String> = pc.members.iter().map(|(h, _)| h.clone()).collect();
            if hashes.len() >= 2 {
                let existing_id = self.db
                    .get_subscription_post_collection(subscription_id, &pc.category, &pc.post_id)
                    .await.ok().flatten();
                let collection_id = match existing_id {
                    Some(id) => id,
                    None => match self.db.create_collection(&pc.preferred_name).await {
                        Ok(id) => id,
                        Err(_) => { self.db.release_events(); continue; }
                    },
                };
                let _ = self.db.add_collection_members_by_hashes(collection_id, &hashes).await;
                let _ = self.db.upsert_subscription_post_collection(
                    subscription_id, &pc.category, &pc.post_id, collection_id,
                ).await;
                changed_collection_ids.push(collection_id);
            }
            self.db.release_events();
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
            let mut scopes: Vec<String> = vec!["system:all".to_string()];
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
                crate::runtime_contract::mutation_builder::MutationImpact::file_lifecycle(self.db),
            );
        }

        gallery_dl_runner::cleanup_temp_dir(&run_summary.temp_dir).await;

        self.emit_progress_force(&sub_id_str, &progress, "Finalizing...");
        let completed_cleanly = run_summary.exit_code == 0 && !progress.cancelled;
        let range_end = file_limit.map(|limit| range_start.saturating_add(limit).saturating_sub(1));
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
            completed_initial_run, completed_cleanly, file_limit, unique_post_count, next_resume_cursor.as_deref(),
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
            let _ = self.db.update_query_progress(query_id, &now, progress.files_downloaded as i64).await;
        }

        if !completed_initial_run && completed_cleanly && !continue_initial_pagination {
            let _ = self.db.set_query_completed_initial_run(query_id, true).await;
        } else if continue_initial_pagination {
            info!(query_id, next_resume_cursor = ?next_resume_cursor, fetched_items = total_items,
                file_limit = ?file_limit, "Initial run continues; resuming next chunk");
        }

        info!(query_id, downloaded = progress.files_downloaded, skipped = progress.files_skipped,
            errors = progress.errors.len(), exit_code = run_summary.exit_code,
            cancelled = progress.cancelled, "Sync query finished");

        progress
    }
}

fn should_continue_initial_pagination(
    completed_initial_run: bool,
    completed_cleanly: bool,
    file_limit: Option<u32>,
    fetched_items: usize,
    next_resume_cursor: Option<&str>,
) -> bool {
    if completed_initial_run || !completed_cleanly {
        return false;
    }
    let Some(limit) = file_limit else {
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
