//! Foreground tasks: imports that report progress and can be paused,
//! stopped, or stopped and undone.
//!
//! A task owns the ingest jobs it queues (`ingest_job.task_id`). Pausing holds
//! them at claim time, stopping drops the ones not yet imported, and undo
//! removes the roots the task created (`task_created_root`) and the tags it
//! added to roots that already existed (`task_added_tag`). Everything here is
//! durable, so a paused task stays paused and an interrupted one resumes.
//! Collection and folder changes use the same replay owners as session undo.

use super::*;
use crate::model::{
    ClaimedIngestJob, PreparedIngestPayload, TagId, TaskId, TaskIngestJob, TaskRecord, TaskState,
};
use rusqlite::{params, OptionalExtension};

fn task_resources() -> [String; 2] {
    ["tasks".to_owned(), "ingest".to_owned()]
}

const TASK_COLUMNS: &str = "t.task_id, t.kind, t.title, t.state, t.scan_complete, t.total_units,
     t.added_units, t.existing_units, t.revertible, t.created_at_ms, t.finished_at_ms,
     t.input_json, t.error,
     COALESCE((SELECT SUM(j.units) FROM ingest_job j
               WHERE j.task_id = t.task_id AND j.status = 'failed'), 0),
     COALESCE((SELECT SUM(j.units) FROM ingest_job j
               WHERE j.task_id = t.task_id AND j.status IN ('pending', 'running')), 0)";

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    let state: String = row.get(3)?;
    Ok(TaskRecord {
        task_id: TaskId(row.get(0)?),
        kind: row.get(1)?,
        title: row.get(2)?,
        state: TaskState::parse(&state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown task state {state}").into(),
            )
        })?,
        scan_complete: row.get(4)?,
        total_units: row.get::<_, i64>(5)?.max(0) as u64,
        added_units: row.get::<_, i64>(6)?.max(0) as u64,
        existing_units: row.get::<_, i64>(7)?.max(0) as u64,
        revertible: row.get(8)?,
        created_at_ms: row.get(9)?,
        finished_at_ms: row.get(10)?,
        input_json: row.get(11)?,
        error: row.get(12)?,
        failed_units: row.get::<_, i64>(13)?.max(0) as u64,
        remaining_units: row.get::<_, i64>(14)?.max(0) as u64,
    })
}

fn task_state(transaction: &rusqlite::Transaction<'_>, task: TaskId) -> Result<TaskState> {
    let state: Option<String> = transaction
        .query_row("SELECT state FROM task WHERE task_id = ?1", [task.0], |row| row.get(0))
        .optional()?;
    let state = state.ok_or_else(|| LibraryError::NotFound(format!("task {}", task.0)))?;
    TaskState::parse(&state)
        .ok_or_else(|| LibraryError::InvalidState(format!("unknown task state {state}")))
}

