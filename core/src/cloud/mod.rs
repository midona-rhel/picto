//! Offline-first semantic replication for Picto libraries.
//!
//! SQLite Session changesets stay local to undo/redo. Cloud peers exchange
//! globally keyed operations whose conflicts are resolved here before normal
//! projection settlement and invalidation.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use roaring::RoaringBitmap;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::app::{resources, Application, ItemId, ItemKind, Lifecycle, MutationReceipt};
use crate::projection_v2::{
    timestamp_ms, FolderOrderProjectionChange, FolderProjectionChange, GroupOrderProjectionChange,
    ItemProjectionChange, MediaClassificationProjectionChange, MembershipProjectionChange,
    ProjectionStore, RootProjectionChange, RootSummaryProjectionChange, StructureProjectionDelta,
    TagProjectionChange,
};

pub mod blob;
pub mod capture;
pub mod epoch;
pub mod provider;
pub mod reconcile;
pub mod snapshot;
pub mod worker;

pub const CLOUD_SCHEMA_GENERATION: i64 = 2;
const APPLIED_MUTATION_DIAGNOSTIC_LIMIT: i64 = 10_000;
const MUTATION_RECEIPT_ITEM_LIMIT: usize = 256;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS,
)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct HybridTimestamp {
    #[ts(type = "number")]
    pub physical_ms: u64,
    pub logical: u32,
}

impl HybridTimestamp {
    fn now_after(previous: Self) -> Self {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if wall > previous.physical_ms {
            Self {
                physical_ms: wall,
                logical: 0,
            }
        } else {
            Self {
                physical_ms: previous.physical_ms,
                logical: previous.logical.saturating_add(1),
            }
        }
    }

    fn receive(local: Self, remote: Self) -> Self {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let physical_ms = wall.max(local.physical_ms).max(remote.physical_ms);
        let logical = if physical_ms == local.physical_ms && physical_ms == remote.physical_ms {
            local.logical.max(remote.logical).saturating_add(1)
        } else if physical_ms == local.physical_ms {
            local.logical.saturating_add(1)
        } else if physical_ms == remote.physical_ms {
            remote.logical.saturating_add(1)
        } else {
            0
        };
        Self {
            physical_ms,
            logical,
        }
    }
}

pub type CausalFrontier = BTreeMap<String, HybridTimestamp>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CloudOperation {
    Batch {
        operations: Vec<CloudOperation>,
    },
    UpsertItem {
        item: RestoredItem,
    },
    ItemFields {
        item_key: String,
        #[ts(type = "Record<string, unknown>")]
        fields: BTreeMap<String, Value>,
    },
    Lifecycle {
        item_key: String,
        lifecycle: String,
    },
    TagMembership {
        item_key: String,
        namespace: String,
        subtag: String,
        present: bool,
    },
    FolderMembership {
        item_key: String,
        folder_key: String,
        present: bool,
        position_rank: Option<i64>,
    },
    UpsertFolder {
        folder: CloudFolder,
        changed_fields: Vec<String>,
    },
    DeleteFolder {
        folder_key: String,
    },
    UpsertSmartFolder {
        smart_folder: CloudSmartFolder,
        changed_fields: Vec<String>,
    },
    DeleteSmartFolder {
        smart_folder_key: String,
    },
    UpsertSubscription {
        subscription: CloudSubscription,
        changed_fields: Vec<String>,
    },
    DeleteSubscription {
        subscription_key: String,
    },
    UpsertSubscriptionQuery {
        query: CloudSubscriptionQuery,
        changed_fields: Vec<String>,
    },
    DeleteSubscriptionQuery {
        query_key: String,
    },
    UpsertSourcePost {
        source_post: CloudSourcePost,
        changed_fields: Vec<String>,
    },
    UpsertSourceItem {
        source_item: CloudSourceItem,
        changed_fields: Vec<String>,
    },
    DeleteSourceItem {
        source_item_key: String,
    },
    RestoreSourceItem {
        tombstone_mutation_id: String,
        source_item: CloudSourceItem,
    },
    SubscriptionSourcePost {
        subscription_key: String,
        query_key: String,
        source_post_key: String,
        present: bool,
    },
    GroupAssignment {
        media_item_key: String,
        collection_item_key: Option<String>,
        position_rank: Option<i64>,
    },
    ReorderMember {
        collection_item_key: String,
        media_item_key: String,
        position_rank: i64,
    },
    DeleteItem {
        item_key: String,
    },
    RestoreItem {
        tombstone_mutation_id: String,
        item: RestoredItem,
    },
}

