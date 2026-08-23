//! Replacement-backend AI tagging execution.
//!
//! Model discovery, image preprocessing, and inference remain in `ai_tagger`.
//! This runtime only caches sessions and persists predictions on the media
//! asset that was actually analyzed.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::ai_tagger::inference::{TagPrediction, Thresholds};
use crate::ai_tagger::models::{self, ModelInfo};
use crate::app::{Application, ItemId, MutationReceipt};
use crate::blob_store::mime_to_extension;

/// Provenance bit shared with the existing AI tagger.
const AI_PROVENANCE_MASK: i64 = 1 << 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaOriginal {
    root_item_id: ItemId,
    file_hash: String,
    mime_type: String,
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
    application.store().read(|connection| {
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
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Media item {} has no persisted original: {error}",
                        media_item_id.0
                    ),
                )))
            })
    })
}

fn models_root(application: &Application) -> PathBuf {
    application
        .store()
        .library_root()
        .parent()
        .unwrap_or_else(|| application.store().library_root())
        .join("models")
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
    let models_root = models_root(application);
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
}
