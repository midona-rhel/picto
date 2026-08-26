//! Atomic model download and activation.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

pub async fn download_model_quiet(
    slug: &str,
    models_root: &Path,
    cancel: &CancellationToken,
    downloaded_bytes: Arc<AtomicU64>,
    lifecycle: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    download_model_inner(slug, models_root, cancel, downloaded_bytes, lifecycle).await
}

#[cfg(target_os = "macos")]
pub(crate) async fn download_coreml_package(
    artifact: &super::models::RegisteredArtifact,
    destination: &Path,
    cancel: &CancellationToken,
    downloaded_bytes: &AtomicU64,
) -> Result<(), String> {
    std::fs::create_dir_all(destination)
        .map_err(|error| format!("Failed to create Core ML staging directory: {error}"))?;
    let archive = destination.join("coreml.zip");
    if let Some(directory) = std::env::var_os("PICTO_COREML_ASSET_DIR") {
        let filename = artifact
            .url
            .rsplit('/')
            .next()
            .ok_or_else(|| "Registered Core ML URL has no filename".to_string())?;
        let source = PathBuf::from(directory).join(filename);
        std::fs::copy(&source, &archive).map_err(|error| {
            format!(
                "Failed to stage local Core ML asset {}: {error}",
                source.display()
            )
        })?;
        verify_file(&archive, &artifact.sha256)?;
        downloaded_bytes.store(artifact.size, Ordering::Relaxed);
    } else {
        download_file(
            &artifact.url,
            &artifact.sha256,
            &archive,
            cancel,
            downloaded_bytes,
        )
        .await
        .map_err(|error| format!("Failed to download Core ML model: {error}"))?;
    }
    let actual_size = std::fs::metadata(&archive)
        .map_err(|error| format!("Failed to inspect Core ML archive: {error}"))?
        .len();
    if actual_size != artifact.size {
        return Err(format!(
            "Core ML archive size mismatch: expected {}, got {actual_size}",
            artifact.size
        ));
    }
    extract_coreml_archive(&archive, destination, artifact.size)?;
    std::fs::remove_file(&archive)
        .map_err(|error| format!("Failed to remove Core ML archive: {error}"))?;
    Ok(())
}

async fn download_model_inner(
    slug: &str,
    models_root: &Path,
    cancel: &CancellationToken,
    downloaded_bytes: Arc<AtomicU64>,
    lifecycle: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    let model_info =
        super::models::find_model(slug).ok_or_else(|| format!("Unknown model: {slug}"))?;

    std::fs::create_dir_all(models_root)
        .map_err(|e| format!("Failed to create model root: {e}"))?;
    let model_dir = super::models::model_dir(models_root, &model_info);
    recover_interrupted_download(models_root, &model_info.slug, &model_dir)?;
    let temp_dir = create_temp_bundle_dir(models_root, slug)?;

    // Download ONNX model
    let onnx_path = temp_dir.join("model.onnx");

    let result = async {
        download_file(
            &model_info.onnx_url,
            &model_info.onnx_sha256,
            &onnx_path,
            cancel,
            &downloaded_bytes,
        )
        .await
        .map_err(|e| format!("Failed to download model ONNX: {e}"))?;
        cancelled(cancel)?;

        // Normalize both registry variants to the filename used by the active bundle.
        let labels_path = temp_dir.join("selected_tags.csv");
        download_file(
            &model_info.labels_url,
            &model_info.labels_sha256,
            &labels_path,
            cancel,
            &downloaded_bytes,
        )
        .await
        .map_err(|e| format!("Failed to download labels CSV: {e}"))?;
        cancelled(cancel)?;

        if let Some(categories) = &model_info.label_categories {
            download_file(
                &categories.url,
                &categories.sha256,
                &temp_dir.join("label-categories.json"),
                cancel,
                &downloaded_bytes,
            )
            .await
            .map_err(|e| format!("Failed to download label categories: {e}"))?;
            cancelled(cancel)?;
        }

        // Validate the portable pair before adding any platform-specific
        // optimization. A model download must never hide a Core ML compile.
        let labels = super::labels::parse_model_labels(&temp_dir, model_info.adapter)?;
        if labels.is_empty() {
            return Err("Downloaded labels CSV is empty".into());
        }
        let validation_dir = temp_dir.clone();
        let validation_slug = slug.to_string();
        let input_size = model_info.input_size;
        let channel_order = model_info.channel_order;
        let session = tokio::task::spawn_blocking(move || {
            super::inference::TaggerSession::load(
                &validation_dir,
                &validation_slug,
                input_size,
                channel_order,
                model_info.output_activation,
                model_info.adapter,
            )
        })
        .await
        .map_err(|error| format!("Model validation task failed: {error}"))??;
        drop(session);
        cancelled(cancel)?;

        super::models::mark_bundle_validated(&temp_dir, &model_info)?;
        let _lifecycle = lifecycle.lock().await;
        activate_bundle(&temp_dir, &model_dir, models_root, slug)?;
        Ok::<(), String>(())
    }
    .await;

    match result {
        Ok(()) => {
            tracing::info!(slug, "Model download complete");
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            Err(error)
        }
    }
}

