//! Single test to generate all TypeScript bindings via ts-rs.
//!
//! Run with: `cargo test --test export_bindings`
//!
//! This replaces 123 individual `#[ts(export)]`-generated tests with one
//! invocation that writes all `.ts` files to `src/shared/types/generated/`.

use ts_rs::TS;

fn normalize_generated_bindings() {
    let generated_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/shared/types/generated");
    let mut directories = vec![generated_root];

    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("ts") {
                continue;
            }

            let source = std::fs::read_to_string(&path).unwrap();
            let normalized = source
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            if source != normalized {
                std::fs::write(path, normalized).unwrap();
            }
        }
    }
}

#[test]
fn export_all_bindings() {
    // dispatch::typed
    picto_core::dispatch::typed::duplicates::ScanDuplicatesInput::export().unwrap();
    picto_core::dispatch::typed::duplicates::GetDuplicatePairsInput::export().unwrap();
    picto_core::dispatch::typed::duplicates::ResolveDuplicatePairInput::export().unwrap();
    picto_core::dispatch::typed::folders::GetFolderCoverHashInput::export().unwrap();
    picto_core::dispatch::typed::folders::MoveFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::CreateFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::UpdateFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::DeleteFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::RemoveFilesFromFolderInput::export().unwrap();
    picto_core::dispatch::typed::folders::ReorderFolderItemsInput::export().unwrap();
    picto_core::dispatch::typed::collections::GetCollectionSummaryInput::export().unwrap();
    picto_core::dispatch::typed::collections::CreateCollectionInput::export().unwrap();
    picto_core::dispatch::typed::collections::ReorderCollectionMembersInput::export().unwrap();
    picto_core::dispatch::typed::collections::AddCollectionMembersInput::export().unwrap();
    picto_core::dispatch::typed::collections::RemoveCollectionMembersInput::export().unwrap();
    picto_core::dispatch::typed::collections::DeleteCollectionInput::export().unwrap();

    picto_core::dispatch::typed::media_io::ResolveFilePathInput::export().unwrap();
    picto_core::dispatch::typed::media_io::OpenInNewWindowInput::export().unwrap();
    picto_core::dispatch::typed::media_io::EnsureThumbnailInput::export().unwrap();
    picto_core::dispatch::typed::media_io::RegenerateThumbnailsBatchInput::export().unwrap();

    picto_core::dispatch::typed::media_lifecycle::AddMediaInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::CreateSmartFolderInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::UpdateSmartFolderInput::export().unwrap();
    picto_core::dispatch::typed::smart_folders::DeleteSmartFolderInput::export().unwrap();

    picto_core::dispatch::typed::subscriptions::CreateGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::DeleteGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::RenameGroupInput::export().unwrap();
    picto_core::dispatch::typed::subscriptions::SetSubscriptionScheduleInput::export().unwrap();
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

    picto_core::dispatch::typed::tags::ManageTagAliasInput::export().unwrap();
    picto_core::dispatch::typed::tags::ManageTagImplicationInput::export().unwrap();
    picto_core::dispatch::typed::tags::GetTagRelationsInput::export().unwrap();
    picto_core::dispatch::typed::tags::MergeTagsInput::export().unwrap();
    picto_core::dispatch::typed::tags::GetTagsPaginatedInput::export().unwrap();
    picto_core::dispatch::typed::tags::RenameTagInput::export().unwrap();
    picto_core::dispatch::typed::tags::DeleteTagInput::export().unwrap();

    // events
    // ai_tagger
    picto_core::dispatch::typed::ai_tagger::AiTaggerStatusInput::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTaggerModelStatus::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTaggerHardware::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTaggerStatusOutput::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTaggerDownloadModelInput::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTagPredictInput::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTagCancelInput::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::FilePrediction::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTagPredictOutput::export().unwrap();
    picto_core::dispatch::typed::ai_tagger::AiTagApplyInput::export().unwrap();
    picto_core::ai_tagger::inference::TagPrediction::export().unwrap();
    picto_core::ai_tagger::models::ModelInfo::export().unwrap();
    picto_core::ai_tagger::models::ChannelOrder::export().unwrap();

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
    picto_core::types::GridScopeKind::export().unwrap();
    picto_core::types::GridSystemScopeKey::export().unwrap();
    picto_core::types::GridScopeSpec::export().unwrap();
    picto_core::types::GridFilterSpec::export().unwrap();
    picto_core::types::GridSortSpec::export().unwrap();
    picto_core::types::ViewPrefsPatch::export().unwrap();
    picto_core::types::FolderReorderMove::export().unwrap();

    normalize_generated_bindings();
}
