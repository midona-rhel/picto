use std::collections::BTreeMap;

use chrono::Utc;
use sha2::{Digest, Sha256};

use super::epoch::{self, DeviceFrontier, EpochPack};
use super::provider::{CloudProvider, RemoteObject};
use super::{apply_downloaded, checksum, HybridTimestamp, CLOUD_SCHEMA_GENERATION};
use crate::app::{Application, MutationReceipt};

const APPLY_BATCH_SIZE: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileMode {
    Startup,
    Reconnect,
    Manual,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ReconcileResult {
    pub downloaded_epochs: usize,
    pub applied_mutations: usize,
    pub duplicate_mutations: usize,
    pub quarantined_mutations: usize,
    pub restored_data: bool,
}

pub async fn metadata_pending(
    application: &Application,
    provider: &dyn CloudProvider,
) -> Result<bool, String> {
    if !provider.connectivity().await? {
        return Ok(false);
    }
    let unpublished = application.store().read(|connection| {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_outbox WHERE published_at IS NULL)",
            [],
            |row| row.get::<_, bool>(0),
        )
    })?;
    if unpublished {
        return Ok(true);
    }
    remote_metadata_pending(application, provider).await
}

pub async fn remote_metadata_pending(
    application: &Application,
    provider: &dyn CloudProvider,
) -> Result<bool, String> {
    if !provider.connectivity().await? {
        return Ok(false);
    }
    let (library_id, local_frontier) = application.store().read(|connection| {
        let library_id: String = connection.query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        let mut statement = connection
            .prepare("SELECT device_id, hlc_physical_ms, hlc_logical FROM cloud_device_frontier")?;
        let frontier = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    HybridTimestamp {
                        physical_ms: row.get::<_, i64>(1)? as u64,
                        logical: row.get::<_, i64>(2)? as u32,
                    },
                ))
            })?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
        Ok((library_id, frontier))
    })?;
    let prefix = format!("picto/{library_id}/devices");
    for object in provider.list(&prefix).await? {
        if !object.path.ends_with("/frontier.json") {
            continue;
        }
        let bytes = verified_download(provider, &object).await?;
        let frontier: DeviceFrontier = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid cloud device frontier {}: {error}", object.path))?;
        if frontier.library_id != library_id {
            return Err("Cloud frontier belongs to another Picto library".to_string());
        }
        if frontier.frontier.iter().any(|(device, remote)| {
            local_frontier
                .get(device)
                .is_none_or(|local| remote > local)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn reconcile(
    application: &Application,
    provider: &dyn CloudProvider,
    mode: ReconcileMode,
) -> Result<ReconcileResult, String> {
    match reconcile_inner(application, provider, mode).await {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = update_status(application, "error", "idle", false, 0, None, &error);
            Err(error)
        }
    }
}

async fn reconcile_inner(
    application: &Application,
    provider: &dyn CloudProvider,
    mode: ReconcileMode,
) -> Result<ReconcileResult, String> {
    let blocking = mode == ReconcileMode::Startup;
    update_status(
        application,
        "reconciling",
        "checking",
        blocking,
        0,
        None,
        "Checking your library",
    )?;
    if !provider.connectivity().await? {
        update_status(
            application,
            "offline",
            "idle",
            false,
            0,
            None,
            "Cloud is unavailable. Local changes will sync when you reconnect.",
        )?;
        return Ok(ReconcileResult::default());
    }

    epoch::flush(application.store(), provider, false).await?;
    let library_id = application.store().read(|connection| {
        connection.query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
    })?;
    update_status(
        application,
        "reconciling",
        "downloading",
        blocking,
        0,
        None,
        "Downloading library updates",
    )?;

    let local_frontier = read_local_frontier(application)?;
    let remote_devices = newer_remote_devices(provider, &library_id, &local_frontier).await?;
    let mut objects = Vec::new();
    for device_id in remote_devices {
        let local = local_frontier.get(&device_id).copied();
        objects.extend(
            provider
                .list(&format!("picto/{library_id}/epochs/{device_id}"))
                .await?
                .into_iter()
                .filter(|object| object.path.ends_with(".epoch.zst"))
                .filter(|object| {
                    object.path.ends_with("/current.epoch.zst")
                        || epoch::sealed_pack_end(&object.path)
                            .is_none_or(|end| local.is_none_or(|local| end > local))
                }),
        );
    }
    let mut packs = Vec::with_capacity(objects.len());
    for object in &objects {
        let bytes = verified_download(provider, object).await?;
        let pack = epoch::decode(&bytes)?;
        validate_pack(&library_id, &pack)?;
        packs.push(pack);
    }

    let mut mutations = packs
        .iter()
        .flat_map(|pack| pack.mutations.iter().cloned())
        .collect::<Vec<_>>();
    mutations.sort_by(|left, right| {
        (
            left.timestamp,
            left.device_id.as_str(),
            left.mutation_id.as_str(),
        )
            .cmp(&(
                right.timestamp,
                right.device_id.as_str(),
                right.mutation_id.as_str(),
            ))
    });
    mutations.dedup_by(|left, right| left.mutation_id == right.mutation_id);

    update_status(
        application,
        "reconciling",
        "applying",
        blocking,
        0,
        Some(mutations.len() as i64),
        "Applying changes",
    )?;
    let mut result = ReconcileResult {
        downloaded_epochs: objects.len(),
        ..ReconcileResult::default()
    };
    for (index, batch) in mutations.chunks(APPLY_BATCH_SIZE).enumerate() {
        let (summary, receipt) = apply_downloaded(application, batch)?;
        result.applied_mutations += summary.applied;
        result.duplicate_mutations += summary.duplicate;
        result.quarantined_mutations += summary.quarantined;
        result.restored_data |= summary.applied > 0;
        publish(application, &receipt);
        update_status(
            application,
            "reconciling",
            "applying",
            blocking,
            ((index + 1) * APPLY_BATCH_SIZE).min(mutations.len()) as i64,
            Some(mutations.len() as i64),
            "Applying changes",
        )?;
    }
    update_status(
        application,
        "idle",
        "files",
        blocking,
        0,
        None,
        "Checking your files",
    )?;
    update_blob_counts(application)?;
    update_status(application, "idle", "idle", false, 0, None, "")?;
    Ok(result)
}

fn read_local_frontier(
    application: &Application,
) -> Result<BTreeMap<String, HybridTimestamp>, String> {
    application.store().read(|connection| {
        let mut statement = connection
            .prepare("SELECT device_id, hlc_physical_ms, hlc_logical FROM cloud_device_frontier")?;
        let frontier = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    HybridTimestamp {
                        physical_ms: row.get::<_, i64>(1)? as u64,
                        logical: row.get::<_, i64>(2)? as u32,
                    },
                ))
            })?
            .collect();
        frontier
    })
}

