//! Handler functions for media metadata operations.

use std::collections::HashMap;

use serde::Deserialize;
use ts_rs::TS;

use crate::runtime_contract::state_change::MediaMetadataField;
use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetMediaEntityMetadataInput {
    pub hash: String,
}

/// Unified metadata update. All fields except `hash` are optional —
/// only present fields are applied. Use `null` to clear rating/name.
#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateMediaEntityMetadataInput {
    pub hash: String,
    #[serde(default, deserialize_with = "deserialize_some")]
    #[ts(type = "number | null")]
    pub rating: Option<Option<i64>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    #[ts(type = "string | null")]
    pub name: Option<Option<String>>,
    #[serde(default)]
    pub notes: Option<HashMap<String, String>>,
    #[serde(default)]
    pub source_urls: Option<Vec<String>>,
}

use super::super::common::deserialize_some;

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_media_entity_metadata(
    state: &AppState,
    input: GetMediaEntityMetadataInput,
) -> Result<serde_json::Value, String> {
    let result =
        crate::metadata::query::MetadataQuery::get_entity_all_metadata(&state.db, input.hash)
            .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn update_media_entity_metadata(
    state: &AppState,
    input: UpdateMediaEntityMetadataInput,
) -> Result<(), String> {
    let hash = input.hash;
    let mut changed_fields = Vec::new();
    let mut affected_hashes = vec![hash.clone()];

    if let Some(rating) = input.rating {
        // Cascade rating to collection members
        let hashes = state
            .db
            .expand_hashes_for_collections(&[hash.clone()])
            .await?;
        for h in &hashes {
            state.db.update_rating(h, rating).await?;
        }
        affected_hashes = hashes;
        changed_fields.push(MediaMetadataField::Rating);
    }

    if let Some(name) = input.name {
        state.db.set_file_name(&hash, name.as_deref()).await?;
        changed_fields.push(MediaMetadataField::Name);
    }

    if let Some(notes) = input.notes {
        let json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        state.db.set_notes(&hash, Some(&json)).await?;
        changed_fields.push(MediaMetadataField::Notes);
    }

    if let Some(ref urls) = input.source_urls {
        let urls_json = if urls.is_empty() {
            None
        } else {
            Some(serde_json::to_string(urls).map_err(|e| e.to_string())?)
        };
        state
            .db
            .set_source_urls(&hash, urls_json.as_deref())
            .await?;
        changed_fields.push(MediaMetadataField::SourceUrls);
    }

    if changed_fields.is_empty() {
        return Ok(());
    }

    let impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .file_hashes(affected_hashes)
        .media_fields_changed(&changed_fields)
        .smart_folder_scopes_changed_for_media_fields(&changed_fields);
    crate::events::emit_state_changed("update_media_entity_metadata", impact);
    Ok(())
}

pub async fn get_storage_stats(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let stats = state.db.aggregate_file_stats().await?;
    let breakdown = state.db.aggregate_media_type_breakdown().await?;
    let blob_store = state.blob_store.clone();
    let (originals_disk, thumbnails_disk) =
        tokio::task::spawn_blocking(move || blob_store.disk_usage())
            .await
            .map_err(|e| e.to_string())?;

    let mut result = serde_json::to_value(&stats).map_err(|e| e.to_string())?;
    let obj = result.as_object_mut().ok_or("expected object")?;
    obj.insert(
        "breakdown".to_string(),
        serde_json::to_value(&breakdown).map_err(|e| e.to_string())?,
    );
    obj.insert(
        "originals_disk".to_string(),
        serde_json::Value::Number(originals_disk.into()),
    );
    obj.insert(
        "thumbnails_disk".to_string(),
        serde_json::Value::Number(thumbnails_disk.into()),
    );
    Ok(result)
}
