//! Duplicate detection and physical-file resolution for the replacement backend.
//!
//! Duplicate review is deliberately file-level. A physical blob can be used by
//! several logical media items, so choosing a better blob must not delete or
//! merge those items. Resolution only changes `media_asset.file_id` references
//! and removes a blob after it is no longer referenced.

use std::collections::{BTreeSet, HashMap};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::app::{resources, Application, FileHash, ItemId, MutationReceipt};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileQuality {
    pub file_id: i64,
    pub file_hash: FileHash,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    let left_pixels = left.pixel_count();
    let right_pixels = right.pixel_count();

    // A real two-times pixel-count advantage is decisive. This is intentionally
    // checked before byte density: a larger source is not discarded just
    // because it compresses better.
    if let (Some(left_pixels), Some(right_pixels)) = (left_pixels, right_pixels) {
        if left_pixels >= right_pixels.saturating_mul(2) {
            return QualityDecision::LeftBetter;
        }
        if right_pixels >= left_pixels.saturating_mul(2) {
            return QualityDecision::RightBetter;
        }
    }

    // When decoded information is available, prefer the materially richer
    // decoded result before considering encoded size.
    if let (Some(left_information), Some(right_information)) =
        (left.decoded_information, right.decoded_information)
    {
        if left_information >= right_information * 1.20 {
            return QualityDecision::LeftBetter;
        }
        if right_information >= left_information * 1.20 {
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

    // Lossless encoding wins only when the decoded dimensions are comparable.
    // A tiny thumbnail should not beat a full-size lossy image merely because
    // its format is lossless.
    if let (Some(left_pixels), Some(right_pixels)) = (left_pixels, right_pixels) {
        let comparable_dimensions = left_pixels.saturating_mul(10)
            >= right_pixels.saturating_mul(9)
            && right_pixels.saturating_mul(10) >= left_pixels.saturating_mul(9);
        if comparable_dimensions && left.is_lossless() != right.is_lossless() {
            return if left.is_lossless() {
                QualityDecision::LeftBetter
            } else {
                QualityDecision::RightBetter
            };
        }
    }

    // Same-size, same-format candidates with a close pHash are usually the
    // same encoded image at different compression settings. Keep the larger
    // representation when it is materially larger; a near tie is resolved by
    // stable file id rather than making the user review noise.
    let close_hash = distance.is_some_and(|value| value <= 1);
    if close_hash && left.mime_type == right.mime_type {
        if left.size_bytes >= right.size_bytes.saturating_mul(5) / 4 {
            return QualityDecision::LeftBetter;
        }
        if right.size_bytes >= left.size_bytes.saturating_mul(5) / 4 {
            return QualityDecision::RightBetter;
        }

        let same_dimensions =
            left.pixel_width == right.pixel_width && left.pixel_height == right.pixel_height;
        let sizes_are_negligible = left.size_bytes.max(1) * 100 <= right.size_bytes.max(1) * 105
            && right.size_bytes.max(1) * 100 <= left.size_bytes.max(1) * 105;
        if same_dimensions && sizes_are_negligible {
            return if left.file_id <= right.file_id {
                QualityDecision::AutoTieLeft
            } else {
                QualityDecision::AutoTieRight
            };
        }
    }

    QualityDecision::NeedsChoice
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicateCandidate {
    pub file_id_a: i64,
    pub file_id_b: i64,
    pub distance: u32,
    pub left: CandidateSide,
    pub right: CandidateSide,
    pub decision: QualityDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateSide {
    pub file: FileQuality,
    pub item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateScanResult {
    pub candidate_count: usize,
    pub affected_item_ids: Vec<ItemId>,
    pub receipt: MutationReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionChoice {
    KeepBoth,
    KeepFile { winner_file_id: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
                    crate::duplicates::phash::parse_supported_hash(file.perceptual_hash.as_deref()?)
                        .map(|hash| (file.file_id, hash))
                })
                .collect::<Vec<_>>();
            let pairs = crate::duplicates::phash::find_candidate_pairs(&parsed, distance_threshold);

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
                let left_file = by_id
                    .get(&file_id_a)
                    .ok_or_else(|| invalid(format!("Duplicate file {file_id_a} disappeared")))?;
                let right_file = by_id
                    .get(&file_id_b)
                    .ok_or_else(|| invalid(format!("Duplicate file {file_id_b} disappeared")))?;
                let left_items = item_ids_for_file(transaction, file_id_a)?;
                let right_items = item_ids_for_file(transaction, file_id_b)?;
                affected_item_ids.extend(left_items.iter().map(|item_id| item_id.0));
                affected_item_ids.extend(right_items.iter().map(|item_id| item_id.0));
                candidates.push(DuplicateCandidate {
                    file_id_a,
                    file_id_b,
                    distance,
                    decision: compare_quality(
                        &left_file.quality(),
                        &right_file.quality(),
                        Some(distance),
                    ),
                    left: CandidateSide {
                        file: left_file.quality(),
                        item_ids: left_items,
                    },
                    right: CandidateSide {
                        file: right_file.quality(),
                        item_ids: right_items,
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
        receipt: receipt(revision, affected_item_ids.clone(), false),
        affected_item_ids,
    })
}

pub fn list_candidates(app: &Application, limit: i64) -> Result<Vec<DuplicateCandidate>, String> {
    let limit = limit.clamp(1, 500);
    app.store().read(|connection| {
        let mut statement = connection.prepare(
            "SELECT file_id_a, file_id_b, distance
             FROM duplicate WHERE status = 'detected'
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
                candidate_for_pair(connection, file_id_a, file_id_b, distance)
            })
            .collect()
    })
}

/// Resolve one file-level pair. Keeping one file never collapses logical
/// media items: all affected `media_asset` rows remain and keep their own
/// metadata, tags, folders, roots, and source provenance.
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
    let ((affected_item_ids, freed_file_hash), revision) = app.transaction(
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
                    collect_pair_items(transaction, file_id_a, file_id_b, &mut affected)?;
                    transaction.execute(
                        "UPDATE duplicate
                         SET status = 'not_duplicate', decided_at = ?3, winner_file_id = NULL
                         WHERE file_id_a = ?1 AND file_id_b = ?2",
                        params![file_id_a, file_id_b, Utc::now().to_rfc3339()],
                    )?;
                    Ok((
                        (affected.into_iter().map(ItemId).collect::<Vec<_>>(), None),
                        (),
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
                    collect_pair_items(transaction, file_id_a, file_id_b, &mut affected)?;

                    transaction.execute(
                        "UPDATE media_asset SET file_id = ?1 WHERE file_id = ?2",
                        params![winner_file_id, loser_file_id],
                    )?;
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
                        ),
                        (),
                    ))
                }
            }
        },
        |_projections, _| Ok(()),
    )?;

    let changes_library = matches!(choice, ResolutionChoice::KeepFile { .. });
    Ok(ResolutionResult {
        choice,
        affected_item_ids: affected_item_ids.clone(),
        freed_file_hash,
        receipt: receipt(revision, affected_item_ids, changes_library),
    })
}

