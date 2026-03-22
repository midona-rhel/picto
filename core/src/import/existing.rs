//! Shared merge path for "ingest hit an existing file".
//!
//! Manual import and subscription import both converge here so that
//! status restoration, metadata merge, source URL merge, and optional
//! subscription ownership behave consistently.

use std::collections::{HashMap, HashSet};

use tracing::warn;

use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::Domain;
use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone)]
pub struct ExistingImportMergeRequest {
    pub restore_status: Option<i64>,
    pub tag_strings: Vec<String>,
    pub source_urls: Vec<String>,
    pub created_at: Option<String>,
    pub name: Option<String>,
    pub note_entries: HashMap<String, String>,
    pub subscription_id: Option<i64>,
    pub change_origin: &'static str,
}

pub async fn merge_existing_import_target(
    db: &SqliteDatabase,
    hex_hash: &str,
    request: ExistingImportMergeRequest,
) -> Result<bool, String> {
    let Some(existing) = db.get_file_by_hash(hex_hash).await? else {
        return Ok(false);
    };

    let mut any_change = false;
    let mut ownership_change = false;
    let mut status_restored = false;
    let mut tags_changed = false;
    let mut metadata_changed = false;

    if let Some(status) = request.restore_status {
        if existing.status == 2 && status != 2 {
            let file_id = existing.file_id;
            let is_merge_loser = db
                .with_read_conn(move |conn| {
                    crate::duplicates::db::is_confirmed_merge_loser(conn, file_id)
                })
                .await?;
            if is_merge_loser {
                tracing::info!(hash = %hex_hash, "skipping status restore: file is loser in confirmed duplicate merge");
            } else {
                db.set_entity_status_by_hash(hex_hash, status).await?;
                any_change = true;
                status_restored = true;
            }
        }
    }

    if !request.tag_strings.is_empty() {
        let existing_tags = db.get_entity_tags(hex_hash).await?;
        let existing_set: HashSet<String> = existing_tags
            .into_iter()
            .map(|t| crate::tags::normalize::combine_tag(&t.namespace, &t.subtag))
            .collect();
        let missing: Vec<String> = request
            .tag_strings
            .iter()
            .filter(|tag| !existing_set.contains(*tag))
            .cloned()
            .collect();
        if !missing.is_empty() {
            db.add_tags_by_strings(hex_hash, &missing).await?;
            any_change = true;
            tags_changed = true;
        }
    }

    if let Some(ref name) = request.name {
        let current_name = existing.name.as_deref().unwrap_or("");
        if current_name != name {
            db.set_file_name(hex_hash, Some(name)).await?;
            any_change = true;
            metadata_changed = true;
        }
    }

    if let Some(ref created_at) = request.created_at {
        if !created_at.is_empty() && existing.imported_at != *created_at {
            db.set_date_created(hex_hash, created_at).await?;
            any_change = true;
            metadata_changed = true;
        }
    }

    if !request.note_entries.is_empty() {
        let current_notes: HashMap<String, String> = existing
            .notes
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        let mut merged_notes = current_notes.clone();
        for (key, value) in &request.note_entries {
            merged_notes
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        if merged_notes != current_notes {
            let json = serde_json::to_string(&merged_notes)
                .map_err(|e| format!("Notes serialization error: {e}"))?;
            db.set_notes(hex_hash, Some(&json)).await?;
            any_change = true;
            metadata_changed = true;
        }
    }

    if !request.source_urls.is_empty() {
        let current_urls: Vec<String> = existing
            .source_urls_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        let original_len = current_urls.len();
        let mut merged_urls = current_urls.clone();
        let mut seen: HashSet<String> = current_urls.into_iter().collect();
        for url in &request.source_urls {
            if !url.is_empty() && seen.insert(url.clone()) {
                merged_urls.push(url.clone());
            }
        }
        if merged_urls.len() > original_len {
            let json = serde_json::to_string(&merged_urls)
                .map_err(|e| format!("URLs serialization error: {e}"))?;
            db.set_source_urls(hex_hash, Some(&json)).await?;
            any_change = true;
            metadata_changed = true;
        }
    }

    if let Some(subscription_id) = request.subscription_id {
        match db.add_subscription_entity(subscription_id, hex_hash).await {
            Ok(changed) => ownership_change = changed,
            Err(e) => warn!(error = %e, "Failed to record subscription-file mapping"),
        };
    }

    if any_change || ownership_change {
        let hash = hex_hash.to_string();
        let mut impact = if status_restored {
            ChangeImpact::file_lifecycle(db).file_hashes(vec![hash.clone()])
        } else if tags_changed {
            ChangeImpact::file_tags(hash.clone())
        } else if metadata_changed {
            ChangeImpact::file_metadata(hash.clone())
        } else {
            ChangeImpact::new().file_hashes(vec![hash.clone()])
        };

        if status_restored && tags_changed {
            impact = impact.tags_changed().all_smart_folder_scopes_changed();
        }

        if ownership_change {
            impact = impact.add_domains(&[Domain::Subscriptions, Domain::Sidebar]);
            let hashes = impact.file_hashes.get_or_insert_with(Vec::new);
            if !hashes.iter().any(|hash| hash == hex_hash) {
                hashes.push(hash);
            }
        }
        crate::events::emit_state_changed(request.change_origin, impact);
    }

    Ok(any_change || ownership_change)
}
