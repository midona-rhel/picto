//! Handler functions for media metadata operations.

use std::collections::HashMap;

use serde::Deserialize;
use ts_rs::TS;

use crate::runtime_contract::state_change::MediaMetadataField;
use crate::sqlite::EntityExpansionMode;
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

async fn expanded_entity_hashes_for_write(
    state: &AppState,
    hash: &str,
) -> Result<Vec<String>, String> {
    state
        .db
        .resolve_entity_hashes_with_expansion(
            &[hash.to_string()],
            EntityExpansionMode::EntityAndDescendants,
        )
        .await
        .map(|pairs| {
            pairs
                .into_iter()
                .map(|(entity_hash, _)| entity_hash)
                .filter(|entity_hash| !entity_hash.is_empty())
                .collect::<Vec<_>>()
        })
}

fn descendant_hashes(top_level_hash: &str, effective_hashes: &[String]) -> Vec<String> {
    effective_hashes
        .iter()
        .filter(|hash| hash.as_str() != top_level_hash)
        .cloned()
        .collect()
}

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
        let resolved = expanded_entity_hashes_for_write(state, &hash).await?;
        for h in &resolved {
            if !h.is_empty() {
                state.db.update_rating(h, rating).await?;
            }
        }
        affected_hashes = resolved;
        if affected_hashes.is_empty() {
            affected_hashes = vec![hash.clone()];
        }
        changed_fields.push(MediaMetadataField::Rating);
    }

    if let Some(name) = input.name {
        state.db.set_file_name(&hash, name.as_deref()).await?;
        changed_fields.push(MediaMetadataField::Name);
    }

    if let Some(notes) = input.notes {
        let json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        let resolved = expanded_entity_hashes_for_write(state, &hash).await?;
        for target_hash in &resolved {
            state.db.set_notes(target_hash, Some(&json)).await?;
        }
        affected_hashes = resolved;
        changed_fields.push(MediaMetadataField::Notes);
    }

    if let Some(ref urls) = input.source_urls {
        let urls_json = if urls.is_empty() {
            None
        } else {
            Some(serde_json::to_string(urls).map_err(|e| e.to_string())?)
        };
        let resolved = expanded_entity_hashes_for_write(state, &hash).await?;
        for target_hash in &resolved {
            state
                .db
                .set_source_urls(target_hash, urls_json.as_deref())
                .await?;
        }
        affected_hashes = resolved;
        changed_fields.push(MediaMetadataField::SourceUrls);
    }

    if changed_fields.is_empty() {
        return Ok(());
    }

    let impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .entity_hashes(vec![hash.clone()])
        .member_hashes(descendant_hashes(&hash, &affected_hashes))
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
