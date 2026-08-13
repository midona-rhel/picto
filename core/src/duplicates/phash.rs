//! Shared perceptual-hash thresholds for duplicate detection and ingest review.
//!
//! Hash computation lives in `media_processing::phash`; duplicate detection
//! compares parsed 256-bit hashes directly during the scan.

use std::collections::HashMap;

use img_hash::ImageHash;

/// Default Hamming distance threshold for likely duplicates.
pub const DEFAULT_DISTANCE_THRESHOLD: u32 = 32;

/// Maximum Hamming distance covered by the fixed eight-partition index.
pub(crate) const MAX_INDEXED_DISTANCE: u32 = 7;

/// Picto stores only 256-bit perceptual hashes. Other decoded lengths are
/// invalid input for duplicate detection, not alternate hash formats.
pub(crate) fn parse_supported_hash(raw: &str) -> Option<ImageHash<Vec<u8>>> {
    let hash = ImageHash::<Vec<u8>>::from_base64(raw).ok()?;
    (hash.as_bytes().len() == 32).then_some(hash)
}

/// Decode a supported pHash into eight 32-bit values for SQLite INTEGER columns.
///
/// The 256-bit hash is divided into eight contiguous big-endian chunks. Any two
/// hashes within `MAX_INDEXED_DISTANCE` differ in at most seven bits, so at
/// least one complete chunk must be identical.
pub(crate) fn indexed_partition_values(raw: &str) -> Option<[i64; 8]> {
    let hash = parse_supported_hash(raw)?;
    let bytes = hash.as_bytes();
    let mut values = [0_i64; 8];
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let chunk: [u8; 4] = chunk.try_into().ok()?;
        values[index] = i64::from(u32::from_be_bytes(chunk));
    }
    Some(values)
}

/// An internal exact Hamming-distance index for duplicate candidate generation.
///
/// A hash is split into `threshold + 1` bit partitions. Any pair within the
/// threshold must share at least one complete partition, so the index can
/// retrieve a superset of matches and verify the original Hamming distance
/// exactly. This remains complete for identical hashes and high-candidate
/// datasets; the final pair emission is inherently quadratic when every pair
/// is a valid result.
pub(crate) struct CandidateIndex {
    threshold: u32,
    buckets: HashMap<(usize, usize, u64), Vec<usize>>,
    entry_count: usize,
}

impl CandidateIndex {
    pub(crate) fn new(threshold: u32) -> Self {
        Self {
            threshold,
            buckets: HashMap::new(),
            entry_count: 0,
        }
    }

    pub(crate) fn insert(&mut self, entry_index: usize, hash: &ImageHash<Vec<u8>>) {
        debug_assert_eq!(entry_index, self.entry_count);
        self.entry_count += 1;
        let bit_len = hash.as_bytes().len() * 8;

        if bit_len != 256 {
            return;
        }

        for (partition, key) in partition_keys(hash.as_bytes(), self.threshold) {
            self.buckets
                .entry((bit_len, partition, key))
                .or_default()
                .push(entry_index);
        }
    }

    fn find_within(
        &self,
        parsed: &[(i64, ImageHash<Vec<u8>>)],
        hash: &ImageHash<Vec<u8>>,
        threshold: u32,
    ) -> Vec<(i64, u32)> {
        if self.entry_count == 0 {
            return Vec::new();
        }

        let bit_len = hash.as_bytes().len() * 8;
        if bit_len != 256 {
            return Vec::new();
        }
        let mut candidate_indices = Vec::new();
        if bit_len == 256 && threshold < bit_len as u32 {
            for (partition, key) in partition_keys(hash.as_bytes(), threshold) {
                if let Some(entries) = self.buckets.get(&(bit_len, partition, key)) {
                    candidate_indices.extend(entries);
                }
            }
        } else {
            candidate_indices.extend(0..self.entry_count);
        }
        candidate_indices.sort_unstable();
        candidate_indices.dedup();

        let mut matches = Vec::new();
        for entry_index in candidate_indices {
            let (file_id, candidate_hash) = &parsed[entry_index];
            let distance = candidate_hash.dist(hash);
            if distance <= threshold {
                matches.push((*file_id, distance));
            }
        }
        matches
    }
}

