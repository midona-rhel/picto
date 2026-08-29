//! Application settings DTOs and canonical subscription limits.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const APPLICATION_SETTINGS_KEY: &str = "application";
pub const DEFAULT_SUBSCRIPTION_INBOX_ITEM_LIMIT: u64 = 1_000;
const MAX_SUBSCRIPTION_INBOX_ITEM_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SettingsSnapshot {
    #[ts(type = "unknown")]
    pub value: serde_json::Value,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn subscription_inbox_item_limit_library(
    application: &crate::library_application::LibraryApplication,
) -> Result<u64, String> {
    let value = application
        .library()
        .read_auxiliary_json("setting", APPLICATION_SETTINGS_KEY)
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_str::<serde_json::Value>(&value))
        .transpose()
        .map_err(|error| format!("Application settings are invalid: {error}"))?
        .unwrap_or_else(|| serde_json::json!({}));
    let Some(limit) = value.get("subscriptionInboxItemLimit") else {
        return Ok(DEFAULT_SUBSCRIPTION_INBOX_ITEM_LIMIT);
    };
    let limit = limit.as_u64().ok_or_else(|| {
        "Application setting subscriptionInboxItemLimit must be a positive integer".to_string()
    })?;
    if !(1..=MAX_SUBSCRIPTION_INBOX_ITEM_LIMIT).contains(&limit) {
        return Err(format!(
            "Application setting subscriptionInboxItemLimit must be between 1 and {MAX_SUBSCRIPTION_INBOX_ITEM_LIMIT}"
        ));
    }
    Ok(limit)
}

pub fn subscription_inbox_is_full_library(
    application: &crate::library_application::LibraryApplication,
) -> Result<bool, String> {
    let limit = subscription_inbox_item_limit_library(application)?;
    let inbox = application
        .library()
        .projections()
        .snapshot()
        .lifecycle
        .get(&picto_library::Lifecycle::Inbox)
        .map_or(0, |bitmap| bitmap.len());
    Ok(inbox >= limit)
}
