use std::fs;
use std::path::{Path, PathBuf};

use picto_library::{
    ClaimedIngestJob, LibraryError, PreparedImport, PreparedIngestPayload, RootId,
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
    let auto_tag = application
        .application_settings()?
        .value
        .get("aiTaggerAutoOnImport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut items = Vec::new();
    let mut collections = Vec::new();
    for mut job in claimed {
        match prepare_job(application, &mut job) {
            Ok(cleanup) => match job.payload {
                PreparedIngestPayload::Item(input) => {
                    items.push((job.ingest_job_id, input, cleanup))
                }
                PreparedIngestPayload::Collection(input) => {
                    collections.push((job.ingest_job_id, input, cleanup));
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

    settle_items(application, items, &now, auto_tag, &mut report)?;
    for (job_id, input, cleanup) in collections {
        let ingested = if auto_tag {
            application
                .library()
                .ingest_collection_with_auto_tags(&input)
        } else {
            application.library().ingest_collection(&input)
        };
        match ingested {
            Ok((root_id, _)) => {
                reconcile_ingested_sources(application, &input.members, root_id, &now)?;
                application
                    .library()
                    .complete_ingest_jobs(&[job_id], &now)
                    .map_err(|error| error.to_string())?;
                report.ingested += 1;
                report.root_ids.push(root_id);
                report.cleanup_failures += cleanup_sources(cleanup);
            }
            Err(error) => {
                if matches!(error, LibraryError::ImportDeleted) {
                    settle_deleted_import(application, job_id, &input.members, &now)?;
                    report.skipped += 1;
                    report.cleanup_failures += cleanup_sources(cleanup);
                } else {
                    application
                        .library()
                        .fail_ingest_job(job_id, &error.to_string(), &now)
                        .map_err(|failure| failure.to_string())?;
                    mark_failed_sources(application, &input.members, &error.to_string(), &now)?;
                    report.failed += 1;
                }
            }
        }
    }
    crate::library_subscription_state::settle_ingest_runs(application, &now)?;
    Ok(report)
}

fn settle_items(
    application: &LibraryApplication,
    jobs: Vec<(i64, PreparedImport, Vec<PathBuf>)>,
    now: &str,
    auto_tag: bool,
    report: &mut CanonicalIngestRunReport,
) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    let inputs = jobs
        .iter()
        .map(|(_, input, _)| input.clone())
        .collect::<Vec<_>>();
    let ingested = if auto_tag {
        application.library().ingest_batch_with_auto_tags(&inputs)
    } else {
        application.library().ingest_batch(&inputs)
    };
    match ingested {
        Ok(outputs) => {
            for (input, (root_id, _)) in inputs.iter().zip(&outputs) {
                reconcile_ingested_sources(
                    application,
                    std::slice::from_ref(input),
                    *root_id,
                    now,
                )?;
            }
            let job_ids = jobs
                .iter()
                .map(|(job_id, _, _)| *job_id)
                .collect::<Vec<_>>();
            application
                .library()
                .complete_ingest_jobs(&job_ids, now)
                .map_err(|error| error.to_string())?;
            report.ingested += outputs.len();
            report
                .root_ids
                .extend(outputs.into_iter().map(|(root_id, _)| root_id));
            report.cleanup_failures += jobs
                .into_iter()
                .map(|(_, _, cleanup)| cleanup_sources(cleanup))
                .sum::<usize>();
            Ok(())
        }
        Err(_) if jobs.len() > 1 => {
            for job in jobs {
                settle_items(application, vec![job], now, auto_tag, report)?;
            }
            Ok(())
        }
        Err(error) => {
            let job_id = jobs[0].0;
            if matches!(error, LibraryError::ImportDeleted) {
                settle_deleted_import(application, job_id, std::slice::from_ref(&jobs[0].1), now)?;
                report.skipped += 1;
                report.cleanup_failures += cleanup_sources(jobs[0].2.clone());
            } else {
                application
                    .library()
                    .fail_ingest_job(job_id, &error.to_string(), now)
                    .map_err(|failure| failure.to_string())?;
                mark_failed_sources(
                    application,
                    std::slice::from_ref(&jobs[0].1),
                    &error.to_string(),
                    now,
                )?;
                report.failed += 1;
            }
            Ok(())
        }
    }
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
    inputs: &[PreparedImport],
    root_id: RootId,
    now: &str,
) -> Result<(), String> {
    let sources = inputs
        .iter()
        .filter_map(|input| input.source_identity.as_ref())
        .filter_map(|source| {
            let (site_id, post_key) = source.source_key.split_once(':')?;
            Some((
                site_id.to_owned(),
                post_key.to_owned(),
                source.source_key.clone(),
                source.source_item_key.clone(),
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
            [root_id],
            |transaction, _| {
                let mut changed = 0;
                for (site_id, post_key, source_key, item_key) in &sources {
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
    if delete_after_ingest && !same_path(&source, &stored) {
        cleanup.push(source);
    }
    input.file_path = stored.to_string_lossy().into_owned();
    Ok(())
}

fn cleanup_sources(paths: Vec<PathBuf>) -> usize {
    paths
        .into_iter()
        .filter(|path| fs::remove_file(path).is_err())
        .count()
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
