//! Dispatch handlers for AI tagger commands.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ai_tagger::inference::{TagPrediction, Thresholds};
use crate::ai_tagger::models::ModelInfo;
use crate::db::types::{EntityTarget, EntityTargetKind, TAG_PROVENANCE_AI};
use crate::engine::tags::TagOperation;
use crate::state::AppState;

// ─── Constants ──────────────────────────────────────────────────────────────

const WD14_SLUG: &str = "wd14-swinv2-v3";
const E621_SLUG: &str = "z3d-e621-convnext";

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
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "camelCase")]
pub struct AiTaggerStatusOutput {
    pub models: Vec<AiTaggerModelStatus>,
    pub gpu_backend: Option<String>,
    pub available_models: Vec<ModelInfo>,
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

    let models = vec![
        AiTaggerModelStatus {
            slug: WD14_SLUG.into(),
            label: all_models
                .iter()
                .find(|m| m.slug == WD14_SLUG)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| WD14_SLUG.into()),
            enabled: settings.ai_tagger_wd14_enabled,
            downloaded: crate::ai_tagger::models::is_model_downloaded(&models_root, WD14_SLUG),
        },
        AiTaggerModelStatus {
            slug: E621_SLUG.into(),
            label: all_models
                .iter()
                .find(|m| m.slug == E621_SLUG)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| E621_SLUG.into()),
            enabled: settings.ai_tagger_e621_enabled,
            downloaded: crate::ai_tagger::models::is_model_downloaded(&models_root, E621_SLUG),
        },
    ];

    let gpu_backend = {
        let guard = state.ai_taggers.lock().await;
        guard.values().next().map(|s| s.gpu_backend())
    };

    Ok(AiTaggerStatusOutput {
        models,
        gpu_backend,
        available_models: all_models,
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
    let thresholds = thresholds_from_settings(&settings);

    // Collect which models to run — explicit list overrides settings
    let slugs: Vec<&str> = if let Some(ref models) = input.models {
        if !models.is_empty() {
            models.iter().map(|s| s.as_str()).collect()
        } else {
            let mut s = Vec::new();
            if settings.ai_tagger_wd14_enabled {
                s.push(WD14_SLUG);
            }
            if settings.ai_tagger_e621_enabled {
                s.push(E621_SLUG);
            }
            s
        }
    } else {
        let mut s = Vec::new();
        if settings.ai_tagger_wd14_enabled {
            s.push(WD14_SLUG);
        }
        if settings.ai_tagger_e621_enabled {
            s.push(E621_SLUG);
        }
        s
    };

    if slugs.is_empty() {
        return Err("No AI tagger models specified or enabled.".into());
    }

    tracing::info!(models = ?slugs, hashes = input.hashes.len(), "ai_tag_predict: starting");

    // Ensure all enabled sessions are loaded
    for slug in &slugs {
        tracing::info!(slug, "ai_tag_predict: ensuring session loaded");
        ensure_session(state, slug).await?;
    }

    let mut predictions = Vec::new();

    for hash in &input.hashes {
        // Read the original file (not thumbnail) for best inference quality
        let image_bytes = match read_original_image(state, hash).await {
            Ok(bytes) => bytes,
            Err(e) => {
                predictions.push(FilePrediction {
                    hash: hash.clone(),
                    tags: vec![],
                    error: Some(e),
                });
                continue;
            }
        };

        // Run each enabled model and merge results
        let mut all_tags: Vec<TagPrediction> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        tracing::info!(
            hash,
            bytes = image_bytes.len(),
            "ai_tag_predict: read original image"
        );

        {
            let mut guard = state.ai_taggers.lock().await;
            for slug in &slugs {
                if let Some(session) = guard.get_mut(*slug) {
                    match session.predict(&image_bytes, &thresholds) {
                        Ok(tags) => {
                            tracing::info!(
                                slug,
                                tags = tags.len(),
                                "ai_tag_predict: model produced tags"
                            );
                            all_tags.extend(tags);
                        }
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

        // Deduplicate: if both models predict the same tag, keep highest confidence
        all_tags.sort_by(|a, b| {
            a.namespace.cmp(&b.namespace).then(a.tag.cmp(&b.tag)).then(
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        all_tags.dedup_by(|a, b| a.namespace == b.namespace && a.tag == b.tag);

        // Re-sort by confidence descending
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
    }

    Ok(AiTagPredictOutput { predictions })
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

    let mut slugs = Vec::new();
    if settings.ai_tagger_wd14_enabled {
        slugs.push(WD14_SLUG);
    }
    if settings.ai_tagger_e621_enabled {
        slugs.push(E621_SLUG);
    }
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
                if let Some(session) = guard.get_mut(*slug) {
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
