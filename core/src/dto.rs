//! Stable IPC and event values owned by the integration shell.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const LIBRARY_CHANGED_EVENT: &str = "library/changed";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct FileHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct LibraryChanged {
    #[ts(type = "number")]
    pub revision: u64,
    pub resources: Vec<String>,
    #[ts(type = "number[]")]
    pub item_ids: Vec<u32>,
}
