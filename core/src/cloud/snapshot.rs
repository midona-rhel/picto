use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Datelike, Utc};
use rusqlite::backup::Backup;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::epoch;
use super::provider::CloudProvider;
use super::{CausalFrontier, HybridTimestamp, CLOUD_SCHEMA_GENERATION};
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotArtifact {
    pub snapshot_id: String,
    pub database_sha256: String,
    pub artifact_sha256: String,
    pub size_bytes: u64,
    pub compressed_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub library_id: String,
    pub schema_generation: i64,
    pub frontier: CausalFrontier,
    pub database_sha256: String,
    pub artifact_sha256: String,
    pub size_bytes: u64,
    pub created_at: String,
    pub artifact_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RestorePoint {
    pub snapshot_id: String,
    pub created_at: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRestore {
    pub snapshot_id: String,
    pub database_path: PathBuf,
    pub emergency_copy_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedJoin {
    pub snapshot_id: String,
    pub library_id: String,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RetentionPolicy {
    #[serde(default = "default_daily")]
    pub daily: usize,
    #[serde(default = "default_weekly")]
    pub weekly: usize,
    #[serde(default = "default_yearly")]
    pub yearly: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            daily: default_daily(),
            weekly: default_weekly(),
            yearly: default_yearly(),
        }
    }
}

const fn default_daily() -> usize {
    30
}

const fn default_weekly() -> usize {
    26
}

const fn default_yearly() -> usize {
    5
}

pub fn create_verified(store: &Store) -> Result<SnapshotArtifact, String> {
    let staging = store.library_root().join("cloud").join("staging");
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create snapshot staging directory: {error}"))?;
    let snapshot_id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        uuid::Uuid::new_v4()
    );
    let database_path = staging.join(format!("{snapshot_id}.sqlite"));
    let compressed_path = staging.join(format!("{snapshot_id}.sqlite.zst"));

    store.read_snapshot_result(|source| {
        let mut destination = Connection::open(&database_path)
            .map_err(|error| format!("Failed to stage SQLite snapshot: {error}"))?;
        let backup = Backup::new(source, &mut destination)
            .map_err(|error| format!("Failed to initialize SQLite backup: {error}"))?;
        backup
            .run_to_completion(256, Duration::from_millis(5), None)
            .map_err(|error| format!("Failed to copy SQLite snapshot: {error}"))?;
        drop(backup);
        validate_database(&destination)
    })?;

    let database_file = std::fs::File::open(&database_path)
        .map_err(|error| format!("Failed to read staged snapshot: {error}"))?;
    let compressed_file = std::fs::File::create(&compressed_path)
        .map_err(|error| format!("Failed to create compressed snapshot: {error}"))?;
    let mut database_reader = HashingReader::new(database_file);
    let mut encoder = zstd::stream::Encoder::new(compressed_file, 9)
        .map_err(|error| format!("Failed to initialize snapshot compression: {error}"))?;
    std::io::copy(&mut database_reader, &mut encoder)
        .map_err(|error| format!("Failed to compress SQLite snapshot: {error}"))?;
    encoder
        .finish()
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Failed to finish compressed snapshot: {error}"))?;
    let database_sha256 = database_reader.finish();
    let artifact_sha256 = hash_file(&compressed_path)?;
    let size_bytes = std::fs::metadata(&compressed_path)
        .map_err(|error| format!("Failed to inspect compressed snapshot: {error}"))?
        .len();
    std::fs::remove_file(&database_path)
        .map_err(|error| format!("Failed to remove uncompressed snapshot staging file: {error}"))?;
    Ok(SnapshotArtifact {
        snapshot_id,
        database_sha256,
        artifact_sha256,
        size_bytes,
        compressed_path,
    })
}

pub fn validate_compressed(path: &Path, expected_sha256: &str) -> Result<(), String> {
    if hash_file(path)? != expected_sha256 {
        return Err("Snapshot artifact checksum mismatch".to_string());
    }
    let compressed = std::fs::File::open(path)
        .map_err(|error| format!("Failed to read compressed snapshot: {error}"))?;
    let mut decoder = zstd::stream::Decoder::new(compressed)
        .map_err(|error| format!("Failed to initialize snapshot decompression: {error}"))?;
    let mut temporary = tempfile::NamedTempFile::new()
        .map_err(|error| format!("Failed to stage snapshot validation: {error}"))?;
    std::io::copy(&mut decoder, &mut temporary)
        .map_err(|error| format!("Failed to decompress snapshot: {error}"))?;
    temporary
        .flush()
        .map_err(|error| format!("Failed to flush snapshot validation: {error}"))?;
    // FTS5's integrity check uses temporary writes even though the database
    // itself is not modified, so validation needs a writable staging handle.
    let connection = Connection::open(temporary.path())
        .map_err(|error| format!("Failed to open snapshot validation database: {error}"))?;
    validate_database(&connection)
}

pub async fn publish(
    store: &Store,
    provider: &dyn CloudProvider,
) -> Result<SnapshotManifest, String> {
    // A snapshot frontier may only reference immutable epoch artifacts.
    epoch::flush(store, provider, true).await?;
    let artifact = create_verified(store)?;
    let (library_id, frontier) = identity_and_frontier(store)?;
    epoch::ensure_manifest(store, provider, &library_id).await?;
    let root = format!("picto/{library_id}/snapshots/{}", artifact.snapshot_id);
    let artifact_path = format!("{root}.sqlite.zst");
    let manifest_path = format!("{root}.json");
    provider
        .upload_file(
            &artifact_path,
            artifact.compressed_path.clone(),
            &artifact.artifact_sha256,
        )
        .await?;
    let manifest = SnapshotManifest {
        snapshot_id: artifact.snapshot_id.clone(),
        library_id,
        schema_generation: CLOUD_SCHEMA_GENERATION,
        frontier,
        database_sha256: artifact.database_sha256.clone(),
        artifact_sha256: artifact.artifact_sha256.clone(),
        size_bytes: artifact.size_bytes,
        created_at: chrono::Utc::now().to_rfc3339(),
        artifact_path: artifact_path.clone(),
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
    let checksum = hex::encode(Sha256::digest(&bytes));
    // Publishing the manifest last makes an incomplete upload unreachable.
    provider.upload(&manifest_path, bytes, &checksum).await?;
    store.transaction(|transaction| {
        transaction.execute(
            "INSERT INTO cloud_snapshot (
                 snapshot_id, frontier_json, database_sha256, artifact_sha256,
                 size_bytes, verified, created_at, remote_path, published_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?6)",
            rusqlite::params![
                manifest.snapshot_id,
                serde_json::to_string(&manifest.frontier).map_err(json_sql_error)?,
                manifest.database_sha256,
                manifest.artifact_sha256,
                manifest.size_bytes as i64,
                manifest.created_at,
                manifest_path,
            ],
        )?;
        transaction.execute(
            "UPDATE cloud_state SET last_snapshot_at = ?1 WHERE singleton = 1",
            [manifest.created_at.as_str()],
        )?;
        Ok(())
    })?;
    let _ = std::fs::remove_file(&artifact.compressed_path);
    Ok(manifest)
}

fn hash_file(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("Failed to hash {}: {error}", path.display()))?;
    let mut reader = HashingReader::new(file);
    std::io::copy(&mut reader, &mut std::io::sink())
        .map_err(|error| format!("Failed to hash {}: {error}", path.display()))?;
    Ok(reader.finish())
}

struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    fn finish(self) -> String {
        hex::encode(self.hasher.finalize())
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

pub async fn list_remote(
    store: &Store,
    provider: &dyn CloudProvider,
) -> Result<Vec<RestorePoint>, String> {
    let (library_id, _) = identity_and_frontier(store)?;
    let mut points = Vec::new();
    for object in provider
        .list(&format!("picto/{library_id}/snapshots"))
        .await?
        .into_iter()
        .filter(|object| object.path.ends_with(".json"))
    {
        let bytes = provider.download(&object.path).await?;
        let manifest: SnapshotManifest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid snapshot manifest {}: {error}", object.path))?;
        validate_manifest(&library_id, &manifest)?;
        points.push(RestorePoint {
            snapshot_id: manifest.snapshot_id,
            created_at: manifest.created_at,
            size_bytes: manifest.size_bytes,
            verified: true,
        });
    }
    points.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(points)
}

pub async fn list_remote_library(
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<Vec<RestorePoint>, String> {
    let manifests = remote_manifests(provider, library_id).await?;
    Ok(manifests
        .into_iter()
        .map(|manifest| RestorePoint {
            snapshot_id: manifest.snapshot_id,
            created_at: manifest.created_at,
            size_bytes: manifest.size_bytes,
            verified: true,
        })
        .collect())
}

/// Removes only recovery snapshots outside the configured retention buckets.
/// Epoch pruning is separate because an epoch is safe to remove only after all
/// retained snapshot frontiers have advanced past it.
pub async fn prune_remote(store: &Store, provider: &dyn CloudProvider) -> Result<usize, String> {
    let (library_id, _) = identity_and_frontier(store)?;
    let policy = store.read(|connection| {
        let json: String = connection.query_row(
            "SELECT retention_json FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        serde_json::from_str(&json).map_err(json_sql_error)
    })?;
    let manifests = remote_manifests(provider, &library_id).await?;
    let retained = retained_snapshot_ids(&manifests, policy)?;
    let mut removed = 0;
    for manifest in manifests {
        if retained.contains(&manifest.snapshot_id) {
            continue;
        }
        provider.delete(&manifest.artifact_path).await?;
        provider
            .delete(&format!(
                "picto/{library_id}/snapshots/{}.json",
                manifest.snapshot_id
            ))
            .await?;
        store.transaction(|transaction| {
            transaction.execute(
                "DELETE FROM cloud_snapshot WHERE snapshot_id = ?1",
                [&manifest.snapshot_id],
            )?;
            Ok(())
        })?;
        removed += 1;
    }
    Ok(removed)
}

fn retained_snapshot_ids(
    manifests: &[SnapshotManifest],
    policy: RetentionPolicy,
) -> Result<HashSet<String>, String> {
    let dated = manifests
        .iter()
        .map(|manifest| {
            DateTime::parse_from_rfc3339(&manifest.created_at)
                .map(|created| (manifest, created.with_timezone(&Utc)))
                .map_err(|error| {
                    format!(
                        "Invalid snapshot creation time {}: {error}",
                        manifest.created_at
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut retained = dated
        .iter()
        .take(2)
        .map(|(manifest, _)| manifest.snapshot_id.clone())
        .collect::<HashSet<_>>();
    retain_bucket(
        &dated,
        policy.daily,
        |date| (date.year(), date.ordinal()),
        &mut retained,
    );
    retain_bucket(
        &dated,
        policy.weekly,
        |date| {
            let week = date.iso_week();
            (week.year(), week.week())
        },
        &mut retained,
    );
    retain_bucket(&dated, policy.yearly, |date| date.year(), &mut retained);
    Ok(retained)
}

fn retain_bucket<K: Eq + std::hash::Hash>(
    manifests: &[(&SnapshotManifest, DateTime<Utc>)],
    limit: usize,
    key: impl Fn(&DateTime<Utc>) -> K,
    retained: &mut HashSet<String>,
) {
    let mut buckets = HashSet::new();
    for (manifest, created) in manifests {
        if buckets.len() >= limit {
            break;
        }
        if buckets.insert(key(created)) {
            retained.insert(manifest.snapshot_id.clone());
        }
    }
}

/// Stage the newest verified snapshot for a library that is not open locally.
/// The caller owns activation and must never replace an open SQLite database.
pub async fn prepare_join(
    provider: &dyn CloudProvider,
    library_id: &str,
    target_root: &Path,
) -> Result<PreparedJoin, String> {
    if target_root.join(crate::store::DATABASE_FILE).exists() {
        return Err("The destination already contains a Picto library".to_string());
    }
    let manifest = remote_manifests(provider, library_id)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "This cloud library has no recovery snapshot yet".to_string())?;
    let database = download_database(provider, library_id, &manifest).await?;
    let directory = target_root
        .join("cloud")
        .join("bootstrap")
        .join(&manifest.snapshot_id);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create cloud library staging directory: {error}"))?;
    let database_path = directory.join(crate::store::DATABASE_FILE);
    std::fs::write(&database_path, database)
        .map_err(|error| format!("Failed to stage cloud library database: {error}"))?;
    let connection = Connection::open(&database_path)
        .map_err(|error| format!("Failed to open staged cloud library: {error}"))?;
    validate_database(&connection)?;
    let restored_library_id: String = connection
        .query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to validate cloud library identity: {error}"))?;
    if restored_library_id != library_id {
        return Err("Snapshot belongs to another Picto library".to_string());
    }
    Ok(PreparedJoin {
        snapshot_id: manifest.snapshot_id,
        library_id: library_id.to_string(),
        database_path,
    })
}

/// Download and validate a restore candidate without touching the active DB.
/// Activation is a separate library-close operation so an open SQLite handle
/// is never replaced underneath readers or writers.
pub async fn prepare_restore(
    store: &Store,
    provider: &dyn CloudProvider,
    snapshot_id: &str,
) -> Result<PreparedRestore, String> {
    let (library_id, _) = identity_and_frontier(store)?;
    let manifest_path = format!("picto/{library_id}/snapshots/{snapshot_id}.json");
    let manifest: SnapshotManifest =
        serde_json::from_slice(&provider.download(&manifest_path).await?)
            .map_err(|error| format!("Invalid snapshot manifest: {error}"))?;
    validate_manifest(&library_id, &manifest)?;
    let database = download_database(provider, &library_id, &manifest).await?;
    let directory = store
        .library_root()
        .join("cloud")
        .join("restore")
        .join(snapshot_id);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create restore staging directory: {error}"))?;
    let database_path = directory.join("library.sqlite");
    std::fs::write(&database_path, database)
        .map_err(|error| format!("Failed to stage restore database: {error}"))?;
    let connection = Connection::open(&database_path)
        .map_err(|error| format!("Failed to open staged restore: {error}"))?;
    validate_database(&connection)?;
    let restored_library_id: String = connection
        .query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to validate restored library identity: {error}"))?;
    if restored_library_id != library_id {
        return Err("Snapshot belongs to another Picto library".to_string());
    }
    drop(connection);
    let emergency_copy_path = store
        .library_root()
        .join("cloud")
        .join("emergency")
        .join(format!(
            "pre-restore-{}-{}.sqlite",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            uuid::Uuid::new_v4()
        ));
    Ok(PreparedRestore {
        snapshot_id: snapshot_id.to_string(),
        database_path,
        emergency_copy_path,
    })
}

async fn remote_manifests(
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<Vec<SnapshotManifest>, String> {
    let mut manifests = Vec::new();
    for object in provider
        .list(&format!("picto/{library_id}/snapshots"))
        .await?
        .into_iter()
        .filter(|object| object.path.ends_with(".json"))
    {
        let bytes = provider.download(&object.path).await?;
        let manifest: SnapshotManifest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid snapshot manifest {}: {error}", object.path))?;
        validate_manifest(library_id, &manifest)?;
        manifests.push(manifest);
    }
    manifests.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(manifests)
}

async fn download_database(
    provider: &dyn CloudProvider,
    library_id: &str,
    manifest: &SnapshotManifest,
) -> Result<Vec<u8>, String> {
    validate_manifest(library_id, manifest)?;
    let compressed = provider.download(&manifest.artifact_path).await?;
    if hex::encode(Sha256::digest(&compressed)) != manifest.artifact_sha256 {
        return Err("Snapshot artifact checksum mismatch".to_string());
    }
    let database = zstd::stream::decode_all(compressed.as_slice())
        .map_err(|error| format!("Failed to decompress snapshot: {error}"))?;
    if hex::encode(Sha256::digest(&database)) != manifest.database_sha256 {
        return Err("Snapshot database checksum mismatch".to_string());
    }
    Ok(database)
}

fn identity_and_frontier(store: &Store) -> Result<(String, CausalFrontier), String> {
    store.read(|connection| {
        let library_id = connection.query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
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
            .collect::<rusqlite::Result<CausalFrontier>>()?;
        Ok((library_id, frontier))
    })
}

fn validate_manifest(library_id: &str, manifest: &SnapshotManifest) -> Result<(), String> {
    if manifest.library_id != library_id {
        return Err("Snapshot belongs to another Picto library".to_string());
    }
    if manifest.schema_generation != CLOUD_SCHEMA_GENERATION {
        return Err("Snapshot uses an incompatible cloud schema generation".to_string());
    }
    Ok(())
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn validate_database(connection: &Connection) -> Result<(), String> {
    let quick: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| format!("Snapshot quick_check failed: {error}"))?;
    if quick != "ok" {
        return Err(format!("Snapshot quick_check reported: {quick}"));
    }
    let has_foreign_key_error = connection
        .prepare("PRAGMA foreign_key_check")
        .and_then(|mut statement| statement.exists([]))
        .map_err(|error| format!("Snapshot foreign_key_check failed: {error}"))?;
    if has_foreign_key_error {
        return Err("Snapshot contains invalid foreign-key references".to_string());
    }
    crate::store::schema::validate(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Application;
    use crate::cloud::provider::DirectoryProvider;
    use crate::cloud::{configure, ConfigureCloudInput};
    use std::sync::Arc;

    #[test]
    fn online_backup_is_compressed_and_revalidated() {
        let library = tempfile::tempdir().unwrap();
        let store = Store::open(library.path()).unwrap();
        let artifact = create_verified(&store).unwrap();
        validate_compressed(&artifact.compressed_path, &artifact.artifact_sha256).unwrap();
    }

    #[test]
    fn retention_keeps_two_newest_and_daily_weekly_yearly_representatives() {
        let manifest = |id: &str, created_at: &str| SnapshotManifest {
            snapshot_id: id.into(),
            library_id: "library".into(),
            schema_generation: CLOUD_SCHEMA_GENERATION,
            frontier: Default::default(),
            database_sha256: "db".into(),
            artifact_sha256: "artifact".into(),
            size_bytes: 1,
            created_at: created_at.into(),
            artifact_path: format!("snapshots/{id}.sqlite.zst"),
        };
        let manifests = vec![
            manifest("newest", "2026-08-25T12:00:00Z"),
            manifest("same-day", "2026-08-25T10:00:00Z"),
            manifest("previous-day", "2026-08-24T10:00:00Z"),
            manifest("previous-week", "2026-08-16T10:00:00Z"),
            manifest("previous-year", "2025-08-16T10:00:00Z"),
            manifest("expired", "2024-08-16T10:00:00Z"),
        ];

        let retained = retained_snapshot_ids(
            &manifests,
            RetentionPolicy {
                daily: 2,
                weekly: 2,
                yearly: 2,
            },
        )
        .unwrap();

        assert!(retained.contains("newest"));
        assert!(retained.contains("same-day"));
        assert!(retained.contains("previous-day"));
        assert!(retained.contains("previous-week"));
        assert!(retained.contains("previous-year"));
        assert!(!retained.contains("expired"));
    }

    #[tokio::test]
    async fn join_stages_the_newest_verified_snapshot() {
        let library = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(library.path()).unwrap());
        let application = Application::try_new(Arc::clone(&store)).unwrap();
        configure(
            &application,
            &ConfigureCloudInput {
                provider: "dropbox".into(),
                account_label: "test".into(),
                root_path: remote.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let provider = DirectoryProvider::open_existing(remote.path()).unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO setting (key, value_json) VALUES ('join-marker', '\"first\"')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        publish(&store, &provider).await.unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE setting SET value_json = '\"newest\"' WHERE key = 'join-marker'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let newest = publish(&store, &provider).await.unwrap();
        let target = tempfile::tempdir().unwrap();

        let prepared = prepare_join(&provider, &newest.library_id, target.path())
            .await
            .unwrap();

        assert_eq!(prepared.snapshot_id, newest.snapshot_id);
        let joined = Connection::open(prepared.database_path).unwrap();
        assert_eq!(
            joined
                .query_row(
                    "SELECT value_json FROM setting WHERE key = 'join-marker'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "\"newest\""
        );
    }
}
