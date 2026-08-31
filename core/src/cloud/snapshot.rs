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
    database_path: PathBuf,
}

impl Drop for SnapshotArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.database_path);
        let _ = std::fs::remove_file(&self.compressed_path);
    }
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
        .read_consistent(
            picto_library::database::WorkPriority::Cloud,
            Ok,
            |source, revision| {
                let mut destination = Connection::open(&database_path)?;
                let backup = Backup::new(source, &mut destination)?;
                backup.run_to_completion(256, Duration::from_millis(5), None)?;
                drop(backup);
                validate_library_database(&destination)
                    .map_err(picto_library::LibraryError::InvalidState)?;
                Ok(revision)
            },
        )
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
    Ok((
        SnapshotArtifact {
            snapshot_id,
            database_sha256,
            artifact_sha256,
            size_bytes,
            compressed_path,
            database_path,
        },
        revision,
    ))
}
pub async fn publish_library(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
) -> Result<SnapshotManifest, String> {
    set_sync_state(
        application,
        "reconciling",
        "preparing",
        "Preparing library sync",
    )?;
    let result = publish_library_inner(application, provider).await;
    if let Err(error) = &result {
        let _ = set_sync_state(application, "error", "idle", error);
    }
    result
}

async fn publish_library_inner(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
) -> Result<SnapshotManifest, String> {
    let (artifact, database_revision) = create_verified_library(application)?;
    let (library_id, frontier) = identity_and_frontier_library(application)?;
    ensure_manifest_library(application, provider, &library_id).await?;
    purge_expired_cloud_blobs(application, provider, &library_id).await?;
    sync_snapshot_blobs(application, provider, &artifact.database_path, &library_id).await?;
    finish_blocking_restore(application)?;
    set_sync_state(
        application,
        "reconciling",
        "snapshot",
        "Syncing library changes",
    )?;
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
    Ok(manifest)
}

#[derive(Debug)]
struct SnapshotBlob {
    hash: String,
    extension: String,
    size_bytes: i64,
}

