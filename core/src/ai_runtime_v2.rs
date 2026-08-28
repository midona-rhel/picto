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
use crate::blob_store::{mime_to_extension, BlobStore};

const MAX_MANUAL_PREDICTION_ITEMS: usize = 256;
const MAX_MANUAL_PREDICTION_MODELS: usize = 8;
const MIN_MANUAL_REVIEW_CONFIDENCE: f32 = 0.05;

trait AiInferenceHost: crate::ai_models_v2::AiModelHost + Send + Sync {
    fn blobs(&self) -> &BlobStore;
    fn prediction_cache(&self) -> &crate::ai_tagger::inference::SharedPredictionCache;
    fn set_worker_status(&self, active: bool, detail: String);
}

impl AiInferenceHost for Application {
    fn blobs(&self) -> &BlobStore {
        self.blobs()
    }

    fn prediction_cache(&self) -> &crate::ai_tagger::inference::SharedPredictionCache {
        self.ai_prediction_cache()
    }

    fn set_worker_status(&self, active: bool, detail: String) {
        self.set_ai_worker_status(active, detail);
    }
}

impl AiInferenceHost for crate::library_application::LibraryApplication {
    fn blobs(&self) -> &BlobStore {
        self.blobs()
    }

    fn prediction_cache(&self) -> &crate::ai_tagger::inference::SharedPredictionCache {
        self.ai_prediction_cache()
    }

    fn set_worker_status(&self, active: bool, detail: String) {
        self.set_ai_worker_status(active, detail);
    }
}

/// Provenance bit shared with the existing AI tagger.

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
    pub optimization_supported: bool,
    pub optimized: bool,
    #[ts(type = "number | null")]
    pub downloaded_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub download_total_bytes: Option<u64>,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub dataset: String,
    pub reference_inference_ms: f32,
}

