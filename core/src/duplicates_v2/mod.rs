//! Duplicate detection and physical-file resolution for the replacement backend.
//!
//! Duplicate review is file-level. Resolution replaces the inferior blob for
//! every occurrence, preserves collection members and sourced occurrences, and
//! collapses only redundant manual standalone roots into a standalone winner.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

use chrono::Utc;
use fast_image_resize as fr;
use img_hash::ImageHash;
use palette::{IntoColor, Lab, Srgb};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, FileHash, ItemId, MutationReceipt};
use crate::blob_store::mime_to_extension;
use crate::media_processing::{PreparedMediaSource, DEFAULT_THUMBNAIL_DIMENSIONS};
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

    fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncodingClass {
    Lossless,
    Lossy,
    /// The format supports both modes, or its stored pixels cannot be
    /// classified reliably from the metadata currently available.
    Unknown,
}

fn default_encoding_class(mime_type: &str) -> EncodingClass {
    match mime_type {
        "image/jpeg" | "image/vnd.djvu" => EncodingClass::Lossy,
        "image/png" | "image/apng" | "image/gif" | "image/bmp" | "image/svg+xml"
        | "image/x-icon" | "image/qoi" | "image/x-tga" | "image/x-ilbm" => EncodingClass::Lossless,
        // TIFF, WebP, HEIF/AVIF, JPEG XL, DDS and EXR may each contain either
        // lossless or lossy image data. Their individual bitstreams are
        // inspected when Smart Merge has access to the stored original.
        _ => EncodingClass::Unknown,
    }
}

