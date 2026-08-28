use chrono::{DateTime, Duration, Utc};
use rusqlite::params;

use crate::database::WorkPriority;
use crate::model::{ClaimedMediaWork, FileId, MediaId, MediaWorkKind, RootId};
use crate::publication::MutationReceipt;
use crate::{Library, LibraryError, Result};

const TASKS_RESOURCE: &str = "tasks";
const MAX_ATTEMPTS: u32 = 8;

impl Library {
    pub fn reset_running_media_work(&self, now: &str) -> Result<Option<MutationReceipt>> {
        let Some(((), receipt)) = self.auxiliary_write_if_changed(
            WorkPriority::CorrectnessRecovery,
            [TASKS_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE work_item
                     SET status = 'pending', available_at = ?1, updated_at = ?1
                     WHERE status = 'running'",
                    [now],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )?
        else {
            return Ok(None);
        };
        Ok(Some(receipt))
    }

    pub fn claim_media_work(&self, limit: usize, now: &str) -> Result<Vec<ClaimedMediaWork>> {
        self.claim_media_work_filtered(limit, now, false)
    }

    pub fn claim_derivative_work(&self, limit: usize, now: &str) -> Result<Vec<ClaimedMediaWork>> {
        self.claim_media_work_filtered(limit, now, true)
    }

    fn claim_media_work_filtered(
        &self,
        limit: usize,
        now: &str,
        derivatives_only: bool,
    ) -> Result<Vec<ClaimedMediaWork>> {
        let limit = limit.clamp(1, 8);
        let Some((work, _)) = self.auxiliary_write_if_changed(
            WorkPriority::Maintenance,
            [TASKS_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let sql = if derivatives_only {
                    "SELECT work_id, root_id, media_item_id, file_id, file_hash,
                            work_type, attempt_count
                     FROM work_item
                     WHERE status = 'pending' AND available_at <= ?1
                       AND work_type IN ('thumbnail', 'dominant_colors', 'perceptual_hash')
                     ORDER BY priority DESC, available_at, work_id
                     LIMIT ?2"
                } else {
                    "SELECT work_id, root_id, media_item_id, file_id, file_hash,
                            work_type, attempt_count
                     FROM work_item
                     WHERE status = 'pending' AND available_at <= ?1
                     ORDER BY priority DESC, available_at, work_id
                     LIMIT ?2"
                };
                let mut statement = transaction.prepare(sql)?;
                let rows = statement
                    .query_map(params![now, limit as i64], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Option<u32>>(1)?,
                            row.get::<_, Option<u32>>(2)?,
                            row.get::<_, Option<u32>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, u32>(6)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(statement);
                if rows.is_empty() {
                    return Ok(None);
                }
                let mut work = Vec::with_capacity(rows.len());
                for (work_id, root_id, media_id, file_id, file_hash, kind, attempt_count) in rows {
                    transaction.execute(
                        "UPDATE work_item
                         SET status = 'running', attempt_count = attempt_count + 1,
                             updated_at = ?1
                         WHERE work_id = ?2 AND status = 'pending'",
                        params![now, work_id],
                    )?;
                    work.push(ClaimedMediaWork {
                        work_id,
                        root_id: root_id.map(RootId),
                        media_id: media_id.map(MediaId),
                        file_id: file_id.map(FileId),
                        file_hash,
                        kind: parse_kind(&kind)?,
                        attempt_count: attempt_count + 1,
                    });
                }
                Ok(Some(work))
            },
        )?
        else {
            return Ok(Vec::new());
        };
        Ok(work)
    }

    pub fn complete_media_work(&self, work_ids: &[i64]) -> Result<Option<MutationReceipt>> {
        if work_ids.is_empty() {
            return Ok(None);
        }
        let Some(((), receipt)) = self.auxiliary_write_if_changed(
            WorkPriority::Maintenance,
            [TASKS_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let mut changed = 0;
                for work_id in work_ids {
                    changed += transaction.execute(
                        "DELETE FROM work_item WHERE work_id = ?1 AND status = 'running'",
                        [work_id],
                    )?;
                }
                Ok((changed != 0).then_some(()))
            },
        )?
        else {
            return Ok(None);
        };
        Ok(Some(receipt))
    }

    pub fn retry_media_work(
        &self,
        work_id: i64,
        attempt_count: u32,
        error: &str,
        now: &str,
    ) -> Result<(bool, Option<MutationReceipt>)> {
        let terminal = attempt_count >= MAX_ATTEMPTS;
        let available_at = if terminal {
            now.to_owned()
        } else {
            retry_at(now, attempt_count)?
        };
        let Some(((), receipt)) = self.auxiliary_write_if_changed(
            WorkPriority::Maintenance,
            [TASKS_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE work_item
                     SET status = ?1, available_at = ?2, last_error = ?3, updated_at = ?4
                     WHERE work_id = ?5 AND status = 'running'",
                    params![
                        if terminal { "failed" } else { "pending" },
                        available_at,
                        error,
                        now,
                        work_id,
                    ],
                )?;
                Ok((changed != 0).then_some(()))
            },
        )?
        else {
            return Ok((terminal, None));
        };
        Ok((terminal, Some(receipt)))
    }
}

fn parse_kind(value: &str) -> Result<MediaWorkKind> {
    match value {
        "thumbnail" => Ok(MediaWorkKind::Thumbnail),
        "dominant_colors" => Ok(MediaWorkKind::DominantColors),
        "perceptual_hash" => Ok(MediaWorkKind::PerceptualHash),
        "ai_tag" => Ok(MediaWorkKind::AiTag),
        "blob_delete" => Ok(MediaWorkKind::BlobDelete),
        _ => Err(LibraryError::InvalidState(format!(
            "unknown media work kind {value}"
        ))),
    }
}

fn retry_at(now: &str, attempt_count: u32) -> Result<String> {
    let parsed = DateTime::parse_from_rfc3339(now)
        .map_err(|error| LibraryError::InvalidInput(format!("invalid work timestamp: {error}")))?;
    let seconds = 1_i64 << attempt_count.saturating_sub(1).min(8);
    Ok((parsed.with_timezone(&Utc) + Duration::seconds(seconds.min(300))).to_rfc3339())
}
