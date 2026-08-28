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
pub enum Lifecycle {
    Active,
    Inbox,
    Trash,
}

impl Lifecycle {
    pub const ALL: [Self; 3] = [Self::Active, Self::Inbox, Self::Trash];

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRecord {
    pub folder_id: FolderId,
    pub stable_key: String,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub display_order: i64,
    pub count: u64,
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
pub struct FolderDeleteResult {
    pub deleted_folder_ids: Vec<FolderId>,
    pub fallback_folder_id: Option<FolderId>,
    pub receipt: crate::MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub source_key: String,
    pub source_item_key: String,
    pub source_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedImport {
    pub stable_key: String,
    pub media_name: String,
    pub file_path: String,
    pub facts: ImmutableMediaFacts,
    pub lifecycle: Lifecycle,
    pub rating: Rating,
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
    pub name: Option<String>,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRequest {
    pub target: crate::selection::SelectionTarget,
    pub cover_root_id: RootId,
    pub winning_collection_id: Option<RootId>,
    pub name: Option<String>,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingBlobCleanup {
    pub file_id: FileId,
    pub file_path: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct CloudJournalRecord {
    pub journal_id: u64,
    pub revision: u64,
    pub operation_kind: String,
    pub target_root_ids: Option<roaring::RoaringBitmap>,
    pub payload: serde_json::Value,
    pub created_at_ms: i64,
}
