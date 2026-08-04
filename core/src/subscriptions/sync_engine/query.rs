use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::subscriptions::archive::subscription_query_archive_prefix;
use crate::subscriptions::gallery_dl_runner::{FailureKind, RunOptions};
use crate::subscriptions::import_policy::{
    collection_group_parts, preferred_import_name, validate_metadata_for_site,
};
use crate::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site, effective_inbox_limit,
    range_start_from_cursor, resolve_query_name,
};
use crate::subscriptions::source_adapter::{
    DownloadedItem, GalleryDlSourceAdapter, ParsedMetadata, SubscriptionSourceAdapter,
};

use super::helpers::{
    compute_incremental_cursor, maybe_cleanup_subscription_temp_root,
    should_continue_initial_pagination,
};
use super::{PendingCollection, PendingMember, SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
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
        let adapter = GalleryDlSourceAdapter::new(self.runner.binary_path().clone());

        let (prior_files, prior_posts, query_kind) = {
            let q = self
                .runtime_service()
                .get_subscription_query(query_id)
                .await;
            match q {
                Ok(Some(q)) => (q.files_found as usize, q.posts_found as usize, q.query_kind),
                _ => (
                    0,
                    0,
                    crate::subscriptions::source_adapter::infer_query_kind(site_id).to_string(),
                ),
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

        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<DownloadedItem>(32);

        let runner_handle = tokio::spawn(async move { adapter.run(&opts, item_tx).await });

        let mut pending_collections: HashMap<String, PendingCollection> = HashMap::new();
        let mut current_pending_collection_key: Option<String> = None;
        let mut all_post_ids: HashSet<String> = HashSet::new();
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
                        if let Err(error) = self
                            .flush_pending_collection(
                                &previous_key,
                                &mut pending_collections,
                                subscription_id,
                                query_id,
                                query_run_id,
                                &sub_id_str,
                                &mut progress,
                            )
                            .await
                        {
                            progress.failure_kind =
                                Some(FailureKind::IngestQueueFailure.as_str().to_string());
                            progress
                                .errors
                                .push(format!("Failed to queue collection for ingest: {error}"));
                            self.record_issue(
                                subscription_id,
                                Some(query_id),
                                FailureKind::IngestQueueFailure,
                                "Failed to queue subscription collection for ingest",
                                Some(&error),
                            )
                            .await;
                        }
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
                    if let Err(error) = self
                        .flush_pending_collection(
                            &previous_key,
                            &mut pending_collections,
                            subscription_id,
                            query_id,
                            query_run_id,
                            &sub_id_str,
                            &mut progress,
                        )
                        .await
                    {
                        progress.failure_kind =
                            Some(FailureKind::IngestQueueFailure.as_str().to_string());
                        progress
                            .errors
                            .push(format!("Failed to queue collection for ingest: {error}"));
                        self.record_issue(
                            subscription_id,
                            Some(query_id),
                            FailureKind::IngestQueueFailure,
                            "Failed to queue subscription collection for ingest",
                            Some(&error),
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

                progress.current_post_id = Some(
                    item.metadata
                        .post_id
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                progress.current_post_items = 1;

                self.set_phase("queueing");
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Queueing {post_id_display}..."),
                );

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

                match self
                    .enqueue_single_subscription_item(
                        subscription_id,
                        query_id,
                        query_run_id,
                        &item.file_path,
                        metadata_ref,
                    )
                    .await
                {
                    Ok(_) => {
                        progress.queued_for_ingest += 1;
                        self.emit_progress(
                            &sub_id_str,
                            &progress,
                            &format!("Queued {} files for ingest", progress.queued_for_ingest),
                        );
                    }
                    Err(error) => {
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
                    }
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
        let mut failed_post_members: HashMap<String, Vec<ParsedMetadata>> = HashMap::new();
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
            pending_collections = pending_collections.len(),
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            cancelled = progress.cancelled,
            "sync_query: finalizing pending collections"
        );
        if progress.cancelled {
            // A cancelled run must not lose posts that already finished
            // downloading: flush every buffered post whose full file set is on
            // disk. Incomplete posts are dropped AND un-archived so the next
            // run re-fetches them whole — otherwise their already-downloaded
            // files would archive-skip and the post could never complete.
            let keys: Vec<String> = pending_collections.keys().cloned().collect();
            let mut dropped_post_ids: Vec<String> = Vec::new();
            for key in keys {
                let complete = pending_collections.get(&key).is_some_and(|pc| {
                    pc.expected_count
                        .is_some_and(|expected| pc.members.len() as u32 >= expected)
                });
                if complete {
                    if let Err(error) = self
                        .flush_pending_collection(
                            &key,
                            &mut pending_collections,
                            subscription_id,
                            query_id,
                            query_run_id,
                            &sub_id_str,
                            &mut progress,
                        )
                        .await
                    {
                        progress.failure_kind =
                            Some(FailureKind::IngestQueueFailure.as_str().to_string());
                        progress
                            .errors
                            .push(format!("Failed to queue collection for ingest: {error}"));
                        self.record_issue(
                            subscription_id,
                            Some(query_id),
                            FailureKind::IngestQueueFailure,
                            "Failed to queue subscription collection for ingest",
                            Some(&error),
                        )
                        .await;
                    }
                } else if let Some(pc) = pending_collections.remove(&key) {
                    info!(
                        query_id,
                        post_id = %pc.post_id,
                        members = pc.members.len(),
                        expected = ?pc.expected_count,
                        "sync_query: dropping incomplete post at cancellation; clearing its archive entries for full re-fetch"
                    );
                    dropped_post_ids.push(pc.post_id);
                }
            }
            if !dropped_post_ids.is_empty() {
                let prefix = crate::subscriptions::archive::subscription_query_archive_prefix(
                    subscription_id,
                    query_id,
                );
                if let Err(error) =
                    crate::subscriptions::archive::clear_post_archive_entries_at_root(
                        &self.library_root,
                        &prefix,
                        &dropped_post_ids,
                    )
                    .await
                {
                    warn!(query_id, %error, "sync_query: failed to clear archive entries for incomplete posts");
                }
            }
        }
        if !progress.cancelled {
            if let Some(last_key) = current_pending_collection_key.take() {
                if let Err(error) = self
                    .flush_pending_collection(
                        &last_key,
                        &mut pending_collections,
                        subscription_id,
                        query_id,
                        query_run_id,
                        &sub_id_str,
                        &mut progress,
                    )
                    .await
                {
                    progress.failure_kind =
                        Some(FailureKind::IngestQueueFailure.as_str().to_string());
                    progress
                        .errors
                        .push(format!("Failed to queue collection for ingest: {error}"));
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        FailureKind::IngestQueueFailure,
                        "Failed to queue subscription collection for ingest",
                        Some(&error),
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
}
