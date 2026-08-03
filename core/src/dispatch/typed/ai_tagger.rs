//! Dispatch handlers for AI tagger commands.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ai_tagger::inference::{TagPrediction, Thresholds};
use crate::ai_tagger::models::ModelInfo;
use crate::db::types::{EntityTarget, EntityTargetKind, TAG_PROVENANCE_AI};
use crate::engine::tags::TagOperation;
use crate::runtime_contract::task::{RuntimeTask, TaskKind, TaskProgress, TaskStatus};
use crate::settings::store::AppSettings;
use crate::state::AppState;
use tokio_util::sync::CancellationToken;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Task id of the singleton auto-tag progress task.
const AUTO_TAG_TASK_ID: &str = "auto_tag";

/// Predictions below this confidence are dropped outright; the panel applies
/// the user's cutoff client-side anywhere above this floor without
/// re-running inference.
const PREDICT_FLOOR: f32 = 0.10;

/// Whether a model slug is enabled in settings.
fn model_enabled(settings: &AppSettings, slug: &str) -> bool {
    match slug {
        "wd14-swinv2-v3" => settings.ai_tagger_wd14_enabled,
        "z3d-e621-convnext" => settings.ai_tagger_e621_enabled,
        "wd14-eva02-large-v3" => settings.ai_tagger_eva02_enabled,
        _ => false,
    }
}

/// Registry slugs enabled in settings, in registry order.
fn enabled_slugs(settings: &AppSettings) -> Vec<String> {
    crate::ai_tagger::models::known_models()
        .into_iter()
        .filter(|m| model_enabled(settings, &m.slug))
        .map(|m| m.slug)
        .collect()
}

// ─── Input / Output structs ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AiTaggerStatusInput {}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "camelCase")]
pub struct AiTaggerModelStatus {
    pub slug: String,
    pub label: String,
    pub enabled: bool,
    pub downloaded: bool,
    /// Recommended default for the current machine.
    pub recommended: bool,
    /// Accuracy-over-speed model; flagged in the UI on modest hardware.
    pub heavy: bool,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub dataset: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "camelCase")]
pub struct AiTaggerHardware {
    pub cpu_model: Option<String>,
    #[ts(type = "number")]
    pub logical_cores: u32,
    #[ts(type = "number | null")]
    pub memory_bytes: Option<u64>,
    /// Execution provider ONNX Runtime is using (e.g. "CPU").
    pub execution_provider: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "camelCase")]