async fn purge_expired_cloud_blobs(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    library_id: &str,
) -> Result<(), String> {
    let due = application
        .library()
        .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
            let mut statement = connection.prepare(
                "SELECT object_key
                     FROM cloud_tombstone
                     WHERE object_kind = 'blob'
                       AND purge_after IS NOT NULL
                       AND purge_after <= ?1
                     ORDER BY purge_after, object_key
                     LIMIT 128",
            )?;
            let rows = statement
                .query_map([Utc::now().to_rfc3339()], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into);
            rows
        })
        .map_err(|error| error.to_string())?;

    for object_key in due {
        let (hash, extension) = object_key
            .split_once('.')
            .ok_or_else(|| format!("Invalid retained cloud blob key: {object_key}"))?;
        if hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || extension.is_empty()
            || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(format!("Invalid retained cloud blob key: {object_key}"));
        }
        let still_due = application
            .library()
            .auxiliary_read(picto_library::database::WorkPriority::Cloud, |connection| {
                connection
                    .query_row(
                        "SELECT EXISTS(
                                 SELECT 1 FROM cloud_tombstone tombstone
                                 WHERE tombstone.object_kind = 'blob'
                                   AND tombstone.object_key = ?1
                                   AND tombstone.purge_after IS NOT NULL
                                   AND NOT EXISTS(
                                       SELECT 1 FROM media_file
                                       WHERE content_hash = ?2
                                   )
                             )",
                        rusqlite::params![object_key, hash],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(Into::into)
            })
            .map_err(|error| error.to_string())?;
        if !still_due {
            continue;
        }
        let remote_path = format!(
            "picto/{library_id}/blobs/f/{}/{}/{}",
            &hash[..2],
            &hash[2..4],
            object_key
        );
        provider.delete(&remote_path).await?;
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Cloud,
                |transaction| {
                    // Keep the logical tombstone for offline peers, but mark
                    // the physical object as purged so it is not retried.
                    transaction.execute(
                        "UPDATE cloud_tombstone SET purge_after = NULL
                         WHERE object_kind = 'blob' AND object_key = ?1",
                        [&object_key],
                    )?;
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn sync_snapshot_blobs(
    application: &LibraryApplication,
    provider: &dyn CloudProvider,
    snapshot_database: &Path,
    library_id: &str,
) -> Result<(), String> {
    let total = count_snapshot_blobs(snapshot_database)?;
    if total == 0 {
        return Ok(());
    }
    set_sync_progress(application, 0, total, "Syncing files")?;

    let staging = application
        .root()
        .join("cloud")
        .join("staging")
        .join("blobs");
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create cloud blob staging directory: {error}"))?;
    let mut cursor = String::new();
    let mut completed = 0_i64;
    loop {
        let page = load_snapshot_blob_page(snapshot_database, &cursor)?;
        if page.is_empty() {
            break;
        }

        let mut settled = Vec::with_capacity(page.len());
        for blob in &page {
            let remote_path = blob_remote_path(library_id, blob)?;
            let local_path = application
                .blobs()
                .original_path_with_ext(&blob.hash, Some(&blob.extension))
                .map_err(|error| format!("Failed to resolve local original: {error}"))?;
            let outcome = if local_path.is_file() {
                if !provider.exists(&remote_path).await? {
                    provider
                        .upload_file(&remote_path, local_path, &blob.hash)
                        .await?;
                }
                Ok(())
            } else if provider.exists(&remote_path).await? {
                let download =
                    staging.join(format!("{}-{}.download", blob.hash, uuid::Uuid::new_v4()));
                let result = async {
                    provider
                        .download_file(&remote_path, download.clone(), &blob.hash)
                        .await?;
                    application
                        .blobs()
                        .write_original_from_path(&blob.hash, &download, Some(&blob.extension))
                        .map_err(|error| format!("Failed to restore cloud original: {error}"))
                }
                .await;
                let _ = std::fs::remove_file(download);
                result
            } else {
                Err("Original is missing locally and from the sync folder".to_string())
            };
            settled.push((blob.hash.clone(), blob.extension.clone(), outcome));
        }
        settle_blob_page(application, &settled)?;
        if let Some((hash, _, Err(error))) = settled.iter().find(|(_, _, result)| result.is_err()) {
            return Err(format!(
                "Cloud original {} could not be synced: {error}",
                &hash[..12]
            ));
        }
        completed += page.iter().map(|blob| blob.size_bytes.max(1)).sum::<i64>();
        set_sync_progress(application, completed, total, "Syncing files")?;
        cursor = page.last().expect("non-empty cloud blob page").hash.clone();
    }
    Ok(())
}

fn count_snapshot_blobs(snapshot_database: &Path) -> Result<i64, String> {
    let snapshot = Connection::open_with_flags(
        snapshot_database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|error| format!("Failed to inspect staged cloud snapshot: {error}"))?;
    snapshot
        .query_row(
            "SELECT COALESCE(SUM(MAX(file.size_bytes, 1)), 0)
             FROM media_file AS file
             LEFT JOIN cloud_blob_state AS blob ON blob.file_hash = file.content_hash
             WHERE blob.file_hash IS NULL OR blob.state != 'available' OR blob.remote_present = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to count cloud originals: {error}"))
}

fn load_snapshot_blob_page(
    snapshot_database: &Path,
    cursor: &str,
) -> Result<Vec<SnapshotBlob>, String> {
    let snapshot = Connection::open_with_flags(
        snapshot_database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|error| format!("Failed to inspect staged cloud snapshot: {error}"))?;
    let mut statement = snapshot
        .prepare(
            "SELECT file.content_hash, file.mime,
                    COALESCE(blob.remote_extension, ''), file.size_bytes
             FROM media_file AS file
             LEFT JOIN cloud_blob_state AS blob ON blob.file_hash = file.content_hash
             WHERE file.content_hash > ?1
               AND (blob.file_hash IS NULL OR blob.state != 'available'
                    OR blob.remote_present = 0)
             ORDER BY file.content_hash
             LIMIT 64",
        )
        .map_err(|error| format!("Failed to prepare cloud blob page: {error}"))?;
    let page = statement
        .query_map([cursor], |row| {
            let hash = row.get::<_, String>(0)?;
            let mime = row.get::<_, String>(1)?;
            let stored_extension = row.get::<_, String>(2)?;
            Ok(SnapshotBlob {
                hash,
                extension: if stored_extension.is_empty() {
                    crate::blob_store::mime_to_extension(&mime).to_string()
                } else {
                    stored_extension
                },
                size_bytes: row.get(3)?,
            })
        })
        .map_err(|error| format!("Failed to read cloud blob page: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("Failed to decode cloud blob page: {error}"))?;
    Ok(page)
}

fn blob_remote_path(library_id: &str, blob: &SnapshotBlob) -> Result<String, String> {
    if blob.hash.len() != 64 || !blob.hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("Invalid media content hash: {}", blob.hash));
    }
    Ok(format!(
        "picto/{library_id}/blobs/f/{}/{}/{}.{}",
        &blob.hash[..2],
        &blob.hash[2..4],
        blob.hash,
        blob.extension
    ))
}

fn settle_blob_page(
    application: &LibraryApplication,
    settled: &[(String, String, Result<(), String>)],
) -> Result<(), String> {
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
                let now = Utc::now().to_rfc3339();
                for (hash, extension, result) in settled {
                    if !transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM media_file WHERE content_hash = ?1)",
                        [hash],
                        |row| row.get::<_, bool>(0),
                    )? {
                        continue;
                    }
                    let (state, remote_present, error) = match result {
                        Ok(()) => ("available", 1_i64, None),
                        Err(error) => ("missing_remote", 0_i64, Some(error.as_str())),
                    };
                    transaction.execute(
                        "INSERT INTO cloud_blob_state
                             (file_hash, state, remote_present, remote_extension,
                              last_error, uploaded_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5,
                                 CASE WHEN ?3 = 1 THEN ?6 ELSE NULL END, ?6)
                         ON CONFLICT(file_hash) DO UPDATE SET
                             state = excluded.state,
                             remote_present = excluded.remote_present,
                             remote_extension = excluded.remote_extension,
                             last_error = excluded.last_error,
                             uploaded_at = excluded.uploaded_at,
                             updated_at = excluded.updated_at",
                        rusqlite::params![hash, state, remote_present, extension, error, now],
                    )?;
                }
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
}

