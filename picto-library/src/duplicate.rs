use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::model::{
    DuplicateCandidate, DuplicateCandidateSide, DuplicateFile, DuplicateOccurrence, DuplicatePair,
    DuplicateQualityDecision, DuplicateResolutionChoice, DuplicateStatus, FileId, Lifecycle,
    MediaId, PendingBlobCleanup, RootId,
};
use crate::projection::ProjectionSnapshot;
use crate::{LibraryError, Result};

const STATUS_DETECTED: u8 = 1;
const STATUS_NOT_DUPLICATE: u8 = 2;
const STATUS_RESOLVED: u8 = 3;

#[derive(Debug, Clone)]
pub struct DuplicateHistoryState {
    file_id_a: FileId,
    file_id_b: FileId,
    after_status: DuplicateStatus,
    decided_at_ms: i64,
    winner_file_id: Option<FileId>,
    loser_file_id: Option<FileId>,
    rewired_media_ids: Arc<Vec<MediaId>>,
    pair_roots: Arc<RoaringBitmap>,
}

impl DuplicateHistoryState {
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.rewired_media_ids.len() * std::mem::size_of::<MediaId>()
            + self.pair_roots.serialized_size()
            + 96
    }

    pub(crate) fn protected_file_id(&self) -> Option<FileId> {
        self.loser_file_id
    }

    pub(crate) fn file_id_a(&self) -> FileId {
        self.file_id_a
    }

    pub(crate) fn file_id_b(&self) -> FileId {
        self.file_id_b
    }

    pub(crate) fn rewires_file(&self) -> bool {
        self.loser_file_id.is_some()
    }
}

pub(crate) struct ResolutionOutput {
    pub snapshot: ProjectionSnapshot,
    pub affected_roots: RoaringBitmap,
    pub history: DuplicateHistoryState,
}

pub(crate) fn record_detected(
    transaction: &Transaction<'_>,
    file_id_a: FileId,
    file_id_b: FileId,
    distance: u32,
    detected_at_ms: i64,
) -> Result<Option<DuplicatePair>> {
    let (file_id_a, file_id_b) = normalize_pair(file_id_a, file_id_b)?;
    require_file(transaction, file_id_a)?;
    require_file(transaction, file_id_b)?;

    if let Some(pair) = load_pair(transaction, file_id_a, file_id_b)? {
        if pair.status != DuplicateStatus::Detected || pair.distance == distance {
            return Ok(None);
        }
        transaction.execute(
            "UPDATE duplicate_pair SET distance = ?3
             WHERE file_id_a = ?1 AND file_id_b = ?2",
            params![file_id_a.0, file_id_b.0, distance],
        )?;
    } else {
        transaction.execute(
            "INSERT INTO duplicate_pair
                 (file_id_a, file_id_b, distance, status, detected_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                file_id_a.0,
                file_id_b.0,
                distance,
                STATUS_DETECTED,
                detected_at_ms
            ],
        )?;
    }
    load_pair(transaction, file_id_a, file_id_b)
}

pub(crate) fn list_pairs(
    connection: &Connection,
    status: Option<DuplicateStatus>,
    limit: usize,
) -> Result<Vec<DuplicatePair>> {
    let limit = limit.clamp(1, 500) as i64;
    let sql = if status.is_some() {
        "SELECT file_id_a, file_id_b, distance, status, detected_at_ms,
                decided_at_ms, winner_file_id
         FROM duplicate_pair WHERE status = ?1
         ORDER BY distance, file_id_a, file_id_b LIMIT ?2"
    } else {
        "SELECT file_id_a, file_id_b, distance, status, detected_at_ms,
                decided_at_ms, winner_file_id
         FROM duplicate_pair
         ORDER BY status, distance, file_id_a, file_id_b LIMIT ?1"
    };
    let mut statement = connection.prepare(sql)?;
    let mut rows = if let Some(status) = status {
        statement.query(params![encode_status(status), limit])?
    } else {
        statement.query([limit])?
    };
    let mut pairs = Vec::new();
    while let Some(row) = rows.next()? {
        pairs.push(pair_from_row(row)?);
    }
    Ok(pairs)
}

