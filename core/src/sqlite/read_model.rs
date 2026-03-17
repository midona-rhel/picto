//! Public read-model event and publish contracts.
//!
//! Domain writes enqueue `ReadModelEvent`s without knowing compiler internals.
//! The compiler/publish subsystem turns those events into derived artifact updates.

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DerivedArtifact {
    Files,
    Tags,
    TagGraph,
    EffectiveTags,
    MetadataProjection,
    Sidebar,
    SmartFolders,
    Bitmaps,
}

impl DerivedArtifact {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Tags => "tags",
            Self::TagGraph => "tag_graph",
            Self::EffectiveTags => "effective_tags",
            Self::MetadataProjection => "metadata_projection",
            Self::Sidebar => "sidebar",
            Self::SmartFolders => "smart_folders",
            Self::Bitmaps => "bitmaps",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ReadModelEvent {
    FileInserted { file_id: i64 },
    FileDeleted { file_id: i64 },
    FileStatusChanged { file_id: i64 },
    StatusBatchChanged,
    FileTagsChanged { file_id: i64 },
    TagChanged { tag_id: i64 },
    TagGraphChanged,
    SmartFolderChanged { smart_folder_id: i64 },
    FolderChanged { folder_id: i64 },
    DuplicateChanged,
    RebuildAll,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PublishedArtifacts {
    pub manifest_epoch: u64,
    pub artifact_versions: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ReadModelBatchResult {
    pub smart_folders_rebuilt: bool,
    pub scope_affected: bool,
    pub published: PublishedArtifacts,
}
