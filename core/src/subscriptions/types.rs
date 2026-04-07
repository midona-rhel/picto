use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub subscription_id: i64,
    pub name: String,
    pub paused: bool,
    pub group_id: Option<i64>,
    pub initial_post_limit: i64,
    pub periodic_post_limit: i64,
    pub auto_collections: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQuery {
    pub query_id: i64,
    pub subscription_id: i64,
    pub site_id: String,
    pub query_kind: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub paused: bool,
    pub last_check_time: Option<String>,
    pub files_found: i64,
    pub posts_found: i64,
    pub completed_initial_run: bool,
    pub resume_cursor: Option<String>,
    pub resume_strategy: Option<String>,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionGroup {
    pub group_id: i64,
    pub name: String,
    pub schedule: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRunRecord {
    pub run_id: i64,
    pub subscription_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub files_downloaded: i64,
    pub files_skipped: i64,
    pub metadata_validated: i64,
    pub metadata_invalid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionQueryRunRecord {
    pub query_run_id: i64,
    pub run_id: Option<i64>,
    pub subscription_id: i64,
    pub query_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub posts_processed: i64,
    pub files_downloaded: i64,
    pub files_skipped: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQueryJob {
    pub job_id: i64,
    pub run_id: Option<i64>,
    pub subscription_id: i64,
    pub query_id: i64,
    pub site_id: String,
    pub status: String,
    pub job_kind: String,
    pub requested_by: String,
    pub post_id: Option<String>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionIssueRecord {
    pub issue_id: i64,
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub issue_kind: String,
    pub status: String,
    pub message: String,
    pub detail: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDownloadAttemptRecord {
    pub attempt_id: i64,
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub item_key: String,
    pub site_category: Option<String>,
    pub post_id: Option<String>,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub retry_url: Option<String>,
    pub retry_count: i64,
    pub status: String,
    pub failure_kind: Option<String>,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPostMemberRecord {
    pub subscription_id: i64,
    pub site_id: String,
    pub post_id: String,
    pub item_key: String,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub entity_hash: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct SubscriptionDownloadAttemptUpsert<'a> {
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub item_key: &'a str,
    pub site_category: Option<&'a str>,
    pub post_id: Option<&'a str>,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<&'a str>,
    pub media_url: Option<&'a str>,
    pub retry_url: Option<&'a str>,
    pub failure_kind: Option<&'a str>,
    pub last_error: Option<&'a str>,
    pub next_retry_at: Option<&'a str>,
}

pub struct SubscriptionPostMemberUpsert<'a> {
    pub subscription_id: i64,
    pub site_id: &'a str,
    pub post_id: &'a str,
    pub item_key: &'a str,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<&'a str>,
    pub media_url: Option<&'a str>,
    pub entity_hash: Option<&'a str>,
    pub status: &'a str,
}

#[derive(Debug, Clone)]
pub struct OwnedSubscriptionPostMemberUpsert {
    pub subscription_id: i64,
    pub site_id: String,
    pub post_id: String,
    pub item_key: String,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub entity_hash: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct OwnedSubscriptionDownloadAttemptUpsert {
    pub subscription_id: i64,
    pub query_id: Option<i64>,
    pub query_run_id: Option<i64>,
    pub item_key: String,
    pub site_category: Option<String>,
    pub post_id: Option<String>,
    pub page_num: Option<i64>,
    pub canonical_post_url: Option<String>,
    pub media_url: Option<String>,
    pub retry_url: Option<String>,
    pub failure_kind: Option<String>,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialDomain {
    pub site_category: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialHealth {
    pub site_category: String,
    pub health_status: String,
    pub last_checked_at: String,
    pub last_error: Option<String>,
}
