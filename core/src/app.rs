//! Replacement application boundary.
//!
//! Commands call one application operation. Operations own their SQLite
//! transaction, projection settlement, revision, and invalidation receipt.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blob_store::BlobStore;
use crate::projection_v2::ProjectionStore;
use crate::store::Store;

pub const LIBRARY_CHANGED_EVENT: &str = "library/changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct ItemId(#[ts(type = "number")] pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct MediaId(#[ts(type = "number")] pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct FileHash(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Media,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Inbox,
    Active,
    Trash,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Active => "active",
            Self::Trash => "trash",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ItemScope {
    All,
    Inbox,
    Trash,
    RecentlyViewed,
    Untagged,
    Uncategorized,
    Folder {
        #[ts(type = "number")]
        folder_id: i64,
    },
    SmartFolder {
        #[ts(type = "number")]
        smart_folder_id: i64,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemFilters {
    #[serde(default)]
    pub include_tags: Vec<String>,
    #[serde(default)]
    pub exclude_tags: Vec<String>,
    pub minimum_rating: Option<i64>,
    pub mime_prefix: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum ItemSortField {
    ImportedAt,
    CapturedAt,
    Name,
    Rating,
    Size,
    Random,
    FolderOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemSort {
    pub field: ItemSortField,
    pub direction: SortDirection,
    pub random_seed: Option<String>,
}

impl Default for ItemSort {
    fn default() -> Self {
        Self {
            field: ItemSortField::ImportedAt,
            direction: SortDirection::Descending,
            random_seed: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemQuery {
    pub scope: ItemScope,
    #[serde(default)]
    pub filters: ItemFilters,
    #[serde(default)]
    pub sort: ItemSort,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ItemTarget {
    Explicit {
        item_ids: Vec<ItemId>,
    },
    Query {
        query: ItemQuery,
        #[serde(default)]
        excluded_item_ids: Vec<ItemId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MutationReceipt {
    #[ts(type = "number")]
    pub revision: u64,
    pub resources: Vec<String>,
    pub item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct LibraryChanged {
    #[ts(type = "number")]
    pub revision: u64,
    pub resources: Vec<String>,
    pub item_ids: Vec<ItemId>,
}

impl From<&MutationReceipt> for LibraryChanged {
    fn from(receipt: &MutationReceipt) -> Self {
        Self {
            revision: receipt.revision,
            resources: receipt.resources.clone(),
            item_ids: receipt.item_ids.clone(),
        }
    }
}

pub struct Application {
    store: Arc<Store>,
    blobs: Arc<BlobStore>,
    projections: Arc<ProjectionStore>,
}

impl Application {
    pub fn new(store: Arc<Store>) -> Self {
        Self::try_new(store).expect("replacement projection initialization failed")
    }

    pub fn try_new(store: Arc<Store>) -> Result<Self, String> {
        let blobs = Arc::new(
            BlobStore::open(store.library_root())
                .map_err(|error| format!("Failed to open blob store: {error}"))?,
        );
        let projections = Arc::new(store.read_result(ProjectionStore::initialize)?);
        Ok(Self {
            store,
            blobs,
            projections,
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn projections(&self) -> &ProjectionStore {
        &self.projections
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    pub fn transaction<T, D>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64), String> {
        let projections = Arc::clone(&self.projections);
        self.store
            .transaction_settled(operation, move |connection, delta| {
                if settle(&projections, delta).is_err() {
                    projections.reload(connection)?;
                }
                Ok(())
            })
    }

    pub fn transaction_rebuilding<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<(T, u64), String> {
        let projections = Arc::clone(&self.projections);
        self.store.transaction_settled(
            |transaction| operation(transaction).map(|value| (value, ())),
            move |connection, ()| projections.reload(connection),
        )
    }

    pub fn transaction_if_changed<T, D>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, bool), String> {
        let projections = Arc::clone(&self.projections);
        self.store
            .transaction_if_changed_settled(operation, move |connection, delta| {
                if settle(&projections, delta).is_err() {
                    projections.reload(connection)?;
                }
                Ok(())
            })
    }

    pub fn transaction_if_changed_rebuilding<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, bool)>,
    ) -> Result<(T, u64, bool), String> {
        let projections = Arc::clone(&self.projections);
        self.store.transaction_if_changed_settled(
            |transaction| {
                let (value, changed) = operation(transaction)?;
                Ok((value, (), changed))
            },
            move |connection, ()| projections.reload(connection),
        )
    }

    pub fn publish(&self, receipt: &MutationReceipt) {
        crate::events::emit(LIBRARY_CHANGED_EVENT, &LibraryChanged::from(receipt));
    }
}

pub mod resources {
    pub const LIBRARY: &str = "library";
    pub const SIDEBAR: &str = "sidebar";
    pub const FOLDERS: &str = "folders";
    pub const SMART_FOLDERS: &str = "smart_folders";
    pub const TAGS: &str = "tags";
    pub const DUPLICATES: &str = "duplicates";
    pub const SUBSCRIPTIONS: &str = "subscriptions";
    pub const SETTINGS: &str = "settings";
    pub const TASKS: &str = "tasks";

    pub fn item(item_id: i64) -> String {
        format!("item:{item_id}")
    }
}
