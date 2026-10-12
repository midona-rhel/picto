use std::fs;
use std::path::{Path, PathBuf};

use picto_library::{
    ClaimedIngestJob, LibraryError, PreparedCollectionImport, PreparedImport,
    PreparedIngestPayload, RootId,
};

use crate::library_application::LibraryApplication;
use crate::media_capabilities::ThumbnailBackend;
use crate::media_processing::{PreparedMediaSource, DEFAULT_THUMBNAIL_DIMENSIONS};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalIngestRunReport {
    pub claimed: usize,
    pub ingested: usize,
    pub skipped: usize,
    pub failed: usize,
    pub root_ids: Vec<RootId>,
    pub cleanup_failures: usize,
}

pub fn recover(application: &LibraryApplication) -> Result<(), String> {
    application
        .library()
        .reset_running_ingest_jobs(&chrono::Utc::now().to_rfc3339())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// A job whose batch keeps failing is retried this many times in all.
const MAX_INGEST_ATTEMPTS: u32 = 5;
/// How long a released job waits before it is claimed again.
const RELEASED_JOB_RETRY_DELAY: chrono::Duration = chrono::Duration::seconds(10);

pub fn run_batch(
    application: &LibraryApplication,
    limit: usize,
) -> Result<CanonicalIngestRunReport, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let claimed = application
        .library()
        .claim_ingest_jobs(limit, &now)
        .map_err(|error| error.to_string())?;
    let mut report = CanonicalIngestRunReport {
        claimed: claimed.len(),
        ..Default::default()
    };
    if claimed.is_empty() {
        return Ok(report);
    }
    let claimed_ids = claimed.iter().map(|job| job.ingest_job_id).collect::<Vec<_>>();
    let settled = settle_claimed(application, claimed, &now, &mut report);
    if let Err(error) = &settled {
        // The worker owns every claim until it settles it. Whatever this batch
        // left running goes back to the queue; replay is idempotent because
        // ingest finds an already-imported item by its stable key.
        let retry_at = (chrono::Utc::now() + RELEASED_JOB_RETRY_DELAY).to_rfc3339();
        if let Err(release) = application.library().release_ingest_jobs(
            &claimed_ids,
            error,
            &chrono::Utc::now().to_rfc3339(),
            &retry_at,
            MAX_INGEST_ATTEMPTS,
        ) {
            tracing::warn!(%release, "Could not release ingest claims; they recover on next open");
        }
    }
    settled.map(|()| report)
}

fn settle_claimed(
    application: &LibraryApplication,
    claimed: Vec<ClaimedIngestJob>,
    now: &str,
    report: &mut CanonicalIngestRunReport,
) -> Result<(), String> {
    let now = now.to_owned();
    let auto_tag = application
        .application_settings()?
        .value
        .get("aiTaggerAutoOnImport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // Prepared single items are written together: one publication, one
    // source reconcile and one completion per group instead of three writes
    // per item. A group is flushed when it is full or has waited long enough
    // that the first items should already be visible.
    let mut pending = PendingItems::default();
    for mut job in claimed {
        match prepare_job(application, &mut job) {
            Ok(cleanup) => match job.payload {
                PreparedIngestPayload::Item(input) => {
                    // One publication holds one task's items, so each
                    // task's created roots are recorded with its own counts.
                    if pending.task != job.task_id {
                        pending.flush(application, &now, auto_tag, report)?;
                        pending.task = job.task_id;
                    }
                    pending.push(job.ingest_job_id, input, cleanup);
                    if pending.should_flush() {
                        pending.flush(application, &now, auto_tag, report)?;
                    }
                }
                PreparedIngestPayload::Collection(input) => {
                    settle_collection(
                        application,
                        job.ingest_job_id,
                        job.task_id,
                        input,
                        cleanup,
                        &now,
                        auto_tag,
                        report,
                    )?;
                }
                PreparedIngestPayload::Pending(_) | PreparedIngestPayload::PendingCollection(_) => {
                    unreachable!("prepare_job rejects unread task files")
                }
            },
            Err(error) => {
                application
                    .library()
                    .fail_ingest_job(job.ingest_job_id, &error, &now)
                    .map_err(|failure| failure.to_string())?;
                mark_failed_sources(application, payload_inputs(&job.payload), &error, &now)?;
                report.failed += 1;
            }
        }
    }
    pending.flush(application, &now, auto_tag, report)?;
    crate::library_subscription_state::settle_ingest_runs(application, &now)?;
    Ok(())
}

const PENDING_FLUSH_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Default)]
struct PendingItems {
    task: Option<picto_library::TaskId>,
    jobs: Vec<i64>,
    inputs: Vec<PreparedImport>,
    cleanup: Vec<Vec<PathBuf>>,
    since: Option<std::time::Instant>,
}