async fn newer_remote_devices(
    provider: &dyn CloudProvider,
    library_id: &str,
    local_frontier: &BTreeMap<String, HybridTimestamp>,
) -> Result<Vec<String>, String> {
    let mut devices = Vec::new();
    for object in provider
        .list(&format!("picto/{library_id}/devices"))
        .await?
    {
        if !object.path.ends_with("/frontier.json") {
            continue;
        }
        let bytes = verified_download(provider, &object).await?;
        let frontier: DeviceFrontier = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid cloud device frontier {}: {error}", object.path))?;
        if frontier.library_id != library_id {
            return Err("Cloud frontier belongs to another Picto library".to_string());
        }
        let Some(remote) = frontier.frontier.get(&frontier.device_id) else {
            continue;
        };
        if local_frontier
            .get(&frontier.device_id)
            .is_none_or(|local| remote > local)
        {
            devices.push(frontier.device_id);
        }
    }
    devices.sort();
    devices.dedup();
    Ok(devices)
}

async fn verified_download(
    provider: &dyn CloudProvider,
    object: &RemoteObject,
) -> Result<Vec<u8>, String> {
    let bytes = provider.download(&object.path).await?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if object
        .checksum
        .as_deref()
        .is_some_and(|expected| expected != actual)
    {
        return Err(format!("Cloud object checksum failed: {}", object.path));
    }
    Ok(bytes)
}

fn validate_pack(library_id: &str, pack: &EpochPack) -> Result<(), String> {
    if pack.library_id != library_id {
        return Err("Cloud epoch belongs to another Picto library".to_string());
    }
    for mutation in &pack.mutations {
        if mutation.library_id != library_id
            || mutation.schema_generation != CLOUD_SCHEMA_GENERATION
            || checksum(mutation).map_err(|error| error.to_string())? != mutation.checksum
        {
            return Err(format!(
                "Cloud epoch contains an invalid mutation: {}",
                mutation.mutation_id
            ));
        }
    }
    Ok(())
}

