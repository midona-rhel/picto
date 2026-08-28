//! Application-shell ownership for the greenfield media-library kernel.
//!
//! This type is the sole owner used by the cutover path. It deliberately does
//! not open the legacy `Store`; auxiliary services share the kernel's
//! `LibraryDatabase` scheduler through `library().database()`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use picto_library::query::PageRequest;
use picto_library::{Library, RootId};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::app::{ItemQuery, ItemTarget, LibraryChanged, LIBRARY_CHANGED_EVENT};
use crate::blob_store::BlobStore;
use crate::query_v2::{ItemDetails, ItemPage, ItemPageRequest, SelectionSummary};

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
    pub receipt: crate::app::MutationReceipt,
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

    pub fn query(&self, query: &ItemQuery, page: ItemPageRequest) -> Result<ItemPage, String> {
        let query = crate::library_v1::query(&self.library, query)?;
        let page = PageRequest {
            limit: usize::try_from(page.limit.clamp(1, 500))
                .expect("positive page limit fits usize"),
            cursor: page.cursor,
        };
        crate::library_v1::page(
            self.library
                .query(&query, &page)
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn details(&self, item_id: i64) -> Result<ItemDetails, String> {
        let root_id = checked_root_id(item_id)?;
        let details = self
            .library
            .details(root_id)
            .map_err(|error| error.to_string())?;
        crate::library_v1::details(&self.library, details)
    }

    pub fn selection_summary(&self, target: &ItemTarget) -> Result<SelectionSummary, String> {
        let target = crate::library_v1::target(&self.library, target)?;
        let summary = self
            .library
            .selection_summary(&target)
            .map_err(|error| error.to_string())?;
        crate::library_v1::selection_summary(&self.library, summary)
    }

    pub fn record_recent_view(&self, item_id: i64) -> Result<crate::app::MutationReceipt, String> {
        self.library
            .record_recent_view(
                checked_root_id(item_id)?,
                chrono::Utc::now().timestamp_millis(),
            )
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn clear_recent_views(&self) -> Result<crate::app::MutationReceipt, String> {
        self.library
            .clear_recent_views()
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn set_lifecycle(
        &self,
        target: &ItemTarget,
        lifecycle: crate::app::Lifecycle,
    ) -> Result<crate::app::MutationReceipt, String> {
        let target = crate::library_v1::target(&self.library, target)?;
        self.library
            .set_lifecycle(&target, crate::library_v1::lifecycle(lifecycle))
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn set_folder_membership(
        &self,
        target: &ItemTarget,
        folder_id: i64,
        present: bool,
    ) -> Result<crate::app::MutationReceipt, String> {
        let target = crate::library_v1::target(&self.library, target)?;
        let folder_id = picto_library::FolderId(checked_local_id(folder_id, "folder")?);
        let result = if present {
            self.library.add_to_folder(&target, folder_id)
        } else {
            self.library.remove_from_folder(&target, folder_id)
        };
        result
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn apply_tags(
        &self,
        target: &ItemTarget,
        tags: &[String],
        add: bool,
    ) -> Result<crate::app::MutationReceipt, String> {
        let target = crate::library_v1::target(&self.library, target)?;
        self.library
            .apply_tags(&target, tags, add)
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn rename_item(
        &self,
        item_id: i64,
        name: &str,
    ) -> Result<crate::app::MutationReceipt, String> {
        self.library
            .rename_root(
                checked_root_id(item_id)?,
                name,
                chrono::Utc::now().timestamp_millis(),
            )
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn patch_metadata(
        &self,
        target: &ItemTarget,
        patch: &crate::operations_v2::MediaMetadataPatch,
    ) -> Result<crate::app::MutationReceipt, String> {
        let target = crate::library_v1::target(&self.library, target)?;
        let rating = patch
            .rating
            .map(|value| value.map_or(Ok(picto_library::Rating::Unrated), checked_rating))
            .transpose()?;
        self.library
            .patch_metadata(
                &target,
                rating,
                patch.notes.clone(),
                patch.source_urls.clone(),
                chrono::Utc::now().timestamp_millis(),
            )
            .map(crate::library_v1::receipt)
            .map_err(|error| error.to_string())
    }

    pub fn sidebar_counts(&self) -> Result<crate::query_v2::SidebarCounts, String> {
        let counts = self.library.counts().map_err(|error| error.to_string())?;
        let recently_viewed = self
            .library
            .query(
                &picto_library::query::RootQuery {
                    scope: picto_library::query::ItemScope::RecentlyViewed,
                    view: picto_library::predicate::ViewQuerySpec::default(),
                },
                &PageRequest {
                    limit: 1,
                    cursor: None,
                },
            )
            .map_err(|error| error.to_string())?
            .total;
        let duplicates = self
            .library
            .database()
            .read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT COUNT(*) FROM duplicate_pair WHERE decision = 0",
                            [],
                            |row| row.get::<_, i64>(0),
                        )
                        .map_err(Into::into)
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(crate::query_v2::SidebarCounts {
            all: checked_count(counts.all)?,
            inbox: checked_count(counts.inbox)?,
            trash: checked_count(counts.trash)?,
            recently_viewed: checked_count(recently_viewed)?,
            untagged: checked_count(counts.untagged)?,
            uncategorized: checked_count(counts.uncategorized)?,
            duplicates,
            folders: counts
                .folders
                .into_iter()
                .map(|(folder_id, count)| {
                    Ok(crate::query_v2::ScopeCount {
                        id: i64::from(folder_id.0),
                        count: checked_count(count)?,
                    })
                })
                .collect::<Result<_, String>>()?,
            smart_folders: counts
                .smart_folders
                .into_iter()
                .map(|(smart_folder_id, count)| {
                    Ok(crate::query_v2::ScopeCount {
                        id: i64::from(smart_folder_id.0),
                        count: checked_count(count)?,
                    })
                })
                .collect::<Result<_, String>>()?,
            revision: counts.revision,
        })
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
            receipt: crate::library_v1::receipt(receipt),
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
            receipt: crate::library_v1::receipt(receipt),
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

fn checked_root_id(value: i64) -> Result<RootId, String> {
    checked_local_id(value, "root").map(RootId)
}

fn checked_local_id(value: i64, kind: &str) -> Result<u32, String> {
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{kind} ID {value} is outside the local ID domain"))
}

fn checked_count(value: u64) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| format!("count {value} exceeds the renderer integer domain"))
}

fn checked_rating(value: i64) -> Result<picto_library::Rating, String> {
    match value {
        1 => Ok(picto_library::Rating::One),
        2 => Ok(picto_library::Rating::Two),
        3 => Ok(picto_library::Rating::Three),
        4 => Ok(picto_library::Rating::Four),
        5 => Ok(picto_library::Rating::Five),
        _ => Err(format!("rating {value} is outside the supported range")),
    }
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
                &ItemQuery {
                    scope: crate::app::ItemScope::All,
                    filters: crate::app::ItemFilters::default(),
                    sort: crate::app::ItemSort::default(),
                },
                ItemPageRequest::new(None, 100),
            )
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].item_id.0, i64::from(root_id.0));
        assert_eq!(
            application.details(i64::from(root_id.0)).unwrap().item_id.0,
            i64::from(root_id.0)
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
