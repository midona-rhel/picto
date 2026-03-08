//! Metadata batch prefetch — fetches full metadata for a batch of file hashes,
//! merging local DB projections with PTR overlay tags.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Instant;

use chrono::Utc;

use crate::ptr::controller::PtrController;
use crate::sqlite::projections::ResolvedMetadataFull;
use crate::tags::db::FileTagInfo;
use crate::sqlite::SqliteDatabase;
use crate::ptr::db::PtrSqliteDatabase;
use crate::tags::normalize;
use crate::types::{
    tag_display_key, DominantColorDto, FileAllMetadata,
    EntityDetails, EntityMetadataBatchResponse,
    ResolvedTagInfo,
};

static METADATA_BATCH_PREFETCH_SEMAPHORE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

fn metadata_batch_prefetch_semaphore() -> &'static tokio::sync::Semaphore {
    METADATA_BATCH_PREFETCH_SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(2))
}

fn file_tag_to_resolved_info(t: FileTagInfo) -> ResolvedTagInfo {
    let raw_tag = normalize::combine_tag(&t.namespace, &t.subtag);
    let disp_ns = t.display_ns.as_deref().unwrap_or(&t.namespace);
    let disp_st = t.display_st.as_deref().unwrap_or(&t.subtag);
    let display_tag = tag_display_key(disp_ns, disp_st);
    let read_only = t.source != "local";
    ResolvedTagInfo {
        raw_tag,
        display_tag,
        namespace: t.display_ns.unwrap_or(t.namespace),
        subtag: t.display_st.unwrap_or(t.subtag),
        source: t.source,
        read_only,
    }
}

pub async fn get_files_metadata_batch(
    db: &SqliteDatabase,
    ptr_db: &PtrSqliteDatabase,
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

    let local_hashes_req = hashes.clone();
    let ptr_hashes_req = hashes.clone();
    let local_fut = async {
        let local_started = Instant::now();
        let projections = db.get_files_metadata_batch(local_hashes_req).await?;
        let local_ms = local_started.elapsed().as_secs_f64() * 1000.0;
        Ok::<_, String>((projections, local_ms))
    };
    let ptr_fut = async {
        let ptr_started = Instant::now();
        let negative_cached: HashSet<String> =
            PtrController::batch_check_negative(ptr_db, ptr_hashes_req.clone())
                .await?
                .into_iter()
                .collect();

        let ptr_lookup_hashes: Vec<String> = ptr_hashes_req
            .iter()
            .filter(|h| !negative_cached.contains(*h))
            .cloned()
            .collect();
        let ptr_lookup_count = ptr_lookup_hashes.len();

        let ptr_overlay_map: HashMap<String, Vec<crate::ptr::db::tags::PtrResolvedTag>> =
            PtrController::batch_get_overlay(ptr_db, ptr_lookup_hashes.clone())
                .await?
                .into_iter()
                .collect();

        let ptr_overlay_hits: HashSet<String> =
            ptr_overlay_map.keys().cloned().collect();
        let new_negative_hashes: Vec<String> = ptr_lookup_hashes
            .into_iter()
            .filter(|h| !ptr_overlay_hits.contains(h))
            .collect();
        if !new_negative_hashes.is_empty() {
            // Memory-only — avoids writer lock contention during sync.
            // DB negative cache is populated during overlay rebuild.
            ptr_db
                .add_negative_cache_mem_only(new_negative_hashes)
                .await;
        }
        let ptr_ms = ptr_started.elapsed().as_secs_f64() * 1000.0;

        Ok::<_, String>((ptr_lookup_count, ptr_overlay_map, ptr_overlay_hits, ptr_ms))
    };

    let (local_res, ptr_res) = tokio::join!(local_fut, ptr_fut);
    let (projections, local_ms) = local_res?;
    let (ptr_lookup_count, mut ptr_overlay_map, ptr_overlay_hits, ptr_ms) = ptr_res?;

    let mut proj_map: HashMap<String, ResolvedMetadataFull> = HashMap::new();
    for p in projections {
        proj_map.insert(p.resolved.file.hash.clone(), p);
    }

    let local_hashes: Vec<String> = proj_map.keys().cloned().collect();

    let merge_started = Instant::now();
    for hash in &hashes {
        if let Some(full) = proj_map.remove(hash) {
            let ptr_tags = ptr_overlay_map.remove(hash).unwrap_or_default();

            let mut seen = HashSet::new();
            let mut tags: Vec<ResolvedTagInfo> = full
                .resolved
                .tags
                .into_iter()
                .map(|t| {
                    let info = file_tag_to_resolved_info(t);
                    seen.insert(info.display_tag.clone());
                    info
                })
                .collect();

            for pt in ptr_tags {
                let display = tag_display_key(&pt.display_ns, &pt.display_st);
                if !seen.contains(&display) {
                    seen.insert(display.clone());
                    tags.push(ResolvedTagInfo {
                        raw_tag: normalize::combine_tag(&pt.raw_ns, &pt.raw_st),
                        display_tag: display,
                        namespace: pt.display_ns,
                        subtag: pt.display_st,
                        source: "ptr".to_string(),
                        read_only: true,
                    });
                }
            }

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
        || ptr_ms >= SLOW_STAGE_WARN_MS
        || merge_ms >= SLOW_STAGE_WARN_MS
    {
        tracing::warn!(
            target: "picto::core::grid_controller",
            "slow get_files_metadata_batch total_ms={:.2} local_ms={:.2} ptr_ms={:.2} merge_ms={:.2} req_hashes={} local_hits={} ptr_lookup={} ptr_hits={} missing={}",
            total_ms,
            local_ms,
            ptr_ms,
            merge_ms,
            hashes.len(),
            local_hashes.len(),
            ptr_lookup_count,
            ptr_overlay_hits.len(),
            missing.len(),
        );
    }

    crate::perf::record_files_metadata_batch(
        total_ms,
        local_ms,
        ptr_ms,
        merge_ms,
        hashes.len(),
        local_hashes.len(),
        ptr_lookup_count,
        ptr_overlay_hits.len(),
        missing.len(),
    );

    Ok(EntityMetadataBatchResponse {
        items,
        missing,
        generated_at: Utc::now().to_rfc3339(),
    })
}