pub(crate) fn list_candidates(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    limit: usize,
) -> Result<Vec<DuplicateCandidate>> {
    let pairs = list_pairs(connection, Some(DuplicateStatus::Detected), 500)?;
    let mut candidates = Vec::with_capacity(limit.min(pairs.len()));
    for pair in pairs {
        if candidates.len() == limit.clamp(1, 500) {
            break;
        }
        let Some(left) = candidate_side(connection, snapshot, pair.file_id_a)? else {
            continue;
        };
        let Some(right) = candidate_side(connection, snapshot, pair.file_id_b)? else {
            continue;
        };
        candidates.push(DuplicateCandidate {
            file_id_a: pair.file_id_a,
            file_id_b: pair.file_id_b,
            distance: pair.distance,
            decision: compare_candidate_quality(&left.file, &right.file, pair.distance),
            left,
            right,
        });
    }
    Ok(candidates)
}

pub(crate) fn count_visible_candidates(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
) -> Result<u64> {
    let mut statement = connection.prepare(
        "SELECT file_id_a, file_id_b FROM duplicate_pair
         WHERE status = ?1 ORDER BY file_id_a, file_id_b",
    )?;
    let pairs = statement.query_map([STATUS_DETECTED], |row| {
        Ok((FileId(row.get(0)?), FileId(row.get(1)?)))
    })?;
    let mut count = 0_u64;
    for pair in pairs {
        let (left, right) = pair?;
        if file_has_active_occurrence(connection, snapshot, left)?
            && file_has_active_occurrence(connection, snapshot, right)?
        {
            count += 1;
        }
    }
    Ok(count)
}

