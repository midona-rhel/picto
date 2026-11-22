//! Cloud sync. A library writes its data into the folder of a Google Drive
//! or Dropbox desktop app, below `picto/<library id>/`:
//!
//! - `library.json`, which names the library. It is written once.
//! - `blobs/f/`, the original files, each under its content hash.
//! - `snapshots/`, compressed copies of the whole database, each with a
//!   manifest (`snapshot.rs`). A device writes a new one when the library
//!   has changed and has been idle for 30 seconds.
//! - `entries/<device id>/`, the entries that the device made: what it
//!   imported, deleted, moved into or out of the Trash, or added again
//!   (`entries.rs`).
//!
//! Another device gets the library by taking the newest snapshot as its
//! database (Open from Cloud…) and copying the originals. After that, the
//! devices share the entries by themselves: an item or a collection that
//! was imported, with its file and what it was given at the import, an item
//! that was deleted permanently, moved to the Trash or restored, a deleted
//! tag, folder or smart folder, and the removal of a tombstone. A change to
//! an item that the devices already hold, other than those, reaches another
//! device only when a snapshot is restored there.
//!
//! A library records the items that it imports from its first sync on
//! (`picto_library::records_items`). What it held before is in its first
//! snapshot. So a copy of a library that has never synced cannot be
//! connected to a cloud folder that already holds the same library
//! ([`configure_library`]): nothing would tell the devices what each of the
//! two copies holds that the other does not.
//!
//! This file holds the configuration and the state of the sync
//! (`cloud_state`), the commands that switch it on and off, and the removal
//! of a library's cloud data.

use std::collections::BTreeMap;

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::library_application::LibraryApplication;

pub mod provider;
pub mod snapshot;
pub mod entries;

pub const CLOUD_SCHEMA_GENERATION: i64 = 1;

/// The message that a library shows after it has found that its cloud data
/// is gone and has switched its sync off.
pub const CLOUD_DATA_REMOVED: &str = "The cloud data of this library was deleted. \
    Cloud sync is now off on this device. Nothing was deleted from this library.";

/// Held by everything that reads or writes a library's cloud data: a sync,
/// the merge of the other devices' entries, and the removal of the cloud
/// data. So the cloud data is never removed while a sync writes into it.
static SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS,
)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct HybridTimestamp {
    #[ts(type = "number")]
    pub physical_ms: u64,
    pub logical: u32,
}

pub type CausalFrontier = BTreeMap<String, HybridTimestamp>;
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
pub fn status_library(application: &LibraryApplication) -> Result<CloudSyncStatus, String> {
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(connection.query_row(
                    "SELECT state, phase, blocking, completed_units, total_units,
                            message, last_sync_at,
                            (SELECT COUNT(*) FROM cloud_journal
                             WHERE expanded_at_ms IS NULL),
                            (SELECT COUNT(*) FROM cloud_blob_state
                             WHERE state IN ('queued', 'downloading')
                                OR (state = 'available' AND remote_present = 0)),
                            (SELECT COUNT(*) FROM cloud_blob_state
                             WHERE state IN ('missing_remote', 'corrupt'))
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
                )?)
            },
        )
        .map_err(|error| error.to_string())
}

pub fn configuration_library(
    application: &LibraryApplication,
) -> Result<CloudConfiguration, String> {
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                connection
                    .query_row(
                        "SELECT provider, account_label, remote_root, library_id,
                                device_id, retention_json
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
                                retention: serde_json::from_str(&retention)
                                    .map_err(json_sql_error)?,
                            })
                        },
                    )
                    .map_err(Into::into)
            },
        )
        .map_err(|error| error.to_string())
}

/// The error of [`configure_library`] for a copy of a library that has
/// never synced and a cloud folder that already holds the same library.
pub const CLOUD_HOLDS_LIBRARY: &str = "This cloud folder already holds this library from     another computer, and this copy has never synced with it. Open the library on this     computer with Open from Cloud…. To bring in what only this copy holds, merge this     copy into the opened library with Merge Library….";