fn partition_keys(bytes: &[u8], threshold: u32) -> Vec<(usize, u64)> {
    let bit_len = bytes.len() * 8;
    if bit_len == 0 {
        return vec![(0, 0)];
    }
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
    let mut key = 0u64;
    for offset in 0..length {
        let bit = start + offset;
        key = (key << 1) | u64::from((bytes[bit / 8] >> (7 - bit % 8)) & 1);
    }
    key
}

/// Return every pair within `threshold`, with each pair emitted once.
pub(crate) fn find_candidate_pairs(
    parsed: &[(i64, ImageHash<Vec<u8>>)],
    threshold: u32,
) -> Vec<(i64, i64, u32)> {
    let parsed: Vec<_> = parsed
        .iter()
        .filter(|(_, hash)| hash.as_bytes().len() == 32)
        .cloned()
        .collect();
    let mut index = CandidateIndex::new(threshold);
    let mut pairs = Vec::new();
    for (entry_index, (file_id, hash)) in parsed.iter().enumerate() {
        pairs.extend(
            index
                .find_within(&parsed, hash, threshold)
                .into_iter()
                .map(|(other_file_id, distance)| (other_file_id, *file_id, distance)),
        );
        index.insert(entry_index, hash);
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::{
        find_candidate_pairs, indexed_partition_values, parse_supported_hash, MAX_INDEXED_DISTANCE,
    };
    use img_hash::ImageHash;
    use std::time::Instant;

    fn brute_force_pairs(
        parsed: &[(i64, ImageHash<Vec<u8>>)],
        threshold: u32,
    ) -> Vec<(i64, i64, u32)> {
        let mut pairs = Vec::new();
        for (index, (file_id_a, hash_a)) in parsed.iter().enumerate() {
            for (file_id_b, hash_b) in parsed.iter().skip(index + 1) {
                let distance = hash_a.dist(hash_b);
                if distance <= threshold {
                    pairs.push((*file_id_a, *file_id_b, distance));
                }
            }
        }
        pairs
    }

    fn deterministic_hashes(count: usize) -> Vec<(i64, ImageHash<Vec<u8>>)> {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut hashes: Vec<(i64, ImageHash<Vec<u8>>)> = Vec::with_capacity(count);
        for index in 0..count {
            let mut bytes = [0u8; 32];
            for byte in &mut bytes {
                state ^= state << 7;
                state ^= state >> 9;
                state ^= state << 8;
                *byte = state as u8;
            }
            if index > 0 && index % 17 == 0 {
                bytes = hashes[index - 1]
                    .1
                    .as_bytes()
                    .try_into()
                    .expect("32-byte hash");
            } else if index > 0 && index % 19 == 0 {
                bytes[0] ^= 1;
                bytes[31] ^= 1;
            }
            hashes.push((
                index as i64,
                ImageHash::from_bytes(&bytes).expect("valid hash"),
            ));
        }
        hashes
    }

    fn sorted(mut pairs: Vec<(i64, i64, u32)>) -> Vec<(i64, i64, u32)> {
        pairs.sort_unstable();
        pairs
    }

    #[test]
    fn indexed_candidates_match_brute_force_at_threshold_edges() {
        let parsed = deterministic_hashes(512);
        for threshold in [0, 1, 2, 8, 32, 128, 256] {
            assert_eq!(
                sorted(find_candidate_pairs(&parsed, threshold)),
                sorted(brute_force_pairs(&parsed, threshold)),
                "threshold {threshold}"
            );
        }
    }

    #[test]
    fn indexed_candidates_keep_all_identical_hash_pairs() {
        let hash = ImageHash::from_bytes(&[0u8; 32]).expect("valid hash");
        let parsed = (0..128)
            .map(|file_id| (file_id, hash.clone()))
            .collect::<Vec<_>>();

        let pairs = find_candidate_pairs(&parsed, 0);
        assert_eq!(pairs.len(), 128 * 127 / 2);
        assert!(pairs.iter().all(|(_, _, distance)| *distance == 0));
    }

    #[test]
    fn non_256_bit_hashes_are_not_indexed_or_compared() {
        let short = ImageHash::from_bytes(&[0u8; 8]).expect("valid short hash");
        let full = ImageHash::from_bytes(&[0u8; 32]).expect("valid full hash");
        assert!(parse_supported_hash(&short.to_base64()).is_none());
        assert!(parse_supported_hash(&full.to_base64()).is_some());

        let pairs = find_candidate_pairs(&[(1, short.clone()), (2, short), (3, full)], 256);
        assert!(pairs.is_empty());
    }

    #[test]
    fn indexed_partitions_share_a_key_for_every_supported_distance() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for distance in 0..=MAX_INDEXED_DISTANCE {
            for _ in 0..128 {
                let mut base = [0_u8; 32];
                for byte in &mut base {
                    state ^= state << 7;
                    state ^= state >> 9;
                    state ^= state << 8;
                    *byte = state as u8;
                }

                let mut candidate = base;
                for bit_index in 0..distance {
                    // Spread flips across the eight chunks instead of relying
                    // on a single partition to make the test pass.
                    let bit = ((bit_index * 37 + distance * 11) % 256) as usize;
                    candidate[bit / 8] ^= 1 << (7 - bit % 8);
                }

                let base_hash = ImageHash::<Vec<u8>>::from_bytes(&base).expect("base hash");
                let candidate_hash =
                    ImageHash::<Vec<u8>>::from_bytes(&candidate).expect("candidate hash");
                assert_eq!(base_hash.dist(&candidate_hash), distance);

                let base_values =
                    indexed_partition_values(&base_hash.to_base64()).expect("base partitions");
                let candidate_values = indexed_partition_values(&candidate_hash.to_base64())
                    .expect("candidate partitions");
                assert!(
                    base_values
                        .iter()
                        .zip(candidate_values.iter())
                        .any(|(left, right)| left == right),
                    "distance {distance} must share a partition"
                );
            }
        }
    }

    #[test]
    fn indexed_partition_values_reject_malformed_and_wrong_length_hashes() {
        assert!(indexed_partition_values("not-a-base64-hash").is_none());

        let short = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 31]).expect("short hash");
        assert!(indexed_partition_values(&short.to_base64()).is_none());

        let valid = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 32]).expect("valid hash");
        let values = indexed_partition_values(&valid.to_base64()).expect("valid partitions");
        assert_eq!(values, [0_i64; 8]);
    }

    #[test]
    fn indexed_candidates_report_baseline_and_indexed_measurements() {
        let parsed = deterministic_hashes(4096);
        let threshold = 32;

        // This is the former scan body and is intentionally retained only as
        // a correctness/performance baseline for the indexed implementation.
        let baseline_start = Instant::now();
        let baseline = brute_force_pairs(&parsed, threshold);
        let baseline_elapsed = baseline_start.elapsed();

        let indexed_start = Instant::now();
        let indexed = find_candidate_pairs(&parsed, threshold);
        let indexed_elapsed = indexed_start.elapsed();

        let baseline_count = baseline.len();
        assert_eq!(sorted(indexed), sorted(baseline));
        eprintln!(
            "duplicate_scan population={} threshold={} pairs={} baseline_ms={} indexed_ms={}",
            parsed.len(),
            threshold,
            baseline_count,
            baseline_elapsed.as_secs_f64() * 1000.0,
            indexed_elapsed.as_secs_f64() * 1000.0,
        );
    }
}
