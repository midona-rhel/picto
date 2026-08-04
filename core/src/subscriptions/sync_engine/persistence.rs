use chrono::{Duration, Utc};

use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::subscriptions::source_adapter::{FailedDownloadedItem, ParsedMetadata};
use crate::subscriptions::types::OwnedSubscriptionDownloadAttemptUpsert;

use super::helpers::metadata_item_key;
use super::SubscriptionSyncEngine;

impl<'a> SubscriptionSyncEngine<'a> {
    pub(super) async fn persist_post_member_state(
        &self,
        subscription_id: i64,
        site_id: &str,
        metadata: &ParsedMetadata,
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

    pub(super) async fn persist_failed_download_attempt(
        &self,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        failed: &FailedDownloadedItem,
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
                failure_kind: Some(FailureKind::DownloadFailure.as_str().to_string()),
                last_error: Some(failed.error_message.clone()),
                next_retry_at: Some(next_retry_at),
            })
            .await;
        let site_id = metadata.category.as_deref().unwrap_or("unknown");
        self.persist_post_member_state(subscription_id, site_id, metadata, None, "failed")
            .await;
    }
}
