use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFolder {
    pub smart_folder_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub predicate_json: String,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub display_order: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SmartFolderPredicate {
    pub groups: Vec<SmartRuleGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SmartRuleGroup {
    pub match_mode: MatchMode,
    #[serde(default)]
    pub negate: bool,
    pub rules: Vec<PredicateRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    All,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct PredicateRule {
    pub field: String,
    pub op: String,
    #[serde(default)]
    #[ts(type = "any")]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    #[ts(type = "any")]
    pub value2: Option<serde_json::Value>,
    #[serde(default)]
    pub values: Option<Vec<String>>,
}
