//! Perceptual duplicate discovery for the canonical library backend.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use chrono::Utc;
use fast_image_resize as fr;
use img_hash::ImageHash;
use palette::{IntoColor, Lab, Srgb};
use rusqlite::{params, Connection, Transaction};

use crate::media_processing::{PreparedMediaSource, DEFAULT_THUMBNAIL_DIMENSIONS};
const HASH_COMPONENT_BYTES: usize = 32;
const SUPPORTED_PHASH_BYTES: usize = HASH_COMPONENT_BYTES * 2;
pub(crate) const DEFAULT_GLOBAL_DISTANCE_THRESHOLD: u32 = 16;
const DETAIL_DISTANCE_THRESHOLD: u32 = 48;
const MAX_NEAR_SIGNATURE_GROUPS: usize = 512;
const MAX_SIGNATURE_REPRESENTATIVES: usize = 32;
const MAX_SPATIAL_COMPARISONS: usize = 1_000_000;
const COMPARISON_SIDE: u32 = 96;
const DISTINCT_COLOR_DELTA: f32 = 8.0;
const STRONG_COLOR_DELTA: f32 = 16.0;
const MAX_ALIGNMENT_SHIFT: isize = 3;
const ALIGNMENT_SAMPLE_STEP: usize = 3;
const DIFFERENCE_TILE_SIDE: usize = 8;
const SPATIAL_CACHE_VERSION: i64 = 1;

fn parse_supported_hash(raw: &str) -> Option<ImageHash<Vec<u8>>> {
    let hash = ImageHash::<Vec<u8>>::from_base64(raw).ok()?;
    (hash.as_bytes().len() == SUPPORTED_PHASH_BYTES).then_some(hash)
}

struct CandidateIndex {
    threshold: u32,
    buckets: HashMap<(usize, usize, u64), Vec<usize>>,
    entry_count: usize,
}

impl CandidateIndex {
    fn new(threshold: u32) -> Self {
        Self {
            threshold,
            buckets: HashMap::new(),
            entry_count: 0,
        }
    }

    fn insert(&mut self, entry_index: usize, hash: &ImageHash<Vec<u8>>) {
        debug_assert_eq!(entry_index, self.entry_count);
        self.entry_count += 1;

        for (partition, key) in partition_keys(hash.as_bytes(), self.threshold) {
            self.buckets
                .entry((HASH_COMPONENT_BYTES * 8, partition, key))
                .or_default()
                .push(entry_index);
        }
    }

    fn find_within(
        &self,
        parsed: &[(i64, ImageHash<Vec<u8>>)],
        hash: &ImageHash<Vec<u8>>,
    ) -> Vec<(i64, u32)> {
        if self.entry_count == 0 {
            return Vec::new();
        }

        let mut candidate_indices = Vec::new();
        if self.threshold < (HASH_COMPONENT_BYTES * 8) as u32 {
            for (partition, key) in partition_keys(hash.as_bytes(), self.threshold) {
                if let Some(entries) = self
                    .buckets
                    .get(&(HASH_COMPONENT_BYTES * 8, partition, key))
                {
                    candidate_indices.extend(entries);
                }
            }
        } else {
            candidate_indices.extend(0..self.entry_count);
        }
        candidate_indices.sort_unstable();
        candidate_indices.dedup();

        candidate_indices
            .into_iter()
            .filter_map(|entry_index| {
                let (file_id, candidate_hash) = &parsed[entry_index];
                let distance = candidate_hash.dist(hash);
                (distance <= self.threshold).then_some((*file_id, distance))
            })
            .collect()
    }
}

fn partition_keys(bytes: &[u8], threshold: u32) -> Vec<(usize, u64)> {
    let bit_len = bytes.len() * 8;
    let minimum_partitions = bit_len.div_ceil(64);
    let required_partitions = threshold.saturating_add(1) as usize;
    let partition_count = minimum_partitions.max(required_partitions).min(bit_len);
    let base_len = bit_len / partition_count;
    let remainder = bit_len % partition_count;
    let mut start = 0;
    let mut keys = Vec::with_capacity(partition_count);
    for partition in 0..partition_count {
        let length = base_len + usize::from(partition < remainder);
        keys.push((partition, bits_as_key(bytes, start, length)));
        start += length;
    }
    keys
}

