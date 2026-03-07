use serde::Serialize;
use ts_rs::TS;

use super::task::RuntimeTask;

/// Full runtime snapshot returned by `GetRuntimeSnapshot`.
///
/// Used by the frontend to seed state on initialization and to recover
/// from missed events (e.g. after a renderer crash/reload).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/runtime-contract/")]
pub struct RuntimeSnapshot {
    #[ts(type = "number")]
    pub seq: u64,
    pub ts: String,
    pub tasks: Vec<RuntimeTask>,
}
