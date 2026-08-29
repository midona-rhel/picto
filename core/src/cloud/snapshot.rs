use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use rusqlite::backup::Backup;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::provider::CloudProvider;
use super::{CausalFrontier, HybridTimestamp, CLOUD_SCHEMA_GENERATION};
use crate::library_application::LibraryApplication;
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

pub fn create_verified_library(
    application: &LibraryApplication,
) -> Result<(SnapshotArtifact, u64), String> {
    let staging = application.root().join("cloud").join("staging");
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create snapshot staging directory: {error}"))?;
    let snapshot_id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        uuid::Uuid::new_v4()
    );
    let database_path = staging.join(format!("{snapshot_id}.sqlite"));
    let compressed_path = staging.join(format!("{snapshot_id}.sqlite.zst"));
    let revision = application
        .library()
        .database()
        .read(picto_library::database::WorkPriority::Cloud, |source| {
            let revision = picto_library::schema::validate(source)?;
            let mut destination = Connection::open(&database_path)?;
            let backup = Backup::new(source, &mut destination)?;
            backup.run_to_completion(256, Duration::from_millis(5), None)?;
            drop(backup);
            validate_library_database(&destination)
                .map_err(picto_library::LibraryError::InvalidState)?;
            Ok(revision)
        })
        .map_err(|error| format!("Failed to stage SQLite snapshot: {error}"))?;

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
    Ok((
        SnapshotArtifact {
            snapshot_id,
            database_sha256,
            artifact_sha256,
            size_bytes,
            compressed_path,
        },
        revision,
    ))
}
pub async fn publish_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
) -> Result<SnapshotManifest, String> {
    let (artifact, database_revision) = create_verified_library(application)?;
    let (library_id, frontier) = identity_and_frontier_library(application)?;
    ensure_manifest_library(application, provider, &library_id).await?;
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
        schema_generation: picto_library::schema::SCHEMA_GENERATION as i64,
        frontier,
        database_sha256: artifact.database_sha256.clone(),
        artifact_sha256: artifact.artifact_sha256.clone(),
        size_bytes: artifact.size_bytes,
        created_at: Utc::now().to_rfc3339(),
        artifact_path,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
    let checksum = hex::encode(Sha256::digest(&bytes));
    provider.upload(&manifest_path, bytes, &checksum).await?;
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
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
                    "UPDATE cloud_journal SET expanded_at_ms = ?1
                     WHERE expanded_at_ms IS NULL AND revision <= ?2",
                    rusqlite::params![Utc::now().timestamp_millis(), database_revision as i64],
                )?;
                transaction.execute(
                    "UPDATE cloud_state
                     SET last_snapshot_at = ?1, last_sync_at = ?1,
                         state = 'idle', phase = 'idle', message = ''
                     WHERE singleton = 1",
                    [manifest.created_at.as_str()],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&artifact.compressed_path);
    Ok(manifest)
}

