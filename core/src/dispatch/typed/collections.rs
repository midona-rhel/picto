//! Handler functions for collection operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetCollectionSummaryInput {
    #[ts(type = "number")]
    pub id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateCollectionInput {
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddCollectionTagsInput {
    #[ts(type = "number")]
    pub id: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveCollectionTagsInput {
    #[ts(type = "number")]
    pub id: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_collections(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<Vec<crate::db::types::CollectionRecord>, String> {
    state.engine.get_collections()
}

pub async fn get_collection_summary(
    state: &AppState,
    input: GetCollectionSummaryInput,
) -> Result<crate::db::types::CollectionSummary, String> {
    state.engine.get_collection_summary(input.id)
}

pub async fn create_collection(
    state: &AppState,
    input: CreateCollectionInput,
) -> Result<i64, String> {
    state.engine.create_collection(&input.name)
}

pub async fn update_collection(
    state: &AppState,
    input: UpdateCollectionInput,
) -> Result<(), String> {
    if input.tags.is_some() {
        return Err(
            "Collection tag editing no longer lives in update_collection; use the canonical tag commands instead"
                .to_string(),
        );
    }
    let Some(name) = input.name.as_deref() else {
        return Ok(());
    };
    state.engine.update_collection(input.id, name)
}

// add_collection_tags / remove_collection_tags removed —
// collection tags use the generic add_tags/remove_tags path via entity_tag_raw.

pub async fn reorder_collection_members(
    state: &AppState,
    input: ReorderCollectionMembersInput,
) -> Result<(), String> {
    state
        .engine
        .reorder_collection_members_by_hashes(input.id, &input.hashes)?;
    crate::events::emit_state_changed(
        "reorder_collection_members",
        crate::runtime_contract::change_builder::ChangeImpact::collection_item_reorder(input.id),
    );
    Ok(())
}

pub async fn add_collection_members(
    state: &AppState,
    input: AddCollectionMembersInput,
) -> Result<usize, String> {
    Ok(state
        .engine
        .add_collection_members_by_hashes(input.id, &input.hashes)?
        .added
        .len())
}

pub async fn remove_collection_members(
    state: &AppState,
    input: RemoveCollectionMembersInput,
) -> Result<usize, String> {
    Ok(state
        .engine
        .remove_collection_members_by_hashes(input.id, &input.hashes)?
        .removed
        .len())
}

pub async fn delete_collection(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<(), String> {
    state.engine.delete_collection(input.id)
}

pub async fn list_collection_member_hashes(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<Vec<String>, String> {
    state.engine.list_collection_member_hashes(input.id)
}
