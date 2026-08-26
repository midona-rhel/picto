use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::provider::CloudProvider;
use super::{CausalFrontier, CloudMutation, CloudOperation, HybridTimestamp};
use crate::store::Store;

const CURRENT_PACK_MAX_BYTES: i64 = 256 * 1024;
const SEALED_PACK_MAX_BYTES: i64 = 16 * 1024 * 1024;
const CURRENT_PACK_MAX_AGE_SECONDS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochPack {
    pub library_id: String,
    pub device_id: String,
    pub created_at: String,
    pub mutations: Vec<CloudMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFrontier {
    pub library_id: String,
    pub device_id: String,
    pub frontier: CausalFrontier,
    pub current_epoch_sha256: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlushResult {
    pub published_mutations: usize,
    pub published_bytes: usize,
    pub sealed: bool,
}

pub fn flush_due(store: &Store, now: DateTime<Utc>) -> Result<bool, String> {
    store.read(|connection| {
        let pending: (i64, i64, Option<String>) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(byte_size), 0), MIN(created_at)
             FROM cloud_outbox WHERE published_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if pending.0 == 0 {
            return Ok(false);
        }
        let old_enough = pending
            .2
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|created| {
                now.signed_duration_since(created.with_timezone(&Utc))
                    .num_seconds()
                    >= CURRENT_PACK_MAX_AGE_SECONDS
            });
        Ok(pending.1 >= CURRENT_PACK_MAX_BYTES || old_enough)
    })
}

pub async fn flush(
    store: &Store,
    provider: &dyn CloudProvider,
    force_seal: bool,
) -> Result<FlushResult, String> {
    let pack = load_current_and_pending(store)?;
    if pack.mutations.is_empty() {
        return Ok(FlushResult::default());
    }
    let encoded = encode(&pack)?;
    let checksum = hex::encode(Sha256::digest(&encoded));
    let seal = force_seal || encoded.len() as i64 >= SEALED_PACK_MAX_BYTES;
    let root = format!("picto/{}/", pack.library_id);
    ensure_manifest(store, provider, &pack.library_id).await?;
    let epoch_path = if seal {
        let last = pack
            .mutations
            .last()
            .expect("non-empty epoch pack must have a final mutation");
        format!(
            "{root}epochs/{}/{:020}-{:010}-{}.epoch.zst",
            pack.device_id,
            last.timestamp.physical_ms,
            last.timestamp.logical,
            uuid::Uuid::new_v4()
        )
    } else {
        format!("{root}epochs/{}/current.epoch.zst", pack.device_id)
    };

    // The epoch must be durable before any frontier can advertise it.
    provider
        .upload(&epoch_path, encoded.clone(), &checksum)
        .await?;
    let frontier = DeviceFrontier {
        library_id: pack.library_id.clone(),
        device_id: pack.device_id.clone(),
        frontier: frontier_with_pack(store, &pack)?,
        current_epoch_sha256: checksum,
        updated_at: Utc::now().to_rfc3339(),
    };
    let frontier_bytes = serde_json::to_vec(&frontier).map_err(|error| error.to_string())?;
    let frontier_checksum = hex::encode(Sha256::digest(&frontier_bytes));
    provider
        .upload(
            &format!("{root}devices/{}/frontier.json", pack.device_id),
            frontier_bytes,
            &frontier_checksum,
        )
        .await?;
    if seal {
        provider
            .delete(&format!(
                "{root}epochs/{}/current.epoch.zst",
                pack.device_id
            ))
            .await?;
    }

    let ids = pack
        .mutations
        .iter()
        .map(|mutation| mutation.mutation_id.as_str())
        .collect::<Vec<_>>();
    mark_published(store, &ids, seal)?;
    Ok(FlushResult {
        published_mutations: ids.len(),
        published_bytes: encoded.len(),
        sealed: seal,
    })
}

pub fn decode(bytes: &[u8]) -> Result<EpochPack, String> {
    let decoded = zstd::stream::decode_all(bytes)
        .map_err(|error| format!("Failed to decompress cloud epoch: {error}"))?;
    serde_json::from_slice(&decoded).map_err(|error| format!("Invalid cloud epoch: {error}"))
}

fn encode(pack: &EpochPack) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(pack).map_err(|error| error.to_string())?;
    zstd::stream::encode_all(json.as_slice(), 7)
        .map_err(|error| format!("Failed to compress cloud epoch: {error}"))
}

