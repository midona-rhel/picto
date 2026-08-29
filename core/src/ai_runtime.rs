//! Replacement-backend AI tagging execution.
//!
//! Model discovery, image preprocessing, and inference remain in `ai_tagger`.
//! This runtime only caches sessions and persists predictions on the media
//! asset that was actually analyzed.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ai_tagger::inference::{TagPrediction, Thresholds};
use crate::ai_tagger::models::{self, ModelInfo};
use crate::blob_store::BlobStore;

const MAX_MANUAL_PREDICTION_ITEMS: usize = 256;
const MAX_MANUAL_PREDICTION_MODELS: usize = 8;
const MIN_MANUAL_REVIEW_CONFIDENCE: f32 = 0.05;

trait AiInferenceHost: crate::ai_models::AiModelHost + Send + Sync {
    fn blobs(&self) -> &BlobStore;
    fn resolve_media_paths(&self, file_hashes: &[String]) -> Result<Vec<PathBuf>, String>;
    fn prediction_cache(&self) -> &crate::ai_tagger::inference::SharedPredictionCache;
    fn set_worker_status(&self, active: bool, detail: String);
}

impl AiInferenceHost for crate::library_application::LibraryApplication {
    fn blobs(&self) -> &BlobStore {
        self.blobs()
    }

    fn resolve_media_paths(&self, file_hashes: &[String]) -> Result<Vec<PathBuf>, String> {
        let hashes = file_hashes
            .iter()
            .cloned()
            .map(crate::dto::FileHash)
            .collect::<Vec<_>>();
        crate::media_io::resolve_file_paths_library(self, &hashes)
            .map(|resolved| resolved.into_iter().map(|file| file.path).collect())
    }

    fn prediction_cache(&self) -> &crate::ai_tagger::inference::SharedPredictionCache {
        self.ai_prediction_cache()
    }