impl PendingItems {
    fn push(&mut self, job_id: i64, input: PreparedImport, cleanup: Vec<PathBuf>) {
        self.since.get_or_insert_with(std::time::Instant::now);
        self.jobs.push(job_id);
        self.inputs.push(input);
        self.cleanup.push(cleanup);
    }

    fn should_flush(&self) -> bool {
        self.jobs.len() >= picto_library::ingest::MAX_INGEST_BATCH
            || self
                .since
                .is_some_and(|since| since.elapsed() >= PENDING_FLUSH_AFTER)
    }

    fn flush(
        &mut self,
        application: &LibraryApplication,
        now: &str,
        auto_tag: bool,
        report: &mut CanonicalIngestRunReport,
    ) -> Result<(), String> {
        let jobs = std::mem::take(&mut self.jobs);
        let inputs = std::mem::take(&mut self.inputs);
        let cleanup = std::mem::take(&mut self.cleanup);
        self.since = None;
        if jobs.is_empty() {
            return Ok(());
        }
        let ingested = ingest_items(application, &inputs, self.task, auto_tag);
        let Ok(outputs) = ingested else {
            // One bad item fails the whole group; settle each on its own so
            // the rest still import and the bad one gets its own error.
            for ((job_id, input), cleanup) in jobs.into_iter().zip(inputs).zip(cleanup) {
                settle_item(application, job_id, self.task, input, cleanup, now, auto_tag, report)?;
            }
            return Ok(());
        };
        let placed = inputs
            .iter()
            .zip(&outputs)
            .map(|(input, (root_id, _))| (std::slice::from_ref(input), *root_id))
            .collect::<Vec<_>>();
        reconcile_ingested_sources(application, &placed, now)?;
        application
            .library()
            .complete_ingest_jobs(&jobs, now)
            .map_err(|error| error.to_string())?;
        for ((root_id, _), cleanup) in outputs.into_iter().zip(cleanup) {
            report.ingested += 1;
            report.root_ids.push(root_id);
            report.cleanup_failures += cleanup_sources(cleanup);
        }
        Ok(())
    }
}

fn ingest_items(
    application: &LibraryApplication,
    inputs: &[PreparedImport],
    task: Option<picto_library::TaskId>,
    auto_tag: bool,
) -> picto_library::Result<Vec<(RootId, picto_library::MutationReceipt)>> {
    match (task, auto_tag) {
        (Some(task), _) => application.library().ingest_batch_for_task(inputs, task, auto_tag),
        (None, true) => application.library().ingest_batch_with_auto_tags(inputs),
        (None, false) => application.library().ingest_batch(inputs),
    }
}

