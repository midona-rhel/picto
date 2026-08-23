//! Explicit TypeScript binding generator for the replacement IPC contract.
//!
//! Run with `npm run generate:bindings`. Normal Rust tests never write source files.

use ts_rs::TS;

fn normalize_generated_bindings() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/shared/types/generated");
    let mut directories = vec![root];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).expect("read generated bindings") {
            let path = entry.expect("read generated binding entry").path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("ts") {
                let source = std::fs::read_to_string(&path).expect("read generated binding");
                let normalized = source
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n";
                if source != normalized {
                    std::fs::write(path, normalized).expect("normalize generated binding");
                }
            }
        }
    }
}

macro_rules! export {
    ($output_base:expr; $($type:ty),+ $(,)?) => {
        $(<$type>::export_all_to($output_base).unwrap();)+
    };
}

fn main() {
    use picto_core::ai_runtime_v2::{
        AiModelStatus, AiRuntimeStatus, AiTagPrediction, AiThresholds, ManualPredictionRequest,
        ManualPredictionResponse, MediaPrediction,
    };
    use picto_core::app::{
        FileHash, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemSort, ItemSortField,
        ItemTarget, LibraryChanged, Lifecycle, MediaId, MutationReceipt, SortDirection,
    };
    use picto_core::auth_v2::{
        CredentialHealthRecord, CredentialRecord, SetCredentialInput, SourceCatalogEntry,
    };
    use picto_core::duplicates_v2::{
        CandidateSide, DuplicateCandidate, DuplicateScanResult, FileQuality, QualityDecision,
        ResolutionChoice, ResolutionResult,
    };
    use picto_core::folders_v2::{
        CreateFolderInput, FolderId, FolderMutationReceipt, FolderWatchInput,
        ReorderFolderChildrenInput, ReorderFolderItemsInput,
    };
    use picto_core::import_v2::{ImportEnqueueReport, ManualImportInput};
    use picto_core::ipc_v2::{
        AddSubscriptionQueryInput, AiAssignmentsInput, AiTagAssignment, ApplyTagsInput,
        AutomaticDuplicateInput, CollectionCoverInput, CreatedFolder, CreatedSmartFolder,
        CreatedSubscription, CreatedSubscriptionQuery, CreatedSubscriptionRun, EmptyOutput,
        FileHashInput, FileHashesInput, FolderInput, FolderMembershipInput, ItemInput,
        LifecycleInput, LimitInput, ListTagsInput, ModelInput, MoveFolderInput,
        MoveSmartFolderInput, PatchMetadataInput, PatchViewSettingsInput, PauseSubscriptionInput,
        PauseSubscriptionQueryInput, QueryItemsInput, RenameFolderInput, RenameSubscriptionInput,
        RenameTagInput, ReorderSmartFoldersInput, ResolveDuplicateInput, ScanDuplicatesInput,
        ScheduleSubscriptionInput, ScopeInput, SiteInput, SmartFolderInput, SubscriptionInput,
        SubscriptionQueryInput, SubscriptionRunActivityInput, SubscriptionRunsInput, TagAliasInput,
        TagImplicationInput, TagInput, TargetInput, UpdateSmartFolderInput,
        UpdateSubscriptionQueryInput, ValueInput,
    };
    use picto_core::media_io_v2::{
        EnsureThumbnailResult, ExportFormat, ExportRequest, ExportResult, ResolvedFilePath,
        ThumbnailQueueResult,
    };
    use picto_core::navigation_v2::{
        CreateSmartFolderInput, FolderNavigationItem, NavigationSnapshot,
        SmartFolderMutationReceipt, SmartFolderNavigationItem,
    };
    use picto_core::operations_v2::{
        DeleteItemsResult, DetachItemsInput, GroupItemsInput, GroupItemsResult, MediaMetadataPatch,
        ReorderCollectionInput,
    };
    use picto_core::query_v2::{
        ItemDetails, ItemPage, ItemPageRequest, ItemSummary, MediaDetails, ScopeCount,
        SelectionSummary, SidebarCounts,
    };
    use picto_core::settings_v2::SettingsSnapshot;
    use picto_core::smart_v2::{MatchMode, PredicateRule, SmartFolderPredicate, SmartRuleGroup};
    use picto_core::subscription_activity_v2::{
        ActivityCounts, CurrentSubscriptionProgress, IngestAttempt, IssueCursor, IssuePage,
        IssuePageRequest, SourceItemActivity, SubscriptionIssue, SubscriptionQueryActivity,
        SubscriptionRunActivity, SubscriptionRunList, SubscriptionRunSummary,
    };
    use picto_core::subscription_catalog_v2::{
        NewSubscription, NewSubscriptionQuery, SubscriptionList, SubscriptionProgress,
        SubscriptionQueryView, SubscriptionView,
    };
    use picto_core::tags_v2::{TagPage, TagRelation, TagRelations, TagSummary};
    use picto_core::tasks_v2::{QueueCounts, TaskIssue, TaskSnapshot};

    // Export paths are relative to core/src in the derive attributes.
    let output_base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    export!(&output_base;
        ItemId, MediaId, FileHash, ItemKind, Lifecycle, ItemScope, ItemFilters,
        ItemSortField, SortDirection, ItemSort, ItemQuery, ItemTarget,
        MutationReceipt, LibraryChanged,
        ItemPageRequest, ItemSummary, ItemPage, MediaDetails, ItemDetails,
        SelectionSummary, ScopeCount, SidebarCounts,
        GroupItemsInput, GroupItemsResult, DetachItemsInput, ReorderCollectionInput,
        MediaMetadataPatch, DeleteItemsResult,
        FolderId, CreateFolderInput, ReorderFolderChildrenInput, ReorderFolderItemsInput,
        FolderWatchInput, FolderMutationReceipt,
        FolderNavigationItem, SmartFolderNavigationItem, NavigationSnapshot,
        CreateSmartFolderInput, SmartFolderMutationReceipt,
        TagSummary, TagPage, TagRelation, TagRelations,
        FileQuality, QualityDecision, DuplicateCandidate, CandidateSide,
        DuplicateScanResult, ResolutionChoice, ResolutionResult,
        NewSubscriptionQuery, NewSubscription, SubscriptionQueryView,
        SubscriptionProgress, SubscriptionView, SubscriptionList,
        ActivityCounts, SubscriptionRunSummary, SubscriptionRunList, IngestAttempt,
        SourceItemActivity, SubscriptionQueryActivity, SubscriptionRunActivity,
        CurrentSubscriptionProgress, SubscriptionIssue, IssueCursor, IssuePageRequest, IssuePage,
        CredentialRecord, CredentialHealthRecord, SetCredentialInput, SourceCatalogEntry,
        SettingsSnapshot, QueueCounts, TaskIssue, TaskSnapshot,
        ResolvedFilePath, EnsureThumbnailResult, ThumbnailQueueResult,
        ExportFormat, ExportRequest, ExportResult,
        AiModelStatus, AiRuntimeStatus, AiTagPrediction, AiThresholds,
        ManualPredictionRequest, MediaPrediction, ManualPredictionResponse,
        SmartFolderPredicate, SmartRuleGroup, MatchMode, PredicateRule,
        ManualImportInput, ImportEnqueueReport,
        QueryItemsInput, ItemInput, FileHashInput, FileHashesInput, TargetInput,
        LifecycleInput, FolderMembershipInput, CollectionCoverInput, ApplyTagsInput,
        PatchMetadataInput, ListTagsInput, TagInput, TagAliasInput, TagImplicationInput,
        RenameTagInput, LimitInput, ScanDuplicatesInput, ResolveDuplicateInput,
        AutomaticDuplicateInput, FolderInput, RenameFolderInput, MoveFolderInput,
        SmartFolderInput, UpdateSmartFolderInput, MoveSmartFolderInput,
        ReorderSmartFoldersInput, AddSubscriptionQueryInput, UpdateSubscriptionQueryInput,
        PauseSubscriptionQueryInput, SubscriptionQueryInput, SubscriptionInput,
        SubscriptionRunsInput, SubscriptionRunActivityInput, RenameSubscriptionInput,
        PauseSubscriptionInput, ScheduleSubscriptionInput, ScopeInput, SiteInput, ValueInput,
        PatchViewSettingsInput, ModelInput, AiTagAssignment, AiAssignmentsInput, EmptyOutput,
        CreatedFolder, CreatedSmartFolder, CreatedSubscription, CreatedSubscriptionQuery,
        CreatedSubscriptionRun,
    );

    normalize_generated_bindings();
}
