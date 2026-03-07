//! Typed command implementations for folder and collection operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Serde helper: null → Some(""), absent → None ─────────────────────────

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    if val.is_null() {
        Ok(Some(String::new()))
    } else if let Some(s) = val.as_str() {
        Ok(Some(s.to_string()))
    } else {
        Err(serde::de::Error::custom("expected string or null"))
    }
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFolderFilesInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFolderCoverHashInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFileFoldersInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetEntityFoldersInput {
    #[ts(type = "number")]
    pub entity_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct MoveFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub sibling_order: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct CreateFolderInput {
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub auto_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateFolderParentInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct AddFileToFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct AddFilesToFolderBatchInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RemoveFileFromFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RemoveFilesFromFolderBatchInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReorderFoldersInput {
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReorderFolderItemsInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub moves: Vec<crate::types::FolderReorderMove>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SortFolderItemsInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub sort_by: String,
    pub direction: String,
    pub hashes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReverseFolderItemsInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hashes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetCollectionSummaryInput {
    #[ts(type = "number")]
    pub id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct CreateCollectionInput {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    #[ts(type = "string | null")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    #[ts(type = "string | null")]
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(alias = "sourceUrls")]
    pub source_urls: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetCollectionRatingInput {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetCollectionSourceUrlsInput {
    #[ts(type = "number")]
    pub id: i64,
    #[serde(alias = "sourceUrls")]
    pub source_urls: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReorderCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct AddCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RemoveCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct ListFolders;
pub struct GetFolderFiles;
pub struct GetFolderCoverHash;
pub struct GetFileFolders;
pub struct GetEntityFolders;
pub struct MoveFolder;
pub struct CreateFolder;
pub struct UpdateFolder;
pub struct DeleteFolder;
pub struct UpdateFolderParent;
pub struct AddFileToFolder;
pub struct AddFilesToFolderBatch;
pub struct RemoveFileFromFolder;
pub struct RemoveFilesFromFolderBatch;
pub struct ReorderFolders;
pub struct ReorderFolderItems;
pub struct SortFolderItems;
pub struct ReverseFolderItems;
pub struct GetCollections;
pub struct GetCollectionSummary;
pub struct CreateCollection;
pub struct UpdateCollection;
pub struct SetCollectionRating;
pub struct SetCollectionSourceUrls;
pub struct ReorderCollectionMembers;
pub struct AddCollectionMembers;
pub struct RemoveCollectionMembers;
pub struct DeleteCollection;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for ListFolders {
    const NAME: &'static str = "list_folders";
    type Input = serde_json::Value;
    type Output = Vec<crate::folders::db::Folder>;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        state.db.list_folders().await
    }
}

impl TypedCommand for GetFolderFiles {
    const NAME: &'static str = "get_folder_files";
    type Input = GetFolderFilesInput;
    type Output = Vec<String>;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.get_folder_entity_hashes(input.folder_id).await
    }
}

impl TypedCommand for GetFolderCoverHash {
    const NAME: &'static str = "get_folder_cover_hash";
    type Input = GetFolderCoverHashInput;
    type Output = Option<String>;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.get_folder_cover_hash(input.folder_id).await
    }
}

impl TypedCommand for GetFileFolders {
    const NAME: &'static str = "get_file_folders";
    type Input = GetFileFoldersInput;
    type Output = Vec<crate::folders::db::FolderMembership>;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.get_entity_folder_memberships(&input.hash).await
    }
}

impl TypedCommand for GetEntityFolders {
    const NAME: &'static str = "get_entity_folders";
    type Input = GetEntityFoldersInput;
    type Output = Vec<crate::folders::db::FolderMembership>;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.get_entity_folder_memberships_by_entity_id(input.entity_id).await
    }
}

impl TypedCommand for MoveFolder {
    const NAME: &'static str = "move_folder";
    type Input = MoveFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.move_folder(input.folder_id, input.new_parent_id, input.sibling_order).await?;
        crate::events::emit_mutation(
            "move_folder",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .folder_ids(vec![input.folder_id]),
        );
        Ok(())
    }
}

