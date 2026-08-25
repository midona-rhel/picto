//! Replacement-backend AI tagging execution.
//!
//! Model discovery, image preprocessing, and inference remain in `ai_tagger`.
//! This runtime only caches sessions and persists predictions on the media
//! asset that was actually analyzed.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ai_tagger::inference::{TagPrediction, Thresholds};
use crate::ai_tagger::models::{self, ModelInfo};
use crate::app::{Application, ItemId, MutationReceipt};
use crate::blob_store::mime_to_extension;

const MAX_MANUAL_PREDICTION_ITEMS: usize = 256;
const MAX_MANUAL_PREDICTION_MODELS: usize = 8;

/// Provenance bit shared with the existing AI tagger.
pub(crate) const AI_PROVENANCE_MASK: i64 = 1 << 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaOriginal {
    root_item_id: ItemId,
    file_hash: String,
    mime_type: String,
}

/// Read-only model state exposed by the replacement AI runtime.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "camelCase")]
pub struct AiModelStatus {
    pub slug: String,
    pub label: String,
    pub enabled: bool,
    pub downloaded: bool,
    pub session_loaded: bool,
    pub recommended: bool,
    pub heavy: bool,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub dataset: String,
}

/// Replacement AI status derived from settings, the model bundle, and the
/// in-process session cache. No download or mutation state is implied.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "camelCase")]
pub struct AiRuntimeStatus {
    pub models: Vec<AiModelStatus>,
    pub configured_model_slugs: Vec<String>,
    pub thresholds: AiThresholds,
    pub cached_backend: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct AiTagPrediction {
    pub tag: String,
    pub namespace: String,
    pub confidence: f32,
    pub model: String,
}

/// Read-only manual prediction request. Item IDs are logical replacement
/// identities; physical file hashes never cross this boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "camelCase")]
pub struct ManualPredictionRequest {
    pub item_ids: Vec<ItemId>,
    #[serde(default)]
    pub model_slugs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "camelCase")]
pub struct MediaPrediction {
    pub media_item_id: ItemId,
    pub predictions: Vec<AiTagPrediction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "camelCase")]