fn load_current_and_pending(store: &Store) -> Result<EpochPack, String> {
    store.read(|connection| {
        let (library_id, device_id): (String, String) = connection.query_row(
            "SELECT library_id, device_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut statement = connection.prepare(
            "SELECT mutation_id, library_id, device_id, hlc_physical_ms, hlc_logical,
                    causal_frontier_json, payload_json, schema_generation, checksum
             FROM cloud_outbox WHERE current_epoch = 1 OR published_at IS NULL
             ORDER BY hlc_physical_ms, hlc_logical, mutation_id",
        )?;
        let rows = statement.query_map([], |row| {
            let frontier_json: String = row.get(5)?;
            let payload_json: String = row.get(6)?;
            Ok(CloudMutation {
                mutation_id: row.get(0)?,
                library_id: row.get(1)?,
                device_id: row.get(2)?,
                timestamp: HybridTimestamp {
                    physical_ms: row.get::<_, i64>(3)? as u64,
                    logical: row.get::<_, i64>(4)? as u32,
                },
                causal_frontier: serde_json::from_str(&frontier_json).map_err(json_sql_error)?,
                operation: serde_json::from_str::<CloudOperation>(&payload_json)
                    .map_err(json_sql_error)?,
                schema_generation: row.get(7)?,
                checksum: row.get(8)?,
            })
        })?;
        Ok(EpochPack {
            library_id,
            device_id,
            created_at: Utc::now().to_rfc3339(),
            mutations: rows.collect::<rusqlite::Result<Vec<_>>>()?,
        })
    })
}

fn frontier_with_pack(store: &Store, pack: &EpochPack) -> Result<CausalFrontier, String> {
    store.read(|connection| {
        let mut statement = connection
            .prepare("SELECT device_id, hlc_physical_ms, hlc_logical FROM cloud_device_frontier")?;
        let mut frontier = statement
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
        if let Some(last) = pack.mutations.last() {
            frontier.insert(pack.device_id.clone(), last.timestamp);
        }
        Ok(frontier)
    })
}

fn mark_published(store: &Store, mutation_ids: &[&str], sealed: bool) -> Result<(), String> {
    store.transaction(|transaction| {
        let now = Utc::now().to_rfc3339();
        let ids_json = serde_json::to_string(mutation_ids).map_err(json_sql_error)?;
        if sealed {
            transaction.execute(
                "DELETE FROM cloud_outbox
                 WHERE mutation_id IN (SELECT CAST(value AS TEXT) FROM json_each(?1))",
                [ids_json],
            )?;
        } else {
            transaction.execute(
                "UPDATE cloud_outbox
                 SET published_at = COALESCE(published_at, ?1), current_epoch = 1
                 WHERE mutation_id IN (SELECT CAST(value AS TEXT) FROM json_each(?2))",
                params![now, ids_json],
            )?;
        }
        transaction.execute(
            "UPDATE cloud_state SET last_sync_at = ?1, state = 'idle', phase = 'idle'
             WHERE singleton = 1",
            [now],
        )?;
        Ok(())
    })?;
    Ok(())
}

pub(crate) fn sealed_pack_end(path: &str) -> Option<HybridTimestamp> {
    let name = path.rsplit('/').next()?.strip_suffix(".epoch.zst")?;
    if name == "current" {
        return None;
    }
    let mut parts = name.splitn(3, '-');
    Some(HybridTimestamp {
        physical_ms: parts.next()?.parse().ok()?,
        logical: parts.next()?.parse().ok()?,
    })
}

pub async fn ensure_manifest(
    store: &Store,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<(), String> {
    let path = format!("picto/{library_id}/library.json");
    let name = library_name(store);
    if provider.exists(&path).await? {
        let bytes = provider.download(&path).await?;
        let mut manifest: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid Picto cloud library manifest: {error}"))?;
        if manifest
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(());
        }
        manifest["name"] = serde_json::Value::String(name);
        let updated = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
        let previous_checksum = hex::encode(Sha256::digest(&bytes));
        let checksum = hex::encode(Sha256::digest(&updated));
        provider
            .upload_if_revision(&path, updated, &checksum, Some(&previous_checksum))
            .await?;
        return Ok(());
    }
    let manifest = serde_json::to_vec(&serde_json::json!({
        "library_id": library_id,
        "name": name,
        "schema_generation": super::CLOUD_SCHEMA_GENERATION,
        "created_at": Utc::now().to_rfc3339(),
    }))
    .map_err(|error| error.to_string())?;
    let checksum = hex::encode(Sha256::digest(&manifest));
    provider.upload(&path, manifest, &checksum).await?;
    Ok(())
}

