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
    effective_inbox_limit, resolve_query_name,
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
    hashes: Vec<String>,
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

        let credential = self
            .load_run_credential(site_id, &url, &sub_id_str, &progress)
            .await;
        let has_credential = credential.is_some();

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

        self.emit_progress(
            &sub_id_str,
            &progress,
            &format!("Starting gallery-dl for '{}'...", query_text),
        );

        let opts = RunOptions {
            url: url.clone(),
            file_limit,
            abort_threshold,
            sleep_request: self.settings.sub_rate_limit_secs,
            credential,
            archive_path,
            archive_prefix: Some(archive_prefix),
            cancel: cancel.clone(),
        };

        let run_result = match self.runner.run(&opts).await {
            Ok(r) => r,
            Err(e) => {
                progress.errors.push(format!("gallery-dl failed: {e}"));
                progress.failure_kind = Some("unknown".to_string());
                self.update_credential_health(site_id, "error", Some(&e))
                    .await;
                return progress;
            }
        };

        if cancel.is_cancelled() {
            progress.cancelled = true;
        }
        if run_result.exit_code != 0 && !progress.cancelled {
            let failure_kind = gallery_dl_runner::classify_failure(&run_result.stderr_output);
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
                run_result.exit_code
            );
            progress.errors.push(summary.clone());
            let health_status = match failure_kind {
                FailureKind::Unauthorized => "unauthorized",
                FailureKind::Expired => "expired",
                _ => "error",
            };
            let err = if run_result.stderr_output.trim().is_empty() {
                summary
            } else {
                run_result
                    .stderr_output
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or(summary.as_str())
                    .trim()
                    .to_string()
            };
            warn!(
                site_id = %site_id,
                query_id,
                failure_kind = failure_kind_str,
                error = %err,
                "gallery-dl query execution failed"
            );
            self.update_credential_health(site_id, health_status, Some(&err))
                .await;
        } else if run_result.exit_code == 0 && has_credential {
            self.update_credential_health(site_id, "valid", None).await;
        }

        let temp_dir = run_result
            .items
            .first()
            .and_then(|item| item.file_path.parent())
            .map(|p| p.to_path_buf());
        let mut collection_groups: HashMap<String, CollectionGroup> = HashMap::new();

        for item in &run_result.items {
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
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Inbox cap reached ({inbox_limit}); pausing download"),
                );
                progress.cancelled = true;
                break;
            }
            progress.pages_fetched += 1;
            let post_id = item.metadata.post_id.as_deref().unwrap_or("unknown");
            self.emit_progress(
                &sub_id_str,
                &progress,
                &format!("Importing post {post_id}..."),
            );

            if let Err(metadata_error) = validate_metadata_for_site(site_id, &item.metadata) {
                progress.metadata_invalid += 1;
                progress.last_metadata_error = Some(metadata_error.clone());
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Skipping invalid metadata: {metadata_error}"),
                );
                continue;
            }
            progress.metadata_validated += 1;

            match self
                .import_item(&item.file_path, &item.metadata, subscription_id, &url)
                .await
            {
                Ok(outcome) => {
                    if outcome.imported_new {
                        progress.files_downloaded += 1;
                    } else {
                        progress.files_skipped += 1;
                    }

                    if let Some((category, post_id, preferred_name)) =
                        collection_group_parts(site_id, &item.metadata)
                    {
                        let key = format!("{category}:{post_id}");
                        let group =
                            collection_groups
                                .entry(key)
                                .or_insert_with(|| CollectionGroup {
                                    category,
                                    post_id,
                                    preferred_name,
                                    hashes: Vec::new(),
                                });
                        if !group.hashes.iter().any(|h| h == &outcome.hex_hash) {
                            group.hashes.push(outcome.hex_hash.clone());
                        }
                    }

                    if outcome.imported_new {
                        self.emit_progress(
                            &sub_id_str,
                            &progress,
                            &format!("Downloaded {} files", progress.files_downloaded),
                        );
                    } else {
                        self.emit_progress(
                            &sub_id_str,
                            &progress,
                            &format!("Checking... ({} existing)", progress.files_skipped),
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        post_id = %post_id,
                        path = %item.file_path.display(),
                        error = %e,
                        "Subscription item import failed"
                    );
                    progress
                        .errors
                        .push(format!("Import error for post {post_id}: {e}"));
                }
            }
        }

        if !collection_groups.is_empty() {
            self.materialize_collection_groups(
                subscription_id,
                &sub_id_str,
                &cancel,
                &mut progress,
                collection_groups,
            )
            .await;
        }

        if let Some(ref dir) = temp_dir {
            let temp_root = dir.parent().unwrap_or(dir);
            gallery_dl_runner::cleanup_temp_dir(temp_root).await;
        }

        self.emit_progress_force(&sub_id_str, &progress, "Finalizing...");
        let completed_cleanly = run_result.exit_code == 0 && !progress.cancelled;
        let next_resume_cursor = resume_strategy
            .as_deref()
            .and_then(|strategy| derive_resume_cursor(&run_result.items, strategy));
        let continue_initial_pagination = should_continue_initial_pagination(
            completed_initial_run,
            completed_cleanly,
            file_limit,
            run_result.items.len(),
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
            if let Err(e) = self
                .db
                .set_query_resume_state(query_id, persisted_cursor, resume_strategy.clone())
                .await
            {
                progress
                    .errors
                    .push(format!("Failed to persist query resume state: {e}"));
            }
        }

        if completed_cleanly {
            let now = Utc::now().to_rfc3339();
            if let Err(e) = self
                .db
                .update_query_progress(query_id, &now, progress.files_downloaded as i64)
                .await
            {
                progress
                    .errors
                    .push(format!("Failed to update query progress: {e}"));
            }
        } else {
            info!(
                query_id,
                exit_code = run_result.exit_code,
                cancelled = progress.cancelled,
                "Skipping progress checkpoint update: run did not complete cleanly"
            );
        }

        if !completed_initial_run && completed_cleanly && !continue_initial_pagination {
            if let Err(e) = self
                .db
                .set_query_completed_initial_run(query_id, true)
                .await
            {
                progress
                    .errors
                    .push(format!("Failed to mark initial run complete: {e}"));
            }
        } else if continue_initial_pagination {
            info!(
                query_id,
                next_resume_cursor = ?next_resume_cursor,
                fetched_items = run_result.items.len(),
                file_limit = ?file_limit,
                "Initial run continues; resuming next chunk"
            );
        }

        info!(
            query_id,
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            exit_code = run_result.exit_code,
            cancelled = progress.cancelled,
            "Sync query finished"
        );

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