pub(super) fn require_running_task(
    transaction: &rusqlite::Transaction<'_>,
    task: TaskId,
) -> Result<()> {
    if task_state(transaction, task)? != TaskState::Running {
        return Err(LibraryError::TaskNotRunning);
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum TaskUndo {
    Structure {
        affected: Arc<RoaringBitmap>,
        before: Box<StructuralState>,
        after: Box<StructuralState>,
    },
    Folder {
        folder_id: FolderId,
        before: Option<Box<FolderDefinitionState>>,
        after: Option<Box<FolderDefinitionState>>,
    },
    Relink(Arc<crate::history::RelinkState>),
}

impl TaskUndo {
    fn into_change(self) -> SemanticChange {
        match self {
            Self::Relink(state) => SemanticChange::Relink(state),
            Self::Structure {
                affected,
                before,
                after,
            } => SemanticChange::Structure {
                affected,
                before: *before,
                after: *after,
            },
            Self::Folder {
                folder_id,
                before,
                after,
            } => SemanticChange::FolderDefinition {
                folder_id,
                before,
                after,
            },
        }
    }
}

pub(super) fn record_task_undo(
    transaction: &rusqlite::Transaction<'_>,
    task: TaskId,
    change: TaskUndo,
) -> Result<()> {
    let kind = match &change {
        TaskUndo::Structure { .. } | TaskUndo::Relink(_) => "structure",
        TaskUndo::Folder { .. } => "folder",
    };
    let payload = bincode::serialize(&change)
        .map_err(|error| LibraryError::InvalidState(error.to_string()))?;
    transaction.execute(
        "INSERT INTO task_undo(task_id, kind, payload) VALUES (?1, ?2, ?3)",
        params![task.0, kind, payload],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn replay_task_undo(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    task: TaskId,
    kind: &str,
    snapshot: &mut ProjectionSnapshot,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<()> {
    let mut statement = transaction.prepare_cached(
        "SELECT payload FROM task_undo WHERE task_id = ?1 AND kind = ?2 ORDER BY undo_id DESC",
    )?;
    let mut rows = statement.query(params![task.0, kind])?;
    while let Some(row) = rows.next()? {
        let bytes: Vec<u8> = row.get(0)?;
        let change: TaskUndo = bincode::deserialize(&bytes)
            .map_err(|error| LibraryError::InvalidState(error.to_string()))?;
        super::undo::apply_semantic_change(
            transaction,
            revision,
            snapshot,
            &change.into_change(),
            false,
            affected,
            resources,
        )?;
    }
    Ok(())
}

impl Library {
    /// Create a running task. A `background` task starts dismissed: it does
    /// its work without appearing in the task list (tutorial seeding).
    pub fn create_task(
        &self,
        kind: &str,
        title: &str,
        input_json: &str,
        revertible: bool,
        background: bool,
        now_ms: i64,
    ) -> Result<TaskId> {
        let (task_id, _) = self.auxiliary_write(
            WorkPriority::ForegroundMutation,
            task_resources(),
            [],
            |transaction, _| {
                transaction.execute(
                    "INSERT INTO task
                         (kind, title, state, input_json, revertible, dismissed, created_at_ms)
                     VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6)",
                    params![kind, title, input_json, revertible, background, now_ms],
                )?;
                Ok(TaskId(transaction.last_insert_rowid()))
            },
        )?;
        Ok(task_id)
    }

    /// Queue jobs for an open task in one transaction and count their files
    /// toward its total. A job already queued (same key, e.g. when an
    /// interrupted scan runs again) is kept as it is. Returns how many jobs
    /// were added; none once the task is stopped.
    pub fn enqueue_task_jobs(&self, task: TaskId, jobs: &[TaskIngestJob], now: &str) -> Result<usize> {
        for job in jobs {
            crate::ingest_queue::validate_job(&job.job)?;
        }
        let payloads = jobs
            .iter()
            .map(|job| serde_json::to_string(&job.job.payload))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let (inserted, _) = self.auxiliary_write(
            WorkPriority::CanonicalIngest,
            task_resources(),
            [],
            |transaction, _| {
                if !task_state(transaction, task)?.is_open() {
                    return Ok(0);
                }
                let mut statement = transaction.prepare_cached(
                    "INSERT INTO ingest_job
                         (job_key, source_kind, source_path, source_item_id, payload_json,
                          delete_after_ingest, status, attempt_count, available_at,
                          created_at, updated_at, task_id, units, prepared)
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5, 'pending', 0, ?6, ?6, ?6, ?7, ?8, ?9)
                     ON CONFLICT(job_key) DO NOTHING",
                )?;
                let mut inserted = 0usize;
                let mut units = 0u64;
                for (job, payload) in jobs.iter().zip(&payloads) {
                    let changed = statement.execute(params![
                        job.job.job_key,
                        job.job.source_kind,
                        job.job.source_path,
                        payload,
                        job.job.delete_after_ingest,
                        now,
                        task.0,
                        job.units as i64,
                        !matches!(
                            job.job.payload,
                            crate::PreparedIngestPayload::Pending(_)
                                | crate::PreparedIngestPayload::PendingCollection(_)
                        ),
                    ])?;
                    if changed == 1 {
                        inserted += 1;
                        units += job.units;
                    }
                }
                transaction.execute(
                    "UPDATE task SET total_units = total_units + ?1 WHERE task_id = ?2",
                    params![units as i64, task.0],
                )?;
                Ok(inserted)
            },
        )?;
        Ok(inserted)
    }

    /// The scan found every file; the task finishes once they are imported.
    pub fn complete_task_scan(&self, task: TaskId, now_ms: i64) -> Result<()> {
        self.auxiliary_write(WorkPriority::CanonicalIngest, task_resources(), [], |transaction, _| {
            transaction.execute(
                "UPDATE task SET scan_complete = 1 WHERE task_id = ?1",
                [task.0],
            )?;
            settle_finished(transaction, now_ms)?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn tasks(&self, include_dismissed: bool) -> Result<Vec<TaskRecord>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            let mut statement = connection.prepare_cached(&format!(
                "SELECT {TASK_COLUMNS} FROM task t
                 WHERE ?1 OR t.dismissed = 0
                 ORDER BY t.task_id"
            ))?;
            let rows = statement
                .query_map([include_dismissed], task_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn task(&self, task: TaskId) -> Result<Option<TaskRecord>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            Ok(connection
                .query_row(
                    &format!("SELECT {TASK_COLUMNS} FROM task t WHERE t.task_id = ?1"),
                    [task.0],
                    task_row,
                )
                .optional()?)
        })
    }

    /// The task's state alone; cheap enough to check between jobs.
    pub fn task_state(&self, task: TaskId) -> Result<Option<TaskState>> {
        self.database.read(WorkPriority::CanonicalIngest, |connection| {
            let state: Option<String> = connection
                .query_row("SELECT state FROM task WHERE task_id = ?1", [task.0], |row| row.get(0))
                .optional()?;
            Ok(state.as_deref().and_then(TaskState::parse))
        })
    }

    /// A task that cannot continue (its folder vanished, say) stops with the
    /// reason; what it imported stays.
    pub fn fail_task(&self, task: TaskId, error: &str, now_ms: i64) -> Result<()> {
        self.auxiliary_write(WorkPriority::CanonicalIngest, task_resources(), [], |transaction, _| {
            transaction.execute(
                "DELETE FROM ingest_job WHERE task_id = ?1 AND status = 'pending'",
                [task.0],
            )?;
            transaction.execute(
                "UPDATE task SET state = 'stopped', scan_complete = 1, error = ?1,
                     finished_at_ms = COALESCE(finished_at_ms, ?2)
                 WHERE task_id = ?3 AND state IN ('running', 'paused')",
                params![error, now_ms, task.0],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Files of a task that could not be imported, with the reason.
    pub fn task_failures(&self, task: TaskId, limit: usize) -> Result<Vec<(String, String)>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT source_path, COALESCE(last_error, '') FROM ingest_job
                 WHERE task_id = ?1 AND status = 'failed'
                 ORDER BY ingest_job_id LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![task.0, limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Open tasks whose scan did not finish (the app quit during it).
    pub fn tasks_needing_scan(&self) -> Result<Vec<TaskRecord>> {
        Ok(self
            .tasks(true)?
            .into_iter()
            .filter(|task| task.state.is_open() && !task.scan_complete)
            .collect())
    }

    /// Pause or resume an open task. Paused jobs stay queued and are not
    /// claimed, across restarts too.
    pub fn set_task_paused(&self, task: TaskId, paused: bool) -> Result<TaskState> {
        let (state, _) = self.auxiliary_write(
            WorkPriority::ForegroundMutation,
            task_resources(),
            [],
            |transaction, _| {
                let state = task_state(transaction, task)?;
                if !state.is_open() {
                    return Ok(state);
                }
                let next = if paused { TaskState::Paused } else { TaskState::Running };
                transaction.execute(
                    "UPDATE task SET state = ?1 WHERE task_id = ?2",
                    params![next.as_str(), task.0],
                )?;
                Ok(next)
            },
        )?;
        Ok(state)
    }

    /// Stop a task. Files not yet imported are dropped. With `undo`, the task
    /// is marked for the ingest worker to remove what it added; without it,
    /// what was imported stays. Undo also applies to a finished or stopped
    /// task, until it is dismissed.
    pub fn stop_task(&self, task: TaskId, undo: bool, now_ms: i64) -> Result<TaskState> {
        let (state, _) = self.auxiliary_write(
            WorkPriority::ForegroundMutation,
            task_resources(),
            [],
            |transaction, _| {
                let state = task_state(transaction, task)?;
                let revertible: bool = transaction.query_row(
                    "SELECT revertible FROM task WHERE task_id = ?1",
                    [task.0],
                    |row| row.get(0),
                )?;
                let next = if undo {
                    if !revertible {
                        return Err(LibraryError::InvalidInput(
                            "this import moved its files into the library, so it cannot be undone"
                                .into(),
                        ));
                    }
                    if !matches!(
                        state,
                        TaskState::Running | TaskState::Paused | TaskState::Finished | TaskState::Stopped
                    ) {
                        return Ok(state);
                    }
                    TaskState::Reverting
                } else {
                    if !state.is_open() {
                        return Ok(state);
                    }
                    TaskState::Stopped
                };
                transaction.execute(
                    "DELETE FROM ingest_job WHERE task_id = ?1 AND status = 'pending'",
                    [task.0],
                )?;
                transaction.execute(
                    "UPDATE task SET state = ?1, scan_complete = 1,
                         finished_at_ms = COALESCE(finished_at_ms, ?2)
                     WHERE task_id = ?3",
                    params![next.as_str(), now_ms, task.0],
                )?;
                Ok(next)
            },
        )?;
        Ok(state)
    }

    /// Hide a task that has ended from the task list.
    pub fn dismiss_task(&self, task: TaskId) -> Result<()> {
        self.auxiliary_write(WorkPriority::ForegroundMutation, task_resources(), [], |transaction, _| {
            transaction.execute(
                "UPDATE task SET dismissed = 1
                 WHERE task_id = ?1 AND state IN ('finished', 'stopped', 'reverted')",
                [task.0],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Mark open tasks whose scan is complete and whose jobs are all
    /// settled as finished.
    pub fn settle_finished_tasks(&self, now_ms: i64) -> Result<()> {
        self.auxiliary_write_if_changed(
            WorkPriority::CanonicalIngest,
            task_resources(),
            [],
            |transaction, _| Ok((settle_finished(transaction, now_ms)? > 0).then_some(())),
        )?;
        Ok(())
    }

    pub fn tasks_to_revert(&self) -> Result<Vec<TaskId>> {
        self.database.read(WorkPriority::CanonicalIngest, |connection| {
            let mut statement = connection
                .prepare_cached("SELECT task_id FROM task WHERE state = 'reverting' ORDER BY task_id")?;
            let rows = statement
                .query_map([], |row| Ok(TaskId(row.get(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Hand claimed jobs of a task that was paused or stopped mid-batch back
    /// to the queue without counting the claim as an attempt.
    pub fn release_task_jobs(&self, ingest_job_ids: &[i64], now: &str) -> Result<()> {
        if ingest_job_ids.is_empty() {
            return Ok(());
        }
        let ids = serde_json::to_string(ingest_job_ids)?;
        self.auxiliary_write(WorkPriority::CanonicalIngest, task_resources(), [], |transaction, _| {
            transaction.execute(
                "UPDATE ingest_job
                 SET status = 'pending', attempt_count = MAX(attempt_count - 1, 0), updated_at = ?1
                 WHERE ingest_job_id IN (SELECT value FROM json_each(?2)) AND status = 'running'",
                params![now, ids],
            )?;
            transaction.execute(
                "DELETE FROM ingest_job
                 WHERE ingest_job_id IN (SELECT value FROM json_each(?1))
                   AND status = 'pending' AND task_id IN (
                     SELECT task_id FROM task WHERE state NOT IN ('running', 'paused')
                 )",
                [ids],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Queued task files not yet read, of running tasks only, oldest first.
    pub fn unprepared_task_jobs(&self, limit: usize) -> Result<Vec<ClaimedIngestJob>> {
        self.database.read(WorkPriority::CanonicalIngest, |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT j.ingest_job_id, j.source_path, j.delete_after_ingest, j.attempt_count,
                        j.payload_json, j.task_id
                 FROM task t
                 JOIN ingest_job j ON j.task_id = t.task_id AND j.status = 'pending'
                 WHERE t.state = 'running' AND j.prepared = 0
                 ORDER BY j.ingest_job_id
                 LIMIT ?1",
            )?;
            let rows = statement
                .query_map([limit as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(ingest_job_id, source_path, delete_after_ingest, attempt_count, payload, task_id)| {
                    Ok(ClaimedIngestJob {
                        ingest_job_id,
                        source_path,
                        delete_after_ingest,
                        attempt_count,
                        payload: serde_json::from_str(&payload)?,
                        task_id: task_id.map(TaskId),
                    })
                })
                .collect()
        })
    }

    /// How many read files of running tasks wait to be imported.
    pub fn prepared_task_backlog(&self) -> Result<usize> {
        self.database.read(WorkPriority::CanonicalIngest, |connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM task t
                 JOIN ingest_job j ON j.task_id = t.task_id AND j.status = 'pending'
                 WHERE t.state = 'running' AND j.prepared = 1",
                [],
                |row| row.get::<_, i64>(0),
            )? as usize)
        })
    }

    /// Store what reading queued files found: a prepared payload makes the
    /// job claimable; an error fails it (an unreadable or unsupported file).
    pub fn settle_unprepared_task_jobs(
        &self,
        results: &[(i64, std::result::Result<PreparedIngestPayload, String>)],
        now: &str,
    ) -> Result<()> {
        if results.is_empty() {
            return Ok(());
        }
        let encoded = results
            .iter()
            .map(|(job_id, result)| {
                Ok((
                    *job_id,
                    match result {
                        Ok(payload) => Ok(serde_json::to_string(payload)?),
                        Err(error) => Err(error.clone()),
                    },
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.auxiliary_write(WorkPriority::CanonicalIngest, task_resources(), [], |transaction, _| {
            for (job_id, result) in &encoded {
                match result {
                    Ok(payload) => transaction.execute(
                        "UPDATE ingest_job SET payload_json = ?1, prepared = 1, updated_at = ?2
                         WHERE ingest_job_id = ?3 AND status = 'pending' AND prepared = 0
                           AND EXISTS (SELECT 1 FROM task WHERE task.task_id = ingest_job.task_id AND state IN ('running', 'paused'))",
                        params![payload, now, job_id],
                    )?,
                    Err(error) => transaction.execute(
                        "UPDATE ingest_job SET status = 'failed', last_error = ?1,
                             payload_json = '{}', updated_at = ?2
                         WHERE ingest_job_id = ?3 AND status = 'pending' AND prepared = 0
                           AND EXISTS (SELECT 1 FROM task WHERE task.task_id = ingest_job.task_id AND state = 'running')",
                        params![error, now, job_id],
                    )?,
                };
            }
            settle_finished(transaction, chrono::Utc::now().timestamp_millis())?;
            Ok(())
        })?;
        Ok(())
    }

    /// Undo a task: remove the roots it created that still exist and the tags
    /// it added to roots that already existed, then mark it reverted. Called
    /// by the ingest worker, so no batch of the task is in flight. Returns the
    /// files that can be deleted now.
    pub fn revert_task(&self, task: TaskId, now_ms: i64) -> Result<Vec<PendingBlobCleanup>> {
        let protected_cleanup_files = self.history.protected_cleanup_files();
        let cleanup = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            transaction.execute(
                "DELETE FROM ingest_job WHERE task_id = ?1 AND status IN ('pending', 'running')",
                [task.0],
            )?;
            let mut next = (*snapshot).clone();
            let mut resources = BTreeSet::from(["tasks".to_owned(), "ingest".to_owned()]);
            let mut affected = RoaringBitmap::new();
            replay_task_undo(transaction, revision, task, "structure", &mut next, &mut affected, &mut resources)?;
            let mut existing = RoaringBitmap::new();

            for kind in [RootKind::Media, RootKind::Collection] {
                if let Some(roots) = next.root_kinds.get(&kind) {
                    existing |= roots;
                }
            }
            let created = {
                let mut statement = transaction
                    .prepare_cached("SELECT root_id FROM task_created_root WHERE task_id = ?1")?;
                let rows = statement
                    .query_map([task.0], |row| row.get::<_, u32>(0))?
                    .collect::<rusqlite::Result<RoaringBitmap>>()?;
                rows & &existing
            };
            let added_tags = {
                let mut statement = transaction.prepare_cached(
                    "SELECT root_id, tag_id FROM task_added_tag WHERE task_id = ?1",
                )?;
                let rows = statement
                    .query_map([task.0], |row| Ok((row.get::<_, u32>(0)?, TagId(row.get(1)?))))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };

            affected |= &created;
            let cleanup = if created.is_empty() {
                Vec::new()
            } else {
                let (deleted, cleanup, receipt) = super::structure::delete_roots(
                    transaction, revision, &next, &created, now_ms, &protected_cleanup_files,
                )?;
                resources.extend(receipt.resources);
                next = deleted;
                cleanup
            };

            let mut by_tag = std::collections::BTreeMap::<TagId, RoaringBitmap>::new();
            for (root_id, tag_id) in added_tags {
                if existing.contains(root_id) && !created.contains(root_id) {
                    by_tag.entry(tag_id).or_default().insert(root_id);
                }
            }
            let mut bitmap_keys = Vec::new();
            let mut untagged = RoaringBitmap::new();
            for (tag_id, roots) in by_tag {
                let Some(members) = Arc::make_mut(&mut next.tags).get_mut(&tag_id) else {
                    continue;
                };
                let removed = &*members & &roots;
                if removed.is_empty() {
                    continue;
                }
                *members -= &removed;
                let counts = Arc::make_mut(&mut next.tag_count);
                for root_id in &removed {
                    counts.insert(root_id, counts.value(root_id).unwrap_or(0).saturating_sub(1));
                }
                untagged |= &removed;
                bitmap_keys.push(BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                });
            }
            if !untagged.is_empty() {
                ingest::persist_touched(
                    transaction,
                    revision,
                    &next,
                    bitmap_keys,
                    std::iter::empty(),
                    untagged.iter().map(RootId),
                )?;
                resources.extend(["roots".to_owned(), "tags".to_owned()]);
                affected |= &untagged;
            }
            replay_task_undo(transaction, revision, task, "folder", &mut next, &mut affected, &mut resources)?;
            crate::smart::settle_affected(transaction, &mut next, &affected)?;
            if !Arc::ptr_eq(&snapshot.smart_results, &next.smart_results) {
                resources.insert("smart_folders".into());
            }
            transaction.execute("DELETE FROM task_undo WHERE task_id = ?1", [task.0])?;
            transaction.execute(
                "UPDATE task SET state = 'reverted', scan_complete = 1,
                     finished_at_ms = COALESCE(finished_at_ms, ?1)
                 WHERE task_id = ?2",
                params![now_ms, task.0],
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(revision, resources, affected.iter().map(RootId));
            Ok((
                cleanup,
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        Ok(cleanup)
    }
}

/// Count one ingested job toward its task: a created root is recorded for
/// undo and its files count as added; otherwise they count as already in the
/// library. Tags added to roots that already existed are recorded too.
pub(crate) fn record_task_ingest(
    transaction: &rusqlite::Transaction<'_>,
    task: TaskId,
    created_root: Option<RootId>,
    added_tags: &[(RootId, TagId)],
    units: u64,
) -> Result<()> {
    let (added, existing) = if created_root.is_some() { (units, 0) } else { (0, units) };
    record_task_units(transaction, task, added as usize, existing as usize, created_root)?;
    let mut statement = transaction.prepare_cached(
        "INSERT INTO task_added_tag (task_id, root_id, tag_id) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
    )?;
    for (root_id, tag_id) in added_tags {
        statement.execute(params![task.0, root_id.0, tag_id.0])?;
    }
    Ok(())
}

pub(crate) fn record_task_units(
    transaction: &rusqlite::Transaction<'_>,
    task: TaskId,
    added: usize,
    existing: usize,
    created_root: Option<RootId>,
) -> Result<()> {
    if added + existing > 0 {
        transaction
            .prepare_cached(
                "UPDATE task SET added_units = added_units + ?1,
                     existing_units = existing_units + ?2
                 WHERE task_id = ?3",
            )?
            .execute(params![added as i64, existing as i64, task.0])?;
    }
    if let Some(root_id) = created_root {
        transaction
            .prepare_cached(
                "INSERT INTO task_created_root (task_id, root_id) VALUES (?1, ?2)
                 ON CONFLICT DO NOTHING",
            )?
            .execute(params![task.0, root_id.0])?;
    }
    Ok(())
}

fn settle_finished(transaction: &rusqlite::Transaction<'_>, now_ms: i64) -> Result<usize> {
    Ok(transaction.execute(
        "UPDATE task SET state = 'finished', finished_at_ms = ?1
         WHERE state IN ('running', 'paused') AND scan_complete = 1
           AND NOT EXISTS (
               SELECT 1 FROM ingest_job j
               WHERE j.task_id = task.task_id AND j.status IN ('pending', 'running')
           )",
        [now_ms],
    )?)
}