fn bits_as_key(bytes: &[u8], start: usize, length: usize) -> u64 {
    debug_assert!(length <= 64);
    let mut key = 0_u64;
    for offset in 0..length {
        let bit = start + offset;
        key = (key << 1) | u64::from((bytes[bit / 8] >> (7 - bit % 8)) & 1);
    }
    key
}

struct SignatureGroup {
    file_ids: Vec<i64>,
    global: ImageHash<Vec<u8>>,
    detail: ImageHash<Vec<u8>>,
}

struct CandidatePlan {
    groups: Vec<SignatureGroup>,
    neighboring_groups: Vec<(usize, usize, u32)>,
}

fn candidate_plan(
    parsed: &[(i64, ImageHash<Vec<u8>>)],
    threshold: u32,
) -> rusqlite::Result<CandidatePlan> {
    let mut group_by_signature = HashMap::<Vec<u8>, usize>::new();
    let mut groups = Vec::<SignatureGroup>::new();
    for (file_id, hash) in parsed
        .iter()
        .filter(|(_, hash)| hash.as_bytes().len() == SUPPORTED_PHASH_BYTES)
    {
        let signature = hash.as_bytes().to_vec();
        if let Some(group_index) = group_by_signature.get(&signature) {
            groups[*group_index].file_ids.push(*file_id);
            continue;
        }
        let Ok(global) = ImageHash::<Vec<u8>>::from_bytes(&signature[..HASH_COMPONENT_BYTES])
        else {
            continue;
        };
        let Ok(detail) = ImageHash::<Vec<u8>>::from_bytes(&signature[HASH_COMPONENT_BYTES..])
        else {
            continue;
        };
        group_by_signature.insert(signature, groups.len());
        groups.push(SignatureGroup {
            file_ids: vec![*file_id],
            global,
            detail,
        });
    }

    let globals = groups
        .iter()
        .enumerate()
        .map(|(group_index, group)| (group_index as i64, group.global.clone()))
        .collect::<Vec<_>>();
    let mut index = CandidateIndex::new(threshold);
    let mut neighboring_groups = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        let neighbors = index.find_within(&globals, &group.global);
        if neighbors.len() > MAX_NEAR_SIGNATURE_GROUPS {
            return Err(invalid(format!(
                "duplicate scan paused: one perceptual-hash neighborhood contains more than {MAX_NEAR_SIGNATURE_GROUPS} distinct signatures"
            )));
        }
        neighboring_groups.extend(neighbors.into_iter().filter_map(
            |(other_group_index, distance)| {
                let other_group = &groups[other_group_index as usize];
                (other_group.detail.dist(&group.detail) <= DETAIL_DISTANCE_THRESHOLD).then_some((
                    other_group_index as usize,
                    group_index,
                    distance,
                ))
            },
        ));
        index.insert(group_index, &group.global);
    }
    Ok(CandidatePlan {
        groups,
        neighboring_groups,
    })
}

#[derive(Clone)]
struct SpatialDescriptor {
    pixels: Vec<Lab>,
}

#[derive(Debug, Clone, Copy)]
struct SpatialComparison {
    difference_basis_points: u32,
    distinct_fraction: f32,
    coherent_fraction: f32,
}

fn spatial_descriptor(bytes: &[u8]) -> Option<SpatialDescriptor> {
    spatial_descriptor_at_side(bytes, COMPARISON_SIDE)
}

fn spatial_descriptor_at_side(bytes: &[u8], side: u32) -> Option<SpatialDescriptor> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgb8();
    let source = fr::images::Image::from_vec_u8(
        decoded.width(),
        decoded.height(),
        decoded.into_raw(),
        fr::PixelType::U8x3,
    )
    .ok()?;
    let mut resized = fr::images::Image::new(side, side, fr::PixelType::U8x3);
    fr::Resizer::new()
        .resize(&source, &mut resized, None)
        .ok()?;
    let pixels = resized
        .buffer()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|rgb| {
            Srgb::new(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            )
            .into_linear::<f32>()
            .into_color()
        })
        .collect();
    Some(SpatialDescriptor { pixels })
}

