use std::collections::HashMap;
use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::subscriptions::gallery_dl_runner::{FailureKind, RunOptions};
use crate::subscriptions::import_policy::{
    collection_group_parts, individual_import_metadata, validate_metadata_for_site,
};
use crate::subscriptions::source_adapter::{
    DownloadedItem, GalleryDlSourceAdapter, SubscriptionSourceAdapter,
};

use super::helpers::cleanup_subscription_temp_root;
use super::{
    incomplete_post_detail, PendingCollection, PendingMember, SubscriptionSyncEngine, SyncProgress,
};

impl<'a> SubscriptionSyncEngine<'a> {
    pub async fn retry_failed_post(
        &mut self,
        query_run_id: i64,
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
        let query_run_id = Some(query_run_id);
        let sub_id_str = subscription_id.to_string();
        let adapter = GalleryDlSourceAdapter::new(self.runner.binary_path().clone());

        if let Ok(Some(query)) = self
            .runtime_service()
            .get_subscription_query(query_id)
            .await
        {
            if let Err(error) = adapter.validate_query_kind(site_id, &query.query_kind) {
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
        }

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
            if let Some(domain) = adapter.extract_domain(retry_url) {
                _domain_run_guard = Some(rate_limiter.acquire_domain_run(&domain).await);
                rate_limiter.wait_for_slot(&domain).await;
            }
        }

        let opts = RunOptions {
            subscription_id: Some(subscription_id),
            query_id: Some(query_id),
            site_id: site_id.to_string(),
            url: retry_url.to_string(),
            post_limit: None,
            range_start: 1,
            abort_threshold: None,
            auth: credential.gallery_dl_auth.clone(),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: cancel.clone(),
        };

        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<DownloadedItem>(32);
        let runner_handle = tokio::spawn(async move { adapter.run(&opts, item_tx).await });

        let mut pending_collections: HashMap<String, PendingCollection> = HashMap::new();
        let mut accepted_files = 0usize;
        let mut accepted_post = false;
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
                        FailureKind::UnexpectedRetryItem,
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
                    FailureKind::MalformedMetadata,
                    "gallery-dl item metadata failed validation during retry",
                    Some(&error),
                )
                .await;
                continue;
            }
            progress.metadata_validated += 1;
            progress.files_downloaded += 1;
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
                let page_num = item.metadata.page_num.unwrap_or(u32::MAX);
                pending.push_member(PendingMember {
                    file_path: item.file_path,
                    metadata: item.metadata,
                    page_num,
                });
                continue;
            }

            let import_metadata = individual_import_metadata(&item.metadata, self.auto_collections);
            match self
                .enqueue_single_subscription_item(
                    subscription_id,
                    query_id,
                    query_run_id,
                    &item.file_path,
                    import_metadata.as_ref(),
                )
                .await
            {
                Ok(_) => {
                    progress.queued_for_ingest += 1;
                    accepted_files += 1;
                    if !accepted_post {
                        accepted_post = true;
                        progress.posts_processed += 1;
                    }
                }
                Err(error) => {
                    progress.failure_kind =
                        Some(FailureKind::IngestQueueFailure.as_str().to_string());
                    progress.errors.push(error.clone());
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        FailureKind::IngestQueueFailure,
                        &format!(
                            "Failed to queue retry item for ingest for post {expected_post_id}"
                        ),
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
                progress.failure_kind = Some(FailureKind::Environment.as_str().to_string());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::Environment,
                    "Subscription retry could not start the downloader",
                    progress.errors.last().map(String::as_str),
                )
                .await;
                return progress;
            }
            Err(error) => {
                progress
                    .errors
                    .push(format!("gallery-dl retry task panicked: {error}"));
                progress.failure_kind = Some(FailureKind::Panic.as_str().to_string());
                self.record_issue(
                    subscription_id,
                    Some(query_id),
                    FailureKind::Panic,
                    "Subscription retry task panicked",
                    progress.errors.last().map(String::as_str),
                )
                .await;
                return progress;
            }
        };

        if run_summary.had_download_errors {
            progress.failure_kind = Some(FailureKind::DownloadFailure.as_str().to_string());
            for failed in &run_summary.failed_items {
                self.persist_failed_download_attempt(
                    subscription_id,
                    query_id,
                    query_run_id,
                    failed,
                )
                .await;
            }
        }

        if !progress.cancelled {
            for pc in pending_collections.into_values() {
                if !pc.is_complete(true) {
                    let detail = incomplete_post_detail(&pc);
                    progress.failure_kind = Some(FailureKind::DownloadFailure.as_str().to_string());
                    progress.errors.push(detail.clone());
                    self.record_issue(
                        subscription_id,
                        Some(query_id),
                        FailureKind::DownloadFailure,
                        "A retried subscription post did not download completely",
                        Some(&detail),
                    )
                    .await;
                    continue;
                }
                let file_count = pc.members.len();
                match self
                    .enqueue_pending_collection(pc, subscription_id, query_id, query_run_id)
                    .await
                {
                    Ok(_) => {
                        progress.queued_for_ingest += 1;
                        accepted_files += file_count;
                        if !accepted_post {
                            accepted_post = true;
                            progress.posts_processed += 1;
                        }
                    }
                    Err(error) => {
                        progress.failure_kind =
                            Some(FailureKind::IngestQueueFailure.as_str().to_string());
                        progress.errors.push(error.clone());
                        self.record_issue(
                            subscription_id,
                            Some(query_id),
                            FailureKind::IngestQueueFailure,
                            &format!("Retry collection queue failed for post {expected_post_id}"),
                            Some(&error),
                        )
                        .await;
                    }
                }
            }
        }

        if accepted_files > 0 {
            let _ = self
                .runtime_service()
                .add_query_progress(
                    query_id,
                    None,
                    accepted_files as i64,
                    progress.posts_processed as i64,
                )
                .await;
        }

        cleanup_subscription_temp_root(&run_summary.temp_dir).await;
        progress
    }
}
