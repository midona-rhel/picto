//! Duplicate detection and physical-file resolution for the replacement backend.
//!
//! Duplicate review is file-level. Resolution replaces the inferior blob for
//! every occurrence, preserves collection members and sourced occurrences, and
//! collapses only redundant manual standalone roots into a standalone winner.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

use chrono::Utc;
use img_hash::ImageHash;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, FileHash, ItemId, MutationReceipt};
use crate::projection_v2::{
    FolderProjectionChange, ItemProjectionChange, RootProjectionChange, StructureProjectionDelta,
    TagProjectionChange,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FileQuality {
    #[ts(type = "number")]
    pub file_id: i64,
    pub file_hash: FileHash,
    pub mime_type: String,
    #[ts(type = "number")]
    pub size_bytes: i64,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
    #[ts(type = "number | null")]
    pub frame_count: Option<i64>,
    /// Optional decoded evidence supplied by a media analysis worker.
    pub decoded_information: Option<f64>,
    /// Optional alpha-channel evidence supplied by a media analysis worker.
    pub has_alpha: Option<bool>,
}

impl FileQuality {
    fn pixel_count(&self) -> Option<i64> {
        Some(
            self.pixel_width?
                .checked_mul(self.pixel_height?)
                .unwrap_or(i64::MAX),
        )
    }

    fn is_lossless(&self) -> bool {
        matches!(
            self.mime_type.as_str(),
            "image/png" | "image/tiff" | "image/bmp" | "image/x-icon" | "image/qoi"
        )
    }

    fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }
}

fn ratio_at_least(value: i64, reference: i64, numerator: u64, denominator: u64) -> bool {
    value > 0
        && reference > 0
        && (value as u128) * u128::from(denominator) >= (reference as u128) * u128::from(numerator)
}

fn ratio_at_most(value: i64, reference: i64, numerator: u64, denominator: u64) -> bool {
    value > 0
        && reference > 0
        && (value as u128) * u128::from(denominator) <= (reference as u128) * u128::from(numerator)
}

fn materially_greater(value: f64, reference: f64) -> bool {
    value.is_finite()
        && reference.is_finite()
        && value > reference
        && (reference <= 0.0 || value >= reference * 1.20)
}

fn dimension_order(left: &FileQuality, right: &FileQuality) -> Option<Ordering> {
    let (left_width, left_height) = (left.pixel_width?, left.pixel_height?);
    let (right_width, right_height) = (right.pixel_width?, right.pixel_height?);
    if left_width >= right_width
        && left_height >= right_height
        && (left_width > right_width || left_height > right_height)
    {
        Some(Ordering::Greater)
    } else if right_width >= left_width
        && right_height >= left_height
        && (right_width > left_width || right_height > left_height)
    {
        Some(Ordering::Less)
    } else if left_width == right_width && left_height == right_height {
        Some(Ordering::Equal)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub enum QualityDecision {
    LeftBetter,
    RightBetter,
    /// The files are close enough that a stable winner can be selected.
    AutoTieLeft,
    AutoTieRight,
    NeedsChoice,
}

impl QualityDecision {
    pub fn winner(self, left_file_id: i64, right_file_id: i64) -> Option<i64> {
        match self {
            Self::LeftBetter | Self::AutoTieLeft => Some(left_file_id),
            Self::RightBetter | Self::AutoTieRight => Some(right_file_id),
            Self::NeedsChoice => None,
        }
    }

    pub fn is_automatic(self) -> bool {
        matches!(
            self,
            Self::LeftBetter | Self::RightBetter | Self::AutoTieLeft | Self::AutoTieRight
        )
    }
}

/// Compare two already-detected candidates. `distance` is the pHash distance
/// used to produce the candidate and is required for safe encoded-quality ties.
pub fn compare_quality(
    left: &FileQuality,
    right: &FileQuality,
    distance: Option<u32>,
) -> QualityDecision {
    compare_quality_with_encoding(left, right, distance, None, None)
}

fn compare_quality_with_encoding(
    left: &FileQuality,
    right: &FileQuality,
    distance: Option<u32>,
    left_jpeg: Option<&JpegQuantization>,
    right_jpeg: Option<&JpegQuantization>,
) -> QualityDecision {
    if left.file_hash == right.file_hash {
        return stable_tie(left, right);
    }

    let left_pixels = left.pixel_count();
    let right_pixels = right.pixel_count();

    // Prefer strict Pareto dominance over an arbitrary resolution ratio. A
    // candidate that is at least as large in both dimensions and encoded with
    // at least as much data is not trading one measurable quality axis for
    // another.
    if left.mime_type == right.mime_type {
        match dimension_order(left, right) {
            Some(Ordering::Greater) if left.size_bytes >= right.size_bytes => {
                return QualityDecision::LeftBetter;
            }
            Some(Ordering::Less) if right.size_bytes >= left.size_bytes => {
                return QualityDecision::RightBetter;
            }
            _ => {}
        }
    }

    // A real two-times pixel-count advantage is decisive. This is intentionally
    // checked before byte density: a larger source is not discarded just
    // because it compresses better.
    if let (Some(left_pixels), Some(right_pixels)) = (left_pixels, right_pixels) {
        if ratio_at_least(left_pixels, right_pixels, 2, 1) {
            return QualityDecision::LeftBetter;
        }
        if ratio_at_least(right_pixels, left_pixels, 2, 1) {
            return QualityDecision::RightBetter;
        }
    }

    // Encoded quality is only evidence for an exact/negligible match. A
    // perceptually distant pair at the same dimensions still needs review.
    let negligible_hash = distance.is_some_and(|value| value <= 1);
    if negligible_hash {
        // When decoded information is available, prefer the materially richer
        // decoded result before considering encoded size.
        if let (Some(left_information), Some(right_information)) =
            (left.decoded_information, right.decoded_information)
        {
            if materially_greater(left_information, right_information) {
                return QualityDecision::LeftBetter;
            }
            if materially_greater(right_information, left_information) {
                return QualityDecision::RightBetter;
            }
        }

        if left.is_image() && right.is_image() {
            if left.has_alpha == Some(true) && right.has_alpha == Some(false) {
                return QualityDecision::LeftBetter;
            }
            if right.has_alpha == Some(true) && left.has_alpha == Some(false) {
                return QualityDecision::RightBetter;
            }
        }

        let same_dimensions =
            left.pixel_width == right.pixel_width && left.pixel_height == right.pixel_height;
        if same_dimensions && left.mime_type == "image/jpeg" && right.mime_type == "image/jpeg" {
            match left_jpeg
                .zip(right_jpeg)
                .and_then(|(left, right)| left.quality_order(right))
            {
                Some(Ordering::Greater) => return QualityDecision::LeftBetter,
                Some(Ordering::Less) => return QualityDecision::RightBetter,
                _ => {}
            }
            // Equal dimensions, format, and a negligible perceptual distance
            // leave no competing visual evidence. Keep the representation
            // carrying more encoded image data when quantization cannot break
            // the tie. Distance one is displayed as a 100% match by the UI and
            // must not behave differently from distance zero.
            if left.size_bytes != right.size_bytes {
                return if left.size_bytes > right.size_bytes {
                    QualityDecision::LeftBetter
                } else {
                    QualityDecision::RightBetter
                };
            }
        }

        // Lossless encoding wins only when the decoded dimensions are
        // comparable. A tiny thumbnail must not beat a full-size lossy image.
        if let (Some(left_pixels), Some(right_pixels)) = (left_pixels, right_pixels) {
            let comparable_dimensions = ratio_at_least(left_pixels, right_pixels, 9, 10)
                && ratio_at_least(right_pixels, left_pixels, 9, 10);
            if comparable_dimensions && left.is_lossless() != right.is_lossless() {
                return if left.is_lossless() {
                    QualityDecision::LeftBetter
                } else {
                    QualityDecision::RightBetter
                };
            }
        }

        let sizes_are_negligible =
            ratio_at_most(left.size_bytes.max(1), right.size_bytes.max(1), 105, 100)
                && ratio_at_most(right.size_bytes.max(1), left.size_bytes.max(1), 105, 100);
        if same_dimensions && sizes_are_negligible {
            return stable_tie(left, right);
        }
    }

    QualityDecision::NeedsChoice
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JpegQuantization {
    tables: BTreeMap<u8, Vec<u16>>,
}

impl JpegQuantization {
    /// Greater means uniformly finer quantization. Crossing tables do not
    /// establish an objective winner and therefore return `None`.
    fn quality_order(&self, other: &Self) -> Option<Ordering> {
        if self.tables.keys().ne(other.tables.keys()) {
            return None;
        }
        let mut finer = false;
        let mut coarser = false;
        for (id, left) in &self.tables {
            let right = other.tables.get(id)?;
            if left.len() != right.len() {
                return None;
            }
            for (left, right) in left.iter().zip(right) {
                finer |= left < right;
                coarser |= left > right;
                if finer && coarser {
                    return None;
                }
            }
        }
        Some(match (finer, coarser) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => Ordering::Equal,
        })
    }
}

fn parse_jpeg_quantization(bytes: &[u8]) -> Option<JpegQuantization> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2usize;
    let mut tables = BTreeMap::new();
    while offset + 1 < bytes.len() {
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = u16::from_be_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]) as usize;
        if length < 2 || offset + length > bytes.len() {
            return None;
        }
        let end = offset + length;
        offset += 2;
        if marker == 0xdb {
            while offset < end {
                let descriptor = bytes[offset];
                offset += 1;
                let precision = descriptor >> 4;
                let table_id = descriptor & 0x0f;
                let mut values = Vec::with_capacity(64);
                for _ in 0..64 {
                    let value = match precision {
                        0 => {
                            let value = *bytes.get(offset)? as u16;
                            offset += 1;
                            value
                        }
                        1 => {
                            let value =
                                u16::from_be_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]);
                            offset += 2;
                            value
                        }
                        _ => return None,
                    };
                    if offset > end {
                        return None;
                    }
                    values.push(value);
                }
                tables.insert(table_id, values);
            }
        } else {
            offset = end;
        }
    }
    (!tables.is_empty()).then_some(JpegQuantization { tables })
}