fn spatial_comparison_at_side(
    left: &SpatialDescriptor,
    right: &SpatialDescriptor,
    side: usize,
) -> SpatialComparison {
    let delta = |left: Lab, right: Lab| {
        ((left.l - right.l).powi(2) + (left.a - right.a).powi(2) + (left.b - right.b).powi(2))
            .sqrt()
    };
    let overlap = |shift: isize| {
        let left_start = (-shift).max(0) as usize;
        let left_end = (side as isize - shift.max(0)) as usize;
        (left_start, left_end)
    };
    let mut best_shift = (0_isize, 0_isize);
    let mut best_cost = f32::INFINITY;
    for shift_y in -MAX_ALIGNMENT_SHIFT..=MAX_ALIGNMENT_SHIFT {
        let (start_y, end_y) = overlap(shift_y);
        for shift_x in -MAX_ALIGNMENT_SHIFT..=MAX_ALIGNMENT_SHIFT {
            let (start_x, end_x) = overlap(shift_x);
            let mut cost = 0.0_f32;
            let mut count = 0usize;
            for y in (start_y..end_y).step_by(ALIGNMENT_SAMPLE_STEP) {
                for x in (start_x..end_x).step_by(ALIGNMENT_SAMPLE_STEP) {
                    let right_x = (x as isize + shift_x) as usize;
                    let right_y = (y as isize + shift_y) as usize;
                    cost += delta(
                        left.pixels[y * side + x],
                        right.pixels[right_y * side + right_x],
                    )
                    .min(DISTINCT_COLOR_DELTA);
                    count += 1;
                }
            }
            let cost = cost / count.max(1) as f32;
            let displacement = shift_x.abs() + shift_y.abs();
            let best_displacement = best_shift.0.abs() + best_shift.1.abs();
            if cost < best_cost - f32::EPSILON
                || ((cost - best_cost).abs() <= f32::EPSILON && displacement < best_displacement)
            {
                best_cost = cost;
                best_shift = (shift_x, shift_y);
            }
        }
    }

    let (start_x, end_x) = overlap(best_shift.0);
    let (start_y, end_y) = overlap(best_shift.1);
    let width = end_x - start_x;
    let height = end_y - start_y;
    let mut deltas = Vec::with_capacity(width * height);
    for y in start_y..end_y {
        for x in start_x..end_x {
            let right_x = (x as isize + best_shift.0) as usize;
            let right_y = (y as isize + best_shift.1) as usize;
            deltas.push(delta(
                left.pixels[y * side + x],
                right.pixels[right_y * side + right_x],
            ));
        }
    }

    let total = deltas.len().max(1);
    let distinct_mask = deltas
        .iter()
        .map(|delta| *delta >= DISTINCT_COLOR_DELTA)
        .collect::<Vec<_>>();
    let distinct = distinct_mask.iter().filter(|changed| **changed).count();
    let mut pooled_difference = 0.0_f32;
    let mut tile_count = 0usize;
    for tile_y in (0..height).step_by(DIFFERENCE_TILE_SIDE) {
        for tile_x in (0..width).step_by(DIFFERENCE_TILE_SIDE) {
            let end_y = (tile_y + DIFFERENCE_TILE_SIDE).min(height);
            let end_x = (tile_x + DIFFERENCE_TILE_SIDE).min(width);
            let mut changed = 0usize;
            let mut pixels = 0usize;
            for y in tile_y..end_y {
                for x in tile_x..end_x {
                    changed += usize::from(distinct_mask[y * width + x]);
                    pixels += 1;
                }
            }
            pooled_difference += (changed as f32 / pixels as f32).powi(3);
            tile_count += 1;
        }
    }
    let pooled_difference = (pooled_difference / tile_count.max(1) as f32).cbrt();
    debug_assert_eq!(deltas.len(), width * height);
    let mut visited = vec![false; deltas.len()];
    let mut largest_region = 0usize;
    for start in 0..deltas.len() {
        if visited[start] || deltas[start] < STRONG_COLOR_DELTA {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![start];
        let mut region = 0usize;
        while let Some(pixel) = stack.pop() {
            region += 1;
            let x = pixel % width;
            let y = pixel / width;
            for neighbor in [
                (x > 0).then(|| pixel - 1),
                (x + 1 < width).then(|| pixel + 1),
                (y > 0).then(|| pixel - width),
                (y + 1 < height).then(|| pixel + width),
            ]
            .into_iter()
            .flatten()
            {
                if !visited[neighbor] && deltas[neighbor] >= STRONG_COLOR_DELTA {
                    visited[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        largest_region = largest_region.max(region);
    }

    SpatialComparison {
        // Cubic spatial pooling keeps a localized edit visible without giving
        // sub-threshold encoder noise any weight.
        difference_basis_points: (pooled_difference * 10_000.0).round() as u32,
        distinct_fraction: distinct as f32 / total as f32,
        coherent_fraction: largest_region as f32 / total as f32,
    }
}

fn spatial_comparison(left: &SpatialDescriptor, right: &SpatialDescriptor) -> SpatialComparison {
    spatial_comparison_at_side(left, right, COMPARISON_SIDE as usize)
}

fn spatially_consistent(comparison: SpatialComparison) -> bool {
    comparison.distinct_fraction <= 0.03 && comparison.coherent_fraction <= 0.0015
}

fn spatially_compare_library_pair(
    application: &crate::library_application::LibraryApplication,
    left: &StoredFile,
    right: &StoredFile,
    cache: &mut HashMap<i64, Option<SpatialDescriptor>>,
) -> Option<SettledPair> {
    let descriptor = |file: &StoredFile, cache: &mut HashMap<i64, Option<SpatialDescriptor>>| {
        cache
            .entry(file.file_id)
            .or_insert_with(|| spatial_descriptor_for_library_file(application, file))
            .clone()
    };
    let comparison = spatial_comparison(&descriptor(left, cache)?, &descriptor(right, cache)?);
    Some(if spatially_consistent(comparison) {
        SettledPair::Detected(comparison.difference_basis_points)
    } else {
        SettledPair::Rejected
    })
}

fn spatial_descriptor_for_library_file(
    application: &crate::library_application::LibraryApplication,
    file: &StoredFile,
) -> Option<SpatialDescriptor> {
    if let Some(bytes) = application
        .blobs()
        .read_thumbnail(&file.file_hash)
        .ok()
        .flatten()
    {
        return spatial_descriptor(&bytes);
    }
    let mut source = PreparedMediaSource::from_stored_metadata(
        file.file_path.clone()?,
        &file.mime_type,
        None,
        file.frame_count,
    );
    let (bytes, _) = source
        .render_inline_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS)
        .ok()?;
    spatial_descriptor(&bytes)
}
pub fn scan_library(
    application: &crate::library_application::LibraryApplication,
    distance_threshold: u32,
) -> Result<picto_library::DuplicateScanResult, String> {
    let started = Instant::now();
    application
        .library()
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Maintenance,
            ensure_spatial_cache,
        )
        .map_err(|error| error.to_string())?;
    let (files, mut settled_pairs) = application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::Maintenance,
            |connection| {
                Ok((
                    load_library_files_with_hash(connection)?,
                    load_known_comparisons(connection)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let loaded_at = Instant::now();
    let parsed = files
        .iter()
        .filter_map(|file| {
            parse_supported_hash(file.perceptual_hash.as_deref()?).map(|hash| (file.file_id, hash))
        })
        .collect::<Vec<_>>();
    let plan = candidate_plan(&parsed, distance_threshold).map_err(|error| error.to_string())?;
    let signature_group_count = plan.groups.len();
    let neighboring_group_count = plan.neighboring_groups.len();
    let planned_at = Instant::now();
    let by_id = files
        .into_iter()
        .map(|file| (file.file_id, file))
        .collect::<HashMap<_, _>>();
    let mut spatial_cache = HashMap::new();
    let mut spatial_comparisons = 0usize;
    let mut reused_pairs = 0usize;
    let mut verified_pairs = Vec::new();
    let mut new_cache_entries = Vec::new();
    let mut representatives = Vec::<Vec<i64>>::with_capacity(plan.groups.len());

    for group in &plan.groups {
        let mut group_representatives = Vec::new();
        for file_id in &group.file_ids {
            let Some(file) = by_id.get(file_id) else {
                continue;
            };
            let mut matched = false;
            for representative_id in &group_representatives {
                match settled_pairs.get(&normalized_pair(*representative_id, *file_id)) {
                    Some(SettledPair::Detected(distance)) => {
                        verified_pairs.push((*representative_id, *file_id, *distance));
                        reused_pairs += 1;
                        matched = true;
                        break;
                    }
                    Some(SettledPair::Rejected) => continue,
                    None => {}
                }
                spatial_comparisons += 1;
                if spatial_comparisons > MAX_SPATIAL_COMPARISONS {
                    return Err(
                        "duplicate scan paused: spatial verification budget exceeded".into(),
                    );
                }
                let Some(representative) = by_id.get(representative_id) else {
                    continue;
                };
                if let Some(outcome) = spatially_compare_library_pair(
                    application,
                    representative,
                    file,
                    &mut spatial_cache,
                ) {
                    new_cache_entries.push(CacheEntry::new(representative, file, outcome));
                    settled_pairs.insert(normalized_pair(*representative_id, *file_id), outcome);
                    if let SettledPair::Detected(distance) = outcome {
                        verified_pairs.push((*representative_id, *file_id, distance));
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                if group_representatives.len() >= MAX_SIGNATURE_REPRESENTATIVES {
                    return Err(format!(
                        "duplicate scan paused: one exact hash signature contains more than {MAX_SIGNATURE_REPRESENTATIVES} visually distinct representatives"
                    ));
                }
                group_representatives.push(*file_id);
            }
        }
        representatives.push(group_representatives);
    }

    for (left_group, right_group, _global_distance) in plan.neighboring_groups {
        for left_id in &representatives[left_group] {
            for right_id in &representatives[right_group] {
                match settled_pairs.get(&normalized_pair(*left_id, *right_id)) {
                    Some(SettledPair::Detected(distance)) => {
                        verified_pairs.push((*left_id, *right_id, *distance));
                        reused_pairs += 1;
                        continue;
                    }
                    Some(SettledPair::Rejected) => continue,
                    None => {}
                }
                spatial_comparisons += 1;
                if spatial_comparisons > MAX_SPATIAL_COMPARISONS {
                    return Err(
                        "duplicate scan paused: spatial verification budget exceeded".into(),
                    );
                }
                let (Some(left), Some(right)) = (by_id.get(left_id), by_id.get(right_id)) else {
                    continue;
                };
                if let Some(outcome) =
                    spatially_compare_library_pair(application, left, right, &mut spatial_cache)
                {
                    new_cache_entries.push(CacheEntry::new(left, right, outcome));
                    settled_pairs.insert(normalized_pair(*left_id, *right_id), outcome);
                    if let SettledPair::Detected(distance) = outcome {
                        verified_pairs.push((*left_id, *right_id, distance));
                    }
                }
            }
        }
    }

    if !new_cache_entries.is_empty() {
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Maintenance,
                |transaction| store_spatial_cache(transaction, &new_cache_entries),
            )
            .map_err(|error| error.to_string())?;
    }
    let verified_at = Instant::now();

    let pairs =
        verified_pairs
            .into_iter()
            .map(|(file_id_a, file_id_b, distance)| {
                Ok((
                    picto_library::FileId(u32::try_from(file_id_a).map_err(|_| {
                        format!("file ID {file_id_a} exceeds canonical ID capacity")
                    })?),
                    picto_library::FileId(u32::try_from(file_id_b).map_err(|_| {
                        format!("file ID {file_id_b} exceeds canonical ID capacity")
                    })?),
                    distance,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
    let result = application
        .library()
        .replace_detected_duplicate_pairs(&pairs, Utc::now().timestamp_millis())
        .map_err(|error| error.to_string())?;
    let completed_at = Instant::now();
    tracing::info!(
        hashed_files = parsed.len(),
        signature_groups = signature_group_count,
        neighboring_groups = neighboring_group_count,
        candidate_pairs = pairs.len(),
        reused_pairs,
        cached_comparisons = new_cache_entries.len(),
        spatial_comparisons,
        decoded_descriptors = spatial_cache.len(),
        load_ms = loaded_at.duration_since(started).as_millis() as u64,
        plan_ms = planned_at.duration_since(loaded_at).as_millis() as u64,
        verify_ms = verified_at.duration_since(planned_at).as_millis() as u64,
        persist_ms = completed_at.duration_since(verified_at).as_millis() as u64,
        elapsed_ms = completed_at.duration_since(started).as_millis() as u64,
        "Duplicate scan completed"
    );
    Ok(result)
}

#[derive(Debug, Clone, Copy)]
enum SettledPair {
    Detected(u32),
    Rejected,
}

#[derive(Debug)]
struct CacheEntry {
    file_id_a: i64,
    file_id_b: i64,
    hash_a: String,
    hash_b: String,
    outcome: SettledPair,
}

impl CacheEntry {
    fn new(left: &StoredFile, right: &StoredFile, outcome: SettledPair) -> Self {
        let (left, right) = if left.file_id < right.file_id {
            (left, right)
        } else {
            (right, left)
        };
        Self {
            file_id_a: left.file_id,
            file_id_b: right.file_id,
            hash_a: left.perceptual_hash.clone().unwrap_or_default(),
            hash_b: right.perceptual_hash.clone().unwrap_or_default(),
            outcome,
        }
    }
}

fn normalized_pair(left: i64, right: i64) -> (i64, i64) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

fn ensure_spatial_cache(transaction: &Transaction<'_>) -> picto_library::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS duplicate_spatial_cache (
             file_id_a INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
             file_id_b INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
             hash_a TEXT NOT NULL,
             hash_b TEXT NOT NULL,
             algorithm_version INTEGER NOT NULL,
             is_match INTEGER NOT NULL CHECK (is_match IN (0, 1)),
             distance INTEGER,
             PRIMARY KEY(file_id_a, file_id_b),
             CHECK (file_id_a < file_id_b),
             CHECK ((is_match = 0 AND distance IS NULL) OR
                    (is_match = 1 AND distance IS NOT NULL))
         ) WITHOUT ROWID, STRICT;",
    )?;
    Ok(())
}

fn load_known_comparisons(
    connection: &Connection,
) -> picto_library::Result<HashMap<(i64, i64), SettledPair>> {
    let mut comparisons = HashMap::new();
    {
        let mut statement = connection.prepare(
            "SELECT cache.file_id_a, cache.file_id_b, cache.is_match, cache.distance
             FROM duplicate_spatial_cache AS cache
             JOIN media_file AS left_file ON left_file.file_id = cache.file_id_a
             JOIN media_file AS right_file ON right_file.file_id = cache.file_id_b
             WHERE cache.algorithm_version = ?1
               AND cache.hash_a = left_file.perceptual_hash
               AND cache.hash_b = right_file.perceptual_hash",
        )?;
        let rows = statement.query_map([SPATIAL_CACHE_VERSION], |row| {
            let outcome = if row.get::<_, bool>(2)? {
                SettledPair::Detected(row.get(3)?)
            } else {
                SettledPair::Rejected
            };
            Ok(((row.get(0)?, row.get(1)?), outcome))
        })?;
        for row in rows {
            let (pair, outcome) = row?;
            comparisons.insert(pair, outcome);
        }
    }
    let mut statement =
        connection.prepare("SELECT file_id_a, file_id_b, distance, status FROM duplicate_pair")?;
    let rows = statement.query_map([], |row| {
        let status = row.get::<_, u8>(3)?;
        let pair = if status == 1 {
            SettledPair::Detected(row.get(2)?)
        } else {
            SettledPair::Rejected
        };
        Ok(((row.get(0)?, row.get(1)?), pair))
    })?;
    for row in rows {
        let (pair, outcome) = row?;
        comparisons.insert(pair, outcome);
    }
    Ok(comparisons)
}

fn store_spatial_cache(
    transaction: &Transaction<'_>,
    entries: &[CacheEntry],
) -> picto_library::Result<()> {
    let mut statement = transaction.prepare_cached(
        "INSERT INTO duplicate_spatial_cache
             (file_id_a, file_id_b, hash_a, hash_b, algorithm_version, is_match, distance)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(file_id_a, file_id_b) DO UPDATE SET
             hash_a = excluded.hash_a,
             hash_b = excluded.hash_b,
             algorithm_version = excluded.algorithm_version,
             is_match = excluded.is_match,
             distance = excluded.distance",
    )?;
    for entry in entries {
        let (is_match, distance) = match entry.outcome {
            SettledPair::Detected(distance) => (true, Some(distance)),
            SettledPair::Rejected => (false, None),
        };
        statement.execute(params![
            entry.file_id_a,
            entry.file_id_b,
            entry.hash_a,
            entry.hash_b,
            SPATIAL_CACHE_VERSION,
            is_match,
            distance,
        ])?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct StoredFile {
    file_id: i64,
    file_hash: String,
    mime_type: String,
    frame_count: Option<i64>,
    perceptual_hash: Option<String>,
    file_path: Option<PathBuf>,
}

fn load_library_files_with_hash(connection: &Connection) -> picto_library::Result<Vec<StoredFile>> {
    let mut statement = connection.prepare(
        "SELECT file_id, content_hash, mime, frame_count, perceptual_hash, file_path
         FROM media_file WHERE perceptual_hash IS NOT NULL ORDER BY file_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredFile {
            file_id: row.get::<_, u32>(0)? as i64,
            file_hash: row.get(1)?,
            mime_type: row.get(2)?,
            frame_count: row.get::<_, Option<u32>>(3)?.map(i64::from),
            perceptual_hash: row.get(4)?,
            file_path: Some(PathBuf::from(row.get::<_, String>(5)?)),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_file(file_id: i64, hash: &str) -> StoredFile {
        StoredFile {
            file_id,
            file_hash: format!("content-{file_id}"),
            mime_type: "image/png".into(),
            frame_count: None,
            perceptual_hash: Some(hash.into()),
            file_path: None,
        }
    }

    #[test]
    fn spatial_cache_reuses_rejections_until_either_hash_changes() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE media_file (
                     file_id INTEGER PRIMARY KEY,
                     perceptual_hash TEXT
                 ) STRICT;
                 CREATE TABLE duplicate_pair (
                     file_id_a INTEGER NOT NULL,
                     file_id_b INTEGER NOT NULL,
                     distance INTEGER NOT NULL,
                     status INTEGER NOT NULL,
                     PRIMARY KEY(file_id_a, file_id_b)
                 ) WITHOUT ROWID, STRICT;
                 INSERT INTO media_file VALUES (1, 'hash-a'), (2, 'hash-b');",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        ensure_spatial_cache(&transaction).unwrap();
        store_spatial_cache(
            &transaction,
            &[CacheEntry::new(
                &stored_file(1, "hash-a"),
                &stored_file(2, "hash-b"),
                SettledPair::Rejected,
            )],
        )
        .unwrap();
        transaction.commit().unwrap();

        assert!(matches!(
            load_known_comparisons(&connection).unwrap().get(&(1, 2)),
            Some(SettledPair::Rejected)
        ));

        connection
            .execute(
                "UPDATE media_file SET perceptual_hash = 'hash-b-v2' WHERE file_id = 2",
                [],
            )
            .unwrap();
        assert!(!load_known_comparisons(&connection)
            .unwrap()
            .contains_key(&(1, 2)));
    }
}
