//! Single test to generate all TypeScript bindings via ts-rs.
//!
//! Run with: `cargo test --test export_bindings`
//!
//! This replaces 123 individual `#[ts(export)]`-generated tests with one
//! invocation that writes all `.ts` files to `src/shared/types/generated/`.

use ts_rs::TS;

#[test]
fn export_all_bindings() {
    // dispatch::typed
    picto_core::dispatch::typed::duplicates::ScanDuplicatesInput::export().unwrap();
    picto_core::dispatch::typed::duplicates::GetDuplicatePairsInput::export().unwrap();
    picto_core::dispatch::typed::duplicates::ResolveDuplicatePairInput::export().unwrap();
    picto_core::dispatch::typed::duplicates::UpdateDuplicateSettingsInput::export().unwrap();

    picto_core::dispatch::typed::folders::GetFolderFilesInput::export().unwrap();
    picto_core::dispatch::typed::folders::GetFolderCoverHashInput::export().unwrap();
    picto_core::dispatch::typed::folders::GetFileFoldersInput::export().unwrap();
    picto_core::dispatch::typed::folders::GetEntityFoldersInput::export().unwrap();
    picto_core::dispatch::typed::folders::MoveFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::CreateFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::UpdateFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::DeleteFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::UpdateFolderParentInput::export().unwrap();
    picto_core::dispatch::typed::folders::AddFilesToFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::RemoveFilesFromFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::ReorderFoldersInput::export().unwrap();
    picto_core::dispatch::typed::folders::ReorderFolderItemsInput::export().unwrap();
    picto_core::dispatch::typed::folders::GetCollectionSummaryInput::export().unwrap();
    picto_core::dispatch::typed::folders::CreateCollectionInput::export().unwrap();
    picto_core::dispatch::typed::folders::UpdateCollectionInput::export().unwrap();
    picto_core::dispatch::typed::folders::ReorderCollectionMembersInput::export().unwrap();
    picto_core::dispatch::typed::folders::AddCollectionMembersInput::export().unwrap();
    picto_core::dispatch::typed::folders::RemoveCollectionMembersInput::export().unwrap();
    picto_core::dispatch::typed::folders::DeleteCollectionInput::export().unwrap();

    picto_core::dispatch::typed::media_io::ResolveFilePathInput::export().unwrap();
    picto_core::dispatch::typed::media_io::OpenFileDefaultInput::export().unwrap();
    picto_core::dispatch::typed::media_io::RevealInFolderInput::export().unwrap();
    picto_core::dispatch::typed::media_io::OpenInNewWindowInput::export().unwrap();
    picto_core::dispatch::typed::media_io::ResolveThumbnailPathInput::export().unwrap();
    picto_core::dispatch::typed::media_io::EnsureThumbnailInput::export().unwrap();
    picto_core::dispatch::typed::media_io::RegenerateThumbnailInput::export().unwrap();
    picto_core::dispatch::typed::media_io::RegenerateThumbnailsBatchInput::export().unwrap();
    picto_core::dispatch::typed::media_io::ReanalyzeFileColorsInput::export().unwrap();

    picto_core::dispatch::typed::media_lifecycle::ImportFilesInput::export().unwrap();
    picto_core::dispatch::typed::media_metadata::GetMediaEntityMetadataInput::export().unwrap();

    picto_core::dispatch::typed::smart_folders::ReorderSmartFoldersInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::CreateSmartFolderInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::UpdateSmartFolderInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::DeleteSmartFolderInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::CountSmartFolderInput::export().unwrap();

    picto_core::dispatch::typed::subscriptions::CreateGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::DeleteGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::RenameGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::SetGroupScheduleInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::RunGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::StopGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::GetSiteMetadataSchemaInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::ValidateSiteMetadataInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::CreateSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::DeleteSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::PauseSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::AddSubscriptionQueryInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::DeleteSubscriptionQueryInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::PauseSubscriptionQueryInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::RunSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::StopSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::ResetSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::RenameSubscriptionInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::RunSubscriptionQueryInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::SetCredentialInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::DeleteCredentialInput::export().unwrap();

    picto_core::dispatch::typed::system::OpenExternalUrlInput::export().unwrap();
    picto_core::dispatch::typed::system::ReorderSidebarNodesInput::export().unwrap();
    picto_core::dispatch::typed::system::GetViewPrefsInput::export().unwrap();
    picto_core::dispatch::typed::system::SetViewPrefsInput::export().unwrap();
    picto_core::dispatch::typed::system::SetZoomFactorInput::export().unwrap();

    picto_core::dispatch::typed::tags::SearchTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::GetFileTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::AddTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::RemoveTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::FindFilesByTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::ManageTagAliasInput::export().unwrap();
    picto_core::dispatch::typed::tags::ManageTagImplicationInput::export().unwrap();
    picto_core::dispatch::typed::tags::GetTagRelationsInput::export().unwrap();
    picto_core::dispatch::typed::tags::MergeTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::GetTagsPaginatedInput::export().unwrap();
    picto_core::dispatch::typed::tags::RenameTagInput::export().unwrap();
    picto_core::dispatch::typed::tags::DeleteTagInput::export().unwrap();
    picto_core::dispatch::typed::tags::CompanionGetNamespaceValuesInput::export().unwrap();
    picto_core::dispatch::typed::tags::CompanionGetFilesByTagInput::export().unwrap();

    // events
    // runtime_contract
    picto_core::runtime_contract::state_change::Domain::export().unwrap();
    picto_core::runtime_contract::state_change::TagChangeDetails::export().unwrap();
    picto_core::runtime_contract::state_change::MediaMetadataField::export().unwrap();
    picto_core::runtime_contract::state_change::MediaDerivativeField::export().unwrap();
    picto_core::runtime_contract::state_change::StateChangedEvent::export().unwrap();
    picto_core::runtime_contract::state_change::StateChanges::export().unwrap();
    picto_core::runtime_contract::state_change::SidebarCounts::export().unwrap();
    picto_core::runtime_contract::snapshot::RuntimeSnapshot::export().unwrap();
    picto_core::runtime_contract::task::RuntimeTask::export().unwrap();
    picto_core::runtime_contract::task::TaskKind::export().unwrap();
    picto_core::runtime_contract::task::TaskStatus::export().unwrap();
    picto_core::runtime_contract::task::TaskProgress::export().unwrap();
    picto_core::runtime_contract::task::TaskUpsertedEvent::export().unwrap();
    picto_core::runtime_contract::task::TaskRemovedEvent::export().unwrap();

    // smart_folders
    picto_core::smart_folders::types::SmartFolderPredicate::export().unwrap();
    picto_core::smart_folders::types::SmartRuleGroup::export().unwrap();
    picto_core::smart_folders::types::MatchMode::export().unwrap();
    picto_core::smart_folders::types::PredicateRule::export().unwrap();

    // types
    picto_core::types::ImportResult::export().unwrap();
    picto_core::types::ImportBatchResult::export().unwrap();
    picto_core::types::GridScopeKind::export().unwrap();
    picto_core::types::GridSystemScopeKey::export().unwrap();
    picto_core::types::GridScopeSpec::export().unwrap();
    picto_core::types::GridFilterSpec::export().unwrap();
    picto_core::types::GridSortSpec::export().unwrap();
    picto_core::types::SelectionMode::export().unwrap();
    picto_core::types::SelectionQuerySpec::export().unwrap();
    picto_core::types::ViewPrefsPatch::export().unwrap();
    picto_core::types::FolderReorderMove::export().unwrap();
}
