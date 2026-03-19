//! Import orchestration — bridges dispatch handlers to the import pipeline.
//!
//! Handles file import requests, FTS index rebuilds, and coordinates
//! auto-merge duplicate detection during import.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::blob_store::BlobStore;
use crate::duplicates::orchestrator::DuplicateOrchestrator;
use crate::events::{self, ManualImportProgressEvent};
use crate::folders::service;
use crate::import::existing::{ExistingImportMergeRequest, merge_existing_import_target};
use crate::import::pipeline::{ImportOptions, ImportPipeline};
use crate::runtime_contract::mutation::Domain;
use crate::runtime_contract::mutation_builder::MutationImpact;
use crate::sqlite::SqliteDatabase;
use crate::tags::normalize;
use crate::types::{ImportBatchResult, ImportResult};
use tracing::warn;

pub struct ImportService;

impl ImportService {
    pub async fn import_files(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        paths: Vec<String>,
        tag_strings: Option<Vec<String>>,
        source_urls: Option<Vec<String>>,
        auto_merge_enabled: bool,
        auto_merge_distance: u32,
        auto_merge_require_matching_dimensions: bool,
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
            .filter(|p| p.is_file() && crate::media_processing::has_supported_extension(p))
            .collect();
        if file_paths.is_empty() {
            return Ok(ImportBatchResult {
                imported: Vec::new(),
                skipped: Vec::new(),
                errors: Vec::new(),
            });
        }

        let mut batch = ImportBatchResult {
            imported: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
        };

        let total = file_paths.len();
        for (index, path) in file_paths.iter().enumerate() {
            let result = pipeline.import_file(path, &options).await;
            match result {
                Ok(imported) => {
                    let surviving_hash = maybe_auto_merge(
                        db,
                        blob_store,
                        &imported.hex_hash,
                        auto_merge_enabled,
                        auto_merge_distance,
                        auto_merge_require_matching_dimensions,
                    )
                    .await;
                    if surviving_hash == imported.hex_hash {
                        emit_file_imported(db, &surviving_hash).await;
                    }
                    crate::events::emit_mutation(
                        "manual_import",
                        crate::runtime_contract::mutation_builder::MutationImpact::file_lifecycle(
                            db,
                        ),
                    );
                    batch
                        .imported
                        .push(build_import_result(db, imported, &surviving_hash).await);
                }
                Err(crate::import::pipeline::ImportError::AlreadyImported(hash)) => {
                    merge_existing_import_target(
                        db,
                        &hash,
                        ExistingImportMergeRequest {
                            restore_status: Some(options.initial_status),
                            tag_strings: options
                                .tags
                                .iter()
                                .map(|(ns, st)| normalize::combine_tag(ns, st))
                                .collect(),
                            source_urls: options.source_urls.clone(),
                            created_at: options.created_at.clone(),
                            name: options.name.clone(),
                            note_entries: options.notes.clone().unwrap_or_default(),
                            subscription_id: None,
                            mutation_name: "manual_import_existing",
                        },
                    )
                    .await?;
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

            emit_progress(
                index + 1,
                total,
                current_file,
                batch.imported.len(),
                batch.skipped.len(),
                batch.errors.len(),
            );
        }

        tracing::info!(
            imported = batch.imported.len(),
            skipped = batch.skipped.len(),
            errors = batch.errors.len(),
            "import batch complete"
        );

        Ok(batch)
    }

    pub async fn import_folder(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        path: String,
        preserve_structure: bool,
        parent_folder_id: Option<i64>,
        auto_merge_enabled: bool,
        auto_merge_distance: u32,
        auto_merge_require_matching_dimensions: bool,
        initial_status: i64,
    ) -> Result<ImportBatchResult, String> {
        let root_path = {
            let path = PathBuf::from(path);
            path.canonicalize().unwrap_or(path)
        };
        if !root_path.is_dir() {
            return Err(format!("Folder not found: {}", root_path.display()));
        }

        let (directories, file_paths) = collect_import_paths(&root_path)?;
        let pipeline = ImportPipeline::new(db, blob_store);

        let mut options = ImportOptions::default();
        options.initial_status = initial_status;

        let mut batch = ImportBatchResult {
            imported: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
        };

        let mut folder_cache = HashMap::<PathBuf, i64>::new();
        let mut created_folder_ids = Vec::<i64>::new();
        let mut touched_folder_ids = HashSet::<i64>::new();

        if preserve_structure {
            let root_name = root_path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("Imported Folder")
                .to_string();
            let root_folder =
                service::create_folder(db, root_name, parent_folder_id, None, None).await?;
            folder_cache.insert(PathBuf::new(), root_folder.folder_id);
            created_folder_ids.push(root_folder.folder_id);
            touched_folder_ids.insert(root_folder.folder_id);

            for directory in directories {
                let relative = match directory.strip_prefix(&root_path) {
                    Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
                    _ => continue,
                };
                let parent_relative = relative
                    .parent()
                    .map(|parent| parent.to_path_buf())
                    .unwrap_or_default();
                let Some(parent_id) = folder_cache.get(&parent_relative).copied() else {
                    continue;
                };
                let name = directory
                    .file_name()
                    .and_then(|entry| entry.to_str())
                    .filter(|entry| !entry.is_empty())
                    .unwrap_or("Imported Folder")
                    .to_string();
                let folder = service::create_folder(db, name, Some(parent_id), None, None).await?;
                folder_cache.insert(relative, folder.folder_id);
                created_folder_ids.push(folder.folder_id);
                touched_folder_ids.insert(folder.folder_id);
            }

            if !created_folder_ids.is_empty() {
                service::refresh_sidebar_projection_for_folder_ids(db, &created_folder_ids).await?;
                crate::events::emit_mutation(
                    "import_folder_structure",
                    MutationImpact::sidebar(Domain::Folders).folder_ids(created_folder_ids.clone()),
                );
            }
        }

        if file_paths.is_empty() {
            return Ok(batch);
        }

        let total = file_paths.len();
        for (index, file_path) in file_paths.iter().enumerate() {
            let target_folder_id = if preserve_structure {
                let relative_parent = file_path
                    .strip_prefix(&root_path)
                    .ok()
                    .and_then(|relative| relative.parent())
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                folder_cache.get(&relative_parent).copied()
            } else {
                parent_folder_id
            };

            let mut imported_hashes = Vec::<String>::new();
            let mut skipped_hashes = Vec::<String>::new();

            match pipeline.import_file(file_path, &options).await {
                Ok(imported) => {
                    let surviving_hash = maybe_auto_merge(
                        db,
                        blob_store,
                        &imported.hex_hash,
                        auto_merge_enabled,
                        auto_merge_distance,
                        auto_merge_require_matching_dimensions,
                    )
                    .await;
                    imported_hashes.push(surviving_hash.clone());
                    if surviving_hash == imported.hex_hash {
                        emit_file_imported(db, &surviving_hash).await;
                    }
                    batch
                        .imported
                        .push(build_import_result(db, imported, &surviving_hash).await);
                }
                Err(crate::import::pipeline::ImportError::AlreadyImported(hash)) => {
                    merge_existing_import_target(
                        db,
                        &hash,
                        ExistingImportMergeRequest {
                            restore_status: Some(options.initial_status),
                            tag_strings: Vec::new(),
                            source_urls: Vec::new(),
                            created_at: options.created_at.clone(),
                            name: None,
                            note_entries: HashMap::new(),
                            subscription_id: None,
                            mutation_name: "import_folder_existing",
                        },
                    )
                    .await?;
                    skipped_hashes.push(hash.clone());
                    batch.skipped.push(hash);
                }
                Err(e) => {
                    batch.errors.push(e.to_string());
                }
            }

            if let Some(folder_id) = target_folder_id {
                let membership_hashes: Vec<String> = imported_hashes
                    .iter()
                    .cloned()
                    .chain(skipped_hashes.iter().cloned())
                    .collect();
                if !membership_hashes.is_empty() {
                    db.add_entities_to_folder_batch(folder_id, &membership_hashes)
                        .await?;
                    touched_folder_ids.insert(folder_id);
                }
            }

            if !imported_hashes.is_empty() {
                let mut impact = MutationImpact::file_lifecycle(db);
                if let Some(folder_id) = target_folder_id {
                    impact = impact.folder_ids(vec![folder_id]);
                }
                crate::events::emit_mutation("import_folder", impact);
            } else if let Some(folder_id) = target_folder_id {
                if !skipped_hashes.is_empty() {
                    crate::events::emit_mutation(
                        "import_folder_membership",
                        MutationImpact::folder_file_change(folder_id),
                    );
                }
            }

            let current_file = file_path
                .strip_prefix(&root_path)
                .unwrap_or(file_path)
                .display()
                .to_string();
            emit_progress(
                index + 1,
                total,
                current_file,
                batch.imported.len(),
                batch.skipped.len(),
                batch.errors.len(),
            );
        }

        if !touched_folder_ids.is_empty() {
            let touched_folder_ids: Vec<i64> = touched_folder_ids.into_iter().collect();
            service::refresh_sidebar_projection_for_folder_ids(db, &touched_folder_ids).await?;
        }

        tracing::info!(
            imported = batch.imported.len(),
            skipped = batch.skipped.len(),
            errors = batch.errors.len(),
            folders = created_folder_ids.len(),
            "folder import batch complete"
        );

        Ok(batch)
    }
}

async fn maybe_auto_merge(
    db: &SqliteDatabase,
    blob_store: &BlobStore,
    hash: &str,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
    auto_merge_require_matching_dimensions: bool,
) -> String {
    if !auto_merge_enabled {
        return hash.to_string();
    }
    match DuplicateOrchestrator::check_and_auto_merge(
        db,
        blob_store,
        hash,
        auto_merge_distance,
        auto_merge_require_matching_dimensions,
    )
    .await
    {
        Ok(Some(result)) => result.winner_hash,
        Ok(None) => hash.to_string(),
        Err(e) => {
            warn!(
                hash = %hash,
                error = %e,
                "Duplicate auto-merge during manual import failed"
            );
            hash.to_string()
        }
    }
}

async fn build_import_result(
    db: &SqliteDatabase,
    imported: crate::import::pipeline::ImportedFile,
    surviving_hash: &str,
) -> ImportResult {
    if surviving_hash == imported.hex_hash {
        return ImportResult {
            hash: imported.hex_hash,
            mime: imported.mime,
            size: imported.size,
            has_thumbnail: imported.has_thumbnail,
            tags_applied: imported.tags_applied,
        };
    }

    match db.get_file_by_hash(surviving_hash).await {
        Ok(Some(record)) => ImportResult {
            hash: surviving_hash.to_string(),
            mime: record.mime.clone(),
            size: record.size as u64,
            has_thumbnail: record.mime.starts_with("image/") || record.mime.starts_with("video/"),
            tags_applied: imported.tags_applied,
        },
        _ => ImportResult {
            hash: surviving_hash.to_string(),
            mime: imported.mime,
            size: imported.size,
            has_thumbnail: imported.has_thumbnail,
            tags_applied: imported.tags_applied,
        },
    }
}

async fn emit_file_imported(db: &SqliteDatabase, hash: &str) {
    if let Ok(Some(record)) = db.get_file_by_hash(hash).await {
        let slim = crate::types::FileInfoSlim::from(record);
        crate::events::emit(crate::events::event_names::FILE_IMPORTED, &slim);
    }
}

fn emit_progress(
    done: usize,
    total: usize,
    current_file: String,
    imported: usize,
    skipped: usize,
    errors: usize,
) {
    events::emit(
        events::event_names::MANUAL_IMPORT_PROGRESS,
        &ManualImportProgressEvent {
            done,
            total,
            current_file,
            imported,
            skipped,
            errors,
        },
    );
}

fn collect_import_paths(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let mut directories = Vec::<PathBuf>::new();
    let mut files = Vec::<PathBuf>::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|err| format!("Failed to read {}: {err}", directory.display()))?;
        let mut child_paths = Vec::<PathBuf>::new();
        for entry in entries {
            let entry = entry
                .map_err(|err| format!("Failed to read entry in {}: {err}", directory.display()))?;
            child_paths.push(entry.path());
        }
        child_paths.sort();

        for path in child_paths {
            if path.is_dir() {
                directories.push(path.clone());
                stack.push(path);
            } else if path.is_file() && crate::media_processing::has_supported_extension(&path) {
                files.push(path.canonicalize().unwrap_or(path));
            }
        }
    }

    directories.sort();
    files.sort();
    Ok((directories, files))
}
