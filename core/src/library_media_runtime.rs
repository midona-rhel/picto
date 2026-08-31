use std::path::PathBuf;
use std::sync::Arc;

use picto_library::database::WorkPriority;
use picto_library::{
    ClaimedMediaWork, FileId, LabColor, LibraryError, MediaFactsUpdate, MediaId, MediaWorkKind,
};

use crate::library_application::LibraryApplication;
use crate::media_capabilities::ThumbnailBackend;
use crate::media_processing::{PreparedMediaSource, DEFAULT_THUMBNAIL_DIMENSIONS};

pub fn drain_blob_cleanup(application: &LibraryApplication, limit: usize) -> Result<usize, String> {
    application
        .library()
        .clean_pending_blobs(limit, |pending| {
            application
                .blobs()
                .delete(&pending.content_hash)
                .map_err(|error| {
                    LibraryError::InvalidState(format!(
                        "failed to delete blob {}: {error}",
                        pending.content_hash
                    ))
                })
        })
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalMediaWorkReport {
    pub claimed: usize,
    pub succeeded: usize,
    pub thumbnails_changed: Vec<String>,
    pub perceptual_hashes_updated: usize,
    pub retried: usize,
    pub failed: usize,
}

struct WorkTarget {
    media_id: MediaId,
    file_path: PathBuf,
    content_hash: String,
    mime: String,
    duration_ms: Option<u64>,
    frame_count: Option<u32>,
}

pub fn recover(application: &LibraryApplication) -> Result<(), String> {
    application
        .library()
        .reset_running_media_work(&chrono::Utc::now().to_rfc3339())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub async fn drain_batch(
    application: &LibraryApplication,
    limit: usize,
) -> Result<CanonicalMediaWorkReport, String> {
    let now = chrono::Utc::now();
    let work = application
        .library()
        .claim_derivative_work(limit, &now.to_rfc3339())
        .map_err(|error| error.to_string())?;
    let mut report = CanonicalMediaWorkReport {
        claimed: work.len(),
        ..Default::default()
    };
    let mut completed = Vec::new();
    for item in work {
        match execute(application, &item, now.timestamp_millis()).await {
            Ok((perceptual_hash_updated, thumbnail_changed)) => {
                completed.push(item.work_id);
                report.succeeded += 1;
                if let Some(content_hash) = thumbnail_changed {
                    report.thumbnails_changed.push(content_hash);
                }
                report.perceptual_hashes_updated += usize::from(perceptual_hash_updated);
            }
            Err(error) => {
                let (terminal, _) = application
                    .library()
                    .retry_media_work(item.work_id, item.attempt_count, &error, &now.to_rfc3339())
                    .map_err(|failure| failure.to_string())?;
                if terminal {
                    report.failed += 1;
                } else {
                    report.retried += 1;
                }
            }
        }
    }
    application
        .library()
        .complete_media_work(&completed)
        .map_err(|error| error.to_string())?;
    Ok(report)
}

pub async fn render_thumbnail_now(
    application: &LibraryApplication,
    content_hash: &str,
) -> Result<bool, String> {
    let file_id = application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            connection
                .query_row(
                    "SELECT file_id FROM media_file WHERE content_hash = ?1",
                    [content_hash],
                    |row| row.get::<_, u32>(0).map(FileId),
                )
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())?;
    let target = load_target(application, file_id)?;
    let mut source = PreparedMediaSource::from_stored_metadata(
        target.file_path.clone(),
        &target.mime,
        target
            .duration_ms
            .and_then(|value| i64::try_from(value).ok()),
        target.frame_count.map(i64::from),
    );
    ensure_thumbnail(application, &target, &mut source).await?;
    Ok(true)
}

async fn execute(
    application: &LibraryApplication,
    work: &ClaimedMediaWork,
    changed_at_ms: i64,
) -> Result<(bool, Option<String>), String> {
    let file_id = work
        .file_id
        .ok_or_else(|| format!("work {} has no physical file", work.work_id))?;
    let target = load_target(application, file_id)?;
    let mut source = PreparedMediaSource::from_stored_metadata(
        target.file_path.clone(),
        &target.mime,
        target
            .duration_ms
            .and_then(|value| i64::try_from(value).ok()),
        target.frame_count.map(i64::from),
    );
    match work.kind {
        MediaWorkKind::Thumbnail => {
            ensure_thumbnail(application, &target, &mut source).await?;
            Ok((false, Some(target.content_hash)))
        }
        MediaWorkKind::DominantColors => {
            if !source.caps.can_dominant_colors {
                return Ok((false, None));
            }
            let image = derivative_image(application, &target, &mut source).await?;
            let palette = crate::media_processing::colors::extract_dominant_colors(&image, 10)
                .into_iter()
                .map(|color| LabColor {
                    l: color.l as f32,
                    a: color.a as f32,
                    b: color.b as f32,
                    weight: 1.0,
                })
                .collect();
            application
                .library()
                .update_media_facts(
                    target.media_id,
                    &MediaFactsUpdate {
                        palette: Some(palette),
                        ..Default::default()
                    },
                    changed_at_ms,
                )
                .map_err(|error| error.to_string())?;
            Ok((false, None))
        }
        MediaWorkKind::PerceptualHash => {
            if !source.caps.can_perceptual_hash {
                return Ok((false, None));
            }
            let image = derivative_image(application, &target, &mut source).await?;
            let hash = crate::media_processing::compute_phash_base64_from_image(&image)
                .map_err(|error| format!("Perceptual hash analysis failed: {error}"))?;
            application
                .library()
                .update_media_facts(
                    target.media_id,
                    &MediaFactsUpdate {
                        perceptual_hash: Some(Some(hash)),
                        ..Default::default()
                    },
                    changed_at_ms,
                )
                .map_err(|error| error.to_string())?;
            Ok((true, None))
        }
        MediaWorkKind::AiTag | MediaWorkKind::BlobDelete => Err(format!(
            "work {} is not derivative media work",
            work.work_id
        )),
    }
}

pub fn has_ready_perceptual_hash_work(
    application: &LibraryApplication,
    now: &str,
) -> Result<bool, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::Maintenance, |connection| {
            connection
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM work_item
                         WHERE work_type = 'perceptual_hash'
                           AND (status = 'running'
                                OR (status = 'pending' AND available_at <= ?1))
                     )",
                    [now],
                    |row| row.get(0),
                )
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())
}