fn jpeg_quantization_for_file(app: &Application, file: &FileQuality) -> Option<JpegQuantization> {
    if file.mime_type != "image/jpeg" {
        return None;
    }
    let (path, _) = app
        .blobs()
        .find_original(&file.file_hash.0, Some("jpg"))
        .ok()??;
    let mut bytes = Vec::with_capacity(128 * 1024);
    std::fs::File::open(path)
        .ok()?
        .take(128 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    parse_jpeg_quantization(&bytes)
}

fn stable_tie(left: &FileQuality, right: &FileQuality) -> QualityDecision {
    if left.file_id <= right.file_id {
        QualityDecision::AutoTieLeft
    } else {
        QualityDecision::AutoTieRight
    }
}

fn compare_quality_with_recency(
    app: &Application,
    connection: &Connection,
    left: &FileQuality,
    right: &FileQuality,
    distance: Option<u32>,
) -> rusqlite::Result<QualityDecision> {
    let left_jpeg = jpeg_quantization_for_file(app, left);
    let right_jpeg = jpeg_quantization_for_file(app, right);
    let decision = compare_quality_with_encoding(
        left,
        right,
        distance,
        left_jpeg.as_ref(),
        right_jpeg.as_ref(),
    );
    if !matches!(
        decision,
        QualityDecision::AutoTieLeft | QualityDecision::AutoTieRight
    ) {
        return Ok(decision);
    }

    let captured_at = |file_id| {
        connection.query_row(
            "SELECT MAX(unixepoch(captured_at)) FROM media_asset WHERE file_id = ?1",
            [file_id],
            |row| row.get::<_, Option<i64>>(0),
        )
    };
    let left_captured_at = captured_at(left.file_id)?;
    let right_captured_at = captured_at(right.file_id)?;

    Ok(match (left_captured_at, right_captured_at) {
        (Some(left_date), Some(right_date)) if left_date > right_date => {
            QualityDecision::AutoTieLeft
        }
        (Some(left_date), Some(right_date)) if right_date > left_date => {
            QualityDecision::AutoTieRight
        }
        _ => decision,
    })
}

const SUPPORTED_PHASH_BYTES: usize = 32;
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
                .entry((SUPPORTED_PHASH_BYTES * 8, partition, key))
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
        if self.threshold < (SUPPORTED_PHASH_BYTES * 8) as u32 {
            for (partition, key) in partition_keys(hash.as_bytes(), self.threshold) {
                if let Some(entries) =
                    self.buckets
                        .get(&(SUPPORTED_PHASH_BYTES * 8, partition, key))
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

fn find_candidate_pairs(
    parsed: &[(i64, ImageHash<Vec<u8>>)],
    threshold: u32,
) -> Vec<(i64, i64, u32)> {
    let parsed = parsed
        .iter()
        .filter(|(_, hash)| hash.as_bytes().len() == SUPPORTED_PHASH_BYTES)
        .cloned()
        .collect::<Vec<_>>();
    let mut index = CandidateIndex::new(threshold);
    let mut pairs = Vec::new();
    for (entry_index, (file_id, hash)) in parsed.iter().enumerate() {
        pairs.extend(
            index
                .find_within(&parsed, hash)
                .into_iter()
                .map(|(other_file_id, distance)| (other_file_id, *file_id, distance)),
        );
        index.insert(entry_index, hash);
    }
    pairs
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct DuplicateCandidate {
    #[ts(type = "number")]
    pub file_id_a: i64,
    #[ts(type = "number")]
    pub file_id_b: i64,
    pub distance: u32,
    pub left: CandidateSide,
    pub right: CandidateSide,
    pub decision: QualityDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CandidateSide {
    pub file: FileQuality,
    pub occurrences: Vec<CandidateOccurrence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CandidateOccurrence {
    pub media_item_id: ItemId,
    pub root_item_id: ItemId,
    pub collection_id: Option<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct DuplicateScanResult {
    #[ts(type = "number")]
    pub candidate_count: usize,
    pub affected_item_ids: Vec<ItemId>,
    pub receipt: MutationReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub enum ResolutionChoice {
    KeepBoth,
    KeepFile {
        #[ts(type = "number")]
        winner_file_id: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ResolutionResult {
    pub choice: ResolutionChoice,
    pub affected_item_ids: Vec<ItemId>,
    pub freed_file_hash: Option<FileHash>,
    pub receipt: MutationReceipt,
}

/// Scan stored pHashes and replace only unresolved candidate rows. Existing
/// `not_duplicate` decisions are retained, so rescans do not reopen a user's
/// explicit decision.
pub fn scan(app: &Application, distance_threshold: u32) -> Result<DuplicateScanResult, String> {
    let ((candidates, affected_item_ids), revision) = app.transaction(
        |transaction| {
            let files = load_files_with_hash(transaction)?;
            let parsed = files
                .iter()
                .filter_map(|file| {
                    parse_supported_hash(file.perceptual_hash.as_deref()?)
                        .map(|hash| (file.file_id, hash))
                })
                .collect::<Vec<_>>();
            let pairs = find_candidate_pairs(&parsed, distance_threshold);

            transaction.execute("DELETE FROM duplicate WHERE status = 'detected'", [])?;
            let by_id = files
                .into_iter()
                .map(|file| (file.file_id, file))
                .collect::<HashMap<_, _>>();
            let mut candidates = Vec::new();
            let mut affected_item_ids: BTreeSet<i64> = BTreeSet::new();

            for (first, second, distance) in pairs {
                let (file_id_a, file_id_b) = if first < second {
                    (first, second)
                } else {
                    (second, first)
                };
                let inserted = transaction.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance, status)
                     VALUES (?1, ?2, ?3, 'detected')
                     ON CONFLICT(file_id_a, file_id_b) DO NOTHING",
                    params![file_id_a, file_id_b, distance],
                )?;
                if inserted == 0 {
                    continue;
                }
                let left_occurrences = occurrences_for_file(transaction, file_id_a)?;
                let right_occurrences = occurrences_for_file(transaction, file_id_b)?;
                // Keep detection durable while a subscription collection is being
                // assembled, but do not expose it until both sides have roots.
                if left_occurrences.is_empty() || right_occurrences.is_empty() {
                    continue;
                }
                let left_file = by_id
                    .get(&file_id_a)
                    .ok_or_else(|| invalid(format!("Duplicate file {file_id_a} disappeared")))?;
                let right_file = by_id
                    .get(&file_id_b)
                    .ok_or_else(|| invalid(format!("Duplicate file {file_id_b} disappeared")))?;
                affected_item_ids.extend(
                    left_occurrences
                        .iter()
                        .map(|occurrence| occurrence.root_item_id.0),
                );
                affected_item_ids.extend(
                    right_occurrences
                        .iter()
                        .map(|occurrence| occurrence.root_item_id.0),
                );
                candidates.push(DuplicateCandidate {
                    file_id_a,
                    file_id_b,
                    distance,
                    decision: compare_quality_with_recency(
                        app,
                        transaction,
                        &left_file.quality(),
                        &right_file.quality(),
                        Some(distance),
                    )?,
                    left: CandidateSide {
                        file: left_file.quality(),
                        occurrences: left_occurrences,
                    },
                    right: CandidateSide {
                        file: right_file.quality(),
                        occurrences: right_occurrences,
                    },
                });
            }

            Ok((
                (
                    candidates,
                    affected_item_ids
                        .into_iter()
                        .map(ItemId)
                        .collect::<Vec<_>>(),
                ),
                (),
            ))
        },
        |_projections, _| Ok(()),
    )?;

    Ok(DuplicateScanResult {
        candidate_count: candidates.len(),
        receipt: receipt(revision, affected_item_ids.clone(), false, false),
        affected_item_ids,
    })
}

pub fn list_candidates(app: &Application, limit: i64) -> Result<Vec<DuplicateCandidate>, String> {
    let limit = limit.clamp(1, 500);
    app.store().read(|connection| {
        let mut statement = connection.prepare(
            "SELECT d.file_id_a, d.file_id_b, d.distance
             FROM duplicate d
             WHERE d.status = 'detected'
               AND EXISTS (
                   SELECT 1
                   FROM media_asset ma
                   LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
                   LEFT JOIN library_root media_root ON media_root.item_id = ma.item_id
                   LEFT JOIN library_root collection_root
                     ON collection_root.item_id = cm.collection_id
                   WHERE ma.file_id = d.file_id_a
                     AND COALESCE(collection_root.item_id, media_root.item_id) IS NOT NULL
               )
               AND EXISTS (
                   SELECT 1
                   FROM media_asset ma
                   LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
                   LEFT JOIN library_root media_root ON media_root.item_id = ma.item_id
                   LEFT JOIN library_root collection_root
                     ON collection_root.item_id = cm.collection_id
                   WHERE ma.file_id = d.file_id_b
                     AND COALESCE(collection_root.item_id, media_root.item_id) IS NOT NULL
               )
             ORDER BY distance ASC, file_id_a ASC, file_id_b ASC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, u32>(2)?,
            ))
        })?;
        let pairs = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        pairs
            .into_iter()
            .map(|(file_id_a, file_id_b, distance)| {
                candidate_for_pair(app, connection, file_id_a, file_id_b, distance)
            })
            .collect()
    })
}

pub fn count_candidates(connection: &Connection) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT COUNT(*)
         FROM duplicate d
         WHERE d.status = 'detected'
           AND EXISTS (
               SELECT 1
               FROM media_asset ma
               LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
               LEFT JOIN library_root media_root ON media_root.item_id = ma.item_id
               LEFT JOIN library_root collection_root ON collection_root.item_id = cm.collection_id
               WHERE ma.file_id = d.file_id_a
                 AND COALESCE(collection_root.item_id, media_root.item_id) IS NOT NULL
           )
           AND EXISTS (
               SELECT 1
               FROM media_asset ma
               LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
               LEFT JOIN library_root media_root ON media_root.item_id = ma.item_id
               LEFT JOIN library_root collection_root ON collection_root.item_id = cm.collection_id
               WHERE ma.file_id = d.file_id_b
                 AND COALESCE(collection_root.item_id, media_root.item_id) IS NOT NULL
           )",
        [],
        |row| row.get(0),
    )
}

/// Resolve one file-level pair. Collection members and sourced occurrences keep
/// their identity and order. Redundant manual standalone roots are collapsed
/// only when a standalone winning root can receive their metadata.
pub fn resolve(
    app: &Application,
    file_id_a: i64,
    file_id_b: i64,
    choice: ResolutionChoice,
) -> Result<ResolutionResult, String> {
    if file_id_a == file_id_b {
        return Err("Duplicate pair must contain two different files".to_string());
    }
    let (file_id_a, file_id_b) = if file_id_a < file_id_b {
        (file_id_a, file_id_b)
    } else {
        (file_id_b, file_id_a)
    };
    let ((affected_item_ids, freed_file_hash, collapsed_items), revision) = app.transaction(
        |transaction| {
            let exists: Option<(String, String)> = transaction
                .query_row(
                    "SELECT status, COALESCE(winner_file_id, 0)
                     FROM duplicate WHERE file_id_a = ?1 AND file_id_b = ?2",
                    params![file_id_a, file_id_b],
                    |row| Ok((row.get(0)?, row.get::<_, i64>(1)?.to_string())),
                )
                .optional()?;
            match exists {
                Some((status, _)) if status == "detected" => {}
                Some((status, _)) => {
                    return Err(invalid(format!("Duplicate pair is already {status}")))
                }
                None => return Err(invalid("Duplicate pair was not found")),
            }

            let mut affected = BTreeSet::new();
            match choice {
                ResolutionChoice::KeepBoth => {
                    collect_pair_roots(transaction, file_id_a, file_id_b, &mut affected)?;
                    transaction.execute(
                        "UPDATE duplicate
                         SET status = 'not_duplicate', decided_at = ?3, winner_file_id = NULL
                         WHERE file_id_a = ?1 AND file_id_b = ?2",
                        params![file_id_a, file_id_b, Utc::now().to_rfc3339()],
                    )?;
                    Ok((
                        (
                            affected.into_iter().map(ItemId).collect::<Vec<_>>(),
                            None,
                            false,
                        ),
                        StructureProjectionDelta::default(),
                    ))
                }
                ResolutionChoice::KeepFile { winner_file_id } => {
                    if winner_file_id != file_id_a && winner_file_id != file_id_b {
                        return Err(invalid("Winner must be one of the duplicate files"));
                    }
                    let loser_file_id = if winner_file_id == file_id_a {
                        file_id_b
                    } else {
                        file_id_a
                    };
                    let loser_hash: String = transaction.query_row(
                        "SELECT file_hash FROM media_file WHERE file_id = ?1",
                        [loser_file_id],
                        |row| row.get(0),
                    )?;
                    collect_pair_roots(transaction, file_id_a, file_id_b, &mut affected)?;
                    let collapse =
                        collapsible_standalone_items(transaction, winner_file_id, loser_file_id)?;
                    let mut delta = StructureProjectionDelta::default();

                    transaction.execute(
                        "UPDATE media_asset SET file_id = ?1 WHERE file_id = ?2",
                        params![winner_file_id, loser_file_id],
                    )?;
                    if let Some((target_item_id, loser_item_ids)) = collapse {
                        for loser_item_id in loser_item_ids {
                            merge_standalone_item(
                                transaction,
                                target_item_id,
                                loser_item_id,
                                &mut delta,
                            )?;
                        }
                    }
                    transaction.execute(
                        "UPDATE duplicate SET status = 'resolved', decided_at = ?3,
                                winner_file_id = ?4
                         WHERE file_id_a = ?1 AND file_id_b = ?2",
                        params![
                            file_id_a,
                            file_id_b,
                            Utc::now().to_rfc3339(),
                            winner_file_id
                        ],
                    )?;
                    crate::workers_v2::enqueue_blob_delete_in(
                        transaction,
                        &loser_hash,
                        &Utc::now().to_rfc3339(),
                    )?;
                    transaction.execute(
                        "DELETE FROM media_file WHERE file_id = ?1
                         AND NOT EXISTS (SELECT 1 FROM media_asset WHERE file_id = ?1)",
                        [loser_file_id],
                    )?;
                    Ok((
                        (
                            affected.into_iter().map(ItemId).collect::<Vec<_>>(),
                            Some(FileHash(loser_hash)),
                            !delta.items.is_empty(),
                        ),
                        delta,
                    ))
                }
            }
        },
        |projections, delta| projections.apply_structure_delta(delta),
    )?;

    let changes_library = matches!(choice, ResolutionChoice::KeepFile { .. });
    Ok(ResolutionResult {
        choice,
        affected_item_ids: affected_item_ids.clone(),
        freed_file_hash,
        receipt: receipt(
            revision,
            affected_item_ids,
            changes_library,
            collapsed_items,
        ),
    })
}

pub fn resolve_automatically(
    app: &Application,
    file_id_a: i64,
    file_id_b: i64,
) -> Result<Option<ResolutionResult>, String> {
    let (file_id_a, file_id_b) = if file_id_a < file_id_b {
        (file_id_a, file_id_b)
    } else {
        (file_id_b, file_id_a)
    };
    let winner_file_id = app.store().read(|connection| {
        let distance = connection.query_row(
            "SELECT distance FROM duplicate
             WHERE file_id_a = ?1 AND file_id_b = ?2 AND status = 'detected'",
            params![file_id_a, file_id_b],
            |row| row.get::<_, u32>(0),
        )?;
        let candidate = candidate_for_pair(app, connection, file_id_a, file_id_b, distance)?;
        Ok(candidate.decision.winner(file_id_a, file_id_b))
    })?;
    let Some(winner_file_id) = winner_file_id else {
        return Ok(None);
    };
    resolve(
        app,
        file_id_a,
        file_id_b,
        ResolutionChoice::KeepFile { winner_file_id },
    )
    .map(Some)
}

#[derive(Debug, Clone)]
struct StoredFile {
    file_id: i64,
    file_hash: FileHash,
    mime_type: String,
    size_bytes: i64,
    pixel_width: Option<i64>,
    pixel_height: Option<i64>,
    frame_count: Option<i64>,
    perceptual_hash: Option<String>,
}

impl StoredFile {
    fn quality(&self) -> FileQuality {
        FileQuality {
            file_id: self.file_id,
            file_hash: self.file_hash.clone(),
            mime_type: self.mime_type.clone(),
            size_bytes: self.size_bytes,
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
            frame_count: self.frame_count,
            decoded_information: None,
            has_alpha: None,
        }
    }
}

fn load_files_with_hash(transaction: &Transaction<'_>) -> rusqlite::Result<Vec<StoredFile>> {
    let mut statement = transaction.prepare(
        "SELECT file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height,
                frame_count, perceptual_hash
         FROM media_file WHERE perceptual_hash IS NOT NULL ORDER BY file_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredFile {
            file_id: row.get(0)?,
            file_hash: FileHash(row.get(1)?),
            mime_type: row.get(2)?,
            size_bytes: row.get(3)?,
            pixel_width: row.get(4)?,
            pixel_height: row.get(5)?,
            frame_count: row.get(6)?,
            perceptual_hash: row.get(7)?,
        })
    })?;
    rows.collect()
}

fn occurrences_for_file(
    connection: &Connection,
    file_id: i64,
) -> rusqlite::Result<Vec<CandidateOccurrence>> {
    let mut statement = connection.prepare(
        "SELECT ma.item_id,
                COALESCE(collection_root.item_id, media_root.item_id),
                CASE WHEN collection_root.item_id IS NOT NULL THEN cm.collection_id END
         FROM media_asset ma
         LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
         LEFT JOIN library_root media_root ON media_root.item_id = ma.item_id
         LEFT JOIN library_root collection_root ON collection_root.item_id = cm.collection_id
         WHERE ma.file_id = ?1
           AND COALESCE(collection_root.item_id, media_root.item_id) IS NOT NULL
         ORDER BY COALESCE(collection_root.item_id, media_root.item_id), ma.item_id",
    )?;
    let occurrences = statement
        .query_map([file_id], |row| {
            Ok(CandidateOccurrence {
                media_item_id: ItemId(row.get(0)?),
                root_item_id: ItemId(row.get(1)?),
                collection_id: row.get::<_, Option<i64>>(2)?.map(ItemId),
            })
        })?
        .collect();
    occurrences
}

fn candidate_for_pair(
    app: &Application,
    connection: &Connection,
    file_id_a: i64,
    file_id_b: i64,
    distance: u32,
) -> rusqlite::Result<DuplicateCandidate> {
    let left = quality_for_file(connection, file_id_a)?;
    let right = quality_for_file(connection, file_id_b)?;
    Ok(DuplicateCandidate {
        file_id_a,
        file_id_b,
        distance,
        decision: compare_quality_with_recency(app, connection, &left, &right, Some(distance))?,
        left: CandidateSide {
            file: left,
            occurrences: occurrences_for_file(connection, file_id_a)?,
        },
        right: CandidateSide {
            file: right,
            occurrences: occurrences_for_file(connection, file_id_b)?,
        },
    })
}

fn quality_for_file(connection: &Connection, file_id: i64) -> rusqlite::Result<FileQuality> {
    connection.query_row(
        "SELECT file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height,
                frame_count
         FROM media_file WHERE file_id = ?1",
        [file_id],
        |row| {
            Ok(FileQuality {
                file_id: row.get(0)?,
                file_hash: FileHash(row.get(1)?),
                mime_type: row.get(2)?,
                size_bytes: row.get(3)?,
                pixel_width: row.get(4)?,
                pixel_height: row.get(5)?,
                frame_count: row.get(6)?,
                decoded_information: None,
                has_alpha: None,
            })
        },
    )
}

fn collect_pair_roots(
    transaction: &Transaction<'_>,
    file_id_a: i64,
    file_id_b: i64,
    affected: &mut BTreeSet<i64>,
) -> rusqlite::Result<()> {
    for occurrence in occurrences_for_file(transaction, file_id_a)?
        .into_iter()
        .chain(occurrences_for_file(transaction, file_id_b)?)
    {
        affected.insert(occurrence.root_item_id.0);
    }
    Ok(())
}

fn collapsible_standalone_items(
    transaction: &Transaction<'_>,
    winner_file_id: i64,
    loser_file_id: i64,
) -> rusqlite::Result<Option<(i64, Vec<i64>)>> {
    let target_item_id = transaction
        .query_row(
            "SELECT ma.item_id
             FROM media_asset ma
             JOIN library_root lr ON lr.item_id = ma.item_id
             WHERE ma.file_id = ?1
             ORDER BY ma.item_id LIMIT 1",
            [winner_file_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(target_item_id) = target_item_id else {
        return Ok(None);
    };

    let mut statement = transaction.prepare(
        "SELECT ma.item_id
         FROM media_asset ma
         JOIN library_root lr ON lr.item_id = ma.item_id
         WHERE ma.file_id = ?1
           AND NOT EXISTS (
               SELECT 1 FROM source_item si WHERE si.media_item_id = ma.item_id
           )
           AND NOT EXISTS (
               SELECT 1 FROM source_post sp WHERE sp.root_item_id = ma.item_id
           )
         ORDER BY ma.item_id",
    )?;
    let loser_item_ids = statement
        .query_map([loser_file_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((!loser_item_ids.is_empty()).then_some((target_item_id, loser_item_ids)))
}

fn merge_standalone_item(
    transaction: &Transaction<'_>,
    target_item_id: i64,
    loser_item_id: i64,
    delta: &mut StructureProjectionDelta,
) -> rusqlite::Result<()> {
    let target_metadata = media_metadata(transaction, target_item_id)?;
    let loser_metadata = media_metadata(transaction, loser_item_id)?;
    let source_urls = merge_source_urls(&target_metadata.source_urls, &loser_metadata.source_urls);
    let source_urls_json = serde_json::to_string(&source_urls)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "UPDATE media_asset
         SET name = ?1, notes = ?2, rating = ?3, source_urls_json = ?4,
             captured_at = ?5, updated_at = ?6
         WHERE item_id = ?7",
        params![
            prefer_text(target_metadata.name, loser_metadata.name),
            merge_notes(target_metadata.notes, loser_metadata.notes),
            target_metadata.rating.or(loser_metadata.rating),
            source_urls_json,
            target_metadata.captured_at.or(loser_metadata.captured_at),
            Utc::now().to_rfc3339(),
            target_item_id,
        ],
    )?;

    let folders = transaction
        .prepare("SELECT folder_id, position_rank FROM folder_item WHERE item_id = ?1")?
        .query_map([loser_item_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (folder_id, position_rank) in folders {
        transaction.execute(
            "INSERT OR IGNORE INTO folder_item (folder_id, item_id, position_rank)
             VALUES (?1, ?2, ?3)",
            params![folder_id, target_item_id, position_rank],
        )?;
        delta.folders.push(FolderProjectionChange {
            folder_id,
            item_id: target_item_id,
            present: true,
        });
        delta.folders.push(FolderProjectionChange {
            folder_id,
            item_id: loser_item_id,
            present: false,
        });
    }

    let tags = transaction
        .prepare("SELECT tag_id, source, provenance_mask FROM media_tag WHERE media_item_id = ?1")?
        .query_map([loser_item_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (tag_id, source, provenance_mask) in tags {
        transaction.execute(
            "INSERT INTO media_tag (media_item_id, tag_id, source, provenance_mask)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(media_item_id, tag_id, source) DO UPDATE SET
                 provenance_mask = media_tag.provenance_mask | excluded.provenance_mask",
            params![target_item_id, tag_id, source, provenance_mask],
        )?;
        delta.tags.push(TagProjectionChange {
            media_id: target_item_id,
            tag_id,
            present: true,
        });
    }

    let viewed_at = transaction
        .query_row(
            "SELECT viewed_at FROM media_view WHERE item_id = ?1",
            [loser_item_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(viewed_at) = viewed_at {
        transaction.execute(
            "INSERT INTO media_view (item_id, viewed_at) VALUES (?1, ?2)
             ON CONFLICT(item_id) DO UPDATE SET viewed_at = MAX(media_view.viewed_at, excluded.viewed_at)",
            params![target_item_id, viewed_at],
        )?;
    }

    transaction.execute(
        "DELETE FROM library_item WHERE item_id = ?1",
        [loser_item_id],
    )?;
    delta.items.push(ItemProjectionChange {
        item_id: loser_item_id,
        kind: crate::app::ItemKind::Media,
        present: false,
    });
    delta.roots.push(RootProjectionChange {
        item_id: loser_item_id,
        lifecycle: None,
    });
    Ok(())
}

struct MediaMetadata {
    name: Option<String>,
    notes: Option<String>,
    rating: Option<i64>,
    source_urls: Vec<String>,
    captured_at: Option<String>,
}

fn media_metadata(transaction: &Transaction<'_>, item_id: i64) -> rusqlite::Result<MediaMetadata> {
    transaction.query_row(
        "SELECT name, notes, rating, source_urls_json, captured_at
         FROM media_asset WHERE item_id = ?1",
        [item_id],
        |row| {
            let raw_urls = row.get::<_, Option<String>>(3)?;
            Ok(MediaMetadata {
                name: row.get(0)?,
                notes: row.get(1)?,
                rating: row.get(2)?,
                source_urls: raw_urls
                    .as_deref()
                    .and_then(|value| serde_json::from_str(value).ok())
                    .unwrap_or_default(),
                captured_at: row.get(4)?,
            })
        },
    )
}

fn prefer_text(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    primary
        .filter(|value| !value.trim().is_empty())
        .or_else(|| secondary.filter(|value| !value.trim().is_empty()))
}

fn merge_notes(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    let primary = primary.filter(|value| !value.trim().is_empty());
    let secondary = secondary.filter(|value| !value.trim().is_empty());
    match (primary, secondary) {
        (Some(primary), Some(secondary)) if primary != secondary => {
            Some(format!("{primary}\n\n{secondary}"))
        }
        (Some(primary), _) => Some(primary),
        (None, secondary) => secondary,
    }
}

fn merge_source_urls(primary: &[String], secondary: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    primary
        .iter()
        .chain(secondary)
        .filter_map(|url| seen.insert(url.clone()).then_some(url.clone()))
        .collect()
}

fn receipt(
    revision: u64,
    item_ids: Vec<ItemId>,
    changes_library: bool,
    collapsed_items: bool,
) -> MutationReceipt {
    let mut resources = vec![
        resources::DUPLICATES.to_string(),
        resources::SIDEBAR.to_string(),
    ];
    if changes_library {
        resources.push(resources::LIBRARY.to_string());
    }
    if collapsed_items {
        resources.extend([resources::FOLDERS.to_string(), resources::TAGS.to_string()]);
    }
    MutationReceipt {
        revision,
        resources,
        item_ids,
    }
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        compare_quality, compare_quality_with_encoding, count_candidates, find_candidate_pairs,
        list_candidates, parse_jpeg_quantization, parse_supported_hash, resolve, scan, FileQuality,
        QualityDecision, ResolutionChoice,
    };
    use crate::app::{Application, FileHash};
    use crate::store::Store;
    use img_hash::ImageHash;
    use rusqlite::params;

    fn quality(
        file_id: i64,
        size_bytes: i64,
        width: i64,
        height: i64,
        mime_type: &str,
    ) -> FileQuality {
        FileQuality {
            file_id,
            file_hash: FileHash(format!("hash-{file_id}")),
            mime_type: mime_type.to_string(),
            size_bytes,
            pixel_width: Some(width),
            pixel_height: Some(height),
            frame_count: Some(1),
            decoded_information: None,
            has_alpha: None,
        }
    }

    #[test]
    fn obvious_resolution_upgrade_wins() {
        let left = quality(1, 1_000_000, 4000, 4000, "image/jpeg");
        let right = quality(2, 800_000, 2000, 2000, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(8)),
            QualityDecision::LeftBetter
        );
    }

    #[test]
    fn exact_same_dimension_match_uses_encoded_information_without_ratio_gate() {
        let left = quality(11, 100_000, 1200, 800, "image/jpeg");
        let right = quality(12, 104_000, 1200, 800, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(1)),
            QualityDecision::RightBetter
        );
    }

    #[test]
    fn exact_same_dimension_match_with_more_encoded_information_wins() {
        let left = quality(1, 1_007_459, 1637, 2315, "image/jpeg");
        let right = quality(2, 818_954, 1637, 2315, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(0)),
            QualityDecision::LeftBetter
        );
    }

    #[test]
    fn resolution_and_size_dominance_has_no_arbitrary_ratio_gate() {
        let left = quality(1, 379_479, 1214, 1720, "image/jpeg");
        let right = quality(2, 533_336, 1518, 2150, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(3)),
            QualityDecision::RightBetter
        );
    }

    #[test]
    fn jpeg_quantization_compares_actual_encoding_quality() {
        fn jpeg_with_table(value: u8) -> Vec<u8> {
            let mut bytes = vec![0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00];
            bytes.extend(std::iter::repeat_n(value, 64));
            bytes.extend([0xff, 0xd9]);
            bytes
        }
        let finer = parse_jpeg_quantization(&jpeg_with_table(4)).unwrap();
        let coarser = parse_jpeg_quantization(&jpeg_with_table(12)).unwrap();
        let left = quality(1, 500_000, 1200, 800, "image/jpeg");
        let right = quality(2, 500_000, 1200, 800, "image/jpeg");
        assert_eq!(
            compare_quality_with_encoding(&left, &right, Some(1), Some(&finer), Some(&coarser),),
            QualityDecision::LeftBetter
        );
    }

    #[test]
    fn materially_different_same_dimensions_need_choice_without_close_hash() {
        let left = quality(1, 2_000_000, 2000, 1200, "image/jpeg");
        let right = quality(2, 500_000, 2000, 1200, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(10)),
            QualityDecision::NeedsChoice
        );
    }

    #[test]
    fn information_preserving_quality_wins_only_for_negligible_hash_distance() {
        let left = quality(1, 600_000, 1200, 800, "image/png");
        let right = quality(2, 550_000, 1200, 800, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(1)),
            QualityDecision::LeftBetter
        );
        assert_eq!(
            compare_quality(&left, &right, Some(10)),
            QualityDecision::NeedsChoice
        );

        let mut richer = quality(3, 500_000, 1200, 800, "image/jpeg");
        let mut poorer = quality(4, 500_000, 1200, 800, "image/jpeg");
        richer.decoded_information = Some(120.0);
        poorer.decoded_information = Some(100.0);
        assert_eq!(
            compare_quality(&richer, &poorer, Some(1)),
            QualityDecision::LeftBetter
        );
        assert_eq!(
            compare_quality(&richer, &poorer, Some(10)),
            QualityDecision::NeedsChoice
        );
    }

    #[test]
    fn replacement_parser_and_pairing_accept_only_supported_hashes() {
        let base = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 32]).unwrap();
        let mut one_bit = [0_u8; 32];
        one_bit[0] = 1;
        let far = ImageHash::<Vec<u8>>::from_bytes(&[0xff_u8; 32]).unwrap();
        let short = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 31]).unwrap();

        assert!(parse_supported_hash(&base.to_base64()).is_some());
        assert!(parse_supported_hash(&short.to_base64()).is_none());
        assert!(parse_supported_hash("not-a-base64-hash").is_none());

        let pairs = find_candidate_pairs(
            &[
                (10, base),
                (11, ImageHash::<Vec<u8>>::from_bytes(&one_bit).unwrap()),
                (12, far),
                (13, short),
            ],
            1,
        );
        assert_eq!(pairs, vec![(10, 11, 1)]);
    }

    #[test]
    fn candidate_queue_lists_most_similar_first() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.store()
            .transaction(|transaction| {
                for item_id in 1..=3_i64 {
                    let file_id = item_id + 9;
                    transaction.execute(
                        "INSERT INTO library_item
                             (item_id, item_key, kind, created_at, updated_at)
                         VALUES (?1, ?2, 'media', 'now', 'now')",
                        params![item_id, format!("item-{item_id}")],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                        [item_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, file_hash, mime_type, size_bytes, created_at)
                         VALUES (?1, ?2, 'image/jpeg', 1, 'now')",
                        params![file_id, format!("hash-{file_id}")],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                         VALUES (?1, ?2, 'now', 'now')",
                        params![item_id, file_id],
                    )?;
                }
                transaction.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance)
                     VALUES (10, 11, 8), (10, 12, 2)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(
            list_candidates(&app, 10)
                .unwrap()
                .into_iter()
                .map(|candidate| candidate.distance)
                .collect::<Vec<_>>(),
            vec![2, 8]
        );
    }

    #[test]
    fn scan_is_file_level_but_exposes_all_logical_occurrences() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let perceptual_hash = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 32])
            .unwrap()
            .to_base64();
        app.store()
            .transaction(|tx| {
                for item_id in 1..=3 {
                    tx.execute(
                        "INSERT INTO library_item
                             (item_id, item_key, kind, created_at, updated_at)
                         VALUES (?1, ?2, 'media', 'now', 'now')",
                        params![item_id, format!("item-{item_id}")],
                    )?;
                    tx.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                        [item_id],
                    )?;
                }
                tx.execute(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, pixel_width,
                          pixel_height, perceptual_hash, created_at)
                     VALUES (10, 'a', 'image/jpeg', 1000, 2000, 2000, ?1, 'now'),
                            (11, 'b', 'image/jpeg', 1000, 2000, 2000, ?1, 'now')",
                    [&perceptual_hash],
                )?;
                tx.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, captured_at, imported_at, updated_at)
                     VALUES (1, 10, '2024-01-01T00:00:00Z', 'now', 'now'),
                            (2, 11, '2025-01-01T00:00:00Z', 'now', 'now'),
                            (3, 10, '2024-01-01T00:00:00Z', 'now', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let result = scan(&app, 0).unwrap();
        assert_eq!(result.candidate_count, 1);
        assert_eq!(
            result.affected_item_ids,
            vec![
                crate::app::ItemId(1),
                crate::app::ItemId(2),
                crate::app::ItemId(3)
            ]
        );
        let candidates = list_candidates(&app, 10).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].decision, QualityDecision::AutoTieRight);
        assert_eq!(
            candidates[0]
                .left
                .occurrences
                .iter()
                .map(|occurrence| occurrence.media_item_id)
                .collect::<Vec<_>>(),
            vec![crate::app::ItemId(1), crate::app::ItemId(3)]
        );
        assert_eq!(
            candidates[0]
                .right
                .occurrences
                .iter()
                .map(|occurrence| occurrence.media_item_id)
                .collect::<Vec<_>>(),
            vec![crate::app::ItemId(2)]
        );
    }

    #[test]
    fn incomplete_subscription_collections_are_not_duplicate_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let perceptual_hash = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 32])
            .unwrap()
            .to_base64();
        app.store()
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'pending-member', 'media', 'now', 'now'),
                            (2, 'visible-media', 'media', 'now', 'now'),
                            (100, 'pending-collection', 'collection', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (2, 'inbox')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, pixel_width,
                          pixel_height, perceptual_hash, created_at)
                     VALUES (10, 'pending', 'image/jpeg', 1000, 1000, 1000, ?1, 'now'),
                            (11, 'visible', 'image/jpeg', 1000, 1000, 1000, ?1, 'now')",
                    [&perceptual_hash],
                )?;
                tx.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (1, 10, 'now', 'now'), (2, 11, 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (100, 1, 1024)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(scan(&app, 0).unwrap().candidate_count, 0);
        assert!(list_candidates(&app, 10).unwrap().is_empty());
        assert_eq!(app.store().read(count_candidates).unwrap(), 0);

        app.store()
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (100, 'inbox')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(app.store().read(count_candidates).unwrap(), 1);
        assert_eq!(
            list_candidates(&app, 10).unwrap()[0].left.occurrences[0].root_item_id,
            crate::app::ItemId(100)
        );
    }

    #[test]
    fn resolution_preserves_occurrences_tags_and_source_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.store()
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'one', 'media', 'now', 'now'),
                            (2, 'two', 'media', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (1, 'active'), (2, 'active')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, pixel_width,
                          pixel_height, perceptual_hash, created_at)
                     VALUES (10, 'winner', 'image/jpeg', 1000, 2000, 2000, 'x', 'now'),
                            (11, 'loser', 'image/jpeg', 500, 1000, 1000, 'y', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, name, notes, imported_at, updated_at)
                     VALUES (1, 10, 'one', 'keep-one', 'now', 'now'),
                            (2, 11, 'two', 'keep-two', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'kept')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_tag (media_item_id, tag_id) VALUES (2, 1)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO source_post
                         (source_post_id, site_id, post_key, created_at, updated_at)
                     VALUES (1, 'test', 'post', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO source_item
                         (source_post_id, item_key, position, media_item_id, state,
                          created_at, updated_at)
                     VALUES (1, 'one', 0, 1, 'ingested', 'now', 'now'),
                            (1, 'two', 1, 2, 'ingested', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance)
                     VALUES (10, 11, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let result = resolve(
            &app,
            10,
            11,
            ResolutionChoice::KeepFile { winner_file_id: 10 },
        )
        .unwrap();
        assert_eq!(result.affected_item_ids.len(), 2);
        assert_eq!(result.freed_file_hash, Some(FileHash("loser".to_string())));

        app.store()
            .read(|connection| {
                let assets: Vec<(i64, i64)> = connection
                    .prepare("SELECT item_id, file_id FROM media_asset ORDER BY item_id")?
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?;
                assert_eq!(assets, vec![(1, 10), (2, 10)]);
                let tag_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_tag WHERE media_item_id = 2",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(tag_count, 1);
                let source_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM source_item WHERE media_item_id IN (1, 2)",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(source_count, 2);
                let files: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?;
                let blob_hash: String = connection.query_row(
                    "SELECT file_hash FROM work_item WHERE work_type = 'blob_delete'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(files, 1);
                assert_eq!(blob_hash, "loser");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn resolution_preserves_collection_members_and_reports_the_collection_root() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.store()
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'member', 'media', 'now', 'now'),
                            (2, 'standalone', 'media', 'now', 'now'),
                            (100, 'collection', 'collection', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (2, 'active'), (100, 'active')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, pixel_width,
                          pixel_height, created_at)
                     VALUES (10, 'member-file', 'image/jpeg', 500, 1000, 1000, 'now'),
                            (11, 'winner-file', 'image/jpeg', 1000, 2000, 2000, 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (1, 10, 'now', 'now'), (2, 11, 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (100, 1, 1024)",
                    [],
                )?;
                tx.execute(
                    "UPDATE library_item SET cover_media_item_id = 1 WHERE item_id = 100",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (10, 11, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let candidate = list_candidates(&app, 10).unwrap().remove(0);
        assert_eq!(
            candidate.left.occurrences[0].media_item_id,
            crate::app::ItemId(1)
        );
        assert_eq!(
            candidate.left.occurrences[0].root_item_id,
            crate::app::ItemId(100)
        );
        assert_eq!(
            candidate.left.occurrences[0].collection_id,
            Some(crate::app::ItemId(100))
        );

        let result = resolve(
            &app,
            10,
            11,
            ResolutionChoice::KeepFile { winner_file_id: 11 },
        )
        .unwrap();
        assert_eq!(
            result.affected_item_ids,
            vec![crate::app::ItemId(2), crate::app::ItemId(100)]
        );
        app.store()
            .read(|connection| {
                let member_file: i64 = connection.query_row(
                    "SELECT file_id FROM media_asset WHERE item_id = 1",
                    [],
                    |row| row.get(0),
                )?;
                let membership: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM collection_member
                     WHERE collection_id = 100 AND media_item_id = 1 AND position_rank = 1024",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(member_file, 11);
                assert_eq!(membership, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn resolution_collapses_manual_standalone_loser_and_preserves_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.store()
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'winner', 'media', 'now', 'now'),
                            (2, 'loser', 'media', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (1, 'active'), (2, 'active')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (10, 'winner-file', 'image/jpeg', 1000, 'now'),
                            (11, 'loser-file', 'image/jpeg', 500, 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, name, notes, source_urls_json, imported_at, updated_at)
                     VALUES (1, 10, 'Winner', 'winner note', '[\"https://winner\"]', 'now', 'now'),
                            (2, 11, 'Loser', 'loser note', '[\"https://loser\"]', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'merged')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_tag (media_item_id, tag_id) VALUES (2, 1)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                     VALUES (1, 'folder', 'Folder', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO folder_item (folder_id, item_id, position_rank)
                     VALUES (1, 2, 1024)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (10, 11, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let result = resolve(
            &app,
            10,
            11,
            ResolutionChoice::KeepFile { winner_file_id: 10 },
        )
        .unwrap();
        assert!(result.receipt.resources.contains(&"sidebar".to_string()));
        app.store()
            .read(|connection| {
                let loser_exists: bool = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM library_item WHERE item_id = 2)",
                    [],
                    |row| row.get(0),
                )?;
                let (notes, urls): (String, String) = connection.query_row(
                    "SELECT notes, source_urls_json FROM media_asset WHERE item_id = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                let tag_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM media_tag WHERE media_item_id = 1 AND tag_id = 1",
                    [],
                    |row| row.get(0),
                )?;
                let folder_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE item_id = 1 AND folder_id = 1",
                    [],
                    |row| row.get(0),
                )?;
                assert!(!loser_exists);
                assert_eq!(notes, "winner note\n\nloser note");
                assert_eq!(urls, "[\"https://winner\",\"https://loser\"]");
                assert_eq!(tag_count, 1);
                assert_eq!(folder_count, 1);
                Ok(())
            })
            .unwrap();
        assert!(!app.projections().active_bitmap().contains(2));
    }

    #[test]
    fn keep_both_only_marks_the_pair_and_does_not_change_assets() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        app.store()
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'one', 'media', 'now', 'now'), (2, 'two', 'media', 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (1, 'active'), (2, 'active')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (10, 'a', 'image/jpeg', 10, 'now'), (11, 'b', 'image/jpeg', 10, 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (1, 10, 'now', 'now'), (2, 11, 'now', 'now')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (10, 11, 2)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        resolve(&app, 10, 11, ResolutionChoice::KeepBoth).unwrap();
        app.store()
            .read(|connection| {
                let status: String = connection.query_row(
                    "SELECT status FROM duplicate WHERE file_id_a = 10 AND file_id_b = 11",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(status, "not_duplicate");
                let files: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?;
                assert_eq!(files, 2);
                Ok(())
            })
            .unwrap();
    }
}