fn file_has_active_occurrence(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    file_id: FileId,
) -> Result<bool> {
    let active = snapshot.lifecycle(Lifecycle::Active);
    let mut statement =
        connection.prepare_cached("SELECT media_id FROM media_item WHERE file_id = ?1")?;
    let rows = statement.query_map([file_id.0], |row| row.get::<_, u32>(0))?;
    for media_id in rows {
        let media_id = media_id?;
        if snapshot
            .media_owner
            .get(media_id)
            .is_some_and(|root_id| active.contains(root_id.0))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn candidate_side(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    file_id: FileId,
) -> Result<Option<DuplicateCandidateSide>> {
    let file = connection.query_row(
        "SELECT file_id, content_hash, mime, size_bytes, width, height, frame_count
         FROM media_file WHERE file_id = ?1",
        [file_id.0],
        |row| {
            Ok(DuplicateFile {
                file_id: FileId(row.get(0)?),
                file_hash: row.get(1)?,
                mime_type: row.get(2)?,
                size_bytes: row.get(3)?,
                pixel_width: row.get(4)?,
                pixel_height: row.get(5)?,
                frame_count: row.get(6)?,
            })
        },
    )?;
    let active = snapshot.lifecycle(Lifecycle::Active);
    let mut statement = connection.prepare_cached(
        "SELECT media_id FROM media_item WHERE file_id = ?1 ORDER BY media_id",
    )?;
    let mut occurrences = Vec::new();
    let rows = statement.query_map([file_id.0], |row| row.get::<_, u32>(0))?;
    for media_id in rows {
        let media_id = MediaId(media_id?);
        let Some(root_id) = snapshot.media_owner.get(media_id.0).copied() else {
            continue;
        };
        if !active.contains(root_id.0) {
            continue;
        }
        occurrences.push(DuplicateOccurrence {
            media_item_id: media_id,
            root_item_id: root_id,
            collection_id: (root_id.0 != media_id.0).then_some(root_id),
        });
    }
    Ok((!occurrences.is_empty()).then_some(DuplicateCandidateSide { file, occurrences }))
}

fn compare_candidate_quality(
    left: &DuplicateFile,
    right: &DuplicateFile,
    distance: u32,
) -> DuplicateQualityDecision {
    let stable_tie = || {
        if left.file_id.0 <= right.file_id.0 {
            DuplicateQualityDecision::AutoTieLeft
        } else {
            DuplicateQualityDecision::AutoTieRight
        }
    };
    if left.file_hash == right.file_hash {
        return stable_tie();
    }
    let dimensions = left
        .pixel_width
        .zip(left.pixel_height)
        .zip(right.pixel_width.zip(right.pixel_height));
    if let Some(((left_width, left_height), (right_width, right_height))) = dimensions {
        if distance <= 1 {
            if left_width >= right_width
                && left_height >= right_height
                && (left_width > right_width || left_height > right_height)
            {
                return DuplicateQualityDecision::LeftBetter;
            }
            if right_width >= left_width
                && right_height >= left_height
                && (right_width > left_width || right_height > left_height)
            {
                return DuplicateQualityDecision::RightBetter;
            }
        }
        if left_width == right_width
            && left_height == right_height
            && left.mime_type == right.mime_type
            && left.frame_count == right.frame_count
            && distance <= 1
        {
            return match left.size_bytes.cmp(&right.size_bytes) {
                std::cmp::Ordering::Greater => DuplicateQualityDecision::LeftBetter,
                std::cmp::Ordering::Less => DuplicateQualityDecision::RightBetter,
                std::cmp::Ordering::Equal => stable_tie(),
            };
        }
    }
    DuplicateQualityDecision::NeedsChoice
}

pub(crate) fn affected_roots(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    file_id_a: FileId,
    file_id_b: FileId,
) -> Result<RoaringBitmap> {
    let (file_id_a, file_id_b) = normalize_pair(file_id_a, file_id_b)?;
    Ok(roots_for_media(
        snapshot,
        file_media_ids(connection, file_id_a)?
            .into_iter()
            .chain(file_media_ids(connection, file_id_b)?),
    ))
}

pub(crate) fn resolve(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    file_id_a: FileId,
    file_id_b: FileId,
    choice: DuplicateResolutionChoice,
    decided_at_ms: i64,
) -> Result<ResolutionOutput> {
    let (file_id_a, file_id_b) = normalize_pair(file_id_a, file_id_b)?;
    let pair = load_pair(transaction, file_id_a, file_id_b)?
        .ok_or_else(|| LibraryError::NotFound("duplicate pair".into()))?;
    if pair.status != DuplicateStatus::Detected {
        return Err(LibraryError::InvalidState(format!(
            "duplicate pair is already {}",
            status_name(pair.status)
        )));
    }

    let pair_media_ids = file_media_ids(transaction, file_id_a)?
        .into_iter()
        .chain(file_media_ids(transaction, file_id_b)?)
        .collect::<BTreeSet<_>>();
    let pair_roots = roots_for_media(&snapshot, pair_media_ids.iter().copied());
    let (after_status, winner_file_id, loser_file_id, rewired_media_ids) = match choice {
        DuplicateResolutionChoice::KeepBoth => {
            transaction.execute(
                "UPDATE duplicate_pair
                 SET status = ?3, decided_at_ms = ?4, winner_file_id = NULL
                 WHERE file_id_a = ?1 AND file_id_b = ?2",
                params![
                    file_id_a.0,
                    file_id_b.0,
                    STATUS_NOT_DUPLICATE,
                    decided_at_ms
                ],
            )?;
            (DuplicateStatus::NotDuplicate, None, None, Vec::new())
        }
        DuplicateResolutionChoice::KeepFile { winner_file_id } => {
            if winner_file_id != file_id_a && winner_file_id != file_id_b {
                return Err(LibraryError::InvalidInput(
                    "winner must be one of the duplicate files".into(),
                ));
            }
            let loser_file_id = if winner_file_id == file_id_a {
                file_id_b
            } else {
                file_id_a
            };
            let rewired_media_ids = file_media_ids(transaction, loser_file_id)?;
            transaction.execute(
                "UPDATE media_item SET file_id = ?1 WHERE file_id = ?2",
                params![winner_file_id.0, loser_file_id.0],
            )?;
            transaction.execute(
                "UPDATE duplicate_pair
                 SET status = ?3, decided_at_ms = ?4, winner_file_id = ?5
                 WHERE file_id_a = ?1 AND file_id_b = ?2",
                params![
                    file_id_a.0,
                    file_id_b.0,
                    STATUS_RESOLVED,
                    decided_at_ms,
                    winner_file_id.0
                ],
            )?;
            enqueue_cleanup(transaction, loser_file_id, revision)?;
            settle_rewired_media(transaction, &mut snapshot, &rewired_media_ids)?;
            (
                DuplicateStatus::Resolved,
                Some(winner_file_id),
                Some(loser_file_id),
                rewired_media_ids,
            )
        }
    };

    Ok(ResolutionOutput {
        snapshot,
        affected_roots: pair_roots.clone(),
        history: DuplicateHistoryState {
            file_id_a,
            file_id_b,
            after_status,
            decided_at_ms,
            winner_file_id,
            loser_file_id,
            rewired_media_ids: Arc::new(rewired_media_ids),
            pair_roots: Arc::new(pair_roots),
        },
    })
}

pub(crate) fn replay(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    state: &DuplicateHistoryState,
    use_after: bool,
) -> Result<RoaringBitmap> {
    let expected_status = if use_after {
        DuplicateStatus::Detected
    } else {
        state.after_status
    };
    let pair = load_pair(transaction, state.file_id_a, state.file_id_b)?
        .ok_or_else(|| LibraryError::NotFound("duplicate pair".into()))?;
    let expected_decided_at_ms = (!use_after).then_some(state.decided_at_ms);
    let expected_winner_file_id = (!use_after).then_some(state.winner_file_id).flatten();
    if pair.status != expected_status
        || pair.decided_at_ms != expected_decided_at_ms
        || pair.winner_file_id != expected_winner_file_id
    {
        return Err(LibraryError::InvalidState(
            "cannot replay history because duplicate pair changed".into(),
        ));
    }

    if let (Some(winner_file_id), Some(loser_file_id)) = (state.winner_file_id, state.loser_file_id)
    {
        let (expected_file_id, replacement_file_id) = if use_after {
            (loser_file_id, winner_file_id)
        } else {
            (winner_file_id, loser_file_id)
        };
        rewire_exact_media(
            transaction,
            &state.rewired_media_ids,
            expected_file_id,
            replacement_file_id,
        )?;
        if use_after {
            enqueue_cleanup(transaction, loser_file_id, revision)?;
        }
        settle_rewired_media(transaction, snapshot, &state.rewired_media_ids)?;
    }

    if use_after {
        transaction.execute(
            "UPDATE duplicate_pair
             SET status = ?3, decided_at_ms = ?4, winner_file_id = ?5
             WHERE file_id_a = ?1 AND file_id_b = ?2",
            params![
                state.file_id_a.0,
                state.file_id_b.0,
                encode_status(state.after_status),
                state.decided_at_ms,
                state.winner_file_id.map(|file_id| file_id.0)
            ],
        )?;
    } else {
        transaction.execute(
            "UPDATE duplicate_pair
             SET status = ?3, decided_at_ms = NULL, winner_file_id = NULL
             WHERE file_id_a = ?1 AND file_id_b = ?2",
            params![state.file_id_a.0, state.file_id_b.0, STATUS_DETECTED],
        )?;
    }
    Ok(state.pair_roots.as_ref().clone())
}

pub(crate) fn ready_cleanup(
    connection: &Connection,
    protected: &BTreeSet<FileId>,
    limit: usize,
) -> Result<Vec<PendingBlobCleanup>> {
    let mut statement = connection.prepare(
        "SELECT queue.file_id, file.content_hash, queue.file_path
         FROM blob_cleanup_queue queue
         JOIN media_file file ON file.file_id = queue.file_id
         WHERE NOT EXISTS(
             SELECT 1 FROM media_item media WHERE media.file_id = queue.file_id
         )
         ORDER BY queue.enqueued_revision, queue.file_id",
    )?;
    let mut rows = statement.query([])?;
    let mut cleanup = Vec::new();
    while cleanup.len() < limit {
        let Some(row) = rows.next()? else {
            break;
        };
        let file_id = FileId(row.get(0)?);
        if !protected.contains(&file_id) {
            cleanup.push(PendingBlobCleanup {
                file_id,
                content_hash: row.get(1)?,
                file_path: row.get(2)?,
            });
        }
    }
    Ok(cleanup)
}

fn settle_rewired_media(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    media_ids: &[MediaId],
) -> Result<()> {
    if media_ids.is_empty() {
        return Ok(());
    }
    let affected_roots = roots_for_media(snapshot, media_ids.iter().copied());
    let mime_by_media = load_media_mimes(transaction, media_ids)?;
    let image_media = Arc::make_mut(&mut snapshot.image_media);
    for media_id in media_ids {
        if mime_by_media
            .get(media_id)
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            image_media.insert(media_id.0);
        } else {
            image_media.remove(media_id.0);
        }
    }

    for root_id in affected_roots.iter().map(RootId) {
        refresh_root_size(transaction, snapshot, root_id)?;
        crate::group::refresh_root_mime_projection(transaction, snapshot, root_id)?;
        crate::group::refresh_cover_projection(transaction, snapshot, root_id)?;
        let has_image = snapshot.collection_orders.get(&root_id).map_or_else(
            || snapshot.image_media.contains(root_id.0),
            |members| {
                members
                    .iter()
                    .any(|media_id| snapshot.image_media.contains(media_id.0))
            },
        );
        if has_image {
            Arc::make_mut(&mut snapshot.roots_with_images).insert(root_id.0);
        } else {
            Arc::make_mut(&mut snapshot.roots_with_images).remove(root_id.0);
        }
    }
    Ok(())
}

fn refresh_root_size(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    root_id: RootId,
) -> Result<()> {
    let media_ids = snapshot.collection_orders.get(&root_id).map_or_else(
        || vec![MediaId(root_id.0)],
        |members| members.as_ref().clone(),
    );
    let mut total = 0u64;
    for chunk in media_ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT COALESCE(SUM(file.size_bytes), 0)
             FROM media_item media
             JOIN media_file file ON file.file_id = media.file_id
             WHERE media.media_id IN ({placeholders})"
        );
        let values = chunk.iter().map(|media_id| media_id.0).collect::<Vec<_>>();
        let chunk_total =
            transaction.query_row(&sql, rusqlite::params_from_iter(values), |row| {
                row.get::<_, i64>(0)
            })?;
        total = total
            .checked_add(
                u64::try_from(chunk_total)
                    .map_err(|_| LibraryError::InvalidState("root size became negative".into()))?,
            )
            .ok_or_else(|| LibraryError::InvalidState("root size overflow".into()))?;
    }
    transaction.execute(
        "UPDATE library_root SET total_size_bytes = ?2 WHERE root_id = ?1",
        params![
            root_id.0,
            i64::try_from(total)
                .map_err(|_| LibraryError::InvalidState("root size exceeds SQLite range".into()))?
        ],
    )?;
    Arc::make_mut(&mut snapshot.total_bytes).insert(root_id.0, total);
    Ok(())
}