pub struct ManualPredictionResponse {
    pub predictions: Vec<MediaPrediction>,
    pub thresholds: AiThresholds,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct AiThresholds {
    pub general: f32,
    pub character: f32,
    pub copyright: f32,
    pub artist: f32,
    pub species: f32,
    pub rating: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PredictionOriginal {
    file_hash: String,
    mime_type: String,
}

/// Return the replacement AI model status without touching model files or
/// starting inference sessions.
pub async fn model_status(application: &Application) -> Result<AiRuntimeStatus, String> {
    let settings = crate::settings_v2::application_settings(application)?.value;
    let models_root = crate::ai_models_v2::models_root(application);
    let configured_model_slugs = configured_models(&settings)
        .iter()
        .map(|model| model.slug.clone())
        .collect::<Vec<_>>();

    let sessions = application.ai_sessions().lock().await;
    let cached_backend = sessions
        .values()
        .find_map(|session| session.lock().ok().map(|session| session.gpu_backend()));
    let models = models::known_models()
        .into_iter()
        .map(|model| AiModelStatus {
            enabled: setting_bool(&settings, model_setting_key(&model.slug)).unwrap_or(false),
            downloaded: models::is_model_downloaded(&models_root, &model.slug),
            session_loaded: sessions.contains_key(&model.slug),
            recommended: !model.heavy,
            slug: model.slug,
            label: model.label,
            heavy: model.heavy,
            size_bytes: model.size_bytes,
            dataset: model.dataset,
        })
        .collect();

    Ok(AiRuntimeStatus {
        models,
        configured_model_slugs,
        thresholds: public_thresholds(&thresholds_from_settings(&settings)),
        cached_backend,
    })
}

/// Predict tags for explicit replacement media items without applying them.
/// Invalid item kinds and item-local read/inference failures are returned next
/// to their item IDs so one bad item does not hide the rest of the result.
pub async fn manual_predict(
    application: &Application,
    request: ManualPredictionRequest,
) -> Result<ManualPredictionResponse, String> {
    if request.item_ids.is_empty() {
        return Err("At least one library item is required".into());
    }
    if request.item_ids.len() > MAX_MANUAL_PREDICTION_ITEMS {
        return Err(format!(
            "Manual prediction accepts at most {} library items",
            MAX_MANUAL_PREDICTION_ITEMS
        ));
    }
    let media_item_ids = resolve_prediction_items(application, &request.item_ids)?;
    if media_item_ids.len() > MAX_MANUAL_PREDICTION_ITEMS {
        return Err(format!(
            "Manual prediction accepts at most {} media items after expanding groups",
            MAX_MANUAL_PREDICTION_ITEMS
        ));
    }

    let models = select_prediction_models(&request.model_slugs, application).await?;
    let settings = crate::settings_v2::application_settings(application)?.value;
    let thresholds = thresholds_from_settings(&settings);
    let mut results = media_item_ids
        .iter()
        .map(|media_item_id| MediaPrediction {
            media_item_id: *media_item_id,
            predictions: Vec::new(),
            error: None,
        })
        .collect::<Vec<_>>();
    let mut valid = Vec::new();
    for (index, media_item_id) in media_item_ids.into_iter().enumerate() {
        match load_prediction_original(application, media_item_id).and_then(|original| {
            validate_image_original(media_item_id, &original)?;
            Ok(original)
        }) {
            Ok(original) => valid.push((index, original)),
            Err(error) => results[index].error = Some(error),
        }
    }
    if !valid.is_empty() {
        let files = valid
            .iter()
            .map(|(_, original)| (original.file_hash.clone(), original.mime_type.clone()))
            .collect::<Vec<_>>();
        match predict_files_cached(application, &models, &files, thresholds.clone()).await {
            Ok(predictions) => {
                for ((index, _), predictions) in valid.into_iter().zip(predictions) {
                    results[index].predictions = predictions
                        .into_iter()
                        .map(|prediction| AiTagPrediction {
                            tag: prediction.tag,
                            namespace: prediction.namespace,
                            confidence: prediction.confidence,
                            model: prediction.model,
                        })
                        .collect();
                }
            }
            Err(error) => {
                for (index, _) in valid {
                    results[index].error = Some(error.clone());
                }
            }
        }
    }

    Ok(ManualPredictionResponse {
        predictions: results,
        thresholds: public_thresholds(&thresholds),
    })
}

fn resolve_prediction_items(
    application: &Application,
    item_ids: &[ItemId],
) -> Result<Vec<ItemId>, String> {
    application.store().read_result(|connection| {
        let mut resolved = Vec::new();
        let mut seen = BTreeSet::new();
        for item_id in item_ids {
            let kind = connection
                .query_row(
                    "SELECT kind FROM library_item WHERE item_id = ?1",
                    [item_id.0],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| format!("Library item {} was not found: {error}", item_id.0))?;
            match kind.as_str() {
                "media" => {
                    if seen.insert(item_id.0) {
                        resolved.push(*item_id);
                    }
                }
                "collection" => {
                    let mut statement = connection
                        .prepare(
                            "SELECT media_item_id
                             FROM collection_member
                             WHERE collection_id = ?1
                             ORDER BY position_rank, media_item_id",
                        )
                        .map_err(|error| error.to_string())?;
                    let members = statement
                        .query_map([item_id.0], |row| row.get::<_, i64>(0))
                        .map_err(|error| error.to_string())?;
                    for member in members {
                        let member = member.map_err(|error| error.to_string())?;
                        if seen.insert(member) {
                            resolved.push(ItemId(member));
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "Library item {} has unsupported kind '{other}'",
                        item_id.0
                    ));
                }
            }
        }
        Ok(resolved)
    })
}

async fn select_prediction_models(
    requested_slugs: &Option<Vec<String>>,
    application: &Application,
) -> Result<Vec<ModelInfo>, String> {
    let settings = crate::settings_v2::application_settings(application)?.value;
    resolve_prediction_models(requested_slugs, &settings)
}

fn resolve_prediction_models(
    requested_slugs: &Option<Vec<String>>,
    settings: &serde_json::Value,
) -> Result<Vec<ModelInfo>, String> {
    let slugs = match requested_slugs {
        Some(slugs) if !slugs.is_empty() => slugs.clone(),
        _ => configured_models(settings)
            .into_iter()
            .map(|model| model.slug)
            .collect(),
    };
    if slugs.is_empty() {
        return Err("No AI tagger models specified or enabled".into());
    }
    let slugs = slugs.into_iter().collect::<BTreeSet<_>>();
    if slugs.len() > MAX_MANUAL_PREDICTION_MODELS {
        return Err(format!(
            "Manual prediction accepts at most {} models",
            MAX_MANUAL_PREDICTION_MODELS
        ));
    }

    slugs
        .into_iter()
        .map(|slug| models::find_model(&slug).ok_or_else(|| format!("Unknown AI model '{slug}'")))
        .collect()
}

fn validate_image_original(
    media_item_id: ItemId,
    original: &PredictionOriginal,
) -> Result<(), String> {
    if original.mime_type.starts_with("image/") {
        Ok(())
    } else {
        Err(format!(
            "AI prediction requires an image; media item {} has MIME type {}",
            media_item_id.0, original.mime_type
        ))
    }
}

fn load_prediction_original(
    application: &Application,
    media_item_id: ItemId,
) -> Result<PredictionOriginal, String> {
    application.store().read_result(|connection| {
        let row = connection
            .query_row(
                "SELECT li.kind, mf.file_hash, mf.mime_type
                 FROM library_item li
                 LEFT JOIN media_asset ma ON ma.item_id = li.item_id
                 LEFT JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE li.item_id = ?1",
                [media_item_id.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .map_err(|error| format!("Media item {} was not found: {error}", media_item_id.0))?;
        if row.0 == "collection" {
            return Err(format!(
                "Media item {} is a group; AI prediction requires an image item",
                media_item_id.0
            ));
        }
        if row.0 != "media" {
            return Err(format!(
                "Media item {} has unsupported kind '{}'",
                media_item_id.0, row.0
            ));
        }
        Ok(PredictionOriginal {
            file_hash: row
                .1
                .ok_or_else(|| format!("Media item {} has no physical file", media_item_id.0))?,
            mime_type: row
                .2
                .ok_or_else(|| format!("Media item {} has no MIME type", media_item_id.0))?,
        })
    })
}

/// The result of tagging one replacement-backend media item.
#[derive(Debug, Clone)]
pub struct AiTagExecutionResult {
    pub media_item_id: ItemId,
    pub root_item_id: ItemId,
    pub models: Vec<String>,
    pub predictions: usize,
    pub applied_tags: Vec<String>,
    pub receipt: Option<MutationReceipt>,
}

/// Run configured AI models for one replacement media asset and apply the
/// resulting tags through the replacement Application operation.
pub async fn execute_ai_tagging(
    application: &Application,
    media_item_id: ItemId,
) -> Result<AiTagExecutionResult, String> {
    execute_ai_tagging_batch(application, &[media_item_id])
        .await?
        .pop()
        .ok_or_else(|| "AI tagging produced no result".to_string())
}

pub async fn execute_ai_tagging_batch(
    application: &Application,
    media_item_ids: &[ItemId],
) -> Result<Vec<AiTagExecutionResult>, String> {
    if media_item_ids.is_empty() {
        return Ok(Vec::new());
    }
    let originals = media_item_ids
        .iter()
        .map(|media_item_id| {
            let original = load_media_original(application, *media_item_id)?;
            if !original.mime_type.starts_with("image/") {
                return Err(format!(
                    "AI tagging requires an image original; media item {} has MIME type {}",
                    media_item_id.0, original.mime_type
                ));
            }
            Ok((*media_item_id, original))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let settings = crate::settings_v2::application_settings(application)?.value;
    let models = configured_models(&settings);
    if models.is_empty() {
        return Ok(originals
            .into_iter()
            .map(|(media_item_id, original)| AiTagExecutionResult {
                media_item_id,
                root_item_id: original.root_item_id,
                models: Vec::new(),
                predictions: 0,
                applied_tags: Vec::new(),
                receipt: None,
            })
            .collect());
    }
    let thresholds = thresholds_from_settings(&settings);
    let write_rating = setting_bool(&settings, "aiTaggerWriteRating").unwrap_or(true);
    let model_slugs = models
        .iter()
        .map(|model| model.slug.clone())
        .collect::<Vec<_>>();
    let files = originals
        .iter()
        .map(|(_, original)| (original.file_hash.clone(), original.mime_type.clone()))
        .collect::<Vec<_>>();
    let predictions = predict_files_cached(application, &models, &files, thresholds).await?;

    originals
        .into_iter()
        .zip(predictions)
        .map(|((media_item_id, original), predictions)| {
            let tags = normalize_predictions(&predictions, write_rating);
            let receipt = if tags.is_empty() {
                None
            } else {
                Some(application.apply_media_tags(media_item_id, &tags, AI_PROVENANCE_MASK)?)
            };
            Ok(AiTagExecutionResult {
                media_item_id,
                root_item_id: original.root_item_id,
                models: model_slugs.clone(),
                predictions: predictions.len(),
                applied_tags: tags,
                receipt,
            })
        })
        .collect()
}

fn load_media_original(
    application: &Application,
    media_item_id: ItemId,
) -> Result<MediaOriginal, String> {
    application.store().read_result(|connection| {
        connection
            .query_row(
                "SELECT COALESCE(cm.collection_id, lr.item_id), mf.file_hash, mf.mime_type
                 FROM media_asset ma
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 LEFT JOIN library_root lr ON lr.item_id = ma.item_id
                 LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
                 WHERE ma.item_id = ?1",
                [media_item_id.0],
                |row| {
                    Ok(MediaOriginal {
                        root_item_id: ItemId(row.get(0)?),
                        file_hash: row.get(1)?,
                        mime_type: row.get(2)?,
                    })
                },
            )
            .map_err(|error| {
                format!(
                    "Media item {} has no persisted original: {error}",
                    media_item_id.0
                )
            })
    })
}

fn configured_models(settings: &serde_json::Value) -> Vec<ModelInfo> {
    models::known_models()
        .into_iter()
        .filter(|model| setting_bool(settings, model_setting_key(&model.slug)).unwrap_or(false))
        .collect()
}

fn model_setting_key(slug: &str) -> &'static str {
    match slug {
        "wd14-swinv2-v3" => "aiTaggerWd14Enabled",
        "z3d-e621-convnext" => "aiTaggerE621Enabled",
        "wd14-eva02-large-v3" => "aiTaggerEva02Enabled",
        _ => "",
    }
}

fn setting_bool(settings: &serde_json::Value, key: &str) -> Option<bool> {
    settings.get(key).and_then(serde_json::Value::as_bool)
}

fn thresholds_from_settings(settings: &serde_json::Value) -> Thresholds {
    Thresholds {
        general: setting_threshold(settings, "aiThresholdGeneral", 0.35),
        character: setting_threshold(settings, "aiThresholdCharacter", 0.85),
        copyright: setting_threshold(settings, "aiThresholdCopyright", 0.85),
        artist: setting_threshold(settings, "aiThresholdArtist", 0.85),
        species: setting_threshold(settings, "aiThresholdSpecies", 0.35),
        rating: setting_threshold(settings, "aiThresholdRating", 0.50),
    }
}

fn public_thresholds(thresholds: &Thresholds) -> AiThresholds {
    AiThresholds {
        general: thresholds.general,
        character: thresholds.character,
        copyright: thresholds.copyright,
        artist: thresholds.artist,
        species: thresholds.species,
        rating: thresholds.rating,
    }
}

fn setting_threshold(settings: &serde_json::Value, key: &str, default: f32) -> f32 {
    settings
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(0.0, 1.0)
}

async fn ensure_sessions(application: &Application, models: &[ModelInfo]) -> Result<(), String> {
    if models.len() > 1 {
        return Err("AI tagging runs exactly one model at a time".into());
    }
    let Some(model) = models.first() else {
        return Ok(());
    };
    let models_root = crate::ai_models_v2::models_root(application);
    let mut sessions = application.ai_sessions().lock().await;
    if sessions.len() == 1 && sessions.contains_key(&model.slug) {
        return Ok(());
    }
    // Unload the previous runtime before constructing its replacement. This
    // keeps model memory bounded and serializes model switches.
    sessions.clear();
    if !models::is_model_downloaded(&models_root, &model.slug) {
        return Err(format!(
            "Configured AI tagger model '{}' is not downloaded",
            model.slug
        ));
    }
    let model_dir = models::model_dir(&models_root, model);
    let slug = model.slug.clone();
    let load_slug = slug.clone();
    let input_size = model.input_size;
    let channel_order = model.channel_order;
    let output_activation = model.output_activation;
    let session = tokio::task::spawn_blocking(move || {
        crate::ai_tagger::inference::TaggerSession::load(
            &model_dir,
            &load_slug,
            input_size,
            channel_order,
            output_activation,
        )
    })
    .await
    .map_err(|error| format!("AI model load task failed: {error}"))??;
    sessions.insert(slug, std::sync::Arc::new(std::sync::Mutex::new(session)));
    Ok(())
}

async fn predict_batch(
    application: &Application,
    models: &[ModelInfo],
    images: Vec<Vec<u8>>,
    thresholds: Thresholds,
) -> Result<Vec<Vec<TagPrediction>>, String> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let images = Arc::new(images);
    let mut combined = vec![Vec::new(); images.len()];
    for model in models {
        ensure_sessions(application, std::slice::from_ref(model)).await?;
        let (slug, session) = {
            let sessions = application.ai_sessions().lock().await;
            sessions
                .get(&model.slug)
                .cloned()
                .map(|session| (model.slug.clone(), session))
                .ok_or_else(|| format!("AI model session '{}' is not loaded", model.slug))?
        };
        let images = Arc::clone(&images);
        let thresholds = thresholds.clone();
        let inferred = tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let spec = session
                .lock()
                .map_err(|_| format!("AI model session '{slug}' lock was poisoned"))?
                .input_spec();
            let preprocess_started = Instant::now();
            let image_refs = images.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let prepared = crate::ai_tagger::inference::prepare_inputs(&image_refs, spec)?;
            let preprocess_ms = preprocess_started.elapsed().as_secs_f64() * 1000.0;
            let predictions = session
                .lock()
                .map_err(|_| format!("AI model session '{slug}' lock was poisoned"))?
                .predict_prepared_batch(&prepared, &thresholds, preprocess_ms)?;
            tracing::debug!(
                target: "ai_inference",
                model = slug,
                preprocess_ms,
                total_ms = started.elapsed().as_secs_f64() * 1000.0,
                images = predictions.len(),
                "AI model batch completed"
            );
            Ok::<_, String>(predictions)
        })
        .await
        .map_err(|error| format!("AI inference task failed: {error}"))??;
        if inferred.len() != combined.len() {
            return Err("AI inference returned an unexpected number of results".into());
        }
        for (predictions, model_predictions) in combined.iter_mut().zip(inferred) {
            predictions.extend(model_predictions);
        }
    }
    Ok(combined)
}

async fn predict_files_cached(
    application: &Application,
    models: &[ModelInfo],
    files: &[(String, String)],
    thresholds: Thresholds,
) -> Result<Vec<Vec<TagPrediction>>, String> {
    // Manual review and background ingestion share one inference lane. The
    // selected models run serially and only the current model remains loaded.
    let _inference_lane = application.ai_model_lifecycle().lock().await;
    let keys = files
        .iter()
        .map(|(file_hash, _)| prediction_cache_key(file_hash, models, &thresholds))
        .collect::<Vec<_>>();
    let mut results = vec![None; files.len()];
    let mut missing_by_key = HashMap::new();
    let mut missing_keys = Vec::new();
    let mut missing_files = Vec::new();
    let mut file_to_missing = vec![None; files.len()];
    let mut cache_hits = 0;

    {
        let mut cache = application
            .ai_prediction_cache()
            .lock()
            .map_err(|_| "AI prediction cache lock was poisoned".to_string())?;
        for (index, (key, file)) in keys.iter().zip(files).enumerate() {
            if let Some(predictions) = cache.get(key) {
                results[index] = Some(predictions.clone());
                cache_hits += 1;
                continue;
            }
            let missing_index = match missing_by_key.get(key) {
                Some(index) => *index,
                None => {
                    let index = missing_files.len();
                    missing_by_key.insert(key.clone(), index);
                    missing_keys.push(key.clone());
                    missing_files.push(file.clone());
                    index
                }
            };
            file_to_missing[index] = Some(missing_index);
        }
    }

    if !missing_files.is_empty() {
        let images = missing_files
            .iter()
            .map(|(file_hash, mime_type)| {
                application
                    .blobs()
                    .read_original(file_hash, Some(mime_to_extension(mime_type)))
                    .map_err(|error| format!("Failed to read original {file_hash}: {error}"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let inferred = predict_batch(application, models, images, thresholds).await?;
        if inferred.len() != missing_files.len() {
            return Err("AI inference returned an unexpected number of results".into());
        }
        let inferred = inferred.into_iter().map(Arc::new).collect::<Vec<_>>();
        let mut cache = application
            .ai_prediction_cache()
            .lock()
            .map_err(|_| "AI prediction cache lock was poisoned".to_string())?;
        for (key, predictions) in missing_keys.into_iter().zip(&inferred) {
            cache.put(key, predictions.clone());
        }
        for (file_index, missing_index) in file_to_missing.into_iter().enumerate() {
            if let Some(missing_index) = missing_index {
                results[file_index] = Some(inferred[missing_index].clone());
            }
        }
    }

    tracing::debug!(
        target: "ai_inference",
        files = files.len(),
        cache_hits,
        inferred = missing_files.len(),
        "AI prediction cache resolved batch"
    );
    results
        .into_iter()
        .map(|predictions| {
            predictions
                .map(|predictions| (*predictions).clone())
                .ok_or_else(|| "AI prediction cache left an unresolved result".to_string())
        })
        .collect()
}

fn prediction_cache_key(file_hash: &str, models: &[ModelInfo], thresholds: &Thresholds) -> String {
    let mut key = String::with_capacity(file_hash.len() + models.len() * 72 + 64);
    key.push_str(file_hash);
    for model in models {
        key.push('|');
        key.push_str(&model.slug);
        key.push(':');
        key.push_str(&model.onnx_sha256);
        if let Some(artifact) = &model.coreml {
            key.push(':');
            key.push_str(&artifact.sha256);
        }
    }
    for threshold in [
        thresholds.general,
        thresholds.character,
        thresholds.copyright,
        thresholds.artist,
        thresholds.species,
        thresholds.rating,
    ] {
        key.push(':');
        key.push_str(&format!("{:08x}", threshold.to_bits()));
    }
    key
}

fn normalize_predictions(predictions: &[TagPrediction], write_rating: bool) -> Vec<String> {
    predictions
        .iter()
        .filter_map(|prediction| normalized_prediction(prediction, write_rating))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalized_prediction(prediction: &TagPrediction, write_rating: bool) -> Option<String> {
    let namespace = match prediction.namespace.as_str() {
        "general" => "general",
        "artist" | "creator" => "creator",
        "copyright" | "series" => "series",
        "character" => "character",
        "species" => "species",
        "rating" if write_rating => "rating",
        "rating" => return None,
        _ => return None,
    };
    let raw = format!("{namespace}:{}", prediction.tag);
    crate::tag_name_v2::parse_external(&raw)
        .ok()
        .map(|(namespace, subtag)| crate::tag_name_v2::format(&namespace, &subtag))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;

    fn prediction(namespace: &str, tag: &str) -> TagPrediction {
        TagPrediction {
            tag: tag.into(),
            namespace: namespace.into(),
            confidence: 0.9,
            model: "test".into(),
        }
    }

    #[test]
    fn configured_models_are_limited_to_the_known_registry() {
        let settings = serde_json::json!({
            "aiTaggerWd14Enabled": true,
            "aiTaggerE621Enabled": true,
            "unknownModelEnabled": true,
        });
        let slugs = configured_models(&settings)
            .into_iter()
            .map(|model| model.slug)
            .collect::<Vec<_>>();
        assert_eq!(slugs, ["wd14-swinv2-v3", "z3d-e621-convnext"]);
    }

    #[test]
    fn predictions_use_only_existing_namespaces() {
        let predictions = vec![
            prediction("artist", "Artist Name"),
            prediction("copyright", "Series Name"),
            prediction("character", "Character Name"),
            prediction("made_up", "not_written"),
        ];
        assert_eq!(
            normalize_predictions(&predictions, true),
            [
                "character:character name",
                "creator:artist name",
                "series:series name"
            ]
        );
    }

    #[test]
    fn rating_predictions_follow_the_setting_and_duplicates_are_removed() {
        let predictions = vec![
            prediction("rating", "general"),
            prediction("general", "landscape"),
            prediction("general", "landscape"),
        ];
        assert_eq!(
            normalize_predictions(&predictions, false),
            ["general:landscape"]
        );
        assert_eq!(
            normalize_predictions(&predictions, true),
            ["general:landscape", "rating:general"]
        );
    }

    #[test]
    fn thresholds_are_bounded_and_have_safe_defaults() {
        let settings = serde_json::json!({
            "aiThresholdGeneral": 2.0,
            "aiThresholdCharacter": -1.0,
            "aiThresholdSpecies": "invalid",
        });
        let thresholds = thresholds_from_settings(&settings);
        assert_eq!(thresholds.general, 1.0);
        assert_eq!(thresholds.character, 0.0);
        assert_eq!(thresholds.species, 0.35);
        assert_eq!(thresholds.rating, 0.50);
    }

    #[test]
    fn manual_model_selection_is_registry_bound_and_deduplicated() {
        let settings = serde_json::json!({
            "aiTaggerWd14Enabled": true,
        });
        let selected = resolve_prediction_models(
            &Some(vec!["wd14-swinv2-v3".into(), "wd14-swinv2-v3".into()]),
            &settings,
        )
        .unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|model| model.slug.as_str())
                .collect::<Vec<_>>(),
            ["wd14-swinv2-v3"]
        );
        assert!(
            resolve_prediction_models(&Some(vec!["not-a-known-model".into()]), &settings)
                .unwrap_err()
                .contains("Unknown AI model")
        );
    }

    #[test]
    fn manual_prediction_model_and_item_limits_are_bounded() {
        let settings = serde_json::json!({
            "aiTaggerWd14Enabled": true,
        });
        let too_many_models = (0..=MAX_MANUAL_PREDICTION_MODELS)
            .map(|index| format!("model-{index}"))
            .collect();
        assert!(resolve_prediction_models(&Some(too_many_models), &settings)
            .unwrap_err()
            .contains("at most"));

        let video = PredictionOriginal {
            file_hash: "video-hash".into(),
            mime_type: "video/mp4".into(),
        };
        let error = validate_image_original(ItemId(7), &video).unwrap_err();
        assert!(error.contains("requires an image"));
    }

    #[test]
    fn prediction_cache_identity_includes_model_artifact_and_thresholds() {
        let mut model = crate::ai_tagger::models::find_model("wd14-swinv2-v3").unwrap();
        let thresholds = thresholds_from_settings(&serde_json::json!({}));
        let original = prediction_cache_key("file", &[model.clone()], &thresholds);

        model.onnx_sha256 = "changed".into();
        assert_ne!(
            original,
            prediction_cache_key("file", &[model], &thresholds)
        );
        let mut changed_thresholds = thresholds.clone();
        changed_thresholds.general += 0.01;
        assert_ne!(
            original,
            prediction_cache_key(
                "file",
                &[crate::ai_tagger::models::find_model("wd14-swinv2-v3").unwrap()],
                &changed_thresholds,
            )
        );
    }

    #[test]
    fn sqlite_media_lookup_expands_collections_and_keeps_member_identity() {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                     VALUES (7, 'media-7', 'media', 'now', 'now'),
                            (8, 'collection-8', 'collection', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (7, 'active'), (8, 'active')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO media_file
                     (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (70, 'hash-7', 'image/png', 10, 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset
                     (item_id, file_id, imported_at, updated_at)
                     VALUES (7, 70, 'now', 'now')",
                    [],
                )?;
                transaction.execute("DELETE FROM library_root WHERE item_id = 7", [])?;
                transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (8, 7, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let original = load_prediction_original(&application, ItemId(7)).unwrap();
        assert_eq!(original.file_hash, "hash-7");
        assert_eq!(original.mime_type, "image/png");
        assert_eq!(
            resolve_prediction_items(&application, &[ItemId(8)]).unwrap(),
            vec![ItemId(7)]
        );
    }
}