fn set_sync_progress(
    application: &LibraryApplication,
    completed: i64,
    total: i64,
    message: &str,
) -> Result<(), String> {
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
                transaction.execute(
                    "UPDATE cloud_state
                     SET state = 'reconciling', phase = 'blobs',
                         message = CASE WHEN blocking = 1
                             THEN 'Restoring library media' ELSE ?1 END,
                         completed_units = ?2, total_units = ?3
                     WHERE singleton = 1",
                    rusqlite::params![message, completed, total],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
}

fn finish_blocking_restore(application: &LibraryApplication) -> Result<(), String> {
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET blocking = 0
                     WHERE singleton = 1 AND blocking = 1",
                    [],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
}

fn set_sync_state(
    application: &LibraryApplication,
    state: &str,
    phase: &str,
    message: &str,
) -> Result<(), String> {
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Cloud,
            |transaction| {
                transaction.execute(
                    "UPDATE cloud_state
                     SET state = ?1, phase = ?2,
                         message = CASE WHEN blocking = 1 AND ?1 = 'reconciling'
                             THEN message ELSE ?3 END,
                         blocking = CASE WHEN blocking = 1 AND ?1 = 'reconciling'
                             THEN 1 ELSE 0 END,
                         completed_units = CASE WHEN blocking = 1 AND ?1 = 'reconciling'
                             THEN completed_units ELSE 0 END,
                         total_units = CASE WHEN blocking = 1 AND ?1 = 'reconciling'
                             THEN total_units ELSE NULL END
                     WHERE singleton = 1",
                    rusqlite::params![state, phase, message],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
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

    #[test]
    fn blob_recovery_progress_counts_bytes_instead_of_files() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        let mut connection = Connection::open(&database_path).unwrap();
        picto_library::schema::create(&mut connection).unwrap();
        for (file_id, size_bytes, hash) in [(1, 4_000, "a"), (2, 8_000, "b")] {
            connection
                .execute(
                    "INSERT INTO media_file
                         (file_id, content_hash, file_path, mime, size_bytes)
                     VALUES (?1, ?2, ?3, 'image/png', ?4)",
                    rusqlite::params![
                        file_id,
                        hash.repeat(64),
                        format!("blobs/{hash}"),
                        size_bytes,
                    ],
                )
                .unwrap();
        }
        drop(connection);

        assert_eq!(count_snapshot_blobs(&database_path).unwrap(), 12_000);
        let page = load_snapshot_blob_page(&database_path, "").unwrap();
        assert_eq!(page.iter().map(|blob| blob.size_bytes).sum::<i64>(), 12_000);
    }

    #[tokio::test]
    async fn expired_blob_retention_removes_only_the_physical_cloud_object() {
        let library_root = tempfile::tempdir().unwrap();
        let cloud_root = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(library_root.path()).unwrap();
        let provider = DirectoryProvider::open(cloud_root.path()).unwrap();
        let hash = "a".repeat(64);
        let library_id = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT library_id FROM cloud_state WHERE singleton = 1",
                            [],
                            |row| row.get::<_, String>(0),
                        )
                        .map_err(Into::into)
                },
            )
            .unwrap();
        let object_key = format!("{hash}.png");
        let remote_path = format!(
            "picto/{library_id}/blobs/f/{}/{}/{}",
            &hash[..2],
            &hash[2..4],
            object_key
        );
        let bytes = b"retained cloud original".to_vec();
        let checksum = hex::encode(Sha256::digest(&bytes));
        provider
            .upload(&remote_path, bytes, &checksum)
            .await
            .unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Cloud,
                |transaction| {
                    transaction.execute(
                        "INSERT INTO cloud_tombstone
                             (object_kind, object_key, mutation_id, hlc_physical_ms,
                              hlc_logical, device_id, causal_frontier_json,
                              deleted_at, purge_after)
                         VALUES ('blob', ?1, 'delete-1', 1, 0, 'device-1', '{}',
                                 '2026-08-01T00:00:00Z', '2026-08-08T00:00:00Z')",
                        [&object_key],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        purge_expired_cloud_blobs(&application, &provider, &library_id)
            .await
            .unwrap();

        assert!(!provider.exists(&remote_path).await.unwrap());
        let purge_after = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT purge_after FROM cloud_tombstone
                             WHERE object_kind = 'blob' AND object_key = ?1",
                            [&object_key],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .map_err(Into::into)
                },
            )
            .unwrap();
        assert_eq!(purge_after, None);
    }

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
                        "UPDATE cloud_state
                         SET provider = 'google_drive', paused = 0, state = 'idle'
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

    #[tokio::test]
    async fn publication_uploads_and_recovers_original_blobs() {
        let library_root = tempfile::tempdir().unwrap();
        let cloud_root = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(library_root.path()).unwrap();
        let bytes = b"canonical cloud original";
        let hash = hex::encode(Sha256::digest(bytes));
        application
            .blobs()
            .write_original(&hash, bytes, Some("png"))
            .unwrap();
        let local_path = application
            .blobs()
            .original_path_with_ext(&hash, Some("png"))
            .unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Cloud,
                |transaction| {
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, content_hash, file_path, mime, size_bytes)
                         VALUES (1, ?1, ?2, 'image/png', ?3)",
                        rusqlite::params![hash, local_path.to_string_lossy(), bytes.len() as i64],
                    )?;
                    transaction.execute(
                        "INSERT INTO cloud_blob_state
                             (file_hash, state, remote_present, remote_extension, updated_at)
                         VALUES (?1, 'available', 0, 'png', 'incorrect-old-state')",
                        [&hash],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        let provider = DirectoryProvider::open(cloud_root.path()).unwrap();
        let manifest = publish_library(&application, &provider).await.unwrap();
        let remote_path = cloud_root
            .path()
            .join("picto")
            .join(&manifest.library_id)
            .join("blobs/f")
            .join(&hash[..2])
            .join(&hash[2..4])
            .join(format!("{hash}.png"));
        assert_eq!(std::fs::read(&remote_path).unwrap(), bytes);
        let state = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT state, remote_present, remote_extension
                             FROM cloud_blob_state WHERE file_hash = ?1",
                            [&hash],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, i64>(1)?,
                                    row.get::<_, String>(2)?,
                                ))
                            },
                        )
                        .map_err(Into::into)
                },
            )
            .unwrap();
        assert_eq!(state, ("available".into(), 1, "png".into()));

        std::fs::remove_file(&local_path).unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Cloud,
                |transaction| {
                    transaction.execute(
                        "UPDATE cloud_blob_state SET state = 'queued' WHERE file_hash = ?1",
                        [&hash],
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        publish_library(&application, &provider).await.unwrap();
        assert_eq!(std::fs::read(local_path).unwrap(), bytes);
    }
}