pub async fn settle_new_perceptual_hashes(
    application: Arc<LibraryApplication>,
    updated: usize,
) -> Result<Option<picto_library::DuplicateScanResult>, String> {
    if updated == 0
        || has_ready_perceptual_hash_work(&application, &chrono::Utc::now().to_rfc3339())?
    {
        return Ok(None);
    }
    tokio::task::spawn_blocking(move || {
        crate::duplicates::scan_library(
            &application,
            crate::duplicates::DEFAULT_GLOBAL_DISTANCE_THRESHOLD,
        )
    })
    .await
    .map_err(|error| format!("Automatic duplicate scan stopped: {error}"))?
    .map(Some)
}

fn load_target(application: &LibraryApplication, file_id: FileId) -> Result<WorkTarget, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::Maintenance, |connection| {
            connection
                .query_row(
                    "SELECT media.media_id, file.file_path, file.content_hash, file.mime,
                            file.duration_ms, file.frame_count
                     FROM media_file file
                     JOIN media_item media ON media.file_id = file.file_id
                     WHERE file.file_id = ?1
                     ORDER BY media.media_id LIMIT 1",
                    [file_id.0],
                    |row| {
                        Ok(WorkTarget {
                            media_id: MediaId(row.get::<_, u32>(0)?),
                            file_path: PathBuf::from(row.get::<_, String>(1)?),
                            content_hash: row.get(2)?,
                            mime: row.get(3)?,
                            duration_ms: row
                                .get::<_, Option<i64>>(4)?
                                .and_then(|value| u64::try_from(value).ok()),
                            frame_count: row.get(5)?,
                        })
                    },
                )
                .map_err(Into::into)
        })
        .map_err(|error| error.to_string())
}

async fn ensure_thumbnail(
    application: &LibraryApplication,
    target: &WorkTarget,
    source: &mut PreparedMediaSource,
) -> Result<Vec<u8>, String> {
    if let Some(bytes) = application
        .blobs()
        .read_thumbnail(&target.content_hash)
        .map_err(|error| format!("Thumbnail read failed: {error}"))?
    {
        return Ok(bytes);
    }
    if !source.caps.can_thumbnail() {
        return Err(format!("No thumbnail backend for {}", target.mime));
    }
    let (bytes, extension) = if source.caps.thumbnail_backend == Some(ThumbnailBackend::Inline) {
        source
            .render_inline_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS)
            .map_err(|error| format!("Thumbnail generation failed: {error}"))?
    } else {
        source
            .render_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS, 35)
            .await
            .map_err(|error| format!("Thumbnail generation failed: {error}"))?
    };
    application
        .blobs()
        .write_thumbnail(&target.content_hash, &bytes, &extension)
        .map_err(|error| format!("Thumbnail write failed: {error}"))?;
    Ok(bytes)
}

