use serde::Serialize;

use super::task::RuntimeTask;

/// Full runtime snapshot returned by `GetRuntimeSnapshot`.
///
/// Used by the frontend to seed state on initialization and to recover
/// from missed events (e.g. after a renderer crash/reload).
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeSnapshot {
    pub seq: u64,
    pub ts: String,
    pub tasks: Vec<RuntimeTask>,
}