fn recover_interrupted_download(
    models_root: &Path,
    slug: &str,
    model_dir: &Path,
) -> Result<(), String> {
    let download_prefix = format!(".{slug}.download-");
    let backup_prefix = format!(".{slug}.previous-");
    let mut backups = Vec::new();
    for entry in std::fs::read_dir(models_root)
        .map_err(|error| format!("Failed to inspect model root: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to inspect model root: {error}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Failed to inspect staged model bundle: {error}"))?;
        if !file_type.is_dir() {
            continue;
        }
        if name.starts_with(&download_prefix) {
            std::fs::remove_dir_all(entry.path())
                .map_err(|error| format!("Failed to remove stale model download: {error}"))?;
        } else if name.starts_with(&backup_prefix) {
            backups.push(entry.path());
        }
    }

    backups.sort();
    if !model_dir.exists() {
        if let Some(backup) = backups.pop() {
            std::fs::rename(&backup, model_dir)
                .map_err(|error| format!("Failed to restore previous model bundle: {error}"))?;
        }
    }
    for backup in backups {
        std::fs::remove_dir_all(backup)
            .map_err(|error| format!("Failed to remove stale model backup: {error}"))?;
    }
    Ok(())
}

/// Recover interrupted bundle swaps and reject corrupted active bundles once
/// when a library opens. Readiness checks remain cheap during normal use.
pub async fn recover_registered_bundles(models_root: &Path) -> Result<(), String> {
    let models_root = models_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&models_root)
            .map_err(|error| format!("Failed to create model root: {error}"))?;
        for model in super::models::known_models() {
            let active = super::models::model_dir(&models_root, &model);
            recover_interrupted_download(&models_root, &model.slug, &active)?;
            if active.exists() {
                if let Err(error) = super::models::validate_bundle_integrity(&active, &model) {
                    tracing::warn!(slug = model.slug, error = %error, "Removing invalid AI model bundle");
                    std::fs::remove_dir_all(&active).map_err(|remove_error| {
                        format!(
                            "{error}; failed to remove invalid model bundle: {remove_error}"
                        )
                    })?;
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Model recovery task failed: {error}"))?
}

fn create_temp_bundle_dir(models_root: &Path, slug: &str) -> Result<PathBuf, String> {
    for _ in 0..10 {
        let candidate =
            models_root.join(format!(".{slug}.download-{:032x}", rand::random::<u128>()));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Failed to create temporary model directory: {error}"
                ))
            }
        }
    }
    Err("Failed to create a unique temporary model directory".into())
}

fn activate_bundle(
    temp_dir: &Path,
    model_dir: &Path,
    models_root: &Path,
    slug: &str,
) -> Result<(), String> {
    let backup_dir = models_root.join(format!(".{slug}.previous-{:032x}", rand::random::<u128>()));
    let had_active = model_dir.exists();

    if had_active {
        std::fs::rename(model_dir, &backup_dir)
            .map_err(|e| format!("Failed to stage the previous model bundle: {e}"))?;
    }

    if let Err(error) = std::fs::rename(temp_dir, model_dir) {
        if had_active {
            let _ = std::fs::rename(&backup_dir, model_dir);
        }
        return Err(format!("Failed to activate model bundle: {error}"));
    }

    if had_active {
        if let Err(error) = std::fs::remove_dir_all(&backup_dir) {
            tracing::warn!(slug, error = %error, "Activated model bundle but could not remove old bundle");
        }
    }
    Ok(())
}

/// Stream-download a URL to a file and verify its digest.
async fn download_file(
    url: &str,
    expected_sha256: &str,
    dest: &Path,
    cancel: &CancellationToken,
    downloaded_bytes: &AtomicU64,
) -> Result<(), String> {
    cancelled(cancel)?;
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}: {}", resp.status(), url));
    }

    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut hasher = Sha256::new();

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err("Model download cancelled".into()),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|e| format!("Download stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        hasher.update(&chunk);
    }

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {e}"))?;
    file.sync_all()
        .await
        .map_err(|e| format!("Sync error: {e}"))?;
    let actual_sha256 = hex::encode(hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        ));
    }
    Ok(())
}