fn load_media_mimes(
    transaction: &Transaction<'_>,
    media_ids: &[MediaId],
) -> Result<HashMap<MediaId, String>> {
    let mut values = HashMap::with_capacity(media_ids.len());
    for chunk in media_ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT media.media_id, file.mime
             FROM media_item media
             JOIN media_file file ON file.file_id = media.file_id
             WHERE media.media_id IN ({placeholders})"
        );
        let ids = chunk.iter().map(|media_id| media_id.0).collect::<Vec<_>>();
        let rows = transaction
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(ids), |row| {
                Ok((MediaId(row.get(0)?), row.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        values.extend(rows);
    }
    if values.len() != media_ids.len() {
        return Err(LibraryError::InvalidState(
            "duplicate resolution references missing media".into(),
        ));
    }
    Ok(values)
}

fn rewire_exact_media(
    transaction: &Transaction<'_>,
    media_ids: &[MediaId],
    expected_file_id: FileId,
    replacement_file_id: FileId,
) -> Result<()> {
    require_file(transaction, replacement_file_id)?;
    let mut changed = 0usize;
    for chunk in media_ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE media_item SET file_id = ?1
             WHERE file_id = ?2 AND media_id IN ({placeholders})"
        );
        let values = [replacement_file_id.0, expected_file_id.0]
            .into_iter()
            .chain(chunk.iter().map(|media_id| media_id.0));
        changed += transaction.execute(&sql, rusqlite::params_from_iter(values))?;
    }
    if changed != media_ids.len() {
        return Err(LibraryError::InvalidState(
            "cannot replay history because duplicate occurrences changed".into(),
        ));
    }
    Ok(())
}

