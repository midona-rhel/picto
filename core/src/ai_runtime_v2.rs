//! Replacement-backend AI tagging execution.
//!
//! Model discovery, image preprocessing, and inference remain in `ai_tagger`.
//! This runtime only caches sessions and persists predictions on the media
//! asset that was actually analyzed.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelStatus {
    pub slug: String,
    pub label: String,
    pub enabled: bool,
    pub downloaded: bool,
    pub session_loaded: bool,
    pub recommended: bool,
    pub heavy: bool,
    pub size_bytes: u64,
    pub dataset: String,
}

/// Replacement AI status derived from settings, the model bundle, and the
/// in-process session cache. No download or mutation state is implied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRuntimeStatus {
    pub models: Vec<AiModelStatus>,
    pub configured_model_slugs: Vec<String>,
    pub thresholds: Thresholds,
    pub cached_backend: Option<String>,
}

/// Read-only manual prediction request. Item IDs are logical replacement
/// identities; physical file hashes never cross this boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManualPredictionRequest {
    pub item_ids: Vec<ItemId>,
    #[serde(default)]
    pub model_slugs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPrediction {
    pub media_item_id: ItemId,
    pub predictions: Vec<TagPrediction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualPredictionResponse {
    pub predictions: Vec<MediaPrediction>,
    pub thresholds: Thresholds,
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
        thresholds: thresholds_from_settings(&settings),
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
            "Manual prediction accepts at most {} media items after expanding collections",
            MAX_MANUAL_PREDICTION_ITEMS
        ));
    }

    let models = select_prediction_models(&request.model_slugs, application).await?;
    let settings = crate::settings_v2::application_settings(application)?.value;
    let thresholds = thresholds_from_settings(&settings);
    let session_error = ensure_sessions(application, &models).await.err();

    let mut results = Vec::with_capacity(media_item_ids.len());
    for media_item_id in media_item_ids {
        let result =
            match load_prediction_original(application, media_item_id).and_then(|original| {
                validate_image_original(media_item_id, &original)?;
                Ok(original)
            }) {
                Err(error) => Err(error),
                Ok(original) => match &session_error {
                    Some(error) => Err(error.clone()),
                    None => {
                        predict_one(
                            application,
                            media_item_id,
                            &models,
                            thresholds.clone(),
                            original,
                        )
                        .await
                    }
                },
            };
        results.push(match result {
            Ok(predictions) => MediaPrediction {
                media_item_id,
                predictions,
                error: None,
            },
            Err(error) => MediaPrediction {
                media_item_id,
                predictions: Vec::new(),
                error: Some(error),
            },
        });
    }

    Ok(ManualPredictionResponse {
        predictions: results,
        thresholds,
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
    if slugs.len() > MAX_MANUAL_PREDICTION_MODELS {
        return Err(format!(
            "Manual prediction accepts at most {} models",
            MAX_MANUAL_PREDICTION_MODELS
        ));
    }

    let mut seen = BTreeSet::new();
    slugs
        .into_iter()
        .filter(|slug| seen.insert(slug.clone()))
        .map(|slug| models::find_model(&slug).ok_or_else(|| format!("Unknown AI model '{slug}'")))
        .collect()
}

async fn predict_one(
    application: &Application,
    media_item_id: ItemId,
    models: &[ModelInfo],
    thresholds: Thresholds,
    original: PredictionOriginal,
) -> Result<Vec<TagPrediction>, String> {
    let image_bytes = application
        .blobs()
        .read_original(
            &original.file_hash,
            Some(mime_to_extension(&original.mime_type)),
        )
        .map_err(|error| {
            format!(
                "Failed to read original for media item {}: {error}",
                media_item_id.0
            )
        })?;
    predict(application, models, image_bytes, thresholds).await
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
                "Media item {} is a collection; AI prediction requires an image item",
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
    let original = load_media_original(application, media_item_id)?;
    if !original.mime_type.starts_with("image/") {
        return Err(format!(
            "AI tagging requires an image original; media item {} has MIME type {}",
            media_item_id.0, original.mime_type
        ));
    }

    let settings = crate::settings_v2::application_settings(application)?.value;
    let models = configured_models(&settings);
    if models.is_empty() {
        return Ok(AiTagExecutionResult {
            media_item_id,
            root_item_id: original.root_item_id,
            models: Vec::new(),
            predictions: 0,
            applied_tags: Vec::new(),
            receipt: None,
        });
    }
    let thresholds = thresholds_from_settings(&settings);
    let write_rating = setting_bool(&settings, "aiTaggerWriteRating").unwrap_or(true);

    let extension = mime_to_extension(&original.mime_type);
    let image_bytes = application
        .blobs()
        .read_original(&original.file_hash, Some(extension))
        .map_err(|error| {
            format!(
                "Failed to read original for media item {}: {error}",
                media_item_id.0
            )
        })?;

    let model_slugs = models.iter().map(|model| model.slug.clone()).collect();
    ensure_sessions(application, &models).await?;
    let predictions = predict(application, &models, image_bytes, thresholds).await?;

    let tags = normalize_predictions(&predictions, write_rating);
    let receipt = if tags.is_empty() {
        None
    } else {
        Some(application.apply_media_tags(media_item_id, &tags, AI_PROVENANCE_MASK)?)
    };

    Ok(AiTagExecutionResult {
        media_item_id,
        root_item_id: original.root_item_id,
        models: model_slugs,
        predictions: predictions.len(),
        applied_tags: tags,
        receipt,
    })
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
    let models_root = crate::ai_models_v2::models_root(application);
    for model in models {
        if application
            .ai_sessions()
            .lock()
            .await
            .contains_key(&model.slug)
        {
            continue;
        }
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
        application
            .ai_sessions()
            .lock()
            .await
            .entry(slug)
            .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(session)));
    }
    Ok(())
}

async fn predict(
    application: &Application,
    models: &[ModelInfo],
    image_bytes: Vec<u8>,
    thresholds: Thresholds,
) -> Result<Vec<TagPrediction>, String> {
    let sessions = {
        let sessions = application.ai_sessions().lock().await;
        models
            .iter()
            .map(|model| {
                sessions
                    .get(&model.slug)
                    .cloned()
                    .map(|session| (model.slug.clone(), session))
                    .ok_or_else(|| format!("AI model session '{}' is not loaded", model.slug))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let image_bytes = std::sync::Arc::new(image_bytes);
    let mut predictions = Vec::new();
    for (slug, session) in sessions {
        let bytes = std::sync::Arc::clone(&image_bytes);
        let thresholds = thresholds.clone();
        let model_predictions = tokio::task::spawn_blocking(move || {
            session
                .lock()
                .map_err(|_| format!("AI model session '{slug}' lock was poisoned"))?
                .predict(bytes.as_slice(), &thresholds)
        })
        .await
        .map_err(|error| format!("AI inference task failed: {error}"))??;
        predictions.extend(model_predictions);
    }
    Ok(predictions)
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
        "artist" => "creator",
        "copyright" => "series",
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
        let too_many_models = vec!["wd14-swinv2-v3".to_string(); MAX_MANUAL_PREDICTION_MODELS + 1];
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
