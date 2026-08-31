//! Explicit TypeScript binding generator for the integration-shell IPC values.
//!
//! Canonical media-library types live in `src/shared/types/canonical.ts` and
//! mirror `picto_library` directly. This generator owns only shell DTOs that
//! derive `TS`; it does not recreate a parallel media-library contract.

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
    use picto_core::ai_runtime::{AiModelStatus, AiRuntimeStatus, AiTagPrediction, AiThresholds};
    use picto_core::auth::{
        CredentialHealthRecord, CredentialRecord, SetCredentialInput, SourceCatalogEntry,
    };
    use picto_core::cloud::snapshot::RestorePoint;
    use picto_core::cloud::{
        CloudConfiguration, CloudLibraryOption, CloudSyncStatus, ConfigureCloudInput,
        HybridTimestamp,
    };
    use picto_core::dto::{FileHash, LibraryChanged};
    use picto_core::ipc::{
        AddSubscriptionQueryInput, CloudPauseInput, EmptyOutput, FileHashInput, FileHashesInput,
        GalleryImportInput, LimitInput, ListTagsInput, ModelInput, PatchViewSettingsInput,
        PauseSubscriptionInput, PauseSubscriptionQueryInput, RenameSubscriptionInput,
        ScanDuplicatesInput, ScheduleSubscriptionInput, ScopeInput,
        SetSubscriptionQueryGroupingInput, SiteInput, SubscriptionCoverCandidatesInput,
        SubscriptionCoverInput, SubscriptionDestinationInput, SubscriptionInput,
        SubscriptionPostsPerRunInput, SubscriptionQueryInput, SubscriptionRunActivityInput,
        SubscriptionRunsInput, UpdateSubscriptionQueryInput, ValueInput,
    };
    use picto_core::library_import::FolderTreeAnalysis;
    use picto_core::media_io::{
        ExportFormat, ExportResult, ResolvedFilePath, ThumbnailQueueResult,
    };
    use picto_core::settings::SettingsSnapshot;
    use picto_core::subscription_activity::{
        ActivityCounts, CurrentSubscriptionProgress, IngestAttempt, IssueCursor, IssuePage,
        IssuePageRequest, SourceItemActivity, SubscriptionIssue, SubscriptionQueryActivity,
        SubscriptionRunActivity, SubscriptionRunList, SubscriptionRunSummary,
    };
    use picto_core::subscription_catalog::{
        NewSubscription, NewSubscriptionQuery, SubscriptionCoverCandidate,
        SubscriptionCoverCandidateCursor, SubscriptionCoverCandidatePage,
        SubscriptionCoverSelection, SubscriptionDestinationPolicy, SubscriptionList,
        SubscriptionProgress, SubscriptionQueryView, SubscriptionView,
    };
    use picto_core::tasks::{QueueCounts, TaskIssue, TaskSnapshot};

    let output_base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    export!(&output_base;
        FileHash,
        LibraryChanged,
        HybridTimestamp,
        CloudSyncStatus,
        ConfigureCloudInput,
        CloudLibraryOption,
        CloudConfiguration,
        RestorePoint,
        NewSubscriptionQuery,
        NewSubscription,
        SubscriptionCoverCandidate,
        SubscriptionCoverCandidateCursor,
        SubscriptionCoverCandidatePage,
        SubscriptionCoverSelection,
        SubscriptionDestinationPolicy,
        SubscriptionQueryView,
        SubscriptionProgress,
        SubscriptionView,
        SubscriptionList,
        ActivityCounts,
        SubscriptionRunSummary,
        SubscriptionRunList,
        IngestAttempt,
        SourceItemActivity,
        SubscriptionQueryActivity,
        SubscriptionRunActivity,
        CurrentSubscriptionProgress,
        SubscriptionIssue,
        IssueCursor,
        IssuePageRequest,
        IssuePage,
        CredentialRecord,
        CredentialHealthRecord,
        SetCredentialInput,
        SourceCatalogEntry,
        SettingsSnapshot,
        QueueCounts,
        TaskIssue,
        TaskSnapshot,
        ResolvedFilePath,
        ThumbnailQueueResult,
        ExportFormat,
        ExportResult,
        FolderTreeAnalysis,
        AiModelStatus,
        AiRuntimeStatus,
        AiTagPrediction,
        AiThresholds,
        CloudPauseInput,
        FileHashInput,
        FileHashesInput,
        ListTagsInput,
        LimitInput,
        ScanDuplicatesInput,
        AddSubscriptionQueryInput,
        GalleryImportInput,
        UpdateSubscriptionQueryInput,
        PauseSubscriptionQueryInput,
        SetSubscriptionQueryGroupingInput,
        SubscriptionQueryInput,
        SubscriptionInput,
        SubscriptionCoverCandidatesInput,
        SubscriptionRunsInput,
        SubscriptionRunActivityInput,
        RenameSubscriptionInput,
        PauseSubscriptionInput,
        ScheduleSubscriptionInput,
        SubscriptionPostsPerRunInput,
        SubscriptionDestinationInput,
        SubscriptionCoverInput,
        ScopeInput,
        SiteInput,
        ValueInput,
        PatchViewSettingsInput,
        ModelInput,
        EmptyOutput,
    );

    normalize_generated_bindings();
}
