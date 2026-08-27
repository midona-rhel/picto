use std::collections::HashMap;

use chrono::Utc;
use rusqlite::params;
use sha2::{Digest, Sha256};

use super::provider::CloudProvider;
use crate::app::Application;
use crate::blob_store::mime_to_extension;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlobSyncResult {
    pub uploaded: usize,
    pub downloaded: usize,
    pub missing_remote: usize,
    pub corrupt: usize,
}

pub fn remote_path(library_id: &str, hash: &str, extension: &str) -> Result<String, String> {
    if hash.len() != 64 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("Invalid cloud blob hash: {hash}"));
    }
    Ok(format!(
        "picto/{library_id}/blobs/f/{}/{}/{}.{}",
        &hash[0..2],
        &hash[2..4],
        hash,
        extension
    ))
}

/// Register originals that predate cloud configuration without reading their
/// bytes into SQLite. BlobStore remains the only owner of local originals.
pub fn seed_local_originals(application: &Application) -> Result<usize, String> {
    let originals = application
        .blobs()
        .list_originals()
        .into_iter()
        .collect::<HashMap<_, _>>();
    application
        .store()
        .transaction_cloud(|transaction| {
            let now = Utc::now().to_rfc3339();
            let mut statement = transaction.prepare("SELECT file_hash FROM media_file")?;
            let hashes = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(statement);
            let mut changed = 0;
            for hash in hashes {
                let Some(extension) = originals.get(&hash) else {
                    continue;
                };
                changed += transaction.execute(
                    "INSERT INTO cloud_blob_state
                         (file_hash, state, remote_extension, updated_at)
                     VALUES (?1, 'available', ?2, ?3)
                     ON CONFLICT(file_hash) DO UPDATE SET
                         state = 'available',
                         remote_extension = COALESCE(cloud_blob_state.remote_extension, excluded.remote_extension),
                         last_error = NULL,
                         updated_at = excluded.updated_at",
                    params![hash, extension, now],
                )?;
            }
            Ok(changed)
        })
        .map(|(changed, _)| changed)
}

pub async fn upload_pending(
    application: &Application,
    provider: &dyn CloudProvider,
    limit: usize,
) -> Result<BlobSyncResult, String> {
    let (library_id, candidates) = candidates(
        application,
        "b.state = 'available' AND b.remote_present = 0",
        limit,
    )?;
    if candidates.is_empty() {
        return Ok(BlobSyncResult::default());
    }
    let mut result = BlobSyncResult::default();
    for candidate in candidates {
        let extension = candidate.extension();
        let Some((path, _)) = application
            .blobs()
            .find_original(&candidate.hash, Some(&extension))
            .map_err(|error| format!("Failed to locate local blob {}: {error}", candidate.hash))?
        else {
            set_state(
                application,
                &candidate.hash,
                "corrupt",
                Some("Original is missing locally"),
                None,
            )?;
            result.corrupt += 1;
            continue;
        };
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read local blob {}: {error}", candidate.hash))?;
        if hex::encode(Sha256::digest(&bytes)) != candidate.hash {
            set_state(
                application,
                &candidate.hash,
                "corrupt",
                Some("Local original checksum failed"),
                None,
            )?;
            result.corrupt += 1;
            continue;
        }
        provider
            .upload(
                &remote_path(&library_id, &candidate.hash, &extension)?,
                bytes,
                &candidate.hash,
            )
            .await?;
        mark_uploaded(application, &candidate.hash, &extension)?;
        result.uploaded += 1;
    }
    refresh_counts(application)?;
    Ok(result)
}

