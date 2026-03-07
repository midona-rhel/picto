//! Background task contract types — visible progress indicators for
//! long-running operations (subscriptions, flows, PTR sync, imports).
//!
//! Tasks are upserted via `runtime/task_upserted` events and removed
//! via `runtime/task_removed` when complete.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A running or recently-finished background task visible to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/runtime-contract/")]
pub struct RuntimeTask {
    pub task_id: String,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub progress: Option<TaskProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub detail: Option<serde_json::Value>,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/runtime-contract/")]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Subscription,
    Flow,
    PtrSync,
    PtrBootstrap,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/runtime-contract/")]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Cancelling,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/runtime-contract/")]
pub struct TaskProgress {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status_text: Option<String>,
}