    fn set_worker_status(&self, active: bool, detail: String) {
        self.set_ai_worker_status(active, detail);
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryManualPredictionTarget {
    pub root_id: picto_library::RootId,
    pub media_item_id: picto_library::MediaId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryManualPredictionRequest {
    pub targets: Vec<LibraryManualPredictionTarget>,
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
pub async fn model_status_library(
    application: &crate::library_application::LibraryApplication,
) -> Result<AiRuntimeStatus, String> {
    let settings = application.application_settings()?.value;
    model_status_for(application, settings).await
}

async fn model_status_for(
    application: &(impl crate::ai_models::AiModelHost + ?Sized),
    settings: serde_json::Value,
) -> Result<AiRuntimeStatus, String> {
    let models_root = crate::ai_models::models_root(application);
    let configured_model_slugs = configured_models(&settings)
        .iter()
        .map(|model| model.slug.clone())
        .collect::<Vec<_>>();

    let sessions = crate::ai_models::AiModelHost::ai_sessions(application)
        .lock()
        .await;
    let cached_backend = sessions
        .values()
        .find_map(|session| session.lock().ok().map(|session| session.gpu_backend()));
    let downloads = crate::ai_models::AiModelHost::ai_model_downloads(application)
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
        storage_bytes: crate::ai_models::storage_bytes(application)?,
        configured_model_slugs,
        thresholds: public_thresholds(&thresholds_from_settings(&settings)),
        cached_backend,
    })
}

/// Predict tags for explicit replacement media items without applying them.
/// Invalid item kinds and item-local read/inference failures are returned next
/// to their item IDs so one bad item does not hide the rest of the result.
pub async fn manual_predict_library(
    application: &crate::library_application::LibraryApplication,
    request: LibraryManualPredictionRequest,
) -> Result<LibraryManualPredictionResponse, String> {
    if request.targets.is_empty() {
        return Err("At least one library media target is required".into());
    }
    if request.targets.len() > MAX_MANUAL_PREDICTION_ITEMS {
        return Err(format!(
            "Manual prediction accepts at most {} image members",
            MAX_MANUAL_PREDICTION_ITEMS
        ));
    }
    let settings = application.application_settings()?.value;
    let models = resolve_prediction_models(&request.model_slugs, &settings)?;
    let thresholds = thresholds_from_settings(&settings);
    let root_ids = request
        .targets
        .iter()
        .map(|target| target.root_id)
        .collect::<BTreeSet<_>>();
    let mut results = root_ids
        .iter()
        .map(|root_id| RootPrediction {
            root_id: *root_id,
            predictions: Vec::new(),
            error: None,
        })
        .collect::<Vec<_>>();
    let owner_indexes = results
        .iter()
        .enumerate()
        .map(|(index, result)| (result.root_id, index))
        .collect::<HashMap<_, _>>();
    let mut files = Vec::new();
    let mut owners = Vec::new();
    let mut details_by_root = HashMap::new();
    for root_id in root_ids {
        match application.library().details(root_id) {
            Ok(details) => {
                details_by_root.insert(root_id, details);
            }
            Err(error) => results[owner_indexes[&root_id]].error = Some(error.to_string()),
        }
    }
    for target in request.targets {
        let owner = owner_indexes[&target.root_id];
        let Some(details) = details_by_root.get(&target.root_id) else {
            continue;
        };
        let Some(media) = details
            .media
            .iter()
            .find(|media| media.media_id == target.media_item_id)
        else {
            results[owner].error = Some("The selected media is not part of this item".into());
            continue;
        };
        if !media.facts.mime.starts_with("image/") {
            results[owner].error = Some("AI prediction requires image media".into());
            continue;
        }
        files.push((media.facts.content_hash.clone(), media.facts.mime.clone()));
        owners.push(owner);
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

pub async fn drain_auto_tag_work(
    application: &crate::library_application::LibraryApplication,
    limit: usize,
) -> Result<usize, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let work = application
        .library()
        .claim_ai_tag_work(limit, &now)
        .map_err(|error| error.to_string())?;
    if work.is_empty() {
        return Ok(0);
    }
    let settings = application.application_settings()?.value;
    let enabled = setting_bool(&settings, "aiTaggerAutoOnImport").unwrap_or(false);
    let mut completed = Vec::new();
    for item in work {
        let result = match item.root_id {
            Some(root_id) if enabled => auto_tag_root(application, root_id, &settings).await,
            Some(_) => Ok(()),
            None => Err(format!("AI-tag work {} has no root", item.work_id)),
        };
        match result {
            Ok(()) => completed.push(item.work_id),
            Err(error) => {
                application
                    .library()
                    .retry_media_work(item.work_id, item.attempt_count, &error, &now)
                    .map_err(|failure| failure.to_string())?;
            }
        }
    }
    application
        .library()
        .complete_media_work(&completed)
        .map_err(|error| error.to_string())?;
    Ok(completed.len())
}

async fn auto_tag_root(
    application: &crate::library_application::LibraryApplication,
    root_id: picto_library::RootId,
    settings: &serde_json::Value,
) -> Result<(), String> {
    let models = configured_models(settings);
    if models.is_empty() {
        return Err("Auto-tagging is enabled but no AI model is enabled".into());
    }
    let details = application
        .library()
        .details(root_id)
        .map_err(|error| error.to_string())?;
    let files = details
        .media
        .into_iter()
        .filter(|media| media.facts.mime.starts_with("image/"))
        .map(|media| (media.facts.content_hash, media.facts.mime))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Ok(());
    }
    let predictions = predict_files_cached(
        application,
        &models,
        &files,
        thresholds_from_settings(settings),
    )
    .await?;
    let write_rating = setting_bool(settings, "aiTaggerWriteRating").unwrap_or(true);
    let tags = predictions
        .iter()
        .flatten()
        .filter_map(|prediction| normalized_prediction(prediction, write_rating))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if tags.is_empty() {
        return Ok(());
    }
    application
        .library()
        .add_tag_assignments(&[picto_library::RootTagAssignment { root_id, tags }])
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn unload_library_sessions(application: &crate::library_application::LibraryApplication) {
    application.ai_sessions().lock().await.clear();
    application.set_ai_worker_status(false, "Idle");
    tracing::debug!(target: "ai_inference", "AI model session unloaded");
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
    let models_root = crate::ai_models::models_root(application);
    let mut sessions = crate::ai_models::AiModelHost::ai_sessions(application)
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
            let sessions = crate::ai_models::AiModelHost::ai_sessions(application)
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
    let _inference_lane = crate::ai_models::AiModelHost::ai_model_lifecycle(application)
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
        let paths = application.resolve_media_paths(
            &missing_files
                .iter()
                .map(|(file_hash, _)| file_hash.clone())
                .collect::<Vec<_>>(),
        )?;
        let images = missing_files
            .iter()
            .zip(paths)
            .map(|((file_hash, _), path)| ai_input_bytes(application.blobs(), &path, file_hash))
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

fn ai_input_bytes(
    blobs: &BlobStore,
    original_path: &Path,
    file_hash: &str,
) -> Result<Vec<u8>, String> {
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
        Ok(None) => {
            if let Some(thumbnail) = adjacent_thumbnail(original_path, file_hash)? {
                return Ok(thumbnail);
            }
            std::fs::read(original_path).map_err(|error| {
                format!(
                    "Failed to read original {file_hash} at {}: {error}",
                    original_path.display()
                )
            })
        }
        Err(error) => Err(format!("Failed to read thumbnail {file_hash}: {error}")),
    }
}

fn adjacent_thumbnail(original_path: &Path, file_hash: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(cd_directory) = original_path.parent() else {
        return Ok(None);
    };
    let Some(ab_directory) = cd_directory.parent() else {
        return Ok(None);
    };
    let Some(originals_directory) = ab_directory.parent() else {
        return Ok(None);
    };
    let Some(blobs_directory) = originals_directory.parent() else {
        return Ok(None);
    };
    if originals_directory
        .file_name()
        .and_then(|name| name.to_str())
        != Some("f")
        || blobs_directory.file_name().and_then(|name| name.to_str()) != Some("blobs")
    {
        return Ok(None);
    }
    let shard = Path::new("t").join(&file_hash[..2]).join(&file_hash[2..4]);
    for extension in ["jpg", "png"] {
        let path = blobs_directory
            .join(&shard)
            .join(format!("{file_hash}.{extension}"));
        match std::fs::read(&path) {
            Ok(bytes) => return Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Failed to read thumbnail {}: {error}",
                    path.display()
                ))
            }
        }
    }
    Ok(None)
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

#[cfg(test)]
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
    crate::tag_name::parse_external(&raw)
        .ok()
        .map(|(namespace, subtag)| crate::tag_name::format(&namespace, &subtag))
}

#[cfg(test)]
mod tests {

    use super::*;

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
        let original_path = blobs.find_original(hash, Some("jpg")).unwrap().unwrap().0;

        assert_eq!(
            ai_input_bytes(&blobs, &original_path, hash).unwrap(),
            b"large-original"
        );

        blobs
            .write_thumbnail(hash, b"small-thumbnail", "jpg")
            .unwrap();
        assert_eq!(
            ai_input_bytes(&blobs, &original_path, hash).unwrap(),
            b"small-thumbnail"
        );
    }
}
