//! Import orchestration — bridges dispatch handlers to the import pipeline.
//!
//! Handles file import requests, FTS index rebuilds, and coordinates
//! auto-merge duplicate detection during import.

use std::path::PathBuf;

use crate::blob_store::BlobStore;
use crate::duplicates::controller::DuplicateController;
use crate::events::{self, ManualImportProgressEvent};
use crate::import::pipeline::{ImportOptions, ImportPipeline};
use crate::sqlite::SqliteDatabase;
use crate::tags::normalize;
use crate::types::{ImportBatchResult, ImportResult};
use tracing::warn;

pub struct ImportController;

impl ImportController {
    pub async fn import_files(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        paths: Vec<String>,
        tag_strings: Option<Vec<String>>,
        source_urls: Option<Vec<String>>,
        auto_merge_enabled: bool,
        auto_merge_distance: u32,
        initial_status: i64,
    ) -> Result<ImportBatchResult, String> {
        let pipeline = ImportPipeline::new(db, blob_store);

        let mut options = ImportOptions::default();
        options.initial_status = initial_status;
        if let Some(tag_strs) = tag_strings {
            options.tags = normalize::parse_tags_ingest(&tag_strs);
        }
        if let Some(urls) = source_urls {
            options.source_urls = urls;
        }

        let file_paths: Vec<PathBuf> = paths
            .into_iter()
            .map(|p| {
                let path = PathBuf::from(&p);
                path.canonicalize().unwrap_or(path)
            })
            .filter(|p| p.is_file())
            .collect();
        if file_paths.is_empty() {
            return Ok(ImportBatchResult {
                imported: Vec::new(),
                skipped: Vec::new(),
                errors: Vec::new(),
            });
        }

        let results = pipeline.import_files(&file_paths, &options).await;

        let mut batch = ImportBatchResult {
            imported: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
        };

        let total = file_paths.len();
        for (index, (path, result)) in file_paths.iter().zip(results.into_iter()).enumerate() {
            match result {
                Ok(imported) => {
                    if auto_merge_enabled {
                        if let Err(e) = DuplicateController::check_and_auto_merge(
                            db,
                            &imported.hex_hash,
                            auto_merge_distance,
                        )
                        .await
                        {
                            warn!(
                                hash = %imported.hex_hash,
                                error = %e,
                                "Duplicate auto-merge during manual import failed"
                            );
                        }
                    }
                    if let Ok(Some(record)) = db.get_file_by_hash(&imported.hex_hash).await {
                        let slim = crate::types::FileInfoSlim::from(record);
                        crate::events::emit(crate::events::event_names::FILE_IMPORTED, &slim);
                    }
                    db.scope_cache_invalidate_all();
                    crate::events::emit_mutation(
                        "manual_import",
                        crate::events::MutationImpact::file_lifecycle(db),
                    );
                    batch.imported.push(ImportResult {
                        hash: imported.hex_hash,
                        mime: imported.mime,
                        size: imported.size,
                        has_thumbnail: imported.has_thumbnail,
                        tags_applied: imported.tags_applied,
                    });
                }
                Err(crate::import::pipeline::ImportError::AlreadyImported(hash)) => {
                    batch.skipped.push(hash);
                }
                Err(e) => {
                    batch.errors.push(e.to_string());
                }
            }

            let current_file = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| path.display().to_string());

            events::emit(
                events::event_names::MANUAL_IMPORT_PROGRESS,
                &ManualImportProgressEvent {
                    done: index + 1,
                    total,
                    current_file,
                    imported: batch.imported.len(),
                    skipped: batch.skipped.len(),
                    errors: batch.errors.len(),
                },
            );
        }

        Ok(batch)
    }
}