fn update_status(
    application: &Application,
    state: &str,
    phase: &str,
    blocking: bool,
    completed: i64,
    total: Option<i64>,
    message: &str,
) -> Result<(), String> {
    let (_, revision) = application.store().transaction(|transaction| {
        transaction.execute(
            "UPDATE cloud_state SET state = ?1, phase = ?2, blocking = ?3,
                    completed_units = ?4, total_units = ?5, message = ?6,
                    last_sync_at = CASE WHEN ?1 = 'idle' AND ?2 = 'idle' THEN ?7 ELSE last_sync_at END
             WHERE singleton = 1",
            rusqlite::params![
                state,
                phase,
                i64::from(blocking),
                completed,
                total,
                message,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    })?;
    application.publish(&MutationReceipt {
        revision,
        resources: vec![
            crate::app::resources::CLOUD.to_string(),
            crate::app::resources::TASKS.to_string(),
        ],
        item_ids: Vec::new(),
    });
    Ok(())
}

fn update_blob_counts(application: &Application) -> Result<(), String> {
    application.store().transaction(|transaction| {
        transaction.execute(
            "UPDATE cloud_state SET
                 pending_blobs = (SELECT COUNT(*) FROM cloud_blob_state WHERE state IN ('queued', 'downloading')),
                 missing_blobs = (SELECT COUNT(*) FROM cloud_blob_state WHERE state IN ('missing_remote', 'corrupt'))
             WHERE singleton = 1",
            [],
        )?;
        Ok(())
    })?;
    Ok(())
}

fn publish(application: &Application, receipt: &MutationReceipt) {
    if !receipt.resources.is_empty() {
        application.publish(receipt);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rusqlite::params;

    use super::*;
    use crate::cloud::provider::DirectoryProvider;
    use crate::store::Store;

    fn application() -> Application {
        let directory = tempfile::tempdir().unwrap();
        Application::new(Arc::new(Store::open(&directory.keep()).unwrap()))
    }

    fn configure(application: &Application, library_id: &str, device_id: &str) {
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET library_id = ?1, device_id = ?2,
                            provider = 'dropbox', state = 'idle'
                     WHERE singleton = 1",
                    params![library_id, device_id],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn add_media(application: &Application) {
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file (file_hash, mime_type, size_bytes, created_at)
                     VALUES ('hash-a', 'image/png', 10, 'now')",
                    [],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES ('item-a', 'media', 'now', 'now')",
                    [],
                )?;
                let item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (?1, ?2, 'now', 'now')",
                    params![item_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'inbox')",
                    [item_id],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn remote_creation_is_applied_once_without_copying_a_database() {
        let cloud = tempfile::tempdir().unwrap();
        let provider = DirectoryProvider::open(cloud.path()).unwrap();
        let first = application();
        let second = application();
        configure(&first, "library-a", "device-a");
        configure(&second, "library-a", "device-b");
        add_media(&first);
        epoch::flush(first.store(), &provider, true).await.unwrap();

        let result = reconcile(&second, &provider, ReconcileMode::Startup)
            .await
            .unwrap();
        assert_eq!(result.applied_mutations, 1);
        assert_eq!(
            second
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_key = 'item-a'",
                    [],
                    |row| row.get::<_, i64>(0),
                ))
                .unwrap(),
            1
        );
        let again = reconcile(&second, &provider, ReconcileMode::Manual)
            .await
            .unwrap();
        assert_eq!(again.applied_mutations, 0);
        assert_eq!(again.duplicate_mutations, 0);
        assert_eq!(again.downloaded_epochs, 0);
    }

    #[tokio::test]
    async fn restored_database_detects_newer_epochs_from_its_own_device() {
        let cloud = tempfile::tempdir().unwrap();
        let provider = DirectoryProvider::open(cloud.path()).unwrap();
        let current = application();
        let restored = application();
        configure(&current, "library-a", "device-a");
        configure(&restored, "library-a", "device-a");
        add_media(&current);
        epoch::flush(current.store(), &provider, true)
            .await
            .unwrap();

        assert!(remote_metadata_pending(&restored, &provider).await.unwrap());
        let result = reconcile(&restored, &provider, ReconcileMode::Startup)
            .await
            .unwrap();

        assert_eq!(result.applied_mutations, 1);
        assert_eq!(
            restored
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM library_item WHERE item_key = 'item-a'",
                    [],
                    |row| row.get::<_, i64>(0),
                ))
                .unwrap(),
            1
        );
    }
}
