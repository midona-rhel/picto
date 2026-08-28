use std::fs;
use std::path::{Path, PathBuf};

use picto_library::{ClaimedIngestJob, PreparedImport, PreparedIngestPayload, RootId};

use crate::library_application::LibraryApplication;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalIngestRunReport {
    pub claimed: usize,
    pub ingested: usize,
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
                report.failed += 1;
            }
        }
    }

    settle_items(application, items, &now, &mut report)?;
    for (job_id, input, cleanup) in collections {
        match application.library().ingest_collection(&input) {
            Ok((root_id, _)) => {
                application
                    .library()
                    .complete_ingest_jobs(&[job_id], &now)
                    .map_err(|error| error.to_string())?;
                report.ingested += 1;
                report.root_ids.push(root_id);
                report.cleanup_failures += cleanup_sources(cleanup);
            }
            Err(error) => {
                application
                    .library()
                    .fail_ingest_job(job_id, &error.to_string(), &now)
                    .map_err(|failure| failure.to_string())?;
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

fn settle_items(
    application: &LibraryApplication,
    jobs: Vec<(i64, PreparedImport, Vec<PathBuf>)>,
    now: &str,
    report: &mut CanonicalIngestRunReport,
) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    let inputs = jobs
        .iter()
        .map(|(_, input, _)| input.clone())
        .collect::<Vec<_>>();
    match application.library().ingest_batch(&inputs) {
        Ok(outputs) => {
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
                settle_items(application, vec![job], now, report)?;
            }
            Ok(())
        }
        Err(error) => {
            let job_id = jobs[0].0;
            application
                .library()
                .fail_ingest_job(job_id, &error.to_string(), now)
                .map_err(|failure| failure.to_string())?;
            report.failed += 1;
            Ok(())
        }
    }
}

fn prepare_job(
    application: &LibraryApplication,
    job: &mut ClaimedIngestJob,
) -> Result<Vec<PathBuf>, String> {
    let mut cleanup = Vec::new();
    match &mut job.payload {
        PreparedIngestPayload::Item(input) => {
            prepare_import(application, input, job.delete_after_ingest, &mut cleanup)?;
        }
        PreparedIngestPayload::Collection(input) => {
            for member in &mut input.members {
                prepare_import(application, member, job.delete_after_ingest, &mut cleanup)?;
            }
        }
    }
    Ok(cleanup)
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
    use picto_library::{ImmutableMediaFacts, Lifecycle, PreparedIngestJob, Rating};

    #[test]
    fn worker_consumes_the_canonical_dto_and_defers_derivatives() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let source = directory.path().join("source.png");
        let bytes = b"canonical-ingest-source";
        fs::write(&source, bytes).unwrap();
        let content_hash = hex::encode(Sha256::digest(bytes));
        let input = PreparedImport {
            stable_key: "runtime-root".into(),
            media_name: "source.png".into(),
            file_path: source.to_string_lossy().into_owned(),
            facts: ImmutableMediaFacts {
                mime: "image/png".into(),
                size_bytes: bytes.len() as u64,
                width: Some(8),
                height: Some(8),
                duration_ms: None,
                frame_count: Some(1),
                content_hash,
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
        };
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
}
