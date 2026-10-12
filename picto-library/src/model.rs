use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! local_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl From<$name> for u32 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

local_id!(RootId);
local_id!(MediaId);
local_id!(FileId);
local_id!(TagId);
local_id!(TagNamespaceId);
local_id!(FolderId);
local_id!(SmartFolderId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartFolderInput {
    pub name: String,
    pub parent_id: Option<SmartFolderId>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub view: crate::predicate::ViewQuerySpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartFolderDeleteResult {
    pub deleted_smart_folder_ids: Vec<SmartFolderId>,
    pub fallback_smart_folder_id: Option<SmartFolderId>,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootKind {
    Media,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentSortField {
    Name,
    ImportedAt,
    CreatedAt,
    ModifiedAt,
    Size,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Active,
    Inbox,
    Trash,
}

impl Lifecycle {
    pub const ALL: [Self; 3] = [Self::Active, Self::Inbox, Self::Trash];

    pub const fn inbox() -> Self {
        Self::Inbox
    }

    /// Watched folders may import into Active or Inbox, never Trash.
    pub(crate) fn from_watch_code(code: i64) -> Self {
        if code == 1 { Self::Active } else { Self::Inbox }
    }

    pub(crate) fn watch_code(self) -> i64 {
        if self == Self::Active { 1 } else { 2 }
    }

    pub const fn bitmap_key(self) -> u32 {
        match self {
            Self::Active => 1,
            Self::Inbox => 2,
            Self::Trash => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Unrated,
    One,
    Two,
    Three,
    Four,
    Five,
}

impl Rating {
    pub const ALL: [Self; 6] = [
        Self::Unrated,
        Self::One,
        Self::Two,
        Self::Three,
        Self::Four,
        Self::Five,
    ];

    pub const fn bitmap_key(self) -> u32 {
        match self {
            Self::Unrated => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::Five => 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabColor {
    pub l: f32,
    pub a: f32,
    pub b: f32,
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImmutableMediaFacts {
    pub mime: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub frame_count: Option<u32>,
    pub content_hash: String,
    pub perceptual_hash: Option<String>,
    #[serde(default)]
    pub palette: Vec<LabColor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootRecord {
    pub root_id: RootId,
    pub stable_key: String,
    pub kind: RootKind,
    pub name: String,
    pub notes: Option<String>,
    pub source_urls: Vec<String>,
    pub cover_media_id: MediaId,
    pub imported_at_ms: i64,
    pub captured_at_ms: Option<i64>,
    pub modified_at_ms: i64,
    pub media_count: u32,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaRecord {
    pub media_id: MediaId,
    pub media_name: String,
    pub media_notes: Option<String>,
    pub file_id: FileId,
    pub file_path: String,
    pub facts: ImmutableMediaFacts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootDetails {
    pub root: RootRecord,
    pub lifecycle: Lifecycle,
    pub rating: Rating,
    pub folder_ids: Vec<FolderId>,
    pub tag_ids: Vec<TagId>,
    pub media: Vec<MediaRecord>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRecord {
    pub folder_id: FolderId,
    pub stable_key: String,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub cover_root_id: Option<RootId>,
    pub watch_path: Option<String>,
    pub watch_enabled: bool,
    pub watch_subfolders: bool,
    pub watch_lifecycle: Lifecycle,
    pub display_order: i64,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderCover {
    pub root_id: RootId,
    pub content_hash: String,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRecord {
    pub tag_id: TagId,
    pub namespace_id: TagNamespaceId,
    pub namespace: String,
    pub subname: String,
    pub active_count: u64,
    pub assignment_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagPage {
    pub tags: Vec<TagRecord>,
    pub next_cursor: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagNamespaceRecord {
    pub namespace_id: TagNamespaceId,
    pub name: String,
    pub tag_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTagNamespaceInput {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameTagNamespaceInput {
    pub namespace_id: TagNamespaceId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagNamespaceInput {
    pub namespace_id: TagNamespaceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderCount {
    pub folder_id: FolderId,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartFolderCount {
    pub smart_folder_id: SmartFolderId,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarCounts {
    pub all: u64,
    pub inbox: u64,
    pub trash: u64,
    pub recently_viewed: u64,
    pub untagged: u64,
    pub uncategorized: u64,
    pub duplicates: u64,
    pub folders: Vec<FolderCount>,
    pub smart_folders: Vec<SmartFolderCount>,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryStatistics {
    pub active_items: i64,
    pub inbox_items: i64,
    pub trash_items: i64,
    pub standalone_items: i64,
    pub collections: i64,
    pub media_assets: i64,
    pub image_assets: i64,
    pub video_assets: i64,
    pub audio_assets: i64,
    pub other_assets: i64,
    pub physical_files: i64,
    pub original_bytes: i64,
    pub tags: i64,
    pub folders: i64,
    pub smart_folders: i64,
    pub subscriptions: i64,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderDeleteResult {
    pub deleted_folder_ids: Vec<FolderId>,
    pub fallback_folder_id: Option<FolderId>,
    pub receipt: crate::MutationReceipt,
}

pub const MAX_FOLDER_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFolderInput {
    pub name: String,
    pub parent_id: Option<FolderId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedFolder {
    pub folder_id: FolderId,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderMetadataInput {
    pub folder_id: FolderId,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderAutoTagsInput {
    pub folder_id: FolderId,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderCoverInput {
    pub folder_id: FolderId,
    pub root_id: RootId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderWatchInput {
    pub folder_id: FolderId,
    pub path: String,
    pub include_subfolders: bool,
    /// Status of files the watcher imports: Inbox (default) or Active.
    #[serde(default = "Lifecycle::inbox")]
    pub lifecycle: Lifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReorderFolderChildrenInput {
    pub parent_id: Option<FolderId>,
    pub folder_ids: Vec<FolderId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReorderFolderRootsInput {
    pub folder_id: FolderId,
    pub root_ids: Vec<RootId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortFolderTreeInput {
    pub folder_id: FolderId,
    pub descending: bool,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub source_key: String,
    pub source_item_key: String,
    pub source_text: Option<String>,
    #[serde(default)]
    pub source_attempt_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedImport {
    pub stable_key: String,
    pub media_name: String,
    pub file_path: String,
    pub facts: ImmutableMediaFacts,
    pub lifecycle: Lifecycle,
    pub rating: Rating,
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub folders: Vec<FolderId>,
    #[serde(default)]
    pub source_urls: Vec<String>,
    #[serde(default)]
    pub source_identity: Option<SourceIdentity>,
    pub imported_at_ms: i64,
    pub captured_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedCollectionImport {
    pub members: Vec<PreparedImport>,
    pub cover_index: usize,
    #[serde(default)]
    pub existing_root_id: Option<RootId>,
    pub name: Option<String>,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum PreparedIngestPayload {
    Item(PreparedImport),
    Collection(PreparedCollectionImport),
    /// A file a task's scan found but has not inspected: the ingest worker
    /// reads, hashes and probes it when it claims the job, so queueing a large
    /// folder is quick and every file is recorded before any is read.
    Pending(PendingImport),
    /// Files a task imports as one collection, likewise not yet inspected.
    PendingCollection(PendingCollectionImport),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingImport {
    pub stable_key: String,
    pub path: String,
    pub lifecycle: Lifecycle,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The destination folder, or the parent of `folder_path`.
    #[serde(default)]
    pub folders: Vec<FolderId>,
    /// A folder path below that destination (kept source structure), created
    /// once the file is read successfully.
    #[serde(default)]
    pub folder_path: Option<String>,
    #[serde(default)]
    pub source_urls: Vec<String>,
    pub imported_at_ms: i64,
    /// A zip archive, imported as a collection of its files.
    #[serde(default)]
    pub archive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingCollectionImport {
    pub members: Vec<PendingImport>,
    pub name: Option<String>,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedIngestJob {
    pub job_key: String,
    pub source_kind: String,
    pub source_path: String,
    pub source_item_id: Option<i64>,
    pub delete_after_ingest: bool,
    pub payload: PreparedIngestPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimedIngestJob {
    pub ingest_job_id: i64,
    pub source_path: String,
    pub delete_after_ingest: bool,
    pub attempt_count: u32,
    pub payload: PreparedIngestPayload,
    /// The foreground task that queued the job, if any.
    #[serde(default)]
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaWorkKind {
    Thumbnail,
    DominantColors,
    PerceptualHash,
    AiTag,
    BlobDelete,
}

impl MediaWorkKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::DominantColors => "dominant_colors",
            Self::PerceptualHash => "perceptual_hash",
            Self::AiTag => "ai_tag",
            Self::BlobDelete => "blob_delete",
        }
    }

    pub const fn priority(self) -> i64 {
        match self {
            Self::Thumbnail => 500,
            Self::DominantColors => 400,
            Self::PerceptualHash => 300,
            Self::AiTag => 200,
            Self::BlobDelete => 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedMediaWork {
    pub work_id: i64,
    pub root_id: Option<RootId>,
    pub media_id: Option<MediaId>,
    pub file_id: Option<FileId>,
    pub file_hash: Option<String>,
    pub kind: MediaWorkKind,
    pub attempt_count: u32,
}

/// Tags produced for one visible library root. Collection callers combine
/// member predictions before submitting this assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootTagAssignment {
    pub root_id: RootId,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RootMetadataPatch {
    pub rating: Option<Rating>,
    /// Absent leaves notes unchanged; an explicit `null` clears them.
    #[serde(
        default,
        deserialize_with = "present_or_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub notes: Option<Option<String>>,
    pub source_urls: Option<Vec<String>>,
}

/// A field that is present maps to `Some`, even when its value is `null`, so
/// "clear" (`Some(None)`) is distinct from "leave unchanged" (`None`).
fn present_or_null<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRename {
    pub root_id: RootId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRequest {
    pub target: crate::selection::SelectionTarget,
    pub cover_root_id: RootId,
    pub winning_collection_id: Option<RootId>,
    pub name: Option<String>,
    pub notes: Option<String>,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrganizeCollectionInput {
    pub target: crate::selection::SelectionTarget,
    pub cover_root_id: RootId,
    pub winning_collection_id: Option<RootId>,
    pub name: Option<String>,
    pub notes: Option<String>,
}

pub const MAX_ROOT_NOTES_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionNoteDraft {
    pub notes: String,
    pub source_count: u64,
    pub byte_length: u64,
    pub maximum_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizeCollectionResult {
    pub collection_id: RootId,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachCollectionInput {
    pub collection_id: RootId,
    pub media_ids: Vec<MediaId>,
    pub target_lifecycle: Option<Lifecycle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionRootsResult {
    pub root_ids: Vec<RootId>,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReorderCollectionInput {
    pub collection_id: RootId,
    pub media_ids: Vec<MediaId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingBlobCleanup {
    pub file_id: FileId,
    pub content_hash: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateStatus {
    Detected,
    NotDuplicate,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicatePair {
    pub file_id_a: FileId,
    pub file_id_b: FileId,
    pub distance: u32,
    pub status: DuplicateStatus,
    pub detected_at_ms: i64,
    pub decided_at_ms: Option<i64>,
    pub winner_file_id: Option<FileId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateFile {
    pub file_id: FileId,
    pub file_hash: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub frame_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateOccurrence {
    pub media_item_id: MediaId,
    pub root_item_id: RootId,
    pub collection_id: Option<RootId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateCandidateSide {
    pub file: DuplicateFile,
    pub occurrences: Vec<DuplicateOccurrence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuplicateQualityDecision {
    LeftBetter,
    RightBetter,
    AutoTieLeft,
    AutoTieRight,
    NeedsChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateCandidate {
    pub file_id_a: FileId,
    pub file_id_b: FileId,
    pub distance: u32,
    pub left: DuplicateCandidateSide,
    pub right: DuplicateCandidateSide,
    pub decision: DuplicateQualityDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateScanResult {
    pub candidate_count: usize,
    pub affected_item_ids: Vec<RootId>,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuplicateResolutionChoice {
    KeepBoth,
    KeepFile { winner_file_id: FileId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateResolutionResult {
    pub choice: DuplicateResolutionChoice,
    pub affected_root_ids: Vec<RootId>,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MediaFactsUpdate {
    pub mime: Option<String>,
    pub width: Option<Option<u32>>,
    pub height: Option<Option<u32>>,
    pub duration_ms: Option<Option<u64>>,
    pub frame_count: Option<Option<u32>>,
    pub perceptual_hash: Option<Option<String>>,
    pub palette: Option<Vec<LabColor>>,
}

/// A validated encoding repair; logical media identity and metadata stay intact.
#[derive(Debug, Clone)]
pub struct RepairedMediaContent {
    pub expected_hash: String,
    pub content_hash: String,
    pub file_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudJournalRecord {
    pub journal_id: u64,
    pub revision: u64,
    pub operation_kind: String,
    pub target_root_ids: Option<roaring::RoaringBitmap>,
    pub payload: serde_json::Value,
    pub created_at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metadata_patch_tells_clearing_notes_from_leaving_them() {
        let clear: RootMetadataPatch = serde_json::from_str(r#"{"notes":null}"#).unwrap();
        assert_eq!(clear.notes, Some(None));
        let set: RootMetadataPatch = serde_json::from_str(r#"{"notes":"hi"}"#).unwrap();
        assert_eq!(set.notes, Some(Some("hi".into())));
        let untouched: RootMetadataPatch = serde_json::from_str(r#"{"rating":"five"}"#).unwrap();
        assert_eq!(untouched.notes, None);
        for patch in [clear, set, untouched] {
            let round_trip: RootMetadataPatch =
                serde_json::from_str(&serde_json::to_string(&patch).unwrap()).unwrap();
            assert_eq!(round_trip, patch);
        }
    }
}

/// A foreground task: a long user-started job (an import) that reports
/// progress and can be paused, stopped, or stopped and undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    Paused,
    /// Stop-and-undo is removing what the task added.
    Reverting,
    Finished,
    /// Stopped by the user; what it added stays.
    Stopped,
    /// Stopped and undone.
    Reverted,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Reverting => "reverting",
            Self::Finished => "finished",
            Self::Stopped => "stopped",
            Self::Reverted => "reverted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "reverting" => Self::Reverting,
            "finished" => Self::Finished,
            "stopped" => Self::Stopped,
            "reverted" => Self::Reverted,
            _ => return None,
        })
    }

    /// Running or paused: the task can still do work.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub kind: String,
    pub title: String,
    pub state: TaskState,
    pub scan_complete: bool,
    /// Files found so far (final once the scan is complete).
    pub total_units: u64,
    /// Files that became new library items.
    pub added_units: u64,
    /// Files that were already in the library.
    pub existing_units: u64,
    pub failed_units: u64,
    /// Files queued or being imported.
    pub remaining_units: u64,
    pub revertible: bool,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub input_json: String,
    /// Why the task stopped on its own, if it did.
    pub error: Option<String>,
}

impl TaskRecord {
    pub fn done_units(&self) -> u64 {
        self.added_units + self.existing_units + self.failed_units
    }
}

/// One job a task queues: its job and how many files it stands for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskIngestJob {
    pub job: PreparedIngestJob,
    pub units: u64,
}