/// Replacement AI status derived from settings, the model bundle, and the
/// in-process session cache. No download or mutation state is implied.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "camelCase")]
pub struct AiRuntimeStatus {
    pub models: Vec<AiModelStatus>,
    #[ts(type = "number")]
    pub storage_bytes: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryManualPredictionRequest {
    pub root_ids: Vec<picto_library::RootId>,
    #[serde(default)]
    pub model_slugs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootPrediction {
    pub root_id: picto_library::RootId,
    pub predictions: Vec<AiTagPrediction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryManualPredictionResponse {
    pub predictions: Vec<RootPrediction>,
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
    model_status_for(application, settings).await
}

pub async fn model_status_library(
    application: &crate::library_application::LibraryApplication,
) -> Result<AiRuntimeStatus, String> {
    let settings = application.application_settings()?.value;
    model_status_for(application, settings).await
}

async fn model_status_for(
    application: &(impl crate::ai_models_v2::AiModelHost + ?Sized),
    settings: serde_json::Value,
) -> Result<AiRuntimeStatus, String> {
    let models_root = crate::ai_models_v2::models_root(application);
    let configured_model_slugs = configured_models(&settings)
        .iter()
        .map(|model| model.slug.clone())
        .collect::<Vec<_>>();

    let sessions = crate::ai_models_v2::AiModelHost::ai_sessions(application)
        .lock()
        .await;
    let cached_backend = sessions
        .values()
        .find_map(|session| session.lock().ok().map(|session| session.gpu_backend()));
    let downloads = crate::ai_models_v2::AiModelHost::ai_model_downloads(application)
        .lock()
        .await;
    let models = models::known_models()
        .into_iter()
        .map(|model| {
            let optimization_supported = models::optimization_supported(&models_root, &model.slug);
            let optimized = models::is_model_optimized(&models_root, &model.slug);
            let download = downloads.get(&model.slug);
            AiModelStatus {
                enabled: setting_bool(&settings, model_setting_key(&model.slug)).unwrap_or(false),
                downloaded: models::is_model_downloaded(&models_root, &model.slug),
                session_loaded: sessions.contains_key(&model.slug),
                recommended: !model.heavy,
                slug: model.slug,
                label: model.label,
                heavy: model.heavy,
                optimization_supported,
                optimized,
                downloaded_bytes: download.map(|download| {
                    download
                        .downloaded_bytes
                        .load(std::sync::atomic::Ordering::Relaxed)
                }),
                download_total_bytes: download.map(|download| download.total_bytes),
                size_bytes: model.size_bytes,
                dataset: model.dataset,
                reference_inference_ms: model.reference_inference_ms,
            }
        })
        .collect();

    Ok(AiRuntimeStatus {
        models,
        storage_bytes: crate::ai_models_v2::storage_bytes(application)?,
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
    let review_thresholds = manual_review_thresholds();
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
        match predict_files_cached(application, &models, &files, review_thresholds).await {
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

pub async fn manual_predict_library(
    application: &crate::library_application::LibraryApplication,
    request: LibraryManualPredictionRequest,
) -> Result<LibraryManualPredictionResponse, String> {
    if request.root_ids.is_empty() {
        return Err("At least one library root is required".into());
    }
    if request.root_ids.len() > MAX_MANUAL_PREDICTION_ITEMS {
        return Err(format!(
            "Manual prediction accepts at most {} library roots",
            MAX_MANUAL_PREDICTION_ITEMS
        ));
    }
    let settings = application.application_settings()?.value;
    let models = resolve_prediction_models(&request.model_slugs, &settings)?;
    let thresholds = thresholds_from_settings(&settings);
    let mut results = request
        .root_ids
        .iter()
        .map(|root_id| RootPrediction {
            root_id: *root_id,
            predictions: Vec::new(),
            error: None,
        })
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut owners = Vec::new();
    for (index, root_id) in request.root_ids.into_iter().enumerate() {
        match application.library().details(root_id) {
            Ok(details) => {
                let before = files.len();
                for media in details.media {
                    if media.facts.mime.starts_with("image/") {
                        files.push((media.facts.content_hash, media.facts.mime));
                        owners.push(index);
                    }
                }
                if files.len() == before {
                    results[index].error = Some("AI prediction requires image media".into());
                }
            }
            Err(error) => results[index].error = Some(error.to_string()),
        }
    }
    if files.len() > MAX_MANUAL_PREDICTION_ITEMS {
        return Err(format!(
            "Manual prediction accepts at most {} image members",
            MAX_MANUAL_PREDICTION_ITEMS
        ));
    }
    if !files.is_empty() {
        match predict_files_cached(application, &models, &files, manual_review_thresholds()).await {
            Ok(predictions) => {
                let mut combined = vec![HashMap::new(); results.len()];
                for (owner, predictions) in owners.into_iter().zip(predictions) {
                    for prediction in predictions {
                        let key = (
                            prediction.namespace.clone(),
                            prediction.tag.clone(),
                            prediction.model.clone(),
                        );
                        let entry = combined[owner].entry(key).or_insert(prediction.clone());
                        if prediction.confidence > entry.confidence {
                            *entry = prediction;
                        }
                    }
                }
                for (result, predictions) in results.iter_mut().zip(combined) {
                    let mut predictions = predictions.into_values().collect::<Vec<_>>();
                    predictions.sort_by(|left, right| {
                        left.namespace
                            .cmp(&right.namespace)
                            .then_with(|| left.tag.cmp(&right.tag))
                            .then_with(|| left.model.cmp(&right.model))
                    });
                    result.predictions = predictions
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
                for result in &mut results {
                    if result.error.is_none() {
                        result.error = Some(error.clone());
                    }
                }
            }
        }
    }
    Ok(LibraryManualPredictionResponse {
        predictions: results,
        thresholds: public_thresholds(&thresholds),
    })
}

pub async fn unload_sessions(application: &Application) {
    application.ai_sessions().lock().await.clear();
    application.set_ai_worker_status(false, "Idle");
    tracing::debug!(target: "ai_inference", "AI model session unloaded");
}

pub async fn unload_library_sessions(application: &crate::library_application::LibraryApplication) {
    application.ai_sessions().lock().await.clear();
    application.set_ai_worker_status(false, "Idle");
    tracing::debug!(target: "ai_inference", "AI model session unloaded");
}

fn resolve_prediction_items(
    application: &Application,
    item_ids: &[ItemId],
) -> Result<Vec<ItemId>, String> {
    let projection = application.projections().selection_snapshot();
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
                    for member in projection.group_order(item_id.0).unwrap_or_default() {
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
                Some(application.apply_media_tags(media_item_id, &tags)?)
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
    let root_item_id = application
        .projections()
        .root_for_media(media_item_id.0)
        .ok_or_else(|| format!("Media item {} has no owning root", media_item_id.0))?;
    application.store().read_result(|connection| {
        connection
            .query_row(
                "SELECT mf.file_hash, mf.mime_type
                 FROM media_asset ma
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE ma.item_id = ?1",
                [media_item_id.0],
                |row| {
                    Ok(MediaOriginal {
                        root_item_id: ItemId(root_item_id),
                        file_hash: row.get(0)?,
                        mime_type: row.get(1)?,
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
        "oppai-oracle-v1-1" => "aiTaggerOppaiOracleEnabled",
        "danbooru-tag-query-b16" => "aiTaggerDanbooruTagQueryEnabled",
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

fn manual_review_thresholds() -> Thresholds {
    Thresholds {
        general: MIN_MANUAL_REVIEW_CONFIDENCE,
        character: MIN_MANUAL_REVIEW_CONFIDENCE,
        copyright: MIN_MANUAL_REVIEW_CONFIDENCE,
        artist: MIN_MANUAL_REVIEW_CONFIDENCE,
        species: MIN_MANUAL_REVIEW_CONFIDENCE,
        rating: MIN_MANUAL_REVIEW_CONFIDENCE,
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

async fn ensure_sessions(
    application: &impl AiInferenceHost,
    models: &[ModelInfo],
) -> Result<(), String> {
    if models.len() > 1 {
        return Err("AI tagging runs exactly one model at a time".into());
    }
    let Some(model) = models.first() else {
        return Ok(());
    };
    let models_root = crate::ai_models_v2::models_root(application);
    let mut sessions = crate::ai_models_v2::AiModelHost::ai_sessions(application)
        .lock()
        .await;
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
    let adapter = model.adapter;
    let session = tokio::task::spawn_blocking(move || {
        crate::ai_tagger::inference::TaggerSession::load(
            &model_dir,
            &load_slug,
            input_size,
            channel_order,
            output_activation,
            adapter,
        )
    })
    .await
    .map_err(|error| format!("AI model load task failed: {error}"))??;
    sessions.insert(slug, std::sync::Arc::new(std::sync::Mutex::new(session)));
    Ok(())
}

async fn predict_batch(
    application: &impl AiInferenceHost,
    models: &[ModelInfo],
    images: Vec<Vec<u8>>,
    thresholds: Thresholds,
) -> Result<Vec<Vec<TagPrediction>>, String> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let images = Arc::new(images);
    let mut combined = vec![Vec::new(); images.len()];
    for (model_index, model) in models.iter().enumerate() {
        application.set_worker_status(
            true,
            format!(
                "Loading {} · model {}/{} · {} images",
                model.label,
                model_index + 1,
                models.len(),
                images.len()
            ),
        );
        ensure_sessions(application, std::slice::from_ref(model)).await?;
        let (slug, session, backend) = {
            let sessions = crate::ai_models_v2::AiModelHost::ai_sessions(application)
                .lock()
                .await;
            sessions
                .get(&model.slug)
                .cloned()
                .map(|session| {
                    let backend = session
                        .lock()
                        .map(|session| session.gpu_backend())
                        .unwrap_or_else(|_| "Unavailable".into());
                    (model.slug.clone(), session, backend)
                })
                .ok_or_else(|| format!("AI model session '{}' is not loaded", model.slug))?
        };
        for image_index in 0..images.len() {
            application.set_worker_status(
                true,
                format!(
                    "Running {} on {} · model {}/{} · image {}/{}",
                    model.label,
                    backend,
                    model_index + 1,
                    models.len(),
                    image_index + 1,
                    images.len()
                ),
            );
            let images = Arc::clone(&images);
            let session = Arc::clone(&session);
            let thresholds = thresholds.clone();
            let slug = slug.clone();
            let mut inferred = tokio::task::spawn_blocking(move || {
                let started = Instant::now();
                let spec = session
                    .lock()
                    .map_err(|_| format!("AI model session '{slug}' lock was poisoned"))?
                    .input_spec();
                let preprocess_started = Instant::now();
                let prepared =
                    crate::ai_tagger::inference::prepare_input(&images[image_index], spec)?;
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
                    image = image_index + 1,
                    "AI model image completed"
                );
                Ok::<_, String>(predictions)
            })
            .await
            .map_err(|error| format!("AI inference task failed: {error}"))??;
            let predictions = inferred
                .pop()
                .ok_or_else(|| "AI inference returned no image result".to_string())?;
            combined[image_index].extend(predictions);
        }
    }
    Ok(combined)
}

async fn predict_files_cached(
    application: &impl AiInferenceHost,
    models: &[ModelInfo],
    files: &[(String, String)],
    thresholds: Thresholds,
) -> Result<Vec<Vec<TagPrediction>>, String> {
    // Manual review and background ingestion share one inference lane. The
    // selected models run serially and only the current model remains loaded.
    let started = Instant::now();
    let _inference_lane = crate::ai_models_v2::AiModelHost::ai_model_lifecycle(application)
        .lock()
        .await;
    let mut activity = AiWorkerActivity::new(application, files.len(), models.len(), started);
    application.set_worker_status(
        true,
        format!("Checking prediction cache · {} images", files.len()),
    );
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
            .prediction_cache()
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
            .map(|(file_hash, mime_type)| ai_input_bytes(application.blobs(), file_hash, mime_type))
            .collect::<Result<Vec<_>, String>>()?;
        let inferred = predict_batch(application, models, images, thresholds).await?;
        if inferred.len() != missing_files.len() {
            return Err("AI inference returned an unexpected number of results".into());
        }
        let inferred = inferred.into_iter().map(Arc::new).collect::<Vec<_>>();
        let mut cache = application
            .prediction_cache()
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
    let resolved = results
        .into_iter()
        .map(|predictions| {
            predictions
                .map(|predictions| (*predictions).clone())
                .ok_or_else(|| "AI prediction cache left an unresolved result".to_string())
        })
        .collect();
    activity.complete(cache_hits, missing_files.len());
    resolved
}

fn ai_input_bytes(blobs: &BlobStore, file_hash: &str, mime_type: &str) -> Result<Vec<u8>, String> {
    match blobs.read_thumbnail(file_hash) {
        Ok(Some(thumbnail)) => {
            tracing::debug!(
                target: "ai_inference",
                file_hash,
                bytes = thumbnail.len(),
                "Using prepared thumbnail for AI input"
            );
            Ok(thumbnail)
        }
        Ok(None) => blobs
            .read_original(file_hash, Some(mime_to_extension(mime_type)))
            .map_err(|error| format!("Failed to read original {file_hash}: {error}")),
        Err(error) => Err(format!("Failed to read thumbnail {file_hash}: {error}")),
    }
}

struct AiWorkerActivity<'a> {
    application: &'a dyn AiInferenceHost,
    files: usize,
    models: usize,
    started: Instant,
    completed: bool,
}

impl<'a> AiWorkerActivity<'a> {
    fn new(
        application: &'a dyn AiInferenceHost,
        files: usize,
        models: usize,
        started: Instant,
    ) -> Self {
        Self {
            application,
            files,
            models,
            started,
            completed: false,
        }
    }

    fn complete(&mut self, cache_hits: usize, inferred: usize) {
        let elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        self.application.set_worker_status(
            false,
            format!(
                "Last run {:.1} s · {} images · {} model(s) · {} cached · {} inferred",
                elapsed_ms / 1000.0,
                self.files,
                self.models,
                cache_hits,
                inferred
            ),
        );
        tracing::info!(
            target: "ai_inference",
            total_ms = elapsed_ms,
            images = self.files,
            models = self.models,
            cache_hits,
            inferred,
            "AI tagging run completed"
        );
        self.completed = true;
    }
}

impl Drop for AiWorkerActivity<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.application.set_worker_status(
                false,
                format!(
                    "Last run stopped after {:.1} s · {} images · {} model(s)",
                    self.started.elapsed().as_secs_f64(),
                    self.files,
                    self.models
                ),
            );
        }
    }
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
    fn manual_review_retains_predictions_down_to_the_slider_minimum() {
        let thresholds = manual_review_thresholds();
        assert_eq!(thresholds.general, MIN_MANUAL_REVIEW_CONFIDENCE);
        assert_eq!(thresholds.character, MIN_MANUAL_REVIEW_CONFIDENCE);
        assert_eq!(thresholds.copyright, MIN_MANUAL_REVIEW_CONFIDENCE);
        assert_eq!(thresholds.artist, MIN_MANUAL_REVIEW_CONFIDENCE);
        assert_eq!(thresholds.species, MIN_MANUAL_REVIEW_CONFIDENCE);
        assert_eq!(thresholds.rating, MIN_MANUAL_REVIEW_CONFIDENCE);
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
    fn ai_input_prefers_the_prepared_thumbnail_and_falls_back_to_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(directory.path()).unwrap();
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        blobs
            .write_original(hash, b"large-original", Some("jpg"))
            .unwrap();

        assert_eq!(
            ai_input_bytes(&blobs, hash, "image/jpeg").unwrap(),
            b"large-original"
        );

        blobs
            .write_thumbnail(hash, b"small-thumbnail", "jpg")
            .unwrap();
        assert_eq!(
            ai_input_bytes(&blobs, hash, "image/jpeg").unwrap(),
            b"small-thumbnail"
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
                Ok(())
            })
            .unwrap();
        application
            .projections()
            .apply_membership_delta(8, 7, true)
            .unwrap();
        application
            .projections()
            .apply_root_delta(
                8,
                crate::app::ItemKind::Collection,
                Some(crate::app::Lifecycle::Active),
            )
            .unwrap();
        application
            .projections()
            .apply_structure_delta(crate::projection_v2::StructureProjectionDelta {
                group_orders: vec![crate::projection_v2::GroupOrderProjectionChange {
                    collection_id: 8,
                    media_ids: vec![7],
                }],
                ..Default::default()
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
