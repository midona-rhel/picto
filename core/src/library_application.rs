//! Application-shell ownership for the greenfield media-library kernel.
//!
//! This type is the sole owner used by the cutover path. It deliberately does
//! not open the legacy `Store`; auxiliary services share the kernel's
//! `LibraryDatabase` scheduler through `library().database()`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use picto_library::{Library, RootId};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::app::{LibraryChanged, LIBRARY_CHANGED_EVENT};
use crate::blob_store::BlobStore;

const DATABASE_FILE: &str = "library.sqlite";

pub struct LibraryApplication {
    root: PathBuf,
    library: Arc<Library>,
    blobs: Arc<BlobStore>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryHistoryEntrySummary {
    pub entry_id: u64,
    pub command: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryHistoryState {
    pub undo: Option<LibraryHistoryEntrySummary>,
    pub redo: Option<LibraryHistoryEntrySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryHistoryOperationResult {
    pub entry: LibraryHistoryEntrySummary,
    pub state: LibraryHistoryState,
    pub receipt: picto_library::MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryNavigationSnapshot {
    pub folders: Vec<picto_library::FolderRecord>,
    pub smart_folders: Vec<picto_library::SmartFolderRecord>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryCreatedSmartFolder {
    pub smart_folder_id: picto_library::SmartFolderId,
    pub receipt: picto_library::MutationReceipt,
}

impl LibraryApplication {
    pub fn create(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = prepare_root(root.as_ref())?;
        let library =
            Library::create(root.join(DATABASE_FILE)).map_err(|error| error.to_string())?;
        Self::from_library(root, library)
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        let library = Library::open(root.join(DATABASE_FILE)).map_err(|error| error.to_string())?;
        Self::from_library(root, library)
    }

    fn from_library(root: PathBuf, library: Library) -> Result<Self, String> {
        let blobs = BlobStore::open(&root)
            .map_err(|error| format!("Failed to open blob store: {error}"))?;
        Ok(Self {
            root,
            library: Arc::new(library),
            blobs: Arc::new(blobs),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn library(&self) -> &Arc<Library> {
        &self.library
    }

    pub fn blobs(&self) -> &Arc<BlobStore> {
        &self.blobs
    }

    pub fn query(
        &self,
        query: &picto_library::query::RootQuery,
        mut page: picto_library::query::PageRequest,
    ) -> Result<picto_library::query::RootPage, String> {
        page.limit = page.limit.clamp(1, 500);
        self.library
            .query(query, &page)
            .map_err(|error| error.to_string())
    }

    pub fn details(&self, root_id: RootId) -> Result<picto_library::RootDetails, String> {
        self
            .library
            .details(root_id)
            .map_err(|error| error.to_string())
    }

    pub fn selection_summary(
        &self,
        target: &picto_library::selection::SelectionTarget,
    ) -> Result<picto_library::selection::SelectionSummary, String> {
        self
            .library
            .selection_summary(target)
            .map_err(|error| error.to_string())
    }

    pub fn record_recent_view(&self, root_id: RootId) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .record_recent_view(
                root_id,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn clear_recent_views(&self) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .clear_recent_views()
            .map_err(|error| error.to_string())
    }

    pub fn set_lifecycle(
        &self,
        target: &picto_library::selection::SelectionTarget,
        lifecycle: picto_library::Lifecycle,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .set_lifecycle(target, lifecycle)
            .map_err(|error| error.to_string())
    }

    pub fn set_folder_membership(
        &self,
        target: &picto_library::selection::SelectionTarget,
        folder_id: picto_library::FolderId,
        present: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        let result = if present {
            self.library.add_to_folder(target, folder_id)
        } else {
            self.library.remove_from_folder(target, folder_id)
        };
        result.map_err(|error| error.to_string())
    }

    pub fn apply_tags(
        &self,
        target: &picto_library::selection::SelectionTarget,
        tags: &[String],
        add: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .apply_tags(target, tags, add)
            .map_err(|error| error.to_string())
    }

    pub fn rename_item(
        &self,
        root_id: RootId,
        name: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .rename_root(
                root_id,
                name,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn rename_items(
        &self,
        renames: &[picto_library::RootRename],
    ) -> Result<picto_library::MutationReceipt, String> {
        let renames = renames
            .iter()
            .map(|rename| (rename.root_id, rename.name.clone()))
            .collect::<Vec<_>>();
        self.library
            .rename_roots(&renames, chrono::Utc::now().timestamp_millis())
            .map_err(|error| error.to_string())
    }

    pub fn patch_metadata(
        &self,
        target: &picto_library::selection::SelectionTarget,
        patch: &picto_library::RootMetadataPatch,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .patch_metadata(
                target,
                patch.rating,
                patch.notes.clone(),
                patch.source_urls.clone(),
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn organize_into_collection(
        &self,
        input: picto_library::OrganizeCollectionInput,
    ) -> Result<picto_library::OrganizeCollectionResult, String> {
        let (collection_id, receipt) = self
            .library
            .organize_into_collection(&picto_library::GroupRequest {
                target: input.target,
                cover_root_id: input.cover_root_id,
                winning_collection_id: input.winning_collection_id,
                name: input.name,
                modified_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(|error| error.to_string())?;
        Ok(picto_library::OrganizeCollectionResult {
            collection_id,
            receipt,
        })
    }

    pub fn ungroup_collection(
        &self,
        collection_id: RootId,
    ) -> Result<picto_library::CollectionRootsResult, String> {
        let (root_ids, receipt) = self
            .library
            .ungroup_collection(
                collection_id,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())?;
        Ok(picto_library::CollectionRootsResult { root_ids, receipt })
    }

    pub fn detach_items(
        &self,
        input: picto_library::DetachCollectionInput,
    ) -> Result<picto_library::CollectionRootsResult, String> {
        let (root_ids, receipt) = self
            .library
            .detach_collection_members(
                input.collection_id,
                input.media_ids,
                input.target_lifecycle,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())?;
        Ok(picto_library::CollectionRootsResult { root_ids, receipt })
    }

    pub fn reorder_collection(
        &self,
        input: picto_library::ReorderCollectionInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .reorder_collection(
                input.collection_id,
                input.media_ids,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn create_folder(
        &self,
        input: picto_library::CreateFolderInput,
    ) -> Result<picto_library::CreatedFolder, String> {
        let (folder_id, receipt) = self
            .library
            .create_folder(&input.name, input.parent_id)
            .map_err(|error| error.to_string())?;
        Ok(picto_library::CreatedFolder {
            folder_id,
            receipt,
        })
    }

    pub fn rename_folder(
        &self,
        folder_id: picto_library::FolderId,
        name: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .rename_folder(folder_id, name)
            .map_err(|error| error.to_string())
    }

    pub fn duplicate_folder(
        &self,
        folder_id: picto_library::FolderId,
    ) -> Result<picto_library::CreatedFolder, String> {
        let (folder_id, receipt) = self
            .library
            .duplicate_folder(folder_id)
            .map_err(|error| error.to_string())?;
        Ok(picto_library::CreatedFolder { folder_id, receipt })
    }

    pub fn set_folder_metadata(
        &self,
        input: &picto_library::FolderMetadataInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .set_folder_metadata(
                input.folder_id,
                input.icon.as_deref(),
                input.color.as_deref(),
                input.notes.as_deref(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn folder_auto_tags(
        &self,
        folder_id: picto_library::FolderId,
    ) -> Result<Vec<String>, String> {
        self.library
            .folder_auto_tags(folder_id)
            .map_err(|error| error.to_string())
    }

    pub fn set_folder_auto_tags(
        &self,
        input: &picto_library::FolderAutoTagsInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        let snapshot = self.library.projections().snapshot();
        let mut tag_ids = input
            .tags
            .iter()
            .map(|name| {
                let name = name.trim();
                snapshot
                    .tag_ids_by_name
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("Auto-tag {name} does not exist"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        tag_ids.sort_unstable();
        tag_ids.dedup();
        drop(snapshot);
        self
            .library
            .set_folder_auto_tags(
                input.folder_id,
                tag_ids,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn folder_cover(
        &self,
        folder_id: picto_library::FolderId,
    ) -> Result<Option<picto_library::FolderCover>, String> {
        self.library
            .folder_cover(folder_id)
            .map_err(|error| error.to_string())
    }

    pub fn set_folder_cover(
        &self,
        input: &picto_library::FolderCoverInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .set_folder_cover(input.folder_id, input.root_id)
            .map_err(|error| error.to_string())
    }

    pub fn set_folder_watch(
        &self,
        input: &picto_library::FolderWatchInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        let path = std::fs::canonicalize(input.path.trim())
            .map_err(|error| format!("Failed to resolve watched folder: {error}"))?;
        if !path.is_dir() {
            return Err(format!("Watched path is not a directory: {}", path.display()));
        }
        let library_root = std::fs::canonicalize(self.root())
            .unwrap_or_else(|_| self.root().to_path_buf());
        if path.starts_with(library_root) {
            return Err("A watched folder cannot be inside the Picto library".into());
        }
        self.library
            .set_folder_watch(
                input.folder_id,
                &path.to_string_lossy(),
                input.include_subfolders,
            )
            .map_err(|error| error.to_string())
    }

    pub fn clear_folder_watch(
        &self,
        folder_id: picto_library::FolderId,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .clear_folder_watch(folder_id)
            .map_err(|error| error.to_string())
    }

    pub fn move_folder(
        &self,
        folder_id: picto_library::FolderId,
        parent_id: Option<picto_library::FolderId>,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .move_folder(folder_id, parent_id)
            .map_err(|error| error.to_string())
    }

    pub fn reorder_folder_children(
        &self,
        input: &picto_library::ReorderFolderChildrenInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .reorder_folder_children(input.parent_id, &input.folder_ids)
            .map_err(|error| error.to_string())
    }

    pub fn reorder_folder_items(
        &self,
        input: &picto_library::ReorderFolderRootsInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .reorder_folder_items(input.folder_id, &input.root_ids)
            .map_err(|error| error.to_string())
    }

    pub fn sort_folder_tree(
        &self,
        input: &picto_library::SortFolderTreeInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .sort_folder_tree(input.folder_id, input.descending, input.recursive)
            .map_err(|error| error.to_string())
    }

    pub fn sort_folder_items_by_name(
        &self,
        folder_id: picto_library::FolderId,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .sort_folder_items_by_name(folder_id)
            .map_err(|error| error.to_string())
    }

    pub fn delete_folders(
        &self,
        folder_ids: &[picto_library::FolderId],
    ) -> Result<picto_library::FolderDeleteResult, String> {
        self
            .library
            .delete_folders(folder_ids)
            .map_err(|error| error.to_string())
    }

    pub fn create_smart_folder(
        &self,
        input: picto_library::SmartFolderInput,
    ) -> Result<LibraryCreatedSmartFolder, String> {
        let (smart_folder_id, receipt) = self
            .library
            .create_smart_folder(input)
            .map_err(|error| error.to_string())?;
        Ok(LibraryCreatedSmartFolder {
            smart_folder_id,
            receipt,
        })
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: picto_library::SmartFolderId,
        input: picto_library::SmartFolderInput,
    ) -> Result<picto_library::MutationReceipt, String> {
        self
            .library
            .update_smart_folder(smart_folder_id, input)
            .map_err(|error| error.to_string())
    }

    pub fn move_smart_folder(
        &self,
        smart_folder_id: i64,
        parent_id: Option<i64>,
    ) -> Result<picto_library::MutationReceipt, String> {
        let smart_folder_id = checked_smart_folder_id(smart_folder_id)?;
        let mut record = self
            .library
            .smart_folders()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|record| record.smart_folder_id == smart_folder_id)
            .ok_or_else(|| format!("Smart folder {} does not exist", smart_folder_id.0))?;
        record.parent_id = parent_id.map(checked_smart_folder_id).transpose()?;
        self
            .library
            .update_smart_folder(
                smart_folder_id,
                picto_library::SmartFolderInput {
                    name: record.name,
                    parent_id: record.parent_id,
                    icon: record.icon,
                    color: record.color,
                    notes: record.notes,
                    view: record.view,
                },
            )
            .map_err(|error| error.to_string())
    }

    pub fn reorder_smart_folder_children(
        &self,
        parent_id: Option<i64>,
        smart_folder_ids: &[i64],
    ) -> Result<picto_library::MutationReceipt, String> {
        let parent_id = parent_id.map(checked_smart_folder_id).transpose()?;
        let ids = smart_folder_ids
            .iter()
            .copied()
            .map(checked_smart_folder_id)
            .collect::<Result<Vec<_>, String>>()?;
        self
            .library
            .reorder_smart_folder_children(parent_id, &ids)
            .map_err(|error| error.to_string())
    }

    pub fn delete_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<picto_library::SmartFolderDeleteResult, String> {
        self
            .library
            .delete_smart_folder(checked_smart_folder_id(smart_folder_id)?)
            .map_err(|error| error.to_string())
    }

    pub fn navigation(&self) -> Result<LibraryNavigationSnapshot, String> {
        let (folders, smart_folders, revision) =
            self.library.navigation().map_err(|error| error.to_string())?;
        Ok(LibraryNavigationSnapshot {
            folders,
            smart_folders,
            revision,
        })
    }

    pub fn list_tags(
        &self,
        namespace: Option<&str>,
        search: Option<&str>,
        cursor: Option<&str>,
        limit: i64,
    ) -> Result<picto_library::TagPage, String> {
        let cursor = cursor
            .filter(|value| !value.is_empty())
            .map(str::parse::<u32>)
            .transpose()
            .map_err(|_| "Invalid tag cursor".to_string())?
            .unwrap_or(0);
        let search = search
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        let limit = usize::try_from(limit.clamp(1, 500)).expect("positive tag limit");
        let (tags, revision) = self
            .library
            .tags_with_revision()
            .map_err(|error| error.to_string())?;
        let mut tags = tags
            .into_iter()
            .filter(|tag| tag.tag_id.0 > cursor)
            .filter(|tag| namespace.is_none_or(|namespace| tag.namespace == namespace))
            .filter(|tag| {
                search
                    .as_ref()
                    .is_none_or(|search| tag.subname.to_lowercase().contains(search))
            })
            .collect::<Vec<_>>();
        tags.sort_by_key(|tag| tag.tag_id.0);
        let next_cursor = (tags.len() > limit).then(|| tags[limit - 1].tag_id.0.to_string());
        tags.truncate(limit);
        Ok(picto_library::TagPage {
            tags,
            next_cursor,
            revision,
        })
    }

    pub fn tag_namespace_counts(&self) -> Result<Vec<(String, u64)>, String> {
        let tags = self.library.tags().map_err(|error| error.to_string())?;
        let mut counts = std::collections::BTreeMap::<String, u64>::new();
        for tag in tags {
            *counts.entry(tag.namespace).or_default() += 1;
        }
        Ok(counts.into_iter().collect())
    }

    pub fn unused_tag_count(&self) -> Result<u64, String> {
        Ok(self.library
            .tags()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|tag| tag.assignment_count == 0)
            .count() as u64)
    }

    pub fn rename_or_merge_tag(
        &self,
        tag_id: picto_library::TagId,
        name: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        let target = self
            .library
            .projections()
            .snapshot()
            .tag_ids_by_name
            .get(name.trim())
            .copied();
        match target {
            Some(target) if target != tag_id => {
                self.library
                    .merge_tags(tag_id, target, chrono::Utc::now().timestamp_millis())
            }
            _ => self.library.rename_tag(tag_id, name),
        }
        .map_err(|error| error.to_string())
    }

    pub fn delete_tag(
        &self,
        tag_id: picto_library::TagId,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library
            .delete_tag(
                tag_id,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn delete_items(
        &self,
        target: &picto_library::selection::SelectionTarget,
    ) -> Result<picto_library::MutationReceipt, String> {
        let (receipt, _) = self
            .library
            .permanently_delete(target, chrono::Utc::now().timestamp_millis())
            .map_err(|error| error.to_string())?;
        Ok(receipt)
    }

    pub fn duplicate_candidates(
        &self,
        limit: i64,
    ) -> Result<Vec<picto_library::DuplicateCandidate>, String> {
        self.library
            .duplicate_candidates(limit.clamp(1, 500) as usize)
            .map_err(|error| error.to_string())
    }

    pub fn resolve_duplicate(
        &self,
        file_id_a: picto_library::FileId,
        file_id_b: picto_library::FileId,
        choice: picto_library::DuplicateResolutionChoice,
    ) -> Result<picto_library::DuplicateResolutionResult, String> {
        self.library
            .resolve_duplicate(
                file_id_a,
                file_id_b,
                choice,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| error.to_string())
    }

    pub fn sidebar_counts(&self) -> Result<picto_library::SidebarCounts, String> {
        self.library
            .sidebar_counts()
            .map_err(|error| error.to_string())
    }

    pub fn history_state(&self) -> LibraryHistoryState {
        map_history_state(self.library.history().state())
    }

    pub fn undo(&self) -> Result<LibraryHistoryOperationResult, String> {
        let entry = self
            .history_state()
            .undo
            .ok_or_else(|| "There is nothing to undo".to_string())?;
        let receipt = self
            .library
            .undo()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "There is nothing to undo".to_string())?;
        Ok(LibraryHistoryOperationResult {
            entry,
            state: self.history_state(),
            receipt,
        })
    }

    pub fn redo(&self) -> Result<LibraryHistoryOperationResult, String> {
        let entry = self
            .history_state()
            .redo
            .ok_or_else(|| "There is nothing to redo".to_string())?;
        let receipt = self
            .library
            .redo()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "There is nothing to redo".to_string())?;
        Ok(LibraryHistoryOperationResult {
            entry,
            state: self.history_state(),
            receipt,
        })
    }

    /// Emit at most one coalesced invalidation for the current render frame.
    pub fn flush_publications(&self) -> Option<LibraryChanged> {
        let event = self.library.publication().flush()?;
        let event = LibraryChanged {
            revision: event.revision,
            resources: event.resources,
            item_ids: event
                .item_ids
                .into_iter()
                .map(|root_id| crate::app::ItemId(i64::from(root_id.0)))
                .collect(),
        };
        crate::events::emit(LIBRARY_CHANGED_EVENT, &event);
        Some(event)
    }

    pub fn start_publication_worker(
        self: &Arc<Self>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let application = Arc::clone(self);
        tokio::spawn(async move {
            let mut frame = tokio::time::interval(std::time::Duration::from_millis(16));
            frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        application.flush_publications();
                        return;
                    }
                    _ = frame.tick() => {
                        application.flush_publications();
                    }
                }
            }
        })
    }
}

fn prepare_root(root: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("Failed to create library directory: {error}"))?;
    Ok(root.to_path_buf())
}

fn checked_smart_folder_id(value: i64) -> Result<picto_library::SmartFolderId, String> {
    checked_local_id(value, "smart folder").map(picto_library::SmartFolderId)
}

fn checked_local_id(value: i64, kind: &str) -> Result<u32, String> {
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{kind} ID {value} is outside the local ID domain"))
}

fn map_history_state(value: picto_library::history::HistoryState) -> LibraryHistoryState {
    LibraryHistoryState {
        undo: value.undo.map(map_history_entry),
        redo: value.redo.map(map_history_entry),
    }
}

fn map_history_entry(
    value: picto_library::history::HistoryEntrySummary,
) -> LibraryHistoryEntrySummary {
    LibraryHistoryEntrySummary {
        entry_id: value.entry_id,
        command: value.command,
        label: value.label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picto_library::{ImmutableMediaFacts, Lifecycle, PreparedImport};

    fn input() -> PreparedImport {
        PreparedImport {
            stable_key: "shell-root".into(),
            media_name: "Shell item".into(),
            file_path: "/tmp/shell.png".into(),
            facts: ImmutableMediaFacts {
                mime: "image/png".into(),
                size_bytes: 12,
                width: Some(10),
                height: Some(20),
                duration_ms: None,
                frame_count: Some(1),
                content_hash: "shell-hash".into(),
                perceptual_hash: None,
                palette: Vec::new(),
            },
            lifecycle: Lifecycle::Active,
            rating: picto_library::Rating::Unrated,
            tags: Vec::new(),
            folders: Vec::new(),
            source_urls: Vec::new(),
            imported_at_ms: 1_700_000_000_000,
            captured_at_ms: None,
            source_identity: None,
        }
    }

    #[test]
    fn shell_owns_one_greenfield_database_and_routes_root_reads() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path()).unwrap();
        let (root_id, _) = application.library().ingest(&input()).unwrap();
        let page = application
            .query(
                &picto_library::query::RootQuery {
                    scope: picto_library::query::ItemScope::All,
                    view: Default::default(),
                },
                picto_library::query::PageRequest {
                    cursor: None,
                    limit: 100,
                },
            )
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].root_id, root_id);
        assert_eq!(
            application.details(root_id).unwrap().root.root_id,
            root_id
        );
        assert_eq!(
            application.library().database().path(),
            directory.path().join(DATABASE_FILE)
        );
    }

    #[test]
    fn shell_coalesces_multiple_publications_into_one_renderer_event() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path()).unwrap();
        application.library().ingest(&input()).unwrap();
        let mut second = input();
        second.stable_key = "shell-root-2".into();
        second.facts.content_hash = "shell-hash-2".into();
        application.library().ingest(&second).unwrap();

        let event = application.flush_publications().unwrap();
        assert_eq!(event.item_ids.len(), 2);
        assert!(event.resources.contains(&"library".to_string()));
        assert!(application.flush_publications().is_none());
    }

    #[test]
    fn shell_exposes_memory_only_history_with_stable_command_identity() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path()).unwrap();
        let (root_id, _) = application.library().ingest(&input()).unwrap();
        application
            .library()
            .set_lifecycle(
                &picto_library::selection::SelectionTarget::Explicit {
                    root_ids: vec![root_id],
                },
                picto_library::Lifecycle::Trash,
            )
            .unwrap();

        let pending = application.history_state().undo.unwrap();
        assert_eq!(pending.command, "items.set_lifecycle");
        let undone = application.undo().unwrap();
        assert_eq!(undone.entry, pending);
        assert_eq!(undone.state.redo.as_ref().unwrap(), &pending);

        drop(application);
        let reopened = LibraryApplication::open(directory.path()).unwrap();
        assert_eq!(reopened.history_state().undo, None);
        assert_eq!(reopened.history_state().redo, None);
    }
}
