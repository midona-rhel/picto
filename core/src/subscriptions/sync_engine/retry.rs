use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::subscriptions::gallery_dl_runner::{FailureKind, RunOptions};
use crate::subscriptions::import_policy::{individual_import_metadata, validate_metadata_for_site};
use crate::subscriptions::source_adapter::{
    DownloadedItem, GalleryDlSourceAdapter, SubscriptionSourceAdapter,
};

use super::helpers::cleanup_subscription_temp_root;
use super::{SubscriptionSyncEngine, SyncProgress};

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
                _domain_run_guard = Some(rate_limiter.acquire_paced_run(&domain).await);
            }
        }

        let opts = RunOptions {
            subscription_id: Some(subscription_id),
            query_id: Some(query_id),
            site_id: site_id.to_string(),
            url: retry_url.to_string(),
            post_limit: None,
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: credential.gallery_dl_auth.clone(),
            archive_path: PathBuf::new(),
            archive_prefix: None,
            cancel: cancel.clone(),
        };

        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<DownloadedItem>(32);
        let runner_handle = tokio::spawn(async move { adapter.run(&opts, item_tx).await });

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
            let import_metadata = individual_import_metadata(&item.metadata);
            match self
                .enqueue_single_subscription_item(
                    subscription_id,
                    query_id,
                    query_run_id,
                    i64::from(!accepted_post),
                    &item.file_path,
                    import_metadata,
                )
                .await
            {
                Ok(_) => {
                    progress.queued_for_ingest += 1;
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

        cleanup_subscription_temp_root(&run_summary.temp_dir).await;
        progress
    }
}
