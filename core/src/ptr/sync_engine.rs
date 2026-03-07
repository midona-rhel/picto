//! PTR sync orchestrator — downloads and processes PTR updates.
//!
//! Three-phase sync:
//! 1. Fetch metadata to learn what updates exist
//! 2. Download missing update files
//! 3. Process updates in order (definitions first, then content)

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::events;
use crate::ptr::client::PtrClient;
use crate::ptr::types::*;
use crate::ptr::db::PtrSqliteDatabase;

/// Number of concurrent HTTP downloads.
const DOWNLOAD_CONCURRENCY: usize = 20;
/// Target max hashes per chunk. Chunks are split by hash count, not index count,
/// so initial sync indices (with huge definition dumps) get smaller chunks.
const CHUNK_MAX_HASHES: usize = 120;
/// Max row-pairs per content DB transaction when writing a PTR chunk.
const CONTENT_WRITE_BATCH_ROWS_MAPPING_ADD: usize = 500_000;
const CONTENT_WRITE_BATCH_ROWS_MAPPING_DEL: usize = 5_000;
const CONTENT_WRITE_BATCH_ROWS_RELATIONS: usize = 100_000;
/// Max unresolved hash/tag def_ids to resolve per DB transaction.
/// Keeps resolve phase incremental so progress/UI stay responsive.
const RESOLVE_HASH_DEF_IDS_BATCH: usize = 500_000;
const RESOLVE_TAG_DEF_IDS_BATCH: usize = 200_000;