fn enqueue_cleanup(transaction: &Transaction<'_>, file_id: FileId, revision: u64) -> Result<()> {
    transaction.execute(
        "INSERT INTO blob_cleanup_queue(file_id, file_path, enqueued_revision)
         SELECT file_id, file_path, ?2
         FROM media_file WHERE file_id = ?1
         ON CONFLICT(file_id) DO NOTHING",
        params![file_id.0, revision as i64],
    )?;
    Ok(())
}

fn load_pair(
    connection: &Connection,
    file_id_a: FileId,
    file_id_b: FileId,
) -> Result<Option<DuplicatePair>> {
    connection
        .query_row(
            "SELECT file_id_a, file_id_b, distance, status, detected_at_ms,
                    decided_at_ms, winner_file_id
             FROM duplicate_pair WHERE file_id_a = ?1 AND file_id_b = ?2",
            params![file_id_a.0, file_id_b.0],
            pair_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn pair_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DuplicatePair> {
    let status = decode_status(row.get(3)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    Ok(DuplicatePair {
        file_id_a: FileId(row.get(0)?),
        file_id_b: FileId(row.get(1)?),
        distance: row.get(2)?,
        status,
        detected_at_ms: row.get(4)?,
        decided_at_ms: row.get(5)?,
        winner_file_id: row.get::<_, Option<u32>>(6)?.map(FileId),
    })
}

fn normalize_pair(file_id_a: FileId, file_id_b: FileId) -> Result<(FileId, FileId)> {
    if file_id_a == file_id_b {
        return Err(LibraryError::InvalidInput(
            "duplicate pair requires two different files".into(),
        ));
    }
    Ok(if file_id_a < file_id_b {
        (file_id_a, file_id_b)
    } else {
        (file_id_b, file_id_a)
    })
}

fn require_file(connection: &Connection, file_id: FileId) -> Result<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM media_file WHERE file_id = ?1)",
        [file_id.0],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(LibraryError::NotFound(format!("file {file_id}")))
    }
}