/// Connects the library to the cloud folder `input.root_path` of a provider.
///
/// When the folder is not the one that the library is connected to, the
/// library forgets what it wrote into and merged from the folder it synced
/// with before, and it records the items that it imports from now on
/// (`picto_library::reset_published_entries`). A library that has never
/// synced is refused with [`CLOUD_HOLDS_LIBRARY`] when the folder already
/// holds the same library.
pub fn configure_library(
    application: &LibraryApplication,
    input: &ConfigureCloudInput,
) -> Result<picto_library::MutationReceipt, String> {
    if !matches!(input.provider.as_str(), "google_drive" | "dropbox") {
        return Err(format!(
            "Unsupported cloud folder provider: {}",
            input.provider
        ));
    }
    let validated = provider::validate_root(&input.provider, &input.root_path)?;
    let root = std::path::PathBuf::from(validated.path);
    let provider = provider::DirectoryProvider::open_existing(&root)?;
    provider.verify_writable()?;
    let root_path = root.to_string_lossy().into_owned();
    let published = application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["cloud".to_string(), "tasks".to_string()],
            [],
            |transaction, _| {
                let (connected, library_id) = transaction.query_row(
                    "SELECT provider IS ?1 AND remote_root IS ?2, library_id
                     FROM cloud_state WHERE singleton = 1",
                    params![input.provider, root_path],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, String>(1)?)),
                )?;
                if !connected {
                    // The metadata of `library.json` is answered also while
                    // the provider's app has not downloaded the file.
                    let held = std::fs::symlink_metadata(
                        root.join("picto").join(&library_id).join("library.json"),
                    )
                    .is_ok();
                    if held && !picto_library::records_items(transaction)? {
                        return Err(picto_library::LibraryError::InvalidState(
                            CLOUD_HOLDS_LIBRARY.to_string(),
                        ));
                    }
                    // The library starts to sync with this folder. What it
                    // knew about the cloud data it synced with before does
                    // not hold here: the time of the last sync, which of its
                    // entries are written there, and which files it merged.
                    transaction.execute(
                        "UPDATE cloud_state SET last_sync_at = NULL, last_snapshot_at = NULL
                         WHERE singleton = 1",
                        [],
                    )?;
                    picto_library::reset_published_entries(transaction)?;
                }
                let changed = transaction.execute(
                    "UPDATE cloud_state
                     SET provider = ?1, account_label = ?2, remote_root = ?3,
                         state = 'idle', phase = 'idle', paused = 0, message = ''
                     WHERE singleton = 1
                       AND (provider IS NOT ?1 OR account_label IS NOT ?2
                            OR remote_root IS NOT ?3 OR paused != 0
                            OR state != 'idle' OR phase != 'idle' OR message != '')",
                    params![input.provider, input.account_label, root_path],
                )?;
                Ok((changed != 0 || !connected).then_some(()))
            },
        )
        .map_err(|error| error.to_string())?;
    seed_local_originals_library(application)?;
    cloud_receipt_or_current(application, published)
}
pub fn directory_provider_library(
    application: &LibraryApplication,
) -> Result<provider::DirectoryProvider, String> {
    let configuration = configuration_library(application)?;
    let root = configuration
        .root_path
        .ok_or_else(|| "Cloud sync is not configured".to_string())?;
    let provider_name = configuration
        .provider
        .ok_or_else(|| "Cloud sync is not configured".to_string())?;
    provider::DirectoryProvider::open_provider_root(&provider_name, root)
}