fn ratio_at_least(value: i64, reference: i64, numerator: u64, denominator: u64) -> bool {
    value > 0
        && reference > 0
        && (value as u128) * u128::from(denominator) >= (reference as u128) * u128::from(numerator)
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

fn has_objective_encoding_advantage(
    candidate: &FileQuality,
    reference: &FileQuality,
    candidate_encoding: EncodingClass,
    reference_encoding: EncodingClass,
) -> bool {
    candidate.is_image()
        && reference.is_image()
        && candidate_encoding == EncodingClass::Lossless
        && reference_encoding == EncodingClass::Lossy
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
    compare_quality_with_encoding(
        left,
        right,
        distance,
        default_encoding_class(&left.mime_type),
        default_encoding_class(&right.mime_type),
        None,
        None,
    )
}

fn compare_quality_with_encoding(
    left: &FileQuality,
    right: &FileQuality,
    distance: Option<u32>,
    left_encoding: EncodingClass,
    right_encoding: EncodingClass,
    left_jpeg: Option<&JpegQuantization>,
    right_jpeg: Option<&JpegQuantization>,
) -> QualityDecision {
    if left.file_hash == right.file_hash {
        return stable_tie(left, right);
    }

    let left_pixels = left.pixel_count();
    let right_pixels = right.pixel_count();
    let negligible_hash = distance.is_some_and(|value| value <= 1);
    let same_dimensions =
        left.pixel_width == right.pixel_width && left.pixel_height == right.pixel_height;

    // Duplicate candidates with exactly matching decoded dimensions do not
    // trade resolution against encoding quality. Prefer the known-lossless
    // representation over the known-lossy one even when small encoder
    // differences push the perceptual hash above the negligible-distance
    // threshold. Near-sized candidates retain the stricter hash requirement
    // below so similar crops and edits are never collapsed automatically.
    if same_dimensions {
        if has_objective_encoding_advantage(left, right, left_encoding, right_encoding) {
            return QualityDecision::LeftBetter;
        }
        if has_objective_encoding_advantage(right, left, right_encoding, left_encoding) {
            return QualityDecision::RightBetter;
        }
    }

    // Auto-select only when one image wins on two independent, objective axes:
    // it is at least as large in both dimensions and changes from a lossy to a
    // lossless representation. File size is deliberately absent: container
    // overhead, metadata and encoder efficiency make byte count unsuitable as
    // image-quality evidence. The candidates must also be an exact/negligible
    // perceptual match so a related-but-different crop cannot be discarded.
    match dimension_order(left, right) {
        Some(Ordering::Greater)
            if negligible_hash
                && has_objective_encoding_advantage(left, right, left_encoding, right_encoding) =>
        {
            return QualityDecision::LeftBetter;
        }
        Some(Ordering::Less)
            if negligible_hash
                && has_objective_encoding_advantage(right, left, right_encoding, left_encoding) =>
        {
            return QualityDecision::RightBetter;
        }
        _ => {}
    }

    // Encoded quality is only evidence for an exact/negligible match. A
    // perceptually distant pair at the same dimensions still needs review.
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

        if same_dimensions && left.mime_type == "image/jpeg" && right.mime_type == "image/jpeg" {
            match left_jpeg
                .zip(right_jpeg)
                .and_then(|(left, right)| left.quality_order(right))
            {
                Some(Ordering::Greater) => return QualityDecision::LeftBetter,
                Some(Ordering::Less) => return QualityDecision::RightBetter,
                _ => {}
            }
        }

        // Lossless encoding wins only when the decoded dimensions are
        // comparable. A tiny thumbnail must not beat a full-size lossy image.
        if let (Some(left_pixels), Some(right_pixels)) = (left_pixels, right_pixels) {
            let comparable_dimensions = ratio_at_least(left_pixels, right_pixels, 9, 10)
                && ratio_at_least(right_pixels, left_pixels, 9, 10);
            if comparable_dimensions && left_encoding != right_encoding {
                return if left_encoding == EncodingClass::Lossless
                    && right_encoding == EncodingClass::Lossy
                {
                    QualityDecision::LeftBetter
                } else if right_encoding == EncodingClass::Lossless
                    && left_encoding == EncodingClass::Lossy
                {
                    QualityDecision::RightBetter
                } else {
                    QualityDecision::NeedsChoice
                };
            }
        }

        // Byte size is a final tie-breaker only for effectively identical
        // images using the same codec and encoding class. It is never compared
        // across formats, dimensions, frame counts or known encoding modes.
        let equivalent_encoding = same_dimensions
            && left.frame_count == right.frame_count
            && left.mime_type == right.mime_type
            && left_encoding == right_encoding;
        if equivalent_encoding {
            return match left.size_bytes.cmp(&right.size_bytes) {
                Ordering::Greater => QualityDecision::LeftBetter,
                Ordering::Less => QualityDecision::RightBetter,
                Ordering::Equal => stable_tie(left, right),
            };
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

fn jpeg_encoding_class(bytes: &[u8]) -> EncodingClass {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return EncodingClass::Unknown;
    }
    // SOF3/7/11/15 are the lossless JPEG processes. SOF55 is JPEG-LS.
    for marker in bytes.windows(2) {
        if marker[0] == 0xff && matches!(marker[1], 0xc3 | 0xc7 | 0xcb | 0xcf | 0xf7) {
            return EncodingClass::Lossless;
        }
    }
    EncodingClass::Lossy
}

fn webp_encoding_class(bytes: &[u8]) -> EncodingClass {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return EncodingClass::Unknown;
    }
    let has_lossless = bytes.windows(4).any(|chunk| chunk == b"VP8L");
    let has_lossy = bytes.windows(4).any(|chunk| chunk == b"VP8 ");
    match (has_lossless, has_lossy) {
        (true, false) => EncodingClass::Lossless,
        (false, true) => EncodingClass::Lossy,
        // Mixed-mode animations and malformed/unsupported containers are not
        // safe automatic winners.
        _ => EncodingClass::Unknown,
    }
}

fn tiff_encoding_class(bytes: &[u8]) -> EncodingClass {
    (|| -> Option<EncodingClass> {
        let little_endian = match bytes.get(..4) {
            Some(b"II\x2a\x00") => true,
            Some(b"MM\x00\x2a") => false,
            _ => return None,
        };
        let read_u16 = |offset: usize| -> Option<u16> {
            let value = [*bytes.get(offset)?, *bytes.get(offset + 1)?];
            Some(if little_endian {
                u16::from_le_bytes(value)
            } else {
                u16::from_be_bytes(value)
            })
        };
        let read_u32 = |offset: usize| -> Option<u32> {
            let value = [
                *bytes.get(offset)?,
                *bytes.get(offset + 1)?,
                *bytes.get(offset + 2)?,
                *bytes.get(offset + 3)?,
            ];
            Some(if little_endian {
                u32::from_le_bytes(value)
            } else {
                u32::from_be_bytes(value)
            })
        };
        let ifd = usize::try_from(read_u32(4)?).ok()?;
        let entries = usize::from(read_u16(ifd)?);
        for index in 0..entries {
            let entry = ifd.checked_add(2 + index * 12)?;
            if read_u16(entry)? != 259 {
                continue;
            }
            let compression = read_u16(entry + 8)?;
            return Some(match compression {
                // None, CCITT, LZW, Deflate, PackBits, Zstd and LZMA preserve
                // stored samples. JPEG-in-TIFF is lossy. Less common hybrid
                // codecs remain unknown rather than being guessed.
                1 | 2 | 3 | 4 | 5 | 8 | 32_773 | 32_946 | 34_925 | 50_000 => {
                    EncodingClass::Lossless
                }
                6 | 7 => EncodingClass::Lossy,
                _ => EncodingClass::Unknown,
            });
        }
        None
    })()
    .unwrap_or(EncodingClass::Unknown)
}

fn encoding_class_for_file(app: &Application, file: &FileQuality) -> EncodingClass {
    let fallback = default_encoding_class(&file.mime_type);
    if !matches!(
        file.mime_type.as_str(),
        "image/jpeg" | "image/webp" | "image/tiff"
    ) {
        return fallback;
    }
    let extension = mime_to_extension(&file.mime_type);
    let Some((path, _)) = app
        .blobs()
        .find_original(&file.file_hash.0, Some(extension))
        .ok()
        .flatten()
    else {
        return fallback;
    };
    let mut bytes = Vec::with_capacity(512 * 1024);
    if std::fs::File::open(path)
        .and_then(|file| file.take(512 * 1024).read_to_end(&mut bytes))
        .is_err()
    {
        return fallback;
    }
    match file.mime_type.as_str() {
        "image/jpeg" => jpeg_encoding_class(&bytes),
        "image/webp" => webp_encoding_class(&bytes),
        "image/tiff" => tiff_encoding_class(&bytes),
        _ => fallback,
    }
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
        encoding_class_for_file(app, left),
        encoding_class_for_file(app, right),
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
        .chunks_exact(3)
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

fn spatially_verify_pair(
    app: &Application,
    left: &StoredFile,
    right: &StoredFile,
    cache: &mut HashMap<i64, Option<SpatialDescriptor>>,
) -> Option<u32> {
    let descriptor = |file: &StoredFile, cache: &mut HashMap<i64, Option<SpatialDescriptor>>| {
        cache
            .entry(file.file_id)
            .or_insert_with(|| spatial_descriptor_for_file(app, file))
            .clone()
    };
    let comparison = spatial_comparison(&descriptor(left, cache)?, &descriptor(right, cache)?);
    spatially_consistent(comparison).then_some(comparison.difference_basis_points)
}

fn spatial_descriptor_for_file(app: &Application, file: &StoredFile) -> Option<SpatialDescriptor> {
    if let Some(bytes) = app.blobs().read_thumbnail(&file.file_hash.0).ok().flatten() {
        return spatial_descriptor(&bytes);
    }

    // Collection members intentionally keep only the cover thumbnail. Decode
    // an original only after pHash has qualified the file for spatial
    // verification, and cache this reduced descriptor for the rest of a scan.
    let extension = mime_to_extension(&file.mime_type);
    let path = app
        .blobs()
        .original_path_with_ext(&file.file_hash.0, Some(extension))
        .ok()?;
    let mut source =
        PreparedMediaSource::from_stored_metadata(path, &file.mime_type, None, file.frame_count);
    let (bytes, _) = source
        .render_inline_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS)
        .ok()?;
    spatial_descriptor(&bytes)
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
            let plan = candidate_plan(&parsed, distance_threshold)?;
            let by_id = files
                .into_iter()
                .map(|file| (file.file_id, file))
                .collect::<HashMap<_, _>>();
            let mut spatial_cache = HashMap::new();
            let mut spatial_comparisons = 0usize;
            let mut verified_pairs = Vec::new();
            let mut representatives = Vec::<Vec<i64>>::with_capacity(plan.groups.len());
            for group in &plan.groups {
                let mut group_representatives = Vec::new();
                for file_id in &group.file_ids {
                    let Some(file) = by_id.get(file_id) else {
                        continue;
                    };
                    let mut matched = false;
                    for representative_id in &group_representatives {
                        spatial_comparisons += 1;
                        if spatial_comparisons > MAX_SPATIAL_COMPARISONS {
                            return Err(invalid("duplicate scan paused: spatial verification budget exceeded"));
                        }
                        let Some(representative) = by_id.get(representative_id) else {
                            continue;
                        };
                        if let Some(distance) =
                            spatially_verify_pair(app, representative, file, &mut spatial_cache)
                        {
                            verified_pairs.push((*representative_id, *file_id, distance));
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        if group_representatives.len() >= MAX_SIGNATURE_REPRESENTATIVES {
                            return Err(invalid(format!(
                                "duplicate scan paused: one exact hash signature contains more than {MAX_SIGNATURE_REPRESENTATIVES} visually distinct representatives"
                            )));
                        }
                        group_representatives.push(*file_id);
                    }
                }
                representatives.push(group_representatives);
            }

            for (left_group, right_group, _global_distance) in plan.neighboring_groups {
                for left_id in &representatives[left_group] {
                    for right_id in &representatives[right_group] {
                        spatial_comparisons += 1;
                        if spatial_comparisons > MAX_SPATIAL_COMPARISONS {
                            return Err(invalid("duplicate scan paused: spatial verification budget exceeded"));
                        }
                        let (Some(left), Some(right)) = (by_id.get(left_id), by_id.get(right_id))
                        else {
                            continue;
                        };
                        if let Some(distance) =
                            spatially_verify_pair(app, left, right, &mut spatial_cache)
                        {
                            verified_pairs.push((*left_id, *right_id, distance));
                        }
                    }
                }
            }

            transaction.execute("DELETE FROM duplicate WHERE status = 'detected'", [])?;
            let mut candidates = Vec::new();
            let mut affected_item_ids: BTreeSet<i64> = BTreeSet::new();

            for (first, second, distance) in verified_pairs {
                let (file_id_a, file_id_b) = if first < second {
                    (first, second)
                } else {
                    (second, first)
                };
                let Some(left_file) = by_id.get(&file_id_a) else {
                    continue;
                };
                let Some(right_file) = by_id.get(&file_id_b) else {
                    continue;
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
        candidate_plan, compare_quality, compare_quality_with_encoding, count_candidates,
        jpeg_encoding_class, list_candidates, parse_jpeg_quantization, parse_supported_hash,
        resolve, scan, spatial_comparison, spatial_descriptor, spatially_consistent,
        tiff_encoding_class, webp_encoding_class, EncodingClass, FileQuality, QualityDecision,
        ResolutionChoice,
    };
    use crate::app::{Application, FileHash};
    use crate::store::Store;
    use img_hash::ImageHash;
    use rusqlite::params;

    fn encoded_png(image: &image::DynamicImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn write_test_thumbnail(app: &Application, hash: &str) {
        write_test_thumbnail_color(app, hash, [80, 120, 160]);
    }

    fn write_test_thumbnail_color(app: &Application, hash: &str, color: [u8; 3]) {
        let image = image::DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(
            64,
            64,
            image::Rgb(color),
        ));
        app.blobs()
            .write_thumbnail(hash, &encoded_png(&image), "png")
            .unwrap();
    }

    #[test]
    fn spatial_verification_ignores_compression_but_rejects_local_structure_changes() {
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageBuffer, Rgb};

        let base = DynamicImage::ImageRgb8(ImageBuffer::from_fn(512, 384, |x, y| {
            Rgb([
                ((x * 7 + y * 3) % 256) as u8,
                ((x * 2 + y * 11) % 256) as u8,
                ((x * 13 + y * 5) % 256) as u8,
            ])
        }));
        let base_descriptor = spatial_descriptor(&encoded_png(&base)).unwrap();

        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 65)
            .encode_image(&base)
            .unwrap();
        let jpeg_descriptor = spatial_descriptor(&jpeg).unwrap();
        let compression = spatial_comparison(&base_descriptor, &jpeg_descriptor);
        assert!(spatially_consistent(compression));

        let mut edited = base.to_rgb8();
        for y in 120..264 {
            for x in 180..332 {
                edited.put_pixel(x, y, Rgb([250, 20, 180]));
            }
        }
        let edited_descriptor =
            spatial_descriptor(&encoded_png(&DynamicImage::ImageRgb8(edited))).unwrap();
        let edit = spatial_comparison(&base_descriptor, &edited_descriptor);
        assert!(!spatially_consistent(edit));
        assert!(edit.difference_basis_points > compression.difference_basis_points);
    }

    #[test]
    fn sub_visible_pixel_noise_does_not_reduce_similarity() {
        use image::{DynamicImage, ImageBuffer, Rgb};

        let left = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(96, 96, Rgb([96, 96, 96])));
        let right = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(96, 96, Rgb([98, 98, 98])));
        let left = spatial_descriptor(&encoded_png(&left)).unwrap();
        let right = spatial_descriptor(&encoded_png(&right)).unwrap();
        let comparison = spatial_comparison(&left, &right);

        assert_eq!(comparison.difference_basis_points, 0);
        assert_eq!(comparison.distinct_fraction, 0.0);
        assert_eq!(comparison.coherent_fraction, 0.0);
    }

    #[test]
    fn a_small_coherent_edit_remains_visible_to_review() {
        use image::{DynamicImage, ImageBuffer, Rgb};

        let base = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(512, 512, Rgb([72, 72, 72])));
        let mut edited = base.to_rgb8();
        for y in 240..264 {
            for x in 240..264 {
                edited.put_pixel(x, y, Rgb([230, 40, 40]));
            }
        }
        let base = spatial_descriptor(&encoded_png(&base)).unwrap();
        let edited = spatial_descriptor(&encoded_png(&DynamicImage::ImageRgb8(edited))).unwrap();
        let comparison = spatial_comparison(&base, &edited);

        assert!(comparison.distinct_fraction < 0.01);
        assert!(comparison.coherent_fraction > 0.0005);
        assert!(comparison.difference_basis_points > 0);
    }

    #[test]
    fn spatial_verification_aligns_small_translations_before_scoring() {
        use image::{DynamicImage, ImageBuffer, Rgb, RgbImage};

        fn shifted(source: &RgbImage, shift_x: i32, shift_y: i32) -> RgbImage {
            ImageBuffer::from_fn(source.width(), source.height(), |x, y| {
                let source_x = x as i32 - shift_x;
                let source_y = y as i32 - shift_y;
                if source_x >= 0
                    && source_y >= 0
                    && source_x < source.width() as i32
                    && source_y < source.height() as i32
                {
                    *source.get_pixel(source_x as u32, source_y as u32)
                } else {
                    Rgb([12, 12, 12])
                }
            })
        }

        let base = ImageBuffer::from_fn(96, 96, |x, y| {
            Rgb([
                ((x * 3 + y * 5) % 220 + 20) as u8,
                ((x * 7 + y * 2) % 210 + 25) as u8,
                ((x * 2 + y * 11) % 200 + 30) as u8,
            ])
        });
        let translated = shifted(&base, 2, -3);
        let left =
            spatial_descriptor(&encoded_png(&DynamicImage::ImageRgb8(base.clone()))).unwrap();
        let right =
            spatial_descriptor(&encoded_png(&DynamicImage::ImageRgb8(translated.clone()))).unwrap();
        let aligned = spatial_comparison(&left, &right);

        assert_eq!(aligned.difference_basis_points, 0);
        assert_eq!(aligned.coherent_fraction, 0.0);

        let mut edited = translated;
        for y in 40..48 {
            for x in 44..52 {
                edited.put_pixel(x, y, Rgb([245, 24, 180]));
            }
        }
        let edited = spatial_descriptor(&encoded_png(&DynamicImage::ImageRgb8(edited))).unwrap();
        let edited = spatial_comparison(&left, &edited);
        assert!(edited.difference_basis_points > 0);
        assert!(edited.coherent_fraction > 0.0);
    }

    #[test]
    fn spatial_verification_rejects_different_marks_on_white_fields() {
        use image::{DynamicImage, ImageBuffer, Rgb};

        let left = DynamicImage::ImageRgb8(ImageBuffer::from_fn(512, 384, |x, y| {
            if (40..280).contains(&x) && (50..330).contains(&y) {
                Rgb([30, 30, 30])
            } else {
                Rgb([255, 255, 255])
            }
        }));
        let right = DynamicImage::ImageRgb8(ImageBuffer::from_fn(512, 384, |x, y| {
            if (300..470).contains(&x) && (140..250).contains(&y) {
                Rgb([30, 30, 30])
            } else {
                Rgb([255, 255, 255])
            }
        }));
        let left = spatial_descriptor(&encoded_png(&left)).unwrap();
        let right = spatial_descriptor(&encoded_png(&right)).unwrap();
        assert!(!spatially_consistent(spatial_comparison(&left, &right)));
    }

    #[test]
    fn spatial_verification_rejects_a_local_color_edit() {
        use image::{DynamicImage, ImageBuffer, Rgb};

        let red_hat = DynamicImage::ImageRgb8(ImageBuffer::from_fn(512, 384, |x, y| {
            if (180..332).contains(&x) && (40..140).contains(&y) {
                Rgb([220, 35, 25])
            } else {
                Rgb([160, 160, 160])
            }
        }));
        let orange_hat = DynamicImage::ImageRgb8(ImageBuffer::from_fn(512, 384, |x, y| {
            if (180..332).contains(&x) && (40..140).contains(&y) {
                Rgb([220, 125, 25])
            } else {
                Rgb([160, 160, 160])
            }
        }));
        let red = spatial_descriptor(&encoded_png(&red_hat)).unwrap();
        let orange = spatial_descriptor(&encoded_png(&orange_hat)).unwrap();
        assert!(!spatially_consistent(spatial_comparison(&red, &orange)));
    }

    #[test]
    fn fast_spatial_score_tracks_the_reference_resolution() {
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageBuffer, Rgb};

        fn thumbnail(image: &DynamicImage) -> Vec<u8> {
            let resized = image.resize(512, 512, image::imageops::FilterType::Lanczos3);
            let mut bytes = Vec::new();
            JpegEncoder::new_with_quality(&mut bytes, 82)
                .encode_image(&resized)
                .unwrap();
            bytes
        }

        let base = DynamicImage::ImageRgb8(ImageBuffer::from_fn(768, 768, |x, y| {
            let band = ((x / 48 + y / 64) % 5) as u8;
            Rgb([
                35 + band * 31,
                70 + ((x * 3 + y) % 130) as u8,
                45 + ((x + y * 2) % 150) as u8,
            ])
        }));
        let mut edited = base.to_rgb8();
        for y in 280..390 {
            for x in 330..460 {
                edited.put_pixel(x, y, Rgb([218, 92, 35]));
            }
        }
        let edited = DynamicImage::ImageRgb8(edited);

        let candidates = [
            {
                let mut jpeg = Vec::new();
                JpegEncoder::new_with_quality(&mut jpeg, 72)
                    .encode_image(&base)
                    .unwrap();
                jpeg
            },
            encoded_png(&edited),
        ];
        let base_bytes = encoded_png(&base);
        let base_thumbnail = thumbnail(&base);
        for candidate in candidates {
            let decoded_candidate = image::load_from_memory(&candidate).unwrap();
            let candidate_thumbnail = thumbnail(&decoded_candidate);

            let reference_left = spatial_descriptor(&base_bytes).unwrap();
            let reference_right = spatial_descriptor(&candidate).unwrap();
            let fast_left = spatial_descriptor(&base_thumbnail).unwrap();
            let fast_right = spatial_descriptor(&candidate_thumbnail).unwrap();
            let reference = spatial_comparison(&reference_left, &reference_right);
            let fast = spatial_comparison(&fast_left, &fast_right);

            assert!(
                fast.difference_basis_points
                    .abs_diff(reference.difference_basis_points)
                    <= 100,
                "fast score {} bps diverged from reference {} bps",
                fast.difference_basis_points,
                reference.difference_basis_points
            );
        }
    }

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
    fn resolution_alone_does_not_prove_a_quality_upgrade() {
        let left = quality(1, 1_000_000, 4000, 4000, "image/jpeg");
        let right = quality(2, 800_000, 2000, 2000, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(8)),
            QualityDecision::NeedsChoice
        );
    }

    #[test]
    fn byte_size_breaks_an_otherwise_equivalent_encoding_tie() {
        let left = quality(11, 100_000, 1200, 800, "image/jpeg");
        let right = quality(12, 104_000, 1200, 800, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(1)),
            QualityDecision::RightBetter
        );
    }

    #[test]
    fn byte_size_breaks_an_exact_same_dimension_jpeg_tie() {
        let left = quality(1, 1_007_459, 1637, 2315, "image/jpeg");
        let right = quality(2, 818_954, 1637, 2315, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(0)),
            QualityDecision::LeftBetter
        );
    }

    #[test]
    fn resolution_and_size_together_still_do_not_prove_quality() {
        let left = quality(1, 379_479, 1214, 1720, "image/jpeg");
        let right = quality(2, 533_336, 1518, 2150, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(3)),
            QualityDecision::NeedsChoice
        );
    }

    #[test]
    fn larger_lossless_image_beats_smaller_lossy_image() {
        let left = quality(1, 213_700, 4096, 1067, "image/jpeg");
        let right = quality(2, 1_200_000, 4570, 1191, "image/png");
        assert_eq!(
            compare_quality(&left, &right, Some(1)),
            QualityDecision::RightBetter
        );
    }

    #[test]
    fn same_dimension_lossless_image_wins_despite_small_hash_variance() {
        let left = quality(1, 653_200, 2889, 4085, "image/jpeg");
        let right = quality(2, 7_900_000, 2889, 4085, "image/png");
        assert_eq!(
            compare_quality(&left, &right, Some(3)),
            QualityDecision::RightBetter
        );
    }

    #[test]
    fn larger_lossy_image_does_not_displace_smaller_lossless_image_without_more_evidence() {
        let left = quality(1, 1_200_000, 4570, 1191, "image/jpeg");
        let right = quality(2, 213_700, 4096, 1067, "image/png");
        assert_eq!(
            compare_quality(&left, &right, Some(1)),
            QualityDecision::NeedsChoice
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
            compare_quality_with_encoding(
                &left,
                &right,
                Some(1),
                EncodingClass::Lossy,
                EncodingClass::Lossy,
                Some(&finer),
                Some(&coarser),
            ),
            QualityDecision::LeftBetter
        );
    }

    #[test]
    fn encoded_mode_sniffers_distinguish_lossless_and_lossy_payloads() {
        assert_eq!(
            jpeg_encoding_class(&[0xff, 0xd8, 0xff, 0xc3, 0x00, 0x02]),
            EncodingClass::Lossless
        );
        assert_eq!(
            jpeg_encoding_class(&[0xff, 0xd8, 0xff, 0xc0, 0x00, 0x02]),
            EncodingClass::Lossy
        );

        let mut webp_lossless = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
        webp_lossless.extend([0; 8]);
        assert_eq!(webp_encoding_class(&webp_lossless), EncodingClass::Lossless);
        let mut webp_lossy = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp_lossy.extend([0; 8]);
        assert_eq!(webp_encoding_class(&webp_lossy), EncodingClass::Lossy);

        let tiff = |compression: u16| {
            let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
            bytes.extend(1_u16.to_le_bytes());
            bytes.extend(259_u16.to_le_bytes());
            bytes.extend(3_u16.to_le_bytes());
            bytes.extend(1_u32.to_le_bytes());
            bytes.extend(compression.to_le_bytes());
            bytes.extend([0, 0]);
            bytes
        };
        assert_eq!(tiff_encoding_class(&tiff(5)), EncodingClass::Lossless);
        assert_eq!(tiff_encoding_class(&tiff(7)), EncodingClass::Lossy);
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
    fn verified_same_dimension_lossless_quality_is_decisive() {
        let left = quality(1, 600_000, 1200, 800, "image/png");
        let right = quality(2, 550_000, 1200, 800, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(1)),
            QualityDecision::LeftBetter
        );
        assert_eq!(
            compare_quality(&left, &right, Some(10)),
            QualityDecision::LeftBetter
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
        let base = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 64]).unwrap();
        let mut one_bit = [0_u8; 64];
        one_bit[0] = 1;
        let mut far_bytes = [0_u8; 64];
        far_bytes[..32].fill(0xff);
        let far = ImageHash::<Vec<u8>>::from_bytes(&far_bytes).unwrap();
        let short = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 31]).unwrap();

        assert!(parse_supported_hash(&base.to_base64()).is_some());
        assert!(parse_supported_hash(&short.to_base64()).is_none());
        assert!(parse_supported_hash("not-a-base64-hash").is_none());

        let plan = candidate_plan(
            &[
                (10, base),
                (11, ImageHash::<Vec<u8>>::from_bytes(&one_bit).unwrap()),
                (12, far),
                (13, short),
            ],
            1,
        )
        .unwrap();
        assert_eq!(plan.groups.len(), 3);
        assert_eq!(plan.neighboring_groups, vec![(0, 1, 1)]);
    }

    #[test]
    fn identical_signature_groups_use_linear_star_edges() {
        let signature = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 64]).unwrap();
        let files = (1..=10_000)
            .map(|file_id| (file_id, signature.clone()))
            .collect::<Vec<_>>();

        let plan = candidate_plan(&files, 16).unwrap();

        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].file_ids.len(), files.len());
        assert!(plan.neighboring_groups.is_empty());
    }

    #[test]
    fn exact_hash_collisions_form_additional_visual_representatives() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let perceptual_hash = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 64])
            .unwrap()
            .to_base64();
        let hashes = [
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        ];
        app.store()
            .transaction(|transaction| {
                for (index, hash) in hashes.iter().enumerate() {
                    let item_id = index as i64 + 1;
                    let file_id = index as i64 + 10;
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
                             (file_id, file_hash, mime_type, size_bytes, pixel_width,
                              pixel_height, perceptual_hash, created_at)
                         VALUES (?1, ?2, 'image/png', 100, 64, 64, ?3, 'now')",
                        params![file_id, hash, perceptual_hash],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                         VALUES (?1, ?2, 'now', 'now')",
                        params![item_id, file_id],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        write_test_thumbnail_color(&app, hashes[0], [220, 20, 20]);
        write_test_thumbnail_color(&app, hashes[1], [20, 20, 220]);
        write_test_thumbnail_color(&app, hashes[2], [20, 20, 220]);

        let result = scan(&app, 16).unwrap();
        let candidates = list_candidates(&app, 10).unwrap();

        assert_eq!(result.candidate_count, 1);
        assert_eq!((candidates[0].file_id_a, candidates[0].file_id_b), (11, 12));
    }

    #[test]
    fn scan_verifies_collection_members_from_originals_without_thumbnails() {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let perceptual_hash = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 64])
            .unwrap()
            .to_base64();
        let hashes = [
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ];
        let original = encoded_png(&image::DynamicImage::ImageRgb8(
            image::ImageBuffer::from_pixel(64, 64, image::Rgb([80, 120, 160])),
        ));
        for hash in hashes {
            app.blobs()
                .write_original(hash, &original, Some("png"))
                .unwrap();
        }
        app.store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item
                         (item_id, item_key, kind, cover_media_item_id, created_at, updated_at)
                     VALUES (1, 'member-a', 'media', NULL, 'now', 'now'),
                            (2, 'member-b', 'media', NULL, 'now', 'now'),
                            (101, 'collection-a', 'collection', 1, 'now', 'now'),
                            (102, 'collection-b', 'collection', 2, 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (101, 'active'), (102, 'active')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, pixel_width,
                          pixel_height, perceptual_hash, created_at)
                     VALUES (11, ?1, 'image/png', ?3, 64, 64, ?4, 'now'),
                            (12, ?2, 'image/png', ?3, 64, 64, ?4, 'now')",
                    params![hashes[0], hashes[1], original.len() as i64, perceptual_hash],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (1, 11, 'now', 'now'), (2, 12, 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (101, 1, 1024), (102, 2, 1024)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(app.blobs().read_thumbnail(hashes[0]).unwrap().is_none());
        assert!(app.blobs().read_thumbnail(hashes[1]).unwrap().is_none());
        assert_eq!(scan(&app, 0).unwrap().candidate_count, 1);
    }

    #[test]
    fn dense_unique_signature_neighborhoods_stop_before_quadratic_expansion() {
        let mut files = Vec::new();
        let mut file_id = 1_i64;
        'outer: for first in 0..33 {
            for second in (first + 1)..33 {
                let mut bytes = [0_u8; 64];
                bytes[first / 8] |= 1 << (7 - first % 8);
                bytes[second / 8] |= 1 << (7 - second % 8);
                files.push((file_id, ImageHash::<Vec<u8>>::from_bytes(&bytes).unwrap()));
                file_id += 1;
                if files.len() == super::MAX_NEAR_SIGNATURE_GROUPS + 2 {
                    break 'outer;
                }
            }
        }

        let error = candidate_plan(&files, 4).err().unwrap();

        assert!(error.to_string().contains("duplicate scan paused"));
        assert!(error.to_string().contains("distinct signatures"));
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
        let hash_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let hash_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let perceptual_hash = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 64])
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
                     VALUES (10, ?1, 'image/jpeg', 1000, 2000, 2000, ?3, 'now'),
                            (11, ?2, 'image/jpeg', 1000, 2000, 2000, ?3, 'now')",
                    params![hash_a, hash_b, perceptual_hash],
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
        write_test_thumbnail(&app, hash_a);
        write_test_thumbnail(&app, hash_b);

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
        let pending_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let visible_hash = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let perceptual_hash = ImageHash::<Vec<u8>>::from_bytes(&[0_u8; 64])
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
                     VALUES (10, ?1, 'image/jpeg', 1000, 1000, 1000, ?3, 'now'),
                            (11, ?2, 'image/jpeg', 1000, 1000, 1000, ?3, 'now')",
                    params![pending_hash, visible_hash, perceptual_hash],
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
        write_test_thumbnail(&app, pending_hash);
        write_test_thumbnail(&app, visible_hash);

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
