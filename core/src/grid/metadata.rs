//! Metadata batch prefetch — fetches full metadata for a batch of file hashes.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Instant;

use chrono::Utc;

use crate::sqlite::projections::ResolvedMetadataFull;
use crate::sqlite::SqliteDatabase;
use crate::metadata::controller::file_tag_to_resolved_info;
use crate::types::{
    DominantColorDto, FileAllMetadata,
    EntityDetails, EntityMetadataBatchResponse,
    ResolvedTagInfo,
};

static METADATA_BATCH_PREFETCH_SEMAPHORE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

fn metadata_batch_prefetch_semaphore() -> &'static tokio::sync::Semaphore {
    METADATA_BATCH_PREFETCH_SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(2))
}

pub async fn get_files_metadata_batch(
    db: &SqliteDatabase,
    hashes: Vec<String>,
) -> Result<EntityMetadataBatchResponse, String> {
    const MAX_BATCH: usize = 200;
    const SLOW_BATCH_WARN_MS: f64 = 200.0;
    const SLOW_STAGE_WARN_MS: f64 = 50.0;

    let _permit = metadata_batch_prefetch_semaphore()
        .acquire()
        .await
        .map_err(|_| "metadata batch prefetch queue closed".to_string())?;

    let batch_started = Instant::now();
    let mut seen = HashSet::with_capacity(hashes.len());
    let hashes: Vec<String> = hashes
        .into_iter()
        .filter(|h| seen.insert(h.clone()))
        .take(MAX_BATCH)
        .collect();
    let mut items: HashMap<String, FileAllMetadata> = HashMap::with_capacity(hashes.len());
    let mut missing = Vec::new();

    let local_started = Instant::now();
    let projections = db.get_files_metadata_batch(hashes.clone()).await?;
    let local_ms = local_started.elapsed().as_secs_f64() * 1000.0;

    let mut proj_map: HashMap<String, ResolvedMetadataFull> = HashMap::new();
    for p in projections {
        proj_map.insert(p.resolved.file.hash.clone(), p);
    }

    let merge_started = Instant::now();
    for hash in &hashes {
        if let Some(full) = proj_map.remove(hash) {
            let tags: Vec<ResolvedTagInfo> = full
                .resolved
                .tags
                .into_iter()
                .map(file_tag_to_resolved_info)
                .collect();

            let source_urls: Option<serde_json::Value> = full
                .source_urls_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            let notes: Option<serde_json::Value> = full
                .notes
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            let dominant_colors: Option<Vec<DominantColorDto>> = if full.colors.is_empty() {
                None
            } else {
                Some(
                    full.colors
                        .into_iter()
                        .map(|(hex, l, a, b)| DominantColorDto { hex, l, a, b })
                        .collect(),
                )
            };

            let slim = full.resolved.file;
            let has_thumbnail =
                slim.mime.starts_with("image/") || slim.mime.starts_with("video/");
            items.insert(
                hash.clone(),
                FileAllMetadata {
                    file: EntityDetails {
                        hash: slim.hash,
                        name: slim.name,
                        size: slim.size,
                        mime: slim.mime,
                        width: slim.width,
                        height: slim.height,
                        duration_ms: slim.duration_ms,
                        num_frames: slim.num_frames,
                        has_audio: slim.has_audio,
                        status: crate::types::status_to_string(slim.status as i64).to_string(),
                        rating: slim.rating,
                        view_count: slim.view_count,
                        source_urls,
                        imported_at: slim.imported_at,
                        has_thumbnail,
                        blurhash: slim.blurhash,
                        dominant_color_hex: slim.dominant_color_hex,
                        dominant_colors,
                        notes,
                    },
                    tags,
                    parent_tags: Vec::new(),
                },
            );
        } else {
            missing.push(hash.clone());
        }
    }
    let merge_ms = merge_started.elapsed().as_secs_f64() * 1000.0;
    let total_ms = batch_started.elapsed().as_secs_f64() * 1000.0;

    if total_ms >= SLOW_BATCH_WARN_MS
        || local_ms >= SLOW_STAGE_WARN_MS
        || merge_ms >= SLOW_STAGE_WARN_MS
    {
        tracing::warn!(
            target: "picto::core::grid_controller",
            "slow get_files_metadata_batch total_ms={:.2} local_ms={:.2} merge_ms={:.2} req_hashes={} missing={}",
            total_ms,
            local_ms,
            merge_ms,
            hashes.len(),
            missing.len(),
        );
    }

    crate::perf::record_files_metadata_batch(
        total_ms,
        local_ms,
        merge_ms,
        hashes.len(),
        missing.len(),
    );

    Ok(EntityMetadataBatchResponse {
        items,
        missing,
        generated_at: Utc::now().to_rfc3339(),
    })
}