pub async fn recover_pending(
    application: &Application,
    provider: &dyn CloudProvider,
    limit: usize,
) -> Result<BlobSyncResult, String> {
    let retry_before = (Utc::now() - chrono::Duration::seconds(30)).to_rfc3339();
    let (library_id, candidates) = recovery_candidates(application, &retry_before, limit)?;
    if candidates.is_empty() {
        return Ok(BlobSyncResult::default());
    }
    let mut result = BlobSyncResult::default();
    for candidate in candidates {
        let extension = candidate.extension();
        let path = remote_path(&library_id, &candidate.hash, &extension)?;
        let bytes = match provider.download(&path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                set_state(
                    application,
                    &candidate.hash,
                    "missing_remote",
                    Some(&error),
                    Some(&extension),
                )?;
                result.missing_remote += 1;
                continue;
            }
        };
        set_state(
            application,
            &candidate.hash,
            "downloading",
            None,
            Some(&extension),
        )?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if actual != candidate.hash {
            set_state(
                application,
                &candidate.hash,
                "corrupt",
                Some("Cloud original checksum failed"),
                Some(&extension),
            )?;
            result.corrupt += 1;
            continue;
        }
        application
            .blobs()
            .write_original(&candidate.hash, &bytes, Some(&extension))
            .map_err(|error| {
                format!(
                    "Failed to promote recovered blob {}: {error}",
                    candidate.hash
                )
            })?;
        mark_available(application, &candidate.hash, &extension)?;
        result.downloaded += 1;
    }
    refresh_counts(application)?;
    Ok(result)
}