fn file_media_ids(connection: &Connection, file_id: FileId) -> Result<Vec<MediaId>> {
    let mut statement = connection
        .prepare("SELECT media_id FROM media_item WHERE file_id = ?1 ORDER BY media_id")?;
    let values = statement
        .query_map([file_id.0], |row| row.get::<_, u32>(0).map(MediaId))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(values)
}

fn roots_for_media(
    snapshot: &ProjectionSnapshot,
    media_ids: impl IntoIterator<Item = MediaId>,
) -> RoaringBitmap {
    media_ids
        .into_iter()
        .filter_map(|media_id| snapshot.media_owner.get(media_id.0))
        .map(|root_id| root_id.0)
        .collect()
}

fn encode_status(status: DuplicateStatus) -> u8 {
    match status {
        DuplicateStatus::Detected => STATUS_DETECTED,
        DuplicateStatus::NotDuplicate => STATUS_NOT_DUPLICATE,
        DuplicateStatus::Resolved => STATUS_RESOLVED,
    }
}

fn decode_status(status: u8) -> std::result::Result<DuplicateStatus, LibraryError> {
    match status {
        STATUS_DETECTED => Ok(DuplicateStatus::Detected),
        STATUS_NOT_DUPLICATE => Ok(DuplicateStatus::NotDuplicate),
        STATUS_RESOLVED => Ok(DuplicateStatus::Resolved),
        value => Err(LibraryError::InvalidState(format!(
            "unknown duplicate status {value}"
        ))),
    }
}

fn status_name(status: DuplicateStatus) -> &'static str {
    match status {
        DuplicateStatus::Detected => "detected",
        DuplicateStatus::NotDuplicate => "not duplicate",
        DuplicateStatus::Resolved => "resolved",
    }
}
