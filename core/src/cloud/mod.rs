//! Low-priority schema-1 cloud snapshot configuration and publication.

use std::collections::BTreeMap;

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::library_application::LibraryApplication;

pub mod provider;
pub mod snapshot;

pub const CLOUD_SCHEMA_GENERATION: i64 = 1;

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
                             WHERE expanded_at_ms IS NULL) +
                            (SELECT COUNT(*) FROM cloud_outbox
                             WHERE published_at IS NULL),
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
    let root = provider::canonical_provider_root(
        &input.provider,
        std::path::PathBuf::from(&input.root_path),
    );
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
                Ok((changed != 0).then_some(()))
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
                     SET state = 'idle', phase = 'idle', message = ''
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
pub fn update_retention_library(
    application: &LibraryApplication,
    retention: &serde_json::Value,
) -> Result<picto_library::MutationReceipt, String> {
    let retention_json = serde_json::to_string(retention).map_err(|error| error.to_string())?;
    let published = application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["cloud".to_string()],
            [],
            |transaction, _| {
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

pub fn disable_library(
    application: &LibraryApplication,
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
                         message = '', pending_blobs = 0, missing_blobs = 0
                     WHERE singleton = 1
                       AND (provider IS NOT NULL OR account_label IS NOT NULL
                            OR remote_root IS NOT NULL OR paused != 0
                            OR state != 'disabled' OR phase != 'idle'
                            OR blocking != 0 OR completed_units != 0
                            OR total_units IS NOT NULL OR message != ''
                            OR pending_blobs != 0 OR missing_blobs != 0)",
                    [],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )
        .map_err(|error| error.to_string())?;
    cloud_receipt_or_current(application, published)
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
                    "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
                     SELECT content_hash, 'queued', ?1 FROM media_file
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
mod tests {
    use super::*;

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
}