fn recovery_candidates(
    application: &Application,
    retry_before: &str,
    limit: usize,
) -> Result<(String, Vec<Candidate>), String> {
    application.store().read(|connection| {
        let library_id = connection.query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let mut rows = Vec::with_capacity(limit);
        for (state, threshold) in [
            ("queued", None),
            ("missing_remote", Some(retry_before)),
            ("corrupt", Some(retry_before)),
        ] {
            if rows.len() == limit {
                break;
            }
            let remaining = (limit - rows.len()) as i64;
            let sql = if threshold.is_some() {
                "SELECT b.file_hash, f.mime_type, b.remote_extension
                 FROM cloud_blob_state b
                 JOIN media_file f ON f.file_hash = b.file_hash
                 WHERE b.state = ?1 AND b.updated_at <= ?2
                 ORDER BY b.priority DESC, b.updated_at
                 LIMIT ?3"
            } else {
                "SELECT b.file_hash, f.mime_type, b.remote_extension
                 FROM cloud_blob_state b
                 JOIN media_file f ON f.file_hash = b.file_hash
                 WHERE b.state = ?1
                 ORDER BY b.priority DESC, b.updated_at, b.file_hash
                 LIMIT ?3"
            };
            let mut statement = connection.prepare(sql)?;
            let mapped = statement.query_map(params![state, threshold, remaining], |row| {
                Ok(Candidate {
                    hash: row.get(0)?,
                    mime_type: row.get(1)?,
                    remote_extension: row.get(2)?,
                })
            })?;
            rows.extend(mapped.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        Ok((library_id, rows))
    })
}

pub fn prioritize(
    application: &Application,
    hashes: &[String],
    priority: i64,
) -> Result<(), String> {
    application.store().transaction_cloud(|transaction| {
        let now = Utc::now().to_rfc3339();
        for hash in hashes {
            transaction.execute(
                "UPDATE cloud_blob_state SET priority = ?1, updated_at = ?2 WHERE file_hash = ?3",
                params![priority, now, hash],
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

struct Candidate {
    hash: String,
    mime_type: String,
    remote_extension: Option<String>,
}

impl Candidate {
    fn extension(&self) -> String {
        self.remote_extension
            .clone()
            .unwrap_or_else(|| mime_to_extension(&self.mime_type).to_string())
    }
}

fn candidates(
    application: &Application,
    predicate: &str,
    limit: usize,
) -> Result<(String, Vec<Candidate>), String> {
    application.store().read(|connection| {
        let library_id = connection.query_row(
            "SELECT library_id FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let sql = format!(
            "SELECT b.file_hash, f.mime_type, b.remote_extension
             FROM cloud_blob_state b
             JOIN media_file f ON f.file_hash = b.file_hash
             WHERE {predicate}
             ORDER BY b.priority DESC, b.updated_at
             LIMIT ?1"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(Candidate {
                    hash: row.get(0)?,
                    mime_type: row.get(1)?,
                    remote_extension: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((library_id, rows))
    })
}

fn set_state(
    application: &Application,
    hash: &str,
    state: &str,
    error: Option<&str>,
    extension: Option<&str>,
) -> Result<(), String> {
    application.store().transaction_cloud(|transaction| {
        transaction.execute(
            "UPDATE cloud_blob_state SET state = ?1, last_error = ?2,
                    remote_extension = COALESCE(?3, remote_extension), updated_at = ?4
             WHERE file_hash = ?5",
            params![state, error, extension, Utc::now().to_rfc3339(), hash],
        )?;
        Ok(())
    })?;
    Ok(())
}

fn mark_uploaded(application: &Application, hash: &str, extension: &str) -> Result<(), String> {
    application.store().transaction_cloud(|transaction| {
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE cloud_blob_state SET remote_present = 1, remote_extension = ?1,
                    uploaded_at = ?2, last_error = NULL, updated_at = ?2
             WHERE file_hash = ?3",
            params![extension, now, hash],
        )?;
        Ok(())
    })?;
    Ok(())
}

fn mark_available(application: &Application, hash: &str, extension: &str) -> Result<(), String> {
    application.store().transaction_cloud(|transaction| {
        transaction.execute(
            "UPDATE cloud_blob_state SET state = 'available', remote_present = 1,
                    remote_extension = ?1, last_error = NULL, updated_at = ?2
             WHERE file_hash = ?3",
            params![extension, Utc::now().to_rfc3339(), hash],
        )?;
        Ok(())
    })?;
    Ok(())
}

fn refresh_counts(application: &Application) -> Result<(), String> {
    application
        .store()
        .transaction_cloud(|transaction| {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cloud::provider::DirectoryProvider;
    use crate::store::Store;

    fn application(root: &std::path::Path, library_id: &str, device_id: &str) -> Application {
        let application = Application::new(Arc::new(Store::open(root).unwrap()));
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE cloud_state SET provider = 'dropbox', library_id = ?1, device_id = ?2 WHERE singleton = 1",
                    params![library_id, device_id],
                )?;
                Ok(())
            })
            .unwrap();
        application
    }

    #[tokio::test]
    async fn uploads_once_and_recovers_on_another_device() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let first = application(first_root.path(), "library-a", "device-a");
        let second = application(second_root.path(), "library-a", "device-b");
        let provider = DirectoryProvider::open(remote_root.path()).unwrap();
        let bytes = b"durable original";
        let hash = hex::encode(Sha256::digest(bytes));
        for application in [&first, &second] {
            application
                .store()
                .transaction(|transaction| {
                    transaction.execute(
                        "INSERT INTO media_file (file_hash, mime_type, size_bytes, created_at)
                         VALUES (?1, 'application/octet-stream', ?2, 'now')",
                        params![hash, bytes.len() as i64],
                    )?;
                    Ok(())
                })
                .unwrap();
        }
        first
            .blobs()
            .write_original(&hash, bytes, Some("bin"))
            .unwrap();
        seed_local_originals(&first).unwrap();
        second
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO cloud_blob_state (file_hash, state, remote_extension, updated_at)
                     VALUES (?1, 'queued', 'bin', 'now')",
                    [&hash],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(
            upload_pending(&first, &provider, 8).await.unwrap().uploaded,
            1
        );
        assert_eq!(
            upload_pending(&first, &provider, 8).await.unwrap().uploaded,
            0
        );
        assert_eq!(
            recover_pending(&second, &provider, 8)
                .await
                .unwrap()
                .downloaded,
            1
        );
        assert_eq!(
            second.blobs().read_original(&hash, Some("bin")).unwrap(),
            bytes
        );
    }
}
