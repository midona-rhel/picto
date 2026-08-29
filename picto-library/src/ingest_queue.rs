use rusqlite::{params, OptionalExtension};

use crate::database::WorkPriority;
use crate::model::{ClaimedIngestJob, PreparedIngestJob, PreparedIngestPayload};
use crate::publication::MutationReceipt;
use crate::{Library, LibraryError, Result};

const INGEST_RESOURCE: &str = "ingest";

impl Library {
    pub fn reset_running_ingest_jobs(&self, now: &str) -> Result<Option<MutationReceipt>> {
        let Some(((), receipt)) = self.auxiliary_write_if_changed(
            WorkPriority::CorrectnessRecovery,
            [INGEST_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let changed = transaction.execute(
                    "UPDATE ingest_job
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

    pub fn enqueue_ingest_job(
        &self,
        job: &PreparedIngestJob,
        now: &str,
    ) -> Result<(i64, MutationReceipt)> {
        validate_job(job)?;
        let payload = serde_json::to_string(&job.payload)?;
        self.auxiliary_write(
            WorkPriority::CanonicalIngest,
            [INGEST_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                if let Some((ingest_job_id, status)) = transaction
                    .query_row(
                        "SELECT ingest_job_id, status FROM ingest_job WHERE job_key = ?1",
                        [&job.job_key],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?
                {
                    if matches!(status.as_str(), "failed" | "succeeded") {
                        transaction.execute(
                            "UPDATE ingest_job
                             SET source_kind = ?1, source_path = ?2, source_item_id = ?3,
                                 payload_json = ?4, delete_after_ingest = ?5,
                                 status = 'pending', attempt_count = 0, available_at = ?6,
                                 last_error = NULL, updated_at = ?6
                             WHERE ingest_job_id = ?7",
                            params![
                                job.source_kind,
                                job.source_path,
                                job.source_item_id,
                                payload,
                                job.delete_after_ingest,
                                now,
                                ingest_job_id,
                            ],
                        )?;
                    }
                    return Ok(ingest_job_id);
                }
                transaction.execute(
                    "INSERT INTO ingest_job
                         (job_key, source_kind, source_path, source_item_id, payload_json,
                          delete_after_ingest, status, attempt_count, available_at,
                          created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0, ?7, ?7, ?7)",
                    params![
                        job.job_key,
                        job.source_kind,
                        job.source_path,
                        job.source_item_id,
                        payload,
                        job.delete_after_ingest,
                        now,
                    ],
                )?;
                Ok(transaction.last_insert_rowid())
            },
        )
    }

    pub fn claim_ingest_jobs(&self, limit: usize, now: &str) -> Result<Vec<ClaimedIngestJob>> {
        let limit = limit.clamp(1, crate::ingest::MAX_INGEST_BATCH);
        let Some((jobs, _)) = self.auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            [INGEST_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let mut statement = transaction.prepare(
                    "SELECT ingest_job_id, source_path, delete_after_ingest,
                            attempt_count, payload_json
                     FROM ingest_job
                     WHERE status = 'pending' AND available_at <= ?1
                     ORDER BY available_at, ingest_job_id
                     LIMIT ?2",
                )?;
                let rows = statement
                    .query_map(params![now, limit as i64], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, bool>(2)?,
                            row.get::<_, u32>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(statement);
                if rows.is_empty() {
                    return Ok(None);
                }
                let mut jobs = Vec::with_capacity(rows.len());
                for (ingest_job_id, source_path, delete_after_ingest, attempt_count, payload) in
                    rows
                {
                    transaction.execute(
                        "UPDATE ingest_job
                         SET status = 'running', attempt_count = attempt_count + 1,
                             updated_at = ?1
                         WHERE ingest_job_id = ?2 AND status = 'pending'",
                        params![now, ingest_job_id],
                    )?;
                    jobs.push(ClaimedIngestJob {
                        ingest_job_id,
                        source_path,
                        delete_after_ingest,
                        attempt_count: attempt_count + 1,
                        payload: serde_json::from_str(&payload)?,
                    });
                }
                Ok(Some(jobs))
            },
        )?
        else {
            return Ok(Vec::new());
        };
        Ok(jobs)
    }

    pub fn complete_ingest_jobs(
        &self,
        ingest_job_ids: &[i64],
        now: &str,
    ) -> Result<Option<MutationReceipt>> {
        self.set_ingest_job_status(ingest_job_ids, "succeeded", None, now)
    }

    pub fn fail_ingest_job(
        &self,
        ingest_job_id: i64,
        error: &str,
        now: &str,
    ) -> Result<Option<MutationReceipt>> {
        self.set_ingest_job_status(&[ingest_job_id], "failed", Some(error), now)
    }

    fn set_ingest_job_status(
        &self,
        ingest_job_ids: &[i64],
        status: &str,
        error: Option<&str>,
        now: &str,
    ) -> Result<Option<MutationReceipt>> {
        if ingest_job_ids.is_empty() {
            return Ok(None);
        }
        let Some(((), receipt)) = self.auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            [INGEST_RESOURCE.to_owned()],
            [],
            |transaction, _| {
                let mut changed = 0;
                for ingest_job_id in ingest_job_ids {
                    changed += transaction.execute(
                        "UPDATE ingest_job
                         SET status = ?1, last_error = ?2, updated_at = ?3,
                             payload_json = CASE WHEN ?1 = 'succeeded' THEN '{}' ELSE payload_json END
                         WHERE ingest_job_id = ?4 AND status = 'running'",
                        params![status, error, now, ingest_job_id],
                    )?;
                }
                Ok((changed != 0).then_some(()))
            },
        )? else {
            return Ok(None);
        };
        Ok(Some(receipt))
    }
}

fn validate_job(job: &PreparedIngestJob) -> Result<()> {
    if job.job_key.trim().is_empty() {
        return Err(LibraryError::InvalidInput("ingest job key is empty".into()));
    }
    if job.source_kind.trim().is_empty() || job.source_path.trim().is_empty() {
        return Err(LibraryError::InvalidInput(
            "ingest source kind and path are required".into(),
        ));
    }
    match &job.payload {
        PreparedIngestPayload::Item(input) if input.stable_key.trim().is_empty() => Err(
            LibraryError::InvalidInput("ingest stable key is empty".into()),
        ),
        PreparedIngestPayload::Collection(input) if input.members.len() < 2 => Err(
            LibraryError::InvalidInput("collection ingest needs at least two members".into()),
        ),
        _ => Ok(()),
    }
}