pub fn resolve_automatically(
    app: &Application,
    candidate: &DuplicateCandidate,
) -> Result<ResolutionResult, String> {
    let winner_file_id = candidate
        .decision
        .winner(candidate.file_id_a, candidate.file_id_b)
        .ok_or_else(|| "Duplicate quality is ambiguous; a user choice is required".to_string())?;
    resolve(
        app,
        candidate.file_id_a,
        candidate.file_id_b,
        ResolutionChoice::KeepFile { winner_file_id },
    )
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

fn item_ids_for_file(connection: &Connection, file_id: i64) -> rusqlite::Result<Vec<ItemId>> {
    let mut statement = connection
        .prepare("SELECT item_id FROM media_asset WHERE file_id = ?1 ORDER BY item_id")?;
    let values = statement
        .query_map([file_id], |row| Ok(ItemId(row.get(0)?)))?
        .collect();
    values
}

fn candidate_for_pair(
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
        decision: compare_quality(&left, &right, Some(distance)),
        left: CandidateSide {
            file: left,
            item_ids: item_ids_for_file(connection, file_id_a)?,
        },
        right: CandidateSide {
            file: right,
            item_ids: item_ids_for_file(connection, file_id_b)?,
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

fn collect_pair_items(
    transaction: &Transaction<'_>,
    file_id_a: i64,
    file_id_b: i64,
    affected: &mut BTreeSet<i64>,
) -> rusqlite::Result<()> {
    let mut statement =
        transaction.prepare("SELECT item_id FROM media_asset WHERE file_id IN (?1, ?2)")?;
    let rows = statement.query_map(params![file_id_a, file_id_b], |row| row.get::<_, i64>(0))?;
    for row in rows {
        affected.insert(row?);
    }
    Ok(())
}

fn receipt(revision: u64, item_ids: Vec<ItemId>, changes_library: bool) -> MutationReceipt {
    let mut resources = vec![resources::DUPLICATES.to_string()];
    if changes_library {
        resources.push(resources::LIBRARY.to_string());
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
        compare_quality, list_candidates, resolve, scan, FileQuality, QualityDecision,
        ResolutionChoice,
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
    fn negligible_encoded_tie_is_stable() {
        let left = quality(11, 100_000, 1200, 800, "image/jpeg");
        let right = quality(12, 104_000, 1200, 800, "image/jpeg");
        assert_eq!(
            compare_quality(&left, &right, Some(0)),
            QualityDecision::AutoTieLeft
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
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (1, 10, 'now', 'now'), (2, 11, 'now', 'now'),
                            (3, 10, 'now', 'now')",
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
        assert_eq!(
            candidates[0].left.item_ids,
            vec![crate::app::ItemId(1), crate::app::ItemId(3)]
        );
        assert_eq!(candidates[0].right.item_ids, vec![crate::app::ItemId(2)]);
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