async fn derivative_image(
    application: &LibraryApplication,
    target: &WorkTarget,
    source: &mut PreparedMediaSource,
) -> Result<image::DynamicImage, String> {
    let bytes = ensure_thumbnail(application, target, source).await?;
    image::load_from_memory(&bytes)
        .map_err(|error| format!("Derivative thumbnail decode failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::{Rgb, RgbImage};
    use picto_library::{
        ImmutableMediaFacts, Lifecycle, PreparedImport, PreparedIngestJob, PreparedIngestPayload,
        Rating,
    };
    use sha2::{Digest, Sha256};

    use super::*;

    #[tokio::test]
    async fn derivative_worker_settles_thumbnail_palette_and_phash() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let source = directory.path().join("source.png");
        RgbImage::from_pixel(32, 32, Rgb([30, 120, 220]))
            .save(&source)
            .unwrap();
        let bytes = fs::read(&source).unwrap();
        let content_hash = hex::encode(Sha256::digest(&bytes));
        application
            .library()
            .enqueue_ingest_job(
                &PreparedIngestJob {
                    job_key: "manual:derivative-root".into(),
                    source_kind: "manual".into(),
                    source_path: source.to_string_lossy().into_owned(),
                    source_item_id: None,
                    delete_after_ingest: false,
                    payload: PreparedIngestPayload::Item(PreparedImport {
                        stable_key: "derivative-root".into(),
                        media_name: "source.png".into(),
                        file_path: source.to_string_lossy().into_owned(),
                        facts: ImmutableMediaFacts {
                            mime: "image/png".into(),
                            size_bytes: bytes.len() as u64,
                            width: Some(32),
                            height: Some(32),
                            duration_ms: None,
                            frame_count: Some(1),
                            content_hash: content_hash.clone(),
                            perceptual_hash: None,
                            palette: Vec::new(),
                        },
                        lifecycle: Lifecycle::Active,
                        rating: Rating::Unrated,
                        notes: None,
                        tags: Vec::new(),
                        folders: Vec::new(),
                        source_urls: Vec::new(),
                        source_identity: None,
                        imported_at_ms: 1_700_000_000_000,
                        captured_at_ms: None,
                    }),
                },
                "2026-08-28T12:00:00Z",
            )
            .unwrap();
        let ingested = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();

        let report = drain_batch(&application, 8).await.unwrap();
        assert_eq!(report.claimed, 3);
        assert_eq!(report.succeeded, 3);
        assert_eq!(report.thumbnails_changed, vec![content_hash.clone()]);
        assert_eq!(report.perceptual_hashes_updated, 1);
        assert_eq!(report.retried, 0);
        assert!(application
            .blobs()
            .find_thumbnail_path(&content_hash)
            .unwrap()
            .is_some());
        let details = application.library().details(ingested.root_ids[0]).unwrap();
        assert!(!details.media[0].facts.palette.is_empty());
        assert!(details.media[0].facts.perceptual_hash.is_some());
        assert!(application
            .library()
            .claim_derivative_work(8, &chrono::Utc::now().to_rfc3339())
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn newly_computed_phashes_settle_duplicate_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let application =
            Arc::new(LibraryApplication::create(directory.path().join("library")).unwrap());
        let first_path = directory.path().join("first.png");
        RgbImage::from_pixel(32, 32, Rgb([30, 120, 220]))
            .save(&first_path)
            .unwrap();
        let first_bytes = fs::read(&first_path).unwrap();
        let mut second_bytes = first_bytes.clone();
        second_bytes.extend_from_slice(b"picto-visual-duplicate");
        let second_path = directory.path().join("second.png");
        fs::write(&second_path, &second_bytes).unwrap();

        for (index, (path, bytes)) in [(first_path, first_bytes), (second_path, second_bytes)]
            .into_iter()
            .enumerate()
        {
            let content_hash = hex::encode(Sha256::digest(&bytes));
            application
                .library()
                .enqueue_ingest_job(
                    &PreparedIngestJob {
                        job_key: format!("manual:duplicate-{index}"),
                        source_kind: "manual".into(),
                        source_path: path.to_string_lossy().into_owned(),
                        source_item_id: None,
                        delete_after_ingest: false,
                        payload: PreparedIngestPayload::Item(PreparedImport {
                            stable_key: format!("duplicate-{index}"),
                            media_name: format!("duplicate-{index}.png"),
                            file_path: path.to_string_lossy().into_owned(),
                            facts: ImmutableMediaFacts {
                                mime: "image/png".into(),
                                size_bytes: bytes.len() as u64,
                                width: Some(32),
                                height: Some(32),
                                duration_ms: None,
                                frame_count: Some(1),
                                content_hash,
                                perceptual_hash: None,
                                palette: Vec::new(),
                            },
                            lifecycle: Lifecycle::Active,
                            rating: Rating::Unrated,
                            notes: None,
                            tags: Vec::new(),
                            folders: Vec::new(),
                            source_urls: Vec::new(),
                            source_identity: None,
                            imported_at_ms: 1_700_000_000_000 + index as i64,
                            captured_at_ms: None,
                        }),
                    },
                    "2026-08-28T12:00:00Z",
                )
                .unwrap();
        }
        crate::library_ingest_runtime::run_batch(&application, 64).unwrap();

        let report = drain_batch(&application, 8).await.unwrap();
        assert_eq!(report.perceptual_hashes_updated, 2);
        let result = settle_new_perceptual_hashes(
            Arc::clone(&application),
            report.perceptual_hashes_updated,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.candidate_count, 1);
        assert_eq!(
            application.library().sidebar_counts().unwrap().duplicates,
            1
        );
    }
}