fn library_name(store: &Store) -> String {
    store
        .library_root()
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_suffix(".library").unwrap_or(name))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Picto Library")
        .to_string()
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::provider::DirectoryProvider;
    use crate::cloud::{record_local, CloudOperation};

    fn cloud_library_id(store: &Store) -> String {
        store
            .read(|connection| {
                connection.query_row(
                    "SELECT library_id FROM cloud_state WHERE singleton = 1",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap()
    }

    #[tokio::test]
    async fn manifest_uses_the_library_name_instead_of_presenting_only_its_id() {
        let temporary = tempfile::tempdir().unwrap();
        let library = temporary.path().join("Reference Art.library");
        std::fs::create_dir(&library).unwrap();
        let remote = tempfile::tempdir().unwrap();
        let store = Store::open(&library).unwrap();
        let provider = DirectoryProvider::open(remote.path()).unwrap();
        let library_id = cloud_library_id(&store);

        ensure_manifest(&store, &provider, &library_id)
            .await
            .unwrap();

        let bytes = provider
            .download(&format!("picto/{library_id}/library.json"))
            .await
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["name"], "Reference Art");
    }

    #[tokio::test]
    async fn manifest_enriches_an_existing_hash_only_cloud_library() {
        let temporary = tempfile::tempdir().unwrap();
        let library = temporary.path().join("Reference Art.library");
        std::fs::create_dir(&library).unwrap();
        let remote = tempfile::tempdir().unwrap();
        let store = Store::open(&library).unwrap();
        let provider = DirectoryProvider::open(remote.path()).unwrap();
        let library_id = cloud_library_id(&store);
        let path = format!("picto/{library_id}/library.json");
        let original = serde_json::to_vec(&serde_json::json!({
            "library_id": library_id,
            "schema_generation": super::super::CLOUD_SCHEMA_GENERATION,
        }))
        .unwrap();
        let checksum = hex::encode(Sha256::digest(&original));
        provider.upload(&path, original, &checksum).await.unwrap();

        ensure_manifest(&store, &provider, &library_id)
            .await
            .unwrap();

        let bytes = provider.download(&path).await.unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["name"], "Reference Art");
    }

    #[tokio::test]
    async fn publishes_epoch_before_frontier_and_marks_outbox_after_confirmation() {
        let library = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let store = Store::open(library.path()).unwrap();
        store
            .transaction(|transaction| {
                record_local(
                    transaction,
                    CloudOperation::DeleteItem {
                        item_key: "gone".into(),
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let provider = DirectoryProvider::open(remote.path()).unwrap();
        let result = flush(&store, &provider, false).await.unwrap();
        assert_eq!(result.published_mutations, 1);
        assert_eq!(
            store
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM cloud_outbox WHERE published_at IS NULL",
                    [],
                    |row| row.get::<_, i64>(0),
                ))
                .unwrap(),
            0
        );
        let objects = provider.list("picto").await.unwrap();
        assert!(objects
            .iter()
            .any(|object| object.path.ends_with("current.epoch.zst")));
        assert!(objects
            .iter()
            .any(|object| object.path.ends_with("frontier.json")));
    }

    #[tokio::test]
    async fn sealed_epoch_removes_confirmed_outbox_rows() {
        let library = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let store = Store::open(library.path()).unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET provider = 'dropbox' WHERE singleton = 1",
                    [],
                )?;
                record_local(
                    transaction,
                    CloudOperation::DeleteItem {
                        item_key: "gone".into(),
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let provider = DirectoryProvider::open(remote.path()).unwrap();
        flush(&store, &provider, false).await.unwrap();
        assert!(provider
            .list("picto")
            .await
            .unwrap()
            .iter()
            .any(|object| object.path.ends_with("current.epoch.zst")));
        store
            .transaction(|transaction| {
                record_local(
                    transaction,
                    CloudOperation::DeleteItem {
                        item_key: "gone-again".into(),
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let result = flush(&store, &provider, true).await.unwrap();
        assert!(result.sealed);
        assert_eq!(
            store
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM cloud_outbox",
                    [],
                    |row| row.get::<_, i64>(0),
                ))
                .unwrap(),
            0
        );
        let objects = provider.list("picto").await.unwrap();
        assert!(objects
            .iter()
            .any(|object| sealed_pack_end(&object.path).is_some()));
        assert!(!objects
            .iter()
            .any(|object| object.path.ends_with("current.epoch.zst")));
    }

    #[test]
    fn sealed_epoch_name_exposes_its_frontier_bound() {
        assert_eq!(
            sealed_pack_end(
                "picto/library/epochs/device/00000000000000000123-0000000004-id.epoch.zst"
            ),
            Some(HybridTimestamp {
                physical_ms: 123,
                logical: 4,
            })
        );
        assert_eq!(
            sealed_pack_end("picto/library/epochs/device/current.epoch.zst"),
            None
        );
    }
}
