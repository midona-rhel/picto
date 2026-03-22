//! Model download with progress tracking via RuntimeTask events.

use std::path::Path;

use crate::runtime_contract::task::{RuntimeTask, TaskKind, TaskProgress, TaskStatus};

/// Download a tagger model (ONNX + labels CSV) to the given directory.
///
/// Emits `runtime/task_upserted` events for progress tracking.
pub async fn download_model(slug: &str, models_root: &Path) -> Result<(), String> {
    let model_info =
        super::models::find_model(slug).ok_or_else(|| format!("Unknown model: {slug}"))?;

    let model_dir = super::models::model_dir(models_root, slug);
    std::fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create model directory: {e}"))?;

    let task_id = format!("model_download:{slug}");

    // Publish start
    emit_task(&task_id, slug, TaskStatus::Running, None);

    // Download ONNX model
    let onnx_path = model_dir.join("model.onnx");
    let onnx_part = model_dir.join("model.onnx.part");

    if let Err(e) = download_file(&model_info.onnx_url, &onnx_part, &task_id, slug).await {
        emit_task(&task_id, slug, TaskStatus::Failed, None);
        schedule_task_removal(&task_id);
        return Err(format!("Failed to download model ONNX: {e}"));
    }
    std::fs::rename(&onnx_part, &onnx_path)
        .map_err(|e| format!("Failed to finalise model file: {e}"))?;

    // Download labels CSV
    let labels_path = model_dir.join("selected_tags.csv");
    let labels_part = model_dir.join("selected_tags.csv.part");

    if let Err(e) = download_file(&model_info.labels_url, &labels_part, &task_id, slug).await {
        emit_task(&task_id, slug, TaskStatus::Failed, None);
        schedule_task_removal(&task_id);
        return Err(format!("Failed to download labels CSV: {e}"));
    }
    std::fs::rename(&labels_part, &labels_path)
        .map_err(|e| format!("Failed to finalise labels file: {e}"))?;

    emit_task(&task_id, slug, TaskStatus::Finished, None);
    schedule_task_removal(&task_id);

    tracing::info!(slug, "Model download complete");
    Ok(())
}

/// Stream-download a URL to a file, reporting progress.
async fn download_file(url: &str, dest: &Path, task_id: &str, slug: &str) -> Result<(), String> {
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

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += chunk.len() as u64;

        // Emit progress at most every 1%
        if total > 0 {
            let pct = (downloaded * 100) / total;
            if pct > last_progress_pct {
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
    Ok(())
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

fn schedule_task_removal(task_id: &str) {
    let id = task_id.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        crate::runtime_state::remove_task(&id);
    });
}