impl TypedCommand for CreateFolder {
    const NAME: &'static str = "create_folder";
    type Input = CreateFolderInput;
    type Output = crate::folders::db::Folder;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let folder = crate::folders::controller::FolderController::create_folder(
            &state.db, input.name, input.parent_id, input.icon, input.color,
        ).await?;
        crate::events::emit_mutation(
            "create_folder",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders),
        );
        Ok(folder)
    }
}

impl TypedCommand for UpdateFolder {
    const NAME: &'static str = "update_folder";
    type Input = UpdateFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::folders::controller::FolderController::update_folder(
            &state.db, input.folder_id, input.name, input.icon, input.color, input.auto_tags,
        ).await?;
        crate::events::emit_mutation(
            "update_folder",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .folder_ids(vec![input.folder_id]),
        );
        Ok(())
    }
}

impl TypedCommand for DeleteFolder {
    const NAME: &'static str = "delete_folder";
    type Input = DeleteFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::folders::controller::FolderController::delete_folder(&state.db, input.folder_id).await?;
        crate::events::emit_mutation(
            "delete_folder",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .folder_ids(vec![input.folder_id])
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for UpdateFolderParent {
    const NAME: &'static str = "update_folder_parent";
    type Input = UpdateFolderParentInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::folders::controller::FolderController::update_folder_parent(
            &state.db, input.folder_id, input.new_parent_id,
        ).await?;
        crate::events::emit_mutation(
            "update_folder_parent",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .folder_ids(vec![input.folder_id]),
        );
        Ok(())
    }
}

impl TypedCommand for AddFileToFolder {
    const NAME: &'static str = "add_file_to_folder";
    type Input = AddFileToFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.add_entity_to_folder(input.folder_id, &input.hash).await?;
        crate::events::emit_mutation(
            "add_file_to_folder",
            crate::events::MutationImpact::folder_file_change(input.folder_id),
        );
        Ok(())
    }
}

impl TypedCommand for AddFilesToFolderBatch {
    const NAME: &'static str = "add_files_to_folder_batch";
    type Input = AddFilesToFolderBatchInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let count = state.db.add_entities_to_folder_batch(input.folder_id, &input.hashes).await?;
        if count > 0 {
            crate::events::emit_mutation(
                "add_files_to_folder_batch",
                crate::events::MutationImpact::folder_file_change(input.folder_id),
            );
        }
        Ok(count)
    }
}

impl TypedCommand for RemoveFileFromFolder {
    const NAME: &'static str = "remove_file_from_folder";
    type Input = RemoveFileFromFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.remove_entity_from_folder(input.folder_id, &input.hash).await?;
        crate::events::emit_mutation(
            "remove_file_from_folder",
            crate::events::MutationImpact::folder_file_change(input.folder_id),
        );
        Ok(())
    }
}

impl TypedCommand for RemoveFilesFromFolderBatch {
    const NAME: &'static str = "remove_files_from_folder_batch";
    type Input = RemoveFilesFromFolderBatchInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let count = state.db.remove_entities_from_folder_batch(input.folder_id, &input.hashes).await?;
        if count > 0 {
            crate::events::emit_mutation(
                "remove_files_from_folder_batch",
                crate::events::MutationImpact::folder_file_change(input.folder_id),
            );
        }
        Ok(count)
    }
}

impl TypedCommand for ReorderFolders {
    const NAME: &'static str = "reorder_folders";
    type Input = ReorderFoldersInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.reorder_folders(input.moves).await?;
        crate::events::emit_mutation(
            "reorder_folders",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders),
        );
        Ok(())
    }
}

impl TypedCommand for ReorderFolderItems {
    const NAME: &'static str = "reorder_folder_items";
    type Input = ReorderFolderItemsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::folders::controller::FolderController::reorder_folder_items(
            &state.db, input.folder_id, input.moves,
        ).await?;
        crate::events::emit_mutation(
            "reorder_folder_items",
            crate::events::MutationImpact::folder_item_reorder(input.folder_id),
        );
        Ok(())
    }
}

impl TypedCommand for SortFolderItems {
    const NAME: &'static str = "sort_folder_items";
    type Input = SortFolderItemsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.sort_folder_items(input.folder_id, input.sort_by, input.direction, input.hashes).await?;
        crate::events::emit_mutation(
            "sort_folder_items",
            crate::events::MutationImpact::folder_item_reorder(input.folder_id),
        );
        Ok(())
    }
}