/// Split indices into chunks where each chunk has at most `max_hashes` hashes.
/// Each index gets at least its own chunk (so an index with 1000 hashes when max=500
/// still becomes a single chunk of 1 index).
fn hash_count_chunks(indices: &[(u64, &PtrMetadataEntry)], max_hashes: usize) -> Vec<Vec<usize>> {
    let mut chunks: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_hashes = 0usize;

    for (i, (_, entry)) in indices.iter().enumerate() {
        let h = entry.hashes.len();
        if !current.is_empty() && current_hashes + h > max_hashes {
            chunks.push(std::mem::take(&mut current));
            current_hashes = 0;
        }
        current.push(i);
        current_hashes += h;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Heartbeat interval for progress events.
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
/// Minimum interval for non-heartbeat progress event emission while processing
/// large PTR chunks.
const PROGRESS_EMIT_MIN_INTERVAL: Duration = Duration::from_millis(200);
/// Cap for tracked changed hashes in a single sync run.
const MAX_TRACKED_CHANGED_HASHES: usize = 200_000;
/// Soft cap for per-sync in-memory def-id caches.
/// When exceeded, caches are cleared to bound memory usage.
const DEF_ID_CACHE_CAP: usize = 750_000;

#[derive(Debug, Clone, Serialize, Default)]
pub struct PtrSyncRunPerf {
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub updates_processed: u64,
    pub tags_added: u64,
    pub siblings_added: u64,
    pub parents_added: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PtrSyncChunkPerf {
    pub ts: String,
    pub index_start: u64,
    pub index_end: u64,
    pub defs_insert_ms: f64,
    pub resolve_ids_ms: f64,
    pub content_write_ms: f64,
    pub mapping_add_apply_ms: f64,
    pub sibling_add_apply_ms: f64,
    pub parent_add_apply_ms: f64,
    pub mapping_del_apply_ms: f64,
    pub sibling_del_apply_ms: f64,
    pub parent_del_apply_ms: f64,
    pub mapping_adds: usize,
    pub sibling_adds: usize,
    pub parent_adds: usize,
    pub mapping_dels: usize,
    pub sibling_dels: usize,
    pub parent_dels: usize,
    pub total_batches: u32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PtrSyncPerfBreakdown {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<PtrSyncRunPerf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_chunk: Option<PtrSyncChunkPerf>,
}

static PTR_SYNC_PERF: OnceLock<Mutex<PtrSyncPerfBreakdown>> = OnceLock::new();

fn ptr_sync_perf_store() -> &'static Mutex<PtrSyncPerfBreakdown> {
    PTR_SYNC_PERF.get_or_init(|| Mutex::new(PtrSyncPerfBreakdown::default()))
}

pub fn get_ptr_sync_perf_breakdown() -> PtrSyncPerfBreakdown {
    ptr_sync_perf_store()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

pub struct PtrSyncEngine {
    client: Arc<PtrClient>,
    ptr_db: Arc<PtrSqliteDatabase>,
}

impl PtrSyncEngine {
    pub fn new(client: PtrClient, ptr_db: Arc<PtrSqliteDatabase>) -> Self {
        Self {
            client: Arc::new(client),
            ptr_db,
        }
    }

    /// Run a full sync: metadata → download → process.
    ///
    /// Downloads use bounded concurrency (20 parallel HTTP requests) in chunks
    /// of 50 update indices. Within each chunk, all hashes are downloaded
    /// concurrently, then definitions and content are processed in index order.
    pub async fn sync(&self, cancel: CancellationToken) -> Result<PtrSyncProgress, String> {
        let sync_start = Instant::now();
        let starting_index = self.ptr_db.get_cursor().await? as u64;
        let mut progress = PtrSyncProgress {
            starting_index,
            current_update_index: starting_index,
            ..Default::default()
        };
        let mut last_phase_emitted = String::new();

        if let Ok(mut perf) = ptr_sync_perf_store().lock() {
            perf.latest_run = Some(PtrSyncRunPerf {
                started_at: Utc::now().to_rfc3339(),
                ..Default::default()
            });
        }

        // Shared progress for heartbeat task
        let shared_progress = Arc::new(std::sync::Mutex::new(progress.clone()));
        let heartbeat_cancel = cancel.clone();
        let heartbeat_progress = shared_progress.clone();
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
            interval.tick().await; // skip first immediate tick
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut snap = crate::poison::mutex_or_recover(&heartbeat_progress, "ptr_sync_heartbeat").clone();
                        snap.heartbeat = true;
                        crate::ptr::controller::PtrController::update_sync_progress(&snap);
                        events::emit(events::event_names::PTR_SYNC_PROGRESS, &snap);
                    }
                    _ = heartbeat_cancel.cancelled() => break,
                }
            }
        });

        // Phase 1: Fetch metadata
        progress.phase = "metadata".into();
        Self::emit_phase_changed(
            &mut last_phase_emitted,
            &progress.phase,
            progress.current_update_index,
        );
        progress.elapsed_ms = sync_start.elapsed().as_millis() as u64;
        self.emit_progress_shared(&mut progress, &shared_progress);
        info!(since = starting_index, "PTR sync: fetching metadata");
        let metadata = self.client.get_metadata(starting_index).await?;

        if metadata.updates.is_empty() {
            info!("PTR sync: no new updates available");
            return Ok(progress);
        }

        let latest_index = metadata
            .updates
            .keys()
            .last()
            .copied()
            .unwrap_or(starting_index);
        progress.latest_server_index = latest_index;

        // Count total update hashes to download
        let total_hashes: usize = metadata.updates.values().map(|e| e.hashes.len()).sum();
        progress.updates_total = total_hashes as u64;

        info!(
            new_indices = metadata.updates.len(),
            total_hashes, latest_index, "PTR sync: metadata received"
        );

        {
            progress.elapsed_ms = sync_start.elapsed().as_millis() as u64;
            self.emit_progress_shared(&mut progress, &shared_progress)
        };

        // Phase 2+3: Download and process in chunks for concurrent I/O.
        // Chunk by hash count (not index count) so initial indices with huge
        // definition dumps get smaller chunks.
        let indices: Vec<(u64, &PtrMetadataEntry)> =
            metadata.updates.iter().map(|(&k, v)| (k, v)).collect();

        let chunks = hash_count_chunks(&indices, CHUNK_MAX_HASHES);

        let semaphore = Arc::new(tokio::sync::Semaphore::new(DOWNLOAD_CONCURRENCY));

        // Disable fsync and auto-checkpoint for bulk writes — PTR data can always be re-downloaded.
        self.ptr_db.set_synchronous_off().await?;
        self.ptr_db.set_wal_autocheckpoint(0).await?;
        // PBI-074: Guard bulk mode entry — restore synchronous + checkpoint on failure.
        if let Err(e) = self.ptr_db.enter_bulk_content_mode().await {
            let _ = self.ptr_db.set_synchronous_normal().await;
            let _ = self.ptr_db.set_wal_autocheckpoint(1000).await;
            return Err(e);
        }

        let run_result: Result<(), String> = async {
            let mut last_progress_emit = Instant::now() - PROGRESS_EMIT_MIN_INTERVAL;
            let mut hash_def_id_cache: HashMap<i64, i64> = HashMap::new();
            let mut tag_def_id_cache: HashMap<i64, i64> = HashMap::new();
            for chunk in &chunks {
                let chunk: Vec<(u64, &PtrMetadataEntry)> =
                    chunk.iter().map(|&i| indices[i]).collect();
                if cancel.is_cancelled() {
                    info!("PTR sync: cancelled");
                    break;
                }

                // ── Download phase: fetch all hashes in this chunk concurrently ──
                let chunk_hashes: usize = chunk.iter().map(|(_, e)| e.hashes.len()).sum();
                let first_idx = chunk.first().map(|(i, _)| *i).unwrap_or(0);
                let last_idx = chunk.last().map(|(i, _)| *i).unwrap_or(0);
                info!(
                    indices = chunk.len(),
                    hashes = chunk_hashes,
                    range = %format!("{first_idx}..{last_idx}"),
                    "PTR sync: downloading chunk"
                );
                progress.phase = "downloading".into();
                Self::emit_phase_changed(
                    &mut last_phase_emitted,
                    &progress.phase,
                    progress.current_update_index,
                );
                self.emit_progress_shared_throttled(
                    &mut progress,
                    &shared_progress,
                    &sync_start,
                    &mut last_progress_emit,
                    true,
                );

                let mut join_set: JoinSet<(u64, Result<PtrUpdate, String>)> = JoinSet::new();

                for &(update_index, entry) in &chunk {
                    for hash in &entry.hashes {
                        let client = self.client.clone();
                        let sem = semaphore.clone();
                        let hash = hash.clone();
                        let cancel = cancel.clone();
                        join_set.spawn(async move {
                            tokio::select! {
                                result = async {
                                    let _permit = sem.acquire().await.unwrap();
                                    client.get_update(&hash).await
                                } => (update_index, result),
                                _ = cancel.cancelled() => (update_index, Err("Cancelled".into())),
                            }
                        });
                    }
                }

                // Collect results grouped by index
                let mut by_index: BTreeMap<u64, (Vec<DefinitionsUpdate>, Vec<ContentUpdate>)> =
                    BTreeMap::new();
                let mut download_errors = 0u32;

                while let Some(result) = join_set.join_next().await {
                    if cancel.is_cancelled() {
                        break;
                    }
                    match result {
                        Ok((idx, Ok(PtrUpdate::Definitions(d)))) => {
                            by_index.entry(idx).or_default().0.push(d);
                        }
                        Ok((idx, Ok(PtrUpdate::Content(c)))) => {
                            by_index.entry(idx).or_default().1.push(c);
                        }
                        Ok((_, Err(e))) if e == "Cancelled" => {}
                        Ok((_, Err(e))) => {
                            download_errors += 1;
                            warn!(error = %e, "Failed to download update, skipping");
                        }
                        Err(e) => {
                            download_errors += 1;
                            warn!(error = %e, "Download task panicked");
                        }
                    }
                    progress.updates_processed += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                }

                info!(
                    ok = by_index
                        .values()
                        .map(|(d, c)| d.len() + c.len())
                        .sum::<usize>(),
                    errors = download_errors,
                    "PTR sync: downloads complete for chunk"
                );

                if cancel.is_cancelled() {
                    break;
                }

                // PBI-079: Stop sync on download failures to prevent cursor
                // advancing past unapplied updates. Advance cursor only to
                // the highest contiguous index where all downloads succeeded.
                if download_errors > 0 {
                    let mut safe_index: Option<u64> = None;
                    for &(idx, _) in &chunk {
                        if by_index.contains_key(&idx) {
                            safe_index = Some(idx);
                        } else {
                            break;
                        }
                    }
                    if let Some(idx) = safe_index {
                        self.ptr_db.set_cursor(idx as i64).await?;
                        progress.current_update_index = idx;
                    }
                    return Err(format!(
                        "PTR sync stopped: {} download failure(s) in chunk {}-{}. \
                         Cursor saved at safe index. Re-run sync to retry.",
                        download_errors, first_idx, last_idx
                    ));
                }

                // ── Processing phase: batch across all indices in this chunk ──
                info!("PTR sync: processing chunk");
                progress.phase = "processing".into();
                Self::emit_phase_changed(
                    &mut last_phase_emitted,
                    &progress.phase,
                    progress.current_update_index,
                );
                self.emit_progress_shared_throttled(
                    &mut progress,
                    &shared_progress,
                    &sync_start,
                    &mut last_progress_emit,
                    true,
                );
                let mut defs_insert_ms = 0.0_f64;

                // 1. Accumulate and deduplicate definitions, then batch-insert
                //    with progress reporting between batches
                {
                    let mut hash_dedup: HashMap<i64, String> = HashMap::new();
                    let mut tag_dedup: HashMap<i64, String> = HashMap::new();
                    for &(update_index, _) in &chunk {
                        if let Some((ref defs, _)) = by_index.get(&update_index) {
                            for def in defs.iter() {
                                for (id, h) in &def.hash_ids_to_hashes {
                                    hash_dedup.insert(*id as i64, h.clone());
                                }
                                for (id, t) in &def.tag_ids_to_tags {
                                    tag_dedup.insert(*id as i64, t.clone());
                                }
                            }
                        }
                    }
                    let all_hash_defs: Vec<(i64, String)> = hash_dedup.into_iter().collect();
                    let all_tag_defs: Vec<(i64, String)> = tag_dedup.into_iter().collect();
                    let total_defs = all_hash_defs.len() + all_tag_defs.len();

                    if total_defs > 0 {
                        info!(
                            hash_defs = all_hash_defs.len(),
                            tag_defs = all_tag_defs.len(),
                            "PTR sync: inserting definitions"
                        );
                        progress.phase = "definitions".into();
                        Self::emit_phase_changed(
                            &mut last_phase_emitted,
                            &progress.phase,
                            progress.current_update_index,
                        );
                        self.emit_progress_shared_throttled(
                            &mut progress,
                            &shared_progress,
                            &sync_start,
                            &mut last_progress_emit,
                            true,
                        );

                        // Single transaction for all defs in this chunk
                        let defs_insert_started = Instant::now();
                        self.ptr_db
                            .insert_all_defs(all_hash_defs, all_tag_defs)
                            .await?;
                        defs_insert_ms = defs_insert_started.elapsed().as_secs_f64() * 1000.0;
                        info!(total_defs, "PTR sync: all definitions committed");
                    }
                }

                // 2. Resolve content def IDs directly to internal integer IDs.
                //    Missing rows are backfilled into persistent mapping caches.
                let mut all_tag_def_ids: Vec<i64> = Vec::new();
                let mut all_hash_def_ids: Vec<i64> = Vec::new();
                for (_, (_, contents)) in by_index.iter() {
                    for content in contents {
                        for (tid, _) in content.mappings_add.iter().chain(&content.mappings_delete)
                        {
                            all_tag_def_ids.push(*tid as i64);
                        }
                        for (_, hids) in content.mappings_add.iter().chain(&content.mappings_delete)
                        {
                            all_hash_def_ids.extend(hids.iter().map(|h| *h as i64));
                        }
                        for (a, b) in content.siblings_add.iter().chain(&content.siblings_delete) {
                            all_tag_def_ids.push(*a as i64);
                            all_tag_def_ids.push(*b as i64);
                        }
                        for (a, b) in content.parents_add.iter().chain(&content.parents_delete) {
                            all_tag_def_ids.push(*a as i64);
                            all_tag_def_ids.push(*b as i64);
                        }
                    }
                }
                all_tag_def_ids.sort_unstable();
                all_tag_def_ids.dedup();
                all_hash_def_ids.sort_unstable();
                all_hash_def_ids.dedup();

                info!(
                    unique_tag_ids = all_tag_def_ids.len(),
                    unique_hash_ids = all_hash_def_ids.len(),
                    "PTR sync: resolving def IDs"
                );

                progress.phase = "resolving_ids".into();
                Self::emit_phase_changed(
                    &mut last_phase_emitted,
                    &progress.phase,
                    progress.current_update_index,
                );
                self.emit_progress_shared_throttled(
                    &mut progress,
                    &shared_progress,
                    &sync_start,
                    &mut last_progress_emit,
                    true,
                );

                let mut hash_def_id_map: HashMap<i64, i64> =
                    HashMap::with_capacity(all_hash_def_ids.len());
                let mut tag_def_id_map: HashMap<i64, i64> =
                    HashMap::with_capacity(all_tag_def_ids.len());

                let mut unresolved_hash_def_ids: Vec<i64> = Vec::new();
                let mut unresolved_tag_def_ids: Vec<i64> = Vec::new();

                for &def_id in &all_hash_def_ids {
                    if let Some(&stub_id) = hash_def_id_cache.get(&def_id) {
                        hash_def_id_map.insert(def_id, stub_id);
                    } else {
                        unresolved_hash_def_ids.push(def_id);
                    }
                }
                for &def_id in &all_tag_def_ids {
                    if let Some(&tag_id) = tag_def_id_cache.get(&def_id) {
                        tag_def_id_map.insert(def_id, tag_id);
                    } else {
                        unresolved_tag_def_ids.push(def_id);
                    }
                }
                let hash_cache_hits = all_hash_def_ids
                    .len()
                    .saturating_sub(unresolved_hash_def_ids.len());
                let tag_cache_hits = all_tag_def_ids
                    .len()
                    .saturating_sub(unresolved_tag_def_ids.len());

                let resolve_started = Instant::now();
                if !unresolved_hash_def_ids.is_empty() {
                    for def_chunk in unresolved_hash_def_ids.chunks(RESOLVE_HASH_DEF_IDS_BATCH) {
                        let (fresh_hash_map, _) = self
                            .ptr_db
                            .resolve_or_create_def_mappings(def_chunk.to_vec(), Vec::new())
                            .await?;
                        for (def_id, stub_id) in fresh_hash_map {
                            hash_def_id_map.insert(def_id, stub_id);
                            if hash_def_id_cache.len() >= DEF_ID_CACHE_CAP {
                                hash_def_id_cache.clear();
                            }
                            hash_def_id_cache.insert(def_id, stub_id);
                        }
                        self.emit_progress_shared_throttled(
                            &mut progress,
                            &shared_progress,
                            &sync_start,
                            &mut last_progress_emit,
                            false,
                        );
                    }
                }
                if !unresolved_tag_def_ids.is_empty() {
                    for def_chunk in unresolved_tag_def_ids.chunks(RESOLVE_TAG_DEF_IDS_BATCH) {
                        let (_, fresh_tag_map) = self
                            .ptr_db
                            .resolve_or_create_def_mappings(Vec::new(), def_chunk.to_vec())
                            .await?;
                        for (def_id, tag_id) in fresh_tag_map {
                            tag_def_id_map.insert(def_id, tag_id);
                            if tag_def_id_cache.len() >= DEF_ID_CACHE_CAP {
                                tag_def_id_cache.clear();
                            }
                            tag_def_id_cache.insert(def_id, tag_id);
                        }
                        self.emit_progress_shared_throttled(
                            &mut progress,
                            &shared_progress,
                            &sync_start,
                            &mut last_progress_emit,
                            false,
                        );
                    }
                }
                info!(
                    hash_cache_hits,
                    tag_cache_hits,
                    tags_resolved = tag_def_id_map.len(),
                    hashes_resolved = hash_def_id_map.len(),
                    "PTR sync: def IDs resolved"
                );
                let resolve_ids_ms = resolve_started.elapsed().as_secs_f64() * 1000.0;

                if !progress.changed_hashes_truncated {
                    let remaining =
                        MAX_TRACKED_CHANGED_HASHES.saturating_sub(progress.changed_hashes.len());
                    if remaining > 0 {
                        let sampled_ids: Vec<i64> =
                            all_hash_def_ids.iter().take(remaining).copied().collect();
                        let sampled_hashes =
                            Self::resolve_hash_defs(&self.ptr_db, &sampled_ids).await?;
                        progress.changed_hashes.extend(sampled_hashes.into_values());
                    }
                    if all_hash_def_ids.len() > remaining {
                        progress.changed_hashes_truncated = true;
                    }
                }

                progress.phase = "ensuring_ids".into();
                Self::emit_phase_changed(
                    &mut last_phase_emitted,
                    &progress.phase,
                    progress.current_update_index,
                );
                self.emit_progress_shared_throttled(
                    &mut progress,
                    &shared_progress,
                    &sync_start,
                    &mut last_progress_emit,
                    true,
                );
                info!(
                    stubs = hash_def_id_map.len(),
                    tags = tag_def_id_map.len(),
                    "PTR sync: integer ID maps built"
                );

                // 3. Accumulate ALL content across the chunk into bulk vectors,
                //    then execute a bounded number of DB calls.
                progress.phase = "processing".into();
                Self::emit_phase_changed(
                    &mut last_phase_emitted,
                    &progress.phase,
                    progress.current_update_index,
                );
                self.emit_progress_shared_throttled(
                    &mut progress,
                    &shared_progress,
                    &sync_start,
                    &mut last_progress_emit,
                    true,
                );

                let mut mappings_add_pairs: Vec<(i64, i64)> = Vec::new();
                let mut siblings_add_pairs: Vec<(i64, i64)> = Vec::new();
                let mut parents_add_pairs: Vec<(i64, i64)> = Vec::new();
                let mut mappings_del_pairs: Vec<(i64, i64)> = Vec::new();
                let mut siblings_del_pairs: Vec<(i64, i64)> = Vec::new();
                let mut parents_del_pairs: Vec<(i64, i64)> = Vec::new();

                for &(update_index, _) in &chunk {
                    if cancel.is_cancelled() {
                        break;
                    }
                    if let Some((_, ref contents)) = by_index.get(&update_index) {
                        for content in contents.iter() {
                            // Mapping adds
                            for (tag_id, hash_ids) in &content.mappings_add {
                                let Some(&tid) = tag_def_id_map.get(&(*tag_id as i64)) else {
                                    continue;
                                };
                                for hid in hash_ids {
                                    let Some(&stub_id) = hash_def_id_map.get(&(*hid as i64)) else {
                                        continue;
                                    };
                                    mappings_add_pairs.push((stub_id, tid));
                                }
                            }
                            // Sibling adds
                            for (a, b) in &content.siblings_add {
                                if let (Some(&ai), Some(&bi)) = (
                                    tag_def_id_map.get(&(*a as i64)),
                                    tag_def_id_map.get(&(*b as i64)),
                                ) {
                                    siblings_add_pairs.push((ai, bi));
                                }
                            }
                            // Parent adds
                            for (a, b) in &content.parents_add {
                                if let (Some(&ai), Some(&bi)) = (
                                    tag_def_id_map.get(&(*a as i64)),
                                    tag_def_id_map.get(&(*b as i64)),
                                ) {
                                    parents_add_pairs.push((ai, bi));
                                }
                            }
                            // Mapping deletes
                            for (tag_id, hash_ids) in &content.mappings_delete {
                                let Some(&tid) = tag_def_id_map.get(&(*tag_id as i64)) else {
                                    continue;
                                };
                                for hid in hash_ids {
                                    let Some(&stub_id) = hash_def_id_map.get(&(*hid as i64)) else {
                                        continue;
                                    };
                                    mappings_del_pairs.push((stub_id, tid));
                                }
                            }
                            // Sibling deletes
                            for (a, b) in &content.siblings_delete {
                                if let (Some(&ai), Some(&bi)) = (
                                    tag_def_id_map.get(&(*a as i64)),
                                    tag_def_id_map.get(&(*b as i64)),
                                ) {
                                    siblings_del_pairs.push((ai, bi));
                                }
                            }
                            // Parent deletes
                            for (a, b) in &content.parents_delete {
                                if let (Some(&ai), Some(&bi)) = (
                                    tag_def_id_map.get(&(*a as i64)),
                                    tag_def_id_map.get(&(*b as i64)),
                                ) {
                                    parents_del_pairs.push((ai, bi));
                                }
                            }
                        }
                    }
                }

                if cancel.is_cancelled() {
                    info!("PTR sync: cancellation observed before content dedup/apply");
                    break;
                }

                // Remove duplicate operations before hitting SQLite. PTR chunks can
                // contain repeated mappings/relations across updates.
                mappings_add_pairs.sort_unstable();
                mappings_add_pairs.dedup();
                siblings_add_pairs.sort_unstable();
                siblings_add_pairs.dedup();
                parents_add_pairs.sort_unstable();
                parents_add_pairs.dedup();
                mappings_del_pairs.sort_unstable();
                mappings_del_pairs.dedup();
                siblings_del_pairs.sort_unstable();
                siblings_del_pairs.dedup();
                parents_del_pairs.sort_unstable();
                parents_del_pairs.dedup();

                let total_content_rows = (mappings_add_pairs.len()
                    + siblings_add_pairs.len()
                    + parents_add_pairs.len()
                    + mappings_del_pairs.len()
                    + siblings_del_pairs.len()
                    + parents_del_pairs.len()) as u64;
                let content_batches_total = |rows: usize, batch_size: usize| -> u32 {
                    if rows == 0 {
                        0
                    } else {
                        rows.div_ceil(batch_size) as u32
                    }
                };
                let total_batches = content_batches_total(
                    mappings_add_pairs.len(),
                    CONTENT_WRITE_BATCH_ROWS_MAPPING_ADD,
                ) + content_batches_total(
                    siblings_add_pairs.len(),
                    CONTENT_WRITE_BATCH_ROWS_RELATIONS,
                ) + content_batches_total(
                    parents_add_pairs.len(),
                    CONTENT_WRITE_BATCH_ROWS_RELATIONS,
                ) + content_batches_total(
                    mappings_del_pairs.len(),
                    CONTENT_WRITE_BATCH_ROWS_MAPPING_DEL,
                ) + content_batches_total(
                    siblings_del_pairs.len(),
                    CONTENT_WRITE_BATCH_ROWS_RELATIONS,
                ) + content_batches_total(
                    parents_del_pairs.len(),
                    CONTENT_WRITE_BATCH_ROWS_RELATIONS,
                );

                // Track sibling/parent changes for global overlay invalidation (PBI-028).
                if !siblings_add_pairs.is_empty()
                    || !parents_add_pairs.is_empty()
                    || !siblings_del_pairs.is_empty()
                    || !parents_del_pairs.is_empty()
                {
                    progress.tag_graph_changed = true;
                }

                // Incremental content commits for responsiveness: large PTR chunks
                // are split into multiple DB transactions with progress updates.
                info!(
                    mapping_adds = mappings_add_pairs.len(),
                    sibling_adds = siblings_add_pairs.len(),
                    parent_adds = parents_add_pairs.len(),
                    mapping_dels = mappings_del_pairs.len(),
                    sibling_dels = siblings_del_pairs.len(),
                    parent_dels = parents_del_pairs.len(),
                    total_rows = total_content_rows,
                    total_batches = total_batches,
                    "PTR sync: writing chunk content"
                );
                progress.phase = "writing_content".into();
                Self::emit_phase_changed(
                    &mut last_phase_emitted,
                    &progress.phase,
                    progress.current_update_index,
                );
                progress.content_rows_total = total_content_rows;
                progress.content_rows_written = 0;
                progress.content_batches_total = total_batches;
                progress.content_batches_done = 0;
                let content_write_started = Instant::now();
                let mut mapping_add_apply_ms = 0.0_f64;
                let mut sibling_add_apply_ms = 0.0_f64;
                let mut parent_add_apply_ms = 0.0_f64;
                let mut mapping_del_apply_ms = 0.0_f64;
                let mut sibling_del_apply_ms = 0.0_f64;
                let mut parent_del_apply_ms = 0.0_f64;
                self.emit_progress_shared_throttled(
                    &mut progress,
                    &shared_progress,
                    &sync_start,
                    &mut last_progress_emit,
                    true,
                );
                let mut chunk_cancelled = false;

                for batch in mappings_add_pairs.chunks(CONTENT_WRITE_BATCH_ROWS_MAPPING_ADD) {
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                    let apply_started = Instant::now();
                    let counts = self
                        .ptr_db
                        .process_chunk_content(crate::ptr::db::sync::ChunkContent {
                            mapping_adds: batch.to_vec(),
                            sibling_adds: Vec::new(),
                            parent_adds: Vec::new(),
                            mapping_dels: Vec::new(),
                            sibling_dels: Vec::new(),
                            parent_dels: Vec::new(),
                        })
                        .await?;
                    mapping_add_apply_ms += apply_started.elapsed().as_secs_f64() * 1000.0;
                    progress.tags_added += counts.tags_added as u64;
                    progress.content_rows_written += batch.len() as u64;
                    progress.content_batches_done += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                }
                if chunk_cancelled {
                    info!("PTR sync: cancellation observed during mapping_add apply");
                    break;
                }
                for batch in siblings_add_pairs.chunks(CONTENT_WRITE_BATCH_ROWS_RELATIONS) {
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                    let apply_started = Instant::now();
                    let counts = self
                        .ptr_db
                        .process_chunk_content(crate::ptr::db::sync::ChunkContent {
                            mapping_adds: Vec::new(),
                            sibling_adds: batch.to_vec(),
                            parent_adds: Vec::new(),
                            mapping_dels: Vec::new(),
                            sibling_dels: Vec::new(),
                            parent_dels: Vec::new(),
                        })
                        .await?;
                    sibling_add_apply_ms += apply_started.elapsed().as_secs_f64() * 1000.0;
                    progress.siblings_added += counts.siblings_added as u64;
                    progress.content_rows_written += batch.len() as u64;
                    progress.content_batches_done += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                }
                if chunk_cancelled {
                    info!("PTR sync: cancellation observed during sibling_add apply");
                    break;
                }
                for batch in parents_add_pairs.chunks(CONTENT_WRITE_BATCH_ROWS_RELATIONS) {
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                    let apply_started = Instant::now();
                    let counts = self
                        .ptr_db
                        .process_chunk_content(crate::ptr::db::sync::ChunkContent {
                            mapping_adds: Vec::new(),
                            sibling_adds: Vec::new(),
                            parent_adds: batch.to_vec(),
                            mapping_dels: Vec::new(),
                            sibling_dels: Vec::new(),
                            parent_dels: Vec::new(),
                        })
                        .await?;
                    parent_add_apply_ms += apply_started.elapsed().as_secs_f64() * 1000.0;
                    progress.parents_added += counts.parents_added as u64;
                    progress.content_rows_written += batch.len() as u64;
                    progress.content_batches_done += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                }
                if chunk_cancelled {
                    info!("PTR sync: cancellation observed during parent_add apply");
                    break;
                }
                for batch in mappings_del_pairs.chunks(CONTENT_WRITE_BATCH_ROWS_MAPPING_DEL) {
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                    let apply_started = Instant::now();
                    self.ptr_db
                        .process_chunk_content(crate::ptr::db::sync::ChunkContent {
                            mapping_adds: Vec::new(),
                            sibling_adds: Vec::new(),
                            parent_adds: Vec::new(),
                            mapping_dels: batch.to_vec(),
                            sibling_dels: Vec::new(),
                            parent_dels: Vec::new(),
                        })
                        .await?;
                    mapping_del_apply_ms += apply_started.elapsed().as_secs_f64() * 1000.0;
                    progress.content_rows_written += batch.len() as u64;
                    progress.content_batches_done += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                }
                if chunk_cancelled {
                    info!("PTR sync: cancellation observed during mapping_del apply");
                    break;
                }
                for batch in siblings_del_pairs.chunks(CONTENT_WRITE_BATCH_ROWS_RELATIONS) {
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                    let apply_started = Instant::now();
                    self.ptr_db
                        .process_chunk_content(crate::ptr::db::sync::ChunkContent {
                            mapping_adds: Vec::new(),
                            sibling_adds: Vec::new(),
                            parent_adds: Vec::new(),
                            mapping_dels: Vec::new(),
                            sibling_dels: batch.to_vec(),
                            parent_dels: Vec::new(),
                        })
                        .await?;
                    sibling_del_apply_ms += apply_started.elapsed().as_secs_f64() * 1000.0;
                    progress.content_rows_written += batch.len() as u64;
                    progress.content_batches_done += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                }
                if chunk_cancelled {
                    info!("PTR sync: cancellation observed during sibling_del apply");
                    break;
                }
                for batch in parents_del_pairs.chunks(CONTENT_WRITE_BATCH_ROWS_RELATIONS) {
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                    let apply_started = Instant::now();
                    self.ptr_db
                        .process_chunk_content(crate::ptr::db::sync::ChunkContent {
                            mapping_adds: Vec::new(),
                            sibling_adds: Vec::new(),
                            parent_adds: Vec::new(),
                            mapping_dels: Vec::new(),
                            sibling_dels: Vec::new(),
                            parent_dels: batch.to_vec(),
                        })
                        .await?;
                    parent_del_apply_ms += apply_started.elapsed().as_secs_f64() * 1000.0;
                    progress.content_rows_written += batch.len() as u64;
                    progress.content_batches_done += 1;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        false,
                    );
                    if cancel.is_cancelled() {
                        chunk_cancelled = true;
                        break;
                    }
                }
                if chunk_cancelled {
                    info!("PTR sync: cancellation observed during parent_del apply");
                    break;
                }
                let content_write_ms = content_write_started.elapsed().as_secs_f64() * 1000.0;
                if let Ok(mut perf) = ptr_sync_perf_store().lock() {
                    perf.latest_chunk = Some(PtrSyncChunkPerf {
                        ts: Utc::now().to_rfc3339(),
                        index_start: first_idx,
                        index_end: last_idx,
                        defs_insert_ms,
                        resolve_ids_ms,
                        content_write_ms,
                        mapping_add_apply_ms,
                        sibling_add_apply_ms,
                        parent_add_apply_ms,
                        mapping_del_apply_ms,
                        sibling_del_apply_ms,
                        parent_del_apply_ms,
                        mapping_adds: mappings_add_pairs.len(),
                        sibling_adds: siblings_add_pairs.len(),
                        parent_adds: parents_add_pairs.len(),
                        mapping_dels: mappings_del_pairs.len(),
                        sibling_dels: siblings_del_pairs.len(),
                        parent_dels: parents_del_pairs.len(),
                        total_batches: progress.content_batches_total,
                    });
                }
                info!(
                    content_write_ms = %format!("{content_write_ms:.2}"),
                    mapping_add_apply_ms = %format!("{mapping_add_apply_ms:.2}"),
                    sibling_add_apply_ms = %format!("{sibling_add_apply_ms:.2}"),
                    parent_add_apply_ms = %format!("{parent_add_apply_ms:.2}"),
                    mapping_del_apply_ms = %format!("{mapping_del_apply_ms:.2}"),
                    sibling_del_apply_ms = %format!("{sibling_del_apply_ms:.2}"),
                    parent_del_apply_ms = %format!("{parent_del_apply_ms:.2}"),
                    total_batches = progress.content_batches_total,
                    "PTR sync: content write timing"
                );

                info!(
                    tags = progress.tags_added,
                    siblings = progress.siblings_added,
                    parents = progress.parents_added,
                    "PTR sync: content processing done for chunk"
                );
                if let Some(&(last_index, _)) = chunk.last() {
                    self.ptr_db.set_cursor(last_index as i64).await?;
                    progress.current_update_index = last_index;
                    self.emit_progress_shared_throttled(
                        &mut progress,
                        &shared_progress,
                        &sync_start,
                        &mut last_progress_emit,
                        true,
                    );
                }
            }
            Ok(())
        }
        .await;

        // Always restore DB write/read shape even if sync fails or is cancelled.
        if cancel.is_cancelled() {
            if let Err(e) = run_result {
                warn!(
                    error = %e,
                    "PTR sync cancelled with run error; deferring heavy cleanup to startup maintenance"
                );
            }
            // Fast-cancel path: do not block on index rebuild/checkpoint.
            // We keep marker bulk_index_rebuild_required=1 and let startup
            // maintenance restore write-path indexes.
            if let Err(e) = self.ptr_db.set_synchronous_normal().await {
                warn!(error = %e, "PTR sync cancel: failed to restore synchronous mode");
            }
            let _ = self.ptr_db.set_wal_autocheckpoint(1000).await;
            heartbeat_handle.abort();
            progress.phase = "cancelled".into();
            progress.elapsed_ms = sync_start.elapsed().as_millis() as u64;
            info!(
                elapsed_ms = progress.elapsed_ms,
                "PTR sync: cancel acknowledged with deferred cleanup"
            );
            return Ok(progress);
        }

        if let Err(e) = self.ptr_db.exit_bulk_content_mode().await {
            warn!(error = %e, "PTR sync: failed to rebuild bulk-mode indexes");
        }

        // Restore normal sync mode, re-enable auto-checkpoint, and flush WAL
        self.ptr_db.set_synchronous_normal().await?;
        self.ptr_db.set_wal_autocheckpoint(1000).await?;
        self.ptr_db.checkpoint_passive().await?;
        run_result?;

        // Stop heartbeat
        heartbeat_handle.abort();

        info!(
            processed = progress.updates_processed,
            tags = progress.tags_added,
            siblings = progress.siblings_added,
            parents = progress.parents_added,
            elapsed_ms = sync_start.elapsed().as_millis() as u64,
            "PTR sync: complete"
        );

        if let Ok(mut perf) = ptr_sync_perf_store().lock() {
            if let Some(run) = perf.latest_run.as_mut() {
                run.finished_at = Some(Utc::now().to_rfc3339());
                run.elapsed_ms = Some(sync_start.elapsed().as_millis() as u64);
                run.updates_processed = progress.updates_processed;
                run.tags_added = progress.tags_added;
                run.siblings_added = progress.siblings_added;
                run.parents_added = progress.parents_added;
            }
        }

        Ok(progress)
    }

    /// Chunk IN clauses for hash-def lookups.
    const CHUNK_SIZE: usize = 4096;

    /// Resolve def_ids → hash hex strings from ptr_hash_def table.
    async fn resolve_hash_defs(
        ptr_db: &Arc<PtrSqliteDatabase>,
        def_ids: &[i64],
    ) -> Result<HashMap<i64, String>, String> {
        if def_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let ids = def_ids.to_vec();
        ptr_db
            .with_read_conn(move |conn| {
                let mut map = HashMap::new();
                for chunk in ids.chunks(Self::CHUNK_SIZE) {
                    let placeholders = std::iter::repeat_n("?", chunk.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "SELECT def_id, hash_hex FROM ptr_hash_def WHERE def_id IN ({})",
                        placeholders
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?;
                    for row in rows {
                        let (id, hash) = row?;
                        map.insert(id, hash);
                    }
                }
                Ok(map)
            })
            .await
    }

    fn emit_progress_shared(
        &self,
        progress: &mut PtrSyncProgress,
        shared: &Arc<std::sync::Mutex<PtrSyncProgress>>,
    ) {
        progress.heartbeat = false;
        *crate::poison::mutex_or_recover(shared, "ptr_sync_progress") = progress.clone();
        crate::ptr::controller::PtrController::update_sync_progress(progress);
        events::emit(events::event_names::PTR_SYNC_PROGRESS, progress);
    }

    fn emit_progress_shared_throttled(
        &self,
        progress: &mut PtrSyncProgress,
        shared: &Arc<std::sync::Mutex<PtrSyncProgress>>,
        sync_start: &Instant,
        last_emit: &mut Instant,
        force: bool,
    ) {
        if force || last_emit.elapsed() >= PROGRESS_EMIT_MIN_INTERVAL {
            progress.elapsed_ms = sync_start.elapsed().as_millis() as u64;
            self.emit_progress_shared(progress, shared);
            *last_emit = Instant::now();
        }
    }

    fn emit_phase_changed(last_phase: &mut String, phase: &str, current_update_index: u64) {
        if last_phase == phase {
            return;
        }
        *last_phase = phase.to_string();
        events::emit(
            events::event_names::PTR_SYNC_PHASE_CHANGED,
            &events::PtrSyncPhaseChangedEvent {
                phase: phase.to_string(),
                current_update_index: Some(current_update_index),
                ts: Some(Utc::now().to_rfc3339()),
            },
        );
    }
}