pub fn snapshot_due_library(
    application: &LibraryApplication,
    now_ms: i64,
    idle_age_ms: i64,
) -> Result<bool, String> {
    application
        .library()
        .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
            connection
                .query_row(
                    "SELECT provider IS NOT NULL AND paused = 0 AND state = 'idle'
                         AND (NOT EXISTS (SELECT 1 FROM cloud_snapshot) OR EXISTS (
                             SELECT 1 FROM cloud_journal WHERE expanded_at_ms IS NULL
                         ) OR EXISTS (
                             SELECT 1 FROM cloud_blob_state
                             WHERE state IN ('queued', 'downloading')
                                OR (state = 'available' AND remote_present = 0)
                         ))
                         AND NOT EXISTS (
                             SELECT 1 FROM cloud_journal
                             WHERE expanded_at_ms IS NULL AND created_at_ms > ?1
                         )
                         FROM cloud_state WHERE singleton = 1",
                    [now_ms.saturating_sub(idle_age_ms)],
                    |row| row.get(0),
                )
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())
}

pub fn recover_interrupted_sync_library(application: &LibraryApplication) -> Result<(), String> {
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
                transaction.execute(
                    "UPDATE cloud_state
                     SET state = CASE WHEN paused THEN 'paused' ELSE 'idle' END,
                         phase = 'idle', message = ''
                     WHERE singleton = 1 AND state = 'reconciling'",
                    [],
                )?;
                transaction.execute(
                    "UPDATE cloud_blob_state SET state = 'queued'
                     WHERE state = 'downloading'",
                    [],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
}

pub async fn discover_libraries(
    provider_name: &str,
    root_path: &str,
) -> Result<Vec<CloudLibraryOption>, String> {
    let validated = provider::validate_root(provider_name, root_path)?;
    let provider = provider::DirectoryProvider::open_existing(validated.path)?;
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
pub fn update_retention_library(
    application: &LibraryApplication,
    retention: &serde_json::Value,
) -> Result<picto_library::MutationReceipt, String> {
    let published = application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["cloud".to_string()],
            [],
            |transaction, _| {
                let current = transaction.query_row(
                    "SELECT retention_json FROM cloud_state WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )?;
                let mut merged = serde_json::from_str::<serde_json::Value>(&current)
                    .unwrap_or_else(|_| serde_json::json!({}));
                let object = merged.as_object_mut().ok_or_else(|| {
                    picto_library::LibraryError::InvalidState(
                        "cloud retention settings are not an object".into(),
                    )
                })?;
                if let Some(updates) = retention.as_object() {
                    for (key, value) in updates {
                        object.insert(key.clone(), value.clone());
                    }
                }
                object
                    .entry("deleted_blobs_days".to_owned())
                    .or_insert(serde_json::json!(7));
                let retention_json = serde_json::to_string(&merged).map_err(|error| {
                    picto_library::LibraryError::InvalidState(error.to_string())
                })?;
                let changed = transaction.execute(
                    "UPDATE cloud_state SET retention_json = ?1
                     WHERE singleton = 1 AND retention_json != ?1",
                    [&retention_json],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map_err(|error| error.to_string())?;
    cloud_receipt_or_current(application, published)
}

pub fn normalize_retention_library(application: &LibraryApplication) -> Result<(), String> {
    application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::Maintenance,
            ["cloud".to_string()],
            [],
            |transaction, _| {
                let current = transaction.query_row(
                    "SELECT retention_json FROM cloud_state WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )?;
                let mut retention = serde_json::from_str::<serde_json::Value>(&current)
                    .unwrap_or_else(|_| serde_json::json!({}));
                let Some(object) = retention.as_object_mut() else {
                    return Err(picto_library::LibraryError::InvalidState(
                        "cloud retention settings are not an object".into(),
                    ));
                };
                let was_legacy_default = object.get("deleted_blobs_days").and_then(|v| v.as_i64())
                    == Some(30)
                    && object.get("daily").and_then(|v| v.as_i64()) == Some(30)
                    && object.get("weekly").and_then(|v| v.as_i64()) == Some(26)
                    && object.get("yearly").and_then(|v| v.as_i64()) == Some(5)
                    && object.get("epochs_days").and_then(|v| v.as_i64()) == Some(30)
                    && object.get("full_media_history").and_then(|v| v.as_bool()) == Some(false);
                if was_legacy_default || !object.contains_key("deleted_blobs_days") {
                    object.insert("deleted_blobs_days".to_owned(), serde_json::json!(7));
                }
                object
                    .entry("epochs_days".to_owned())
                    .or_insert(serde_json::json!(30));
                object
                    .entry("full_media_history".to_owned())
                    .or_insert(serde_json::json!(false));
                let normalized = serde_json::to_string(&retention).map_err(|error| {
                    picto_library::LibraryError::InvalidState(error.to_string())
                })?;
                let changed = transaction.execute(
                    "UPDATE cloud_state SET retention_json = ?1
                     WHERE singleton = 1 AND retention_json != ?1",
                    [&normalized],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn set_paused_library(
    application: &LibraryApplication,
    paused: bool,
) -> Result<picto_library::MutationReceipt, String> {
    let published = application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["cloud".to_string(), "tasks".to_string()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE cloud_state
                     SET paused = ?1,
                         state = CASE WHEN ?1 THEN 'paused' ELSE 'idle' END,
                         phase = CASE WHEN ?1 THEN phase ELSE 'idle' END
                     WHERE singleton = 1
                       AND (paused != ?1 OR state != CASE WHEN ?1 THEN 'paused' ELSE 'idle' END
                            OR (?1 = 0 AND phase != 'idle'))",
                    [i64::from(paused)],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map_err(|error| error.to_string())?;
    cloud_receipt_or_current(application, published)
}

/// Switches cloud sync off for the library on this device (Stop Syncing).
/// The library and its cloud data stay as they are.
pub fn disable_library(
    application: &LibraryApplication,
) -> Result<picto_library::MutationReceipt, String> {
    disable_with_message(application, "")
}

/// Switches cloud sync off and leaves `message` as the sync's message, which
/// the app shows where it shows the sync state.
fn disable_with_message(
    application: &LibraryApplication,
    message: &str,
) -> Result<picto_library::MutationReceipt, String> {
    let published = application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["cloud".to_string(), "tasks".to_string()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE cloud_state
                     SET provider = NULL, account_label = NULL, remote_root = NULL,
                         paused = 0, state = 'disabled', phase = 'idle',
                         blocking = 0, completed_units = 0, total_units = NULL,
                         message = ?1, pending_blobs = 0, missing_blobs = 0
                     WHERE singleton = 1
                       AND (provider IS NOT NULL OR account_label IS NOT NULL
                            OR remote_root IS NOT NULL OR paused != 0
                            OR state != 'disabled' OR phase != 'idle'
                            OR blocking != 0 OR completed_units != 0
                            OR total_units IS NOT NULL OR message != ?1
                            OR pending_blobs != 0 OR missing_blobs != 0)",
                    [message],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map_err(|error| error.to_string())?;
    cloud_receipt_or_current(application, published)
}

/// Returns whether the library's cloud data is gone: the folder of the
/// provider is there, the library has synced with it before, and its
/// `library.json` is no longer in it. A library that has not synced with the
/// folder yet has no cloud data to lose; its first sync writes it.
async fn cloud_data_removed(
    application: &LibraryApplication,
    provider: &dyn provider::CloudProvider,
    library_id: &str,
) -> Result<bool, String> {
    if !provider.connectivity().await? {
        return Err("Cloud sync folder is unavailable".to_string());
    }
    if provider
        .exists(&format!("picto/{library_id}/library.json"))
        .await?
    {
        return Ok(false);
    }
    application
        .library()
        .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
            Ok(connection.query_row(
                "SELECT last_sync_at IS NOT NULL FROM cloud_state WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )?)
        })
        .map_err(|error| error.to_string())
}

/// Stops a sync that finds the library's cloud data gone: it switches cloud
/// sync off on this device, leaves [`CLOUD_DATA_REMOVED`] as the message,
/// and returns that message as the error. It writes nothing into the cloud
/// folder and changes nothing else in the library.
pub(crate) async fn stop_if_cloud_data_removed(
    application: &LibraryApplication,
    provider: &dyn provider::CloudProvider,
    library_id: &str,
) -> Result<(), String> {
    if !cloud_data_removed(application, provider, library_id).await? {
        return Ok(());
    }
    disable_with_message(application, CLOUD_DATA_REMOVED)?;
    Err(CLOUD_DATA_REMOVED.to_string())
}

/// Does Delete Cloud Data…: removes the library's data from the cloud folder
/// (`DirectoryProvider::remove_library`) and then switches cloud sync off on
/// this device. The library on this device keeps every item and file. It
/// waits for a running sync to finish first.
pub async fn delete_cloud_data_library(
    application: &LibraryApplication,
) -> Result<provider::CloudDataRemoval, String> {
    let _running = SYNC.lock().await;
    let provider = directory_provider_library(application)?;
    let library_id = configuration_library(application)?.library_id;
    let removal = tokio::task::spawn_blocking(move || provider.remove_library(&library_id))
        .await
        .map_err(|error| format!("Cloud data removal stopped: {error}"))??;
    disable_library(application)?;
    Ok(removal)
}

/// Returns whether the worker should merge the entries in the cloud folder
/// now: cloud sync is on, not paused, and no sync runs or failed.
pub fn receive_due_library(application: &LibraryApplication) -> Result<bool, String> {
    application
        .library()
        .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
            Ok(connection.query_row(
                "SELECT provider IS NOT NULL AND paused = 0 AND state = 'idle'
                 FROM cloud_state WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )?)
        })
        .map_err(|error| error.to_string())
}

/// Merges the entry files that are in the cloud folder and were not merged
/// yet, copies the originals of the items that wait for them, and returns
/// the number of files that were merged. The worker calls it between two
/// syncs, so that an import or a deletion made on another device arrives
/// while this library is idle. A damaged file leaves the sync state `error`
/// with a message that names it, and nothing is merged. A file that the
/// provider's app has not delivered yet leaves no error.
pub async fn receive_library(
    application: &LibraryApplication,
    provider: &dyn provider::CloudProvider,
) -> Result<u64, String> {
    let _running = SYNC.lock().await;
    let library_id = configuration_library(application)?.library_id;
    stop_if_cloud_data_removed(application, provider, &library_id).await?;
    let merged = entries::receive_library(application, provider, &library_id).await;
    if let Err(error) = &merged {
        let _ = snapshot::set_sync_state(application, "error", "idle", error);
    }
    merged
}

fn cloud_receipt_or_current(
    application: &LibraryApplication,
    published: Option<((), picto_library::MutationReceipt)>,
) -> Result<picto_library::MutationReceipt, String> {
    if let Some(((), receipt)) = published {
        return Ok(receipt);
    }
    Ok(picto_library::MutationReceipt {
        revision: application
            .library()
            .database()
            .revision()
            .map_err(|error| error.to_string())?,
        resources: vec!["cloud".to_string(), "tasks".to_string()],
        item_ids: Vec::new(),
    })
}

fn seed_local_originals_library(application: &LibraryApplication) -> Result<(), String> {
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
                transaction.execute(
                    // `WHERE true` ends the SELECT: after `FROM media_file`
                    // SQLite would otherwise read `ON` as a join constraint.
                    "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
                     SELECT content_hash, 'queued', ?1 FROM media_file WHERE true
                     ON CONFLICT(file_hash) DO UPDATE SET
                         state = 'queued', remote_present = 0, last_error = NULL,
                         uploaded_at = NULL, updated_at = excluded.updated_at",
                    [Utc::now().to_rfc3339()],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod two_device_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manual_snapshot_and_restart_recovery_preserve_pause() {
        let temp = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(temp.path().join("Paused.library")).unwrap();
        let provider = provider::DirectoryProvider::open(temp.path().join("provider")).unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Cloud,
                |transaction| {
                    transaction.execute(
                        "UPDATE cloud_state SET provider = 'dropbox' WHERE singleton = 1",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        for paused in [true, false] {
            set_paused_library(&application, paused).unwrap();
            snapshot::publish_library(&application, &provider)
                .await
                .unwrap();
            let expected = if paused { "paused" } else { "idle" };
            assert_eq!(status_library(&application).unwrap().state, expected);
            application
                .library()
                .database()
                .maintenance_write(
                    picto_library::database::WorkPriority::Cloud,
                    |transaction| {
                        transaction.execute(
                            "UPDATE cloud_state SET state = 'reconciling' WHERE singleton = 1",
                            [],
                        )?;
                        Ok(())
                    },
                )
                .unwrap();
            recover_interrupted_sync_library(&application).unwrap();
            assert_eq!(status_library(&application).unwrap().state, expected);
        }
    }

    #[test]
    fn disabling_cloud_clears_binding_and_preserves_pending_journal() {
        let temp = tempfile::tempdir().unwrap();
        let application =
            LibraryApplication::create(temp.path().join("CloudDisable.library")).unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                |transaction| {
                    transaction.execute(
                        "UPDATE cloud_state
                         SET provider = 'dropbox', account_label = 'Personal',
                             remote_root = '/tmp/dropbox', state = 'paused', paused = 1,
                             phase = 'uploading', blocking = 1, completed_units = 2,
                             total_units = 5, message = 'Uploading', pending_blobs = 3,
                             missing_blobs = 1
                         WHERE singleton = 1",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO cloud_journal
                             (revision, operation_kind, payload_json, created_at_ms)
                         VALUES (1, 'fixture', '{}', 1)",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        disable_library(&application).unwrap();

        let configuration = configuration_library(&application).unwrap();
        assert_eq!(configuration.provider, None);
        assert_eq!(configuration.account_label, None);
        assert_eq!(configuration.root_path, None);
        let status = status_library(&application).unwrap();
        assert_eq!(status.state, "disabled");
        assert_eq!(status.phase, "idle");
        assert!(!status.blocking);
        assert_eq!(status.pending_mutations, 1);
        assert!(!snapshot_due_library(&application, i64::MAX, 0).unwrap());
    }

    #[test]
    fn snapshot_waits_until_the_newest_pending_mutation_is_idle() {
        let temp = tempfile::tempdir().unwrap();
        let application =
            LibraryApplication::create(temp.path().join("CloudIdle.library")).unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                |transaction| {
                    transaction.execute(
                        "UPDATE cloud_state SET provider = 'dropbox', state = 'idle'
                         WHERE singleton = 1",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO cloud_journal
                             (revision, operation_kind, payload_json, created_at_ms)
                         VALUES (1, 'first', '{}', 100), (2, 'latest', '{}', 200)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO cloud_snapshot
                             (snapshot_id, frontier_json, database_sha256, artifact_sha256,
                              size_bytes, verified, created_at)
                         VALUES ('existing', '{}', 'db', 'artifact', 1, 1, 'now')",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        assert!(!snapshot_due_library(&application, 229, 30).unwrap());
        assert!(snapshot_due_library(&application, 230, 30).unwrap());
    }

    #[test]
    fn retention_updates_preserve_deleted_blob_policy() {
        let temp = tempfile::tempdir().unwrap();
        let application =
            LibraryApplication::create(temp.path().join("CloudRetention.library")).unwrap();

        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Maintenance,
                |transaction| {
                    transaction.execute(
                        "UPDATE cloud_state SET retention_json =
                         '{\"daily\":30,\"weekly\":26,\"yearly\":5,\"epochs_days\":30,\"deleted_blobs_days\":30,\"full_media_history\":false}'
                         WHERE singleton = 1",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        normalize_retention_library(&application).unwrap();

        update_retention_library(
            &application,
            &serde_json::json!({"daily": 12, "weekly": 8, "yearly": 2}),
        )
        .unwrap();
        let retention = configuration_library(&application).unwrap().retention;
        assert_eq!(retention["daily"], 12);
        assert_eq!(retention["weekly"], 8);
        assert_eq!(retention["yearly"], 2);
        assert_eq!(retention["deleted_blobs_days"], 7);
        assert_eq!(retention["epochs_days"], 30);
        assert_eq!(retention["full_media_history"], false);
    }

    /// Returns an import job for a PNG that it writes into `directory`. The
    /// picture's colour, and so its content hash, depends on `index`.
    pub(super) fn image_job(
        directory: &std::path::Path,
        index: u8,
    ) -> picto_library::PreparedIngestJob {
        use sha2::Digest;
        let path = directory.join(format!("cloud-{index}.png"));
        image::RgbaImage::from_pixel(8, 8, image::Rgba([index, 20, 30, 255]))
            .save(&path)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        picto_library::PreparedIngestJob {
            job_key: format!("manual:cloud-{index}"),
            source_kind: "manual".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_item_id: None,
            delete_after_ingest: false,
            payload: picto_library::PreparedIngestPayload::Item(picto_library::PreparedImport {
                stable_key: format!("cloud-root-{index}"),
                media_name: format!("cloud-{index}.png"),
                file_path: path.to_string_lossy().into_owned(),
                facts: picto_library::ImmutableMediaFacts {
                    mime: "image/png".into(),
                    size_bytes: bytes.len() as u64,
                    width: Some(8),
                    height: Some(8),
                    duration_ms: None,
                    frame_count: Some(1),
                    content_hash: hex::encode(sha2::Sha256::digest(&bytes)),
                    perceptual_hash: None,
                    palette: Vec::new(),
                },
                lifecycle: picto_library::Lifecycle::Inbox,
                rating: picto_library::Rating::Unrated,
                notes: None,
                tags: Vec::new(),
                folders: Vec::new(),
                source_urls: Vec::new(),
                source_identity: None,
                imported_at_ms: 1_700_000_000_000,
                captured_at_ms: None,
            }),
        }
    }

    /// Configure, publish and stage a restore against a disposable folder that
    /// stands in for Dropbox, with an empty and with a populated library.
    #[tokio::test]
    async fn configure_publish_and_restore_round_trip_through_a_provider_folder() {
        let provider_root = tempfile::tempdir().unwrap();
        std::env::set_var("PICTO_DROPBOX_ROOT", provider_root.path());
        for roots in [0u8, 2] {
            let temp = tempfile::tempdir().unwrap();
            let application =
                LibraryApplication::create(temp.path().join(format!("Cloud{roots}.library")))
                    .unwrap();
            for index in 0..roots {
                application
                    .library()
                    .enqueue_ingest_job(&image_job(temp.path(), index), "2026-08-28T12:00:00Z")
                    .unwrap();
            }
            if roots > 0 {
                let report = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
                assert_eq!(report.ingested, roots as usize);
            }

            configure_library(
                &application,
                &ConfigureCloudInput {
                    provider: "dropbox".into(),
                    account_label: "Test".into(),
                    root_path: provider_root.path().to_string_lossy().into_owned(),
                },
            )
            .unwrap();
            let queued = application
                .library()
                .auxiliary_read(picto_library::database::WorkPriority::Maintenance, |connection| {
                    Ok(connection.query_row(
                        "SELECT COUNT(*) FROM cloud_blob_state WHERE state = 'queued'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?)
                })
                .unwrap();
            assert_eq!(queued, roots as i64, "every original is queued for upload");

            let provider = directory_provider_library(&application).unwrap();
            let manifest = snapshot::publish_library(&application, &provider).await.unwrap();
            let points = snapshot::list_remote_for_library(&application, &provider)
                .await
                .unwrap();
            assert_eq!(points.len(), 1);
            let restore = snapshot::prepare_restore(&application, &provider, &manifest.snapshot_id)
                .await
                .unwrap();
            let staged = rusqlite::Connection::open(&restore.database_path).unwrap();
            let restored_roots: i64 = staged
                .query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))
                .unwrap();
            assert_eq!(restored_roots, roots as i64);
        }
    }
}