impl TypedCommand for ReverseFolderItems {
    const NAME: &'static str = "reverse_folder_items";
    type Input = ReverseFolderItemsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.reverse_folder_items(input.folder_id, input.hashes).await?;
        crate::events::emit_mutation(
            "reverse_folder_items",
            crate::events::MutationImpact::folder_item_reorder(input.folder_id),
        );
        Ok(())
    }
}

impl TypedCommand for GetCollections {
    const NAME: &'static str = "get_collections";
    type Input = serde_json::Value;
    type Output = Vec<crate::folders::collections_db::CollectionRecord>;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        state.db.list_collections().await
    }
}

impl TypedCommand for GetCollectionSummary {
    const NAME: &'static str = "get_collection_summary";
    type Input = GetCollectionSummaryInput;
    type Output = crate::folders::collections_db::CollectionSummary;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.get_collection_summary(input.id).await
    }
}

impl TypedCommand for CreateCollection {
    const NAME: &'static str = "create_collection";
    type Input = CreateCollectionInput;
    type Output = i64;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let tags = input.tags.unwrap_or_default();
        let collection_id = state.db.create_collection(
            &input.name, input.description.as_deref(), &tags,
        ).await?;
        crate::events::emit_mutation(
            "create_collection",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .grid_all()
                .selection_summary(),
        );
        Ok(collection_id)
    }
}

impl TypedCommand for UpdateCollection {
    const NAME: &'static str = "update_collection";
    type Input = UpdateCollectionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.update_collection(
            input.id,
            input.name.as_deref(),
            input.description.as_deref(),
            input.tags.as_deref(),
            input.source_urls.as_deref(),
        ).await?;
        crate::events::emit_mutation(
            "update_collection",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .folder_ids(vec![input.id]),
        );
        crate::events::emit_mutation(
            "update_collection_grid",
            crate::events::MutationImpact::selection_metadata_grid(),
        );
        Ok(())
    }
}

impl TypedCommand for SetCollectionRating {
    const NAME: &'static str = "set_collection_rating";
    type Input = SetCollectionRatingInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.set_collection_rating(input.id, input.rating).await?;
        crate::events::emit_mutation(
            "set_collection_rating",
            crate::events::MutationImpact::collection_update(input.id),
        );
        Ok(())
    }
}

impl TypedCommand for SetCollectionSourceUrls {
    const NAME: &'static str = "set_collection_source_urls";
    type Input = SetCollectionSourceUrlsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.set_collection_source_urls(input.id, &input.source_urls).await?;
        crate::events::emit_mutation(
            "set_collection_source_urls",
            crate::events::MutationImpact::collection_update(input.id),
        );
        Ok(())
    }
}

impl TypedCommand for ReorderCollectionMembers {
    const NAME: &'static str = "reorder_collection_members";
    type Input = ReorderCollectionMembersInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.reorder_collection_members_by_hashes(input.id, &input.hashes).await?;
        state.db.scope_cache_invalidate_scope("collection");
        crate::events::emit_mutation(
            "reorder_collection_members",
            crate::events::MutationImpact::domain_only(crate::events::Domain::Files)
                .grid_all(),
        );
        Ok(())
    }
}