impl CloudOperation {
    fn name(&self) -> &'static str {
        match self {
            Self::Batch { .. } => "batch",
            Self::UpsertItem { .. } => "upsert_item",
            Self::ItemFields { .. } => "item_fields",
            Self::Lifecycle { .. } => "lifecycle",
            Self::TagMembership { .. } => "tag_membership",
            Self::FolderMembership { .. } => "folder_membership",
            Self::UpsertFolder { .. } => "upsert_folder",
            Self::DeleteFolder { .. } => "delete_folder",
            Self::UpsertSmartFolder { .. } => "upsert_smart_folder",
            Self::DeleteSmartFolder { .. } => "delete_smart_folder",
            Self::UpsertSubscription { .. } => "upsert_subscription",
            Self::DeleteSubscription { .. } => "delete_subscription",
            Self::UpsertSubscriptionQuery { .. } => "upsert_subscription_query",
            Self::DeleteSubscriptionQuery { .. } => "delete_subscription_query",
            Self::UpsertSourcePost { .. } => "upsert_source_post",
            Self::UpsertSourceItem { .. } => "upsert_source_item",
            Self::DeleteSourceItem { .. } => "delete_source_item",
            Self::RestoreSourceItem { .. } => "restore_source_item",
            Self::SubscriptionSourcePost { .. } => "subscription_source_post",
            Self::GroupAssignment { .. } => "group_assignment",
            Self::ReorderMember { .. } => "reorder_member",
            Self::DeleteItem { .. } => "delete_item",
            Self::RestoreItem { .. } => "restore_item",
        }
    }

    fn object_key(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Batch { .. } => None,
            Self::UpsertItem { item } => Some(("item", &item.item_key)),
            Self::ItemFields { item_key, .. }
            | Self::Lifecycle { item_key, .. }
            | Self::TagMembership { item_key, .. }
            | Self::FolderMembership { item_key, .. }
            | Self::DeleteItem { item_key } => Some(("item", item_key)),
            Self::UpsertFolder { folder, .. } => Some(("folder", &folder.folder_key)),
            Self::DeleteFolder { folder_key } => Some(("folder", folder_key)),
            Self::UpsertSmartFolder { smart_folder, .. } => {
                Some(("smart_folder", &smart_folder.smart_folder_key))
            }
            Self::DeleteSmartFolder { smart_folder_key } => {
                Some(("smart_folder", smart_folder_key))
            }
            Self::UpsertSubscription { subscription, .. } => {
                Some(("subscription", &subscription.subscription_key))
            }
            Self::DeleteSubscription { subscription_key } => {
                Some(("subscription", subscription_key))
            }
            Self::UpsertSubscriptionQuery { query, .. } => {
                Some(("subscription_query", &query.query_key))
            }
            Self::DeleteSubscriptionQuery { query_key } => Some(("subscription_query", query_key)),
            Self::UpsertSourcePost { source_post, .. } => {
                Some(("source_post", &source_post.source_post_key))
            }
            Self::UpsertSourceItem { source_item, .. } => {
                Some(("source_item", &source_item.source_item_key))
            }
            Self::DeleteSourceItem { source_item_key } => Some(("source_item", source_item_key)),
            Self::RestoreSourceItem { source_item, .. } => {
                Some(("source_item", &source_item.source_item_key))
            }
            Self::SubscriptionSourcePost { query_key, .. } => {
                Some(("subscription_query", query_key))
            }
            Self::GroupAssignment { media_item_key, .. } => Some(("item", media_item_key)),
            Self::ReorderMember { media_item_key, .. } => Some(("item", media_item_key)),
            Self::RestoreItem { item, .. } => Some(("item", &item.item_key)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudFolder {
    pub folder_key: String,
    pub name: String,
    pub parent_key: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    #[ts(type = "number | null")]
    pub sort_rank: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudSmartFolder {
    pub smart_folder_key: String,
    pub name: String,
    pub parent_key: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub predicate_json: String,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    #[ts(type = "number | null")]
    pub display_order: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudSubscription {
    pub subscription_key: String,
    pub name: String,
    pub schedule: String,
    pub paused: bool,
    #[ts(type = "number | null")]
    pub initial_post_limit: Option<i64>,
    #[ts(type = "number | null")]
    pub periodic_post_limit: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudSubscriptionQuery {
    pub query_key: String,
    pub subscription_key: String,
    pub site_id: String,
    pub domain_key: String,
    pub query_kind: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub group_posts: bool,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudSourcePost {
    pub source_post_key: String,
    pub site_id: String,
    pub post_key: String,
    pub canonical_url: Option<String>,
    pub creator_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub captured_at: Option<String>,
    pub metadata_json: Option<String>,
    pub root_item_key: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudSourceItem {
    pub source_item_key: String,
    pub source_post_key: String,
    pub item_key: String,
    #[ts(type = "number")]
    pub position: i64,
    pub media_url: Option<String>,
    pub canonical_url: Option<String>,
    pub media_item_key: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RestoredItem {
    pub item_key: String,
    pub kind: String,
    pub label: Option<String>,
    pub cover_media_item_key: Option<String>,
    pub lifecycle: String,
    pub media: Option<RestoredMedia>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RestoredMedia {
    pub file_hash: String,
    pub mime_type: String,
    #[ts(type = "number")]
    pub size_bytes: i64,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub name: Option<String>,
    pub notes: Option<String>,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
    pub source_urls_json: Option<String>,
    pub captured_at: Option<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudMutation {
    pub mutation_id: String,
    pub library_id: String,
    pub device_id: String,
    pub timestamp: HybridTimestamp,
    pub causal_frontier: CausalFrontier,
    pub operation: CloudOperation,
    #[ts(type = "number")]
    pub schema_generation: i64,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub struct CloudSyncStatus {
    pub state: String,
    pub phase: String,
    pub blocking: bool,
    #[ts(type = "number")]
    pub completed_units: i64,
    #[ts(type = "number | null")]
    pub total_units: Option<i64>,
    pub message: String,
    pub last_sync_at: Option<String>,
    #[ts(type = "number")]
    pub pending_mutations: i64,
    #[ts(type = "number")]
    pub pending_blobs: i64,
    #[ts(type = "number")]
    pub missing_blobs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ConfigureCloudInput {
    pub provider: String,
    pub account_label: String,
    pub root_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudLibraryOption {
    pub library_id: String,
    pub name: String,
    pub schema_generation: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudConfiguration {
    pub provider: Option<String>,
    pub account_label: Option<String>,
    pub root_path: Option<String>,
    pub library_id: String,
    pub device_id: String,
    #[ts(type = "Record<string, unknown>")]
    pub retention: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplySummary {
    pub applied: usize,
    pub duplicate: usize,
    pub ignored: usize,
    pub quarantined: usize,
    pub item_ids: Vec<ItemId>,
}

pub fn status(application: &Application) -> Result<CloudSyncStatus, String> {
    // Status is a single SQLite snapshot and has no projection dependency.
    // It must remain responsive while ingestion or reconciliation is settling.
    application.store().read_snapshot(|connection| {
        connection.query_row(
            "SELECT state, phase, blocking, completed_units, total_units, message, last_sync_at,
                    (SELECT COUNT(*) FROM cloud_outbox WHERE published_at IS NULL),
                    pending_blobs, missing_blobs
             FROM cloud_state WHERE singleton = 1",
            [],
            |row| {
                Ok(CloudSyncStatus {
                    state: row.get(0)?,
                    phase: row.get(1)?,
                    blocking: row.get::<_, i64>(2)? != 0,
                    completed_units: row.get(3)?,
                    total_units: row.get(4)?,
                    message: row.get(5)?,
                    last_sync_at: row.get(6)?,
                    pending_mutations: row.get(7)?,
                    pending_blobs: row.get(8)?,
                    missing_blobs: row.get(9)?,
                })
            },
        )
    })
}

pub fn configuration(application: &Application) -> Result<CloudConfiguration, String> {
    application.store().read_snapshot(|connection| {
        connection.query_row(
            "SELECT provider, account_label, remote_root, library_id, device_id, retention_json
             FROM cloud_state WHERE singleton = 1",
            [],
            |row| {
                let retention: String = row.get(5)?;
                Ok(CloudConfiguration {
                    provider: row.get(0)?,
                    account_label: row.get(1)?,
                    root_path: row.get(2)?,
                    library_id: row.get(3)?,
                    device_id: row.get(4)?,
                    retention: serde_json::from_str(&retention).map_err(json_sql_error)?,
                })
            },
        )
    })
}

pub fn configure(
    application: &Application,
    input: &ConfigureCloudInput,
) -> Result<MutationReceipt, String> {
    if !matches!(input.provider.as_str(), "google_drive" | "dropbox") {
        return Err(format!(
            "Unsupported cloud folder provider: {}",
            input.provider
        ));
    }
    let root = provider::canonical_provider_root(
        &input.provider,
        std::path::PathBuf::from(&input.root_path),
    );
    let provider = provider::DirectoryProvider::open_existing(&root)?;
    provider.verify_writable()?;
    let root_path = root.to_string_lossy().into_owned();
    let (_, revision) = application.transaction(
        |transaction| {
            transaction.execute(
                "UPDATE cloud_state SET provider = ?1, account_label = ?2, remote_root = ?3,
                        state = 'idle', phase = 'idle',
                        paused = 0, message = ''
                 WHERE singleton = 1",
                params![input.provider, input.account_label, root_path],
            )?;
            Ok(((), ()))
        },
        |_, ()| Ok(()),
    )?;
    blob::seed_local_originals(application)?;
    Ok(MutationReceipt {
        revision,
        resources: vec![resources::CLOUD.to_string(), resources::TASKS.to_string()],
        item_ids: Vec::new(),
    })
}

pub fn directory_provider(
    application: &Application,
) -> Result<provider::DirectoryProvider, String> {
    let configuration = configuration(application)?;
    let root = configuration
        .root_path
        .ok_or_else(|| "Cloud sync is not configured".to_string())?;
    let provider_name = configuration
        .provider
        .ok_or_else(|| "Cloud sync is not configured".to_string())?;
    provider::DirectoryProvider::open_provider_root(&provider_name, root)
}

pub async fn discover_libraries(root_path: &str) -> Result<Vec<CloudLibraryOption>, String> {
    let provider = provider::DirectoryProvider::open_existing(root_path)?;
    let mut libraries = Vec::new();
    for manifest in provider.library_manifests()? {
        let value: serde_json::Value = serde_json::from_slice(&provider.read_local(&manifest)?)
            .map_err(|error| format!("Invalid Picto cloud library manifest: {error}"))?;
        let Some(library_id) = value.get("library_id").and_then(|value| value.as_str()) else {
            continue;
        };
        libraries.push(CloudLibraryOption {
            library_id: library_id.to_string(),
            name: value
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Picto {}", &library_id[..library_id.len().min(8)])),
            schema_generation: value
                .get("schema_generation")
                .and_then(|value| value.as_i64())
                .unwrap_or_default(),
            created_at: value
                .get("created_at")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    }
    libraries.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(libraries)
}

pub fn update_retention(
    application: &Application,
    retention: &serde_json::Value,
) -> Result<MutationReceipt, String> {
    let retention_json = serde_json::to_string(retention).map_err(|error| error.to_string())?;
    let (_, revision) = application.transaction(
        |transaction| {
            transaction.execute(
                "UPDATE cloud_state SET retention_json = ?1 WHERE singleton = 1",
                [&retention_json],
            )?;
            Ok(((), ()))
        },
        |_, ()| Ok(()),
    )?;
    Ok(MutationReceipt {
        revision,
        resources: vec![resources::CLOUD.to_string()],
        item_ids: Vec::new(),
    })
}

pub fn set_paused(application: &Application, paused: bool) -> Result<MutationReceipt, String> {
    let (_, revision) = application.transaction(
        |transaction| {
            transaction.execute(
                "UPDATE cloud_state SET paused = ?1, state = CASE WHEN ?1 THEN 'paused' ELSE 'idle' END,
                        phase = CASE WHEN ?1 THEN phase ELSE 'idle' END
                 WHERE singleton = 1",
                [i64::from(paused)],
            )?;
            Ok(((), ()))
        },
        |_, ()| Ok(()),
    )?;
    Ok(MutationReceipt {
        revision,
        resources: vec![resources::CLOUD.to_string(), resources::TASKS.to_string()],
        item_ids: Vec::new(),
    })
}

pub fn record_local(
    transaction: &Transaction<'_>,
    operation: CloudOperation,
) -> rusqlite::Result<CloudMutation> {
    let (library_id, device_id, previous, schema_generation) = transaction.query_row(
        "SELECT library_id, device_id, hlc_physical_ms, hlc_logical, schema_generation
         FROM cloud_state WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                HybridTimestamp {
                    physical_ms: row.get::<_, i64>(2)? as u64,
                    logical: row.get::<_, i64>(3)? as u32,
                },
                row.get::<_, i64>(4)?,
            ))
        },
    )?;
    let timestamp = HybridTimestamp::now_after(previous);
    let causal_frontier = read_frontier(transaction)?;
    let mut mutation = CloudMutation {
        mutation_id: uuid::Uuid::new_v4().to_string(),
        library_id,
        device_id,
        timestamp,
        causal_frontier,
        operation,
        schema_generation,
        checksum: String::new(),
    };
    mutation.checksum = checksum(&mutation).map_err(json_sql_error)?;
    let payload_json = serde_json::to_string(&mutation.operation).map_err(json_sql_error)?;
    let frontier_json = serde_json::to_string(&mutation.causal_frontier).map_err(json_sql_error)?;
    let byte_size = payload_json.len() + frontier_json.len();
    transaction.execute(
        "INSERT INTO cloud_outbox
             (mutation_id, library_id, device_id, hlc_physical_ms, hlc_logical,
              causal_frontier_json, operation, payload_json, schema_generation, checksum,
              byte_size, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            mutation.mutation_id,
            mutation.library_id,
            mutation.device_id,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            frontier_json,
            mutation.operation.name(),
            payload_json,
            mutation.schema_generation,
            mutation.checksum,
            byte_size as i64,
            Utc::now().to_rfc3339(),
        ],
    )?;
    stamp_local_operation(transaction, &mutation.operation, &mutation)?;
    transaction.execute(
        "UPDATE cloud_state SET hlc_physical_ms = ?1, hlc_logical = ?2 WHERE singleton = 1",
        params![timestamp.physical_ms as i64, timestamp.logical as i64],
    )?;
    advance_frontier(transaction, &mutation.device_id, timestamp)?;
    Ok(mutation)
}

fn stamp_local_operation(
    transaction: &Transaction<'_>,
    operation: &CloudOperation,
    mutation: &CloudMutation,
) -> rusqlite::Result<()> {
    let mut plan = LocalStampPlan::default();
    collect_local_stamps(transaction, operation, &mut plan)?;
    apply_local_stamp_plan(transaction, plan, mutation)
}

#[derive(Default)]
struct LocalStampPlan {
    field_clocks: BTreeSet<(String, String, String)>,
    membership_clocks: BTreeMap<(String, String, String), bool>,
    membership_removals: BTreeSet<(String, String, String)>,
    tombstones: BTreeMap<(String, String), bool>,
    available_blobs: BTreeSet<String>,
}

impl LocalStampPlan {
    fn field(&mut self, kind: &str, key: &str, field: &str) {
        self.field_clocks
            .insert((kind.to_owned(), key.to_owned(), field.to_owned()));
    }

    fn membership(&mut self, relation: &str, owner: &str, member: &str, present: bool) {
        let key = (relation.to_owned(), owner.to_owned(), member.to_owned());
        if !present {
            self.membership_removals.insert(key.clone());
        }
        self.membership_clocks.insert(key, present);
    }

    fn has_field(&self, kind: &str, key: &str, field: &str) -> bool {
        self.field_clocks
            .contains(&(kind.to_owned(), key.to_owned(), field.to_owned()))
    }

    fn tombstone(&mut self, kind: &str, key: &str, present: bool) {
        self.tombstones
            .insert((kind.to_owned(), key.to_owned()), present);
        self.field(kind, key, "tombstone");
    }
}

fn collect_local_stamps(
    transaction: &Transaction<'_>,
    operation: &CloudOperation,
    plan: &mut LocalStampPlan,
) -> rusqlite::Result<()> {
    match operation {
        CloudOperation::Batch { operations } => {
            for operation in operations {
                collect_local_stamps(transaction, operation, plan)?;
            }
        }
        CloudOperation::UpsertItem { item } => {
            plan.field("item", &item.item_key, "exists");
            if let Some(media) = &item.media {
                plan.available_blobs.insert(media.file_hash.clone());
            }
        }
        CloudOperation::ItemFields { item_key, fields } => {
            for field in fields.keys() {
                plan.field("item", item_key, field);
            }
        }
        CloudOperation::Lifecycle { item_key, .. } => {
            plan.field("item", item_key, "lifecycle");
        }
        CloudOperation::TagMembership {
            item_key,
            namespace,
            subtag,
            present,
        } => {
            plan.membership(
                "tag",
                item_key,
                &format!("{namespace}\u{0}{subtag}"),
                *present,
            );
        }
        CloudOperation::FolderMembership {
            item_key,
            folder_key,
            present,
            ..
        } => {
            plan.membership("folder", folder_key, item_key, *present);
        }
        CloudOperation::UpsertFolder {
            folder,
            changed_fields,
        } => {
            for field in changed_fields {
                plan.field("folder", &folder.folder_key, field);
            }
        }
        CloudOperation::DeleteFolder { folder_key } => {
            plan.tombstone("folder", folder_key, true);
        }
        CloudOperation::UpsertSmartFolder {
            smart_folder,
            changed_fields,
        } => {
            for field in changed_fields {
                plan.field("smart_folder", &smart_folder.smart_folder_key, field);
            }
        }
        CloudOperation::DeleteSmartFolder { smart_folder_key } => {
            plan.tombstone("smart_folder", smart_folder_key, true);
        }
        CloudOperation::UpsertSubscription {
            subscription,
            changed_fields,
        } => {
            for field in changed_fields {
                plan.field("subscription", &subscription.subscription_key, field);
            }
        }
        CloudOperation::DeleteSubscription { subscription_key } => {
            plan.tombstone("subscription", subscription_key, true);
        }
        CloudOperation::UpsertSubscriptionQuery {
            query,
            changed_fields,
        } => {
            for field in changed_fields {
                plan.field("subscription_query", &query.query_key, field);
            }
        }
        CloudOperation::DeleteSubscriptionQuery { query_key } => {
            plan.tombstone("subscription_query", query_key, true);
        }
        CloudOperation::UpsertSourcePost {
            source_post,
            changed_fields,
        } => {
            if plan.has_field("source_post", &source_post.source_post_key, "exists")
                || field_clock_exists(
                    transaction,
                    "source_post",
                    &source_post.source_post_key,
                    "exists",
                )?
            {
                for field in changed_fields
                    .iter()
                    .filter(|field| field.as_str() != "exists")
                {
                    plan.field("source_post", &source_post.source_post_key, field);
                }
            } else {
                plan.field("source_post", &source_post.source_post_key, "exists");
            }
        }
        CloudOperation::UpsertSourceItem {
            source_item,
            changed_fields,
        } => {
            if plan.has_field("source_item", &source_item.source_item_key, "exists")
                || field_clock_exists(
                    transaction,
                    "source_item",
                    &source_item.source_item_key,
                    "exists",
                )?
            {
                for field in changed_fields
                    .iter()
                    .filter(|field| field.as_str() != "exists")
                {
                    plan.field("source_item", &source_item.source_item_key, field);
                }
            } else {
                plan.field("source_item", &source_item.source_item_key, "exists");
            }
        }
        CloudOperation::DeleteSourceItem { source_item_key } => {
            plan.tombstone("source_item", source_item_key, true);
        }
        CloudOperation::RestoreSourceItem { source_item, .. } => {
            plan.tombstone("source_item", &source_item.source_item_key, false);
            for field in [
                "exists",
                "position",
                "media_url",
                "canonical_url",
                "media_item",
                "tombstone",
            ] {
                plan.field("source_item", &source_item.source_item_key, field);
            }
        }
        CloudOperation::SubscriptionSourcePost {
            query_key,
            source_post_key,
            present,
            ..
        } => {
            plan.membership(
                "subscription_source_post",
                query_key,
                source_post_key,
                *present,
            );
        }
        CloudOperation::GroupAssignment { media_item_key, .. } => {
            plan.field("item", media_item_key, "collection");
        }
        CloudOperation::ReorderMember { media_item_key, .. } => {
            plan.field("item", media_item_key, "position_rank");
        }
        CloudOperation::DeleteItem { item_key } => {
            plan.tombstone("item", item_key, true);
        }
        CloudOperation::RestoreItem { item, .. } => {
            plan.tombstone("item", &item.item_key, false);
        }
    }
    Ok(())
}

fn apply_local_stamp_plan(
    transaction: &Transaction<'_>,
    plan: LocalStampPlan,
    mutation: &CloudMutation,
) -> rusqlite::Result<()> {
    let field_clocks = serde_json::to_string(&plan.field_clocks).map_err(json_sql_error)?;
    transaction.execute(
        "INSERT INTO cloud_field_clock
             (object_kind, object_key, field_name, hlc_physical_ms, hlc_logical,
              device_id, mutation_id)
         SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]'),
                json_extract(value, '$[2]'), ?2, ?3, ?4, ?5
         FROM json_each(?1)
         WHERE 1
         ON CONFLICT(object_kind, object_key, field_name) DO UPDATE SET
             hlc_physical_ms=excluded.hlc_physical_ms,
             hlc_logical=excluded.hlc_logical,
             device_id=excluded.device_id,
             mutation_id=excluded.mutation_id",
        params![
            field_clocks,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.device_id,
            mutation.mutation_id,
        ],
    )?;

    let memberships = serde_json::to_string(
        &plan
            .membership_clocks
            .into_iter()
            .map(|((relation, owner, member), present)| {
                let retain_history = plan.membership_removals.contains(&(
                    relation.clone(),
                    owner.clone(),
                    member.clone(),
                ));
                (relation, owner, member, present, retain_history)
            })
            .collect::<Vec<_>>(),
    )
    .map_err(json_sql_error)?;
    let frontier = serde_json::to_string(&mutation.causal_frontier).map_err(json_sql_error)?;
    transaction.execute(
        "WITH incoming AS (
             SELECT json_extract(value, '$[0]') AS relation_kind,
                    json_extract(value, '$[1]') AS owner_key,
                    json_extract(value, '$[2]') AS member_key,
                    CAST(json_extract(value, '$[3]') AS INTEGER) AS present,
                    CAST(json_extract(value, '$[4]') AS INTEGER) AS retain_history
             FROM json_each(?1)
         )
         INSERT INTO cloud_membership_clock
             (relation_kind, owner_key, member_key, present, hlc_physical_ms, hlc_logical,
              device_id, mutation_id, causal_frontier_json)
         SELECT incoming.relation_kind, incoming.owner_key, incoming.member_key,
                incoming.present, ?2, ?3, ?4, ?5, ?6
         FROM incoming
         WHERE incoming.retain_history = 1 OR EXISTS (
             SELECT 1 FROM cloud_membership_clock current
             WHERE current.relation_kind = incoming.relation_kind
               AND current.owner_key = incoming.owner_key
               AND current.member_key = incoming.member_key
         )
         ON CONFLICT(relation_kind, owner_key, member_key) DO UPDATE SET
             present=excluded.present,
             hlc_physical_ms=excluded.hlc_physical_ms,
             hlc_logical=excluded.hlc_logical,
             device_id=excluded.device_id,
             mutation_id=excluded.mutation_id,
             causal_frontier_json=excluded.causal_frontier_json",
        params![
            memberships,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.device_id,
            mutation.mutation_id,
            frontier,
        ],
    )?;

    let (deleted_tombstones, restored_tombstones): (Vec<_>, Vec<_>) = plan
        .tombstones
        .into_iter()
        .partition(|(_, present)| *present);
    let deleted_tombstones = serde_json::to_string(
        &deleted_tombstones
            .into_iter()
            .map(|((kind, key), _)| (kind, key))
            .collect::<Vec<_>>(),
    )
    .map_err(json_sql_error)?;
    let now = Utc::now();
    transaction.execute(
        "INSERT INTO cloud_tombstone
             (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
              device_id, causal_frontier_json, deleted_at, purge_after)
         SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]'),
                ?2, ?3, ?4, ?5, ?6, ?7, ?8
         FROM json_each(?1)
         WHERE 1
         ON CONFLICT(object_kind, object_key) DO UPDATE SET
             mutation_id=excluded.mutation_id,
             hlc_physical_ms=excluded.hlc_physical_ms,
             hlc_logical=excluded.hlc_logical,
             device_id=excluded.device_id,
             causal_frontier_json=excluded.causal_frontier_json,
             deleted_at=excluded.deleted_at,
             purge_after=excluded.purge_after",
        params![
            deleted_tombstones,
            mutation.mutation_id,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.device_id,
            serde_json::to_string(&mutation.causal_frontier).map_err(json_sql_error)?,
            now.to_rfc3339(),
            (now + chrono::Duration::days(30)).to_rfc3339(),
        ],
    )?;
    let restored_tombstones = serde_json::to_string(
        &restored_tombstones
            .into_iter()
            .map(|((kind, key), _)| (kind, key))
            .collect::<Vec<_>>(),
    )
    .map_err(json_sql_error)?;
    transaction.execute(
        "DELETE FROM cloud_tombstone
         WHERE (object_kind, object_key) IN (
             SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]')
             FROM json_each(?1)
         )",
        [restored_tombstones],
    )?;

    let blobs = serde_json::to_string(&plan.available_blobs).map_err(json_sql_error)?;
    transaction.execute(
        "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
         SELECT value, 'available', ?2 FROM json_each(?1)
         WHERE 1
         ON CONFLICT(file_hash) DO UPDATE SET
             state='available', last_error=NULL, updated_at=excluded.updated_at",
        params![blobs, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

#[derive(Default)]
struct CloudProjectionFootprint {
    item_keys: BTreeSet<String>,
    folder_keys: BTreeSet<String>,
}

impl CloudProjectionFootprint {
    fn collect(operation: &CloudOperation, footprint: &mut Self) {
        match operation {
            CloudOperation::Batch { operations } => {
                for operation in operations {
                    Self::collect(operation, footprint);
                }
            }
            CloudOperation::UpsertItem { item } | CloudOperation::RestoreItem { item, .. } => {
                footprint.item_keys.insert(item.item_key.clone());
                if let Some(cover_key) = &item.cover_media_item_key {
                    footprint.item_keys.insert(cover_key.clone());
                }
            }
            CloudOperation::ItemFields { item_key, .. }
            | CloudOperation::Lifecycle { item_key, .. }
            | CloudOperation::TagMembership { item_key, .. }
            | CloudOperation::DeleteItem { item_key } => {
                footprint.item_keys.insert(item_key.clone());
            }
            CloudOperation::FolderMembership {
                item_key,
                folder_key,
                ..
            } => {
                footprint.item_keys.insert(item_key.clone());
                footprint.folder_keys.insert(folder_key.clone());
            }
            CloudOperation::UpsertFolder { folder, .. } => {
                footprint.folder_keys.insert(folder.folder_key.clone());
            }
            CloudOperation::DeleteFolder { folder_key } => {
                footprint.folder_keys.insert(folder_key.clone());
            }
            CloudOperation::GroupAssignment {
                media_item_key,
                collection_item_key,
                ..
            } => {
                footprint.item_keys.insert(media_item_key.clone());
                if let Some(collection_item_key) = collection_item_key {
                    footprint.item_keys.insert(collection_item_key.clone());
                }
            }
            CloudOperation::ReorderMember {
                collection_item_key,
                media_item_key,
                ..
            } => {
                footprint.item_keys.insert(collection_item_key.clone());
                footprint.item_keys.insert(media_item_key.clone());
            }
            CloudOperation::UpsertSmartFolder { .. }
            | CloudOperation::DeleteSmartFolder { .. }
            | CloudOperation::UpsertSubscription { .. }
            | CloudOperation::DeleteSubscription { .. }
            | CloudOperation::UpsertSubscriptionQuery { .. }
            | CloudOperation::DeleteSubscriptionQuery { .. }
            | CloudOperation::UpsertSourcePost { .. }
            | CloudOperation::UpsertSourceItem { .. }
            | CloudOperation::DeleteSourceItem { .. }
            | CloudOperation::RestoreSourceItem { .. }
            | CloudOperation::SubscriptionSourcePost { .. } => {}
        }
    }

    fn from_mutations(mutations: &[CloudMutation]) -> Self {
        let mut footprint = Self::default();
        for mutation in mutations {
            Self::collect(&mutation.operation, &mut footprint);
        }
        footprint
    }
}

#[derive(Default)]
struct CloudProjectionState {
    ids: BTreeSet<i64>,
    items: BTreeMap<i64, (ItemKind, Option<Lifecycle>)>,
    folders: BTreeSet<(i64, i64)>,
    tags: BTreeSet<(i64, i64)>,
    memberships: BTreeSet<(i64, i64)>,
    group_orders: BTreeMap<i64, Vec<u32>>,
    folder_orders: BTreeMap<i64, Vec<u32>>,
    summaries: BTreeMap<i64, RootSummaryProjectionChange>,
    image_media_ids: BTreeSet<i64>,
    mime_types: BTreeMap<i64, String>,
}

impl CloudProjectionState {
    fn capture(
        transaction: &Transaction<'_>,
        footprint: &CloudProjectionFootprint,
        known_ids: &BTreeSet<i64>,
    ) -> rusqlite::Result<Self> {
        let mut state = Self {
            ids: known_ids.clone(),
            ..Self::default()
        };

        let item_keys = serde_json::to_string(&footprint.item_keys).map_err(json_sql_error)?;
        let mut statement = transaction.prepare(
            "SELECT li.item_id
             FROM library_item li
             WHERE li.item_key IN (SELECT value FROM json_each(?1))",
        )?;
        let item_ids = statement
            .query_map([item_keys], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        state.ids.extend(item_ids);

        let folder_keys = serde_json::to_string(&footprint.folder_keys).map_err(json_sql_error)?;
        let mut statement = transaction.prepare(
            "SELECT f.folder_id
             FROM folder f
             WHERE f.folder_key IN (SELECT value FROM json_each(?1))",
        )?;
        let folder_ids = statement
            .query_map([folder_keys], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for folder_id in folder_ids {
            let members = crate::canonical_bitmap::load_bitmap(
                transaction,
                crate::canonical_bitmap::BitmapDomain::Folder,
                folder_id,
            )?;
            state.ids.extend(members.iter().map(i64::from));
        }

        if state.ids.is_empty() {
            return Ok(state);
        }

        for (collection_id, members) in crate::canonical_bitmap::load_domain(
            transaction,
            crate::canonical_bitmap::BitmapDomain::GroupMember,
        )? {
            let touches_scope = state.ids.contains(&collection_id)
                || members
                    .iter()
                    .any(|media| state.ids.contains(&i64::from(media)));
            if !touches_scope {
                continue;
            }
            state.ids.insert(collection_id);
            for media in members.iter().map(i64::from) {
                state.ids.insert(media);
                state.memberships.insert((collection_id, media));
            }
            if let Some(order) =
                crate::canonical_bitmap::load_order(transaction, "group", collection_id)?
            {
                state.group_orders.insert(collection_id, order);
            }
        }

        let ids = serde_json::to_string(&state.ids).map_err(json_sql_error)?;
        let mut statement = transaction.prepare(
            "SELECT li.item_id, li.kind, lr.lifecycle
             FROM library_item li
             LEFT JOIN library_root lr ON lr.item_id = li.item_id
             WHERE li.item_id IN (SELECT value FROM json_each(?1))",
        )?;
        let items = statement
            .query_map([ids.clone()], |row| {
                let kind = parse_projection_kind(&row.get::<_, String>(1)?)?;
                let lifecycle = row
                    .get::<_, Option<String>>(2)?
                    .map(|value| parse_projection_lifecycle(&value))
                    .transpose()?;
                Ok((row.get::<_, i64>(0)?, kind, lifecycle))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (item_id, kind, lifecycle) in items {
            state.items.insert(item_id, (kind, lifecycle));
        }

        let mut statement = transaction.prepare(
            "SELECT asset.item_id, file.mime_type
             FROM media_asset asset
             JOIN media_file file ON file.file_id = asset.file_id
             WHERE asset.item_id IN (SELECT value FROM json_each(?1))",
        )?;
        for (media_id, mime_type) in statement
            .query_map([ids.clone()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        {
            if mime_type.starts_with("image/") {
                state.image_media_ids.insert(media_id);
            }
            state.mime_types.insert(media_id, mime_type);
        }

        for (folder_id, members) in crate::canonical_bitmap::load_domain(
            transaction,
            crate::canonical_bitmap::BitmapDomain::Folder,
        )? {
            let mut touches_scope = false;
            for media in members.iter().map(i64::from) {
                if state.ids.contains(&media) {
                    state.folders.insert((folder_id, media));
                    touches_scope = true;
                }
            }
            if touches_scope {
                if let Some(order) =
                    crate::canonical_bitmap::load_order(transaction, "folder", folder_id)?
                {
                    state.folder_orders.insert(folder_id, order);
                }
            }
        }

        for (tag_id, members) in crate::canonical_bitmap::load_domain(
            transaction,
            crate::canonical_bitmap::BitmapDomain::Tag,
        )? {
            for root in members.iter().map(i64::from) {
                if state.ids.contains(&root) {
                    state.tags.insert((root, tag_id));
                }
            }
        }

        let mut statement = transaction.prepare(
            "SELECT summary.root_item_id, summary.total_size_bytes,
                    summary.media_count, summary.sort_rating,
                    file.duration_ms, file.pixel_width, file.pixel_height,
                    summary.imported_at, summary.updated_at
             FROM root_summary summary
             LEFT JOIN media_asset cover
               ON cover.item_id = summary.cover_media_item_id
             LEFT JOIN media_file file ON file.file_id = cover.file_id
             WHERE summary.root_item_id IN (SELECT value FROM json_each(?1))",
        )?;
        let summaries = statement
            .query_map([ids], |row| {
                Ok(RootSummaryProjectionChange {
                    item_id: row.get(0)?,
                    total_size_bytes: u64::try_from(row.get::<_, i64>(1)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    media_count: u64::try_from(row.get::<_, i64>(2)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    rating: row
                        .get::<_, Option<i64>>(3)?
                        .map(u8::try_from)
                        .transpose()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    display_duration_ms: cloud_optional_u64(row.get(4)?)?,
                    display_width: cloud_optional_u64(row.get(5)?)?,
                    display_height: cloud_optional_u64(row.get(6)?)?,
                    imported_at_ms: cloud_timestamp_ms(row.get(7)?),
                    modified_at_ms: cloud_timestamp_ms(row.get(8)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for summary in summaries {
            state.summaries.insert(summary.item_id, summary);
        }
        Ok(state)
    }
}

fn cloud_optional_u64(value: Option<i64>) -> rusqlite::Result<Option<u64>> {
    value
        .map(|value| u64::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}

fn cloud_timestamp_ms(value: Option<String>) -> Option<i64> {
    value.as_deref().and_then(timestamp_ms)
}

#[derive(Default)]
struct CloudProjectionDelta {
    structure: StructureProjectionDelta,
    summaries: Vec<RootSummaryProjectionChange>,
    removed_summaries: RoaringBitmap,
    item_ids: Vec<i64>,
}

impl CloudProjectionDelta {
    fn between(before: CloudProjectionState, after: CloudProjectionState) -> Self {
        let item_ids = before.ids.union(&after.ids).copied().collect::<Vec<_>>();
        let final_roots = after
            .items
            .iter()
            .filter_map(|(item_id, (_, lifecycle))| lifecycle.map(|_| *item_id))
            .collect::<BTreeSet<_>>();
        let mut structure = StructureProjectionDelta::default();
        for item_id in &item_ids {
            match after.items.get(item_id) {
                Some((kind, lifecycle)) => {
                    structure.items.push(ItemProjectionChange {
                        item_id: *item_id,
                        kind: *kind,
                        present: true,
                    });
                    if matches!(kind, ItemKind::Media) {
                        structure
                            .media_classifications
                            .push(MediaClassificationProjectionChange {
                                media_id: *item_id,
                                is_image: after.image_media_ids.contains(item_id),
                                mime_type: after
                                    .mime_types
                                    .get(item_id)
                                    .cloned()
                                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                            });
                    }
                    structure.roots.push(RootProjectionChange {
                        item_id: *item_id,
                        lifecycle: *lifecycle,
                    });
                }
                None => {
                    structure.items.push(ItemProjectionChange {
                        item_id: *item_id,
                        kind: before
                            .items
                            .get(item_id)
                            .map(|(kind, _)| *kind)
                            .unwrap_or(ItemKind::Media),
                        present: false,
                    });
                    structure.roots.push(RootProjectionChange {
                        item_id: *item_id,
                        lifecycle: None,
                    });
                }
            }
        }
        structure
            .memberships
            .extend(before.memberships.difference(&after.memberships).map(
                |(collection_id, media_id)| MembershipProjectionChange {
                    collection_id: *collection_id,
                    media_id: *media_id,
                    present: false,
                },
            ));
        structure
            .memberships
            .extend(after.memberships.difference(&before.memberships).map(
                |(collection_id, media_id)| MembershipProjectionChange {
                    collection_id: *collection_id,
                    media_id: *media_id,
                    present: true,
                },
            ));
        structure
            .folders
            .extend(
                before
                    .folders
                    .difference(&after.folders)
                    .map(|(folder_id, item_id)| FolderProjectionChange {
                        folder_id: *folder_id,
                        item_id: *item_id,
                        present: false,
                    }),
            );
        structure
            .folders
            .extend(
                after
                    .folders
                    .difference(&before.folders)
                    .map(|(folder_id, item_id)| FolderProjectionChange {
                        folder_id: *folder_id,
                        item_id: *item_id,
                        present: true,
                    }),
            );
        structure.tags.extend(
            before
                .tags
                .difference(&after.tags)
                .filter(|(item_id, _)| final_roots.contains(item_id))
                .map(|(item_id, tag_id)| TagProjectionChange {
                    media_id: *item_id,
                    tag_id: *tag_id,
                    present: false,
                }),
        );
        structure.tags.extend(
            after
                .tags
                .difference(&before.tags)
                .filter(|(item_id, _)| final_roots.contains(item_id))
                .map(|(item_id, tag_id)| TagProjectionChange {
                    media_id: *item_id,
                    tag_id: *tag_id,
                    present: true,
                }),
        );
        for (collection_id, order) in &after.group_orders {
            if before.group_orders.get(collection_id) != Some(order) {
                structure.group_orders.push(GroupOrderProjectionChange {
                    collection_id: *collection_id,
                    media_ids: order.iter().map(|media| i64::from(*media)).collect(),
                });
            }
        }
        for (folder_id, order) in &after.folder_orders {
            if before.folder_orders.get(folder_id) != Some(order) {
                structure.folder_orders.push(FolderOrderProjectionChange {
                    folder_id: *folder_id,
                    item_ids: order.iter().map(|item| i64::from(*item)).collect(),
                });
            }
        }

        let summaries = after.summaries.into_values().collect::<Vec<_>>();
        let summary_ids = summaries
            .iter()
            .map(|summary| summary.item_id)
            .collect::<BTreeSet<_>>();
        let removed_summaries = RoaringBitmap::from_iter(
            item_ids
                .iter()
                .filter(|item_id| !summary_ids.contains(item_id))
                .filter_map(|item_id| u32::try_from(*item_id).ok()),
        );
        Self {
            structure,
            summaries,
            removed_summaries,
            item_ids,
        }
    }

    fn settle(self, projections: &ProjectionStore) -> Result<(), String> {
        projections.apply_structure_delta(self.structure)?;
        projections.apply_root_summary_changes(&self.summaries, &self.removed_summaries)
    }
}

fn parse_projection_kind(value: &str) -> rusqlite::Result<ItemKind> {
    match value {
        "media" => Ok(ItemKind::Media),
        "collection" => Ok(ItemKind::Collection),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_projection_lifecycle(value: &str) -> rusqlite::Result<Lifecycle> {
    match value {
        "inbox" => Ok(Lifecycle::Inbox),
        "active" => Ok(Lifecycle::Active),
        "trash" => Ok(Lifecycle::Trash),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

pub fn apply_downloaded(
    application: &Application,
    mutations: &[CloudMutation],
) -> Result<(ApplySummary, MutationReceipt), String> {
    let footprint = CloudProjectionFootprint::from_mutations(mutations);
    let (summary, revision, changed) = application
        .store()
        .transaction_if_changed_settled_without_cloud_maintenance(
            |transaction| {
                // Applied remote changes must not be captured again as local
                // membership operations at projection persistence.
                transaction.execute(
                    "UPDATE projection_write_control
                     SET suppress_membership_capture = 1 WHERE singleton = 1",
                    [],
                )?;
                let before =
                    CloudProjectionState::capture(transaction, &footprint, &BTreeSet::new())?;
                let mut summary = ApplySummary::default();
                for mutation in mutations {
                    match apply_one(transaction, mutation)? {
                        ApplyOutcome::Applied(_) => summary.applied += 1,
                        ApplyOutcome::AppliedAndQuarantined(_) => {
                            summary.applied += 1;
                            summary.quarantined += 1;
                        }
                        ApplyOutcome::Duplicate => summary.duplicate += 1,
                        ApplyOutcome::Ignored => summary.ignored += 1,
                        ApplyOutcome::Quarantined => summary.quarantined += 1,
                    }
                }
                prune_applied_mutations(transaction)?;
                let changed = summary.applied > 0 || summary.quarantined > 0;
                if !changed {
                    // No projection settlement will consume the one-shot
                    // suppression, so clear it before this transaction commits.
                    transaction.execute(
                        "UPDATE projection_write_control
                         SET suppress_membership_capture = 0 WHERE singleton = 1",
                        [],
                    )?;
                }
                let delta = if summary.applied > 0 {
                    let after =
                        CloudProjectionState::capture(transaction, &footprint, &before.ids)?;
                    CloudProjectionDelta::between(before, after)
                } else {
                    CloudProjectionDelta::default()
                };
                if delta.item_ids.len() <= MUTATION_RECEIPT_ITEM_LIMIT {
                    summary.item_ids = delta.item_ids.iter().copied().map(ItemId).collect();
                }
                Ok((summary, delta, changed))
            },
            |delta| {
                application
                    .projections()
                    .prepare(|candidate| delta.settle(candidate))
            },
            |prepared| application.projections().publish_prepared(prepared),
        )?;
    let receipt = MutationReceipt {
        revision,
        resources: if changed {
            vec![
                resources::CLOUD.to_string(),
                resources::TASKS.to_string(),
                resources::LIBRARY.to_string(),
                resources::SIDEBAR.to_string(),
                resources::FOLDERS.to_string(),
                resources::SMART_FOLDERS.to_string(),
                resources::TAGS.to_string(),
                resources::SUBSCRIPTIONS.to_string(),
            ]
        } else {
            vec![resources::CLOUD.to_string()]
        },
        item_ids: summary.item_ids.clone(),
    };
    Ok((summary, receipt))
}

fn prune_applied_mutations(transaction: &Transaction<'_>) -> rusqlite::Result<usize> {
    transaction.execute(
        "DELETE FROM cloud_applied_mutation
         WHERE mutation_id IN (
             SELECT mutation_id FROM cloud_applied_mutation
             ORDER BY hlc_physical_ms DESC, hlc_logical DESC, applied_at DESC
             LIMIT -1 OFFSET ?1
         )",
        [APPLIED_MUTATION_DIAGNOSTIC_LIMIT],
    )
}

enum ApplyOutcome {
    Applied(Option<i64>),
    AppliedAndQuarantined(Option<i64>),
    Duplicate,
    Ignored,
    Quarantined,
}

fn apply_one(
    transaction: &Transaction<'_>,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let local_library: String = transaction.query_row(
        "SELECT library_id FROM cloud_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    if mutation.library_id != local_library
        || mutation.schema_generation != CLOUD_SCHEMA_GENERATION
        || checksum(mutation).map_err(json_sql_error)? != mutation.checksum
    {
        quarantine(
            transaction,
            mutation,
            "incompatible library, schema, or checksum",
        )?;
        return Ok(ApplyOutcome::Quarantined);
    }
    if transaction
        .query_row(
            "SELECT 1 FROM cloud_applied_mutation WHERE mutation_id = ?1",
            [&mutation.mutation_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Ok(ApplyOutcome::Duplicate);
    }
    let observed_frontier = transaction
        .query_row(
            "SELECT hlc_physical_ms, hlc_logical
             FROM cloud_device_frontier WHERE device_id = ?1",
            [&mutation.device_id],
            |row| {
                Ok(HybridTimestamp {
                    physical_ms: row.get::<_, i64>(0)? as u64,
                    logical: row.get::<_, i64>(1)? as u32,
                })
            },
        )
        .optional()?;
    if observed_frontier.is_some_and(|frontier| frontier >= mutation.timestamp) {
        return Ok(ApplyOutcome::Duplicate);
    }

    let local_clock = transaction.query_row(
        "SELECT hlc_physical_ms, hlc_logical FROM cloud_state WHERE singleton = 1",
        [],
        |row| {
            Ok(HybridTimestamp {
                physical_ms: row.get::<_, i64>(0)? as u64,
                logical: row.get::<_, i64>(1)? as u32,
            })
        },
    )?;
    let next_clock = HybridTimestamp::receive(local_clock, mutation.timestamp);
    transaction.execute(
        "UPDATE cloud_state SET hlc_physical_ms = ?1, hlc_logical = ?2 WHERE singleton = 1",
        params![next_clock.physical_ms as i64, next_clock.logical as i64],
    )?;

    let outcome = apply_checked_operation(transaction, mutation)?;

    transaction.execute(
        "INSERT INTO cloud_applied_mutation
             (mutation_id, device_id, hlc_physical_ms, hlc_logical, checksum, applied_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            mutation.mutation_id,
            mutation.device_id,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.checksum,
            Utc::now().to_rfc3339(),
        ],
    )?;
    advance_frontier(transaction, &mutation.device_id, mutation.timestamp)?;
    Ok(outcome)
}

fn apply_operation(
    transaction: &Transaction<'_>,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    match &mutation.operation {
        CloudOperation::Batch { operations } => {
            let mut applied = false;
            let mut applied_item_id = None;
            let mut quarantined = false;
            for operation in operations {
                // Do not clone the enclosing batch for every child. A 10k
                // operation envelope otherwise copies the complete payload
                // 10k times before any SQL is applied.
                let child = CloudMutation {
                    mutation_id: mutation.mutation_id.clone(),
                    library_id: mutation.library_id.clone(),
                    device_id: mutation.device_id.clone(),
                    timestamp: mutation.timestamp,
                    causal_frontier: mutation.causal_frontier.clone(),
                    operation: operation.clone(),
                    schema_generation: mutation.schema_generation,
                    checksum: mutation.checksum.clone(),
                };
                match apply_checked_operation(transaction, &child)? {
                    ApplyOutcome::Applied(item_id) => {
                        applied = true;
                        applied_item_id = item_id.or(applied_item_id);
                    }
                    ApplyOutcome::AppliedAndQuarantined(item_id) => {
                        applied = true;
                        applied_item_id = item_id.or(applied_item_id);
                        quarantined = true;
                    }
                    ApplyOutcome::Quarantined => quarantined = true,
                    ApplyOutcome::Ignored | ApplyOutcome::Duplicate => {}
                }
            }
            Ok(match (applied, quarantined) {
                (true, true) => ApplyOutcome::AppliedAndQuarantined(applied_item_id),
                (true, false) => ApplyOutcome::Applied(applied_item_id),
                (false, true) => ApplyOutcome::Quarantined,
                (false, false) => ApplyOutcome::Ignored,
            })
        }
        CloudOperation::UpsertItem { item } => {
            if item_id(transaction, &item.item_key)?.is_some() {
                return Ok(ApplyOutcome::Ignored);
            }
            create_item(transaction, item, mutation)
        }
        CloudOperation::ItemFields { item_key, fields } => {
            let Some(item_id) = item_id(transaction, item_key)? else {
                quarantine(transaction, mutation, "item fields target does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            let mut changed = false;
            for (field, value) in fields {
                if scalar_wins(transaction, "item", item_key, field, mutation)? {
                    changed |= apply_item_field(transaction, item_id, field, value)?;
                    write_field_clock(transaction, "item", item_key, field, mutation)?;
                }
            }
            Ok(if changed {
                ApplyOutcome::Applied(Some(item_id))
            } else {
                ApplyOutcome::Ignored
            })
        }
        CloudOperation::Lifecycle {
            item_key,
            lifecycle,
        } => {
            if !matches!(lifecycle.as_str(), "inbox" | "active" | "trash") {
                quarantine(transaction, mutation, "invalid lifecycle")?;
                return Ok(ApplyOutcome::Quarantined);
            }
            let Some(item_id) = item_id(transaction, item_key)? else {
                quarantine(transaction, mutation, "lifecycle target does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            if !scalar_wins(transaction, "item", item_key, "lifecycle", mutation)? {
                return Ok(ApplyOutcome::Ignored);
            }
            transaction.execute(
                "UPDATE library_root SET lifecycle = ?1 WHERE item_id = ?2",
                params![lifecycle, item_id],
            )?;
            write_field_clock(transaction, "item", item_key, "lifecycle", mutation)?;
            Ok(ApplyOutcome::Applied(Some(item_id)))
        }
        CloudOperation::TagMembership {
            item_key,
            namespace,
            subtag,
            present,
        } => {
            let Some(item_id) = item_id(transaction, item_key)? else {
                quarantine(transaction, mutation, "tag target does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            let member_key = format!("{namespace}\u{0}{subtag}");
            if !membership_wins(
                transaction,
                "tag",
                item_key,
                &member_key,
                *present,
                mutation,
            )? {
                return Ok(ApplyOutcome::Ignored);
            }
            if *present {
                transaction.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)
                     ON CONFLICT(namespace, subtag) DO NOTHING",
                    params![namespace, subtag],
                )?;
            }
            let tag_id: Option<i64> = transaction
                .query_row(
                    "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                    params![namespace, subtag],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(tag_id) = tag_id {
                update_membership_bitmap(
                    transaction,
                    crate::canonical_bitmap::BitmapDomain::Tag,
                    tag_id,
                    item_id,
                    *present,
                )?;
            }
            write_membership_clock(
                transaction,
                "tag",
                item_key,
                &member_key,
                *present,
                mutation,
            )?;
            Ok(ApplyOutcome::Applied(Some(item_id)))
        }
        CloudOperation::FolderMembership {
            item_key,
            folder_key,
            present,
            position_rank,
        } => {
            let Some(item_id) = item_id(transaction, item_key)? else {
                quarantine(transaction, mutation, "folder target does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            let folder_id: Option<i64> = transaction
                .query_row(
                    "SELECT folder_id FROM folder WHERE folder_key = ?1",
                    [folder_key],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(folder_id) = folder_id else {
                quarantine(transaction, mutation, "folder does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            if !membership_wins(
                transaction,
                "folder",
                folder_key,
                item_key,
                *present,
                mutation,
            )? {
                return Ok(ApplyOutcome::Ignored);
            }
            update_membership_bitmap(
                transaction,
                crate::canonical_bitmap::BitmapDomain::Folder,
                folder_id,
                item_id,
                *present,
            )?;
            // Manual folder ordering exists only when the folder has a
            // canonical order vector; splice the member into it.
            if let Some(mut order) =
                crate::canonical_bitmap::load_order(transaction, "folder", folder_id)?
            {
                let member = membership_id(item_id)?;
                order.retain(|entry| *entry != member);
                if *present {
                    let index = position_rank
                        .and_then(|rank| usize::try_from(rank).ok())
                        .unwrap_or(order.len())
                        .min(order.len());
                    order.insert(index, member);
                }
                crate::canonical_bitmap::replace_order(
                    transaction,
                    "folder",
                    folder_id,
                    next_revision(transaction)?,
                    &order,
                )?;
            }
            write_membership_clock(
                transaction,
                "folder",
                folder_key,
                item_key,
                *present,
                mutation,
            )?;
            Ok(ApplyOutcome::Applied(Some(item_id)))
        }
        CloudOperation::UpsertFolder {
            folder,
            changed_fields,
        } => apply_folder_upsert(transaction, folder, changed_fields, mutation),
        CloudOperation::DeleteFolder { folder_key } => {
            apply_folder_delete(transaction, folder_key, mutation)
        }
        CloudOperation::UpsertSmartFolder {
            smart_folder,
            changed_fields,
        } => apply_smart_folder_upsert(transaction, smart_folder, changed_fields, mutation),
        CloudOperation::DeleteSmartFolder { smart_folder_key } => {
            apply_smart_folder_delete(transaction, smart_folder_key, mutation)
        }
        CloudOperation::UpsertSubscription {
            subscription,
            changed_fields,
        } => apply_subscription_upsert(transaction, subscription, changed_fields, mutation),
        CloudOperation::DeleteSubscription { subscription_key } => {
            apply_subscription_delete(transaction, subscription_key, mutation)
        }
        CloudOperation::UpsertSubscriptionQuery {
            query,
            changed_fields,
        } => apply_subscription_query_upsert(transaction, query, changed_fields, mutation),
        CloudOperation::DeleteSubscriptionQuery { query_key } => {
            apply_subscription_query_delete(transaction, query_key, mutation)
        }
        CloudOperation::UpsertSourcePost {
            source_post,
            changed_fields,
        } => apply_source_post_upsert(transaction, source_post, changed_fields, mutation),
        CloudOperation::UpsertSourceItem {
            source_item,
            changed_fields,
        } => apply_source_item_upsert(transaction, source_item, changed_fields, mutation),
        CloudOperation::DeleteSourceItem { source_item_key } => {
            apply_source_item_delete(transaction, source_item_key, mutation)
        }
        CloudOperation::RestoreSourceItem {
            tombstone_mutation_id,
            source_item,
        } => apply_source_item_restore(transaction, tombstone_mutation_id, source_item, mutation),
        CloudOperation::SubscriptionSourcePost {
            subscription_key,
            query_key,
            source_post_key,
            present,
        } => apply_subscription_source_post(
            transaction,
            subscription_key,
            query_key,
            source_post_key,
            *present,
            mutation,
        ),
        CloudOperation::GroupAssignment {
            media_item_key,
            collection_item_key,
            position_rank,
        } => apply_group_assignment(
            transaction,
            media_item_key,
            collection_item_key.as_deref(),
            *position_rank,
            mutation,
        ),
        CloudOperation::ReorderMember {
            collection_item_key,
            media_item_key,
            position_rank,
        } => {
            let Some(media_id) = item_id(transaction, media_item_key)? else {
                quarantine(transaction, mutation, "reorder member does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            let Some(collection_id) = item_id(transaction, collection_item_key)? else {
                quarantine(transaction, mutation, "reorder collection does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            };
            if !scalar_wins(
                transaction,
                "item",
                media_item_key,
                "position_rank",
                mutation,
            )? {
                return Ok(ApplyOutcome::Ignored);
            }
            let in_group = crate::canonical_bitmap::load_order(
                transaction,
                "group",
                collection_id,
            )?
            .is_some_and(|order| {
                u32::try_from(media_id)
                    .ok()
                    .is_some_and(|media| order.contains(&media))
            });
            if !in_group {
                quarantine(
                    transaction,
                    mutation,
                    "reorder target is not in the collection",
                )?;
                return Ok(ApplyOutcome::Quarantined);
            }
            splice_group_member(
                transaction,
                collection_id,
                media_id,
                usize::try_from(*position_rank).ok(),
                true,
            )?;
            write_field_clock(
                transaction,
                "item",
                media_item_key,
                "position_rank",
                mutation,
            )?;
            Ok(ApplyOutcome::Applied(Some(collection_id)))
        }
        CloudOperation::DeleteItem { item_key } => {
            if !scalar_wins(transaction, "item", item_key, "tombstone", mutation)? {
                return Ok(ApplyOutcome::Ignored);
            }
            let item_id = item_id(transaction, item_key)?;
            write_tombstone(transaction, "item", item_key, mutation)?;
            transaction.execute("DELETE FROM library_item WHERE item_key = ?1", [item_key])?;
            write_field_clock(transaction, "item", item_key, "tombstone", mutation)?;
            Ok(ApplyOutcome::Applied(item_id))
        }
        CloudOperation::RestoreItem {
            tombstone_mutation_id,
            item,
        } => restore_item(transaction, tombstone_mutation_id, item, mutation),
    }
}

fn apply_checked_operation(
    transaction: &Transaction<'_>,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let Some((object_kind, object_key)) = mutation.operation.object_key() else {
        return apply_operation(transaction, mutation);
    };
    let tombstone = tombstone_id(transaction, object_kind, object_key)?;
    let restored = matches!(
        &mutation.operation,
        CloudOperation::RestoreItem {
            tombstone_mutation_id,
            ..
        } if Some(tombstone_mutation_id.as_str()) == tombstone.as_deref()
    ) || matches!(
        &mutation.operation,
        CloudOperation::RestoreSourceItem {
            tombstone_mutation_id,
            ..
        } if Some(tombstone_mutation_id.as_str()) == tombstone.as_deref()
    );
    let deletion = matches!(
        mutation.operation,
        CloudOperation::DeleteItem { .. }
            | CloudOperation::DeleteFolder { .. }
            | CloudOperation::DeleteSmartFolder { .. }
            | CloudOperation::DeleteSubscription { .. }
            | CloudOperation::DeleteSubscriptionQuery { .. }
            | CloudOperation::DeleteSourceItem { .. }
    );
    if tombstone.is_some() && !deletion && !restored {
        Ok(ApplyOutcome::Ignored)
    } else {
        apply_operation(transaction, mutation)
    }
}

fn apply_subscription_upsert(
    transaction: &Transaction<'_>,
    subscription: &CloudSubscription,
    changed_fields: &[String],
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let next_run_at = subscription_next_run(&subscription.schedule)?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM subscription WHERE subscription_key = ?1)",
        [&subscription.subscription_key],
        |row| row.get(0),
    )?;
    if !exists {
        transaction.execute(
            "INSERT INTO subscription
                 (subscription_key, name, schedule, paused, initial_post_limit,
                  periodic_post_limit, next_run_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                subscription.subscription_key,
                subscription.name,
                subscription.schedule,
                subscription.paused,
                subscription.initial_post_limit,
                subscription.periodic_post_limit,
                next_run_at,
                subscription.created_at,
            ],
        )?;
        for field in [
            "name",
            "schedule",
            "paused",
            "initial_post_limit",
            "periodic_post_limit",
        ] {
            write_field_clock(
                transaction,
                "subscription",
                &subscription.subscription_key,
                field,
                mutation,
            )?;
        }
        return Ok(ApplyOutcome::Applied(None));
    }

    let mut changed = false;
    for field in changed_fields {
        if !scalar_wins(
            transaction,
            "subscription",
            &subscription.subscription_key,
            field,
            mutation,
        )? {
            continue;
        }
        changed |= apply_subscription_field(transaction, subscription, field, &next_run_at)?;
        write_field_clock(
            transaction,
            "subscription",
            &subscription.subscription_key,
            field,
            mutation,
        )?;
    }
    Ok(if changed {
        ApplyOutcome::Applied(None)
    } else {
        ApplyOutcome::Ignored
    })
}

fn apply_subscription_field(
    transaction: &Transaction<'_>,
    subscription: &CloudSubscription,
    field: &str,
    next_run_at: &Option<String>,
) -> rusqlite::Result<bool> {
    let changed = match field {
        "name" => transaction.execute(
            "UPDATE subscription SET name = ?1 WHERE subscription_key = ?2",
            params![subscription.name, subscription.subscription_key],
        )?,
        "schedule" => transaction.execute(
            "UPDATE subscription SET schedule = ?1, next_run_at = ?2 WHERE subscription_key = ?3",
            params![
                subscription.schedule,
                next_run_at,
                subscription.subscription_key
            ],
        )?,
        "paused" => transaction.execute(
            "UPDATE subscription SET paused = ?1 WHERE subscription_key = ?2",
            params![subscription.paused, subscription.subscription_key],
        )?,
        "initial_post_limit" => transaction.execute(
            "UPDATE subscription SET initial_post_limit = ?1 WHERE subscription_key = ?2",
            params![
                subscription.initial_post_limit,
                subscription.subscription_key
            ],
        )?,
        "periodic_post_limit" => transaction.execute(
            "UPDATE subscription SET periodic_post_limit = ?1 WHERE subscription_key = ?2",
            params![
                subscription.periodic_post_limit,
                subscription.subscription_key
            ],
        )?,
        _ => 0,
    };
    Ok(changed > 0)
}

fn subscription_next_run(schedule: &str) -> rusqlite::Result<Option<String>> {
    crate::subscriptions_v2::next_schedule_at(schedule, &Utc::now().to_rfc3339())
        .map_err(rusqlite::Error::InvalidParameterName)
}

fn apply_subscription_delete(
    transaction: &Transaction<'_>,
    subscription_key: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !scalar_wins(
        transaction,
        "subscription",
        subscription_key,
        "tombstone",
        mutation,
    )? {
        return Ok(ApplyOutcome::Ignored);
    }
    write_tombstone(transaction, "subscription", subscription_key, mutation)?;
    transaction.execute(
        "DELETE FROM subscription WHERE subscription_key = ?1",
        [subscription_key],
    )?;
    write_field_clock(
        transaction,
        "subscription",
        subscription_key,
        "tombstone",
        mutation,
    )?;
    Ok(ApplyOutcome::Applied(None))
}

fn apply_subscription_query_upsert(
    transaction: &Transaction<'_>,
    query: &CloudSubscriptionQuery,
    changed_fields: &[String],
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let subscription_id: Option<i64> = transaction
        .query_row(
            "SELECT subscription_id FROM subscription WHERE subscription_key = ?1",
            [&query.subscription_key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(subscription_id) = subscription_id else {
        quarantine(
            transaction,
            mutation,
            "subscription query parent does not exist",
        )?;
        return Ok(ApplyOutcome::Quarantined);
    };
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM subscription_query WHERE query_key = ?1)",
        [&query.query_key],
        |row| row.get(0),
    )?;
    if !exists {
        transaction.execute(
            "INSERT INTO subscription_query
                 (query_key, subscription_id, site_id, domain_key, query_kind, query_text,
                  display_name, notes, group_posts, paused)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                query.query_key,
                subscription_id,
                query.site_id,
                query.domain_key,
                query.query_kind,
                query.query_text,
                query.display_name,
                query.notes,
                query.group_posts,
                query.paused,
            ],
        )?;
        for field in [
            "subscription",
            "site_id",
            "domain_key",
            "query_kind",
            "query_text",
            "display_name",
            "notes",
            "group_posts",
            "paused",
        ] {
            write_field_clock(
                transaction,
                "subscription_query",
                &query.query_key,
                field,
                mutation,
            )?;
        }
        return Ok(ApplyOutcome::Applied(None));
    }

    let mut changed = false;
    for field in changed_fields {
        if !scalar_wins(
            transaction,
            "subscription_query",
            &query.query_key,
            field,
            mutation,
        )? {
            continue;
        }
        changed |= apply_subscription_query_field(transaction, query, subscription_id, field)?;
        write_field_clock(
            transaction,
            "subscription_query",
            &query.query_key,
            field,
            mutation,
        )?;
    }
    Ok(if changed {
        ApplyOutcome::Applied(None)
    } else {
        ApplyOutcome::Ignored
    })
}

fn apply_subscription_query_field(
    transaction: &Transaction<'_>,
    query: &CloudSubscriptionQuery,
    subscription_id: i64,
    field: &str,
) -> rusqlite::Result<bool> {
    let changed = match field {
        "subscription" => transaction.execute(
            "UPDATE subscription_query SET subscription_id = ?1 WHERE query_key = ?2",
            params![subscription_id, query.query_key],
        )?,
        "site_id" | "domain_key" | "query_kind" | "query_text" => {
            let value = match field {
                "site_id" => &query.site_id,
                "domain_key" => &query.domain_key,
                "query_kind" => &query.query_kind,
                _ => &query.query_text,
            };
            let sql = format!(
                "UPDATE subscription_query SET {field} = ?1, resume_cursor = NULL,
                 initial_run_complete = 0, last_failure_at = NULL,
                 last_failure_kind = NULL, last_failure_message = NULL WHERE query_key = ?2"
            );
            transaction.execute(&sql, params![value, query.query_key])?
        }
        "display_name" => transaction.execute(
            "UPDATE subscription_query SET display_name = ?1 WHERE query_key = ?2",
            params![query.display_name, query.query_key],
        )?,
        "notes" => transaction.execute(
            "UPDATE subscription_query SET notes = ?1 WHERE query_key = ?2",
            params![query.notes, query.query_key],
        )?,
        "group_posts" => transaction.execute(
            "UPDATE subscription_query SET group_posts = ?1 WHERE query_key = ?2",
            params![query.group_posts, query.query_key],
        )?,
        "paused" => transaction.execute(
            "UPDATE subscription_query SET paused = ?1 WHERE query_key = ?2",
            params![query.paused, query.query_key],
        )?,
        _ => 0,
    };
    Ok(changed > 0)
}

fn apply_subscription_query_delete(
    transaction: &Transaction<'_>,
    query_key: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !scalar_wins(
        transaction,
        "subscription_query",
        query_key,
        "tombstone",
        mutation,
    )? {
        return Ok(ApplyOutcome::Ignored);
    }
    write_tombstone(transaction, "subscription_query", query_key, mutation)?;
    transaction.execute(
        "DELETE FROM subscription_query WHERE query_key = ?1",
        [query_key],
    )?;
    write_field_clock(
        transaction,
        "subscription_query",
        query_key,
        "tombstone",
        mutation,
    )?;
    Ok(ApplyOutcome::Applied(None))
}

pub(crate) fn source_post_sync_key(site_id: &str, post_key: &str) -> String {
    serde_json::to_string(&[site_id, post_key]).expect("source identity always serializes")
}

pub(crate) fn source_item_sync_key(site_id: &str, post_key: &str, item_key: &str) -> String {
    serde_json::to_string(&[site_id, post_key, item_key])
        .expect("source item identity always serializes")
}

fn apply_source_post_upsert(
    transaction: &Transaction<'_>,
    source_post: &CloudSourcePost,
    changed_fields: &[String],
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if source_post.site_id.trim().is_empty()
        || source_post.post_key.trim().is_empty()
        || source_post.source_post_key
            != source_post_sync_key(&source_post.site_id, &source_post.post_key)
    {
        quarantine(transaction, mutation, "invalid source post identity")?;
        return Ok(ApplyOutcome::Quarantined);
    }
    let root_item_id = match &source_post.root_item_key {
        Some(key) => match item_id(transaction, key)? {
            Some(item_id) => Some(item_id),
            None => {
                quarantine(transaction, mutation, "source post root does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            }
        },
        None => None,
    };
    let existing_id: Option<i64> = transaction
        .query_row(
            "SELECT source_post_id FROM source_post WHERE site_id = ?1 AND post_key = ?2",
            params![source_post.site_id, source_post.post_key],
            |row| row.get(0),
        )
        .optional()?;
    if existing_id.is_none() {
        transaction.execute(
            "INSERT INTO source_post (
                 site_id, post_key, canonical_url, creator_name, title, description,
                 captured_at, metadata_json, root_item_id, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                source_post.site_id,
                source_post.post_key,
                source_post.canonical_url,
                source_post.creator_name,
                source_post.title,
                source_post.description,
                source_post.captured_at,
                source_post.metadata_json,
                root_item_id,
                source_post.created_at,
                source_post.updated_at,
            ],
        )?;
        write_field_clock(
            transaction,
            "source_post",
            &source_post.source_post_key,
            "exists",
            mutation,
        )?;
        return Ok(ApplyOutcome::Applied(root_item_id));
    }

    let fields = [
        (
            "canonical_url",
            optional_json_string(&source_post.canonical_url),
        ),
        (
            "creator_name",
            optional_json_string(&source_post.creator_name),
        ),
        ("title", optional_json_string(&source_post.title)),
        (
            "description",
            optional_json_string(&source_post.description),
        ),
        (
            "captured_at",
            optional_json_string(&source_post.captured_at),
        ),
        (
            "metadata_json",
            optional_json_string(&source_post.metadata_json),
        ),
        (
            "root_item",
            root_item_id.map(Value::from).unwrap_or(Value::Null),
        ),
    ];
    let mut changed = false;
    for (field, value) in fields {
        if !changed_fields.iter().any(|candidate| candidate == field)
            || !scalar_wins(
                transaction,
                "source_post",
                &source_post.source_post_key,
                field,
                mutation,
            )?
        {
            continue;
        }
        let column = if field == "root_item" {
            "root_item_id"
        } else {
            field
        };
        let sql = format!(
            "UPDATE source_post SET {column} = ?1, updated_at = ?2
             WHERE site_id = ?3 AND post_key = ?4"
        );
        match value {
            Value::Null => transaction.execute(
                &sql,
                params![
                    Option::<String>::None,
                    source_post.updated_at,
                    source_post.site_id,
                    source_post.post_key
                ],
            )?,
            Value::String(value) => transaction.execute(
                &sql,
                params![
                    value,
                    source_post.updated_at,
                    source_post.site_id,
                    source_post.post_key
                ],
            )?,
            Value::Number(value) => transaction.execute(
                &sql,
                params![
                    value.as_i64(),
                    source_post.updated_at,
                    source_post.site_id,
                    source_post.post_key
                ],
            )?,
            _ => 0,
        };
        write_field_clock(
            transaction,
            "source_post",
            &source_post.source_post_key,
            field,
            mutation,
        )?;
        changed = true;
    }
    Ok(if changed {
        ApplyOutcome::Applied(root_item_id)
    } else {
        ApplyOutcome::Ignored
    })
}

fn apply_source_item_upsert(
    transaction: &Transaction<'_>,
    source_item: &CloudSourceItem,
    changed_fields: &[String],
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let source_post_id: Option<i64> = transaction
        .query_row(
            "SELECT source_post_id FROM source_post
             WHERE site_id || '' IS NOT NULL AND json_array(site_id, post_key) = ?1",
            [&source_item.source_post_key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(source_post_id) = source_post_id else {
        quarantine(transaction, mutation, "source item parent does not exist")?;
        return Ok(ApplyOutcome::Quarantined);
    };
    let parent_identity: (String, String) = transaction.query_row(
        "SELECT site_id, post_key FROM source_post WHERE source_post_id = ?1",
        [source_post_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if source_item.item_key.trim().is_empty()
        || source_item.source_item_key
            != source_item_sync_key(
                &parent_identity.0,
                &parent_identity.1,
                &source_item.item_key,
            )
    {
        quarantine(transaction, mutation, "invalid source item identity")?;
        return Ok(ApplyOutcome::Quarantined);
    }
    let media_item_id = match &source_item.media_item_key {
        Some(key) => match item_id(transaction, key)? {
            Some(item_id) => Some(item_id),
            None => {
                quarantine(transaction, mutation, "source item media does not exist")?;
                return Ok(ApplyOutcome::Quarantined);
            }
        },
        None => None,
    };
    let existing_id: Option<i64> = transaction
        .query_row(
            "SELECT source_item_id FROM source_item WHERE source_post_id = ?1 AND item_key = ?2",
            params![source_post_id, source_item.item_key],
            |row| row.get(0),
        )
        .optional()?;
    if existing_id.is_none() {
        transaction.execute(
            "INSERT INTO source_item (
                 source_post_id, item_key, position, media_url, canonical_url,
                 media_item_id, state, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                source_post_id,
                source_item.item_key,
                source_item.position,
                source_item.media_url,
                source_item.canonical_url,
                media_item_id,
                if media_item_id.is_some() {
                    "ingested"
                } else {
                    "pending"
                },
                source_item.created_at,
                source_item.updated_at,
            ],
        )?;
        write_field_clock(
            transaction,
            "source_item",
            &source_item.source_item_key,
            "exists",
            mutation,
        )?;
        return Ok(ApplyOutcome::Applied(media_item_id));
    }

    let fields = ["position", "media_url", "canonical_url", "media_item"];
    let mut changed = false;
    for field in fields {
        if !changed_fields.iter().any(|candidate| candidate == field)
            || !scalar_wins(
                transaction,
                "source_item",
                &source_item.source_item_key,
                field,
                mutation,
            )?
        {
            continue;
        }
        match field {
            "position" => transaction.execute(
                "UPDATE source_item SET position = ?1, updated_at = ?2
                 WHERE source_post_id = ?3 AND item_key = ?4",
                params![
                    source_item.position,
                    source_item.updated_at,
                    source_post_id,
                    source_item.item_key
                ],
            )?,
            "media_url" => transaction.execute(
                "UPDATE source_item SET media_url = ?1, updated_at = ?2
                 WHERE source_post_id = ?3 AND item_key = ?4",
                params![
                    source_item.media_url,
                    source_item.updated_at,
                    source_post_id,
                    source_item.item_key
                ],
            )?,
            "canonical_url" => transaction.execute(
                "UPDATE source_item SET canonical_url = ?1, updated_at = ?2
                 WHERE source_post_id = ?3 AND item_key = ?4",
                params![
                    source_item.canonical_url,
                    source_item.updated_at,
                    source_post_id,
                    source_item.item_key
                ],
            )?,
            "media_item" => transaction.execute(
                "UPDATE source_item SET media_item_id = ?1,
                     state = CASE WHEN ?1 IS NULL THEN state ELSE 'ingested' END,
                     last_error = CASE WHEN ?1 IS NULL THEN last_error ELSE NULL END,
                     updated_at = ?2
                 WHERE source_post_id = ?3 AND item_key = ?4",
                params![
                    media_item_id,
                    source_item.updated_at,
                    source_post_id,
                    source_item.item_key
                ],
            )?,
            _ => 0,
        };
        write_field_clock(
            transaction,
            "source_item",
            &source_item.source_item_key,
            field,
            mutation,
        )?;
        changed = true;
    }
    Ok(if changed {
        ApplyOutcome::Applied(media_item_id)
    } else {
        ApplyOutcome::Ignored
    })
}

fn apply_source_item_delete(
    transaction: &Transaction<'_>,
    source_item_key: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !scalar_wins(
        transaction,
        "source_item",
        source_item_key,
        "tombstone",
        mutation,
    )? {
        return Ok(ApplyOutcome::Ignored);
    }
    write_tombstone(transaction, "source_item", source_item_key, mutation)?;
    let identity: Vec<String> = serde_json::from_str(source_item_key).map_err(json_sql_error)?;
    if identity.len() != 3 {
        quarantine(
            transaction,
            mutation,
            "invalid source item tombstone identity",
        )?;
        return Ok(ApplyOutcome::Quarantined);
    }
    transaction.execute(
        "UPDATE source_item SET state = 'deleted', media_item_id = NULL,
             last_error = NULL, updated_at = ?1
         WHERE item_key = ?2 AND source_post_id IN (
             SELECT source_post_id FROM source_post WHERE site_id = ?3 AND post_key = ?4
         )",
        params![
            Utc::now().to_rfc3339(),
            identity[2],
            identity[0],
            identity[1]
        ],
    )?;
    write_field_clock(
        transaction,
        "source_item",
        source_item_key,
        "tombstone",
        mutation,
    )?;
    Ok(ApplyOutcome::Applied(None))
}

fn apply_source_item_restore(
    transaction: &Transaction<'_>,
    tombstone_mutation_id: &str,
    source_item: &CloudSourceItem,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let stored = tombstone_id(transaction, "source_item", &source_item.source_item_key)?;
    if stored.as_deref() != Some(tombstone_mutation_id) {
        quarantine(
            transaction,
            mutation,
            "source item restore does not reference the current tombstone",
        )?;
        return Ok(ApplyOutcome::Quarantined);
    }
    transaction.execute(
        "DELETE FROM cloud_tombstone
         WHERE object_kind = 'source_item' AND object_key = ?1",
        [&source_item.source_item_key],
    )?;
    let fields = [
        "exists",
        "position",
        "media_url",
        "canonical_url",
        "media_item",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    let outcome = apply_source_item_upsert(transaction, source_item, &fields, mutation)?;
    write_field_clock(
        transaction,
        "source_item",
        &source_item.source_item_key,
        "tombstone",
        mutation,
    )?;
    Ok(outcome)
}

fn apply_subscription_source_post(
    transaction: &Transaction<'_>,
    subscription_key: &str,
    query_key: &str,
    source_post_key: &str,
    present: bool,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !membership_wins(
        transaction,
        "subscription_source_post",
        query_key,
        source_post_key,
        present,
        mutation,
    )? {
        return Ok(ApplyOutcome::Ignored);
    }
    let ids: Option<(i64, i64, i64)> = transaction
        .query_row(
            "SELECT s.subscription_id, q.query_id, sp.source_post_id
             FROM subscription s
             JOIN subscription_query q ON q.subscription_id = s.subscription_id
             JOIN source_post sp ON json_array(sp.site_id, sp.post_key) = ?3
             WHERE s.subscription_key = ?1 AND q.query_key = ?2",
            params![subscription_key, query_key, source_post_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((subscription_id, query_id, source_post_id)) = ids else {
        quarantine(
            transaction,
            mutation,
            "subscription source post target does not exist",
        )?;
        return Ok(ApplyOutcome::Quarantined);
    };
    if present {
        transaction.execute(
            "INSERT INTO subscription_source_post
                 (subscription_id, query_id, source_post_id, last_seen_run_id)
             VALUES (?1, ?2, ?3, NULL) ON CONFLICT DO NOTHING",
            params![subscription_id, query_id, source_post_id],
        )?;
    } else {
        transaction.execute(
            "DELETE FROM subscription_source_post
             WHERE subscription_id = ?1 AND query_id = ?2 AND source_post_id = ?3",
            params![subscription_id, query_id, source_post_id],
        )?;
    }
    write_membership_clock(
        transaction,
        "subscription_source_post",
        query_key,
        source_post_key,
        present,
        mutation,
    )?;
    Ok(ApplyOutcome::Applied(None))
}

fn apply_item_field(
    transaction: &Transaction<'_>,
    item_id: i64,
    field: &str,
    value: &Value,
) -> rusqlite::Result<bool> {
    let now = Utc::now().to_rfc3339();
    let changed = match field {
        "label" | "notes" | "source_urls_json" => {
            let column = if field == "label" { "name" } else { field };
            let sql = format!(
                "UPDATE root_metadata SET {column} = ?1, updated_at = ?2
                 WHERE root_item_id = ?3"
            );
            transaction.execute(&sql, params![json_optional_string(value)?, now, item_id])?
        }
        "name" => transaction.execute(
            "UPDATE media_asset SET name = ?1, updated_at = ?2 WHERE item_id = ?3",
            params![json_optional_string(value)?, now, item_id],
        )?,
        "captured_at" => transaction.execute(
            "UPDATE media_asset SET captured_at = ?1, updated_at = ?2 WHERE item_id = ?3",
            params![json_optional_string(value)?, now, item_id],
        )?,
        "rating" => transaction.execute(
            "UPDATE root_metadata SET rating = ?1, updated_at = ?2 WHERE root_item_id = ?3",
            params![json_optional_i64(value)?, now, item_id],
        )?,
        _ => return Ok(false),
    };
    Ok(changed > 0)
}

fn apply_folder_upsert(
    transaction: &Transaction<'_>,
    folder: &CloudFolder,
    changed_fields: &[String],
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let existing_id: Option<i64> = transaction
        .query_row(
            "SELECT folder_id FROM folder WHERE folder_key = ?1",
            [&folder.folder_key],
            |row| row.get(0),
        )
        .optional()?;
    let parent_id = if existing_id.is_none() || changed_fields.iter().any(|field| field == "parent")
    {
        let parent_id = resolve_folder_parent(
            transaction,
            existing_id,
            &folder.folder_key,
            folder.parent_key.as_deref(),
            mutation,
        )?;
        if folder.parent_key.is_some() && parent_id.is_none() {
            return Ok(ApplyOutcome::Quarantined);
        }
        parent_id
    } else {
        None
    };

    if existing_id.is_none() {
        transaction.execute(
            "INSERT INTO folder
                 (folder_key, name, parent_id, icon, color, notes, sort_rank, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                folder.folder_key,
                folder.name,
                parent_id,
                folder.icon,
                folder.color,
                folder.notes,
                folder.sort_rank,
                folder.created_at,
                folder.updated_at,
            ],
        )?;
        for field in changed_fields {
            write_field_clock(transaction, "folder", &folder.folder_key, field, mutation)?;
        }
        return Ok(ApplyOutcome::Applied(None));
    }

    let mut changed = false;
    if changed_fields.iter().any(|field| field == "name")
        && scalar_wins(transaction, "folder", &folder.folder_key, "name", mutation)?
    {
        transaction.execute(
            "UPDATE folder SET name = ?1, updated_at = ?2 WHERE folder_key = ?3",
            params![folder.name, folder.updated_at, folder.folder_key],
        )?;
        write_field_clock(transaction, "folder", &folder.folder_key, "name", mutation)?;
        changed = true;
    }
    if changed_fields.iter().any(|field| field == "parent")
        && scalar_wins(
            transaction,
            "folder",
            &folder.folder_key,
            "parent",
            mutation,
        )?
    {
        transaction.execute(
            "UPDATE folder SET parent_id = ?1, updated_at = ?2 WHERE folder_key = ?3",
            params![parent_id, folder.updated_at, folder.folder_key],
        )?;
        write_field_clock(
            transaction,
            "folder",
            &folder.folder_key,
            "parent",
            mutation,
        )?;
        changed = true;
    }
    if changed_fields.iter().any(|field| field == "icon")
        && scalar_wins(transaction, "folder", &folder.folder_key, "icon", mutation)?
    {
        transaction.execute(
            "UPDATE folder SET icon = ?1, updated_at = ?2 WHERE folder_key = ?3",
            params![folder.icon, folder.updated_at, folder.folder_key],
        )?;
        write_field_clock(transaction, "folder", &folder.folder_key, "icon", mutation)?;
        changed = true;
    }
    if changed_fields.iter().any(|field| field == "color")
        && scalar_wins(transaction, "folder", &folder.folder_key, "color", mutation)?
    {
        transaction.execute(
            "UPDATE folder SET color = ?1, updated_at = ?2 WHERE folder_key = ?3",
            params![folder.color, folder.updated_at, folder.folder_key],
        )?;
        write_field_clock(transaction, "folder", &folder.folder_key, "color", mutation)?;
        changed = true;
    }
    if changed_fields.iter().any(|field| field == "notes")
        && scalar_wins(transaction, "folder", &folder.folder_key, "notes", mutation)?
    {
        transaction.execute(
            "UPDATE folder SET notes = ?1, updated_at = ?2 WHERE folder_key = ?3",
            params![folder.notes, folder.updated_at, folder.folder_key],
        )?;
        write_field_clock(transaction, "folder", &folder.folder_key, "notes", mutation)?;
        changed = true;
    }
    if changed_fields.iter().any(|field| field == "sort_rank")
        && scalar_wins(
            transaction,
            "folder",
            &folder.folder_key,
            "sort_rank",
            mutation,
        )?
    {
        transaction.execute(
            "UPDATE folder SET sort_rank = ?1, updated_at = ?2 WHERE folder_key = ?3",
            params![folder.sort_rank, folder.updated_at, folder.folder_key],
        )?;
        write_field_clock(
            transaction,
            "folder",
            &folder.folder_key,
            "sort_rank",
            mutation,
        )?;
        changed = true;
    }
    Ok(if changed {
        ApplyOutcome::Applied(None)
    } else {
        ApplyOutcome::Ignored
    })
}

fn resolve_folder_parent(
    transaction: &Transaction<'_>,
    existing_id: Option<i64>,
    folder_key: &str,
    parent_key: Option<&str>,
    mutation: &CloudMutation,
) -> rusqlite::Result<Option<i64>> {
    let Some(parent_key) = parent_key else {
        return Ok(None);
    };
    if parent_key == folder_key {
        quarantine(transaction, mutation, "folder cannot parent itself")?;
        return Ok(None);
    }
    let parent_id: Option<i64> = transaction
        .query_row(
            "SELECT folder_id FROM folder WHERE folder_key = ?1",
            [parent_key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(parent_id) = parent_id else {
        quarantine(transaction, mutation, "folder parent does not exist")?;
        return Ok(None);
    };
    if let Some(existing_id) = existing_id {
        let creates_cycle: bool = transaction.query_row(
            "WITH RECURSIVE descendants(folder_id) AS (
                 SELECT folder_id FROM folder WHERE parent_id = ?1
                 UNION ALL
                 SELECT folder.folder_id FROM folder
                 JOIN descendants ON folder.parent_id = descendants.folder_id
             ) SELECT EXISTS(SELECT 1 FROM descendants WHERE folder_id = ?2)",
            params![existing_id, parent_id],
            |row| row.get(0),
        )?;
        if creates_cycle {
            quarantine(
                transaction,
                mutation,
                "folder hierarchy would contain a cycle",
            )?;
            return Ok(None);
        }
    }
    Ok(Some(parent_id))
}

fn apply_folder_delete(
    transaction: &Transaction<'_>,
    folder_key: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !scalar_wins(transaction, "folder", folder_key, "tombstone", mutation)? {
        return Ok(ApplyOutcome::Ignored);
    }
    write_tombstone(transaction, "folder", folder_key, mutation)?;
    transaction.execute("DELETE FROM folder WHERE folder_key = ?1", [folder_key])?;
    write_field_clock(transaction, "folder", folder_key, "tombstone", mutation)?;
    Ok(ApplyOutcome::Applied(None))
}

fn apply_smart_folder_upsert(
    transaction: &Transaction<'_>,
    folder: &CloudSmartFolder,
    changed_fields: &[String],
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    serde_json::from_str::<Value>(&folder.predicate_json).map_err(json_sql_error)?;
    let existing_id: Option<i64> = transaction
        .query_row(
            "SELECT smart_folder_id FROM smart_folder WHERE smart_folder_key = ?1",
            [&folder.smart_folder_key],
            |row| row.get(0),
        )
        .optional()?;
    let parent_id = if existing_id.is_none() || changed_fields.iter().any(|field| field == "parent")
    {
        let parent_id = resolve_smart_folder_parent(
            transaction,
            existing_id,
            &folder.smart_folder_key,
            folder.parent_key.as_deref(),
            mutation,
        )?;
        if folder.parent_key.is_some() && parent_id.is_none() {
            return Ok(ApplyOutcome::Quarantined);
        }
        parent_id
    } else {
        None
    };

    if existing_id.is_none() {
        transaction.execute(
            "INSERT INTO smart_folder
                 (smart_folder_key, name, parent_id, icon, color, notes, predicate_json,
                  sort_field, sort_order, display_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                folder.smart_folder_key,
                folder.name,
                parent_id,
                folder.icon,
                folder.color,
                folder.notes,
                folder.predicate_json,
                folder.sort_field,
                folder.sort_order,
                folder.display_order,
                folder.created_at,
                folder.updated_at,
            ],
        )?;
        for field in changed_fields {
            write_field_clock(
                transaction,
                "smart_folder",
                &folder.smart_folder_key,
                field,
                mutation,
            )?;
        }
        return Ok(ApplyOutcome::Applied(None));
    }

    let fields = [
        ("name", Value::String(folder.name.clone())),
        ("parent", parent_id.map(Value::from).unwrap_or(Value::Null)),
        ("icon", optional_json_string(&folder.icon)),
        ("color", optional_json_string(&folder.color)),
        ("notes", optional_json_string(&folder.notes)),
        (
            "predicate_json",
            Value::String(folder.predicate_json.clone()),
        ),
        ("sort_field", optional_json_string(&folder.sort_field)),
        ("sort_order", optional_json_string(&folder.sort_order)),
        (
            "display_order",
            folder.display_order.map(Value::from).unwrap_or(Value::Null),
        ),
    ];
    let mut changed = false;
    for (field, value) in fields {
        if !changed_fields.iter().any(|changed| changed == field) {
            continue;
        }
        if !scalar_wins(
            transaction,
            "smart_folder",
            &folder.smart_folder_key,
            field,
            mutation,
        )? {
            continue;
        }
        apply_smart_folder_field(
            transaction,
            &folder.smart_folder_key,
            field,
            &value,
            &folder.updated_at,
        )?;
        write_field_clock(
            transaction,
            "smart_folder",
            &folder.smart_folder_key,
            field,
            mutation,
        )?;
        changed = true;
    }
    Ok(if changed {
        ApplyOutcome::Applied(None)
    } else {
        ApplyOutcome::Ignored
    })
}

fn optional_json_string(value: &Option<String>) -> Value {
    value.clone().map(Value::String).unwrap_or(Value::Null)
}

fn apply_smart_folder_field(
    transaction: &Transaction<'_>,
    folder_key: &str,
    field: &str,
    value: &Value,
    updated_at: &str,
) -> rusqlite::Result<()> {
    let column = match field {
        "name" => "name",
        "parent" => "parent_id",
        "icon" => "icon",
        "color" => "color",
        "notes" => "notes",
        "predicate_json" => "predicate_json",
        "sort_field" => "sort_field",
        "sort_order" => "sort_order",
        "display_order" => "display_order",
        _ => return Ok(()),
    };
    let sql = format!(
        "UPDATE smart_folder SET {column} = ?1, updated_at = ?2 WHERE smart_folder_key = ?3"
    );
    match value {
        Value::Null => transaction.execute(
            &sql,
            params![Option::<String>::None, updated_at, folder_key],
        )?,
        Value::String(value) => {
            transaction.execute(&sql, params![value, updated_at, folder_key])?
        }
        Value::Number(value) => transaction.execute(
            &sql,
            params![
                value
                    .as_i64()
                    .ok_or_else(|| rusqlite::Error::InvalidParameterName(
                        "smart folder field must be an integer".into()
                    ))?,
                updated_at,
                folder_key
            ],
        )?,
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid smart folder field".into(),
            ))
        }
    };
    Ok(())
}

fn resolve_smart_folder_parent(
    transaction: &Transaction<'_>,
    existing_id: Option<i64>,
    folder_key: &str,
    parent_key: Option<&str>,
    mutation: &CloudMutation,
) -> rusqlite::Result<Option<i64>> {
    let Some(parent_key) = parent_key else {
        return Ok(None);
    };
    if parent_key == folder_key {
        quarantine(transaction, mutation, "smart folder cannot parent itself")?;
        return Ok(None);
    }
    let parent_id: Option<i64> = transaction
        .query_row(
            "SELECT smart_folder_id FROM smart_folder WHERE smart_folder_key = ?1",
            [parent_key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(parent_id) = parent_id else {
        quarantine(transaction, mutation, "smart folder parent does not exist")?;
        return Ok(None);
    };
    if let Some(existing_id) = existing_id {
        let creates_cycle: bool = transaction.query_row(
            "WITH RECURSIVE descendants(smart_folder_id) AS (
                 SELECT smart_folder_id FROM smart_folder WHERE parent_id = ?1
                 UNION ALL
                 SELECT smart_folder.smart_folder_id FROM smart_folder
                 JOIN descendants ON smart_folder.parent_id = descendants.smart_folder_id
             ) SELECT EXISTS(SELECT 1 FROM descendants WHERE smart_folder_id = ?2)",
            params![existing_id, parent_id],
            |row| row.get(0),
        )?;
        if creates_cycle {
            quarantine(
                transaction,
                mutation,
                "smart folder hierarchy would contain a cycle",
            )?;
            return Ok(None);
        }
    }
    Ok(Some(parent_id))
}

fn apply_smart_folder_delete(
    transaction: &Transaction<'_>,
    folder_key: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !scalar_wins(
        transaction,
        "smart_folder",
        folder_key,
        "tombstone",
        mutation,
    )? {
        return Ok(ApplyOutcome::Ignored);
    }
    write_tombstone(transaction, "smart_folder", folder_key, mutation)?;
    transaction.execute(
        "DELETE FROM smart_folder WHERE smart_folder_key = ?1",
        [folder_key],
    )?;
    write_field_clock(
        transaction,
        "smart_folder",
        folder_key,
        "tombstone",
        mutation,
    )?;
    Ok(ApplyOutcome::Applied(None))
}

fn apply_group_assignment(
    transaction: &Transaction<'_>,
    media_item_key: &str,
    collection_item_key: Option<&str>,
    position_rank: Option<i64>,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let Some(media_id) = item_id(transaction, media_item_key)? else {
        quarantine(transaction, mutation, "group member does not exist")?;
        return Ok(ApplyOutcome::Quarantined);
    };
    let kind: String = transaction.query_row(
        "SELECT kind FROM library_item WHERE item_id = ?1",
        [media_id],
        |row| row.get(0),
    )?;
    if kind != "media" {
        quarantine(transaction, mutation, "collections cannot be nested")?;
        return Ok(ApplyOutcome::Quarantined);
    }
    if !scalar_wins(transaction, "item", media_item_key, "collection", mutation)? {
        return Ok(ApplyOutcome::Ignored);
    }
    if let Some(previous_group) = owning_group(transaction, media_id)? {
        splice_group_member(transaction, previous_group, media_id, None, false)?;
    }
    if let Some(collection_key) = collection_item_key {
        let collection: Option<(i64, String)> = transaction
            .query_row(
                "SELECT item_id, kind FROM library_item WHERE item_key = ?1",
                [collection_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((collection_id, collection_kind)) = collection else {
            quarantine(transaction, mutation, "group collection does not exist")?;
            return Ok(ApplyOutcome::Quarantined);
        };
        if collection_kind != "collection" {
            quarantine(transaction, mutation, "group target is not a collection")?;
            return Ok(ApplyOutcome::Quarantined);
        }
        transaction.execute("DELETE FROM library_root WHERE item_id = ?1", [media_id])?;
        splice_group_member(
            transaction,
            collection_id,
            media_id,
            position_rank.and_then(|rank| usize::try_from(rank).ok()),
            true,
        )?;
    } else {
        transaction.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')
             ON CONFLICT(item_id) DO NOTHING",
            [media_id],
        )?;
    }
    write_field_clock(transaction, "item", media_item_key, "collection", mutation)?;
    Ok(ApplyOutcome::Applied(Some(media_id)))
}

fn restore_item(
    transaction: &Transaction<'_>,
    tombstone_mutation_id: &str,
    item: &RestoredItem,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    let stored: Option<String> = transaction
        .query_row(
            "SELECT mutation_id FROM cloud_tombstone WHERE object_kind = 'item' AND object_key = ?1",
            [&item.item_key],
            |row| row.get(0),
        )
        .optional()?;
    if stored.as_deref() != Some(tombstone_mutation_id) {
        quarantine(
            transaction,
            mutation,
            "restore does not reference the current tombstone",
        )?;
        return Ok(ApplyOutcome::Quarantined);
    }
    let outcome = create_item(transaction, item, mutation)?;
    if matches!(outcome, ApplyOutcome::Quarantined) {
        return Ok(outcome);
    }
    transaction.execute(
        "DELETE FROM cloud_tombstone WHERE object_kind = 'item' AND object_key = ?1",
        [&item.item_key],
    )?;
    write_field_clock(transaction, "item", &item.item_key, "tombstone", mutation)?;
    Ok(outcome)
}

fn create_item(
    transaction: &Transaction<'_>,
    item: &RestoredItem,
    mutation: &CloudMutation,
) -> rusqlite::Result<ApplyOutcome> {
    if !matches!(item.kind.as_str(), "media" | "collection")
        || !matches!(item.lifecycle.as_str(), "inbox" | "active" | "trash")
        || (item.kind == "media" && item.media.is_none())
        || (item.kind == "collection" && item.media.is_some())
    {
        quarantine(transaction, mutation, "invalid item shape")?;
        return Ok(ApplyOutcome::Quarantined);
    }
    let now = Utc::now().to_rfc3339();
    transaction.execute(
        "INSERT INTO library_item (item_key, kind, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![item.item_key, item.kind, now],
    )?;
    let item_id = transaction.last_insert_rowid();
    if item.kind == "media" {
        let media = item.media.as_ref().expect("media shape was validated");
        transaction.execute(
            "INSERT INTO media_file
                 (file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms,
                  frame_count, has_audio, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(file_hash) DO NOTHING",
            params![
                media.file_hash,
                media.mime_type,
                media.size_bytes,
                media.pixel_width,
                media.pixel_height,
                media.duration_ms,
                media.frame_count,
                i64::from(media.has_audio),
                Utc::now().to_rfc3339()
            ],
        )?;
        let file_id: i64 = transaction.query_row(
            "SELECT file_id FROM media_file WHERE file_hash = ?1",
            [&media.file_hash],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO media_asset
                 (item_id, file_id, name, captured_at, imported_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                item_id,
                file_id,
                media.name,
                media.captured_at,
                media.imported_at,
                Utc::now().to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
             VALUES (?1, 'queued', ?2)
             ON CONFLICT(file_hash) DO UPDATE SET state = 'queued', updated_at = excluded.updated_at",
            params![media.file_hash, Utc::now().to_rfc3339()],
        )?;
    }
    transaction.execute(
        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
        params![item_id, item.lifecycle],
    )?;
    let media = item.media.as_ref();
    transaction.execute(
        "INSERT INTO root_metadata
             (root_item_id, name, rating, notes, source_urls_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, COALESCE(?5, '[]'), ?6)",
        params![
            item_id,
            item.label,
            media.and_then(|media| media.rating),
            media.and_then(|media| media.notes.as_deref()),
            media.and_then(|media| media.source_urls_json.as_deref()),
            Utc::now().to_rfc3339(),
        ],
    )?;
    if let Some(cover_key) = &item.cover_media_item_key {
        transaction.execute(
            "UPDATE library_item SET cover_media_item_id = (
                 SELECT item_id FROM library_item WHERE item_key = ?1 AND kind = 'media'
             ) WHERE item_id = ?2",
            params![cover_key, item_id],
        )?;
    }
    write_field_clock(transaction, "item", &item.item_key, "exists", mutation)?;
    Ok(ApplyOutcome::Applied(Some(item_id)))
}

fn next_revision(transaction: &Transaction<'_>) -> rusqlite::Result<i64> {
    transaction.query_row(
        "SELECT revision + 1 FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
}

fn membership_id(value: i64) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

/// Set or clear one root in a canonical membership bitmap domain.
fn update_membership_bitmap(
    transaction: &Transaction<'_>,
    domain: crate::canonical_bitmap::BitmapDomain,
    key_id: i64,
    item_id: i64,
    present: bool,
) -> rusqlite::Result<()> {
    let mut members = crate::canonical_bitmap::load_bitmap(transaction, domain, key_id)?;
    let item = membership_id(item_id)?;
    let changed = if present {
        members.insert(item)
    } else {
        members.remove(item)
    };
    if !changed {
        return Ok(());
    }
    crate::canonical_bitmap::replace_bitmap(
        transaction,
        domain,
        key_id,
        next_revision(transaction)?,
        &members,
    )
}

/// Splice one member into (or out of) a canonical group order and its
/// membership bitmap. `position` is the target vector index.
fn splice_group_member(
    transaction: &Transaction<'_>,
    collection_id: i64,
    media_id: i64,
    position: Option<usize>,
    present: bool,
) -> rusqlite::Result<bool> {
    let mut order = crate::canonical_bitmap::load_order(transaction, "group", collection_id)?
        .unwrap_or_default();
    let media = membership_id(media_id)?;
    let existing = order.iter().position(|entry| *entry == media);
    if !present {
        let Some(index) = existing else {
            return Ok(false);
        };
        order.remove(index);
    } else {
        if let Some(index) = existing {
            order.remove(index);
        }
        let index = position
            .unwrap_or(order.len())
            .min(order.len());
        order.insert(index, media);
    }
    crate::canonical_bitmap::replace_ordered_membership(
        transaction,
        "group",
        collection_id,
        next_revision(transaction)?,
        &order,
    )?;
    Ok(true)
}

/// The group that canonically owns a media item, if any.
fn owning_group(transaction: &Transaction<'_>, media_id: i64) -> rusqlite::Result<Option<i64>> {
    let media = membership_id(media_id)?;
    for (group_id, members) in crate::canonical_bitmap::load_domain(
        transaction,
        crate::canonical_bitmap::BitmapDomain::GroupMember,
    )? {
        if members.contains(media) {
            return Ok(Some(group_id));
        }
    }
    Ok(None)
}

fn scalar_wins(
    transaction: &Transaction<'_>,
    object_kind: &str,
    object_key: &str,
    field: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<bool> {
    let existing = transaction
        .query_row(
            "SELECT hlc_physical_ms, hlc_logical, device_id FROM cloud_field_clock
             WHERE object_kind = ?1 AND object_key = ?2 AND field_name = ?3",
            params![object_kind, object_key, field],
            |row| {
                Ok((
                    HybridTimestamp {
                        physical_ms: row.get::<_, i64>(0)? as u64,
                        logical: row.get::<_, i64>(1)? as u32,
                    },
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(existing.is_none_or(|(timestamp, device)| {
        (mutation.timestamp, mutation.device_id.as_str()) > (timestamp, device.as_str())
    }))
}

fn field_clock_exists(
    transaction: &Transaction<'_>,
    object_kind: &str,
    object_key: &str,
    field: &str,
) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM cloud_field_clock
             WHERE object_kind = ?1 AND object_key = ?2 AND field_name = ?3
         )",
        params![object_kind, object_key, field],
        |row| row.get(0),
    )
}

fn membership_wins(
    transaction: &Transaction<'_>,
    relation: &str,
    owner: &str,
    member: &str,
    incoming_present: bool,
    mutation: &CloudMutation,
) -> rusqlite::Result<bool> {
    let existing = transaction
        .query_row(
            "SELECT present, hlc_physical_ms, hlc_logical, device_id, causal_frontier_json
             FROM cloud_membership_clock
             WHERE relation_kind = ?1 AND owner_key = ?2 AND member_key = ?3",
            params![relation, owner, member],
            |row| {
                let frontier_json: String = row.get(4)?;
                let frontier: CausalFrontier =
                    serde_json::from_str(&frontier_json).map_err(json_sql_error)?;
                Ok((
                    row.get::<_, i64>(0)? != 0,
                    HybridTimestamp {
                        physical_ms: row.get::<_, i64>(1)? as u64,
                        logical: row.get::<_, i64>(2)? as u32,
                    },
                    row.get::<_, String>(3)?,
                    frontier,
                ))
            },
        )
        .optional()?;
    let Some((existing_present, existing_ts, existing_device, existing_frontier)) = existing else {
        return Ok(true);
    };
    let incoming_after = observed(
        &mutation.causal_frontier,
        &existing_device,
        existing_ts,
        &mutation.device_id,
        mutation.timestamp,
    );
    let existing_after = observed(
        &existing_frontier,
        &mutation.device_id,
        mutation.timestamp,
        &existing_device,
        existing_ts,
    );
    if incoming_after {
        return Ok(true);
    }
    if existing_after {
        return Ok(false);
    }
    if existing_present != incoming_present {
        return Ok(!incoming_present);
    }
    Ok((mutation.timestamp, mutation.device_id.as_str()) > (existing_ts, existing_device.as_str()))
}

fn observed(
    frontier: &CausalFrontier,
    prior_device: &str,
    prior_timestamp: HybridTimestamp,
    current_device: &str,
    current_timestamp: HybridTimestamp,
) -> bool {
    (prior_device == current_device && current_timestamp > prior_timestamp)
        || frontier
            .get(prior_device)
            .is_some_and(|seen| *seen >= prior_timestamp)
}

fn write_field_clock(
    transaction: &Transaction<'_>,
    object_kind: &str,
    object_key: &str,
    field: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO cloud_field_clock
             (object_kind, object_key, field_name, hlc_physical_ms, hlc_logical,
              device_id, mutation_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(object_kind, object_key, field_name) DO UPDATE SET
             hlc_physical_ms=excluded.hlc_physical_ms,
             hlc_logical=excluded.hlc_logical,
             device_id=excluded.device_id,
             mutation_id=excluded.mutation_id",
        params![
            object_kind,
            object_key,
            field,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.device_id,
            mutation.mutation_id,
        ],
    )?;
    Ok(())
}

fn write_membership_clock(
    transaction: &Transaction<'_>,
    relation: &str,
    owner: &str,
    member: &str,
    present: bool,
    mutation: &CloudMutation,
) -> rusqlite::Result<()> {
    // An initial add needs no conflict row: the relation itself is authoritative.
    // Once removed, retain the clock even after a later explicit re-add so an
    // out-of-order stale removal cannot undo the observed restoration.
    if present {
        let has_removal_history = transaction
            .query_row(
                "SELECT 1 FROM cloud_membership_clock
                 WHERE relation_kind = ?1 AND owner_key = ?2 AND member_key = ?3",
                params![relation, owner, member],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !has_removal_history {
            return Ok(());
        }
    }
    transaction.execute(
        "INSERT INTO cloud_membership_clock
             (relation_kind, owner_key, member_key, present, hlc_physical_ms, hlc_logical,
              device_id, mutation_id, causal_frontier_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(relation_kind, owner_key, member_key) DO UPDATE SET
             present=excluded.present,
             hlc_physical_ms=excluded.hlc_physical_ms,
             hlc_logical=excluded.hlc_logical,
             device_id=excluded.device_id,
             mutation_id=excluded.mutation_id,
             causal_frontier_json=excluded.causal_frontier_json",
        params![
            relation,
            owner,
            member,
            i64::from(present),
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.device_id,
            mutation.mutation_id,
            serde_json::to_string(&mutation.causal_frontier).map_err(json_sql_error)?,
        ],
    )?;
    Ok(())
}

fn read_frontier(transaction: &Transaction<'_>) -> rusqlite::Result<CausalFrontier> {
    let mut statement = transaction
        .prepare("SELECT device_id, hlc_physical_ms, hlc_logical FROM cloud_device_frontier")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            HybridTimestamp {
                physical_ms: row.get::<_, i64>(1)? as u64,
                logical: row.get::<_, i64>(2)? as u32,
            },
        ))
    })?;
    rows.collect()
}

fn advance_frontier(
    transaction: &Transaction<'_>,
    device_id: &str,
    timestamp: HybridTimestamp,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO cloud_device_frontier (device_id, hlc_physical_ms, hlc_logical, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(device_id) DO UPDATE SET
             hlc_physical_ms = CASE
                 WHEN (excluded.hlc_physical_ms, excluded.hlc_logical)
                      > (hlc_physical_ms, hlc_logical)
                 THEN excluded.hlc_physical_ms ELSE hlc_physical_ms END,
             hlc_logical = CASE
                 WHEN (excluded.hlc_physical_ms, excluded.hlc_logical)
                      > (hlc_physical_ms, hlc_logical)
                 THEN excluded.hlc_logical ELSE hlc_logical END,
             updated_at = excluded.updated_at",
        params![
            device_id,
            timestamp.physical_ms as i64,
            timestamp.logical as i64,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn item_id(transaction: &Transaction<'_>, item_key: &str) -> rusqlite::Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT item_id FROM library_item WHERE item_key = ?1",
            [item_key],
            |row| row.get(0),
        )
        .optional()
}

fn tombstone_id(
    transaction: &Transaction<'_>,
    object_kind: &str,
    object_key: &str,
) -> rusqlite::Result<Option<String>> {
    transaction
        .query_row(
            "SELECT mutation_id FROM cloud_tombstone WHERE object_kind = ?1 AND object_key = ?2",
            params![object_kind, object_key],
            |row| row.get(0),
        )
        .optional()
}

fn write_tombstone(
    transaction: &Transaction<'_>,
    object_kind: &str,
    object_key: &str,
    mutation: &CloudMutation,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO cloud_tombstone
             (object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
              device_id, causal_frontier_json, deleted_at, purge_after)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(object_kind, object_key) DO UPDATE SET
             mutation_id=excluded.mutation_id,
             hlc_physical_ms=excluded.hlc_physical_ms,
             hlc_logical=excluded.hlc_logical,
             device_id=excluded.device_id,
             causal_frontier_json=excluded.causal_frontier_json,
             deleted_at=excluded.deleted_at,
             purge_after=excluded.purge_after",
        params![
            object_kind,
            object_key,
            mutation.mutation_id,
            mutation.timestamp.physical_ms as i64,
            mutation.timestamp.logical as i64,
            mutation.device_id,
            serde_json::to_string(&mutation.causal_frontier).map_err(json_sql_error)?,
            Utc::now().to_rfc3339(),
            (Utc::now() + chrono::Duration::days(30)).to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn quarantine(
    transaction: &Transaction<'_>,
    mutation: &CloudMutation,
    reason: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO cloud_quarantine (mutation_id, reason, envelope_json, created_at)
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT(mutation_id) DO NOTHING",
        params![
            mutation.mutation_id,
            reason,
            serde_json::to_string(mutation).map_err(json_sql_error)?,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn checksum(mutation: &CloudMutation) -> Result<String, serde_json::Error> {
    let mut unsigned = mutation.clone();
    unsigned.checksum.clear();
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&unsigned)?)))
}

fn json_optional_string(value: &Value) -> rusqlite::Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.clone())),
        _ => Err(rusqlite::Error::InvalidParameterName(
            "cloud field must be a string or null".to_string(),
        )),
    }
}

fn json_optional_i64(value: &Value) -> rusqlite::Result<Option<i64>> {
    match value {
        Value::Null => Ok(None),
        Value::Number(value) => value.as_i64().map(Some).ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("cloud field must be an integer or null".into())
        }),
        _ => Err(rusqlite::Error::InvalidParameterName(
            "cloud field must be an integer or null".into(),
        )),
    }
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::sync::Arc;

    fn application() -> Application {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.keep();
        Application::new(Arc::new(Store::open(&path).unwrap()))
    }

    #[test]
    fn configure_stores_the_writable_google_drive_content_root() {
        let application = application();
        let account_root = tempfile::tempdir().unwrap();
        let my_drive = account_root.path().join("My Drive");
        std::fs::create_dir(&my_drive).unwrap();

        configure(
            &application,
            &ConfigureCloudInput {
                provider: "google_drive".into(),
                account_label: "person@example.com".into(),
                root_path: account_root.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();

        assert_eq!(
            configuration(&application).unwrap().root_path.as_deref(),
            Some(my_drive.to_string_lossy().as_ref())
        );
        assert!(my_drive.join("picto").is_dir());
    }

    fn add_media(application: &Application, key: &str) -> i64 {
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file (file_hash, mime_type, size_bytes, created_at)
                     VALUES (?1, 'image/png', 10, 'now')",
                    [format!("hash-{key}")],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES (?1, 'media', 'now', 'now')",
                    [key],
                )?;
                let item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (?1, ?2, 'now', 'now')",
                    params![item_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                    [item_id],
                )?;
                transaction.execute(
                    "INSERT INTO root_metadata
                         (root_item_id, name, source_urls_json, updated_at)
                     VALUES (?1, ?2, '[]', 'now')",
                    params![item_id, key],
                )?;
                Ok(item_id)
            })
            .unwrap()
            .0
    }

    fn remote_mutation(
        application: &Application,
        device: &str,
        timestamp: HybridTimestamp,
        frontier: CausalFrontier,
        operation: CloudOperation,
    ) -> CloudMutation {
        let library_id = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT library_id FROM cloud_state", [], |row| row.get(0))
            })
            .unwrap();
        let mut mutation = CloudMutation {
            mutation_id: uuid::Uuid::new_v4().to_string(),
            library_id,
            device_id: device.to_string(),
            timestamp,
            causal_frontier: frontier,
            operation,
            schema_generation: CLOUD_SCHEMA_GENERATION,
            checksum: String::new(),
        };
        mutation.checksum = checksum(&mutation).unwrap();
        mutation
    }

    fn enable_capture(application: &Application) {
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET provider = 'dropbox' WHERE singleton = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn latest_operation(application: &Application) -> CloudOperation {
        application
            .store()
            .read(|connection| {
                let payload: String = connection.query_row(
                    "SELECT payload_json FROM cloud_outbox
                     ORDER BY hlc_physical_ms DESC, hlc_logical DESC, mutation_id DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )?;
                serde_json::from_str(&payload).map_err(json_sql_error)
            })
            .unwrap()
    }

    #[test]
    fn local_transaction_records_one_semantic_batch_and_remote_apply_does_not_echo() {
        let application = application();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET provider = 'dropbox' WHERE singleton = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        add_media(&application, "item-a");
        let (count, payload): (i64, String) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*), payload_json FROM cloud_outbox
                     WHERE published_at IS NULL",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(count, 1);
        let operation: CloudOperation = serde_json::from_str(&payload).unwrap();
        let CloudOperation::Batch { operations } = operation else {
            panic!("local transaction must produce one semantic batch");
        };
        assert!(operations.iter().any(|operation| matches!(
            operation,
            CloudOperation::UpsertItem { item }
                if item.item_key == "item-a" && item.lifecycle == "active"
        )));
        assert_eq!(
            application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM cloud_field_clock
                     WHERE object_kind = 'item' AND object_key = 'item-a'",
                    [],
                    |row| row.get::<_, i64>(0),
                ))
                .unwrap(),
            1,
            "creation needs one baseline clock, not one row per initial field",
        );

        let remote = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::ItemFields {
                item_key: "item-a".into(),
                fields: BTreeMap::from([("name".into(), Value::String("remote name".into()))]),
            },
        );
        apply_downloaded(&application, &[remote]).unwrap();
        let count_after = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM cloud_outbox WHERE published_at IS NULL",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(count_after, 1, "remote replay must not create an echo");
    }

    #[test]
    fn one_batch_can_tombstone_multiple_objects() {
        let application = application();
        enable_capture(&application);

        let mutation = application
            .store()
            .transaction(|transaction| {
                record_local(
                    transaction,
                    CloudOperation::Batch {
                        operations: vec![
                            CloudOperation::DeleteItem {
                                item_key: "item-a".into(),
                            },
                            CloudOperation::DeleteItem {
                                item_key: "item-b".into(),
                            },
                        ],
                    },
                )
            })
            .unwrap()
            .0;

        let tombstones = application
            .store()
            .read(|connection| {
                connection
                    .prepare(
                        "SELECT object_key, mutation_id FROM cloud_tombstone
                         WHERE object_kind = 'item' ORDER BY object_key",
                    )?
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();

        assert_eq!(
            tombstones,
            vec![
                ("item-a".to_string(), mutation.mutation_id.clone()),
                ("item-b".to_string(), mutation.mutation_id),
            ]
        );
    }

    #[test]
    fn broad_local_batch_stamps_clocks_and_tombstones_set_wise() {
        let application = application();
        enable_capture(&application);
        let mut operations = Vec::with_capacity(11_000);
        operations.extend((0..1_000).map(|index| CloudOperation::GroupAssignment {
            media_item_key: format!("member-{index:04}"),
            collection_item_key: Some("group-a".into()),
            position_rank: Some(index * 1024),
        }));
        operations.extend((0..10_000).map(|index| CloudOperation::DeleteItem {
            item_key: format!("deleted-{index:05}"),
        }));

        let mutation = application
            .store()
            .transaction(|transaction| {
                record_local(transaction, CloudOperation::Batch { operations })
            })
            .unwrap()
            .0;

        let (outbox, clocks, tombstones, distinct_tombstone_mutations) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM cloud_outbox
                          WHERE mutation_id = ?1),
                         (SELECT COUNT(*) FROM cloud_field_clock
                          WHERE mutation_id = ?1),
                         (SELECT COUNT(*) FROM cloud_tombstone
                          WHERE mutation_id = ?1),
                         (SELECT COUNT(DISTINCT mutation_id) FROM cloud_tombstone
                          WHERE mutation_id = ?1)",
                    [&mutation.mutation_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
            })
            .unwrap();
        assert_eq!(outbox, 1);
        assert_eq!(clocks, 11_000);
        assert_eq!(tombstones, 10_000);
        assert_eq!(distinct_tombstone_mutations, 1);
    }

    #[test]
    fn local_batch_retains_remove_wins_history_after_observed_readd() {
        let application = application();
        enable_capture(&application);
        application
            .store()
            .transaction(|transaction| {
                record_local(
                    transaction,
                    CloudOperation::Batch {
                        operations: vec![
                            CloudOperation::TagMembership {
                                item_key: "item-a".into(),
                                namespace: "general".into(),
                                subtag: "blue".into(),
                                present: false,
                            },
                            CloudOperation::TagMembership {
                                item_key: "item-a".into(),
                                namespace: "general".into(),
                                subtag: "blue".into(),
                                present: true,
                            },
                        ],
                    },
                )
            })
            .unwrap();

        let present = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT present FROM cloud_membership_clock
                     WHERE relation_kind = 'tag' AND owner_key = 'item-a'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(present, 1);
    }

    #[test]
    fn metadata_update_captures_unchanged_item_identity() {
        let application = application();
        let item_id = add_media(&application, "rated-item");
        enable_capture(&application);

        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE root_metadata SET rating = 4, updated_at = 'later'
                     WHERE root_item_id = ?1",
                    [item_id],
                )?;
                Ok(())
            })
            .unwrap();

        let CloudOperation::Batch { operations } = latest_operation(&application) else {
            panic!("metadata capture must produce one semantic batch");
        };
        assert!(operations.iter().any(|operation| matches!(
            operation,
            CloudOperation::ItemFields { item_key, fields }
                if item_key == "rated-item" && fields.get("rating") == Some(&Value::from(4))
        )));
    }

    #[test]
    fn remote_item_fields_update_canonical_root_metadata() {
        let application = application();
        add_media(&application, "metadata-item");
        let mutation = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::ItemFields {
                item_key: "metadata-item".into(),
                fields: BTreeMap::from([
                    ("label".into(), Value::String("Root name".into())),
                    ("name".into(), Value::String("Media name".into())),
                    ("notes".into(), Value::String("Root notes".into())),
                    ("rating".into(), Value::from(5)),
                    ("source_urls_json".into(), Value::String("[]".into())),
                    ("captured_at".into(), Value::String("captured".into())),
                ]),
            },
        );

        apply_downloaded(&application, &[mutation]).unwrap();
        let (root_name, media_name, notes, rating, source_urls, captured_at) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT rm.name, ma.name, rm.notes, rm.rating, rm.source_urls_json,
                            ma.captured_at
                     FROM library_item li
                     JOIN root_metadata rm ON rm.root_item_id = li.item_id
                     JOIN media_asset ma ON ma.item_id = li.item_id
                     WHERE li.item_key = 'metadata-item'",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                        ))
                    },
                )
            })
            .unwrap();
        assert_eq!(root_name.as_deref(), Some("Root name"));
        assert_eq!(media_name.as_deref(), Some("Media name"));
        assert_eq!(notes.as_deref(), Some("Root notes"));
        assert_eq!(rating, Some(5));
        assert_eq!(source_urls, "[]");
        assert_eq!(captured_at.as_deref(), Some("captured"));
    }

    #[test]
    fn remote_item_lifecycle_tag_and_folder_changes_settle_incrementally() {
        let application = application();
        let item_id = add_media(&application, "item-a");
        let unrelated_id = add_media(&application, "unrelated");
        assert!(application.projections().active_bitmap().is_empty());

        let setup = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::Batch {
                operations: vec![
                    CloudOperation::UpsertFolder {
                        folder: CloudFolder {
                            folder_key: "folder-a".into(),
                            name: "Folder A".into(),
                            parent_key: None,
                            icon: None,
                            color: None,
                            notes: None,
                            sort_rank: None,
                            created_at: "now".into(),
                            updated_at: "now".into(),
                        },
                        changed_fields: vec!["exists".into(), "name".into()],
                    },
                    CloudOperation::FolderMembership {
                        item_key: "item-a".into(),
                        folder_key: "folder-a".into(),
                        present: true,
                        position_rank: Some(1024),
                    },
                    CloudOperation::TagMembership {
                        item_key: "item-a".into(),
                        namespace: "general".into(),
                        subtag: "blue".into(),
                        present: true,
                    },
                    CloudOperation::ItemFields {
                        item_key: "item-a".into(),
                        fields: BTreeMap::from([
                            ("label".into(), Value::String("Remote name".into())),
                            ("rating".into(), Value::from(4)),
                        ]),
                    },
                    CloudOperation::Lifecycle {
                        item_key: "missing-item".into(),
                        lifecycle: "active".into(),
                    },
                ],
            },
        );
        let (summary, receipt) = apply_downloaded(&application, &[setup]).unwrap();
        assert_eq!(summary.applied, 1);
        assert_eq!(summary.quarantined, 1);
        assert_eq!(receipt.item_ids, vec![ItemId(item_id)]);

        let (folder_id, tag_id): (i64, i64) = application
            .store()
            .read(|connection| {
                Ok((
                    connection.query_row(
                        "SELECT folder_id FROM folder WHERE folder_key = 'folder-a'",
                        [],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT tag_id FROM tag
                         WHERE namespace = 'general' AND subtag = 'blue'",
                        [],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert!(application
            .projections()
            .active_bitmap()
            .contains(item_id as u32));
        assert!(application
            .projections()
            .folder_bitmap(folder_id)
            .contains(item_id as u32));
        assert!(application
            .projections()
            .direct_tag_bitmap(tag_id)
            .contains(item_id as u32));
        assert_eq!(
            application
                .projections()
                .rating_aggregate(&RoaringBitmap::from_iter([item_id as u32]))
                .sum,
            4
        );
        assert!(
            !application
                .projections()
                .active_bitmap()
                .contains(unrelated_id as u32),
            "incremental cloud settlement must not reconstruct unrelated SQL rows"
        );

        let lifecycle = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 20,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::Lifecycle {
                item_key: "item-a".into(),
                lifecycle: "trash".into(),
            },
        );
        apply_downloaded(&application, &[lifecycle]).unwrap();
        assert!(!application
            .projections()
            .active_bitmap()
            .contains(item_id as u32));
        assert!(application
            .projections()
            .trash_bitmap()
            .contains(item_id as u32));

        let delete = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 30,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::DeleteItem {
                item_key: "item-a".into(),
            },
        );
        apply_downloaded(&application, &[delete]).unwrap();
        assert!(!application
            .projections()
            .trash_bitmap()
            .contains(item_id as u32));
        assert!(!application
            .projections()
            .direct_tag_bitmap(tag_id)
            .contains(item_id as u32));
        assert!(!application
            .projections()
            .folder_bitmap(folder_id)
            .contains(item_id as u32));
    }

    #[test]
    fn captured_updates_read_stable_keys_across_synced_tables() {
        let application = application();
        let media_id = add_media(&application, "member-a");
        let (collection_id, folder_id, smart_folder_id, _tag_id) = application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES ('group-a', 'collection', 'now', 'now')",
                    [],
                )?;
                let collection_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                    [collection_id],
                )?;
                transaction.execute(
                    "INSERT INTO root_metadata
                         (root_item_id, name, source_urls_json, updated_at)
                     VALUES (?1, 'group-a', '[]', 'now')",
                    [collection_id],
                )?;
                transaction.execute("DELETE FROM library_root WHERE item_id = ?1", [media_id])?;
                transaction.execute(
                    "INSERT INTO folder (folder_key, name, created_at, updated_at)
                     VALUES ('folder-a', 'Folder', 'now', 'now')",
                    [],
                )?;
                let folder_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO smart_folder
                         (smart_folder_key, name, predicate_json, created_at, updated_at)
                     VALUES ('smart-a', 'Smart', '{\"groups\":[]}', 'now', 'now')",
                    [],
                )?;
                let smart_folder_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES ('general', 'blue')",
                    [],
                )?;
                let tag_id = transaction.last_insert_rowid();
                Ok((collection_id, folder_id, smart_folder_id, tag_id))
            })
            .unwrap()
            .0;
        enable_capture(&application);

        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE root_metadata SET name = 'Group', updated_at = 'later'
                     WHERE root_item_id = ?1",
                    [collection_id],
                )?;
                transaction.execute(
                    "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = ?1",
                    [collection_id],
                )?;
                transaction.execute(
                    "UPDATE root_metadata SET notes = 'note', updated_at = 'later'
                     WHERE root_item_id = ?1",
                    [collection_id],
                )?;
                transaction.execute(
                    "UPDATE folder SET name = 'Renamed', updated_at = 'later'
                     WHERE folder_id = ?1",
                    [folder_id],
                )?;
                transaction.execute(
                    "UPDATE smart_folder SET name = 'Renamed', updated_at = 'later'
                     WHERE smart_folder_id = ?1",
                    [smart_folder_id],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(matches!(
            latest_operation(&application),
            CloudOperation::Batch { operations } if !operations.is_empty()
        ));
    }

    #[test]
    fn concurrent_tag_removal_wins_and_later_observed_add_restores_it() {
        let application = application();
        add_media(&application, "item-a");
        let at = HybridTimestamp {
            physical_ms: 10,
            logical: 0,
        };
        let add = remote_mutation(
            &application,
            "a",
            at,
            CausalFrontier::new(),
            CloudOperation::TagMembership {
                item_key: "item-a".into(),
                namespace: "general".into(),
                subtag: "blue".into(),
                present: true,
            },
        );
        let remove = remote_mutation(
            &application,
            "b",
            at,
            CausalFrontier::new(),
            CloudOperation::TagMembership {
                item_key: "item-a".into(),
                namespace: "general".into(),
                subtag: "blue".into(),
                present: false,
            },
        );
        apply_downloaded(&application, &[add]).unwrap();
        assert_eq!(
            application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM cloud_membership_clock",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            0,
            "an ordinary present relation must not be duplicated in a clock row",
        );

        let blue_members = |application: &Application| -> u64 {
            application
                .store()
                .read(|connection| {
                    let tag_id: Option<i64> = connection
                        .query_row(
                            "SELECT tag_id FROM tag
                             WHERE namespace = 'general' AND subtag = 'blue'",
                            [],
                            |row| row.get(0),
                        )
                        .optional()?;
                    Ok(match tag_id {
                        Some(tag_id) => crate::canonical_bitmap::load_bitmap(
                            connection,
                            crate::canonical_bitmap::BitmapDomain::Tag,
                            tag_id,
                        )?
                        .len(),
                        None => 0,
                    })
                })
                .unwrap()
        };
        apply_downloaded(&application, &[remove]).unwrap();
        assert_eq!(blue_members(&application), 0);
        assert_eq!(
            application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT present FROM cloud_membership_clock",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            0,
            "a removal tombstone must retain its causal clock",
        );

        let mut frontier = CausalFrontier::new();
        frontier.insert("b".into(), at);
        let later_add = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 11,
                logical: 0,
            },
            frontier,
            CloudOperation::TagMembership {
                item_key: "item-a".into(),
                namespace: "general".into(),
                subtag: "blue".into(),
                present: true,
            },
        );
        apply_downloaded(&application, &[later_add]).unwrap();
        assert_eq!(blue_members(&application), 1);
        assert_eq!(
            application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT present FROM cloud_membership_clock",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            1,
            "an explicit restoration retains history against stale removals",
        );
    }

    #[test]
    fn tombstone_rejects_stale_edits_and_requires_explicit_restore() {
        let application = application();
        add_media(&application, "item-a");
        let delete = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 20,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::DeleteItem {
                item_key: "item-a".into(),
            },
        );
        let tombstone_id = delete.mutation_id.clone();
        apply_downloaded(&application, &[delete]).unwrap();
        let edit = remote_mutation(
            &application,
            "b",
            HybridTimestamp {
                physical_ms: 30,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::ItemFields {
                item_key: "item-a".into(),
                fields: BTreeMap::from([("name".into(), Value::String("stale".into()))]),
            },
        );
        let (summary, _) = apply_downloaded(&application, &[edit]).unwrap();
        assert_eq!(summary.ignored, 1);

        let restore = remote_mutation(
            &application,
            "b",
            HybridTimestamp {
                physical_ms: 31,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::RestoreItem {
                tombstone_mutation_id: tombstone_id,
                item: RestoredItem {
                    item_key: "item-a".into(),
                    kind: "media".into(),
                    label: None,
                    cover_media_item_key: None,
                    lifecycle: "active".into(),
                    media: Some(RestoredMedia {
                        file_hash: "hash-item-a".into(),
                        mime_type: "image/png".into(),
                        size_bytes: 10,
                        pixel_width: None,
                        pixel_height: None,
                        duration_ms: None,
                        frame_count: None,
                        has_audio: false,
                        name: Some("restored".into()),
                        notes: None,
                        rating: None,
                        source_urls_json: None,
                        captured_at: None,
                        imported_at: "now".into(),
                    }),
                },
            },
        );
        apply_downloaded(&application, &[restore]).unwrap();
        assert_eq!(
            application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_key = 'item-a'",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
    }

    #[test]
    fn duplicate_mutation_is_applied_once() {
        let application = application();
        add_media(&application, "item-a");
        let mutation = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::Lifecycle {
                item_key: "item-a".into(),
                lifecycle: "trash".into(),
            },
        );
        let (summary, _) = apply_downloaded(&application, &[mutation.clone(), mutation]).unwrap();
        assert_eq!(summary.applied, 1);
        assert_eq!(summary.duplicate, 1);
    }

    #[test]
    fn device_frontier_preserves_idempotency_after_exact_ids_are_pruned() {
        let application = application();
        add_media(&application, "item-a");
        let mutation = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::Lifecycle {
                item_key: "item-a".into(),
                lifecycle: "trash".into(),
            },
        );
        apply_downloaded(&application, std::slice::from_ref(&mutation)).unwrap();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute("DELETE FROM cloud_applied_mutation", [])?;
                Ok(())
            })
            .unwrap();

        let (summary, _) = apply_downloaded(&application, &[mutation]).unwrap();
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.duplicate, 1);
    }

    #[test]
    fn pausing_and_resuming_updates_the_reported_state() {
        let application = application();

        set_paused(&application, true).unwrap();
        assert_eq!(status(&application).unwrap().state, "paused");

        set_paused(&application, false).unwrap();
        let resumed = status(&application).unwrap();
        assert_eq!(resumed.state, "idle");
        assert_eq!(resumed.phase, "idle");
    }

    #[test]
    fn folder_hierarchy_membership_and_smart_folder_replicate_together() {
        let sender = application();
        enable_capture(&sender);
        let item_id = add_media(&sender, "item-a");
        let (child_id, _) = sender
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, watch_path, watch_enabled, created_at, updated_at)
                     VALUES ('folder-parent', 'Parent', '/device-only', 1, 'now', 'now')",
                    [],
                )?;
                let parent_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, parent_id, created_at, updated_at)
                     VALUES ('folder-child', 'Child', ?1, 'now', 'now')",
                    [parent_id],
                )?;
                let child_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO smart_folder
                         (smart_folder_key, name, predicate_json, created_at, updated_at)
                     VALUES ('smart-a', 'Smart', '{\"groups\":[]}', 'now', 'now')",
                    [],
                )?;
                Ok(child_id)
            })
            .unwrap();

        let structure_operation = latest_operation(&sender);
        let CloudOperation::Batch { operations } = &structure_operation else {
            panic!("capture must create one semantic batch");
        };
        let parent_index = operations
            .iter()
            .position(|operation| matches!(operation, CloudOperation::UpsertFolder { folder, .. } if folder.folder_key == "folder-parent"))
            .unwrap();
        let child_index = operations
            .iter()
            .position(|operation| matches!(operation, CloudOperation::UpsertFolder { folder, .. } if folder.folder_key == "folder-child"))
            .unwrap();
        assert!(parent_index < child_index);

        sender
            .set_folder_membership(
                &crate::app::ItemTarget::Explicit {
                    item_ids: vec![crate::app::ItemId(item_id)],
                },
                child_id,
                true,
            )
            .unwrap();
        let membership_operation = latest_operation(&sender);
        let CloudOperation::Batch {
            operations: membership_operations,
        } = &membership_operation
        else {
            panic!("canonical membership must record one semantic batch");
        };
        assert!(membership_operations.iter().any(|operation| matches!(
            operation,
            CloudOperation::FolderMembership { folder_key, item_key, present: true, .. }
                if folder_key == "folder-child" && item_key == "item-a"
        )));

        let receiver = application();
        add_media(&receiver, "item-a");
        enable_capture(&receiver);
        let structure_mutation = remote_mutation(
            &receiver,
            "sender",
            HybridTimestamp {
                physical_ms: 100,
                logical: 0,
            },
            CausalFrontier::new(),
            structure_operation,
        );
        let membership_mutation = remote_mutation(
            &receiver,
            "sender",
            HybridTimestamp {
                physical_ms: 101,
                logical: 0,
            },
            CausalFrontier::new(),
            membership_operation,
        );
        let (summary, receipt) =
            apply_downloaded(&receiver, &[structure_mutation, membership_mutation]).unwrap();
        assert_eq!(summary.quarantined, 0);
        let echoed: i64 = receiver
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM cloud_outbox", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(
            echoed, 0,
            "remote applies must not echo into the local outbox"
        );
        assert!(receipt
            .resources
            .contains(&resources::SMART_FOLDERS.to_string()));

        let (child_name, parent_name, watch_path, smart_count, member_count): (
            String,
            Option<String>,
            Option<String>,
            i64,
            u64,
        ) = receiver
            .store()
            .read(|connection| {
                let (child_name, parent_name, watch_path, folder_id, smart_count) = connection
                    .query_row(
                        "SELECT child.name, parent.name, parent.watch_path, child.folder_id,
                                (SELECT COUNT(*) FROM smart_folder
                                 WHERE smart_folder_key = 'smart-a')
                         FROM folder child
                         JOIN folder parent ON parent.folder_id = child.parent_id
                         WHERE child.folder_key = 'folder-child'",
                        [],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, Option<String>>(2)?,
                                row.get::<_, i64>(3)?,
                                row.get::<_, i64>(4)?,
                            ))
                        },
                    )?;
                let members = crate::canonical_bitmap::load_bitmap(
                    connection,
                    crate::canonical_bitmap::BitmapDomain::Folder,
                    folder_id,
                )?;
                Ok((child_name, parent_name, watch_path, smart_count, members.len()))
            })
            .unwrap();
        assert_eq!(child_name, "Child");
        assert_eq!(parent_name.as_deref(), Some("Parent"));
        assert_eq!(watch_path, None, "watch paths are device-local");
        assert_eq!(smart_count, 1);
        assert_eq!(member_count, 1);
    }

    #[test]
    fn folder_membership_apply_preserves_manual_order() {
        let receiver = application();
        let first = add_media(&receiver, "item-a");
        let second = add_media(&receiver, "item-b");
        let (folder_id, _) = receiver
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder (folder_key, name, created_at, updated_at)
                     VALUES ('folder-a', 'Folder', 'now', 'now')",
                    [],
                )?;
                let folder_id = transaction.last_insert_rowid();
                crate::canonical_bitmap::seed_test_state(
                    transaction,
                    &crate::canonical_bitmap::TestMembership {
                        folders: vec![(folder_id, vec![first as u32])],
                        ..Default::default()
                    },
                )?;
                Ok(folder_id)
            })
            .unwrap();
        receiver
            .store()
            .read_result(|connection| receiver.projections().reload(connection))
            .unwrap();

        let mutation = remote_mutation(
            &receiver,
            "sender",
            HybridTimestamp {
                physical_ms: 50,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::FolderMembership {
                item_key: "item-b".into(),
                folder_key: "folder-a".into(),
                present: true,
                position_rank: Some(0),
            },
        );
        let (summary, _) = apply_downloaded(&receiver, &[mutation]).unwrap();
        assert_eq!(summary.applied, 1);

        let order = receiver
            .store()
            .read(|connection| {
                crate::canonical_bitmap::load_order(connection, "folder", folder_id)
            })
            .unwrap()
            .expect("manual folder order must survive projection persistence");
        assert_eq!(order, vec![second as u32, first as u32]);
        assert_eq!(
            receiver
                .projections()
                .selection_snapshot()
                .folder_order(folder_id),
            Some(vec![second, first])
        );
    }

    #[test]
    fn deleted_folder_tombstone_blocks_stale_recreation() {
        let application = application();
        let upsert = |name: &str| CloudOperation::UpsertFolder {
            folder: CloudFolder {
                folder_key: "folder-a".into(),
                name: name.into(),
                parent_key: None,
                icon: None,
                color: None,
                notes: None,
                sort_rank: None,
                created_at: "now".into(),
                updated_at: "now".into(),
            },
            changed_fields: vec![
                "exists".into(),
                "name".into(),
                "parent".into(),
                "icon".into(),
                "color".into(),
                "notes".into(),
                "sort_rank".into(),
            ],
        };
        let create = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            upsert("Original"),
        );
        let delete = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 20,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::DeleteFolder {
                folder_key: "folder-a".into(),
            },
        );
        let stale_recreate = remote_mutation(
            &application,
            "b",
            HybridTimestamp {
                physical_ms: 30,
                logical: 0,
            },
            CausalFrontier::new(),
            upsert("Stale"),
        );
        let (summary, _) =
            apply_downloaded(&application, &[create, delete, stale_recreate]).unwrap();
        assert_eq!(summary.ignored, 1);
        let counts: (i64, i64) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM folder WHERE folder_key = 'folder-a'),
                         (SELECT COUNT(*) FROM cloud_tombstone
                          WHERE object_kind = 'folder' AND object_key = 'folder-a')",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(counts, (0, 1));
    }

    #[test]
    fn folder_fields_merge_independently_and_local_edits_capture_exact_fields() {
        let application = application();
        enable_capture(&application);
        let (folder_id, _) = application
            .create_folder(&crate::folders_v2::CreateFolderInput {
                name: "Original".into(),
                parent_id: None,
                folder_key: Some("folder-a".into()),
            })
            .unwrap();
        application
            .rename_folder(folder_id, "Local rename")
            .unwrap();
        let operation = latest_operation(&application);
        let CloudOperation::Batch { operations } = operation else {
            panic!("folder rename must produce one semantic batch");
        };
        assert!(matches!(
            operations.as_slice(),
            [CloudOperation::UpsertFolder { changed_fields, .. }]
                if changed_fields == &["name".to_string()]
        ));

        let rename = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 9_000_000_000_000,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertFolder {
                folder: CloudFolder {
                    folder_key: "folder-a".into(),
                    name: "Remote rename".into(),
                    parent_key: None,
                    icon: None,
                    color: None,
                    notes: None,
                    sort_rank: None,
                    created_at: "now".into(),
                    updated_at: "now".into(),
                },
                changed_fields: vec!["name".into()],
            },
        );
        let recolor = remote_mutation(
            &application,
            "b",
            HybridTimestamp {
                physical_ms: 9_000_000_000_000,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertFolder {
                folder: CloudFolder {
                    folder_key: "folder-a".into(),
                    name: "Ignored stale name".into(),
                    parent_key: None,
                    icon: None,
                    color: Some("blue".into()),
                    notes: None,
                    sort_rank: None,
                    created_at: "now".into(),
                    updated_at: "now".into(),
                },
                changed_fields: vec!["color".into()],
            },
        );
        apply_downloaded(&application, &[rename, recolor]).unwrap();
        let merged: (String, Option<String>) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT name, color FROM folder WHERE folder_key = 'folder-a'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(merged, ("Remote rename".into(), Some("blue".into())));
    }

    fn subscription_definition(key: &str, name: &str, schedule: &str) -> CloudSubscription {
        CloudSubscription {
            subscription_key: key.into(),
            name: name.into(),
            schedule: schedule.into(),
            paused: false,
            initial_post_limit: Some(100),
            periodic_post_limit: Some(100),
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn subscription_definitions_replicate_but_runtime_progress_does_not() {
        let sender = application();
        enable_capture(&sender);
        sender
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO subscription
                         (subscription_key, name, schedule, paused, initial_post_limit,
                          periodic_post_limit, created_at)
                     VALUES ('subscription-a', 'Artist', 'daily', 0, 100, 100,
                             '2026-01-01T00:00:00Z')",
                    [],
                )?;
                let subscription_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO subscription_query
                         (query_key, subscription_id, site_id, domain_key, query_kind,
                          query_text, display_name, notes, group_posts, paused)
                     VALUES ('query-a', ?1, 'pixiv', 'pixiv.net', 'user', '123',
                             'Artist', NULL, 1, 0)",
                    [subscription_id],
                )?;
                let query_id = transaction.last_insert_rowid();
                crate::cloud::capture::record_subscription_created(
                    transaction,
                    subscription_id,
                    &[query_id],
                )?;
                Ok(())
            })
            .unwrap();
        let operation = latest_operation(&sender);
        let CloudOperation::Batch { operations } = &operation else {
            panic!("subscription creation must produce one semantic batch");
        };
        let subscription_index = operations
            .iter()
            .position(|operation| {
                matches!(operation,
                CloudOperation::UpsertSubscription { subscription, .. }
                    if subscription.subscription_key == "subscription-a")
            })
            .unwrap();
        let query_index = operations
            .iter()
            .position(|operation| {
                matches!(operation,
                CloudOperation::UpsertSubscriptionQuery { query, .. }
                    if query.query_key == "query-a")
            })
            .unwrap();
        assert!(subscription_index < query_index);

        let receiver = application();
        let mutation = remote_mutation(
            &receiver,
            "sender",
            HybridTimestamp {
                physical_ms: 100,
                logical: 0,
            },
            CausalFrontier::new(),
            operation,
        );
        let (summary, receipt) = apply_downloaded(&receiver, &[mutation]).unwrap();
        assert_eq!(summary.quarantined, 0);
        assert!(receipt
            .resources
            .contains(&resources::SUBSCRIPTIONS.to_string()));
        let replicated: (String, String, String, bool) = receiver
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT s.name, s.schedule, q.query_text, q.group_posts
                     FROM subscription s JOIN subscription_query q
                       ON q.subscription_id = s.subscription_id
                     WHERE s.subscription_key = 'subscription-a' AND q.query_key = 'query-a'",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get::<_, i64>(3)? != 0,
                        ))
                    },
                )
            })
            .unwrap();
        assert_eq!(
            replicated,
            ("Artist".into(), "daily".into(), "123".into(), true)
        );

        let outbox_before: i64 = sender
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM cloud_outbox", [], |row| row.get(0))
            })
            .unwrap();
        sender
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE subscription_query
                     SET resume_cursor = 'opaque', last_success_at = '2026-01-02T00:00:00Z'
                     WHERE query_key = 'query-a'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let outbox_after: i64 = sender
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM cloud_outbox", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(outbox_after, outbox_before);
    }

    #[test]
    fn subscription_fields_merge_independently_and_delete_blocks_stale_recreation() {
        let application = application();
        let create = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertSubscription {
                subscription: subscription_definition("subscription-a", "Original", "daily"),
                changed_fields: vec![
                    "name".into(),
                    "schedule".into(),
                    "paused".into(),
                    "initial_post_limit".into(),
                    "periodic_post_limit".into(),
                ],
            },
        );
        apply_downloaded(&application, &[create]).unwrap();

        let rename = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 20,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertSubscription {
                subscription: subscription_definition("subscription-a", "Renamed", "daily"),
                changed_fields: vec!["name".into()],
            },
        );
        let reschedule = remote_mutation(
            &application,
            "b",
            HybridTimestamp {
                physical_ms: 20,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertSubscription {
                subscription: subscription_definition("subscription-a", "Original", "weekly"),
                changed_fields: vec!["schedule".into()],
            },
        );
        apply_downloaded(&application, &[rename, reschedule]).unwrap();
        let merged: (String, String) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT name, schedule FROM subscription
                     WHERE subscription_key = 'subscription-a'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(merged, ("Renamed".into(), "weekly".into()));

        let delete = remote_mutation(
            &application,
            "a",
            HybridTimestamp {
                physical_ms: 30,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::DeleteSubscription {
                subscription_key: "subscription-a".into(),
            },
        );
        let stale = remote_mutation(
            &application,
            "b",
            HybridTimestamp {
                physical_ms: 40,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertSubscription {
                subscription: subscription_definition("subscription-a", "Stale", "monthly"),
                changed_fields: vec!["name".into(), "schedule".into()],
            },
        );
        let (summary, _) = apply_downloaded(&application, &[delete, stale]).unwrap();
        assert_eq!(summary.ignored, 1);
        let count: i64 = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM subscription WHERE subscription_key = 'subscription-a'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn source_provenance_replicates_and_tombstones_block_reingest() {
        let application = application();
        let subscription = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 10,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::Batch {
                operations: vec![
                    CloudOperation::UpsertSubscription {
                        subscription: subscription_definition("subscription-a", "Artist", "daily"),
                        changed_fields: vec!["name".into(), "schedule".into()],
                    },
                    CloudOperation::UpsertSubscriptionQuery {
                        query: CloudSubscriptionQuery {
                            query_key: "query-a".into(),
                            subscription_key: "subscription-a".into(),
                            site_id: "example".into(),
                            domain_key: "example.test".into(),
                            query_kind: "user".into(),
                            query_text: "artist".into(),
                            display_name: None,
                            notes: None,
                            group_posts: true,
                            paused: false,
                        },
                        changed_fields: [
                            "subscription",
                            "site_id",
                            "domain_key",
                            "query_kind",
                            "query_text",
                            "display_name",
                            "notes",
                            "group_posts",
                            "paused",
                        ]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    },
                ],
            },
        );
        apply_downloaded(&application, &[subscription]).unwrap();

        let post_key = source_post_sync_key("example", "post-1");
        let item_key = source_item_sync_key("example", "post-1", "image-1");
        let source_item = CloudSourceItem {
            source_item_key: item_key.clone(),
            source_post_key: post_key.clone(),
            item_key: "image-1".into(),
            position: 0,
            media_url: Some("https://example.test/image.png".into()),
            canonical_url: None,
            media_item_key: None,
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        let provenance = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 11,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::Batch {
                operations: vec![
                    CloudOperation::UpsertSourcePost {
                        source_post: CloudSourcePost {
                            source_post_key: post_key.clone(),
                            site_id: "example".into(),
                            post_key: "post-1".into(),
                            canonical_url: Some("https://example.test/post/1".into()),
                            creator_name: Some("Artist".into()),
                            title: None,
                            description: None,
                            captured_at: None,
                            metadata_json: None,
                            root_item_key: None,
                            created_at: "now".into(),
                            updated_at: "now".into(),
                        },
                        changed_fields: vec![
                            "exists".into(),
                            "canonical_url".into(),
                            "creator_name".into(),
                        ],
                    },
                    CloudOperation::UpsertSourceItem {
                        source_item: source_item.clone(),
                        changed_fields: vec![
                            "exists".into(),
                            "position".into(),
                            "media_url".into(),
                        ],
                    },
                    CloudOperation::SubscriptionSourcePost {
                        subscription_key: "subscription-a".into(),
                        query_key: "query-a".into(),
                        source_post_key: post_key,
                        present: true,
                    },
                ],
            },
        );
        apply_downloaded(&application, &[provenance]).unwrap();
        let persisted: (String, i64, String, i64) = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT sp.creator_name, si.position, si.state,
                            (SELECT COUNT(*) FROM subscription_source_post)
                     FROM source_post sp JOIN source_item si USING (source_post_id)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            })
            .unwrap();
        assert_eq!(persisted, ("Artist".into(), 0, "pending".into(), 1));

        let deletion = remote_mutation(
            &application,
            "remote",
            HybridTimestamp {
                physical_ms: 12,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::DeleteSourceItem {
                source_item_key: item_key,
            },
        );
        let stale = remote_mutation(
            &application,
            "other",
            HybridTimestamp {
                physical_ms: 13,
                logical: 0,
            },
            CausalFrontier::new(),
            CloudOperation::UpsertSourceItem {
                source_item,
                changed_fields: vec!["media_url".into()],
            },
        );
        let (summary, _) = apply_downloaded(&application, &[deletion, stale]).unwrap();
        assert_eq!(summary.ignored, 1);
        let state: String = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT state FROM source_item", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(state, "deleted");
    }
}