#[allow(clippy::too_many_arguments)]
fn settle_item(
    application: &LibraryApplication,
    job_id: i64,
    task: Option<picto_library::TaskId>,
    input: PreparedImport,
    cleanup: Vec<PathBuf>,
    now: &str,
    auto_tag: bool,
    report: &mut CanonicalIngestRunReport,
) -> Result<(), String> {
    let ingested = ingest_items(application, std::slice::from_ref(&input), task, auto_tag)
        .map(|mut values| values.remove(0));
    match ingested {
        Ok((root_id, _)) => {
            reconcile_ingested_sources(application, &[(std::slice::from_ref(&input), root_id)], now)?;
            application
                .library()
                .complete_ingest_jobs(&[job_id], now)
                .map_err(|error| error.to_string())?;
            report.ingested += 1;
            report.root_ids.push(root_id);
            report.cleanup_failures += cleanup_sources(cleanup);
            Ok(())
        }
        Err(error) => {
            if matches!(error, LibraryError::ImportDeleted) {
                settle_deleted_import(application, job_id, std::slice::from_ref(&input), now)?;
                report.skipped += 1;
                report.cleanup_failures += cleanup_sources(cleanup);
            } else {
                application
                    .library()
                    .fail_ingest_job(job_id, &error.to_string(), now)
                    .map_err(|failure| failure.to_string())?;
                mark_failed_sources(
                    application,
                    std::slice::from_ref(&input),
                    &error.to_string(),
                    now,
                )?;
                report.failed += 1;
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn settle_collection(
    application: &LibraryApplication,
    job_id: i64,
    task: Option<picto_library::TaskId>,
    input: PreparedCollectionImport,
    cleanup: Vec<PathBuf>,
    now: &str,
    auto_tag: bool,
    report: &mut CanonicalIngestRunReport,
) -> Result<(), String> {
    let ingested = match (task, auto_tag) {
        (Some(task), _) => application
            .library()
            .ingest_collection_for_task(&input, task, auto_tag),
        (None, true) => application
            .library()
            .ingest_collection_with_auto_tags(&input),
        (None, false) => application.library().ingest_collection(&input),
    };
    match ingested {
        Ok((root_id, _)) => {
            reconcile_ingested_sources(application, &[(&input.members, root_id)], now)?;
            application
                .library()
                .complete_ingest_jobs(&[job_id], now)
                .map_err(|error| error.to_string())?;
            report.ingested += 1;
            report.root_ids.push(root_id);
            report.cleanup_failures += cleanup_sources(cleanup);
        }
        Err(error) => {
            if matches!(error, LibraryError::ImportDeleted) {
                settle_deleted_import(application, job_id, &input.members, now)?;
                report.skipped += 1;
                report.cleanup_failures += cleanup_sources(cleanup);
            } else {
                application
                    .library()
                    .fail_ingest_job(job_id, &error.to_string(), now)
                    .map_err(|failure| failure.to_string())?;
                mark_failed_sources(application, &input.members, &error.to_string(), now)?;
                report.failed += 1;
            }
        }
    }
    Ok(())
}

fn settle_deleted_import(
    application: &LibraryApplication,
    ingest_job_id: i64,
    inputs: &[PreparedImport],
    now: &str,
) -> Result<(), String> {
    let sources = inputs
        .iter()
        .filter_map(|input| input.source_identity.as_ref())
        .filter_map(|source| {
            let (site_id, post_key) = source.source_key.split_once(':')?;
            Some((site_id, post_key, source.source_item_key.as_str()))
        })
        .collect::<Vec<_>>();
    application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::CanonicalIngest,
            ["subscriptions".to_owned(), "tasks".to_owned()],
            [],
            |transaction, _| {
                let mut changed = transaction.execute(
                    "UPDATE ingest_job
                     SET status = 'succeeded', last_error = NULL, payload_json = '{}', updated_at = ?1
                     WHERE ingest_job_id = ?2 AND status = 'running'",
                    rusqlite::params![now, ingest_job_id],
                )?;
                for (site_id, post_key, item_key) in &sources {
                    changed += transaction.execute(
                        "UPDATE source_item
                         SET state = 'deleted', media_item_id = NULL,
                             last_error = NULL, updated_at = ?1
                         WHERE item_key = ?2 AND source_post_id = (
                             SELECT source_post_id FROM source_post
                             WHERE site_id = ?3 AND post_key = ?4
                         ) AND state != 'ingested'",
                        rusqlite::params![now, item_key, site_id, post_key],
                    )?;
                }
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn payload_inputs(payload: &PreparedIngestPayload) -> &[PreparedImport] {
    match payload {
        PreparedIngestPayload::Item(input) => std::slice::from_ref(input),
        PreparedIngestPayload::Collection(input) => &input.members,
        PreparedIngestPayload::Pending(_) | PreparedIngestPayload::PendingCollection(_) => &[],
    }
}

fn mark_failed_sources(
    application: &LibraryApplication,
    inputs: &[PreparedImport],
    error: &str,
    now: &str,
) -> Result<(), String> {
    let sources = inputs
        .iter()
        .filter_map(|input| input.source_identity.as_ref())
        .filter_map(|source| {
            let (site_id, post_key) = source.source_key.split_once(':')?;
            Some((site_id, post_key, source.source_item_key.as_str()))
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Ok(());
    }
    application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::CanonicalIngest,
            ["subscriptions".to_owned(), "tasks".to_owned()],
            [],
            |transaction, _| {
                let mut changed = 0;
                for (site_id, post_key, item_key) in &sources {
                    changed += transaction.execute(
                        "UPDATE source_item
                         SET state = 'failed', last_error = ?1, updated_at = ?2
                         WHERE item_key = ?3 AND source_post_id = (
                             SELECT source_post_id FROM source_post
                             WHERE site_id = ?4 AND post_key = ?5
                         ) AND state NOT IN ('ingested', 'deleted')",
                        rusqlite::params![error, now, item_key, site_id, post_key],
                    )?;
                }
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|failure| failure.to_string())
}

fn reconcile_ingested_sources(
    application: &LibraryApplication,
    placed: &[(&[PreparedImport], RootId)],
    now: &str,
) -> Result<(), String> {
    let sources = placed
        .iter()
        .flat_map(|(inputs, root_id)| inputs.iter().map(move |input| (input, *root_id)))
        .filter_map(|(input, root_id)| Some((input.source_identity.as_ref()?, root_id)))
        .filter_map(|(source, root_id)| {
            let (site_id, post_key) = source.source_key.split_once(':')?;
            Some((
                site_id.to_owned(),
                post_key.to_owned(),
                source.source_key.clone(),
                source.source_item_key.clone(),
                root_id,
            ))
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Ok(());
    }
    let snapshot = application.library().projections().snapshot();
    application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::CanonicalIngest,
            ["subscriptions".to_owned(), "tasks".to_owned()],
            // Source bookkeeping changes no library item; listing the root would
            // invalidate SQL-derived query results for nothing.
            std::iter::empty(),
            |transaction, _| {
                let mut changed = 0;
                for (site_id, post_key, source_key, item_key, root_id) in &sources {
                    let root_id = *root_id;
                    let mut statement = transaction.prepare_cached(
                        "SELECT media_id FROM source_provenance
                         WHERE source_key = ?1 AND source_item_key = ?2
                         ORDER BY media_id",
                    )?;
                    let media_ids = statement
                        .query_map(rusqlite::params![source_key, item_key], |row| {
                            row.get::<_, u32>(0)
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    let media_id = media_ids
                        .into_iter()
                        .find(|media_id| snapshot.media_owner.get(*media_id) == Some(&root_id))
                        .ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "source item {item_key} has no media owned by root {}",
                                root_id.0
                            ))
                        })?;
                    changed += transaction.execute(
                        "UPDATE source_item
                         SET media_item_id = ?1, state = 'ingested', last_error = NULL, updated_at = ?2
                         WHERE item_key = ?3 AND source_post_id = (
                             SELECT source_post_id FROM source_post
                             WHERE site_id = ?4 AND post_key = ?5
                         ) AND (media_item_id IS NOT ?1 OR state != 'ingested')",
                        rusqlite::params![media_id, now, item_key, site_id, post_key],
                    )?;
                    changed += transaction.execute(
                        "UPDATE source_post SET root_item_id = ?1, updated_at = ?2
                         WHERE site_id = ?3 AND post_key = ?4 AND root_item_id IS NOT ?1",
                        rusqlite::params![root_id.0, now, site_id, post_key],
                    )?;
                }
                Ok((changed != 0).then_some(()))
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn prepare_job(
    application: &LibraryApplication,
    job: &mut ClaimedIngestJob,
) -> Result<Vec<PathBuf>, String> {
    let mut cleanup = Vec::new();
    match &mut job.payload {
        PreparedIngestPayload::Item(input) => {
            prepare_import(application, input, job.delete_after_ingest, &mut cleanup)?;
            ensure_visible_thumbnail(application, input)?;
        }
        PreparedIngestPayload::Collection(input) => {
            for member in &mut input.members {
                prepare_import(application, member, job.delete_after_ingest, &mut cleanup)?;
            }
            let cover = input.members.get(input.cover_index).ok_or_else(|| {
                "Collection cover index is outside the prepared members".to_string()
            })?;
            ensure_visible_thumbnail(application, cover)?;
        }
        PreparedIngestPayload::Pending(_) | PreparedIngestPayload::PendingCollection(_) => {
            return Err("Import file was queued but never read".into());
        }
    }
    Ok(cleanup)
}

fn ensure_visible_thumbnail(
    application: &LibraryApplication,
    input: &PreparedImport,
) -> Result<(), String> {
    if application
        .blobs()
        .find_thumbnail_path(&input.facts.content_hash)
        .map_err(|error| format!("Thumbnail lookup failed: {error}"))?
        .is_some()
    {
        return Ok(());
    }

    let mut source = PreparedMediaSource::from_stored_metadata(
        PathBuf::from(&input.file_path),
        &input.facts.mime,
        input
            .facts
            .duration_ms
            .and_then(|value| i64::try_from(value).ok()),
        input.facts.frame_count.map(i64::from),
    );
    if source.caps.thumbnail_backend != Some(ThumbnailBackend::Inline) {
        return Ok(());
    }
    let (bytes, extension) = source
        .render_inline_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS)
        .map_err(|error| format!("Initial thumbnail generation failed: {error}"))?;
    application
        .blobs()
        .write_thumbnail(&input.facts.content_hash, &bytes, &extension)
        .map_err(|error| format!("Initial thumbnail write failed: {error}"))
}

fn prepare_import(
    application: &LibraryApplication,
    input: &mut PreparedImport,
    delete_after_ingest: bool,
    cleanup: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let source = PathBuf::from(&input.file_path);
    let actual_size = fs::metadata(&source)
        .map_err(|error| format!("Ingest source is unavailable: {error}"))?
        .len();
    if actual_size != input.facts.size_bytes {
        return Err(format!(
            "Ingest source size changed: expected {}, found {actual_size}",
            input.facts.size_bytes
        ));
    }
    let extension = crate::blob_store::mime_to_extension(&input.facts.mime);
    application
        .blobs()
        .write_original_from_path(&input.facts.content_hash, &source, Some(extension))
        .map_err(|error| format!("Failed to persist original blob: {error}"))?;
    let stored = application
        .blobs()
        .original_path_with_ext(&input.facts.content_hash, Some(extension))
        .map_err(|error| format!("Failed to resolve original blob: {error}"))?;
    let archive_staging = application.root().join("temp/archive-import");
    if (delete_after_ingest || source.starts_with(archive_staging)) && !same_path(&source, &stored)
    {
        cleanup.push(source);
    }
    input.file_path = stored.to_string_lossy().into_owned();
    Ok(())
}

fn cleanup_sources(paths: Vec<PathBuf>) -> usize {
    let mut failures = 0;
    for path in paths {
        let parent = path.parent().map(Path::to_path_buf);
        if fs::remove_file(&path).is_err() {
            failures += 1;
        } else if let Some(parent) = parent {
            let _ = fs::remove_dir(parent);
        }
    }
    failures
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use picto_library::{
        ImmutableMediaFacts, Lifecycle, PreparedCollectionImport, PreparedIngestJob, Rating,
        RootKind,
    };

    fn image_import(path: &Path, stable_key: &str, color: [u8; 4]) -> PreparedImport {
        image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
            .save(path)
            .unwrap();
        let bytes = fs::read(path).unwrap();
        PreparedImport {
            stable_key: stable_key.into(),
            media_name: path.file_name().unwrap().to_string_lossy().into_owned(),
            file_path: path.to_string_lossy().into_owned(),
            facts: ImmutableMediaFacts {
                mime: "image/png".into(),
                size_bytes: bytes.len() as u64,
                width: Some(8),
                height: Some(8),
                duration_ms: None,
                frame_count: Some(1),
                content_hash: hex::encode(Sha256::digest(&bytes)),
                perceptual_hash: None,
                palette: Vec::new(),
            },
            lifecycle: Lifecycle::Inbox,
            rating: Rating::Unrated,
            notes: None,
            tags: vec!["source:test".into()],
            folders: Vec::new(),
            source_urls: Vec::new(),
            source_identity: None,
            imported_at_ms: 1_700_000_000_000,
            captured_at_ms: None,
        }
    }

    #[test]
    fn worker_prepares_the_visible_thumbnail_and_defers_other_derivatives() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let source = directory.path().join("source.png");
        let input = image_import(&source, "runtime-root", [10, 20, 30, 255]);
        let content_hash = input.facts.content_hash.clone();
        application
            .library()
            .enqueue_ingest_job(
                &PreparedIngestJob {
                    job_key: "manual:runtime-root".into(),
                    source_kind: "manual".into(),
                    source_path: source.to_string_lossy().into_owned(),
                    source_item_id: None,
                    delete_after_ingest: true,
                    payload: PreparedIngestPayload::Item(input),
                },
                "2026-08-28T12:00:00Z",
            )
            .unwrap();

        let report = run_batch(&application, 64).unwrap();
        assert_eq!(report.claimed, 1);
        assert_eq!(report.ingested, 1);
        assert_eq!(report.failed, 0);
        assert!(!source.exists());
        let details = application.library().details(report.root_ids[0]).unwrap();
        assert!(Path::new(&details.media[0].file_path).exists());
        assert!(application
            .blobs()
            .find_thumbnail_path(&content_hash)
            .unwrap()
            .is_some());
        let work = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::Maintenance,
                |connection| {
                    Ok(connection.query_row(
                        "SELECT COUNT(*) FROM work_item WHERE status = 'pending'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?)
                },
            )
            .unwrap();
        assert_eq!(work, 3);
    }

    #[test]
    fn a_failed_batch_returns_its_claims_and_the_replay_imports_each_item_once() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let library = application.library();
        for (index, color) in [[10, 20, 30, 255], [40, 50, 60, 255]].into_iter().enumerate() {
            let path = directory.path().join(format!("claim-{index}.png"));
            let input = image_import(&path, &format!("claim-root-{index}"), color);
            library
                .enqueue_ingest_job(
                    &PreparedIngestJob {
                        job_key: format!("manual:claim-root-{index}"),
                        source_kind: "manual".into(),
                        source_path: path.to_string_lossy().into_owned(),
                        source_item_id: None,
                        delete_after_ingest: false,
                        payload: PreparedIngestPayload::Item(input),
                    },
                    "2026-08-28T12:00:00Z",
                )
                .unwrap();
        }
        let maintenance = picto_library::database::WorkPriority::Maintenance;
        // Fail the acknowledgement after the items are already imported.
        library
            .database()
            .maintenance_write(maintenance, |transaction| {
                transaction.execute_batch(
                    "CREATE TEMP TRIGGER fail_completion BEFORE UPDATE OF status ON ingest_job
                     WHEN NEW.status = 'succeeded'
                     BEGIN SELECT RAISE(ABORT, 'injected completion failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();
        assert!(run_batch(&application, 64).is_err());

        let statuses = || {
            library
                .auxiliary_read(maintenance, |connection| {
                    let mut statement = connection.prepare(
                        "SELECT status, available_at > ?1 FROM ingest_job ORDER BY ingest_job_id",
                    )?;
                    let rows = statement
                        .query_map([chrono::Utc::now().to_rfc3339()], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(rows)
                })
                .unwrap()
        };
        // Nothing is stranded as running; both wait out the retry delay.
        assert_eq!(statuses(), vec![("pending".into(), true), ("pending".into(), true)]);
        assert_eq!(run_batch(&application, 64).unwrap().claimed, 0);

        library
            .database()
            .maintenance_write(maintenance, |transaction| {
                transaction.execute_batch("DROP TRIGGER temp.fail_completion;")?;
                Ok(())
            })
            .unwrap();
        library
            .auxiliary_write(maintenance, ["ingest".to_owned()], [], |transaction, _| {
                transaction.execute("UPDATE ingest_job SET available_at = '2026-08-28T12:00:00Z'", [])?;
                Ok(())
            })
            .unwrap();
        let replay = run_batch(&application, 64).unwrap();
        assert_eq!(replay.claimed, 2);
        assert_eq!(statuses(), vec![("succeeded".into(), false), ("succeeded".into(), false)]);
        let roots = library
            .auxiliary_read(maintenance, |connection| {
                Ok(connection.query_row("SELECT COUNT(*) FROM library_item WHERE item_kind = 1", [], |row| row.get::<_, i64>(0))?)
            })
            .unwrap();
        assert_eq!(roots, 2);
    }

    #[test]
    fn standalone_jobs_prepared_together_publish_together() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        for (index, color) in [[10, 20, 30, 255], [40, 50, 60, 255], [70, 80, 90, 255]]
            .into_iter()
            .enumerate()
        {
            let path = directory.path().join(format!("source-{index}.png"));
            let input = image_import(&path, &format!("runtime-root-{index}"), color);
            application
                .library()
                .enqueue_ingest_job(
                    &PreparedIngestJob {
                        job_key: format!("manual:runtime-root-{index}"),
                        source_kind: "manual".into(),
                        source_path: path.to_string_lossy().into_owned(),
                        source_item_id: None,
                        delete_after_ingest: false,
                        payload: PreparedIngestPayload::Item(input),
                    },
                    "2026-08-28T12:00:00Z",
                )
                .unwrap();
        }

        let report = run_batch(&application, 64).unwrap();
        assert_eq!(report.claimed, 3);
        assert_eq!(report.ingested, 3);
        let journal = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT COUNT(*), COUNT(DISTINCT revision)
                             FROM cloud_journal WHERE operation_kind = 'root.ingest'",
                            [],
                            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                        )
                        .map_err(Into::into)
                },
            )
            .unwrap();
        // Three small files are prepared well inside one flush window, so they
        // land in one publication. Larger imports still appear progressively,
        // one group at least every PENDING_FLUSH_AFTER.
        assert_eq!(journal, (1, 1));
    }

    #[test]
    fn collection_is_published_with_its_cover_thumbnail_ready() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let cover_path = directory.path().join("cover.png");
        let member_path = directory.path().join("member.png");
        let cover = image_import(&cover_path, "cover", [10, 20, 30, 255]);
        let member = image_import(&member_path, "member", [40, 50, 60, 255]);
        let cover_hash = cover.facts.content_hash.clone();
        let member_hash = member.facts.content_hash.clone();
        application
            .library()
            .enqueue_ingest_job(
                &PreparedIngestJob {
                    job_key: "manual:collection".into(),
                    source_kind: "manual".into(),
                    source_path: cover_path.to_string_lossy().into_owned(),
                    source_item_id: None,
                    delete_after_ingest: false,
                    payload: PreparedIngestPayload::Collection(PreparedCollectionImport {
                        members: vec![cover, member],
                        cover_index: 0,
                        existing_root_id: None,
                        name: Some("Collection".into()),
                        modified_at_ms: 1_700_000_000_000,
                    }),
                },
                "2026-08-28T12:00:00Z",
            )
            .unwrap();

        let report = run_batch(&application, 64).unwrap();
        assert_eq!(report.ingested, 1);
        let details = application.library().details(report.root_ids[0]).unwrap();
        assert_eq!(details.root.kind, RootKind::Collection);
        assert_eq!(details.root.media_count, 2);
        assert!(application
            .blobs()
            .find_thumbnail_path(&cover_hash)
            .unwrap()
            .is_some());
        assert!(application
            .blobs()
            .find_thumbnail_path(&member_hash)
            .unwrap()
            .is_none());
    }

    #[test]
    fn auto_tag_enabled_collection_queues_only_the_coherent_collection_root() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        application
            .patch_application_settings(&serde_json::json!({
                "aiTaggerAutoOnImport": true,
                "aiTaggerWd14Enabled": true
            }))
            .unwrap();
        let cover_path = directory.path().join("auto-cover.png");
        let member_path = directory.path().join("auto-member.png");
        let cover = image_import(&cover_path, "auto-cover", [10, 20, 30, 255]);
        let member = image_import(&member_path, "auto-member", [40, 50, 60, 255]);
        application
            .library()
            .enqueue_ingest_job(
                &PreparedIngestJob {
                    job_key: "manual:auto-collection".into(),
                    source_kind: "manual".into(),
                    source_path: cover_path.to_string_lossy().into_owned(),
                    source_item_id: None,
                    delete_after_ingest: false,
                    payload: PreparedIngestPayload::Collection(PreparedCollectionImport {
                        members: vec![cover, member],
                        cover_index: 0,
                        existing_root_id: None,
                        name: Some("Auto-tag collection".into()),
                        modified_at_ms: 1_700_000_000_000,
                    }),
                },
                "2026-08-28T12:00:00Z",
            )
            .unwrap();

        let report = run_batch(&application, 64).unwrap();
        assert_eq!(report.ingested, 1);
        let queued_root = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::Maintenance,
                |connection| {
                    connection
                        .query_row(
                            "SELECT root_id FROM work_item WHERE work_type = 'ai_tag'",
                            [],
                            |row| row.get::<_, u32>(0).map(RootId),
                        )
                        .map_err(Into::into)
                },
            )
            .unwrap();
        assert_eq!(queued_root, report.root_ids[0]);
    }
}