fn verify_file(path: &Path, expected_sha256: &str) -> Result<(), String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual_sha256 = hex::encode(hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn extract_coreml_archive(
    archive: &Path,
    destination: &Path,
    archive_size: u64,
) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|error| format!("Failed to open Core ML archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Failed to read Core ML archive: {error}"))?;
    let mut extracted_size = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Failed to read Core ML archive entry: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "Core ML archive contains an unsafe path".to_string())?;
        if !relative.starts_with("model.mlpackage") {
            return Err(format!(
                "Core ML archive contains an unexpected entry: {}",
                relative.display()
            ));
        }
        extracted_size = extracted_size.saturating_add(entry.size());
        if extracted_size > archive_size.saturating_mul(2) {
            return Err("Core ML archive expands beyond its registered size limit".into());
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| format!("Failed to create Core ML directory: {error}"))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create Core ML directory: {error}"))?;
        }
        let mut output_file = std::fs::File::create(&output)
            .map_err(|error| format!("Failed to create Core ML file: {error}"))?;
        std::io::copy(&mut entry, &mut output_file)
            .map_err(|error| format!("Failed to extract Core ML file: {error}"))?;
    }
    if !destination.join("model.mlpackage/Manifest.json").is_file() {
        return Err("Core ML archive did not contain a model package".into());
    }
    Ok(())
}

fn cancelled(cancel: &CancellationToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("Model download cancelled".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_tagger::models;
    use tempfile::TempDir;

    #[test]
    fn temporary_bundle_is_a_unique_sibling_of_active_bundle() {
        let root = TempDir::new().unwrap();
        let first = create_temp_bundle_dir(root.path(), "wd14-swinv2-v3").unwrap();
        let second = create_temp_bundle_dir(root.path(), "wd14-swinv2-v3").unwrap();

        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(root.path()));
        assert_eq!(second.parent(), Some(root.path()));
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".wd14-swinv2-v3.download-"));
        assert!(second
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".wd14-swinv2-v3.download-"));
    }

    #[test]
    fn activation_only_exposes_the_complete_staged_directory() {
        let root = TempDir::new().unwrap();
        let temp = create_temp_bundle_dir(root.path(), "wd14-swinv2-v3").unwrap();
        let model = models::find_model("wd14-swinv2-v3").unwrap();
        let active = models::model_dir(root.path(), &model);
        std::fs::write(temp.join("model.onnx"), b"model").unwrap();
        std::fs::write(temp.join("selected_tags.csv"), b"labels").unwrap();
        #[cfg(target_os = "macos")]
        std::fs::create_dir_all(temp.join("model.mlpackage")).unwrap();
        models::mark_bundle_validated(&temp, &model).unwrap();

        activate_bundle(&temp, &active, root.path(), "wd14-swinv2-v3").unwrap();
        assert!(!temp.exists());
        assert!(models::bundle_is_marked(&active, &model));
    }

    #[test]
    fn cancellation_is_detected_before_activation() {
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(cancelled(&token).unwrap_err(), "Model download cancelled");
    }

    #[test]
    fn restart_cleanup_restores_backup_and_removes_partial_download() {
        let root = TempDir::new().unwrap();
        let model = models::find_model("wd14-swinv2-v3").unwrap();
        let active = models::model_dir(root.path(), &model);
        let backup = root.path().join(format!(".{}.previous-1", model.slug));
        let partial = root.path().join(format!(".{}.download-1", model.slug));
        std::fs::create_dir(&backup).unwrap();
        std::fs::write(backup.join("old"), b"old").unwrap();
        std::fs::create_dir(&partial).unwrap();

        recover_interrupted_download(root.path(), &model.slug, &active).unwrap();

        assert!(active.join("old").is_file());
        assert!(!partial.exists());
        assert!(!backup.exists());
    }

    #[tokio::test]
    async fn startup_recovery_removes_a_corrupted_registered_bundle() {
        let root = TempDir::new().unwrap();
        let model = models::find_model("wd14-swinv2-v3").unwrap();
        let active = models::model_dir(root.path(), &model);
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(active.join("model.onnx"), b"corrupt").unwrap();
        std::fs::write(active.join("selected_tags.csv"), b"corrupt").unwrap();
        models::mark_bundle_validated(&active, &model).unwrap();

        recover_registered_bundles(root.path()).await.unwrap();

        assert!(!active.exists());
        assert!(!models::is_model_downloaded(root.path(), &model.slug));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn coreml_extraction_rejects_files_outside_the_model_package() {
        use zip::write::SimpleFileOptions;

        let root = TempDir::new().unwrap();
        let archive_path = root.path().join("invalid.zip");
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file("unexpected.txt", SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut archive, b"unexpected").unwrap();
        archive.finish().unwrap();

        let error = extract_coreml_archive(&archive_path, root.path(), 1024).unwrap_err();
        assert!(error.contains("unexpected entry"));
    }
}