/// Look up the cover-file hash for a collection entity (best-effort, returns None on error).
async fn collection_cover_hash(
    db: &crate::sqlite::SqliteDatabase,
    entity_id: i64,
) -> Option<String> {
    db.with_read_conn(move |conn| {
        use rusqlite::OptionalExtension;
        conn.query_row(
            "SELECT f.hash FROM media_entity me \
             JOIN file f ON f.file_id = me.cover_file_id \
             WHERE me.entity_id = ?1",
            [entity_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .ok()
    .flatten()
}

impl TypedCommand for AddCollectionMembers {
    const NAME: &'static str = "add_collection_members";
    type Input = AddCollectionMembersInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let added = state.db.add_collection_members_by_hashes(input.id, &input.hashes).await?;
        state.db.scope_cache_invalidate_scope("collection");
        let cover_hash = collection_cover_hash(&state.db, input.id).await;
        let mut impact = crate::events::MutationImpact::all_domains_change(&state.db)
            .grid_scopes(vec![format!("collection:{}", input.id), "folder:all".into()]);
        if let Some(h) = cover_hash {
            impact = impact.metadata_hashes(vec![h]);
        }
        crate::events::emit_mutation("add_collection_members", impact);
        Ok(added)
    }
}

impl TypedCommand for RemoveCollectionMembers {
    const NAME: &'static str = "remove_collection_members";
    type Input = RemoveCollectionMembersInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let removed = state.db.remove_collection_members_by_hashes(input.id, &input.hashes).await?;
        state.db.scope_cache_invalidate_scope("collection");
        let cover_hash = collection_cover_hash(&state.db, input.id).await;
        let mut impact = crate::events::MutationImpact::all_domains_change(&state.db)
            .grid_scopes(vec![format!("collection:{}", input.id), "folder:all".into()]);
        if let Some(h) = cover_hash {
            impact = impact.metadata_hashes(vec![h]);
        }
        crate::events::emit_mutation("remove_collection_members", impact);
        Ok(removed)
    }
}

impl TypedCommand for DeleteCollection {
    const NAME: &'static str = "delete_collection";
    type Input = DeleteCollectionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.delete_collection(input.id).await?;
        state.db.scope_cache_invalidate_scope("collection");
        crate::events::emit_mutation(
            "delete_collection",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                .folder_ids(vec![input.id])
                .grid_all()
                .selection_summary(),
        );
        Ok(())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        ListFolders::NAME => Some(run_typed::<ListFolders>(state, args).await),
        GetFolderFiles::NAME => Some(run_typed::<GetFolderFiles>(state, args).await),
        GetFolderCoverHash::NAME => Some(run_typed::<GetFolderCoverHash>(state, args).await),
        GetFileFolders::NAME => Some(run_typed::<GetFileFolders>(state, args).await),
        GetEntityFolders::NAME => Some(run_typed::<GetEntityFolders>(state, args).await),
        MoveFolder::NAME => Some(run_typed::<MoveFolder>(state, args).await),
        CreateFolder::NAME => Some(run_typed::<CreateFolder>(state, args).await),
        UpdateFolder::NAME => Some(run_typed::<UpdateFolder>(state, args).await),
        DeleteFolder::NAME => Some(run_typed::<DeleteFolder>(state, args).await),
        UpdateFolderParent::NAME => Some(run_typed::<UpdateFolderParent>(state, args).await),
        AddFileToFolder::NAME => Some(run_typed::<AddFileToFolder>(state, args).await),
        AddFilesToFolderBatch::NAME => Some(run_typed::<AddFilesToFolderBatch>(state, args).await),
        RemoveFileFromFolder::NAME => Some(run_typed::<RemoveFileFromFolder>(state, args).await),
        RemoveFilesFromFolderBatch::NAME => Some(run_typed::<RemoveFilesFromFolderBatch>(state, args).await),
        ReorderFolders::NAME => Some(run_typed::<ReorderFolders>(state, args).await),
        ReorderFolderItems::NAME => Some(run_typed::<ReorderFolderItems>(state, args).await),
        SortFolderItems::NAME => Some(run_typed::<SortFolderItems>(state, args).await),
        ReverseFolderItems::NAME => Some(run_typed::<ReverseFolderItems>(state, args).await),
        GetCollections::NAME => Some(run_typed::<GetCollections>(state, args).await),
        GetCollectionSummary::NAME => Some(run_typed::<GetCollectionSummary>(state, args).await),
        CreateCollection::NAME => Some(run_typed::<CreateCollection>(state, args).await),
        UpdateCollection::NAME => Some(run_typed::<UpdateCollection>(state, args).await),
        SetCollectionRating::NAME => Some(run_typed::<SetCollectionRating>(state, args).await),
        SetCollectionSourceUrls::NAME => Some(run_typed::<SetCollectionSourceUrls>(state, args).await),
        ReorderCollectionMembers::NAME => Some(run_typed::<ReorderCollectionMembers>(state, args).await),
        AddCollectionMembers::NAME => Some(run_typed::<AddCollectionMembers>(state, args).await),
        RemoveCollectionMembers::NAME => Some(run_typed::<RemoveCollectionMembers>(state, args).await),
        DeleteCollection::NAME => Some(run_typed::<DeleteCollection>(state, args).await),
        _ => None,
    }
}