pub async fn list_remote_for_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
) -> Result<Vec<RestorePoint>, String> {
    let (library_id, _) = identity_and_frontier_library(application)?;
    list_remote_library(provider, &library_id).await
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

/// Stage the newest verified schema-1 snapshot without touching an open library.
pub async fn prepare_join(
    provider: &dyn CloudProvider,
    library_id: &str,
    target_root: &Path,
) -> Result<PreparedJoin, String> {
    let database_target = target_root.join("library.sqlite");
    if database_target.exists() {
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
    let database_path = directory.join("library.sqlite");
    std::fs::write(&database_path, database)
        .map_err(|error| format!("Failed to stage cloud library database: {error}"))?;
    validate_staged_database(&database_path, library_id)?;
    Ok(PreparedJoin {
        snapshot_id: manifest.snapshot_id,
        library_id: library_id.to_string(),
        database_path,
    })
}

/// Stage a verified replacement while the active database remains mounted.
pub async fn prepare_restore(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    snapshot_id: &str,
) -> Result<PreparedRestore, String> {
    let (library_id, _) = identity_and_frontier_library(application)?;
    let manifest_path = format!("picto/{library_id}/snapshots/{snapshot_id}.json");
    let manifest: SnapshotManifest =
        serde_json::from_slice(&provider.download(&manifest_path).await?)
            .map_err(|error| format!("Invalid snapshot manifest: {error}"))?;
    validate_manifest(&library_id, &manifest)?;
    let database = download_database(provider, &library_id, &manifest).await?;
    let directory = application
        .root()
        .join("cloud")
        .join("restore")
        .join(snapshot_id);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create restore staging directory: {error}"))?;
    let database_path = directory.join("library.sqlite");
    std::fs::write(&database_path, database)
        .map_err(|error| format!("Failed to stage restore database: {error}"))?;
    validate_staged_database(&database_path, &library_id)?;
    let emergency_copy_path = application
        .root()
        .join("cloud")
        .join("emergency")
        .join(format!(
            "pre-restore-{}-{}.sqlite",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            uuid::Uuid::new_v4()
        ));
    Ok(PreparedRestore {
        snapshot_id: snapshot_id.to_string(),
        database_path,
        emergency_copy_path,
    })
}

fn validate_staged_database(path: &Path, library_id: &str) -> Result<(), String> {
    let connection = Connection::open(path)
        .map_err(|error| format!("Failed to open staged cloud library: {error}"))?;
    validate_library_database(&connection)?;
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
    Ok(())
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
fn identity_and_frontier_library(
    application: &LibraryApplication,
) -> Result<(String, CausalFrontier), String> {
    application
        .library()
        .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
            let library_id = connection.query_row(
                "SELECT library_id FROM cloud_state WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let mut statement = connection.prepare(
                "SELECT device_id, hlc_physical_ms, hlc_logical
                     FROM cloud_device_frontier",
            )?;
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
        .map_err(|error| error.to_string())
}

async fn ensure_manifest_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<(), String> {
    let path = format!("picto/{library_id}/library.json");
    if provider.exists(&path).await? {
        return Ok(());
    }
    let name = application
        .root()
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_suffix(".library").unwrap_or(name))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Picto Library");
    let manifest = serde_json::to_vec(&serde_json::json!({
        "library_id": library_id,
        "name": name,
        "schema_generation": picto_library::schema::SCHEMA_GENERATION,
        "created_at": Utc::now().to_rfc3339(),
    }))
    .map_err(|error| error.to_string())?;
    let checksum = hex::encode(Sha256::digest(&manifest));
    provider.upload(&path, manifest, &checksum).await?;
    Ok(())
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

fn validate_library_database(connection: &Connection) -> Result<(), String> {
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
    picto_library::schema::validate(connection)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::provider::DirectoryProvider;

    #[tokio::test]
    async fn canonical_snapshot_covers_only_the_published_schema_one_revision() {
        let library_root = tempfile::tempdir().unwrap();
        let cloud_root = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(library_root.path()).unwrap();
        application
            .library()
            .auxiliary_semantic_write_if_changed(
                picto_library::database::WorkPriority::ForegroundMutation,
                ["settings".to_string()],
                [],
                "setting.patch",
                serde_json::json!({"key": "fixture"}),
                |transaction, _| {
                    transaction.execute(
                        "INSERT INTO setting (key, value_json) VALUES ('fixture', 'true')",
                        [],
                    )?;
                    Ok(Some(()))
                },
            )
            .unwrap();
        assert_eq!(
            application
                .library()
                .pending_cloud_journal(10)
                .unwrap()
                .len(),
            1
        );
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Cloud,
                |transaction| {
                    transaction.execute(
                        "UPDATE cloud_state SET provider = 'google_drive', paused = 0
                         WHERE singleton = 1",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        assert!(
            crate::cloud::snapshot_due_library(&application, Utc::now().timestamp_millis(), 0,)
                .unwrap()
        );

        let provider = DirectoryProvider::open(cloud_root.path()).unwrap();
        let manifest = publish_library(&application, &provider).await.unwrap();
        assert_eq!(manifest.schema_generation, 1);
        assert!(application
            .library()
            .pending_cloud_journal(10)
            .unwrap()
            .is_empty());
        let points = list_remote_for_library(&application, &provider)
            .await
            .unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].snapshot_id, manifest.snapshot_id);

        let joined_root = tempfile::tempdir().unwrap().path().join("Joined.library");
        let joined = prepare_join(&provider, &manifest.library_id, &joined_root)
            .await
            .unwrap();
        assert_eq!(joined.snapshot_id, manifest.snapshot_id);
        assert_eq!(joined.library_id, manifest.library_id);
        assert!(joined.database_path.is_file());

        let restored = prepare_restore(&application, &provider, &manifest.snapshot_id)
            .await
            .unwrap();
        assert_eq!(restored.snapshot_id, manifest.snapshot_id);
        assert!(restored.database_path.is_file());
        assert!(restored
            .emergency_copy_path
            .starts_with(application.root().join("cloud").join("emergency")));
        assert!(!crate::cloud::snapshot_due_library(
            &application,
            Utc::now().timestamp_millis(),
            0,
        )
        .unwrap());
    }
}
