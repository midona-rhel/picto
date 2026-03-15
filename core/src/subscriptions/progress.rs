//! Subscription progress payloads read from the runtime task registry.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubscriptionProgressEvent {
    pub subscription_id: String,
    pub subscription_name: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_name: Option<String>,
    pub files_downloaded: usize,
    pub files_skipped: usize,
    pub pages_fetched: usize,
    pub metadata_validated: usize,
    pub metadata_invalid: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_metadata_error: Option<String>,
    pub status_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn list_runtime_progress_from_tasks() -> Vec<SubscriptionProgressEvent> {
    let mut events: Vec<SubscriptionProgressEvent> = crate::runtime_state::list_tasks()
        .into_iter()
        .filter(|task| task.kind == crate::runtime_contract::task::TaskKind::Subscription)
        .filter_map(|task| task.detail.and_then(|detail| serde_json::from_value(detail).ok()))
        .collect();
    events.sort_by(|a, b| a.subscription_id.cmp(&b.subscription_id));
    events
}
