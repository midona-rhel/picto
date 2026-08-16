use std::collections::HashSet;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::subscriptions::archive::subscription_query_archive_prefix;
use crate::subscriptions::gallery_dl_runner::{FailureKind, RunOptions};
use crate::subscriptions::import_policy::{individual_import_metadata, validate_metadata_for_site};
use crate::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site, effective_inbox_limit,
    range_start_from_cursor, resolve_query_name,
};
use crate::subscriptions::source_adapter::{
    DownloadedItem, GalleryDlSourceAdapter, SubscriptionSourceAdapter,
};

use super::helpers::{
    cleanup_subscription_temp_root, compute_committed_cursor, initial_history_has_more,
};
use super::{SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
    pub async fn sync_query(
        &mut self,
        query_run_id: i64,
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
        let query_run_id = Some(query_run_id);
        let sub_id_str = subscription_id.to_string();
        let adapter = GalleryDlSourceAdapter::new(self.runner.binary_path().clone());

        let query_kind = {
            let q = self
                .runtime_service()
                .get_subscription_query(query_id)
                .await;
            match q {
                Ok(Some(q)) => q.query_kind,
                _ => crate::subscriptions::source_adapter::infer_query_kind(site_id).to_string(),
            }
        };
        if let Err(error) = adapter.validate_query_kind(site_id, &query_kind) {
            progress.failure_kind = Some(FailureKind::InvalidQueryKind.as_str().to_string());
            progress.errors.push(error.clone());
            self.record_issue(
                subscription_id,
                Some(query_id),
                FailureKind::InvalidQueryKind,
                "Subscription query kind is invalid for this source",
                Some(&error),
            )
            .await;
            return progress;
        }
        let inbox_limit = effective_inbox_limit(self.settings.sub_inbox_pause_limit);

        let inbox_count = self
            .db
            .get_scope_counts()
            .map(|counts| counts.inbox.max(0) as u64)
            .unwrap_or(0);
        if inbox_count >= inbox_limit as u64 {
            progress.failure_kind = Some(FailureKind::InboxFull.as_str().to_string());
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
        let url = match adapter.build_url(site_id, &query_for_run) {
            Some(u) => u,
            None => {
                let error = format!("Unknown subscription site: {site_id}");
                progress.failure_kind = Some(FailureKind::InvalidQueryKind.as_str().to_string());
                progress.errors.push(error.clone());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::InvalidQueryKind,
                    "Subscription source is not supported",
                    Some(&error),
                )
                .await;
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
            if let Some(domain) = adapter.extract_domain(&url) {
                _domain_run_guard = Some(rate_limiter.acquire_paced_run(&domain).await);
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

        let range_start = if !completed_initial_run {
            range_start_from_cursor(resume_cursor, resume_strategy.as_deref())
        } else {
            1
        };

        info!(
            elapsed_ms = sync_start.elapsed().as_millis(),
            "sync_query: pre-spawn ready"
        );
        self.set_phase("starting");
        self.emit_progress(
            &sub_id_str,
            &progress,
            &format!("Starting gallery-dl for '{}'...", query_text),
        );

        info!(
            query_id,
            url = %url,
            post_limit = ?post_limit,
            range_start,
            abort_threshold = ?abort_threshold,
            completed_initial_run,
            use_archive = true,
            archive_prefix = %archive_prefix,
            resume_cursor = ?resume_cursor,
            resume_strategy = ?resume_strategy,
            has_credential,
            request_interval_seconds = 1,
            "sync_query: run options"
        );

        let opts = RunOptions {
            subscription_id: Some(subscription_id),
            query_id: Some(query_id),
            site_id: site_id.to_string(),
            url: url.clone(),
            post_limit,
            range_start,
            source_cursor: resume_strategy
                .as_deref()
                .filter(|strategy| *strategy == "source_cursor")
                .and_then(|_| resume_cursor.map(str::to_string)),
            abort_threshold,
            auth: credential.gallery_dl_auth.clone(),
            archive_path,
            archive_prefix: Some(archive_prefix),
            cancel: cancel.clone(),
        };

        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<DownloadedItem>(32);

        let runner_handle = tokio::spawn(async move { adapter.run(&opts, item_tx).await });

        let mut all_post_ids: HashSet<String> = HashSet::new();
        let mut committed_post_ids: HashSet<String> = HashSet::new();
        let mut posts_to_unarchive: HashSet<String> = HashSet::new();
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
                progress.failure_kind = Some(FailureKind::InboxFull.as_str().to_string());
                progress.cancelled = true;
                break;
            }

            if let Some(post_id) = item.metadata.post_id.as_deref() {
                all_post_ids.insert(post_id.to_string());
            }
            let file_path_display = item.file_path.display().to_string();
            if let Err(e) = validate_metadata_for_site(site_id, &item.metadata) {
                info!(query_id, path = %file_path_display, error = %e, "sync_query: metadata validation failed, skipping");
                progress.metadata_invalid += 1;
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::MalformedMetadata,
                    "gallery-dl item metadata failed validation",
                    Some(&e),
                )
                .await;
                continue;
            }
            progress.metadata_validated += 1;
            total_items += 1;
            progress.files_downloaded += 1;

            let post_id_display = item.metadata.post_id.as_deref().unwrap_or("unknown");

            progress.current_post_id = Some(
                item.metadata
                    .post_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            );
            progress.current_post_items += 1;
            self.set_phase("queueing");
            self.emit_progress(
                &sub_id_str,
                &progress,
                &format!("Queueing {post_id_display}..."),
            );

            let import_metadata = individual_import_metadata(&item.metadata);
            let committed_post_id = item.metadata.post_id.clone();
            let first_item_for_post = committed_post_id
                .as_ref()
                .is_none_or(|post_id| !committed_post_ids.contains(post_id));

            match self
                .enqueue_single_subscription_item(
                    subscription_id,
                    query_id,
                    query_run_id,
                    i64::from(first_item_for_post),
                    &item.file_path,
                    import_metadata,
                )
                .await
            {
                Ok(_) => {
                    if let Some(post_id) = committed_post_id {
                        committed_post_ids.insert(post_id);
                    }
                    progress.queued_for_ingest += 1;
                    self.emit_progress(
                        &sub_id_str,
                        &progress,
                        &format!("Queued {} files for ingest", progress.queued_for_ingest),
                    );
                    if first_item_for_post {
                        self.finalize_current_post_progress(
                            &mut progress,
                            &mut posts_processed_this_run,
                        );
                    }
                }
                Err(error) => {
                    if let Some(post_id) = item.metadata.post_id.clone() {
                        posts_to_unarchive.insert(post_id);
                    }
                    progress.failure_kind =
                        Some(FailureKind::IngestQueueFailure.as_str().to_string());
                    progress.errors.push(format!(
                        "Failed to queue subscription item for ingest: {error}"
                    ));
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        FailureKind::IngestQueueFailure,
                        "Failed to queue subscription item for ingest",
                        Some(&error),
                    )
                    .await;
                    progress.current_post_id = None;
                    progress.current_post_items = 0;
                }
            }
        }

        let run_summary = match runner_handle.await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                progress.errors.push(format!("gallery-dl failed: {e}"));
                // gallery-dl never ran — a local environment problem, not a
                // credential problem. Record an issue; never touch health.
                progress.failure_kind = Some(FailureKind::Environment.as_str().to_string());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::Environment,
                    &format!("gallery-dl failed to run: {e}"),
                    None,
                )
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
                return progress;
            }
            Err(e) => {
                progress
                    .errors
                    .push(format!("gallery-dl task panicked: {e}"));
                progress.failure_kind = Some(FailureKind::Panic.as_str().to_string());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::Panic,
                    "Subscription downloader task panicked",
                    progress.errors.last().map(String::as_str),
                )
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
                return progress;
            }
        };

        if cancel.is_cancelled() {
            progress.cancelled = true;
        }
        progress.files_skipped += run_summary.skipped_archive_items;
        if run_summary.had_download_errors {
            progress.failure_kind = Some(FailureKind::DownloadFailure.as_str().to_string());
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
            }
            self.record_issue(
                subscription_id,
                Some(query_id),
                FailureKind::DownloadFailure,
                "One or more subscription items failed after gallery-dl retries",
                run_summary
                    .failed_items
                    .first()
                    .map(|item| item.error_message.as_str()),
            )
            .await;
        }
        if run_summary.exit_code != 0 && !progress.cancelled {
            let failure_kind = crate::subscriptions::gallery_dl_runner::classify_failure(
                &run_summary.stderr_output,
            );
            let failure_kind_str = failure_kind.as_str();
            progress.failure_kind = Some(failure_kind_str.to_string());
            let summary = format!(
                "gallery-dl exited with code {} ({failure_kind_str})",
                run_summary.exit_code
            );
            // The stored/displayed error is the actual exception line, not the
            // exit-code wrapper — "user could not be found" beats "code 4".
            let err = crate::subscriptions::gallery_dl_runner::final_error_line(
                &run_summary.stderr_output,
            )
            .unwrap_or_else(|| summary.clone());
            progress.errors.push(err.clone());
            warn!(site_id = %site_id, query_id, failure_kind = failure_kind_str, summary = %summary, error = %err, "gallery-dl query execution failed");
            let health_status = match failure_kind {
                FailureKind::Unauthorized | FailureKind::Expired => Some(failure_kind),
                _ => None,
            };
            // Only auth-classified failures for sites with a stored credential
            // may touch credential health — content errors (not-found, rate
            // limits, network) must never mark an account as broken.
            match health_status {
                Some(kind) if has_credential => {
                    self.note_run_auth_failure(
                        subscription_id,
                        query_id,
                        site_id,
                        kind,
                        Some(&err),
                    )
                    .await;
                }
                _ => {
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        failure_kind,
                        &err,
                        Some(&summary),
                    )
                    .await;
                }
            }
        } else if run_summary.exit_code == 0 {
            if has_credential {
                self.note_run_success(subscription_id, query_id, site_id, true)
                    .await;
            } else {
                let _ = self
                    .runtime_service()
                    .resolve_subscription_issues(
                        subscription_id,
                        Some(query_id),
                        FailureKind::CredentialBlocked,
                    )
                    .await;
            }
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(
                    subscription_id,
                    Some(query_id),
                    FailureKind::RateLimited,
                )
                .await;
            let _ = self
                .runtime_service()
                .resolve_subscription_issues(subscription_id, Some(query_id), FailureKind::Network)
                .await;
        }

        info!(
            query_id,
            total_items,
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            cancelled = progress.cancelled,
            "sync_query: finalized independent media items"
        );

        if !progress.cancelled {
            let bridge_discovered_without_downloads = has_unexplained_discovery(
                completed_initial_run,
                run_summary.exit_code,
                run_summary.discovered_items,
                run_summary.skipped_archive_items,
                total_items,
            );
            if bridge_discovered_without_downloads {
                let unexplained = run_summary
                    .discovered_items
                    .saturating_sub(run_summary.skipped_archive_items);
                let detail = format!(
                    "gallery-dl discovered {unexplained} unaccounted items across {} posts but produced no downloadable files",
                    run_summary.discovered_post_ids.len(),
                );
                warn!(
                    query_id,
                    discovered_items = run_summary.discovered_items,
                    discovered_posts = run_summary.discovered_post_ids.len(),
                    skipped_archive_items = run_summary.skipped_archive_items,
                    "sync_query: bridge discovered items but no downloads were emitted"
                );
                progress.failure_kind = Some(FailureKind::BridgeNoDownloads.as_str().to_string());
                progress.errors.push(detail.clone());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::BridgeNoDownloads,
                    "gallery-dl discovered items but emitted no downloads",
                    Some(&detail),
                )
                .await;
            } else if run_summary.exit_code == 0 {
                let _ = self
                    .runtime_service()
                    .resolve_subscription_issues(
                        subscription_id,
                        Some(query_id),
                        FailureKind::BridgeNoDownloads,
                    )
                    .await;
            }
        }

        if !posts_to_unarchive.is_empty() {
            let prefix = crate::subscriptions::archive::subscription_query_archive_prefix(
                subscription_id,
                query_id,
            );
            let post_ids = posts_to_unarchive.into_iter().collect::<Vec<_>>();
            if let Err(error) = crate::subscriptions::archive::clear_post_archive_entries_at_root(
                &self.library_root,
                &prefix,
                &post_ids,
            )
            .await
            {
                warn!(query_id, %error, "sync_query: failed to unarchive uncommitted posts");
            }
        }

        cleanup_subscription_temp_root(&run_summary.temp_dir).await;

        self.set_phase("finalizing");
        self.emit_progress_force(&sub_id_str, &progress, "Finalizing...");
        let completed_cleanly = run_summary.exit_code == 0
            && !progress.cancelled
            && progress.errors.is_empty()
            && progress.metadata_invalid == 0
            && committed_post_ids.len() == all_post_ids.len();
        let next_resume_cursor = compute_committed_cursor(
            completed_cleanly,
            resume_strategy.as_deref(),
            range_start,
            posts_processed_this_run,
            &all_post_ids,
            run_summary.source_cursor.as_deref(),
        );
        progress.resume_cursor = next_resume_cursor.clone();
        let unique_post_count = all_post_ids.len();
        let pagination_item_count = if run_summary.source_page_items > 0 {
            run_summary.source_page_items
        } else {
            unique_post_count
        };
        // A source page can include posts without supported media. Those posts
        // were still checked and count toward source pagination, while file
        // counters continue to report only actual media.
        progress.posts_processed = progress.posts_processed.max(pagination_item_count);
        let initial_history_has_more = initial_history_has_more(
            completed_initial_run,
            completed_cleanly,
            post_limit,
            pagination_item_count,
            next_resume_cursor.as_deref(),
        );

        if !completed_initial_run {
            let persisted_cursor = if completed_cleanly {
                if initial_history_has_more {
                    next_resume_cursor.clone()
                } else {
                    None
                }
            } else {
                resume_cursor.map(str::to_string)
            };
            let _ = self
                .runtime_service()
                .set_query_resume_state(query_id, persisted_cursor, resume_strategy.clone())
                .await;
        }

        if completed_cleanly {
            let _ = self
                .runtime_service()
                .add_query_progress(query_id, Some(Utc::now().to_rfc3339()), 0, 0)
                .await;
        }

        info!(
            query_id,
            completed_cleanly,
            completed_initial_run,
            initial_history_has_more,
            unique_post_count,
            next_resume_cursor = ?next_resume_cursor,
            exit_code = run_summary.exit_code,
            "sync_query: pagination decision"
        );

        if !completed_initial_run && completed_cleanly && !initial_history_has_more {
            info!(query_id, "sync_query: marking initial run as complete");
            let _ = self
                .runtime_service()
                .set_query_completed_initial_run(query_id, true)
                .await;
            let _ = self
                .runtime_service()
                .set_query_resume_state(query_id, None, None)
                .await;
        } else if initial_history_has_more {
            info!(query_id, next_resume_cursor = ?next_resume_cursor, fetched_items = total_items,
                post_limit = ?post_limit, "sync_query: more initial history remains for the next run");
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
}

fn has_unexplained_discovery(
    completed_initial_run: bool,
    exit_code: i32,
    discovered_items: usize,
    skipped_archive_items: usize,
    downloaded_items: usize,
) -> bool {
    !completed_initial_run
        && exit_code == 0
        && downloaded_items == 0
        && discovered_items.saturating_sub(skipped_archive_items) > 0
}

#[cfg(test)]
mod tests {
    use super::has_unexplained_discovery;

    #[test]
    fn archive_only_rerun_is_not_a_missing_download() {
        assert!(!has_unexplained_discovery(false, 0, 3, 3, 0));
    }

    #[test]
    fn unexplained_initial_discovery_is_a_missing_download() {
        assert!(has_unexplained_discovery(false, 0, 3, 2, 0));
        assert!(has_unexplained_discovery(false, 0, 1, 0, 0));
    }

    #[test]
    fn downloads_and_terminal_history_do_not_trigger_the_guard() {
        assert!(!has_unexplained_discovery(false, 0, 3, 0, 1));
        assert!(!has_unexplained_discovery(true, 0, 1, 0, 0));
        assert!(!has_unexplained_discovery(false, 4, 1, 0, 0));
    }
}
