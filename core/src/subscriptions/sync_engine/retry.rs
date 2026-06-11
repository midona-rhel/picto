use std::collections::HashMap;
use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::subscriptions::gallery_dl_runner::RunOptions;
use crate::subscriptions::import_policy::{collection_group_parts, validate_metadata_for_site};
use crate::subscriptions::source_adapter::{
    DownloadedItem, GalleryDlSourceAdapter, ParsedMetadata, SubscriptionSourceAdapter,
};

use super::helpers::metadata_item_key;
use super::{PendingCollection, PendingMember, SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
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
        let adapter = GalleryDlSourceAdapter::new(self.runner.binary_path().clone());

        if let Ok(Some(query)) = self.runtime_service().get_subscription_query(query_id).await {
            if let Err(error) = adapter.validate_query_kind(site_id, &query.query_kind) {
                progress.failure_kind = Some("invalid_query_kind".to_string());
                progress.errors.push(error);
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

        let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<DownloadedItem>(32);
        let runner_handle = tokio::spawn(async move { adapter.run(&opts, item_tx).await });

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

        let mut failed_post_members: HashMap<String, Vec<ParsedMetadata>> = HashMap::new();
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

        crate::subscriptions::gallery_dl_runner::cleanup_temp_dir(&run_summary.temp_dir).await;
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
}