pub struct AiTaggerStatusOutput {
    pub models: Vec<AiTaggerModelStatus>,
    pub gpu_backend: Option<String>,
    pub available_models: Vec<ModelInfo>,
    pub hardware: AiTaggerHardware,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AiTaggerDownloadModelInput {
    pub model: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AiTagPredictInput {
    pub hashes: Vec<String>,
    /// If provided, only run these specific model slugs (ignoring settings toggles).
    /// If absent/empty, use the enabled models from settings.
    #[serde(default)]
    pub models: Option<Vec<String>>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "camelCase")]
pub struct FilePrediction {
    pub hash: String,
    pub tags: Vec<TagPrediction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AiTagPredictOutput {
    pub predictions: Vec<FilePrediction>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AiTagApplyInput {
    pub hashes: Vec<String>,
    pub tags: Vec<String>,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn ai_tagger_status(
    state: &AppState,
    _input: AiTaggerStatusInput,
) -> Result<AiTaggerStatusOutput, String> {
    let settings = state.settings.get();
    let models_root = models_root_for(state);
    let all_models = crate::ai_tagger::models::known_models();

    let models = all_models
        .iter()
        .map(|m| AiTaggerModelStatus {
            slug: m.slug.clone(),
            label: m.label.clone(),
            enabled: model_enabled(&settings, &m.slug),
            downloaded: crate::ai_tagger::models::is_model_downloaded(&models_root, &m.slug),
            // Light models are the recommended pair everywhere; heavy models
            // are an explicit opt-in regardless of hardware.
            recommended: !m.heavy,
            heavy: m.heavy,
            size_bytes: m.size_bytes,
            dataset: m.dataset.clone(),
        })
        .collect();

    let gpu_backend = {
        let guard = state.ai_taggers.lock().await;
        guard.values().next().map(|s| s.gpu_backend())
    };

    let hardware = AiTaggerHardware {
        cpu_model: detect_cpu_model(),
        logical_cores: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        memory_bytes: detect_memory_bytes(),
        execution_provider: gpu_backend.clone().unwrap_or_else(|| "CPU".into()),
    };

    Ok(AiTaggerStatusOutput {
        models,
        gpu_backend,
        available_models: all_models,
        hardware,
    })
}

pub async fn ai_tagger_download_model(
    state: &AppState,
    input: AiTaggerDownloadModelInput,
) -> Result<(), String> {
    let models_root = models_root_for(state);
    let slug = input.model.clone();

    tokio::spawn(async move {
        if let Err(e) = crate::ai_tagger::download::download_model(&slug, &models_root).await {
            tracing::error!(slug, error = %e, "Model download failed");
        }
    });

    Ok(())
}

pub async fn ai_tagger_delete_model(
    state: &AppState,
    input: AiTaggerDownloadModelInput,
) -> Result<(), String> {
    let models_root = models_root_for(state);
    let model_dir = crate::ai_tagger::models::model_dir(&models_root, &input.model);

    // Remove cached session if loaded
    {
        let mut guard = state.ai_taggers.lock().await;
        guard.remove(&input.model);
    }

    // Delete files
    if model_dir.exists() {
        std::fs::remove_dir_all(&model_dir)
            .map_err(|e| format!("Failed to delete model directory: {e}"))?;
    }

    tracing::info!(slug = input.model, "Model deleted");
    Ok(())
}

pub async fn ai_tag_predict(
    state: &AppState,
    input: AiTagPredictInput,
) -> Result<AiTagPredictOutput, String> {
    let settings = state.settings.get();

    // Collect which models to run — explicit list overrides settings
    let slugs: Vec<String> = match &input.models {
        Some(models) if !models.is_empty() => models.clone(),
        _ => enabled_slugs(&settings),
    };
    if slugs.is_empty() {
        return Err("No AI tagger models specified or enabled.".into());
    }

    // One run at a time — the panel drives a single prediction pass.
    let token = {
        let mut guard = state.ai_tag_run.lock().await;
        if guard.is_some() {
            return Err("An auto-tag run is already in progress.".into());
        }
        let token = CancellationToken::new();
        *guard = Some(token.clone());
        token
    };

    let result = run_predict(state, &input.hashes, &slugs, &token).await;

    state.ai_tag_run.lock().await.take();
    result
}

async fn run_predict(
    state: &AppState,
    hashes: &[String],
    slugs: &[String],
    token: &CancellationToken,
) -> Result<AiTagPredictOutput, String> {
    let total = hashes.len() as u64;
    tracing::info!(models = ?slugs, hashes = total, "ai_tag_predict: starting");
    emit_autotag_task(
        TaskStatus::Running,
        0,
        total,
        Some("Loading models…".into()),
    );

    // Ensure all requested sessions are loaded
    for slug in slugs {
        if let Err(e) = ensure_session(state, slug).await {
            emit_autotag_task(TaskStatus::Failed, 0, total, Some(e.clone()));
            schedule_autotag_task_removal();
            return Err(e);
        }
    }

    // Everything above the floor comes back; the panel applies the user's
    // cutoff client-side.
    let floor = Thresholds {
        general: PREDICT_FLOOR,
        character: PREDICT_FLOOR,
        copyright: PREDICT_FLOOR,
        artist: PREDICT_FLOOR,
        species: PREDICT_FLOOR,
        rating: PREDICT_FLOOR,
    };

    let mut predictions = Vec::new();

    for (idx, hash) in hashes.iter().enumerate() {
        if token.is_cancelled() {
            break;
        }

        // Read the original file (not thumbnail) for best inference quality
        let image_bytes = match read_original_image(state, hash).await {
            Ok(bytes) => bytes,
            Err(e) => {
                predictions.push(FilePrediction {
                    hash: hash.clone(),
                    tags: vec![],
                    error: Some(e),
                });
                emit_autotag_task(TaskStatus::Running, (idx + 1) as u64, total, None);
                continue;
            }
        };

        // Run each model; per-model attribution is kept (TagPrediction.model)
        // and merging happens in the panel.
        let mut all_tags: Vec<TagPrediction> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        {
            let mut guard = state.ai_taggers.lock().await;
            for slug in slugs {
                if token.is_cancelled() {
                    break;
                }
                if let Some(session) = guard.get_mut(slug.as_str()) {
                    let result =
                        tokio::task::block_in_place(|| session.predict(&image_bytes, &floor));
                    match result {
                        Ok(tags) => all_tags.extend(tags),
                        Err(e) => {
                            tracing::error!(slug, error = %e, "ai_tag_predict: inference failed");
                            errors.push(format!("{slug}: {e}"));
                        }
                    }
                } else {
                    tracing::warn!(slug, "ai_tag_predict: session not found in map");
                }
            }
        }

        all_tags.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        predictions.push(FilePrediction {
            hash: hash.clone(),
            tags: all_tags,
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            },
        });

        emit_autotag_task(TaskStatus::Running, (idx + 1) as u64, total, None);
    }

    let status_text = token.is_cancelled().then(|| "Cancelled".to_string());
    emit_autotag_task(
        TaskStatus::Finished,
        predictions.len() as u64,
        total,
        status_text,
    );
    schedule_autotag_task_removal();

    Ok(AiTagPredictOutput { predictions })
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AiTagCancelInput {}

/// Cancel the in-flight auto-tag prediction, if any. The running predict
/// call returns its partial results.
pub async fn ai_tag_cancel(state: &AppState, _input: AiTagCancelInput) -> Result<(), String> {
    let guard = state.ai_tag_run.lock().await;
    if let Some(token) = guard.as_ref() {
        token.cancel();
        emit_autotag_task(TaskStatus::Cancelling, 0, 0, None);
    }
    Ok(())
}

pub async fn ai_tag_apply(state: &AppState, input: AiTagApplyInput) -> Result<usize, String> {
    if input.hashes.is_empty() || input.tags.is_empty() {
        return Ok(0);
    }

    let normalized_tags: Vec<String> = input
        .tags
        .iter()
        .filter_map(|tag_str| {
            crate::tags::normalize::parse_tag(tag_str)
                .map(|(ns, st)| crate::tags::normalize::combine_tag(&ns, &st))
        })
        .collect();
    if normalized_tags.is_empty() {
        return Ok(0);
    }

    state.engine.apply_entity_tags(
        EntityTarget {
            kind: EntityTargetKind::EntityHashes,
            entity_hashes: Some(input.hashes.clone()),
            query: None,
            excluded_entity_hashes: None,
        },
        TagOperation::Add,
        &normalized_tags,
        Some(TAG_PROVENANCE_AI),
    )?;

    Ok(input.hashes.len() * normalized_tags.len())
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn emit_autotag_task(status: TaskStatus, done: u64, total: u64, status_text: Option<String>) {
    let now = chrono::Utc::now().to_rfc3339();
    crate::runtime_state::upsert_task(RuntimeTask {
        task_id: AUTO_TAG_TASK_ID.into(),
        kind: TaskKind::AutoTag,
        status,
        label: "Auto tagging".into(),
        parent_task_id: None,
        progress: Some(TaskProgress {
            done,
            total,
            status_text,
        }),
        detail: None,
        started_at: now.clone(),
        updated_at: now,
    });
}

fn schedule_autotag_task_removal() {
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        crate::runtime_state::remove_task(AUTO_TAG_TASK_ID);
    });
}

fn detect_cpu_model() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()?
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn detect_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
    }
    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = meminfo
            .lines()
            .find(|l| l.starts_with("MemTotal"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        Some(kb * 1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Read the original image bytes for a hash from the blob store.
async fn read_original_image(state: &AppState, hash: &str) -> Result<Vec<u8>, String> {
    let path = state
        .engine
        .resolve_file_path(&state.blob_store, hash)
        .await
        .map_err(|e| format!("File not found: {e}"))?;
    tokio::fs::read(path)
        .await
        .map_err(|e| format!("Failed to read original file: {e}"))
}

fn models_root_for(state: &AppState) -> std::path::PathBuf {
    state
        .library_root
        .parent()
        .unwrap_or(&state.library_root)
        .join("models")
}

async fn ensure_session(state: &AppState, slug: &str) -> Result<(), String> {
    {
        let guard = state.ai_taggers.lock().await;
        if guard.contains_key(slug) {
            return Ok(());
        }
    }
    // Drop the lock before the heavy model load

    let models_root = models_root_for(state);
    let model_info = crate::ai_tagger::models::find_model(slug)
        .ok_or_else(|| format!("Unknown model: {slug}"))?;

    if !crate::ai_tagger::models::is_model_downloaded(&models_root, slug) {
        return Err(format!(
            "Model '{slug}' is not downloaded. Please download it first."
        ));
    }

    let model_dir = crate::ai_tagger::models::model_dir(&models_root, slug);
    let slug_owned = slug.to_string();
    let input_size = model_info.input_size;
    let channel_order = model_info.channel_order;

    // Load the model on a blocking thread — ONNX graph optimization is CPU-heavy
    let session = tokio::task::spawn_blocking(move || {
        crate::ai_tagger::inference::TaggerSession::load(
            &model_dir,
            &slug_owned,
            input_size,
            channel_order,
        )
    })
    .await
    .map_err(|e| format!("Model load task failed: {e}"))??;

    let mut guard = state.ai_taggers.lock().await;
    guard.insert(slug.to_string(), session);
    Ok(())
}

/// Auto-tag a batch of newly imported files if the setting is enabled and
/// at least one model is downloaded. Called from import dispatch handlers.
/// Silently skips if disabled, no models enabled, or models not downloaded.
pub async fn auto_tag_imported(state: &AppState, hashes: &[String]) {
    if hashes.is_empty() {
        return;
    }
    let settings = state.settings.get();
    if !settings.ai_tagger_auto_on_import {
        return;
    }

    let slugs = enabled_slugs(&settings);
    if slugs.is_empty() {
        return;
    }

    // Ensure sessions are loaded — skip silently if model not downloaded
    for slug in &slugs {
        if let Err(e) = ensure_session(state, slug).await {
            tracing::debug!(slug, error = %e, "auto_tag_imported: skipping (session not available)");
            return;
        }
    }

    let thresholds = thresholds_from_settings(&settings);

    for hash in hashes {
        let image_bytes = match read_original_image(state, hash).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::debug!(hash, error = %e, "auto_tag_imported: skipping file");
                continue;
            }
        };

        let mut all_tags: Vec<crate::ai_tagger::inference::TagPrediction> = Vec::new();
        {
            let mut guard = state.ai_taggers.lock().await;
            for slug in &slugs {
                if let Some(session) = guard.get_mut(slug.as_str()) {
                    match session.predict(&image_bytes, &thresholds) {
                        Ok(tags) => all_tags.extend(tags),
                        Err(e) => {
                            tracing::warn!(slug, hash, error = %e, "auto_tag_imported: inference failed");
                        }
                    }
                }
            }
        }

        // Deduplicate (keep highest confidence)
        all_tags.sort_by(|a, b| {
            a.namespace.cmp(&b.namespace).then(a.tag.cmp(&b.tag)).then(
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        all_tags.dedup_by(|a, b| a.namespace == b.namespace && a.tag == b.tag);

        if !settings.ai_tagger_write_rating {
            all_tags.retain(|t| t.namespace != "rating");
        }

        // Apply tags
        let tag_strings: Vec<String> = all_tags
            .iter()
            .map(|pred| {
                if pred.namespace.is_empty() {
                    pred.tag.clone()
                } else {
                    format!("{}:{}", pred.namespace, pred.tag)
                }
            })
            .collect();
        if !tag_strings.is_empty() {
            let target = EntityTarget {
                kind: EntityTargetKind::EntityHashes,
                entity_hashes: Some(vec![hash.clone()]),
                query: None,
                excluded_entity_hashes: None,
            };
            if let Err(e) = state.engine.apply_entity_tags(
                target,
                TagOperation::Add,
                &tag_strings,
                Some(TAG_PROVENANCE_AI),
            ) {
                tracing::warn!(hash, error = %e, "auto_tag_imported: tag apply failed");
                continue;
            }
        }

        if !all_tags.is_empty() {
            tracing::info!(hash, tags = all_tags.len(), "auto_tag_imported: applied");
        }
    }
}

fn thresholds_from_settings(settings: &crate::settings::store::AppSettings) -> Thresholds {
    Thresholds {
        general: settings.ai_threshold_general,
        character: settings.ai_threshold_character,
        copyright: settings.ai_threshold_copyright,
        artist: settings.ai_threshold_artist,
        species: settings.ai_threshold_species,
        rating: settings.ai_threshold_rating,
    }
}
