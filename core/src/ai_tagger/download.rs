//! Model download with progress tracking via RuntimeTask events.

use std::path::{Path, PathBuf};

use crate::runtime_contract::task::{RuntimeTask, TaskKind, TaskProgress, TaskStatus};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// Download a tagger model (ONNX + labels CSV) to the given directory.
///
/// Emits `runtime/task_upserted` events for progress tracking.
pub async fn download_model(
    slug: &str,
    models_root: &Path,
    cancel: &CancellationToken,
    lifecycle: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    download_model_inner(slug, models_root, cancel, lifecycle, true).await
}

pub async fn download_model_quiet(
    slug: &str,
    models_root: &Path,
    cancel: &CancellationToken,
    lifecycle: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    download_model_inner(slug, models_root, cancel, lifecycle, false).await
}

async fn download_model_inner(
    slug: &str,
    models_root: &Path,
    cancel: &CancellationToken,
    lifecycle: &tokio::sync::Mutex<()>,
    report_legacy_task: bool,
) -> Result<(), String> {
    let model_info =
        super::models::find_model(slug).ok_or_else(|| format!("Unknown model: {slug}"))?;

    std::fs::create_dir_all(models_root)
        .map_err(|e| format!("Failed to create model root: {e}"))?;
    let model_dir = super::models::model_dir(models_root, &model_info);
    recover_interrupted_download(models_root, &model_info.slug, &model_dir)?;
    let temp_dir = create_temp_bundle_dir(models_root, slug)?;

    let task_id = format!("model_download:{slug}");

    // Publish start
    if report_legacy_task {
        emit_task(&task_id, slug, TaskStatus::Running, None);
    }

    // Download ONNX model
    let onnx_path = temp_dir.join("model.onnx");

    let result = async {
        download_file(
            &model_info.onnx_url,
            &model_info.onnx_sha256,
            &onnx_path,
            &task_id,
            slug,
            cancel,
            report_legacy_task,
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
            &task_id,
            slug,
            cancel,
            report_legacy_task,
        )
        .await
        .map_err(|e| format!("Failed to download labels CSV: {e}"))?;
        cancelled(cancel)?;

        // Validate the complete pair before it can become visible to the app.
        let labels = super::labels::parse_labels_csv(&labels_path)?;
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
            if report_legacy_task {
                emit_task(&task_id, slug, TaskStatus::Finished, None);
                crate::runtime_state::remove_task(&task_id);
            }
            tracing::info!(slug, "Model download complete");
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            if report_legacy_task && cancel.is_cancelled() {
                crate::runtime_state::remove_task(&task_id);
            } else if report_legacy_task {
                emit_task(
                    &task_id,
                    slug,
                    TaskStatus::Failed,
                    Some(TaskProgress {
                        done: 0,
                        total: 0,
                        status_text: Some(error.clone()),
                    }),
                );
                crate::runtime_state::remove_task(&task_id);
            }
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

/// Stream-download a URL to a file, reporting progress.
async fn download_file(
    url: &str,
    expected_sha256: &str,
    dest: &Path,
    task_id: &str,
    slug: &str,
    cancel: &CancellationToken,
    report_legacy_task: bool,
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

    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut last_progress_pct: u64 = 0;
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
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;

        // Emit progress at most every 1%
        if total > 0 {
            let pct = (downloaded * 100) / total;
            if report_legacy_task && pct > last_progress_pct {
                last_progress_pct = pct;
                emit_task(
                    task_id,
                    slug,
                    TaskStatus::Running,
                    Some(TaskProgress {
                        done: downloaded,
                        total,
                        status_text: Some(format!(
                            "{}MB / {}MB",
                            downloaded / (1024 * 1024),
                            total / (1024 * 1024)
                        )),
                    }),
                );
            }
        }
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

fn cancelled(cancel: &CancellationToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("Model download cancelled".into())
    } else {
        Ok(())
    }
}

fn emit_task(task_id: &str, slug: &str, status: TaskStatus, progress: Option<TaskProgress>) {
    let task = RuntimeTask {
        task_id: task_id.to_string(),
        kind: TaskKind::ModelDownload,
        status,
        label: format!("Downloading model: {slug}"),
        parent_task_id: None,
        progress,
        detail: None,
        started_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    crate::runtime_state::upsert_task(task);
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
}
